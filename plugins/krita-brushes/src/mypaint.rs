//! Krita's MyPaint brushes (its `mypaintbrush` paint op): MyPaint brush files (`.myb`, and presets
//! keeping one) drawn as Krita draws them, through MyPaint's brush engine.
//!
//! The engine here (`Engine`: the brush's settings and inputs, its states and how a stroke becomes
//! dabs, their colour, smudge and shape) is a port of libmypaint's brush engine to Rust, which is
//! free software under the ISC licence:
//!
//! ```text
//! libmypaint - The MyPaint Brush Library
//! Copyright (C) 2007-2011 Martin Renold <martinxyz@gmx.ch>
//! Copyright (C) 2008-2011 Martin Renold and contributors
//!
//! Permission to use, copy, modify, and/or distribute this software for any
//! purpose with or without fee is hereby granted, provided that the above
//! copyright notice and this permission notice appear in all copies.
//!
//! THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
//! WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
//! MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
//! ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
//! WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
//! ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
//! OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
//! ```
//!
//! What Krita does around it (`Stroke`: where it hands the engine the pen, the size and colour it
//! sets, how it lays the engine's dabs and reads the colours under them) is this plugin's own code,
//! from reading how Krita works. Krita drives the engine through its older calls, so the engine
//! here is the older path only: its colours mixed as plain RGB, no spectral paint, no posterizing.
//!
//! The colours under a dab (for smudging) come from the app as near as the GPU allows: a batch of
//! samples late (see the app's guide, dab flag 16384). Until they come, a dab takes the newest it
//! was told; once they come, what the brush carries is worked out again from them.

use crate::engine::{Begin, Dab, Random, Sample, Under};
use crate::json::Value;

/// MyPaint's settings, by their names in its files, and their defaults.
pub const SETTINGS: [(&str, f32); 64] = [
    ("opaque", 1.0),
    ("opaque_multiply", 0.0),
    ("opaque_linearize", 0.9),
    ("radius_logarithmic", 2.0),
    ("hardness", 0.8),
    ("anti_aliasing", 1.0),
    ("dabs_per_basic_radius", 0.0),
    ("dabs_per_actual_radius", 2.0),
    ("dabs_per_second", 0.0),
    ("radius_by_random", 0.0),
    ("speed1_slowness", 0.04),
    ("speed2_slowness", 0.8),
    ("speed1_gamma", 4.0),
    ("speed2_gamma", 4.0),
    ("offset_by_random", 0.0),
    ("offset_by_speed", 0.0),
    ("offset_by_speed_slowness", 1.0),
    ("slow_tracking", 0.0),
    ("slow_tracking_per_dab", 0.0),
    ("tracking_noise", 0.0),
    ("color_h", 0.0),
    ("color_s", 0.0),
    ("color_v", 0.0),
    ("restore_color", 0.0),
    ("change_color_h", 0.0),
    ("change_color_l", 0.0),
    ("change_color_hsl_s", 0.0),
    ("change_color_v", 0.0),
    ("change_color_hsv_s", 0.0),
    ("smudge", 0.0),
    ("smudge_length", 0.5),
    ("smudge_radius_log", 0.0),
    ("eraser", 0.0),
    ("stroke_threshold", 0.0),
    ("stroke_duration_logarithmic", 4.0),
    ("stroke_holdtime", 0.0),
    ("custom_input", 0.0),
    ("custom_input_slowness", 0.0),
    ("elliptical_dab_ratio", 1.0),
    ("elliptical_dab_angle", 90.0),
    ("direction_filter", 2.0),
    ("lock_alpha", 0.0),
    ("colorize", 0.0),
    ("snap_to_pixel", 0.0),
    ("pressure_gain_log", 0.0),
    ("gridmap_scale", 0.0),
    ("gridmap_scale_x", 1.0),
    ("gridmap_scale_y", 1.0),
    ("smudge_length_log", 0.0),
    ("smudge_bucket", 0.0),
    ("smudge_transparency", 0.0),
    ("offset_y", 0.0),
    ("offset_x", 0.0),
    ("offset_angle", 0.0),
    ("offset_angle_asc", 0.0),
    ("offset_angle_view", 0.0),
    ("offset_angle_2", 0.0),
    ("offset_angle_2_asc", 0.0),
    ("offset_angle_2_view", 0.0),
    ("offset_angle_adj", 0.0),
    ("offset_multiplier", 0.0),
    ("posterize", 0.0),
    ("posterize_num", 0.05),
    ("paint_mode", 1.0),
];

/// Each setting's range, as MyPaint's settings list gives it.
pub const RANGES: [(f32, f32); 64] = [
    (0.0, 2.0),
    (0.0, 2.0),
    (0.0, 2.0),
    (-2.0, 6.0),
    (0.0, 1.0),
    (0.0, 5.0),
    (0.0, 200.0),
    (0.0, 200.0),
    (0.0, 200.0),
    (0.0, 1.5),
    (0.0, 0.2),
    (0.0, 3.0),
    (-8.0, 8.0),
    (-8.0, 8.0),
    (0.0, 25.0),
    (-3.0, 3.0),
    (0.0, 15.0),
    (0.0, 10.0),
    (0.0, 10.0),
    (0.0, 12.0),
    (0.0, 1.0),
    (-0.5, 1.5),
    (-0.5, 1.5),
    (0.0, 1.0),
    (-2.0, 2.0),
    (-2.0, 2.0),
    (-2.0, 2.0),
    (-2.0, 2.0),
    (-2.0, 2.0),
    (0.0, 1.0),
    (0.0, 1.0),
    (-1.6, 1.6),
    (0.0, 1.0),
    (0.0, 0.5),
    (-1.0, 14.0),
    (0.0, 10.0),
    (-5.0, 5.0),
    (0.0, 10.0),
    (1.0, 10.0),
    (0.0, 180.0),
    (0.0, 10.0),
    (0.0, 1.0),
    (0.0, 1.0),
    (0.0, 1.0),
    (-1.8, 1.8),
    (-10.0, 10.0),
    (0.0, 10.0),
    (0.0, 10.0),
    (0.0, 20.0),
    (0.0, 255.0),
    (-1.0, 1.0),
    (-40.0, 40.0),
    (-40.0, 40.0),
    (-40.0, 40.0),
    (-40.0, 40.0),
    (-40.0, 40.0),
    (0.0, 40.0),
    (0.0, 40.0),
    (0.0, 40.0),
    (-180.0, 180.0),
    (-2.0, 3.0),
    (0.0, 1.0),
    (0.01, 1.28),
    (0.0, 1.0),
];

/// The inputs settings follow, by their names in MyPaint's files.
pub const INPUTS: [&str; 18] = [
    "pressure",
    "speed1",
    "speed2",
    "random",
    "stroke",
    "direction",
    "tilt_declination",
    "tilt_ascension",
    "custom",
    "direction_angle",
    "attack_angle",
    "tilt_declinationx",
    "tilt_declinationy",
    "gridmap_x",
    "gridmap_y",
    "viewzoom",
    "barrel_rotation",
    "brush_radius",
];

