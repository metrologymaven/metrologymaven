//! Supersession of released records (spec node `CORE.JRN.UNDO.SUPER`).
//!
//! A released record is never undone; it is superseded. `journal.supersede_record@1` (W3, D0,
//! not undoable, MCP projection `request_only`) records that a new released record replaces an
//! old one, and leaves the old record's entry unchanged and visible. [`plan`] checks one call
//! and returns the one [`Supersession`] to commit, or a [`SupersedeError`] whose
//! [`SupersedeError::bus_code`] is a code and reason of the bus error catalogue (v3).
//! [`Supersessions`] folds the committed journal into the released-record and `superseded_by`
//! indexes, and refuses any supersession its entry's origin could not have produced. With
//! feature `store`, [`command_defs`] declares the command. The bus handler is in
//! `fid-journal-cmd`.
//!
//! # Pinned rules
//!
//! - **Released record.** A digest is a released record on a branch when it is an output
//!   digest (not the tombstone) of a committed W3 command entry on that branch, other than a
//!   `journal.supersede_record@1` entry. Only the entry's own branch counts: records reached
//!   through a fork are not looked up across branches.
//! - **Who.** Only a human supersedes. The caller is the [`Actor`] the bus derived from the
//!   channel; anything else is `policy/class_forbidden` (W3). Over MCP the bus turns an agent's
//!   call into a pending request (`CORE.BUS.POL.PROJ`); nothing is superseded.
//! - **Order of checks** (first refusal wins): caller; reason of 1 to [`MAX_REASON_CHARS`]
//!   characters (`validation/schema`); old equal to new; old, then new, a released record on
//!   the call's branch; old, then new, not already superseded on it. Every refusal after the
//!   reason is `precondition/not_supersedable`.
//! - **Reason.** The reason is untrusted text. It is not copied into the event: the entry
//!   records it as its `reason` (a salted, detachable commitment, `CORE.JRN.ENTRY`), and the
//!   fold keeps the header's `reason_commit` as the supersession's commitment.
//! - **Event.** `RecordSuperseded {head, new, old}`, where `head` is the logical head of the
//!   branch the call was decided against. The commit stage commits the entry only if that is
//!   still the branch's head (compare-and-swap; `conflict/head_moved` otherwise).
//!
//! # The fold ([`Supersessions::apply_entry`], REQ-CORE.RPL.RECON.FOLD-008)
//!
//! Each entry is folded with its [`SupersedeEntry`], built only from the verified header and
//! the commit stage (never from an event body). An entry that holds a `RecordSuperseded`
//! event is accepted only if all of these hold, else the whole entry is refused as
//! `internal/invariant_violated` and nothing of it is folded:
//!
//! 1. **(a) origin:** its events were added by the handler of `journal.supersede_record@1`,
//!    class W3, and the principal is human;
//! 2. **(b) reserved effect:** `RecordSuperseded` is reserved to that command in
//!    `fid_registry::RESERVED_EFFECTS`; any other command's entry holding it is refused by 1;
//! 3. **(c) requests:** the entry holds no `Request*` event (a supersession is a direct human
//!    action; an approved agent request does not carry it);
//! 4. exactly one `RecordSuperseded` event, with exactly the keys `head`, `new`, `old`, all
//!    digests, and the header records a `reason_commit`;
//! 5. **(d) decided-against state:** `head` is the logical head of the entry's branch before
//!    the entry, as the caller gives it from the verified journal: a replayed or stale event is
//!    refused;
//! 6. **(e) branches:** the supersession counts on the entry's own branch only; an entry that
//!    is a promoted copy (`promoted_from`) is refused, since a supersession is made directly
//!    on the branch where it applies;
//! 7. old and new differ, both are released records on the branch, and neither is already
//!    superseded there.
//!
//! **(f) refusals are fatal:** the fold fails closed and names the entry. Entries without a
//! `RecordSuperseded` event are never refused; only their release outputs are recorded.

#[cfg(feature = "store")]
mod defs;

