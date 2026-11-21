//! Supersession of released records (spec node `CORE.JRN.UNDO.SUPER`).
//!
//! A released record is never undone; it is superseded. `journal.supersede_record@1` (W3, D0,
//! not undoable, MCP projection `request_only`) records that a new released record replaces an
//! old one, and leaves the old record's entry unchanged and visible. [`plan`] checks one call
//! and returns the one [`Supersession`] to commit, or a [`SupersedeError`] whose
//! [`SupersedeError::bus_code`] is a code and reason of the bus error catalogue (v3).
//! [`Supersessions`] folds the committed journal into the released-record and `superseded_by`
//! indexes, and refuses any supersession or release its entry's origin could not have
//! produced. [`Supersessions::precommit`] is the commit stage's check. With feature `store`,
//! [`command_defs`] declares the command. The bus handler is in `fid-journal-cmd`.
//!
//! # Pinned rules
//!
//! - **Released record.** A digest is a released record at a head when it is an output digest
//!   (not the tombstone) of a committed W3 command entry in that head's ancestry, signed by a
//!   human principal whose command added the entry's events (adding stage `handler`, or
//!   `bus_release` where the commit stage added release protection to that human W3 entry),
//!   other than a `journal.supersede_record@1` entry and other than a promoted copy. A W3
//!   entry with release outputs signed by any other principal kind (agent, script, ...) or
//!   whose events the bus routing module added (`bus_route`) is refused as fatal: no such
//!   entry can be a release.
//! - **Ancestry.** Every lookup is made at a head: the logical head of the branch being read.
//!   It walks that head's logical ancestry (each entry's logical parent, the head of its branch
//!   before it), so a fork created after a supersession, and a branch fast-forwarded over one,
//!   see it; a fork created before it does not. The fold therefore takes the entries of **all
//!   branches in commit order**.
//! - **Promoted copies** (entries with `promoted_from`) are **skipped** for both indexes and
//!   are not refused: a copy re-records on the target an entry whose authority is the
//!   original's, decided on the source branch; the promotion that made it is W2 (or a
//!   protected W3 promotion bound to a diff), not the human W3 act of releasing or
//!   superseding there. Counting it would let a promotion mint releases or supersessions on
//!   the target without that act; skipping it adds nothing to either index, so it cannot carry
//!   authority (REQ-CORE.RPL.RECON.FOLD-008 (f)). A supersession reaches another branch only
//!   through ancestry (a fork or a fast-forward); `CORE.JRN.BR.PROMO` and its Rebaser must
//!   refuse to promote or rebase a range holding `RecordSuperseded`, so a supersession is never
//!   dropped silently on a target.
//! - **Who.** Only a human supersedes. The caller is the [`Actor`] the bus derived from the
//!   channel; anything else is `policy/class_forbidden` (W3). Over MCP the bus turns an agent's
//!   call into a pending request (`CORE.BUS.POL.PROJ`); nothing is superseded.
//! - **Order of checks** (first refusal wins): caller; reason of 1 to [`MAX_REASON_CHARS`]
//!   characters (`validation/schema`); old equal to new; old, then new, a released record at
//!   the call's head; old, then new, not already superseded there. Every refusal after the
//!   reason is `precondition/not_supersedable`.
//! - **Reason.** The reason is untrusted text. It is not copied into the event: the entry
//!   records it as its `reason` (a salted, detachable commitment, `CORE.JRN.ENTRY`), and the
//!   fold keeps the header's `reason_commit` as the supersession's commitment.
//! - **Event.** `RecordSuperseded {head, new, old}`, where `head` is the logical head of the
//!   branch the call was decided against. The commit stage commits the entry only if that is
//!   still the branch's head (compare-and-swap; `conflict/head_moved` otherwise).
//! - **Parameters commitment** ([`params_commitment`]): the `sha256:` digest of the RFC 8785
//!   bytes of the call's parameters `{new, old, reason}`, the same value as the bus
//!   `EntryOrigin` parameters commitment.
//!
//! # The fold ([`Supersessions::apply_entry`], REQ-CORE.RPL.RECON.FOLD-008)
//!
//! Each entry is folded with its [`SupersedeEntry`], built only from the verified header, its
//! payload and the commit stage (never from an event body). Every entry: its hash is not
//! folded already, and its `head_before` is `None` or an entry folded before (fatal
//! otherwise). A promoted copy is then skipped. An entry without a `RecordSuperseded` event
//! only records its releases (above). An entry that holds a `RecordSuperseded` event is
//! accepted only if all of these hold, else the whole entry is refused as
//! `internal/invariant_violated` and nothing of it is folded:
//!
//! 1. **(a) origin:** its events were added by the handler of `journal.supersede_record@1`,
//!    class W3, and the principal is human;
//! 2. **(b) reserved effect:** `RecordSuperseded` is reserved to that command in
//!    `fid_registry::RESERVED_EFFECTS`; any other command's entry holding it is refused by 1;
//! 3. **(c) requests:** the entry holds no `Request*` event (a supersession is a direct human
//!    action; an approved agent request does not carry it);
//! 4. exactly one `RecordSuperseded` event, the entry records a parameters commitment and the
//!    header a `reason_commit`; the body has exactly the keys `head`, `new`, `old`, all
//!    digests;
//! 5. **(a) parameters:** while the reason's opening (salt and text) is still in the payload,
//!    `reason_commit` opens to it, and the parameters commitment is that of
//!    `{new, old, reason}` with the event's `new` and `old` and the opened reason: the event is
//!    the call the principal made. Once the reason is erased the commitment cannot be
//!    recomputed, and only its presence is required;
//! 6. **(d) decided-against state:** `head` is the logical head of the entry's branch before
//!    the entry, as the caller gives it from the verified journal: a replayed or stale event is
//!    refused;
//! 7. old and new differ, both are released records at `head`, and neither is already
//!    superseded there.
//!
//! **(f) refusals are fatal:** the fold fails closed and names the entry. Only promoted copies
//! are skipped (above).

