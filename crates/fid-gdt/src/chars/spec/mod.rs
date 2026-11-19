//! The governing spec of a characteristic: dialect, standard designators with editions and
//! one defaults profile, plus the gate every evaluation command calls first (spec node
//! `GDT.CHAR.SPEC`, REQ-GDT.CHAR-001, 002, 003).
//!
//! - [`types`]: [`GoverningSpec`], [`StdRef`] (`std:<asme|iso>:<designator>:<edition>`),
//!   [`Standards`], [`InheritedFrom`] and [`PlanId`]. Every field is private and every
//!   constructor applies the same rules as the loaders, so no value of these types can
//!   hold something a loader would refuse.
//! - [`state`]: [`SpecState`], the per-characteristic governing specs on the working branch
//!   and the agent overlay, the plan and tenant defaults, the command
//!   `gdt.set_governing_spec@1`, creation-time inheritance, the gate
//!   [`SpecState::require_governing_spec`] and [`SpecState::evaluate`], and the re-point
//!   wrapper that keeps governing specs and `GDT.CHAR.DPROF` bindings in step.
//! - [`event`]: the `gdt.set_governing_spec@1` journal event (canonical JSON, strict reader).
//! - [`project`]: the `fid.gdt-spec-project/1` project file (`.fidproj`): profile store plus
//!   governing specs and defaults.
//! - [`replay`]: exact replay of the events (`IDENTICAL` when every re-derived event matches
//!   byte for byte).
//!
//! The defaults profiles themselves, their store and their branch rules come from
//! [`super::dprof`]: a governing spec may only name a profile selectable on the principal's
//! branch, so an agent-branch profile is refused on the working branch until a human adopts
//! it. No floating point is used anywhere (D0), and nothing here panics.

pub mod event;
pub mod project;
pub mod replay;
pub mod state;
pub mod types;

pub use event::SpecEvent;
pub use project::{load_project, project_bytes, PROJECT_SCHEMA};
pub use replay::{replay, SpecReplayed};
pub use state::{Created, SetRequest, SetSpec, SpecState};
pub use types::{GoverningSpec, InheritedFrom, PlanId, Standards, StdRef};

use super::dprof::DprofError;
use std::fmt;

/// Command ID of the set command.
pub const SET_COMMAND: &str = "gdt.set_governing_spec@1";

/// Longest text echoed back inside an error, in bytes.
const MAX_ECHO: usize = 160;

pub(crate) fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

/// Errors of governing-spec handling. [`SpecError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    /// `governing_spec_missing{field}`: the spec, or one of its fields, is absent. `field` is
    /// the first missing one in the order `dialect`, `standards`, `defaults_profile`,
    /// `inherited_from`; a characteristic with no governing spec names `dialect`.
    GoverningSpecMissing {
        /// The missing field.
        field: &'static str,
    },
    /// `dialect_mismatch{item}`: a standard designator or the profile is of another dialect.
    DialectMismatch {
        /// The designator or profile reference (truncated).
        item: String,
    },
    /// `bad_std_ref{text}`: not `std:<asme|iso>:<designator>:<edition>`.
    BadStdRef {
        /// The text (truncated).
        text: String,
    },
    /// `bad_profile_ref`: the text is not a defaults-profile reference.
    BadProfileRef,
    /// `defaults_profile_unknown{ref}`: no profile with this ID is selectable on the branch.
    DefaultsProfileUnknown {
        /// The reference (truncated).
        reference: String,
    },
    /// `defaults_profile_digest_mismatch{ref}`: the store holds this profile version with
    /// another digest.
    DefaultsProfileDigestMismatch {
        /// The reference (truncated).
        reference: String,
    },
    /// `defaults_profile_edition_mismatch{ref}`: the profile's edition is not
    /// `<designator>-<edition>` of any listed standard.
    DefaultsProfileEditionMismatch {
        /// The reference (truncated).
        reference: String,
    },
    /// `defaults_profile_binding_mismatch{char}`: the profile store binds the characteristic
    /// to another profile than its governing spec names (the store was changed behind the
    /// governing spec's back).
    DefaultsProfileBindingMismatch {
        /// The characteristic.
        char: String,
    },
    /// `char_unknown{char}`: no such characteristic on this branch.
    CharUnknown {
        /// The characteristic.
        char: String,
    },
    /// `char_exists{char}`: the characteristic already exists.
    CharExists {
        /// The characteristic.
        char: String,
    },
    /// `char_case_conflict{char}`: another characteristic differs from this one only in
    /// ASCII case.
    CharCaseConflict {
        /// The characteristic as given.
        char: String,
    },
    /// `human_only`: only a human principal may do this.
    HumanOnly,
    /// `bad_request{reason}`: a malformed command parameter.
    BadRequest {
        /// Static reason.
        reason: &'static str,
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
    /// An error of the defaults-profile store (`GDT.CHAR.DPROF`).
    Dprof(DprofError),
}

