//! A small SVG reader for Krita's SVG tips: its filled shapes (paths, rectangles, circles,
//! ellipses, polygons) under their groups' transforms, each in its fill's grey (its colour's
//! average) as far as its fill and opacity say, by the non-zero or even-odd rule, drawn over white
//! as Krita draws an SVG tip: its view box fitted to a picture 1000 pixels across. Strokes, text,
//! pictures, gradients and anything `use` brings in aren't drawn.

use crate::files::Rgba;
use crate::xml::Element;

/// How wide Krita draws an SVG tip.
pub const WIDTH: u32 = 1000;

/// Whether `bytes` look like an SVG picture.
pub fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    t.starts_with('<') && text.contains("<svg")
}

/// The view box (left, top, width, height) of the picture's root, or its width and height.
fn view_box(svg: &Element) -> Option<[f32; 4]> {
    if let Some(v) = svg.attribute("viewBox") {
        let n = numbers(v);
        if n.len() == 4 && n[2] > 0.0 && n[3] > 0.0 {
            return Some([n[0], n[1], n[2], n[3]]);
        }
    }
    let (w, h) = (length(svg.attribute("width")?)?, length(svg.attribute("height")?)?);
    (w > 0.0 && h > 0.0).then_some([0.0, 0.0, w, h])
}

/// A length without its unit.
fn length(v: &str) -> Option<f32> {
    let v = v.trim();
    let end = v.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))).unwrap_or(v.len());
    v[..end].parse::<f32>().ok().filter(|n| n.is_finite())
}

/// The numbers in `text`, as SVG writes them (commas or spaces between, signs and dots starting
/// the next).
fn numbers(text: &str) -> Vec<f32> {
    let mut out = Vec::new();
    let mut p = Numbers { s: text.as_bytes(), at: 0 };
    while let Some(n) = p.number() {
        out.push(n);
    }
    out
}

/// The size Krita draws the picture at: 1000 across, as tall as its view box says.
pub fn size(bytes: &[u8]) -> Option<(u32, u32)> {
    let root = crate::xml::parse(std::str::from_utf8(bytes).ok()?.trim_start_matches('\u{feff}'))?;
    let [_, _, w, h] = view_box(&root)?;
    Some((WIDTH, ((WIDTH as f32 * h / w) as u32).clamp(1, 4 * WIDTH)))
}

/// The picture drawn as Krita draws an SVG tip (grey, opaque: black where it paints).
pub fn render(bytes: &[u8]) -> Option<Rgba> {
    let root = crate::xml::parse(std::str::from_utf8(bytes).ok()?.trim_start_matches('\u{feff}'))?;
    let [vx, vy, vw, vh] = view_box(&root)?;
    let (width, height) = (WIDTH, ((WIDTH as f32 * vh / vw) as u32).clamp(1, 4 * WIDTH));
    let k = width as f32 / vw;
    let fit = [k, 0.0, 0.0, height as f32 / vh, -vx * k, -vy * (height as f32 / vh)];
    let mut canvas = vec![1.0_f32; (width * height) as usize];
    let style = Style { fill: Some(0.0), fill_opacity: 1.0, opacity: 1.0, even_odd: false };
    draw(&root, fit, style, &mut canvas, (width, height), 0);
    let pixels = canvas
        .iter()
        .flat_map(|g| {
            let v = (g.clamp(0.0, 1.0) * 255.0).round() as u8;
            [v, v, v, 255]
        })
        .collect();
    Some(Rgba { width, height, pixels })
}

/// What a shape is filled with, as it is inherited: its grey (none: not filled), the fill's
/// opacity, the opacity, and the rule.
#[derive(Clone, Copy, Debug)]
struct Style {
    fill: Option<f32>,
    fill_opacity: f32,
    opacity: f32,
    even_odd: bool,
}

type Matrix = [f32; 6];