// the settings and inputs by their places
const OPAQUE: usize = 0;
const OPAQUE_MULTIPLY: usize = 1;
const OPAQUE_LINEARIZE: usize = 2;
pub const RADIUS_LOGARITHMIC: usize = 3;
pub const HARDNESS: usize = 4;
const ANTI_ALIASING: usize = 5;
const DABS_PER_BASIC_RADIUS: usize = 6;
const DABS_PER_ACTUAL_RADIUS: usize = 7;
const DABS_PER_SECOND: usize = 8;
const RADIUS_BY_RANDOM: usize = 9;
const SPEED1_SLOWNESS: usize = 10;
const SPEED2_SLOWNESS: usize = 11;
const SPEED1_GAMMA: usize = 12;
const SPEED2_GAMMA: usize = 13;
const OFFSET_BY_RANDOM: usize = 14;
const OFFSET_BY_SPEED: usize = 15;
const OFFSET_BY_SPEED_SLOWNESS: usize = 16;
const SLOW_TRACKING: usize = 17;
const SLOW_TRACKING_PER_DAB: usize = 18;
const TRACKING_NOISE: usize = 19;
const COLOR_H: usize = 20;
const COLOR_S: usize = 21;
const COLOR_V: usize = 22;
const CHANGE_COLOR_H: usize = 24;
const CHANGE_COLOR_L: usize = 25;
const CHANGE_COLOR_HSL_S: usize = 26;
const CHANGE_COLOR_V: usize = 27;
const CHANGE_COLOR_HSV_S: usize = 28;
pub const SMUDGE: usize = 29;
pub const SMUDGE_LENGTH: usize = 30;
const SMUDGE_RADIUS_LOG: usize = 31;
pub const ERASER: usize = 32;
const STROKE_THRESHOLD: usize = 33;
const STROKE_DURATION_LOGARITHMIC: usize = 34;
const STROKE_HOLDTIME: usize = 35;
const CUSTOM_INPUT: usize = 36;
const CUSTOM_INPUT_SLOWNESS: usize = 37;
const ELLIPTICAL_DAB_RATIO: usize = 38;
const ELLIPTICAL_DAB_ANGLE: usize = 39;
const DIRECTION_FILTER: usize = 40;
const COLORIZE: usize = 42;
const SNAP_TO_PIXEL: usize = 43;
const PRESSURE_GAIN_LOG: usize = 44;
const GRIDMAP_SCALE: usize = 45;
const GRIDMAP_SCALE_X: usize = 46;
const GRIDMAP_SCALE_Y: usize = 47;
const SMUDGE_LENGTH_LOG: usize = 48;
const SMUDGE_TRANSPARENCY: usize = 50;
const OFFSET_Y: usize = 51;
const OFFSET_X: usize = 52;
const OFFSET_ANGLE: usize = 53;
const OFFSET_ANGLE_ASC: usize = 54;
const OFFSET_ANGLE_VIEW: usize = 55;
const OFFSET_ANGLE_2: usize = 56;
const OFFSET_ANGLE_2_ASC: usize = 57;
const OFFSET_ANGLE_2_VIEW: usize = 58;
const OFFSET_ANGLE_ADJ: usize = 59;
const OFFSET_MULTIPLIER: usize = 60;

const PRESSURE: usize = 0;
const SPEED1: usize = 1;
const SPEED2: usize = 2;
const RANDOM: usize = 3;
const STROKE: usize = 4;
const DIRECTION: usize = 5;
const TILT_DECLINATION: usize = 6;
const TILT_ASCENSION: usize = 7;
const CUSTOM: usize = 8;
const DIRECTION_ANGLE: usize = 9;
const ATTACK_ANGLE: usize = 10;
const TILT_DECLINATIONX: usize = 11;
const TILT_DECLINATIONY: usize = 12;
const GRIDMAP_X: usize = 13;
const GRIDMAP_Y: usize = 14;
const VIEWZOOM: usize = 15;
const BARREL_ROTATION: usize = 16;
const BRUSH_RADIUS: usize = 17;

const ACTUAL_RADIUS_MIN: f32 = 0.2;
const ACTUAL_RADIUS_MAX: f32 = 1000.0;
const GRID_SIZE: f32 = 256.0;

/// A setting: its base value and the curve each input it follows adds (points across and up, in
/// order across).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mapping {
    pub base: f32,
    pub curves: Vec<(usize, Vec<(f32, f32)>)>,
}

impl Mapping {
    pub fn constant(&self) -> bool {
        self.curves.is_empty()
    }

    fn value(&self, inputs: &[f32; 18]) -> f32 {
        let mut result = self.base;
        for (input, p) in &self.curves {
            let (Some(&x), true) = (inputs.get(*input), p.len() >= 2) else { continue };
            let ((mut x0, mut y0), (mut x1, mut y1)) = (p[0], p[1]);
            let mut i = 2;
            while i < p.len() && x > x1 {
                (x0, y0) = (x1, y1);
                (x1, y1) = p[i];
                i += 1;
            }
            result += if x0 == x1 || y0 == y1 { y0 } else { (y1 * (x - x0) + y0 * (x1 - x)) / (x1 - x0) };
        }
        result
    }
}

/// A MyPaint brush's settings.
#[derive(Clone, Debug, PartialEq)]
pub struct MyPaint {
    pub map: Vec<Mapping>,
}

impl MyPaint {
    /// MyPaint's defaults: each setting's base, the opacity following the pressure.
    pub fn defaults() -> MyPaint {
        let mut map: Vec<Mapping> = SETTINGS.iter().map(|&(_, base)| Mapping { base, curves: Vec::new() }).collect();
        map[OPAQUE_MULTIPLY].curves.push((PRESSURE, vec![(0.0, 0.0), (1.0, 1.0)]));
        MyPaint { map }
    }

    /// The brush a MyPaint file's text holds, as MyPaint reads it: its settings over the defaults
    /// (none of them if it isn't a file of version 3); none if it isn't JSON.
    pub fn read(text: &str) -> Option<MyPaint> {
        let json = crate::json::parse(text.trim_start_matches('\u{feff}'))?;
        let mut brush = MyPaint::defaults();
        if json.get("version").and_then(Value::number).map(|v| v as i64) != Some(3) {
            return Some(brush);
        }
        for (name, setting) in json.get("settings").map(Value::entries).unwrap_or_default() {
            let Some(index) = SETTINGS.iter().position(|(n, _)| n == name) else { continue };
            let Some(base) = setting.get("base_value").and_then(Value::number) else { continue };
            let Some(inputs) = setting.get("inputs") else { continue };
            let m = &mut brush.map[index];
            m.base = base as f32;
            for (input, points) in inputs.entries() {
                let Some(input) = INPUTS.iter().position(|n| n == input) else { break };
                let points: Vec<(f32, f32)> = points
                    .list()
                    .iter()
                    .filter_map(|p| Some((p.list().first()?.number()? as f32, p.list().get(1)?.number()? as f32)))
                    .take(64)
                    .collect();
                m.curves.retain(|(i, _)| *i != input);
                // a curve needs two points, in order across
                if points.len() >= 2 && points.windows(2).all(|w| w[1].0 >= w[0].0) {
                    m.curves.push((input, points));
                }
            }
        }
        Some(brush)
    }

    pub fn base(&self, setting: usize) -> f32 {
        self.map.get(setting).map_or(0.0, |m| m.base)
    }

    /// Whether its dabs smudge (pick up the colours under them), as MyPaint asks for them.
    pub fn smudges(&self) -> bool {
        let smudge = &self.map[SMUDGE];
        smudge.base != 0.0 || !smudge.constant()
    }

    /// Whether Krita takes it as an eraser (its eraser setting rounding to 1).
    pub fn erases(&self) -> bool {
        self.base(ERASER).round() != 0.0
    }
}

/// What the brush carries for smudging: its colour (premultiplied) and alpha, the colours it last
/// picked up (premultiplied) and how recently.
const BUCKET: usize = 9;
const SMUDGE_R: usize = 0;
const SMUDGE_A: usize = 3;
const PREV_R: usize = 4;
const PREV_A: usize = 7;
const RECENTNESS: usize = 8;

/// The engine's states (MyPaint's own, by its names).
#[derive(Clone, Copy, Debug, Default)]
struct States {
    x: f32,
    y: f32,
    pressure: f32,
    partial_dabs: f32,
    actual_radius: f32,
    actual_x: f32,
    actual_y: f32,
    norm_dx_slow: f32,
    norm_dy_slow: f32,
    norm_speed1_slow: f32,
    norm_speed2_slow: f32,
    stroke: f32,
    stroke_started: f32,
    custom_input: f32,
    actual_elliptical_dab_ratio: f32,
    actual_elliptical_dab_angle: f32,
    direction_dx: f32,
    direction_dy: f32,
    declination: f32,
    ascension: f32,
    viewzoom: f32,
    viewrotation: f32,
    direction_angle_dx: f32,
    direction_angle_dy: f32,
    flip: f32,
    gridmap_x: f32,
    gridmap_y: f32,
    declinationx: f32,
    declinationy: f32,
    dabs_per_basic_radius: f32,
    dabs_per_actual_radius: f32,
    dabs_per_second: f32,
    barrel_rotation: f32,
}

fn exp_decay(t_const: f32, t: f32) -> f32 {
    if t_const <= 0.001 { 0.0 } else { (-t / t_const).exp() }
}

fn mod_arith(a: f32, n: f32) -> f32 {
    a - n * (a / n).floor()
}

