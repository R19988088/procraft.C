//! Krita's dynamics: an option (size, opacity, rotation...) follows sensors of the pen and the
//! stroke, each through a curve of its own (or one they share), the sensors' values combined as the
//! option says (multiplied, added, the most, the least or the difference), then scaled by the
//! option's strength. Sensors that turn (drawing angle, tilt direction, the pen's barrel) and the
//! random ones add rather than scale.

use crate::curve::Curve;
use crate::preset::Preset;
use crate::xml::Element;

/// What a dab is drawn with: the pen's state where it lies, and the stroke's so far.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub pressure: f32,
    /// The most the pen has pressed in the stroke so far.
    pub pressure_in: f32,
    /// Degrees the pen leans across and down (-60..60), and its barrel's turn (degrees).
    pub x_tilt: f32,
    pub y_tilt: f32,
    pub rotation: f32,
    /// 0..1, as Krita measures it.
    pub speed: f32,
    /// Radians, the way the stroke goes; the way it went at its start.
    pub drawing_angle: f32,
    pub locked_angle: f32,
    /// Canvas pixels drawn, milliseconds since the stroke began, dabs laid before this one.
    pub distance: f32,
    pub time: f32,
    pub dab: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pressure,
    PressureIn,
    XTilt,
    YTilt,
    TiltDirection,
    TiltElevation,
    Speed,
    DrawingAngle,
    Rotation,
    Distance,
    Time,
    Fade,
    FuzzyDab,
    FuzzyStroke,
    Perspective,
    TangentialPressure,
}

impl Kind {
    fn named(id: &str) -> Option<Kind> {
        Some(match id {
            "pressure" => Kind::Pressure,
            "pressurein" => Kind::PressureIn,
            "xtilt" => Kind::XTilt,
            "ytilt" => Kind::YTilt,
            "ascension" | "tiltdirection" => Kind::TiltDirection,
            "declination" | "tiltelevation" => Kind::TiltElevation,
            "speed" => Kind::Speed,
            "drawingangle" => Kind::DrawingAngle,
            "rotation" => Kind::Rotation,
            "distance" => Kind::Distance,
            "time" => Kind::Time,
            "fade" => Kind::Fade,
            "fuzzy" => Kind::FuzzyDab,
            "fuzzystroke" => Kind::FuzzyStroke,
            "perspective" => Kind::Perspective,
            "tangentialpressure" => Kind::TangentialPressure,
            _ => return None,
        })
    }

    /// It adds to the option rather than scaling it (-1..1).
    fn adds(self) -> bool {
        matches!(self, Kind::Rotation | Kind::TiltDirection | Kind::FuzzyDab | Kind::FuzzyStroke)
    }

    /// It gives the option an angle outright (0..1 round the circle).
    fn absolute(self) -> bool {
        self == Kind::DrawingAngle
    }
}

#[derive(Clone, Debug)]
pub struct Sensor {
    pub kind: Kind,
    curve: Option<Curve>,
    /// For distance, time and fade: how long it takes to reach 1, and whether it starts again.
    length: f32,
    periodic: bool,
    /// For the drawing angle: the angle added (degrees), the steps (degrees) of a fan of dabs
    /// laid round a corner, and whether it keeps the way the stroke went at its start.
    angle_offset: f32,
    fan: Option<f32>,
    locked: bool,
}

/// Randomness for the fuzzy sensors: a dab's, and the stroke's for an option.
pub trait Chance {
    fn dab(&mut self) -> f32;
    fn stroke(&mut self, key: &str) -> f32;
}

