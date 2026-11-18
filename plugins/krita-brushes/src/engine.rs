//! A stroke of a Krita pixel brush (or colour smudge brush), dab by dab: its input walked along, a
//! dab laid each time the path has gone as far as the spacing of the last dab reaches (an ellipse
//! turned with the dab, or a circle when the spacing is the same every way), each dab's size, turn,
//! squash, scatter, mirroring, opacity, flow, colour, tip and texture strength from the brush's
//! options and the pen where it lies. A colour smudge brush's dabs move the paint under the last
//! dab to where they lie, mixing in the colour as its rates say. A masking brush walks the same
//! path with its own spacing. The sketch brush lays lines at each move instead, and the spray brush
//! its particles at each dab (see `others.rs`).

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};
use std::rc::Rc;

use crate::brush::{Brush, PipePick, Tip, Tips};
use crate::gimp::Selection;
use crate::options::{Chance, CurveOption, Info};

/// What a stroke starts with: the app's colour and secondary colour (Krita's background), its size
/// now and where the brush's size starts (the preset's own size there), a seed.
#[derive(Clone, Copy, Debug)]
pub struct Begin {
    pub colour: [f32; 3],
    pub secondary: [f32; 3],
    pub diameter: f32,
    pub start_diameter: f32,
    pub seed: u32,
    /// How far the canvas is turned on screen (radians).
    pub turn: f32,
}

/// A sample of the pen: canvas pixels, pressure, lean from upright and its way (radians,
/// clockwise from up), the barrel's turn (radians), milliseconds since the stroke began.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub tilt: f32,
    pub azimuth: f32,
    pub roll: f32,
    pub ms: f64,
}

/// The pen at a point of the path, as Krita takes it.
#[derive(Clone, Copy, Debug, Default)]
struct Point {
    x: f32,
    y: f32,
    pressure: f32,
    /// The most the pen has pressed so far.
    pressure_in: f32,
    x_tilt: f32,
    y_tilt: f32,
    rotation: f32,
    time: f32,
    speed: f32,
}

impl Point {
    fn between(a: &Point, b: &Point, t: f32) -> Point {
        let l = |p: f32, q: f32| p + (q - p) * t;
        Point {
            x: l(a.x, b.x),
            y: l(a.y, b.y),
            pressure: l(a.pressure, b.pressure),
            pressure_in: l(a.pressure_in, b.pressure_in),
            x_tilt: l(a.x_tilt, b.x_tilt),
            y_tilt: l(a.y_tilt, b.y_tilt),
            rotation: l(a.rotation, b.rotation),
            time: l(a.time, b.time),
            speed: l(a.speed, b.speed),
        }
    }
}

/// xorshift: the stroke's randomness, from its seed.
#[derive(Clone, Debug)]
pub struct Random(u32);

impl Random {
    pub fn new(seed: u32) -> Random {
        Random(seed | 1)
    }

    /// 0..1.
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1 << 24) as f32
    }
}

struct Fuzz<'a> {
    random: &'a mut Random,
    seed: u32,
}

impl Chance for Fuzz<'_> {
    fn dab(&mut self) -> f32 {
        self.random.next()
    }

    fn stroke(&mut self, key: &str) -> f32 {
        // the same for the whole stroke, apart for each option
        let mut h = self.seed ^ 0x811c_9dc5;
        for b in key.bytes() {
            h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
        Random::new(h).next()
    }
}

/// Where one brush (the main one, or its masking brush) is along the path: how far it has gone
/// since its last dab, across and down in that dab's frame, the spacing that dab set and its turn.
#[derive(Clone, Debug, Default)]
struct Walker {
    gone: (f32, f32),
    spacing: (f32, f32),
    turn: f32,
    dabs: u32,
    /// When its last dab was laid (ms), for an airbrush.
    last_ms: f32,
    /// Where its last dab lay on the path (the pen there) and the way the stroke went there
    /// (radians); the way it went at its start, as a locked drawing angle keeps it.
    at: Option<Point>,
    angle: f32,
    locked: Option<f32>,
}

/// Spacing below this (canvas pixels) is taken as this, as Krita does.
const LEAST_SPACING: f32 = 0.5;
/// The most dabs one call lays, and the most points a sketch brush keeps.
const MOST_DABS: usize = 20_000;
const MOST_SKETCH_POINTS: usize = 4096;
/// The most corners a shape brush's outline keeps.
const MOST_OUTLINE: usize = 16_384;

/// Where a bristle brush's bristle was (from the pen, after its last move), how many pixels it has
/// painted and how much ink it has left.
#[derive(Clone, Copy, Debug)]
struct BristleState {
    x: f32,
    y: f32,
    counter: u32,
    ink: f32,
    /// The colour it soaked up where the stroke started.
    colour: Option<[f32; 3]>,
}

/// One of a particle brush's particles: where it is, its pull towards the pen as it builds, and how
/// hard the pen pulls it.
#[derive(Clone, Copy, Debug)]
struct Particle {
    at: (f32, f32),
    next: (f32, f32),
    pull: f32,
}

/// The lines of a hatching at `angle` degrees, `separation` apart, anchored to the canvas at its
/// origin, across the box `(x, y, w, h)`: each from where it meets the box's sides to where it
/// leaves them (on whole pixels unless it says otherwise).
fn hatch_lines(
    hatching: &crate::others::Hatching,
    angle: f32,
    separation: f32,
    (x, y, w, h): (f32, f32, f32, f32),
) -> Vec<((f32, f32), (f32, f32))> {
    let mut lines = Vec::new();
    let round = |v: f32| if hatching.subpixel { v } else { if v - v.floor() >= 0.5 { v.ceil() } else { v.floor() } };
    let (ox, oy) = hatching.origin;
    if angle.abs() == 90.0 {
        // upright lines, a separation apart
        let first = (ox - x) % separation;
        let mut k = ((-first) / separation).floor() as i32;
        while k < 10_000 {
            let lx = first + separation * k as f32;
            k += 1;
            if lx < 0.0 {
                continue;
            }
            if lx > w {
                break;
            }
            let lx = round(lx);
            lines.push(((x + lx, y), (x + lx, y + round(h))));
        }
        return lines;
    }
    let slope = angle.to_radians().tan();
    let mut dy = (separation / angle.to_radians().cos()).abs();
    if !hatching.subpixel {
        dy = dy.trunc();
    }
    if dy <= 0.0 {
        return lines;
    }
    // the lines' intercepts on the box's left side: the canvas's lines through the origin
    let base = oy - slope * ox;
    let here = y - slope * x;
    let hot = (base - here) % dy;
    let (lo, hi) = (-(slope * w).abs() - h, h + (slope * w).abs());
    let first = ((lo - hot) / dy).floor() as i32;
    let last = ((hi - hot) / dy).ceil() as i32;
    for k in first..=last.min(first + 10_000) {
        let c = hot + dy * k as f32;
        // where the line v = slope u + c meets the box's sides
        let mut ends = Vec::with_capacity(2);
        if (0.0..=h).contains(&c) {
            ends.push((0.0, c));
        }
        let right = slope * w + c;
        if (0.0..=h).contains(&right) {
            ends.push((w, right));
        }
        if slope != 0.0 {
            let top = -c / slope;
            if top > 0.0 && top < w {
                ends.push((top, 0.0));
            }
            let bottom = (h - c) / slope;
            if bottom > 0.0 && bottom < w {
                ends.push((bottom, h));
            }
        }
        if ends.len() >= 2 {
            let (a, b) = (ends[0], ends[1]);
            lines.push(((x + round(a.0), y + round(a.1)), (x + round(b.0), y + round(b.1))));
        }
    }
    lines
}

/// A number of the standard normal distribution (Box and Muller's way).
fn gaussian(random: &mut Random) -> f32 {
    let u = random.next().max(1e-7);
    let v = random.next();
    (-2.0 * u.ln()).sqrt() * (TAU * v).cos()
}

impl Walker {
    /// How far along `from`..`to` (0..1) its next dab goes, if it goes there at all.
    fn next(&mut self, from: (f32, f32), to: (f32, f32)) -> Option<f32> {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        if dx == 0.0 && dy == 0.0 {
            return None;
        }
        let (a, b) = (1.0 / self.spacing.0.max(LEAST_SPACING), 1.0 / self.spacing.1.max(LEAST_SPACING));
        let (x, y) = self.gone;
        let gamma = (x * a).powi(2) + (y * b).powi(2) - 1.0;
        if gamma >= 0.0 {
            self.gone = (0.0, 0.0);
            return Some(0.0);
        }
        // the move in the frame of the last dab
        let (s, c) = (-self.turn).sin_cos();
        let (mx, my) = ((dx * c - dy * s).abs(), (dx * s + dy * c).abs());
        let alpha = (mx * a).powi(2) + (my * b).powi(2);
        let beta = x * mx * a * a + y * my * b * b;
        let d = beta * beta - alpha * gamma;
        if d >= 0.0 && alpha > 0.0 {
            let k = (-beta + d.sqrt()) / alpha;
            if (0.0..=1.0).contains(&k) {
                self.gone = (0.0, 0.0);
                return Some(k);
            }
        }
        self.gone = (x + mx, y + my);
        None
    }
}

/// The last dab drawn afresh, as a brush's precision compares the next with it: its colour, turn,
/// box (pixels), place within a pixel and the box's half, softness, lightness, squash, tip and
/// mirroring, and its diameter.
#[derive(Clone, Copy, Debug)]
struct Kept {
    colour: [f32; 3],
    angle: f32,
    size: (f32, f32),
    within: (f32, f32),
    half: (f32, f32),
    softness: f32,
    lightness: f32,
    ratio: f32,
    tip: u32,
    flips: u32,
    diameter: f32,
}

/// How near a dab must be to the kept one, by precision level: its turn (radians), its box's
/// size (a share), its place within a pixel, its softness, lightness and squash.
const NEAR: [(f32, f32, f32, f32, f32, f32); 5] = [
    (PI / 180.0, 0.05, 1.0, 0.01, 0.01, 0.05),
    (PI / 180.0, 0.01, 1.0, 0.01, 0.01, 0.01),
    (PI / 180.0, 0.0, 1.0, 0.01, 0.01, 1e-6),
    (PI / 180.0, 0.0, 0.5, 0.01, 0.01, 1e-6),
    (1e-6, 0.0, 1e-6, 1e-6, 1e-6, 1e-6),
];

impl Kept {
    /// Whether `self`, the next dab, is near enough `kept` at `level` to be it laid again.
    fn near(&self, kept: &Kept, level: u32) -> bool {
        let Some(&(angle, size, within, softness, lightness, ratio)) = NEAR.get(level.clamp(1, 5) as usize - 1) else {
            return false;
        };
        let sized = |a: f32, b: f32| (a - b).abs() <= (size * a).trunc();
        self.colour == kept.colour
            && (self.angle - kept.angle).abs() <= angle
            && sized(self.size.0, kept.size.0)
            && sized(self.size.1, kept.size.1)
            && (self.within.0 - kept.within.0).abs() <= within
            && (self.within.1 - kept.within.1).abs() <= within
            && (self.softness - kept.softness).abs() <= softness
            && (self.lightness - kept.lightness).abs() <= lightness
            && (self.ratio - kept.ratio).abs() <= ratio
            && self.tip == kept.tip
            && self.flips == kept.flips
    }
}

/// How a pipe stands: each of its dimensions' step.
#[derive(Clone, Debug, Default)]
struct PipeState {
    steps: Vec<u32>,
}

/// A dab, as the app takes it (see the app's guide): 24 numbers.
#[derive(Clone, Copy, Debug, Default)]
pub struct Dab {
    pub x: f32,
    pub y: f32,
    pub diameter: f32,
    pub angle: f32,
    pub ratio: f32,
    pub opacity: f32,
    pub flow: f32,
    pub tip: u32,
    pub flags: u32,
    pub colour: [f32; 3],
    pub strength: f32,
    pub texture_offset: (f32, f32),
    /// How strongly a lightness tip's grey shows; its sharpness threshold (0: none) and the share
    /// below it kept as it is.
    pub lightness: f32,
    pub threshold: f32,
    pub softness: f32,
    /// A dab that moves paint: the share of what it lays that is its colour, and the share of where
    /// the last lay it picks one colour from (dulling).
    pub paint_share: f32,
    pub radius: f32,
    /// Words 20 to 23 as its flags say: where it picks up from (across, down), a warp's or a
    /// blur's amount, and a warp's kind (or a sharpening's amount, as its bits).
    pub extra: [f32; 3],
    pub kind: u32,
}