fn smallest_angular_difference(a: f32, b: f32) -> f32 {
    let mut d = mod_arith(b - a + 180.0, 360.0) - 180.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    d
}

/// RGB (each 0..1) to hue, saturation and value, as MyPaint takes them.
fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = [r, g, b].map(|c| c.clamp(0.0, 1.0));
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let delta = max - min;
    if delta <= 0.0001 {
        return [0.0, 0.0, max];
    }
    let mut h = if r == max {
        let h = (g - b) / delta;
        if h < 0.0 { h + 6.0 } else { h }
    } else if g == max {
        2.0 + (b - r) / delta
    } else {
        4.0 + (r - g) / delta
    };
    h /= 6.0;
    [h, delta / max, max]
}

fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let h = h - h.floor();
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    if s == 0.0 {
        return [v; 3];
    }
    let hue = if h == 1.0 { 0.0 } else { f64::from(h) * 6.0 };
    let i = hue as i32;
    let f = hue - f64::from(i);
    let (v64, s64) = (f64::from(v), f64::from(s));
    let w = (v64 * (1.0 - s64)) as f32;
    let q = (v64 * (1.0 - s64 * f)) as f32;
    let t = (v64 * (1.0 - s64 * (1.0 - f))) as f32;
    match i {
        0 => [v, t, w],
        1 => [q, v, w],
        2 => [w, v, t],
        3 => [w, q, v],
        4 => [t, w, v],
        _ => [v, w, q],
    }
}

fn rgb_to_hsl([r, g, b]: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = [r, g, b].map(|c| f64::from(c.clamp(0.0, 1.0)));
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min {
        return [0.0, 0.0, l as f32];
    }
    let s = if l <= 0.5 { (max - min) / (max + min) } else { (max - min) / (2.0 - max - min) };
    let delta = max - min;
    let mut h = if r == max {
        (g - b) / delta
    } else if g == max {
        2.0 + (b - r) / delta
    } else {
        4.0 + (r - g) / delta
    };
    h /= 6.0;
    if h < 0.0 {
        h += 1.0;
    }
    [h as f32, s as f32, l as f32]
}

fn hsl_to_rgb([h, s, l]: [f32; 3]) -> [f32; 3] {
    let (h, s, l) = (f64::from(h - h.floor()), f64::from(s.clamp(0.0, 1.0)), f64::from(l.clamp(0.0, 1.0)));
    if s == 0.0 {
        return [l as f32; 3];
    }
    let m2 = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let m1 = 2.0 * l - m2;
    let value = |hue: f64| {
        let hue = if hue > 6.0 {
            hue - 6.0
        } else if hue < 0.0 {
            hue + 6.0
        } else {
            hue
        };
        if hue < 1.0 {
            m1 + (m2 - m1) * hue
        } else if hue < 3.0 {
            m2
        } else if hue < 4.0 {
            m1 + (m2 - m1) * (4.0 - hue)
        } else {
            m1
        }
    };
    [value(h * 6.0 + 2.0) as f32, value(h * 6.0) as f32, value(h * 6.0 - 2.0) as f32]
}

/// A dab as the engine draws it (MyPaint's surface's): its middle, radius, colour, opacity,
/// hardness, the alpha it takes what is there towards, its squash (across over along), turn
/// (degrees) and colorizing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawn {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub colour: [f32; 3],
    pub opaque: f32,
    pub hardness: f32,
    pub alpha: f32,
    pub aspect: f32,
    pub angle: f32,
    pub colorize: f32,
}

/// Where the engine lays its dabs and picks colours up from.
pub trait Surface {
    /// The colours under a disc (premultiplied red, green and blue, and alpha), as near as known.
    fn colours(&mut self, x: f32, y: f32, radius: f32) -> [f32; 4];
    fn draw(&mut self, dab: Drawn);
}

/// A smudge's update of what the brush carries, kept until the colours it took are told.
#[derive(Clone, Copy, Debug)]
struct Update {
    length: f32,
    length_log: f32,
    /// It picked colours up (the nth asked about), rather than taking the last again.
    picked: bool,
}

/// MyPaint's brush engine (see the module's notes): a brush's settings, as a stroke goes.
pub struct Engine {
    pub brush: MyPaint,
    s: States,
    bucket: [f32; BUCKET],
    values: [f32; 64],
    speed_gamma: [f32; 2],
    speed_m: [f32; 2],
    speed_q: [f32; 2],
    random_input: f32,
    skip: f32,
    skip_last: (f32, f32),
    skipped_dtime: f64,
    rng: Random,
    reset_requested: bool,
    /// What the brush carried before the updates not yet told, and those updates.
    confirmed: [f32; BUCKET],
    updates: Vec<Update>,
}

/// The most smudge updates kept waiting for colours (past that, what was guessed stands).
const MOST_UPDATES: usize = 8192;

impl Engine {
    pub fn new(brush: MyPaint, seed: u32) -> Engine {
        let mut e = Engine {
            brush,
            s: States::default(),
            bucket: [0.0; BUCKET],
            values: [0.0; 64],
            speed_gamma: [0.0; 2],
            speed_m: [0.0; 2],
            speed_q: [0.0; 2],
            random_input: 0.0,
            skip: 0.0,
            skip_last: (0.0, 0.0),
            skipped_dtime: 0.0,
            rng: Random::new(seed ^ 0x6d79_7061),
            reset_requested: true,
            confirmed: [0.0; BUCKET],
            updates: Vec::new(),
        };
        e.reset();
        e.base_values_changed();
        e
    }

    fn reset(&mut self) {
        self.skip = 0.0;
        self.skip_last = (0.0, 0.0);
        self.skipped_dtime = 0.0;
        self.s = States { flip: -1.0, ..States::default() };
        self.bucket = [0.0; BUCKET];
        self.confirmed = [0.0; BUCKET];
        self.updates.clear();
    }

    fn base(&self, setting: usize) -> f32 {
        self.brush.base(setting)
    }

    fn value(&self, setting: usize) -> f32 {
        self.values.get(setting).copied().unwrap_or(0.0)
    }

    pub fn set_base(&mut self, setting: usize, value: f32) {
        if let Some(m) = self.brush.map.get_mut(setting) {
            m.base = value;
        }
        self.base_values_changed();
    }

    fn base_values_changed(&mut self) {
        for i in 0..2 {
            let gamma = (if i == 0 { self.base(SPEED1_GAMMA) } else { self.base(SPEED2_GAMMA) }).exp();
            let (fix1_x, fix1_y, fix2_x, fix2_dy) = (45.0_f32, 0.5_f32, 45.0_f32, 0.015_f32);
            let c1 = (fix1_x + gamma).ln();
            let m = fix2_dy * (fix2_x + gamma);
            let q = fix1_y - m * c1;
            self.speed_gamma[i] = gamma;
            self.speed_m[i] = m;
            self.speed_q[i] = q;
        }
    }

    /// Whether a stroke has started (the pressure past its threshold).
    pub fn started(&self) -> bool {
        self.s.stroke_started != 0.0
    }

    fn gauss(&mut self) -> f32 {
        let sum: f32 = (0..4).map(|_| self.rng.next()).sum();
        sum * 1.732_050_8 - 3.464_101_6
    }

