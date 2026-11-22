//! The edit screen: the preset list, the model browser, the signal chain and the block panel,
//! and the gestures over them: drag and drop (inserting with push everywhere), Ctrl+C/Ctrl+V,
//! Delete, Ctrl+Z/Ctrl+Y (the preset's edits) and Ctrl+S.

use eframe::egui::{Align2, FontId, Key, Modifiers, Rect, Sense, Stroke, pos2, vec2};
use hxs_core::chain::{self, Layout, Op};

use super::*;
use crate::{icons, knob, theme};

const HISTORY: usize = 100;
const NEW_BLOCK: u64 = u64::MAX;
const TILE: f32 = 62.0;
const ROW_GAP: f32 = 44.0;
const IO_WIDTH: f32 = 46.0;
const CELL: egui::Vec2 = egui::Vec2::new(112.0, 124.0);
const CLIP_MARK: &str = "hxtra-editor";

#[derive(Clone, Copy, PartialEq, Default)]
pub(crate) enum Focus {
    #[default]
    Chain,
    Presets,
}

/// What a drag carries.
#[derive(Clone)]
pub(crate) enum Drag {
    Block(usize),
    Model { preset_id: u64, cab: Option<u64>, kind: u64, name: String },
    Preset(usize),
}

#[derive(Default)]
pub(crate) struct EditorState {
    selected: Option<usize>,
    focus: Focus,
    clip: Option<Block>,
    preset_clip: Option<usize>,
    paste_preset: Option<(usize, usize)>,
    history: Vec<Vec<Block>>,
    redo: Vec<Vec<Block>>,
    picker: Option<usize>,
    browser_search: String,
    picker_search: String,
    footswitch_window: bool,
}

pub(crate) fn position_name(position: usize) -> String {
    snapshot::slot_label(position).unwrap_or_else(|| format!("#{position}"))
}

fn layout_of(blocks: &[Block]) -> Layout {
    blocks.iter().map(|b| (b.position, b.position as u64)).collect()
}

fn category_of(c: Option<&Catalog>, preset_id: u64) -> String {
    match c.and_then(|c| c.model(preset_id)) {
        Some(m) if m.is_capture() => "capture".into(),
        Some(m) => m.category.clone().unwrap_or_default(),
        None => String::new(),
    }
}

fn model_name(c: Option<&Catalog>, style: NameStyle, preset_id: u64) -> String {
    c.and_then(|c| c.model(preset_id)).map_or(format!("model {preset_id}"), |m| m.display_name(style).to_owned())
}

/// Each model's defaults, for the edits that give a block a new model.
fn defaults_of(c: Option<&Catalog>) -> impl Fn(u64) -> Option<Vec<Value>> + '_ {
    move |id| c.and_then(|c| c.model(id)).map(|m| m.params.iter().map(Param::default_value).collect())
}

/// Leave out the writes of internal data parameters, which are not knobs.
fn without_data(c: Option<&Catalog>, target: &[Block], ops: Vec<Op>) -> Vec<Op> {
    let is_data = |position: u64, index: u64| {
        let block = target.iter().find(|b| b.position as u64 == position);
        let param = block.and_then(|b| c.and_then(|c| c.model(b.preset_id))).and_then(|m| m.param(index as usize));
        param.is_some_and(|p| p.kind == ParamKind::Data)
    };
    ops.into_iter().filter(|op| !matches!(op, Op::Set { position, index, .. } if is_data(*position, *index))).collect()
}

/// A gesture on the chain or the panel, handled after drawing.
enum Act {
    Select(usize),
    Pick(usize),
    Drop(Drag, usize),
    Toggle(usize),
    Param { position: usize, index: usize, value: Value, old: Value, began: bool },
    Target(Vec<Block>, &'static str),
    Favorite(usize, String),
    Bypass(usize, u64),
    FollowSnapshots(usize, bool),
    Controller(usize, usize, stomp::ControllerSource),
    Range(usize, usize, bool, Value),
    Remove(usize),
    Copy,
    Paste,
}

impl App {
    pub(crate) fn screen_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        let screens = [
            (Tab::Edit, "edit", "Edit the preset"),
            (Tab::Library, "library", "Library: backup, restore, IRs and captures"),
            (Tab::Favorites, "favorites", "Favorites"),
            (Tab::Catalog, "catalog", "Model catalog"),
            (Tab::Settings, "settings", "Global settings"),
            (Tab::Log, "log", "Log"),
        ];
        for (tab, icon, tip) in screens {
            let (rect, r) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
            let on = self.tab == tab;
            if on {
                ui.painter().rect_filled(rect, 6.0, theme::TILE);
            }
            let color = match (on, r.hovered()) {
                (true, _) => theme::SELECTED,
                (false, true) => theme::TEXT,
                (false, false) => theme::MUTED,
            };
            icons::screen(ui.painter(), rect, icon, color);
            if r.on_hover_text(tip).clicked() {
                self.tab = tab;
            }
        }
    }

    fn catalog_in_use(&self) -> Option<&Catalog> {
        self.catalog.as_ref().filter(|_| !self.catalog_ignored)
    }

    fn push_history(&mut self) {
        let Some(d) = &self.device else { return };
        self.ed.history.push(d.blocks.clone());
        if self.ed.history.len() > HISTORY {
            self.ed.history.remove(0);
        }
        self.ed.redo.clear();
    }

    /// Run edits of the edit buffer on the unit, then read it back for the screen.
    fn edit_job(&mut self, label: &str, f: impl FnOnce(&mut stomp::Stomp) -> hxs_core::Result<()> + Send + 'static) {
        if let Some(d) = self.device.as_mut() {
            d.edited = true;
        }
        self.start_job(label, move |s| {
            f(s)?;
            Ok(JobResult::EditBuffer(s.edit_buffer()?))
        });
    }

