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

/// A knob with some depth: a drop shadow, a ribbed skirt with ticks, a cap lit from the top
/// left, the value arc with a glow, and a pointer.
fn paint_dial(ui: &egui::Ui, c: Pos2, t: f32, hot: bool) {
    let p = ui.painter();
    let radius = DIAL * 0.40;
    let start = 0.75 * PI;
    let arc = |from: f32, to: f32, r: f32| -> Vec<Pos2> { (0..=40).map(|i| from + (to - from) * i as f32 / 40.0).map(|a| c + vec2(a.cos(), a.sin()) * r).collect() };
    for k in 0..=10 {
        let a = start + SWEEP * k as f32 / 10.0;
        let (inner, outer) = (radius + 5.5, radius + if k % 5 == 0 { 9.0 } else { 7.5 });
        p.line_segment([c + vec2(a.cos(), a.sin()) * inner, c + vec2(a.cos(), a.sin()) * outer], Stroke::new(1.0_f32, Color32::from_gray(80)));
    }
    p.add(Shape::line(arc(start, start + SWEEP, radius + 3.5), Stroke::new(3.0_f32, Color32::from_gray(44))));
    p.add(Shape::line(arc(start, start + SWEEP * t, radius + 3.5), Stroke::new(6.0_f32, theme::ACCENT.linear_multiply(0.25))));
    p.add(Shape::line(arc(start, start + SWEEP * t, radius + 3.5), Stroke::new(3.0_f32, theme::ACCENT)));
    p.circle_filled(c + vec2(1.5, 3.0), radius, Color32::from_black_alpha(140));
    p.circle_filled(c, radius, Color32::from_gray(22));
    let lift = if hot { 12 } else { 0 };
    for k in 0..14 {
        let f = k as f32 / 13.0;
        let rad = radius * (0.96 - 0.62 * f);
        let shade = 38 + (52.0 * f) as u8 + lift;
        p.circle_filled(c - vec2(radius * 0.18 * f, radius * 0.22 * f), rad, Color32::from_gray(shade));
    }
    p.circle_stroke(c, radius, Stroke::new(1.0_f32, Color32::from_gray(70)));
    let a = start + SWEEP * t;
    let dir = vec2(a.cos(), a.sin());
    p.line_segment([c + dir * radius * 0.2, c + dir * radius * 0.88], Stroke::new(3.0_f32, Color32::from_black_alpha(120)));
    p.line_segment([c + dir * radius * 0.2, c + dir * radius * 0.86], Stroke::new(2.0_f32, theme::TEXT));
    p.circle_filled(c + dir * radius * 0.86, 1.8, theme::TEXT);
}

/// An on/off switch; `true` when clicked.
pub fn switch(ui: &mut egui::Ui, on: &mut bool) -> Touch {
    let (rect, r) = ui.allocate_exact_size(vec2(44.0, 22.0), Sense::click());
    let p = ui.painter();
    let fill = if *on { theme::ACCENT } else { Color32::from_gray(48) };
    p.rect_filled(rect.translate(vec2(0.0, 2.0)), 11.0, Color32::from_black_alpha(120));
    p.rect_filled(rect, 11.0, fill);
    p.rect_filled(rect.shrink2(vec2(3.0, 0.0)).with_max_y(rect.center().y), 8.0, Color32::from_white_alpha(14));
    let knob_x = if *on { rect.right() - 11.0 } else { rect.left() + 11.0 };
    p.circle_filled(pos2(knob_x, rect.center().y + 1.5), 8.0, Color32::from_black_alpha(120));
    p.circle_filled(pos2(knob_x, rect.center().y), 8.0, Color32::from_gray(210));
    p.circle_filled(pos2(knob_x - 2.0, rect.center().y - 2.0), 4.0, Color32::from_gray(245));
    ui.label(egui::RichText::new(if *on { "On" } else { "Off" }).small());
    match r.clicked() {
        true => {
            *on = !*on;
            Touch { changed: true, began: true }
        }
        false => Touch::default(),
    }
}
