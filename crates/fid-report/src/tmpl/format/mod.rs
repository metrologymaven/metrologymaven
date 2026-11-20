//! The report-template file format. Spec node `RPT.MODEL.TMPL.FORMAT`.
//!
//! A template is a report document (`RPT.MODEL.DOC`) whose blocks may carry a slot instead
//! of bound stable IDs, plus template-level flags. It is exchanged as RFC 8785 canonical
//! JSON with media type [`MEDIA_TYPE`] and is a separate format, not a QIF extension;
//! templates name QIF and project entities only by stable ID, inside slots.
//!
//! Schema version 1 (exactly these keys everywhere; integers are decimal strings, so the
//! document holds no JSON number):
//!
//! ```text
//! { "clearance": null | {"state": "pending"} | {"review_ref": <untrusted text>, "state": "cleared"},
//!   "locked": <bool>,
//!   "media_type": "application/vnd.fid.report-template+json",
//!   "pages": [ { "blocks": [ { "continuation_block": null | <uuidv7 of a block on this page>,
//!                              "id": <uuidv7>, "kind": <kind id>, "kind_version": "<u32 >= 1>",
//!                              "payload": {"untrusted": true, "value": <template json>},
//!                              "repeat_header_rows": "0".."8",
//!                              "slot": null | {"kind": <slot kind>,
//!                                              "spec": {"untrusted": true, "value": <template json object>}} } ],
//!                "id": <uuidv7> } ],
//!   "schema_version": "1",
//!   "template_id": <uuidv7>,
//!   "title": {"text": <string>, "untrusted": true} }
//! ```
//!
//! "Template json" ([`TemplateJson`]) holds no JSON number and no object tagged
//! `value_binding@...` or `value_text@...`, so a template can never carry a bound or
//! measured value.
//!
//! [`parse_template`] runs, in order: the byte budget; the pre-scan budgets (depth, string
//! length, unique keys) in input byte order; `fid-canon`'s strict parse; the schema
//! version; migration ([`migrate`]); for a migrated value the pre-scan again over its
//! canonical bytes; the schema-version-1 check. Every failure is a [`TemplateError`].

mod error;
mod migrate;
mod model;
mod read;
mod scan;

pub use error::{TemplateError, MAX_ERROR_POINTER_BYTES};
pub use migrate::{migrate, V0_FORMAT};
pub use model::{Clearance, Slot, SlotKind, Template, TemplateBlock, TemplateId, TemplateJson, TemplatePage};

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use scan::Budgets;

/// Media type of the exchanged template file.
pub const MEDIA_TYPE: &str = "application/vnd.fid.report-template+json";
/// The current (and only written) schema version.
pub const SCHEMA_VERSION: u32 = 1;
/// Largest template file, in bytes.
pub const MAX_TEMPLATE_BYTES: usize = 16 * 1024 * 1024;
/// Deepest nesting: the root object is depth 1, a scalar adds nothing.
pub const MAX_TEMPLATE_DEPTH: usize = 32;
/// Most blocks over all pages.
pub const MAX_TEMPLATE_BLOCKS: usize = 10_000;
/// Most pages.
pub const MAX_PAGES: usize = 10_000;
/// Longest string anywhere (keys included), in bytes of decoded UTF-8.
pub const MAX_STRING_BYTES: usize = 64 * 1024;
/// Largest `repeat_header_rows`.
pub const MAX_REPEAT_HEADER_ROWS: u8 = 8;

const BUDGETS: Budgets = Budgets { max_depth: MAX_TEMPLATE_DEPTH, max_string: MAX_STRING_BYTES };

/// RFC 8785 canonical bytes of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalBytes(Vec<u8>);

impl CanonicalBytes {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }

    /// `sha256:` digest of the bytes.
    pub fn digest(&self) -> String {
        fid_canon::digest_bytes(&self.0).to_string()
    }
}

/// Parses a template file of any supported schema version into the current one.
pub fn parse_template(bytes: &[u8]) -> Result<Template, TemplateError> {
    if bytes.len() > MAX_TEMPLATE_BYTES {
        return Err(TemplateError::invalid("", "too_large"));
    }
    scan::scan(bytes, BUDGETS)?;
    // The scan already holds the depth to the budget; one more level of headroom here
    // keeps the scan the single authority on depth errors.
    let value =
        parse_with_limits(bytes, MAX_TEMPLATE_BYTES, MAX_TEMPLATE_DEPTH + 1).map_err(|e| canon_error(bytes, e))?;
    let version = schema_version(&value)?;
    if version == SCHEMA_VERSION {
        return read::template(&value);
    }
    let migrated = migrate(&value, version)?;
    let canonical = to_canonical_bytes(&migrated).map_err(|_| TemplateError::invalid("", "non_finite_number"))?;
    if canonical.len() > MAX_TEMPLATE_BYTES {
        return Err(TemplateError::invalid("", "too_large"));
    }
    scan::scan(&canonical, BUDGETS)?;
    read::template(&migrated)
}

/// The canonical bytes of `template` (always schema version 1).
pub fn serialise(template: &Template) -> Result<CanonicalBytes, TemplateError> {
    to_canonical_bytes(&template.to_value())
        .map(CanonicalBytes)
        .map_err(|_| TemplateError::invalid("", "non_finite_number"))
}

/// The declared schema version: `schema_version` of the root object, as a canonical
/// decimal string or a JSON integer.
fn schema_version(v: &CValue) -> Result<u32, TemplateError> {
    let CValue::Obj(o) = v else { return Err(TemplateError::invalid("", "wrong_type")) };
    let bad = || TemplateError::invalid("/schema_version", "bad_version");
    match o.get("schema_version") {
        None => Err(TemplateError::invalid("/schema_version", "missing_field")),
        Some(v @ CValue::Num(_)) => crate::json_num::as_u32(v).ok_or_else(bad),
        Some(s @ CValue::Str(_)) => read::dec_u32(s, "/schema_version").map_err(|_| bad()),
        Some(_) => Err(bad()),
    }
}

fn canon_error(bytes: &[u8], e: CanonError) -> TemplateError {
    match e {
        CanonError::Syntax(at) => TemplateError::invalid(&scan::pointer_at(bytes, at, BUDGETS), "syntax"),
        CanonError::InvalidUtf8 => {
            let at = std::str::from_utf8(bytes).err().map_or(0, |u| u.valid_up_to());
            TemplateError::invalid(&scan::pointer_at(bytes, at, BUDGETS), "invalid_utf8")
        }
        CanonError::DuplicateKey(_) => TemplateError::invalid("", "duplicate_key"),
        CanonError::NonFiniteNumber => TemplateError::invalid("", "non_finite_number"),
        CanonError::InputLimit => TemplateError::invalid("", "too_deep"),
    }
}