#[cfg(feature = "store")]
mod defs;

#[cfg(feature = "store")]
pub use defs::{command_defs, params_schema, result_schema};

use crate::branch::{Actor, EntryOrigin, EventSource};
use crate::entry::{commitment, PrincipalKind, Salt, TOMBSTONE};
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
    /// At the commit stage: the branch head is no longer the one the call was decided against
    /// (`conflict/head_moved`).
    HeadMoved(String),
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
            SupersedeError::HeadMoved(_) => ("conflict", "head_moved"),
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
            SupersedeError::Schema(d)
            | SupersedeError::NotSupersedable(d)
            | SupersedeError::HeadMoved(d)
            | SupersedeError::Tampered(d) => d.clone(),
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

impl SupersedeParams {
    /// The parameters as the call's JSON object `{new, old, reason}`.
    pub fn to_value(&self) -> CValue {
        CValue::Obj(BTreeMap::from([
            ("new".to_string(), text(self.new.as_str())),
            ("old".to_string(), text(self.old.as_str())),
            ("reason".to_string(), text(&self.reason)),
        ]))
    }
}

/// The parameters commitment of a call: the `sha256:` digest of the RFC 8785 bytes of
/// `{new, old, reason}` (the bus `EntryOrigin` parameters commitment of the same call).
pub fn params_commitment(p: &SupersedeParams) -> Result<Digest, SupersedeError> {
    fid_canon::digest(&p.to_value())
        .map(|d| Digest::from_bytes(d.0))
        .map_err(|e| SupersedeError::Schema(format!("parameters have no canonical form: {}", e.code())))
}

