//! A Krita pixel-brush preset made ready to draw: its tip or tips, its spacing, its options, how
//! it paints (washed or built up, its blend), its texture and its masking brush; and how it looks to
//! the app (its tips as grey pictures, the rest as the app's own numbers).

use crate::files::{Rgba, png_decode, png_size};
use crate::gimp::{self, Mask, Selection};
use crate::options::CurveOption;
use crate::preset::Preset;
use crate::rec::Record;
use crate::tips::AutoTip;
use crate::xml::Element;

/// How a tip set's pictures are picked from.
#[derive(Clone, Debug)]
pub enum Tips {
    /// An auto tip: `variants` drawings of it (holes and noise drawn afresh) at each of `levels`
    /// softnesses, by variant then level.
    Auto { variants: u32, levels: u32 },
    /// Pictures: one, or a pipe's cells and how it picks among them.
    Images { count: u32, pipe: Option<PipePick> },
}

#[derive(Clone, Debug)]
pub struct PipePick {
    pub ranks: Vec<u32>,
    pub selections: Vec<Selection>,
}

/// What a tip's pictures are to the app (its tip kinds), by how Krita applies a picture: how much
/// each pixel paints (a grey byte each), or RGBA pictures painting their own colours, their grey
/// the colour's lightness, or their grey a place along the gradient from the colour to the
/// secondary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Mask = 0,
    Stamp = 1,
    Lightness = 2,
    Gradient = 3,
}

/// A tip as a brush (or its masking brush) has it.
#[derive(Clone, Debug)]
pub struct Tip {
    pub tips: Tips,
    pub kind: Kind,
    /// Its own width and height (canvas pixels at size 1), its turn (radians) and its spacing.
    pub width: f32,
    pub height: f32,
    pub angle: f32,
    pub spacing: f32,
    pub auto_spacing: Option<f32>,
    /// Its pictures, for the app.
    pub masks: Vec<Mask>,
}

/// Options a brush and its masking brush both have.
#[derive(Clone, Debug)]
pub struct Dynamics {
    pub size: CurveOption,
    pub opacity: CurveOption,
    pub flow: CurveOption,
    pub rotation: CurveOption,
    pub ratio: CurveOption,
    pub spacing: CurveOption,
    pub scatter: CurveOption,
    pub mirror: CurveOption,
    pub softness: CurveOption,
    pub scatter_axes: (bool, bool),
    pub mirror_axes: (bool, bool),
    pub isotropic: bool,
}

impl Dynamics {
    fn read(p: &Preset, prefix: &str) -> Dynamics {
        let masked = !prefix.is_empty();
        let mut scatter = CurveOption::read(p, prefix, "Scatter", true, 0.0, 5.0);
        // older presets kept its amount apart
        if let (Some(amount), None) =
            (p.get(&format!("{prefix}Scattering/Amount")), p.get(&format!("{prefix}ScatterValue")))
        {
            scatter.strength = amount.trim().parse::<f32>().unwrap_or(scatter.strength);
        }
        Dynamics {
            size: CurveOption::read(p, prefix, "Size", true, 0.0, 1.0),
            // opacity and flow can be switched off only on a masking brush
            opacity: CurveOption::read(p, prefix, "Opacity", masked, 0.0, 1.0),
            flow: CurveOption::read(p, prefix, "Flow", masked, 0.0, 1.0),
            rotation: CurveOption::read(p, prefix, "Rotation", true, 0.0, 1.0),
            ratio: CurveOption::read(p, prefix, "Ratio", true, 0.0, 1.0),
            spacing: CurveOption::read(p, prefix, "Spacing", true, 0.0, 5.0),
            scatter,
            mirror: CurveOption::read(p, prefix, "Mirror", true, 0.0, 1.0),
            softness: CurveOption::read(p, prefix, "Softness", true, 0.1, 1.0),
            scatter_axes: (
                p.flag(&format!("{prefix}Scattering/AxisX"), true),
                p.flag(&format!("{prefix}Scattering/AxisY"), true),
            ),
            mirror_axes: (
                p.flag(&format!("{prefix}HorizontalMirrorEnabled"), false),
                p.flag(&format!("{prefix}VerticalMirrorEnabled"), false),
            ),
            isotropic: p.flag(&format!("{prefix}Spacing/Isotropic"), false),
        }
    }
}

/// A texture as Krita makes its mask of the pattern, and how it combines with each dab.
#[derive(Clone, Debug)]
pub struct Texture {
    pub mask: Mask,
    pub scale: f32,
    pub rule: u32,
    pub offset: (f32, f32),
    pub random_offset: (bool, bool),
    pub strength: CurveOption,
}

#[derive(Clone, Debug)]
pub struct Masking {
    pub tip: Tip,
    pub dynamics: Dynamics,
    /// Its size for each 1 of the main brush's own, and how it combines (the app's numbers).
    pub size: f32,
    pub combine: u32,
}

/// Krita's sharpness option: each dab's mask made hard at a threshold (the option's value), what
/// lies within `softness` of it below kept as it is; `align`: its dabs moved onto whole pixels as
/// far as the value goes.
#[derive(Clone, Debug)]
pub struct Sharpness {
    pub option: CurveOption,
    pub softness: f32,
    pub align: bool,
}

impl Sharpness {
    fn read(p: &Preset) -> Sharpness {
        let mut option = CurveOption::read(p, "", "Sharpness", true, 0.0, 1.0);
        let mut softness = p.number("Sharpness/softness", 0.0);
        // older presets' one number for both
        if let (Some(factor), None) = (p.get("Sharpness/factor"), p.get("SharpnessValue")) {
            let factor = factor.trim().parse::<f32>().unwrap_or(0.5);
            option.strength = factor.clamp(0.0, 1.0);
            softness = (factor * 100.0).trunc();
        }
        Sharpness {
            option,
            softness: (softness / 100.0).clamp(0.0, 1.0),
            align: p.flag("Sharpness/alignoutline", false),
        }
    }
}

/// Krita's precision: it keeps the last dab it drew and lays that again while the next is near
/// enough to it (its turn, size, place within a pixel, softness, lightness, squash, tip and
/// mirroring, as near as its level says, 1 the least near, 5 the nearest), so a stroke steps in
/// size, turn and place; automatic, the level is 5 for small dabs or where nothing moves dabs off
/// the path, else 3.
#[derive(Clone, Debug)]
pub struct Precision {
    pub level: u32,
    pub auto: bool,
    pub imprecise: bool,
}

