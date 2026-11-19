//! Evaluation harness of Metrology Maven (`fid-eval`, D-032). Spec nodes
//! `AGENT.HARNESS.SANDBOX` (the per-run sandbox project and task-scoped roots) and
//! `AGENT.HARNESS.ADAPT` (the agent adapter contract, see [`Adapter`] and the `adapt` module).
//!
//! [`Sandbox::build`] makes a fresh, isolated run directory for one run of one task:
//!
//! ```text
//! <work>/run/<run-id>/            created exclusively; run-id is a fresh UUIDv7
//!     project/                    the unpacked start state, nothing else
//!     roots/read/<root>/          one folder per granted read root, holding the start
//!                                 state's files under <root>/ (the granted inputs)
//!     roots/write/<root>/         one empty folder per granted write root
//!     roots.json                  the root configuration: exactly the task's roots
//!     home/                       private home and configuration folders for child processes
//!     artifacts/                  for the run's outputs (filled by later harness stages)
//! ```
//!
//! The start-state archive is checked against the task's digest before anything is written
//! (`start_state_mismatch`), then parsed completely and fail-closed (`bad_archive`) before
//! unpacking: only POSIX ustar regular files and directories; every path relative, of
//! `[A-Za-z0-9._-]` segments, never `.` or `..`, no segment ending in a dot, no Windows device
//! name; no two entries that would collide on a case-insensitive file system; no entry below
//! a file. Files are created with `create_new`, directories one segment at a time, and an
//! existing symbolic link or junction is never followed.
//!
//! The run directory and `home/` are created with mode 0o700 on Unix.
//!
//! [`FsSnapshot`] records the path and SHA-256 of every file in the run directory, without
//! following symbolic links or junctions (they are recorded as links with their targets).
//! Each file is re-checked through its opened handle (no-follow, non-blocking), so a leftover
//! process that swaps in a link or FIFO cannot redirect or hang the snapshot.
//!
//! The crate makes no network connection and calls no `fid` command; it reads the fields of
//! `task.json` it needs ([`TaskSpec`]) itself, because the layer rules forbid linking the
//! format crate `fid-eval-suite` (another tool crate).

mod adapt;
mod path;
mod sandbox;
mod snapshot;
mod tar;
mod task;

pub use adapt::{
    check_profile, find_oracle_entries, find_oracle_entries_capped, mcp_config, parse_token_report, read_token_report,
    scrub_env, suite_dir_of, Adapter, AdapterError, AgentPaths, LaunchInput, LaunchPlan, Limits, McpCommand,
    PromptMode, Tokens, DEFAULT_OUTPUT_CAP, ENV_ALLOWLIST, MAX_ADAPTER_BYTES, MAX_ARGV, MAX_ARG_BYTES, MAX_SETTINGS,
    MAX_TEXT_BYTES, MAX_TOKEN_REPORT_BYTES, MCP_SERVER_NAME, PLACEHOLDERS,
};
pub use path::check_rel;
pub use sandbox::{new_run_id, RootDecl, Sandbox, ROOTS_FORMAT};
pub use snapshot::{escape_name, Change, ChangeKind, EntryKind, FsSnapshot, SnapEntry};
pub use task::{AgentTask, TaskSpec};

use std::fmt;

/// Largest accepted `task.json` or start-state archive, in bytes (as AGENT.EVAL.FORMAT).
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Stable error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorCode {
    /// The start-state archive's digest differs from the task's (REQ-002).
    StartStateMismatch,
    /// The run directory already exists (REQ-001).
    RunDirExists,
    /// The start-state archive is unreadable, malformed or unsafe to unpack.
    BadArchive,
    /// `task.json` lacks a field the sandbox needs or has a wrong type.
    InvalidTask,
    /// The task's roots are not distinct, safe, relative folder paths.
    InvalidRoots,
    /// The harness work folder is unusable (not absolute, not a directory, or `run` is a link).
    InvalidWorkDir,
    /// A path leaves its base folder, or passes through a symbolic link or junction.
    PathEscape,
    /// A snapshot exceeded its entry or depth limit.
    SnapshotLimit,
    /// No time or randomness for a run ID.
    RunIdUnavailable,
    /// Any other file-system failure.
    Io,
    /// An adapter file is malformed or unsafe (AGENT.HARNESS.ADAPT REQ-003).
    InvalidAdapter,
    /// An adapter's argv lacks a placeholder the agent needs (AGENT.HARNESS.ADAPT).
    MissingPlaceholder,
}

impl ErrorCode {
    /// The wire string, e.g. `start_state_mismatch`.
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::StartStateMismatch => "start_state_mismatch",
            ErrorCode::RunDirExists => "run_dir_exists",
            ErrorCode::BadArchive => "bad_archive",
            ErrorCode::InvalidTask => "invalid_task",
            ErrorCode::InvalidRoots => "invalid_roots",
            ErrorCode::InvalidWorkDir => "invalid_work_dir",
            ErrorCode::PathEscape => "path_escape",
            ErrorCode::SnapshotLimit => "snapshot_limit",
            ErrorCode::RunIdUnavailable => "run_id_unavailable",
            ErrorCode::Io => "io_error",
            ErrorCode::InvalidAdapter => "invalid_adapter",
            ErrorCode::MissingPlaceholder => "missing_placeholder",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One typed error with a human-readable detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxError {
    pub code: ErrorCode,
    pub detail: String,
}

impl SandboxError {
    pub(crate) fn new(code: ErrorCode, detail: impl Into<String>) -> SandboxError {
        SandboxError { code, detail: detail.into() }
    }
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for SandboxError {}

/// Reads a regular file (never through a symbolic link) of at most [`MAX_FILE_BYTES`].
pub(crate) fn read_bounded(p: &std::path::Path, code: ErrorCode) -> Result<Vec<u8>, SandboxError> {
    use std::io::Read;
    let name = p.display();
    let meta = std::fs::symlink_metadata(p).map_err(|e| SandboxError::new(code, format!("{name}: {e}")))?;
    if !meta.file_type().is_file() {
        return Err(SandboxError::new(code, format!("{name}: not a regular file")));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(SandboxError::new(code, format!("{name}: larger than 64 MiB")));
    }
    let f = std::fs::File::open(p).map_err(|e| SandboxError::new(code, format!("{name}: {e}")))?;
    let mut buf = Vec::new();
    f.take(MAX_FILE_BYTES + 1).read_to_end(&mut buf).map_err(|e| SandboxError::new(code, format!("{name}: {e}")))?;
    if buf.len() as u64 > MAX_FILE_BYTES {
        return Err(SandboxError::new(code, format!("{name}: larger than 64 MiB")));
    }
    Ok(buf)
}