    /// Turn the chain into `target` (undo, redo, paste, switch, defaults).
    fn reach(&mut self, target: Vec<Block>, label: &str) {
        let Some(d) = &self.device else { return };
        let c = self.catalog_in_use();
        match chain::reconcile(&d.blocks, &target, defaults_of(c)) {
            Ok(ops) => {
                let ops = without_data(c, &target, ops);
                self.edit_job(label, move |s| chain::apply(s, &ops));
            }
            Err(e) => self.note(format!("{label}: {e}")),
        }
    }

    pub(crate) fn preset_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Presets");
        let Some(d) = &self.device else {
            ui.weak("not connected");
            return;
        };
        let (setlist, current) = (d.current.setlist, d.current.slot as usize);
        let (mut load, mut reorder, mut copy, mut paste, mut export) = (None, None, None, None, None);
        egui::ScrollArea::vertical().id_salt("presets").show(ui, |ui| {
            for p in &d.presets {
                let r = ui.add(egui::SelectableLabel::new(p.slot == current, format!("{:03}  {}", p.slot, p.name))).interact(Sense::click_and_drag());
                r.dnd_set_drag_payload(Drag::Preset(p.slot));
                if let Some(drag) = r.dnd_release_payload::<Drag>() {
                    if let Drag::Preset(from) = *drag {
                        reorder = Some((from, p.slot));
                    }
                }
                if r.dnd_hover_payload::<Drag>().is_some_and(|d| matches!(*d, Drag::Preset(_))) {
                    ui.painter().hline(r.rect.x_range(), r.rect.top(), Stroke::new(2.0_f32, theme::SELECTED));
                }
                if r.clicked() {
                    load = Some(p.slot);
                }
                r.context_menu(|ui| {
                    if ui.button("Copy  (Ctrl+C)").clicked() {
                        copy = Some(p.slot);
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.ed.preset_clip.is_some(), egui::Button::new("Paste over this preset  (Ctrl+V)")).clicked() {
                        paste = Some(p.slot);
                        ui.close_menu();
                    }
                    if ui.button("Export to a file...").clicked() {
                        export = Some((p.slot, p.name.clone()));
                        ui.close_menu();
                    }
                });
            }
        });
        if let Some(slot) = load {
            self.ed.focus = Focus::Presets;
            let p = self.handle.select_preset(setlist, slot);
            self.op(format!("load preset {slot:03}"), p);
        }
        if let Some((from, to)) = reorder.filter(|(a, b)| a != b) {
            let Some(d) = &self.device else { return };
            let mut order = d.presets.clone();
            let item = order.remove(from);
            order.insert(to, item);
            self.start_job("moving the preset", move |s| {
                s.reorder_presets(setlist, &order)?;
                Ok(JobResult::Presets(s.preset_list(setlist)?))
            });
        }
        if let Some(slot) = copy {
            self.ed.preset_clip = Some(slot);
            ui.ctx().copy_text(CLIP_MARK.into());
        }
        if let Some(to) = paste {
            self.ed.paste_preset = self.ed.preset_clip.map(|from| (from, to));
        }
        if let Some((slot, name)) = export {
            self.export_slot = slot;
            self.export_file = default_export_path(slot, &name);
            self.tab = Tab::Library;
        }
    }

    pub(crate) fn browser_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Models");
        ui.add(egui::TextEdit::singleline(&mut self.ed.browser_search).hint_text("search"));
        ui.weak("Drag a model onto the chain");
        let style = self.style;
        let Some(c) = self.catalog_in_use() else {
            ui.weak("Load a catalog (Catalog screen) to browse the models.");
            return;
        };
        let search = self.ed.browser_search.to_lowercase();
        egui::ScrollArea::vertical().id_salt("browser").show(ui, |ui| {
            model_list(ui, c, style, &search, "browser");
        });
    }

    pub(crate) fn editor_screen(&mut self, ui: &mut egui::Ui) {
        if self.device.is_none() {
            ui.heading("Not connected");
            ui.label("Close HX Edit, connect the HX Stomp over USB and press Connect.");
            return;
        }
        self.preset_header(ui);
        ui.add_space(6.0);
        let acts = self.chain_grid(ui);
        ui.add_space(6.0);
        let mut acts = acts;
        acts.extend(self.block_panel(ui));
        for a in acts {
            self.act(a, ui.ctx());
        }
    }

    fn preset_header(&mut self, ui: &mut egui::Ui) {
        let Some(d) = &self.device else { return };
        let (setlist, slot, edited, current_snapshot) = (d.current.setlist, d.current.slot as usize, d.edited, d.snapshot);
        let names = d.snapshot_names.clone();
        let mut save = false;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.save_name).font(egui::TextStyle::Heading).desired_width(220.0).char_limit(stomp::MAX_PRESET_NAME));
            ui.weak(format!("{:03}{}", slot, if edited { "  edited" } else { "" }));
            save = ui.button("Save").on_hover_text("Ctrl+S: stores the preset in its slot").clicked();
            if ui.button("Refresh").on_hover_text("Reads the edit buffer again (briefly interrupts the sound in snapshots 4-6)").clicked() {
                self.start_job("reading the edit buffer", |s| Ok(JobResult::EditBuffer(s.edit_buffer()?)));
            }
            if ui.button("Footswitches...").clicked() {
                self.ed.footswitch_window = true;
            }
        });
        if save {
            self.save_current(setlist, slot);
        }
        let mut job: Option<(bool, u64, String, u64)> = None;
        ui.horizontal(|ui| {
            ui.label("Snapshots");
            for n in 1..=6u8 {
                let name = names.get(usize::from(n) - 1).cloned().unwrap_or_default();
                let text = match name.is_empty() {
                    true => n.to_string(),
                    false => format!("{n} {name}"),
                };
                let on = current_snapshot == Some(u64::from(n));
                let r = ui.selectable_label(on, text).on_hover_text("Click: recall (MIDI CC 69). Right-click: name and color");
                if r.clicked() {
                    let p = self.handle.recall_snapshot(n);
                    self.op(format!("snapshot {n}"), p);
                }
                r.context_menu(|ui| match on {
                    false => {
                        ui.label("Recall this snapshot first: the unit names and colors the active one.");
                    }
                    true => {
                        let index = u64::from(n - 1) % 3;
                        ui.add(egui::TextEdit::singleline(&mut self.snapshot_name).hint_text("name").char_limit(stomp::MAX_SNAPSHOT_NAME));
                        if ui.button("Rename").clicked() {
                            job = Some((true, index, self.snapshot_name.clone(), 0));
                            ui.close_menu();
                        }
                        ui.add(egui::DragValue::new(&mut self.snapshot_color).range(0..=15).prefix("color "));
                        if ui.button("Set color").on_hover_text("The number the unit stores; the palette is not mapped yet").clicked() {
                            job = Some((false, index, String::new(), self.snapshot_color));
                            ui.close_menu();
                        }
                    }
                });
            }
            ui.separator();
            self.tempo_controls(ui);
            ui.separator();
            self.dsp_meter(ui);
        });
        match job {
            Some((true, index, name, _)) => self.edit_job("renaming the snapshot", move |s| s.rename_snapshot(index, &name)),
            Some((false, index, _, color)) => self.edit_job("coloring the snapshot", move |s| s.set_snapshot_color(index, color)),
            None => {}
        }
    }

    fn save_current(&mut self, setlist: u64, slot: usize) {
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

    fn tempo_controls(&mut self, ui: &mut egui::Ui) {
        let Some(d) = self.device.as_mut() else { return };
        let mut writes = Vec::new();
        ui.label("Tempo");
        if let Some(t) = d.preset_tempo.as_mut() {
            if ui.add(egui::DragValue::new(t).range(TEMPO_RANGE).speed(0.1).suffix(" BPM")).on_hover_text("This preset's tempo").changed() {
                writes.push((SETTING_PRESET_TEMPO, *t));
            }
        }
        if ui.button("Tap").clicked() {
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
        ui.weak("global");
        if let Some(t) = d.global_bpm.as_mut() {
            if ui.add(egui::DragValue::new(t).range(TEMPO_RANGE).speed(0.1).suffix(" BPM")).on_hover_text("The global tempo").changed() {
                writes.push((SETTING_GLOBAL_BPM, *t));
            }
        }
        if writes.iter().any(|(id, _)| *id == SETTING_PRESET_TEMPO) {
            d.edited = true;
        }
        for (id, bpm) in writes {
            let p = self.handle.set_setting(id, Value::F32(bpm));
            self.op(format!("tempo {bpm:.1} BPM"), p);
        }
    }

    fn dsp_meter(&self, ui: &mut egui::Ui) {
        let (Some(d), Some(c)) = (&self.device, self.catalog_in_use()) else { return };
        let ids: Vec<u64> = d.blocks.iter().flat_map(|b| std::iter::once(b.preset_id).chain(b.cab)).collect();
        let (total, _) = c.dsp_total(&ids);
        let bar = egui::ProgressBar::new(total as f32 / 10000.0).desired_width(150.0).text(format!("DSP {:.0}%", total as f32 / 100.0));
        ui.add(bar).on_hover_text("Declared costs: 15% fixed plus each block (HXtra's research); the unit refuses more than 90%");
    }

    fn chain_grid(&mut self, ui: &mut egui::Ui) -> Vec<Act> {
        let mut acts = Vec::new();
        let style = self.style;
        let c = self.catalog_in_use();
        let Some(d) = &self.device else { return acts };
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, 2.0 * TILE + ROW_GAP + 34.0), Sense::hover());
        let slot_w = ((width - 2.0 * IO_WIDTH - 24.0) / chain::ROW_LEN as f32).clamp(TILE + 6.0, 130.0);
        let x0 = rect.left() + IO_WIDTH + 12.0;
        let rows_y = [rect.top() + 6.0, rect.top() + 6.0 + TILE + ROW_GAP];
        let center = |row: usize, idx: usize| pos2(x0 + slot_w * (idx as f32 + 0.5), rows_y[row] + TILE / 2.0);
        let painter = ui.painter_at(rect);
        let line = Stroke::new(2.0_f32, theme::LINE);
        let parallel = d.blocks.iter().any(|b| ROW_B.contains(&b.position));
        let (in_x, out_x) = (x0 - IO_WIDTH / 2.0 - 6.0, x0 + slot_w * chain::ROW_LEN as f32 + IO_WIDTH / 2.0 + 6.0);
        painter.line_segment([pos2(in_x, center(0, 0).y), pos2(out_x, center(0, 0).y)], line);
        if parallel {
            let (split_x, join_x) = (x0 - 4.0, x0 + slot_w * chain::ROW_LEN as f32 + 4.0);
            painter.line_segment([pos2(split_x, center(0, 0).y), pos2(split_x, center(1, 0).y)], line);
            painter.line_segment([pos2(split_x, center(1, 0).y), pos2(join_x, center(1, 0).y)], line);
            painter.line_segment([pos2(join_x, center(1, 0).y), pos2(join_x, center(0, 0).y)], line);
        }
        for (x, label) in [(in_x, "In"), (out_x, "Out")] {
            let r = Rect::from_center_size(pos2(x, center(0, 0).y), vec2(IO_WIDTH, TILE));
            painter.rect_filled(r, 10.0, theme::TILE);
            painter.text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(14.0), theme::TEXT);
        }
        for row in 0..2 {
            for idx in 0..chain::ROW_LEN {
                let position = chain::position(row, idx);
                let tile = Rect::from_center_size(center(row, idx), vec2(TILE, TILE));
                let id = ui.id().with(("slot", position));
                let r = ui.interact(tile, id, Sense::click_and_drag());
                if let Some(drag) = r.dnd_release_payload::<Drag>() {
                    acts.push(Act::Drop((*drag).clone(), position));
                }
                let block = d.blocks.iter().find(|b| b.position == position);
                let selected = self.ed.selected == Some(position);
                let hovering_drop = r.dnd_hover_payload::<Drag>().is_some_and(|p| !matches!(*p, Drag::Preset(_)));
                match block {
                    Some(b) => {
                        let category = category_of(c, b.preset_id);
                        let color = theme::category_color(&category);
                        let color = if b.enabled { color } else { color.linear_multiply(0.35) };
                        painter.rect_filled(tile, 10.0, theme::TILE);
                        let border = match (selected, hovering_drop) {
                            (_, true) => Stroke::new(3.0_f32, theme::SELECTED),
                            (true, false) => Stroke::new(3.0_f32, theme::SELECTED),
                            (false, false) => Stroke::new(2.0_f32, color),
                        };
                        painter.rect_stroke(tile, 10.0, border);
                        icons::category(&painter, tile, &category, color);
                        let name: String = model_name(c, style, b.preset_id).chars().take(16).collect();
                        painter.text(pos2(tile.center().x, tile.bottom() + 3.0), Align2::CENTER_TOP, name, FontId::proportional(11.0), if b.enabled { theme::TEXT } else { theme::MUTED });
                        r.dnd_set_drag_payload(Drag::Block(position));
                        let power_at = tile.right_top() + vec2(-11.0, 11.0);
                        let pr = ui.interact(Rect::from_center_size(power_at, vec2(18.0, 18.0)), id.with("power"), Sense::click());
                        icons::power(&painter, power_at, 6.0, b.enabled, theme::ACCENT);
                        if pr.on_hover_text("On/off").clicked() {
                            acts.push(Act::Toggle(position));
                        } else if r.clicked() {
                            acts.push(Act::Select(position));
                        }
                    }
                    None => {
                        let stroke = match (selected, hovering_drop, r.hovered()) {
                            (_, true, _) | (true, _, _) => Stroke::new(2.0_f32, theme::SELECTED),
                            (false, false, true) => Stroke::new(1.5_f32, theme::MUTED),
                            (false, false, false) => Stroke::new(1.0_f32, egui::Color32::from_gray(52)),
                        };
                        painter.rect_stroke(tile.shrink(8.0), 8.0, stroke);
                        if r.hovered() {
                            painter.text(tile.center(), Align2::CENTER_CENTER, "+", FontId::proportional(22.0), theme::MUTED);
                        }
                        if r.on_hover_text("Click to add a block, or drop one here").clicked() {
                            acts.push(Act::Pick(position));
                        }
                    }
                }
            }
        }
        acts
    }

    fn block_panel(&mut self, ui: &mut egui::Ui) -> Vec<Act> {
        let mut acts = Vec::new();
        let style = self.style;
        let hidden = self.show_hidden;
        let catalog = self.catalog.as_ref().filter(|_| !self.catalog_ignored);
        let irs = &self.irs;
        let Some(position) = self.ed.selected else {
            ui.weak("Select a block to edit it; drag models from the list on the left onto the chain.");
            return acts;
        };
        let Some(d) = self.device.as_mut() else { return acts };
        let Some(b) = d.blocks.iter_mut().find(|b| b.position == position) else {
            ui.weak(format!("{} is empty: click it to add a block, or drop one there.", position_name(position)));
            return acts;
        };
        let model = catalog.and_then(|c| c.model(b.preset_id));
        let category = category_of(catalog, b.preset_id);
        egui::Frame::none().fill(theme::TILE).rounding(10.0).inner_margin(10.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                icons::category(ui.painter(), r, &category, theme::category_color(&category));
                let cab = b.cab.map(|id| format!(" + {}", model_name(catalog, style, id))).unwrap_or_default();
                ui.weak(format!("{}  {}", position_name(position), if category.is_empty() { "block" } else { &category }));
                ui.heading(format!("{}{cab}", model_name(catalog, style, b.preset_id)));
                if let Some(cost) = model.and_then(|m| m.dsp_cost) {
                    ui.weak(format!("DSP {:.1}%", cost as f32 / 100.0));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button("...", |ui| block_menu(ui, b, catalog, model, style, &mut self.show_hidden, &mut acts));
                });
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                for i in 0..b.params.len() {
                    let p = model.and_then(|m| m.param(i));
                    if p.is_some_and(|p| !(p.visible || hidden)) {
                        continue;
                    }
                    ui.allocate_ui(CELL, |ui| {
                        ui.vertical_centered(|ui| {
                            let title = p.map_or(format!("[{i}]"), |p| p.title.to_uppercase());
                            let t = ui.add(egui::Label::new(egui::RichText::new(title).small().strong()).sense(Sense::click()));
                            t.on_hover_text("Right-click: controller").context_menu(|ui| controller_menu(ui, position, i, &b.params[i], &mut acts));
                            let id = ui.id().with(("param", position, i));
                            let old = b.params[i].clone();
                            let touch = param_control(ui, id, p, &mut b.params[i], irs);
                            if touch.changed {
                                acts.push(Act::Param { position, index: i, value: b.params[i].clone(), old, began: touch.began });
                            }
                            if p.and_then(|p| p.hxtra_added).unwrap_or(false) {
                                ui.weak("HXtra");
                            }
                        });
                    });
                }
            });
        });
        acts
    }

    fn act(&mut self, a: Act, ctx: &egui::Context) {
        let Some(d) = &self.device else { return };
        let blocks = d.blocks.clone();
        match a {
            Act::Select(p) => {
                self.ed.selected = Some(p);
                self.ed.focus = Focus::Chain;
            }
            Act::Pick(p) => {
                self.ed.selected = Some(p);
                self.ed.focus = Focus::Chain;
                self.ed.picker = Some(p);
            }
            Act::Drop(drag, to) => self.drop_on(drag, to),
            Act::Toggle(p) => {
                self.push_history();
                if let Some(b) = self.device.as_mut().and_then(|d| d.blocks.iter_mut().find(|b| b.position == p)) {
                    b.enabled = !b.enabled;
                    let pending = self.handle.set_block_enabled(p as u64, b.enabled);
                    self.op(format!("block {} on/off", position_name(p)), pending);
                }
                self.mark_edited();
            }
            Act::Param { position, index, value, old, began } => {
                if began {
                    let mut before = blocks;
                    if let Some(v) = before.iter_mut().find(|b| b.position == position).and_then(|b| b.params.get_mut(index)) {
                        *v = old;
                    }
                    self.ed.history.push(before);
                    if self.ed.history.len() > HISTORY {
                        self.ed.history.remove(0);
                    }
                    self.ed.redo.clear();
                }
                let pending = self.handle.set_parameter(position as u64, 0, index as u64, value);
                self.op(format!("{} parameter {index}", position_name(position)), pending);
                self.mark_edited();
            }
            Act::Target(target, label) => {
                self.push_history();
                self.reach(target, label);
            }
            Act::Favorite(p, name) => self.new_favorite = Some((p as u64, name)),
            Act::Bypass(p, fs) => self.edit_job("assigning the footswitch", move |s| s.assign_bypass(p as u64, fs)),
            Act::FollowSnapshots(p, follows) => self.edit_job("setting the snapshot bypass", move |s| s.set_snapshot_bypass(p as u64, follows)),
            Act::Controller(p, i, source) => self.edit_job("assigning the controller", move |s| s.assign_controller(p as u64, i as u64, source)),
            Act::Range(p, i, max, v) => self.edit_job("setting the controller range", move |s| s.set_controller_range(p as u64, i as u64, max, v)),
            Act::Remove(p) => self.remove_block(p),
            Act::Copy => self.copy(ctx),
            Act::Paste => self.paste(),
        }
    }

    fn mark_edited(&mut self) {
        if let Some(d) = self.device.as_mut() {
            d.edited = true;
        }
    }

    fn remove_block(&mut self, p: usize) {
        let has = self.device.as_ref().is_some_and(|d| d.blocks.iter().any(|b| b.position == p));
        if has {
            self.push_history();
            self.edit_job("removing the block", move |s| s.remove_block(p as u64));
        }
    }

    /// Insert with push: a block moved, or a model added, onto `to`.
    fn drop_on(&mut self, drag: Drag, to: usize) {
        let Some(d) = &self.device else { return };
        let layout = layout_of(&d.blocks);
        match drag {
            Drag::Preset(_) => {}
            Drag::Block(from) if from == to => {}
            Drag::Block(from) => match chain::insert(&layout, from as u64, Some(from), to) {
                Some(new) => {
                    let moves = chain::plan_moves(&layout, &new);
                    self.ed.selected = new.iter().find(|(_, id)| **id == from as u64).map(|(p, _)| *p);
                    self.push_history();
                    self.edit_job("moving the block", move |s| moves.iter().try_for_each(|(a, b)| s.move_block(*a as u64, *b as u64)));
                }
                None => self.note(format!("no room to insert there: row {} is full", if ROW_A.contains(&to) { "A" } else { "B" })),
            },
            Drag::Model { preset_id, cab, kind, name } => match chain::insert(&layout, NEW_BLOCK, None, to) {
                Some(new) => {
                    let Some(target) = new.iter().find(|(_, id)| **id == NEW_BLOCK).map(|(p, _)| *p) else { return };
                    let existing: Layout = new.into_iter().filter(|(_, id)| *id != NEW_BLOCK).collect();
                    let moves = chain::plan_moves(&layout, &existing);
                    self.ed.selected = Some(target);
                    self.push_history();
                    self.edit_job(&format!("adding {name}"), move |s| {
                        moves.iter().try_for_each(|(a, b)| s.move_block(*a as u64, *b as u64))?;
                        s.add_block(target as u64, preset_id, cab, kind)
                    });
                }
                None => self.note("no room to insert there: the row is full".to_owned()),
            },
        }
    }

    fn copy(&mut self, ctx: &egui::Context) {
        match self.ed.focus {
            Focus::Chain => {
                let block = self.device.as_ref().and_then(|d| d.blocks.iter().find(|b| Some(b.position) == self.ed.selected).cloned());
                if let Some(b) = block {
                    self.note(format!("copied {}", position_name(b.position)));
                    self.ed.clip = Some(b);
                    ctx.copy_text(CLIP_MARK.into());
                }
            }
            Focus::Presets => {
                if let Some(slot) = self.device.as_ref().map(|d| d.current.slot as usize) {
                    self.note(format!("copied preset {slot:03}"));
                    self.ed.preset_clip = Some(slot);
                    ctx.copy_text(CLIP_MARK.into());
                }
            }
        }
    }

    fn paste(&mut self) {
        match self.ed.focus {
            Focus::Chain => {
                let (Some(clip), Some(p), Some(d)) = (self.ed.clip.clone(), self.ed.selected, &self.device) else { return };
                let mut target: Vec<Block> = d.blocks.iter().filter(|b| b.position != p).cloned().collect();
                target.push(Block { position: p, ..clip });
                self.push_history();
                self.reach(target, "pasting the block");
            }
            Focus::Presets => {
                let (Some(from), Some(d)) = (self.ed.preset_clip, &self.device) else { return };
                self.ed.paste_preset = Some((from, d.current.slot as usize));
            }
        }
    }

    fn undo(&mut self) {
        let (Some(target), Some(d)) = (self.ed.history.pop(), &self.device) else { return };
        self.ed.redo.push(d.blocks.clone());
        self.reach(target, "undoing");
    }

    fn redo(&mut self) {
        let (Some(target), Some(d)) = (self.ed.redo.pop(), &self.device) else { return };
        self.ed.history.push(d.blocks.clone());
        self.reach(target, "redoing");
    }

    pub(crate) fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.tab != Tab::Edit || self.device.is_none() || ctx.wants_keyboard_input() {
            return;
        }
        let (copy, paste, undo, redo, save, delete) = ctx.input_mut(|i| {
            let copy = i.events.iter().any(|e| matches!(e, egui::Event::Copy));
            let paste = i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)));
            let redo = i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y);
            let undo = i.consume_key(Modifiers::COMMAND, Key::Z);
            let save = i.consume_key(Modifiers::COMMAND, Key::S);
            let delete = i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace);
            (copy, paste, undo, redo, save, delete)
        });
        if copy {
            self.copy(ctx);
        }
        if paste {
            self.paste();
        }
        if redo {
            self.redo();
        }
        if undo {
            self.undo();
        }
        if save {
            if let Some((setlist, slot)) = self.device.as_ref().map(|d| (d.current.setlist, d.current.slot as usize)) {
                self.save_current(setlist, slot);
            }
        }
        if delete && self.ed.focus == Focus::Chain {
            if let Some(p) = self.ed.selected {
                self.remove_block(p);
            }
        }
    }

    pub(crate) fn editor_windows(&mut self, ctx: &egui::Context) {
        self.picker_window(ctx);
        self.paste_preset_window(ctx);
        self.favorite_window(ctx);
        self.footswitch_window(ctx);
    }

    fn picker_window(&mut self, ctx: &egui::Context) {
        let Some(position) = self.ed.picker else { return };
        let mut open = true;
        let mut chosen = None;
        let style = self.style;
        egui::Window::new(format!("Add a block at {}", position_name(position))).open(&mut open).default_size([320.0, 420.0]).collapsible(false).show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.ed.picker_search).hint_text("search"));
            let Some(c) = self.catalog.as_ref().filter(|_| !self.catalog_ignored) else {
                ui.weak("Load a catalog to pick models.");
                return;
            };
            let search = self.ed.picker_search.to_lowercase();
            egui::ScrollArea::vertical().show(ui, |ui| {
                chosen = model_list(ui, c, style, &search, "picker");
            });
        });
        match (open, chosen) {
            (_, Some(drag)) => {
                self.ed.picker = None;
                self.drop_on(drag, position);
            }
            (false, None) => self.ed.picker = None,
            (true, None) => {}
        }
    }

    fn paste_preset_window(&mut self, ctx: &egui::Context) {
        let (Some((from, to)), Some(d)) = (self.ed.paste_preset, &self.device) else { return };
        let name = |s: usize| d.presets.get(s).map_or("?".to_owned(), |p| p.name.clone());
        let (from_name, to_name, setlist) = (name(from), name(to), d.current.setlist);
        let (mut paste, mut cancel) = (false, false);
        egui::Window::new("Paste preset").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(format!("Paste {from:03} \"{from_name}\" over {to:03} \"{to_name}\"?"));
            ui.label(format!("{to:03} \"{to_name}\" is replaced on the unit."));
            ui.horizontal(|ui| {
                paste = ui.button("Paste").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if paste {
            self.ed.paste_preset = None;
            self.start_job("pasting the preset", move |s| {
                let file = s.preset_file(setlist, from)?;
                s.batch_begin()?;
                let written = s.upload_preset(setlist, to, &from_name, ListFlags::default(), &file);
                let ended = s.batch_end();
                written?;
                ended?;
                Ok(JobResult::Presets(s.preset_list(setlist)?))
            });
        }
        if cancel {
            self.ed.paste_preset = None;
        }
    }

    fn favorite_window(&mut self, ctx: &egui::Context) {
        let Some((block, name)) = self.new_favorite.as_mut() else { return };
        let block = *block;
        let (mut save, mut cancel) = (false, false);
        egui::Window::new("Save as favorite").collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(format!("Block {} as a new favorite, named:", position_name(block as usize)));
            ui.add(egui::TextEdit::singleline(name).char_limit(stomp::MAX_PRESET_NAME));
            ui.horizontal(|ui| {
                save = ui.button("Save").clicked();
                cancel = ui.button("Cancel").clicked();
            });
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

    fn footswitch_window(&mut self, ctx: &egui::Context) {
        if !self.ed.footswitch_window {
            return;
        }
        let mut open = true;
        let mut job = None;
        egui::Window::new("Footswitch labels and colors").open(&mut open).collapsible(false).show(ctx, |ui| {
            egui::Grid::new("footswitches").show(ui, |ui| {
                for fs in 0..FOOTSWITCHES as usize {
                    ui.label(format!("FS{}", fs + 1));
                    ui.add(egui::TextEdit::singleline(&mut self.footswitch_labels[fs]).desired_width(140.0).char_limit(stomp::MAX_PRESET_NAME));
                    if ui.button("Set label").clicked() {
                        job = Some((fs as u64, Some(self.footswitch_labels[fs].clone()), None));
                    }
                    ui.add(egui::DragValue::new(&mut self.footswitch_colors[fs]).range(0..=15).prefix("color "));
                    if ui.button("Set color").clicked() {
                        job = Some((fs as u64, None, Some(self.footswitch_colors[fs])));
                    }
                    ui.end_row();
                }
            });
        });
        self.ed.footswitch_window = open;
        match job {
            Some((fs, Some(label), _)) => self.edit_job("labeling the footswitch", move |s| s.set_footswitch_label(fs, &label)),
            Some((fs, None, Some(color))) => self.edit_job("coloring the footswitch", move |s| s.set_footswitch_color(fs, color)),
            _ => {}
        }
    }
}

/// The model browser's folders; each model can be dragged, and a click returns it.
fn model_list(ui: &mut egui::Ui, c: &Catalog, style: NameStyle, search: &str, salt: &str) -> Option<Drag> {
    let mut clicked = None;
    for (fi, folder) in c.browser.iter().enumerate() {
        let items: Vec<(&catalog::FolderItem, String, String)> = folder
            .items
            .iter()
            .filter_map(|it| {
                let id = it.model?;
                let name = model_name(Some(c), style, id);
                let category = category_of(Some(c), id);
                (search.is_empty() || name.to_lowercase().contains(search)).then_some((it, name, category))
            })
            .collect();
        if items.is_empty() {
            continue;
        }
        let title = format!("{} / {}", folder.category.as_deref().unwrap_or("?"), folder.folder);
        egui::CollapsingHeader::new(title).id_salt((salt, fi)).open((!search.is_empty()).then_some(true)).show(ui, |ui| {
            for (it, name, category) in items {
                let r = ui
                    .horizontal(|ui| {
                        let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(dot.center(), 4.0, theme::category_color(&category));
                        ui.add(egui::Label::new(&name).selectable(false).sense(Sense::click_and_drag()))
                    })
                    .inner;
                let drag = Drag::Model { preset_id: it.model.unwrap_or_default(), cab: it.cab, kind: it.kind, name: name.clone() };
                r.dnd_set_drag_payload(drag.clone());
                if r.clicked() {
                    clicked = Some(drag);
                }
            }
        });
    }
    clicked
}

/// One parameter's control in the block panel.
fn param_control(ui: &mut egui::Ui, id: egui::Id, p: Option<&Param>, v: &mut Value, irs: &[IrSlot]) -> knob::Touch {
    let number = v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)).unwrap_or(0.0);
    match p {
        Some(p) if p.role.as_deref() == Some(ROLE_CAPTURE_SLOT) => capture_select(ui, id, v, number, irs),
        Some(p) => match (p.kind, &p.choices) {
            (ParamKind::Float, _) => {
                let mut x = number;
                let log = p.min > 0.0 && p.max / p.min >= 100.0;
                let text = p.format(v);
                let touch = knob::knob(ui, id, &mut x, (p.min, p.max), p.default, log, &text, |t| p.parse_display(t, number));
                if touch.changed {
                    *v = Value::F32(x as f32);
                }
                touch
            }
            (ParamKind::Bool, _) => {
                let mut on = v.as_bool().unwrap_or(number != 0.0);
                let touch = knob::switch(ui, &mut on);
                if touch.changed {
                    *v = Value::from(on);
                }
                touch
            }
            (ParamKind::List | ParamKind::Int, Some(choices)) => {
                let mut selected = number as i64;
                let current = choices.iter().find(|c| c.value == selected).map_or(selected.to_string(), |c| c.label.clone());
                egui::ComboBox::from_id_salt(id).width(CELL.x - 16.0).selected_text(current).show_ui(ui, |ui| {
                    for c in choices {
                        ui.selectable_value(&mut selected, c.value, &c.label);
                    }
                });
                let changed = selected != number as i64;
                if changed {
                    *v = Value::from(selected);
                }
                knob::Touch { changed, began: changed }
            }
            (ParamKind::List | ParamKind::Int, None) => {
                let mut n = number as i64;
                let changed = ui.add(egui::DragValue::new(&mut n).range(p.min as i64..=p.max as i64)).changed();
                if changed {
                    *v = Value::from(n);
                }
                knob::Touch { changed, began: changed }
            }
            (ParamKind::Data, _) => {
                ui.weak(restore::value_text(v));
                knob::Touch::default()
            }
        },
        None => {
            let changed = match v {
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
                    ui.weak(restore::value_text(other));
                    false
                }
            };
            knob::Touch { changed, began: changed }
        }
    }
}

