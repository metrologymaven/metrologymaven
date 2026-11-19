//! The errors of the normaliser, the version check and the round-trip harness.

use fid_io_reg::IoError;
use fid_xml::XmlError;
use std::fmt;

/// Why normalising, comparing or checking stopped. No variant holds document text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormError {
    /// The XML reader refused the input (`malformed`, `truncated_file`,
    /// `xml_forbidden_construct`, `xml_encoding_unsupported`, `limit_exceeded`, `io`).
    Xml(XmlError),
    /// The harness refused: a budget could not be built, or the normalised output or the
    /// held-back content passed the allocation limit (`limit_exceeded`), or a file read
    /// failed (`io`).
    Harness(IoError),
    /// The rule file is not a valid qif-norm rule file (`norm_rules_invalid`).
    RulesInvalid {
        /// Static reason.
        reason: &'static str,
    },
    /// The rule file differs from the digest locked for its version
    /// (`norm_version_not_bumped`).
    VersionNotBumped {
        /// The version the rule file declares.
        version: String,
    },
    /// The rule file declares a version that is not the last line of the lock file
    /// (`norm_lock_stale`).
    LockStale {
        /// The version the rule file declares.
        version: String,
    },
    /// The lock file is not well formed (`norm_lock_invalid`).
    LockInvalid {
        /// Static reason.
        reason: &'static str,
    },
    /// A corpus manifest is missing (`manifest_missing`) or not well formed (`malformed`).
    Manifest {
        /// The stable code.
        code: &'static str,
        /// Static reason.
        reason: &'static str,
    },
}

impl NormError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            NormError::Xml(e) => e.code(),
            NormError::Harness(e) => e.code(),
            NormError::RulesInvalid { .. } => "norm_rules_invalid",
            NormError::VersionNotBumped { .. } => "norm_version_not_bumped",
            NormError::LockStale { .. } => "norm_lock_stale",
            NormError::LockInvalid { .. } => "norm_lock_invalid",
            NormError::Manifest { code, .. } => code,
        }
    }
}

impl fmt::Display for NormError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NormError::Xml(e) => write!(f, "{e}"),
            NormError::Harness(e) => write!(f, "{e}"),
            NormError::RulesInvalid { reason } => write!(f, "norm_rules_invalid: {reason}"),
            NormError::VersionNotBumped { version } => {
                write!(f, "norm_version_not_bumped: the rule file changed but still declares {version}")
            }
            NormError::LockStale { version } => {
                write!(f, "norm_lock_stale: {version} is not the last version in qif-norm.lock")
            }
            NormError::LockInvalid { reason } => write!(f, "norm_lock_invalid: {reason}"),
            NormError::Manifest { code, reason } => write!(f, "{code}: {reason}"),
        }
    }
}

impl std::error::Error for NormError {}

impl From<XmlError> for NormError {
    fn from(e: XmlError) -> Self {
        NormError::Xml(e)
    }
}

impl From<IoError> for NormError {
    fn from(e: IoError) -> Self {
        NormError::Harness(e)
    }
}
