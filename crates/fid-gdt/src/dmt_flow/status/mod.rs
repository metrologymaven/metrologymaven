//! Interpretation status register and sign-off binding (spec node `GDT.RES.STATUS`, D-045).
//!
//! D-045 labels a result `interpretation: provisional` when any interpretation rule it used is
//! not signed off. This module decides, from the `GDT.RES.ROWFLOW` journal alone, which rules
//! are signed off:
//!
//! - [`build_register`] folds the journal into the register `gdt-rule-status/v1`: one entry
//!   per (rule ID, revision) for every DMT row key and every published defaults profile, with
//!   its kind, `revised_at` (UTC, from the journal entry that created the revision), content
//!   digest, author kind, status, reason and the counted approvals (REQ-002).
//! - **Approvals are recounted, never copied.** `GDT.RES.ROWFLOW.STATE`'s `signed_off` is not
//!   a counted sign-off. Each revision is rebuilt with STATE's [`replay`](super::state::replay)
//!   one journal entry at a time; an entry STATE refuses (a stale digest, a non-human
//!   principal, a reviewer without a roster role on the day, ...) is not applied. An approval
//!   then counts only when it is live (no later `request_changes`), names the revision's
//!   digest, comes from a `human` principal and breaks none of the conflict-of-interest rules
//!   read from the reviewer register (`gdt-reviewer-register/v1`): same organisation ID as the
//!   other approver, a declared recusal for the rule, or a reviewer the register does not list
//!   (REQ-003). Two counted approvals from distinct reviewers on the latest revision give
//!   `signed_off`; an agent-authored latest revision is `draft`; everything else is
//!   `provisional` (REQ-004, REQ-005).
//! - Profiles come from `gdt.publish_defaults_profile@1` entries on the working branch. Their
//!   sign-off key (`GDT.RES.ROWASME.DPROF`) is not settled, so every profile is `provisional`
//!   with reason `no_signoff_path` (REQ-006, failing closed).
//! - [`verify_register`] rebuilds the register and accepts a file only when it is
//!   byte-identical to the rebuild and both its digest and its roster digest are listed, as a
//!   pair, in the project-published, append-only list `gdt-published/v1`; for purpose
//!   [`Purpose::Evaluate`] the register must also be the list's last entry (REQ-007, REQ-010).
//! - [`status_of`] answers for one rule at one content digest and never answers `signed_off`
//!   without a matching entry in a verified register (REQ-008).
//! - [`TrustedRosters`] holds only rosters whose digests `gdt-published/v1` lists; it has no
//!   public constructor other than [`Published::trusted_rosters`], and STATE's `transition`
//!   and `replay` require it, so no bare roster reaches them. The trust rests on where the
//!   caller got `gdt-published/v1` (the DMT data repository, loaded with
//!   [`Published::load_extending`] against a pinned prior); a self-made list still admits a
//!   self-made roster.
//! - [`export_rule_status`] is the command `gdt.export_rule_status@1` (W1, `write_root`).
//!
//! **Pseudonymous.** The register names reviewers by roster reviewer ID only; it never holds a
//! display name, an organisation, a principal identity or any free text.
//!
//! Deterministic (D0): no clock (every time comes from a journal entry), sorted maps only, no
//! floating point, no panics.

mod command;
mod inputs;
mod register;
mod verify;

pub use command::{
    command_def, export_rule_status, invoke_cli, invoke_mcp, replay_export, ExportContext, ExportEvent, Exported,
    EXPORT_CLI, EXPORT_COMMAND, EXPORT_MCP,
};
pub use inputs::{
    CoiInputs, JournalEntry, JournalExtract, Published, PublishedEntry, ReviewerCoi, TrustedRosters, Utc,
    JOURNAL_SCHEMA, MAX_JOURNAL_BYTES, MAX_JOURNAL_ENTRIES, PROFILE_REGISTRY_SCHEMA, PUBLISHED_SCHEMA,
    REVIEWER_REGISTER_SCHEMA,
};
pub use register::{
    build_register, Approval, BuiltFrom, RuleKind, RuleStatusEntry, RuleStatusRegister, REGISTER_SCHEMA,
};
pub use verify::{status_of, verify_register, CheckedRegister, StatusAnswer, Unverified, Verification};

/// The one content-digest function of a DMT row (REQ-GDT.RES.STATUS-001), built in `fid-gps`
/// (module `dmt`) so the resolver can call it; re-exported here, never duplicated.
pub use fid_gps::dmt::row_content_digest;

use std::fmt;

/// Journal command of a row proposal.
pub const PROPOSE_COMMAND: &str = "gdt.propose_dmt_row@1";
/// Journal command of a row review.
pub const REVIEW_COMMAND: &str = "gdt.review_dmt_row@1";