impl Precision {
    fn read(p: &Preset, tip: &Tip, d: &Dynamics, texture: bool) -> Option<Precision> {
        // the engines whose dabs Krita keeps, and tips that may be kept (not speckled afresh)
        let kept = matches!(p.paintop.as_str(), "paintbrush" | "colorsmudge" | "tangentnormal");
        let speckled = matches!(tip.tips, Tips::Auto { variants, .. } if variants > 1);
        (kept && !speckled).then(|| Precision {
            level: (p.number("KisPrecisionOption/precisionLevel", 5.0) as u32).clamp(1, 5),
            auto: p.flag("KisPrecisionOption/AutoPrecisionEnabled", false),
            imprecise: d.scatter.checked || d.rotation.checked || d.mirror.checked || texture,
        })
    }

    /// Its level for a dab whose box is `least` pixels across at its narrowest.
    pub fn level(&self, least: f32) -> u32 {
        if !self.auto {
            self.level
        } else if least < 30.0 || !self.imprecise {
            5
        } else {
            3
        }
    }
}

/// Krita's colour smudge engine: each dab moves the paint under where the last lay to where it
/// lies (smearing), or that paint's one colour (dulling, picked from a share of it, weighed by the
/// dab's mask in the newer engine), mixing in the colour at its colour rate; its smudge rate how far
/// it lays that. A gradient option moves the colour towards the secondary colour.
#[derive(Clone, Debug)]
pub struct Smudge {
    pub rate: CurveOption,
    pub colour_rate: CurveOption,
    pub radius: CurveOption,
    pub gradient: CurveOption,
    pub dulls: bool,
    pub newer: bool,
    pub smear_alpha: bool,
    /// It picks up from every layer (Krita's overlay mode).
    pub overlay: bool,
    /// A lightness tip's paint thickness (how far its grey keeps from a flat middle grey, 0..1) and
    /// how that meets the thickness laid before: over it in its place (1), or over it as far as the
    /// smudge length and the thickness take it (2).
    pub thickness: CurveOption,
    pub thickness_mode: u32,
}

impl Smudge {
    fn read(p: &Preset) -> Smudge {
        let newer = p.flag("SmudgeRateUseNewEngine", false);
        let mut radius = CurveOption::read(p, "", "SmudgeRadius", true, 0.0, 3.0);
        // older presets kept it in hundredths
        let kept = p.number("SmudgeRadiusValue", 3.0);
        let kept = if p.number("SmudgeRadiusVersion", 1.0) < 2.0 { kept / 100.0 } else { kept };
        radius.strength = kept.clamp(0.0, if newer { 1.0 } else { 3.0 });
        Smudge {
            rate: CurveOption::read(p, "", "SmudgeRate", true, 0.0, 1.0),
            colour_rate: CurveOption::read(p, "", "ColorRate", true, 0.0, 1.0),
            radius,
            gradient: CurveOption::read(p, "", "Gradient", true, 0.0, 1.0),
            dulls: p.number("SmudgeRateMode", 0.0) as u32 == 1,
            newer,
            smear_alpha: p.flag("SmudgeRateSmearAlpha", true),
            overlay: p.flag("MergedPaint", false),
            thickness: CurveOption::read(p, "", "PaintThickness", true, 0.0, 1.0),
            // (Krita reads its reserved 0 as the second)
            thickness_mode: if p.number("PaintThicknessThicknessMode", 2.0) as u32 == 1 { 1 } else { 2 },
        }
    }

    /// Whether its dabs keep their paint with the layer, lit by the thickness their tips lay (a
    /// lightness tip's, as Krita's colour smudge keeps it between strokes).
    pub fn keeps_thickness(&self, tip: Kind) -> bool {
        tip == Kind::Lightness
    }

    /// How far a dab lays what it moves, and what share of that is the colour, for its smudge rate,
    /// colour rate and opacity: Krita's two engines' sums, taken as one mix laid at one strength.
    pub fn laid(&self, rate: f32, colour_rate: f32, opacity: f32) -> (f32, f32) {
        if !self.newer {
            let most = (1.0 - self.rate.strength).max(0.2);
            return ((rate * opacity).clamp(0.0, 1.0), (most * colour_rate * opacity).clamp(0.0, 1.0));
        }
        let colour = (colour_rate * colour_rate * opacity).clamp(0.0, 1.0);
        let moved = (if self.dulls { 0.8 } else { 1.0 } * rate * opacity).clamp(0.0, 1.0);
        if self.dulls && !self.smear_alpha && colour > 0.0 {
            // the colour mixed into the dulled one first, that laid as far as the dab moves paint
            return (moved, colour);
        }
        let laid = 1.0 - (1.0 - colour) * (1.0 - moved);
        (laid, if laid > 0.0 { colour / laid } else { 0.0 })
    }
}

/// Krita's airbrush: a dab every `interval` milliseconds (scaled by the rate option) as well as by
/// its spacing, or only by time.
#[derive(Clone, Debug)]
pub struct Airbrush {
    pub interval: f32,
    pub ignore_spacing: bool,
    pub rate: CurveOption,
}

