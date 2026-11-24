//! Error codes and structured bus API errors mapped cleanly onto `AUTO.SURF.ENV` and
//! `CORE.BUS.PIPE.ERR`.

use crate::envelope::{BusEnvelope, BusOutcome};
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;

/// Maximum error message length retained in [`BusApiError`], in Unicode scalar values.
pub const MAX_ERROR_CHARS: usize = 2000;

/// Closed surface-facing error code set (mirrors `AUTO.SURF.ENV` `ErrorCode`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BusApiErrorCode {
    UsageError,
    ValidationError,
    UnitError,
    PolicyRefused,
    PreconditionFailed,
    Conflict,
    NotFound,
    VersionMismatch,
    RawPathRefused,
    RootEscape,
    DryRunUnsupported,
    Timeout,
    Cancelled,
    InternalError,
    Unknown(String),
}

impl BusApiErrorCode {
    /// Wire string representation.
    pub fn as_str(&self) -> &str {
        match self {
            BusApiErrorCode::UsageError => "usage_error",
            BusApiErrorCode::ValidationError => "validation_error",
            BusApiErrorCode::UnitError => "unit_error",
            BusApiErrorCode::PolicyRefused => "policy_refused",
            BusApiErrorCode::PreconditionFailed => "precondition_failed",
            BusApiErrorCode::Conflict => "conflict",
            BusApiErrorCode::NotFound => "not_found",
            BusApiErrorCode::VersionMismatch => "version_mismatch",
            BusApiErrorCode::RawPathRefused => "raw_path_refused",
            BusApiErrorCode::RootEscape => "root_escape",
            BusApiErrorCode::DryRunUnsupported => "dry_run_unsupported",
            BusApiErrorCode::Timeout => "timeout",
            BusApiErrorCode::Cancelled => "cancelled",
            BusApiErrorCode::InternalError => "internal_error",
            BusApiErrorCode::Unknown(s) => s.as_str(),
        }
    }

    /// Parses a wire error code string; unknown values are preserved as [`Self::Unknown`].
    pub fn from_wire(s: &str) -> Self {
        match s {
            "usage_error" => BusApiErrorCode::UsageError,
            "validation_error" => BusApiErrorCode::ValidationError,
            "unit_error" => BusApiErrorCode::UnitError,
            "policy_refused" => BusApiErrorCode::PolicyRefused,
            "precondition_failed" => BusApiErrorCode::PreconditionFailed,
            "conflict" => BusApiErrorCode::Conflict,
            "not_found" => BusApiErrorCode::NotFound,
            "version_mismatch" => BusApiErrorCode::VersionMismatch,
            "raw_path_refused" => BusApiErrorCode::RawPathRefused,
            "root_escape" => BusApiErrorCode::RootEscape,
            "dry_run_unsupported" => BusApiErrorCode::DryRunUnsupported,
            "timeout" => BusApiErrorCode::Timeout,
            "cancelled" => BusApiErrorCode::Cancelled,
            "internal_error" => BusApiErrorCode::InternalError,
            other => BusApiErrorCode::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for BusApiErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Truncates `text` cleanly at a Unicode scalar boundary without panicking.
fn truncate_detail(text: &str) -> String {
    if text.chars().nth(MAX_ERROR_CHARS).is_none() {
        return text.to_string();
    }
    const MARKER: &str = "...[truncated]";
    let keep = MAX_ERROR_CHARS.saturating_sub(MARKER.chars().count());
    let head = match text.char_indices().nth(keep) {
        Some((at, _)) => text.get(..at).unwrap_or(""),
        None => text,
    };
    format!("{head}{MARKER}")
}

/// Structured error returned by command handlers or bus adapters.
#[derive(Debug, Clone, PartialEq)]
pub struct BusApiError {
    pub code: BusApiErrorCode,
    pub bus_code: Option<String>,
    pub bus_reason: Option<String>,
    pub message: String,
    pub details: Option<CValue>,
}

impl BusApiError {
    /// Creates an error with `code` and bounded `message`.
    pub fn new(code: BusApiErrorCode, message: impl AsRef<str>) -> Self {
        Self { code, bus_code: None, bus_reason: None, message: truncate_detail(message.as_ref()), details: None }
    }

    /// Attaches optional bus catalogue `(code, reason)` metadata.
    pub fn with_bus_reason(mut self, bus_code: impl Into<String>, bus_reason: impl Into<String>) -> Self {
        self.bus_code = Some(bus_code.into());
        self.bus_reason = Some(bus_reason.into());
        self
    }

    /// Attaches structured canonical JSON details.
    pub fn with_details(mut self, details: CValue) -> Self {
        self.details = Some(details);
        self
    }

    pub fn usage(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::UsageError, message)
    }

    pub fn validation(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::ValidationError, message)
    }

    pub fn unit_error(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::UnitError, message)
    }

    pub fn policy_refused(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::PolicyRefused, message)
    }

    pub fn precondition_failed(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::PreconditionFailed, message)
    }

    pub fn conflict(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::Conflict, message)
    }

    pub fn not_found(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::NotFound, message)
    }

    pub fn version_mismatch(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::VersionMismatch, message)
    }

    pub fn raw_path_refused(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::RawPathRefused, message)
    }

    pub fn root_escape(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::RootEscape, message)
    }

    pub fn dry_run_unsupported(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::DryRunUnsupported, message)
    }

    pub fn timeout(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::Timeout, message)
    }

    pub fn cancelled(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::Cancelled, message)
    }

    pub fn internal(message: impl AsRef<str>) -> Self {
        Self::new(BusApiErrorCode::InternalError, message)
    }

    /// Converts this error into a failed [`BusEnvelope`] for `command`.
    pub fn to_envelope(&self, command: impl Into<String>) -> BusEnvelope {
        let details = match (&self.details, &self.bus_code, &self.bus_reason) {
            (Some(d), _, _) => Some(d.clone()),
            (None, Some(bc), Some(br)) => {
                let mut m = BTreeMap::new();
                m.insert("bus_code".to_string(), CValue::Str(bc.clone()));
                m.insert("bus_reason".to_string(), CValue::Str(br.clone()));
                Some(CValue::Obj(m))
            }
            _ => None,
        };
        let outcome = match self.code {
            BusApiErrorCode::Cancelled => BusOutcome::Cancelled,
            _ => BusOutcome::Failed,
        };
        BusEnvelope {
            command: command.into(),
            outcome,
            result: None,
            error_code: Some(self.code.clone()),
            message: Some(self.message.clone()),
            error_details: details,
            request_id: None,
            journal_entry: None,
            job_id: None,
            artifacts: Vec::new(),
        }
    }
}

impl fmt::Display for BusApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BusApiError {}
