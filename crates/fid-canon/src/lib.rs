//! RFC 8785 (JCS) canonical JSON and digests. Spec node `CORE.JRN.CANON`.
//!
//! The journal, the registry manifest, the verifier and replay all hash through
//! this crate, so they agree byte for byte. Project number rules on top of JCS:
//!
//! - 64-bit integers ([`CValue::I64`], [`CValue::U64`]) are JSON strings of decimal digits;
//! - bit-significant floats ([`CValue::F64Bits`]) are `"0x"` plus 16 lowercase hex digits
//!   of their IEEE 754 bits, so `-0.0` and `0.0` differ;
//! - NaN and infinity are rejected everywhere ([`CanonError::NonFiniteNumber`]).
//!
//! Parsing and writing are iterative, so hostile nesting cannot overflow the stack.

use sha2::{Digest as _, Sha256};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

/// Largest accepted input, in bytes (REQ-CORE.JRN.CANON-007).
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// Deepest accepted nesting of arrays and objects (REQ-CORE.JRN.CANON-007).
pub const MAX_DEPTH: usize = 128;

/// Typed JSON value model for hashed structures.
#[derive(Debug, Clone, PartialEq)]
pub enum CValue {
    Null,
    Bool(bool),
    Str(String),
    /// Encoded as a decimal string.
    I64(i64),
    /// Encoded as a decimal string.
    U64(u64),
    /// Encoded as `"0x"` + 16 lowercase hex digits of the IEEE 754 bits.
    F64Bits(f64),
    /// A finite, non-bit-significant number, printed per ECMAScript rules.
    Num(f64),
    Arr(Vec<CValue>),
    Obj(BTreeMap<String, CValue>),
}

/// Typed errors with stable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonError {
    NonFiniteNumber,
    DuplicateKey(String),
    InputLimit,
    InvalidUtf8,
    Syntax(usize),
}

impl CanonError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            CanonError::NonFiniteNumber => "non_finite_number",
            CanonError::DuplicateKey(_) => "duplicate_key",
            CanonError::InputLimit => "input_limit",
            CanonError::InvalidUtf8 => "invalid_utf8",
            CanonError::Syntax(_) => "syntax",
        }
    }
}

