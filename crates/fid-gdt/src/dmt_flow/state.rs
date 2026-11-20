//! Row revisions, the review state machine and its invariants (spec node
//! `GDT.RES.ROWFLOW.STATE`, D-009, D-016, D-045).
//!
//! The pure core of the DMT workflow. Every command of `GDT.RES.ROWFLOW` (propose, review,
//! adopt) goes through the functions here, so the rules cannot drift between commands:
//!
//! - [`propose`] starts a new [`RowRevision`] of a key: number one more than the previous
//!   revision, state `draft`, no reviews, `derived_from` naming the previous revision. So any
//!   new content, including a change to a `signed_off` row, starts over and the old sign-off
//!   covers only the old revision (REQ-006, REQ-GDT.RES.ROWFLOW-003). A row whose own `review`
//!   field is not `draft` is refused (`review_field_not_draft`): review state is never taken
//!   from a proposer.
//! - [`transition`] applies one review event to a revision (REQ-001 .. 005, 007).
//! - [`replay`] rebuilds a revision from its journaled [`FlowRecord`]s, re-checking every
//!   invariant and requiring each rebuilt record to equal the journaled one byte for byte.
//!
//! **A row leaves the workflow only through [`RowRevision::reviewed_row`]**, whose `review`
//! field is set from [`RowRevision::review_state`], the state this module derived. The row as
//! proposed is not exposed.
//!
//! **Principals come from the bus.** Authors and reviewers are [`Principal`]s, which only
//! `fid_bus::principal::SessionTable::derive_principal` can build from an authenticated
//! channel; no kind or identity is ever taken from request parameters. Only a `human`
//! principal may review (`reviewer_not_human`), and a revision whose author is not human
//! accepts no review until a person re-proposes it (`agent_authored_revision`); script,
//! service, device and system authors are refused like agents (fail closed). The review
//! command that wraps [`transition`] is `human_only` and request-only on MCP
//! (`GDT.RES.ROWFLOW.REVIEW`, the `AUTO.REC.ADOPT` pattern).
//!
//! **The content digest is never taken from a caller.** [`RowRevision::content_digest`] is
//! [`fid_gps::dmt::row_content_digest`] of the revision's own row: SHA-256 over the RFC 8785
//! canonical JSON of every `DmtRow` field except `review` (REQ-GDT.RES.STATUS-001), so a
//! flipped `synthetic` flag, a changed note or any other field is new content. A review names
//! the digest the reviewer read; any other digest is `stale_review` (REQ-GDT.RES.ROWFLOW-006).
//!
//! **Journaling.** Each successful call returns a [`Step`]: the new revision and the
//! [`FlowRecord`] the command journals for it. A failed call returns an error and the input
//! revision is untouched (it is only borrowed), so nothing is journaled.
//!
//! **Pseudonymous reviewers.** A review record names the reviewer by roster reviewer ID only
//! (for example `r1`), never by display name or principal identity. The roster has no consent
//! field, so no record here ever carries more.
//!
//! **`signed_off` here is not a counted sign-off.** It means two distinct roster reviewers
//! hold live approvals. Conflict-of-interest rules (`OUTR.GDTC.REVIEW`: one organisation,
//! sponsorship, recusal) are not checked here; `GDT.RES.STATUS` recounts approvals from the
//! journal with them, and only its verified register says whether a rule is signed off.
//!
//! **Check order of a review event** (the first failure is returned):
//! 1. revision `signed_off`: `bad_transition{signed_off, <event>}` (a signed-off row changes
//!    only through a new revision);
//! 2. author not human: `agent_authored_revision`;
//! 3. reviewer not human: `reviewer_not_human`;
//! 4. review date before the proposal date or before the last review: `backdated_review`;
//! 5. reviewed digest differs from the revision's: `stale_review`;
//! 6. reviewer identity equals the author's (ASCII case-insensitive): `reviewer_is_author`;
//! 7. no roster role for the row's dialect on the review date: `reviewer_not_in_roster`;
//! 8. the author holds that same roster reviewer ID: `reviewer_is_author`;
//! 9. an approval from a reviewer whose approval is still live: `duplicate_reviewer`;
//! 10. attestations: `field_too_long{attestations}` past [`MAX_ATTESTATIONS`],
//!     `field_too_long{attestation}` on an over-long label,
//!     `bad_attestation{placeholder_not_allowed}` for [`Attestation::NotRequiredPlaceholder`]
//!     on a row that is not a placeholder (status `unsupported`, every chain empty), then
//!     `attestation_missing{designator, edition}` for the first cited pair not attested,
//!     unless a placeholder row carries the placeholder attestation (REQ-004);
//! 11. IDR: `bad_idr{idr_on_approve}`, `bad_idr{bad_question_code}`,
//!     `bad_idr{bad_position_count}`, then per position `field_too_long{position}` (over 280
//!     characters) and `bad_idr{bad_position}` (empty or a control character).
//!
//! **Approvals.** An approval is *live* until a later `request_changes` on the same revision;
//! `request_changes` moves the revision to `draft` and clears the live approvals (REQ-005),
//! while the review history is kept. The revision is `signed_off` exactly when two distinct
//! reviewers hold live approvals (REQ-003). No floating point, no hash maps, no panics.

