//! The register `gdt-rule-status/v1` and the fold that builds it from the journal
//! (REQ-GDT.RES.STATUS-002 .. 006).

use super::inputs::{CoiInputs, JournalEntry, JournalExtract, TrustedRosters, Utc};
use super::{Reason, Status, StatusError, PROPOSE_COMMAND, REVIEW_COMMAND};
use crate::chars::dprof::{Branch, DprofEvent, ProfileRef, PUBLISH_COMMAND};
use crate::dmt_flow::roster::{ReviewerId, Roster};
use crate::dmt_flow::state::{replay, replay_review, Decision, FlowError, Provenance, RecordedFlow, RowRevision};
use fid_bus::principal::PrincipalKind;
use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_gps::dmt::row::row_from_value;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// Schema tag of the register.
pub const REGISTER_SCHEMA: &str = "gdt-rule-status/v1";

fn journal(reason: &'static str) -> StatusError {
    StatusError::JournalUnreadable { reason }
}

fn unreadable(reason: &'static str) -> StatusError {
    StatusError::RegisterUnreadable { reason }
}

/// The kind of an interpretation rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuleKind {
    /// `dmt_row`: a DMT row key; the revision is the ROWFLOW revision number.
    DmtRow,
    /// `profile`: a defaults profile; the rule ID is the profile ID with its version, and the
    /// revision is always 1 (profiles are immutable; a new version is a new rule).
    Profile,
}

impl RuleKind {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            RuleKind::DmtRow => "dmt_row",
            RuleKind::Profile => "profile",
        }
    }
}

/// One counted approval: the pseudonymous roster reviewer ID, the hash of the journal entry
/// that carries it and that entry's time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Approval {
    /// Roster reviewer ID.
    pub reviewer: ReviewerId,
    /// Hash of the journal entry.
    pub journal_entry: Digest,
    /// Time of the journal entry.
    pub at: Utc,
}

/// One register entry: one revision of one rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleStatusEntry {
    /// DMT key, or profile ID with version.
    pub rule: String,
    /// Rule kind.
    pub kind: RuleKind,
    /// Revision number, from 1.
    pub revision: u32,
    /// Time of the journal entry that created the revision.
    pub revised_at: Utc,
    /// Content digest (`row_content_digest` for a row, the `GDT.CHAR.DPROF` digest for a
    /// profile).
    pub content_digest: Digest,
    /// Principal kind of the revision's author.
    pub author_kind: PrincipalKind,
    /// Status.
    pub status: Status,
    /// Reason, for every status but `signed_off` and `draft`.
    pub reason: Option<Reason>,
    /// Counted approvals, ascending by reviewer ID.
    pub approvals: Vec<Approval>,
}

impl RuleStatusEntry {
    fn to_cvalue(&self) -> CValue {
        let approvals = self
            .approvals
            .iter()
            .map(|a| {
                obj([
                    ("at", s(a.at.as_str())),
                    ("journal_entry", s(a.journal_entry.as_str())),
                    ("reviewer", s(a.reviewer.as_str())),
                ])
            })
            .collect();
        obj([
            ("approvals", CValue::Arr(approvals)),
            ("author_kind", s(self.author_kind.as_str())),
            ("content_digest", s(self.content_digest.as_str())),
            ("kind", s(self.kind.as_str())),
            ("reason", self.reason.map_or(CValue::Null, |r| s(r.as_str()))),
            ("revised_at", s(self.revised_at.as_str())),
            ("revision", CValue::Num(f64::from(self.revision))),
            ("rule", s(&self.rule)),
            ("status", s(self.status.as_str())),
        ])
    }
}

/// What a register was built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltFrom {
    /// Hash of the last journal entry folded.
    pub journal_head: Digest,
    /// Digest of the current roster (`dmt-reviewers/v1`).
    pub roster_digest: Digest,
    /// Digest of the conflict-of-interest inputs ([`CoiInputs::digest`]).
    pub coi_digest: Digest,
    /// Digests of the trusted rosters reviews were checked against, ascending.
    pub trusted_rosters: Vec<Digest>,
}

impl BuiltFrom {
    fn to_cvalue(&self) -> CValue {
        obj([
            ("coi_digest", s(self.coi_digest.as_str())),
            ("journal_head", s(self.journal_head.as_str())),
            ("roster_digest", s(self.roster_digest.as_str())),
            ("trusted_rosters", CValue::Arr(self.trusted_rosters.iter().map(|d| s(d.as_str())).collect())),
        ])
    }

