//! Value fields hold bindings, never values.
//!
//! The document model never formats, computes or trusts a value or its labels. A block kind
//! declares where its values sit ([`super::BlockKind::value_fields`], RFC 6901 pointers),
//! and each declared field must hold a `value_binding@1`: a reference to a stored record
//! and its stored uncertainty statement, by ID and digest. `RPT.MODEL.BIND` resolves the
//! binding against the stored record and statement (refusing a digest mismatch), and
//! `RPT.MODEL.FMT` alone turns it into value text with its own per-value labels and side.
//!
//! `value_binding@1` (exactly these keys):
//!
//! ```text
//! { "schema": "value_binding@1",
//!   "record": <canonical lowercase UUID, not nil or max>,
//!   "record_digest": "sha256:<64 hex>",
//!   "field": <RFC 6901 pointer into the record, "" or "/...", at most 256 bytes>,
//!   "statement_digest": "sha256:<64 hex>" }
//! ```
//!
//! Anything else in a declared value field is refused:
//!
//! - a number, string (number-like or not), boolean, null or array, an object without a
//!   `schema` key, or one whose `schema` is not `value_binding@1` (a hand-typed
//!   `value_text@1` included) is a literal: `report.value_literal_refused`;
//! - an object tagged `value_binding@1` that breaks the shape is
//!   `report.value_binding_invalid`, with the reason.

use super::error::ReportError;
use super::kind::{BlockKind, PayloadError};
use super::model::{parse_canonical_uuid, CanonicalJson};
use super::text::must_escape;
use fid_canon::CValue;
use fid_project::Uuid;

/// Schema tag of a value binding.
pub const VALUE_BINDING_SCHEMA: &str = "value_binding@1";
/// Most value fields one block may declare.
pub const MAX_VALUE_FIELDS: usize = 4096;
/// Longest pointer (a declared value field, or a binding's `field`), in bytes.
pub const MAX_POINTER_BYTES: usize = 256;

/// Why a declared value field was refused. The field of the error is set by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingFault {
    /// Not a binding at all (`report.value_literal_refused`).
    Literal,
    /// Tagged as a binding but malformed (`report.value_binding_invalid`).
    Invalid(&'static str),
}

/// Whether `s` is a well-formed RFC 6901 pointer within the bounds: `""` or starting
/// with `/`, every `~` followed by `0` or `1`, at most [`MAX_POINTER_BYTES`] bytes, and no
/// character [`must_escape`] flags.
pub fn is_pointer(s: &str) -> bool {
    if s.len() > MAX_POINTER_BYTES || s.chars().any(must_escape) || !(s.is_empty() || s.starts_with('/')) {
        return false;
    }
    let b = s.as_bytes();
    b.iter().enumerate().all(|(i, c)| *c != b'~' || matches!(b.get(i + 1), Some(b'0' | b'1')))
}

fn digest_ok(v: Option<&CValue>) -> bool {
    matches!(v, Some(CValue::Str(s)) if fid_types::Digest::is_valid(s))
}

/// Checks one value field's content.
pub fn check_value_binding(v: &CValue) -> Result<(), BindingFault> {
    let CValue::Obj(o) = v else { return Err(BindingFault::Literal) };
    match o.get("schema") {
        Some(CValue::Str(s)) if s == VALUE_BINDING_SCHEMA => {}
        _ => return Err(BindingFault::Literal),
    }
    const KEYS: [&str; 5] = ["field", "record", "record_digest", "schema", "statement_digest"];
    if o.len() != KEYS.len() || KEYS.iter().any(|k| !o.contains_key(*k)) {
        return Err(BindingFault::Invalid("binding_keys"));
    }
    let record = match o.get("record") {
        Some(CValue::Str(s)) => parse_canonical_uuid(s),
        _ => None,
    };
    if !record.is_some_and(|r| r != Uuid::NIL && r != Uuid::MAX) {
        return Err(BindingFault::Invalid("record"));
    }
    if !digest_ok(o.get("record_digest")) {
        return Err(BindingFault::Invalid("record_digest"));
    }
    if !digest_ok(o.get("statement_digest")) {
        return Err(BindingFault::Invalid("statement_digest"));
    }
    match o.get("field") {
        Some(CValue::Str(f)) if is_pointer(f) => Ok(()),
        _ => Err(BindingFault::Invalid("field")),
    }
}

/// RFC 6901 JSON pointer lookup (`""` is the whole value; `~0` is `~`, `~1` is `/`).
pub fn resolve_pointer<'a>(root: &'a CValue, pointer: &str) -> Option<&'a CValue> {
    if !is_pointer(pointer) {
        return None;
    }
    if pointer.is_empty() {
        return Some(root);
    }
    let rest = pointer.strip_prefix('/')?;
    let mut at = root;
    for raw in rest.split('/') {
        let token = raw.replace("~1", "/").replace("~0", "~");
        at = match at {
            CValue::Obj(o) => o.get(&token)?,
            CValue::Arr(a) => {
                let canonical = token == "0"
                    || (!token.is_empty() && !token.starts_with('0') && token.bytes().all(|c| c.is_ascii_digit()));
                if !canonical {
                    return None;
                }
                a.get(token.parse::<usize>().ok()?)?
            }
            _ => return None,
        };
    }
    Some(at)
}

/// The value rule for one block: every declared value field exists and holds a
/// `value_binding@1`.
pub(crate) fn check_block_values(kind: &dyn BlockKind, payload: &CanonicalJson) -> Result<(), ReportError> {
    let fields = kind.value_fields(payload).map_err(ReportError::InvalidBlockPayload)?;
    if fields.len() > MAX_VALUE_FIELDS {
        return Err(ReportError::InvalidBlockPayload(PayloadError::new("", "too_many_value_fields")));
    }
    for f in &fields {
        if !is_pointer(f) {
            return Err(ReportError::InvalidBlockPayload(PayloadError::new(f, "bad_value_field_pointer")));
        }
        let v = resolve_pointer(payload.value(), f)
            .ok_or_else(|| ReportError::ValueBindingInvalid(PayloadError::new(f, "missing")))?;
        check_value_binding(v).map_err(|e| match e {
            BindingFault::Literal => ReportError::ValueLiteralRefused(PayloadError::new(f, "literal")),
            BindingFault::Invalid(r) => ReportError::ValueBindingInvalid(PayloadError::new(f, r)),
        })?;
    }
    Ok(())
}