    fn update_states(&mut self, step: [f32; 10], viewzoom: f32, viewrotation: f32) {
        let [
            step_ddab,
            step_dx,
            step_dy,
            step_dpressure,
            step_declination,
            step_ascension,
            mut step_dtime,
            step_declinationx,
            step_declinationy,
            step_barrel,
        ] = step;
        if step_dtime <= 0.0 {
            step_dtime = 0.001;
        }
        let s = &mut self.s;
        s.x += step_dx;
        s.y += step_dy;
        s.pressure += step_dpressure;
        s.dabs_per_basic_radius = self.values[DABS_PER_BASIC_RADIUS];
        s.dabs_per_actual_radius = self.values[DABS_PER_ACTUAL_RADIUS];
        s.dabs_per_second = self.values[DABS_PER_SECOND];
        s.declination += step_declination;
        s.ascension += step_ascension;
        s.declinationx += step_declinationx;
        s.declinationy += step_declinationy;
        s.viewzoom = viewzoom;
        let viewrotation = mod_arith(viewrotation.to_degrees() + 180.0, 360.0) - 180.0;
        s.viewrotation = viewrotation;
        {
            let (x, y) = (s.actual_x, s.actual_y);
            let scale = self.values[GRIDMAP_SCALE].exp();
            let (scale_x, scale_y) = (self.values[GRIDMAP_SCALE_X], self.values[GRIDMAP_SCALE_Y]);
            let scaled = scale * GRID_SIZE;
            s.gridmap_x = mod_arith((x * scale_x).abs(), scaled) / scaled * GRID_SIZE;
            s.gridmap_y = mod_arith((y * scale_y).abs(), scaled) / scaled * GRID_SIZE;
            if x < 0.0 {
                s.gridmap_x = GRID_SIZE - s.gridmap_x;
            }
            if y < 0.0 {
                s.gridmap_y = GRID_SIZE - s.gridmap_y;
            }
        }
        let base_radius = self.brush.base(RADIUS_LOGARITHMIC).exp();
        s.barrel_rotation += step_barrel;
        if s.pressure <= 0.0 {
            s.pressure = 0.0;
        }
        let pressure = s.pressure;
        {
            let lim = 0.0001;
            let threshold = self.brush.base(STROKE_THRESHOLD);
            let started = s.stroke_started != 0.0;
            if !started && pressure > threshold + lim {
                s.stroke_started = 1.0;
                s.stroke = 0.0;
            } else if started && pressure <= threshold * 0.9 + lim {
                s.stroke_started = 0.0;
            }
        }
        let norm_dx = step_dx / step_dtime * s.viewzoom;
        let norm_dy = step_dy / step_dtime * s.viewzoom;
        let norm_speed = norm_dx.hypot(norm_dy);
        let norm_dist = (step_dx / step_dtime / base_radius).hypot(step_dy / step_dtime / base_radius) * step_dtime;

        let mut inputs = [0.0_f32; 18];
        inputs[PRESSURE] = pressure * self.brush.base(PRESSURE_GAIN_LOG).exp();
        inputs[SPEED1] = (self.speed_gamma[0] + s.norm_speed1_slow).ln() * self.speed_m[0] + self.speed_q[0];
        inputs[SPEED2] = (self.speed_gamma[1] + s.norm_speed2_slow).ln() * self.speed_m[1] + self.speed_q[1];
        inputs[RANDOM] = self.random_input;
        inputs[STROKE] = s.stroke.min(1.0);
        let dir_angle = s.direction_dy.atan2(s.direction_dx);
        inputs[DIRECTION] = mod_arith(dir_angle.to_degrees() + viewrotation + 180.0, 180.0);
        let dir_angle_360 = s.direction_angle_dy.atan2(s.direction_angle_dx);
        inputs[DIRECTION_ANGLE] = (dir_angle_360.to_degrees() + viewrotation + 360.0) % 360.0;
        inputs[TILT_DECLINATION] = s.declination;
        inputs[TILT_ASCENSION] = mod_arith(s.ascension + viewrotation + 180.0, 360.0) - 180.0;
        inputs[VIEWZOOM] = self.brush.base(RADIUS_LOGARITHMIC) - (base_radius / s.viewzoom).ln();
        inputs[ATTACK_ANGLE] =
            smallest_angular_difference(s.ascension, mod_arith(dir_angle_360.to_degrees() + 90.0, 360.0));
        inputs[BRUSH_RADIUS] = self.brush.base(RADIUS_LOGARITHMIC);
        inputs[GRIDMAP_X] = s.gridmap_x.clamp(0.0, GRID_SIZE);
        inputs[GRIDMAP_Y] = s.gridmap_y.clamp(0.0, GRID_SIZE);
        inputs[TILT_DECLINATIONX] = s.declinationx;
        inputs[TILT_DECLINATIONY] = s.declinationy;
        inputs[CUSTOM] = s.custom_input;
        inputs[BARREL_ROTATION] = mod_arith(s.barrel_rotation, 360.0);

        for (v, m) in self.values.iter_mut().zip(&self.brush.map) {
            *v = m.value(&inputs);
        }
        let values = &self.values;
        {
            let fac = 1.0 - exp_decay(values[SLOW_TRACKING_PER_DAB], step_ddab);
            s.actual_x += (s.x - s.actual_x) * fac;
            s.actual_y += (s.y - s.actual_y) * fac;
        }
        {
            let fac1 = 1.0 - exp_decay(values[SPEED1_SLOWNESS], step_dtime);
            s.norm_speed1_slow += (norm_speed - s.norm_speed1_slow) * fac1;
            let fac2 = 1.0 - exp_decay(values[SPEED2_SLOWNESS], step_dtime);
            s.norm_speed2_slow += (norm_speed - s.norm_speed2_slow) * fac2;
        }
        {
            let time_constant = ((values[OFFSET_BY_SPEED_SLOWNESS] * 0.01).exp() - 1.0).max(0.002);
            let fac = 1.0 - exp_decay(time_constant, step_dtime);
            s.norm_dx_slow += (norm_dx - s.norm_dx_slow) * fac;
            s.norm_dy_slow += (norm_dy - s.norm_dy_slow) * fac;
        }
        {
            let (mut dx, mut dy) = (step_dx * s.viewzoom, step_dy * s.viewzoom);
            let step_in_dabtime = dx.hypot(dy);
            let fac = 1.0 - exp_decay((values[DIRECTION_FILTER] * 0.5).exp() - 1.0, step_in_dabtime);
            let (dx_old, dy_old) = (s.direction_dx, s.direction_dy);
            s.direction_angle_dx += (dx - s.direction_angle_dx) * fac;
            s.direction_angle_dy += (dy - s.direction_angle_dy) * fac;
            // the opposite way if it is nearer (a half turn doesn't count)
            if (dx_old - dx).powi(2) + (dy_old - dy).powi(2) > (dx_old + dx).powi(2) + (dy_old + dy).powi(2) {
                dx = -dx;
                dy = -dy;
            }
            s.direction_dx += (dx - s.direction_dx) * fac;
            s.direction_dy += (dy - s.direction_dy) * fac;
        }
        {
            let fac = 1.0 - exp_decay(values[CUSTOM_INPUT_SLOWNESS], 0.1);
            s.custom_input += (values[CUSTOM_INPUT] - s.custom_input) * fac;
        }
        {
            let frequency = (-values[STROKE_DURATION_LOGARITHMIC]).exp();
            let stroke = (s.stroke + norm_dist * frequency).max(0.0);
            let wrap = 1.0 + values[STROKE_HOLDTIME].max(0.0);
            s.stroke = if stroke >= wrap && wrap > 9.9 + 1.0 {
                1.0
            } else if stroke >= wrap {
                stroke % wrap
            } else {
                stroke
            };
        }
        s.actual_radius = values[RADIUS_LOGARITHMIC].exp().clamp(ACTUAL_RADIUS_MIN, ACTUAL_RADIUS_MAX);
        s.actual_elliptical_dab_ratio = values[ELLIPTICAL_DAB_RATIO];
        s.actual_elliptical_dab_angle = mod_arith(values[ELLIPTICAL_DAB_ANGLE] - viewrotation + 180.0, 180.0) - 180.0;
    }

    fn directional_offsets(&self, base_radius: f32, flip: f32) -> (f32, f32) {
        let v = &self.values;
        let mult = v[OFFSET_MULTIPLIER].exp();
        if !mult.is_finite() {
            return (0.0, 0.0);
        }
        let (mut dx, mut dy) = (v[OFFSET_X], v[OFFSET_Y]);
        let adj = v[OFFSET_ANGLE_ADJ];
        let angle_deg = (self.s.direction_angle_dy.atan2(self.s.direction_angle_dx).to_degrees() - 90.0) % 360.0;
        let view = self.s.viewrotation;
        let mut add = |angle: f32, by: f32| {
            dx += angle.cos() * by;
            dy += angle.sin() * by;
        };
        if v[OFFSET_ANGLE] != 0.0 {
            add((angle_deg + adj).to_radians(), v[OFFSET_ANGLE]);
        }
        if v[OFFSET_ANGLE_ASC] != 0.0 {
            add((self.s.ascension - view + adj).to_radians(), v[OFFSET_ANGLE_ASC]);
        }
        if v[OFFSET_ANGLE_VIEW] != 0.0 {
            add(-(view + adj).to_radians(), v[OFFSET_ANGLE_VIEW]);
        }
        let mirrored = v[OFFSET_ANGLE_2].max(0.0);
        if mirrored != 0.0 {
            add((angle_deg + adj * flip).to_radians(), mirrored * flip);
        }
        let mirrored = v[OFFSET_ANGLE_2_ASC].max(0.0);
        if mirrored != 0.0 {
            add((self.s.ascension - view + adj * flip).to_radians(), mirrored * flip);
        }
        let mirrored = v[OFFSET_ANGLE_2_VIEW].max(0.0);
        if mirrored != 0.0 {
            add(-(view + adj).to_radians(), mirrored * flip);
        }
        let lim = 3240.0;
        let by = base_radius * mult;
        ((dx * by).clamp(-lim, lim), (dy * by).clamp(-lim, lim))
    }

