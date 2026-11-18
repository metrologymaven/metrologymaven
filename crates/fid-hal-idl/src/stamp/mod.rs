//! Frozen v0 record schemas: the provenance stamp (`stamp/0`), the acquisition-batch
//! envelope (`acquisition-batch/0`) and the device-computed claim (`external-claim/0`).
//! Spec node `HAL.IDL.STAMP`.
//!
//! Validation only: choosing stamp values is `HAL.ACQ.STAMP`. One descriptor table per
//! record (see `desc`) drives both the validator and the generated JSON Schemas under
//! `schemas/hal/`, so the two cannot drift apart.
//!
//! Wire rules (JSON through `fid-canon`, so every record hashes canonically, RFC 8785):
//!
//! - every field is required; an absent optional value is spelled `null`, never omitted;
//! - unknown fields are refused at every level (`stamp_field_unknown`);
//! - `u64` and `i64` values are canonical decimal strings, the project's `fid-canon` rule;
//! - the one `f64` (the claim's SI quantity) is the bit-exact `"0x"` + 16 hex digits form of
//!   `HAL.IDL.UNITS`;
//! - lists of references and feature names are strictly ascending, so a record has exactly
//!   one spelling and parse followed by write gives the same canonical bytes;
//! - every string and list is bounded, and the input is limited to [`MAX_INPUT_BYTES`]
//!   bytes and [`MAX_DEPTH`] levels of nesting.
//!
//! Errors carry a stable code and the RFC 6901 JSON pointer of the first offending field.
//! "First" means: the `schema` field of an object, then the object's fields (declared and
//! unknown alike) in canonical (sorted) key order, depth first; cross-field rules (gap
//! ranges, envelope and stamp agreement, compatibility status and report digest, driver tier
//! and host digest) run after the whole record is structurally valid.

mod desc;
mod model;
mod text;

pub use model::{
    AcquisitionBatchEnvelope, AlgorithmSource, BatchRef, Calibration, CalibrationStatus, Capability, ClaimedQuantity,
    CompatStatus, DeviceIdentity, DriverIdentity, DriverTier, ExternalClaim, Gap, GapReason, ProvenanceStamp, Record,
    RecordKind, SeqRange, SourceKind, TimeInfo, Trust, UtcAnchor, WarmupState,
};
pub use text::UntrustedText;

use fid_canon::CValue;
use std::fmt;

/// Largest accepted record, in bytes.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
/// Deepest accepted nesting of arrays and objects.
pub const MAX_DEPTH: usize = 16;
/// Most gaps in one stamp or envelope.
pub const MAX_GAPS: usize = 1024;
/// Most entries in a list of references or feature names.
pub const MAX_LIST: usize = 64;
/// Longest device identity string, in Unicode scalar values.
pub const MAX_TEXT_CHARS: usize = 256;
/// Longest claim method description, in Unicode scalar values.
pub const MAX_METHOD_CHARS: usize = 4096;

/// What went wrong; [`StampErrorKind::code`] gives the stable wire code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StampErrorKind {
    /// A required field is absent (REQ-004).
    FieldMissing,
    /// A field the record type does not have (REQ-004).
    FieldUnknown,
    /// A value outside a closed enum (REQ-004).
    EnumInvalid,
    /// A sequence range whose end precedes its start, or a gap that is reversed, outside
    /// the range or out of order (REQ-004).
    SeqInvalid,
    /// A `schema` whose major version is not 0 (REQ-007).
    SchemaVersionUnsupported,
    /// A value of the wrong JSON type, outside its pattern or length bound, or a list that is
    /// too long or not strictly ascending. Not named by the leaf; see the module docs.
    ValueInvalid,
    /// An envelope field that differs from the same field of its stamp. Not named by the leaf.
    BatchMismatch,
    /// Input that is not JSON `fid-canon` accepts (syntax, invalid UTF-8, duplicate key,
    /// size or depth limit). Not named by the leaf.
    JsonInvalid,
}

