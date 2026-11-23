//! Bounded, zero-float RFC 8259 JSON parser and RFC 8785 canonical JSON serializer.
//!
//! Used inside `fid-report-pdf` so report layout, painting, and PDF writing never touch `f32`
//! or `f64` (CONSTITUTION C1 / RPT float rule / `RPT.OUTPUT.PDF.LAYOUT` REQ-001).

use std::collections::BTreeMap;
use std::fmt;

/// Maximum input JSON size accepted (64 MiB).
pub const MAX_JSON_BYTES: usize = 64 * 1024 * 1024;
/// Maximum JSON container nesting depth.
pub const MAX_JSON_DEPTH: usize = 64;
/// Maximum total JSON nodes (values) allocated in one parse.
pub const MAX_JSON_NODES: usize = 2_000_000;

/// Pure-integer JSON value with no floating-point representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonVal {
    Null,
    Bool(bool),
    /// Exact signed 64-bit integer.
    Int(i64),
    /// Any JSON number with a fraction, exponent, or exceeding `i64` range, stored verbatim as
    /// ASCII text without ever converting to floating point.
    Dec(String),
    Str(String),
    Arr(Vec<JsonVal>),
    Obj(BTreeMap<String, JsonVal>),
}

impl JsonVal {
    pub fn as_obj(&self) -> Option<&BTreeMap<String, JsonVal>> {
        match self {
            JsonVal::Obj(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[JsonVal]> {
        match self {
            JsonVal::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonVal::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            JsonVal::Int(n) => Some(*n),
            JsonVal::Str(s) => s.parse::<i64>().ok(),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonVal::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsonVal> {
        self.as_obj()?.get(key)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, JsonVal::Null)
    }
}

/// Errors returned while parsing JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonError {
    TooLarge,
    DepthExceeded,
    NodeLimitExceeded,
    InvalidUtf8,
    DuplicateKey(String),
    Syntax { offset: usize, detail: &'static str },
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonError::TooLarge => f.write_str("json input exceeds MAX_JSON_BYTES"),
            JsonError::DepthExceeded => f.write_str("json nesting exceeds MAX_JSON_DEPTH"),
            JsonError::NodeLimitExceeded => f.write_str("json node count exceeds MAX_JSON_NODES"),
            JsonError::InvalidUtf8 => f.write_str("json input is not valid UTF-8"),
            JsonError::DuplicateKey(k) => write!(f, "duplicate json object key: {k}"),
            JsonError::Syntax { offset, detail } => write!(f, "json syntax error at byte {offset}: {detail}"),
        }
    }
}

impl std::error::Error for JsonError {}

/// Parses strict RFC 8259 UTF-8 JSON with size, depth, node, and duplicate-key bounds.
pub fn parse_json(bytes: &[u8]) -> Result<JsonVal, JsonError> {
    if bytes.len() > MAX_JSON_BYTES {
        return Err(JsonError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| JsonError::InvalidUtf8)?;
    let mut p = Parser { src: text.as_bytes(), pos: 0, nodes: 0 };
    p.skip_ws();
    let val = p.parse_value(0)?;
    p.skip_ws();
    if p.pos != p.src.len() {
        return Err(JsonError::Syntax { offset: p.pos, detail: "trailing bytes after value" });
    }
    Ok(val)
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    nodes: usize,
}

impl<'a> Parser<'a> {
    fn bump_node(&mut self) -> Result<(), JsonError> {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > MAX_JSON_NODES {
            return Err(JsonError::NodeLimitExceeded);
        }
        Ok(())
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn expect_bytes(&mut self, lit: &[u8], detail: &'static str) -> Result<(), JsonError> {
        if self.src.get(self.pos..self.pos + lit.len()) == Some(lit) {
            self.pos += lit.len();
            Ok(())
        } else {
            Err(JsonError::Syntax { offset: self.pos, detail })
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<JsonVal, JsonError> {
        self.bump_node()?;
        match self.peek() {
            Some(b'n') => {
                self.expect_bytes(b"null", "expected null")?;
                Ok(JsonVal::Null)
            }
            Some(b't') => {
                self.expect_bytes(b"true", "expected true")?;
                Ok(JsonVal::Bool(true))
            }
            Some(b'f') => {
                self.expect_bytes(b"false", "expected false")?;
                Ok(JsonVal::Bool(false))
            }
            Some(b'"') => self.parse_string().map(JsonVal::Str),
            Some(b'[') => self.parse_array(depth),
            Some(b'{') => self.parse_object(depth),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            _ => Err(JsonError::Syntax { offset: self.pos, detail: "unexpected token" }),
        }
    }

    fn parse_array(&mut self, depth: usize) -> Result<JsonVal, JsonError> {
        if depth >= MAX_JSON_DEPTH {
            return Err(JsonError::DepthExceeded);
        }
        self.pos += 1;
        self.skip_ws();
        let mut out = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(JsonVal::Arr(out));
        }
        loop {
            let v = self.parse_value(depth + 1)?;
            out.push(v);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.skip_ws();
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(JsonVal::Arr(out));
                }
                _ => return Err(JsonError::Syntax { offset: self.pos, detail: "expected , or ]" }),
            }
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<JsonVal, JsonError> {
        if depth >= MAX_JSON_DEPTH {
            return Err(JsonError::DepthExceeded);
        }
        self.pos += 1;
        self.skip_ws();
        let mut map = BTreeMap::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(JsonVal::Obj(map));
        }
        loop {
            if self.peek() != Some(b'"') {
                return Err(JsonError::Syntax { offset: self.pos, detail: "expected object key string" });
            }
            let key = self.parse_string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(JsonError::Syntax { offset: self.pos, detail: "expected :" });
            }
            self.pos += 1;
            self.skip_ws();
            let val = self.parse_value(depth + 1)?;
            if map.contains_key(&key) {
                return Err(JsonError::DuplicateKey(key));
            }
            map.insert(key, val);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                    self.skip_ws();
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(JsonVal::Obj(map));
                }
                _ => return Err(JsonError::Syntax { offset: self.pos, detail: "expected , or }" }),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, JsonError> {
        if self.peek() != Some(b'"') {
            return Err(JsonError::Syntax { offset: self.pos, detail: "expected \"" });
        }
        self.pos += 1;
        let mut out = String::new();
        let mut seg_start = self.pos;
        while self.pos < self.src.len() {
            let b = self.src[self.pos];
            if b == b'"' {
                if seg_start < self.pos {
                    let s = std::str::from_utf8(&self.src[seg_start..self.pos]).map_err(|_| JsonError::InvalidUtf8)?;
                    out.push_str(s);
                }
                self.pos += 1;
                return Ok(out);
            }
            if b < 0x20 {
                return Err(JsonError::Syntax { offset: self.pos, detail: "unescaped control character in string" });
            }
            if b == b'\\' {
                if seg_start < self.pos {
                    let s = std::str::from_utf8(&self.src[seg_start..self.pos]).map_err(|_| JsonError::InvalidUtf8)?;
                    out.push_str(s);
                }
                self.pos += 1;
                let esc = self.peek().ok_or(JsonError::Syntax { offset: self.pos, detail: "unterminated escape" })?;
                self.pos += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let u1 = self.parse_hex4()?;
                        if (0xD800..=0xDBFF).contains(&u1) {
                            if self.src.get(self.pos..self.pos + 2) != Some(b"\\u") {
                                return Err(JsonError::Syntax { offset: self.pos, detail: "unpaired high surrogate" });
                            }
                            self.pos += 2;
                            let u2 = self.parse_hex4()?;
                            if !(0xDC00..=0xDFFF).contains(&u2) {
                                return Err(JsonError::Syntax { offset: self.pos, detail: "invalid low surrogate" });
                            }
                            let cp = 0x10000 + ((u32::from(u1) - 0xD800) << 10) + (u32::from(u2) - 0xDC00);
                            let ch = char::from_u32(cp)
                                .ok_or(JsonError::Syntax { offset: self.pos, detail: "invalid surrogate codepoint" })?;
                            out.push(ch);
                        } else if (0xDC00..=0xDFFF).contains(&u1) {
                            return Err(JsonError::Syntax { offset: self.pos, detail: "unpaired low surrogate" });
                        } else {
                            let ch = char::from_u32(u32::from(u1))
                                .ok_or(JsonError::Syntax { offset: self.pos, detail: "invalid unicode escape" })?;
                            out.push(ch);
                        }
                    }
                    _ => return Err(JsonError::Syntax { offset: self.pos - 1, detail: "unknown escape sequence" }),
                }
                seg_start = self.pos;
            } else {
                self.pos += 1;
            }
        }
        Err(JsonError::Syntax { offset: self.pos, detail: "unterminated string" })
    }

    fn parse_hex4(&mut self) -> Result<u16, JsonError> {
        let slice =
            self.src.get(self.pos..self.pos + 4).ok_or(JsonError::Syntax { offset: self.pos, detail: "short \\u" })?;
        let mut val: u16 = 0;
        for &b in slice {
            let d = match b {
                b'0'..=b'9' => u16::from(b - b'0'),
                b'a'..=b'f' => u16::from(b - b'a' + 10),
                b'A'..=b'F' => u16::from(b - b'A' + 10),
                _ => return Err(JsonError::Syntax { offset: self.pos, detail: "non-hex digit in \\u" }),
            };
            val = (val << 4) | d;
        }
        self.pos += 4;
        Ok(val)
    }

    fn parse_number(&mut self) -> Result<JsonVal, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.pos += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(JsonError::Syntax { offset: self.pos, detail: "invalid number" }),
        }
        let mut is_decimal = false;
        if self.peek() == Some(b'.') {
            is_decimal = true;
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(JsonError::Syntax { offset: self.pos, detail: "expected digit after ." });
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_decimal = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(JsonError::Syntax { offset: self.pos, detail: "expected digit in exponent" });
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let slice = std::str::from_utf8(&self.src[start..self.pos]).map_err(|_| JsonError::InvalidUtf8)?;
        if !is_decimal {
            if let Ok(n) = slice.parse::<i64>() {
                return Ok(JsonVal::Int(n));
            }
        }
        Ok(JsonVal::Dec(slice.to_string()))
    }
}

/// Serializes `val` to RFC 8785 canonical JSON UTF-8 bytes without touching floating point.
pub fn to_canonical_bytes(val: &JsonVal) -> Vec<u8> {
    let mut out = Vec::new();
    write_val(val, &mut out);
    out
}

fn write_val(val: &JsonVal, out: &mut Vec<u8>) {
    match val {
        JsonVal::Null => out.extend_from_slice(b"null"),
        JsonVal::Bool(true) => out.extend_from_slice(b"true"),
        JsonVal::Bool(false) => out.extend_from_slice(b"false"),
        JsonVal::Int(n) => out.extend_from_slice(n.to_string().as_bytes()),
        JsonVal::Dec(s) => out.extend_from_slice(s.as_bytes()),
        JsonVal::Str(s) => write_str(s, out),
        JsonVal::Arr(arr) => {
            out.push(b'[');
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_val(item, out);
            }
            out.push(b']');
        }
        JsonVal::Obj(map) => {
            // Sort keys by UTF-16 code units per RFC 8785 §3.2.3.
            let mut entries: Vec<(&String, &JsonVal)> = map.iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
            out.push(b'{');
            for (i, (k, v)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_str(k, out);
                out.push(b':');
                write_val(v, out);
            }
            out.push(b'}');
        }
    }
}

fn write_str(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for ch in s.chars() {
        match ch {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{0008}' => out.extend_from_slice(b"\\b"),
            '\u{000C}' => out.extend_from_slice(b"\\f"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            c if (c as u32) < 0x20 => {
                let esc = format!("\\u{:04x}", c as u32);
                out.extend_from_slice(esc.as_bytes());
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

/// Helper to construct a [`JsonVal::Obj`] from key/value pairs.
pub fn obj<const N: usize>(pairs: [(&str, JsonVal); N]) -> JsonVal {
    let mut map = BTreeMap::new();
    for (k, v) in pairs {
        map.insert(k.to_string(), v);
    }
    JsonVal::Obj(map)
}
