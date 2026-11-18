//! Krita's engines besides its pixel brush and colour smudge, as far as dabs draw them: the sketch
//! brush (lines from the pen to the points it passed near, each a thin dab), the spray brush
//! (particles scattered around the pen, each a dab), the quick brush (hard round dabs, read as a
//! pixel brush), the tangent normal brush (a pixel brush whose colour is the pen's lean as a
//! normal), the bristle brush (a line for each bristle at each move), the hatching brush (the lines
//! of a hatching laid inside each dab of its tip), the curve brush (lines and curves through the
//! points it passed), the particle brush (particles pulled towards the pen, each a dot) and the
//! grid brush (the cells of a grid around the pen, each a shape); and Krita 3's chalk brush (a
//! speckled round tip at every pixel) and dyna brush (circles, a ribbon or lines drawn by a weight the
//! pen pulls). The rest (shape, filter, deform and clone brushes) come in as the app reads them.

use crate::curve::Curve;
use crate::options::{CurveOption, Info};
use crate::preset::Preset;

/// The sketch brush (Krita's "Harmony"-like one): at each move it draws a line from the pen to
/// each point it passed within its tip's reach, as likely as its density says (and the further,
/// the more likely), each line `line_width` wide and moved by its offset.
#[derive(Clone, Debug)]
pub struct Sketch {
    pub probability: f32,
    pub offset: f32,
    pub line_width: f32,
    pub connect: bool,
    pub magnetify: bool,
    pub random_rgb: bool,
    pub random_opacity: bool,
    pub distance_density: bool,
    pub distance_opacity: bool,
    /// Its lines smooth (else hard, a pixel's worth or not at all, as Krita draws them unless it
    /// says).
    pub antialias: bool,
    pub line_width_option: CurveOption,
    pub offset_option: CurveOption,
    pub density_option: CurveOption,
}

impl Sketch {
    pub fn read(p: &Preset) -> Sketch {
        Sketch {
            probability: p.number("Sketch/probability", 0.5),
            offset: p.number("Sketch/offset", 30.0),
            line_width: p.number("Sketch/lineWidth", 1.0).trunc().max(0.0),
            connect: p.flag("Sketch/makeConnection", true),
            magnetify: p.flag("Sketch/magnetify", true),
            random_rgb: p.flag("Sketch/randomRGB", false),
            random_opacity: p.flag("Sketch/randomOpacity", false),
            distance_density: p.flag("Sketch/distanceDensity", true),
            distance_opacity: p.flag("Sketch/distanceOpacity", false),
            antialias: p.flag("Sketch/antiAliasing", false),
            line_width_option: CurveOption::read(p, "", "Line width", true, 0.0, 1.0),
            offset_option: CurveOption::read(p, "", "Offset scale", true, 0.0, 1.0),
            density_option: CurveOption::read(p, "", "Density", true, 0.0, 1.0),
        }
    }
}

/// The spray brush: at each of its dabs (spaced by a share of its diameter), particles scattered
/// over a circle `diameter` across (squashed by its aspect, turned, scaled), as many as its count
/// or as cover its area, each an ellipse, a rectangle or a pixel `size` big (or a share of the
/// diameter), or with its shape option off the brush's own tip, turned at random, sized at random,
/// coloured at random if it says.
#[derive(Clone, Debug)]
pub struct Spray {
    pub diameter: f32,
    pub aspect: f32,
    pub rotation: f32,
    pub scale: f32,
    pub spacing: f32,
    pub jitter: Option<f32>,
    pub count: f32,
    pub coverage: Option<f32>,
    pub gaussian: bool,
    pub centre_biased: bool,
    pub deviation: f32,
    pub shape: u32,
    /// Its particles are the brush's tip (its shape option off).
    pub own_tip: bool,
    pub size: (f32, f32),
    pub proportional: bool,
    pub random_size: bool,
    pub turn: f32,
    pub random_turn: Option<f32>,
    pub follow_cursor: Option<f32>,
    pub follow_angle: Option<f32>,
    pub random_hsv: Option<(f32, f32, f32)>,
    pub random_opacity: bool,
    pub per_particle: bool,
    pub mix_background: bool,
}

