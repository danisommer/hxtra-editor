//! hxtra-editor: a window over an HX Stomp running HXtra or stock firmware.

#![windows_subsystem = "windows"]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui;
use hxs_core::backup;
use hxs_core::catalog::{self, Catalog, Model, NameStyle, Param, ParamKind, ROLE_CAPTURE_SLOT};
use hxs_core::device::{DeviceHandle, Event, Options, Pending, Snapshot, snapshot_number};
use hxs_core::globals::Globals;
use hxs_core::nam;
use hxs_core::notify::Notification;
use hxs_core::preset::{Block, PresetFile};
use hxs_core::restore::{self, Plan};
use hxs_core::snapshot;
use hxs_core::stomp::{self, CurrentPreset, IndexedEntry, ListFlags, PresetEntry};
use rmpv::Value;

const TIMEOUT: Duration = Duration::from_secs(5);
const LOG_LINES: usize = 500;
const IR_SLOTS: u64 = 128;
const TOOL: &str = concat!("hxtra-editor ", env!("CARGO_PKG_VERSION"));

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 840.0]).with_title("hxtra-editor"),
        ..Default::default()
    };
    eframe::run_native("hxtra-editor", options, Box::new(|_cc| Ok(Box::new(App::new()))))
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Preset,
    Library,
    Catalog,
    Settings,
    Log,
}

enum JobResult {
    Text(String),
    Plan(PathBuf, Plan),
    Irs(Vec<IrSlot>),
    EditBuffer(Vec<u8>),
}

#[derive(Clone)]
struct IrSlot {
    index: u64,
    name: String,
    capture: bool,
}

/// The IR list, each slot read to tell HXtra's NAM captures from impulse responses.
fn read_irs(s: &mut stomp::Stomp) -> hxs_core::Result<JobResult> {
    let list: Vec<IndexedEntry> = s.ir_list()?;
    let slots = list
        .into_iter()
        .map(|ir| Ok(IrSlot { capture: nam::is_capture(&s.ir_data(ir.index)?), index: ir.index, name: ir.name }))
        .collect::<hxs_core::Result<Vec<_>>>()?;
    Ok(JobResult::Irs(slots))
}

enum Edit {
    Enable { block: u64, on: bool },
    Param { block: u64, index: u64, value: Value },
}

struct Device {
    current: CurrentPreset,
    presets: Vec<PresetEntry>,
    blocks: Vec<Block>,
    snapshot_names: Vec<String>,
    globals: Option<Globals>,
    firmware: Option<u64>,
    snapshot: Option<u64>,
    edited: bool,
}

struct App {
    handle: DeviceHandle,
    events: Receiver<Event>,
    connecting: Option<Pending<Snapshot>>,
    device: Option<Device>,
    catalog: Option<Catalog>,
    catalog_note: String,
    catalog_path: String,
    catalog_ignored: bool,
    style: NameStyle,
    irs: Vec<IrSlot>,
    nam_file: String,
    nam_slot: u64,
    nam_name: String,
    confirm_nam: bool,
    ops: Vec<(String, Pending<()>)>,
    job: Option<(String, Pending<JobResult>)>,
    tab: Tab,
    show_hidden: bool,
    log: VecDeque<String>,
    save_name: String,
    backup_dir: String,
    restore_dir: String,
    plan: Option<(PathBuf, Plan)>,
    confirm_restore: bool,
    upload_file: String,
    upload_slot: usize,
    upload_name: String,
    confirm_upload: bool,
    allow_ir_delete: bool,
    search: String,
    setting_id: String,
    setting_value: String,
}

impl App {
    fn new() -> Self {
        let (handle, events) = DeviceHandle::usb(TIMEOUT, Options { midi_channel: 0 });
        let mut app = Self {
            handle,
            events,
            connecting: None,
            device: None,
            catalog: None,
            catalog_note: String::new(),
            catalog_path: String::new(),
            catalog_ignored: false,
            style: NameStyle::Unit,
            irs: Vec::new(),
            nam_file: String::new(),
            nam_slot: 1,
            nam_name: String::new(),
            confirm_nam: false,
            ops: Vec::new(),
            job: None,
            tab: Tab::Preset,
            show_hidden: false,
            log: VecDeque::new(),
            save_name: String::new(),
            backup_dir: default_backup_dir(),
            restore_dir: String::new(),
            plan: None,
            confirm_restore: false,
            upload_file: String::new(),
            upload_slot: 0,
            upload_name: String::new(),
            confirm_upload: false,
            allow_ir_delete: false,
            search: String::new(),
            setting_id: String::new(),
            setting_value: String::new(),
        };
        app.load_catalog(None);
        app
    }

