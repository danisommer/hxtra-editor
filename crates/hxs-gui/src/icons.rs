//! Our own drawings: a miniature of the gear of each block category, the power symbol and the
//! screen and toolbar icons.

use std::f32::consts::{PI, TAU};

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::theme;

fn line(painter: &Painter, points: Vec<Pos2>, stroke: Stroke) {
    painter.add(Shape::line(points, stroke));
}

fn wave(painter: &Painter, r: Rect, cycles: f32, amp: f32, stroke: Stroke) {
    let pts: Vec<Pos2> = (0..=40)
        .map(|i| {
            let t = i as f32 / 40.0;
            pos2(r.left() + t * r.width(), r.center().y - (t * cycles * TAU).sin() * amp * r.height() / 2.0)
        })
        .collect();
    line(painter, pts, stroke);
}

/// A block as a miniature of its gear, in its category color: a pedal (knobs, LED, footswitch
/// and a mark of what it does), an amp head, a cabinet with its speaker, an expression pedal,
/// a looper, a jack plate. `on` lights the LED and keeps the colors; off they fade.
pub fn category(painter: &Painter, r: Rect, category: &str, color: Color32) {
    miniature(painter, r, category, color, true);
}

pub fn miniature(painter: &Painter, r: Rect, category: &str, color: Color32, on: bool) {
    let color = if on { color } else { theme::lerp(color, Color32::from_gray(70), 0.7) };
    let s = r.width().min(r.height());
    let c = r.center();
    let knob = |at: Pos2, rad: f32| {
        painter.circle_filled(at + vec2(0.0, rad * 0.25), rad, Color32::from_black_alpha(90));
        theme::radial(painter, at, rad, Color32::from_gray(205), Color32::from_gray(40));
    };
    let led = |at: Pos2, rad: f32| match on {
        true => {
            painter.circle_filled(at, rad * 2.2, theme::shade(color, 0.3).linear_multiply(0.18));
            painter.circle_filled(at, rad, theme::shade(color, 0.55));
            painter.circle_filled(at - vec2(rad * 0.3, rad * 0.3), rad * 0.4, Color32::from_white_alpha(200));
        }
        false => {
            painter.circle_filled(at, rad, Color32::from_gray(38));
        }
    };
    let ink = Stroke::new((s * 0.03).max(1.0), Color32::from_white_alpha(if on { 150 } else { 70 }));
    match category {
        "amp" | "preamp" => {
            let h = if category == "amp" { s * 0.62 } else { s * 0.5 };
            let body = Rect::from_center_size(c + vec2(0.0, s * 0.06), vec2(s * 0.94, h));
            painter.rect_filled(Rect::from_center_size(pos2(c.x, body.top() - s * 0.03), vec2(s * 0.3, s * 0.06)), s * 0.03, Color32::from_gray(30));
            theme::gradient(painter, body, s * 0.07, Color32::from_gray(48), Color32::from_gray(22));
            let grille = Rect::from_min_max(body.min + vec2(s * 0.05, s * 0.05), pos2(body.right() - s * 0.05, body.center().y + s * 0.02));
            theme::gradient(painter, grille, s * 0.03, theme::shade(color, -0.25), theme::shade(color, -0.55));
            for k in 1..6 {
                let y = grille.top() + grille.height() * k as f32 / 6.0;
                painter.line_segment([pos2(grille.left() + 2.0, y), pos2(grille.right() - 2.0, y)], Stroke::new(0.6_f32, Color32::from_black_alpha(60)));
            }
            let panel = Rect::from_min_max(pos2(body.left() + s * 0.05, grille.bottom() + s * 0.03), body.max - vec2(s * 0.05, s * 0.05));
            theme::gradient(painter, panel, s * 0.02, theme::shade(color, 0.35), theme::shade(color, 0.05));
            for k in 0..5 {
                knob(pos2(panel.left() + panel.width() * (0.14 + 0.18 * k as f32), panel.center().y), s * 0.035);
            }
            led(pos2(panel.right() - s * 0.05, panel.center().y), s * 0.025);
        }
        "cab" | "cab_ir_interp" => {
            let body = Rect::from_center_size(c, vec2(s * 0.86, s * 0.86));
            theme::gradient(painter, body, s * 0.08, Color32::from_gray(46), Color32::from_gray(20));
            let cloth = body.shrink(s * 0.07);
            theme::gradient(painter, cloth, s * 0.04, theme::shade(color, -0.2), theme::shade(color, -0.55));
            painter.circle_filled(cloth.center(), cloth.width() * 0.36, Color32::from_black_alpha(70));
            theme::radial(painter, cloth.center(), cloth.width() * 0.13, theme::shade(color, 0.2), theme::shade(color, -0.5));
        }
        "wah" | "volume" => {
            let body = Rect::from_center_size(c + vec2(0.0, s * 0.04), vec2(s * 0.62, s * 0.86));
            theme::gradient(painter, body, s * 0.1, Color32::from_gray(52), Color32::from_gray(22));
            let treadle = body.shrink2(vec2(s * 0.06, s * 0.08)).translate(vec2(0.0, -s * 0.02));
            theme::gradient(painter, treadle, s * 0.06, theme::shade(color, 0.1), theme::shade(color, -0.45));
            for k in 1..7 {
                let y = treadle.top() + treadle.height() * k as f32 / 7.0;
                painter.line_segment([pos2(treadle.left() + 3.0, y), pos2(treadle.right() - 3.0, y)], Stroke::new(0.8_f32, Color32::from_black_alpha(80)));
            }
            led(pos2(body.center().x, body.bottom() - s * 0.05), s * 0.025);
        }
        "looper" => {
            theme::radial(painter, c, s * 0.42, theme::shade(color, 0.1), theme::shade(color, -0.6));
            let pts: Vec<Pos2> = (0..=24).map(|i| 0.4 + 5.2 * i as f32 / 24.0).map(|a| c + vec2(a.cos(), a.sin()) * s * 0.22).collect();
            line(painter, pts, Stroke::new(s * 0.05, Color32::from_black_alpha(120)));
            led(c + vec2(s * 0.22, -s * 0.24), s * 0.03);
        }
        "fxloop" | "send" | "return" => {
            let body = Rect::from_center_size(c, vec2(s * 0.84, s * 0.5));
            theme::gradient(painter, body, s * 0.06, theme::shade(color, 0.15), theme::shade(color, -0.4));
            for x in [-0.2, 0.2] {
                theme::radial(painter, c + vec2(s * x, 0.0), s * 0.09, Color32::from_gray(170), Color32::from_gray(30));
                painter.circle_filled(c + vec2(s * x, 0.0), s * 0.035, Color32::from_gray(10));
            }
        }
        _ => {
            let body = Rect::from_center_size(c, vec2(s * 0.7, s * 0.9));
            painter.rect_filled(body.translate(vec2(0.0, s * 0.02)), s * 0.09, Color32::from_black_alpha(80));
            theme::gradient(painter, body, s * 0.09, theme::shade(color, 0.18), theme::shade(color, -0.42));
            painter.rect_stroke(body, s * 0.09, Stroke::new(1.0_f32, theme::shade(color, -0.6)));
            let knobs = if category == "eq" { 3 } else { 2 };
            for k in 0..knobs {
                let x = body.left() + body.width() * (k as f32 + 1.0) / (knobs as f32 + 1.0);
                knob(pos2(x, body.top() + s * 0.15), s * 0.065);
            }
            led(pos2(c.x, body.top() + s * 0.3), s * 0.028);
            let mark = Rect::from_center_size(pos2(c.x, body.top() + s * 0.5), vec2(s * 0.42, s * 0.18));
            match category {
                "modulation" => wave(painter, mark, 1.5, 0.8, ink),
                "delay" => {
                    for k in 0..4 {
                        painter.circle_filled(pos2(mark.left() + mark.width() * (0.12 + 0.25 * k as f32), mark.center().y), s * (0.04 - 0.007 * k as f32), ink.color);
                    }
                }
                "reverb" => {
                    for k in 1..4 {
                        let pts: Vec<Pos2> = (0..=12).map(|i| PI + PI * i as f32 / 12.0).map(|a| pos2(mark.center().x, mark.bottom()) + vec2(a.cos(), a.sin()) * mark.height() * 0.4 * k as f32).collect();
                        line(painter, pts, Stroke::new(ink.width, ink.color.linear_multiply(1.0 - 0.25 * k as f32)));
                    }
                }
                "distortion" => line(painter, vec![mark.left_center(), pos2(mark.left() + mark.width() * 0.3, mark.top()), pos2(mark.left() + mark.width() * 0.55, mark.bottom()), pos2(mark.left() + mark.width() * 0.8, mark.top()), mark.right_center()], ink),
                "dynamics" => line(painter, vec![mark.left_bottom(), mark.center(), mark.right_top() + vec2(0.0, mark.height() * 0.35)], ink),
                "pitch" | "synth" => line(painter, vec![pos2(mark.left(), mark.bottom()), pos2(mark.center().x, mark.top()), pos2(mark.right(), mark.bottom())], ink),
                "filter" => line(painter, vec![pos2(mark.left(), mark.top() + mark.height() * 0.3), pos2(mark.center().x, mark.top() + mark.height() * 0.3), pos2(mark.right(), mark.bottom())], ink),
                "capture" => wave(painter, mark, 2.5, 0.9, ink),
                "ir" => line(painter, (0..=16).map(|i| i as f32 / 16.0).map(|t| pos2(mark.left() + mark.width() * t, mark.bottom() - mark.height() * (-5.0 * t).exp())).collect(), ink),
                _ => {}
            }
            let switch = pos2(c.x, body.bottom() - s * 0.17);
            painter.circle_filled(switch + vec2(0.0, s * 0.015), s * 0.085, Color32::from_black_alpha(110));
            theme::radial(painter, switch, s * 0.08, Color32::from_gray(225), Color32::from_gray(70));
        }
    }
}