impl Sensor {
    fn value(&self, info: &Info, chance: &mut dyn Chance, option: &str) -> f32 {
        let wrap = |v: f32| v.rem_euclid(1.0);
        let along = |v: f32| {
            let length = self.length.max(1e-3);
            let v = if self.periodic { v.rem_euclid(length) } else { v.min(length) };
            v / length
        };
        match self.kind {
            Kind::Pressure => info.pressure,
            Kind::PressureIn => info.pressure_in,
            Kind::XTilt => 1.0 - info.x_tilt.abs() / 60.0,
            Kind::YTilt => 1.0 - info.y_tilt.abs() / 60.0,
            Kind::TiltDirection => 2.0 * tilt_direction(info) - 1.0,
            Kind::TiltElevation => tilt_elevation(info),
            Kind::Speed => info.speed,
            Kind::DrawingAngle => {
                let angle = if self.locked { info.locked_angle } else { info.drawing_angle };
                wrap(0.5 + angle / std::f32::consts::TAU + self.angle_offset / 360.0)
            }
            Kind::Rotation => info.rotation / 180.0,
            Kind::Distance => along(info.distance),
            Kind::Time => along(info.time),
            Kind::Fade => along(info.dab as f32),
            Kind::FuzzyDab => 2.0 * chance.dab() - 1.0,
            Kind::FuzzyStroke => 2.0 * chance.stroke(option) - 1.0,
            // no perspective assistant: 1, as in Krita without one; no airbrush wheel: 0, as a pen
            // without one gives Krita
            Kind::Perspective => 1.0,
            Kind::TangentialPressure => 0.0,
        }
    }

    /// Its value through its curve: one that adds is taken to 0..1 for the curve and back, one
    /// that gives an angle is turned half round for it and back.
    fn parameter(&self, info: &Info, chance: &mut dyn Chance, option: &str) -> f32 {
        let v = self.value(info, chance, option);
        let Some(curve) = &self.curve else { return v };
        if self.kind.adds() {
            2.0 * curve.at(0.5 * (1.0 + v)) - 1.0
        } else if self.kind.absolute() {
            (curve.at((v + 0.5).rem_euclid(1.0)) + 0.5).rem_euclid(1.0)
        } else {
            curve.at(v)
        }
    }
}

/// The direction the pen leans, 0..1 round the circle, as Krita reckons it.
pub fn tilt_direction(info: &Info) -> f32 {
    let (x, y) = (info.x_tilt, info.y_tilt);
    let angle = if x.abs() < 1e-6 && y.abs() < 1e-6 {
        let sign = |v: f32| if v < 0.0 { -1.0 } else { 1.0 };
        -sign(x) * sign(y) * std::f32::consts::FRAC_PI_2
    } else {
        (-x).atan2(y)
    };
    angle / std::f32::consts::TAU + 0.5
}

/// How upright the pen is, 0..1 (1 upright), as Krita reckons it.
pub fn tilt_elevation(info: &Info) -> f32 {
    let x = (info.x_tilt / 60.0).clamp(-1.0, 1.0);
    let y = (info.y_tilt / 60.0).clamp(-1.0, 1.0);
    let e = if x.abs() > y.abs() { (1.0 + y * y).sqrt() } else { (1.0 + x * x).sqrt() };
    let cos = ((x * x + y * y).sqrt() / e).clamp(-1.0, 1.0);
    cos.acos() / std::f32::consts::FRAC_PI_2
}

/// An option that follows sensors.
#[derive(Clone, Debug)]
pub struct CurveOption {
    pub id: String,
    pub checked: bool,
    use_curve: bool,
    mode: u32,
    pub strength: f32,
    min: f32,
    max: f32,
    sensors: Vec<Sensor>,
}

/// What its sensors come to: a scaling, what adds, and an angle given outright.
#[derive(Default)]
struct Parts {
    scaling: Option<f32>,
    additive: Option<f32>,
    absolute: Option<f32>,
}

impl CurveOption {
    /// The option `id` as `preset` keeps it (its keys after `prefix`, as a masking brush's are),
    /// its strength `min..=max`; `checkable`: whether it can be switched off. Its sensors share one
    /// curve (see [`common_curve`]) unless it says each has its own.
    pub fn read(preset: &Preset, prefix: &str, id: &str, checkable: bool, min: f32, max: f32) -> CurveOption {
        let key = |suffix: &str| format!("{prefix}{id}{suffix}");
        let checked = !checkable || preset.flag(&format!("{prefix}Pressure{id}"), false);
        let use_same = preset.flag(&key("UseSameCurve"), true);
        let definition = preset.get(&key("Sensor")).unwrap_or("");
        // the oldest presets kept one curve for all the sensors apart, where theirs keep none
        let legacy = (!definition.contains("curve") && preset.flag(&format!("{prefix}Custom{id}"), false))
            .then(|| preset.get(&format!("{prefix}Curve{id}")).unwrap_or(LINE));
        let mut sensors = Vec::new();
        for e in &sensor_elements(definition) {
            sensors.extend(sensor(e));
        }
        if sensors.is_empty() {
            sensors.push(Sensor {
                kind: Kind::Pressure,
                curve: None,
                length: 1.0,
                periodic: false,
                angle_offset: 0.0,
                fan: None,
                locked: false,
            });
        }
        if use_same {
            let common = Curve::parse(&common_curve(preset, prefix, id));
            for s in &mut sensors {
                s.curve = common.clone();
            }
        } else if let Some(curve) = legacy {
            for s in &mut sensors {
                s.curve = Curve::parse(curve);
            }
        }
        CurveOption {
            id: id.to_owned(),
            checked,
            use_curve: preset.flag(&key("UseCurve"), true),
            mode: preset.number(&key("curveMode"), 0.0) as u32,
            strength: preset.number(&key("Value"), max).clamp(min, max),
            min,
            max,
            sensors,
        }
    }