    /// What the brush carries updated by the colours under a dab (picked up, or the last again):
    /// false if the dab isn't to be drawn (too little picked up, as its transparency says).
    fn update_smudge(&mut self, surface: &mut dyn Surface, (px, py): (f32, f32), radius: f32) -> bool {
        let length = self.value(SMUDGE_LENGTH);
        let length_log = self.value(SMUDGE_LENGTH_LOG);
        let recentness = self.bucket[RECENTNESS] * length.max(0.01);
        let picks = recentness < ((0.5 * length.max(0.01)).powf(length_log) + 1e-16).min(1.0);
        let mut picked = None;
        if picks {
            let smudge_radius =
                (radius * self.value(SMUDGE_RADIUS_LOG).exp()).clamp(ACTUAL_RADIUS_MIN, ACTUAL_RADIUS_MAX);
            let c = surface.colours(px, py, smudge_radius);
            let lim = self.value(SMUDGE_TRANSPARENCY);
            if (lim > 0.0 && c[3] < lim) || (lim < 0.0 && c[3] > -lim) {
                // (the bucket's recentness is as MyPaint leaves it then)
                self.bucket[RECENTNESS] = 1.0;
                return false;
            }
            picked = Some(c);
        }
        let update = Update { length, length_log, picked: picks };
        apply_update(&mut self.bucket, &update, picked);
        if self.updates.len() >= MOST_UPDATES {
            self.confirmed = self.bucket;
            self.updates.clear();
        } else {
            self.updates.push(update);
        }
        true
    }

    /// The colours under the dabs it asked about since, told: what the brush carries worked out
    /// again from them, from what it carried before them (`told` premultiplied, in order); the
    /// updates after the last told guessed again from it. How many of `told` it used.
    pub fn told(&mut self, told: &[[f32; 4]]) -> usize {
        let mut bucket = self.confirmed;
        let mut used = 0;
        let mut kept = 0;
        for (k, u) in self.updates.iter().enumerate() {
            if u.picked {
                let Some(c) = told.get(used) else { break };
                used += 1;
                apply_update(&mut bucket, u, Some(*c));
            } else {
                apply_update(&mut bucket, u, None);
            }
            kept = k + 1;
        }
        self.confirmed = bucket;
        self.updates.drain(..kept);
        // what is still to be told guessed from the newest told
        let newest = told.get(used.wrapping_sub(1)).copied();
        for u in &self.updates {
            apply_update(&mut bucket, u, if u.picked { newest } else { None });
        }
        self.bucket = bucket;
        used
    }

    /// Mixes what the brush carries into the colour (RGB, changed in place): the alpha the dab takes
    /// what is there towards.
    fn apply_smudge(&self, colour: &mut [f32; 3]) -> f32 {
        let factor = self.value(SMUDGE).min(1.0);
        let alpha = ((1.0 - factor) + factor * self.bucket[SMUDGE_A]).clamp(0.0, 1.0);
        if alpha > 0.0 {
            for (i, c) in colour.iter_mut().enumerate() {
                *c = (factor * self.bucket[SMUDGE_R + i] + (1.0 - factor) * *c) / alpha;
            }
        } else {
            *colour = [1.0, 0.0, 0.0];
        }
        alpha
    }

    /// Lays a dab where the brush is now (MyPaint's `prepare_and_draw_dab`, older path): whether it
    /// did.
    fn draw_dab(&mut self, surface: &mut dyn Surface) -> bool {
        let opaque_fac = self.value(OPAQUE_MULTIPLY);
        let mut opaque = (self.value(OPAQUE).max(0.0) * opaque_fac).clamp(0.0, 1.0);
        let linearize = self.base(OPAQUE_LINEARIZE);
        if linearize != 0.0 {
            let mut per_pixel = (self.base(DABS_PER_ACTUAL_RADIUS) + self.base(DABS_PER_BASIC_RADIUS)) * 2.0;
            per_pixel = per_pixel.max(1.0);
            per_pixel = 1.0 + linearize * (per_pixel - 1.0);
            let beta = 1.0 - opaque;
            opaque = 1.0 - beta.powf(1.0 / per_pixel);
        }
        let (mut x, mut y) = (self.s.actual_x, self.s.actual_y);
        let base_radius = self.base(RADIUS_LOGARITHMIC).exp();
        let (ox, oy) = self.directional_offsets(base_radius, self.s.flip);
        x += ox;
        y += oy;
        let by_speed = self.value(OFFSET_BY_SPEED);
        if by_speed != 0.0 {
            x += self.s.norm_dx_slow * by_speed * 0.1 / self.s.viewzoom;
            y += self.s.norm_dy_slow * by_speed * 0.1 / self.s.viewzoom;
        }
        let by_random = self.value(OFFSET_BY_RANDOM);
        if by_random != 0.0 {
            let amp = by_random.max(0.0);
            x += self.gauss() * amp * base_radius;
            y += self.gauss() * amp * base_radius;
        }
        let mut radius = self.s.actual_radius;
        let radius_by_random = self.value(RADIUS_BY_RANDOM);
        if radius_by_random != 0.0 {
            let noise = self.gauss() * radius_by_random;
            radius = (self.value(RADIUS_LOGARITHMIC) + noise).exp().clamp(ACTUAL_RADIUS_MIN, ACTUAL_RADIUS_MAX);
            let correction = (self.s.actual_radius / radius).powi(2);
            if correction <= 1.0 {
                opaque *= correction;
            }
        }
        let mut colour = hsv_to_rgb([self.base(COLOR_H), self.base(COLOR_S), self.base(COLOR_V)]);
        let length = self.value(SMUDGE_LENGTH);
        if length < 1.0 && self.brush.smudges() {
            let at = ((x + 0.5).floor(), (y + 0.5).floor());
            if !self.update_smudge(surface, at, radius) {
                return false;
            }
        }
        let mut alpha = 1.0;
        if self.value(SMUDGE) > 0.0 {
            alpha = self.apply_smudge(&mut colour);
        }
        let eraser = self.value(ERASER);
        if eraser != 0.0 {
            alpha *= 1.0 - eraser;
        }
        let hsv_changes = [CHANGE_COLOR_H, CHANGE_COLOR_HSV_S, CHANGE_COLOR_V].iter().any(|&s| self.value(s) != 0.0);
        let hsl_changes = [CHANGE_COLOR_L, CHANGE_COLOR_HSL_S].iter().any(|&s| self.value(s) != 0.0);
        if hsv_changes {
            let [mut h, mut s, mut v] = rgb_to_hsv(colour);
            h += self.value(CHANGE_COLOR_H);
            s += s * v * self.value(CHANGE_COLOR_HSV_S);
            v += self.value(CHANGE_COLOR_V);
            colour = hsv_to_rgb([h, s, v]);
        }
        if hsl_changes {
            let [h, mut s, mut l] = rgb_to_hsl(colour);
            l += self.value(CHANGE_COLOR_L);
            s += s * (1.0 - l).abs().min(l.abs()) * 2.0 * self.value(CHANGE_COLOR_HSL_S);
            colour = hsl_to_rgb([h, s, l]);
        }
        let mut hardness = self.value(HARDNESS).clamp(0.0, 1.0);
        // soft enough to be antialiased, as wide to the eye
        let fadeout = radius * (1.0 - hardness);
        let least_fadeout = self.value(ANTI_ALIASING);
        if fadeout < least_fadeout {
            let optical = radius - (1.0 - hardness) * radius / 2.0;
            let new_hardness = (optical - least_fadeout / 2.0) / (optical + least_fadeout / 2.0);
            radius = least_fadeout / (1.0 - new_hardness);
            hardness = new_hardness;
        }
        let snap = self.value(SNAP_TO_PIXEL);
        if snap > 0.0 {
            x += ((x).floor() + 0.5 - x) * snap;
            y += ((y).floor() + 0.5 - y) * snap;
            let mut snapped = ((radius * 2.0).round() / 2.0).max(0.5);
            if snap > 0.9999 {
                snapped -= 0.0001;
            }
            radius += (snapped - radius) * snap;
        }
        surface.draw(Drawn {
            x,
            y,
            radius,
            colour,
            opaque,
            hardness,
            alpha,
            aspect: self.s.actual_elliptical_dab_ratio,
            angle: self.s.actual_elliptical_dab_angle,
            colorize: self.value(COLORIZE),
        });
        true
    }