use super::roster::{Date, Dialect as RosterDialect, ReviewerId, Roster};
use super::status::TrustedRosters;
use fid_bus::principal::{Principal, PrincipalKind};
use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_gps::dmt::{row_content_digest, Dialect, DmtKey, DmtRow, ReviewState, Reviewers, RowStatus};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Longest IDR position statement, in Unicode scalar values (REQ-005).
pub const MAX_IDR_POSITION_CHARS: usize = 280;
/// Most position statements in one IDR.
pub const MAX_IDR_POSITIONS: usize = 8;
/// Longest IDR question code, in bytes (`[a-z][a-z0-9_]{0,63}`).
pub const MAX_QUESTION_CODE_LEN: usize = 64;
/// Most attestations one review may carry.
pub const MAX_ATTESTATIONS: usize = 32;
/// Longest attested designator or edition, in bytes (the schema's label limit).
pub const MAX_ATTESTATION_LABEL_LEN: usize = 32;

// ------------------------------------------------------------------ errors

/// Errors of the state machine. [`FlowError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowError {
    /// `agent_authored_revision`: the revision's author is not a human principal (REQ-001).
    AgentAuthoredRevision,
    /// `attestation_missing{designator, edition}`: a cited edition not attested (REQ-004).
    AttestationMissing {
        /// Designator of the first cited pair not attested.
        designator: String,
        /// Its edition.
        edition: String,
    },
    /// `bad_attestation{reason}`: `placeholder_not_allowed` on a row that is not a placeholder.
    BadAttestation {
        /// The reason.
        reason: &'static str,
    },
    /// `reviewer_is_author` (REQ-007).
    ReviewerIsAuthor,
    /// `duplicate_reviewer` (REQ-007).
    DuplicateReviewer,
    /// `reviewer_not_in_roster` (REQ-007).
    ReviewerNotInRoster,
    /// `reviewer_not_human` (REQ-007).
    ReviewerNotHuman,
    /// `field_too_long{field}`: an IDR position over 280 characters, or too many or too long
    /// attestations.
    FieldTooLong {
        /// `position`, `attestations` or `attestation`.
        field: &'static str,
    },
    /// `bad_transition{state, event}`: the event is not allowed in this state.
    BadTransition {
        /// The revision's state.
        state: FlowState,
        /// `approve`, `request_changes` or `propose`.
        event: &'static str,
    },
    /// `stale_review`: the review names a content digest other than the revision's
    /// (REQ-GDT.RES.ROWFLOW-006).
    StaleReview,
    /// `backdated_review`: the review date is before the proposal or the last review.
    BackdatedReview,
    /// `review_field_not_draft`: a proposed row whose own `review` field is not `draft`.
    ReviewFieldNotDraft,
    /// `key_mismatch`: a proposal for a different key than the previous revision's.
    KeyMismatch,
    /// `bad_idr{reason}`: an IDR that breaks its format.
    BadIdr {
        /// `idr_on_approve`, `bad_question_code`, `bad_position_count` or `bad_position`.
        reason: &'static str,
    },
    /// `content_digest_unavailable{reason}`: the row's canonical form could not be written.
    ContentDigest {
        /// The schema or canonical-JSON error code.
        reason: &'static str,
    },
    /// `reviewers_invalid{reason}`: the live approvals do not form a valid schema reviewer
    /// list (unreachable with roster reviewer IDs; reported, never ignored).
    ReviewersInvalid {
        /// The schema's reason.
        reason: &'static str,
    },
    /// `unknown_roster`: a journaled review names a roster digest replay was not given.
    UnknownRoster,
    /// `replay_mismatch{reason}`: the journaled records do not rebuild the same revision.
    ReplayMismatch {
        /// `empty`, `first_not_propose`, `propose_inside_revision`, `malformed_record`,
        /// `principal_kind`, `reviewer_differs` or `record_differs`.
        reason: &'static str,
    },
}

impl FlowError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            FlowError::AgentAuthoredRevision => "agent_authored_revision",
            FlowError::AttestationMissing { .. } => "attestation_missing",
            FlowError::BadAttestation { .. } => "bad_attestation",
            FlowError::ReviewerIsAuthor => "reviewer_is_author",
            FlowError::DuplicateReviewer => "duplicate_reviewer",
            FlowError::ReviewerNotInRoster => "reviewer_not_in_roster",
            FlowError::ReviewerNotHuman => "reviewer_not_human",
            FlowError::FieldTooLong { .. } => "field_too_long",
            FlowError::BadTransition { .. } => "bad_transition",
            FlowError::StaleReview => "stale_review",
            FlowError::BackdatedReview => "backdated_review",
            FlowError::ReviewFieldNotDraft => "review_field_not_draft",
            FlowError::KeyMismatch => "key_mismatch",
            FlowError::BadIdr { .. } => "bad_idr",
            FlowError::ContentDigest { .. } => "content_digest_unavailable",
            FlowError::ReviewersInvalid { .. } => "reviewers_invalid",
            FlowError::UnknownRoster => "unknown_roster",
            FlowError::ReplayMismatch { .. } => "replay_mismatch",
        }
    }
}

impl fmt::Display for FlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FlowError::AttestationMissing { designator, edition } => {
                write!(f, "{}{{{}, {}}}", self.code(), designator.escape_debug(), edition.escape_debug())
            }
            FlowError::FieldTooLong { field } => write!(f, "{}{{{field}}}", self.code()),
            FlowError::BadTransition { state, event } => write!(f, "{}{{{}, {event}}}", self.code(), state.as_str()),
            FlowError::BadAttestation { reason }
            | FlowError::BadIdr { reason }
            | FlowError::ContentDigest { reason }
            | FlowError::ReviewersInvalid { reason }
            | FlowError::ReplayMismatch { reason } => write!(f, "{}{{{reason}}}", self.code()),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for FlowError {}

