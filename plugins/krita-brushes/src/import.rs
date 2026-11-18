//! Brushes made of a Krita file, and read again from what was kept of each.
//!
//! A preset (`.kpp`) is a PNG: its picture the preset's preview, its settings XML in a text chunk
//! called `preset`, newer ones carrying the pictures they name. A bundle is a zip of presets
//! (`paintoppresets/`) and pictures (`brushes/`, `patterns/`). What a brush keeps is a record of
//! its settings (field 1) and the pictures it names (field 2 each: its file name, field 1, and
//! bytes, field 2): those the preset carries taken out of its settings and unpacked once, so that a
//! brush read again needn't pass over them (one kept with them in its settings reads as well).
//!
//! A MyPaint brush (a `.myb`, MyPaint's JSON, alone or in a bundle's `paintoppresets/` beside its
//! preview `<name>_prev.png`; or a preset of Krita's MyPaint engine keeping one) keeps that JSON
//! (field 4) instead (field 3 is the brush studio's: the settings as they came).

use crate::brush::Brush;
use crate::files::{Zip, png_picture_only, png_texts};
use crate::preset::Preset;
use crate::rec::{Record, fields};

/// The preset a `.kpp` holds: its XML text.
fn preset_text(kpp: &[u8]) -> Option<String> {
    png_texts(kpp).into_iter().find(|(k, _)| k == "preset").map(|(_, t)| t)
}

/// The brushes made of a file called `name` (`bytes`): a bundle's or a preset's, as the record the
/// app takes; empty for none.
pub fn import(name: &str, bytes: &[u8]) -> Vec<u8> {
    let mut made = Record::new();
    if let Some(zip) = Zip::read(bytes) {
        let names: Vec<String> = zip.names().map(str::to_owned).collect();
        // the bundle's pictures, by their file names, each unpacked once
        let unpacked = std::cell::RefCell::new(std::collections::HashMap::<String, Option<Vec<u8>>>::new());
        let find = |file: &str| -> Option<Vec<u8>> {
            let plain = file.rsplit(['/', '\\']).next().unwrap_or(file);
            let found = names.iter().find(|n| {
                (n.starts_with("brushes/") || n.starts_with("patterns/")) && n.rsplit('/').next() == Some(plain)
            })?;
            unpacked.borrow_mut().entry(found.clone()).or_insert_with(|| zip.file(found)).clone()
        };
        for n in names.iter().filter(|n| n.starts_with("paintoppresets/") && n.ends_with(".kpp")) {
            if let Some(kpp) = zip.file(n) {
                add(&mut made, &kpp, &find);
            }
        }
        // MyPaint's brushes, each beside its preview
        for n in names.iter().filter(|n| n.starts_with("paintoppresets/") && n.to_ascii_lowercase().ends_with(".myb")) {
            let stem = &n[..n.len() - 4];
            let preview = zip.file(&format!("{stem}_prev.png"));
            if let Some(json) = zip.file(n) {
                add_mypaint(&mut made, stem.rsplit('/').next().unwrap_or(stem), &json, preview);
            }
        }
    } else if bytes.starts_with(b"\x89PNG") {
        add(&mut made, bytes, &|_| None);
    } else if bytes.trim_ascii_start().starts_with(b"{") {
        let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let stem = file.split('.').next().unwrap_or(file);
        add_mypaint(&mut made, if stem.is_empty() { "MyPaint" } else { stem }, bytes, None);
    }
    made.bytes()
}

/// A brush made of a MyPaint brush file (`json`) called `name` (as Krita names it: the file's name
/// up to its first dot, its underscores spaces), with its preview if it has one, if it is one.
fn add_mypaint(made: &mut Record, name: &str, json: &[u8], preview: Option<Vec<u8>>) {
    let name = name.replace('_', " ");
    let name = name.as_str();
    let Ok(text) = std::str::from_utf8(json) else { return };
    if crate::mypaint::MyPaint::read(text).is_none() {
        return;
    }
    let mut data = Record::new();
    data.field(4, text.as_bytes());
    let mut brush = Record::new();
    brush.field(1, name.as_bytes()).field(2, &data.bytes());
    if let Some(png) = preview.filter(|p| p.starts_with(b"\x89PNG")) {
        brush.field(3, &png);
    }
    made.field(1, &brush.bytes());
}

/// The MyPaint JSON a preset of Krita's MyPaint engine keeps (as text, or in base64).
fn mypaint_json(preset: &Preset) -> Option<String> {
    let kept = preset.get("MyPaint/json")?.trim();
    if kept.starts_with('{') {
        return Some(kept.to_owned());
    }
    String::from_utf8(crate::files::base64(kept)?).ok()
}

/// A brush made of preset `kpp`, its pictures found in it or by `find`, if it is one this engine
/// draws.
fn add(made: &mut Record, kpp: &[u8], find: &dyn Fn(&str) -> Option<Vec<u8>>) {
    let Some(text) = preset_text(kpp) else { return };
    let Some(preset) = Preset::read(&text) else { return };
    // Krita's MyPaint engine's: the MyPaint brush it keeps
    if preset.paintop == "mypaintbrush" {
        if let Some(json) = mypaint_json(&preset) {
            add_mypaint(made, &preset.name, json.as_bytes(), png_picture_only(kpp));
        }
        return;
    }
    if !crate::brush::ENGINES.contains(&preset.paintop.as_str()) {
        return;
    }
    let mut data = Record::new();
    let (settings, pattern) = without_pattern(&without_resources(&text), &preset);
    data.field(1, settings.as_bytes());
    let mut added = Vec::new();
    if let Some((name, bytes)) = pattern {
        let mut r = Record::new();
        r.field(1, name.as_bytes()).field(2, &bytes);
        data.field(2, &r.bytes());
        added.push(name);
    }
    for file in preset.wanted_files().into_iter().filter(|f| !added.contains(f)) {
        let bytes = match preset.resource(&file) {
            Some(r) => r.bytes(),
            None => find(&file),
        };
        if let Some(bytes) = bytes {
            let mut r = Record::new();
            r.field(1, file.as_bytes()).field(2, &bytes);
            data.field(2, &r.bytes());
        }
    }
    let mut brush = Record::new();
    brush.field(1, preset.name.as_bytes()).field(2, &data.bytes());
    if let Some(png) = png_picture_only(kpp) {
        brush.field(3, &png);
    }
    made.field(1, &brush.bytes());
}

/// A preset's text without the pictures it carries (its `resources`).
fn without_resources(text: &str) -> String {
    let Some(from) = text.find("<resources") else { return text.to_owned() };
    let to = match text[from..].find("</resources>") {
        Some(end) => from + end + "</resources>".len(),
        // an empty one
        None => match text[from..].find("/>") {
            Some(end) => from + end + 2,
            None => return text.to_owned(),
        },
    };
    let mut out = String::with_capacity(text.len() - (to - from));
    out.push_str(&text[..from]);
    out.push_str(&text[to..]);
    out
}