    fn count_dabs_to(&mut self, x: f32, y: f32, dt: f32) -> f32 {
        let base_radius = self.base(RADIUS_LOGARITHMIC).exp().clamp(ACTUAL_RADIUS_MIN, ACTUAL_RADIUS_MAX);
        if self.s.actual_radius == 0.0 {
            self.s.actual_radius = base_radius;
        }
        let (dx, dy) = (x - self.s.x, y - self.s.y);
        let dist = if self.s.actual_elliptical_dab_ratio > 1.0 {
            let (sn, cs) = self.s.actual_elliptical_dab_angle.to_radians().sin_cos();
            let yyr = (dy * cs - dx * sn) * self.s.actual_elliptical_dab_ratio;
            let xxr = dy * sn + dx * cs;
            (yyr * yyr + xxr * xxr).sqrt()
        } else {
            dx.hypot(dy)
        };
        dist / self.s.actual_radius * self.base(DABS_PER_ACTUAL_RADIUS)
            + dist / base_radius * self.base(DABS_PER_BASIC_RADIUS)
            + dt * self.base(DABS_PER_SECOND)
    }

    /// The pen moves to (x, y) with this pressure and lean (each -1..1 across and down), `dtime`
    /// seconds after it was last handed (MyPaint's `stroke_to`, as Krita calls it): the dabs on the
    /// way drawn on `surface`.
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_to(
        &mut self,
        surface: &mut dyn Surface,
        mut x: f32,
        mut y: f32,
        mut pressure: f32,
        xtilt: f32,
        ytilt: f32,
        dtime: f64,
    ) {
        let (viewzoom, viewrotation, barrel) = (1.0, 0.0, 0.0);
        let max_dtime = 5.0;
        let (mut ascension, mut declination, mut declinationx, mut declinationy) = (0.0_f32, 90.0_f32, 90.0, 90.0);
        if xtilt != 0.0 || ytilt != 0.0 {
            let (xt, yt) = (xtilt.clamp(-1.0, 1.0), ytilt.clamp(-1.0, 1.0));
            ascension = (-xt).atan2(yt).to_degrees();
            declination = 90.0 - xt.hypot(yt) * 60.0;
            declinationx = xt * 60.0;
            declinationy = yt * 60.0;
        }
        if pressure <= 0.0 {
            pressure = 0.0;
        }
        if !(x.is_finite() && y.is_finite()) || x.abs() > 1e8 || y.abs() > 1e8 {
            return;
        }
        let mut dtime = if dtime <= 0.0 { 0.0001 } else { dtime };
        if dtime > 0.1 && pressure > 0.0 && self.s.pressure == 0.0 {
            // (as MyPaint does for tablets that move unpressed unreported)
            self.stroke_to(surface, x, y, 0.0, 90.0, 0.0, dtime - 0.0001);
            dtime = 0.0001;
        }
        if self.skip > 0.001 {
            let dist = (self.skip_last.0 - x).hypot(self.skip_last.1 - y);
            self.skip_last = (x, y);
            self.skipped_dtime += dtime;
            self.skip -= dist;
            dtime = self.skipped_dtime;
            if self.skip > 0.001 && !(dtime > max_dtime || self.reset_requested) {
                return;
            }
            self.skip = 0.0;
            self.skip_last = (0.0, 0.0);
            self.skipped_dtime = 0.0;
        }
        {
            let noise_base = self.base(TRACKING_NOISE);
            if noise_base != 0.0 {
                let noise = self.base(RADIUS_LOGARITHMIC).exp() * noise_base;
                if noise > 0.001 {
                    self.skip = 0.5 * noise;
                    self.skip_last = (x, y);
                    x += noise * self.gauss();
                    y += noise * self.gauss();
                }
            }
            let fac = 1.0 - exp_decay(self.base(SLOW_TRACKING), 100.0 * dtime as f32);
            x = self.s.x + (x - self.s.x) * fac;
            y = self.s.y + (y - self.s.y) * fac;
        }
        if dtime > max_dtime || self.reset_requested {
            self.reset_requested = false;
            self.reset();
            self.random_input = self.rng.next();
            self.s.x = x;
            self.s.y = y;
            self.s.pressure = pressure;
            self.s.actual_x = x;
            self.s.actual_y = y;
            self.s.stroke = 1.0;
            return;
        }
        let mut dtime_left = dtime as f32;
        let mut dabs_moved = self.s.partial_dabs;
        let mut dabs_todo = self.count_dabs_to(x, y, dtime_left);
        let mut laid = 0;
        while dabs_moved + dabs_todo >= 1.0 && laid < MOST_DABS_A_MOVE {
            let step_ddab = if dabs_moved > 0.0 {
                let s = 1.0 - dabs_moved;
                dabs_moved = 0.0;
                s
            } else {
                1.0
            };
            let frac = step_ddab / dabs_todo;
            let s = self.s;
            let step = [
                step_ddab,
                frac * (x - s.x),
                frac * (y - s.y),
                frac * (pressure - s.pressure),
                frac * (declination - s.declination),
                frac * smallest_angular_difference(s.ascension, ascension),
                frac * dtime_left,
                frac * (declinationx - s.declinationx),
                frac * (declinationy - s.declinationy),
                frac * smallest_angular_difference(s.barrel_rotation, barrel * 360.0),
            ];
            self.update_states(step, viewzoom, viewrotation);
            self.s.flip *= -1.0;
            self.draw_dab(surface);
            self.random_input = self.rng.next();
            dtime_left -= step[6];
            dabs_todo = self.count_dabs_to(x, y, dtime_left);
            laid += 1;
        }
        let s = self.s;
        let step = [
            dabs_todo,
            x - s.x,
            y - s.y,
            pressure - s.pressure,
            declination - s.declination,
            smallest_angular_difference(s.ascension, ascension),
            dtime_left,
            declinationx - s.declinationx,
            declinationy - s.declinationy,
            smallest_angular_difference(s.barrel_rotation, barrel * 360.0),
        ];
        self.update_states(step, viewzoom, viewrotation);
        self.s.partial_dabs = dabs_moved + dabs_todo;
    }
}

/// The most dabs one move of the pen lays.
const MOST_DABS_A_MOVE: usize = 20_000;

/// A smudge update of what the brush carries (`bucket`), with the colours it picked up, or (none)
/// the last it picked up again.
fn apply_update(bucket: &mut [f32; BUCKET], u: &Update, picked: Option<[f32; 4]>) {
    let mut factor = u.length.max(0.01);
    let recentness = bucket[RECENTNESS] * factor;
    bucket[RECENTNESS] = recentness;
    let c = if u.picked {
        if recentness == 0.0 {
            // the first: what it carries is what it picked up
            factor = 0.0;
        }
        bucket[RECENTNESS] = 1.0;
        let c = picked.unwrap_or([bucket[PREV_R], bucket[PREV_R + 1], bucket[PREV_R + 2], bucket[PREV_A]]);
        bucket[PREV_R..PREV_R + 4].copy_from_slice(&c);
        c
    } else {
        [bucket[PREV_R], bucket[PREV_R + 1], bucket[PREV_R + 2], bucket[PREV_A]]
    };
    let _ = u.length_log;
    // (the colours premultiplied: MyPaint's straight colour times its alpha)
    for i in 0..3 {
        bucket[SMUDGE_R + i] = factor * bucket[SMUDGE_R + i] + (1.0 - factor) * c[i];
    }
    bucket[SMUDGE_A] = (factor * bucket[SMUDGE_A] + (1.0 - factor) * c[3]).clamp(0.0, 1.0);
}