fn mismatch(reason: &'static str) -> FlowError {
    FlowError::ReplayMismatch { reason }
}

// ------------------------------------------------------------------ value types

/// Review state of one revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FlowState {
    /// `draft`.
    Draft,
    /// `under_review`.
    UnderReview,
    /// `signed_off`: two distinct roster reviewers hold live approvals. **Not a counted
    /// sign-off:** conflict-of-interest rules are applied only by `GDT.RES.STATUS`, which
    /// recounts approvals from the journal; only its verified register may answer
    /// `signed_off` for a rule (D-045).
    SignedOff,
}

impl FlowState {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            FlowState::Draft => "draft",
            FlowState::UnderReview => "under_review",
            FlowState::SignedOff => "signed_off",
        }
    }
}

/// Where a revision's content comes from: `human` only when a human principal proposed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provenance {
    /// `human`.
    Human,
    /// `agent`: any non-human author (agent, script, service, device, system).
    Agent,
}

impl Provenance {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Provenance::Human => "human",
            Provenance::Agent => "agent",
        }
    }
}

/// A review decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Decision {
    /// `approve`.
    Approve,
    /// `request_changes`.
    RequestChanges,
}

impl Decision {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Approve => "approve",
            Decision::RequestChanges => "request_changes",
        }
    }

    fn parse(s: &str) -> Option<Decision> {
        match s {
            "approve" => Some(Decision::Approve),
            "request_changes" => Some(Decision::RequestChanges),
            _ => None,
        }
    }
}

/// An "edition held" attestation (D-016), or the placeholder exception (REQ-004).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Attestation {
    /// The reviewer holds this edition of this standard.
    EditionHeld {
        /// Standard designator, as the row cites it.
        designator: String,
        /// Edition, as the row cites it.
        edition: String,
    },
    /// Accepted only on an `unsupported` row whose chains are all empty.
    NotRequiredPlaceholder,
}

impl Attestation {
    fn to_cvalue(&self) -> CValue {
        match self {
            Attestation::EditionHeld { designator, edition } => {
                obj([("designator", s(designator)), ("edition", s(edition)), ("kind", s("edition_held"))])
            }
            Attestation::NotRequiredPlaceholder => obj([("kind", s("not_required_placeholder"))]),
        }
    }

    fn from_cvalue(v: &CValue) -> Result<Attestation, FlowError> {
        match field_str(v, "kind")? {
            "edition_held" => Ok(Attestation::EditionHeld {
                designator: field_str(v, "designator")?.to_owned(),
                edition: field_str(v, "edition")?.to_owned(),
            }),
            "not_required_placeholder" => Ok(Attestation::NotRequiredPlaceholder),
            _ => Err(mismatch("malformed_record")),
        }
    }
}

/// The author of a revision, as the bus derived it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    kind: PrincipalKind,
    id: String,
}

impl Author {
    /// Principal kind.
    pub fn kind(&self) -> PrincipalKind {
        self.kind
    }

    /// Principal identity.
    pub fn id(&self) -> &str {
        &self.id
    }

    fn is_human(&self) -> bool {
        self.kind == PrincipalKind::Human
    }
}

/// The revision a new revision was derived from: its number and content digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedFrom {
    /// Revision number.
    pub revision: u32,
    /// Its content digest.
    pub content_digest: Digest,
}

/// One review of a revision. The reviewer is named by roster reviewer ID only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRecord {
    reviewer: ReviewerId,
    decision: Decision,
    edition_attestations: Vec<Attestation>,
    idr: Option<String>,
    at: Date,
    roster_digest: Digest,
    reviewed_digest: Digest,
}

impl ReviewRecord {
    /// Roster reviewer ID (pseudonymous).
    pub fn reviewer(&self) -> &ReviewerId {
        &self.reviewer
    }

    /// The decision.
    pub fn decision(&self) -> Decision {
        self.decision
    }

    /// Attestations, sorted and duplicate-free.
    pub fn edition_attestations(&self) -> &[Attestation] {
        &self.edition_attestations
    }

    /// ID of the IDR this review opened, if any.
    pub fn idr(&self) -> Option<&str> {
        self.idr.as_deref()
    }

    /// Review date (the date the roster role was checked on).
    pub fn at(&self) -> Date {
        self.at
    }

    /// Digest of the roster the reviewer was checked against (REQ-GDT.RES.ROWFLOW.ROSTER-004).
    pub fn roster_digest(&self) -> Digest {
        self.roster_digest
    }

    /// The content digest the reviewer read; always equal to the revision's.
    pub fn reviewed_digest(&self) -> Digest {
        self.reviewed_digest
    }

    fn to_cvalue(&self) -> CValue {
        obj([
            ("at", s(&self.at.to_string())),
            ("decision", s(self.decision.as_str())),
            (
                "edition_attestations",
                CValue::Arr(self.edition_attestations.iter().map(Attestation::to_cvalue).collect()),
            ),
            ("idr", self.idr.as_deref().map_or(CValue::Null, s)),
            ("reviewed_digest", s(self.reviewed_digest.as_str())),
            ("reviewer", s(self.reviewer.as_str())),
            ("roster_digest", s(self.roster_digest.as_str())),
        ])
    }
}

