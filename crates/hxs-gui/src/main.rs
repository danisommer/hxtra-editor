//! hxtra-editor: a window over an HX Stomp running HXtra or stock firmware.

#![windows_subsystem = "windows"]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use hxs_core::backup;
use hxs_core::catalog::{self, Catalog, Model, NameStyle, Param, ParamKind, ROLE_CAPTURE_SLOT};
use hxs_core::device::{DeviceHandle, Event, Options, Pending, Snapshot, snapshot_number};
use hxs_core::globals::Globals;
use hxs_core::nam;
use hxs_core::notify::Notification;
use hxs_core::preset::{Block, PresetFile, ROW_A, ROW_B};
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
    Favorites,
    Catalog,
    Settings,
    Log,
}

enum JobResult {
    Text(String),
    Plan(PathBuf, Plan),
    Irs(Vec<IrSlot>),
    EditBuffer(Vec<u8>),
    Favorites(Vec<FavoriteView>),
    Lists(Vec<IrSlot>, Vec<FavoriteView>),
}

#[derive(Clone)]
struct FavoriteView {
    index: u64,
    name: String,
    block: Option<Block>,
}

/// The favorites with the block each one holds (`0x40` is a grid cell, its key 20 the block body).
fn read_favorites(s: &mut stomp::Stomp) -> hxs_core::Result<JobResult> {
    let list = s.favorite_list()?;
    let views = list
        .into_iter()
        .map(|f| {
            let v = s.favorite(f.index)?;
            let block = hxs_core::msg::get(&v, 0x40).and_then(|cell| hxs_core::msg::get(cell, 20)).and_then(|body| Block::parse(0, body).ok());
            Ok(FavoriteView { index: f.index, name: f.name, block })
        })
        .collect::<hxs_core::Result<Vec<_>>>()?;
    Ok(JobResult::Favorites(views))
}

const SETTING_GLOBAL_BPM: u64 = 15;
const SETTING_PRESET_TEMPO: u64 = 16;
const TEMPO_RANGE: std::ops::RangeInclusive<f32> = 20.0..=240.0;
const TAP_GAP: Duration = Duration::from_millis(2500);
const TAPS_KEPT: usize = 5;

/// BPM from tap times: the mean of the last intervals.
fn tap_bpm(taps: &[Instant]) -> Option<f32> {
    let gaps: Vec<f32> = taps.windows(2).map(|w| (w[1] - w[0]).as_secs_f32()).collect();
    match gaps.is_empty() {
        true => None,
        false => Some((60.0 * gaps.len() as f32 / gaps.iter().sum::<f32>()).clamp(*TEMPO_RANGE.start(), *TEMPO_RANGE.end())),
    }
}

fn setting_f32(g: &Globals, id: u64) -> Option<f32> {
    g.entries().iter().filter(|e| e.id == id).min_by_key(|e| e.group == 3).and_then(|e| e.value.as_f64()).map(|v| v as f32)
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
    Favorite { block: u64, name: String },
    Switch { block: u64, preset_id: u64, cab: Option<u64>, values: Vec<(u64, Value)> },
    Defaults { block: u64, values: Vec<(u64, Value)> },
    Move { from: u64, to: u64 },
    Remove { block: u64 },
    Bypass { block: u64, footswitch: u64 },
    FollowSnapshots { block: u64, follows: bool },
    Controller { block: u64, index: u64, source: stomp::ControllerSource },
    Range { block: u64, index: u64, max: bool, value: Value },
}

const FOOTSWITCHES: u64 = 5;

/// Positions of the grid with no block, row A then row B.
fn free_positions(blocks: &[Block]) -> Vec<usize> {
    ROW_A.chain(ROW_B).filter(|p| blocks.iter().all(|b| b.position != *p)).collect()
}

fn position_name(position: usize) -> String {
    snapshot::slot_label(position).unwrap_or_else(|| format!("#{position}"))
}

/// Set parameters one by one, then read the edit buffer back for the screen.
fn set_values_then_read(s: &mut stomp::Stomp, block: u64, values: &[(u64, Value)]) -> hxs_core::Result<JobResult> {
    for (i, v) in values {
        s.set_parameter(block, 0, *i, v.clone())?;
    }
    Ok(JobResult::EditBuffer(s.edit_buffer()?))
}