fn times(a: Matrix, b: Matrix) -> Matrix {
    // a after b: a * b
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

fn apply(m: Matrix, (x, y): (f32, f32)) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// A `transform` attribute's matrix.
fn transform(text: &str) -> Matrix {
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut rest = text;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let Some(close) = rest[open..].find(')') else { break };
        let n = numbers(&rest[open + 1..open + close]);
        let get = |i: usize, d: f32| n.get(i).copied().unwrap_or(d);
        let t = match name {
            "matrix" if n.len() >= 6 => [n[0], n[1], n[2], n[3], n[4], n[5]],
            "translate" => [1.0, 0.0, 0.0, 1.0, get(0, 0.0), get(1, 0.0)],
            "scale" => [get(0, 1.0), 0.0, 0.0, get(1, get(0, 1.0)), 0.0, 0.0],
            "rotate" => {
                let (s, c) = get(0, 0.0).to_radians().sin_cos();
                let (cx, cy) = (get(1, 0.0), get(2, 0.0));
                let r = [c, s, -s, c, 0.0, 0.0];
                times(times([1.0, 0.0, 0.0, 1.0, cx, cy], r), [1.0, 0.0, 0.0, 1.0, -cx, -cy])
            }
            "skewX" => [1.0, 0.0, get(0, 0.0).to_radians().tan(), 1.0, 0.0, 0.0],
            "skewY" => [1.0, get(0, 0.0).to_radians().tan(), 0.0, 1.0, 0.0, 0.0],
            _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        };
        m = times(m, t);
        rest = &rest[open + close + 1..];
    }
    m
}

/// A colour's grey (its channels' average), or none for `none`.
fn grey(text: &str) -> Option<Option<f32>> {
    let t = text.trim().to_ascii_lowercase();
    let rgb: [f32; 3] = if t == "none" || t == "transparent" {
        return Some(None);
    } else if let Some(hex) = t.strip_prefix('#') {
        let v: Vec<u32> = hex.chars().filter_map(|c| c.to_digit(16)).collect();
        match v.len() {
            3 => [v[0] * 17, v[1] * 17, v[2] * 17].map(|c| c as f32 / 255.0),
            6 => [v[0] * 16 + v[1], v[2] * 16 + v[3], v[4] * 16 + v[5]].map(|c| c as f32 / 255.0),
            _ => return None,
        }
    } else if let Some(inner) = t.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() != 3 {
            return None;
        }
        let part = |p: &str| {
            let p = p.trim();
            match p.strip_suffix('%') {
                Some(pc) => pc.parse::<f32>().ok().map(|v| v / 100.0),
                None => p.parse::<f32>().ok().map(|v| v / 255.0),
            }
        };
        [part(parts[0])?, part(parts[1])?, part(parts[2])?]
    } else {
        match t.as_str() {
            "black" | "currentcolor" => [0.0; 3],
            "white" => [1.0; 3],
            "gray" | "grey" => [128.0 / 255.0; 3],
            "red" => [1.0, 0.0, 0.0],
            "green" => [0.0, 128.0 / 255.0, 0.0],
            "blue" => [0.0, 0.0, 1.0],
            _ => return None,
        }
    };
    Some(Some((rgb[0] + rgb[1] + rgb[2]).clamp(0.0, 3.0) / 3.0))
}

/// An element's own style: its attributes, then its `style`'s declarations over them.
fn styled(e: &Element, inherited: Style) -> (Style, bool) {
    let mut s = Style { opacity: 1.0, ..inherited };
    let mut hidden = false;
    let mut set = |key: &str, value: &str| match key.trim() {
        "fill" => {
            if let Some(g) = grey(value) {
                s.fill = g;
            }
        }
        "fill-opacity" => s.fill_opacity = value.trim().parse().unwrap_or(1.0_f32).clamp(0.0, 1.0),
        "opacity" => s.opacity = value.trim().parse().unwrap_or(1.0_f32).clamp(0.0, 1.0),
        "fill-rule" => s.even_odd = value.trim() == "evenodd",
        "display" => hidden |= value.trim() == "none",
        "visibility" => hidden |= value.trim() == "hidden",
        _ => {}
    };
    for (k, v) in &e.attributes {
        set(k, v);
    }
    if let Some(style) = e.attribute("style") {
        for decl in style.split(';') {
            if let Some((k, v)) = decl.split_once(':') {
                set(k, v);
            }
        }
    }
    (s, hidden)
}

/// The element's local name (without its namespace's prefix).
fn local(e: &Element) -> &str {
    e.name.rsplit(':').next().unwrap_or(&e.name)
}