impl Spray {
    pub fn read(p: &Preset) -> Spray {
        let n = |k: &str, otherwise: f32| p.number(k, otherwise);
        // its shape's dynamics as a newer preset keeps them, else as Krita 2.2 did
        let newer = p.get("ShapeDynamicsVersion").is_some_and(|v| v.trim() != "2.2");
        let dynamics = |key: &str| if newer { format!("ShapeDynamics/{key}") } else { format!("SprayShape/{key}") };
        let on = !newer || p.flag("ShapeDynamics/enabled", false);
        let weight =
            |flag: &str, w: &str| (on && p.flag(&dynamics(flag), false)).then(|| n(&dynamics(w), 0.5).clamp(0.0, 1.0));
        let radial = p.get("Spray/radialDistributionType").map(str::trim).unwrap_or("");
        Spray {
            diameter: n("Spray/diameter", 100.0).trunc().max(1.0),
            aspect: n("Spray/aspect", 1.0),
            rotation: n("Spray/rotation", 0.0),
            scale: n("Spray/scale", 1.0),
            spacing: n("Spray/spacing", 0.5),
            jitter: p.flag("Spray/jitterMovement", false).then(|| n("Spray/jitterMoveAmount", 0.0)),
            count: n("Spray/particleCount", 12.0).max(0.0),
            coverage: p.flag("Spray/useDensity", false).then(|| n("Spray/coverage", 10.0) / 100.0),
            gaussian: radial == "gaussian" || (radial.is_empty() && p.flag("Spray/gaussianDistribution", false)),
            centre_biased: p.flag("Spray/radialDistributionCenterBiased", true),
            deviation: n("Spray/radialDistributionStdDeviation", 0.5),
            shape: n("SprayShape/shape", 0.0) as u32,
            own_tip: !p.flag("SprayShape/enabled", true),
            size: (n("SprayShape/width", 6.0).trunc().max(0.0), n("SprayShape/height", 6.0).trunc().max(0.0)),
            proportional: p.flag("SprayShape/proportional", false),
            random_size: on && p.flag(&dynamics("randomSize"), false),
            turn: if on && p.flag(&dynamics("fixedRotation"), false) {
                n(&dynamics("fixedAngle"), 0.0).trunc()
            } else {
                0.0
            },
            random_turn: weight("randomRotation", "randomRotationWeight"),
            follow_cursor: weight("followCursor", "followCursorWeigth"),
            follow_angle: weight("followDrawingAngle", "followDrawingAngleWeigth"),
            random_hsv: p
                .flag("ColorOption/useRandomHSV", false)
                .then(|| (n("ColorOption/hue", 0.0), n("ColorOption/saturation", 0.0), n("ColorOption/value", 0.0))),
            random_opacity: p.flag("ColorOption/useRandomOpacity", false),
            per_particle: p.flag("ColorOption/colorPerParticle", false),
            mix_background: p.flag("ColorOption/mixBgColor", false),
        }
    }

    /// A particle's width and height, for the spray `diameter` across now.
    pub fn particle(&self, diameter: f32) -> (f32, f32) {
        let (w, h) = self.size;
        let (w, h) = if self.proportional { (w / 100.0 * diameter, h / 100.0 * diameter) } else { (w, h) };
        ((w * self.scale).max(1.0), (h * self.scale).max(1.0))
    }
}

/// The tangent normal brush's colour: the pen's lean (or the stroke's way, or the pen's turn) as
/// a normal, each axis to the channel its swizzle says.
#[derive(Clone, Debug)]
pub struct Tangent {
    pub channels: [u32; 3],
    pub direction: u32,
    pub sensitivity: f32,
    pub mix: f32,
}

impl Tangent {
    pub fn read(p: &Preset) -> Tangent {
        let n = |k: &str, otherwise: f32| p.number(k, otherwise);
        Tangent {
            channels: [n("Tangent/swizzleRed", 0.0), n("Tangent/swizzleGreen", 2.0), n("Tangent/swizzleBlue", 4.0)]
                .map(|v| v as u32),
            direction: n("Tangent/directionType", 0.0) as u32,
            sensitivity: n("Tangent/elevationSensitivity", 100.0),
            mix: n("Tangent/mixValue", 50.0),
        }
    }