    fn parts(&self, info: &Info, chance: &mut dyn Chance) -> Parts {
        let mut parts = Parts::default();
        if !self.use_curve {
            return parts;
        }
        let mut scaling = Vec::new();
        for s in &self.sensors {
            let v = s.parameter(info, chance, &self.id);
            if s.kind.adds() {
                *parts.additive.get_or_insert(0.0) += v;
            } else if s.kind.absolute() {
                parts.absolute = Some(v);
            } else {
                scaling.push(v);
            }
        }
        if !scaling.is_empty() {
            let most = scaling.iter().copied().fold(f32::MIN, f32::max);
            let least = scaling.iter().copied().fold(f32::MAX, f32::min);
            // (one sensor is its value, however several would combine)
            parts.scaling = Some(match self.mode {
                _ if scaling.len() == 1 => most,
                1 => scaling.iter().sum(),
                2 => most,
                3 => least,
                4 => most - least,
                _ => scaling.iter().product(),
            });
        }
        parts
    }

    /// Its value as a size (a share, scaled by its strength if `strength`), held to its range.
    pub fn size_like(&self, info: &Info, chance: &mut dyn Chance, strength: bool) -> f32 {
        let parts = self.parts(info, chance);
        let constant = if strength { self.strength } else { 1.0 };
        let offset = parts.absolute.unwrap_or(1.0);
        let scaling = parts.scaling.unwrap_or(1.0);
        let additive = parts.additive.map_or(1.0, |a| 0.5 * (1.0 + a));
        (constant * offset * scaling * additive).clamp(self.min, self.max)
    }

    /// Its value as a turn (-1..1, a whole turn 2), from `base` (0..1 of a turn) where nothing
    /// gives it outright; the scaling's share taken `coeff` times.
    pub fn rotation_like(&self, info: &Info, chance: &mut dyn Chance, base: f32, coeff: f32) -> f32 {
        let parts = self.parts(info, chance);
        let offset = parts.absolute.unwrap_or(base);
        let scaling = parts.scaling.map_or(0.0, |s| 2.0 * s - 1.0);
        let additive = parts.additive.unwrap_or(0.0);
        let v = 2.0 * offset + self.strength * (coeff * scaling + additive);
        (v + 1.0).rem_euclid(2.0) - 1.0
    }

    /// The option's value as Krita's size-like options give it: its strength while unchecked.
    pub fn apply(&self, info: &Info, chance: &mut dyn Chance) -> f32 {
        if self.checked { self.size_like(info, chance, true) } else { self.strength }
    }

    /// The steps (radians) of the fan of dabs it lays round a corner: where it is switched on and
    /// follows the way the stroke goes with fan corners (not where that way is locked).
    pub fn fan_step(&self) -> Option<f32> {
        if !self.checked {
            return None;
        }
        let s = self.sensors.iter().find(|s| s.kind == Kind::DrawingAngle)?;
        s.fan.filter(|_| !s.locked).map(|step| step.max(1.0).to_radians())
    }

    /// Whether anything about it changes from dab to dab.
    pub fn varies(&self) -> bool {
        self.checked && self.use_curve
    }
}

/// The straight curve, which changes nothing.
const LINE: &str = "0,0;1,1;";