/// Status of an Interpretation Decision Record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IdrStatus {
    /// `open`: every IDR this leaf creates. Resolving one is the IDR node's job.
    Open,
}

impl IdrStatus {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            IdrStatus::Open => "open",
        }
    }
}

/// What a `request_changes` review supplies to open an IDR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdrDraft {
    /// `[a-z][a-z0-9_]{0,63}`.
    pub question_code: String,
    /// 1 to 8 position statements, each at most 280 characters, no control characters.
    pub positions: Vec<String>,
}

/// An Interpretation Decision Record linked to a revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Idr {
    id: String,
    question_code: String,
    positions: Vec<String>,
    status: IdrStatus,
}

impl Idr {
    /// `idr-<revision>-<n>`, `n` counting from 1 within the revision.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Question code.
    pub fn question_code(&self) -> &str {
        &self.question_code
    }

    /// Position statements.
    pub fn positions(&self) -> &[String] {
        &self.positions
    }

    /// Status.
    pub fn status(&self) -> IdrStatus {
        self.status
    }

    fn to_cvalue(&self) -> CValue {
        obj([
            ("id", s(&self.id)),
            ("positions", CValue::Arr(self.positions.iter().map(|p| s(p)).collect())),
            ("question_code", s(&self.question_code)),
            ("status", s(self.status.as_str())),
        ])
    }
}

/// One revision of a DMT row and its review state. Fields are private: a revision is only
/// built by [`propose`] or [`replay`] and changed by [`transition`], so its digest always
/// matches its row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowRevision {
    row: DmtRow,
    revision: u32,
    content_digest: Digest,
    author: Author,
    provenance: Provenance,
    derived_from: Option<DerivedFrom>,
    proposed_at: Date,
    state: FlowState,
    reviews: Vec<ReviewRecord>,
    idrs: Vec<Idr>,
}

impl RowRevision {
    /// The row key.
    pub fn key(&self) -> &DmtKey {
        self.row.key()
    }

    /// The row with its `review` field set from [`RowRevision::review_state`]: the only way
    /// to take a row out of the workflow, so a row never carries review state the workflow
    /// did not derive. Remember that `signed_off` here is not a counted sign-off (see
    /// [`FlowState::SignedOff`]).
    pub fn reviewed_row(&self) -> Result<DmtRow, FlowError> {
        self.row.with_review(self.review_state()?).map_err(|e| FlowError::ContentDigest { reason: e.code() })
    }

    /// The cited (designator, edition) pairs, ascending and unique.
    pub fn cited_editions(&self) -> Vec<(String, String)> {
        let set: BTreeSet<(String, String)> =
            self.row.clauses().iter().map(|c| (c.designator().to_owned(), c.edition().to_owned())).collect();
        set.into_iter().collect()
    }

    /// Revision number, from 1.
    pub fn revision(&self) -> u32 {
        self.revision
    }

    /// Content digest, recomputed from the revision's row when the revision was built.
    pub fn content_digest(&self) -> Digest {
        self.content_digest
    }

    /// The author, as the bus derived it.
    pub fn author(&self) -> &Author {
        &self.author
    }

    /// Author kind (the principal kind).
    pub fn author_kind(&self) -> PrincipalKind {
        self.author.kind
    }

    /// Provenance.
    pub fn provenance(&self) -> Provenance {
        self.provenance
    }

    /// The previous revision, if any.
    pub fn derived_from(&self) -> Option<DerivedFrom> {
        self.derived_from
    }

    /// The date the revision was proposed.
    pub fn proposed_at(&self) -> Date {
        self.proposed_at
    }

    /// Review state.
    pub fn state(&self) -> FlowState {
        self.state
    }

    /// Every review of this revision, in order.
    pub fn reviews(&self) -> &[ReviewRecord] {
        &self.reviews
    }

    /// IDRs opened on this revision.
    pub fn idrs(&self) -> &[Idr] {
        &self.idrs
    }

    /// Reviewer IDs with a live approval (after the last `request_changes`), ascending.
    pub fn live_approvals(&self) -> Vec<ReviewerId> {
        let mut live: BTreeSet<ReviewerId> = BTreeSet::new();
        for r in &self.reviews {
            match r.decision {
                Decision::RequestChanges => live.clear(),
                Decision::Approve => {
                    live.insert(r.reviewer.clone());
                }
            }
        }
        live.into_iter().collect()
    }

    /// The schema's `ReviewState` for this revision (REQ-008): `signed_off` lists both
    /// reviewer IDs, sorted. This is the field `GDT.RES.LINT` rule L5 checks. **Not a counted
    /// sign-off:** conflict-of-interest rules are applied only by `GDT.RES.STATUS`, which
    /// recounts from the journal (D-045).
    pub fn review_state(&self) -> Result<ReviewState, FlowError> {
        Ok(match self.state {
            FlowState::Draft => ReviewState::Draft,
            FlowState::UnderReview => ReviewState::UnderReview,
            FlowState::SignedOff => {
                let ids = self.live_approvals().into_iter().map(|r| r.as_str().to_owned()).collect();
                ReviewState::SignedOff(
                    Reviewers::new(ids)
                        .map_err(|e| FlowError::ReviewersInvalid { reason: e.reason().unwrap_or_else(|| e.code()) })?,
                )
            }
        })
    }

