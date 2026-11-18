//! Krita brushes, a plugin for ProCraft: a brush engine (the app's `brush_engines`) that reads
//! Krita's presets (`.kpp`) and bundles (`.bundle`) and draws their brushes as Krita's pixel
//! brush and colour smudge brush do, and its quick, tangent normal, sketch, spray, bristle, hatching,
//! curve, particle and grid brushes as far as dabs draw them. The app hands it each stroke's input
//! and lays the dabs it answers on the GPU.
//!
//! - Importing: each preset of an engine it draws becomes a brush whose data is the preset's
//!   settings and the pictures it names (its tip, its masking brush's, its texture's), found in it
//!   or its bundle. See `import.rs`.
//! - Loading: the data read again into a brush ready to draw, and how it looks to the app (its tips
//!   drawn as grey pictures, its texture's mask, whether it washes, its blend), unless the app holds
//!   that already (it keeps it on the disk). See `brush.rs`.
//! - Drawing: dab by dab, as Krita spaces and shapes them. See `engine.rs`; Krita's MyPaint
//!   brushes through MyPaint's engine, see `mypaint.rs`.
//!
//! What it knows of Krita's brushes comes from Krita's manual and from reading how Krita works; the
//! code here is its own (see README.md).

mod brush;
mod curve;
mod engine;
mod files;
mod gimp;
mod import;
mod json;
mod mypaint;
mod options;
mod others;
mod preset;
mod rec;
mod studio;
mod svg;
mod tips;
mod utilities;
mod xml;

use std::cell::RefCell;
use std::rc::Rc;

use brush::Brush;
use engine::{Begin, Sample, Stroke};
use rec::{f32_of, f32s, fields, u32_of, u64_of};

/// What it holds between calls: what the app hands over and takes back, the brushes it has read by
/// the app's ids for them (the latest last), and the strokes being drawn.
#[derive(Default)]
struct State {
    input: Vec<u8>,
    output: Vec<u8>,
    brushes: Vec<(u64, Rc<Brush>)>,
    strokes: Vec<(i32, Stroke)>,
    next_stroke: i32,
    studio: studio::Studio,
}

/// How many brushes it keeps read, and strokes open at once.
const BRUSHES_KEPT: usize = 24;
const MOST_STROKES: usize = 16;

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Where `len` bytes go for the next call.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_brush_in(len: i32) -> i32 {
    with(|s| {
        s.input.clear();
        s.input.resize(usize::try_from(len).unwrap_or(0), 0);
        s.input.as_mut_ptr() as i32
    })
}

/// Where the last answer is.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_brush_out() -> i32 {
    with(|s| s.output.as_ptr() as i32)
}

/// A brush file (a record of its name and bytes): the brushes made of it, as a record; its
/// length.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_brush_import(_engine: i32, len: i32) -> i32 {
    with(|s| {
        let input = s.input.get(..usize::try_from(len).unwrap_or(0)).unwrap_or_default();
        let (mut name, mut bytes) = (String::new(), &[][..]);
        for (id, value) in fields(input) {
            match id {
                1 => name = String::from_utf8_lossy(value).into_owned(),
                2 => bytes = value,
                _ => {}
            }
        }
        s.output = import::import(&name, bytes);
        s.output.len() as i32
    })
}

/// A brush (a record of the app's id for it, its data, and whether the app holds its look): read,
/// kept, and how it looks (nothing if the app holds that); its length, or -1 if it isn't one this
/// engine draws.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_brush_load(_engine: i32, len: i32) -> i32 {
    with(|s| {
        let input = s.input.get(..usize::try_from(len).unwrap_or(0)).unwrap_or_default();
        let (mut id, mut data, mut look_held) = (None, &[][..], false);
        for (field, value) in fields(input) {
            match field {
                1 => id = u64_of(value),
                2 => data = value,
                3 => look_held = value.first() == Some(&1),
                _ => {}
            }
        }
        let Some(brush) = import::load(data, look_held) else { return -1 };
        let look = if look_held { Vec::new() } else { brush.look() };
        if let Some(id) = id {
            s.brushes.retain(|(i, _)| *i != id);
            s.brushes.push((id, Rc::new(brush)));
            let over = s.brushes.len().saturating_sub(BRUSHES_KEPT);
            s.brushes.drain(..over);
        }
        s.output = look;
        s.output.len() as i32
    })
}