/// The power symbol, filled when on.
pub fn power(painter: &Painter, c: Pos2, radius: f32, on: bool, color: Color32) {
    let col = if on { color } else { Color32::from_gray(90) };
    let s = Stroke::new(1.6_f32, col);
    let pts: Vec<Pos2> = (0..=20).map(|i| -PI / 2.0 + 0.6 + (TAU - 1.2) * i as f32 / 20.0).map(|a| c + vec2(a.cos(), a.sin()) * radius).collect();
    line(painter, pts, s);
    painter.line_segment([c - vec2(0.0, radius * 1.1), c - vec2(0.0, radius * 0.2)], s);
}

/// Small icons of the edit screen: save, refresh, footswitch, snapshot, tempo, tap, in, out.
pub fn ui(painter: &Painter, r: Rect, name: &str, color: Color32) {
    let s = Stroke::new(1.6_f32, color);
    let r = r.shrink(r.width() * 0.22);
    let (c, w, h) = (r.center(), r.width(), r.height());
    match name {
        "save" => {
            painter.rect_stroke(r, 2.0, s);
            painter.rect_stroke(Rect::from_min_max(pos2(r.left() + w * 0.25, r.top()), pos2(r.right() - w * 0.25, r.top() + h * 0.35)), 1.0, s);
            painter.rect_filled(Rect::from_min_max(pos2(r.left() + w * 0.2, c.y + h * 0.1), pos2(r.right() - w * 0.2, r.bottom() - 2.0)), 1.0, color.linear_multiply(0.5));
        }
        "refresh" => {
            let pts: Vec<Pos2> = (0..=20).map(|i| 0.5 + 4.6 * i as f32 / 20.0).map(|a| c + vec2(a.cos(), a.sin()) * w * 0.45).collect();
            let end = *pts.last().unwrap_or(&c);
            line(painter, pts, s);
            line(painter, vec![end + vec2(-4.0, -1.0), end, end + vec2(1.0, 4.0)], s);
        }
        "footswitch" => {
            painter.circle_stroke(pos2(c.x, r.top() + h * 0.3), w * 0.28, s);
            painter.rect_stroke(Rect::from_min_max(pos2(r.left(), c.y + h * 0.1), r.right_bottom()), 3.0, s);
        }
        "snapshot" => {
            painter.rect_stroke(Rect::from_min_max(pos2(r.left(), r.top() + h * 0.2), r.right_bottom()), 3.0, s);
            painter.circle_stroke(pos2(c.x, c.y + h * 0.1), w * 0.22, s);
            painter.rect_filled(Rect::from_min_size(pos2(r.left() + w * 0.25, r.top()), vec2(w * 0.3, h * 0.2)), 1.0, color);
        }
        "tempo" => {
            line(painter, vec![pos2(r.left() + w * 0.15, r.bottom()), pos2(c.x, r.top()), pos2(r.right() - w * 0.15, r.bottom()), pos2(r.left() + w * 0.15, r.bottom())], s);
            painter.line_segment([pos2(c.x, r.bottom() - h * 0.15), pos2(r.right() - w * 0.1, r.top() + h * 0.2)], s);
        }
        "tap" => {
            painter.circle_filled(c, w * 0.18, color);
            for k in 1..3 {
                painter.circle_stroke(c, w * (0.18 + 0.16 * k as f32), Stroke::new(1.0_f32, color.linear_multiply(1.0 - 0.35 * k as f32)));
            }
        }
        "in" | "out" => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(w * 0.5, h * 0.9)), 3.0, s);
            painter.circle_filled(c, w * 0.12, color);
            let dir = if name == "in" { -1.0 } else { 1.0 };
            painter.line_segment([c + vec2(dir * w * 0.25, 0.0), c + vec2(dir * w * 0.55, 0.0)], s);
        }
        _ => {
            for k in 0..3 {
                painter.circle_filled(pos2(c.x, r.top() + h * (0.2 + 0.3 * k as f32)), 2.0, color);
            }
        }
    }
}