    /// The colour for the pen as `info` has it, the canvas turned `turn` (radians).
    pub fn colour(&self, info: &Info, turn: f32) -> [f32; 3] {
        let tilt_direction = crate::options::tilt_direction(info) * 360.0;
        let elevation = crate::options::tilt_elevation(info) * 90.0;
        let drawing = (0.75 + info.drawing_angle / std::f32::consts::TAU) * 360.0;
        let (mut direction, elevation) = match self.direction {
            1 => (drawing, 0.0),
            2 => (info.rotation, elevation),
            3 => {
                let m = self.mix / 100.0;
                (tilt_direction * (1.0 - m) + drawing * m, elevation)
            }
            _ => (tilt_direction, elevation),
        };
        if self.direction != 1 {
            direction = (direction - turn.to_degrees()).rem_euclid(360.0);
        }
        let s = self.sensitivity / 100.0;
        let elevation = (elevation * s + (90.0 - s * 90.0)).trunc().to_radians();
        let direction = direction.to_radians();
        let half = |v: f32| if v > 0.0 { 0.5 + v.abs() * 0.5 } else { 0.5 - v.abs() * 0.5 };
        let horizontal = half(elevation.cos() * direction.sin());
        let vertical = half(elevation.cos() * direction.cos());
        let depth = elevation.sin();
        self.channels.map(|c| match c {
            0 => horizontal,
            1 => 1.0 - horizontal,
            2 => vertical,
            3 => 1.0 - vertical,
            4 => depth,
            _ => 1.0 - depth,
        })
    }
}

/// One of a bristle brush's bristles: where it sits on the tip (pixels from its middle), its length
/// (how opaque the tip is there, 0..1) and, for a colour tip, its colour.
#[derive(Clone, Copy, Debug)]
pub struct Bristle {
    pub x: f32,
    pub y: f32,
    pub length: f32,
    pub colour: Option<[f32; 3]>,
}

/// The most bristles a bristle brush keeps (the rest thinned out at random).
pub const MOST_BRISTLES: usize = 4096;

/// Krita's bristle brush: its tip's pixels its bristles; at each move each bristle draws a line
/// from where it was to where it is, the bristles spread by its scale and size, shaken by its
/// randomness, sheared as the pen presses, turned by its rotation option; their ink running out
/// along the stroke if it says.
#[derive(Clone, Debug)]
pub struct Hairy {
    pub bristles: Vec<Bristle>,
    pub scale: f32,
    pub shear: f32,
    pub random: f32,
    pub threshold: bool,
    pub connected: bool,
    pub mouse_pressure: bool,
    /// Its bristles' lines smooth (else hard).
    pub antialias: bool,
    pub ink: Option<Ink>,
}

impl Hairy {
    /// How big a square of the colours under where a stroke starts it soaks its bristles from (0
    /// none): its tip's, where its ink runs out and it soaks it up.
    pub fn soaks(&self) -> u32 {
        if !self.ink.as_ref().is_some_and(|i| i.soak) {
            return 0;
        }
        let reach = self.bristles.iter().fold(0.0_f32, |r, b| r.max(b.x.abs()).max(b.y.abs()));
        (2.0 * reach).ceil() as u32 + 3
    }
}

/// How a bristle brush's ink runs out: over `depletion` (the curve taken at each pixel a bristle
/// has painted), its opacity and saturation falling, by its weights or not.
#[derive(Clone, Debug)]
pub struct Ink {
    pub depletion: Vec<f32>,
    pub saturation: bool,
    pub opacity: bool,
    /// The pressure's, the bristle's length's, its ink's and the depletion's weights.
    pub weights: Option<[f32; 4]>,
    /// Its bristles take their colours from the layer where the stroke starts.
    pub soak: bool,
}