/// Draws `e` and what it holds onto `canvas` (greys, `size`) through `m`.
fn draw(e: &Element, m: Matrix, inherited: Style, canvas: &mut [f32], size: (u32, u32), depth: u32) {
    if depth > 32 {
        return;
    }
    let (mut style, hidden) = styled(e, inherited);
    if hidden {
        return;
    }
    // (a group's opacity taken by what it holds)
    style.opacity *= inherited.opacity;
    let m = match e.attribute("transform") {
        Some(t) => times(m, transform(t)),
        None => m,
    };
    let name = local(e);
    match name {
        "svg" | "g" | "a" | "switch" => {
            for child in e.elements() {
                draw(child, m, style, canvas, size, depth + 1);
            }
            return;
        }
        "defs" | "symbol" | "clipPath" | "mask" | "pattern" | "metadata" | "title" | "desc" | "style" => return,
        _ => {}
    }
    let Some(fill) = style.fill else { return };
    let alpha = style.fill_opacity * style.opacity;
    if alpha <= 0.0 {
        return;
    }
    let num = |k: &str| e.attribute(k).and_then(length).unwrap_or(0.0);
    let shapes: Vec<Vec<(f32, f32)>> = match name {
        "path" => path(e.attribute("d").unwrap_or(""), m),
        "rect" => {
            let (x, y, w, h) = (num("x"), num("y"), num("width"), num("height"));
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            vec![[(x, y), (x + w, y), (x + w, y + h), (x, y + h)].iter().map(|&p| apply(m, p)).collect()]
        }
        "circle" | "ellipse" => {
            let (cx, cy) = (num("cx"), num("cy"));
            let (rx, ry) = if name == "circle" { (num("r"), num("r")) } else { (num("rx"), num("ry")) };
            if rx <= 0.0 || ry <= 0.0 {
                return;
            }
            let n = 96;
            vec![
                (0..n)
                    .map(|i| {
                        let t = i as f32 / n as f32 * std::f32::consts::TAU;
                        apply(m, (cx + rx * t.cos(), cy + ry * t.sin()))
                    })
                    .collect(),
            ]
        }
        "polygon" | "polyline" => {
            let n = numbers(e.attribute("points").unwrap_or(""));
            vec![n.as_chunks::<2>().0.iter().map(|p| apply(m, (p[0], p[1]))).collect()]
        }
        _ => return,
    };
    let cover = coverage(&shapes, size, style.even_odd);
    for (c, k) in canvas.iter_mut().zip(cover) {
        let a = k * alpha;
        if a > 0.0 {
            *c = *c * (1.0 - a) + fill * a;
        }
    }
}

struct Numbers<'a> {
    s: &'a [u8],
    at: usize,
}

impl Numbers<'_> {
    fn skip(&mut self) {
        while self.s.get(self.at).is_some_and(|c| c.is_ascii_whitespace() || *c == b',') {
            self.at += 1;
        }
    }

    fn number(&mut self) -> Option<f32> {
        self.skip();
        let from = self.at;
        let mut dot = false;
        let mut i = self.at;
        if matches!(self.s.get(i), Some(b'-' | b'+')) {
            i += 1;
        }
        while let Some(&c) = self.s.get(i) {
            if c.is_ascii_digit() {
                i += 1;
            } else if c == b'.' && !dot {
                dot = true;
                i += 1;
            } else if (c == b'e' || c == b'E') && i > from {
                let mut j = i + 1;
                if matches!(self.s.get(j), Some(b'-' | b'+')) {
                    j += 1;
                }
                if !self.s.get(j).is_some_and(u8::is_ascii_digit) {
                    break;
                }
                i = j;
                while self.s.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                }
                break;
            } else {
                break;
            }
        }
        let v = std::str::from_utf8(self.s.get(from..i)?).ok()?.parse::<f32>().ok()?;
        self.at = i;
        Some(v)
    }

    /// An arc's flag: a single 0 or 1.
    fn flag(&mut self) -> Option<bool> {
        self.skip();
        let c = *self.s.get(self.at)?;
        self.at += 1;
        match c {
            b'0' => Some(false),
            b'1' => Some(true),
            _ => None,
        }
    }

    fn command(&mut self) -> Option<u8> {
        self.skip();
        let c = *self.s.get(self.at)?;
        c.is_ascii_alphabetic().then(|| {
            self.at += 1;
            c
        })
    }
}

