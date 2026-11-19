//! Typed errors of the report document model. Every variant has a stable `report.*` code; a
//! failed command journals nothing and changes nothing.

use super::kind::PayloadError;
use super::model::BlockKindId;
use super::value::{labels_value, ValueLabel};
use fid_canon::CValue;
use fid_project::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Why a user string was refused (`report.text_invalid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFault {
    /// Longer than [`super::MAX_TEXT_BYTES`] bytes of UTF-8.
    TooLong,
    /// Contains U+0000.
    Nul,
}

impl TextFault {
    pub fn as_str(self) -> &'static str {
        match self {
            TextFault::TooLong => "too_long",
            TextFault::Nul => "nul",
        }
    }
}

/// Errors of the report commands, the model and the storage reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportError {
    /// `report.not_found`: the named report, page or block does not exist (REQ-006).
    NotFound(Uuid),
    /// `report.page_not_empty`: the page holds blocks and `force` is false (REQ-005).
    PageNotEmpty(Uuid),
    /// `report.unknown_block_kind`: the kind is not registered (REQ-007). Holds the kind ID
    /// only when it is well formed, so hostile text never reaches the error.
    UnknownBlockKind(Option<BlockKindId>),
    /// `report.invalid_block_payload`: the payload fails the kind's check or the payload
    /// bounds (REQ-007).
    InvalidBlockPayload(PayloadError),
    /// `report.value_text_invalid`: a declared value field does not hold a `value_text@1`
    /// (`field` is the value field's JSON pointer).
    ValueTextInvalid(PayloadError),
    /// `report.value_label_mismatch`: the block's labels are not exactly the labels its
    /// values require; a claimed value can never be stored unlabelled.
    ValueLabelMismatch { missing: BTreeSet<ValueLabel>, extra: BTreeSet<ValueLabel> },
    /// `report.text_invalid`: a user string is too long or holds NUL (REQ-010).
    TextInvalid(TextFault),
    /// `report.bad_params`: parameters of the wrong shape (unknown key, missing field, wrong
    /// type, a non-canonical ID, an unknown command).
    BadParams(&'static str),
    /// `report.id_collision`: a freshly drawn ID is already live or retired in the report or
    /// the project. Nothing is retried, so a broken ID source fails closed (REQ-004).
    IdCollision(Uuid),
    /// `report.id_unavailable`: the clock or random source could not supply a UUIDv7.
    IdUnavailable(&'static str),
    /// `report.limit_exceeded`: a size bound of the model would be exceeded.
    LimitExceeded(&'static str),
    /// `report.store_invalid`: a stored document or journal event is not a valid canonical
    /// report (wrong digest, non-canonical bytes, unknown field, broken invariant).
    StoreInvalid(&'static str),
    /// `report.backend`: the project backend refused the commit or the read.
    Backend(&'static str),
}

impl ReportError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            ReportError::NotFound(_) => "report.not_found",
            ReportError::PageNotEmpty(_) => "report.page_not_empty",
            ReportError::UnknownBlockKind(_) => "report.unknown_block_kind",
            ReportError::InvalidBlockPayload(_) => "report.invalid_block_payload",
            ReportError::ValueTextInvalid(_) => "report.value_text_invalid",
            ReportError::ValueLabelMismatch { .. } => "report.value_label_mismatch",
            ReportError::TextInvalid(_) => "report.text_invalid",
            ReportError::BadParams(_) => "report.bad_params",
            ReportError::IdCollision(_) => "report.id_collision",
            ReportError::IdUnavailable(_) => "report.id_unavailable",
            ReportError::LimitExceeded(_) => "report.limit_exceeded",
            ReportError::StoreInvalid(_) => "report.store_invalid",
            ReportError::Backend(_) => "report.backend",
        }
    }

    /// The `--json` / MCP error object: `{"code": ..., <detail>}`. Details are IDs, fixed
    /// reason words or well-formed kind IDs only, never user text.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code().to_string()));
        let mut put = |k: &str, v: CValue| {
            o.insert(k.to_string(), v);
        };
        let st = CValue::Str;
        match self {
            ReportError::NotFound(id) | ReportError::PageNotEmpty(id) | ReportError::IdCollision(id) => {
                put("id", st(id.to_string()))
            }
            ReportError::UnknownBlockKind(Some(k)) => put("kind", st(k.as_str().to_string())),
            ReportError::UnknownBlockKind(None) => {}
            ReportError::InvalidBlockPayload(p) | ReportError::ValueTextInvalid(p) => {
                put("field", st(p.field().to_string()));
                put("reason", st(p.reason().to_string()));
            }
            ReportError::ValueLabelMismatch { missing, extra } => {
                put("missing", labels_value(missing));
                put("extra", labels_value(extra));
            }
            ReportError::TextInvalid(f) => put("reason", st(f.as_str().to_string())),
            ReportError::BadParams(r)
            | ReportError::IdUnavailable(r)
            | ReportError::LimitExceeded(r)
            | ReportError::StoreInvalid(r)
            | ReportError::Backend(r) => put("reason", st((*r).to_string())),
        }
        CValue::Obj(o)
    }
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReportError::NotFound(id) | ReportError::PageNotEmpty(id) | ReportError::IdCollision(id) => {
                write!(f, "{}: {id}", self.code())
            }
            ReportError::UnknownBlockKind(Some(k)) => write!(f, "{}: {}", self.code(), k.as_str()),
            ReportError::UnknownBlockKind(None) => f.write_str(self.code()),
            ReportError::InvalidBlockPayload(p) | ReportError::ValueTextInvalid(p) => {
                write!(f, "{}: {}: {}", self.code(), p.field(), p.reason())
            }
            ReportError::ValueLabelMismatch { missing, extra } => {
                let names = |s: &BTreeSet<ValueLabel>| s.iter().map(|l| l.as_str()).collect::<Vec<_>>().join(",");
                write!(f, "{}: missing [{}] extra [{}]", self.code(), names(missing), names(extra))
            }
            ReportError::TextInvalid(t) => write!(f, "{}: {}", self.code(), t.as_str()),
            ReportError::BadParams(r)
            | ReportError::IdUnavailable(r)
            | ReportError::LimitExceeded(r)
            | ReportError::StoreInvalid(r)
            | ReportError::Backend(r) => write!(f, "{}: {r}", self.code()),
        }
    }
}

impl std::error::Error for ReportError {}