impl StampErrorKind {
    /// Stable machine-readable code.
    pub fn code(self) -> &'static str {
        match self {
            StampErrorKind::FieldMissing => "stamp_field_missing",
            StampErrorKind::FieldUnknown => "stamp_field_unknown",
            StampErrorKind::EnumInvalid => "stamp_enum_invalid",
            StampErrorKind::SeqInvalid => "stamp_seq_invalid",
            StampErrorKind::SchemaVersionUnsupported => "schema_version_unsupported",
            StampErrorKind::ValueInvalid => "stamp_value_invalid",
            StampErrorKind::BatchMismatch => "stamp_batch_mismatch",
            StampErrorKind::JsonInvalid => "stamp_json_invalid",
        }
    }
}

/// A validation failure: a typed kind and the JSON pointer (RFC 6901) of the offending
/// field. The pointer of the whole record is the empty string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StampError {
    pub kind: StampErrorKind,
    pub pointer: String,
}

impl StampError {
    pub(crate) fn new(kind: StampErrorKind, pointer: &str) -> StampError {
        StampError { kind, pointer: pointer.to_string() }
    }

    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }
}

impl fmt::Display for StampError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {:?}", self.code(), self.pointer)
    }
}

impl std::error::Error for StampError {}

/// Longest unknown key kept in a pointer, in bytes.
const KEEP: usize = 64;

/// Appends one reference token to a JSON pointer (`~` as `~0`, `/` as `~1`). Keys longer
/// than [`KEEP`] bytes are cut at a char boundary so a hostile key cannot bloat the error.
pub(crate) fn child(ptr: &str, key: &str) -> String {
    let mut end = key.len().min(KEEP);
    while !key.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(ptr.len() + end + 1);
    out.push_str(ptr);
    out.push('/');
    for c in key[..end].chars() {
        match c {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            c => out.push(c),
        }
    }
    out
}

/// Parses and validates one v0 record of any of the three kinds, chosen by its `schema`
/// field. Fails closed with the first offending field.
pub fn validate(input: &[u8]) -> Result<Record, StampError> {
    let value = fid_canon::parse_with_limits(input, MAX_INPUT_BYTES, MAX_DEPTH)
        .map_err(|_| StampError::new(StampErrorKind::JsonInvalid, ""))?;
    validate_value(value)
}

/// [`validate`] for an already parsed value.
pub fn validate_value(value: CValue) -> Result<Record, StampError> {
    let kind = desc::record_kind(&value)?;
    desc::check(&desc::Ty::Obj(kind.descriptor_fields()), &value, "")?;
    let record = match kind {
        RecordKind::Stamp => Record::Stamp(ProvenanceStamp::read(value, "")?),
        RecordKind::Batch => Record::Batch(AcquisitionBatchEnvelope::read(value, "")?),
        RecordKind::Claim => Record::Claim(ExternalClaim::read(value, "")?),
    };
    record.semantic()?;
    Ok(record)
}

/// The JSON Schema (draft 2020-12) of a record kind, as committed under `schemas/hal/`:
/// sorted keys, two-space indent, trailing newline.
pub fn json_schema(kind: RecordKind) -> Result<String, StampError> {
    let doc = desc::schema_document(kind);
    let mut out = String::new();
    desc::pretty(&doc, 0, &mut out)?;
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_tokens_are_escaped_and_bounded() {
        assert_eq!(child("", "a/b~c"), "/a~1b~0c");
        assert_eq!(child("/x", "0"), "/x/0");
        let long = "é".repeat(100);
        let p = child("", &long);
        assert!(p.len() <= KEEP + 1 && p.len() > 1);
    }

    #[test]
    fn codes_are_stable() {
        let all = [
            (StampErrorKind::FieldMissing, "stamp_field_missing"),
            (StampErrorKind::FieldUnknown, "stamp_field_unknown"),
            (StampErrorKind::EnumInvalid, "stamp_enum_invalid"),
            (StampErrorKind::SeqInvalid, "stamp_seq_invalid"),
            (StampErrorKind::SchemaVersionUnsupported, "schema_version_unsupported"),
            (StampErrorKind::ValueInvalid, "stamp_value_invalid"),
            (StampErrorKind::BatchMismatch, "stamp_batch_mismatch"),
            (StampErrorKind::JsonInvalid, "stamp_json_invalid"),
        ];
        for (k, c) in all {
            assert_eq!(k.code(), c);
        }
    }

    #[test]
    fn oversized_input_is_refused() {
        let big = vec![b' '; MAX_INPUT_BYTES + 1];
        assert_eq!(validate(&big).map_err(|e| e.code()).err(), Some("stamp_json_invalid"));
    }
}
