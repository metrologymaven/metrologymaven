//! Cross-host and cross-artifact bit-identity gate (spec node `KRN.MAN.XART`).
//!
//! Run, tabulate, compare:
//!
//! - [`calls`]: the call set (one canonical `KernelCall` per line) and its digest;
//! - [`artifact`]: runs every call through one kernel artifact (the WASM module through the
//!   pinned runner, or a native build) and builds a [`table::DigestTable`]. It fails closed: an
//!   artifact that cannot answer a call, or answers with anything but a well-formed result or
//!   kernel error, is an error for the whole run, never a skipped row;
//! - [`table`]: the digest table and its canonical JSON (written with `fid-kci`);
//! - [`matrix`]: the host matrix file (`xart-matrix.toml`);
//! - [`report`]: compares the tables: WASM digests must agree on every host (REQ-001), native
//!   artifacts are measured against WASM and never gate (REQ-002), a patch release must
//!   reproduce the previous release's table (REQ-003), and a host without a table is `not_run`
//!   and fails the gate unless optional (REQ-005). The report is canonical JSON (REQ-004);
//! - [`testart`]: test-only native artifacts for the stand-in test algorithms (AC-2).
//!
//! Every digest of an artifact's answer is computed here from the bytes the artifact wrote;
//! nothing an artifact claims about itself is read from disk.

pub mod artifact;
pub mod calls;
mod json;
pub mod matrix;
pub mod report;
pub mod table;
pub mod testart;

use std::fmt;

/// Errors, each with a stable code. Every one stops the run or the comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XartError {
    /// The call-set file is malformed (1-based line, reason).
    Calls { line: usize, reason: &'static str },
    /// The matrix file is malformed (1-based line, reason).
    Matrix { line: usize, reason: &'static str },
    /// A digest table is malformed.
    Table(String),
    /// An artifact did not answer a call with a result or a kernel error.
    Artifact { call: usize, name: String, algorithm: String, reason: String },
    /// The tables given to a comparison do not fit together.
    Inputs(String),
}

impl XartError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            XartError::Calls { .. } => "bad_call_set",
            XartError::Matrix { .. } => "bad_matrix",
            XartError::Table(_) => "bad_table",
            XartError::Artifact { .. } => "artifact_failed",
            XartError::Inputs(_) => "bad_inputs",
        }
    }
}

impl fmt::Display for XartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XartError::Calls { line, reason } => write!(f, "bad_call_set: line {line}: {reason}"),
            XartError::Matrix { line, reason } => write!(f, "bad_matrix: line {line}: {reason}"),
            XartError::Table(r) => write!(f, "bad_table: {r}"),
            XartError::Artifact { call, name, algorithm, reason } => {
                write!(f, "artifact_failed: call {call} ({name}) algorithm {algorithm}: {reason}")
            }
            XartError::Inputs(r) => write!(f, "bad_inputs: {r}"),
        }
    }
}

impl std::error::Error for XartError {}

/// True for a host or artifact label: 1 to 64 bytes of `[a-z0-9._-]`, starting with a letter
/// or digit.
pub fn is_label(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=64).contains(&b.len())
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
}

/// True for a call name: 1 to 64 bytes of `[A-Za-z0-9._-]`.
pub fn is_name(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}
