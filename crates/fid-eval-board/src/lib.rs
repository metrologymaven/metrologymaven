//! Published scoreboard tooling of Metrology Maven (`fid-eval board *`, D-032).
//!
//! This first slice is spec node `AGENT.BOARD.CLAIMS`: the claim register
//! (`agent/claims.json`) and `board check-claims`, which keeps public statements about agent
//! capability honest. Every such statement in docs, website and release notes carries a tag
//! `<!-- fid-claim: <id> -->`; the register maps each ID to a scoreboard figure (release,
//! model, scope, metric) and a threshold. The check fails when
//!
//! - a tag names an ID absent from the register (`unknown_claim`),
//! - a register entry names no board entry (`entry_missing`); the check takes one board, so
//!   entries for any other release (for example claims about earlier releases kept in the
//!   register) report `entry_missing`. This is intended: check each release's claims against
//!   that release's board, or retire claims about superseded releases,
//! - the board figure does not meet the entry's threshold, compared as exact fractions
//!   (`threshold_not_met`),
//! - the release's red-team verdict is `block` or `incomplete` (`claim_on_blocked_release`),
//! - a unit (paragraph, heading, list item, table row; HTML block element) contains an
//!   agent-capability phrase from `agent/claim-phrases.txt` but no tag (`untagged_claim`, a
//!   heuristic; markup and character references are removed first, see the `scan` module).
//!
//! "Claim" here means a public capability claim. It is unrelated to *work* claims: the
//! `/claim` issue-comment workflow (`.github/workflows/claim.yml`, which assigns an issue) and
//! the `claimed_by:` field in spec-node frontmatter. This crate reads neither.
//!
//! The check reads only local files (determinism class D0): no network, no GitHub API.

mod board;
mod check;
mod fraction;
pub mod guide;
mod register;
mod scan;

pub use board::{parse_board, Board, Figures, ModelEntry, Outcome};
pub use check::{check_claims, Inputs, Report};
pub use fraction::Fraction;
pub use guide::{
    build_guide, write_guide, GuideError, GuideInputs, GuideOutput, GuideRow, HardwareInfo, WorkflowSuccess,
    GUIDE_HTML_FILE, GUIDE_MD_FILE,
};
pub use register::{is_claim_id, parse_register, Claim, Metric, Register, Scope, ScopeKind, REGISTER_FORMAT};
pub use scan::{parse_phrases, scan_text, SourceKind, Tag, SOURCE_EXTENSIONS};

use std::fmt;

/// Stable finding codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Code {
    /// A tag names an ID absent from the register, or a tag is malformed.
    UnknownClaim,
    /// A register entry names a release, model, scope or figure the board does not have.
    EntryMissing,
    /// The board figure does not meet the register threshold.
    ThresholdNotMet,
    /// A capability phrase with no tag in the same paragraph.
    UntaggedClaim,
    /// A register entry on a release whose red-team verdict is `block` or `incomplete`.
    ClaimOnBlockedRelease,
    /// The register is not valid.
    InvalidRegister,
    /// The board file is not valid.
    InvalidBoard,
    /// The phrase list is not valid.
    InvalidPhrases,
    /// A source file or directory could not be read.
    InvalidSource,
}

impl Code {
    /// The wire string, e.g. `untagged_claim`.
    pub fn as_str(self) -> &'static str {
        match self {
            Code::UnknownClaim => "unknown_claim",
            Code::EntryMissing => "entry_missing",
            Code::ThresholdNotMet => "threshold_not_met",
            Code::UntaggedClaim => "untagged_claim",
            Code::ClaimOnBlockedRelease => "claim_on_blocked_release",
            Code::InvalidRegister => "invalid_register",
            Code::InvalidBoard => "invalid_board",
            Code::InvalidPhrases => "invalid_phrases",
            Code::InvalidSource => "invalid_source",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One failure of the check, located at a file and 1-based line (0 when no line applies).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub code: Code,
    pub claim: Option<String>,
    pub detail: String,
}

impl Finding {
    pub fn new(code: Code, claim: Option<&str>, file: &str, line: usize, detail: impl Into<String>) -> Finding {
        Finding { file: file.to_string(), line, code, claim: claim.map(str::to_string), detail: detail.into() }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.code)?;
        if let Some(c) = &self.claim {
            write!(f, ": claim {c}")?;
        }
        write!(f, ": {}", self.detail)
    }
}

/// A path as shown in findings: as given, with `/` separators and no trailing separator.
pub fn display_path(p: &std::path::Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let t = s.trim_end_matches('/');
    if t.is_empty() {
        s
    } else {
        t.to_string()
    }
}