impl Hairy {
    pub fn read(p: &Preset, bristles: Vec<Bristle>) -> Hairy {
        let n = |k: &str, otherwise: f32| p.number(k, otherwise);
        let ink = p.flag("HairyInk/enabled", false).then(|| {
            let amount = (n("HairyInk/inkAmount", 1024.0) as usize).clamp(1, 1 << 16);
            let curve = p.get("HairyInk/inkDepletionCurve").and_then(|c| Curve::parse(c.trim()));
            let depletion = (0..amount)
                .map(|i| {
                    let x = if amount > 1 { i as f32 / (amount - 1) as f32 } else { 0.0 };
                    curve.as_ref().map_or(x, |c| c.at(x))
                })
                .collect();
            Ink {
                depletion,
                saturation: p.flag("HairyInk/useSaturation", false),
                opacity: p.flag("HairyInk/useOpacity", true),
                weights: p.flag("HairyInk/useWeights", false).then(|| {
                    [
                        n("HairyInk/pressureWeights", 50.0) / 100.0,
                        n("HairyInk/bristleLengthWeights", 50.0) / 100.0,
                        n("HairyInk/bristleInkAmountWeight", 50.0) / 100.0,
                        // (Krita takes this one as it is)
                        n("HairyInk/inkDepletionWeight", 50.0),
                    ]
                }),
                soak: p.flag("HairyInk/soak", false),
            }
        });
        Hairy {
            bristles,
            scale: n("HairyBristle/scale", 2.0),
            shear: n("HairyBristle/shear", 0.0),
            random: n("HairyBristle/random", 2.0),
            threshold: p.flag("HairyBristle/threshold", false),
            connected: p.flag("HairyBristle/isConnected", false),
            antialias: p.flag("HairyBristle/antialias", false),
            mouse_pressure: p.flag("HairyBristle/useMousePressure", false),
            ink,
        }
    }
}

/// Krita's hatching brush: at each dab of its tip, the lines of a hatching (anchored to the canvas
/// at its origin, `separation` apart, `thickness` wide, at `angle` degrees) laid inside the dab;
/// crosshatched as its style says.
#[derive(Clone, Debug)]
pub struct Hatching {
    pub angle: f32,
    pub separation: f32,
    pub thickness: f32,
    pub origin: (f32, f32),
    /// 0 none, 1 perpendicular, 2 minus then plus, 3 plus then minus, 4 moiré.
    pub style: u32,
    pub intervals: u32,
    pub opaque_background: bool,
    pub subpixel: bool,
    pub angle_option: CurveOption,
    pub crosshatching_option: CurveOption,
    pub separation_option: CurveOption,
    pub thickness_option: CurveOption,
}

impl Hatching {
    pub fn read(p: &Preset) -> Hatching {
        let n = |k: &str, otherwise: f32| p.number(k, otherwise);
        let style = if p.flag("Hatching/bool_moirepattern", false) {
            4
        } else if p.flag("Hatching/bool_nocrosshatching", true) {
            0
        } else if p.flag("Hatching/bool_perpendicular", false) {
            1
        } else if p.flag("Hatching/bool_minusthenplus", false) {
            2
        } else if p.flag("Hatching/bool_plusthenminus", false) {
            3
        } else {
            0
        };
        Hatching {
            angle: n("Hatching/angle", -60.0),
            separation: n("Hatching/separation", 6.0).max(0.5),
            thickness: n("Hatching/thickness", 1.0),
            origin: (n("Hatching/origin_x", 50.0), n("Hatching/origin_y", 50.0)),
            style,
            intervals: n("Hatching/separationintervals", 2.0) as u32,
            opaque_background: p.flag("Hatching/bool_opaquebackground", false),
            subpixel: p.flag("Hatching/bool_subpixelprecision", false),
            angle_option: CurveOption::read(p, "", "Angle", true, 0.0, 1.0),
            crosshatching_option: CurveOption::read(p, "", "Crosshatching", true, 0.0, 1.0),
            separation_option: CurveOption::read(p, "", "Separation", true, 0.0, 1.0),
            thickness_option: CurveOption::read(p, "", "Thickness", true, 0.0, 1.0),
        }
    }

