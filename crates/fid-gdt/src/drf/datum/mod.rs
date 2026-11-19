//! Datum feature and datum target definitions (spec node `GDT.DRF.DATUM`,
//! REQ-GDT.DRF.DATUM-001 .. 004).
//!
//! A user, or an adopted PMI proposal, declares a datum with `gdt.define_datum@1` (W2; CLI
//! `fid gdt define-datum`, MCP `gdt_define_datum`): its label, the measured feature it is
//! bound to with that feature's nominal CAD faces, the feature type, and optional datum
//! targets (point, line or area, stored as geometry references only). A DRF
//! (`GDT.DRF.BUILD`) can only be built from datums declared here.
//!
//! - [`types`]: [`DatumLabel`], [`LabelPattern`], [`RefId`], [`FeatureType`],
//!   [`TargetKind`], [`DatumTargetRef`] and [`DatumDefinition`]. Every field is private and
//!   every constructor applies the same rules as the loaders, so no value of these types can
//!   hold something a loader would refuse.
//! - [`model`]: [`DatumModel`], the plan's measured features, nominal faces and target
//!   geometry, the datums on the working branch and the agent overlay, the command
//!   [`DatumModel::define_datum`], and [`active_label_pattern`], which reads the label
//!   pattern from the plan's active defaults profile.
//! - [`event`]: the `gdt.define_datum@1` journal event (canonical JSON, strict reader).
//! - [`project`]: the `fid.gdt-datum-plan/1` project file (`.fidproj`).
//! - [`mod@replay`]: exact replay of the events (`IDENTICAL` when every re-derived event matches
//!   byte for byte).
//! - [`command`]: the CLI and MCP entry points and the registry definition.
//!
//! **Branches.** The branch is derived from the principal ([`Branch::for_principal`]): a
//! person writes and reads the working branch only, so agent work never reaches it; an agent
//! or a script writes an overlay that sees the working branch beneath it.
//!
//! **Labels.** A label is one to three ASCII capital letters, and must match the
//! `gdt.datum.label_pattern` of the plan's active defaults profile. Lower case, digits and
//! every non-ASCII character (including full-width and other look-alike letters) are
//! `bad_label`, so two accepted labels can never differ only in case or by a look-alike;
//! uniqueness is then exact comparison.
//!
//! No floating point is used anywhere (D0), and nothing here panics.

pub mod command;
pub mod event;
pub mod model;
pub mod project;
pub mod replay;
pub mod types;

pub use command::{command_def, invoke_cli, invoke_mcp, DEFINE_DATUM_CLI, DEFINE_DATUM_MCP};
pub use event::DatumEvent;
pub use model::{active_label_pattern, DatumModel, DefineRequest, Defined, TargetRequest, LABEL_PATTERN_KEY};
pub use project::{load_project, project_bytes, PROJECT_SCHEMA};
pub use replay::{replay, DatumReplayed};
pub use types::{DatumDefinition, DatumLabel, DatumTargetRef, FeatureType, LabelPattern, RefId, TargetKind};

use crate::chars::dprof::Branch;
use std::fmt;

/// Command ID of the define command.
pub const DEFINE_COMMAND: &str = "gdt.define_datum@1";

/// Longest text echoed back inside an error, in bytes.
const MAX_ECHO: usize = 160;

pub(crate) fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