    fn note(&mut self, line: impl Into<String>) {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
        self.log.push_back(format!("{h:02}:{m:02}:{s:02} UTC  {}", line.into()));
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }

    fn load_catalog(&mut self, path: Option<&Path>) {
        match Catalog::find(path) {
            Ok(Some((p, c))) => {
                self.catalog_note = format!("{} ({} models)", p.display(), c.models.len());
                self.catalog_path = p.display().to_string();
                self.catalog = Some(c);
                self.catalog_ignored = false;
                self.check_catalog();
            }
            Ok(None) => {
                self.catalog = None;
                self.catalog_note = "no catalog: names show as numbers. Extract one from your firmware file and load it below.".into();
            }
            Err(e) => {
                self.catalog = None;
                self.catalog_note = format!("catalog not loaded: {e}");
            }
        }
    }

    fn check_catalog(&mut self) {
        let firmware = self.device.as_ref().and_then(|d| d.firmware);
        let problem = match (&self.catalog, firmware) {
            (Some(c), Some(f)) => c.check_firmware(f).err(),
            _ => None,
        };
        if let Some(e) = problem {
            self.catalog_note = format!("not used: {e}");
            self.catalog_ignored = true;
        }
    }

    fn start_job(&mut self, label: &str, f: impl FnOnce(&mut stomp::Stomp) -> hxs_core::Result<JobResult> + Send + 'static) {
        self.note(format!("{label}..."));
        self.job = Some((label.to_owned(), self.handle.run(f)));
    }

    fn op(&mut self, label: impl Into<String>, p: Pending<()>) {
        self.ops.push((label.into(), p));
    }

    fn set_device(&mut self, s: Snapshot) {
        let parsed = PresetFile::parse(s.edit_buffer.clone());
        let firmware = parsed.as_ref().ok().and_then(|p| p.device_meta()).map(|m| m.firmware);
        let (blocks, snapshot_names) = parsed.as_ref().map(preset_view).unwrap_or_default();
        self.save_name = s.current.name.clone();
        self.note(format!("connected: setlist {} slot {:03} \"{}\", {} presets", s.current.setlist, s.current.slot, s.current.name, s.presets.len()));
        self.device = Some(Device {
            current: s.current,
            presets: s.presets,
            blocks,
            snapshot_names,
            globals: Globals::parse(s.globals).ok(),
            firmware,
            snapshot: None,
            edited: false,
        });
        self.catalog_ignored = false;
        self.check_catalog();
        self.start_job("reading the IR list", read_irs);
    }

    fn load_edit_buffer(&mut self, bytes: Vec<u8>) {
        let view = PresetFile::parse(bytes).map(|p| preset_view(&p));
        match (view, self.device.as_mut()) {
            (Ok((blocks, names)), Some(d)) => {
                d.blocks = blocks;
                d.snapshot_names = names;
            }
            (Err(e), _) => self.note(format!("could not read the edit buffer: {e}")),
            (_, None) => {}
        }
    }

    fn poll(&mut self) {
        if let Some(r) = self.connecting.as_ref().and_then(Pending::poll) {
            self.connecting = None;
            match r {
                Ok(s) => self.set_device(s),
                Err(e) => self.note(format!("connect failed: {e}")),
            }
        }
        while let Ok(e) = self.events.try_recv() {
            self.event(e);
        }
        let mut failed = Vec::new();
        self.ops.retain(|(label, p)| match p.poll() {
            None => true,
            Some(Ok(())) => false,
            Some(Err(e)) => {
                failed.push(format!("failed: {label}: {e}"));
                false
            }
        });
        for f in failed {
            self.note(f);
        }
        if let Some(r) = self.job.as_ref().and_then(|(_, p)| p.poll()) {
            let (label, _) = self.job.take().expect("a polled job");
            match r {
                Ok(JobResult::Text(t)) => self.note(format!("{label}: {t}")),
                Ok(JobResult::Plan(dir, plan)) => {
                    self.note(format!("{label}: {} differences", plan.actions.len()));
                    self.plan = Some((dir, plan));
                    self.confirm_restore = false;
                }
                Ok(JobResult::Irs(irs)) => {
                    let captures = irs.iter().filter(|i| i.capture).count();
                    self.note(format!("{label}: {} IRs and {captures} NAM captures", irs.len() - captures));
                    self.irs = irs;
                }
                Ok(JobResult::EditBuffer(b)) => self.load_edit_buffer(b),
                Err(e) => self.note(format!("{label} failed: {e}")),
            }
        }
    }

