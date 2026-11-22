//! The look: a dark "premium plugin" theme (soft gradients, thin lit edges, warm accent), the
//! color of each block category, and the gradient and glow helpers the drawings use.

use eframe::egui::{self, Color32, Mesh, Painter, Pos2, Rect, Rounding, Shape, Stroke, pos2, vec2};

pub const BG: Color32 = Color32::from_rgb(17, 18, 20);
pub const PANEL: Color32 = Color32::from_rgb(25, 26, 29);
pub const EDGE: Color32 = Color32::from_rgb(52, 54, 60);
pub const TILE: Color32 = Color32::from_rgb(36, 37, 42);
pub const TEXT: Color32 = Color32::from_rgb(230, 230, 234);
pub const MUTED: Color32 = Color32::from_rgb(138, 140, 148);
pub const ACCENT: Color32 = Color32::from_rgb(236, 128, 52);
pub const SELECTED: Color32 = Color32::from_rgb(248, 196, 96);
pub const LINE: Color32 = Color32::from_rgb(86, 88, 96);

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = BG;
    v.faint_bg_color = TILE;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT.linear_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0_f32, SELECTED);
    v.hyperlink_color = SELECTED;
    v.window_stroke = Stroke::new(1.0_f32, EDGE);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, EDGE);
    v.widgets.inactive.weak_bg_fill = TILE;
    v.widgets.inactive.bg_fill = TILE;
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(48, 49, 55);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(58, 59, 66);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.rounding = Rounding::same(6.0);
    }
    v.window_rounding = Rounding::same(10.0);
    v.window_shadow = egui::epaint::Shadow { offset: vec2(0.0, 6.0), blur: 24.0, spread: 0.0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = v.window_shadow;
    ctx.set_visuals(v);
    ctx.style_mut(|s| s.spacing.item_spacing = egui::vec2(8.0, 6.0));
}

/// Our color for a model category (the catalog's `category`, or `capture` for a capture).
pub fn category_color(category: &str) -> Color32 {
    match category {
        "amp" => Color32::from_rgb(206, 70, 62),
        "preamp" => Color32::from_rgb(214, 108, 70),
        "cab" | "cab_ir_interp" => Color32::from_rgb(168, 120, 84),
        "ir" => Color32::from_rgb(188, 156, 110),
        "capture" => Color32::from_rgb(56, 186, 196),
        "distortion" => Color32::from_rgb(226, 140, 50),
        "dynamics" => Color32::from_rgb(214, 190, 70),
        "eq" => Color32::from_rgb(170, 176, 188),
        "modulation" => Color32::from_rgb(72, 128, 222),
        "delay" => Color32::from_rgb(70, 178, 112),
        "reverb" => Color32::from_rgb(144, 102, 214),
        "pitch" | "synth" => Color32::from_rgb(214, 100, 166),
        "filter" => Color32::from_rgb(62, 172, 154),
        "wah" => Color32::from_rgb(184, 84, 196),
        "volume" => Color32::from_rgb(140, 146, 160),
        "fxloop" | "send" | "return" => Color32::from_rgb(110, 130, 152),
        "looper" => Color32::from_rgb(220, 220, 226),
        _ => MUTED,
    }
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_premultiplied(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()), mix(a.a(), b.a()))
}

/// `c` lighter (`amount` > 0) or darker (< 0), toward white or black.
pub fn shade(c: Color32, amount: f32) -> Color32 {
    match amount >= 0.0 {
        true => lerp(c, Color32::WHITE, amount),
        false => lerp(c, Color32::BLACK, -amount),
    }
}

/// The outline of a rounded rectangle, clockwise from the top left.
fn outline(r: Rect, radius: f32) -> Vec<Pos2> {
    let radius = radius.min(r.width() / 2.0).min(r.height() / 2.0);
    let corners = [
        (pos2(r.left() + radius, r.top() + radius), std::f32::consts::PI),
        (pos2(r.right() - radius, r.top() + radius), 1.5 * std::f32::consts::PI),
        (pos2(r.right() - radius, r.bottom() - radius), 0.0),
        (pos2(r.left() + radius, r.bottom() - radius), 0.5 * std::f32::consts::PI),
    ];
    corners
        .iter()
        .flat_map(|(c, start)| (0..=6).map(move |i| start + 0.5 * std::f32::consts::PI * i as f32 / 6.0).map(move |a| *c + vec2(a.cos(), a.sin()) * radius))
        .collect()
}

/// A rounded rectangle filled with a vertical gradient.
pub fn gradient(p: &Painter, r: Rect, radius: f32, top: Color32, bottom: Color32) {
    let pts = outline(r, radius);
    let color = |y: f32| lerp(top, bottom, (y - r.top()) / r.height().max(1.0));
    let mut mesh = Mesh::default();
    mesh.colored_vertex(r.center(), color(r.center().y));
    for q in &pts {
        mesh.colored_vertex(*q, color(q.y));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(Shape::mesh(mesh));
}

/// A disc lit from the top left: `light` at the hot spot fading to `dark` at the rim.
pub fn radial(p: &Painter, c: Pos2, radius: f32, light: Color32, dark: Color32) {
    let hot = c - vec2(radius * 0.35, radius * 0.4);
    let mut mesh = Mesh::default();
    mesh.colored_vertex(hot, light);
    let n = 48u32;
    for i in 0..n {
        let a = std::f32::consts::TAU * i as f32 / n as f32;
        let q = c + vec2(a.cos(), a.sin()) * radius;
        let t = (q.distance(hot) / (radius * 1.6)).clamp(0.0, 1.0);
        mesh.colored_vertex(q, lerp(light, dark, 0.35 + 0.65 * t));
    }
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(Shape::mesh(mesh));
}

/// A soft glow around a rounded rectangle.
pub fn glow(p: &Painter, r: Rect, radius: f32, color: Color32, strength: f32) {
    for k in 1..=5 {
        let a = strength * (1.0 - k as f32 / 6.0) * 0.22;
        p.rect_stroke(r.expand(k as f32 * 1.4), radius + k as f32 * 1.4, Stroke::new(1.6_f32, color.linear_multiply(a)));
    }
}

/// A raised plate: gradient body, a thin lit top edge and a dark bottom edge.
pub fn plate(p: &Painter, r: Rect, radius: f32, base: Color32) {
    p.rect_filled(r.translate(vec2(0.0, 1.5)), radius, Color32::from_black_alpha(70));
    gradient(p, r, radius, shade(base, 0.10), shade(base, -0.25));
    p.rect_stroke(r, radius, Stroke::new(1.0_f32, shade(base, -0.45)));
    let lit = Rect::from_min_max(r.min + vec2(radius * 0.6, 0.6), pos2(r.right() - radius * 0.6, r.top() + 1.2));
    p.rect_filled(lit, 0.5, Color32::from_white_alpha(28));
}

/// A recessed area (wells, the chain's bed).
pub fn well(p: &Painter, r: Rect, radius: f32) {
    gradient(p, r, radius, Color32::from_rgb(14, 15, 17), Color32::from_rgb(22, 23, 26));
    p.rect_stroke(r, radius, Stroke::new(1.0_f32, Color32::from_rgb(10, 10, 12)));
    let lit = Rect::from_min_max(pos2(r.left() + radius, r.bottom() - 1.0), pos2(r.right() - radius, r.bottom()));
    p.rect_filled(lit, 0.5, Color32::from_white_alpha(16));
}
