//! A small JSON reader (RFC 8259), for MyPaint's brush files: objects keep their keys in order.

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    /// The value under `key`, if this is an object that has one.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::Bool(b) => Some(f64::from(u8::from(*b))),
            _ => None,
        }
    }

    pub fn list(&self) -> &[Value] {
        match self {
            Value::List(items) => items,
            _ => &[],
        }
    }

    pub fn entries(&self) -> &[(String, Value)] {
        match self {
            Value::Object(entries) => entries,
            _ => &[],
        }
    }

    /// The value under `key` of an object, made (null) if it has none; none if this isn't one.
    pub fn entry(&mut self, key: &str) -> Option<&mut Value> {
        let Value::Object(entries) = self else { return None };
        let at = match entries.iter().position(|(k, _)| k == key) {
            Some(at) => at,
            None => {
                entries.push((key.to_owned(), Value::Null));
                entries.len() - 1
            }
        };
        entries.get_mut(at).map(|(_, v)| v)
    }

    /// As JSON text, indented as MyPaint writes its files.
    pub fn write(&self) -> String {
        let mut out = String::new();
        self.write_into(&mut out, 0);
        out
    }

    fn write_into(&self, out: &mut String, depth: usize) {
        let indent = |out: &mut String, d: usize| {
            out.push('\n');
            out.push_str(&"    ".repeat(d));
        };
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => out.push_str(&format!("{n:?}")),
            Value::Text(t) => {
                out.push('"');
                for c in t.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            Value::List(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    indent(out, depth + 1);
                    v.write_into(out, depth + 1);
                }
                if !items.is_empty() {
                    indent(out, depth);
                }
                out.push(']');
            }
            Value::Object(entries) => {
                out.push('{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    indent(out, depth + 1);
                    Value::Text(k.clone()).write_into(out, depth + 1);
                    out.push_str(": ");
                    v.write_into(out, depth + 1);
                }
                if !entries.is_empty() {
                    indent(out, depth);
                }
                out.push('}');
            }
        }
    }
}

/// The value `text` holds, if it is JSON (trailing blanks allowed).
pub fn parse(text: &str) -> Option<Value> {
    let mut r = Reader { bytes: text.as_bytes(), at: 0, depth: 0 };
    let v = r.value()?;
    r.blanks();
    (r.at == r.bytes.len()).then_some(v)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    depth: u32,
}

/// How deep values may nest.
const MOST_DEPTH: u32 = 64;

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn blanks(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn word(&mut self, w: &[u8]) -> bool {
        if self.bytes.get(self.at..self.at + w.len()) == Some(w) {
            self.at += w.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Option<Value> {
        self.blanks();
        match self.peek()? {
            b'{' => self.object(),
            b'[' => self.list(),
            b'"' => self.text().map(Value::Text),
            b't' => self.word(b"true").then_some(Value::Bool(true)),
            b'f' => self.word(b"false").then_some(Value::Bool(false)),
            b'n' => self.word(b"null").then_some(Value::Null),
            _ => self.number(),
        }
    }

    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        if self.depth >= MOST_DEPTH {
            return None;
        }
        self.depth += 1;
        let v = f(self);
        self.depth -= 1;
        v
    }

    fn object(&mut self) -> Option<Value> {
        self.nested(|r| {
            r.at += 1;
            let mut entries = Vec::new();
            r.blanks();
            if r.peek() == Some(b'}') {
                r.at += 1;
                return Some(Value::Object(entries));
            }
            loop {
                r.blanks();
                let key = r.text()?;
                r.blanks();
                if r.peek()? != b':' {
                    return None;
                }
                r.at += 1;
                let v = r.value()?;
                entries.push((key, v));
                r.blanks();
                match r.peek()? {
                    b',' => r.at += 1,
                    b'}' => {
                        r.at += 1;
                        return Some(Value::Object(entries));
                    }
                    _ => return None,
                }
            }
        })
    }

    fn list(&mut self) -> Option<Value> {
        self.nested(|r| {
            r.at += 1;
            let mut items = Vec::new();
            r.blanks();
            if r.peek() == Some(b']') {
                r.at += 1;
                return Some(Value::List(items));
            }
            loop {
                items.push(r.value()?);
                r.blanks();
                match r.peek()? {
                    b',' => r.at += 1,
                    b']' => {
                        r.at += 1;
                        return Some(Value::List(items));
                    }
                    _ => return None,
                }
            }
        })
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.bytes.get(self.at..self.at + 4)?;
        let v = u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()?;
        self.at += 4;
        Some(v)
    }

    fn text(&mut self) -> Option<String> {
        if self.peek()? != b'"' {
            return None;
        }
        self.at += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let b = self.peek()?;
            self.at += 1;
            match b {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = self.peek()?;
                    self.at += 1;
                    let c = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut u = self.hex4()?;
                            // a pair of surrogates
                            if (0xd800..0xdc00).contains(&u) && self.word(b"\\u") {
                                let low = self.hex4()?;
                                u = 0x10000 + ((u - 0xd800) << 10) + (low.wrapping_sub(0xdc00) & 0x3ff);
                            }
                            char::from_u32(u).unwrap_or('\u{fffd}')
                        }
                        _ => return None,
                    };
                    let mut buf = [0; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(b),
            }
        }
    }

    fn number(&mut self) -> Option<Value> {
        let from = self.at;
        while matches!(self.peek(), Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')) {
            self.at += 1;
        }
        let text = std::str::from_utf8(self.bytes.get(from..self.at)?).ok()?;
        text.parse::<f64>().ok().filter(|n| n.is_finite()).map(Value::Number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_objects_lists_numbers_and_texts() {
        let v = parse(r#" {"a": [1, -2.5e1, true, null], "b": {"c": "xé\"y"}} "#).unwrap();
        assert_eq!(v.get("a").unwrap().list().len(), 4);
        assert_eq!(v.get("a").unwrap().list()[1].number(), Some(-25.0));
        assert_eq!(v.get("b").unwrap().get("c"), Some(&Value::Text("x\u{e9}\"y".into())));
        assert!(parse("{\"a\": }").is_none());
        assert!(parse("[1, 2").is_none());
        assert!(parse(&"[".repeat(100)).is_none());
        // written and read again, the same
        assert_eq!(parse(&v.write()), Some(v.clone()));
        let mut edited = v;
        *edited.entry("d").unwrap() = Value::Number(0.25);
        assert_eq!(parse(&edited.write()).unwrap().get("d").and_then(Value::number), Some(0.25));
    }
}