impl Airbrush {
    fn read(p: &Preset) -> Option<Airbrush> {
        if !p.flag("PaintOpSettings/isAirbrushing", false) {
            return None;
        }
        let rate = p.number("PaintOpSettings/rate", 20.0);
        Some(Airbrush {
            interval: if rate > 0.0 { 1000.0 / rate } else { 1.0e9 },
            ignore_spacing: p.flag("PaintOpSettings/ignoreSpacing", false),
            rate: CurveOption::read(p, "", "Rate", true, 0.0, 1.0),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Brush {
    pub tip: Tip,
    pub dynamics: Dynamics,
    pub airbrush: Option<Airbrush>,
    /// Its colour smudge, if it is one.
    pub smudge: Option<Smudge>,
    /// Krita's other engines it may be (see `others.rs`), and a sketch brush's reach (half its tip
    /// across, at size 1).
    pub sketch: Option<crate::others::Sketch>,
    pub spray: Option<crate::others::Spray>,
    pub tangent: Option<crate::others::Tangent>,
    pub hairy: Option<crate::others::Hairy>,
    pub hatching: Option<crate::others::Hatching>,
    /// A hatching brush's tip, drawn (its lines are kept inside each dab of it).
    pub hatch_tip: Option<Tip>,
    pub curves: Option<crate::others::Curves>,
    pub particles: Option<crate::others::Particles>,
    pub grid: Option<crate::others::Grid>,
    pub chalk: Option<crate::others::Chalk>,
    pub dyna: Option<crate::others::Dyna>,
    /// Krita's clone, deform and filter brushes: dabs that lay the layer's own pixels, from
    /// elsewhere, moved about or blurred.
    pub clone: Option<crate::others::Clone>,
    pub deform: Option<crate::others::Deform>,
    pub filter: Option<crate::others::Filter>,
    pub shape: Option<crate::others::Shape>,
    pub sketch_radius: f32,
    /// How strongly a lightness tip's grey shows, and its sharpness.
    pub lightness: CurveOption,
    pub sharpness: Sharpness,
    pub darken: CurveOption,
    pub mix: CurveOption,
    pub hue: CurveOption,
    pub saturation: CurveOption,
    pub value: CurveOption,
    pub wash: bool,
    pub blend: Option<u32>,
    pub erases: bool,
    pub texture: Option<Texture>,
    pub masking: Option<Masking>,
    /// How near a dab may be to the last one drawn afresh to be that one laid again (its engine
    /// keeping its dabs that way and its tip allowing it).
    pub precision: Option<Precision>,
    /// A MyPaint brush's settings, if it is one (drawn by MyPaint's engine, see `mypaint.rs`).
    pub mypaint: Option<crate::mypaint::MyPaint>,
}

/// How big a square of the layer under where it starts a MyPaint brush that smudges wants, for
/// its first dabs (before it is told what lies under them).
const MYPAINT_UNDER: u32 = 256;

/// Krita's engines this one draws: its pixel brush, colour smudge, quick brush, tangent normal,
/// sketch, spray, bristle, hatching, curve, particle and grid brushes, and Krita 3's chalk and dyna
/// brushes.
pub const ENGINES: [&str; 17] = [
    "paintbrush",
    "colorsmudge",
    "roundmarker",
    "tangentnormal",
    "sketchbrush",
    "spraybrush",
    "hairybrush",
    "hatchingbrush",
    "curvebrush",
    "particlebrush",
    "gridbrush",
    "chalkbrush",
    "dynabrush",
    "duplicate",
    "deformbrush",
    "filter",
    "experimentbrush",
];

/// The options of the pixel brush's that each of Krita's engines takes, as its paint op applies
/// them (besides its own).
fn engine_options(engine: &str) -> &'static [&'static str] {
    match engine {
        "paintbrush" => &[
            "size",
            "opacity",
            "flow",
            "rotation",
            "ratio",
            "spacing",
            "scatter",
            "mirror",
            "softness",
            "sharpness",
            "lightness",
            "darken",
            "mix",
            "hsv",
            "texture",
            "masking",
            "airbrush",
        ],
        // no flow, softness, sharpness, darken, mix, lightness strength nor masking brush
        "colorsmudge" => {
            &["size", "opacity", "rotation", "ratio", "spacing", "scatter", "mirror", "hsv", "texture", "airbrush"]
        }
        "tangentnormal" => &[
            "size",
            "opacity",
            "flow",
            "rotation",
            "spacing",
            "scatter",
            "mirror",
            "softness",
            "sharpness",
            "texture",
            "airbrush",
        ],
        "roundmarker" => &["size", "spacing"],
        "sketchbrush" | "spraybrush" => &["size", "opacity", "rotation", "airbrush"],
        "hairybrush" => &["size", "opacity", "rotation"],
        "hatchingbrush" => &["size", "opacity"],
        "curvebrush" | "chalkbrush" => &["opacity"],
        "particlebrush" => &["airbrush"],
        "duplicate" => &["size", "opacity", "rotation", "mirror", "texture"],
        "deformbrush" => &["size", "opacity", "rotation", "airbrush"],
        "filter" => &["size", "opacity", "rotation", "mirror"],
        _ => &[],
    }
}

/// The engines that lay lines, dots and shapes rather than their tip, each over the last.
const SHAPES: [&str; 8] = [
    "sketchbrush",
    "spraybrush",
    "hairybrush",
    "curvebrush",
    "particlebrush",
    "gridbrush",
    "dynabrush",
    "experimentbrush",
];

/// Krita's composite ops as the app's blend modes number them (the Layers panel's order: Normal 0,
/// Multiply 1, Darken 2, Colour Burn 3 ... Luminosity 25; 29 behind).
pub fn blend_number(op: &str) -> Option<u32> {
    Some(match op {
        "normal" | "over" => 0,
        "multiply" => 1,
        "darken" => 2,
        "burn" => 3,
        "linear_burn" => 4,
        "darker color" => 5,
        "lighten" => 6,
        "screen" => 7,
        "dodge" => 8,
        "add" | "linear_dodge" => 9,
        "lighter color" => 10,
        "overlay" => 11,
        "soft_light" | "soft_light_svg" | "soft_light_pegtop_delphi" | "soft_light_ifs_illusions" => 12,
        "hard_light" => 13,
        "vivid_light" => 14,
        "linear light" => 15,
        "pin_light" => 16,
        "hard_mix" | "hard_mix_photoshop" | "hard_mix_softer_photoshop" => 17,
        "diff" => 18,
        "exclusion" => 19,
        "subtract" => 20,
        "divide" => 21,
        "hue" => 22,
        "saturation" => 23,
        "color" => 24,
        "luminize" => 25,
        "behind" => 29,
        _ => return None,
    })
}

/// A masking brush's op as the app's dual brushes combine two strokes: a blend mode's number, or
/// its two heights.
fn combine_number(op: &str) -> u32 {
    match op {
        "height" | "height_photoshop" => 27,
        "linear_height" | "linear_height_photoshop" => 28,
        other => blend_number(other).unwrap_or(1),
    }
}

/// Krita's texturing modes as the app's texture rules number them, soft ones 16 on.
fn texture_rule(mode: u32, soft: bool) -> u32 {
    let rule = match mode {
        0 => 32,
        4 => 34,
        5 => 35,
        6 => 36,
        7 => 37,
        8 => 38,
        9 => 39,
        10 => 40,
        11 => 41,
        12 => 42,
        13 => 43,
        14 => 44,
        15 => 45,
        // lightness and gradient colour the dab, softly or not
        2 => return 46,
        3 => return 47,
        _ => 33,
    };
    if soft { rule + 16 } else { rule }
}

/// A picture file's pictures as Krita takes them (RGBA: see [`gimp::gbr`]): a GIMP brush or pipe's
/// cells, or a PNG; whether they are colour pictures (not masks: a PNG with colour or see-through
/// pixels, a colour GIMP brush) and whether they hold colour (not all grey); a pipe's picking.
/// `light`: only their sizes, no pixels.
struct Pictures {
    cells: Vec<Rgba>,
    image: bool,
    colourful: bool,
    pipe: Option<PipePick>,
}

fn pictures(bytes: &[u8], light: bool) -> Option<Pictures> {
    // an SVG tip, drawn as Krita draws it (grey over white, 1000 pixels across)
    if crate::svg::is_svg(bytes) {
        let picture = if light {
            let (width, height) = crate::svg::size(bytes)?;
            Rgba { width, height, pixels: Vec::new() }
        } else {
            crate::svg::render(bytes)?
        };
        return Some(Pictures { cells: vec![picture], image: false, colourful: false, pipe: None });
    }
    if bytes.starts_with(b"\x89PNG") {
        if light {
            let (width, height) = png_size(bytes)?;
            let cells = vec![Rgba { width, height, pixels: Vec::new() }];
            return Some(Pictures { cells, image: false, colourful: false, pipe: None });
        }
        let picture = png_decode(bytes)?;
        let grey = gimp::all_grey(&picture);
        let see_through = picture.pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 255);
        let image = !(grey && !see_through);
        return Some(Pictures { cells: vec![picture], image, colourful: !grey, pipe: None });
    }
    if let Some((cell, grey, _)) = gimp::gbr(bytes, !light) {
        let colourful = !grey && !light && !gimp::all_grey(&cell);
        return Some(Pictures { cells: vec![cell], image: !grey, colourful, pipe: None });
    }
    let pipe = gimp::gih(bytes, !light)?;
    let colourful = !pipe.grey && !light && pipe.cells.first().is_some_and(|c| !gimp::all_grey(c));
    let pick = PipePick { ranks: pipe.ranks, selections: pipe.selections };
    Some(Pictures { cells: pipe.cells, image: !pipe.grey, colourful, pipe: Some(pick) })
}

/// How Krita applies a picture tip, by what its `Brush` element says (and, for presets from before
/// it said, whether the picture holds colour).
fn application(brush: &Element, colourful: bool) -> Kind {
    let legacy = |as_mask: bool| if colourful && !as_mask { Kind::Stamp } else { Kind::Mask };
    let as_mask = |otherwise: bool| brush.attribute("ColorAsMask").map_or(otherwise, |v| v.trim() != "0");
    if let Some(lightness) = brush.attribute("preserveLightness") {
        return if lightness.trim().parse::<i32>().unwrap_or(0) != 0 { Kind::Lightness } else { legacy(as_mask(true)) };
    }
    if let Some(application) = brush.attribute("brushApplication") {
        return match application.trim().parse::<u32>().unwrap_or(0) {
            1 => Kind::Stamp,
            2 => Kind::Lightness,
            3 => Kind::Gradient,
            _ => Kind::Mask,
        };
    }
    legacy(as_mask(false))
}

/// Krita's lightness adjustments of a colour picture's grey (not stamped in its own colours): the
/// grey taken as its middle (none: the picture's average), and how much lighter and of more
/// contrast; older presets' read as Krita reads them.
struct Adjustment {
    mid: Option<f32>,
    brightness: f32,
    contrast: f32,
}

impl Adjustment {
    fn read(brush: &Element) -> Adjustment {
        let mut mid = brush.number("AdjustmentMidPoint").map_or(127.0, |m| m.trunc().clamp(0.0, 255.0));
        let mut brightness = brush.number("BrightnessAdjustment").unwrap_or(0.0);
        let mut contrast = brush.number("ContrastAdjustment").unwrap_or(0.0);
        let version = brush.number("AdjustmentVersion").unwrap_or(1.0);
        let auto = brush.attribute("AutoAdjustMidPoint");
        if version < 2.0 && auto.is_none() {
            // applied twice before: about as much again
            mid = (127.0 + (mid - 127.0) * 2.0).clamp(0.0, 255.0);
            brightness *= 2.0;
            contrast *= 2.0;
            if contrast < 0.0 {
                contrast = 1.0 / (1.0 - contrast) - 1.0;
            }
        }
        let auto = auto.is_some_and(|a| a.trim() == "1" || a.trim() == "true");
        Adjustment { mid: (!auto).then_some(mid), brightness, contrast }
    }

    /// Each pixel of `picture` as its adjusted grey and its alpha.
    fn apply(&self, picture: &Rgba) -> Vec<[u8; 2]> {
        let px = picture.pixels.as_chunks::<4>().0;
        let mid = self.mid.unwrap_or_else(|| {
            // the picture's average grey, by its opacity
            let (mut lit, mut alpha) = (0_i64, 0_i64);
            for p in px {
                lit += (f32::from(gimp::grey(p[0], p[1], p[2])) * f32::from(p[3]) / 255.0).round() as i64;
                alpha += i64::from(p[3]);
            }
            if alpha == 0 { 0.0 } else { 255.0 * lit as f32 / alpha as f32 }
        });
        let (b, c) = (self.brightness, self.contrast);
        let plain = (mid - 127.0).abs() <= 0.1 && b == 0.0 && c == 0.0;
        let mid_y = if b > 0.0 { 127.0 + 128.0 * b } else { 127.0 + 127.0 * b };
        let (mut lo_a, mut hi_a, mut lo_b, mut hi_b) = (0.0, 0.0, 0.0, 255.0);
        if c != 1.0 {
            if c > 0.0 {
                lo_a = mid_y / (1.0 - c) / mid;
                hi_a = (255.0 - mid_y) / (1.0 - c) / (255.0 - mid);
            } else {
                lo_a = mid_y * (1.0 + c) / mid;
                hi_a = (255.0 - mid_y) * (1.0 + c) / (255.0 - mid);
            }
            lo_b = mid_y - mid * lo_a;
            hi_b = mid_y - mid * hi_a;
        }
        px.iter()
            .map(|p| {
                let v = f32::from(gimp::grey(p[0], p[1], p[2]));
                let v = if plain {
                    v
                } else if v >= mid {
                    (hi_a * v + hi_b).round().min(255.0)
                } else {
                    (lo_a * v + lo_b).round().max(0.0)
                };
                [v.clamp(0.0, 255.0) as u8, p[3]]
            })
            .collect()
    }
}

/// A picture as the app takes it as a tip of `kind`: how much it paints (its grey, adjusted for a
/// colour picture, what is dark painting as far as it is opaque), or RGBA: its own colours, or its
/// grey (adjusted for a colour picture) and alpha.
fn tip_picture(picture: &Rgba, kind: Kind, image: bool, adjust: &Adjustment) -> Mask {
    let (width, height) = (picture.width, picture.height);
    let px = picture.pixels.as_chunks::<4>().0;
    let pixels = match (kind, image) {
        (Kind::Stamp, _) => picture.pixels.clone(),
        (Kind::Mask, true) => {
            adjust.apply(picture).iter().map(|[v, a]| ((255 - u32::from(*v)) * u32::from(*a) / 255) as u8).collect()
        }
        (Kind::Mask, false) => px.iter().map(|p| 255 - gimp::grey(p[0], p[1], p[2])).collect(),
        (_, true) => adjust.apply(picture).iter().flat_map(|&[v, a]| [v, v, v, a]).collect(),
        (_, false) => px
            .iter()
            .flat_map(|p| {
                let v = gimp::grey(p[0], p[1], p[2]);
                [v, v, v, 255]
            })
            .collect(),
    };
    Mask { width, height, pixels }
}

/// A tip as drawn once: its width, height, coverage and (a colour picture's) colours.
type DrawnTip = (u32, u32, Vec<u8>, Option<Vec<[u8; 3]>>);

/// A bristle brush's bristles: each pixel of its tip at its own size that isn't clear (kept as likely
/// as `density` says, at most [`crate::others::MOST_BRISTLES`]), where it lies from the tip's middle,
/// how opaque it is, and its colour where the tip stamps its own.
fn bristles_of(
    definition: Option<&Element>,
    find: &dyn Fn(&str) -> Option<Vec<u8>>,
    density: f32,
) -> Vec<crate::others::Bristle> {
    let stand = crate::xml::parse(STAND_IN).unwrap_or_default();
    let element = definition.unwrap_or(&stand);
    // the tip as Krita draws it once: its coverage and (a colour picture's) colours
    let drawn = |e: &Element| -> Option<DrawnTip> {
        if e.attribute("type") == Some("auto_brush") {
            let tip = AutoTip::read(e)?;
            let mut seed = 0x2545_f491_u32;
            let mut random = move || {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed >> 8) as f32 / (1 << 24) as f32
            };
            let m = tip.draw(tip.diameter.round().max(1.0) as u32, 1.0, &mut random);
            return Some((m.width, m.height, m.pixels, None));
        }
        let older = e.attribute("BrushVersion").is_none_or(|v| v.trim() == "1");
        let scale = e.number("scale").unwrap_or(1.0).max(1e-3) * if older { 2.0 } else { 1.0 };
        let Pictures { cells, image, colourful, .. } = pictures(&find(e.attribute("filename")?)?, false)?;
        let cell = cells.first()?;
        let mask = tip_picture(cell, Kind::Mask, image, &Adjustment::read(e));
        let (w, h) =
            (((cell.width as f32 * scale).round() as u32).max(1), ((cell.height as f32 * scale).round() as u32).max(1));
        let at = |x: u32, y: u32| {
            let sx = ((x as f32 + 0.5) / scale) as u32;
            let sy = ((y as f32 + 0.5) / scale) as u32;
            (sy.min(cell.height - 1) * cell.width + sx.min(cell.width - 1)) as usize
        };
        let coverage = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| mask.pixels[at(x, y)]).collect();
        let colours = (application(e, colourful) == Kind::Stamp).then(|| {
            let px = cell.pixels.as_chunks::<4>().0;
            (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let [r, g, b, _] = px[at(x, y)];
                    [r, g, b]
                })
                .collect()
        });
        Some((w, h, coverage, colours))
    };
    let Some((w, h, coverage, colours)) = drawn(element).or_else(|| drawn(&stand)) else { return Vec::new() };
    let (cx, cy) = ((w as f32 * 0.5) as i32, (h as f32 * 0.5) as i32);
    let mut seed = 0x9e37_79b9_u32;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / (1 << 24) as f32
    };
    let mut bristles = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let alpha = f32::from(coverage[i]) / 255.0;
            if alpha <= 0.0 || (density < 1.0 && random() > density) {
                continue;
            }
            let colour = colours.as_ref().map(|c| c[i].map(|v| f32::from(v) / 255.0));
            bristles.push(crate::others::Bristle {
                x: (x as i32 - cx) as f32,
                y: (y as i32 - cy) as f32,
                length: alpha,
                colour,
            });
        }
    }
    // thinned out evenly where there are too many
    let most = crate::others::MOST_BRISTLES;
    if bristles.len() > most {
        let n = bristles.len();
        bristles = (0..most).map(|k| bristles[k * n / most]).collect();
    }
    bristles
}