impl SpecError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            SpecError::GoverningSpecMissing { .. } => "governing_spec_missing",
            SpecError::DialectMismatch { .. } => "dialect_mismatch",
            SpecError::BadStdRef { .. } => "bad_std_ref",
            SpecError::BadProfileRef => "bad_profile_ref",
            SpecError::DefaultsProfileUnknown { .. } => "defaults_profile_unknown",
            SpecError::DefaultsProfileDigestMismatch { .. } => "defaults_profile_digest_mismatch",
            SpecError::DefaultsProfileEditionMismatch { .. } => "defaults_profile_edition_mismatch",
            SpecError::DefaultsProfileBindingMismatch { .. } => "defaults_profile_binding_mismatch",
            SpecError::CharUnknown { .. } => "char_unknown",
            SpecError::CharExists { .. } => "char_exists",
            SpecError::CharCaseConflict { .. } => "char_case_conflict",
            SpecError::HumanOnly => "human_only",
            SpecError::BadRequest { .. } => "bad_request",
            SpecError::ProjectInvalid { .. } => "project_invalid",
            SpecError::Event { .. } => "event_invalid",
            SpecError::Dprof(e) => e.code(),
        }
    }

    /// The static reason or field name, where there is one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            SpecError::GoverningSpecMissing { field: r }
            | SpecError::BadRequest { reason: r }
            | SpecError::ProjectInvalid { reason: r }
            | SpecError::Event { reason: r } => Some(r),
            SpecError::Dprof(e) => e.reason(),
            _ => None,
        }
    }

    pub(crate) fn bad_request(reason: &'static str) -> SpecError {
        SpecError::BadRequest { reason }
    }

    pub(crate) fn project(reason: &'static str) -> SpecError {
        SpecError::ProjectInvalid { reason }
    }

    pub(crate) fn missing(field: &'static str) -> SpecError {
        SpecError::GoverningSpecMissing { field }
    }
}

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpecError::DialectMismatch { item: s }
            | SpecError::BadStdRef { text: s }
            | SpecError::DefaultsProfileUnknown { reference: s }
            | SpecError::DefaultsProfileDigestMismatch { reference: s }
            | SpecError::DefaultsProfileEditionMismatch { reference: s }
            | SpecError::DefaultsProfileBindingMismatch { char: s }
            | SpecError::CharUnknown { char: s }
            | SpecError::CharExists { char: s }
            | SpecError::CharCaseConflict { char: s } => write!(f, "{}{{{}}}", self.code(), s.escape_debug()),
            SpecError::Dprof(e) => write!(f, "{e}"),
            other => match other.reason() {
                Some(r) => write!(f, "{}{{{r}}}", other.code()),
                None => f.write_str(other.code()),
            },
        }
    }
}

impl std::error::Error for SpecError {}

impl From<DprofError> for SpecError {
    fn from(e: DprofError) -> Self {
        SpecError::Dprof(e)
    }
}