    /// The revision as a JSON value (every field; `content` is the row without `review`).
    pub fn to_cvalue(&self) -> CValue {
        obj([
            ("author", author_cvalue(&self.author)),
            ("content", self.row.content_cvalue()),
            ("content_digest", s(self.content_digest.as_str())),
            ("derived_from", derived_cvalue(self.derived_from)),
            ("idrs", CValue::Arr(self.idrs.iter().map(Idr::to_cvalue).collect())),
            ("key", s(self.row.key().as_str())),
            ("proposed_at", s(&self.proposed_at.to_string())),
            ("provenance", s(self.provenance.as_str())),
            ("reviews", CValue::Arr(self.reviews.iter().map(ReviewRecord::to_cvalue).collect())),
            ("revision", CValue::U64(u64::from(self.revision))),
            ("state", s(self.state.as_str())),
        ])
    }

    /// RFC 8785 canonical JSON of [`RowRevision::to_cvalue`].
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, FlowError> {
        canonical(&self.to_cvalue())
    }
}

/// A review event. The reviewer is a bus-derived [`Principal`]; the date is the date of the
/// journal entry the command writes (the roster role is checked on it).
#[derive(Debug, Clone)]
pub struct ReviewEvent<'a> {
    /// Who reviews.
    pub reviewer: &'a Principal,
    /// The decision.
    pub decision: Decision,
    /// The attestations (REQ-004).
    pub attestations: Vec<Attestation>,
    /// The content digest the reviewer read (REQ-GDT.RES.ROWFLOW-006).
    pub reviewed_digest: Digest,
    /// An IDR to open; only with [`Decision::RequestChanges`].
    pub idr: Option<IdrDraft>,
    /// Review date.
    pub at: Date,
}

/// The journal payload of one transition. The command adds the principal and time; the
/// principal kind of the entry is what [`replay`] reads back as [`RecordedFlow::principal_kind`].
#[derive(Debug, Clone, PartialEq)]
pub struct FlowRecord {
    event: &'static str,
    from: Option<FlowState>,
    to: FlowState,
    value: CValue,
}

impl FlowRecord {
    /// `propose`, `approve` or `request_changes`.
    pub fn event(&self) -> &'static str {
        self.event
    }

    /// State before the event (`None` for a proposal).
    pub fn from(&self) -> Option<FlowState> {
        self.from
    }

    /// State after the event.
    pub fn to(&self) -> FlowState {
        self.to
    }

    /// The payload as a JSON value.
    pub fn to_cvalue(&self) -> &CValue {
        &self.value
    }

    /// RFC 8785 canonical JSON of the payload.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, FlowError> {
        canonical(&self.value)
    }

    /// SHA-256 of [`FlowRecord::canonical_bytes`].
    pub fn digest(&self) -> Result<Digest, FlowError> {
        let bytes = self.canonical_bytes()?;
        Ok(Digest::from_bytes(digest_bytes(&bytes).0))
    }
}

/// The outcome of a successful call: the new revision and what to journal for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// The revision after the event.
    pub revision: RowRevision,
    /// The journal payload of the transition.
    pub record: FlowRecord,
}

/// One journaled transition as [`replay`] reads it back: the record payload as stored, and
/// the principal of the journal entry that carries it. Kind and identity come from the entry
/// header, which the bus wrote; they are never read from the payload, and no payload carries
/// a principal identity.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedFlow {
    /// Principal kind of the journal entry header.
    pub principal_kind: PrincipalKind,
    /// Principal identity of the journal entry header.
    pub principal_id: String,
    /// The payload, as parsed from the journal.
    pub record: CValue,
}

// ------------------------------------------------------------------ propose

/// Proposes a new revision of a row (REQ-001, REQ-006). `prev` is the key's latest revision,
/// if any. A row whose own `review` field is not `draft` is refused
/// (`review_field_not_draft`). The new revision is numbered `prev + 1` (or 1), starts in
/// `draft` with no reviews and no IDRs, and names `prev` in `derived_from`; its content
/// digest is recomputed from `row`. A non-human author gives `author_kind` of that principal
/// and provenance `agent`.
pub fn propose(prev: Option<&RowRevision>, row: DmtRow, author: &Principal, at: Date) -> Result<Step, FlowError> {
    let author = Author { kind: author.kind(), id: author.id().as_str().to_owned() };
    propose_as(prev, row, author, at)
}

fn propose_as(prev: Option<&RowRevision>, row: DmtRow, author: Author, at: Date) -> Result<Step, FlowError> {
    if *row.review() != ReviewState::Draft {
        return Err(FlowError::ReviewFieldNotDraft);
    }
    let content_digest = row_content_digest(&row).map_err(|e| FlowError::ContentDigest { reason: e.code() })?;
    let (revision, derived_from) = match prev {
        None => (1, None),
        Some(p) => {
            if p.row.key() != row.key() {
                return Err(FlowError::KeyMismatch);
            }
            let next =
                p.revision.checked_add(1).ok_or(FlowError::BadTransition { state: p.state, event: "propose" })?;
            (next, Some(DerivedFrom { revision: p.revision, content_digest: p.content_digest }))
        }
    };
    let provenance = if author.is_human() { Provenance::Human } else { Provenance::Agent };
    // No principal identity in the payload: the journal entry header carries it.
    let value = obj([
        ("at", s(&at.to_string())),
        ("author_kind", s(author.kind.as_str())),
        ("content_digest", s(content_digest.as_str())),
        ("derived_from", derived_cvalue(derived_from)),
        ("event", s("propose")),
        ("from", CValue::Null),
        ("key", s(row.key().as_str())),
        ("provenance", s(provenance.as_str())),
        ("revision", CValue::U64(u64::from(revision))),
        ("to", s(FlowState::Draft.as_str())),
    ]);
    let rev = RowRevision {
        row,
        revision,
        content_digest,
        author,
        provenance,
        derived_from,
        proposed_at: at,
        state: FlowState::Draft,
        reviews: Vec::new(),
        idrs: Vec::new(),
    };
    let record = FlowRecord { event: "propose", from: None, to: FlowState::Draft, value };
    Ok(Step { revision: rev, record })
}