/// The capture a NAM block plays: only the IR slots that hold captures.
fn capture_select(ui: &mut egui::Ui, id: egui::Id, v: &mut Value, number: f64, irs: &[IrSlot]) -> knob::Touch {
    let mut slot = number.round().clamp(0.0, IR_SLOTS as f64) as u64;
    let name = |s: u64| match irs.iter().find(|i| i.index + 1 == s) {
        Some(i) if i.capture => i.name.clone(),
        Some(i) => format!("{} (not a capture)", i.name),
        None => "(empty slot)".to_owned(),
    };
    egui::ComboBox::from_id_salt(id).width(CELL.x - 16.0).selected_text(format!("{slot:03} {}", name(slot))).show_ui(ui, |ui| {
        for ir in irs.iter().filter(|i| i.capture) {
            ui.selectable_value(&mut slot, ir.index + 1, format!("{:03} {}", ir.index + 1, ir.name));
        }
    });
    let changed = slot != number.round() as u64;
    if changed {
        *v = Value::F32(slot as f32);
    }
    knob::Touch { changed, began: changed }
}

/// The block panel's "..." menu.
fn block_menu(ui: &mut egui::Ui, b: &Block, c: Option<&Catalog>, model: Option<&Model>, style: NameStyle, hidden: &mut bool, acts: &mut Vec<Act>) {
    let p = b.position;
    if let (Some(c), Some(m)) = (c, model) {
        ui.menu_button("Switch to", |ui| {
            let mut targets: Vec<(String, u64, Option<u64>)> = c
                .variants(m)
                .into_iter()
                .map(|v| (format!("{} ({})", v.display_name(style), v.subcategory.as_deref().unwrap_or(v.shown_category())), v.preset_id, b.cab))
                .collect();
            match (b.cab, c.default_cab(m.preset_id)) {
                (Some(_), _) => targets.push(("without its cab".into(), m.preset_id, None)),
                (None, Some(cab)) => targets.push((format!("with its cab ({})", model_name(Some(c), style, cab)), m.preset_id, Some(cab))),
                (None, None) => {}
            }
            if targets.is_empty() {
                ui.weak("no variant of this model");
            }
            for (label, id, cab) in targets {
                if ui.button(label).on_hover_text("Keeps the values both models have").clicked() {
                    if let Some(to) = c.model(id) {
                        match catalog::carry_values(m, &b.params, to) {
                            Ok(params) => acts.push(Act::Target(vec![Block { preset_id: id, cab, kind: c.block_kind(id, cab).or(b.kind), params, ..b.clone() }], "switching the model")),
                            Err(e) => {
                                ui.label(format!("cannot switch: {e}"));
                            }
                        }
                    }
                    ui.close_menu();
                }
            }
        });
        if ui.button("Factory defaults").clicked() {
            acts.push(Act::Target(vec![Block { params: catalog::default_values(m, &b.params), ..b.clone() }], "restoring the defaults"));
            ui.close_menu();
        }
    }
    ui.separator();
    if ui.button("Copy  (Ctrl+C)").clicked() {
        acts.push(Act::Copy);
        ui.close_menu();
    }
    if ui.button("Paste over it  (Ctrl+V)").clicked() {
        acts.push(Act::Paste);
        ui.close_menu();
    }
    if ui.button("Save as favorite...").clicked() {
        let name: String = model.map_or("Favorite".to_owned(), |m| m.display_name(style).chars().filter(|c| (' '..='~').contains(c)).take(stomp::MAX_PRESET_NAME).collect());
        acts.push(Act::Favorite(p, name));
        ui.close_menu();
    }
    ui.separator();
    ui.menu_button("Bypass on footswitch", |ui| {
        for fs in 0..FOOTSWITCHES {
            if ui.button(format!("FS{}", fs + 1)).clicked() {
                acts.push(Act::Bypass(p, fs));
                ui.close_menu();
            }
        }
    });
    if ui.button("Ignore snapshots").on_hover_text("Snapshots stop turning this block on or off").clicked() {
        acts.push(Act::FollowSnapshots(p, false));
        ui.close_menu();
    }
    if ui.button("Follow snapshots").clicked() {
        acts.push(Act::FollowSnapshots(p, true));
        ui.close_menu();
    }
    ui.checkbox(hidden, "Show hidden parameters");
    ui.separator();
    if ui.button("Remove  (Delete)").clicked() {
        acts.push(Act::Remove(p));
        ui.close_menu();
    }
}

