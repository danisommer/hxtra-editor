//! Our own icons, drawn with soft fills and crisp strokes: one per block category (on a tinted
//! plate in the chain), the power symbol and the toolbar and screen icons. Small sizes leave out
//! the finest details.

use std::f32::consts::{PI, TAU};

use eframe::egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

use crate::theme;

fn stroke_path(painter: &Painter, points: Vec<Pos2>, width: f32, color: Color32) {
    let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) else { return };
    painter.add(Shape::line(points, Stroke::new(width, color)));
    if width >= 1.4 && color.a() == 255 {
        painter.circle_filled(first, width / 2.0, color);
        painter.circle_filled(last, width / 2.0, color);
    }
}

fn signed_area(points: &[Pos2]) -> f32 {
    points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - b.x * a.y).sum()
}

/// Draws in a square box in unit coordinates (0..1 from the top left), with a stroke width
/// scaled to the size.
struct Pen<'a> {
    p: &'a Painter,
    b: Rect,
    w: f32,
    detail: bool,
}

impl<'a> Pen<'a> {
    fn new(p: &'a Painter, r: Rect, pad: f32, weight: f32, max: f32) -> Self {
        let s = r.width().min(r.height());
        let b = Rect::from_center_size(r.center(), vec2(s, s)).shrink(s * pad);
        Pen { p, b, w: (b.width() * weight).clamp(1.1, max), detail: b.width() >= 20.0 }
    }

    fn q(&self, (x, y): (f32, f32)) -> Pos2 {
        pos2(self.b.left() + x * self.b.width(), self.b.top() + y * self.b.height())
    }

    fn u(&self, v: f32) -> f32 {
        v * self.b.width()
    }

    fn pts(&self, xy: &[(f32, f32)]) -> Vec<Pos2> {
        xy.iter().map(|p| self.q(*p)).collect()
    }

    fn samples(&self) -> usize {
        ((self.b.width() * 1.2) as usize).clamp(12, 48)
    }

    fn path(&self, xy: &[(f32, f32)], width: f32, color: Color32) {
        stroke_path(self.p, self.pts(xy), width, color);
    }

    fn line(&self, points: Vec<Pos2>, width: f32, color: Color32) {
        stroke_path(self.p, points, width, color);
    }

    fn curve(&self, x0: f32, x1: f32, f: impl Fn(f32) -> f32) -> Vec<Pos2> {
        let n = self.samples();
        (0..=n).map(|i| x0 + (x1 - x0) * i as f32 / n as f32).map(|x| self.q((x, f(x)))).collect()
    }

    fn arc(&self, (cx, cy): (f32, f32), (rx, ry): (f32, f32), a0: f32, a1: f32) -> Vec<Pos2> {
        let n = self.samples();
        (0..=n).map(|i| a0 + (a1 - a0) * i as f32 / n as f32).map(|a| self.q((cx + rx * a.cos(), cy + ry * a.sin()))).collect()
    }

    /// The area between a curve and the horizontal line `base`, `near` at the curve fading to
    /// `far` at the line.
    fn area(&self, points: &[Pos2], base: f32, near: Color32, far: Color32) {
        let y = self.q((0.0, base)).y;
        let mut mesh = Mesh::default();
        for p in points {
            mesh.colored_vertex(*p, near);
            mesh.colored_vertex(pos2(p.x, y), far);
        }
        for i in 0..points.len().saturating_sub(1) as u32 {
            mesh.add_triangle(2 * i, 2 * i + 1, 2 * i + 2);
            mesh.add_triangle(2 * i + 1, 2 * i + 3, 2 * i + 2);
        }
        self.p.add(Shape::mesh(mesh));
    }

    fn rect(&self, a: (f32, f32), b: (f32, f32), radius: f32, fill: Color32, stroke: Option<(f32, Color32)>) {
        let r = Rect::from_min_max(self.q(a), self.q(b));
        self.p.rect_filled(r, self.u(radius), fill);
        if let Some((width, color)) = stroke {
            self.p.rect_stroke(r, self.u(radius), Stroke::new(width, color));
        }
    }

    fn bar(&self, a: (f32, f32), b: (f32, f32), radius: f32, top: Color32, bottom: Color32) {
        theme::gradient(self.p, Rect::from_min_max(self.q(a), self.q(b)), self.u(radius), top, bottom);
    }