/// How many hardnesses a MyPaint brush's tips are drawn at (0 to 1), and their side.
pub const HARDNESSES: u32 = 64;
pub const TIP_SIDE: u32 = 128;

/// MyPaint's dab at a hardness: how much it paints at rr (its distance from the middle over its
/// radius, squared).
fn dab_alpha(rr: f32, hardness: f32) -> f32 {
    if rr > 1.0 || hardness <= 0.0 {
        return 0.0;
    }
    if hardness >= 1.0 {
        return 1.0;
    }
    if rr <= hardness { 1.0 - rr * (1.0 / hardness - 1.0) } else { hardness / (1.0 - hardness) * (1.0 - rr) }
}

/// The tips a MyPaint brush picks from: its round dab at each hardness, `TIP_SIDE` a side.
pub fn tips(light: bool) -> Vec<crate::gimp::Mask> {
    (0..HARDNESSES)
        .map(|k| {
            let hardness = k as f32 / (HARDNESSES - 1) as f32;
            let n = TIP_SIDE;
            let pixels = if light {
                Vec::new()
            } else {
                (0..n * n)
                    .map(|i| {
                        let (px, py) = (i % n, i / n);
                        let u = (px as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                        let v = (py as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                        (dab_alpha(u * u + v * v, hardness) * 255.0).round() as u8
                    })
                    .collect()
            };
            crate::gimp::Mask { width: n, height: n, pixels }
        })
        .collect()
}

/// The dab flags it lays with (see the app's guide).
const OWN_COLOUR: u32 = 4;
const MOVES_PAINT: u32 = 16;
const PICKS_HERE: u32 = 512;
const TELLS_UNDER: u32 = 16384;
const SEE_THROUGH: u32 = 32768;

/// Krita's MyPaint stroke: the pen handed to MyPaint's engine where Krita hands it (at the start,
/// then every square root of the brush's diameter along the way), its dabs laid as Krita's MyPaint
/// surface lays them.
pub struct Stroke {
    engine: Engine,
    /// The spacing of its paint events (canvas pixels).
    spacing: f32,
    /// Where the last event was, and how far the pen has gone since.
    last: Option<Event>,
    gone: f32,
    previous_ms: f64,
    surface: Laid,
}

#[derive(Clone, Copy, Debug)]
struct Event {
    x: f32,
    y: f32,
    pressure: f32,
    x_tilt: f32,
    y_tilt: f32,
    ms: f64,
}

impl Event {
    fn between(a: &Event, b: &Event, t: f32) -> Event {
        let l = |p: f32, q: f32| p + (q - p) * t;
        Event {
            x: l(a.x, b.x),
            y: l(a.y, b.y),
            pressure: l(a.pressure, b.pressure),
            x_tilt: l(a.x_tilt, b.x_tilt),
            y_tilt: l(a.y_tilt, b.y_tilt),
            ms: a.ms + (b.ms - a.ms) * f64::from(t),
        }
    }
}

/// Where the stroke's dabs go: the dabs (and the dabs asking what lies under them), what the brush
/// has been told lies under its dabs (the newest), and the layer under where it started.
struct Laid {
    out: Vec<Dab>,
    smudges: bool,
    erases: bool,
    colour: [f32; 3],
    newest: Option<[f32; 4]>,
    under: Option<Under>,
}

impl Surface for Laid {
    fn colours(&mut self, x: f32, y: f32, radius: f32) -> [f32; 4] {
        // (Krita's surface reads a pixel's radius at least)
        let radius = radius.max(1.0);
        self.out.push(Dab { x, y, diameter: radius * 2.0, tip: 1, flags: TELLS_UNDER, ..Dab::default() });
        // until told, the newest told, or the layer under where the stroke started
        match (self.newest, &self.under) {
            (Some(c), _) => c,
            (None, Some(under)) => under.colours(x, y, radius).unwrap_or([0.0; 4]),
            (None, None) => [0.0; 4],
        }
    }

    fn draw(&mut self, d: Drawn) {
        if !(d.x.is_finite() && d.y.is_finite() && d.radius.is_finite()) || d.radius <= 0.0 {
            return;
        }
        let hardness = d.hardness.clamp(0.0, 1.0);
        let aspect = d.aspect.max(1.0);
        let opacity = if self.erases { d.opaque } else { d.opaque * (1.0 - d.colorize) };
        let colour = d.colour.map(|c| c.clamp(0.0, 1.0));
        // the app lays a tip's picture over its dab and as far again as this past it each way: the
        // dab that much smaller, so the picture lies over MyPaint's own (fainter where it can't be)
        const PAST: f32 = std::f32::consts::FRAC_1_SQRT_2;
        let (along, across) = (d.radius, d.radius / aspect);
        let half = (along - PAST).max(0.01);
        let ratio = ((across - PAST).max(0.01) / half).clamp(0.05, 1.0);
        let laid = (half + PAST) * (half * ratio + PAST);
        let opacity = opacity * (along * across / laid).min(1.0);
        let mut dab = Dab {
            x: d.x,
            y: d.y,
            diameter: 2.0 * half,
            angle: d.angle.to_radians(),
            ratio,
            opacity: opacity.clamp(0.0, 1.0),
            flow: 1.0,
            tip: (hardness * (HARDNESSES - 1) as f32).round() as u32,
            colour,
            strength: 1.0,
            lightness: 1.0,
            ..Dab::default()
        };
        if self.smudges && !self.erases {
            dab.flags = OWN_COLOUR | MOVES_PAINT | PICKS_HERE | SEE_THROUGH;
            dab.paint_share = 1.0;
            dab.extra[2] = d.alpha.clamp(0.0, 1.0);
        } else if colour.iter().zip(&self.colour).any(|(a, b)| (a - b).abs() > 1.0 / 512.0) {
            dab.flags = OWN_COLOUR;
        }
        self.out.push(dab);
    }
}

impl Under {
    /// The colours of the square under a disc, as Krita's MyPaint surface weighs them (see the
    /// app's guide, dab flag 16384, its weighing 1), premultiplied: none if it lies off the square.
    pub fn colours(&self, x: f32, y: f32, r: f32) -> Option<[f32; 4]> {
        let r = r.max(1.0);
        let (x0, y0, side) = ((x - r) as i32, (y - r) as i32, ((2.0 * r) as i32).max(1));
        let (mut sum, mut total) = ([0.0_f32; 4], 0.0_f32);
        let mut any = false;
        for py in y0..y0 + side {
            for px in x0..x0 + side {
                let (cx, cy) = (px as f32 - x, py as f32 - y);
                let (dx, dy) = (px as f32 + 0.5 - x, py as f32 + 0.5 - y);
                let rr = (dx * dx + dy * dy) / (r * r);
                let w =
                    if cx * cx + cy * cy <= (r + 1.0) * (r + 1.0) { ((1.0 - rr) * 255.0).round() / 255.0 } else { 1.0 };
                total += w;
                if let Some([cr, cg, cb, ca]) = self.pixel(px, py) {
                    any = true;
                    let a = ca * w;
                    sum[0] += cr * a;
                    sum[1] += cg * a;
                    sum[2] += cb * a;
                    sum[3] += a;
                }
            }
        }
        (any && total > 0.0).then(|| sum.map(|s| (s / total).clamp(0.0, 1.0)))
    }
}

impl Stroke {
    pub fn new(brush: &MyPaint, begin: Begin, under: Option<Under>) -> Stroke {
        let mut engine = Engine::new(brush.clone(), begin.seed);
        let erases = brush.erases();
        if erases {
            engine.set_base(ERASER, 0.0);
        }
        // the brush's colour
        let [h, s, v] = rgb_to_hsv(begin.colour);
        engine.set_base(COLOR_H, h);
        engine.set_base(COLOR_S, s);
        engine.set_base(COLOR_V, v);
        // its size as the slider says, over the preset's own
        let factor = {
            let (d, s) = (begin.diameter, begin.start_diameter);
            if d.is_finite() && s.is_finite() && s > 0.0 && d > 0.0 { d / s } else { 1.0 }
        };
        let radius = brush.base(RADIUS_LOGARITHMIC).exp() * factor;
        engine.set_base(RADIUS_LOGARITHMIC, radius.max(1e-3).ln());
        // (Krita hands the pen of its first event with no time: no slow tracking)
        engine.set_base(SLOW_TRACKING, 0.0);
        let diameter = 2.0 * radius;
        let spacing = if diameter < 1.0 { diameter } else { diameter.sqrt() }.max(0.5);
        let smudges = brush.smudges();
        Stroke {
            engine,
            spacing,
            last: None,
            gone: 0.0,
            previous_ms: 0.0,
            surface: Laid { out: Vec::new(), smudges, erases, colour: begin.colour, newest: None, under },
        }
    }

    /// The dabs laid so far, taken.
    pub fn take(&mut self) -> Vec<Dab> {
        std::mem::take(&mut self.surface.out)
    }

    /// Krita's paint event at `e`: MyPaint's engine handed the pen (twice while its stroke hasn't
    /// started: Krita starts it afresh each time).
    fn paint_at(&mut self, e: Event) {
        let dtime = if !self.engine.started() {
            self.engine.stroke_to(&mut self.surface, e.x, e.y, e.pressure, e.x_tilt, e.y_tilt, 1.0);
            0.015
        } else {
            (e.ms - self.previous_ms).abs() * 0.001
        };
        self.engine.stroke_to(&mut self.surface, e.x, e.y, e.pressure, e.x_tilt, e.y_tilt, dtime);
        self.previous_ms = e.ms;
    }

    /// The pen's samples, walked: a paint event at the first, then one each time the pen has gone
    /// the spacing.
    pub fn add(&mut self, samples: &[Sample]) {
        for s in samples {
            if !(s.x.is_finite() && s.y.is_finite()) || self.surface.out.len() >= MOST_DABS_A_MOVE {
                continue;
            }
            let (x_tilt, y_tilt) = crate::engine::lean(s.tilt, s.azimuth);
            let e = Event { x: s.x, y: s.y, pressure: s.pressure.clamp(0.0, 1.0), x_tilt, y_tilt, ms: s.ms };
            let Some(last) = self.last else {
                self.paint_at(e);
                self.last = Some(e);
                continue;
            };
            let length = (e.x - last.x).hypot(e.y - last.y);
            let mut from = last;
            let mut left = length;
            while self.gone + left >= self.spacing && left > 0.0 {
                let step = self.spacing - self.gone;
                let t = step / left;
                let at = Event::between(&from, &e, t);
                self.paint_at(at);
                from = at;
                left -= step;
                self.gone = 0.0;
            }
            self.gone += left;
            self.last = Some(e);
        }
    }

    /// The colours under the dabs it asked about, told.
    pub fn told(&mut self, told: &[[f32; 4]]) {
        let used = self.engine.told(told);
        if used > 0 {
            self.surface.newest = told.get(used - 1).copied();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WET: &str = r#"{"version": 3, "settings": {
        "radius_logarithmic": {"base_value": 2.0, "inputs": {}},
        "smudge": {"base_value": 0.9, "inputs": {"pressure": [[0.0, 0.2], [1.0, -0.4]]}},
        "hardness": {"base_value": 0.5, "inputs": {"pressure": [[0.0, 0.0], [1.0, 1.0]]}},
        "opaque_multiply": {"base_value": 0.0, "inputs": {"pressure": [[0.0, 0.0], [1.0, 1.0]]}},
        "dabs_per_actual_radius": {"base_value": 4.0, "inputs": {}}
    }}"#;

    struct Paper {
        dabs: Vec<Drawn>,
        asked: Vec<(f32, f32, f32)>,
        under: [f32; 4],
    }

    impl Surface for Paper {
        fn colours(&mut self, x: f32, y: f32, radius: f32) -> [f32; 4] {
            self.asked.push((x, y, radius));
            self.under
        }
        fn draw(&mut self, dab: Drawn) {
            self.dabs.push(dab);
        }
    }

    #[test]
    fn reads_a_brush_file_over_the_defaults_as_mypaint_does() {
        let b = MyPaint::read(WET).unwrap();
        assert_eq!(b.base(RADIUS_LOGARITHMIC), 2.0);
        assert_eq!(b.base(OPAQUE), 1.0);
        assert_eq!(b.map[SMUDGE].curves, vec![(PRESSURE, vec![(0.0, 0.2), (1.0, -0.4)])]);
        assert!(b.smudges() && !b.erases());
        // pressure 0.5: 0.9 + 0.2 * 0.5 + -0.4 * 0.5
        let mut inputs = [0.0; 18];
        inputs[PRESSURE] = 0.5;
        assert!((b.map[SMUDGE].value(&inputs) - 0.8).abs() < 1e-6);
        // a file of another version keeps the defaults
        let old = MyPaint::read(&WET.replace("\"version\": 3", "\"version\": 2")).unwrap();
        assert_eq!(old, MyPaint::defaults());
        assert!(MyPaint::read("not json").is_none());
    }

    #[test]
    fn a_stroke_lays_dabs_spaced_by_its_radius_and_picks_up_what_lies_under_them() {
        let mut e = Engine::new(MyPaint::read(WET).unwrap(), 7);
        let mut paper = Paper { dabs: Vec::new(), asked: Vec::new(), under: [0.5, 0.0, 0.0, 0.5] };
        // the first hand-over starts it afresh
        e.stroke_to(&mut paper, 10.0, 10.0, 0.5, 0.0, 0.0, 1.0);
        assert!(paper.dabs.is_empty());
        e.stroke_to(&mut paper, 10.0, 10.0, 0.5, 0.0, 0.0, 0.015);
        e.stroke_to(&mut paper, 110.0, 10.0, 0.5, 0.0, 0.0, 0.1);
        // 4 dabs a radius (e^2 = 7.4 pixels) over 100 pixels
        let n = paper.dabs.len();
        assert!((50..=56).contains(&n), "{n}");
        assert_eq!(paper.asked.len(), n);
        let last = paper.dabs[n - 1];
        // the brush carries the half-clear red: its black (smudge 0.8 at this pressure) mixed with it,
        // (0.8 * 0.5) / (0.2 + 0.8 * 0.5) red, its alpha taking what is there towards 0.6
        assert!((last.colour[0] - 0.4 / 0.6).abs() < 1e-3 && (last.alpha - 0.6).abs() < 1e-3, "{last:?}");
    }

    #[test]
    fn what_the_brush_carries_is_worked_out_again_once_told() {
        let brush = MyPaint::read(WET).unwrap();
        let begin = Begin {
            colour: [0.0, 0.0, 1.0],
            secondary: [1.0; 3],
            diameter: 1.0,
            start_diameter: 1.0,
            seed: 3,
            turn: 0.0,
        };
        let mut stroke = Stroke::new(&brush, begin, None);
        let at = |x: f32, ms: f64| Sample { x, y: 20.0, pressure: 0.6, ms, ..Sample::default() };
        stroke.add(&[at(10.0, 0.0), at(60.0, 50.0)]);
        let dabs = stroke.take();
        let asked = dabs.iter().filter(|d| d.flags & TELLS_UNDER != 0).count();
        let laid: Vec<&Dab> = dabs.iter().filter(|d| d.flags & TELLS_UNDER == 0).collect();
        assert!(asked > 5 && laid.len() == asked, "{asked} {}", laid.len());
        // laid in place, its colour as opaque as what it carries says
        assert!(laid.iter().all(|d| d.flags & SEE_THROUGH != 0 && d.paint_share == 1.0));
        // told it was all opaque green under them: the next dabs go green and opaque
        stroke.told(&vec![[0.0, 1.0, 0.0, 1.0]; asked]);
        stroke.add(&[at(110.0, 100.0)]);
        let next = stroke.take();
        let d = next.iter().rev().find(|d| d.flags & TELLS_UNDER == 0).unwrap();
        assert!(d.colour[1] > 0.5 && d.extra[2] > 0.9, "{d:?}");
    }

    #[test]
    fn its_tips_are_its_dab_at_each_hardness() {
        let tips = tips(false);
        assert_eq!(tips.len(), HARDNESSES as usize);
        let mid = (TIP_SIDE / 2 * TIP_SIDE + TIP_SIDE / 2) as usize;
        assert_eq!(tips[0].pixels[mid], 0);
        assert_eq!(tips[63].pixels[mid], 255);
        // a soft one falls off towards its rim
        let soft = &tips[32].pixels;
        assert!(soft[mid] > soft[mid + 40] && soft[mid + 40] > soft[mid + 62]);
    }
}
