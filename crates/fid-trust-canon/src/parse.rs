//! Bounded JSON parser (RFC 8259 grammar) producing an ordered [`Value`] tree.

use crate::{CanonError, InvalidKind, Value, MAX_DEPTH, MAX_INPUT_BYTES};

/// Parser limits. [`parse_with_limits`] clamps them to [`MAX_INPUT_BYTES`] and [`MAX_DEPTH`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_depth: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_bytes: MAX_INPUT_BYTES, max_depth: MAX_DEPTH }
    }
}

/// Parses JSON text with the default limits.
pub fn parse(input: &[u8]) -> Result<Value, CanonError> {
    parse_with_limits(input, Limits::default())
}

/// Parses JSON text with caller limits, which can only be tighter than the defaults.
pub fn parse_with_limits(input: &[u8], limits: Limits) -> Result<Value, CanonError> {
    let max_bytes = limits.max_bytes.min(MAX_INPUT_BYTES);
    let max_depth = limits.max_depth.min(MAX_DEPTH);
    if input.len() > max_bytes {
        return Err(CanonError::invalid(InvalidKind::TooLarge, max_bytes));
    }
    if let Err(e) = std::str::from_utf8(input) {
        return Err(CanonError::invalid(InvalidKind::Utf8, e.valid_up_to()));
    }
    let mut p = Parser { b: input, pos: 0, max_depth };
    p.skip_ws();
    let v = p.value(0)?;
    p.skip_ws();
    if p.pos != input.len() {
        return Err(p.err(InvalidKind::Syntax));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    pos: usize,
    max_depth: usize,
}

impl Parser<'_> {
    fn err(&self, kind: InvalidKind) -> CanonError {
        CanonError::invalid(kind, self.pos)
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.pos += 1;
        }
    }

    /// Consumes `want` or fails with a syntax error.
    fn eat(&mut self, want: u8) -> Result<(), CanonError> {
        if self.peek() == Some(want) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.err(InvalidKind::Syntax))
        }
    }

    /// `depth` is the nesting depth of the container that starts here, if any.
    fn value(&mut self, depth: usize) -> Result<Value, CanonError> {
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.err(InvalidKind::Syntax)),
        }
    }

    fn literal(&mut self, word: &[u8], v: Value) -> Result<Value, CanonError> {
        if self.b.get(self.pos..self.pos + word.len()) == Some(word) {
            self.pos += word.len();
            Ok(v)
        } else {
            Err(self.err(InvalidKind::Syntax))
        }
    }

    fn enter(&self, depth: usize) -> Result<(), CanonError> {
        if depth > self.max_depth {
            Err(self.err(InvalidKind::TooDeep))
        } else {
            Ok(())
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, CanonError> {
        self.enter(depth)?;
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth)?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.err(InvalidKind::Syntax)),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, CanonError> {
        self.enter(depth)?;
        let start = self.pos;
        self.pos += 1;
        let mut members: Vec<(String, Value)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.err(InvalidKind::Syntax));
            }
            let key = self.string()?;
            self.skip_ws();
            self.eat(b':')?;
            self.skip_ws();
            let v = self.value(depth)?;
            members.push((key, v));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.err(InvalidKind::Syntax)),
            }
        }
        // Fast path for already-strictly-sorted objects (e.g., canonical JSON): O(n) with zero
        // allocation. Fall back to O(n log n) index sort otherwise (never O(n^2) on hostile input).
        if !members.windows(2).all(|w| w[0].0 < w[1].0) {
            let mut order: Vec<usize> = (0..members.len()).collect();
            order.sort_unstable_by(|&a, &b| members[a].0.cmp(&members[b].0));
            if order.windows(2).any(|w| members[w[0]].0 == members[w[1]].0) {
                return Err(CanonError::invalid(InvalidKind::DuplicateKey, start));
            }
        }
        Ok(Value::Object(members))
    }

    fn hex4(&mut self) -> Result<u32, CanonError> {
        let mut cp = 0u32;
        for _ in 0..4 {
            let d = match self.peek() {
                Some(c @ b'0'..=b'9') => u32::from(c - b'0'),
                Some(c @ b'a'..=b'f') => u32::from(c - b'a') + 10,
                Some(c @ b'A'..=b'F') => u32::from(c - b'A') + 10,
                _ => return Err(self.err(InvalidKind::Syntax)),
            };
            cp = cp * 16 + d;
            self.pos += 1;
        }
        Ok(cp)
    }

    /// Parses a string starting at its opening quote. The input is known to be UTF-8.
    fn string(&mut self) -> Result<String, CanonError> {
        self.pos += 1;
        let start = self.pos;
        let bytes = self.b;
        // Fast path for strings without backslash escapes (the common case): single allocation.
        while let Some(&c) = bytes.get(self.pos) {
            if c == b'"' {
                let end = self.pos;
                self.pos += 1;
                let s = std::str::from_utf8(&bytes[start..end]).map_err(|_| self.err(InvalidKind::Utf8))?;
                return Ok(s.to_owned());
            }
            if c == b'\\' {
                break;
            }
            if c < 0x20 {
                return Err(self.err(InvalidKind::Syntax));
            }
            self.pos += 1;
        }
        let mut buf: Vec<u8> = Vec::with_capacity(self.pos.saturating_sub(start).saturating_add(16));
        buf.extend_from_slice(&bytes[start..self.pos]);
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err(InvalidKind::Syntax));
            };
            match c {
                b'"' => {
                    self.pos += 1;
                    break;
                }
                b'\\' => {
                    let at = self.pos;
                    self.pos += 1;
                    let Some(e) = self.peek() else {
                        return Err(self.err(InvalidKind::Syntax));
                    };
                    self.pos += 1;
                    let short = match e {
                        b'"' => Some(b'"'),
                        b'\\' => Some(b'\\'),
                        b'/' => Some(b'/'),
                        b'b' => Some(0x08),
                        b'f' => Some(0x0c),
                        b'n' => Some(b'\n'),
                        b'r' => Some(b'\r'),
                        b't' => Some(b'\t'),
                        b'u' => None,
                        _ => return Err(CanonError::invalid(InvalidKind::Syntax, at)),
                    };
                    if let Some(byte) = short {
                        buf.push(byte);
                        continue;
                    }
                    let unit = self.hex4()?;
                    let cp = if (0xD800..0xDC00).contains(&unit) {
                        // A high surrogate must be followed by an escaped low surrogate.
                        if self.b.get(self.pos) != Some(&b'\\') || self.b.get(self.pos + 1) != Some(&b'u') {
                            return Err(CanonError::invalid(InvalidKind::LoneSurrogate, at));
                        }
                        self.pos += 2;
                        let low = self.hex4()?;
                        if !(0xDC00..0xE000).contains(&low) {
                            return Err(CanonError::invalid(InvalidKind::LoneSurrogate, at));
                        }
                        0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
                    } else if (0xDC00..0xE000).contains(&unit) {
                        return Err(CanonError::invalid(InvalidKind::LoneSurrogate, at));
                    } else {
                        unit
                    };
                    let Some(ch) = char::from_u32(cp) else {
                        return Err(CanonError::invalid(InvalidKind::LoneSurrogate, at));
                    };
                    let mut tmp = [0u8; 4];
                    buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                }
                0x00..=0x1f => return Err(self.err(InvalidKind::Syntax)),
                _ => {
                    let span_start = self.pos;
                    self.pos += 1;
                    while let Some(&b) = bytes.get(self.pos) {
                        if b == b'"' || b == b'\\' || b < 0x20 {
                            break;
                        }
                        self.pos += 1;
                    }
                    buf.extend_from_slice(&bytes[span_start..self.pos]);
                }
            }
        }
        String::from_utf8(buf).map_err(|_| self.err(InvalidKind::Utf8))
    }

    fn digits(&mut self) -> usize {
        let from = self.pos;
        while let Some(b'0'..=b'9') = self.peek() {
            self.pos += 1;
        }
        self.pos - from
    }

    fn number(&mut self) -> Result<Value, CanonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.err(InvalidKind::Syntax)),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return Err(self.err(InvalidKind::Syntax));
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.pos += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(self.err(InvalidKind::Syntax));
            }
        }
        // The slice is ASCII and matches the JSON number grammar, which Rust's
        // correctly rounded `f64` parser accepts; overflow yields infinity.
        let text = std::str::from_utf8(&self.b[start..self.pos])
            .map_err(|_| CanonError::invalid(InvalidKind::Syntax, start))?;
        let v: f64 = text.parse().map_err(|_| CanonError::invalid(InvalidKind::Syntax, start))?;
        if !v.is_finite() {
            return Err(CanonError::invalid(InvalidKind::NumberRange, start));
        }
        Ok(Value::Number(v))
    }
}