    /// `spin` degrees on from its angle, folded as Krita folds it: into -90..90.
    pub fn spun(&self, spin: f32) -> f32 {
        let a = self.angle + spin;
        let sign = if a < 0.0 { -1.0 } else { 1.0 };
        let folded = (a % 180.0).abs();
        if folded <= 90.0 { sign * folded } else { sign * -(180.0 - folded) }
    }

    /// Its separation for the separation option's value `v`: a power of two of it by which of its
    /// intervals `v` falls in.
    pub fn separation_at(&self, v: f32) -> f32 {
        let n = self.intervals;
        if !(2..=7).contains(&n) {
            return self.separation;
        }
        let size = 1.0 / n as f32;
        let mut base = (n / 2) as i32;
        if n.is_multiple_of(2) {
            base -= 1;
        }
        let mut upper = 0.0;
        for i in 0..n {
            let lower = upper;
            upper += size;
            if i == n - 1 {
                upper = 1.0;
            }
            if v >= lower && v <= upper {
                return self.separation * 2_f32.powi(base - i as i32);
            }
        }
        self.separation
    }
}

/// Krita's curve brush: at each move a line from where the pen was (if it says), and once it has
/// passed `history` points, a curve from the first of them to the last through those between.
#[derive(Clone, Debug)]
pub struct Curves {
    pub line_width: f32,
    pub connection: bool,
    pub history: usize,
    pub smoothing: bool,
    pub opacity: f32,
    pub line_width_option: CurveOption,
    pub opacity_option: CurveOption,
}

impl Curves {
    pub fn read(p: &Preset) -> Curves {
        Curves {
            line_width: p.number("Curve/lineWidth", 1.0).trunc(),
            connection: p.flag("Curve/makeConnection", false),
            history: (p.number("Curve/strokeHistorySize", 30.0) as usize).clamp(3, 1000),
            smoothing: p.flag("Curve/smoothing", false),
            opacity: p.number("Curve/curvesOpacity", 0.0),
            line_width_option: CurveOption::read(p, "", "Line width", true, 0.1, 1.0),
            opacity_option: CurveOption::read(p, "", "Curves opacity", true, 0.1, 1.0),
        }
    }
}

/// Krita's particle brush: `count` particles pulled towards the pen, each by its own pull, slowed by
/// its gravity, `iterations` steps at each move, each step a dot of `weight`.
#[derive(Clone, Debug)]
pub struct Particles {
    pub count: usize,
    pub iterations: usize,
    pub gravity: f32,
    pub weight: f32,
    pub scale: (f32, f32),
}

impl Particles {
    pub fn read(p: &Preset) -> Particles {
        Particles {
            count: (p.number("Particle/count", 50.0) as usize).min(2000),
            iterations: (p.number("Particle/iterations", 10.0) as usize).min(200),
            gravity: p.number("Particle/gravity", 0.989),
            weight: p.number("Particle/weight", 0.2),
            scale: (p.number("Particle/scaleX", 0.3), p.number("Particle/scaleY", 0.3)),
        }
    }
}

/// Krita's grid brush: at each dab (a cell apart), the cells of a grid anchored to the canvas
/// within its diameter, each divided and drawn as a shape inset by its borders, in the colour
/// (moved at random if it says) or the one under the cell's middle.
#[derive(Clone, Debug)]
pub struct Grid {
    pub diameter: f32,
    pub cell: (f32, f32),
    pub offset: (f32, f32),
    pub division: u32,
    pub pressure_division: bool,
    pub scale: f32,
    pub border: (f32, f32),
    pub random_border: bool,
    /// 0 ellipse, 1 rectangle, 2 a pixel line, 3 a line, 4 a thick line.
    pub shape: u32,
    pub sample: bool,
    pub fill_background: bool,
    pub mix_background: bool,
    pub random_hsv: Option<(f32, f32, f32)>,
    pub random_opacity: bool,
    pub per_cell: bool,
}