impl fmt::Display for CanonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanonError::DuplicateKey(k) => write!(f, "duplicate_key: {k:?}"),
            CanonError::Syntax(at) => write!(f, "syntax: at byte {at}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for CanonError {}

/// A SHA-256 digest of canonical bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Digest(pub [u8; 32]);

impl fmt::Display for Digest {
    /// Formats as `sha256:` + 64 lowercase hex digits (REQ-CORE.JRN.CANON-006).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sha256:")?;
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// SHA-256 over already-canonical bytes.
pub fn digest_bytes(canonical: &[u8]) -> Digest {
    Digest(Sha256::digest(canonical).into())
}

/// Canonical bytes and their digest.
pub fn digest(value: &CValue) -> Result<Digest, CanonError> {
    to_canonical_bytes(value).map(|b| digest_bytes(&b))
}

// ------------------------------------------------------------------ writer

enum Work<'a> {
    Val(&'a CValue),
    Raw(&'static str),
    Key(&'a str),
}

/// Serialises `value` to RFC 8785 canonical bytes. Produces no bytes on error.
pub fn to_canonical_bytes(value: &CValue) -> Result<Vec<u8>, CanonError> {
    let mut out = String::new();
    let mut stack = vec![Work::Val(value)];
    while let Some(w) = stack.pop() {
        match w {
            Work::Raw(s) => out.push_str(s),
            Work::Key(k) => {
                write_str(&mut out, k);
                out.push(':');
            }
            Work::Val(v) => match v {
                CValue::Null => out.push_str("null"),
                CValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
                CValue::Str(s) => write_str(&mut out, s),
                CValue::I64(i) => {
                    out.push('"');
                    out.push_str(&i.to_string());
                    out.push('"');
                }
                CValue::U64(u) => {
                    out.push('"');
                    out.push_str(&u.to_string());
                    out.push('"');
                }
                CValue::F64Bits(f) => {
                    if !f.is_finite() {
                        return Err(CanonError::NonFiniteNumber);
                    }
                    out.push_str(&format!("\"0x{:016x}\"", f.to_bits()));
                }
                CValue::Num(f) => out.push_str(&format_number(*f)?),
                CValue::Arr(items) => {
                    out.push('[');
                    stack.push(Work::Raw("]"));
                    for (i, item) in items.iter().enumerate().rev() {
                        stack.push(Work::Val(item));
                        if i > 0 {
                            stack.push(Work::Raw(","));
                        }
                    }
                }
                CValue::Obj(map) => {
                    out.push('{');
                    stack.push(Work::Raw("}"));
                    let mut keys: Vec<(&String, &CValue)> = map.iter().collect();
                    keys.sort_by(|a, b| utf16_cmp(a.0, b.0));
                    for (i, (k, item)) in keys.into_iter().enumerate().rev() {
                        stack.push(Work::Val(item));
                        stack.push(Work::Key(k));
                        if i > 0 {
                            stack.push(Work::Raw(","));
                        }
                    }
                }
            },
        }
    }
    Ok(out.into_bytes())
}

/// RFC 8785 §3.2.3: property names sort by their UTF-16 code units.
fn utf16_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// RFC 8785 §3.2.2.2 string serialisation.
fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// RFC 8785 §3.2.2.3: ECMAScript Number::toString for finite doubles,
/// built from Rust's shortest round-trip digits, with exact ties resolved to the even
/// digit (ECMA-262 Number::toString, Note 2), as V8, CPython and other JCS writers do.
pub fn format_number(v: f64) -> Result<String, CanonError> {
    if !v.is_finite() {
        return Err(CanonError::NonFiniteNumber);
    }
    if v == 0.0 {
        return Ok("0".to_string());
    }
    // Rust's `{:e}` gives the shortest round-trip digits, e.g. "1.2345e-7", "5e-324".
    let (shortest, exp) = sci_digits(&format!("{:e}", v.abs())).ok_or(CanonError::NonFiniteNumber)?;
    let digits = tie_to_even(v.abs(), &shortest, exp).unwrap_or(shortest);
    let k = digits.len() as i32;
    let n = exp + 1;
    let mut s = String::new();
    if v < 0.0 {
        s.push('-');
    }
    if k <= n && n <= 21 {
        s.push_str(&digits);
        s.push_str(&"0".repeat((n - k) as usize));
    } else if 0 < n && n <= 21 {
        let (a, b) = digits.split_at(n as usize);
        s.push_str(a);
        s.push('.');
        s.push_str(b);
    } else if -6 < n && n <= 0 {
        s.push_str("0.");
        s.push_str(&"0".repeat((-n) as usize));
        s.push_str(&digits);
    } else {
        let e = n - 1;
        let (first, rest) = digits.split_at(1);
        s.push_str(first);
        if !rest.is_empty() {
            s.push('.');
            s.push_str(rest);
        }
        s.push('e');
        s.push(if e < 0 { '-' } else { '+' });
        s.push_str(&e.abs().to_string());
    }
    Ok(s)
}

/// Splits Rust's `{:e}` output (`"d.ddde-x"`) into its significant digits and decimal exponent.
fn sci_digits(sci: &str) -> Option<(String, i32)> {
    let (mant, exp) = sci.split_once('e')?;
    let exp: i32 = exp.parse().ok()?;
    Some((mant.chars().filter(|c| *c != '.').collect(), exp))
}

/// ECMA-262 Number::toString, Note 2: if two shortest digit strings are equally close to
/// `a`, the even one wins. Rust's shortest formatting rounds such exact midpoints up, so
/// when `a` lies exactly halfway between `digits` and its neighbour, this returns the even
/// neighbour (if it also round-trips to `a`). `None` keeps `digits`.
///
/// Exactness is decided on Rust's exact-mode digits, which are correctly rounded and need
/// no tie rule here: a midpoint has a finite expansion of `k + 1` digits, and every double's
/// full expansion (at most 767 significant digits) fits in the 801 digits requested.
fn tie_to_even(a: f64, digits: &str, exp: i32) -> Option<String> {
    let k = digits.len();
    let last = *digits.as_bytes().last()?;
    if (last - b'0') % 2 == 0 {
        return None;
    }
    // Cheap filter: an exact midpoint prints exactly with k + 1 significant digits.
    let (near, near_exp) = sci_digits(&format!("{a:.k$e}"))?;
    if near_exp != exp || !near.ends_with('5') {
        return None;
    }
    let stem = near.get(..k)?;
    let head = digits.get(..k - 1)?;
    let candidate = if stem == digits {
        // Midpoint above `digits`: the even neighbour is one unit up (a carry from 9 would
        // give a shorter string, which the shortest search would already have found).
        if last == b'9' {
            return None;
        }
        format!("{head}{}", char::from(last + 1))
    } else if stem.get(..k - 1) == Some(head) && stem.as_bytes().last() == Some(&(last - 1)) {
        // Midpoint below `digits`: the even neighbour is one unit down.
        stem.to_string()
    } else {
        return None;
    };
    if candidate.starts_with('0') {
        return None;
    }
    // Exactness: the full expansion must be stem, '5', then only zeros.
    let (full, full_exp) = sci_digits(&format!("{a:.800e}"))?;
    let tail = full.get(k + 1..)?;
    if full_exp != exp || !full.starts_with(&near) || !tail.bytes().all(|b| b == b'0') {
        return None;
    }
    // The even neighbour must still round-trip (it is as close as `digits`, but the
    // rounding interval can exclude its ends).
    let back: f64 = format!("{candidate}e{}", i64::from(exp) - k as i64 + 1).parse().ok()?;
    if back.to_bits() != a.to_bits() {
        return None;
    }
    Some(candidate.trim_end_matches('0').to_string())
}

// ------------------------------------------------------------------ parser

enum Frame {
    Arr(Vec<CValue>),
    Obj(BTreeMap<String, CValue>, String),
}

/// Parses JSON text into a [`CValue`]. Numbers become [`CValue::Num`].
/// Rejects duplicate keys, inputs over [`MAX_INPUT_BYTES`] and nesting over [`MAX_DEPTH`].
pub fn parse(input: &[u8]) -> Result<CValue, CanonError> {
    parse_with_limits(input, MAX_INPUT_BYTES, MAX_DEPTH)
}

/// [`parse`] with caller-chosen limits, for surfaces with tighter budgets.
/// The limits are clamped to [`MAX_INPUT_BYTES`] and [`MAX_DEPTH`].
pub fn parse_with_limits(input: &[u8], max_bytes: usize, max_depth: usize) -> Result<CValue, CanonError> {
    let max_depth = max_depth.min(MAX_DEPTH);
    if input.len() > max_bytes.min(MAX_INPUT_BYTES) {
        return Err(CanonError::InputLimit);
    }
    let text = std::str::from_utf8(input).map_err(|_| CanonError::InvalidUtf8)?;
    let mut p = Parser { s: text.as_bytes(), text, pos: 0 };
    let mut stack: Vec<Frame> = Vec::new();
    p.ws();
    'value: loop {
        // Parse the start of a value.
        let mut v = match p.peek() {
            Some(b'{') => {
                p.pos += 1;
                if stack.len() >= max_depth {
                    return Err(CanonError::InputLimit);
                }
                p.ws();
                if p.peek() == Some(b'}') {
                    p.pos += 1;
                    CValue::Obj(BTreeMap::new())
                } else {
                    let key = p.key()?;
                    stack.push(Frame::Obj(BTreeMap::new(), key));
                    p.ws();
                    continue 'value;
                }
            }
            Some(b'[') => {
                p.pos += 1;
                if stack.len() >= max_depth {
                    return Err(CanonError::InputLimit);
                }
                p.ws();
                if p.peek() == Some(b']') {
                    p.pos += 1;
                    CValue::Arr(Vec::new())
                } else {
                    stack.push(Frame::Arr(Vec::new()));
                    continue 'value;
                }
            }
            _ => p.scalar()?,
        };
        // Attach the finished value to its parent, closing frames as needed.
        loop {
            p.ws();
            match stack.last_mut() {
                None => {
                    if p.pos != p.s.len() {
                        return Err(CanonError::Syntax(p.pos));
                    }
                    return Ok(v);
                }
                Some(Frame::Arr(items)) => {
                    items.push(v);
                    match p.bump() {
                        Some(b',') => {
                            p.ws();
                            continue 'value;
                        }
                        Some(b']') => match stack.pop() {
                            Some(Frame::Arr(items)) => v = CValue::Arr(items),
                            _ => return Err(CanonError::Syntax(p.pos)),
                        },
                        _ => return Err(CanonError::Syntax(p.pos)),
                    }
                }
                Some(Frame::Obj(map, key)) => {
                    let k = std::mem::take(key);
                    if map.contains_key(&k) {
                        return Err(CanonError::DuplicateKey(k));
                    }
                    map.insert(k, v);
                    match p.bump() {
                        Some(b',') => {
                            p.ws();
                            *key = p.key()?;
                            p.ws();
                            continue 'value;
                        }
                        Some(b'}') => match stack.pop() {
                            Some(Frame::Obj(map, _)) => v = CValue::Obj(map),
                            _ => return Err(CanonError::Syntax(p.pos)),
                        },
                        _ => return Err(CanonError::Syntax(p.pos)),
                    }
                }
            }
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    text: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn lit(&mut self, word: &str, v: CValue) -> Result<CValue, CanonError> {
        if self.s[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(v)
        } else {
            Err(CanonError::Syntax(self.pos))
        }
    }

    /// `"key"` followed by optional whitespace and `:`.
    fn key(&mut self) -> Result<String, CanonError> {
        if self.peek() != Some(b'"') {
            return Err(CanonError::Syntax(self.pos));
        }
        let k = self.string()?;
        self.ws();
        if self.bump() != Some(b':') {
            return Err(CanonError::Syntax(self.pos));
        }
        self.ws();
        Ok(k)
    }

    fn scalar(&mut self) -> Result<CValue, CanonError> {
        match self.peek() {
            Some(b'"') => self.string().map(CValue::Str),
            Some(b't') => self.lit("true", CValue::Bool(true)),
            Some(b'f') => self.lit("false", CValue::Bool(false)),
            Some(b'n') => self.lit("null", CValue::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(CanonError::Syntax(self.pos)),
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> Result<CValue, CanonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(CanonError::Syntax(self.pos)),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return Err(CanonError::Syntax(self.pos));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(CanonError::Syntax(self.pos));
            }
        }
        let f: f64 = self.text[start..self.pos].parse().map_err(|_| CanonError::Syntax(start))?;
        if !f.is_finite() {
            return Err(CanonError::NonFiniteNumber);
        }
        Ok(CValue::Num(f))
    }

    fn hex4(&mut self) -> Result<u32, CanonError> {
        let end = self.pos + 4;
        let h = self.text.get(self.pos..end).ok_or(CanonError::Syntax(self.pos))?;
        if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CanonError::Syntax(self.pos));
        }
        let v = u32::from_str_radix(h, 16).map_err(|_| CanonError::Syntax(self.pos))?;
        self.pos = end;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, CanonError> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            let start = self.pos;
            while let Some(b) = self.peek() {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            out.push_str(&self.text[start..self.pos]);
            match self.bump() {
                Some(b'"') => return Ok(out),
                Some(b'\\') => {
                    let c = match self.bump() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                                    return Err(CanonError::Syntax(self.pos));
                                }
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(CanonError::Syntax(self.pos));
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            char::from_u32(cp).ok_or(CanonError::Syntax(self.pos))?
                        }
                        _ => return Err(CanonError::Syntax(self.pos)),
                    };
                    out.push(c);
                }
                _ => return Err(CanonError::Syntax(self.pos)),
            }
        }
    }
}

/// Parses JSON text and returns its canonical bytes.
pub fn canonicalize(input: &[u8]) -> Result<Vec<u8>, CanonError> {
    to_canonical_bytes(&parse(input)?)
}
