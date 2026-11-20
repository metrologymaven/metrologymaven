//! Typed errors of binding and resolution. Every variant has a stable `report.*` code; a
//! failed bind journals nothing, and resolution never journals anything.
//!
//! Details are IDs, payload pointers and fixed reason words only. In particular
//! `report.cross_project_binding` names the block and the foreign project and nothing else:
//! no record content, digest, journal position or existence fact of the other project.

use crate::doc::{BlockId, BlockKindId, PayloadError, ReportError};
use fid_canon::CValue;
use fid_project::Uuid;
use std::collections::BTreeMap;
use std::fmt;

/// Errors of `report.bind_block@1` and of resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    /// An error of the document model (`report.not_found` for a report or block,
    /// `report.value_binding_invalid`, `report.store_invalid`, ...), with its own code.
    Report(ReportError),
    /// `report.binding_has_value`: a reference carries something besides its kind and ID,
    /// or the payload to store holds a JSON number (REQ-001). `field` is the pointer.
    BindingHasValue(PayloadError),
    /// `report.not_found`: a referenced entity does not exist at the current head
    /// (REQ-006), or a field no source supplies, or an unknown journal head.
    NotFound(String),
    /// `report.unjournaled_record`: the record is not committed by the given head (REQ-003).
    UnjournaledRecord { block: BlockId, record: String },
    /// `report.cross_project_binding`: a referenced entity belongs to another project
    /// (REQ-004). Holds the block and the foreign project only.
    CrossProjectBinding { block: BlockId, project: Uuid },
    /// `report.no_resolver`: no [`super::BlockResolver`] for the block's kind (REQ-005).
    NoResolver(BlockKindId),
    /// `report.binding_digest_mismatch`: a recomputed record or statement digest differs
    /// from the binding, or the statement is not the one the record references for the
    /// field (REQ-RPT.MODEL.BIND-010). `binding` is the value field's pointer and `digest`
    /// names which digest (`record_digest` or `statement_digest`).
    BindingDigestMismatch { block: BlockId, binding: String, digest: &'static str },
    /// `report.binding_field_not_value`: the binding's `field` is not one of the record's
    /// value fields (REQ-RPT.MODEL.BIND-010).
    BindingFieldNotValue { block: BlockId, binding: String },
    /// `report.policy`: the caller may not write on this branch (agents write to their agent
    /// branch only; a person never writes on an agent branch).
    Policy(&'static str),
    /// `report.bad_params`: parameters of the wrong shape.
    BadParams(&'static str),
    /// `report.store_invalid`: a stored binding, record or statement is malformed.
    StoreInvalid(&'static str),
}

impl From<ReportError> for BindError {
    fn from(e: ReportError) -> BindError {
        BindError::Report(e)
    }
}

impl BindError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            BindError::Report(e) => e.code(),
            BindError::BindingHasValue(_) => "report.binding_has_value",
            BindError::NotFound(_) => "report.not_found",
            BindError::UnjournaledRecord { .. } => "report.unjournaled_record",
            BindError::CrossProjectBinding { .. } => "report.cross_project_binding",
            BindError::NoResolver(_) => "report.no_resolver",
            BindError::BindingDigestMismatch { .. } => "report.binding_digest_mismatch",
            BindError::BindingFieldNotValue { .. } => "report.binding_field_not_value",
            BindError::Policy(_) => "report.policy",
            BindError::BadParams(_) => "report.bad_params",
            BindError::StoreInvalid(_) => "report.store_invalid",
        }
    }

    /// The `--json` / MCP error object: `{"code": ..., <detail>}`.
    pub fn to_value(&self) -> CValue {
        if let BindError::Report(e) = self {
            return e.to_value();
        }
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code().to_string()));
        let mut put = |k: &str, v: String| {
            o.insert(k.to_string(), CValue::Str(v));
        };
        match self {
            BindError::Report(_) => {}
            BindError::BindingHasValue(p) => {
                put("field", p.field().to_string());
                put("reason", p.reason().to_string());
            }
            BindError::NotFound(id) => put("id", id.clone()),
            BindError::UnjournaledRecord { block, record } => {
                put("block", block.uuid().to_string());
                put("record", record.clone());
            }
            BindError::CrossProjectBinding { block, project } => {
                put("block", block.uuid().to_string());
                put("project", project.to_string());
            }
            BindError::NoResolver(k) => put("kind", k.as_str().to_string()),
            BindError::BindingDigestMismatch { block, binding, digest } => {
                put("block", block.uuid().to_string());
                put("binding", binding.clone());
                put("digest", (*digest).to_string());
            }
            BindError::BindingFieldNotValue { block, binding } => {
                put("block", block.uuid().to_string());
                put("binding", binding.clone());
            }
            BindError::Policy(r) | BindError::BadParams(r) | BindError::StoreInvalid(r) => {
                put("reason", (*r).to_string())
            }
        }
        CValue::Obj(o)
    }
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BindError::Report(e) => e.fmt(f),
            BindError::BindingHasValue(p) => write!(f, "{}: {}: {}", self.code(), p.field(), p.reason()),
            BindError::NotFound(id) => write!(f, "{}: {id}", self.code()),
            BindError::UnjournaledRecord { block, record } => {
                write!(f, "{}: block {} record {record}", self.code(), block.uuid())
            }
            BindError::CrossProjectBinding { block, project } => {
                write!(f, "{}: block {} project {project}", self.code(), block.uuid())
            }
            BindError::NoResolver(k) => write!(f, "{}: {}", self.code(), k.as_str()),
            BindError::BindingDigestMismatch { block, binding, digest } => {
                write!(f, "{}: block {} binding {binding} ({digest})", self.code(), block.uuid())
            }
            BindError::BindingFieldNotValue { block, binding } => {
                write!(f, "{}: block {} binding {binding}", self.code(), block.uuid())
            }
            BindError::Policy(r) | BindError::BadParams(r) | BindError::StoreInvalid(r) => {
                write!(f, "{}: {r}", self.code())
            }
        }
    }
}

impl std::error::Error for BindError {}
