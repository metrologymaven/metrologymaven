//! Findings of `corpus-check`, each with a stable snake_case code.

use fid_types::Digest;
use std::fmt;

/// One failure found while checking a corpus. A check collects all of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorpError {
    /// A registered first-party format has no `corpus/<format-id>.toml` (REQ-IO.CORP.MANIF-001).
    ManifestMissing { format: String },
    /// An in-repository entry's licence is `unknown` or not on the redistribution allowlist
    /// (REQ-IO.CORP.MANIF-002).
    LicenceNotRedistributable { format: String, entry: String, licence: String },
    /// A committed or fetched file's SHA-256 differs from its manifest entry
    /// (REQ-IO.CORP.MANIF-003, REQ-IO.CORP.MANIF-004).
    DigestMismatch { format: String, entry: String, expected: Digest, actual: Digest },
    /// A manifest or registry listing breaks the schema. `at` names the file (and entry).
    Malformed { at: String, detail: String },
    /// A committed file could not be read, or exceeded the size cap.
    Io { format: String, entry: String, detail: String },
    /// A `url` entry could not be fetched (network outcome, class `N`; fails closed).
    FetchFailed { format: String, entry: String, detail: String },
}

impl CorpError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            CorpError::ManifestMissing { .. } => "manifest_missing",
            CorpError::LicenceNotRedistributable { .. } => "licence_not_redistributable",
            CorpError::DigestMismatch { .. } => "digest_mismatch",
            CorpError::Malformed { .. } => "malformed",
            CorpError::Io { .. } => "io",
            CorpError::FetchFailed { .. } => "fetch_failed",
        }
    }

    pub(crate) fn malformed(at: impl Into<String>, detail: impl Into<String>) -> CorpError {
        CorpError::Malformed { at: at.into(), detail: detail.into() }
    }
}

impl fmt::Display for CorpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            CorpError::ManifestMissing { format } => write!(f, "{code}{{{format}}}: no manifest corpus/{format}.toml"),
            CorpError::LicenceNotRedistributable { format, entry, licence } => {
                write!(f, "{code}{{{format}: {entry}}}: licence {licence:?} does not allow storing it in the repository")
            }
            CorpError::DigestMismatch { format, entry, expected, actual } => {
                write!(f, "{code}{{{format}: {entry}}}: expected {expected}, got {actual}")
            }
            CorpError::Malformed { at, detail } => write!(f, "{code}{{{at}}}: {detail}"),
            CorpError::Io { format, entry, detail } | CorpError::FetchFailed { format, entry, detail } => {
                write!(f, "{code}{{{format}: {entry}}}: {detail}")
            }
        }
    }
}

impl std::error::Error for CorpError {}