/// A hard round tip `diameter` across.
/// The half length of a thin line's pieces the thin line tips are drawn for (canvas pixels).
pub const THIN_PIECE: f32 = 5.5;

/// The tip a line a pixel thin is laid with, a piece at a time: across it, as much as a line a pixel
/// wide covers of each pixel by how far its middle is from the line (one less that distance, the
/// tip being as wide as two pixels once the app's dab reaches past it); along it, ramps where it
/// meets the next piece, so that the two add up to one (none for a `hard` line, made hard by a
/// threshold that keeps the pixels nearest).
fn thin_line(hard: bool, light: bool) -> Mask {
    const SIDE: u32 = 64;
    // the app lays a picture over its dab and as far again as this past it each way
    const PAST: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let pixels = if light {
        Vec::new()
    } else {
        (0..SIDE * SIDE)
            .map(|i| {
                let u = ((i % SIDE) as f32 + 0.5) / (SIDE / 2) as f32 - 1.0;
                let v = ((i / SIDE) as f32 + 0.5) / (SIDE / 2) as f32 - 1.0;
                let across = 1.0 - v.abs();
                let along =
                    if hard { 1.0 } else { ((1.0 - u.abs()) * (THIN_PIECE + PAST) / (2.0 * PAST)).clamp(0.0, 1.0) };
                (across * along * 255.0).round() as u8
            })
            .collect()
    };
    Mask { width: SIDE, height: SIDE, pixels }
}