    fn dot(&self, c: (f32, f32), r: f32, color: Color32) {
        self.p.circle_filled(self.q(c), self.u(r), color);
    }

    fn ring(&self, c: (f32, f32), r: f32, width: f32, color: Color32) {
        self.p.circle_stroke(self.q(c), self.u(r), Stroke::new(width, color));
    }

    /// A small highlight on the top left of a disc.
    fn shine(&self, c: (f32, f32), r: f32) {
        let r = self.u(r);
        self.p.circle_filled(self.q(c) - vec2(r * 0.35, r * 0.35), r * 0.38, Color32::from_white_alpha(80));
    }

    fn convex(&self, mut points: Vec<Pos2>, fill: Color32, (width, color): (f32, Color32)) {
        if signed_area(&points) < 0.0 {
            points.reverse();
        }
        self.p.add(Shape::convex_polygon(points, fill, Stroke::new(width, color)));
    }

    /// A polygon that every point of sees `center` (a star, a gear, a fan of a circle).
    fn fan(&self, center: Pos2, points: Vec<Pos2>, fill: Color32, stroke: Option<(f32, Color32)>) {
        let mut mesh = Mesh::default();
        mesh.colored_vertex(center, fill);
        for p in &points {
            mesh.colored_vertex(*p, fill);
        }
        let n = points.len() as u32;
        for i in 0..n {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
        }
        self.p.add(Shape::mesh(mesh));
        if let Some((width, color)) = stroke {
            self.p.add(Shape::closed_line(points, Stroke::new(width, color)));
        }
    }

    /// An arrowhead with its tip at `tip`, pointing along `dir` (a unit vector).
    fn head(&self, tip: Pos2, dir: Vec2, size: f32, color: Color32) {
        let (s, n) = (self.u(size), vec2(-dir.y, dir.x));
        self.convex(vec![tip, tip - dir * s + n * s * 0.62, tip - dir * s - n * s * 0.62], color, (0.8, color));
    }
}

fn tone(color: Color32, on: bool) -> Color32 {
    match on {
        true => color,
        false => theme::lerp(color, Color32::from_gray(78), 0.72),
    }
}

/// A block of the chain: a raised plate tinted by its category, a rim in the category color and
/// the category's icon; off, the colors fade.
pub fn block(painter: &Painter, r: Rect, category: &str, color: Color32, on: bool) {
    let col = tone(color, on);
    theme::plate(painter, r, 12.0, theme::lerp(theme::TILE, col, if on { 0.16 } else { 0.05 }));
    painter.rect_stroke(r.shrink(0.5), 12.0, Stroke::new(1.2_f32, col.linear_multiply(if on { 0.75 } else { 0.35 })));
    let icon = Rect::from_center_size(r.center() + vec2(0.0, 2.0), r.size() * 0.6);
    glyph(&Pen::new(painter, icon, 0.12, 0.075, 2.3), category, col);
}

/// The icon of a model category in its color.
pub fn category(painter: &Painter, r: Rect, category: &str, color: Color32) {
    glyph(&Pen::new(painter, r, 0.12, 0.075, 2.3), category, color);
}

