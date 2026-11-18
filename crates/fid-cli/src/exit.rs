//! The exit-code table (spec node AUTO.CLI.OUT, REQ-AUTO.CLI.OUT-002 and -003).
//!
//! [`ExitCode`] is the single definition: [`ExitCode::from_envelope`] maps outcomes to
//! codes, `fid --help` prints it, and `docs/cli/exit-codes.md` is generated from it by
//! [`exit_codes_markdown`] (a test fails when the checked-in page drifts).

use fid_surface::{Envelope, ErrorCode, Outcome};

/// One documented process exit code per outcome class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum ExitCode {
    /// `committed`, `computed` or `dry_run`.
    Success = 0,
    /// Internal error.
    Internal = 1,
    /// Usage error.
    Usage = 2,
    /// Refused by side-effect policy.
    PolicyRefused = 3,
    /// The command failed.
    Failed = 4,
    /// The command was cancelled.
    Cancelled = 5,
    /// `not_run` timeout.
    NotRun = 6,
    /// `no_reply` timeout.
    NoReply = 7,
    /// A request was filed for a human.
    RequestFiled = 8,
}

impl ExitCode {
    /// Every exit code, in numeric order.
    pub const ALL: [ExitCode; 9] = [
        ExitCode::Success,
        ExitCode::Internal,
        ExitCode::Usage,
        ExitCode::PolicyRefused,
        ExitCode::Failed,
        ExitCode::Cancelled,
        ExitCode::NotRun,
        ExitCode::NoReply,
        ExitCode::RequestFiled,
    ];

    /// The numeric process exit status.
    pub fn code(self) -> u8 {
        self as u8
    }

    /// Stable short name.
    pub fn name(self) -> &'static str {
        match self {
            ExitCode::Success => "success",
            ExitCode::Internal => "internal_error",
            ExitCode::Usage => "usage_error",
            ExitCode::PolicyRefused => "policy_refused",
            ExitCode::Failed => "failed",
            ExitCode::Cancelled => "cancelled",
            ExitCode::NotRun => "not_run",
            ExitCode::NoReply => "no_reply",
            ExitCode::RequestFiled => "request_filed",
        }
    }

    /// One-line meaning, used by `--help` and the generated reference page.
    pub fn meaning(self) -> &'static str {
        match self {
            ExitCode::Success => "The command committed, computed or completed a dry run (outcome `committed`, `computed` or `dry_run`).",
            ExitCode::Internal => "Internal error: outcome `failed` with `internal_error`, an envelope that could not be encoded or written, or an outcome the CLI cannot end on (`accepted` or unknown).",
            ExitCode::Usage => "Usage error: the invocation was malformed (`usage_error`, `dry_run_unsupported`). Nothing ran.",
            ExitCode::PolicyRefused => "Refused by side-effect policy (`policy_refused`, `raw_path_refused`), including M1, S1 and A0 commands refused at the CLI. Nothing ran.",
            ExitCode::Failed => "The command failed with any other error code.",
            ExitCode::Cancelled => "The command was cancelled (outcome `cancelled`, or `failed` with `cancelled`).",
            ExitCode::NotRun => "Timeout with outcome `not_run`: the command did not start.",
            ExitCode::NoReply => "Timeout with outcome `no_reply`: the command may or may not have run; check the journal.",
            ExitCode::RequestFiled => "A request was filed for a human (outcome `request_filed`); the command has not run yet.",
        }
    }

    /// Maps a final envelope to its exit code (REQ-AUTO.CLI.OUT-002).
    pub fn from_envelope(env: &Envelope) -> ExitCode {
        match &env.outcome {
            Outcome::Committed | Outcome::Computed | Outcome::DryRun => ExitCode::Success,
            Outcome::RequestFiled => ExitCode::RequestFiled,
            Outcome::Cancelled => ExitCode::Cancelled,
            Outcome::NotRun => ExitCode::NotRun,
            Outcome::NoReply => ExitCode::NoReply,
            Outcome::Failed => match env.error.as_ref().map(|e| &e.code) {
                Some(ErrorCode::PolicyRefused | ErrorCode::RawPathRefused) => ExitCode::PolicyRefused,
                Some(ErrorCode::UsageError | ErrorCode::DryRunUnsupported) => ExitCode::Usage,
                Some(ErrorCode::Cancelled) => ExitCode::Cancelled,
                Some(ErrorCode::InternalError) | None => ExitCode::Internal,
                Some(_) => ExitCode::Failed,
            },
            Outcome::Accepted | Outcome::Unknown(_) => ExitCode::Internal,
        }
    }
}

impl From<ExitCode> for std::process::ExitCode {
    fn from(c: ExitCode) -> Self {
        std::process::ExitCode::from(c.code())
    }
}

/// Path of the generated reference page, relative to the repository root.
pub const EXIT_CODES_DOC: &str = "docs/cli/exit-codes.md";

/// The content of `docs/cli/exit-codes.md`, generated from [`ExitCode::ALL`].
pub fn exit_codes_markdown() -> String {
    let mut s = String::new();
    s.push_str("<!-- Generated from crates/fid-cli/src/exit.rs (spec node AUTO.CLI.OUT). Do not edit by hand. -->\n");
    s.push_str("<!-- Regenerate: FID_BLESS=1 cargo test -p fid-cli --test out -->\n\n");
    s.push_str("# `fid` exit codes\n\n");
    s.push_str("Every `fid` command exits with exactly one of these codes, so scripts and CI can branch on the\n");
    s.push_str("outcome without parsing text. With `--json`, stdout holds exactly one result envelope\n");
    s.push_str("(`schema/envelope-v1.json`) and everything else goes to stderr.\n\n");
    s.push_str("| Code | Name | Meaning |\n|---:|---|---|\n");
    for c in ExitCode::ALL {
        s.push_str(&format!("| {} | `{}` | {} |\n", c.code(), c.name(), c.meaning()));
    }
    s
}