/// A preset's text without the texture's picture its settings carry (older presets carry it in a
/// setting, in base64), and that picture unpacked, by the name its settings give it (a setting
/// naming it put in where none does).
fn without_pattern(text: &str, preset: &Preset) -> (String, Option<(String, Vec<u8>)>) {
    const KEY: &str = "name=\"Texture/Pattern/Pattern\"";
    let carried = preset.get("Texture/Pattern/Pattern").and_then(crate::files::carried_picture);
    let (Some(bytes), Some(at)) = (carried, text.find(KEY)) else { return (text.to_owned(), None) };
    let (Some(from), Some(end)) = (text[..at].rfind("<param"), text[at..].find("</param>")) else {
        return (text.to_owned(), None);
    };
    let to = at + end + "</param>".len();
    let named = ["Texture/Pattern/PatternFileName", "Texture/Pattern/Name"]
        .iter()
        .find_map(|k| preset.get(k).map(str::trim).filter(|n| !n.is_empty()));
    let name = named.unwrap_or("pattern").to_owned();
    let naming = if named.is_none() {
        format!(r#"<param name="Texture/Pattern/PatternFileName" type="string"><![CDATA[{name}]]></param>"#)
    } else {
        String::new()
    };
    (format!("{}{naming}{}", &text[..from], &text[to..]), Some((name, bytes)))
}

/// How much of a picture a preset carries is read when its look isn't made: its header (a pipe's
/// first cell's too).
const LIGHT_HEAD: usize = 4096;

/// The brush kept as `data`, read again ready to draw; `light`: its look not made (the app holds
/// it).
pub fn load(data: &[u8], light: bool) -> Option<Brush> {
    let mut text = None;
    let mut files: Vec<(String, &[u8])> = Vec::new();
    for (id, value) in fields(data) {
        match id {
            4 => {
                let settings = crate::mypaint::MyPaint::read(&String::from_utf8_lossy(value))?;
                return Brush::mypaint(settings, light);
            }
            1 => text = Some(String::from_utf8_lossy(value).into_owned()),
            2 => {
                let (mut name, mut bytes) = (String::new(), &[][..]);
                for (f, v) in fields(value) {
                    match f {
                        1 => name = String::from_utf8_lossy(v).into_owned(),
                        2 => bytes = v,
                        _ => {}
                    }
                }
                files.push((name, bytes));
            }
            _ => {}
        }
    }
    let preset = Preset::read(&text?)?;
    let find = |file: &str| -> Option<Vec<u8>> {
        // its look not made, only the headers of the pictures it carries are read
        if let Some(r) = preset.resource(file) {
            return if light { r.head(LIGHT_HEAD) } else { r.bytes() };
        }
        let plain = file.rsplit(['/', '\\']).next().unwrap_or(file);
        files.iter().find(|(n, _)| n == file || n.rsplit(['/', '\\']).next() == Some(plain)).map(|(_, b)| b.to_vec())
    };
    Brush::read(&preset, &find, light)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Kind;
    use crate::engine::{Begin, Sample, Stroke};
    use std::rc::Rc;

    /// A GIMP brush `w` x `h`, grey `v`.
    fn gbr(w: u32, h: u32, v: u8) -> Vec<u8> {
        let mut b = Vec::new();
        for x in [29, 2, w, h, 1] {
            b.extend_from_slice(&u32::to_be_bytes(x));
        }
        b.extend_from_slice(b"GIMP");
        b.extend_from_slice(&u32::to_be_bytes(10));
        b.push(0);
        b.extend(std::iter::repeat_n(v, (w * h) as usize));
        b
    }

    fn base64_of(bytes: &[u8]) -> String {
        const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for i in 0..4 {
                out.push(if i <= chunk.len() { ABC[((n >> (18 - 6 * i)) & 63) as usize] as char } else { '=' });
            }
        }
        out
    }

    /// An SVG tip is drawn as Krita draws one: 1000 pixels across, black where it paints, at the
    /// preset's scale; and a MyPaint brush file in a bundle comes in by Krita's name for it.
    #[test]
    fn an_svg_tip_and_a_mypaint_brush_come_in() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50"><rect x="0" y="0" width="50" height="50"/></svg>"#;
        let xml = r#"<Preset name="S" paintopid="paintbrush"><param name="brush_definition"><![CDATA[<Brush type="svg_brush" scale="0.5" spacing="0.2" BrushVersion="2" filename="half.svg"/>]]></param></Preset>"#;
        let mut file = Record::new();
        file.field(1, b"half.svg").field(2, svg);
        let mut data = Record::new();
        data.field(1, xml.as_bytes()).field(2, &file.bytes());
        let data = data.bytes();
        let brush = load(&data, false).unwrap();
        assert_eq!((brush.tip.width, brush.tip.height), (500.0, 250.0));
        let m = &brush.tip.masks[0];
        assert_eq!((m.width, m.height), (1000, 500));
        // painting on its left half, nothing on its right
        assert!(m.pixels[(250 * 1000 + 250) as usize] > 250 && m.pixels[(250 * 1000 + 750) as usize] < 5);
        // light: its size only
        assert_eq!(load(&data, true).unwrap().tip.width, 500.0);
        // a bundle's MyPaint brush file, by its name with spaces, drawn by MyPaint's engine
        let myb = br#"{"version": 3, "settings": {"radius_logarithmic": {"base_value": 1.0, "inputs": {}}}}"#;
        let made = import("x.myb", myb);
        let (_, made) = fields(&made).next().unwrap();
        let mut name = String::new();
        let mut kept = Vec::new();
        for (f, v) in fields(made) {
            match f {
                1 => name = String::from_utf8_lossy(v).into_owned(),
                2 => kept = v.to_vec(),
                _ => {}
            }
        }
        assert_eq!(name, "x");
        let brush = load(&kept, true).unwrap();
        assert_eq!(brush.mypaint.as_ref().unwrap().base(crate::mypaint::RADIUS_LOGARITHMIC), 1.0);
        assert_eq!(brush.tip.masks.len(), crate::mypaint::HARDNESSES as usize);
    }

    /// A colour picture tip paints as its preset says Krita applies it, and at twice its scale in a
    /// preset from before tips had their second version.
    #[test]
    fn a_colour_picture_tip_paints_as_its_preset_applies_it() {
        let mut red = Vec::new();
        for x in [29, 2, 2, 1, 4] {
            red.extend_from_slice(&u32::to_be_bytes(x));
        }
        red.extend_from_slice(b"GIMP");
        red.extend_from_slice(&u32::to_be_bytes(10));
        red.push(0);
        red.extend([255, 0, 0, 255].repeat(2));
        let tip = |attributes: &str| {
            let xml = format!(
                r#"<Preset name="P" paintopid="paintbrush"><param name="brush_definition"><![CDATA[<Brush type="gbr_brush" filename="red.gbr" scale="1" {attributes}/>]]></param></Preset>"#
            );
            let mut file = Record::new();
            file.field(1, b"red.gbr").field(2, &red);
            let mut data = Record::new();
            data.field(1, xml.as_bytes()).field(2, &file.bytes());
            let brush = load(&data.bytes(), false).unwrap();
            (brush.tip.kind, brush.tip.masks[0].pixels.clone(), brush.tip.width)
        };
        // red's grey, as Krita weighs it
        let grey = crate::gimp::grey(255, 0, 0);
        let v2 = r#"BrushVersion="2""#;
        assert_eq!(tip(&format!(r#"{v2} brushApplication="1""#)), (Kind::Stamp, [255, 0, 0, 255].repeat(2), 2.0));
        assert_eq!(tip(&format!(r#"{v2} ColorAsMask="1""#)), (Kind::Mask, vec![255 - grey; 2], 2.0));
        assert_eq!(tip(&format!(r#"{v2} ColorAsMask="0""#)).0, Kind::Stamp);
        assert_eq!(
            tip(&format!(r#"{v2} preserveLightness="1""#)),
            (Kind::Lightness, [grey, grey, grey, 255].repeat(2), 2.0)
        );
        assert_eq!(tip(&format!(r#"{v2} brushApplication="3""#)).0, Kind::Gradient);
        // saying nothing, a picture with colour stamps itself; an older preset's is twice its size
        assert_eq!(tip("").0, Kind::Stamp);
        assert_eq!(tip("").2, 4.0);
    }

    /// A colour smudge preset's dabs move paint: the first only marks where the next picks up from,
    /// the rest laid at its smudge rate (1 where it follows no sensor), the colour's share as the
    /// older engine reckons it from the colour rate and the most smudge rate; dulling as it says.
    #[test]
    fn a_colour_smudge_s_dabs_move_paint() {
        let xml = r#"<Preset name="S" paintopid="colorsmudge"><param name="brush_definition"><![CDATA[<Brush type="auto_brush" spacing="0.5" BrushVersion="2"><MaskGenerator type="circle" id="default" diameter="20" ratio="1" hfade="1" vfade="1"/></Brush>]]></param><param name="PressureSmudgeRate"><![CDATA[false]]></param><param name="SmudgeRateValue"><![CDATA[0.6]]></param><param name="PressureColorRate"><![CDATA[true]]></param><param name="ColorRateValue"><![CDATA[1]]></param><param name="ColorRateUseCurve"><![CDATA[false]]></param><param name="SmudgeRateMode"><![CDATA[1]]></param><param name="PressureSmudgeRadius"><![CDATA[true]]></param><param name="SmudgeRadiusValue"><![CDATA[50]]></param><param name="SmudgeRadiusUseCurve"><![CDATA[false]]></param></Preset>"#;
        let mut data = Record::new();
        data.field(1, xml.as_bytes());
        let brush = load(&data.bytes(), false).unwrap();
        let look = brush.look();
        assert!(fields(&look).any(|(f, v)| f == 9 && v == [1]), "its dabs move paint");
        assert!(fields(&look).any(|(f, v)| f == 2 && v == [0]), "each laid as far as it says");
        let mut stroke = Stroke::new(
            Rc::new(brush),
            Begin { colour: [0.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 3, turn: 0.0 },
        );
        let samples: Vec<Sample> =
            (0..40).map(|i| Sample { x: 10.0 + i as f32 * 2.0, y: 50.0, pressure: 1.0, ..Sample::default() }).collect();
        stroke.add(&samples);
        let dabs = &stroke.out;
        assert!(dabs.len() > 4);
        assert_eq!(dabs[0].opacity, 0.0, "the first marks its place");
        assert!(dabs.iter().all(|d| d.flags & 48 == 48), "moving paint, dulled");
        let second = dabs[1];
        assert!((second.opacity - 1.0).abs() < 1e-5, "{}", second.opacity);
        assert!((second.paint_share - 0.4).abs() < 1e-5, "{}", second.paint_share);
        assert!((second.radius - 0.5).abs() < 1e-5, "{}", second.radius);
        // an eraser's colour is clear
        let eraser = xml.replace("</Preset>", r#"<param name="EraserMode"><![CDATA[true]]></param></Preset>"#);
        let mut data = Record::new();
        data.field(1, eraser.as_bytes());
        let mut stroke = Stroke::new(
            Rc::new(load(&data.bytes(), false).unwrap()),
            Begin { colour: [0.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 3, turn: 0.0 },
        );
        stroke.add(&samples);
        assert!(stroke.out.iter().all(|d| d.flags & 176 == 176), "moving paint, dulled, clear");
        // without smearing alpha, what it moves is laid over what is there
        let over = xml.replace("</Preset>", r#"<param name="SmudgeRateSmearAlpha"><![CDATA[false]]></param></Preset>"#);
        let mut data = Record::new();
        data.field(1, over.as_bytes());
        let mut stroke = Stroke::new(
            Rc::new(load(&data.bytes(), false).unwrap()),
            Begin { colour: [0.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 3, turn: 0.0 },
        );
        stroke.add(&samples);
        assert!(stroke.out.iter().all(|d| d.flags & 256 == 256), "laid over");
        // in overlay mode its look says it picks up from every layer
        let overlay = xml.replace("</Preset>", r#"<param name="MergedPaint"><![CDATA[true]]></param></Preset>"#);
        let mut data = Record::new();
        data.field(1, overlay.as_bytes());
        let look = load(&data.bytes(), false).unwrap().look();
        assert!(fields(&look).any(|(f, v)| f == 10 && v == [1]));
    }

    /// A picture tip the preset names but doesn't hold is Krita's default tip, as Krita stands it
    /// in: round, 40 across, spaced automatically.
    #[test]
    fn a_missing_tip_is_krita_s_default() {
        let xml = r#"<Preset name="M" paintopid="paintbrush"><param name="brush_definition"><![CDATA[<Brush type="gbr_brush" filename="gone.gih" scale="1" spacing="2"/>]]></param></Preset>"#;
        let mut data = Record::new();
        data.field(1, xml.as_bytes());
        let brush = load(&data.bytes(), false).unwrap();
        assert_eq!((brush.tip.width, brush.tip.height), (40.0, 40.0));
        assert_eq!(brush.tip.auto_spacing, Some(0.8));
    }

    /// A brush of `engine` with `params`, its tip a round one `diameter` across, drawn along a line.
    fn drawn(engine: &str, params: &str, diameter: f32) -> Vec<crate::engine::Dab> {
        let xml = format!(
            r#"<Preset name="E" paintopid="{engine}"><param name="brush_definition"><![CDATA[<Brush type="auto_brush" spacing="0.2" BrushVersion="2"><MaskGenerator type="circle" id="default" diameter="{diameter}" ratio="1" hfade="1" vfade="1"/></Brush>]]></param>{params}</Preset>"#
        );
        let mut data = Record::new();
        data.field(1, xml.as_bytes());
        let brush = load(&data.bytes(), false).unwrap();
        let mut stroke = Stroke::new(
            Rc::new(brush),
            Begin {
                colour: [1.0, 0.0, 0.0],
                secondary: [1.0; 3],
                diameter: 1.0,
                start_diameter: 1.0,
                seed: 5,
                turn: 0.0,
            },
        );
        let samples: Vec<Sample> = (0..30)
            .map(|i| Sample {
                x: 50.0 + i as f32 * 3.0,
                y: 50.0,
                pressure: 1.0,
                ms: f64::from(i) * 8.0,
                ..Sample::default()
            })
            .collect();
        stroke.add(&samples);
        stroke.out
    }

    /// A brush along a path turning a right angle (right, then down), the canvas turned `turn`.
    fn cornered(params: &str, turn: f32) -> Vec<crate::engine::Dab> {
        let xml = format!(
            r#"<Preset name="E" paintopid="paintbrush"><param name="brush_definition"><![CDATA[<Brush type="auto_brush" spacing="0.5" BrushVersion="2"><MaskGenerator type="rect" id="default" diameter="10" ratio="0.3" hfade="1" vfade="1"/></Brush>]]></param>{params}</Preset>"#
        );
        let mut data = Record::new();
        data.field(1, xml.as_bytes());
        let brush = load(&data.bytes(), false).unwrap();
        let begin = Begin { colour: [1.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 5, turn };
        let mut stroke = Stroke::new(Rc::new(brush), begin);
        let at = |i: i32| if i < 10 { (50.0 + i as f32 * 5.0, 50.0) } else { (95.0, 50.0 + (i - 9) as f32 * 5.0) };
        let samples: Vec<Sample> = (0..20)
            .map(|i| {
                let (x, y) = at(i);
                Sample { x, y, pressure: 1.0, ms: f64::from(i) * 8.0, ..Sample::default() }
            })
            .collect();
        stroke.add(&samples);
        stroke.out
    }

    /// A dab turned by the way the stroke goes takes that way from where the last dab lay; with fan
    /// corners, dabs at each of their steps fill a corner (Krita's drawing angle and its fans).
    #[test]
    fn a_drawing_angle_fans_round_a_corner_when_it_says() {
        let rotation = |fan: &str| {
            format!(
                r#"<param name="PressureRotation"><![CDATA[true]]></param><param name="RotationSensor"><![CDATA[<params id="drawingangle" fanCornersEnabled="{fan}" fanCornersStep="30" angleOffset="0" lockedAngleMode="0"/>]]></param>"#
            )
        };
        // the dabs turned neither along nor across
        let between = |dabs: &[crate::engine::Dab]| {
            let ways = dabs.iter().map(|d| d.angle.to_degrees().rem_euclid(180.0));
            ways.filter(|a| *a > 10.0 && *a < 80.0).count()
        };
        let plain = cornered(&rotation("0"), 0.0);
        let fanned = cornered(&rotation("1"), 0.0);
        // 0 to 90 degrees in steps of 30: two dabs between
        assert_eq!(between(&plain), 0);
        assert_eq!(between(&fanned), 2);
        assert_eq!(fanned.len(), plain.len() + 2);
    }

    /// A dab its rotation option leaves alone turns with the canvas, so it looks the same on screen.
    #[test]
    fn an_unturned_dab_turns_with_the_canvas() {
        let dabs = cornered("", 0.5);
        assert!(dabs.iter().all(|d| (d.angle + 0.5).abs() < 1e-5), "{}", dabs[0].angle);
    }

    /// At precision 1 a dab near enough the last drawn afresh is that one laid again, at the
    /// whole pixel its box starts on: along a steady stroke, every dab at the same place within a
    /// pixel; at 5, each where it falls.
    #[test]
    fn a_low_precision_lays_the_kept_dab_again() {
        let within = |level: &str| {
            let params = format!(r#"<param name="KisPrecisionOption/precisionLevel"><![CDATA[{level}]]></param>"#);
            let dabs = drawn("paintbrush", &params, 21.0);
            let mut places: Vec<i32> =
                dabs.iter().map(|d| ((d.x - 10.5).rem_euclid(1.0) * 1000.0).round() as i32).collect();
            places.sort_unstable();
            places.dedup();
            (dabs.len(), places.len())
        };
        let (kept, one) = within("1");
        let (fresh, many) = within("5");
        assert_eq!(kept, fresh);
        assert_eq!(one, 1);
        assert!(many > 1, "{many}");
    }

    /// Spacing measures the tip as Krita does: its box turned by its own angle, not squashed by the
    /// ratio option.
    #[test]
    fn spacing_measures_the_tip_turned_and_unsquashed() {
        let down = |tip: &str, params: &str| {
            let xml = format!(
                r#"<Preset name="E" paintopid="paintbrush"><param name="brush_definition"><![CDATA[<Brush type="auto_brush" spacing="0.5" BrushVersion="2" {tip}><MaskGenerator type="rect" id="default" diameter="20" ratio="0.25" hfade="1" vfade="1"/></Brush>]]></param>{params}</Preset>"#
            );
            let mut data = Record::new();
            data.field(1, xml.as_bytes());
            let brush = load(&data.bytes(), false).unwrap();
            let begin =
                Begin { colour: [1.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 5, turn: 0.0 };
            let mut stroke = Stroke::new(Rc::new(brush), begin);
            let samples: Vec<Sample> = (0..=20)
                .map(|i| Sample {
                    x: 50.0,
                    y: 50.0 + i as f32 * 5.0,
                    pressure: 1.0,
                    ms: f64::from(i) * 8.0,
                    ..Sample::default()
                })
                .collect();
            stroke.add(&samples);
            stroke.out.len()
        };
        // 100 down: a dab every 2.5 (half of 20 x 0.25), or every 10 with the tip turned upright
        let flat = down("", "");
        let upright = down(r#"angle="1.5707964""#, "");
        assert!((38..=42).contains(&flat), "{flat}");
        assert!((10..=12).contains(&upright), "{upright}");
        // the ratio option squashes the dab, not its spacing
        let squashed = down(
            r#"angle="1.5707964""#,
            r#"<param name="PressureRatio"><![CDATA[true]]></param><param name="RatioUseCurve"><![CDATA[false]]></param><param name="RatioValue"><![CDATA[0.25]]></param>"#,
        );
        assert_eq!(squashed, upright);
    }

    /// Krita's deform and filter brushes lay the layer's own pixels: moved about under each dab, or
    /// blurred; the clone brush from where a tap set its source, as far from where it lies as its
    /// stroke started from the source.
    #[test]
    fn deform_filter_and_clone_dabs_pick_up_the_layer() {
        let swirl = r#"<param name="Deform/deformAction"><![CDATA[3]]></param><param name="Deform/deformAmount"><![CDATA[0.5]]></param><param name="Brush/diameter"><![CDATA[30]]></param>"#;
        let dabs = drawn("deformbrush", swirl, 20.0);
        assert!(!dabs.is_empty());
        assert!(dabs.iter().all(|d| d.flags & (16 | 512 | 2048) == 16 | 512 | 2048 && d.kind == 2 + 16));
        assert!((dabs[0].extra[2] - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        // (each dab as much smaller as the app lays its tip's picture past it)
        assert!((dabs[0].diameter + std::f32::consts::SQRT_2 - 30.0).abs() < 0.5, "{}", dabs[0].diameter);
        let blur = r#"<param name="Filter/id"><![CDATA[gaussian blur]]></param><filterconfig version="1"><param name="horizRadius"><![CDATA[5]]></param><param name="vertRadius"><![CDATA[5]]></param></filterconfig>"#;
        let dabs = drawn("filter", blur, 20.0);
        assert!(!dabs.is_empty() && dabs.iter().all(|d| d.flags & (16 | 512 | 4096 | 8192) == 16 | 512 | 4096 | 8192));
        assert!(dabs[0].extra[2] > 0.1 && dabs[0].extra[2] < 0.5, "{}", dabs[0].extra[2]);
        // a tap at (10, 50) sets the source; a stroke from (50, 50) picks up 40 to its left
        let clone = |samples: &[(f32, f32)]| {
            let xml = r#"<Preset name="C" paintopid="duplicate"><param name="brush_definition"><![CDATA[<Brush type="auto_brush" spacing="0.2" BrushVersion="2"><MaskGenerator type="circle" id="default" diameter="10" ratio="1" hfade="1" vfade="1"/></Brush>]]></param></Preset>"#;
            let mut data = Record::new();
            data.field(1, xml.as_bytes());
            let begin =
                Begin { colour: [1.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 5, turn: 0.0 };
            let mut stroke = Stroke::new(Rc::new(load(&data.bytes(), false).unwrap()), begin);
            let samples: Vec<Sample> = samples
                .iter()
                .enumerate()
                .map(|(i, &(x, y))| Sample { x, y, pressure: 1.0, ms: i as f64 * 8.0, ..Sample::default() })
                .collect();
            stroke.add(&samples);
            stroke.finish();
            stroke.out
        };
        assert!(clone(&[(10.0, 50.0)]).is_empty());
        let dabs = clone(&(0..10).map(|i| (50.0 + i as f32 * 3.0, 50.0)).collect::<Vec<_>>());
        assert!(!dabs.is_empty());
        assert!(
            dabs.iter().all(|d| d.flags & (1024 | 8192 | 256) == 1024 | 8192 | 256 && (d.extra[0] + 40.0).abs() < 1e-3)
        );
    }

    /// Krita's shape brush fills the outline its stroke makes as the stroke ends: a square loop's
    /// inside, row by row.
    #[test]
    fn a_shape_brush_fills_its_outline() {
        let xml = r#"<Preset name="S" paintopid="experimentbrush"><param name="Experiment/windingFill"><![CDATA[true]]></param></Preset>"#;
        let mut data = Record::new();
        data.field(1, xml.as_bytes());
        let begin =
            Begin { colour: [1.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 5, turn: 0.0 };
        let mut stroke = Stroke::new(Rc::new(load(&data.bytes(), false).unwrap()), begin);
        let corners = [(10.0, 10.0), (50.0, 10.0), (50.0, 50.0), (10.0, 50.0), (10.0, 12.0)];
        let samples: Vec<Sample> = corners
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| Sample { x, y, pressure: 1.0, ms: i as f64 * 8.0, ..Sample::default() })
            .collect();
        stroke.add(&samples);
        assert!(stroke.out.is_empty(), "nothing until it ends");
        stroke.finish();
        let rows: std::collections::BTreeSet<i32> = stroke.out.iter().map(|d| d.y.floor() as i32).collect();
        assert_eq!(rows.len(), 40, "{rows:?}");
        let covered: f32 = stroke.out.iter().map(|d| d.diameter).sum();
        assert!((covered - 40.0 * 40.0).abs() < 60.0, "{covered}");
    }

    /// Krita's composite ops go to the app's blend modes by the app's own numbers (Colour Burn 3 on,
    /// with no Shade among them).
    #[test]
    fn composite_ops_are_the_app_s_blend_numbers() {
        let n = crate::brush::blend_number;
        assert_eq!([n("darken"), n("burn"), n("add"), n("overlay"), n("subtract")], [2, 3, 9, 11, 20].map(Some));
        assert_eq!([n("color"), n("luminize"), n("behind"), n("erase")], [Some(24), Some(25), Some(29), None]);
    }

    /// Krita's quick brush lays hard round dabs of its own diameter, close together, each over the
    /// last.
    #[test]
    fn a_quick_brush_lays_hard_round_dabs() {
        let dabs = drawn(
            "roundmarker",
            r#"<param name="diameter"><![CDATA[12]]></param><param name="spacing"><![CDATA[0.1]]></param>"#,
            40.0,
        );
        assert!(dabs.len() > 30);
        assert!(dabs.iter().all(|d| (d.diameter + std::f32::consts::SQRT_2 - 12.0).abs() < 0.5 && d.opacity == 1.0));
        // the preset's opacity whatever the pressure: the quick brush has no opacity option
        let xml = r#"<Preset name="Q" paintopid="roundmarker"><param name="diameter"><![CDATA[12]]></param><param name="OpacityValue"><![CDATA[1]]></param></Preset>"#;
        let mut data = Record::new();
        data.field(1, xml.as_bytes());
        let mut stroke = Stroke::new(
            Rc::new(load(&data.bytes(), false).unwrap()),
            Begin {
                colour: [1.0, 0.0, 0.0],
                secondary: [1.0; 3],
                diameter: 1.0,
                start_diameter: 1.0,
                seed: 5,
                turn: 0.0,
            },
        );
        let light: Vec<Sample> = (0..30)
            .map(|i| Sample {
                x: 50.0 + i as f32 * 3.0,
                y: 50.0,
                pressure: 0.3,
                ms: f64::from(i) * 8.0,
                ..Sample::default()
            })
            .collect();
        stroke.add(&light);
        assert!(!stroke.out.is_empty() && stroke.out.iter().all(|d| d.opacity == 1.0 && d.flow == 1.0));
    }

    /// Krita's tangent normal brush paints the pen's lean as a normal: an upright pen the normal
    /// straight up (blue).
    #[test]
    fn a_tangent_normal_brush_paints_the_pen_s_lean() {
        let dabs = drawn("tangentnormal", "", 20.0);
        let c = dabs[0].colour;
        assert!((c[0] - 0.5).abs() < 0.02 && (c[1] - 0.5).abs() < 0.02 && (c[2] - 1.0).abs() < 0.02, "{c:?}");
    }

    /// Krita's sketch brush lays lines: from each move's start to its end, and to points it passed
    /// within its tip's reach.
    #[test]
    fn a_sketch_brush_lays_lines() {
        let lines = r#"<param name="Sketch/probability"><![CDATA[1]]></param><param name="Sketch/distanceDensity"><![CDATA[false]]></param>"#;
        let dabs = drawn("sketchbrush", lines, 40.0);
        // a line for each move at the least, and more back to the points it passed
        assert!(dabs.len() > 40, "{}", dabs.len());
        // (lines a pixel thin with the thin line tips)
        assert!(dabs.iter().all(|d| matches!(d.tip, 1 | 3 | 4) && d.ratio <= 1.0));
        assert!(dabs.iter().any(|d| d.diameter > 6.0), "lines longer than a move");
    }

    /// Krita's spray brush lays its particles at each of its dabs: as many as its count, within
    /// its diameter of where it lies.
    #[test]
    fn a_spray_brush_lays_its_particles() {
        let spray = r#"<param name="Spray/diameter"><![CDATA[40]]></param><param name="Spray/particleCount"><![CDATA[10]]></param><param name="Spray/spacing"><![CDATA[1]]></param><param name="SprayShape/width"><![CDATA[4]]></param><param name="SprayShape/height"><![CDATA[4]]></param>"#;
        let dabs = drawn("spraybrush", spray, 20.0);
        assert!(dabs.len() >= 20 && dabs.len().is_multiple_of(10), "{}", dabs.len());
        assert!(dabs.iter().all(|d| (d.y - 50.0).abs() <= 20.5 && d.diameter >= 1.0 && d.diameter <= 4.5));
        // its shape option off, its particles are its own tip, 20 across
        let own = format!("{spray}{}", r#"<param name="SprayShape/enabled"><![CDATA[false]]></param>"#);
        let dabs = drawn("spraybrush", &own, 20.0);
        assert!(dabs.iter().all(|d| d.tip == dabs[0].tip && (d.diameter - 20.0).abs() < 0.5), "{:?}", dabs[0].diameter);
    }

    /// Krita's bristle brush: each bristle of its tip a line at each move, a pixel wide, spread by its
    /// scale, as opaque as the tip is there.
    #[test]
    fn a_bristle_brush_draws_a_line_for_each_bristle_at_each_move() {
        let bristles = r#"<param name="HairyBristle/scale"><![CDATA[2]]></param><param name="HairyBristle/random"><![CDATA[0]]></param><param name="HairyBristle/isConnected"><![CDATA[true]]></param>"#;
        let dabs = drawn("hairybrush", bristles, 6.0);
        // a 6-across round tip has a few dozen bristles; 29 moves after the first point
        assert!(!dabs.is_empty() && dabs.len().is_multiple_of(29), "{}", dabs.len());
        let per_move = dabs.len() / 29;
        assert!((20..=40).contains(&per_move), "{per_move}");
        assert!(dabs.iter().all(|d| matches!(d.tip, 3 | 4) && d.ratio <= 1.0 && d.opacity <= 1.0));
        // spread by the scale: twice the tip's reach from the path (each half a pixel on, as Krita
        // lays a bristle's points)
        assert!(dabs.iter().all(|d| (d.y - 50.5).abs() <= 7.0), "within reach");
        assert!(dabs.iter().any(|d| (d.y - 50.5).abs() >= 3.5), "spread");
    }

    /// Krita 3's chalk brush: its speckled round tip at every pixel; its dyna brush: circles a
    /// weight on a spring lays as the pen pulls it.
    #[test]
    fn a_chalk_and_a_dyna_brush_lay_as_krita_3_laid_them() {
        let chalk = drawn("chalkbrush", r#"<param name="Chalk/radius"><![CDATA[3]]></param>"#, 10.0);
        // a dab at every pixel of the path (30 samples 3 apart), each 7 across
        assert!(chalk.len() >= 80, "{}", chalk.len());
        assert!(chalk.iter().all(|d| (d.diameter + std::f32::consts::SQRT_2 - 7.0).abs() < 0.01));
        let dots = r#"<param name="Dyna/action"><![CDATA[0]]></param><param name="Dyna/diameter"><![CDATA[3]]></param><param name="Dyna/mass"><![CDATA[0]]></param><param name="Dyna/drag"><![CDATA[0.65]]></param><param name="Dyna/width"><![CDATA[0.1]]></param>"#;
        let dyna = drawn("dynabrush", dots, 10.0);
        // a circle for each move after the first two (the weight starting where the pen first was)
        assert!(
            dyna.len() >= 20 && dyna.iter().all(|d| d.tip == 0 && d.diameter >= 2.0 && d.diameter <= 8.0),
            "{}",
            dyna.len()
        );
        // following the pen, behind it
        assert!(dyna.iter().all(|d| (d.y - 50.0).abs() < 1.0 && d.x >= 50.0 && d.x <= 137.0));
    }

    /// A bristle brush's ink runs out pixel by pixel along its lines, and soaking it up its bristles
    /// take the colours where the stroke starts.
    #[test]
    fn a_bristle_brush_s_ink_runs_out_and_soaks_up_the_colours_under_it() {
        let ink = r#"<param name="HairyBristle/scale"><![CDATA[1]]></param><param name="HairyBristle/random"><![CDATA[0]]></param><param name="HairyBristle/isConnected"><![CDATA[true]]></param><param name="HairyInk/enabled"><![CDATA[true]]></param><param name="HairyInk/inkAmount"><![CDATA[40]]></param><param name="HairyInk/soak"><![CDATA[true]]></param>"#;
        let xml = format!(
            r#"<Preset name="E" paintopid="hairybrush"><param name="brush_definition"><![CDATA[<Brush type="auto_brush" spacing="0.2" BrushVersion="2"><MaskGenerator type="circle" id="default" diameter="4" ratio="1" hfade="1" vfade="1"/></Brush>]]></param>{ink}</Preset>"#
        );
        let mut data = Record::new();
        data.field(1, xml.as_bytes());
        let brush = load(&data.bytes(), false).unwrap();
        // it wants the colours where its strokes start
        let side =
            fields(&brush.look()).find(|(f, _)| *f == 11).and_then(|(_, v)| v.try_into().ok()).map(u32::from_le_bytes);
        assert!(side.is_some_and(|s| s >= 4), "{side:?}");
        let mut stroke = Stroke::new(
            Rc::new(brush),
            Begin {
                colour: [1.0, 0.0, 0.0],
                secondary: [1.0; 3],
                diameter: 1.0,
                start_diameter: 1.0,
                seed: 5,
                turn: 0.0,
            },
        );
        // green under where it starts
        let side = 16_u32;
        let mut bytes = Vec::new();
        for v in [42_u32, 42, side] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend((0..side * side).flat_map(|_| [0_u8, 255, 0, 255]));
        stroke.under = crate::engine::Under::read(&bytes);
        let samples: Vec<Sample> =
            (0..30).map(|i| Sample { x: 50.0 + i as f32 * 3.0, y: 50.0, pressure: 1.0, ..Sample::default() }).collect();
        stroke.add(&samples);
        let dabs = &stroke.out;
        assert!(dabs.iter().all(|d| d.colour == [0.0, 1.0, 0.0] && d.flags & 4 != 0), "soaked up green");
        // running out: the last lines fainter than the first, in steps a 64th apart at most
        let first = dabs.iter().take(8).map(|d| d.opacity).fold(0.0, f32::max);
        let last = dabs.iter().rev().take(8).map(|d| d.opacity).fold(0.0, f32::max);
        assert!(last < first * 0.5, "{first} {last}");
    }

    /// Krita's hatching brush: inside each dab of its tip, the lines of a hatching anchored to the
    /// canvas, a separation apart, each kept inside the dab's tip.
    #[test]
    fn a_hatching_brush_lays_canvas_lines_inside_its_tip() {
        let hatching = r#"<param name="Hatching/angle"><![CDATA[0]]></param><param name="Hatching/separation"><![CDATA[4]]></param><param name="Hatching/thickness"><![CDATA[1]]></param><param name="Hatching/origin_x"><![CDATA[0]]></param><param name="Hatching/origin_y"><![CDATA[0]]></param><param name="Hatching/bool_nocrosshatching"><![CDATA[true]]></param>"#;
        let dabs = drawn("hatchingbrush", hatching, 20.0);
        assert!(!dabs.is_empty() && dabs.iter().all(|d| d.flags & 8 == 0), "no masking stroke");
        // level lines on the canvas's rows a separation apart
        assert!(dabs.iter().all(|d| d.angle.abs() < 1e-3 && d.tip == 3), "level");
        assert!(dabs.iter().all(|d| (d.y.rem_euclid(4.0)).min(4.0 - d.y.rem_euclid(4.0)) < 0.01), "on the rows");
        assert!(dabs.iter().any(|d| (d.y - 44.0).abs() < 0.01) && dabs.iter().any(|d| (d.y - 56.0).abs() < 0.01));
        // one dab's lines: inside its round tip, 20 across, not out in its box's corners
        let xml = format!(
            r#"<Preset name="E" paintopid="hatchingbrush"><param name="brush_definition"><![CDATA[<Brush type="auto_brush" spacing="0.2" BrushVersion="2"><MaskGenerator type="circle" id="default" diameter="20" ratio="1" hfade="1" vfade="1"/></Brush>]]></param>{hatching}</Preset>"#
        );
        let mut data = Record::new();
        data.field(1, xml.as_bytes());
        let mut stroke = Stroke::new(
            Rc::new(load(&data.bytes(), false).unwrap()),
            Begin {
                colour: [1.0, 0.0, 0.0],
                secondary: [1.0; 3],
                diameter: 1.0,
                start_diameter: 1.0,
                seed: 5,
                turn: 0.0,
            },
        );
        stroke.add(&[Sample { x: 50.0, y: 50.0, pressure: 1.0, ..Sample::default() }]);
        assert!(!stroke.out.is_empty());
        assert!(stroke.out.iter().all(|d| (d.x - 50.0).hypot(d.y - 50.0) <= 11.0), "inside the tip");
    }

    /// Krita's curve brush: a line for each move, and once it has passed as many points as it
    /// keeps, a curve through them at each move.
    #[test]
    fn a_curve_brush_draws_lines_and_curves_through_its_points() {
        let curves = r#"<param name="Curve/strokeHistorySize"><![CDATA[6]]></param><param name="Curve/makeConnection"><![CDATA[true]]></param><param name="Curve/curvesOpacity"><![CDATA[0.5]]></param><param name="Curve/lineWidth"><![CDATA[2]]></param>"#;
        let dabs = drawn("curvebrush", curves, 10.0);
        // 29 lines, and 24 pieces of a curve from the sixth point on
        assert_eq!(dabs.len(), 29 + (29 - 5) * 24);
        assert!(dabs.iter().all(|d| d.tip == 1));
        assert!(dabs.iter().any(|d| (d.opacity - 0.5).abs() < 1e-4), "the curves at their opacity");
    }

    /// Krita's particle brush: its particles pulled towards the pen, a dot for each at each step.
    #[test]
    fn a_particle_brush_lays_a_dot_for_each_particle_at_each_step() {
        let particles = r#"<param name="Particle/count"><![CDATA[5]]></param><param name="Particle/iterations"><![CDATA[3]]></param><param name="Particle/weight"><![CDATA[0.5]]></param>"#;
        let dabs = drawn("particlebrush", particles, 10.0);
        assert_eq!(dabs.len(), 30 * 5 * 3);
        assert!(dabs.iter().all(|d| d.diameter == 1.0 && (d.opacity - 0.5).abs() < 1e-4));
        // they set out from the first point
        assert!(dabs.iter().take(15).all(|d| (d.x - 50.0).abs() < 1.0 && (d.y - 50.0).abs() < 1.0));
    }

    /// Krita's grid brush: the cells of a grid anchored to the canvas, each its shape; or each
    /// picking up the colour under its middle.
    #[test]
    fn a_grid_brush_lays_the_canvas_grid_s_cells() {
        let grid = r#"<param name="Grid/gridWidth"><![CDATA[10]]></param><param name="Grid/gridHeight"><![CDATA[10]]></param><param name="Grid/diameter"><![CDATA[20]]></param><param name="Grid/scale"><![CDATA[1]]></param><param name="GridShape/shape"><![CDATA[1]]></param>"#;
        let dabs = drawn("gridbrush", grid, 10.0);
        assert!(!dabs.is_empty());
        // boxes 10 across, their middles on the grid's
        assert!(dabs.iter().all(|d| (d.diameter - 10.0).abs() < 1e-3 && d.tip == 1), "boxes");
        assert!(
            dabs.iter().all(|d| (d.x - 5.0).rem_euclid(10.0) < 1e-3 && (d.y - 5.0).rem_euclid(10.0) < 1e-3),
            "on the grid"
        );
        let sampled = format!("{grid}{}", r#"<param name="ColorOption/sampleInputColor"><![CDATA[true]]></param>"#);
        let dabs = drawn("gridbrush", &sampled, 10.0);
        assert!(dabs.iter().all(|d| d.flags & (16 | 32 | 512) == 16 | 32 | 512), "picking up under them");
    }

    /// A texture's picture carried in a setting comes out of it as a file of the brush's, by its name.
    #[test]
    fn a_texture_carried_in_a_setting_leaves_it_unpacked() {
        let text = r#"<Preset name="P" paintopid="paintbrush"><param name="Texture/Pattern/Enabled"><![CDATA[true]]></param><param name="Texture/Pattern/Pattern" type="bytearray"><![CDATA[aGVsbG8=]]></param><param name="Texture/Pattern/Name"><![CDATA[paper]]></param></Preset>"#;
        let preset = Preset::read(text).unwrap();
        let (settings, pattern) = without_pattern(text, &preset);
        assert!(!settings.contains("aGVsbG8") && settings.contains("Texture/Pattern/Name"));
        assert_eq!(pattern, Some(("paper".to_owned(), b"hello".to_vec())));
        let again = Preset::read(&settings).unwrap();
        assert_eq!(again.get("Texture/Pattern/Pattern"), None);
        assert_eq!(again.wanted_files(), ["paper"]);
    }

    /// The pictures a preset carries come out of its settings as files of the brush's, unpacked.
    #[test]
    fn a_preset_s_pictures_leave_its_settings_unpacked() {
        let xml = r#"<Preset name="P" paintopid="paintbrush"><resources><resource name="t" filename="t.png"><![CDATA[aGVsbG8=]]></resource></resources><param name="brush_definition"><![CDATA[<Brush type="png_brush" filename="t.png"/>]]></param></Preset>"#;
        let mut kpp = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut text = b"preset\0".to_vec();
        text.extend_from_slice(xml.as_bytes());
        kpp.extend_from_slice(&(text.len() as u32).to_be_bytes());
        kpp.extend_from_slice(b"tEXt");
        kpp.extend_from_slice(&text);
        kpp.extend_from_slice(&[0; 4]);
        let made = import("p.kpp", &kpp);
        let (_, brush) = fields(&made).next().unwrap();
        let (_, data) = fields(brush).find(|(f, _)| *f == 2).unwrap();
        let settings =
            fields(data).find(|(f, _)| *f == 1).map(|(_, v)| String::from_utf8_lossy(v).into_owned()).unwrap();
        assert!(!settings.contains("resource") && settings.contains("brush_definition"));
        let (_, file) = fields(data).find(|(f, _)| *f == 2).unwrap();
        let file: Vec<(u16, &[u8])> = fields(file).collect();
        assert_eq!(file, [(1, &b"t.png"[..]), (2, &b"hello"[..])]);
    }

    /// Read only to draw (the app holding its look), a brush lays the same dabs as read whole: a pipe
    /// it carries, larger than the part of it read, picks among as many cells.
    #[test]
    fn a_brush_read_light_lays_the_dabs_it_does_read_whole() {
        let mut pipe = b"cells\n40 ncells:40 dim:1 rank0:40 sel0:random\n".to_vec();
        for i in 0..40 {
            pipe.extend(gbr(30, 20, i as u8));
        }
        let xml = format!(
            r#"<Preset name="Pipe" paintopid="paintbrush"><resources><resource name="cells" filename="cells.gih"><![CDATA[{}]]></resource></resources><param name="brush_definition" type="string"><![CDATA[<Brush type="gbr_brush" filename="cells.gih" spacing="0.5" scale="1" angle="0"/>]]></param><param name="Texture/Pattern/Enabled" type="string"><![CDATA[true]]></param><param name="Texture/Pattern/PatternFileName" type="string"><![CDATA[cells.gih]]></param></Preset>"#,
            base64_of(&pipe)
        );
        let mut data = Record::new();
        data.field(1, xml.as_bytes());
        let data = data.bytes();
        let draw = |light: bool| {
            let brush = load(&data, light).unwrap();
            assert_eq!(brush.tip.masks.len(), 40);
            assert_eq!(brush.tip.masks.iter().all(|m| m.pixels.is_empty()), light);
            let mut stroke = Stroke::new(
                Rc::new(brush),
                Begin { colour: [0.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 3, turn: 0.0 },
            );
            let samples: Vec<Sample> = (0..60)
                .map(|i| Sample {
                    x: 10.0 + i as f32 * 4.0,
                    y: 50.0,
                    pressure: 0.8,
                    ms: i as f64 * 8.0,
                    ..Sample::default()
                })
                .collect();
            stroke.add(&samples);
            let mut out = Vec::new();
            stroke.out.iter().for_each(|d| d.write(&mut out));
            out
        };
        let whole = draw(false);
        assert!(whole.len() > 64 * 10);
        assert_eq!(draw(true), whole);
    }

    /// A Krita file's brushes made, read again and each drawn along a wave, with how long it took:
    /// `KRITA_FILE=x.bundle cargo test --release real_file -- --ignored --nocapture` (`KRITA_SHOW=name`
    /// lists the first dabs of the brushes whose names hold it).
    #[test]
    #[ignore]
    fn a_real_file_s_brushes_draw() {
        let Ok(path) = std::env::var("KRITA_FILE") else { return };
        let bytes = std::fs::read(&path).unwrap();
        let t = std::time::Instant::now();
        let made = import(&path, &bytes);
        let brushes: Vec<(String, Vec<u8>)> = fields(&made)
            .map(|(_, b)| {
                let mut name = String::new();
                let mut data = Vec::new();
                for (f, v) in fields(b) {
                    if f == 1 {
                        name = String::from_utf8_lossy(v).into_owned();
                    }
                    if f == 2 {
                        data = v.to_vec();
                    }
                }
                (name, data)
            })
            .collect();
        println!("{} brushes made in {:?}", brushes.len(), t.elapsed());
        for (name, data) in brushes {
            let t = std::time::Instant::now();
            let Some(brush) = load(&data, false) else {
                println!("  {name}: not read");
                continue;
            };
            let look = brush.look().len();
            let loaded = t.elapsed();
            let mut stroke = Stroke::new(
                Rc::new(brush),
                Begin { colour: [0.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 7, turn: 0.0 },
            );
            let samples: Vec<Sample> = (0..120)
                .map(|i| Sample {
                    x: 100.0 + i as f32 * 7.0,
                    y: 300.0 + (i as f32 * 0.05).sin() * 40.0,
                    pressure: (i as f32 / 120.0 * std::f32::consts::PI).sin().max(0.05),
                    ms: i as f64 * 8.0,
                    ..Sample::default()
                })
                .collect();
            let t2 = std::time::Instant::now();
            stroke.add(&samples);
            println!("  {name}: look {look} B in {loaded:?}, {} dabs in {:?}", stroke.out.len(), t2.elapsed());
            if std::env::var("KRITA_SHOW").is_ok_and(|w| name.contains(&w)) {
                for d in stroke.out.iter().take(12) {
                    println!(
                        "    {:.1} {:.1} d={:.2} o={:.3} f={:.3} tip={} flags={}",
                        d.x, d.y, d.diameter, d.opacity, d.flow, d.tip, d.flags
                    );
                }
            }
        }
    }
}