/// The closed outlines a path's data draws (canvas pixels through `m`), its curves as lines.
fn path(d: &str, m: Matrix) -> Vec<Vec<(f32, f32)>> {
    let mut out: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut now: Vec<(f32, f32)> = Vec::new();
    let mut p = Numbers { s: d.as_bytes(), at: 0 };
    let (mut cx, mut cy) = (0.0_f32, 0.0_f32);
    let (mut sx, mut sy) = (0.0_f32, 0.0_f32);
    // the last control point, for the smooth curves
    let mut last_c: Option<(f32, f32)> = None;
    let mut last_q: Option<(f32, f32)> = None;
    let mut command = b'M';
    // how finely curves are cut, by how much a unit of the path is on the canvas
    let scale = (m[0] * m[3] - m[1] * m[2]).abs().sqrt().max(1e-6);
    let pieces = |len: f32| ((len * scale / 4.0).ceil() as usize).clamp(4, 64);
    let mut steps = 0;
    loop {
        steps += 1;
        if steps > 1_000_000 {
            break;
        }
        let before = p.at;
        if let Some(c) = p.command() {
            command = c;
        } else if p.at >= p.s.len() {
            break;
        }
        let rel = command.is_ascii_lowercase();
        let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
        let add = |now: &mut Vec<(f32, f32)>, x: f32, y: f32| now.push(apply(m, (x, y)));
        let upper = command.to_ascii_uppercase();
        match upper {
            b'Z' => {
                if now.len() > 1 {
                    out.push(std::mem::take(&mut now));
                }
                now.clear();
                (cx, cy) = (sx, sy);
                last_c = None;
                last_q = None;
                // (no numbers follow: the next is a command)
                if p.at == before {
                    break;
                }
                continue;
            }
            b'M' => {
                let (Some(x), Some(y)) = (p.number(), p.number()) else { break };
                if now.len() > 1 {
                    out.push(std::mem::take(&mut now));
                }
                now.clear();
                (cx, cy) = (x + ox, y + oy);
                (sx, sy) = (cx, cy);
                add(&mut now, cx, cy);
                // more pairs after a move are lines
                command = if rel { b'l' } else { b'L' };
                last_c = None;
                last_q = None;
            }
            b'L' => {
                let (Some(x), Some(y)) = (p.number(), p.number()) else { break };
                (cx, cy) = (x + ox, y + oy);
                add(&mut now, cx, cy);
                last_c = None;
                last_q = None;
            }
            b'H' => {
                let Some(x) = p.number() else { break };
                cx = x + ox;
                add(&mut now, cx, cy);
                last_c = None;
                last_q = None;
            }
            b'V' => {
                let Some(y) = p.number() else { break };
                cy = y + oy;
                add(&mut now, cx, cy);
                last_c = None;
                last_q = None;
            }
            b'C' | b'S' => {
                let (x1, y1) = if upper == b'C' {
                    let (Some(a), Some(b)) = (p.number(), p.number()) else { break };
                    (a + ox, b + oy)
                } else {
                    last_c.map_or((cx, cy), |(lx, ly)| (2.0 * cx - lx, 2.0 * cy - ly))
                };
                let (Some(a), Some(b), Some(x), Some(y)) = (p.number(), p.number(), p.number(), p.number()) else {
                    break;
                };
                let (x2, y2, x3, y3) = (a + ox, b + oy, x + ox, y + oy);
                let len = (x1 - cx).hypot(y1 - cy) + (x2 - x1).hypot(y2 - y1) + (x3 - x2).hypot(y3 - y2);
                let n = pieces(len);
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    add(&mut now, b0 * cx + b1 * x1 + b2 * x2 + b3 * x3, b0 * cy + b1 * y1 + b2 * y2 + b3 * y3);
                }
                last_c = Some((x2, y2));
                last_q = None;
                (cx, cy) = (x3, y3);
            }
            b'Q' | b'T' => {
                let (x1, y1) = if upper == b'Q' {
                    let (Some(a), Some(b)) = (p.number(), p.number()) else { break };
                    (a + ox, b + oy)
                } else {
                    last_q.map_or((cx, cy), |(lx, ly)| (2.0 * cx - lx, 2.0 * cy - ly))
                };
                let (Some(x), Some(y)) = (p.number(), p.number()) else { break };
                let (x2, y2) = (x + ox, y + oy);
                let n = pieces((x1 - cx).hypot(y1 - cy) + (x2 - x1).hypot(y2 - y1));
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    add(
                        &mut now,
                        u * u * cx + 2.0 * u * t * x1 + t * t * x2,
                        u * u * cy + 2.0 * u * t * y1 + t * t * y2,
                    );
                }
                last_q = Some((x1, y1));
                last_c = None;
                (cx, cy) = (x2, y2);
            }
            b'A' => {
                let (Some(rx), Some(ry), Some(turn)) = (p.number(), p.number(), p.number()) else { break };
                let (Some(large), Some(sweep)) = (p.flag(), p.flag()) else { break };
                let (Some(x), Some(y)) = (p.number(), p.number()) else { break };
                let (x2, y2) = (x + ox, y + oy);
                for (ax, ay) in arc((cx, cy), (rx, ry), turn, large, sweep, (x2, y2), &pieces) {
                    add(&mut now, ax, ay);
                }
                (cx, cy) = (x2, y2);
                last_c = None;
                last_q = None;
            }
            _ => break,
        }
        if p.at == before {
            break;
        }
    }
    if now.len() > 1 {
        out.push(now);
    }
    out
}