/// The knobs whose wanted value differs from `baseline` (what the block has, or will have,
/// without them); internal data parameters are never sent.
fn changed_knobs(model: &Model, wanted: &[Value], baseline: impl Fn(&Param) -> Option<Value>) -> Vec<(u64, Value)> {
    model
        .params
        .iter()
        .filter(|p| p.kind != ParamKind::Data)
        .filter_map(|p| wanted.get(p.index).filter(|v| baseline(p).as_ref() != Some(*v)).map(|v| (p.index as u64, v.clone())))
        .collect()
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
    preset_tempo: Option<f32>,
    global_bpm: Option<f32>,
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
    taps: Vec<Instant>,
    favorites: Vec<FavoriteView>,
    allow_favorite_delete: bool,
    new_favorite: Option<(u64, String)>,
    copy_from: usize,
    copy_to: usize,
    copy_name: String,
    confirm_copy: bool,
    export_slot: usize,
    export_file: String,
    add_position: usize,
    add_folder: usize,
    add_item: usize,
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
            taps: Vec::new(),
            favorites: Vec::new(),
            allow_favorite_delete: false,
            new_favorite: None,
            copy_from: 0,
            copy_to: 0,
            copy_name: String::new(),
            confirm_copy: false,
            export_slot: 0,
            export_file: String::new(),
            add_position: 0,
            add_folder: 0,
            add_item: 0,
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
        let globals = Globals::parse(s.globals).ok();
        let tempo = |id| globals.as_ref().and_then(|g| setting_f32(g, id));
        let (preset_tempo, global_bpm) = (tempo(SETTING_PRESET_TEMPO), tempo(SETTING_GLOBAL_BPM));
        self.device = Some(Device {
            current: s.current,
            presets: s.presets,
            blocks,
            snapshot_names,
            globals,
            firmware,
            snapshot: None,
            edited: false,
            preset_tempo,
            global_bpm,
        });
        self.catalog_ignored = false;
        self.check_catalog();
        self.start_job("reading the IR list and the favorites", |s| match (read_irs(s)?, read_favorites(s)?) {
            (JobResult::Irs(irs), JobResult::Favorites(favorites)) => Ok(JobResult::Lists(irs, favorites)),
            _ => unreachable!("read_irs and read_favorites return their own variants"),
        });
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
                Ok(JobResult::Favorites(f)) => {
                    self.note(format!("{label}: {} favorites", f.len()));
                    self.favorites = f;
                }
                Ok(JobResult::Lists(irs, f)) => {
                    self.note(format!("{label}: {} IR slots, {} favorites", irs.len(), f.len()));
                    self.irs = irs;
                    self.favorites = f;
                }
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
            Event::SettingChanged { id, value } => {
                let v = value.as_f64().map(|v| v as f32);
                if let Some(d) = self.device.as_mut() {
                    match id {
                        SETTING_PRESET_TEMPO => d.preset_tempo = v.or(d.preset_tempo),
                        SETTING_GLOBAL_BPM => d.global_bpm = v.or(d.global_bpm),
                        _ => {}
                    }
                }
            }
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
            for (tab, name) in [
                (Tab::Preset, "Preset"),
                (Tab::Library, "Library"),
                (Tab::Favorites, "Favorites"),
                (Tab::Catalog, "Catalog"),
                (Tab::Settings, "Settings"),
                (Tab::Log, "Log"),
            ] {
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
        let (mut chosen, mut copy, mut export) = (None, None, None);
        egui::ScrollArea::vertical().id_salt("presets").show(ui, |ui| {
            for p in &d.presets {
                let r = ui.selectable_label(p.slot == current, format!("{:03}  {}", p.slot, p.name)).on_hover_text("Right-click: copy or export");
                if r.clicked() {
                    chosen = Some(p.slot);
                }
                r.context_menu(|ui| {
                    if ui.button("Copy to another slot...").clicked() {
                        copy = Some((p.slot, p.name.clone()));
                        ui.close_menu();
                    }
                    if ui.button("Export to a file...").clicked() {
                        export = Some((p.slot, p.name.clone()));
                        ui.close_menu();
                    }
                });
            }
        });
        if let Some(slot) = chosen {
            let p = self.handle.select_preset(setlist, slot);
            self.op(format!("load preset {slot:03}"), p);
        }
        if let Some((slot, name)) = copy {
            (self.copy_from, self.copy_name, self.confirm_copy, self.tab) = (slot, name, false, Tab::Library);
        }
        if let Some((slot, name)) = export {
            self.export_slot = slot;
            self.export_file = default_export_path(slot, &name);
            self.tab = Tab::Library;
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
        self.tempo_row(ui);
        self.dsp_row(ui);
        self.add_block_row(ui);
        self.favorite_prompt(ui);
        ui.separator();
        let mut edits = Vec::new();
        let catalog = match self.catalog_ignored {
            true => None,
            false => self.catalog.as_ref(),
        };
        let (style, hidden, irs) = (self.style, self.show_hidden, &self.irs);
        let Some(d) = self.device.as_mut() else { return };
        let free = free_positions(&d.blocks);
        egui::ScrollArea::vertical().id_salt("blocks").show(ui, |ui| {
            if d.blocks.is_empty() {
                ui.label("no blocks in this preset");
            }
            for b in d.blocks.iter_mut() {
                block_ui(ui, b, catalog, style, hidden, irs, &free, &mut edits);
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
                Edit::Favorite { block, name } => self.new_favorite = Some((block, name)),
                Edit::Switch { block, preset_id, cab, values } => self.start_job("switching the model", move |s| {
                    s.change_model(block, preset_id, cab)?;
                    set_values_then_read(s, block, &values)
                }),
                Edit::Defaults { block, values } => self.start_job("restoring the block's defaults", move |s| set_values_then_read(s, block, &values)),
                Edit::Move { from, to } => self.start_job("moving the block", move |s| {
                    s.move_block(from, to)?;
                    Ok(JobResult::EditBuffer(s.edit_buffer()?))
                }),
                Edit::Remove { block } => self.start_job("removing the block", move |s| {
                    s.remove_block(block)?;
                    Ok(JobResult::EditBuffer(s.edit_buffer()?))
                }),
                Edit::Bypass { block, footswitch } => self.start_job("assigning the footswitch", move |s| {
                    s.assign_bypass(block, footswitch)?;
                    Ok(JobResult::Text(format!("block {} on/off on FS{}", position_name(block as usize), footswitch + 1)))
                }),
                Edit::FollowSnapshots { block, follows } => self.start_job("setting the snapshot bypass", move |s| {
                    s.set_snapshot_bypass(block, follows)?;
                    Ok(JobResult::Text(format!("block {} {} the snapshots", position_name(block as usize), if follows { "follows" } else { "ignores" })))
                }),
                Edit::Controller { block, index, source } => self.start_job("assigning the controller", move |s| {
                    s.assign_controller(block, index, source)?;
                    Ok(JobResult::Text(format!("block {} parameter {index}: {source:?}", position_name(block as usize))))
                }),
                Edit::Range { block, index, max, value } => self.start_job("setting the controller range", move |s| {
                    s.set_controller_range(block, index, max, value)?;
                    Ok(JobResult::Text(format!("block {} parameter {index}: controller {} set", position_name(block as usize), if max { "max" } else { "min" })))
                }),
            }
        }
    }

    fn add_block_row(&mut self, ui: &mut egui::Ui) {
        let (Some(d), Some(c)) = (&self.device, self.catalog.as_ref().filter(|_| !self.catalog_ignored)) else { return };
        let free = free_positions(&d.blocks);
        if free.is_empty() || c.browser.is_empty() {
            return;
        }
        let style = self.style;
        self.add_position = self.add_position.min(free.len() - 1);
        self.add_folder = self.add_folder.min(c.browser.len() - 1);
        let folder = &c.browser[self.add_folder];
        self.add_item = self.add_item.min(folder.items.len().saturating_sub(1));
        let mut add = None;
        ui.horizontal(|ui| {
            ui.label("Add block at");
            egui::ComboBox::from_id_salt("add_pos").selected_text(position_name(free[self.add_position])).show_ui(ui, |ui| {
                for (i, p) in free.iter().enumerate() {
                    ui.selectable_value(&mut self.add_position, i, position_name(*p));
                }
            });
            let folder_name = |f: &catalog::Folder| format!("{} / {}", f.category.as_deref().unwrap_or("?"), f.folder);
            egui::ComboBox::from_id_salt("add_folder").width(200.0).selected_text(folder_name(folder)).show_ui(ui, |ui| {
                for (i, f) in c.browser.iter().enumerate() {
                    ui.selectable_value(&mut self.add_folder, i, folder_name(f));
                }
            });
            let item_name = |it: &catalog::FolderItem| it.model.and_then(|m| c.model(m)).map_or(it.title.clone(), |m| m.display_name(style).to_owned());
            let current = folder.items.get(self.add_item).map(item_name).unwrap_or_default();
            egui::ComboBox::from_id_salt("add_item").width(220.0).selected_text(current).show_ui(ui, |ui| {
                for (i, it) in folder.items.iter().enumerate() {
                    ui.selectable_value(&mut self.add_item, i, item_name(it));
                }
            });
            if ui.button("Add").on_hover_text("Puts the model with its defaults in the edit buffer; Save stores it").clicked() {
                add = folder.items.get(self.add_item).and_then(|it| it.model.map(|m| (free[self.add_position] as u64, m, it.cab, it.kind)));
            }
        });
        if let Some((position, model, cab, kind)) = add {
            self.start_job("adding the block", move |s| {
                s.add_block(position, model, cab, kind)?;
                Ok(JobResult::EditBuffer(s.edit_buffer()?))
            });
        }
    }

    fn tempo_row(&mut self, ui: &mut egui::Ui) {
        let Some(d) = self.device.as_mut() else { return };
        let mut writes = Vec::new();
        ui.horizontal(|ui| {
            ui.label("Preset tempo:");
            match d.preset_tempo.as_mut() {
                Some(t) => {
                    if ui.add(egui::DragValue::new(t).range(TEMPO_RANGE).speed(0.1).suffix(" BPM")).changed() {
                        writes.push((SETTING_PRESET_TEMPO, *t));
                    }
                }
                None => {
                    ui.label("?");
                }
            }
            if ui.button("Tap").on_hover_text("Tap at least twice: sets the current preset's tempo (as the unit's tap does)").clicked() {
                let now = Instant::now();
                if self.taps.last().is_some_and(|t| now - *t > TAP_GAP) {
                    self.taps.clear();
                }
                self.taps.push(now);
                if self.taps.len() > TAPS_KEPT {
                    self.taps.remove(0);
                }
                if let Some(bpm) = tap_bpm(&self.taps) {
                    d.preset_tempo = Some(bpm);
                    writes.push((SETTING_PRESET_TEMPO, bpm));
                }
            }
            ui.separator();
            ui.label("Global tempo:");
            match d.global_bpm.as_mut() {
                Some(t) => {
                    if ui.add(egui::DragValue::new(t).range(TEMPO_RANGE).speed(0.1).suffix(" BPM")).changed() {
                        writes.push((SETTING_GLOBAL_BPM, *t));
                    }
                }
                None => {
                    ui.label("?");
                }
            }
        });
        if writes.iter().any(|(id, _)| *id == SETTING_PRESET_TEMPO) {
            d.edited = true;
        }
        for (id, bpm) in writes {
            let p = self.handle.set_setting(id, Value::F32(bpm));
            self.op(format!("tempo {bpm:.1} BPM (setting {id})"), p);
        }
    }

    fn dsp_row(&self, ui: &mut egui::Ui) {
        let (Some(d), Some(c)) = (&self.device, self.catalog.as_ref().filter(|_| !self.catalog_ignored)) else { return };
        let ids: Vec<u64> = d.blocks.iter().flat_map(|b| std::iter::once(b.preset_id).chain(b.cab)).collect();
        let (total, unknown) = c.dsp_total(&ids);
        ui.horizontal(|ui| {
            ui.label("DSP:");
            let bar = egui::ProgressBar::new(total as f32 / 10000.0).desired_width(240.0).text(format!("{:.1}% of {:.0}%", total as f32 / 100.0, catalog::DSP_LIMIT as f32 / 100.0));
            ui.add(bar).on_hover_text("The firmware's declared costs: 15% fixed plus each block's (HXtra's research); the unit refuses a preset above 90%");
            if !unknown.is_empty() {
                ui.label(format!("(not counted: models {unknown:?})"));
            }
        });
    }

    fn favorite_prompt(&mut self, ui: &mut egui::Ui) {
        let Some((block, name)) = self.new_favorite.as_mut() else { return };
        let block = *block;
        let (mut save, mut cancel) = (false, false);
        ui.horizontal(|ui| {
            ui.label(format!("Save block {} as a new favorite, named:", snapshot::slot_label(block as usize).unwrap_or_default()));
            ui.add(egui::TextEdit::singleline(name).desired_width(160.0).char_limit(stomp::MAX_PRESET_NAME));
            save = ui.button("Save favorite").on_hover_text("Writes a new favorite to the unit").clicked();
            cancel = ui.button("Cancel").clicked();
        });
        match (save, cancel) {
            (true, _) => {
                let name = name.clone();
                let index = self.favorites.len() as u64;
                self.new_favorite = None;
                self.start_job("saving the favorite", move |s| {
                    s.save_block_as_favorite(block, index, &name)?;
                    read_favorites(s)
                });
            }
            (false, true) => self.new_favorite = None,
            (false, false) => {}
        }
    }

    fn favorites_tab(&mut self, ui: &mut egui::Ui) {
        let connected = self.device.is_some() && self.job.is_none();
        ui.horizontal(|ui| {
            ui.heading("Favorites");
            if ui.add_enabled(connected, egui::Button::new("Refresh")).clicked() {
                self.start_job("reading the favorites", read_favorites);
            }
            ui.checkbox(&mut self.allow_favorite_delete, "allow deleting");
        });
        ui.label("A favorite is a block saved with its settings. Save one from a block in the Preset tab.");
        let catalog = self.catalog.as_ref().filter(|_| !self.catalog_ignored);
        let style = self.style;
        let mut delete = None;
        egui::ScrollArea::vertical().id_salt("favorites").show(ui, |ui| {
            egui::Grid::new("favorites_grid").striped(true).show(ui, |ui| {
                for f in &self.favorites {
                    ui.label(format!("{:02}", f.index + 1));
                    ui.strong(&f.name);
                    let model = f.block.as_ref().map(|b| b.preset_id);
                    ui.label(match (model, catalog.zip(model).and_then(|(c, id)| c.model(id))) {
                        (_, Some(m)) => format!("{} ({})", m.display_name(style), m.shown_category()),
                        (Some(id), None) => format!("model {id}"),
                        (None, None) => "?".into(),
                    });
                    if ui.add_enabled(connected && self.allow_favorite_delete, egui::Button::new("Delete")).clicked() {
                        delete = Some(f.index);
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(index) = delete {
            self.allow_favorite_delete = false;
            self.start_job("deleting the favorite", move |s| {
                s.delete_favorite(index)?;
                read_favorites(s)
            });
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
            ui.heading("Copy a preset to another slot");
            ui.label("The whole preset as the unit stores it, snapshots 4-6 included. Right-click a preset in the list to start here.");
            ui.horizontal(|ui| {
                ui.label("From slot:");
                ui.add(egui::DragValue::new(&mut self.copy_from).range(0..=125));
                ui.label("to slot:");
                ui.add(egui::DragValue::new(&mut self.copy_to).range(0..=125));
                ui.label("named:");
                ui.add(egui::TextEdit::singleline(&mut self.copy_name).desired_width(160.0).char_limit(stomp::MAX_PRESET_NAME));
                let target = self.device.as_ref().and_then(|d| d.presets.get(self.copy_to)).map_or("?".to_owned(), |p| p.name.clone());
                ui.checkbox(&mut self.confirm_copy, format!("overwrite {:03} \"{target}\"", self.copy_to));
                let ready = connected && self.confirm_copy && self.copy_from != self.copy_to;
                if ui.add_enabled(ready, egui::Button::new("Copy")).clicked() {
                    let (from, to, name) = (self.copy_from, self.copy_to, self.copy_name.clone());
                    self.confirm_copy = false;
                    self.start_job("copying the preset", move |s| {
                        stomp::validate_name(&name)?;
                        let file = s.preset_file(setlist, from)?;
                        s.batch_begin()?;
                        let written = s.upload_preset(setlist, to, &name, ListFlags::default(), &file);
                        let ended = s.batch_end();
                        written?;
                        ended?;
                        let back = s.preset_file(setlist, to)?;
                        Ok(JobResult::Text(match back == file {
                            true => format!("{from:03} copied to {to:03} \"{name}\", read back identical"),
                            false => format!("{from:03} copied to {to:03}, but the read back differs"),
                        }))
                    });
                }
            });
            ui.separator();
            ui.heading("Export a preset to a file");
            ui.label("Saved as the unit stores it (.l6p, snapshots 4-6 included): it imports back with \"Write one preset file\" below.");
            ui.horizontal(|ui| {
                ui.label("Slot:");
                ui.add(egui::DragValue::new(&mut self.export_slot).range(0..=125));
                ui.label("file:");
                ui.add(egui::TextEdit::singleline(&mut self.export_file).desired_width(420.0));
                if ui.add_enabled(connected && !self.export_file.is_empty(), egui::Button::new("Export")).clicked() {
                    let (slot, file) = (self.export_slot, PathBuf::from(&self.export_file));
                    self.start_job("exporting the preset", move |s| {
                        let bytes = s.preset_file(setlist, slot)?;
                        PresetFile::parse(bytes.clone())?;
                        if let Some(dir) = file.parent() {
                            std::fs::create_dir_all(dir)?;
                        }
                        std::fs::write(&file, &bytes)?;
                        Ok(JobResult::Text(format!("{slot:03} -> {} ({} bytes, sha256 {})", file.display(), bytes.len(), backup::sha256_hex(&bytes))))
                    });
                }
            });
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
                        stomp::check_listed_ir(&listed, slot, &name, &data)?;
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
            Tab::Favorites => self.favorites_tab(ui),
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

#[allow(clippy::too_many_arguments)]
fn block_ui(ui: &mut egui::Ui, b: &mut Block, catalog: Option<&Catalog>, style: NameStyle, hidden: bool, irs: &[IrSlot], free: &[usize], edits: &mut Vec<Edit>) {
    let model = catalog.and_then(|c| c.model(b.preset_id));
    let position = position_name(b.position);
    let cab = b.cab.map(|id| catalog.and_then(|c| c.model(id)).map_or(format!("cab {id}"), |m| m.display_name(style).to_owned()));
    let title = match (model, &cab) {
        (Some(m), _) if m.is_capture() => format!("{position}  Capture: {}", m.display_name(style)),
        (Some(m), Some(cab)) => format!("{position}  {} + {cab}", m.display_name(style)),
        (Some(m), None) => format!("{position}  {}", m.display_name(style)),
        (None, _) => format!("{position}  model {}", b.preset_id),
    };
    let block = b.position as u64;
    let (c, m) = match (catalog, model) {
        (Some(c), Some(m)) => (Some(c), Some(m)),
        _ => (None, None),
    };
    ui.group(|ui| {
        ui.horizontal(|ui| {
            if ui.checkbox(&mut b.enabled, "").changed() {
                edits.push(Edit::Enable { block, on: b.enabled });
            }
            ui.strong(title);
            if let Some(m) = model {
                ui.label(model_notes(m, style));
                if let Some(cost) = m.dsp_cost {
                    ui.label(format!("DSP {:.1}%", cost as f32 / 100.0));
                }
            }
            if ui.small_button("Save as favorite").clicked() {
                let name: String = model.map_or("Favorite".to_owned(), |m| m.display_name(style).chars().filter(|c| (' '..='~').contains(c)).take(stomp::MAX_PRESET_NAME).collect());
                edits.push(Edit::Favorite { block, name });
            }
        });
        ui.horizontal(|ui| block_actions(ui, b, c.zip(m), style, free, edits));
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
                if p.is_none_or(|p| p.kind != ParamKind::Data) {
                    controller_menu(ui, block, i, v, edits);
                }
                ui.end_row();
            }
        });
    });
}

/// Switch to a variant (keeping shared values), add or drop an amp's cab, restore defaults,
/// move or remove: the block's row of actions.
fn block_actions(ui: &mut egui::Ui, b: &Block, cm: Option<(&Catalog, &Model)>, style: NameStyle, free: &[usize], edits: &mut Vec<Edit>) {
    let block = b.position as u64;
    if let Some((c, m)) = cm {
        let mut targets: Vec<(String, u64, Option<u64>)> = c
            .variants(m)
            .into_iter()
            .map(|v| (format!("{} ({})", v.display_name(style), v.subcategory.as_deref().unwrap_or(v.shown_category())), v.preset_id, b.cab))
            .collect();
        match (b.cab, c.default_cab(m.preset_id)) {
            (Some(_), _) => targets.push(("without its cab".into(), m.preset_id, None)),
            (None, Some(cab)) => targets.push((format!("with its cab ({})", c.model(cab).map_or(cab.to_string(), |x| x.display_name(style).to_owned())), m.preset_id, Some(cab))),
            (None, None) => {}
        }
        let mut chosen = None;
        egui::ComboBox::from_id_salt(("switch", b.position)).selected_text("Switch to...").show_ui(ui, |ui| {
            for (label, id, cab) in &targets {
                if ui.selectable_label(false, label).on_hover_text("Keeps the values both models have").clicked() {
                    chosen = Some((*id, *cab));
                }
            }
        });
        if let Some((id, cab)) = chosen {
            match c.model(id).map(|to| (to, catalog::carry_values(m, &b.params, to))) {
                Some((to, Ok(values))) => {
                    let values = changed_knobs(to, &values, |p| Some(p.default_value()));
                    edits.push(Edit::Switch { block, preset_id: id, cab, values });
                }
                Some((_, Err(e))) => {
                    ui.label(format!("cannot switch: {e}"));
                }
                None => {}
            }
        }
        if ui.small_button("Factory defaults").on_hover_text("Sets every parameter of this block back to the model's default").clicked() {
            let values = changed_knobs(m, &catalog::default_values(m, &b.params), |p| b.params.get(p.index).cloned());
            edits.push(Edit::Defaults { block, values });
        }
    }
    let mut to = None;
    egui::ComboBox::from_id_salt(("move", b.position)).selected_text("Move to...").show_ui(ui, |ui| {
        for p in free {
            let row = if ROW_B.contains(p) { " (parallel path)" } else { "" };
            if ui.selectable_label(false, format!("{}{row}", position_name(*p))).clicked() {
                to = Some(*p as u64);
            }
        }
    });
    if let Some(to) = to {
        edits.push(Edit::Move { from: block, to });
    }
    if ui.small_button("Remove").clicked() {
        edits.push(Edit::Remove { block });
    }
    let mut footswitch = None;
    egui::ComboBox::from_id_salt(("bypass", b.position)).selected_text("Bypass on FS...").show_ui(ui, |ui| {
        for fs in 0..FOOTSWITCHES {
            if ui.selectable_label(false, format!("FS{}", fs + 1)).clicked() {
                footswitch = Some(fs);
            }
        }
    });
    if let Some(footswitch) = footswitch {
        edits.push(Edit::Bypass { block, footswitch });
    }
    if ui.small_button("Ignore snapshots").on_hover_text("Snapshots stop turning this block on or off").clicked() {
        edits.push(Edit::FollowSnapshots { block, follows: false });
    }
    if ui.small_button("Follow snapshots").clicked() {
        edits.push(Edit::FollowSnapshots { block, follows: true });
    }
}

/// A parameter's controller menu: EXP 1, none, and the current value as the pedal's heel or toe.
fn controller_menu(ui: &mut egui::Ui, block: u64, index: usize, value: &Value, edits: &mut Vec<Edit>) {
    let index = index as u64;
    ui.menu_button("ctl", |ui| {
        if ui.button("Assign to EXP 1").clicked() {
            edits.push(Edit::Controller { block, index, source: stomp::ControllerSource::Exp1 });
            ui.close_menu();
        }
        if ui.button("Remove controller").clicked() {
            edits.push(Edit::Controller { block, index, source: stomp::ControllerSource::None });
            ui.close_menu();
        }
        if ui.button("Use current value as min (heel)").clicked() {
            edits.push(Edit::Range { block, index, max: false, value: value.clone() });
            ui.close_menu();
        }
        if ui.button("Use current value as max (toe)").clicked() {
            edits.push(Edit::Range { block, index, max: true, value: value.clone() });
            ui.close_menu();
        }
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

fn default_export_path(slot: usize, name: &str) -> String {
    let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    let base = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default();
    base.join("Documents").join("hxtra-editor presets").join(format!("{slot:03} {}.l6p", safe.trim())).display().to_string()
}

fn version_text(f: u64) -> String {
    format!("{:x}.{:x}.{:x}", f >> 24, (f >> 16) & 0xFF, (f >> 8) & 0xFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_tempo_is_the_mean_interval() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        assert_eq!(tap_bpm(&[at(0)]), None);
        assert_eq!(tap_bpm(&[at(0), at(500), at(1000)]), Some(120.0));
        assert_eq!(tap_bpm(&[at(0), at(10)]), Some(240.0), "clamped to the unit's range");
        assert_eq!(tap_bpm(&[at(0), at(5000)]), Some(20.0));
    }
}
