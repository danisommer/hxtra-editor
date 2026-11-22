//! Our own drawings: one glyph per block category, the power symbol and the screen icons.

use std::f32::consts::{PI, TAU};

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

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

/// The glyph of a category inside `r`.
pub fn category(painter: &Painter, r: Rect, category: &str, color: Color32) {
    let s = Stroke::new(2.0_f32, color);
    let r = r.shrink(r.width() * 0.18);
    let (c, w, h) = (r.center(), r.width(), r.height());
    match category {
        "amp" | "preamp" => {
            painter.rect_stroke(r, 3.0, s);
            for k in 1..4 {
                let y = r.top() + h * k as f32 / 4.0;
                painter.line_segment([pos2(r.left() + 4.0, y), pos2(r.right() - 4.0, y)], Stroke::new(1.0_f32, color));
            }
            if category == "preamp" {
                painter.circle_filled(pos2(r.right() - 5.0, r.top() + 5.0), 2.5, color);
            }
        }
        "cab" | "cab_ir_interp" => {
            painter.rect_stroke(r, 3.0, s);
            painter.circle_stroke(c, w * 0.3, s);
            painter.circle_filled(c, w * 0.08, color);
        }
        "ir" => {
            painter.line_segment([pos2(r.left(), r.bottom()), pos2(r.right(), r.bottom())], s);
            painter.line_segment([pos2(r.left() + w * 0.2, r.bottom()), pos2(r.left() + w * 0.2, r.top())], s);
            line(painter, 
                (0..=20).map(|i| i as f32 / 20.0).map(|t| pos2(r.left() + w * (0.2 + 0.8 * t), r.bottom() - h * 0.7 * (-5.0 * t).exp())).collect(),
                Stroke::new(1.5_f32, color),
            );
        }
        "capture" => {
            wave(painter, r, 2.0, 0.6, s);
            painter.circle_stroke(c, w * 0.48, Stroke::new(1.0_f32, color));
        }
        "distortion" => {
            let pts = vec![
                pos2(r.left(), c.y),
                pos2(r.left() + w * 0.2, r.top()),
                pos2(r.left() + w * 0.4, r.bottom()),
                pos2(r.left() + w * 0.6, r.top()),
                pos2(r.left() + w * 0.8, r.bottom()),
                pos2(r.right(), c.y),
            ];
            line(painter, pts, s);
        }
        "dynamics" => {
            line(painter, vec![r.left_bottom(), pos2(c.x, c.y), r.right_top() + vec2(0.0, h * 0.35)], s);
            painter.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0_f32, color));
        }
        "eq" => {
            for (k, y) in [(0.2, 0.3), (0.5, 0.7), (0.8, 0.45)] {
                let x = r.left() + w * k;
                painter.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], Stroke::new(1.0_f32, color));
                painter.circle_filled(pos2(x, r.top() + h * y), 3.5, color);
            }
        }
        "modulation" => wave(painter, r, 1.5, 0.7, s),
        "delay" => {
            for k in 0..4 {
                let x = r.left() + w * (0.1 + 0.27 * k as f32);
                let top = r.top() + h * (0.15 * k as f32);
                painter.line_segment([pos2(x, r.bottom()), pos2(x, top)], s);
            }
        }
        "reverb" => {
            for k in 1..4 {
                let rad = w * 0.16 * k as f32;
                let pts: Vec<Pos2> = (0..=16).map(|i| PI + PI * i as f32 / 16.0).map(|a| pos2(r.left() + w * 0.15 + rad * a.cos().abs(), r.bottom() + rad * a.sin())).collect();
                line(painter, pts, Stroke::new(1.5 + 0.3 * k as f32, color.linear_multiply(1.0 - 0.2 * k as f32)));
            }
        }
        "pitch" | "synth" => {
            painter.line_segment([pos2(c.x, r.bottom()), pos2(c.x, r.top())], s);
            line(painter, vec![pos2(c.x - w * 0.3, r.top() + h * 0.35), pos2(c.x, r.top()), pos2(c.x + w * 0.3, r.top() + h * 0.35)], s);
        }
        "filter" => {
            let pts = vec![pos2(r.left(), r.top() + h * 0.3), pos2(c.x, r.top() + h * 0.3), pos2(r.left() + w * 0.7, r.top()), pos2(r.right(), r.bottom())];
            line(painter, pts, s);
        }
        "wah" => {
            line(painter, vec![r.left_bottom(), r.right_bottom(), pos2(r.right() - w * 0.1, r.top() + h * 0.2), r.left_bottom()], s);
        }
        "volume" => {
            line(painter, vec![r.left_bottom(), r.right_bottom(), r.right_top(), r.left_bottom()], s);
        }
        "fxloop" | "send" | "return" => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(w * 0.8, h * 0.6)), 6.0, s);
            line(painter, vec![pos2(c.x - 4.0, c.y - h * 0.3 - 4.0), pos2(c.x + 2.0, c.y - h * 0.3), pos2(c.x - 4.0, c.y - h * 0.3 + 4.0)], s);
        }
        "looper" => {
            let pts: Vec<Pos2> = (0..=24).map(|i| 0.3 + 5.6 * i as f32 / 24.0).map(|a| c + vec2(a.cos(), a.sin()) * w * 0.4).collect();
            line(painter, pts, s);
        }
        _ => {
            painter.circle_stroke(c, w * 0.35, s);
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