fn glyph(pen: &Pen, category: &str, col: Color32) {
    let w = pen.w;
    let fade = |a: f32| col.linear_multiply(a);
    let (soft, half, light, deep) = (fade(0.2), fade(0.5), theme::shade(col, 0.45), theme::shade(col, -0.45));
    let thin = (w * 0.5).max(0.7);
    match category {
        "amp" => {
            pen.path(&[(0.34, 0.22), (0.34, 0.12), (0.66, 0.12), (0.66, 0.22)], w, col);
            pen.rect((0.04, 0.22), (0.96, 0.88), 0.1, soft, Some((w, col)));
            pen.rect((0.14, 0.32), (0.86, 0.6), 0.05, fade(0.32), None);
            if pen.detail {
                for y in [0.39, 0.46, 0.53] {
                    pen.path(&[(0.19, y), (0.81, y)], thin, fade(0.4));
                }
            }
            for x in [0.22, 0.37, 0.52, 0.67] {
                pen.dot((x, 0.74), 0.045, light);
            }
            pen.dot((0.83, 0.74), 0.045, col);
            pen.shine((0.83, 0.74), 0.045);
        }
        "preamp" => {
            let mut glass = pen.arc((0.5, 0.32), (0.22, 0.22), PI, TAU);
            glass.extend(pen.pts(&[(0.72, 0.74), (0.28, 0.74)]));
            pen.convex(glass, soft, (w, col));
            pen.dot((0.5, 0.5), 0.14, fade(0.32));
            pen.rect((0.4, 0.3), (0.6, 0.62), 0.02, Color32::TRANSPARENT, Some((thin, half)));
            pen.dot((0.5, 0.5), 0.05, light);
            pen.rect((0.24, 0.74), (0.76, 0.86), 0.04, col, None);
            for x in [0.36, 0.5, 0.64] {
                pen.path(&[(x, 0.86), (x, 0.96)], w * 0.8, col);
            }
        }
        "cab" | "cab_ir_interp" => {
            pen.rect((0.06, 0.06), (0.94, 0.94), 0.14, soft, Some((w, col)));
            if pen.detail {
                for c in [(0.17, 0.17), (0.83, 0.17), (0.17, 0.83), (0.83, 0.83)] {
                    pen.dot(c, 0.03, half);
                }
            }
            pen.dot((0.5, 0.5), 0.31, fade(0.3));
            pen.ring((0.5, 0.5), 0.31, w, col);
            if pen.detail {
                pen.ring((0.5, 0.5), 0.2, thin, half);
            }
            pen.dot((0.5, 0.5), 0.1, light);
            pen.shine((0.5, 0.5), 0.1);
        }
        "ir" => {
            pen.path(&[(0.04, 0.78), (0.96, 0.78)], thin, half);
            let pts = pen.curve(0.08, 0.94, |x| {
                let t = (x - 0.08) / 0.86;
                let v = (-4.2 * t).exp() * (t * 5.5 * TAU).cos();
                0.78 - if v > 0.0 { 0.66 * v } else { 0.2 * v }
            });
            pen.area(&pts, 0.78, fade(0.3), fade(0.04));
            pen.line(pts, w, col);
            pen.dot((0.08, 0.12), 0.05, light);
        }
        "capture" => {
            let layers: [&[(f32, f32)]; 3] = [&[(0.12, 0.32), (0.12, 0.68)], &[(0.5, 0.16), (0.5, 0.5), (0.5, 0.84)], &[(0.88, 0.5)]];
            for pair in layers.windows(2) {
                for a in pair[0] {
                    for b in pair[1] {
                        pen.path(&[*a, *b], thin, fade(0.45));
                    }
                }
            }
            let node = theme::lerp(Color32::from_gray(28), col, 0.4);
            for (i, layer) in layers.iter().enumerate() {
                for n in *layer {
                    let (r, fill) = if i == 2 { (0.1, col) } else { (0.085, node) };
                    pen.dot(*n, r, fill);
                    pen.ring(*n, r, w * 0.8, col);
                    pen.shine(*n, r);
                }
            }
        }
        "distortion" => {
            if pen.detail {
                for y in [0.2, 0.8] {
                    pen.path(&[(0.04, y), (0.96, y)], thin, fade(0.35));
                }
            }
            let pts = pen.curve(0.06, 0.94, |x| 0.5 - (((x - 0.06) / 0.88 * 1.5 * TAU).sin() * 1.7).clamp(-1.0, 1.0) * 0.3);
            pen.area(&pts, 0.5, fade(0.32), fade(0.06));
            pen.line(pts, w, col);
        }
        "dynamics" => {
            pen.path(&[(0.08, 0.06), (0.08, 0.92), (0.94, 0.92)], thin, half);
            let knee = |x: f32| {
                let (t, k, r) = (0.5, 0.24, 0.28);
                match x {
                    x if x < t - k / 2.0 => x,
                    x if x > t + k / 2.0 => t + (x - t) * r,
                    x => x + (r - 1.0) * (x - t + k / 2.0).powi(2) / (2.0 * k),
                }
            };
            let at = |v: f32| (0.08 + 0.86 * v, 0.92 - 0.86 * knee(v));
            if pen.detail {
                pen.path(&[at(0.5), (0.94, 0.06)], thin, fade(0.3));
            }
            let pts = pen.curve(0.08, 0.94, |x| at((x - 0.08) / 0.86).1);
            pen.area(&pts, 0.92, fade(0.3), fade(0.04));
            pen.line(pts, w, col);
            pen.dot(at(0.5), 0.055, light);
        }
        "eq" => {
            let bands = [(0.24, 0.2), (0.5, -0.17), (0.76, 0.24)];
            let y = |x: f32| 0.56 - bands.iter().map(|(c, gain)| gain * (-((x - c) / 0.11).powi(2)).exp()).sum::<f32>();
            pen.path(&[(0.04, 0.56), (0.96, 0.56)], thin, half);
            let pts = pen.curve(0.04, 0.96, y);
            pen.area(&pts, 0.56, fade(0.3), fade(0.08));
            pen.line(pts, w, col);
            for (c, _) in bands {
                pen.dot((c, y(c)), 0.06, light);
                pen.ring((c, y(c)), 0.06, thin, deep);
            }
        }
        "modulation" => {
            let phase = |x: f32| (x - 0.04) / 0.92 * 1.25 * TAU;
            pen.line(pen.curve(0.04, 0.96, |x| 0.5 - (phase(x) + 1.1).sin() * 0.24), w * 0.75, fade(0.4));
            let pts = pen.curve(0.04, 0.96, |x| 0.5 - phase(x).sin() * 0.32);
            pen.area(&pts, 0.5, fade(0.28), fade(0.04));
            pen.line(pts, w, col);
        }
        "delay" => {
            pen.path(&[(0.04, 0.9), (0.96, 0.9)], thin, half);
            for ((x, h), a) in [(0.18, 0.74), (0.4, 0.54), (0.62, 0.38), (0.84, 0.24)].into_iter().zip([1.0, 0.72, 0.5, 0.32]) {
                pen.bar((x - 0.07, 0.9 - h), (x + 0.07, 0.9), 0.05, fade(a), fade(a * 0.35));
            }
            pen.shine((0.18, 0.2), 0.06);
        }
        "reverb" => {
            let o = (0.14, 0.86);
            let room = pen.arc(o, (0.78, 0.78), -PI / 2.0, 0.0).into_iter().chain(std::iter::once(pen.q(o))).collect();
            pen.fan(pen.q((o.0 + 0.26, o.1 - 0.26)), room, fade(0.1), None);
            for (k, rad) in [0.26, 0.46, 0.66].into_iter().enumerate() {
                let k = k as f32;
                pen.line(pen.arc(o, (rad, rad), -PI / 2.0, 0.0), w * (1.0 - 0.12 * k), fade(1.0 - 0.28 * k));
            }
            pen.dot(o, 0.08, col);
            pen.shine(o, 0.08);
        }
        "pitch" => {
            let tilt = -0.42_f32;
            let head = (0..20)
                .map(|i| TAU * i as f32 / 20.0)
                .map(|a| {
                    let (x, y) = (0.155 * a.cos(), 0.105 * a.sin());
                    pen.q((0.36 + x * tilt.cos() - y * tilt.sin(), 0.76 + x * tilt.sin() + y * tilt.cos()))
                })
                .collect();
            pen.path(&[(0.5, 0.73), (0.5, 0.12)], w, col);
            pen.convex(head, col, (0.8, col));
            let flag = (0..=10)
                .map(|i| i as f32 / 10.0)
                .map(|t| {
                    let ((ax, ay), (cx, cy), (bx, by), m) = ((0.5, 0.12), (0.74, 0.2), (0.7, 0.46), 1.0 - t);
                    pen.q((m * m * ax + 2.0 * m * t * cx + t * t * bx, m * m * ay + 2.0 * m * t * cy + t * t * by))
                })
                .collect();
            pen.line(flag, w, col);
            if pen.detail {
                pen.path(&[(0.8, 0.62), (0.87, 0.54), (0.94, 0.62)], w * 0.7, half);
                pen.path(&[(0.8, 0.74), (0.87, 0.82), (0.94, 0.74)], w * 0.7, half);
            }
        }
        "synth" => {
            pen.rect((0.04, 0.2), (0.96, 0.84), 0.06, soft, Some((w, col)));
            let key = |k: usize| 0.04 + 0.92 * k as f32 / 6.0;
            for k in 1..6 {
                pen.path(&[(key(k), 0.2), (key(k), 0.84)], thin, half);
            }
            for k in [1, 2, 4, 5] {
                pen.rect((key(k) - 0.045, 0.2), (key(k) + 0.045, 0.58), 0.015, col, None);
            }
        }
        "filter" => {
            let y = |x: f32| {
                let fall = if x < 0.6 { 0.0 } else { ((x - 0.6) / 0.34).powf(1.6) * 0.52 };
                (0.42 + fall - 0.22 * (-((x - 0.6) / 0.07).powi(2)).exp()).min(0.92)
            };
            if pen.detail {
                pen.path(&[(0.6, 0.92), (0.6, y(0.6))], thin, fade(0.35));
            }
            let pts = pen.curve(0.06, 0.94, y);
            pen.area(&pts, 0.92, fade(0.3), fade(0.04));
            pen.line(pts, w, col);
            pen.dot((0.6, y(0.6)), 0.05, light);
        }
        "wah" => {
            pen.rect((0.1, 0.64), (0.9, 0.88), 0.06, soft, Some((w, col)));
            let (a, b, c, d) = ((0.06, 0.56), (0.9, 0.26), (0.94, 0.37), (0.1, 0.67));
            pen.convex(pen.pts(&[a, b, c, d]), fade(0.45), (w, col));
            if pen.detail {
                let mix = |p: (f32, f32), q: (f32, f32), t: f32| (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
                for k in 1..6 {
                    let t = k as f32 / 6.0;
                    let (top, bottom) = (mix(a, b, t), mix(d, c, t));
                    pen.path(&[mix(top, bottom, 0.25), mix(top, bottom, 0.75)], thin, deep);
                }
            }
            pen.dot((0.56, 0.64), 0.05, light);
        }
        "volume" => {
            pen.p.add(Shape::closed_line(pen.pts(&[(0.06, 0.88), (0.94, 0.88), (0.94, 0.12)]), Stroke::new(thin, fade(0.35))));
            for k in 0..5 {
                let x = 0.16 + 0.17 * k as f32;
                let h = 0.82 * (x - 0.06) * 0.864 + 0.02;
                let a = 0.35 + 0.16 * k as f32;
                pen.bar((x - 0.055, 0.88 - h), (x + 0.055, 0.88), 0.03, fade(a), fade(a * 0.4));
            }
        }
        "fxloop" | "send" | "return" => {
            pen.line(pen.arc((0.5, 0.5), (0.24, 0.3), PI, TAU), w, col);
            pen.head(pen.q((0.74, 0.55)), vec2(0.0, 1.0), 0.11, col);
            for x in [0.26, 0.74] {
                pen.dot((x, 0.7), 0.15, soft);
                pen.ring((x, 0.7), 0.15, w, col);
                pen.dot((x, 0.7), 0.055, col);
            }
        }
        "looper" => {
            let (a0, a1) = (-PI / 2.0 + 0.55, -PI / 2.0 + TAU - 0.3);
            pen.line(pen.arc((0.5, 0.5), (0.36, 0.36), a0, a1), w * 1.1, col);
            let end = pen.q((0.5 + 0.36 * a1.cos(), 0.5 + 0.36 * a1.sin()));
            let dir = vec2(-a1.sin(), a1.cos());
            pen.head(end + dir * pen.u(0.14), dir, 0.18, col);
            pen.dot((0.5, 0.5), 0.12, fade(0.55));
            pen.ring((0.5, 0.5), 0.12, w * 0.7, col);
            pen.shine((0.5, 0.5), 0.12);
        }
        _ => {
            pen.dot((0.5, 0.5), 0.34, soft);
            pen.ring((0.5, 0.5), 0.34, w, col);
            pen.dot((0.5, 0.5), 0.1, col);
        }
    }
}

/// The power symbol; on, it glows.
pub fn power(painter: &Painter, c: Pos2, radius: f32, on: bool, color: Color32) {
    let col = if on { color } else { Color32::from_gray(90) };
    let w = (radius * 0.3).clamp(1.3, 2.0);
    if on {
        painter.circle_filled(c, radius * 1.6, col.linear_multiply(0.12));
    }
    let arc: Vec<Pos2> = (0..=24).map(|i| -PI / 2.0 + 0.62 + (TAU - 1.24) * i as f32 / 24.0).map(|a| c + vec2(a.cos(), a.sin()) * radius).collect();
    stroke_path(painter, arc, w, col);
    stroke_path(painter, vec![c - vec2(0.0, radius * 1.15), c - vec2(0.0, radius * 0.15)], w, col);
}

/// Small icons of the edit screen: save, refresh, footswitch, snapshot, tempo, tap, in, out.
pub fn ui(painter: &Painter, r: Rect, name: &str, color: Color32) {
    let pen = Pen::new(painter, r, 0.18, 0.085, 1.9);
    let (w, col) = (pen.w, color);
    let fade = |a: f32| col.linear_multiply(a);
    let (soft, half, thin) = (fade(0.16), fade(0.5), (pen.w * 0.55).max(0.8));
    match name {
        "save" => {
            pen.convex(pen.pts(&[(0.08, 0.08), (0.74, 0.08), (0.92, 0.26), (0.92, 0.92), (0.08, 0.92)]), soft, (w, col));
            pen.rect((0.26, 0.08), (0.66, 0.34), 0.03, Color32::TRANSPARENT, Some((w * 0.8, col)));
            pen.rect((0.52, 0.13), (0.6, 0.29), 0.02, col, None);
            pen.rect((0.22, 0.54), (0.78, 0.86), 0.05, fade(0.35), None);
            if pen.detail {
                for y in [0.65, 0.76] {
                    pen.path(&[(0.32, y), (0.68, y)], thin, fade(0.75));
                }
            }
        }
        "refresh" => {
            for (a0, a1) in [(PI + 0.35, TAU - 0.2), (0.35, PI - 0.2)] {
                pen.line(pen.arc((0.5, 0.5), (0.36, 0.36), a0, a1), w, col);
                let end = pen.q((0.5 + 0.36 * a1.cos(), 0.5 + 0.36 * a1.sin()));
                let dir = vec2(-a1.sin(), a1.cos());
                pen.head(end + dir * pen.u(0.12), dir, 0.2, col);
            }
        }
        "footswitch" => {
            let hex = (0..6).map(|i| PI / 6.0 + TAU * i as f32 / 6.0).map(|a| pen.q((0.5 + 0.44 * a.cos(), 0.5 + 0.44 * a.sin()))).collect();
            pen.convex(hex, soft, (w, col));
            pen.ring((0.5, 0.5), 0.29, thin, half);
            pen.dot((0.5, 0.5), 0.2, fade(0.6));
            pen.shine((0.5, 0.5), 0.2);
        }
        "snapshot" => {
            pen.convex(pen.pts(&[(0.32, 0.28), (0.4, 0.14), (0.6, 0.14), (0.68, 0.28)]), soft, (w, col));
            pen.rect((0.06, 0.28), (0.94, 0.86), 0.1, soft, Some((w, col)));
            pen.dot((0.5, 0.57), 0.2, fade(0.25));
            pen.ring((0.5, 0.57), 0.2, w, col);
            pen.dot((0.5, 0.57), 0.09, fade(0.7));
            pen.shine((0.5, 0.57), 0.09);
            pen.dot((0.8, 0.4), 0.04, col);
        }
        "tempo" => {
            pen.convex(pen.pts(&[(0.22, 0.9), (0.38, 0.08), (0.62, 0.08), (0.78, 0.9)]), soft, (w, col));
            if pen.detail {
                for k in 0..3 {
                    let y = 0.3 + 0.14 * k as f32;
                    pen.path(&[(0.45, y), (0.55, y)], thin, half);
                }
            }
            pen.path(&[(0.5, 0.78), (0.76, 0.2)], w, col);
            pen.dot((0.66, 0.42), 0.07, col);
            pen.path(&[(0.12, 0.9), (0.88, 0.9)], w, col);
        }
        "tap" => {
            pen.dot((0.5, 0.5), 0.4, fade(0.07));
            pen.ring((0.5, 0.5), 0.4, thin, fade(0.3));
            pen.ring((0.5, 0.5), 0.27, w * 0.8, fade(0.6));
            pen.dot((0.5, 0.5), 0.14, col);
            pen.shine((0.5, 0.5), 0.14);
        }
        "in" | "out" => {
            let (jack, from, to) = if name == "in" { (0.66, 0.02, 0.36) } else { (0.34, 0.64, 0.98) };
            pen.dot((jack, 0.5), 0.26, soft);
            pen.ring((jack, 0.5), 0.26, w, col);
            pen.ring((jack, 0.5), 0.14, thin, half);
            pen.dot((jack, 0.5), 0.07, col);
            pen.path(&[(from, 0.5), (to - 0.1, 0.5)], w, col);
            pen.head(pen.q((to, 0.5)), vec2(1.0, 0.0), 0.16, col);
        }
        _ => {
            for y in [0.2, 0.5, 0.8] {
                pen.dot((0.5, y), 0.08, col);
            }
        }
    }
}

/// The icons of the screen bar.
pub fn screen(painter: &Painter, r: Rect, name: &str, color: Color32) {
    let pen = Pen::new(painter, r, 0.24, 0.085, 2.0);
    let (w, col) = (pen.w, color);
    let fade = |a: f32| col.linear_multiply(a);
    let (soft, half, thin) = (fade(0.16), fade(0.5), (pen.w * 0.55).max(0.8));
    match name {
        "edit" => {
            pen.path(&[(0.02, 0.5), (0.98, 0.5)], thin, half);
            for (k, x) in [0.17, 0.5, 0.83].into_iter().enumerate() {
                let fill = if k == 1 { theme::lerp(theme::PANEL, col, 0.55) } else { theme::lerp(theme::PANEL, col, 0.12) };
                pen.rect((x - 0.12, 0.33), (x + 0.12, 0.67), 0.07, fill, Some((w, col)));
            }
        }
        "library" => {
            for (x0, x1, top) in [(0.04, 0.24, 0.14), (0.3, 0.5, 0.06), (0.56, 0.7, 0.2)] {
                pen.rect((x0, top), (x1, 0.92), 0.04, soft, Some((w, col)));
                if pen.detail {
                    pen.path(&[(x0 + 0.05, top + 0.16), (x1 - 0.05, top + 0.16)], thin, col);
                }
            }
            pen.convex(pen.pts(&[(0.72, 0.92), (0.84, 0.22), (0.98, 0.27), (0.88, 0.92)]), soft, (w, col));
        }
        "favorites" => {
            let star = (0..10)
                .map(|i| {
                    let a = -PI / 2.0 + i as f32 * PI / 5.0;
                    let rad = if i % 2 == 0 { 0.5 } else { 0.22 };
                    pen.q((0.5 + rad * a.cos(), 0.54 + rad * a.sin()))
                })
                .collect();
            pen.fan(pen.q((0.5, 0.54)), star, fade(0.24), Some((w, col)));
        }
        "catalog" => {
            pen.convex(pen.pts(&[(0.04, 0.12), (0.5, 0.22), (0.5, 0.9), (0.04, 0.8)]), soft, (w, col));
            pen.convex(pen.pts(&[(0.5, 0.22), (0.96, 0.12), (0.96, 0.8), (0.5, 0.9)]), soft, (w, col));
            if pen.detail {
                for k in 0..3 {
                    let y = 0.36 + 0.14 * k as f32;
                    pen.path(&[(0.13, y), (0.41, y + 0.06)], thin, half);
                    pen.path(&[(0.59, y + 0.06), (0.87, y)], thin, half);
                }
            }
        }
        "settings" => {
            let teeth = (0..8)
                .flat_map(|t| {
                    let base = TAU * t as f32 / 8.0;
                    [(-0.2, 0.33), (-0.1, 0.48), (0.1, 0.48), (0.2, 0.33)].map(|(da, rad)| pen.q((0.5 + rad * (base + da).cos(), 0.5 + rad * (base + da).sin())))
                })
                .collect();
            pen.fan(pen.q((0.5, 0.5)), teeth, soft, Some((w, col)));
            pen.ring((0.5, 0.5), 0.14, w, col);
        }
        _ => {
            pen.convex(pen.pts(&[(0.16, 0.06), (0.64, 0.06), (0.84, 0.26), (0.84, 0.94), (0.16, 0.94)]), soft, (w, col));
            pen.path(&[(0.64, 0.06), (0.64, 0.26), (0.84, 0.26)], w * 0.8, col);
            for (k, x1) in [0.72, 0.72, 0.56].into_iter().enumerate() {
                let y = 0.46 + 0.15 * k as f32;
                pen.path(&[(0.28, y), (x1, y)], w * 0.8, half);
            }
        }
    }
}
