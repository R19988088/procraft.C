//! A small XML reader, enough for Krita's presets: elements, their attributes, text and CDATA,
//! comments and declarations passed over, the five named entities and numbered ones.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// An attribute as a number.
    pub fn number(&self, name: &str) -> Option<f32> {
        self.attribute(name)?.trim().parse::<f32>().ok().filter(|v| v.is_finite())
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// The first child element called `name`.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.children.iter_mut().find_map(|n| match n {
            Node::Element(e) if e.name == name => Some(e),
            _ => None,
        })
    }

    /// Attribute `name` set to `value`, added if it isn't there.
    pub fn set_attribute(&mut self, name: &str, value: &str) {
        match self.attributes.iter_mut().find(|(n, _)| n == name) {
            Some((_, v)) => *v = value.to_owned(),
            None => self.attributes.push((name.to_owned(), value.to_owned())),
        }
    }

    /// It written as XML again.
    pub fn to_xml(&self) -> String {
        let escape = |t: &str| t.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;");
        let mut out = format!("<{}", self.name);
        for (n, v) in &self.attributes {
            out.push_str(&format!(" {n}=\"{}\"", escape(v)));
        }
        if self.children.is_empty() {
            out.push_str("/>");
            return out;
        }
        out.push('>');
        for c in &self.children {
            match c {
                Node::Element(e) => out.push_str(&e.to_xml()),
                Node::Text(t) => out.push_str(&escape(t)),
            }
        }
        out.push_str(&format!("</{}>", self.name));
        out
    }

    /// All its text, CDATA included.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for n in &self.children {
            match n {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => out.push_str(&e.text()),
            }
        }
        out
    }
}

/// The document's root element, if it holds up.
pub fn parse(text: &str) -> Option<Element> {
    let mut reader = Reader { t: text, s: text.as_bytes(), at: 0, depth: 0 };
    loop {
        reader.skip_space();
        if reader.starts("<?") {
            reader.skip_past("?>")?;
        } else if reader.starts("<!--") {
            reader.skip_past("-->")?;
        } else if reader.starts("<!") {
            reader.skip_declaration()?;
        } else if reader.starts("<") {
            return reader.element();
        } else {
            return None;
        }
    }
}

/// Where it has read to in the text: always just before or after a mark (`<`, `>`, a quote, white
/// space), so slicing the text there is slicing its characters.
struct Reader<'a> {
    t: &'a str,
    s: &'a [u8],
    at: usize,
    depth: usize,
}

const MOST_DEPTH: usize = 64;

impl Reader<'_> {
    fn starts(&self, what: &str) -> bool {
        self.s.get(self.at..).is_some_and(|rest| rest.starts_with(what.as_bytes()))
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    /// Where the next `mark` is (a search many bytes at a time: a long text, a picture in base64
    /// say, is passed over quickly).
    fn next(&self, from: usize, mark: char) -> Option<usize> {
        Some(from + self.t.get(from..)?.find(mark)?)
    }

    /// On past the next `end`.
    fn skip_past(&mut self, end: &str) -> Option<()> {
        let first = end.chars().next()?;
        let mut from = self.at;
        loop {
            let found = self.next(from, first)?;
            if self.s.get(found..)?.starts_with(end.as_bytes()) {
                self.at = found + end.len();
                return Some(());
            }
            from = found + first.len_utf8();
        }
    }

    /// The text from `from` to where it has read to.
    fn read_since(&self, from: usize) -> Option<&str> {
        self.t.get(from..self.at)
    }

    /// A `<!DOCTYPE ...>`, brackets inside it counted.
    fn skip_declaration(&mut self) -> Option<()> {
        let mut nested = 0_i32;
        while let Some(c) = self.peek() {
            self.at += 1;
            match c {
                b'[' => nested += 1,
                b']' => nested -= 1,
                b'>' if nested <= 0 => return Some(()),
                _ => {}
            }
        }
        None
    }

    fn name(&mut self) -> Option<String> {
        let from = self.at;
        while self.peek().is_some_and(|c| !c.is_ascii_whitespace() && !matches!(c, b'>' | b'/' | b'=')) {
            self.at += 1;
        }
        (self.at > from).then(|| self.read_since(from).map(str::to_owned)).flatten()
    }

    fn element(&mut self) -> Option<Element> {
        self.depth += 1;
        if self.depth > MOST_DEPTH {
            return None;
        }
        // past the '<'
        self.at += 1;
        let name = self.name()?;
        let mut element = Element { name, ..Element::default() };
        loop {
            self.skip_space();
            match self.peek()? {
                b'/' => {
                    self.at += 1;
                    if self.peek()? != b'>' {
                        return None;
                    }
                    self.at += 1;
                    self.depth -= 1;
                    return Some(element);
                }
                b'>' => {
                    self.at += 1;
                    break;
                }
                _ => {
                    let key = self.name()?;
                    self.skip_space();
                    if self.peek()? != b'=' {
                        return None;
                    }
                    self.at += 1;
                    self.skip_space();
                    let quote = self.peek()?;
                    if quote != b'"' && quote != b'\'' {
                        return None;
                    }
                    self.at += 1;
                    let from = self.at;
                    while self.peek()? != quote {
                        self.at += 1;
                    }
                    let value = unescape(self.read_since(from)?);
                    self.at += 1;
                    element.attributes.push((key, value));
                }
            }
        }
        // its content, to its end tag
        loop {
            if self.starts("</") {
                self.skip_past(">")?;
                self.depth -= 1;
                return Some(element);
            }
            if self.starts("<![CDATA[") {
                self.at += 9;
                let from = self.at;
                self.skip_past("]]>")?;
                let text = self.t.get(from..self.at - 3)?.to_owned();
                element.children.push(Node::Text(text));
            } else if self.starts("<!--") {
                self.skip_past("-->")?;
            } else if self.starts("<?") {
                self.skip_past("?>")?;
            } else if self.starts("<") {
                let child = self.element()?;
                element.children.push(Node::Element(child));
            } else {
                let from = self.at;
                self.at = self.next(from, '<')?;
                let text = unescape(self.read_since(from)?);
                if !text.trim().is_empty() {
                    element.children.push(Node::Text(text));
                }
            }
        }
    }
}

/// `text` with its entities written out.
pub fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let c = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_elements_attributes_and_cdata() {
        let doc = r#"<?xml version="1.0"?><!DOCTYPE params><Preset name="a &amp; b"><param name="x"><![CDATA[<i>1</i>]]></param><e k='2'/></Preset>"#;
        let root = parse(doc).unwrap();
        assert_eq!(root.attribute("name"), Some("a & b"));
        assert_eq!(root.child("param").unwrap().text(), "<i>1</i>");
        assert_eq!(root.child("e").unwrap().number("k"), Some(2.0));
        assert!(parse("<a><b></a").is_none());
    }
}
