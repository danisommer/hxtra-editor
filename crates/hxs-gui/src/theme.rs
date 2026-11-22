//! The dark theme and the color of each block category.

use eframe::egui::{self, Color32, Rounding, Stroke};

pub const BG: Color32 = Color32::from_rgb(18, 18, 20);
pub const PANEL: Color32 = Color32::from_rgb(26, 26, 29);
pub const TILE: Color32 = Color32::from_rgb(34, 34, 38);
pub const TEXT: Color32 = Color32::from_rgb(222, 222, 226);
pub const MUTED: Color32 = Color32::from_rgb(130, 130, 138);
pub const ACCENT: Color32 = Color32::from_rgb(214, 74, 60);
pub const SELECTED: Color32 = Color32::from_rgb(240, 196, 64);
pub const LINE: Color32 = Color32::from_rgb(92, 92, 100);

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = BG;
    v.faint_bg_color = TILE;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT.linear_multiply(0.6);
    v.selection.stroke = Stroke::new(1.0_f32, SELECTED);
    v.hyperlink_color = SELECTED;
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.rounding = Rounding::same(6.0);
    }
    v.window_rounding = Rounding::same(10.0);
    ctx.set_visuals(v);
    ctx.style_mut(|s| s.spacing.item_spacing = egui::vec2(8.0, 6.0));
}

/// Our color for a model category (the catalog's `category`, or `capture` for a capture).
pub fn category_color(category: &str) -> Color32 {
    match category {
        "amp" => Color32::from_rgb(220, 72, 64),
        "preamp" => Color32::from_rgb(236, 120, 76),
        "cab" | "cab_ir_interp" => Color32::from_rgb(176, 126, 88),
        "ir" => Color32::from_rgb(196, 160, 112),
        "capture" => Color32::from_rgb(64, 200, 210),
        "distortion" => Color32::from_rgb(240, 150, 48),
        "dynamics" => Color32::from_rgb(230, 206, 70),
        "eq" => Color32::from_rgb(200, 200, 208),
        "modulation" => Color32::from_rgb(80, 140, 240),
        "delay" => Color32::from_rgb(80, 196, 120),
        "reverb" => Color32::from_rgb(160, 110, 230),
        "pitch" | "synth" => Color32::from_rgb(232, 110, 180),
        "filter" => Color32::from_rgb(70, 190, 170),
        "wah" => Color32::from_rgb(200, 90, 210),
        "volume" => Color32::from_rgb(150, 156, 168),
        "fxloop" | "send" | "return" => Color32::from_rgb(120, 140, 160),
        "looper" => Color32::from_rgb(236, 236, 240),
        _ => MUTED,
    }
}