/// A stroke starts (a record of the brush's id, colours, size and seed): its number, or -1 if its
/// brush isn't read yet.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_brush_begin(len: i32) -> i32 {
    with(|s| {
        let input = s.input.get(..usize::try_from(len).unwrap_or(0)).unwrap_or_default();
        let mut id = None;
        let mut under = None;
        let mut canvas = None;
        let mut begin =
            Begin { colour: [0.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 1, turn: 0.0 };
        for (field, value) in fields(input) {
            let three = |v: &[u8]| {
                let f = f32s(v);
                (f.len() >= 3).then(|| [f[0], f[1], f[2]].map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 }))
            };
            match field {
                1 => id = u64_of(value),
                2 => begin.colour = three(value).unwrap_or(begin.colour),
                3 => begin.secondary = three(value).unwrap_or(begin.secondary),
                4 => begin.diameter = f32_of(value).unwrap_or(1.0),
                8 => begin.seed = u32_of(value).unwrap_or(1),
                9 => begin.turn = f32_of(value).filter(|t| t.is_finite()).unwrap_or(0.0),
                10 => begin.start_diameter = f32_of(value).unwrap_or(1.0),
                11 => under = engine::Under::read(value),
                12 => {
                    let f = f32s(value);
                    canvas = (f.len() >= 2).then(|| (f[0], f[1]));
                }
                _ => {}
            }
        }
        let Some(brush) = id.and_then(|id| s.brushes.iter().find(|(i, _)| *i == id)).map(|(_, b)| Rc::clone(b)) else {
            return -1;
        };
        let number = s.next_stroke;
        s.next_stroke = s.next_stroke.wrapping_add(1).max(0);
        // a stroke the app let go of long ago goes
        if s.strokes.len() >= MOST_STROKES {
            s.strokes.remove(0);
        }
        let mut stroke = Stroke::new(brush, begin);
        stroke.under = under;
        stroke.canvas = canvas;
        s.strokes.push((number, stroke));
        number
    })
}

/// Samples of stroke `stroke` (32 bytes each): the dabs they lay, at out; how many.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_brush_add(stroke: i32, len: i32) -> i32 {
    with(|s| {
        let input = s.input.get(..usize::try_from(len).unwrap_or(0)).unwrap_or_default();
        let samples: Vec<Sample> = input
            .as_chunks::<32>()
            .0
            .iter()
            .map(|b| {
                let f = f32s(&b[..24]);
                let ms = f64::from_le_bytes(b[24..32].try_into().unwrap_or([0; 8]));
                Sample { x: f[0], y: f[1], pressure: f[2], tilt: f[3], azimuth: f[4], roll: f[5], ms }
            })
            .collect();
        let State { strokes, output, .. } = s;
        output.clear();
        let Some((_, open)) = strokes.iter_mut().find(|(n, _)| *n == stroke) else { return 0 };
        open.add(&samples);
        let dabs = std::mem::take(&mut open.out);
        for d in &dabs {
            d.write(output);
        }
        dabs.len() as i32
    })
}

/// The colours under the dabs stroke `stroke` asked about (four f32 each, premultiplied, in
/// order), told before its next samples.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_brush_under(stroke: i32, len: i32) -> i32 {
    with(|s| {
        let input = s.input.get(..usize::try_from(len).unwrap_or(0)).unwrap_or_default();
        let told: Vec<[f32; 4]> = input
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| {
                let f = f32s(b);
                [f[0], f[1], f[2], f[3]].map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 })
            })
            .collect();
        if let Some((_, open)) = s.strokes.iter_mut().find(|(n, _)| *n == stroke) {
            open.told(&told);
        }
        0
    })
}

/// Stroke `stroke` ends: its last dabs (most are laid as their samples come).
#[unsafe(no_mangle)]
pub extern "C" fn procraft_brush_end(stroke: i32) -> i32 {
    with(|s| {
        let State { strokes, output, .. } = s;
        output.clear();
        let mut laid = 0;
        if let Some((_, open)) = strokes.iter_mut().find(|(n, _)| *n == stroke) {
            open.finish();
            let dabs = std::mem::take(&mut open.out);
            for d in &dabs {
                d.write(output);
            }
            laid = dabs.len() as i32;
        }
        strokes.retain(|(n, _)| *n != stroke);
        laid
    })
}

/// Its action: the Krita brush studio, on the brush the app hands over.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_action(_index: i32) -> i32 {
    with(|s| s.studio.open());
    0
}

/// A touch on the studio's panel.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_panel_event(row: i32, value: f32) -> i32 {
    with(|s| s.studio.touched(usize::try_from(row).unwrap_or(usize::MAX), value));
    0
}