const FLIP_ACROSS: u32 = 1;
const FLIP_DOWN: u32 = 2;
const OWN_COLOUR: u32 = 4;
const FOR_SECOND: u32 = 8;
const MOVES_PAINT: u32 = 16;
const DULLS: u32 = 32;
const WEIGHED: u32 = 64;
const CLEAR_PAINT: u32 = 128;
const LAID_OVER: u32 = 256;
const PICKS_HERE: u32 = 512;
const PICKS_FROM: u32 = 1024;
const WARPS: u32 = 2048;
const FILTERS: u32 = 4096;
const PICKS_BEFORE: u32 = 8192;
/// Into the layer's kept paint: its tip's grey over the thickness kept there, as opaque as word 20.
const LAYS_HEIGHT: u32 = 65536;

/// Where a clone brush picks up from: the point a tap set, and how far its first stroke after that
/// started from it (kept while the source moves with the brush).
type CloneSource = (Option<(f32, f32)>, Option<(f32, f32)>);

thread_local! {
    static CLONE_SOURCE: std::cell::Cell<CloneSource> = const { std::cell::Cell::new((None, None)) };
}
/// The tip lines, dots and boxes are drawn with: a square (the round one first).
const LINE_TIP: u32 = 1;
/// A line a pixel thin's tips: smoothed, and hard.
const THIN_TIP: u32 = 3;
const THIN_HARD_TIP: u32 = 4;
/// The least squash the app lays a dab at.
const LEAST_SQUASH: f32 = 0.05;

impl Dab {
    pub fn write(&self, out: &mut Vec<u8>) {
        let words: [u32; 24] = [
            self.x.to_bits(),
            self.y.to_bits(),
            self.diameter.to_bits(),
            self.angle.to_bits(),
            self.ratio.to_bits(),
            self.opacity.to_bits(),
            self.flow.to_bits(),
            self.tip,
            self.flags,
            self.colour[0].to_bits(),
            self.colour[1].to_bits(),
            self.colour[2].to_bits(),
            self.strength.to_bits(),
            self.texture_offset.0.to_bits(),
            self.texture_offset.1.to_bits(),
            self.lightness.to_bits(),
            self.threshold.to_bits(),
            self.softness.to_bits(),
            self.paint_share.to_bits(),
            self.radius.to_bits(),
            self.extra[0].to_bits(),
            self.extra[1].to_bits(),
            self.extra[2].to_bits(),
            self.kind,
        ];
        for w in words {
            out.extend_from_slice(&w.to_le_bytes());
        }
    }
}

/// The colours of the layer under where a stroke starts, as the app hands them: a square's left and
/// top (canvas pixels), its side and its pixels (RGBA8, straight).
pub struct Under {
    x0: i32,
    y0: i32,
    side: u32,
    pixels: Vec<u8>,
}

impl Under {
    pub fn read(bytes: &[u8]) -> Option<Under> {
        let word = |i: usize| bytes.get(i * 4..i * 4 + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes);
        let (x0, y0, side) = (word(0)? as i32, word(1)? as i32, word(2)?);
        let pixels = bytes.get(12..)?.to_vec();
        (pixels.len() as u64 == u64::from(side) * u64::from(side) * 4).then_some(Under { x0, y0, side, pixels })
    }

    /// The colour and alpha at canvas pixel (x, y) (straight, 0..1), if the square holds it.
    pub fn pixel(&self, x: i32, y: i32) -> Option<[f32; 4]> {
        let (i, j) = (x - self.x0, y - self.y0);
        if i < 0 || j < 0 || i >= self.side as i32 || j >= self.side as i32 {
            return None;
        }
        let at = ((j as u32 * self.side + i as u32) * 4) as usize;
        let p = self.pixels.get(at..at + 4)?;
        Some([p[0], p[1], p[2], p[3]].map(|v| f32::from(v) / 255.0))
    }

    /// The colour at canvas pixel (x, y), if the square holds it.
    fn at(&self, x: i32, y: i32) -> Option<[f32; 3]> {
        let (i, j) = (x - self.x0, y - self.y0);
        if i < 0 || j < 0 || i >= self.side as i32 || j >= self.side as i32 {
            return None;
        }
        let at = ((j as u32 * self.side + i as u32) * 4) as usize;
        let p = self.pixels.get(at..at + 3)?;
        Some([p[0], p[1], p[2]].map(|v| f32::from(v) / 255.0))
    }
}

pub struct Stroke {
    brush: Rc<Brush>,
    begin: Begin,
    random: Random,
    last: Option<Point>,
    main: Walker,
    second: Walker,
    pipe: PipeState,
    second_pipe: PipeState,
    /// The path drawn so far (canvas pixels), its way (radians) and the dabs laid.
    length: f32,
    angle: f32,
    dabs: u32,
    /// What the stroke's opacity has averaged (washed strokes keep near it).
    average: f32,
    texture_offset: (f32, f32),
    /// For the speed: the last samples' moves and the time between samples, averaged.
    moves: Vec<f32>,
    /// A sketch or curve brush's points passed, the latest last.
    points: Vec<(f32, f32)>,
    /// A bristle brush's bristles as they went, its moves and the mouse's pressure; a particle
    /// brush's particles.
    bristles: Vec<BristleState>,
    hairy_moves: u32,
    old_pressure: f32,
    particles: Vec<Particle>,
    pace: f32,
    speed: f32,
    /// The way the stroke went at its start, for a drawing angle locked there.
    locked: f32,
    /// The last dab drawn afresh, for the brush's precision.
    kept: Option<Kept>,
    /// A clone brush's samples held until the pen has moved (a tap sets its source), and how far
    /// from where it lies each dab picks up while the source moves with it.
    held: Vec<Sample>,
    moving: bool,
    clone_offset: (f32, f32),
    /// A shape brush's outline so far, and where its smoothing and speed have got to.
    outline: Vec<(f32, f32)>,
    smoothing: ((f32, f32), f32),
    speed_point: ((f32, f32), f32),
    pub out: Vec<Dab>,
    /// The colours under where it starts, if the app handed them.
    pub under: Option<Under>,
    /// The canvas's size (canvas pixels), if the app said.
    pub canvas: Option<(f32, f32)>,
    /// A dyna brush's weight, the way it was drawn across last, and its lines' ends.
    dyna: Option<crate::others::DynaWeight>,
    dyna_across: (f32, f32),
    dyna_lines: Vec<(f32, f32)>,
    /// A MyPaint brush's stroke, once it has samples.
    mypaint: Option<crate::mypaint::Stroke>,
}

impl Stroke {
    pub fn new(brush: Rc<Brush>, begin: Begin) -> Stroke {
        let mut random = Random::new(begin.seed);
        let texture_offset = brush.texture.as_ref().map_or((0.0, 0.0), |t| {
            let (w, h) = (t.mask.width as f32 * t.scale, t.mask.height as f32 * t.scale);
            let x = if t.random_offset.0 { random.next() * w } else { t.offset.0 };
            let y = if t.random_offset.1 { random.next() * h } else { t.offset.1 };
            (-x, -y)
        });
        let pipe = |tip: &Tip, random: &mut Random| PipeState {
            steps: match &tip.tips {
                Tips::Images { pipe: Some(p), .. } => p
                    .ranks
                    .iter()
                    .zip(&p.selections)
                    .map(|(&r, s)| if *s == Selection::Random { (random.next() * r as f32) as u32 } else { 0 })
                    .collect(),
                _ => Vec::new(),
            },
        };
        let main_pipe = pipe(&brush.tip, &mut random);
        let bristles = brush.hairy.as_ref().map_or(0, |h| h.bristles.len());
        let second_pipe = brush.masking.as_ref().map(|m| pipe(&m.tip, &mut random)).unwrap_or_default();
        Stroke {
            brush,
            begin,
            random,
            last: None,
            main: Walker::default(),
            second: Walker::default(),
            pipe: main_pipe,
            second_pipe,
            length: 0.0,
            angle: 0.0,
            dabs: 0,
            average: 0.0,
            texture_offset,
            moves: Vec::new(),
            points: Vec::new(),
            bristles: vec![BristleState { x: 0.0, y: 0.0, counter: 0, ink: 1.0, colour: None }; bristles],
            hairy_moves: 0,
            old_pressure: 1.0,
            particles: Vec::new(),
            pace: 0.0,
            speed: 0.0,
            locked: 0.0,
            kept: None,
            held: Vec::new(),
            moving: false,
            clone_offset: (0.0, 0.0),
            outline: Vec::new(),
            smoothing: ((0.0, 0.0), 0.0),
            speed_point: ((0.0, 0.0), 0.0),
            out: Vec::new(),
            under: None,
            canvas: None,
            dyna: None,
            dyna_across: (0.0, 0.0),
            dyna_lines: Vec::new(),
            mypaint: None,
        }
    }

    /// The colours under the dabs it asked about, told (premultiplied, in order).
    pub fn told(&mut self, told: &[[f32; 4]]) {
        if let Some(m) = self.mypaint.as_mut() {
            m.told(told);
        }
    }

    /// The size the app's slider asks for, as a share of the brush's own.
    fn size_factor(&self) -> f32 {
        let (d, s) = (self.begin.diameter, self.begin.start_diameter);
        if d.is_finite() && s.is_finite() && s > 0.0 && d > 0.0 { d / s } else { 1.0 }
    }

    /// The pen at `s`, its speed measured as Krita measures it (the last few moves over the time
    /// samples take on average, in pixels a millisecond, a thirtieth of that the sensor's 1).
    fn point(&mut self, s: &Sample) -> Point {
        let (x_tilt, y_tilt) = lean(s.tilt, s.azimuth);
        if let Some(last) = self.last {
            let moved = ((s.x - last.x).powi(2) + (s.y - last.y).powi(2)).sqrt();
            if moved > 0.0 {
                let gap = (s.ms as f32 - last.time).max(0.0);
                self.pace = if self.pace == 0.0 { gap.max(1.0) } else { 0.8 * self.pace + 0.2 * gap.max(0.5) };
                self.moves.push(moved);
                if self.moves.len() > 64 {
                    self.moves.remove(0);
                }
                let (mut total, mut n) = (0.0, 0);
                for m in self.moves.iter().rev() {
                    total += m;
                    n += 1;
                    if n > 3 && total > 5.0 {
                        break;
                    }
                }
                if total > 5.0 {
                    self.speed = (total / (n as f32 * self.pace)) / 30.0;
                }
            }
        }
        let pressure = s.pressure.clamp(0.0, 1.0);
        Point {
            x: s.x,
            y: s.y,
            pressure,
            pressure_in: self.last.map_or(pressure, |l| l.pressure_in.max(pressure)),
            x_tilt,
            y_tilt,
            rotation: s.roll.to_degrees(),
            time: s.ms as f32,
            speed: self.speed.min(1.0),
        }
    }