/// Errors of datum definition. [`DatumError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatumError {
    /// `bad_label{reason}`: the label is empty, longer than three characters, holds anything
    /// but ASCII capital letters, or does not match the active label pattern.
    BadLabel {
        /// Static reason: `empty`, `too_long`, `not_ascii_capital` or `pattern`.
        reason: &'static str,
    },
    /// `label_pattern_unavailable{reason}`: no active defaults profile, or it sets no
    /// `gdt.datum.label_pattern` (fail closed: no datum can be declared without one).
    LabelPatternUnavailable {
        /// Static reason.
        reason: &'static str,
    },
    /// `label_pattern_invalid{reason}`: the profile's label pattern breaks its grammar.
    LabelPatternInvalid {
        /// Static reason.
        reason: &'static str,
    },
    /// `datum_label_exists{label}`: the label is already declared in the plan and `replace`
    /// was not given.
    DatumLabelExists {
        /// The label.
        label: String,
    },
    /// `unsupported_datum_feature_type{type}`: the type is unknown or not datum-capable.
    UnsupportedDatumFeatureType {
        /// The type as given (truncated).
        feature_type: String,
    },
    /// `feature_type_mismatch{feature}`: the declared type differs from the measured
    /// feature's type.
    FeatureTypeMismatch {
        /// The feature.
        feature: String,
    },
    /// `reference_missing{id}`: no feature, nominal face or target geometry with this ID.
    ReferenceMissing {
        /// The ID.
        id: String,
    },
    /// `face_not_on_feature{face}`: the face exists but is not a nominal face of the bound
    /// feature.
    FaceNotOnFeature {
        /// The face.
        face: String,
    },
    /// `datum_target_invalid{target}`: a datum target breaks a rule; [`DatumError::reason`]
    /// names which.
    DatumTargetInvalid {
        /// The target ID as given (truncated).
        target: String,
        /// Static reason.
        reason: &'static str,
    },
    /// `invalid_field{field}`: a parameter outside its grammar or limits;
    /// [`DatumError::reason`] names the rule.
    InvalidField {
        /// The field.
        field: &'static str,
        /// Static reason.
        reason: &'static str,
    },
    /// `bad_params{reason}`: CLI arguments or MCP parameters that are not a valid call.
    BadParams {
        /// Static reason.
        reason: &'static str,
    },
    /// `model_full{branch}`: the branch holds the most datums allowed.
    ModelFull {
        /// The branch.
        branch: Branch,
    },
    /// `project_invalid{reason}`: a project file breaks the format or a limit.
    ProjectInvalid {
        /// Static reason.
        reason: &'static str,
    },
    /// `event_invalid{reason}`: a journal event could not be read.
    Event {
        /// Static reason.
        reason: &'static str,
    },
}

impl DatumError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            DatumError::BadLabel { .. } => "bad_label",
            DatumError::LabelPatternUnavailable { .. } => "label_pattern_unavailable",
            DatumError::LabelPatternInvalid { .. } => "label_pattern_invalid",
            DatumError::DatumLabelExists { .. } => "datum_label_exists",
            DatumError::UnsupportedDatumFeatureType { .. } => "unsupported_datum_feature_type",
            DatumError::FeatureTypeMismatch { .. } => "feature_type_mismatch",
            DatumError::ReferenceMissing { .. } => "reference_missing",
            DatumError::FaceNotOnFeature { .. } => "face_not_on_feature",
            DatumError::DatumTargetInvalid { .. } => "datum_target_invalid",
            DatumError::InvalidField { .. } => "invalid_field",
            DatumError::BadParams { .. } => "bad_params",
            DatumError::ModelFull { .. } => "model_full",
            DatumError::ProjectInvalid { .. } => "project_invalid",
            DatumError::Event { .. } => "event_invalid",
        }
    }

    /// The static reason, where there is one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            DatumError::BadLabel { reason: r }
            | DatumError::LabelPatternUnavailable { reason: r }
            | DatumError::LabelPatternInvalid { reason: r }
            | DatumError::DatumTargetInvalid { reason: r, .. }
            | DatumError::InvalidField { reason: r, .. }
            | DatumError::BadParams { reason: r }
            | DatumError::ProjectInvalid { reason: r }
            | DatumError::Event { reason: r } => Some(r),
            _ => None,
        }
    }

    /// The field an `invalid_field` error names.
    pub fn field(&self) -> Option<&'static str> {
        match self {
            DatumError::InvalidField { field, .. } => Some(field),
            _ => None,
        }
    }

    pub(crate) fn invalid(field: &'static str, reason: &'static str) -> DatumError {
        DatumError::InvalidField { field, reason }
    }

    pub(crate) fn missing(id: &str) -> DatumError {
        DatumError::ReferenceMissing { id: echo(id) }
    }

    pub(crate) fn target(target: &str, reason: &'static str) -> DatumError {
        DatumError::DatumTargetInvalid { target: echo(target), reason }
    }
}

impl fmt::Display for DatumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DatumError::DatumLabelExists { label: s }
            | DatumError::UnsupportedDatumFeatureType { feature_type: s }
            | DatumError::FeatureTypeMismatch { feature: s }
            | DatumError::ReferenceMissing { id: s }
            | DatumError::FaceNotOnFeature { face: s } => write!(f, "{}{{{}}}", self.code(), s.escape_debug()),
            DatumError::DatumTargetInvalid { target, reason } => {
                write!(f, "{}{{{}:{reason}}}", self.code(), target.escape_debug())
            }
            DatumError::InvalidField { field, reason } => write!(f, "{}{{{field}:{reason}}}", self.code()),
            DatumError::ModelFull { branch } => write!(f, "{}{{{}}}", self.code(), branch.as_str()),
            other => match other.reason() {
                Some(r) => write!(f, "{}{{{r}}}", other.code()),
                None => f.write_str(other.code()),
            },
        }
    }
}

impl std::error::Error for DatumError {}