    /// Reads `built_from` out of a register file without trusting anything else in it.
    pub(crate) fn of_file(bytes: &[u8]) -> Result<BuiltFrom, StatusError> {
        let v = fid_canon::parse_with_limits(bytes, super::MAX_JOURNAL_BYTES, 8).map_err(|e| unreadable(e.code()))?;
        let CValue::Obj(top) = &v else { return Err(unreadable("not_an_object")) };
        if top.get("schema") != Some(&s(REGISTER_SCHEMA)) {
            return Err(unreadable("wrong_schema"));
        }
        let Some(CValue::Obj(b)) = top.get("built_from") else { return Err(unreadable("missing_field")) };
        let d = |k: &str| match b.get(k) {
            Some(CValue::Str(t)) => Digest::parse(t).map_err(|_| unreadable("bad_digest")),
            _ => Err(unreadable("missing_field")),
        };
        let Some(CValue::Arr(list)) = b.get("trusted_rosters") else { return Err(unreadable("missing_field")) };
        let trusted_rosters = list
            .iter()
            .map(|x| match x {
                CValue::Str(t) => Digest::parse(t).map_err(|_| unreadable("bad_digest")),
                _ => Err(unreadable("bad_digest")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(BuiltFrom {
            journal_head: d("journal_head")?,
            roster_digest: d("roster_digest")?,
            coi_digest: d("coi_digest")?,
            trusted_rosters,
        })
    }
}

/// The interpretation status register `gdt-rule-status/v1`. Built only by
/// [`build_register`]; its file form is the RFC 8785 canonical JSON of
/// [`RuleStatusRegister::to_cvalue`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleStatusRegister {
    built_from: BuiltFrom,
    entries: Vec<RuleStatusEntry>,
}

impl RuleStatusRegister {
    /// What it was built from.
    pub fn built_from(&self) -> &BuiltFrom {
        &self.built_from
    }

    /// The entries, ascending by (rule ID, revision).
    pub fn entries(&self) -> &[RuleStatusEntry] {
        &self.entries
    }

    /// `{"schema", "built_from", "entries"}`.
    pub fn to_cvalue(&self) -> CValue {
        obj([
            ("built_from", self.built_from.to_cvalue()),
            ("entries", CValue::Arr(self.entries.iter().map(RuleStatusEntry::to_cvalue).collect())),
            ("schema", s(REGISTER_SCHEMA)),
        ])
    }

    /// The file form: RFC 8785 canonical JSON.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, StatusError> {
        to_canonical_bytes(&self.to_cvalue()).map_err(|e| unreadable(e.code()))
    }

    /// The register digest: SHA-256 of [`RuleStatusRegister::canonical_bytes`].
    pub fn digest(&self) -> Result<Digest, StatusError> {
        Ok(digest_of(&self.canonical_bytes()?))
    }
}

/// SHA-256 of bytes, as a `sha256:` digest.
pub(crate) fn digest_of(bytes: &[u8]) -> Digest {
    Digest::from_bytes(digest_bytes(bytes).0)
}

// ------------------------------------------------------------------ the fold

/// One revision's journal entries: the proposal and the review entries naming it.
struct Group<'a> {
    propose: &'a JournalEntry,
    reviews: Vec<&'a JournalEntry>,
}

fn record_str<'a>(e: &'a JournalEntry, key: &str) -> Option<&'a str> {
    match e.record() {
        CValue::Obj(m) => match m.get(key) {
            Some(CValue::Str(t)) => Some(t),
            _ => None,
        },
        _ => None,
    }
}

/// A revision number as journaled (`"1"`, `"2"`, ...: no sign, no leading zero).
fn record_revision(e: &JournalEntry) -> Option<u32> {
    let t = record_str(e, "revision")?;
    if t.starts_with('0') || !t.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    t.parse().ok()
}

fn recorded(e: &JournalEntry) -> RecordedFlow {
    RecordedFlow {
        principal_kind: e.principal_kind(),
        principal_id: e.principal_id().to_owned(),
        record: e.record().clone(),
    }
}