/// A parameter's controller menu (right-click on its name).
fn controller_menu(ui: &mut egui::Ui, position: usize, index: usize, value: &Value, acts: &mut Vec<Act>) {
    for (label, source) in [
        ("Assign to EXP 1", stomp::ControllerSource::Exp1),
        ("Assign to FS4", stomp::ControllerSource::Fs4),
        ("Assign to FS5", stomp::ControllerSource::Fs5),
        ("Remove controller", stomp::ControllerSource::None),
    ] {
        if ui.button(label).clicked() {
            acts.push(Act::Controller(position, index, source));
            ui.close_menu();
        }
    }
    ui.separator();
    if ui.button("Current value as min (heel)").clicked() {
        acts.push(Act::Range(position, index, false, value.clone()));
        ui.close_menu();
    }
    if ui.button("Current value as max (toe)").clicked() {
        acts.push(Act::Range(position, index, true, value.clone()));
        ui.close_menu();
    }
}

/// What is being dragged, drawn at the pointer.
pub(crate) fn paint_drag_ghost(ctx: &egui::Context) {
    let (Some(drag), Some(at)) = (egui::DragAndDrop::payload::<Drag>(ctx), ctx.pointer_interact_pos()) else { return };
    let text = match &*drag {
        Drag::Block(p) => format!("block {}", position_name(*p)),
        Drag::Model { name, .. } => name.clone(),
        Drag::Preset(s) => format!("preset {s:03}"),
    };
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("drag-ghost")));
    let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), theme::TEXT);
    let rect = Rect::from_min_size(at + vec2(14.0, 10.0), galley.size()).expand(6.0);
    painter.rect_filled(rect, 6.0, theme::TILE);
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, theme::SELECTED));
    painter.galley(rect.min + vec2(6.0, 6.0), galley, theme::TEXT);
}