/// Errors of this module. [`StatusError::code`] is the stable code. Verification outcomes and
/// statuses are data, not errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusError {
    /// `journal_unreadable{reason}`: the journal extract is malformed or breaks the workflow.
    JournalUnreadable {
        /// What is wrong.
        reason: &'static str,
    },
    /// `roster_unreadable{reason}`: a roster file does not load.
    RosterUnreadable {
        /// The roster loader's code.
        reason: &'static str,
    },
    /// `coi_inputs_unreadable{reason}`: the reviewer register or profile registry is malformed.
    CoiInputsUnreadable {
        /// What is wrong.
        reason: &'static str,
    },
    /// `published_unreadable{reason}`: `gdt-published/v1` is malformed, or not an append-only
    /// extension of an earlier copy.
    PublishedUnreadable {
        /// What is wrong.
        reason: &'static str,
    },
    /// `foreign_roster`: a roster whose digest `gdt-published/v1` does not list.
    ForeignRoster,
    /// `register_unreadable{reason}`: a register file is not a well-formed `gdt-rule-status/v1`.
    RegisterUnreadable {
        /// What is wrong.
        reason: &'static str,
    },
    /// `export_root_denied{reason}`: the export target was refused (the I/O registry's code).
    ExportRootDenied {
        /// The I/O registry's code.
        reason: &'static str,
    },
    /// `bad_params{reason}`.
    BadParams {
        /// What is wrong.
        reason: &'static str,
    },
}

impl StatusError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            StatusError::JournalUnreadable { .. } => "journal_unreadable",
            StatusError::RosterUnreadable { .. } => "roster_unreadable",
            StatusError::CoiInputsUnreadable { .. } => "coi_inputs_unreadable",
            StatusError::PublishedUnreadable { .. } => "published_unreadable",
            StatusError::ForeignRoster => "foreign_roster",
            StatusError::RegisterUnreadable { .. } => "register_unreadable",
            StatusError::ExportRootDenied { .. } => "export_root_denied",
            StatusError::BadParams { .. } => "bad_params",
        }
    }

    /// The detail reason, if any.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            StatusError::JournalUnreadable { reason }
            | StatusError::RosterUnreadable { reason }
            | StatusError::CoiInputsUnreadable { reason }
            | StatusError::PublishedUnreadable { reason }
            | StatusError::RegisterUnreadable { reason }
            | StatusError::ExportRootDenied { reason }
            | StatusError::BadParams { reason } => Some(reason),
            StatusError::ForeignRoster => None,
        }
    }
}

impl fmt::Display for StatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.reason() {
            Some(r) => write!(f, "{}{{{r}}}", self.code()),
            None => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for StatusError {}

/// A rule's status (D-045 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    /// `draft`: an agent-authored latest revision no human has re-proposed; never evaluated.
    Draft,
    /// `provisional`.
    Provisional,
    /// `signed_off`: two counted approvals from distinct reviewers on the latest revision.
    SignedOff,
}

impl Status {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::Provisional => "provisional",
            Status::SignedOff => "signed_off",
        }
    }

    /// Parses the wire form.
    pub fn parse(s: &str) -> Option<Status> {
        match s {
            "draft" => Some(Status::Draft),
            "provisional" => Some(Status::Provisional),
            "signed_off" => Some(Status::SignedOff),
            _ => None,
        }
    }
}

/// Why a rule is not `signed_off` (or why an answer is `provisional`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reason {
    /// `awaiting_review`: the latest revision lacks two counted approvals.
    AwaitingReview,
    /// `superseded`: a later revision of the rule exists.
    Superseded,
    /// `not_in_register`: the verified register has no entry for the rule at that digest.
    NotInRegister,
    /// `no_signoff_path`: a profile, whose sign-off key does not exist yet.
    NoSignoffPath,
    /// `no_register`: no register was loaded.
    NoRegister,
    /// `unverified_register`: the register did not verify (including `stale_register`).
    UnverifiedRegister,
}

impl Reason {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::AwaitingReview => "awaiting_review",
            Reason::Superseded => "superseded",
            Reason::NotInRegister => "not_in_register",
            Reason::NoSignoffPath => "no_signoff_path",
            Reason::NoRegister => "no_register",
            Reason::UnverifiedRegister => "unverified_register",
        }
    }

    /// Parses the wire form.
    pub fn parse(s: &str) -> Option<Reason> {
        match s {
            "awaiting_review" => Some(Reason::AwaitingReview),
            "superseded" => Some(Reason::Superseded),
            "not_in_register" => Some(Reason::NotInRegister),
            "no_signoff_path" => Some(Reason::NoSignoffPath),
            "no_register" => Some(Reason::NoRegister),
            "unverified_register" => Some(Reason::UnverifiedRegister),
            _ => None,
        }
    }
}

/// What a verified register is used for (REQ-010).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Purpose {
    /// `evaluate`: a new evaluation; only the latest published register verifies.
    Evaluate,
    /// `replay`: re-checking an earlier result; any published register verifies.
    Replay,
}

impl Purpose {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Purpose::Evaluate => "evaluate",
            Purpose::Replay => "replay",
        }
    }

    /// Parses the wire form.
    pub fn parse(s: &str) -> Option<Purpose> {
        match s {
            "evaluate" => Some(Purpose::Evaluate),
            "replay" => Some(Purpose::Replay),
            _ => None,
        }
    }
}