/// Builds the register from the journal up to `head` (default: its last entry).
///
/// - Every `gdt.propose_dmt_row@1` entry starts the next revision of its key (numbers must run
///   1, 2, 3, ... per key, the row must load, and STATE's `replay` must accept the proposal;
///   otherwise `journal_unreadable`).
/// - Every `gdt.review_dmt_row@1` entry is applied to the revision it names with STATE's
///   `replay`, one entry at a time; an entry replay refuses, one naming a revision not yet
///   proposed, or one whose record date is not its entry's date is not applied and so never
///   counts.
/// - `gdt.publish_defaults_profile@1` entries on the working branch add profiles; other
///   commands are ignored.
///
/// `current` is the roster the register records as its own (`built_from.roster_digest`); each
/// review is checked against the roster its record names, which must be in `trusted`.
pub fn build_register(
    journal_extract: &JournalExtract,
    head: Option<Digest>,
    current: &Roster,
    trusted: &TrustedRosters,
    coi: &CoiInputs,
) -> Result<RuleStatusRegister, StatusError> {
    let entries = match head {
        Some(h) => journal_extract.up_to(h)?,
        None => journal_extract.entries(),
    };
    let last = entries.last().ok_or(journal("empty"))?;
    let built_from = BuiltFrom {
        journal_head: last.hash(),
        roster_digest: current.digest(),
        coi_digest: coi.digest()?,
        trusted_rosters: trusted.digests().copied().collect(),
    };

    let mut rows: BTreeMap<&str, Vec<Group<'_>>> = BTreeMap::new();
    let mut profiles: BTreeMap<String, (ProfileRef, &JournalEntry)> = BTreeMap::new();
    for e in entries {
        match e.command() {
            PROPOSE_COMMAND => {
                let key = record_str(e, "key").ok_or(journal("malformed_record"))?;
                let groups = rows.entry(key).or_default();
                let want = u32::try_from(groups.len()).ok().and_then(|n| n.checked_add(1));
                if record_revision(e).is_none() || record_revision(e) != want {
                    return Err(journal("revision_out_of_order"));
                }
                groups.push(Group { propose: e, reviews: Vec::new() });
            }
            REVIEW_COMMAND => {
                let target = record_str(e, "key").zip(record_revision(e));
                let group = target.and_then(|(k, r)| {
                    rows.get_mut(k).and_then(|g| g.get_mut(usize::try_from(r).ok()?.checked_sub(1)?))
                });
                if let Some(g) = group {
                    g.reviews.push(e);
                }
            }
            PUBLISH_COMMAND => add_profile(&mut profiles, e)?,
            _ => {}
        }
    }

    let mut out = Vec::new();
    for (key, groups) in &rows {
        let mut prev: Option<RowRevision> = None;
        for (i, g) in groups.iter().enumerate() {
            let (rev, applied) = rebuild(prev.as_ref(), g, trusted)?;
            let latest = i + 1 == groups.len();
            out.push(row_entry(key, &rev, &applied, g.propose, latest, coi));
            prev = Some(rev);
        }
    }
    for (id, (r, e)) in profiles {
        out.push(RuleStatusEntry {
            rule: id,
            kind: RuleKind::Profile,
            revision: 1,
            revised_at: e.utc().clone(),
            content_digest: *r.digest(),
            author_kind: e.principal_kind(),
            status: Status::Provisional,
            reason: Some(Reason::NoSignoffPath),
            approvals: Vec::new(),
        });
    }
    out.sort_by(|a, b| (a.rule.as_str(), a.revision).cmp(&(b.rule.as_str(), b.revision)));
    Ok(RuleStatusRegister { built_from, entries: out })
}

fn add_profile<'a>(
    profiles: &mut BTreeMap<String, (ProfileRef, &'a JournalEntry)>,
    e: &'a JournalEntry,
) -> Result<(), StatusError> {
    let bytes = to_canonical_bytes(e.record()).map_err(|_| journal("bad_profile_event"))?;
    let DprofEvent::Publish(p) = DprofEvent::from_canonical_bytes(&bytes).map_err(|_| journal("bad_profile_event"))?
    else {
        return Err(journal("bad_profile_event"));
    };
    if p.principal().as_str() != e.principal_kind().as_str() {
        return Err(journal("principal_kind"));
    }
    if p.branch() != Branch::Working {
        return Ok(());
    }
    let id = p.reference().id().to_string();
    match profiles.get(&id) {
        Some((r, _)) if r != p.reference() => Err(journal("profile_changed")),
        Some(_) => Ok(()),
        None => {
            profiles.insert(id, (p.reference().clone(), e));
            Ok(())
        }
    }
}