    fn event(&mut self, e: Event) {
        match e {
            Event::Connected(s) => self.set_device(s),
            Event::Disconnected { reason } => {
                self.device = None;
                self.note(format!("disconnected: {reason}"));
            }
            Event::PresetLoaded { current, edit_buffer } => {
                self.note(format!("preset loaded: {:03} \"{}\"", current.slot, current.name));
                self.save_name = current.name.clone();
                if let Some(d) = self.device.as_mut() {
                    d.current = current;
                    d.edited = false;
                    d.snapshot = Some(1);
                }
                self.load_edit_buffer(edit_buffer);
            }
            Event::SettingChanged { .. } => {}
            Event::PresetRenamed { setlist, slot, name } => {
                if let Some(p) = self.device.as_mut().and_then(|d| d.presets.iter_mut().find(|p| p.slot == slot as usize)) {
                    p.name = name.clone();
                }
                self.note(format!("preset renamed: setlist {setlist} slot {slot:03} \"{name}\""));
            }
            Event::SnapshotSelected { index, bank } => {
                let number = snapshot_number(index, bank);
                if let Some(d) = self.device.as_mut() {
                    d.snapshot = number;
                }
            }
            Event::Notified(Notification::ParameterChanged { block, path: 0, index, value }) => {
                if let Some(d) = self.device.as_mut() {
                    d.edited = true;
                    if let Some(v) = d.blocks.iter_mut().find(|b| b.position as u64 == block).and_then(|b| b.params.get_mut(index as usize)) {
                        *v = value;
                    }
                }
            }
            Event::Notified(Notification::BlockChanged { block }) => {
                self.note(format!("block {block} changed on the unit (Refresh reads it again)"));
                if let Some(d) = self.device.as_mut() {
                    d.edited = true;
                }
            }
            Event::Notified(Notification::PresetSaved { setlist, slot, .. }) => self.note(format!("preset saved: setlist {setlist} slot {slot:03}")),
            Event::Notified(Notification::Unknown { .. } | Notification::SettingChanged { .. } | Notification::Completion { .. }) => {}
            Event::Notified(n) => self.note(format!("{n:?}")),
            Event::Problem { message } => self.note(format!("problem: {message}")),
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            match (&self.device, &self.connecting) {
                (Some(_), _) => {
                    if ui.button("Disconnect").clicked() {
                        let p = self.handle.disconnect();
                        self.op("disconnect", p);
                        self.device = None;
                    }
                }
                (None, Some(_)) => {
                    ui.spinner();
                    ui.label("connecting...");
                }
                (None, None) => {
                    if ui.button("Connect").clicked() {
                        self.connecting = Some(self.handle.connect());
                    }
                    ui.label("Close HX Edit first.");
                }
            }
            ui.separator();
            if let Some(d) = &self.device {
                ui.label(format!(
                    "setlist {} slot {:03} \"{}\"{}  snapshot {}",
                    d.current.setlist,
                    d.current.slot,
                    d.current.name,
                    if d.edited { " (edited)" } else { "" },
                    d.snapshot.map_or("?".into(), |n| n.to_string())
                ));
            }
            ui.separator();
            if let Some((label, _)) = &self.job {
                ui.spinner();
                ui.label(label);
            }
        });
        ui.horizontal(|ui| {
            for (tab, name) in [(Tab::Preset, "Preset"), (Tab::Library, "Library"), (Tab::Catalog, "Catalog"), (Tab::Settings, "Settings"), (Tab::Log, "Log")] {
                ui.selectable_value(&mut self.tab, tab, name);
            }
            ui.separator();
            ui.label(format!("catalog: {}", self.catalog_note));
        });
    }

    fn preset_list(&mut self, ui: &mut egui::Ui) {
        let Some(d) = &self.device else {
            ui.label("not connected");
            return;
        };
        let (setlist, current) = (d.current.setlist, d.current.slot as usize);
        let mut chosen = None;
        egui::ScrollArea::vertical().id_salt("presets").show(ui, |ui| {
            for p in &d.presets {
                if ui.selectable_label(p.slot == current, format!("{:03}  {}", p.slot, p.name)).clicked() {
                    chosen = Some(p.slot);
                }
            }
        });
        if let Some(slot) = chosen {
            let p = self.handle.select_preset(setlist, slot);
            self.op(format!("load preset {slot:03}"), p);
        }
    }

    fn preset_tab(&mut self, ui: &mut egui::Ui) {
        let Some(d) = &self.device else {
            ui.heading("Not connected");
            ui.label("Close HX Edit, connect the HX Stomp over USB and press Connect.");
            return;
        };
        let (setlist, slot) = (d.current.setlist, d.current.slot as usize);
        let snapshot_names = d.snapshot_names.clone();
        let current_snapshot = d.snapshot;
        ui.horizontal(|ui| {
            ui.label("Snapshot:");
            for n in 1..=6u8 {
                let name = snapshot_names.get(usize::from(n) - 1).cloned().unwrap_or_default();
                let text = match name.is_empty() {
                    true => n.to_string(),
                    false => format!("{n} {name}"),
                };
                if ui.selectable_label(current_snapshot == Some(u64::from(n)), text).clicked() {
                    let p = self.handle.recall_snapshot(n);
                    self.op(format!("snapshot {n} (MIDI CC 69)"), p);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.add(egui::TextEdit::singleline(&mut self.save_name).desired_width(160.0).char_limit(stomp::MAX_PRESET_NAME));
            if ui.button("Save to this slot").on_hover_text("Stores the edited preset in its slot (writes to the unit)").clicked() {
                match stomp::validate_name(&self.save_name) {
                    Ok(()) => {
                        let p = self.handle.save_preset(setlist, slot, &self.save_name);
                        self.op(format!("save preset {slot:03}"), p);
                        if let Some(d) = self.device.as_mut() {
                            d.edited = false;
                        }
                    }
                    Err(e) => self.note(format!("not saved: {e}")),
                }
            }
            if ui.button("Refresh").on_hover_text("Reads the edit buffer again (briefly interrupts the sound in snapshots 4-6)").clicked() {
                self.start_job("reading the edit buffer", |s| Ok(JobResult::EditBuffer(s.edit_buffer()?)));
            }
            ui.checkbox(&mut self.show_hidden, "hidden parameters");
        });
        ui.separator();
        let mut edits = Vec::new();
        let catalog = match self.catalog_ignored {
            true => None,
            false => self.catalog.as_ref(),
        };
        let (style, hidden, irs) = (self.style, self.show_hidden, &self.irs);
        let Some(d) = self.device.as_mut() else { return };
        egui::ScrollArea::vertical().id_salt("blocks").show(ui, |ui| {
            if d.blocks.is_empty() {
                ui.label("no blocks in this preset");
            }
            for b in d.blocks.iter_mut() {
                block_ui(ui, b, catalog.and_then(|c| c.model(b.preset_id)), style, hidden, irs, &mut edits);
            }
        });
        if !edits.is_empty() {
            d.edited = true;
        }
        for e in edits {
            match e {
                Edit::Enable { block, on } => {
                    let p = self.handle.set_block_enabled(block, on);
                    self.op(format!("block {block} {}", if on { "on" } else { "off" }), p);
                }
                Edit::Param { block, index, value } => {
                    let p = self.handle.set_parameter(block, 0, index, value);
                    self.op(format!("block {block} parameter {index}"), p);
                }
            }
        }
    }

    fn library_tab(&mut self, ui: &mut egui::Ui) {
        let connected = self.device.is_some() && self.job.is_none();
        let setlist = self.device.as_ref().map_or(0, |d| d.current.setlist);
        egui::ScrollArea::vertical().id_salt("library").show(ui, |ui| {
            ui.heading("Backup");
            ui.label("Every preset as stored (with snapshots 4-6), IRs and captures, favorites and global settings, with SHA-256 hashes. Reads only.");
            ui.horizontal(|ui| {
                ui.label("Into a new folder:");
                ui.add(egui::TextEdit::singleline(&mut self.backup_dir).desired_width(520.0));
                if ui.add_enabled(connected, egui::Button::new("Back up")).clicked() {
                    let dir = PathBuf::from(&self.backup_dir);
                    self.start_job("backup", move |s| {
                        let m = backup::run(s, &dir, setlist, TOOL, |_| {})?;
                        Ok(JobResult::Text(format!("{} presets, {} IRs, {} favorites -> {}", m.presets.len(), m.irs.len(), m.favorites.len(), dir.display())))
                    });
                    self.backup_dir = default_backup_dir();
                }
            });
            ui.separator();
            ui.heading("Restore a backup");
            ui.label("Compares the backup with the unit first and lists only what differs. Nothing is written until you confirm.");
            ui.horizontal(|ui| {
                ui.label("Backup folder:");
                ui.add(egui::TextEdit::singleline(&mut self.restore_dir).desired_width(520.0));
                if ui.add_enabled(connected, egui::Button::new("Compare with the unit")).clicked() {
                    let dir = PathBuf::from(&self.restore_dir);
                    self.plan = None;
                    self.start_job("comparing the backup", move |s| {
                        let b = restore::Backup::open(&dir)?;
                        let plan = restore::plan(s, &b)?;
                        Ok(JobResult::Plan(dir, plan))
                    });
                }
            });
            let mut apply = None;
            if let Some((dir, plan)) = &self.plan {
                for n in &plan.notes {
                    ui.label(format!("note: {n}"));
                }
                match plan.is_empty() {
                    true => {
                        ui.label("The unit already matches this backup.");
                    }
                    false => {
                        for a in &plan.actions {
                            ui.label(format!("  {}", a.describe()));
                        }
                        ui.checkbox(&mut self.confirm_restore, format!("Write these {} changes to the unit", plan.actions.len()));
                        if ui.add_enabled(connected && self.confirm_restore, egui::Button::new("Restore")).clicked() {
                            apply = Some(dir.clone());
                        }
                    }
                }
            }
            if let (Some(dir), Some((_, plan))) = (apply, self.plan.take()) {
                self.start_job("restoring", move |s| {
                    let b = restore::Backup::open(&dir)?;
                    restore::apply(s, &plan, b.manifest.setlist, |_| {})?;
                    let left = restore::plan(s, &b)?;
                    Ok(JobResult::Text(match left.is_empty() {
                        true => "done, the unit now matches the backup".into(),
                        false => format!("done, but {} differences remain: compare again", left.actions.len()),
                    }))
                });
            }
            ui.separator();
            ui.heading("Write one preset file into a slot");
            ui.horizontal(|ui| {
                ui.label("File (.l6p from a backup):");
                ui.add(egui::TextEdit::singleline(&mut self.upload_file).desired_width(420.0));
            });
            ui.horizontal(|ui| {
                ui.label("Slot:");
                ui.add(egui::DragValue::new(&mut self.upload_slot).range(0..=125));
                ui.label("Name:");
                ui.add(egui::TextEdit::singleline(&mut self.upload_name).desired_width(160.0).char_limit(stomp::MAX_PRESET_NAME));
                ui.checkbox(&mut self.confirm_upload, "overwrite that slot");
                if ui.add_enabled(connected && self.confirm_upload, egui::Button::new("Write")).clicked() {
                    let (file, slot, name) = (PathBuf::from(&self.upload_file), self.upload_slot, self.upload_name.clone());
                    self.confirm_upload = false;
                    self.start_job("writing the preset", move |s| {
                        let bytes = std::fs::read(&file)?;
                        let preset = PresetFile::parse(bytes)?;
                        stomp::validate_name(&name)?;
                        s.batch_begin()?;
                        let written = s.upload_preset(setlist, slot, &name, ListFlags::default(), preset.bytes());
                        let ended = s.batch_end();
                        written?;
                        ended?;
                        let back = s.preset_file(setlist, slot)?;
                        Ok(JobResult::Text(match back == preset.bytes() {
                            true => format!("slot {slot:03} written and read back identical"),
                            false => format!("slot {slot:03} written, but the read back differs"),
                        }))
                    });
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.heading("IRs and NAM captures");
                if ui.add_enabled(connected, egui::Button::new("Refresh")).clicked() {
                    self.start_job("reading the IR list", read_irs);
                }
                ui.checkbox(&mut self.allow_ir_delete, "allow emptying slots");
            });
            ui.label("Upload a NAM A2 Lite capture (.nam): it is converted as HXtra does and stored in an empty IR slot; the .nam itself is not kept.");
            ui.horizontal(|ui| {
                ui.label(".nam file:");
                ui.add(egui::TextEdit::singleline(&mut self.nam_file).desired_width(420.0));
            });
            ui.horizontal(|ui| {
                ui.label("Into IR slot:");
                ui.add(egui::DragValue::new(&mut self.nam_slot).range(1..=IR_SLOTS));
                ui.label("Name (empty: the capture's own):");
                ui.add(egui::TextEdit::singleline(&mut self.nam_name).desired_width(220.0).char_limit(stomp::MAX_IR_NAME));
                let taken = self.irs.iter().find(|i| i.index + 1 == self.nam_slot).map(|i| i.name.clone());
                match &taken {
                    Some(n) => {
                        ui.label(format!("slot holds \"{n}\": empty it first"));
                    }
                    None => {
                        ui.checkbox(&mut self.confirm_nam, "write that slot");
                    }
                }
                if ui.add_enabled(connected && self.confirm_nam && taken.is_none(), egui::Button::new("Upload capture")).clicked() {
                    self.confirm_nam = false;
                    let (file, slot, name) = (PathBuf::from(&self.nam_file), self.nam_slot - 1, self.nam_name.trim().to_owned());
                    self.start_job("uploading the capture", move |s| {
                        let text = std::fs::read_to_string(&file)?;
                        let c = nam::convert(&text)?;
                        let name = match name.is_empty() {
                            true => c.name.clone().ok_or_else(|| hxs_core::Error::Nam("the capture has no name: type one".into()))?,
                            false => name,
                        };
                        let data = c.ir_bytes();
                        stomp::new_ir_info(slot, &name, 0)?;
                        let id = s.ir_id_for(&data)?;
                        let info = stomp::new_ir_info(slot, &name, id)?;
                        let listed = stomp::upload_new_ir(s, slot, &info, &data)?;
                        let shown = listed.into_iter().map(|ir| IrSlot { capture: ir.index == slot, index: ir.index, name: ir.name });
                        Ok(JobResult::Irs(shown.collect()))
                    });
                }
            });
            let mut delete = None;
            egui::Grid::new("irs").striped(true).show(ui, |ui| {
                for ir in &self.irs {
                    ui.label(format!("{:03}", ir.index + 1));
                    ui.label(if ir.capture { "NAM capture" } else { "IR" });
                    ui.label(&ir.name);
                    if ui.add_enabled(connected && self.allow_ir_delete, egui::Button::new("Empty")).clicked() {
                        delete = Some(ir.index);
                    }
                    ui.end_row();
                }
            });
            if let Some(slot) = delete {
                self.allow_ir_delete = false;
                self.start_job("emptying the IR slot", move |s| {
                    s.batch_begin()?;
                    let deleted = s.delete_ir(slot);
                    let ended = s.batch_end();
                    deleted?;
                    ended?;
                    read_irs(s)
                });
            }
        });
    }

    fn catalog_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(&self.catalog_note);
        ui.label("The catalog holds the names of the models and parameters. It is extracted from your own firmware file (stock or HXtra) and never shipped with the editor.");
        ui.horizontal(|ui| {
            ui.label("File:");
            ui.add(egui::TextEdit::singleline(&mut self.catalog_path).desired_width(520.0));
            if ui.button("Load").clicked() {
                let p = PathBuf::from(&self.catalog_path);
                self.load_catalog(Some(&p));
            }
            if ui.button("Install").on_hover_text("Copies it into the data folder, where the editor finds it on every start").clicked() {
                let p = PathBuf::from(&self.catalog_path);
                match install_catalog(&p) {
                    Ok(dest) => {
                        self.note(format!("catalog installed in {}", dest.display()));
                        self.load_catalog(Some(&dest));
                    }
                    Err(e) => self.note(format!("not installed: {e}")),
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Names:");
            ui.radio_value(&mut self.style, NameStyle::Unit, "as on the unit");
            ui.radio_value(&mut self.style, NameStyle::Line6, "Line 6");
            ui.radio_value(&mut self.style, NameStyle::Real, "real-world (HXtra)");
        });
        let Some(c) = self.catalog.as_ref().filter(|_| !self.catalog_ignored) else { return };
        let s = &c.source;
        ui.label(format!(
            "{} firmware {}, {} models, {} added by HXtra, {}",
            s.product.as_deref().unwrap_or("?"),
            s.firmware.map_or("?".into(), version_text),
            c.models.len(),
            c.models.iter().filter(|m| m.hxtra_added == Some(true)).count(),
            match s.hxtra {
                Some(_) => "HXtra build",
                None => "stock",
            }
        ));
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.text_edit_singleline(&mut self.search);
        });
        let needle = self.search.to_lowercase();
        let style = self.style;
        egui::ScrollArea::vertical().id_salt("catalog").show(ui, |ui| {
            egui::Grid::new("models").striped(true).show(ui, |ui| {
                let names = |m: &Model| [Some(&m.title), m.stock_title.as_ref(), m.real_name.as_ref()].into_iter().flatten().any(|n| n.to_lowercase().contains(&needle));
                for m in c.models.iter().filter(|m| needle.is_empty() || names(m)).take(300) {
                    ui.label(m.preset_id.to_string());
                    ui.label(m.display_name(style));
                    ui.label(m.shown_category());
                    ui.label(m.subcategory.as_deref().unwrap_or(""));
                    ui.label(model_notes(m, style));
                    ui.end_row();
                }
            });
        });
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        let Some(d) = &self.device else {
            ui.label("not connected");
            return;
        };
        let Some(g) = &d.globals else {
            ui.label("the global settings could not be read");
            return;
        };
        ui.label("Global settings as read at connect (raw ids; names are not mapped yet). Changing one writes to the unit.");
        ui.horizontal(|ui| {
            ui.label("Setting id:");
            ui.add(egui::TextEdit::singleline(&mut self.setting_id).desired_width(60.0));
            ui.label("value:");
            ui.add(egui::TextEdit::singleline(&mut self.setting_value).desired_width(100.0));
            if ui.button("Set").clicked() {
                let typed = self.setting_id.trim().parse::<u64>().map_err(|_| "the id is a number".to_owned()).and_then(|id| typed_setting(g, id, self.setting_value.trim()).map(|v| (id, v)));
                match typed {
                    Ok((id, v)) => {
                        let p = self.handle.set_setting(id, v);
                        self.ops.push((format!("setting {id}"), p));
                    }
                    Err(e) => self.log.push_back(format!("not set: {e}")),
                }
            }
        });
        egui::ScrollArea::vertical().id_salt("globals").show(ui, |ui| {
            egui::Grid::new("globals_grid").striped(true).show(ui, |ui| {
                for e in g.entries() {
                    ui.label(format!("group {}", e.group));
                    ui.label(format!("id {}", e.id));
                    ui.label(restore::value_text(&e.value));
                    ui.end_row();
                }
            });
        });
    }

    fn log_tab(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("log").stick_to_bottom(true).show(ui, |ui| {
            for l in &self.log {
                ui.monospace(l);
            }
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        egui::TopBottomPanel::top("top").show(ctx, |ui| self.top_bar(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(self.log.back().map_or("", String::as_str));
        });
        egui::SidePanel::left("presets").default_width(220.0).show(ctx, |ui| {
            ui.heading("Presets");
            self.preset_list(ui);
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Preset => self.preset_tab(ui),
            Tab::Library => self.library_tab(ui),
            Tab::Catalog => self.catalog_tab(ui),
            Tab::Settings => self.settings_tab(ui),
            Tab::Log => self.log_tab(ui),
        });
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = self.handle.disconnect().wait();
        self.handle.shutdown();
    }
}

fn preset_view(p: &PresetFile) -> (Vec<Block>, Vec<String>) {
    let blocks = p.blocks().unwrap_or_default();
    let names = snapshot::snapshots(p).map(|s| s.into_iter().map(|s| s.name).collect()).unwrap_or_default();
    (blocks, names)
}

fn block_ui(ui: &mut egui::Ui, b: &mut Block, model: Option<&Model>, style: NameStyle, hidden: bool, irs: &[IrSlot], edits: &mut Vec<Edit>) {
    let position = snapshot::slot_label(b.position).unwrap_or_else(|| format!("#{}", b.position));
    let title = match model {
        Some(m) if m.is_capture() => format!("{position}  Capture: {}", m.display_name(style)),
        Some(m) => format!("{position}  {}", m.display_name(style)),
        None => format!("{position}  model {}", b.preset_id),
    };
    let block = b.position as u64;
    ui.group(|ui| {
        ui.horizontal(|ui| {
            if ui.checkbox(&mut b.enabled, "").changed() {
                edits.push(Edit::Enable { block, on: b.enabled });
            }
            ui.strong(title);
            if let Some(m) = model {
                ui.label(model_notes(m, style));
            }
        });
        egui::Grid::new(("params", b.position)).num_columns(3).show(ui, |ui| {
            for (i, v) in b.params.iter_mut().enumerate() {
                let p = model.and_then(|m| m.param(i));
                if p.is_some_and(|p| !(p.visible || hidden)) {
                    continue;
                }
                ui.label(p.map_or(format!("[{i}]"), |p| p.title.clone()));
                if param_ui(ui, p, v, irs, (b.position, i)) {
                    edits.push(Edit::Param { block, index: i as u64, value: v.clone() });
                }
                ui.label(p.filter(|p| p.hxtra_added == Some(true)).map_or("", |_| "HXtra"));
                ui.end_row();
            }
        });
    });
}

fn param_ui(ui: &mut egui::Ui, p: Option<&Param>, v: &mut Value, irs: &[IrSlot], id: (usize, usize)) -> bool {
    let number = v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)).unwrap_or(0.0);
    match p {
        Some(p) if p.role.as_deref() == Some(ROLE_CAPTURE_SLOT) => capture_ui(ui, v, number, irs, id),
        Some(p) => match p.kind {
            ParamKind::Float => {
                let mut x = number;
                let log = p.min > 0.0 && p.max / p.min >= 100.0;
                let fmt = |n: f64, _| p.format(&Value::F64(n));
                let changed = ui.add(egui::Slider::new(&mut x, p.min..=p.max).logarithmic(log).custom_formatter(fmt)).changed();
                if changed {
                    *v = Value::F32(x as f32);
                }
                changed
            }
            ParamKind::List | ParamKind::Int => match &p.choices {
                Some(choices) => {
                    let mut selected = number as i64;
                    let current = choices.iter().find(|c| c.value == selected).map_or(selected.to_string(), |c| c.label.clone());
                    egui::ComboBox::from_id_salt(("list", id)).selected_text(current).show_ui(ui, |ui| {
                        for c in choices {
                            ui.selectable_value(&mut selected, c.value, &c.label);
                        }
                    });
                    let changed = selected != number as i64;
                    if changed {
                        *v = Value::from(selected);
                    }
                    changed
                }
                None => {
                    let mut n = number as i64;
                    let changed = ui.add(egui::DragValue::new(&mut n).range(p.min as i64..=p.max as i64)).changed();
                    if changed {
                        *v = Value::from(n);
                    }
                    changed
                }
            },
            ParamKind::Bool => {
                let mut on = v.as_bool().unwrap_or(number != 0.0);
                let changed = ui.checkbox(&mut on, "").changed();
                if changed {
                    *v = Value::from(on);
                }
                changed
            }
            ParamKind::Data => {
                ui.label(restore::value_text(v));
                false
            }
        },
        None => raw_ui(ui, v),
    }
}

fn capture_ui(ui: &mut egui::Ui, v: &mut Value, number: f64, irs: &[IrSlot], id: (usize, usize)) -> bool {
    let mut slot = number.round().clamp(0.0, IR_SLOTS as f64) as u64;
    let name = |s: u64| match irs.iter().find(|i| i.index + 1 == s) {
        Some(i) if i.capture => i.name.clone(),
        Some(i) => format!("{} (an IR, not a capture)", i.name),
        None => "(empty slot)".to_owned(),
    };
    egui::ComboBox::from_id_salt(("capture", id)).width(300.0).selected_text(format!("{slot:03} {}", name(slot))).show_ui(ui, |ui| {
        for ir in irs.iter().filter(|i| i.capture) {
            ui.selectable_value(&mut slot, ir.index + 1, format!("{:03} {}", ir.index + 1, ir.name));
        }
    });
    let changed = slot != number.round() as u64;
    if changed {
        *v = Value::F32(slot as f32);
    }
    changed
}

fn raw_ui(ui: &mut egui::Ui, v: &mut Value) -> bool {
    match v {
        Value::F32(x) => ui.add(egui::DragValue::new(x).speed(0.01)).changed(),
        Value::Boolean(b) => ui.checkbox(b, "").changed(),
        Value::Integer(_) => {
            let mut n = v.as_i64().unwrap_or(0);
            let changed = ui.add(egui::DragValue::new(&mut n)).changed();
            if changed {
                *v = Value::from(n);
            }
            changed
        }
        other => {
            ui.label(restore::value_text(other));
            false
        }
    }
}

fn model_notes(m: &Model, style: NameStyle) -> String {
    let other = match style {
        NameStyle::Line6 => Some(("on the unit", m.title.as_str())),
        NameStyle::Unit | NameStyle::Real => m.stock_title.as_deref().map(|s| ("Line 6", s)),
    };
    let renamed = other.filter(|(_, n)| *n != m.display_name(style)).map(|(what, n)| format!("{what}: {n}"));
    let added = (m.hxtra_added == Some(true)).then(|| "added by HXtra".to_owned());
    let channels = m.subcategory.as_deref().filter(|s| matches!(*s, "mono" | "stereo")).map(str::to_owned);
    channels.into_iter().chain(renamed).chain(added).collect::<Vec<_>>().join("; ")
}

fn typed_setting(g: &Globals, id: u64, text: &str) -> Result<Value, String> {
    let current = g.entries().iter().filter(|e| e.id == id).min_by_key(|e| e.group == 3).map(|e| &e.value).ok_or(format!("setting {id} is not among the unit's settings"))?;
    let bad = |what: &str| format!("setting {id} holds {what}");
    match current {
        Value::F32(_) => text.parse::<f32>().map(Value::F32).map_err(|_| bad("a float")),
        Value::F64(_) => text.parse::<f64>().map(Value::F64).map_err(|_| bad("a double")),
        Value::Integer(_) => text.parse::<i64>().map(Value::from).map_err(|_| bad("an int")),
        Value::Boolean(_) => match text {
            "true" | "1" | "on" => Ok(Value::from(true)),
            "false" | "0" | "off" => Ok(Value::from(false)),
            _ => Err(bad("a bool (true/false)")),
        },
        _ => Err(bad("a value this form does not write")),
    }
}

fn install_catalog(from: &Path) -> hxs_core::Result<PathBuf> {
    Catalog::load(from)?;
    let dir = catalog::data_folder().ok_or_else(|| hxs_core::Error::Catalog("no data folder (APPDATA is not set)".into()))?;
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(catalog::FILE_NAME);
    std::fs::copy(from, &dest)?;
    Ok(dest)
}

fn default_backup_dir() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let base = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default();
    base.join("Documents").join("hxtra-editor backups").join(format!("backup-{secs}")).display().to_string()
}

fn version_text(f: u64) -> String {
    format!("{:x}.{:x}.{:x}", f >> 24, (f >> 16) & 0xFF, (f >> 8) & 0xFF)
}