    pub fn add(&mut self, samples: &[Sample]) {
        // a MyPaint brush's through MyPaint's engine
        if let Some(brush) = self.brush.mypaint.as_ref() {
            let (begin, under) = (self.begin, &mut self.under);
            let m = self.mypaint.get_or_insert_with(|| crate::mypaint::Stroke::new(brush, begin, under.take()));
            m.add(samples);
            self.out.extend(m.take());
            return;
        }
        // a clone brush lays nothing until the pen has moved: a tap sets where it picks up from
        if self.brush.clone.is_some() && !self.moving {
            self.held.extend_from_slice(samples);
            let first = self.held.first().copied();
            let far = |s: &Sample| first.is_some_and(|f| (s.x - f.x).hypot(s.y - f.y) >= 3.0);
            if !self.held.iter().any(far) {
                return;
            }
            self.moving = true;
            self.clone_offset = self.clone_start(first.map_or((0.0, 0.0), |f| (f.x, f.y)));
            let held = std::mem::take(&mut self.held);
            return self.add(&held);
        }
        for s in samples {
            if !(s.x.is_finite() && s.y.is_finite()) || self.out.len() >= MOST_DABS {
                continue;
            }
            let p = self.point(s);
            // a sketch brush draws its lines at each move
            if self.brush.sketch.is_some() {
                let from = self.last.unwrap_or(p);
                self.sketch_move(&from, &p);
                self.length += ((p.x - from.x).powi(2) + (p.y - from.y).powi(2)).sqrt();
                self.last = Some(p);
                continue;
            }
            // a shape brush's outline goes on to each place the pen goes
            if self.brush.shape.is_some() {
                if let Some(from) = self.last {
                    self.shape_move(&from, &p);
                }
                self.last = Some(p);
                continue;
            }
            // a dyna brush's weight is pulled towards where the pen was at each move
            if self.brush.dyna.is_some() {
                let at = self.last.unwrap_or(p);
                self.dyna_move(&at);
                self.last = Some(p);
                continue;
            }
            // a bristle, curve or particle brush draws at each move (a particle brush where it
            // starts too)
            if self.brush.hairy.is_some() || self.brush.curves.is_some() || self.brush.particles.is_some() {
                match self.last {
                    Some(from) => {
                        // the way it went, from where it last drew (a bristle brush's drawing angle)
                        if p.x != from.x || p.y != from.y {
                            self.angle = (p.y - from.y).atan2(p.x - from.x);
                        }
                        self.line_move(&from, &p);
                        self.length += ((p.x - from.x).powi(2) + (p.y - from.y).powi(2)).sqrt();
                    }
                    None if self.brush.particles.is_some() => self.line_move(&p, &p),
                    None => {}
                }
                self.last = Some(p);
                continue;
            }
            let Some(last) = self.last else {
                // the first point gets a dab of each brush
                self.walk_dab(&p, false);
                if self.brush.masking.is_some() {
                    self.walk_dab(&p, true);
                }
                self.last = Some(p);
                continue;
            };
            let (dx, dy) = (p.x - last.x, p.y - last.y);
            if dx != 0.0 || dy != 0.0 {
                self.angle = dy.atan2(dx);
            }
            for second in [false, true] {
                if second && self.brush.masking.is_none() {
                    continue;
                }
                let mut at = last;
                let start_length = self.length;
                while self.out.len() < MOST_DABS {
                    // an airbrush lays a dab when its time comes, wherever its spacing has got to
                    let timed = if second { None } else { self.airbrushed(&at, &p) };
                    let until = timed.map_or(p, |k| Point::between(&at, &p, k));
                    let ignore = !second && self.brush.airbrush.as_ref().is_some_and(|a| a.ignore_spacing);
                    let walker = if second { &mut self.second } else { &mut self.main };
                    let spaced = if ignore { None } else { walker.next((at.x, at.y), (until.x, until.y)) };
                    let (t, here) = match (spaced, timed) {
                        (Some(t), _) => (t, Point::between(&at, &until, t)),
                        (None, Some(_)) => {
                            walker.gone = (0.0, 0.0);
                            (1.0, until)
                        }
                        (None, None) => break,
                    };
                    // the distance sensor's: as far along as this dab
                    let done = ((here.x - last.x).powi(2) + (here.y - last.y).powi(2)).sqrt();
                    self.length = start_length + done;
                    self.walk_dab(&here, second);
                    if t <= 0.0 && (here.x - at.x).abs() < 1e-6 && (here.y - at.y).abs() < 1e-6 {
                        // a dab where the walk stood: go on from it
                        at = here;
                        continue;
                    }
                    at = here;
                }
                self.length = start_length;
            }
            self.length += (dx * dx + dy * dy).sqrt();
            self.last = Some(p);
        }
    }

    /// A dab of one brush (the main one, or its masking brush with `second`) where its walk has
    /// got to: the way the stroke goes there taken from where its last dab lay, as Krita takes its
    /// drawing angle; first, where its rotation follows that way with fan corners, a dab at each of
    /// their steps as the way turns from the last dab's to this one's, along the way between them.
    fn walk_dab(&mut self, here: &Point, second: bool) {
        let brush = Rc::clone(&self.brush);
        let rotation = match (second, &brush.masking) {
            (true, Some(m)) => &m.dynamics.rotation,
            _ => &brush.dynamics.rotation,
        };
        let walker = if second { &self.second } else { &self.main };
        let (last, last_angle) = (walker.at, walker.angle);
        let moved = last.filter(|l| (here.x - l.x).abs() > 1e-6 || (here.y - l.y).abs() > 1e-6);
        let angle = moved.map_or(last_angle, |l| (here.y - l.y).atan2(here.x - l.x));
        if let (Some(step), Some(last)) = (rotation.fan_step(), last) {
            let full = turned(last_angle, angle);
            let mut current = last_angle;
            let mut i = 0;
            // (a step's worth left, give or take rounding, is the dab's own)
            while turned(current, angle) > step + 1e-4 && i < 720 && self.out.len() < MOST_DABS {
                current = towards(current, step, angle);
                let t = step * i as f32 / full;
                i += 1;
                self.lay(&Point::between(&last, here, t), current, second, false);
            }
        }
        self.lay(here, angle, second, moved.is_some());
    }

    /// A dab at `p`, the stroke going the way `angle` there (`moved`: a way it really went).
    fn lay(&mut self, p: &Point, angle: f32, second: bool, moved: bool) {
        let walker = if second { &mut self.second } else { &mut self.main };
        if moved && walker.locked.is_none() {
            walker.locked = Some(angle);
        }
        self.locked = walker.locked.unwrap_or(angle);
        self.angle = angle;
        self.dab_at(p, second);
        let walker = if second { &mut self.second } else { &mut self.main };
        walker.at = Some(*p);
        walker.angle = angle;
    }

    /// A shape brush's move from `a` to `b`: its outline goes on to `b` (as far as the speed takes
    /// it), or rounds off a corner through where it last turned once the pen has gone as far as its
    /// smoothing says.
    fn shape_move(&mut self, a: &Point, b: &Point) {
        let Some(shape) = self.brush.shape.clone() else { return };
        if self.outline.is_empty() {
            self.outline = vec![(a.x, a.y), (b.x, b.y)];
            self.smoothing = ((a.x, a.y), 0.0);
            self.speed_point = ((a.x, a.y), 0.0);
            return;
        }
        let mut to = (b.x, b.y);
        if let Some(multiplier) = shape.speed {
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let length = dx.hypot(dy);
            if length >= 0.1 {
                let ((sx, sy), coeff) = self.speed_point;
                let coeff = 0.6 * coeff + 0.4 * (0.5 * length * multiplier);
                let new = (a.x + dx * coeff / length, a.y + dy * coeff / length);
                let point = (0.6 * sx + 0.4 * new.0, 0.6 * sy + 0.4 * new.1);
                self.speed_point = (point, coeff);
                to = point;
            }
        }
        let length = (to.0 - a.x).abs() + (to.1 - a.y).abs();
        match shape.smoothing {
            Some(threshold) => {
                let (through, gone) = self.smoothing;
                let gone = gone + length.trunc();
                if gone > threshold {
                    let end = ((through.0 + to.0) * 0.5, (through.1 + to.1) * 0.5);
                    let from = self.outline.last().copied().unwrap_or(through);
                    // a quadratic curve through where it last turned, in eight pieces
                    for i in 1..=8 {
                        let t = i as f32 / 8.0;
                        let u = 1.0 - t;
                        self.outline.push((
                            u * u * from.0 + 2.0 * u * t * through.0 + t * t * end.0,
                            u * u * from.1 + 2.0 * u * t * through.1 + t * t * end.1,
                        ));
                    }
                    self.smoothing = (to, 0.0);
                } else {
                    self.smoothing = (through, gone);
                }
            }
            None => self.outline.push(to),
        }
        let over = self.outline.len().saturating_sub(MOST_OUTLINE);
        self.outline.drain(..over);
    }