#[cfg(feature = "store")]
pub use defs::{command_defs, params_schema, result_schema};

use crate::branch::{Actor, EntryOrigin, EventSource};
use crate::entry::{PrincipalKind, TOMBSTONE};
use fid_canon::CValue;
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;
use std::fmt;

/// Command ID (major 1).
pub const SUPERSEDE_RECORD: &str = "journal.supersede_record";
/// Event type; reserved to [`SUPERSEDE_RECORD`].
pub const RECORD_SUPERSEDED: &str = "RecordSuperseded";
/// Longest reason, in characters.
pub const MAX_REASON_CHARS: usize = 2000;
/// Prefix of the pending-request event types (`CORE.BUS.POL.PROJ`).
pub const REQUEST_EVENT_PREFIX: &str = "Request";
/// The keys of a `RecordSuperseded` body.
pub const BODY_KEYS: [&str; 3] = ["head", "new", "old"];

// ------------------------------------------------------------------ errors

/// Why a call or a committed entry was refused, with its bus error (catalogue v3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupersedeError {
    /// Only a human may run this W3 command (`policy/class_forbidden`).
    ClassForbidden,
    /// A parameter is malformed (`validation/schema`).
    Schema(String),
    /// The records cannot be superseded (`precondition/not_supersedable`).
    NotSupersedable(String),
    /// A committed entry its origin could not have produced (`internal/invariant_violated`).
    Tampered(String),
}

impl SupersedeError {
    /// The bus error `(code, reason)` of catalogue v3.
    pub fn bus_code(&self) -> (&'static str, &'static str) {
        match self {
            SupersedeError::ClassForbidden => ("policy", "class_forbidden"),
            SupersedeError::Schema(_) => ("validation", "schema"),
            SupersedeError::NotSupersedable(_) => ("precondition", "not_supersedable"),
            SupersedeError::Tampered(_) => ("internal", "invariant_violated"),
        }
    }

    /// The bus error code.
    pub fn code(&self) -> &'static str {
        self.bus_code().0
    }

    /// The bus error reason.
    pub fn reason(&self) -> &'static str {
        self.bus_code().1
    }

    /// The detail text.
    pub fn detail(&self) -> String {
        match self {
            SupersedeError::ClassForbidden => "only a human principal may supersede a released record (W3)".into(),
            SupersedeError::Schema(d) | SupersedeError::NotSupersedable(d) | SupersedeError::Tampered(d) => d.clone(),
        }
    }
}

impl fmt::Display for SupersedeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}: {}", self.code(), self.reason(), self.detail())
    }
}

impl std::error::Error for SupersedeError {}

fn tampered(why: impl Into<String>) -> SupersedeError {
    SupersedeError::Tampered(why.into())
}

/// The tombstone digest (`CORE.JRN.ENTRY` REQ-012) as a `fid_types` digest.
pub fn tombstone() -> Digest {
    Digest::from_bytes(TOMBSTONE.0)
}

fn truncate(s: &str) -> String {
    s.chars().take(120).collect()
}

// ------------------------------------------------------------------ calls and events

/// The parameters of one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupersedeParams {
    pub old: Digest,
    pub new: Digest,
    /// Untrusted text.
    pub reason: String,
}

/// The content of one `RecordSuperseded` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Supersession {
    /// The logical head of the branch the call was decided against.
    pub head: Digest,
    pub new: Digest,
    pub old: Digest,
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