/// The sensors a setting's XML lists: one, or a list of them.
fn sensor_elements(definition: &str) -> Vec<Element> {
    let Some(root) = crate::xml::parse(definition) else { return Vec::new() };
    if root.attribute("id") == Some("sensorslist") {
        root.elements().filter(|e| e.name == "ChildSensor").cloned().collect()
    } else {
        vec![root]
    }
}

/// The curve option `id`'s sensors share, as Krita reads it: its `commonCurve` setting; else (a
/// preset from before it kept one) the curve its last sensor keeps; else, where its sensors keep
/// none, the one the oldest presets kept apart (`Custom` and `Curve`); else the straight line.
pub fn common_curve(preset: &Preset, prefix: &str, id: &str) -> String {
    let key = |suffix: &str| format!("{prefix}{id}{suffix}");
    let definition = preset.get(&key("Sensor")).unwrap_or("");
    let mut common = sensor_elements(definition)
        .iter()
        .rfind(|e| e.attribute("id").and_then(Kind::named).is_some())
        .and_then(|e| e.child("curve").map(|c| c.text().trim().to_owned()))
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| LINE.to_owned());
    if !definition.contains("curve") {
        common = if preset.flag(&format!("{prefix}Custom{id}"), false) {
            preset.get(&format!("{prefix}Curve{id}")).unwrap_or(LINE).trim().to_owned()
        } else {
            LINE.to_owned()
        };
    }
    if let (true, Some(kept)) = (preset.flag(&key("UseSameCurve"), true), preset.get(&key("commonCurve"))) {
        common = kept.trim().to_owned();
    }
    if common.is_empty() { LINE.to_owned() } else { common }
}

