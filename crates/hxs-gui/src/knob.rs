//! The parameter controls of the block panel: a knob (drag up/down, mouse wheel, double-click
//! for the default, click the value to type it) and an on/off switch.

use std::f32::consts::PI;

use eframe::egui::{self, Color32, Id, Pos2, Sense, Shape, Stroke, pos2, vec2};

use crate::theme;

const DIAL: f32 = 48.0;
const SWEEP: f32 = 1.5 * PI;
const DRAG_PIXELS: f32 = 180.0;
const FINE: f32 = 6.0;
const WHEEL_STEP: f64 = 0.01;

/// What the user did to a control this frame.
#[derive(Default)]
pub struct Touch {
    /// The value changed.
    pub changed: bool,
    /// A new gesture began (a drag, a wheel turn, a double-click, a typed value): one undo step.
    pub began: bool,
}

/// A knob over `min..=max` (logarithmic when `log`), showing `text` under it.
#[allow(clippy::too_many_arguments)]
pub fn knob(
    ui: &mut egui::Ui,
    id: Id,
    value: &mut f64,
    (min, max): (f64, f64),
    default: f64,
    log: bool,
    text: &str,
    parse: impl Fn(&str) -> Option<f64>,
) -> Touch {
    let to_t = |v: f64| match log {
        true => ((v.max(min) / min).ln() / (max / min).ln()).clamp(0.0, 1.0),
        false => ((v - min) / (max - min)).clamp(0.0, 1.0),
    };
    let from_t = |t: f64| match log {
        true => min * (max / min).powf(t),
        false => min + t * (max - min),
    };
    let mut touch = Touch::default();
    let (rect, r) = ui.allocate_exact_size(vec2(DIAL, DIAL), Sense::click_and_drag());
    if r.drag_started() {
        touch.began = true;
    }
    if r.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { FINE } else { 1.0 };
        let t = to_t(*value) + f64::from(-r.drag_delta().y / (DRAG_PIXELS * fine));
        *value = from_t(t.clamp(0.0, 1.0));
        touch.changed = true;
    }
    if r.hovered() {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll != 0.0 {
            ui.ctx().input_mut(|i| {
                i.raw_scroll_delta = egui::Vec2::ZERO;
                i.smooth_scroll_delta = egui::Vec2::ZERO;
            });
            *value = from_t((to_t(*value) + WHEEL_STEP * f64::from(scroll.signum())).clamp(0.0, 1.0));
            touch.changed = true;
            touch.began = true;
        }
    }
    if r.double_clicked() {
        *value = default;
        touch.changed = true;
        touch.began = true;
    }
    paint_dial(ui, rect.center(), to_t(*value) as f32, r.hovered() || r.dragged());
    r.on_hover_text("Drag up/down (Shift: fine), wheel, double-click: default, click the value to type");
    let editing_id = id.with("typing");
    let mut typing: Option<String> = ui.data(|d| d.get_temp(editing_id));
    match typing.as_mut() {
        Some(buf) => {
            let edit = ui.add(egui::TextEdit::singleline(buf).desired_width(DIAL + 24.0).horizontal_align(egui::Align::Center));
            edit.request_focus();
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
            match (enter, escape || edit.lost_focus() && !enter) {
                (true, _) => {
                    if let Some(v) = parse(buf) {
                        *value = v;
                        touch.changed = true;
                        touch.began = true;
                    }
                    ui.data_mut(|d| d.remove::<String>(editing_id));
                }
                (false, true) => ui.data_mut(|d| d.remove::<String>(editing_id)),
                (false, false) => ui.data_mut(|d| d.insert_temp(editing_id, buf.clone())),
            }
        }
        None => {
            let label = ui.add(egui::Label::new(egui::RichText::new(text).small().color(theme::TEXT)).sense(Sense::click()));
            if label.clicked() {
                ui.data_mut(|d| d.insert_temp(editing_id, text.split_whitespace().next().unwrap_or("").to_owned()));
            }
        }
    }
    touch
}

/// A knob as on hardware: the value ring around it (dim track, warm lit arc with a soft glow),
/// a dark knurled skirt with a pointer notch, and a satin cap lit from the top left.
fn paint_dial(ui: &egui::Ui, c: Pos2, t: f32, hot: bool) {
    let p = ui.painter();
    let radius = DIAL * 0.38;
    let start = 0.75 * PI;
    let ring = radius + 5.0;
    let arc = |from: f32, to: f32, r: f32| -> Vec<Pos2> { (0..=48).map(|i| from + (to - from) * i as f32 / 48.0).map(|a| c + vec2(a.cos(), a.sin()) * r).collect() };
    p.add(Shape::line(arc(start, start + SWEEP, ring), Stroke::new(2.5_f32, Color32::from_gray(38))));
    p.add(Shape::line(arc(start, start + SWEEP * t, ring), Stroke::new(7.0_f32, theme::ACCENT.linear_multiply(0.12))));
    p.add(Shape::line(arc(start, start + SWEEP * t, ring), Stroke::new(2.5_f32, theme::ACCENT)));
    p.circle_filled(c + vec2(0.0, 1.5), radius + 0.5, Color32::from_black_alpha(90));
    theme::radial(p, c, radius, Color32::from_gray(if hot { 74 } else { 62 }), Color32::from_gray(16));
    for k in 0..40 {
        let a = std::f32::consts::TAU * k as f32 / 40.0;
        let d = vec2(a.cos(), a.sin());
        let lit = ((a - 1.25 * PI).cos() * 0.5 + 0.5) * 50.0;
        p.line_segment([c + d * radius * 0.78, c + d * radius * 0.98], Stroke::new(1.0_f32, Color32::from_gray(20 + lit as u8)));
    }
    theme::radial(p, c, radius * 0.66, Color32::from_gray(if hot { 236 } else { 222 }), Color32::from_gray(96));
    p.circle_stroke(c, radius * 0.66, Stroke::new(0.8_f32, Color32::from_black_alpha(90)));
    let a = start + SWEEP * t;
    let dir = vec2(a.cos(), a.sin());
    p.line_segment([c + dir * radius * 0.12, c + dir * radius * 0.6], Stroke::new(2.2_f32, Color32::from_gray(40)));
    p.line_segment([c + dir * radius * 0.7, c + dir * radius * 0.98], Stroke::new(2.0_f32, theme::TEXT));
}

/// An on/off switch; `true` when clicked.
pub fn switch(ui: &mut egui::Ui, on: &mut bool) -> Touch {
    let (rect, r) = ui.allocate_exact_size(vec2(44.0, 22.0), Sense::click());
    let p = ui.painter();
    let track = rect.shrink2(vec2(0.0, 2.0));
    theme::gradient(p, track, 9.0, Color32::from_gray(14), Color32::from_gray(30));
    if *on {
        theme::gradient(p, track.shrink(2.0), 7.0, theme::shade(theme::ACCENT, 0.15), theme::shade(theme::ACCENT, -0.3));
    }
    let knob_x = if *on { rect.right() - 10.0 } else { rect.left() + 10.0 };
    p.circle_filled(pos2(knob_x, rect.center().y + 1.2), 8.0, Color32::from_black_alpha(100));
    theme::radial(p, pos2(knob_x, rect.center().y), 8.0, Color32::from_gray(238), Color32::from_gray(120));
    ui.label(egui::RichText::new(if *on { "On" } else { "Off" }).small());
    match r.clicked() {
        true => {
            *on = !*on;
            Touch { changed: true, began: true }
        }
        false => Touch::default(),
    }
}