// ------------------------------------------------------------------ review

/// Applies one review event (REQ-001 .. 005, 007; REQ-GDT.RES.ROWFLOW-006). The checks run in
/// the order of the module documentation; on any failure `rev` is unchanged and nothing is
/// returned to journal.
///
/// The reviewer is checked against [`TrustedRosters::current`], the roster of the latest
/// register the project published (`gdt-published/v1`); `unknown_roster` when the loader was
/// not given that roster (checked after check 5). A [`TrustedRosters`] can only come from the
/// verified loader of `GDT.RES.STATUS`, so a self-made roster cannot make anyone a reviewer.
pub fn transition(rev: &RowRevision, event: &ReviewEvent<'_>, rosters: &TrustedRosters) -> Result<Step, FlowError> {
    check_state(rev, event.decision)?;
    if event.reviewer.kind() != PrincipalKind::Human {
        return Err(FlowError::ReviewerNotHuman);
    }
    check_time_and_digest(rev, event.at, event.reviewed_digest)?;
    let roster = rosters.current().ok_or(FlowError::UnknownRoster)?;
    let rid = check_reviewer(rev, event.reviewer.id().as_str(), roster, event.at)?;
    finish_review(
        rev,
        rid,
        roster.digest(),
        ReviewInput {
            decision: event.decision,
            attestations: &event.attestations,
            reviewed_digest: event.reviewed_digest,
            idr: event.idr.as_ref(),
            at: event.at,
        },
    )
}

/// Checks 1 and 2: the revision accepts reviews at all.
fn check_state(rev: &RowRevision, decision: Decision) -> Result<(), FlowError> {
    if rev.state == FlowState::SignedOff {
        return Err(FlowError::BadTransition { state: rev.state, event: decision.as_str() });
    }
    if !rev.author.is_human() {
        return Err(FlowError::AgentAuthoredRevision);
    }
    Ok(())
}

/// Checks 6 to 8: the reviewing principal is not the author, holds a roster role for the
/// row's dialect on `at`, and that roster ID is not also the author's. Returns the ID.
fn check_reviewer(rev: &RowRevision, reviewer_id: &str, roster: &Roster, at: Date) -> Result<ReviewerId, FlowError> {
    if reviewer_id.eq_ignore_ascii_case(&rev.author.id) {
        return Err(FlowError::ReviewerIsAuthor);
    }
    let dialect = roster_dialect(rev.row.dialect());
    let rid = roster.has_role(&roster_identity(reviewer_id), dialect, at).ok_or(FlowError::ReviewerNotInRoster)?;
    if roster.has_role(&roster_identity(&rev.author.id), dialect, at).as_ref() == Some(&rid) {
        return Err(FlowError::ReviewerIsAuthor);
    }
    Ok(rid)
}

/// Checks 4 and 5: the date never goes back, and the reviewer read this content.
fn check_time_and_digest(rev: &RowRevision, at: Date, reviewed: Digest) -> Result<(), FlowError> {
    let last = rev.reviews.last().map_or(rev.proposed_at, |r| r.at);
    if at < rev.proposed_at || at < last {
        return Err(FlowError::BackdatedReview);
    }
    if reviewed != rev.content_digest {
        return Err(FlowError::StaleReview);
    }
    Ok(())
}

struct ReviewInput<'a> {
    decision: Decision,
    attestations: &'a [Attestation],
    reviewed_digest: Digest,
    idr: Option<&'a IdrDraft>,
    at: Date,
}

/// Checks 9 to 11, then the new revision and its record.
fn finish_review(
    rev: &RowRevision,
    rid: ReviewerId,
    roster_digest: Digest,
    input: ReviewInput<'_>,
) -> Result<Step, FlowError> {
    let decision = input.decision;
    if decision == Decision::Approve && rev.live_approvals().contains(&rid) {
        return Err(FlowError::DuplicateReviewer);
    }
    let attestations = check_attestations(&rev.row, input.attestations)?;
    let idr = match (input.idr, decision) {
        (None, _) => None,
        (Some(_), Decision::Approve) => return Err(FlowError::BadIdr { reason: "idr_on_approve" }),
        (Some(d), Decision::RequestChanges) => {
            check_idr(d)?;
            let n = rev.idrs.len().saturating_add(1);
            Some(Idr {
                id: format!("idr-{}-{n}", rev.revision),
                question_code: d.question_code.clone(),
                positions: d.positions.clone(),
                status: IdrStatus::Open,
            })
        }
    };

    let mut next = rev.clone();
    next.reviews.push(ReviewRecord {
        reviewer: rid.clone(),
        decision,
        edition_attestations: attestations.clone(),
        idr: idr.as_ref().map(|i| i.id.clone()),
        at: input.at,
        roster_digest,
        reviewed_digest: input.reviewed_digest,
    });
    next.state = match decision {
        Decision::RequestChanges => FlowState::Draft,
        Decision::Approve if next.live_approvals().len() >= 2 => FlowState::SignedOff,
        Decision::Approve => FlowState::UnderReview,
    };
    let idr_value = idr.as_ref().map_or(CValue::Null, Idr::to_cvalue);
    if let Some(i) = idr {
        next.idrs.push(i);
    }
    let value = obj([
        ("at", s(&input.at.to_string())),
        ("attestations", CValue::Arr(attestations.iter().map(Attestation::to_cvalue).collect())),
        ("content_digest", s(rev.content_digest.as_str())),
        ("event", s(decision.as_str())),
        ("from", s(rev.state.as_str())),
        ("idr", idr_value),
        ("key", s(rev.row.key().as_str())),
        ("reviewed_digest", s(input.reviewed_digest.as_str())),
        ("reviewer", s(rid.as_str())),
        ("revision", CValue::U64(u64::from(rev.revision))),
        ("roster_digest", s(roster_digest.as_str())),
        ("to", s(next.state.as_str())),
    ]);
    let record = FlowRecord { event: decision.as_str(), from: Some(rev.state), to: next.state, value };
    Ok(Step { revision: next, record })
}