impl Grid {
    pub fn read(p: &Preset) -> Grid {
        let n = |k: &str, otherwise: f32| p.number(k, otherwise);
        let cell = (n("Grid/gridWidth", 25.0).trunc().max(1.0), n("Grid/gridHeight", 25.0).trunc().max(1.0));
        let diameter = n("Grid/diameter", 0.0).trunc();
        Grid {
            // older presets without one: a cell across
            diameter: if diameter == 0.0 { cell.0 } else { diameter.max(1.0) },
            cell,
            offset: (n("Grid/horizontalOffset", 0.0), n("Grid/verticalOffset", 0.0)),
            division: n("Grid/divisionLevel", 0.0).max(0.0) as u32,
            pressure_division: p.flag("Grid/pressureDivision", false),
            scale: n("Grid/scale", 1.0),
            border: (n("Grid/verticalBorder", 0.0), n("Grid/horizontalBorder", 0.0)),
            random_border: p.flag("Grid/randomBorder", false),
            shape: n("GridShape/shape", 0.0) as u32,
            sample: p.flag("ColorOption/sampleInputColor", false),
            fill_background: p.flag("ColorOption/fillBackground", false),
            mix_background: p.flag("ColorOption/mixBgColor", false),
            random_hsv: p
                .flag("ColorOption/useRandomHSV", false)
                .then(|| (n("ColorOption/hue", 0.0), n("ColorOption/saturation", 0.0), n("ColorOption/value", 0.0))),
            random_opacity: p.flag("ColorOption/useRandomOpacity", false),
            per_cell: p.flag("ColorOption/colorPerParticle", false),
        }
    }
}

/// Krita 3's chalk brush (gone from Krita since): at every pixel of the path, half the pixels within
/// its radius at random, in the colour, its ink running out (opacity and saturation falling by the
/// log of its dabs) if it says.
#[derive(Clone, Debug)]
pub struct Chalk {
    pub radius: f32,
    pub depletion: bool,
    pub opacity: bool,
    pub saturation: bool,
}

impl Chalk {
    pub fn read(p: &Preset) -> Chalk {
        Chalk {
            radius: p.number("Chalk/radius", 5.0).trunc().max(0.0),
            depletion: p.flag("Chalk/inkDepletion", false),
            opacity: p.flag("Chalk/opacity", false),
            saturation: p.flag("Chalk/saturation", false),
        }
    }

    /// Its tip: a hard circle its radius across each way, half its pixels gone at random.
    pub fn tip(&self) -> String {
        let diameter = 2.0 * self.radius + 1.0;
        format!(
            r#"<Brush type="auto_brush" spacing="{}" useAutoSpacing="0" density="0.5" randomness="0" BrushVersion="2"><MaskGenerator type="circle" id="default" diameter="{diameter}" ratio="1" hfade="1" vfade="1" antialiasEdges="0"/></Brush>"#,
            1.0 / diameter
        )
    }
}

/// Krita 3's dyna brush (Paul Haeberli's "dynadraw", gone from Krita since): the pen pulls a weight
/// on a spring (its mass and drag), and at each move the weight draws: circles growing with its
/// speed, a ribbon (filled or its outline) as wide as it is slow, or lines.
#[derive(Clone, Debug)]
pub struct Dyna {
    /// 0 circles, 1 a filled ribbon, 2 its outline, 3 lines.
    pub action: u32,
    pub width: f32,
    pub width_range: f32,
    pub mass: f32,
    pub drag: f32,
    pub fixed_angle: Option<(f32, f32)>,
    pub diameter: f32,
    pub line_count: usize,
    pub line_spacing: f32,
    pub line: bool,
    pub two_circles: bool,
}

impl Dyna {
    pub fn read(p: &Preset) -> Dyna {
        let n = |k: &str, otherwise: f32| p.number(k, otherwise);
        let angle = n("Dyna/angle", 0.0).to_radians();
        Dyna {
            action: n("Dyna/action", 0.0) as u32,
            width: n("Dyna/width", 1.0),
            width_range: n("Dyna/widthRange", 0.0),
            mass: n("Dyna/mass", 0.5),
            drag: n("Dyna/drag", 0.5),
            fixed_angle: p.flag("Dyna/useFixedAngle", false).then(|| (angle.cos(), angle.sin())),
            diameter: n("Dyna/diameter", 6.0).trunc(),
            line_count: (n("Dyna/lineCount", 10.0) as usize).min(200),
            line_spacing: n("Dyna/lineSpacing", 1.0),
            line: p.flag("Dyna/enableLine", false),
            two_circles: p.flag("Dyna/useTwoCirles", false),
        }
    }
}

