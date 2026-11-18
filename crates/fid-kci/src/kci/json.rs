//! Kernel canonical JSON. Spec node `KRN.MAN.KCI` (REQ-KRN.MAN.KCI-007).
//!
//! RFC 8785 rules restricted to strings, arrays and objects: object keys sorted by UTF-16
//! code units, no insignificant whitespace, strings escaped as RFC 8785 §3.2.2.2 prescribes.
//! Numbers are never emitted: by kernel convention every numeric value is a string.

use std::cmp::Ordering;
use std::fmt;

/// A kernel JSON value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    Str(String),
    Arr(Vec<Json>),
    /// Members in any order; the writer sorts them.
    Obj(Vec<(String, Json)>),
}

/// Canonical JSON writer errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonError {
    /// An object has two members with the same key.
    DuplicateKey(String),
}

impl JsonError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            JsonError::DuplicateKey(_) => "duplicate_key",
        }
    }
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonError::DuplicateKey(k) => write!(f, "duplicate_key: {k:?}"),
        }
    }
}

impl std::error::Error for JsonError {}

/// Orders keys by their UTF-16 code units (RFC 8785 §3.2.3).
pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Writes `v` as canonical JSON (UTF-8 bytes).
pub fn to_canonical(v: &Json) -> Result<Vec<u8>, JsonError> {
    let mut out = String::new();
    write_value(&mut out, v)?;
    Ok(out.into_bytes())
}

/// Writes an object whose keys are fixed by the caller, already unique and in UTF-16 order
/// (kernel-owned schemas), so writing cannot fail.
pub(crate) fn fixed_object(members: &[(&str, Flat<'_>)]) -> Vec<u8> {
    debug_assert!(members.windows(2).all(|p| cmp_utf16(p[0].0, p[1].0) == Ordering::Less));
    let mut out = String::from("{");
    for (i, (k, v)) in members.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_string(&mut out, k);
        out.push(':');
        match v {
            Flat::Str(s) => write_string(&mut out, s),
            Flat::List(items) => {
                out.push('[');
                for (j, x) in items.iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    write_string(&mut out, x);
                }
                out.push(']');
            }
        }
    }
    out.push('}');
    out.into_bytes()
}

/// A member value of a fixed-schema object: a string or an array of strings.
pub(crate) enum Flat<'a> {
    Str(&'a str),
    List(&'a [String]),
}

fn write_value(out: &mut String, v: &Json) -> Result<(), JsonError> {
    match v {
        Json::Str(s) => write_string(out, s),
        Json::Arr(items) => {
            out.push('[');
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, x)?;
            }
            out.push(']');
        }
        Json::Obj(members) => {
            let mut sorted: Vec<&(String, Json)> = members.iter().collect();
            sorted.sort_by(|a, b| cmp_utf16(&a.0, &b.0));
            for pair in sorted.windows(2) {
                if pair[0].0 == pair[1].0 {
                    return Err(JsonError::DuplicateKey(pair[0].0.clone()));
                }
            }
            out.push('{');
            for (i, (k, x)) in sorted.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, k);
                out.push(':');
                write_value(out, x)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn write_string(out: &mut String, s: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let b = u32::from(c) as usize;
                out.push_str("\\u00");
                out.push(char::from(HEX[b >> 4]));
                out.push(char::from(HEX[b & 15]));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