// ------------------------------------------------------------------ replay

/// Rebuilds one revision from its journaled records: a `propose` record, then the review
/// records of that revision in journal order. `prev` is the rebuilt previous revision of the
/// key (or `None` for the first), `row` the row the proposal journaled (with `review:
/// draft`), and `rosters` the trusted rosters the reviews may name by digest.
///
/// Principals come only from the entry headers ([`RecordedFlow::principal_kind`] and
/// [`RecordedFlow::principal_id`]): the author of the revision is the proposal entry's
/// principal, and each review is bound to its entry's principal. Every invariant is checked
/// again with the same code and in the same order as [`propose`] and [`transition`]: the
/// proposal's `author_kind` must be its entry's kind (`replay_mismatch{principal_kind}`);
/// every review entry must be `human`, must not be the author (ASCII case-insensitive), and
/// its principal must hold, in the roster the review names (`unknown_roster` if not given),
/// a role for the row's dialect on the review date whose reviewer ID is the one the record
/// names (`reviewer_not_in_roster`, else `replay_mismatch{reviewer_differs}`), and which the
/// author does not also hold (`reviewer_is_author`). So one person cannot journal two
/// approvals under two reviewer IDs, and an author cannot journal a review naming someone
/// else. Finally each rebuilt record must equal the journaled payload byte for byte in RFC
/// 8785 form (`replay_mismatch{record_differs}`), so an edited, dropped, reordered or
/// inserted record is refused.
///
/// **Trusted rosters:** `rosters` is a [`TrustedRosters`], which only the verified loader of
/// `GDT.RES.STATUS` builds (rosters whose digests `gdt-published/v1` lists), so a self-made
/// roster cannot make any principal a reviewer.
pub fn replay(
    prev: Option<&RowRevision>,
    row: DmtRow,
    entries: &[RecordedFlow],
    rosters: &TrustedRosters,
) -> Result<RowRevision, FlowError> {
    let (first, rest) = entries.split_first().ok_or_else(|| mismatch("empty"))?;
    if field_str(&first.record, "event")? != "propose" {
        return Err(mismatch("first_not_propose"));
    }
    let kind =
        PrincipalKind::parse(field_str(&first.record, "author_kind")?).map_err(|_| mismatch("malformed_record"))?;
    if kind != first.principal_kind {
        return Err(mismatch("principal_kind"));
    }
    let author = Author { kind, id: first.principal_id.clone() };
    let at = date_of(&first.record)?;
    let step = propose_as(prev, row, author, at)?;
    same_record(&step.record, &first.record)?;
    let mut rev = step.revision;

    for entry in rest {
        let r = &entry.record;
        let decision = match field_str(r, "event")? {
            "propose" => return Err(mismatch("propose_inside_revision")),
            other => Decision::parse(other).ok_or_else(|| mismatch("malformed_record"))?,
        };
        let at = date_of(r)?;
        let reviewed = digest_of(r, "reviewed_digest")?;
        let roster_digest = digest_of(r, "roster_digest")?;
        let rid = ReviewerId::parse(field_str(r, "reviewer")?).map_err(|_| mismatch("malformed_record"))?;
        let attestations = match field(r, "attestations")? {
            CValue::Arr(a) => a.iter().map(Attestation::from_cvalue).collect::<Result<Vec<_>, _>>()?,
            _ => return Err(mismatch("malformed_record")),
        };
        let idr = match field(r, "idr")? {
            CValue::Null => None,
            v => Some(IdrDraft {
                question_code: field_str(v, "question_code")?.to_owned(),
                positions: match field(v, "positions")? {
                    CValue::Arr(a) => a
                        .iter()
                        .map(|p| match p {
                            CValue::Str(t) => Ok(t.clone()),
                            _ => Err(mismatch("malformed_record")),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => return Err(mismatch("malformed_record")),
                },
            }),
        };

        check_state(&rev, decision)?;
        if entry.principal_kind != PrincipalKind::Human {
            return Err(FlowError::ReviewerNotHuman);
        }
        check_time_and_digest(&rev, at, reviewed)?;
        let roster = rosters.get(roster_digest).ok_or(FlowError::UnknownRoster)?;
        if check_reviewer(&rev, &entry.principal_id, roster, at)? != rid {
            return Err(mismatch("reviewer_differs"));
        }
        let input =
            ReviewInput { decision, attestations: &attestations, reviewed_digest: reviewed, idr: idr.as_ref(), at };
        let step = finish_review(&rev, rid, roster_digest, input)?;
        same_record(&step.record, r)?;
        rev = step.revision;
    }
    Ok(rev)
}

fn same_record(built: &FlowRecord, journaled: &CValue) -> Result<(), FlowError> {
    if built.canonical_bytes()? == canonical(journaled)? {
        Ok(())
    } else {
        Err(mismatch("record_differs"))
    }
}

fn field<'a>(v: &'a CValue, name: &str) -> Result<&'a CValue, FlowError> {
    match v {
        CValue::Obj(m) => m.get(name).ok_or_else(|| mismatch("malformed_record")),
        _ => Err(mismatch("malformed_record")),
    }
}