impl Supersession {
    /// The event type.
    pub fn type_id(&self) -> &'static str {
        RECORD_SUPERSEDED
    }

    /// The event body `{head, new, old}`.
    pub fn body(&self) -> CValue {
        CValue::Obj(BTreeMap::from([
            ("head".to_string(), text(self.head.as_str())),
            ("new".to_string(), text(self.new.as_str())),
            ("old".to_string(), text(self.old.as_str())),
        ]))
    }

    /// Parses a stored body strictly: exactly [`BODY_KEYS`], each a digest. Anything else is
    /// [`SupersedeError::Tampered`].
    pub fn from_body(body: &CValue) -> Result<Supersession, SupersedeError> {
        let CValue::Obj(o) = body else { return Err(tampered("RecordSuperseded body is not an object")) };
        let keys: Vec<&str> = o.keys().map(String::as_str).collect();
        if keys != BODY_KEYS {
            return Err(tampered(format!("RecordSuperseded has keys {keys:?}, expected {BODY_KEYS:?}")));
        }
        let digest = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => {
                Digest::parse(s).map_err(|_| tampered(format!("RecordSuperseded.{k} is not a digest")))
            }
            _ => Err(tampered(format!("RecordSuperseded.{k} is not a string"))),
        };
        Ok(Supersession { head: digest("head")?, new: digest("new")?, old: digest("old")? })
    }
}

/// Checks the reason: 1 to [`MAX_REASON_CHARS`] characters.
pub fn check_reason(reason: &str) -> Result<(), SupersedeError> {
    let n = reason.chars().count();
    if n == 0 || n > MAX_REASON_CHARS {
        return Err(SupersedeError::Schema(format!("reason has {n} characters; it must have 1 to {MAX_REASON_CHARS}")));
    }
    Ok(())
}

// ------------------------------------------------------------------ the fold

/// One committed entry as its verified header and the commit stage record it. Never built from
/// an event body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupersedeEntry {
    /// Principal kind, command, class, branch and the stage that added the events.
    pub origin: EntryOrigin,
    /// This entry's hash.
    pub hash: Digest,
    /// The header's `promoted_from`.
    pub promoted_from: Option<Digest>,
    /// The header's `reason_commit`.
    pub reason_commit: Option<Digest>,
    /// The header's output digests (command entries only).
    pub outputs: Vec<Digest>,
    /// The logical head of the entry's branch before this entry (`None`: the empty branch), as
    /// the replay of the verified journal gives it.
    pub head_before: Option<Digest>,
}

/// What a supersession left behind: the new record, the supersession entry and the header's
/// reason commitment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Superseded {
    pub new: Digest,
    pub entry: Digest,
    pub reason_commit: Digest,
}

/// The released records and the `superseded_by` index of a journal, per branch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Supersessions {
    released: BTreeMap<(String, Digest), Digest>,
    superseded: BTreeMap<(String, Digest), Superseded>,
}

impl Supersessions {
    /// An empty journal.
    pub fn new() -> Supersessions {
        Supersessions::default()
    }