    /// A shape brush's outline filled, closed back to where it started: each row of pixels across
    /// it, between where the outline crosses it (by the even-odd rule, or the winding one), in
    /// strips no longer than a dab can be squashed to, laid towards its opacity.
    fn fill_outline(&mut self) {
        let Some(shape) = self.brush.shape.clone() else { return };
        let points = std::mem::take(&mut self.outline);
        if points.len() < 3 {
            return;
        }
        let opacity = self.brush.dynamics.opacity.strength.clamp(0.0, 1.0);
        let (top, bottom) = points.iter().fold((f32::MAX, f32::MIN), |(t, b), p| (t.min(p.1), b.max(p.1)));
        let (top, bottom) = (top.floor().max(-1.0e6) as i32, bottom.ceil().min(1.0e6) as i32);
        let edges: Vec<((f32, f32), (f32, f32))> =
            points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| (*a, *b)).collect();
        for row in top..bottom {
            if self.out.len() >= MOST_DABS {
                return;
            }
            let y = row as f32 + 0.5;
            // where the outline crosses the row's middle, and which way
            let mut crossings: Vec<(f32, i32)> = edges
                .iter()
                .filter(|(a, b)| (a.1 <= y) != (b.1 <= y))
                .map(|(a, b)| (a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0), if b.1 > a.1 { 1 } else { -1 }))
                .collect();
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut winding = 0;
            for pair in crossings.windows(2) {
                let ((x0, way), (x1, _)) = (pair[0], pair[1]);
                winding += way;
                let inside = if shape.winding { winding != 0 } else { winding % 2 != 0 };
                if !inside {
                    continue;
                }
                let (x0, x1) = if shape.hard { (x0.round(), x1.round()) } else { (x0, x1) };
                let mut x = x0;
                while x < x1 && self.out.len() < MOST_DABS {
                    let end = (x + 1.0 / LEAST_SQUASH).min(x1);
                    let long = (end - x).max(1.0);
                    self.out.push(Dab {
                        x: (x + end) * 0.5,
                        y,
                        diameter: long,
                        angle: 0.0,
                        ratio: (1.0 / long).clamp(LEAST_SQUASH, 1.0),
                        opacity,
                        flow: 1.0,
                        tip: LINE_TIP,
                        strength: 1.0,
                        texture_offset: self.texture_offset,
                        ..Dab::default()
                    });
                    x = end;
                }
            }
        }
    }

    /// Where a clone stroke starting at `start` picks up from, from where it lies: from its source
    /// (kept that far apart from then on, as the source moves with the brush); none set, from where
    /// it lies.
    fn clone_start(&mut self, start: (f32, f32)) -> (f32, f32) {
        let moves = self.brush.clone.as_ref().is_some_and(|c| c.move_source);
        CLONE_SOURCE.with(|c| {
            let (source, offset) = c.get();
            let Some((sx, sy)) = source else { return (0.0, 0.0) };
            if !moves {
                return (sx, sy);
            }
            let offset = offset.unwrap_or((sx - start.0, sy - start.1));
            c.set((source, Some(offset)));
            offset
        })
    }

    /// The stroke's last dabs as it ends: a clone stroke that never moved (a tap) sets where the
    /// brush picks up from instead.
    pub fn finish(&mut self) {
        self.fill_outline();
        if self.brush.clone.is_some() && !self.moving {
            if let Some(f) = self.held.first() {
                let at = (f.x, f.y);
                CLONE_SOURCE.with(|c| c.set((Some(at), None)));
            }
            self.held.clear();
        }
    }

    /// How far from `at` to `to` (0..1) the airbrush's next dab comes, if it comes before `to`:
    /// once its interval (scaled by its rate option where `at` is) has passed since the last.
    fn airbrushed(&mut self, at: &Point, to: &Point) -> Option<f32> {
        let airbrush = self.brush.airbrush.clone()?;
        let info = self.info(at, self.main.dabs);
        let seed = self.begin.seed;
        let mut fuzz = Fuzz { random: &mut self.random, seed };
        let scale = if airbrush.rate.checked { airbrush.rate.size_like(&info, &mut fuzz, true) } else { 1.0 };
        if scale <= 0.0 {
            return None;
        }
        let need = airbrush.interval / scale - (at.time - self.main.last_ms);
        let span = to.time - at.time;
        if need <= 0.0 {
            Some(0.0)
        } else if span > 0.0 && need <= span {
            Some(need / span)
        } else {
            None
        }
    }

    /// A dab of a line from `a` to `b`, `width` across (a dot where they meet), with the square tip,
    /// `opacity`, its colour its own if given.
    fn line(&mut self, a: (f32, f32), b: (f32, f32), width: f32, opacity: f32, colour: Option<[f32; 3]>) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = (dx * dx + dy * dy).sqrt();
        if !length.is_finite() || opacity <= 0.0 || self.out.len() >= MOST_DABS {
            return;
        }
        // the app squashes a dab no further than a twentieth: a longer line goes in pieces
        let pieces = ((length * LEAST_SQUASH / width.max(1e-3)).ceil() as usize).clamp(1, 1000);
        // a sketch or bristle brush that doesn't smooth its lines draws them hard: what a line
        // covers nearly half of or more, fully, the rest not at all (a line between two rows of
        // pixels lighting both rather than neither)
        let hard = self.brush.sketch.as_ref().is_some_and(|s| !s.antialias)
            || self.brush.hairy.as_ref().is_some_and(|h| !h.antialias);
        // a line a pixel thin (or thinner) covers each pixel as Krita's does: as much as a pixel wide
        // line covers of it, smoothed (a thinner one fainter); or the pixels nearest it, hard
        if width <= 1.5 {
            let n = ((length / (2.0 * crate::brush::THIN_PIECE)).ceil() as usize).clamp(1, 1000);
            let r = (length / n as f32 * 0.5).max(0.5);
            // as wide as two pixels once the app's dab reaches past it
            let ratio = ((1.0 - std::f32::consts::FRAC_1_SQRT_2) / r).clamp(LEAST_SQUASH, 1.0);
            // (a hard line exactly between two rows takes the lower, as rounding does)
            let nudge = if hard { 1e-3 } else { 0.0 };
            for i in 0..n {
                if self.out.len() >= MOST_DABS {
                    return;
                }
                let t = (i as f32 + 0.5) / n as f32;
                self.out.push(Dab {
                    x: a.0 + dx * t + nudge,
                    y: a.1 + dy * t + nudge,
                    diameter: 2.0 * r,
                    angle: dy.atan2(dx),
                    ratio,
                    opacity: if hard { opacity } else { opacity * width.min(1.0) },
                    flow: 1.0,
                    tip: if hard { THIN_HARD_TIP } else { THIN_TIP },
                    flags: if colour.is_some() { OWN_COLOUR } else { 0 },
                    colour: colour.unwrap_or([0.0; 3]),
                    strength: 1.0,
                    texture_offset: self.texture_offset,
                    threshold: if hard { 0.5 } else { 0.0 },
                    ..Dab::default()
                });
            }
            return;
        }
        let piece = length / pieces as f32;
        let long = piece.max(width);
        // (the app lays a picture over its dab and as far again as this past it each way: the dab
        // that much narrower, so the line covers its own width)
        let width = (width - 2.0 * std::f32::consts::FRAC_1_SQRT_2).max(0.02);
        for i in 0..pieces {
            if self.out.len() >= MOST_DABS {
                return;
            }
            let t = (i as f32 + 0.5) / pieces as f32;
            self.out.push(Dab {
                x: a.0 + dx * t,
                y: a.1 + dy * t,
                diameter: long,
                angle: dy.atan2(dx),
                ratio: (width.min(long) / long).clamp(LEAST_SQUASH, 1.0),
                opacity,
                flow: 1.0,
                tip: LINE_TIP,
                flags: if colour.is_some() { OWN_COLOUR } else { 0 },
                colour: colour.unwrap_or([0.0; 3]),
                strength: 1.0,
                texture_offset: self.texture_offset,
                threshold: if hard { 0.55 } else { 0.0 },
                ..Dab::default()
            });
        }
    }

    /// A sketch brush's move from `last` to `p`: a line between them if it says, and a line from
    /// `p` towards each point it passed within its reach, as likely as its density says.
    fn sketch_move(&mut self, last: &Point, p: &Point) {
        let brush = Rc::clone(&self.brush);
        let Some(sketch) = &brush.sketch else { return };
        self.points.push((p.x, p.y));
        let over = self.points.len().saturating_sub(MOST_SKETCH_POINTS);
        self.points.drain(..over);
        let info = self.info(p, self.dabs);
        let seed = self.begin.seed;
        let k = self.size_factor();
        let mut fuzz = Fuzz { random: &mut self.random, seed };
        let one = |o: &CurveOption, fuzz: &mut Fuzz<'_>| if o.checked { o.size_like(&info, fuzz, true) } else { 1.0 };
        let scale = one(&brush.dynamics.size, &mut fuzz) * k;
        if scale * brush.tip.width <= 0.01 {
            return;
        }
        let width = (one(&sketch.line_width_option, &mut fuzz) * sketch.line_width).max(0.9);
        let offset = one(&sketch.offset_option, &mut fuzz) * sketch.offset * 0.01;
        let density = one(&sketch.density_option, &mut fuzz) * sketch.probability;
        let opacity = if brush.dynamics.opacity.checked {
            brush.dynamics.opacity.size_like(&info, &mut fuzz, true)
        } else {
            brush.dynamics.opacity.strength
        };
        // its reach: half the tip across, at its size
        let reach = brush.sketch_radius * scale;
        let threshold = reach * reach;
        let points = self.points.clone();
        if sketch.connect {
            self.line((last.x, last.y), (p.x, p.y), width, opacity, None);
        }
        let mut chance = 1.0 - density;
        for (qx, qy) in points {
            let (dx, dy) = (qx - p.x, qy - p.y);
            let distance = dx * dx + dy * dy;
            if distance >= threshold {
                continue;
            }
            if sketch.distance_density {
                chance = distance / (threshold * density).max(1e-6);
            }
            if self.random.next() < chance {
                continue;
            }
            let colour = sketch.random_rgb.then(|| {
                let c = self.begin.colour;
                [self.random.next() * c[0], self.random.next() * c[1], self.random.next() * c[2]]
            });
            let mut alpha = opacity;
            if sketch.distance_opacity {
                alpha *= (1.0 - distance / threshold).round();
            }
            if sketch.random_opacity {
                alpha *= self.random.next();
            }
            let (ox, oy) = (dx * offset, dy * offset);
            let to = if sketch.magnetify { (qx - ox, qy - oy) } else { (p.x - ox, p.y - oy) };
            self.line((p.x + ox, p.y + oy), to, width, alpha, colour);
        }
        self.dabs += 1;
    }

    /// A spray brush's dab at `p`: its particles.
    fn spray_at(&mut self, p: &Point, spray: &crate::others::Spray) {
        let brush = Rc::clone(&self.brush);
        let info = self.info(p, self.main.dabs);
        let seed = self.begin.seed;
        let k = self.size_factor();
        let mut fuzz = Fuzz { random: &mut self.random, seed };
        let one = |o: &CurveOption, fuzz: &mut Fuzz<'_>| if o.checked { o.size_like(&info, fuzz, true) } else { 1.0 };
        let scale = one(&brush.dynamics.size, &mut fuzz) * k;
        let rotation = turn_of(&brush.dynamics.rotation, &info, &mut fuzz, self.begin.turn);
        let opacity = if brush.dynamics.opacity.checked {
            brush.dynamics.opacity.size_like(&info, &mut fuzz, true)
        } else {
            brush.dynamics.opacity.strength
        };
        let radius = spray.diameter * 0.5 * scale;
        let (mut x, mut y) = (p.x, p.y);
        if let Some(amount) = spray.jitter {
            x += (2.0 * radius * self.random.next() - radius) * amount;
            y += (2.0 * radius * self.random.next() - radius) * amount;
        }
        let count = match spray.coverage {
            Some(c) => c * PI * radius * radius,
            None => spray.count,
        };
        let count = (count.max(0.0) as usize).min(MOST_DABS.saturating_sub(self.out.len()));
        let (turn_sin, turn_cos) = (spray.rotation.to_radians() - rotation).sin_cos();
        let (pw, ph) = spray.particle(spray.diameter);
        // particles of the brush's own tip: at its size, turned as it is, a pipe's cell picked
        // as its dabs pick them
        let own = spray.own_tip.then(|| {
            let tip = &brush.tip;
            let index = match &tip.tips {
                Tips::Auto { levels, .. } => levels.saturating_sub(1),
                Tips::Images { count, pipe: Some(pick) } => {
                    pipe_cell(pick, &mut self.pipe, &info, *count, self.main.dabs, &mut self.random)
                }
                Tips::Images { .. } => 0,
            };
            (tip.width, tip.height, tip.angle, index)
        });
        let mut colour = None;
        for _ in 0..count {
            let angle = self.random.next() * TAU;
            let length = if spray.gaussian {
                let n = gaussian(&mut self.random) * spray.deviation;
                if spray.centre_biased { n.abs().min(1.0) } else { n.abs().min(1.0).sqrt() }
            } else if spray.centre_biased {
                self.random.next()
            } else {
                self.random.next().sqrt()
            };
            let mut turn = spray.turn.to_radians();
            if let Some(w) = spray.random_turn {
                turn += self.random.next() * TAU * w;
            }
            if let Some(w) = spray.follow_cursor {
                turn += (angle - turn) * w;
            }
            if let Some(w) = spray.follow_angle {
                turn += (info.drawing_angle - turn) * w;
            }
            let size = if spray.random_size { self.random.next() } else { 1.0 };
            let (nx, ny) = (radius * angle.cos() * length, radius * angle.sin() * length * spray.aspect);
            let (nx, ny) =
                ((nx * turn_cos - ny * turn_sin) * spray.scale, (nx * turn_sin + ny * turn_cos) * spray.scale);
            // its colour: the colour moved at random, once or for each particle
            if colour.is_none() || spray.per_particle {
                let mut c = self.begin.colour;
                if spray.mix_background {
                    let w = p.pressure;
                    c = [0, 1, 2].map(|j| c[j] * w + self.begin.secondary[j] * (1.0 - w));
                }
                if let Some((h, s, v)) = spray.random_hsv {
                    let [mut ch, mut cs, mut cv] = hsv(c);
                    ch = (ch + h / 360.0 * self.random.next()).rem_euclid(1.0);
                    cs = (cs + s / 100.0 * self.random.next()).clamp(0.0, 1.0);
                    cv = (cv + v / 100.0 * self.random.next()).clamp(0.0, 1.0);
                    c = rgb([ch, cs, cv]);
                }
                colour = Some(c);
            }
            let alpha = if spray.random_opacity { opacity * self.random.next() } else { opacity };
            let (w, h) = match (own, spray.shape) {
                (Some((tw, th, ..)), _) => ((tw * size).max(1.0), (th * size).max(1.0)),
                (None, 2 | 3) => (1.0, 1.0),
                (None, _) => ((pw * size).max(1.0), (ph * size).max(1.0)),
            };
            let long = w.max(h);
            self.out.push(Dab {
                x: x + nx,
                y: y + ny,
                diameter: long,
                angle: -turn - own.map_or(0.0, |o| o.2),
                ratio: (w.min(h) / long).clamp(0.01, 1.0),
                opacity: alpha,
                flow: 1.0,
                // an ellipse the round tip, the rest the square, or the brush's own
                tip: own.map_or(u32::from(spray.shape != 0), |o| o.3),
                flags: OWN_COLOUR,
                colour: colour.unwrap_or(self.begin.colour),
                strength: 1.0,
                texture_offset: self.texture_offset,
                ..Dab::default()
            });
        }
        // the next dab a share of its diameter on
        let step = if spray.diameter * 0.5 > 1.0 { spray.diameter * 0.5 * spray.spacing } else { 1.0 };
        self.main.spacing = (step.max(LEAST_SPACING), step.max(LEAST_SPACING));
        self.main.turn = 0.0;
        self.main.dabs += 1;
        self.main.last_ms = p.time;
        self.dabs += 1;
    }

    /// The pen where it lies, for the options.
    fn info(&self, p: &Point, dabs: u32) -> Info {
        Info {
            pressure: p.pressure,
            pressure_in: p.pressure_in,
            x_tilt: p.x_tilt,
            y_tilt: p.y_tilt,
            rotation: p.rotation,
            speed: p.speed,
            drawing_angle: self.angle,
            locked_angle: self.locked,
            distance: self.length,
            time: p.time,
            dab: dabs,
        }
    }

    /// A move of a brush that draws at each move (a bristle, curve or particle brush) from `a` to
    /// `b`.
    fn line_move(&mut self, a: &Point, b: &Point) {
        if self.brush.hairy.is_some() {
            self.hairy_move(a, b);
        } else if self.brush.curves.is_some() {
            self.curve_move(a, b);
        } else if self.brush.particles.is_some() {
            self.particle_move(b);
        }
        self.dabs += 1;
    }

    /// The options' values a line-drawing brush takes at `p`: its size (the app's slider in it),
    /// its turn (radians) and its opacity.
    fn size_turn_opacity(&mut self, p: &Point) -> (f32, f32, f32) {
        let brush = Rc::clone(&self.brush);
        let info = self.info(p, self.dabs);
        let seed = self.begin.seed;
        let k = self.size_factor();
        let mut fuzz = Fuzz { random: &mut self.random, seed };
        let d = &brush.dynamics;
        let size = if d.size.checked { d.size.size_like(&info, &mut fuzz, true) } else { 1.0 } * k;
        let turn = turn_of(&d.rotation, &info, &mut fuzz, self.begin.turn);
        let opacity = if d.opacity.checked { d.opacity.size_like(&info, &mut fuzz, true) } else { d.opacity.strength };
        (size, turn, opacity.clamp(0.0, 1.0))
    }

    /// A dyna brush's weight pulled towards `p` (where the pen was), drawing as it goes: circles
    /// growing with its speed, a ribbon (or its outline) as wide as it is slow, or lines.
    fn dyna_move(&mut self, p: &Point) {
        let brush = Rc::clone(&self.brush);
        let Some(d) = &brush.dyna else { return };
        // where the pen was, in the canvas as 1 each way
        let (w, h) = self.canvas.filter(|(w, h)| *w > 0.0 && *h > 0.0).unwrap_or((1000.0, 1000.0));
        let to = (p.x / w, p.y / h);
        let Some(weight) = self.dyna.as_mut() else {
            self.dyna = Some(crate::others::DynaWeight { at: to, was: to, ..Default::default() });
            self.dyna_lines = vec![(p.x, p.y); d.line_count];
            return;
        };
        if !weight.pull(d, to) {
            return;
        }
        let weight = weight.clone();
        let opacity = brush.dynamics.opacity.strength.clamp(0.0, 1.0);
        let wide = ((d.width_range - weight.speed) * d.width).max(0.000_01);
        let across = (weight.across.0 * wide, weight.across.1 * wide);
        let before = self.dyna_across;
        let px = |q: (f32, f32)| (q.0 * w, q.1 * h);
        let (was, now) = (px(weight.was), px(weight.at));
        let was_right = px((weight.was.0 + before.0, weight.was.1 + before.1));
        let was_left = px((weight.was.0 - before.0, weight.was.1 - before.1));
        let now_left = px((weight.at.0 - across.0, weight.at.1 - across.1));
        let now_right = px((weight.at.0 + across.0, weight.at.1 + across.1));
        if d.line {
            self.line(was, now, 1.0, opacity, None);
        }
        match d.action {
            0 => {
                // a circle as big as the weight is fast (and one where it got to)
                let speed = (weight.velocity.0 * w).hypot(weight.velocity.1 * h).clamp(0.0, d.diameter.max(0.0));
                let radius = (d.diameter * 0.5 + speed).trunc().max(0.5);
                let circles: &[(f32, f32)] = if d.two_circles { &[was, now] } else { &[was] };
                for &(x, y) in circles {
                    if self.out.len() < MOST_DABS {
                        self.out.push(Dab {
                            x,
                            y,
                            diameter: 2.0 * radius,
                            opacity,
                            flow: 1.0,
                            tip: 0,
                            strength: 1.0,
                            texture_offset: self.texture_offset,
                            ..Dab::default()
                        });
                    }
                }
            }
            1 => {
                // the ribbon filled: lines along it, side by side across it
                let span = (was_right.0 - was_left.0)
                    .hypot(was_right.1 - was_left.1)
                    .max((now_right.0 - now_left.0).hypot(now_right.1 - now_left.1));
                let n = (span.ceil() as usize).clamp(1, 512);
                for k in 0..=n {
                    let t = k as f32 / n as f32;
                    let a = (was_left.0 + (was_right.0 - was_left.0) * t, was_left.1 + (was_right.1 - was_left.1) * t);
                    let b = (now_left.0 + (now_right.0 - now_left.0) * t, now_left.1 + (now_right.1 - now_left.1) * t);
                    self.line(a, b, 1.5, opacity, None);
                }
            }
            2 => {
                for (a, b) in
                    [(was_right, was_left), (was_left, now_left), (now_left, now_right), (now_right, was_right)]
                {
                    self.line(a, b, 1.0, opacity, None);
                }
            }
            3 => {
                let half = (d.line_count / 2) as f32;
                for i in 0..self.dyna_lines.len() {
                    let k = (i as f32 - half) * d.line_spacing * weight.pull;
                    let to = (now.0 + weight.across.0 * k, now.1 + weight.across.1 * k);
                    let from = self.dyna_lines[i];
                    self.line(from, to, 1.0, opacity, None);
                    self.dyna_lines[i] = to;
                }
            }
            _ => {}
        }
        self.dyna_across = across;
        self.dabs += 1;
    }

    /// A bristle brush's move from `a` to `b`: each bristle a line from where it was to where it is
    /// now (from where it would have been at `a`, unless its path is connected), a pixel wide.
    fn hairy_move(&mut self, a: &Point, b: &Point) {
        let brush = Rc::clone(&self.brush);
        let Some(h) = &brush.hairy else { return };
        let (size, turn, opacity) = self.size_turn_opacity(b);
        let mut scale = size * h.scale;
        self.hairy_moves += 1;
        let first = self.hairy_moves == 1;
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        // the mouse's pressure: how slowly it moves, kept smooth
        let mut mouse = 1.0;
        if h.mouse_pressure {
            let factor = (1.0 - (dx * dx + dy * dy).sqrt() / 20.0).max(0.0);
            let kept = (4.0 * self.old_pressure + 0.02 + factor) / 5.0;
            self.old_pressure = kept;
            mouse = 1.0 - kept;
            scale *= mouse;
        }
        let pressure = mouse * b.pressure * 2.0;
        let threshold = 1.0 - b.pressure;
        let shear = pressure * h.shear;
        let (sin, cos) = (-turn).sin_cos();
        let paint = self.begin.colour;
        // soaking its ink up: each bristle takes the colour under it where the stroke starts
        if first
            && h.ink.as_ref().is_some_and(|i| i.soak)
            && let Some(under) = &self.under
        {
            for (state, bristle) in self.bristles.iter_mut().zip(&h.bristles) {
                let (x, y) = ((bristle.x + a.x).round() as i32, (bristle.y + a.y).round() as i32);
                state.colour = Some(under.at(x, y).unwrap_or([0.0; 3]));
            }
        }
        for (i, bristle) in h.bristles.iter().enumerate() {
            let rx = (self.random.next() * 2.0 - 1.0) * h.random;
            let ry = (self.random.next() * 2.0 - 1.0) * h.random;
            // sheared, shaken, scaled, turned
            let (sx, sy) = (bristle.x + shear * bristle.y, bristle.y + shear * bristle.x);
            let (tx, ty) = ((sx + rx) * scale, (sy + ry) * scale);
            let (fx, fy) = (tx * cos - ty * sin, tx * sin + ty * cos);
            let Some(state) = self.bristles.get_mut(i) else { continue };
            let (fx1, fy1) = if first || !h.connected { (fx, fy) } else { (state.x, state.y) };
            state.x = fx;
            state.y = fy;
            if h.threshold && bristle.length < threshold {
                continue;
            }
            // (Krita lays each point of a bristle's line over the pixel it is the corner of: half a
            // pixel on from a line through the middles)
            let from = (a.x + fx1 + 0.5, a.y + fy1 + 0.5);
            let to = (b.x + fx + 0.5, b.y + fy + 0.5);
            let pixels = (((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt().ceil() as u32).max(1);
            let base = state.colour.or(bristle.colour);
            let Some(ink) = h.ink.as_ref().filter(|i| !i.depletion.is_empty()) else {
                self.line(from, to, 1.0, bristle.length * opacity, base);
                continue;
            };
            // the ink as it runs out pixel by pixel along the line: at pixel `k` of it, its
            // opacity and colour (the ink it has the last pixel's leaving)
            let last = ink.depletion.len() - 1;
            let (counter, ink_then) = (state.counter as usize, state.ink);
            let at = |k: usize| -> (f32, Option<[f32; 3]>) {
                let depletion = ink.depletion[(counter + k).min(last)];
                let left = if k == 0 { ink_then } else { 1.0 - ink.depletion[(counter + k - 1).min(last)] };
                let weighed = |[pw, lw, iw, dw]: [f32; 4]| {
                    pressure * pw + bristle.length * lw + left * iw + (1.0 - depletion) * dw
                };
                let mut colour = base;
                if ink.saturation {
                    let s = ink.weights.map_or(pressure * bristle.length * left * (1.0 - depletion), weighed) - 1.0;
                    let [ch, cs, cv] = hsv(colour.unwrap_or(paint));
                    let cs = if s < 0.0 { cs * (1.0 + s) } else { cs + (1.0 - cs) * s };
                    colour = Some(rgb([ch, cs.clamp(0.0, 1.0), cv]));
                }
                let alpha = if ink.opacity {
                    ink.weights.map_or(bristle.length * left, weighed).clamp(0.0, 1.0)
                } else {
                    bristle.length
                };
                (alpha, colour)
            };
            // pieces as long as the ink stays within a 64th
            let n = pixels as usize;
            let mut start = 0;
            let (mut alpha0, _) = at(0);
            for k in 1..=n {
                let next = (k < n).then(|| at(k).0);
                if next.is_some_and(|a| (a - alpha0).abs() <= 1.0 / 64.0) {
                    continue;
                }
                let (alpha, colour) = at((start + k) / 2);
                let (t0, t1) = (start as f32 / n as f32, k as f32 / n as f32);
                let p0 = (from.0 + (to.0 - from.0) * t0, from.1 + (to.1 - from.1) * t0);
                let p1 = (from.0 + (to.0 - from.0) * t1, from.1 + (to.1 - from.1) * t1);
                self.line(p0, p1, 1.0, alpha * opacity, colour);
                start = k;
                alpha0 = next.unwrap_or(alpha0);
            }
            let Some(state) = self.bristles.get_mut(i) else { continue };
            state.ink = 1.0 - ink.depletion[(counter + n - 1).min(last)];
            state.counter = state.counter.saturating_add(pixels);
        }
    }

    /// A curve brush's move from `a` to `b`: a line between them if it says; once it has passed
    /// as many points as it keeps, a curve from the first to the last through those between.
    fn curve_move(&mut self, a: &Point, b: &Point) {
        let brush = Rc::clone(&self.brush);
        let Some(c) = &brush.curves else { return };
        let info = self.info(b, self.dabs);
        let seed = self.begin.seed;
        let mut fuzz = Fuzz { random: &mut self.random, seed };
        let one = |o: &CurveOption, fuzz: &mut Fuzz<'_>| if o.checked { o.size_like(&info, fuzz, true) } else { 1.0 };
        let width = (one(&c.line_width_option, &mut fuzz) * c.line_width).max(1.0);
        let curve_opacity = one(&c.opacity_option, &mut fuzz) * c.opacity;
        let d = &brush.dynamics;
        let opacity = if d.opacity.checked { d.opacity.size_like(&info, &mut fuzz, true) } else { d.opacity.strength };
        self.points.push((b.x, b.y));
        let over = self.points.len().saturating_sub(c.history);
        self.points.drain(..over);
        if c.connection {
            self.line((a.x, a.y), (b.x, b.y), width, opacity, None);
        }
        if self.points.len() < c.history {
            return;
        }
        let points = self.points.clone();
        let (first, last) = (points[0], points[points.len() - 1]);
        let at = |t: f32| -> (f32, f32) {
            let u = 1.0 - t;
            if c.smoothing {
                let m = points[c.history / 2];
                (
                    u * u * first.0 + 2.0 * u * t * m.0 + t * t * last.0,
                    u * u * first.1 + 2.0 * u * t * m.1 + t * t * last.1,
                )
            } else {
                let step = c.history / 3;
                let (p1, p2) = (points[step], points[2 * step]);
                let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                (
                    w[0] * first.0 + w[1] * p1.0 + w[2] * p2.0 + w[3] * last.0,
                    w[0] * first.1 + w[1] * p1.1 + w[2] * p2.1 + w[3] * last.1,
                )
            }
        };
        const PIECES: u32 = 24;
        let mut from = first;
        for i in 1..=PIECES {
            let to = at(i as f32 / PIECES as f32);
            self.line(from, to, width, opacity * curve_opacity, None);
            from = to;
        }
    }

    /// A particle brush's move to `p`: each particle pulled towards it a few steps, each step a
    /// dot of the brush's weight where the particle got to.
    fn particle_move(&mut self, p: &Point) {
        let brush = Rc::clone(&self.brush);
        let Some(pt) = &brush.particles else { return };
        // they start where the stroke does, each pulled harder than the last
        if self.particles.is_empty() {
            self.particles = (0..pt.count)
                .map(|i| Particle { at: (p.x, p.y), next: (p.x, p.y), pull: (i + pt.iterations) as f32 * 0.5 })
                .collect();
        }
        let opacity = brush.dynamics.opacity.strength.clamp(0.0, 1.0) * pt.weight;
        const TIME: f32 = 0.000_03;
        for _ in 0..pt.iterations {
            for j in 0..self.particles.len() {
                let q = &mut self.particles[j];
                let pull = ((p.x - q.at.0) * pt.scale.0 * q.pull, (p.y - q.at.1) * pt.scale.1 * q.pull);
                q.next = ((q.next.0 + pull.0) * pt.gravity, (q.next.1 + pull.1) * pt.gravity);
                q.at = (q.at.0 + q.next.0 * TIME, q.at.1 + q.next.1 * TIME);
                let at = q.at;
                // (one whose pull grows without end is drawn no more)
                if at.0.abs() < 1e7 && at.1.abs() < 1e7 && self.out.len() < MOST_DABS {
                    self.out.push(Dab {
                        x: at.0,
                        y: at.1,
                        diameter: 1.0,
                        opacity,
                        flow: 1.0,
                        tip: LINE_TIP,
                        strength: 1.0,
                        texture_offset: self.texture_offset,
                        ..Dab::default()
                    });
                }
            }
        }
    }

    /// A hatching brush's dab at `p`: the hatching's lines across the dab's box, each as thick as
    /// it says, crosshatched as it says, kept inside the dab's tip: in pieces, each as opaque as the
    /// tip is where it lies.
    fn hatch_at(&mut self, p: &Point, hatching: &crate::others::Hatching) {
        let brush = Rc::clone(&self.brush);
        let Some(tip) = &brush.hatch_tip else { return };
        let info = self.info(p, self.main.dabs);
        let seed = self.begin.seed;
        let k = self.size_factor();
        let mut fuzz = Fuzz { random: &mut self.random, seed };
        let d = &brush.dynamics;
        let one = |o: &CurveOption, fuzz: &mut Fuzz<'_>| if o.checked { o.size_like(&info, fuzz, true) } else { 1.0 };
        let scale = one(&d.size, &mut fuzz) * k;
        let opacity = if d.opacity.checked { d.opacity.size_like(&info, &mut fuzz, true) } else { d.opacity.strength };
        let angle_value = one(&hatching.angle_option, &mut fuzz);
        let cross_value = one(&hatching.crosshatching_option, &mut fuzz);
        let separation_value = one(&hatching.separation_option, &mut fuzz);
        let thickness_value = one(&hatching.thickness_option, &mut fuzz);
        let (dw, dh) = (tip.width * scale, tip.height * scale);
        // the next dab as the tip spaces it
        let (mw, mh) = measure(tip, dw, dh);
        let spacing = spacing_of(tip, mw.max(1e-3), mh.max(1e-3), 1.0, false);
        self.main.spacing = spacing;
        self.main.turn = 0.0;
        self.main.dabs += 1;
        self.main.last_ms = p.time;
        self.dabs += 1;
        if dw <= 0.01 || dh <= 0.01 {
            return;
        }
        // the dab's box, on whole pixels
        let (w, h) = (dw.ceil(), dh.ceil());
        let (x0, y0) = ((p.x - w * 0.5).round(), (p.y - h * 0.5).round());
        let thickness = (hatching.thickness * thickness_value).round().max(1.0);
        let separation = if hatching.separation_option.checked {
            hatching.separation_at(separation_value)
        } else {
            hatching.separation
        };
        let (cx, cy) = (x0 + w * 0.5, y0 + h * 0.5);
        if hatching.opaque_background {
            // the secondary colour behind them, in the tip's own shape
            let long = dw.max(dh);
            self.out.push(Dab {
                x: cx,
                y: cy,
                diameter: long,
                angle: tip.angle,
                ratio: (dw.min(dh) / long).clamp(0.01, 1.0),
                opacity,
                flow: 1.0,
                tip: LINE_TIP + 1,
                flags: OWN_COLOUR,
                colour: self.begin.secondary,
                strength: 1.0,
                texture_offset: self.texture_offset,
                ..Dab::default()
            });
        }
        // how much of the tip is at a point on the canvas
        let (turn_sin, turn_cos) = (-tip.angle).sin_cos();
        let cover = |x: f32, y: f32| -> f32 {
            let Some(m) = tip.masks.first() else { return 1.0 };
            let (u, v) = (x - cx, y - cy);
            let (u, v) = (u * turn_cos - v * turn_sin, u * turn_sin + v * turn_cos);
            let (fu, fv) = (u / dw + 0.5, v / dh + 0.5);
            if !(0.0..1.0).contains(&fu) || !(0.0..1.0).contains(&fv) || m.width == 0 || m.height == 0 {
                return 0.0;
            }
            let (i, j) = ((fu * m.width as f32) as usize, (fv * m.height as f32) as usize);
            m.pixels.get(j * m.width as usize + i).map_or(1.0, |&a| f32::from(a) / 255.0)
        };
        // pieces short enough to follow the tip's edge
        let step = (dw.min(dh) / 24.0).max(1.5);
        // the passes: a crosshatching's by its style and value, then the hatching's own
        let mut angles = Vec::new();
        let cross = hatching.crosshatching_option.checked;
        match (hatching.style, cross) {
            (1, true) if cross_value > 0.5 => angles.push(hatching.spun(90.0)),
            (1, false) => angles.push(hatching.spun(90.0)),
            (2 | 3, true) => {
                let (first, then) = if hatching.style == 2 { (-45.0, 45.0) } else { (45.0, -45.0) };
                if cross_value > 0.33 {
                    angles.push(hatching.spun(first));
                }
                if cross_value > 0.67 {
                    angles.push(hatching.spun(then));
                }
            }
            (2 | 3, false) => {
                let (first, then) = if hatching.style == 2 { (-45.0, 45.0) } else { (45.0, -45.0) };
                angles.push(hatching.spun(first));
                angles.push(hatching.spun(then));
            }
            (4, true) => angles.push(hatching.spun(cross_value * 360.0)),
            (4, false) => angles.push(hatching.spun(-10.0)),
            _ => {}
        }
        if hatching.angle_option.checked {
            angles.push(hatching.spun(angle_value * 360.0 + hatching.angle));
        } else if hatching.style != 4 {
            angles.push(hatching.angle);
        }
        for angle in angles {
            // Krita's lines come out half as wide across as their thickness says, whatever their
            // angle (as its own strokes show)
            let across = thickness * 0.5;
            for (a, b) in hatch_lines(hatching, angle, separation, (x0, y0, w, h)) {
                let length = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                let pieces = ((length / step).ceil() as usize).clamp(1, 4096);
                let point = |t: f32| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                // each piece halved where the tip's edge crosses it, down to a pixel
                let mut left: Vec<(f32, f32, f32, f32)> = (0..pieces)
                    .rev()
                    .map(|i| {
                        let (t0, t1) = (i as f32 / pieces as f32, (i + 1) as f32 / pieces as f32);
                        let (p0, p1) = (point(t0), point(t1));
                        (t0, t1, cover(p0.0, p0.1), cover(p1.0, p1.1))
                    })
                    .collect();
                while let Some((t0, t1, c0, c1)) = left.pop() {
                    if (c0 - c1).abs() > 0.04 && (t1 - t0) * length > 1.0 {
                        let t = (t0 + t1) * 0.5;
                        let m = point(t);
                        let cm = cover(m.0, m.1);
                        left.push((t, t1, cm, c1));
                        left.push((t0, t, c0, cm));
                        continue;
                    }
                    let (p0, p1) = (point(t0), point(t1));
                    let c = cover((p0.0 + p1.0) * 0.5, (p0.1 + p1.1) * 0.5).max((c0 + c1) * 0.5);
                    if c > 0.002 {
                        self.line(p0, p1, across, opacity * c, None);
                    }
                }
            }
        }
    }

    /// A grid brush's dab at `p`: the cells of its grid around it, each divided, inset by its
    /// borders and drawn as its shape, in the colour (moved as it says) or picking up the one
    /// under its middle.
    fn grid_at(&mut self, p: &Point, g: &crate::others::Grid) {
        let brush = Rc::clone(&self.brush);
        let opacity = brush.dynamics.opacity.strength.clamp(0.0, 1.0);
        let grid = g.diameter * g.scale;
        let cell = (g.cell.0 * g.scale, g.cell.1 * g.scale);
        let mut divide = if g.pressure_division { (g.division as f32 * p.pressure) as i32 } else { g.division as i32 };
        divide = ((g.scale * divide as f32).round() as i32).max(1);
        // the grid anchored to the canvas
        let mut x = p.x - grid / 2.0 + cell.0 / 2.0 - g.offset.0;
        let mut y = p.y - grid / 2.0 + cell.1 / 2.0 - g.offset.1;
        x = x - x % cell.0 + g.offset.0;
        y = y - y % cell.1 + g.offset.1;
        let step = (cell.0 / divide as f32, cell.1 / divide as f32);
        let (mut vertical, mut horizontal) = g.border;
        if g.random_border {
            if vertical == horizontal {
                let r = self.random.next();
                vertical *= r;
                horizontal *= r;
            } else {
                vertical *= self.random.next();
                horizontal *= self.random.next();
            }
        }
        let rect =
            |dab: &mut Vec<Dab>, (x0, y0, w, h): (f32, f32, f32, f32), tip: u32, colour: [f32; 3], alpha: f32| {
                let long = w.max(h);
                if long <= 0.0 {
                    return;
                }
                dab.push(Dab {
                    x: x0 + w * 0.5,
                    y: y0 + h * 0.5,
                    diameter: long,
                    angle: if w >= h { 0.0 } else { FRAC_PI_2 },
                    ratio: (w.min(h) / long).clamp(0.01, 1.0),
                    opacity: alpha,
                    flow: 1.0,
                    tip,
                    flags: OWN_COLOUR,
                    colour,
                    strength: 1.0,
                    ..Dab::default()
                });
            };
        let mut out = std::mem::take(&mut self.out);
        if g.fill_background && !g.sample {
            rect(
                &mut out,
                (x.floor(), y.floor(), cell.0.ceil(), cell.1.ceil()),
                LINE_TIP,
                self.begin.secondary,
                opacity,
            );
        }
        let mut colour = None;
        let rows = (grid / step.1).ceil() as i32;
        let columns = (grid / step.0).ceil() as i32;
        'cells: for row in 0..rows {
            for column in 0..columns {
                if out.len() >= MOST_DABS {
                    break 'cells;
                }
                let (tx, ty) = (x + column as f32 * step.0 + vertical, y + row as f32 * step.1 + horizontal);
                let (tw, th) = ((step.0 - 2.0 * vertical).abs(), (step.1 - 2.0 * horizontal).abs());
                let (tx, ty) = (tx.min(tx + tw), ty.min(ty + th));
                // the colour: the stroke's mixed with the secondary, moved at random, once or for each cell
                let mut alpha = opacity;
                if colour.is_none() || g.per_cell {
                    let mut c = self.begin.colour;
                    if g.mix_background {
                        let w = p.pressure;
                        c = [0, 1, 2].map(|i| c[i] * w + self.begin.secondary[i] * (1.0 - w));
                    }
                    if let Some((hue, sat, val)) = g.random_hsv {
                        let [mut ch, mut cs, mut cv] = hsv(c);
                        ch = (ch + hue / 360.0 * self.random.next()).rem_euclid(1.0);
                        cs = (cs + sat / 100.0 * self.random.next()).clamp(0.0, 1.0);
                        cv = (cv + val / 100.0 * self.random.next()).clamp(0.0, 1.0);
                        c = rgb([ch, cs, cv]);
                    }
                    if g.random_opacity {
                        alpha *= self.random.next();
                    }
                    colour = Some((c, alpha));
                }
                let (c, a) = colour.unwrap_or((self.begin.colour, opacity));
                if g.sample {
                    // the colour under the cell's middle, laid over the cell
                    let before = out.len();
                    rect(&mut out, (tx, ty, tw, th), if g.shape == 0 { 0 } else { LINE_TIP }, c, 1.0);
                    if let Some(d) = out.get_mut(before) {
                        d.flags = MOVES_PAINT | DULLS | PICKS_HERE;
                        d.opacity = a;
                    }
                    continue;
                }
                match g.shape {
                    0 => rect(&mut out, (tx, ty, tw, th), 0, c, a * a),
                    1 => rect(&mut out, (tx, ty, tw, th), LINE_TIP, c, a),
                    _ => {
                        let width = if g.shape == 4 { 5.5 } else { 1.0 };
                        self.out = out;
                        self.line((tx + tw, ty), (tx, ty + th), width, a * a, Some(c));
                        out = std::mem::take(&mut self.out);
                    }
                }
            }
        }
        for d in &mut out {
            d.texture_offset = self.texture_offset;
        }
        self.out = out;
        // the next a cell on
        let spacing = (g.cell.0 * g.scale).max(LEAST_SPACING);
        self.main.spacing = (spacing, spacing);
        self.main.turn = 0.0;
        self.main.dabs += 1;
        self.main.last_ms = p.time;
        self.dabs += 1;
    }

    /// A dab of the main brush, or its masking brush, at `p`.
    fn dab_at(&mut self, p: &Point, second: bool) {
        let brush = Rc::clone(&self.brush);
        let k = self.size_factor();
        // a spray brush lays its particles there instead, a hatching brush its lines and a grid
        // brush its cells
        if let (Some(spray), false) = (&brush.spray, second) {
            self.spray_at(p, spray);
            return;
        }
        if let (Some(hatching), false) = (&brush.hatching, second) {
            self.hatch_at(p, hatching);
            return;
        }
        if let (Some(grid), false) = (&brush.grid, second) {
            self.grid_at(p, grid);
            return;
        }
        let (tip, dynamics, coeff) = match (second, &brush.masking) {
            (true, Some(m)) => (&m.tip, &m.dynamics, Some(m.size)),
            _ => (&brush.tip, &brush.dynamics, None),
        };
        let walker_dabs = if second { self.second.dabs } else { self.main.dabs };
        let info = self.info(p, walker_dabs);
        let seed = self.begin.seed;
        let mut fuzz = Fuzz { random: &mut self.random, seed };
        let one = |o: &CurveOption, fuzz: &mut Fuzz<'_>| if o.checked { o.size_like(&info, fuzz, true) } else { 1.0 };
        // its size: a masking brush's is a share of the main brush's own (a deform's scaled as it says)
        let mut scale = one(&dynamics.size, &mut fuzz) * k * brush.deform.as_ref().map_or(1.0, |d| d.scale);
        let (mut w, mut h) = (tip.width, tip.height);
        if let Some(c) = coeff.filter(|&c| c > 0.0) {
            let main_w = brush.tip.width.max(1e-3);
            let to = main_w * c / tip.width.max(1e-3);
            w *= to;
            h *= to;
        }
        if scale * w < 0.01 || scale * h < 0.01 {
            scale = 0.0;
        }
        let rotation = turn_of(&dynamics.rotation, &info, &mut fuzz, self.begin.turn);
        let turn = rotation + tip.angle;
        let ratio = one(&dynamics.ratio, &mut fuzz).clamp(0.01, 1.0);
        let (dw, dh) = (w * scale, h * scale * ratio);
        // scattered from the path (a deform's moved about as it says)
        let (mut x, mut y) = (p.x, p.y);
        if let (Some(j), false) = (brush.deform.as_ref().and_then(|d| d.jitter.map(|j| (j, d.diameter))), second) {
            let (amount, diameter) = j;
            x += (diameter * fuzz.random.next() - diameter * 0.5) * amount;
            y += (diameter * fuzz.random.next() - diameter * 0.5) * amount;
        }
        if dynamics.scatter.checked && (dynamics.scatter_axes.0 || dynamics.scatter_axes.1) && scale > 0.0 {
            let reach = dw.max(dh) * dynamics.scatter.size_like(&info, &mut fuzz, true);
            let jitter = (2.0 * fuzz.random.next() - 1.0) * reach;
            if dynamics.scatter_axes.0 && dynamics.scatter_axes.1 {
                let jitter_y = (2.0 * fuzz.random.next() - 1.0) * reach;
                x += jitter;
                y += jitter_y;
            } else {
                let (s, c) = self.angle.sin_cos();
                let (ax, ay) = if dynamics.scatter_axes.0 { (c, s) } else { (-s, c) };
                x += ax * jitter;
                y += ay * jitter;
            }
        }
        let mut flags = if second { FOR_SECOND } else { 0 };
        if dynamics.mirror.checked && (dynamics.mirror_axes.0 || dynamics.mirror_axes.1) {
            let flip = dynamics.mirror.size_like(&info, &mut fuzz, true) >= 0.5;
            if flip && dynamics.mirror_axes.0 {
                flags |= FLIP_ACROSS;
            }
            if flip && dynamics.mirror_axes.1 {
                flags |= FLIP_DOWN;
            }
        }
        let wash = brush.wash || second;
        let opacity = if dynamics.opacity.checked {
            dynamics.opacity.size_like(&info, &mut fuzz, !wash) * if wash { dynamics.opacity.strength } else { 1.0 }
        } else {
            dynamics.opacity.strength
        };
        let flow = if dynamics.flow.checked {
            dynamics.flow.size_like(&info, &mut fuzz, true)
        } else {
            dynamics.flow.strength
        };
        let softness =
            (dynamics.softness.varies() && !second).then(|| dynamics.softness.size_like(&info, &mut fuzz, true));
        let mut dab = Dab {
            x,
            y,
            diameter: w.max(h) * scale,
            // Krita turns its dabs the other way round
            angle: -turn,
            ratio,
            opacity,
            flow,
            tip: 0,
            flags,
            colour: [0.0; 3],
            strength: 1.0,
            texture_offset: self.texture_offset,
            lightness: 1.0,
            threshold: 0.0,
            softness: 0.0,
            paint_share: 0.0,
            radius: 0.0,
            extra: [0.0; 3],
            kind: 0,
        };
        // (a deform moving with the pen lays nothing at its first dab)
        let mut lays = true;
        // its tip: a pipe's cell, or an auto tip's drawing (noise afresh, softness as it says)
        dab.tip = match &tip.tips {
            Tips::Auto { variants, levels, .. } => {
                let v = if *variants > 1 { (fuzz.random.next() * *variants as f32) as u32 % variants } else { 0 };
                let level = softness.map_or(levels - 1, |s| (((s - 0.1) / 0.9) * (*levels - 1) as f32).round() as u32);
                v * levels + level.min(levels - 1)
            }
            Tips::Images { count, pipe: Some(pick) } => {
                let state = if second { &mut self.second_pipe } else { &mut self.pipe };
                pipe_cell(pick, state, &info, *count, walker_dabs, fuzz.random)
            }
            Tips::Images { .. } => 0,
        };
        if !second {
            // washed: the stroke keeps near the most its opacity has been lately
            if wash {
                let blend =
                    if self.average < dab.opacity { dab.opacity } else { 0.1 * dab.opacity + 0.9 * self.average };
                self.average = blend;
                if blend > dab.opacity && blend > 0.0 {
                    dab.flow *= dab.opacity / blend;
                    dab.opacity = blend;
                }
            }
            if let Some(t) = &brush.texture {
                dab.strength = t.strength.apply(&info, &mut fuzz).clamp(0.0, 1.0);
            }
            if brush.lightness.checked {
                dab.lightness = brush.lightness.size_like(&info, &mut fuzz, true).clamp(0.0, 1.0);
            }
            if let Some(smudge) = &brush.smudge {
                let one = |o: &CurveOption, otherwise: f32, fuzz: &mut Fuzz<'_>| {
                    if o.checked { o.size_like(&info, fuzz, true) } else { otherwise }
                };
                // a lightness tip's thickness, as Krita takes it first (all of it when off)
                let keeps = smudge.keeps_thickness(brush.tip.kind);
                let thickness = if keeps { one(&smudge.thickness, 1.0, &mut fuzz).clamp(0.0, 1.0) } else { 1.0 };
                let rate = one(&smudge.rate, 1.0, &mut fuzz);
                let colour_rate = one(&smudge.colour_rate, 0.0, &mut fuzz);
                let (laid, share) = smudge.laid(rate, colour_rate, dab.opacity);
                if keeps {
                    // its grey flattened as far as it is thin, laid over the thickness there at its
                    // opacity: in its place (Krita's overwrite, and with the option off), or as far
                    // as the smudge length goes, the rest of the way as far as it is thick
                    let over = if smudge.thickness.checked && smudge.thickness_mode == 2 {
                        let from = rate - 0.01;
                        from + (1.0 - from) * thickness
                    } else {
                        1.0
                    };
                    dab.lightness = thickness;
                    dab.extra[0] = if self.dabs == 0 { 0.0 } else { (dab.opacity * over).clamp(0.0, 1.0) };
                    dab.flags |= LAYS_HEIGHT;
                }
                // the first dab only marks where the next picks up from
                dab.opacity = if self.dabs == 0 { 0.0 } else { laid };
                dab.flow = 1.0;
                dab.paint_share = share;
                dab.radius = one(&smudge.radius, 0.0, &mut fuzz).max(0.0);
                dab.flags |= MOVES_PAINT;
                if smudge.dulls {
                    dab.flags |= DULLS;
                }
                if smudge.newer {
                    dab.flags |= WEIGHED;
                }
                // Krita mixes the colour in by the brush's composite op: an eraser's takes paint away
                if brush.erases {
                    dab.flags |= CLEAR_PAINT;
                }
                // without smearing alpha, what it moves goes over what is there
                if !smudge.smear_alpha {
                    dab.flags |= LAID_OVER;
                }
                if smudge.gradient.checked {
                    let g = smudge.gradient.size_like(&info, &mut fuzz, true).clamp(0.0, 1.0);
                    let from = if dab.flags & OWN_COLOUR != 0 { dab.colour } else { self.begin.colour };
                    dab.colour = [0, 1, 2].map(|i| from[i] + (self.begin.secondary[i] - from[i]) * g);
                    dab.flags |= OWN_COLOUR;
                }
            }
            // a clone, deform or filter brush's dab lays the layer's own pixels
            if brush.moves_paint() && brush.smudge.is_none() {
                dab.opacity = (dab.opacity * dab.flow).clamp(0.0, 1.0);
                dab.flow = 1.0;
                dab.paint_share = 0.0;
                dab.flags |= MOVES_PAINT;
                if brush.clone.is_some() {
                    let (ox, oy) = self.clone_offset;
                    let moves = brush.clone.as_ref().is_some_and(|c| c.move_source);
                    // from the source moved with it, or from the source itself
                    dab.extra = if moves { [ox, oy, 0.0] } else { [ox - dab.x, oy - dab.y, 0.0] };
                    dab.flags |= LAID_OVER | PICKS_FROM | PICKS_BEFORE;
                }
                if let Some(f) = &brush.filter {
                    dab.extra = [0.0, 0.0, f.blur];
                    dab.kind = f.sharpen.to_bits();
                    dab.flags |= PICKS_HERE | FILTERS | PICKS_BEFORE;
                }
                if let Some(d) = &brush.deform {
                    let count = (self.main.dabs + 1) as f32;
                    let (kind, amount) = match d.action {
                        1 | 2 => {
                            let sign = if d.action == 1 { 1.0 } else { -1.0 };
                            (1, 1.0 + sign * if d.counter { count * count / 100.0 } else { d.amount })
                        }
                        3 | 4 => {
                            let sign = if d.action == 3 { 1.0 } else { -1.0 };
                            (2, sign * if d.counter { count.to_radians() } else { PI * d.amount })
                        }
                        5 => (3, d.amount),
                        6 => (5, d.amount),
                        7 => (4, d.amount),
                        8 => (6, d.amount),
                        _ => (0, 0.0),
                    };
                    // moved: as far as the pen went since the last dab (the first lays nothing)
                    if kind == 3 {
                        match self.main.at {
                            Some(last) => dab.extra = [(p.x - last.x) * amount, (p.y - last.y) * amount, 1.0],
                            None => lays = false,
                        }
                    } else {
                        dab.extra = [0.0, 0.0, amount];
                    }
                    dab.kind = kind | if d.bilinear { 0 } else { 16 };
                    dab.flags |= PICKS_HERE | WARPS;
                    if d.old_data {
                        dab.flags |= PICKS_BEFORE;
                    }
                }
            }
            let sharp = &brush.sharpness;
            if sharp.option.checked {
                let value = sharp.option.size_like(&info, &mut fuzz, true).clamp(0.0, 1.0);
                // (a threshold of nothing would leave nothing: the app takes 0 as none)
                dab.threshold = value.max(1e-6);
                dab.softness = sharp.softness;
                if sharp.align && sharp.option.strength > 0.0 {
                    // its corner moved towards a whole pixel as far as the value goes
                    let snap = |c: f32, side: f32| {
                        let corner = c - side * 0.5;
                        value * corner.round() + (1.0 - value) * corner + side * 0.5
                    };
                    let side = dab.diameter.round().max(1.0);
                    dab.x = snap(dab.x, side);
                    dab.y = snap(dab.y, side);
                }
            }
            if let Some(colour) = colour_of(&brush, &self.begin, &info, &mut fuzz) {
                dab.colour = colour;
                dab.flags |= OWN_COLOUR;
            }
            // a tangent normal brush's colour: the pen's lean as a normal
            if let Some(t) = &brush.tangent {
                dab.colour = t.colour(&info, self.begin.turn);
                dab.flags |= OWN_COLOUR;
            }
            // a chalk brush's ink running out with the log of its dabs
            if let Some(c) = brush.chalk.as_ref().filter(|c| c.depletion) {
                let less = -(((self.main.dabs + 1) as f32).ln() * 10.0) / 100.0;
                if c.opacity {
                    dab.opacity *= (1.0 + less).clamp(0.0, 1.0);
                }
                if c.saturation {
                    let from = if dab.flags & OWN_COLOUR != 0 { dab.colour } else { self.begin.colour };
                    let [ch, cs, cv] = hsv(from);
                    let s = 1.0 + less;
                    let cs = if s < 0.0 { cs * (1.0 + s) } else { cs + (1.0 - cs) * s };
                    dab.colour = rgb([ch, cs.clamp(0.0, 1.0), cv]);
                    dab.flags |= OWN_COLOUR;
                }
            }
        }
        if let (Some(precision), false) = (&brush.precision, second) {
            self.keep(&mut dab, precision, (dw, dh), softness.unwrap_or(1.0));
        }
        if scale > 0.0 && dab.diameter.is_finite() && lays {
            fit_to_reach(&mut dab);
            self.out.push(dab);
        }
        // the spacing this dab sets for the next: as Krita measures the tip (its box turned by its
        // own angle, at its size, not squashed by the ratio option), across and down the frame its
        // rotation option turns
        let extra = one(&dynamics.spacing, &mut Fuzz { random: &mut self.random, seed });
        let (mw, mh) = measure(tip, w * scale, h * scale);
        let spacing = spacing_of(tip, mw.max(1e-3), mh.max(1e-3), extra, dynamics.isotropic);
        let walker = if second { &mut self.second } else { &mut self.main };
        walker.spacing = spacing;
        walker.turn = if dynamics.isotropic { 0.0 } else { rotation };
        walker.dabs += 1;
        walker.last_ms = p.time;
        if !second {
            self.dabs += 1;
        }
    }
}