fn round_tip(diameter: f32) -> Tip {
    tip_of(
        &format!(
            r#"<Brush type="auto_brush" spacing="0.1"><MaskGenerator type="circle" id="default" diameter="{diameter}" ratio="1" hfade="1" vfade="1" antialiasEdges="1"/></Brush>"#
        ),
        diameter,
    )
}

/// The tip Krita stands in for one whose picture it can't find: its default, a hard round tip 40
/// across, spaced automatically.
pub const STAND_IN: &str = r#"<Brush useAutoSpacing="1" angle="0" spacing="0.1" density="1" BrushVersion="2" type="auto_brush" randomness="0" autoSpacingCoeff="0.8"><MaskGenerator spikes="2" hfade="1" ratio="1" diameter="40" id="default" type="circle" antialiasEdges="1" vfade="1"/></Brush>"#;

fn stand_in() -> Tip {
    tip_of(STAND_IN, 40.0)
}

/// The auto tip `xml` describes (`diameter` across, should it not draw).
fn tip_of(xml: &str, diameter: f32) -> Tip {
    let brush = crate::xml::parse(xml).unwrap_or_default();
    read_tip(&brush, &|_| None, None, false, true).unwrap_or(Tip {
        tips: Tips::Images { count: 1, pipe: None },
        kind: Kind::Mask,
        width: diameter,
        height: diameter,
        angle: 0.0,
        spacing: 0.1,
        auto_spacing: None,
        masks: vec![Mask { width: 1, height: 1, pixels: vec![255] }],
    })
}