    /// Folds every entry in append order; the first refused entry stops the fold.
    pub fn replay<'a>(
        entries: impl IntoIterator<Item = (&'a SupersedeEntry, &'a [(String, CValue)])>,
    ) -> Result<Supersessions, SupersedeError> {
        let mut s = Supersessions::new();
        for (e, events) in entries {
            s.apply_entry(e, events)?;
        }
        Ok(s)
    }

    /// The entry that released `record` on `branch`, if it is a released record there.
    pub fn released(&self, branch: &str, record: &Digest) -> Option<Digest> {
        self.released.get(&(branch.to_string(), *record)).copied()
    }

    /// `superseded_by(record)` on `branch`: the new record, the supersession entry and the
    /// reason commitment (REQ-003).
    pub fn superseded_by(&self, branch: &str, record: &Digest) -> Option<Superseded> {
        self.superseded.get(&(branch.to_string(), *record)).copied()
    }

    /// Every supersession of `branch`, keyed by the superseded record.
    pub fn on_branch(&self, branch: &str) -> BTreeMap<Digest, Superseded> {
        self.superseded.iter().filter(|((b, _), _)| b == branch).map(|((_, d), s)| (*d, *s)).collect()
    }

    /// Checks that `old` may be superseded by `new` on `branch` (the planner and the fold).
    fn check_pair(&self, branch: &str, old: &Digest, new: &Digest) -> Result<(), SupersedeError> {
        let refuse = |why: String| Err(SupersedeError::NotSupersedable(why));
        if old == new {
            return refuse(format!("{old} cannot supersede itself"));
        }
        for (role, d) in [("old", old), ("new", new)] {
            if self.released(branch, d).is_none() {
                return refuse(format!("{role} {d} is not a record released by a W3 entry on {}", truncate(branch)));
            }
        }
        for (role, d) in [("old", old), ("new", new)] {
            if let Some(s) = self.superseded_by(branch, d) {
                return refuse(format!("{role} {d} is already superseded by {}", s.new));
            }
        }
        Ok(())
    }

    /// Folds one committed entry (module documentation, The fold). On refusal nothing of the
    /// entry is folded.
    pub fn apply_entry(&mut self, e: &SupersedeEntry, events: &[(String, CValue)]) -> Result<(), SupersedeError> {
        let ours: Vec<&CValue> = events.iter().filter(|(t, _)| t == RECORD_SUPERSEDED).map(|(_, b)| b).collect();
        let branch = e.origin.branch.as_str();
        if ours.is_empty() {
            if e.origin.class == SideEffect::W3 && e.origin.command != SUPERSEDE_RECORD {
                for d in e.outputs.iter().filter(|d| **d != tombstone()) {
                    self.released.entry((branch.to_string(), *d)).or_insert(e.hash);
                }
            }
            return Ok(());
        }
        let at = || format!("entry {}", e.hash);
        let o = &e.origin;
        if o.source != EventSource::Handler
            || o.command != SUPERSEDE_RECORD
            || o.major != 1
            || o.class != SideEffect::W3
            || o.kind != PrincipalKind::Human
        {
            return Err(tampered(format!(
                "{}: RecordSuperseded from {}@{} ({:?}, principal {}, {:?})",
                at(),
                truncate(&o.command),
                o.major,
                o.source,
                o.kind.as_str(),
                o.class
            )));
        }
        if let Some((t, _)) = events.iter().find(|(t, _)| t.starts_with(REQUEST_EVENT_PREFIX)) {
            return Err(tampered(format!("{}: RecordSuperseded next to request event {}", at(), truncate(t))));
        }
        if ours.len() != 1 {
            return Err(tampered(format!("{}: {} RecordSuperseded events in one entry", at(), ours.len())));
        }
        if let Some(p) = e.promoted_from {
            return Err(tampered(format!("{}: RecordSuperseded in a promoted copy of {p}", at())));
        }
        let Some(reason_commit) = e.reason_commit else {
            return Err(tampered(format!("{}: RecordSuperseded without a reason commitment", at())));
        };
        let s = Supersession::from_body(ours[0]).map_err(|err| tampered(format!("{}: {}", at(), err.detail())))?;
        if e.head_before != Some(s.head) {
            return Err(tampered(format!(
                "{}: RecordSuperseded decided against head {} of {}, which was at {}",
                at(),
                s.head,
                truncate(branch),
                e.head_before.map_or("the empty state".to_string(), |h| h.to_string())
            )));
        }
        self.check_pair(branch, &s.old, &s.new).map_err(|err| tampered(format!("{}: {}", at(), err.detail())))?;
        self.superseded.insert((branch.to_string(), s.old), Superseded { new: s.new, entry: e.hash, reason_commit });
        Ok(())
    }
}

// ------------------------------------------------------------------ the planner

/// Checks one call on `branch`, whose logical head is `head` (module documentation, Order of
/// checks), against the folded journal, and returns the event to commit.
pub fn plan(
    actor: Actor,
    state: &Supersessions,
    branch: &str,
    head: Option<Digest>,
    p: &SupersedeParams,
) -> Result<Supersession, SupersedeError> {
    if actor != Actor::Human {
        return Err(SupersedeError::ClassForbidden);
    }
    check_reason(&p.reason)?;
    state.check_pair(branch, &p.old, &p.new)?;
    // A branch holding released records has a head.
    let Some(head) = head else {
        return Err(SupersedeError::NotSupersedable(format!("{} has no entries", truncate(branch))));
    };
    Ok(Supersession { head, new: p.new, old: p.old })
}