/// The weight a dyna brush's pen pulls (in the canvas's size as 1 each way): where it is and was,
/// its speed and the way it is drawn across.
#[derive(Clone, Debug, Default)]
pub struct DynaWeight {
    pub at: (f32, f32),
    pub was: (f32, f32),
    pub velocity: (f32, f32),
    pub speed: f32,
    pub pull: f32,
    pub across: (f32, f32),
}

impl DynaWeight {
    /// The weight pulled towards `to` once; false where it doesn't move.
    pub fn pull(&mut self, dyna: &Dyna, to: (f32, f32)) -> bool {
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let mass = lerp(5.0, 160.0, dyna.mass);
        let drag = lerp(0.0, 1.0, dyna.drag * dyna.drag);
        let force = (to.0 - self.at.0, to.1 - self.at.1);
        self.pull = force.0.hypot(force.1);
        if self.pull < 0.000_001 {
            return false;
        }
        self.velocity = (self.velocity.0 + force.0 / mass, self.velocity.1 + force.1 / mass);
        self.speed = self.velocity.0.hypot(self.velocity.1);
        if self.speed < 0.000_001 {
            return false;
        }
        self.across = dyna.fixed_angle.unwrap_or((-self.velocity.1 / self.speed, self.velocity.0 / self.speed));
        self.velocity = (self.velocity.0 * (1.0 - drag), self.velocity.1 * (1.0 - drag));
        self.was = self.at;
        self.at = (self.at.0 + self.velocity.0, self.at.1 + self.velocity.1);
        true
    }
}

/// Krita's clone brush: each dab lays the layer's pixels from elsewhere under its tip, over what
/// is there, from the layer as it was before the stroke (or the image, all its layers): from as far
/// away as the stroke's start was from the source (the source moving with the brush), or from the
/// source itself at each dab. Its source is set by a tap (Krita's is set with a key held; the app
/// hands an engine no keys). Healing (matching the colours around) isn't done.
#[derive(Clone, Debug)]
pub struct Clone {
    pub from_image: bool,
    pub move_source: bool,
}

impl Clone {
    pub fn read(p: &Preset) -> Clone {
        Clone {
            from_image: p.flag("Duplicateop/CloneFromProjection", false),
            move_source: p.flag("Duplicateop/MoveSourcePoint", true),
        }
    }
}

/// Krita's deform brush: each dab (an ellipse of its own diameter, squash, scale and turn, as dense
/// as it says) takes the pixels under it from elsewhere under it, by its action: grown or shrunk,
/// swirled, moved with the pen, through a lens, or scattered; by its amount (or more at each dab,
/// with its counter), from the layer as it is (or as it was before the stroke), mixed from the
/// pixels around or not.
#[derive(Clone, Debug)]
pub struct Deform {
    pub action: u32,
    pub amount: f32,
    pub bilinear: bool,
    pub counter: bool,
    pub old_data: bool,
    pub diameter: f32,
    pub aspect: f32,
    pub scale: f32,
    /// Degrees.
    pub rotation: f32,
    pub spacing: f32,
    pub density: f32,
    pub jitter: Option<f32>,
}

impl Deform {
    pub fn read(p: &Preset) -> Deform {
        let n = |k: &str, otherwise: f32| p.number(k, otherwise);
        Deform {
            action: n("Deform/deformAction", 1.0) as u32,
            amount: n("Deform/deformAmount", 0.2),
            bilinear: p.flag("Deform/bilinear", false),
            counter: p.flag("Deform/useCounter", false),
            old_data: p.flag("Deform/useOldData", false),
            diameter: n("Brush/diameter", 20.0).max(1.0),
            aspect: n("Brush/aspect", 1.0).max(0.01),
            scale: n("Brush/scale", 1.0).max(0.0),
            rotation: n("Brush/rotation", 0.0),
            spacing: n("Brush/spacing", 0.3).max(0.0),
            density: n("Brush/density", 1.0).clamp(0.0, 1.0),
            jitter: p.flag("Brush/jitterMovementEnabled", false).then(|| n("Brush/jitterMovement", 0.0)),
        }
    }

