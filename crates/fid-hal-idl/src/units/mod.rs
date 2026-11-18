//! Quantities, declared encodings, frames and timestamp types. Spec node `HAL.IDL.UNITS`.
//!
//! Validation only: every type checks its invariants when it is built or read, and nothing
//! here converts payloads (that is `HAL.ACQ.CONV`) or estimates clocks (`HAL.ACQ.TIME`).
//!
//! Wire form (JSON through `fid-canon`, so it hashes canonically):
//!
//! - every `f64` (quantity values, encoding scale and offset, direction components) is
//!   written bit-exactly as `"0x"` + 16 lowercase hex digits of its IEEE 754 bits
//!   (`fid-canon` `F64Bits`). This keeps `-0.0`, keeps every bit of a declared scale, and
//!   lets a NaN or infinity on the wire be read and then rejected with its proper code;
//! - every `i64` (timestamps) is a decimal string (`fid-canon` `I64`);
//! - enums are their wire strings; frame IDs are lowercase hyphenated UUIDv5s.
//!
//! Readers refuse unknown fields and non-canonical spellings, so a value read and written
//! again gives the same canonical bytes.

mod direction;
mod encoding;
mod frame;
mod quantity;
mod time;

pub use direction::UnitVector3;
pub use encoding::{EncodingDescriptor, NumericType, RawValue};
pub use frame::{Frame, FrameEdge, FrameGraph, FrameId, FrameKind, MAX_EDGES, MAX_FRAMES, MAX_REF_BYTES};
pub use quantity::{Quantity, SiUnit};
pub use time::{TimeSource, Timestamp};

use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;

/// Errors with stable codes. The first five are the leaf's codes; the rest report a wire
/// value of the wrong shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitsError {
    /// A unit outside m, rad, s, K, Pa and `1` (REQ-001).
    UnitUnsupported(String),
    /// A NaN or infinite quantity value (REQ-001).
    QuantityNotFinite,
    /// An encoding descriptor breaks REQ-002 or REQ-003; the text says why.
    EncodingInvalid(String),
    /// A frame graph with a cycle, two parents for one frame, or an unknown frame (REQ-005).
    FrameGraphInvalid(String),
    /// A direction whose norm differs from 1 by more than 1e-9, or is not finite (REQ-006).
    DirectionNotUnit,
    /// A required field is absent.
    MissingField(&'static str),
    /// A field this type does not have.
    UnknownField(String),
    /// A field of the wrong JSON type.
    BadType { field: String, expected: &'static str },
    /// A field of the right JSON type with a value outside its domain.
    BadValue { field: &'static str, value: String },
    /// The bytes are not JSON `fid-canon` accepts; carries its error code.
    Json(&'static str),
}

impl UnitsError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            UnitsError::UnitUnsupported(_) => "unit_unsupported",
            UnitsError::QuantityNotFinite => "quantity_not_finite",
            UnitsError::EncodingInvalid(_) => "encoding_invalid",
            UnitsError::FrameGraphInvalid(_) => "frame_graph_invalid",
            UnitsError::DirectionNotUnit => "direction_not_unit",
            UnitsError::MissingField(_) => "missing_field",
            UnitsError::UnknownField(_) => "unknown_field",
            UnitsError::BadType { .. } => "bad_type",
            UnitsError::BadValue { .. } => "bad_value",
            UnitsError::Json(c) => c,
        }
    }
}

impl fmt::Display for UnitsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnitsError::UnitUnsupported(u) => write!(f, "unit_unsupported: {u:?}"),
            UnitsError::EncodingInvalid(why) => write!(f, "encoding_invalid: {why}"),
            UnitsError::FrameGraphInvalid(why) => write!(f, "frame_graph_invalid: {why}"),
            UnitsError::MissingField(k) => write!(f, "missing_field: {k}"),
            UnitsError::UnknownField(k) => write!(f, "unknown_field: {k:?}"),
            UnitsError::BadType { field, expected } => write!(f, "bad_type: {field} must be {expected}"),
            UnitsError::BadValue { field, value } => write!(f, "bad_value: {field} = {value:?}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for UnitsError {}

/// Parses JSON bytes with `fid-canon` (duplicate keys, size and depth limits enforced).
pub fn parse_json(input: &[u8]) -> Result<CValue, UnitsError> {
    fid_canon::parse(input).map_err(|e| UnitsError::Json(e.code()))
}

/// RFC 8785 canonical bytes of a wire value.
pub fn canonical_json(value: &CValue) -> Result<Vec<u8>, UnitsError> {
    fid_canon::to_canonical_bytes(value).map_err(|e| UnitsError::Json(e.code()))
}

// ------------------------------------------------------------------ wire helpers

/// Longest rejected input kept in an error, in bytes.
const KEEP: usize = 64;

/// Copies at most [`KEEP`] bytes of `s`, cut at a char boundary.
pub(crate) fn kept(s: &str) -> String {
    let mut end = s.len().min(KEEP);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

pub(crate) type Obj = BTreeMap<String, CValue>;

/// Takes an object whose keys are all in `allowed`.
pub(crate) fn object(v: CValue, what: &'static str, allowed: &[&str]) -> Result<Obj, UnitsError> {
    let CValue::Obj(o) = v else {
        return Err(UnitsError::BadType { field: what.to_string(), expected: "an object" });
    };
    if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(UnitsError::UnknownField(kept(k)));
    }
    Ok(o)
}

pub(crate) fn take(o: &mut Obj, field: &'static str) -> Result<CValue, UnitsError> {
    o.remove(field).ok_or(UnitsError::MissingField(field))
}

pub(crate) fn string(field: &'static str, v: CValue) -> Result<String, UnitsError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(UnitsError::BadType { field: field.to_string(), expected: "a string" }),
    }
}