/// The salted commitment of a reason (`CORE.JRN.ENTRY` REQ-003): what the header's
/// `reason_commit` holds for `reason` under `salt`.
pub fn reason_commitment(salt: &Salt, reason: &str) -> Result<Digest, SupersedeError> {
    commitment(salt, &CValue::Str(reason.to_string()))
        .map(|d| Digest::from_bytes(d.0))
        .map_err(|e| SupersedeError::Schema(format!("reason has no canonical form: {e}")))
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

/// The opening of an entry's `reason_commit`: the salt and the text from the entry's
/// detachable payload (`CORE.JRN.ENTRY`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasonOpening {
    pub salt: Salt,
    pub reason: String,
}

/// One committed entry as its verified header, its payload and the commit stage record it.
/// Never built from an event body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupersedeEntry {
    /// Principal kind, command, class, branch and the stage that added the events.
    pub origin: EntryOrigin,
    /// The header's principal identity commitment (`principal.commit`).
    pub principal: Digest,
    /// The parameters commitment of the entry's command (the bus `EntryOrigin`'s, from the
    /// entry's arguments); `None` if the entry records none.
    pub params_commitment: Option<Digest>,
    /// This entry's hash.
    pub hash: Digest,
    /// The header's `promoted_from`.
    pub promoted_from: Option<Digest>,
    /// The header's `reason_commit`.
    pub reason_commit: Option<Digest>,
    /// The opening of `reason_commit` while the payload still holds it (`None` once erased).
    pub reason_opening: Option<ReasonOpening>,
    /// The header's output digests (command entries only).
    pub outputs: Vec<Digest>,
    /// The logical head of the entry's branch before this entry, its logical parent (`None`:
    /// the empty branch), as the replay of the verified journal gives it.
    pub head_before: Option<Digest>,
}

/// What a supersession left behind: the new record, the supersession entry, the header's
/// reason commitment and the superseding principal's identity commitment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Superseded {
    pub new: Digest,
    pub entry: Digest,
    pub reason_commit: Digest,
    pub by: Digest,
}

/// What the fold did with one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folded {
    /// Neither a release nor a supersession.
    Plain,
    /// A release of this many records.
    Released(usize),
    /// A supersession.
    Superseded,
    /// A promoted copy, skipped for both indexes.
    PromotedCopy,
}

/// One folded entry: its logical parent and what it added.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    parent: Option<Digest>,
    released: Vec<Digest>,
    superseded: Option<(Digest, Superseded)>,
}

/// The released records and the `superseded_by` index of a journal, per entry, looked up along
/// a head's ancestry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Supersessions {
    nodes: BTreeMap<Digest, Node>,
}

/// The logical ancestry of a head, newest first. It ends because every node's parent was
/// folded before it.
struct Ancestry<'a> {
    nodes: &'a BTreeMap<Digest, Node>,
    at: Option<Digest>,
}

impl<'a> Iterator for Ancestry<'a> {
    type Item = (Digest, &'a Node);

    fn next(&mut self) -> Option<(Digest, &'a Node)> {
        let h = self.at?;
        let n = self.nodes.get(&h)?;
        self.at = n.parent;
        Some((h, n))
    }
}

impl Supersessions {
    /// An empty journal.
    pub fn new() -> Supersessions {
        Supersessions::default()
    }

    /// Folds every entry in commit order; the first refused entry stops the fold.
    pub fn replay<'a>(
        entries: impl IntoIterator<Item = (&'a SupersedeEntry, &'a [(String, CValue)])>,
    ) -> Result<Supersessions, SupersedeError> {
        let mut s = Supersessions::new();
        for (e, events) in entries {
            s.apply_entry(e, events)?;
        }
        Ok(s)
    }

