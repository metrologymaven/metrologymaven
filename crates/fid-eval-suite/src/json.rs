//! Bounded JSON parsing and an error-collecting field reader.

use crate::{ErrorCode, FormatError, MAX_DEPTH, MAX_FILE_BYTES};
use fid_canon::{CValue, CanonError};
use fid_types::Digest;
use std::collections::BTreeMap;

pub(crate) type Map = BTreeMap<String, CValue>;

/// Parses `bytes` with the format limits. Syntax errors carry `code`.
pub(crate) fn parse_bounded(bytes: &[u8], file: &str, code: ErrorCode) -> Result<CValue, FormatError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(FormatError::new(ErrorCode::InputLimit, file, "", "file larger than 64 MiB"));
    }
    fid_canon::parse_with_limits(bytes, MAX_FILE_BYTES, MAX_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => {
            FormatError::new(ErrorCode::InputLimit, file, "", "file larger than 64 MiB or JSON nesting deeper than 64")
        }
        CanonError::DuplicateKey(k) => FormatError::new(ErrorCode::DuplicateKey, file, &k, "duplicate key"),
        other => FormatError::new(code, file, "", format!("not valid JSON: {other}")),
    })
}

/// `parent.key`, or `key` at the top level.
pub(crate) fn join(parent: &str, key: &str) -> String {
    if parent.is_empty() {
        key.to_string()
    } else {
        format!("{parent}.{key}")
    }
}

pub(crate) fn index(parent: &str, i: usize) -> String {
    format!("{parent}[{i}]")
}

/// Collects every field error of one file.
pub(crate) struct Ctx<'a> {
    code: ErrorCode,
    file: &'a str,
    errors: Vec<FormatError>,
}

impl<'a> Ctx<'a> {
    pub(crate) fn new(code: ErrorCode, file: &'a str) -> Ctx<'a> {
        Ctx { code, file, errors: Vec::new() }
    }

    pub(crate) fn err(&mut self, field: &str, detail: impl Into<String>) {
        self.errors.push(FormatError::new(self.code, self.file, field, detail));
    }

    pub(crate) fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// The collected errors, sorted by field, or a fallback error if there are none.
    pub(crate) fn finish(mut self) -> Vec<FormatError> {
        if self.errors.is_empty() {
            self.err("", "incomplete value");
        }
        self.errors.sort_by(|a, b| (&a.field, &a.detail).cmp(&(&b.field, &b.detail)));
        self.errors
    }

    /// An object with exactly the `allowed` keys, every `required` one present.
    pub(crate) fn obj<'v>(
        &mut self,
        v: Option<&'v CValue>,
        field: &str,
        allowed: &[&str],
        required: &[&str],
    ) -> Option<&'v Map> {
        let map = match v? {
            CValue::Obj(m) => m,
            _ => {
                self.err(field, "expected an object");
                return None;
            }
        };
        for k in map.keys() {
            if !allowed.contains(&k.as_str()) {
                self.err(&join(field, k), "unknown field");
            }
        }
        for k in required {
            if !map.contains_key(*k) {
                self.err(&join(field, k), "missing");
            }
        }
        Some(map)
    }

    pub(crate) fn arr<'v>(&mut self, v: Option<&'v CValue>, field: &str) -> Option<&'v [CValue]> {
        match v? {
            CValue::Arr(a) => Some(a),
            _ => {
                self.err(field, "expected an array");
                None
            }
        }
    }

    pub(crate) fn string<'v>(&mut self, v: Option<&'v CValue>, field: &str) -> Option<&'v str> {
        match v? {
            CValue::Str(s) => Some(s),
            _ => {
                self.err(field, "expected a string");
                None
            }
        }
    }

    /// A one-line label: 1 to 256 characters, no control characters.
    pub(crate) fn text(&mut self, v: Option<&CValue>, field: &str) -> Option<String> {
        let s = self.string(v, field)?;
        let n = s.chars().count();
        if n == 0 || n > 256 || s.chars().any(|c| c < ' ' || c == '\u{7f}') {
            self.err(field, "expected 1 to 256 characters without control characters");
            return None;
        }
        Some(s.to_string())
    }

    pub(crate) fn digest(&mut self, v: Option<&CValue>, field: &str) -> Option<Digest> {
        let s = self.string(v, field)?;
        match Digest::parse(s) {
            Ok(d) => Some(d),
            Err(_) => {
                self.err(field, "expected sha256: followed by 64 lowercase hex digits");
                None
            }
        }
    }

    /// A positive integer that fits in u32.
    pub(crate) fn pos_u32(&mut self, v: Option<&CValue>, field: &str) -> Option<u32> {
        match v? {
            // In range, and an integer: the truncated value round-trips bit for bit.
            CValue::Num(f) if *f >= 1.0 && *f <= f64::from(u32::MAX) && f64::from(*f as u32).to_bits() == f.to_bits() => {
                Some(*f as u32)
            }
            _ => {
                self.err(field, "expected an integer from 1 to 4294967295");
                None
            }
        }
    }
}

/// `[A-Za-z0-9][A-Za-z0-9._-]{0,63}`: task IDs and suite names.
pub(crate) fn is_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}
