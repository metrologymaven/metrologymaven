//! Errors of the template commands. Every variant has a stable code; a failed command stores
//! nothing and journals nothing.

use crate::doc::ReportError;
use crate::tmpl::format::{TemplateError, TemplateId};
use crate::tmpl::slot::SlotError;
use fid_canon::CValue;
use fid_project::Uuid;
use std::collections::BTreeMap;
use std::fmt;

/// Errors of `report.save_template@1`, `report.apply_template@1`, `report.show_template@1`
/// and `report.list_template@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerError {
    /// `report.template_locked`: the template ID belongs to a built-in with `locked = true`,
    /// which may be applied but never saved over (REQ-003).
    Locked(TemplateId),
    /// `report.template_invalid` or `report.template_schema_unsupported`: the template failed
    /// the format check (`RPT.MODEL.TMPL.FORMAT`) or one of this leaf's own checks
    /// (`template_id_mismatch`, `label_control`).
    Template(TemplateError),
    /// `report.slot_invalid`: a slot failed `RPT.MODEL.TMPL.SLOT`.
    Slot(SlotError),
    /// `conflict/head_moved`: another save of this template ID was committed between this
    /// save's read and its commit; nothing was stored or journaled. Retry the save.
    HeadMoved(TemplateId),
    /// `report.not_found`: no such template, template version or report.
    NotFound { id: Uuid, version: Option<u32> },
    /// A refusal of the import-root resolver (`raw_path_forbidden`, `path_outside_root`,
    /// `root_not_found`, `io`, ...), or `unreadable`. The code is the resolver's own.
    File(&'static str),
    /// A report-model error: `report.unknown_block_kind`, `report.invalid_block_payload`,
    /// `report.value_literal_refused`, `report.value_binding_invalid` (REQ-008),
    /// `report.bad_params`, `report.id_collision`, `report.limit_exceeded`,
    /// `report.store_invalid`, `report.backend`, ...
    Report(ReportError),
}

impl VerError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            VerError::Locked(_) => "report.template_locked",
            VerError::HeadMoved(_) => "conflict/head_moved",
            VerError::Template(e) => e.code(),
            VerError::Slot(e) => e.code(),
            VerError::NotFound { .. } => "report.not_found",
            VerError::File(code) => code,
            VerError::Report(e) => e.code(),
        }
    }

    /// The `--json` / MCP error object. Details are IDs, versions and fixed reason words;
    /// pointers built from input keys are marked untrusted by the inner errors.
    pub fn to_value(&self) -> CValue {
        let base = |pairs: Vec<(&str, CValue)>| {
            let mut o: BTreeMap<String, CValue> = pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
            o.insert("code".to_string(), CValue::Str(self.code().to_string()));
            CValue::Obj(o)
        };
        match self {
            VerError::Locked(id) | VerError::HeadMoved(id) => {
                base(vec![("template", CValue::Str(id.uuid().to_string()))])
            }
            VerError::Template(e) => e.to_value(),
            VerError::Slot(e) => e.to_value(),
            VerError::NotFound { id, version } => {
                let mut v = vec![("id", CValue::Str(id.to_string()))];
                if let Some(n) = version {
                    v.push(("version", CValue::Str(n.to_string())));
                }
                base(v)
            }
            VerError::File(_) => base(Vec::new()),
            VerError::Report(e) => e.to_value(),
        }
    }
}

impl From<ReportError> for VerError {
    fn from(e: ReportError) -> VerError {
        match e {
            ReportError::NotFound(id) => VerError::NotFound { id, version: None },
            other => VerError::Report(other),
        }
    }
}

impl From<TemplateError> for VerError {
    fn from(e: TemplateError) -> VerError {
        VerError::Template(e)
    }
}

impl From<SlotError> for VerError {
    fn from(e: SlotError) -> VerError {
        VerError::Slot(e)
    }
}

impl fmt::Display for VerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerError::Locked(id) | VerError::HeadMoved(id) => write!(f, "{}: {}", self.code(), id.uuid()),
            VerError::Template(e) => fmt::Display::fmt(e, f),
            VerError::Slot(e) => fmt::Display::fmt(e, f),
            VerError::NotFound { id, version: Some(n) } => write!(f, "{}: {id}@{n}", self.code()),
            VerError::NotFound { id, version: None } => write!(f, "{}: {id}", self.code()),
            VerError::File(code) => f.write_str(code),
            VerError::Report(e) => fmt::Display::fmt(e, f),
        }
    }
}

impl std::error::Error for VerError {}