impl Stroke {
    /// `dab` (`size` across and down before it turns) as the brush's precision lays it: the last
    /// dab drawn afresh laid again in its place, at the whole pixel this one's box starts on, where
    /// this one is near enough to it; else this one drawn afresh, kept.
    fn keep(&mut self, dab: &mut Dab, precision: &crate::brush::Precision, size: (f32, f32), softness: f32) {
        let (s, c) = dab.angle.sin_cos();
        let (w, h) = ((size.0 * c).abs() + (size.1 * s).abs(), (size.0 * s).abs() + (size.1 * c).abs());
        let half = (w * 0.5, h * 0.5);
        let corner = (dab.x - half.0, dab.y - half.1);
        let next = Kept {
            colour: dab.colour,
            angle: dab.angle.rem_euclid(TAU),
            size: (w.ceil(), h.ceil()),
            within: (corner.0 - corner.0.floor(), corner.1 - corner.1.floor()),
            half,
            softness,
            lightness: dab.lightness,
            ratio: dab.ratio,
            tip: dab.tip,
            flips: dab.flags & (FLIP_ACROSS | FLIP_DOWN),
            diameter: dab.diameter,
        };
        let level = precision.level(w.min(h));
        match self.kept {
            Some(kept) if level < 5 && next.near(&kept, level) => {
                dab.angle = kept.angle;
                dab.diameter = kept.diameter;
                dab.ratio = kept.ratio;
                dab.x = corner.0.floor() + kept.within.0 + kept.half.0;
                dab.y = corner.1.floor() + kept.within.1 + kept.half.1;
            }
            _ => self.kept = Some(next),
        }
    }
}