    fn ancestry(&self, at: Option<Digest>) -> Ancestry<'_> {
        Ancestry { nodes: &self.nodes, at }
    }

    /// Whether `entry` has been folded.
    pub fn contains(&self, entry: &Digest) -> bool {
        self.nodes.contains_key(entry)
    }

    /// The first entry that released `record` in the ancestry of `at`, if it is a released
    /// record there.
    pub fn released(&self, at: Option<Digest>, record: &Digest) -> Option<Digest> {
        self.ancestry(at).filter(|(_, n)| n.released.contains(record)).last().map(|(h, _)| h)
    }

    /// Every released record in the ancestry of `at`, with the first entry that released it.
    pub fn released_records(&self, at: Option<Digest>) -> BTreeMap<Digest, Digest> {
        let mut out = BTreeMap::new();
        for (h, n) in self.ancestry(at) {
            for d in &n.released {
                out.insert(*d, h);
            }
        }
        out
    }

    /// `superseded_by(record)` at `at`, the logical head of the branch being read: the new
    /// record, the supersession entry, the reason commitment and who superseded (REQ-003).
    pub fn superseded_by(&self, at: Option<Digest>, record: &Digest) -> Option<Superseded> {
        self.ancestry(at).find_map(|(_, n)| match n.superseded {
            Some((old, s)) if old == *record => Some(s),
            _ => None,
        })
    }

    /// Every supersession in the ancestry of `at`, keyed by the superseded record.
    pub fn supersessions(&self, at: Option<Digest>) -> BTreeMap<Digest, Superseded> {
        self.ancestry(at).filter_map(|(_, n)| n.superseded).collect()
    }

    /// Checks that `old` may be superseded by `new` at `at` (the planner and the fold).
    fn check_pair(&self, at: Option<Digest>, old: &Digest, new: &Digest) -> Result<(), SupersedeError> {
        let refuse = |why: String| Err(SupersedeError::NotSupersedable(why));
        if old == new {
            return refuse(format!("{old} cannot supersede itself"));
        }
        let mut released = [false; 2];
        let mut superseded: [Option<Digest>; 2] = [None; 2];
        for (_, n) in self.ancestry(at) {
            for (i, d) in [old, new].into_iter().enumerate() {
                released[i] |= n.released.contains(d);
                if let Some((o, s)) = n.superseded {
                    if o == *d && superseded[i].is_none() {
                        superseded[i] = Some(s.new);
                    }
                }
            }
        }
        let at_text = at.map_or("the empty state".to_string(), |h| h.to_string());
        for (i, (role, d)) in [("old", old), ("new", new)].into_iter().enumerate() {
            if !released[i] {
                return refuse(format!("{role} {d} is not a record released by a W3 entry at {at_text}"));
            }
        }
        for (i, (role, d)) in [("old", old), ("new", new)].into_iter().enumerate() {
            if let Some(by) = superseded[i] {
                return refuse(format!("{role} {d} is already superseded by {by}"));
            }
        }
        Ok(())
    }

    /// The node one entry would add, or why it is refused. `stale` builds the error of a
    /// decided-against mismatch (fatal in the fold, `conflict/head_moved` at the commit stage).
    fn verdict(
        &self,
        e: &SupersedeEntry,
        events: &[(String, CValue)],
        stale: &dyn Fn(String) -> SupersedeError,
    ) -> Result<(Node, Folded), SupersedeError> {
        let at = || format!("entry {}", e.hash);
        if self.nodes.contains_key(&e.hash) {
            return Err(tampered(format!("{}: folded twice", at())));
        }
        if let Some(p) = e.head_before {
            if !self.nodes.contains_key(&p) {
                return Err(tampered(format!("{}: its logical parent {p} is not an entry folded before", at())));
            }
        }
        let mut node = Node { parent: e.head_before, released: Vec::new(), superseded: None };
        if e.promoted_from.is_some() {
            return Ok((node, Folded::PromotedCopy));
        }
        let o = &e.origin;
        let ours: Vec<&CValue> = events.iter().filter(|(t, _)| t == RECORD_SUPERSEDED).map(|(_, b)| b).collect();
        if ours.is_empty() {
            if o.class != SideEffect::W3 || o.command == SUPERSEDE_RECORD {
                return Ok((node, Folded::Plain));
            }
            let mut released: Vec<Digest> = e.outputs.iter().copied().filter(|d| *d != tombstone()).collect();
            released.sort();
            released.dedup();
            if released.is_empty() {
                return Ok((node, Folded::Plain));
            }
            if o.kind != PrincipalKind::Human {
                return Err(tampered(format!("{}: a W3 release by a principal of kind {}", at(), o.kind.as_str())));
            }
            if o.source == EventSource::BusRoute {
                return Err(tampered(format!("{}: a W3 release whose events the bus routing module added", at())));
            }
            let n = released.len();
            node.released = released;
            return Ok((node, Folded::Released(n)));
        }
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
        let Some(committed_params) = e.params_commitment else {
            return Err(tampered(format!("{}: RecordSuperseded without a parameters commitment", at())));
        };
        let Some(reason_commit) = e.reason_commit else {
            return Err(tampered(format!("{}: RecordSuperseded without a reason commitment", at())));
        };
        let s = Supersession::from_body(ours[0]).map_err(|err| tampered(format!("{}: {}", at(), err.detail())))?;
        if let Some(open) = &e.reason_opening {
            let opened = reason_commitment(&open.salt, &open.reason).map_err(|err| tampered(err.detail()))?;
            if opened != reason_commit {
                return Err(tampered(format!("{}: reason_commit does not open to the entry's reason", at())));
            }
            let call = SupersedeParams { old: s.old, new: s.new, reason: open.reason.clone() };
            if params_commitment(&call).map_err(|err| tampered(err.detail()))? != committed_params {
                return Err(tampered(format!(
                    "{}: RecordSuperseded {} -> {} is not the call the entry's parameters commit to",
                    at(),
                    s.old,
                    s.new
                )));
            }
        }
        if e.head_before != Some(s.head) {
            return Err(stale(format!(
                "{}: RecordSuperseded decided against head {} of {}, which was at {}",
                at(),
                s.head,
                truncate(&o.branch),
                e.head_before.map_or("the empty state".to_string(), |h| h.to_string())
            )));
        }
        self.check_pair(e.head_before, &s.old, &s.new)
            .map_err(|err| tampered(format!("{}: {}", at(), err.detail())))?;
        node.superseded = Some((s.old, Superseded { new: s.new, entry: e.hash, reason_commit, by: e.principal }));
        Ok((node, Folded::Superseded))
    }

    /// Folds one committed entry (module documentation, The fold). On refusal nothing of the
    /// entry is folded.
    pub fn apply_entry(&mut self, e: &SupersedeEntry, events: &[(String, CValue)]) -> Result<Folded, SupersedeError> {
        let (node, folded) = self.verdict(e, events, &SupersedeError::Tampered)?;
        self.nodes.insert(e.hash, node);
        Ok(folded)
    }

    /// The commit stage's check of the entry of call `p`, run atomically with the commit: the
    /// entry holds the opening of its reason, which is the call's reason (so `reason_commit`
    /// opens to it and the parameters commitment is checked against the event's `old` and
    /// `new`), and the fold's rules hold, a decided-against mismatch being
    /// `conflict/head_moved`.
    pub fn precommit(
        &self,
        e: &SupersedeEntry,
        events: &[(String, CValue)],
        p: &SupersedeParams,
    ) -> Result<Folded, SupersedeError> {
        let at = || format!("entry {}", e.hash);
        let Some(open) = &e.reason_opening else {
            return Err(tampered(format!("{}: the commit stage holds no opening of the reason", at())));
        };
        if open.reason != p.reason {
            return Err(tampered(format!("{}: the entry's reason is not the call's", at())));
        }
        let (_, folded) = self.verdict(e, events, &SupersedeError::HeadMoved)?;
        if folded != Folded::Superseded {
            return Err(tampered(format!("{}: the call's entry is not a supersession", at())));
        }
        Ok(folded)
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
    state.check_pair(head, &p.old, &p.new)?;
    // A head with released records in its ancestry is an entry.
    let Some(head) = head else {
        return Err(SupersedeError::NotSupersedable(format!("{} has no entries", truncate(branch))));
    };
    Ok(Supersession { head, new: p.new, old: p.old })
}
