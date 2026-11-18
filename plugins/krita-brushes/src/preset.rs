//! A Krita preset's settings: its XML (a `Preset` element of `param`s, each a name and a text
//! value), the pictures it carries itself (`resources`, base64), and the files its settings name.

use std::collections::BTreeMap;

use crate::files::base64;
use crate::xml::{self, Element};

/// A picture a preset holds: its file name, its name, and its bytes as base64 (decoded only when
/// they are wanted).
#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub file: String,
    pub name: String,
    pub base64: String,
}

impl Resource {
    pub fn bytes(&self) -> Option<Vec<u8>> {
        base64(&self.base64)
    }

    /// Its first `n` bytes or so (all of them if it is shorter): enough of a picture to read its
    /// header.
    pub fn head(&self, n: usize) -> Option<Vec<u8>> {
        let end = (n.div_ceil(3) * 4).min(self.base64.len());
        base64(self.base64.get(..end)?)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preset {
    pub name: String,
    /// Which of Krita's paint engines it is for (`paintbrush` the pixel brush).
    pub paintop: String,
    pub params: BTreeMap<String, String>,
    pub resources: Vec<Resource>,
}

impl Preset {
    /// The preset `text` writes (its own pictures with it); none if it isn't one.
    pub fn read(text: &str) -> Option<Preset> {
        let root = xml::parse(text)?;
        if root.name != "Preset" {
            return None;
        }
        let mut params = BTreeMap::new();
        let mut resources = Vec::new();
        for e in root.elements() {
            match e.name.as_str() {
                "param" => {
                    if let Some(name) = e.attribute("name") {
                        params.insert(name.to_owned(), e.text());
                    }
                }
                "resources" => resources.extend(e.elements().filter_map(resource)),
                // a filter brush's filter's own settings, kept apart
                "filterconfig" => {
                    for c in e.elements().flat_map(|c| std::iter::once(c).chain(c.elements())) {
                        if let (true, Some(name)) = (c.name == "param", c.attribute("name")) {
                            params.insert(format!("Filter/config/{name}"), c.text());
                        }
                    }
                }
                _ => {}
            }
        }
        Some(Preset {
            name: root.attribute("name").unwrap_or_default().to_owned(),
            paintop: root.attribute("paintopid").unwrap_or_default().to_owned(),
            params,
            resources,
        })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(String::as_str)
    }

    pub fn flag(&self, key: &str, otherwise: bool) -> bool {
        match self.get(key).map(str::trim) {
            Some("true" | "1") => true,
            Some("false" | "0") => false,
            _ => otherwise,
        }
    }

    pub fn number(&self, key: &str, otherwise: f32) -> f32 {
        self.get(key).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).unwrap_or(otherwise)
    }

    /// A setting holding XML of its own, read.
    pub fn element(&self, key: &str) -> Option<Element> {
        xml::parse(self.get(key)?)
    }

    /// The picture called `file` (by its file name, else by its name), from those it holds.
    pub fn resource(&self, file: &str) -> Option<&Resource> {
        let plain = file.rsplit(['/', '\\']).next().unwrap_or(file);
        self.resources.iter().find(|r| r.file == plain).or_else(|| {
            let stem = plain.rsplit_once('.').map_or(plain, |(s, _)| s);
            self.resources.iter().find(|r| r.name == plain || r.name == stem)
        })
    }

    /// The file names its settings name: its tip's, its masking brush's and its texture's.
    pub fn wanted_files(&self) -> Vec<String> {
        let mut out = Vec::new();
        for key in ["brush_definition", "MaskingBrush/Preset/brush_definition"] {
            if let Some(file) = self.element(key).and_then(|b| b.attribute("filename").map(str::to_owned)) {
                out.push(file);
            }
        }
        if self.flag("Texture/Pattern/Enabled", false) {
            for key in ["Texture/Pattern/PatternFileName", "Texture/Pattern/Name"] {
                if let Some(file) = self.get(key).filter(|f| !f.trim().is_empty()) {
                    out.push(file.trim().to_owned());
                    break;
                }
            }
        }
        out
    }
}

fn resource(e: &Element) -> Option<Resource> {
    if e.name != "resource" {
        return None;
    }
    Some(Resource {
        file: e.attribute("filename").unwrap_or_default().to_owned(),
        name: e.attribute("name").unwrap_or_default().to_owned(),
        base64: e.text().trim().to_owned(),
    })
}