/// How far past a dab the app lays its tip's picture each way (canvas pixels).
const PAST: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// `dab` made that much smaller, so that its tip's picture lies where Krita lays the tip (fainter
/// where it can't be made small enough).
fn fit_to_reach(dab: &mut Dab) {
    let half = dab.diameter * 0.5;
    let across = half * dab.ratio;
    if !(half > 0.0 && across > 0.0) {
        return;
    }
    let fitted = (half - PAST).max(0.01);
    dab.ratio = ((across - PAST).max(0.01) / fitted).clamp(LEAST_SQUASH, 1.0);
    dab.diameter = 2.0 * fitted;
    let laid = (fitted + PAST) * (fitted * dab.ratio + PAST);
    dab.opacity *= (half * across / laid).min(1.0);
}

/// A dab's turn (radians, as Krita turns its dabs) as its rotation option gives it, the canvas
/// turned `canvas` (radians) on screen: switched off, the dab turns with the canvas, so it looks
/// the same on screen; switched on, from there.
fn turn_of(option: &CurveOption, info: &Info, fuzz: &mut Fuzz<'_>, canvas: f32) -> f32 {
    if !option.checked {
        return canvas;
    }
    (1.0 - option.rotation_like(info, fuzz, -canvas / TAU, -1.0)) * PI
}