    /// Its tip: a hard ellipse its diameter across and its aspect down, as dense as it says.
    pub fn tip(&self) -> String {
        let (ratio, upright) = if self.aspect <= 1.0 { (self.aspect, 0.0) } else { (1.0 / self.aspect, 1.570_796_4) };
        let angle = upright + self.rotation.to_radians();
        let across = self.diameter * self.aspect.max(1.0);
        format!(
            r#"<Brush type="auto_brush" spacing="{}" useAutoSpacing="0" angle="{angle}" density="{}" randomness="0" BrushVersion="2"><MaskGenerator type="circle" id="default" diameter="{across}" ratio="{ratio}" hfade="1" vfade="1" antialiasEdges="0"/></Brush>"#,
            0.5 * self.spacing / self.aspect.max(1.0),
            self.density,
        )
    }
}

/// Krita's filter brush, as far as the app's own blur goes: each dab lays the layer as it was before
/// the stroke, blurred (and sharpened, an unsharp mask), under its tip. `blur` is the app's (0..1),
/// matched to the filter's spread; other filters lay nothing.
#[derive(Clone, Debug)]
pub struct Filter {
    pub blur: f32,
    pub sharpen: f32,
}

impl Filter {
    pub fn read(p: &Preset) -> Option<Filter> {
        let c = |k: &str, otherwise: f32| p.number(&format!("Filter/config/{k}"), otherwise);
        // a gaussian's spread from its radius, as Krita reckons it
        let sigma_of = |radius: f32| 0.3 * radius + 0.3;
        let (sigma, sharpen) = match p.get("Filter/id").map(str::trim)? {
            "gaussian blur" => (sigma_of(c("horizRadius", 5.0).max(c("vertRadius", 5.0))), 0.0),
            // a round kernel half its width across: about half of that a gaussian's spread
            "blur" => (0.5 * c("halfWidth", 5.0).max(c("halfHeight", 5.0)) * c("strength", 100.0) / 100.0, 0.0),
            "unsharp" => (sigma_of(c("halfSize", 1.0)), c("amount", 0.5).max(0.0)),
            _ => return None,
        };
        Some(Filter { blur: app_blur(sigma), sharpen })
    }
}

/// The app's blur (0..1) whose spread is nearest `sigma` pixels (its spread is a / 3 times the
/// whole 64 a above, its reach 32 a).
fn app_blur(sigma: f32) -> f32 {
    let spread = |a: f32| a / 3.0 * (64.0 * a).ceil();
    (1..=1000)
        .map(|i| i as f32 / 1000.0)
        .min_by(|a, b| (spread(*a) - sigma).abs().total_cmp(&(spread(*b) - sigma).abs()))
        .unwrap_or(0.0)
}

/// Krita's shape brush: the outline the stroke makes, from where it started, filled in the colour
/// (by the even-odd rule, or the winding one), its corners rounded off as the pen goes as far as its
/// smoothing says, or the pen's place drawn on by its speed. Filled as the stroke ends (Krita fills
/// it as it goes); its displacement isn't done.
#[derive(Clone, Debug)]
pub struct Shape {
    pub smoothing: Option<f32>,
    pub speed: Option<f32>,
    pub winding: bool,
    pub hard: bool,
}

impl Shape {
    pub fn read(p: &Preset) -> Shape {
        let n = |k: &str, otherwise: f32| p.number(k, otherwise);
        Shape {
            smoothing: p.flag("Experiment/smoothing", false).then(|| n("Experiment/smoothingValue", 20.0)),
            speed: p.flag("Experiment/speedEnabled", false).then(|| n("Experiment/speed", 42.0) * 0.01 * 35.0),
            winding: p.flag("Experiment/windingFill", false),
            hard: p.flag("Experiment/hardEdge", false),
        }
    }
}
