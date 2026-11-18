//! Evaluation suite format of Metrology Maven. Spec node `AGENT.EVAL.FORMAT`.
//!
//! An evaluation suite is a directory:
//!
//! ```text
//! suite/suite.json                  {"format": "fid-eval-suite@1", "name": ...}
//! suite/tasks/<id>/task.json        the task (schema/eval-task-v1.schema.json)
//! suite/tasks/<id>/<start-state>    the start-state archive the task names
//! suite/oracles/<id>/oracle.json    the oracle (schema/eval-oracle-v1.schema.json)
//! ```
//!
//! A task may carry the red-team extension (`family`, `predicates`; AGENT.REDTEAM.PRED) and,
//! for injection tasks, `payloads` (AGENT.REDTEAM.INJECT); tasks of family `escal` carry a
//! `subfamily` (AGENT.REDTEAM.ESCAL); tasks of family `shell` require `shell: true` and are
//! either a variant (`variant_of`) or `shell_only` (AGENT.REDTEAM.SHELL), and
//! [`check_variants`] checks that a shell suite has exactly one variant per base task.
//!
//! Every file is read fail-closed: at most [`MAX_FILE_BYTES`], JSON nesting at most
//! [`MAX_DEPTH`], duplicate keys and unknown fields rejected, and all field errors of a
//! file collected before failing. The suite version ([`suite_version`]) is `sha256:` over
//! the RFC 8785 canonical JSON of the sorted `[relative path, file digest]` pairs.
//!
//! [`check_suite`] is the suite consistency, coverage and oracle isolation check
//! (AGENT.EVAL.CHECK): digests against FLOW job packages, catalog coverage, granted-root
//! isolation of the unpacked start states and oracle values in prompts.
//!
//! This is the shared format library of the `fid-eval` binary (D-032). It canonicalises
//! and hashes through `fid-canon` and uses `fid-types` digests.

mod check;
mod json;
mod oracle;
mod redteam;
mod shell;
mod suite;
mod tar;
mod task;

pub use check::{
    check_loaded, check_suite, has_token, load_catalog, load_jobs, value_in_prompt, CheckFinding, CheckReport, JobPackage,
    Rule,
};
pub use oracle::{parse_oracle, Basis, Forbidden, Oracle, OracleItem, Outcome, Quantity, DEFAULT_TOL, SI_UNITS};
pub use redteam::{is_marker, Family, Payload, Predicate, PredicateKind, Subfamily, Vector, PREDICATE_KINDS};
pub use shell::{check_variants, VariantCoverage};
pub use suite::{load_suite, read_oracle, read_task, suite_version, Suite, SuiteTask, SUITE_FORMAT};
pub use task::{parse_task, App, Budget, Licence, RelPath, Roots, StartState, Task, Tier};

use fid_types::Digest;
use std::fmt;

/// Largest accepted file (task, oracle, suite header or start-state archive), in bytes.
pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
/// Deepest accepted JSON nesting; the top-level value is depth 1.
pub const MAX_DEPTH: usize = 64;

/// Stable error codes of the format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorCode {
    InvalidTask,
    InvalidOracle,
    /// Layout or `suite.json` problems (not named by the spec; see the PR).
    InvalidSuite,
    InputLimit,
    DuplicateKey,
    MissingFile,
    /// A red-team task without predicates (AGENT.REDTEAM.PRED).
    MissingPredicate,
    /// A predicate kind outside the closed list (AGENT.REDTEAM.PRED).
    UnknownPredicate,
}

impl ErrorCode {
    /// The wire string, e.g. `invalid_task`.
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::InvalidTask => "invalid_task",
            ErrorCode::InvalidOracle => "invalid_oracle",
            ErrorCode::InvalidSuite => "invalid_suite",
            ErrorCode::InputLimit => "input_limit",
            ErrorCode::DuplicateKey => "duplicate_key",
            ErrorCode::MissingFile => "missing_file",
            ErrorCode::MissingPredicate => "missing_predicate",
            ErrorCode::UnknownPredicate => "unknown_predicate",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One typed error. `file` is the path as given (suite-relative inside a suite);
/// `field` is a dotted path such as `budget.wall_s` or `items[0].tol` (empty for
/// whole-file errors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError {
    pub code: ErrorCode,
    pub file: String,
    pub field: String,
    pub detail: String,
}

impl FormatError {
    pub(crate) fn new(code: ErrorCode, file: &str, field: &str, detail: impl Into<String>) -> FormatError {
        FormatError { code, file: file.to_string(), field: field.to_string(), detail: detail.into() }
    }
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.field.is_empty() {
            write!(f, "{}: {}: {}", self.code, self.file, self.detail)
        } else {
            write!(f, "{}: {}: {}: {}", self.code, self.file, self.field, self.detail)
        }
    }
}

impl std::error::Error for FormatError {}

/// SHA-256 of raw bytes as a `fid-types` digest.
pub fn file_digest(bytes: &[u8]) -> Digest {
    Digest::from_bytes(fid_canon::digest_bytes(bytes).0)
}

/// The tooling canonical JSON encoder (REQ-AGENT.EVAL.FORMAT-007): RFC 8785 bytes of
/// `input` and their digest, through `fid-canon`.
pub fn canonical_json(input: &[u8]) -> Result<(Vec<u8>, Digest), fid_canon::CanonError> {
    let bytes = fid_canon::canonicalize(input)?;
    let d = file_digest(&bytes);
    Ok((bytes, d))
}
