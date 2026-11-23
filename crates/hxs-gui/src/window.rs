//! Our own window frame, in place of the system one: a title bar that moves the window (double
//! click maximizes) with the minimize, maximize and close buttons, edges that resize it and a
//! thin border.

use eframe::egui::{self, Align, Color32, CursorIcon, Id, LayerId, Layout, Order, PointerButton, Sense, Stroke, ViewportCommand, vec2};
use egui::viewport::ResizeDirection;

use crate::{icons, theme};

pub const TITLE_HEIGHT: f32 = 36.0;
const EDGE: f32 = 5.0;
const BUTTON_WIDTH: f32 = 46.0;
const CLOSE_HOVER: Color32 = Color32::from_rgb(196, 43, 28);

fn maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

/// The title bar, with `content` between the app's name and the window buttons.
pub fn title_bar(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    let bar = ui.interact(ui.max_rect(), Id::new("title_bar"), Sense::click_and_drag());
    let max = maximized(ui.ctx());
    if bar.double_clicked() {
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!max));
    } else if bar.drag_started_by(PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    ui.horizontal_centered(|ui| {
        ui.add_space(10.0);
        let (mark, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
        icons::ui(ui.painter(), mark.expand(4.0), "footswitch", theme::ACCENT);
        ui.label(egui::RichText::new("hxtra-editor").color(theme::MUTED));
        ui.add_space(8.0);
        content(ui);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if button(ui, "close", "Close").clicked() {
                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
            }
            let (kind, tip) = if max { ("restore", "Restore") } else { ("maximize", "Maximize") };
            if button(ui, kind, tip).clicked() {
                ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!max));
            }
            if button(ui, "minimize", "Minimize").clicked() {
                ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
            }
        });
    });
}

fn button(ui: &mut egui::Ui, kind: &str, tip: &str) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(vec2(BUTTON_WIDTH, TITLE_HEIGHT), Sense::click());
    let (fill, color) = match (r.hovered(), kind) {
        (true, "close") => (CLOSE_HOVER, Color32::WHITE),
        (true, _) => (theme::TILE, theme::TEXT),
        (false, _) => (Color32::TRANSPARENT, theme::MUTED),
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    icons::window(ui.painter(), rect, kind, color);
    r.on_hover_text(tip)
}

/// Resizing from the edges of the window and its border; nothing when maximized.
pub fn edges(ctx: &egui::Context) {
    if maximized(ctx) {
        return;
    }
    let screen = ctx.screen_rect();
    ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("window_border"))).rect_stroke(screen.shrink(0.5), 0.0, Stroke::new(1.0_f32, theme::EDGE));
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let near = (p.y - screen.top() < EDGE, screen.bottom() - p.y < EDGE, p.x - screen.left() < EDGE, screen.right() - p.x < EDGE);
    let direction = match near {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (true, _, _, true) => ResizeDirection::NorthEast,
        (_, true, true, _) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, _, _, _) => ResizeDirection::North,
        (_, true, _, _) => ResizeDirection::South,
        (_, _, true, _) => ResizeDirection::West,
        (_, _, _, true) => ResizeDirection::East,
        _ => return,
    };
    ctx.set_cursor_icon(match direction {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::West | ResizeDirection::East => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
    });
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
}