fn field_str<'a>(v: &'a CValue, name: &str) -> Result<&'a str, FlowError> {
    match field(v, name)? {
        CValue::Str(t) => Ok(t),
        _ => Err(mismatch("malformed_record")),
    }
}

fn date_of(v: &CValue) -> Result<Date, FlowError> {
    Date::parse(field_str(v, "at")?).ok_or_else(|| mismatch("malformed_record"))
}

fn digest_of(v: &CValue, name: &str) -> Result<Digest, FlowError> {
    Digest::parse(field_str(v, name)?).map_err(|_| mismatch("malformed_record"))
}

// ------------------------------------------------------------------ helpers

fn roster_dialect(d: Dialect) -> RosterDialect {
    match d {
        Dialect::Asme => RosterDialect::Asme,
        Dialect::Iso => RosterDialect::Iso,
    }
}

/// The roster form of a bus identity: `human:` and the identity, ASCII-lowercased (roster
/// identities are lowercase; identities compare case-insensitively).
fn roster_identity(id: &str) -> String {
    format!("{}{}", super::roster::PRINCIPAL_KIND, id.to_ascii_lowercase())
}

fn is_placeholder(row: &DmtRow) -> bool {
    row.status() == RowStatus::Unsupported && row.variants().iter().all(|c| c.steps().is_empty())
}

/// Checks the attestations of a review against the row (REQ-004) and returns them sorted and
/// duplicate-free.
fn check_attestations(row: &DmtRow, given: &[Attestation]) -> Result<Vec<Attestation>, FlowError> {
    if given.len() > MAX_ATTESTATIONS {
        return Err(FlowError::FieldTooLong { field: "attestations" });
    }
    let mut set: BTreeSet<Attestation> = BTreeSet::new();
    for a in given {
        if let Attestation::EditionHeld { designator, edition } = a {
            if designator.len() > MAX_ATTESTATION_LABEL_LEN || edition.len() > MAX_ATTESTATION_LABEL_LEN {
                return Err(FlowError::FieldTooLong { field: "attestation" });
            }
        }
        set.insert(a.clone());
    }
    let placeholder_given = set.contains(&Attestation::NotRequiredPlaceholder);
    let placeholder_row = is_placeholder(row);
    if placeholder_given && !placeholder_row {
        return Err(FlowError::BadAttestation { reason: "placeholder_not_allowed" });
    }
    if !(placeholder_given && placeholder_row) {
        let held: BTreeSet<(&str, &str)> = set
            .iter()
            .filter_map(|a| match a {
                Attestation::EditionHeld { designator, edition } => Some((designator.as_str(), edition.as_str())),
                Attestation::NotRequiredPlaceholder => None,
            })
            .collect();
        // Clauses are ascending by (designator, edition, clause), so the first missing pair
        // reported is the smallest.
        for c in row.clauses() {
            if !held.contains(&(c.designator(), c.edition())) {
                return Err(FlowError::AttestationMissing {
                    designator: c.designator().to_owned(),
                    edition: c.edition().to_owned(),
                });
            }
        }
    }
    Ok(set.into_iter().collect())
}

fn check_idr(d: &IdrDraft) -> Result<(), FlowError> {
    let q = d.question_code.as_bytes();
    let code_ok = !q.is_empty()
        && q.len() <= MAX_QUESTION_CODE_LEN
        && q.first().is_some_and(u8::is_ascii_lowercase)
        && q.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_');
    if !code_ok {
        return Err(FlowError::BadIdr { reason: "bad_question_code" });
    }
    if d.positions.is_empty() || d.positions.len() > MAX_IDR_POSITIONS {
        return Err(FlowError::BadIdr { reason: "bad_position_count" });
    }
    for p in &d.positions {
        if p.chars().count() > MAX_IDR_POSITION_CHARS {
            return Err(FlowError::FieldTooLong { field: "position" });
        }
        if p.is_empty() || p.chars().any(char::is_control) {
            return Err(FlowError::BadIdr { reason: "bad_position" });
        }
    }
    Ok(())
}

fn author_cvalue(a: &Author) -> CValue {
    obj([("id", s(&a.id)), ("kind", s(a.kind.as_str()))])
}

fn derived_cvalue(d: Option<DerivedFrom>) -> CValue {
    match d {
        None => CValue::Null,
        Some(d) => {
            obj([("content_digest", s(d.content_digest.as_str())), ("revision", CValue::U64(u64::from(d.revision)))])
        }
    }
}

fn canonical(v: &CValue) -> Result<Vec<u8>, FlowError> {
    to_canonical_bytes(v).map_err(|e| FlowError::ContentDigest { reason: e.code() })
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect::<BTreeMap<_, _>>())
}
