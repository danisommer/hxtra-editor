//! hxtra-editor: a window over an HX Stomp running HXtra or stock firmware.

#![windows_subsystem = "windows"]

mod editor;
mod icons;
mod knob;
mod theme;
mod window;

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
use hxs_core::preset::{Block, PresetFile, ROW_B};
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
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 840.0]).with_min_inner_size([900.0, 560.0]).with_title("hxtra-editor").with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "hxtra-editor",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Edit,
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
    /// An edit the unit refused, with its edit buffer as it now stands.
    Rejected(Vec<u8>, String),
    Favorites(Vec<FavoriteView>),
    Lists(Vec<IrSlot>, Vec<FavoriteView>),
    Presets(Vec<PresetEntry>),
    BankB(usize, Vec<String>),
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

const FOOTSWITCHES: u64 = 5;
/// Moves the unit reports smaller than this share of a knob's range do not move it on screen:
/// an expression pedal at rest reports neighbouring values back and forth.
const DISPLAY_HYSTERESIS: f64 = 0.01;

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
    /// The names of snapshots 4..6 from the stored preset (the edit buffer read lacks bank B);
    /// empty when the preset has no bank B.
    bank_b_names: Vec<String>,
    routing: hxs_core::preset::Routing,
    footswitches: Vec<hxs_core::preset::Footswitch>,
}

/// The names of the stored preset's snapshots 4..6, or none without a bank B.
fn read_bank_b_names(s: &mut stomp::Stomp, setlist: u64, slot: usize) -> hxs_core::Result<JobResult> {
    let p = PresetFile::parse(s.preset_file(setlist, slot)?)?;
    let names = match p.has_bank_b() {
        true => snapshot::snapshots(&p)?.into_iter().filter(|s| s.bank == snapshot::Bank::B).map(|s| s.name).collect(),
        false => Vec::new(),
    };
    Ok(JobResult::BankB(slot, names))
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
    jobs: Vec<(String, Pending<JobResult>)>,
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
    snapshot_name: String,
    snapshot_color: u64,
    footswitch_labels: [String; FOOTSWITCHES as usize],
    footswitch_colors: [u64; FOOTSWITCHES as usize],
    ed: editor::EditorState,
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
            jobs: Vec::new(),
            tab: Tab::Edit,
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
            snapshot_name: String::new(),
            snapshot_color: 0,
            footswitch_labels: Default::default(),
            footswitch_colors: [0; FOOTSWITCHES as usize],
            ed: editor::EditorState::default(),
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
        self.jobs.push((label.to_owned(), self.handle.run(f)));
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
            bank_b_names: Vec::new(),
            routing: parsed.as_ref().map(PresetFile::routing).unwrap_or_default(),
            footswitches: parsed.as_ref().map(PresetFile::footswitches).unwrap_or_default(),
        });
        self.catalog_ignored = false;
        self.check_catalog();
        self.read_bank_b();
        self.start_job("reading the IR list and the favorites", |s| match (read_irs(s)?, read_favorites(s)?) {
            (JobResult::Irs(irs), JobResult::Favorites(favorites)) => Ok(JobResult::Lists(irs, favorites)),
            _ => unreachable!("read_irs and read_favorites return their own variants"),
        });
    }

    fn read_bank_b(&mut self) {
        if let Some((setlist, slot)) = self.device.as_ref().map(|d| (d.current.setlist, d.current.slot as usize)) {
            self.start_job("reading snapshots 4-6", move |s| read_bank_b_names(s, setlist, slot));
        }
    }

    fn load_edit_buffer(&mut self, bytes: Vec<u8>) {
        let view = PresetFile::parse(bytes).map(|p| (preset_view(&p), p.routing(), p.footswitches()));
        match (view, self.device.as_mut()) {
            (Ok(((blocks, names), routing, footswitches)), Some(d)) => {
                d.blocks = blocks;
                d.snapshot_names = names;
                d.routing = routing;
                d.footswitches = footswitches;
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
        let mut done = Vec::new();
        self.jobs.retain(|(label, p)| match p.poll() {
            None => true,
            Some(r) => {
                done.push((label.clone(), r));
                false
            }
        });
        for (label, r) in done {
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
                Ok(JobResult::Rejected(b, e)) => {
                    self.note(format!("{label} failed: {e}"));
                    self.load_edit_buffer(b);
                }
                Ok(JobResult::Favorites(f)) => {
                    self.note(format!("{label}: {} favorites", f.len()));
                    self.favorites = f;
                }
                Ok(JobResult::Presets(list)) => {
                    self.note(format!("{label}: done"));
                    if let Some(d) = self.device.as_mut() {
                        d.presets = list;
                    }
                }
                Ok(JobResult::BankB(slot, names)) => {
                    if let Some(d) = self.device.as_mut().filter(|d| d.current.slot as usize == slot) {
                        d.bank_b_names = names;
                    }
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
                    d.bank_b_names.clear();
                }
                self.load_edit_buffer(edit_buffer);
                self.read_bank_b();
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
                let catalog = self.catalog.as_ref().filter(|_| !self.catalog_ignored);
                if let Some(d) = self.device.as_mut() {
                    d.edited = true;
                    if let Some(b) = d.blocks.iter_mut().find(|b| b.position as u64 == block) {
                        let range = catalog.and_then(|c| c.model(b.preset_id)).and_then(|m| m.param(index as usize)).filter(|p| p.kind == ParamKind::Float).map(|p| (p.max - p.min).abs());
                        if let Some(v) = b.params.get_mut(index as usize) {
                            let jitter = match (range, v.as_f64(), value.as_f64()) {
                                (Some(range), Some(old), Some(new)) => (new - old).abs() < range * DISPLAY_HYSTERESIS,
                                _ => false,
                            };
                            if !jitter {
                                *v = value;
                            }
                        }
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
            if let Some((label, _)) = self.jobs.first() {
                ui.spinner();
                ui.label(label);
            }
        });
    }

    fn favorites_tab(&mut self, ui: &mut egui::Ui) {
        let connected = self.device.is_some() && self.jobs.is_empty();
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
        let connected = self.device.is_some() && self.jobs.is_empty();
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
        egui::TopBottomPanel::top("top").exact_height(window::TITLE_HEIGHT).frame(egui::Frame::none().fill(theme::PANEL)).show(ctx, |ui| window::title_bar(ui, |ui| self.top_bar(ui)));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.log.back().map_or("", String::as_str));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.weak(format!("catalog: {}", self.catalog_note)));
            });
        });
        egui::SidePanel::left("screens").exact_width(48.0).resizable(false).show(ctx, |ui| self.screen_bar(ui));
        egui::SidePanel::left("presets").default_width(210.0).min_width(180.0).show(ctx, |ui| self.preset_panel(ui));
        if self.tab == Tab::Edit {
            egui::SidePanel::left("browser").default_width(220.0).show(ctx, |ui| self.browser_panel(ui));
        }
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Edit => self.editor_screen(ui),
            Tab::Library => self.library_tab(ui),
            Tab::Favorites => self.favorites_tab(ui),
            Tab::Catalog => self.catalog_tab(ui),
            Tab::Settings => self.settings_tab(ui),
            Tab::Log => self.log_tab(ui),
        });
        self.editor_windows(ctx);
        self.shortcuts(ctx);
        editor::paint_drag_ghost(ctx);
        window::edges(ctx);
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
