//! Instrument uncertainty models as versioned data (spec node `UNC.INST.SCHEMA`,
//! REQ-UNC.INST.SCHEMA-001 .. 006, REQ-UNC.INST-001, -004). Layer engine, outside the kernel.
//!
//! - [`model`]: the `instrument-model/v1` document, its bounded reader with typed errors
//!   naming the field, unit conversion to SI by one correctly rounded parse, the RFC 8785
//!   canonical form and its SHA-256 digest.
//! - [`range`]: `check_range(model, ctx) -> RangeVerdict`, which producers run before they
//!   evaluate a model; anything but `InRange` is a flag, never a licence to extrapolate.
//! - [`store`]: the imported models and the bindings of one branch, a derived view of the
//!   journal.
//! - [`command`]: `unc.import_instrument_model@1` (W2), `unc.show_instrument_model@1` (R0)
//!   and `unc.bind_instrument_model@1` (W2) with their CLI and MCP surfaces.
//! - [`event`]: the journal events. [`mod@replay`]: exact replay.
//!
//! # Declared, not measured (D-045)
//!
//! A model holds what its source states: specification terms, a maker's parameter set, or a
//! per-observation model. Its parameter values are claims carried as data, never
//! measurements this crate made, and its `source` is an untrusted citation that is stored and
//! never interpreted (CONSTITUTION C6).
//!
//! # Device bindings and HAL.SIM
//!
//! A device binding records the model digest, the digest of the canonical HAL
//! `DeviceInstance` and the `instrument-profile/v1` descriptor digest (`fid-hal-idl`). The
//! device record comes through the [`command::DeviceDirectory`] seam. The HAL simulators
//! (`HAL.SIM`) that should supply the descriptor of `sim-tracker-1` are not built yet; the
//! tests bind against a stand-in descriptor under `fixtures/unc/inst/standin/`.
//!
//! No floating point is used in a decision beyond the closed-bound comparisons of the range
//! check (D0), maps are `BTreeMap`, and nothing here panics.

pub mod command;
pub mod event;
pub mod model;
pub mod range;
pub mod replay;
pub mod store;

pub use command::{
    bind_instrument_model, cli_params, command_defs, declarations, import_instrument_model, invoke_cli, invoke_mcp,
    mcp_params, show_instrument_model, BindOutcome, BindParams, BindResult, DeviceDirectory, DeviceRecord,
    ImportOutcome, ImportParams, ImportResult, InstCall, InstCtx, InstJournal, ShowParams, ShowResult, TargetKind,
    SHOW_INSTRUMENT_MODEL,
};
pub use event::{
    BadEvent, BindEvent, DeviceFacts, ImportEvent, InstEvent, BIND_INSTRUMENT_MODEL, IMPORT_INSTRUMENT_MODEL,
};
pub use model::{InstrumentModel, ModelError, ModelKind, Param, RangeAxis, SiUnit, UntrustedString};
pub use range::{
    check_range, BoundSide, ContextError, NotCheckedReason, ObservationContext, Observed, RangeBound, RangeVerdict,
};
pub use replay::{replay, InstReplayed, ReplayedInstruments};
pub use store::{
    read_back, BindTarget, DeviceEvidence, InstrumentBinding, InstrumentStore, ModelSelector, StoredInstrumentModel,
};

use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Longest text echoed back inside an error, in bytes.
const MAX_ECHO: usize = 160;

pub(crate) fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default().to_owned()
}

/// Every refusal of the commands, each with a stable code. A refused command commits nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstError {
    /// Parameters break the command's schema or surface rules.
    BadParams { detail: &'static str },
    /// The class, author-kind or branch rule refuses the caller.
    PolicyRefused { reason: &'static str },
    /// The import handle could not be read.
    FileUnreadable { detail: String },
    /// The document is not a valid instrument model (`schema_invalid`, `missing_unit`,
    /// `invalid_range`, `missing_source`).
    Model(ModelError),
    /// The id is imported with another digest.
    VersionConflict { model: String, stored: Box<Digest>, given: Box<Digest> },
    /// No imported model has this id.
    ModelNotFound { model: String },
    /// A reference or stored bytes disagree with the stored digest.
    DigestMismatch { expected: Box<Digest>, found: Box<Digest> },
    /// The device directory has no such device.
    DeviceNotFound { device: String },
    /// The device directory could not answer.
    DeviceUnavailable { detail: String },
    /// The device's `instrument-profile/v1` descriptor is invalid (`fid-hal-idl` code).
    DescriptorInvalid { code: &'static str, detail: String },
    /// The blob store failed.
    BlobStore { detail: String },
    /// The journal did not commit the event; nothing was stored.
    Journal { detail: String },
    /// A broken invariant (a bug); nothing was committed.
    Internal { reason: &'static str },
}

impl From<ModelError> for InstError {
    fn from(e: ModelError) -> InstError {
        InstError::Model(e)
    }
}

impl InstError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            InstError::BadParams { .. } => "bad_params",
            InstError::PolicyRefused { .. } => "policy_refused",
            InstError::FileUnreadable { .. } => "file_unreadable",
            InstError::Model(e) => e.code(),
            InstError::VersionConflict { .. } => "version_conflict",
            InstError::ModelNotFound { .. } => "model_not_found",
            InstError::DigestMismatch { .. } => "digest_mismatch",
            InstError::DeviceNotFound { .. } => "device_not_found",
            InstError::DeviceUnavailable { .. } => "device_unavailable",
            InstError::DescriptorInvalid { .. } => "descriptor_invalid",
            InstError::BlobStore { .. } => "blob_store",
            InstError::Journal { .. } => "journal_failed",
            InstError::Internal { .. } => "internal",
        }
    }

    /// `digest_mismatch` with the expected and the found digest.
    pub fn mismatch(expected: Digest, found: Digest) -> InstError {
        InstError::DigestMismatch { expected: Box::new(expected), found: Box::new(found) }
    }

    pub(crate) fn bad(detail: &'static str) -> InstError {
        InstError::BadParams { detail }
    }

    /// The error as JSON: `{"code", "message"}`, plus `field` for a model error.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_owned(), CValue::Str(self.code().to_owned()));
        o.insert("message".to_owned(), CValue::Str(self.to_string()));
        if let InstError::Model(e) = self {
            o.insert("field".to_owned(), CValue::Str(e.field().to_owned()));
        }
        CValue::Obj(o)
    }
}

impl fmt::Display for InstError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            InstError::BadParams { detail } => write!(f, "{code}: {detail}"),
            InstError::PolicyRefused { reason } => write!(f, "{code}: {reason}"),
            InstError::FileUnreadable { detail }
            | InstError::DeviceUnavailable { detail }
            | InstError::BlobStore { detail }
            | InstError::Journal { detail } => write!(f, "{code}: {detail}"),
            InstError::Model(e) => write!(f, "{e}"),
            InstError::VersionConflict { model, stored, given } => {
                write!(f, "{code}: {model} is stored as {stored}, not {given}")
            }
            InstError::ModelNotFound { model } => write!(f, "{code}: {model}"),
            InstError::DigestMismatch { expected, found } => write!(f, "{code}: expected {expected}, found {found}"),
            InstError::DeviceNotFound { device } => write!(f, "{code}: {device}"),
            InstError::DescriptorInvalid { code: c, detail } => write!(f, "{code}: {c} {detail}"),
            InstError::Internal { reason } => write!(f, "{code}: {reason}"),
        }
    }
}

impl std::error::Error for InstError {}