/// Rebuilds one revision: the proposal (which must replay), then each review entry that
/// replays on top of the entries applied so far. Returns the revision and the applied review
/// entries, in the order of the revision's reviews.
fn rebuild<'a>(
    prev: Option<&RowRevision>,
    g: &Group<'a>,
    trusted: &TrustedRosters,
) -> Result<(RowRevision, Vec<&'a JournalEntry>), StatusError> {
    let row = row_from_value(g.propose.row().ok_or(journal("missing_row"))?).map_err(|_| journal("bad_row"))?;
    if record_str(g.propose, "at") != Some(g.propose.utc().date()) {
        return Err(journal("date_mismatch"));
    }
    let mut rev = replay(prev, row, &[recorded(g.propose)], trusted).map_err(|e| journal(proposal_reason(&e)))?;
    let mut applied = Vec::new();
    // One STATE step per entry (linear): `replay_review` is exactly `replay`'s loop body.
    for e in &g.reviews {
        if record_str(e, "at") != Some(e.utc().date()) {
            continue;
        }
        if let Ok(r) = replay_review(&rev, &recorded(e), trusted) {
            rev = r;
            applied.push(*e);
        }
    }
    Ok((rev, applied))
}

fn proposal_reason(e: &FlowError) -> &'static str {
    match e {
        FlowError::ReplayMismatch { reason } => reason,
        other => other.code(),
    }
}

/// The register entry of one rebuilt revision (REQ-003 .. 005).
fn row_entry(
    key: &str,
    rev: &RowRevision,
    applied: &[&JournalEntry],
    propose: &JournalEntry,
    latest: bool,
    coi: &CoiInputs,
) -> RuleStatusEntry {
    let approvals = counted(key, rev, applied, coi);
    let distinct: BTreeSet<&ReviewerId> = approvals.iter().map(|a| &a.reviewer).collect();
    let (status, reason) = if !latest {
        (Status::Provisional, Some(Reason::Superseded))
    } else if rev.provenance() == Provenance::Agent {
        (Status::Draft, None)
    } else if distinct.len() >= 2 {
        (Status::SignedOff, None)
    } else {
        (Status::Provisional, Some(Reason::AwaitingReview))
    };
    RuleStatusEntry {
        rule: key.to_owned(),
        kind: RuleKind::DmtRow,
        revision: rev.revision(),
        revised_at: propose.utc().clone(),
        content_digest: rev.content_digest(),
        author_kind: rev.author_kind(),
        status,
        reason,
        approvals,
    }
}

/// The counted approvals of a revision (REQ-003): live (after the last `request_changes`),
/// naming the revision's digest, from a `human` entry principal, and breaking no conflict
/// rule: a loaded reviewer register must list the reviewer, the reviewer must not be recused
/// from the rule, and no other live approver may share the reviewer's organisation ID.
fn counted(rule: &str, rev: &RowRevision, applied: &[&JournalEntry], coi: &CoiInputs) -> Vec<Approval> {
    let mut live: Vec<Approval> = Vec::new();
    for (r, e) in rev.reviews().iter().zip(applied) {
        match r.decision() {
            Decision::RequestChanges => live.clear(),
            Decision::Approve => {
                if r.reviewed_digest() == rev.content_digest() && e.principal_kind() == PrincipalKind::Human {
                    live.push(Approval {
                        reviewer: r.reviewer().clone(),
                        journal_entry: e.hash(),
                        at: e.utc().clone(),
                    });
                }
            }
        }
    }
    let org = |id: &ReviewerId| coi.reviewer(id.as_str()).flatten().map(|c| c.org_id.as_str());
    let mut out: Vec<Approval> = live
        .iter()
        .filter(|a| match coi.reviewer(a.reviewer.as_str()) {
            None => true,
            Some(None) => false,
            Some(Some(c)) => {
                !c.recusals.contains(rule)
                    && !live.iter().any(|o| o.reviewer != a.reviewer && org(&o.reviewer) == Some(c.org_id.as_str()))
            }
        })
        .cloned()
        .collect();
    out.sort();
    out
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}