fn sensor(e: &Element) -> Option<Sensor> {
    let kind = Kind::named(e.attribute("id")?)?;
    let curve = e.child("curve").and_then(|c| Curve::parse(&c.text()));
    // how long distance, fade and time take to reach 1: the time's kept as its duration (ms)
    let length = match kind {
        Kind::Time => e.number("duration"),
        _ => e.number("length"),
    }
    .map(f32::trunc)
    .unwrap_or(match kind {
        Kind::Fade => 1000.0,
        _ => 30.0,
    });
    Some(Sensor {
        kind,
        curve,
        length,
        periodic: e.attribute("periodic").is_some_and(|p| p.trim() == "1" || p.trim() == "true"),
        angle_offset: e.number("angleOffset").unwrap_or(0.0),
        fan: (kind == Kind::DrawingAngle && e.number("fanCornersEnabled").is_some_and(|f| f != 0.0))
            .then(|| e.number("fanCornersStep").unwrap_or(30.0).trunc()),
        locked: kind == Kind::DrawingAngle && e.number("lockedAngleMode").is_some_and(|l| l != 0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Still;
    impl Chance for Still {
        fn dab(&mut self) -> f32 {
            0.5
        }
        fn stroke(&mut self, _: &str) -> f32 {
            0.5
        }
    }

    fn preset(params: &[(&str, &str)]) -> Preset {
        Preset { params: params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), ..Preset::default() }
    }

    #[test]
    fn a_size_follows_the_pressure_through_its_curve() {
        let p = preset(&[
            ("PressureSize", "true"),
            ("SizeSensor", r#"<params id="pressure"/>"#),
            ("SizeUseSameCurve", "true"),
            ("SizecommonCurve", "0,0.5;1,1;"),
            ("SizeValue", "0.8"),
        ]);
        let size = CurveOption::read(&p, "", "Size", true, 0.0, 1.0);
        let info = Info { pressure: 0.0, ..Info::default() };
        assert!((size.apply(&info, &mut Still) - 0.4).abs() < 1e-3);
        let full = Info { pressure: 1.0, ..Info::default() };
        assert!((size.apply(&full, &mut Still) - 0.8).abs() < 1e-3);
        // unchecked, its strength
        let off = CurveOption::read(&preset(&[("SizeValue", "0.6")]), "", "Size", true, 0.0, 1.0);
        assert!((off.apply(&info, &mut Still) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn sensors_listed_together_multiply_and_fuzz_adds() {
        let p = preset(&[
            ("PressureOpacity", "true"),
            (
                "OpacitySensor",
                r#"<params id="sensorslist"><ChildSensor id="pressure"/><ChildSensor id="fuzzy"/></params>"#,
            ),
            ("OpacityUseSameCurve", "true"),
        ]);
        let o = CurveOption::read(&p, "", "Opacity", true, 0.0, 1.0);
        // fuzz at its middle adds nothing: half of the scaling
        let v = o.size_like(&Info { pressure: 0.5, ..Info::default() }, &mut Still, true);
        assert!((v - 0.25).abs() < 1e-4, "{v}");
    }

    #[test]
    fn a_lone_sensor_is_its_value_whatever_the_mode_says() {
        // "difference" of one value would be nothing
        let p = preset(&[("PressureOpacity", "true"), ("OpacitycurveMode", "4"), ("OpacityValue", "0.8")]);
        let o = CurveOption::read(&p, "", "Opacity", true, 0.0, 1.0);
        let v = o.size_like(&Info { pressure: 0.5, ..Info::default() }, &mut Still, true);
        assert!((v - 0.4).abs() < 1e-4, "{v}");
    }

    #[test]
    fn a_sensors_own_curve_is_shared_where_no_common_curve_is_kept() {
        // as older presets keep it: the curve in the sensor alone
        let p = preset(&[
            ("PressureSize", "true"),
            ("SizeSensor", r#"<params id="pressure"><curve>0,0.5;1,1;</curve></params>"#),
            ("SizeUseSameCurve", "true"),
        ]);
        let size = CurveOption::read(&p, "", "Size", true, 0.0, 1.0);
        let v = size.apply(&Info { pressure: 0.0, ..Info::default() }, &mut Still);
        assert!((v - 0.5).abs() < 1e-3, "{v}");
        assert_eq!(common_curve(&p, "", "Size"), "0,0.5;1,1;");
        // a list's last sensor's, for all of them
        let list = preset(&[
            ("PressureSize", "true"),
            (
                "SizeSensor",
                r#"<params id="sensorslist"><ChildSensor id="speed"><curve>0,1;1,1;</curve></ChildSensor><ChildSensor id="pressure"><curve>0,0.25;1,1;</curve></ChildSensor></params>"#,
            ),
        ]);
        assert_eq!(common_curve(&list, "", "Size"), "0,0.25;1,1;");
        // a kept common curve wins
        let kept = preset(&[
            ("SizeSensor", r#"<params id="pressure"><curve>0,0.5;1,1;</curve></params>"#),
            ("SizecommonCurve", "0,1;1,0;"),
        ]);
        assert_eq!(common_curve(&kept, "", "Size"), "0,1;1,0;");
    }

    #[test]
    fn the_oldest_presets_curve_kept_apart_is_taken() {
        let p = preset(&[
            ("PressureOpacity", "true"),
            ("OpacitySensor", r#"<params id="pressure"/>"#),
            ("CustomOpacity", "true"),
            ("CurveOpacity", "0,0;0.5,1;"),
        ]);
        let o = CurveOption::read(&p, "", "Opacity", true, 0.0, 1.0);
        let v = o.apply(&Info { pressure: 0.5, ..Info::default() }, &mut Still);
        assert!((v - 1.0).abs() < 5e-3, "{v}");
        // not where the sensor keeps a curve of its own
        let own = preset(&[
            ("OpacitySensor", r#"<params id="pressure"><curve>0,0;1,1;</curve></params>"#),
            ("CustomOpacity", "true"),
            ("CurveOpacity", "0,0;0.5,1;"),
        ]);
        assert_eq!(common_curve(&own, "", "Opacity"), "0,0;1,1;");
    }

    #[test]
    fn time_runs_for_its_duration_and_pressure_in_holds_the_most() {
        let p =
            preset(&[("PressureSize", "true"), ("SizeSensor", r#"<params id="time" duration="200" periodic="0"/>"#)]);
        let size = CurveOption::read(&p, "", "Size", true, 0.0, 1.0);
        let v = size.apply(&Info { time: 100.0, ..Info::default() }, &mut Still);
        assert!((v - 0.5).abs() < 1e-3, "{v}");
        let p = preset(&[("PressureSize", "true"), ("SizeSensor", r#"<params id="pressurein"/>"#)]);
        let size = CurveOption::read(&p, "", "Size", true, 0.0, 1.0);
        let v = size.apply(&Info { pressure: 0.2, pressure_in: 0.9, ..Info::default() }, &mut Still);
        assert!((v - 0.9).abs() < 1e-3, "{v}");
    }
}