/// How far apart two ways are (radians, 0..pi), the shorter way round.
fn turned(a: f32, b: f32) -> f32 {
    ((b - a + PI).rem_euclid(TAU) - PI).abs()
}

/// `from` turned `step` (radians) towards `to`, the shorter way round.
fn towards(from: f32, step: f32, to: f32) -> f32 {
    let d = (to - from + PI).rem_euclid(TAU) - PI;
    from + step.copysign(d)
}

/// A dab's colour where its options change it: mixed with the background colour, darkened, its
/// hue, saturation and value moved; none where none does.
fn colour_of(b: &Brush, begin: &Begin, info: &Info, fuzz: &mut Fuzz<'_>) -> Option<[f32; 3]> {
    {
        if !(b.mix.checked || b.darken.checked || b.hue.checked || b.saturation.checked || b.value.checked) {
            return None;
        }
        let mut c = begin.colour;
        if b.mix.checked {
            let v = b.mix.size_like(info, fuzz, true);
            for (i, ch) in c.iter_mut().enumerate() {
                *ch = *ch * v + begin.secondary[i] * (1.0 - v);
            }
        }
        if b.darken.checked {
            let v = b.darken.size_like(info, fuzz, true);
            for ch in &mut c {
                *ch *= 1.0 - v;
            }
        }
        if b.hue.checked || b.saturation.checked || b.value.checked {
            let [mut h, mut s, mut v] = hsv(c);
            if b.hue.checked {
                h = (h + 0.5 * b.hue.rotation_like(info, fuzz, 0.0, 1.0)).rem_euclid(1.0);
            }
            let moved = |o: &CurveOption, x: f32, fuzz: &mut Fuzz<'_>| {
                let raw = o.size_like(info, fuzz, true);
                let adj = 2.0 * (raw * o.strength + 0.5 - 0.5 * o.strength) - 1.0;
                if adj >= 0.0 { x + (1.0 - x) * adj } else { x * (1.0 + adj) }
            };
            if b.saturation.checked {
                s = moved(&b.saturation, s, fuzz);
            }
            if b.value.checked {
                v = moved(&b.value, v, fuzz);
            }
            c = rgb([h, s.clamp(0.0, 1.0), v.clamp(0.0, 1.0)]);
        }
        Some(c.map(|v| v.clamp(0.0, 1.0)))
    }
}