/// The points along an elliptical arc (SVG's endpoint form) after its start, to its end.
fn arc(
    (x1, y1): (f32, f32),
    (rx, ry): (f32, f32),
    turn: f32,
    large: bool,
    sweep: bool,
    (x2, y2): (f32, f32),
    pieces: &dyn Fn(f32) -> usize,
) -> Vec<(f32, f32)> {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 || (x1 == x2 && y1 == y2) {
        return vec![(x2, y2)];
    }
    let (s, c) = turn.to_radians().sin_cos();
    let (dx, dy) = ((x1 - x2) / 2.0, (y1 - y2) / 2.0);
    let (x1p, y1p) = (c * dx + s * dy, -s * dx + c * dy);
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut k = (num / den).max(0.0).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cxp, cyp) = (k * rx * y1p / ry, -k * ry * x1p / rx);
    let (cx, cy) = (c * cxp - s * cyp + (x1 + x2) / 2.0, s * cxp + c * cyp + (y1 + y2) / 2.0);
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        if a.is_finite() { a } else { 0.0 }
    };
    let theta = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut delta = angle((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    let tau = std::f32::consts::TAU;
    if !sweep && delta > 0.0 {
        delta -= tau;
    } else if sweep && delta < 0.0 {
        delta += tau;
    }
    let n = pieces(delta.abs() * rx.max(ry));
    (1..=n)
        .map(|i| {
            let t = theta + delta * i as f32 / n as f32;
            let (st, ct) = t.sin_cos();
            (c * rx * ct - s * ry * st + cx, s * rx * ct + c * ry * st + cy)
        })
        .collect()
}

/// How much of each pixel of a picture `size` the closed outlines cover (0..1), by the non-zero
/// rule (or the even-odd): four rows of samples a pixel, each across exactly.
fn coverage(shapes: &[Vec<(f32, f32)>], (width, height): (u32, u32), even_odd: bool) -> Vec<f32> {
    const ROWS: u32 = 4;
    let mut cover = vec![0.0_f32; (width * height) as usize];
    // the edges, each from its top to its bottom with the way it went
    let mut edges: Vec<(f32, f32, f32, f32, i32)> = Vec::new();
    for shape in shapes {
        for i in 0..shape.len() {
            let (a, b) = (shape[i], shape[(i + 1) % shape.len()]);
            if !(a.0.is_finite() && a.1.is_finite() && b.0.is_finite() && b.1.is_finite()) || a.1 == b.1 {
                continue;
            }
            if a.1 < b.1 { edges.push((a.0, a.1, b.0, b.1, 1)) } else { edges.push((b.0, b.1, a.0, a.1, -1)) }
        }
    }
    edges.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut active: Vec<usize> = Vec::new();
    let mut next = 0;
    let mut crossings: Vec<(f32, i32)> = Vec::new();
    for row in 0..height * ROWS {
        let y = (row as f32 + 0.5) / ROWS as f32;
        while next < edges.len() && edges[next].1 <= y {
            active.push(next);
            next += 1;
        }
        active.retain(|&i| edges[i].3 > y);
        crossings.clear();
        for &i in &active {
            let (x0, y0, x1, y1, w) = edges[i];
            if y >= y0 && y < y1 {
                crossings.push((x0 + (y - y0) * (x1 - x0) / (y1 - y0), w));
            }
        }
        crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
        let line = (row / ROWS * width) as usize;
        let mut winding = 0;
        for pair in crossings.windows(2) {
            winding += pair[0].1;
            let inside = if even_odd { winding % 2 != 0 } else { winding != 0 };
            if !inside {
                continue;
            }
            let (a, b) = (pair[0].0.max(0.0), pair[1].0.min(width as f32));
            if b <= a {
                continue;
            }
            let share = 1.0 / ROWS as f32;
            let (ia, ib) = (a.floor() as usize, (b.ceil() as usize).min(width as usize));
            for px in ia..ib {
                let left = (px as f32).max(a);
                let right = (px as f32 + 1.0).min(b);
                if let Some(c) = cover.get_mut(line + px) {
                    *c += (right - left).max(0.0) * share;
                }
            }
        }
    }
    for c in &mut cover {
        *c = c.min(1.0);
    }
    cover
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(p: &Rgba, x: u32, y: u32) -> u8 {
        p.pixels[((y * p.width + x) * 4) as usize]
    }

    #[test]
    fn draws_filled_shapes_through_their_transforms_fitted_to_its_view_box() {
        let svg = br##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="5mm" viewBox="0 0 200 100">
            <g transform="translate(0,-50)"><path style="fill:#000000" d="m 10,60 80,0 0,80 -80,0 z"/></g>
            <rect x="110" y="10" width="80" height="80" fill="#808080"/>
            <circle cx="150" cy="50" r="10" fill="none"/>
            <path d="M100 0 L 110 0 L 110 10 Z" fill="black" display="none"/>
        </svg>"##;
        assert!(is_svg(svg));
        assert_eq!(size(svg), Some((1000, 500)));
        let p = render(svg).unwrap();
        assert_eq!((p.width, p.height), (1000, 500));
        // the black square on the left, the grey one on the right, white between and around
        assert_eq!(at(&p, 250, 250), 0);
        assert!((127..=129).contains(&at(&p, 750, 250)));
        assert_eq!(at(&p, 500, 250), 255);
        assert_eq!(at(&p, 10, 10), 255);
        // its edges antialiased: a pixel half in
        let edge =
            render(br#"<svg viewBox="0 0 1000 1000"><rect x="100.5" y="0" width="100" height="1000"/></svg>"#).unwrap();
        assert!((120..=135).contains(&at(&edge, 100, 500)), "{}", at(&edge, 100, 500));
    }

    #[test]
    fn reads_curves_arcs_and_the_even_odd_rule() {
        // a ring by even-odd: two circles as arcs, the hole white
        let ring = br#"<svg viewBox="0 0 100 100"><path fill-rule="evenodd" d="M50 10 A40 40 0 1 1 49.99 10 Z M50 30 a20 20 0 1 0 0.01 0 z"/></svg>"#;
        let p = render(ring).unwrap();
        assert_eq!(at(&p, 500, 500), 255, "the hole");
        assert_eq!(at(&p, 500, 150), 0, "the ring");
        assert_eq!(at(&p, 20, 20), 255);
        // a cubic bump
        let bump = br#"<svg viewBox="0 0 100 100"><path d="M0 100 C 0 0, 100 0, 100 100 z"/></svg>"#;
        let p = render(bump).unwrap();
        assert_eq!(at(&p, 500, 400), 0);
        assert_eq!(at(&p, 50, 300), 255);
        assert!(render(b"<svg/>").is_none());
    }
}