/// The icons of the screen bar.
pub fn screen(painter: &Painter, r: Rect, name: &str, color: Color32) {
    let s = Stroke::new(1.8_f32, color);
    let r = r.shrink(r.width() * 0.26);
    let (c, w, h) = (r.center(), r.width(), r.height());
    match name {
        "edit" => {
            for k in 0..3 {
                let x = r.left() + w * k as f32 / 2.0;
                painter.rect_stroke(Rect::from_center_size(pos2(x, c.y), vec2(w * 0.28, h * 0.4)), 2.0, s);
            }
            painter.line_segment([pos2(r.left(), c.y), pos2(r.right(), c.y)], Stroke::new(1.0_f32, color));
        }
        "library" => {
            for k in 0..3 {
                let x = r.left() + w * (0.15 + 0.3 * k as f32);
                painter.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], Stroke::new(3.0_f32, color));
            }
            painter.line_segment([r.left_bottom(), r.right_bottom()], s);
        }
        "favorites" => {
            let pts: Vec<Pos2> = (0..=10).map(|i| {
                let a = -PI / 2.0 + i as f32 * PI / 5.0;
                let rad = if i % 2 == 0 { w * 0.5 } else { w * 0.2 };
                c + vec2(a.cos(), a.sin()) * rad
            }).collect();
            line(painter, pts, s);
        }
        "catalog" => {
            painter.rect_stroke(r, 2.0, s);
            painter.line_segment([pos2(c.x, r.top()), pos2(c.x, r.bottom())], s);
        }
        "settings" => {
            painter.circle_stroke(c, w * 0.22, s);
            for k in 0..8 {
                let a = k as f32 * TAU / 8.0;
                painter.line_segment([c + vec2(a.cos(), a.sin()) * w * 0.32, c + vec2(a.cos(), a.sin()) * w * 0.5], s);
            }
        }
        _ => {
            for k in 0..3 {
                let y = r.top() + h * (0.2 + 0.3 * k as f32);
                painter.line_segment([pos2(r.left(), y), pos2(r.right(), y)], s);
            }
        }
    }
}