pub(crate) fn boolean(field: &'static str, v: CValue) -> Result<bool, UnitsError> {
    match v {
        CValue::Bool(b) => Ok(b),
        _ => Err(UnitsError::BadType { field: field.to_string(), expected: "a boolean" }),
    }
}

/// Reads an `f64` written as `"0x"` + 16 lowercase hex digits (or an in-memory `F64Bits`).
/// The value may be NaN or infinite; the caller decides which code that earns.
pub(crate) fn f64_bits(field: &'static str, v: CValue) -> Result<f64, UnitsError> {
    let s = match v {
        CValue::F64Bits(f) => return Ok(f),
        CValue::Str(s) => s,
        _ => return Err(UnitsError::BadType { field: field.to_string(), expected: "an f64 bit string" }),
    };
    let bad = || UnitsError::BadValue { field, value: kept(&s) };
    let hex = s.strip_prefix("0x").ok_or_else(bad)?;
    if hex.len() != 16 || !hex.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
        return Err(bad());
    }
    let bits = u64::from_str_radix(hex, 16).map_err(|_| bad())?;
    Ok(f64::from_bits(bits))
}

/// Reads an `i64` written as a canonical decimal string (or an in-memory `I64`).
pub(crate) fn i64_dec(field: &'static str, v: CValue) -> Result<i64, UnitsError> {
    let s = match v {
        CValue::I64(i) => return Ok(i),
        CValue::Str(s) => s,
        _ => return Err(UnitsError::BadType { field: field.to_string(), expected: "a decimal i64 string" }),
    };
    let bad = || UnitsError::BadValue { field, value: kept(&s) };
    let digits = s.strip_prefix('-').unwrap_or(&s);
    let canonical = !digits.is_empty()
        && digits.bytes().all(|c| c.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && s != "-0";
    if !canonical {
        return Err(bad());
    }
    s.parse::<i64>().map_err(|_| bad())
}

/// Reads a bounded, non-empty reference string.
pub(crate) fn reference(field: &'static str, v: CValue, max: usize) -> Result<String, UnitsError> {
    let s = string(field, v)?;
    if s.is_empty() || s.len() > max || s.chars().any(char::is_control) {
        return Err(UnitsError::BadValue { field, value: kept(&s) });
    }
    Ok(s)
}

pub(crate) fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f64_bits_is_strict() {
        let ok = CValue::Str("0x3ff0000000000000".into());
        assert_eq!(f64_bits("v", ok), Ok(1.0));
        for s in ["0x3FF0000000000000", "3ff0000000000000", "0x3ff", "0x+3ff000000000000", "0x3ff00000000000000"] {
            assert_eq!(f64_bits("v", CValue::Str(s.into())).map_err(|e| e.code()), Err("bad_value"), "{s}");
        }
        assert_eq!(f64_bits("v", CValue::Num(1.0)).map_err(|e| e.code()), Err("bad_type"));
    }

    #[test]
    fn i64_dec_is_strict() {
        assert_eq!(i64_dec("t", CValue::Str("-9223372036854775808".into())), Ok(i64::MIN));
        assert_eq!(i64_dec("t", CValue::Str("0".into())), Ok(0));
        for s in ["", "-", "-0", "01", "+1", "1.0", "9223372036854775808", " 1"] {
            assert_eq!(i64_dec("t", CValue::Str(s.into())).map_err(|e| e.code()), Err("bad_value"), "{s:?}");
        }
    }

    #[test]
    fn kept_cuts_at_char_boundary() {
        let long = "é".repeat(100);
        let k = kept(&long);
        assert!(k.len() <= KEEP && long.starts_with(&k) && !k.is_empty());
    }
}