/// The tip a `Brush` element describes, its pictures found by `find`; `softness`: the brush varies
/// its softness, so an auto tip is drawn at several. `light`: its pictures' sizes only, none drawn.
/// `masks`: its pictures taken as masks whatever the preset says (a masking brush's are).
fn read_tip(
    brush: &Element,
    find: &dyn Fn(&str) -> Option<Vec<u8>>,
    softness: Option<&CurveOption>,
    light: bool,
    masks: bool,
) -> Option<Tip> {
    let spacing = brush.number("spacing").unwrap_or(0.1).max(0.0);
    let auto_spacing = (brush.attribute("useAutoSpacing") == Some("1")
        || brush.attribute("useAutoSpacing") == Some("true"))
    .then(|| brush.number("autoSpacingCoeff").unwrap_or(1.0));
    let angle = brush.number("angle").unwrap_or(0.0);
    if brush.attribute("type") == Some("auto_brush") {
        let tip = AutoTip::read(brush)?;
        let (w, h) = tip.size();
        let variants = if tip.speckled() { 4 } else { 1 };
        let levels = if softness.is_some_and(CurveOption::varies) { 4 } else { 1 };
        // drawn at its own size (holes and noise as fine as Krita's), else large enough to scale (the GPU
        // draws it smaller through its mip levels)
        let side = if tip.speckled() {
            tip.diameter.round().clamp(8.0, 256.0)
        } else {
            tip.diameter.round().clamp(64.0, 256.0)
        };
        let mut seed = 0x2545_f491_u32;
        let mut random = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 8) as f32 / (1 << 24) as f32
        };
        let mut masks = Vec::new();
        for _ in 0..if light { 0 } else { variants } {
            for level in 0..levels {
                let soft = if levels == 1 { 1.0 } else { 0.1 + 0.9 * level as f32 / (levels - 1) as f32 };
                masks.push(tip.draw(side as u32, soft, &mut random));
            }
        }
        return Some(Tip {
            tips: Tips::Auto { variants, levels },
            kind: Kind::Mask,
            width: w,
            height: h,
            angle,
            spacing,
            auto_spacing,
            masks,
        });
    }
    // presets from before the second version of tips took them at twice their scale
    let older = brush.attribute("BrushVersion").is_none_or(|v| v.trim() == "1");
    let scale = brush.number("scale").unwrap_or(1.0).max(1e-3) * if older { 2.0 } else { 1.0 };
    let file = brush.attribute("filename")?;
    let Pictures { cells, image, colourful, pipe } = pictures(&find(file)?, light)?;
    let first = cells.first()?;
    let (w, h) = (first.width as f32 * scale, first.height as f32 * scale);
    let count = cells.len() as u32;
    let kind = if masks { Kind::Mask } else { application(brush, colourful) };
    let masks = if light {
        cells.iter().map(|c| Mask { width: c.width, height: c.height, pixels: Vec::new() }).collect()
    } else {
        let adjust = Adjustment::read(brush);
        cells.iter().map(|c| tip_picture(c, kind, image, &adjust)).collect()
    };
    Some(Tip { tips: Tips::Images { count, pipe }, kind, width: w, height: h, angle, spacing, auto_spacing, masks })
}

/// A pattern's picture: a PNG or a GIMP pattern. `light`: its size only.
fn pattern(bytes: &[u8], light: bool) -> Option<Rgba> {
    if !bytes.starts_with(b"\x89PNG") {
        return gimp::pat(bytes, !light);
    }
    if light {
        let (width, height) = png_size(bytes)?;
        return Some(Rgba { width, height, pixels: Vec::new() });
    }
    png_decode(bytes)
}