/// A tip `w` x `h` as Krita measures it for its spacing: the box it fills turned by its own angle.
fn measure(tip: &Tip, w: f32, h: f32) -> (f32, f32) {
    let (s, c) = tip.angle.sin_cos();
    ((w * c).abs() + (h * s).abs(), (w * s).abs() + (h * c).abs())
}

/// The spacing a dab `w` x `h` sets for the next: across and down its own frame (or the same both
/// ways), automatic (a share of the square root of its size) or a share of its size, scaled by
/// the spacing option.
fn spacing_of(tip: &Tip, w: f32, h: f32, extra: f32, isotropic: bool) -> (f32, f32) {
    let auto = |v: f32, coeff: f32| coeff * if v < 1.0 { v } else { v.sqrt() };
    let (sx, sy) = if isotropic {
        let d = w.max(h);
        let s = match tip.auto_spacing {
            Some(coeff) => auto(d, coeff),
            None => d * tip.spacing,
        };
        (s, s)
    } else {
        match tip.auto_spacing {
            Some(coeff) => (auto(w, coeff), auto(h, coeff)),
            None => (w * tip.spacing, h * tip.spacing),
        }
    };
    (sx * extra, sy * extra)
}

/// The pipe's cell for a dab, each dimension picked as it says; those that go on by themselves
/// (in turn, at random) step on after.
fn pipe_cell(pick: &PipePick, state: &mut PipeState, info: &Info, count: u32, seq: u32, random: &mut Random) -> u32 {
    let mut index = 0_u32;
    let mut stride = 1_u32;
    // the last dimension steps fastest
    let n = pick.ranks.len();
    let mut strides = vec![1_u32; n];
    for i in (0..n).rev() {
        strides[i] = stride;
        stride = stride.saturating_mul(pick.ranks[i].max(1));
    }
    for (i, (&rank, sel)) in pick.ranks.iter().zip(&pick.selections).enumerate() {
        let rank = rank.max(1);
        let current = state.steps.get(i).copied().unwrap_or(0);
        let step = match sel {
            Selection::Pressure => (info.pressure * (rank - 1) as f32 + 0.5) as u32,
            Selection::Angular => {
                let a = (info.drawing_angle + FRAC_PI_2 + FRAC_PI_4).rem_euclid(TAU);
                (a / TAU * rank as f32) as u32
            }
            Selection::TiltX => ((info.x_tilt / 2.0 * rank as f32).round() as i64 + i64::from(rank / 2)).max(0) as u32,
            Selection::TiltY => ((info.y_tilt / 2.0 * rank as f32).round() as i64 + i64::from(rank / 2)).max(0) as u32,
            Selection::Velocity => {
                let v = ((info.speed + 1.0).ln()).min(3.0) / 3.0;
                (v * ((rank - 1) as f32 + 0.5)).round() as u32
            }
            Selection::Constant | Selection::Incremental | Selection::Random => current,
        };
        index = index.wrapping_add(strides[i].wrapping_mul(step));
        // on to the next for those that go by themselves
        let next = match sel {
            Selection::Incremental => (seq + 1) % rank,
            Selection::Random => (random.next() * rank as f32) as u32 % rank,
            _ => current,
        };
        if let Some(s) = state.steps.get_mut(i) {
            *s = next;
        }
    }
    index % count.max(1)
}

/// The pen's lean across and down (degrees, -60..60) from its lean from upright and the way it
/// leans (clockwise from up).
pub(crate) fn lean(tilt: f32, azimuth: f32) -> (f32, f32) {
    if !(tilt.is_finite() && azimuth.is_finite()) || tilt <= 0.0 {
        return (0.0, 0.0);
    }
    let t = tilt.min(FRAC_PI_2 - 1e-3).tan();
    let x = (t * azimuth.sin()).atan().to_degrees();
    let y = (-t * azimuth.cos()).atan().to_degrees();
    (x.clamp(-60.0, 60.0), y.clamp(-60.0, 60.0))
}

fn hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let hi = r.max(g).max(b);
    let d = hi - r.min(g).min(b);
    if d <= 1e-6 {
        return [0.0, 0.0, hi];
    }
    let h = if hi == r {
        (g - b) / d
    } else if hi == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [(h / 6.0).rem_euclid(1.0), d / hi, hi]
}

fn rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let f = |n: f32| {
        let k = (n + h * 6.0).rem_euclid(6.0);
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thin_line_covers_its_pixels_as_a_line_a_pixel_wide_does() {
        let preset = crate::preset::Preset::read(
            r#"<Preset name="B" paintopid="hairybrush"><param name="brush_definition"><![CDATA[<Brush type="auto_brush" spacing="0.1"><MaskGenerator type="circle" id="default" diameter="4"/></Brush>]]></param><param name="HairyBristle/antialias"><![CDATA[true]]></param></Preset>"#,
        )
        .unwrap();
        let brush = Brush::read(&preset, &|_| None, true).unwrap();
        let begin =
            Begin { colour: [0.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 1, turn: 0.0 };
        let mut s = Stroke::new(Rc::new(brush), begin);
        // 30 pixels: three pieces 10 long, two pixels wide once the app's dab reaches past them
        s.line((0.0, 5.0), (30.0, 5.0), 1.0, 0.8, None);
        assert_eq!(s.out.len(), 3);
        assert!(s.out.iter().all(|d| d.tip == THIN_TIP && d.diameter == 10.0 && d.threshold == 0.0));
        assert!((s.out[0].ratio * 5.0 - (1.0 - std::f32::consts::FRAC_1_SQRT_2)).abs() < 1e-5);
        assert!((s.out[1].x - 15.0).abs() < 1e-4 && s.out[0].opacity == 0.8);
        // a wider one its own width once the app's dab reaches past it
        s.out.clear();
        s.line((0.0, 5.0), (10.0, 5.0), 4.0, 1.0, None);
        assert_eq!(s.out[0].tip, LINE_TIP);
        assert!((s.out[0].ratio * s.out[0].diameter - (4.0 - std::f32::consts::SQRT_2)).abs() < 1e-4);
        // the thin tips: across, one less the distance from the middle (two pixels across)
        let tips = &s.brush.tip.masks;
        assert_eq!(tips.len(), 5);
    }

    #[test]
    fn a_walk_lays_a_dab_each_spacing() {
        let mut w = Walker { spacing: (10.0, 10.0), ..Walker::default() };
        // 25 pixels: one at 10, one at 20, 5 left over
        let mut at = (0.0, 0.0);
        let mut laid = Vec::new();
        while let Some(t) = w.next(at, (25.0, 0.0)) {
            at = (at.0 + (25.0 - at.0) * t, 0.0);
            laid.push(at.0);
        }
        assert_eq!(laid.len(), 2);
        assert!((laid[0] - 10.0).abs() < 1e-3 && (laid[1] - 20.0).abs() < 1e-3);
        assert!((w.gone.0 - 5.0).abs() < 1e-3);
        // an ellipse twice as long across as down: a move down reaches it in half the way
        let mut flat = Walker { spacing: (10.0, 5.0), ..Walker::default() };
        let t = flat.next((0.0, 0.0), (0.0, 20.0)).unwrap();
        assert!((t - 0.25).abs() < 1e-3);
    }

    #[test]
    fn colours_go_round_and_back() {
        let c = [0.2, 0.6, 0.4];
        let back = rgb(hsv(c));
        assert!(c.iter().zip(back).all(|(a, b)| (a - b).abs() < 1e-4));
        let (x, y) = lean(std::f32::consts::FRAC_PI_4, FRAC_PI_2);
        assert!((x - 45.0).abs() < 1e-3 && y.abs() < 1e-3);
    }
}