/// The mask Krita makes of a pattern for its texture: each pixel's grey where it is opaque (white
/// where clear), made lighter, of more contrast, inverted, its middle moved, cut outside its
/// bounds.
fn texture_mask(p: &Preset, picture: &Rgba) -> Mask {
    let k = |key: &str, otherwise: f32| p.number(&format!("Texture/Pattern/{key}"), otherwise);
    let (brightness, contrast, neutral) = (k("Brightness", 0.0), k("Contrast", 1.0), k("NeutralPoint", 0.5));
    let invert = p.flag("Texture/Pattern/Invert", false);
    let policy = k("CutoffPolicy", 0.0) as u32;
    let (left, right) = (k("CutoffLeft", 0.0) / 255.0, k("CutoffRight", 255.0) / 255.0);
    let pixels = picture
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| {
            let alpha = f32::from(px[3]) / 255.0;
            let g = f32::from(gimp::grey(px[0], px[1], px[2])) / 255.0;
            let lit = g * alpha + (1.0 - alpha);
            let mut m = ((lit - brightness - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
            if invert {
                m = 1.0 - m;
            }
            let n = neutral.clamp(0.0, 1.0);
            m = if n >= 1.0 || (n > 0.0 && m <= n) { m / (2.0 * n) } else { 0.5 + (m - n) / (2.0 - 2.0 * n) };
            let outside = m < left || m > right;
            m = match policy {
                1 if outside => 0.0,
                2 if outside => 1.0,
                _ => m.clamp(0.0, 1.0),
            };
            (m * 255.0).round() as u8
        })
        .collect();
    Mask { width: picture.width, height: picture.height, pixels }
}

impl Brush {
    /// The pixel brush (or colour smudge brush) `p` describes, its pictures found by `find`; none for
    /// another engine's.
    /// `light`: only to draw strokes with, its look not made (its pictures neither drawn nor
    /// decoded).
    pub fn read(p: &Preset, find: &dyn Fn(&str) -> Option<Vec<u8>>, light: bool) -> Option<Brush> {
        if !ENGINES.contains(&p.paintop.as_str()) {
            return None;
        }
        let smudge = (p.paintop == "colorsmudge").then(|| Smudge::read(p));
        let mut dynamics = Dynamics::read(p, "");
        // each of Krita's engines has only some of the pixel brush's options: what a preset still
        // says of the rest (from the engine it was made with) does nothing
        let only = engine_options(&p.paintop);
        let has = |name: &str| only.contains(&name);
        {
            let d = &mut dynamics;
            let options = [
                ("size", &mut d.size),
                ("opacity", &mut d.opacity),
                ("flow", &mut d.flow),
                ("rotation", &mut d.rotation),
                ("ratio", &mut d.ratio),
                ("spacing", &mut d.spacing),
                ("scatter", &mut d.scatter),
                ("mirror", &mut d.mirror),
                ("softness", &mut d.softness),
            ];
            for (name, option) in options {
                if !has(name) {
                    option.checked = false;
                    // the brush's opacity stays the preset's; its flow is full
                    if name == "flow" {
                        option.strength = 1.0;
                    }
                }
            }
        }
        let definition = match p.paintop.as_str() {
            // the quick brush: a hard round tip of its own diameter and spacing
            "roundmarker" => crate::xml::parse(&format!(
                r#"<Brush type="auto_brush" spacing="{}" useAutoSpacing="{}" autoSpacingCoeff="{}"><MaskGenerator type="circle" id="default" diameter="{}" ratio="1" hfade="1" vfade="1" antialiasEdges="1"/></Brush>"#,
                p.number("spacing", 0.02),
                u8::from(p.flag("useAutoSpacing", false)),
                p.number("autoSpacingCoeff", 1.0),
                p.number("diameter", 30.0),
            )),
            // the chalk brush: a speckled round tip at every pixel
            "chalkbrush" => crate::xml::parse(&crate::others::Chalk::read(p).tip()),
            // the deform brush: an ellipse of its own
            "deformbrush" => crate::xml::parse(&crate::others::Deform::read(p).tip()),
            _ => p.element("brush_definition"),
        };
        let mut tip = definition
            .as_ref()
            .and_then(|b| read_tip(b, find, Some(&dynamics.softness), light, false))
            // as Krita does, its default tip where the preset's picture can't be found
            .unwrap_or_else(stand_in);
        let sketch_radius = 0.5 * tip.width.max(tip.height);
        let engine = p.paintop.as_str();
        let (sketch, spray) = match engine {
            "sketchbrush" => (Some(crate::others::Sketch::read(p)), None),
            "spraybrush" => (None, Some(crate::others::Spray::read(p))),
            _ => (None, None),
        };
        let hairy = (engine == "hairybrush").then(|| {
            let density = (p.number("HairyBristle/density", 100.0) / 100.0).clamp(0.0, 1.0);
            crate::others::Hairy::read(p, bristles_of(definition.as_ref(), find, density))
        });
        let hatching = (engine == "hatchingbrush").then(|| crate::others::Hatching::read(p));
        let curves = (engine == "curvebrush").then(|| crate::others::Curves::read(p));
        let particles = (engine == "particlebrush").then(|| crate::others::Particles::read(p));
        let grid = (engine == "gridbrush").then(|| crate::others::Grid::read(p));
        let chalk = (engine == "chalkbrush").then(|| crate::others::Chalk::read(p));
        let dyna = (engine == "dynabrush").then(|| crate::others::Dyna::read(p));
        let clone = (engine == "duplicate").then(|| crate::others::Clone::read(p));
        let deform = (engine == "deformbrush").then(|| crate::others::Deform::read(p));
        let filter = (engine == "filter").then(|| crate::others::Filter::read(p)).flatten();
        let shape = (engine == "experimentbrush").then(|| crate::others::Shape::read(p));
        // a hatching's lines are laid inside each dab of its tip: the tip drawn, to be read where
        // they lie
        let hatch_tip = hatching
            .as_ref()
            .map(|_| definition.as_ref().and_then(|b| read_tip(b, find, None, false, true)).unwrap_or_else(stand_in));
        let own_tip = spray.as_ref().is_some_and(|s| s.own_tip);
        if (SHAPES.contains(&engine) && !own_tip) || hatching.is_some() {
            // its lines', dots' and shapes' tips: a hard round one, a square (unless a spray's
            // particles are its own tip)
            let round = round_tip(64.0).masks.into_iter().next();
            let square = Mask { width: 1, height: 1, pixels: if light { Vec::new() } else { vec![255] } };
            // (and a hatching's own, for the background behind its lines; then the thin lines')
            let own = hatch_tip.as_ref().and_then(|t| t.masks.first().cloned()).unwrap_or_else(|| square.clone());
            let thin = [thin_line(false, light), thin_line(true, light)];
            let masks: Vec<Mask> = round.into_iter().chain([square, own]).chain(thin).collect();
            let count = masks.len() as u32;
            tip = Tip { tips: Tips::Images { count, pipe: None }, kind: Kind::Mask, masks, ..tip };
        }
        let op = p.get("CompositeOp").unwrap_or("normal").trim().to_owned();
        let texture_enabled = has("texture") && p.flag("Texture/Pattern/Enabled", false);
        let texture = texture_enabled.then(|| Brush::read_texture(p, find, light)).flatten();
        let washes = p.number("PaintOpAction", 2.0) != 1.0;
        let masking = (has("masking") && p.flag("MaskingBrush/Enabled", false))
            .then(|| Brush::read_masking(p, find, light))
            .flatten();
        // a masked one always washes; a shape's rows of fill never build past its opacity
        let wash = ((washes || masking.is_some()) && !SHAPES.contains(&engine) && engine != "roundmarker")
            || engine == "experimentbrush";

        let colour = |key: &str, name: &str| {
            let mut option = CurveOption::read(p, "", key, true, 0.0, 1.0);
            option.checked &= has(name);
            option
        };
        let mut sharpness = Sharpness::read(p);
        sharpness.option.checked &= has("sharpness");
        let precision = Precision::read(p, &tip, &dynamics, texture_enabled);
        Some(Brush {
            tip,
            dynamics,
            airbrush: Airbrush::read(p).filter(|_| has("airbrush")),
            smudge,
            sketch,
            spray,
            tangent: (p.paintop == "tangentnormal").then(|| crate::others::Tangent::read(p)),
            hairy,
            hatching,
            hatch_tip,
            curves,
            particles,
            grid,
            chalk,
            dyna,
            clone,
            deform,
            filter,
            shape,
            sketch_radius,
            lightness: colour("LightnessStrength", "lightness"),
            sharpness,
            darken: colour("Darken", "darken"),
            mix: colour("Mix", "mix"),
            hue: colour("h", "hsv"),
            saturation: colour("s", "hsv"),
            value: colour("v", "hsv"),
            // washed unless it says it builds up; the quick brush and those laying lines, dots and
            // shapes lay each over the last
            wash,
            blend: blend_number(&op),
            erases: op == "erase" || p.flag("EraserMode", false),
            texture,
            precision,
            masking,
            mypaint: None,
        })
    }

    /// A MyPaint brush (`settings`): MyPaint's round dab at each of its hardnesses for its tips;
    /// `light`: only to draw strokes with (its tips not drawn).
    pub fn mypaint(settings: crate::mypaint::MyPaint, light: bool) -> Option<Brush> {
        let stand_in = format!(
            r#"<Preset name="MyPaint" paintopid="paintbrush"><param name="brush_definition" type="string"><![CDATA[{STAND_IN}]]></param><param name="PaintOpAction" type="string"><![CDATA[1]]></param></Preset>"#
        );
        let preset = Preset::read(&stand_in)?;
        let mut brush = Brush::read(&preset, &|_| None, true)?;
        let masks = crate::mypaint::tips(light);
        let count = masks.len() as u32;
        brush.tip = Tip { tips: Tips::Images { count, pipe: None }, kind: Kind::Mask, masks, ..brush.tip };
        brush.wash = false;
        brush.erases = settings.erases();
        brush.precision = None;
        brush.mypaint = Some(settings);
        Some(brush)
    }

    fn read_texture(p: &Preset, find: &dyn Fn(&str) -> Option<Vec<u8>>, light: bool) -> Option<Texture> {
        // its picture: carried in the setting itself (older presets), else a file it names
        // (read only to draw, only its header: its size)
        let carried =
            p.get("Texture/Pattern/Pattern").map(|t| if light { t.get(..t.len().min(8192)).unwrap_or(t) } else { t });
        let bytes = carried.and_then(crate::files::carried_picture).or_else(|| {
            ["Texture/Pattern/PatternFileName", "Texture/Pattern/Name"].iter().find_map(|k| find(p.get(k)?.trim()))
        })?;
        let picture = pattern(&bytes, light)?;
        let k = |key: &str, otherwise: f32| p.number(&format!("Texture/Pattern/{key}"), otherwise);
        Some(Texture {
            mask: texture_mask(p, &picture),
            scale: k("Scale", 1.0).max(0.01),
            rule: texture_rule(k("TexturingMode", 0.0) as u32, p.flag("Texture/Pattern/UseSoftTexturing", false)),
            offset: (k("OffsetX", 0.0), k("OffsetY", 0.0)),
            random_offset: (
                p.flag("Texture/Pattern/isRandomOffsetX", false),
                p.flag("Texture/Pattern/isRandomOffsetY", false),
            ),
            strength: CurveOption::read(p, "", "Texture/Strength/", true, 0.0, 1.0),
        })
    }

    fn read_masking(p: &Preset, find: &dyn Fn(&str) -> Option<Vec<u8>>, light: bool) -> Option<Masking> {
        let prefix = "MaskingBrush/Preset/";
        let dynamics = Dynamics::read(p, prefix);
        let definition = p.element(&format!("{prefix}brush_definition"))?;
        let tip = read_tip(&definition, find, None, light, true)?;
        let size = if p.flag("MaskingBrush/UseMasterSize", true) {
            p.number("MaskingBrush/MasterSizeCoeff", 1.0).max(0.0)
        } else {
            0.0
        };
        let combine = combine_number(p.get("MaskingBrush/MaskingCompositeOp").unwrap_or("multiply").trim());
        Some(Masking { tip, dynamics, size, combine })
    }

    /// Whether its dabs move the layer's paint (a colour smudge's, a clone's, a deform's, a filter's).
    pub fn moves_paint(&self) -> bool {
        self.smudge.is_some()
            || self.clone.is_some()
            || self.deform.is_some()
            || self.filter.is_some()
            || self.mypaint.as_ref().is_some_and(|m| m.smudges() && !m.erases())
    }

    /// How it looks to the app (see the app's guide to brush engines).
    pub fn look(&self) -> Vec<u8> {
        let mut look = Record::new();
        for m in &self.tip.masks {
            look.field(1, &tip_record(m));
        }
        if self.tip.kind != Kind::Mask {
            look.byte(7, self.tip.kind as u8);
        }
        if let Some(a) = &self.airbrush {
            look.real(8, a.interval);
        }
        // a colour smudge's dabs move paint, a grid's that takes the colour under it pick it up, a
        // clone's, a deform's and a filter's lay the layer's own
        if self.moves_paint() || self.grid.as_ref().is_some_and(|g| g.sample) {
            look.byte(9, 1);
        }
        // in overlay mode (a clone from the image), from every layer; a smudge keeping its paint with
        // the layer picks up from that alone
        let keeps = self.smudge.as_ref().is_some_and(|s| s.keeps_thickness(self.tip.kind));
        if self.smudge.as_ref().is_some_and(|s| s.overlay && !keeps)
            || self.clone.as_ref().is_some_and(|c| c.from_image)
        {
            look.byte(10, 1);
        }
        // a lightness tip's smudge keeps its paint with the layer, lit by the thickness laid
        if keeps {
            look.byte(12, 1);
        }
        // a bristle brush soaking up its ink wants the colours where its strokes start
        if let Some(side) = self.hairy.as_ref().map(crate::others::Hairy::soaks).filter(|&s| s > 0) {
            look.word(11, side);
        }
        // a MyPaint brush that smudges, for its first dabs
        if self.mypaint.is_some() && self.moves_paint() {
            look.word(11, MYPAINT_UNDER);
        }
        // (a smudge's dabs are laid each as far as it says)
        look.byte(2, u8::from(self.wash && !self.moves_paint() && self.grid.is_none()));
        if let Some(blend) = self.blend {
            look.word(3, blend);
        }
        look.byte(4, u8::from(self.erases));
        if let Some(t) = &self.texture {
            let mut r = Record::new();
            r.word(1, t.mask.width).word(2, t.mask.height).field(3, &t.mask.pixels).real(4, t.scale).word(5, t.rule);
            look.field(5, &r.bytes());
        }
        if let Some(m) = &self.masking {
            let mut r = Record::new();
            for mask in &m.tip.masks {
                r.field(1, &tip_record(mask));
            }
            r.byte(2, 1).word(3, m.combine);
            look.field(6, &r.bytes());
        }
        look.bytes()
    }
}

fn tip_record(m: &Mask) -> Vec<u8> {
    let mut r = Record::new();
    r.word(1, m.width).word(2, m.height).field(3, &m.pixels);
    r.bytes()
}
