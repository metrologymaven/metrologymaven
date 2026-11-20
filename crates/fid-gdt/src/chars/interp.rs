//! Interpretation block and provisional label of a ResultRecord (spec node
//! `GDT.CHAR.INTERP`, D-045).
//!
//! Every GD&T result names each interpretation rule it used (the characteristic's defaults
//! profile and every DMT row the evaluation resolved) and says `interpretation: provisional`
//! when any of them lacks expert sign-off, so an unreviewed reading never looks settled.
//!
//! - [`RuleUse`]: one rule a record used, with its kind, its revision as `GDT.RES.STATUS`
//!   defines it, its content digest (always recomputed here: [`row_content_digest`] on the
//!   resolved row, SHA-256 of the canonical bytes for the profile; the builder takes no digest
//!   from a caller, a table or a register), its status and its reason (REQ-001, REQ-002).
//! - [`RegisterView`]: the status register an evaluation consults. Its only constructors are
//!   [`RegisterView::none`] and [`RegisterView::load`], which loads `gdt-published/v1` with
//!   [`Published::load_extending`] against a non-empty pinned prior (never a bare `load`) and
//!   checks the register file with [`verify_register`]. A rule's status comes from
//!   [`status_of`] on a register that verified, and only from there; a missing register gives
//!   `provisional`/`no_register`, a register (or published list) that does not verify gives
//!   `provisional`/`unverified_register` (REQ-003). Fail closed: nothing but a verified
//!   register can ever yield `signed_off`.
//! - **Drafts are refused whatever the register state.** Both constructors take the
//!   `GDT.RES.ROWFLOW` journal; a row whose (key, content digest) only an agent or a script
//!   ever proposed (no human proposal of that key carries the digest) is a draft and fails the
//!   build with `rule_not_admitted{rule}`, with a verified register, an unverified one or none
//!   (REQ-005). A verified register's `draft` status is refused too. A draft is never emitted,
//!   not even as provisional.
//! - The primary row is tied to the evaluated chain: some row with the primary key
//!   (`dmt_row`) must have a variant whose chain digest is the record's
//!   `operator_chain_digest`, else `incomplete_result{interpretation_rules}`. So a signed-off
//!   revision cannot vouch for a chain it does not contain.
//! - New evaluations need a register verified with [`Purpose::Evaluate`], which accepts only
//!   the latest published register; a view checked for replay is treated as unverified by
//!   the builder (REQ-003, AC-13).
//! - A rule whose status is `draft` is refused (`rule_not_admitted{rule}`, REQ-005);
//!   missing rows, a missing primary row or a missing profile are
//!   `incomplete_result{interpretation_rules}` (REQ-006). Both are checked by
//!   [`ResultRecordBuilder::finish`](super::record::ResultRecordBuilder::finish).
//! - [`InterpretationPin`]: what the evaluating command writes into its journal entry (the
//!   register digest and how it verified); [`replay_interpretation`] loads the pinned
//!   register from a [`RegisterStore`] (it must be there: `register_unavailable` otherwise),
//!   verifies a `verified` pin with [`Purpose::Replay`] and recomputes
//!   the block: `IDENTICAL`, `DIVERGENT`, or `NOT_REPLAYABLE` with `register_unavailable`,
//!   never a re-labelled record (REQ-007).
//! - [`requires_provisional_label`] (on a record) and [`requires_provisional_label_bytes`] (on
//!   stored canonical bytes) are the predicates reports, exports and the release queue call
//!   (REQ-009); the bytes form says true for anything but a `fid.gdt-result/2` record whose
//!   `interpretation` and every rule are `signed_off` (an older `/1` record, a missing or
//!   unreadable field all count as provisional). The label text comes from catalog key
//!   [`PROVISIONAL_LABEL_KEY`].
//!
//! Deterministic (D0): no clock, sorted collections only, no floating point, no panics.

use crate::chars::dprof::{ProfileDoc, ProfileRef};
use crate::chars::record::{RecordError, ResultRecord, RECORD_SCHEMA};
use crate::dmt_flow::roster::Roster;
use crate::dmt_flow::status::{
    row_content_digest, status_of, verify_register, CheckedRegister, CoiInputs, JournalExtract, Published, Purpose,
    Reason, RuleKind, Status, Verification, PROPOSE_COMMAND,
};
use fid_bus::principal::PrincipalKind;
use fid_canon::CValue;
use fid_gps::dmt::row::row_from_value;
use fid_gps::dmt::DmtRow;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// Catalog key of the provisional label (D-045).
pub const PROVISIONAL_LABEL_KEY: &str = "gdt.interpretation.provisional";
/// Source (English) text of [`PROVISIONAL_LABEL_KEY`].
pub const PROVISIONAL_LABEL_TEXT: &str = "interpretation: provisional";
/// The line a signed-off record shows in its place.
pub const SIGNED_OFF_LINE: &str = "interpretation: signed_off";
/// Replay reason when the pinned register cannot be loaded or does not verify for replay.
pub const REGISTER_UNAVAILABLE: &str = "register_unavailable";
/// `schema` of an [`InterpretationPin`].
pub const PIN_SCHEMA: &str = "fid.gdt-interp-pin/1";
/// Most resolved rows one evaluation may pass (the profile comes on top).
pub const MAX_RULE_ROWS: usize = 64;
/// Most registers one [`RegisterStore`] holds.
pub const MAX_STORED_REGISTERS: usize = 4096;
/// Largest register file a [`RegisterStore`] takes, in bytes.
pub const MAX_REGISTER_BYTES: usize = 64 * 1024 * 1024;
/// Largest record [`requires_provisional_label_bytes`] reads, in bytes (larger: provisional).
pub const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECORD_DEPTH: usize = 8;

const FIELD: &str = "interpretation_rules";

fn incomplete() -> RecordError {
    RecordError::IncompleteResult { field: FIELD }
}

// ------------------------------------------------------------------ labels

/// The record-level label (REQ-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Interpretation {
    /// `provisional`: at least one rule is not signed off.
    Provisional,
    /// `signed_off`: every rule is signed off.
    SignedOff,
}

impl Interpretation {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Interpretation::Provisional => "provisional",
            Interpretation::SignedOff => "signed_off",
        }
    }

    /// The display line: the catalog text of [`PROVISIONAL_LABEL_KEY`] for a provisional
    /// record, [`SIGNED_OFF_LINE`] otherwise.
    pub fn line(self) -> &'static str {
        match self {
            Interpretation::Provisional => PROVISIONAL_LABEL_TEXT,
            Interpretation::SignedOff => SIGNED_OFF_LINE,
        }
    }

    /// `signed_off` exactly when every rule is (REQ-004); an empty list is `provisional`.
    pub fn of(rules: &[RuleUse]) -> Interpretation {
        if !rules.is_empty() && rules.iter().all(|r| r.status == RuleStatus::SignedOff) {
            Interpretation::SignedOff
        } else {
            Interpretation::Provisional
        }
    }
}

/// Whether a record must show the provisional label (REQ-009): true exactly when its
/// `interpretation` is `provisional`. Reports (`RPT.MODEL.FMT`), exports (`IO.RES.ILABEL`)
/// and the release batch call this and nothing else.
pub fn requires_provisional_label(record: &ResultRecord) -> bool {
    record.interpretation() == Interpretation::Provisional
}

/// [`requires_provisional_label`] on stored canonical record bytes (REQ-009), for readers
/// that never hold a [`ResultRecord`] (reports, exports, the release queue). False only for
/// a readable `fid.gdt-result/2` record whose `interpretation` is `signed_off` and whose
/// `interpretation_rules` are non-empty and all `signed_off`; true for anything else: bytes
/// that do not parse (or exceed [`MAX_RECORD_BYTES`]), another schema (a `/1` record has no
/// interpretation block), or a missing, mistyped or other `interpretation`.
pub fn requires_provisional_label_bytes(record: &[u8]) -> bool {
    let signed = CValue::Str(Interpretation::SignedOff.as_str().to_owned());
    let Ok(CValue::Obj(o)) = fid_canon::parse_with_limits(record, MAX_RECORD_BYTES, MAX_RECORD_DEPTH) else {
        return true;
    };
    if o.get("schema") != Some(&CValue::Str(RECORD_SCHEMA.to_owned())) || o.get("interpretation") != Some(&signed) {
        return true;
    }
    let Some(CValue::Arr(rules)) = o.get("interpretation_rules") else { return true };
    rules.is_empty() || !rules.iter().all(|r| matches!(r, CValue::Obj(m) if m.get("status") == Some(&signed)))
}

/// The status of a rule in a record. There is no `draft`: a draft rule never reaches one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuleStatus {
    /// `provisional`.
    Provisional,
    /// `signed_off`.
    SignedOff,
}

impl RuleStatus {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            RuleStatus::Provisional => "provisional",
            RuleStatus::SignedOff => "signed_off",
        }
    }
}

/// One interpretation rule a record used (REQ-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleUse {
    rule: String,
    kind: RuleKind,
    revision: Option<u32>,
    content_digest: Digest,
    status: RuleStatus,
    reason: Option<Reason>,
}

impl RuleUse {
    /// DMT key, or profile ID with version.
    pub fn rule(&self) -> &str {
        &self.rule
    }
    /// `dmt_row` or `profile`.
    pub fn kind(&self) -> RuleKind {
        self.kind
    }
    /// The revision as `GDT.RES.STATUS` numbers it: the register entry's revision; 1 for a
    /// profile (profiles are immutable); `None` for a row the consulted register does not
    /// hold at this digest (or when no verified register was consulted).
    pub fn revision(&self) -> Option<u32> {
        self.revision
    }
    /// The content digest, recomputed from the row or profile the evaluation used.
    pub fn content_digest(&self) -> Digest {
        self.content_digest
    }
    /// Status.
    pub fn status(&self) -> RuleStatus {
        self.status
    }
    /// Reason, for every status but `signed_off`.
    pub fn reason(&self) -> Option<Reason> {
        self.reason
    }

    pub(crate) fn to_cvalue(&self) -> CValue {
        let s = |x: &str| CValue::Str(x.to_owned());
        let mut m = BTreeMap::new();
        m.insert("content_digest".to_owned(), s(self.content_digest.as_str()));
        m.insert("kind".to_owned(), s(self.kind.as_str()));
        m.insert("reason".to_owned(), self.reason.map_or(CValue::Null, |r| s(r.as_str())));
        m.insert("revision".to_owned(), self.revision.map_or(CValue::Null, |r| CValue::Num(f64::from(r))));
        m.insert("rule".to_owned(), s(&self.rule));
        m.insert("status".to_owned(), s(self.status.as_str()));
        CValue::Obj(m)
    }
}

// ------------------------------------------------------------------ the register view

/// What [`RegisterView::load`] checks a register file against. `published` is the raw
/// `gdt-published/v1` list from the DMT data repository; it is loaded only with
/// [`Published::load_extending`] against `pinned_prior`, so a rewritten or truncated list
/// never verifies anything. An empty `pinned_prior` is refused (`empty_pinned_prior`): it
/// would accept any list.
///
/// **Trust.** The check is only as good as `pinned_prior`: it must come from a place the
/// caller trusts independently of `published` (a copy pinned in the project, or the list as
/// of the last verified release), never from the same download. A self-made prior admits a
/// self-made list.
#[derive(Debug, Clone, Copy)]
pub struct RegisterSources<'a> {
    /// The `GDT.RES.ROWFLOW` journal extract the register is rebuilt from.
    pub journal: &'a JournalExtract,
    /// The rosters the register names (current and trusted).
    pub rosters: &'a [Roster],
    /// The conflict-of-interest inputs.
    pub coi: &'a CoiInputs,
    /// The raw `gdt-published/v1` bytes.
    pub published: &'a [u8],
    /// A pinned earlier copy of the list that `published` must extend.
    pub pinned_prior: &'a Published,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum View {
    None,
    Unverified { file: Digest, code: &'static str },
    Verified { checked: Box<CheckedRegister>, digest: Digest },
}

/// The status register an evaluation consults: none, unverified (with the file digest and
/// the reason), or verified for a purpose. Fields are private; the constructors are
/// [`RegisterView::none`] and [`RegisterView::load`], so a verified view always went through
/// [`verify_register`] on a list loaded with [`Published::load_extending`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterView {
    view: View,
    /// Agent-only (row key, content digest) pairs of the ROWFLOW journal ([`agent_drafts`]).
    drafts: BTreeSet<(String, Digest)>,
}

/// Unverified reasons a pin may record (the `verify_register` codes plus the loader's).
const UNVERIFIED_CODES: [&str; 6] = [
    "register_mismatch",
    "register_not_published",
    "foreign_roster",
    "stale_register",
    "published_unreadable",
    "empty_pinned_prior",
];

/// The (row key, content digest) pairs that only agents or scripts ever proposed in the
/// `GDT.RES.ROWFLOW` journal: every `gdt.propose_dmt_row@1` entry's row is read and its
/// content digest recomputed; pairs some human proposal also carries are removed. These are
/// drafts whatever the register says (D-045, REQ-005).
fn agent_drafts(journal: &JournalExtract) -> BTreeSet<(String, Digest)> {
    let mut agent = BTreeSet::new();
    let mut human = BTreeSet::new();
    for e in journal.entries() {
        if e.command() != PROPOSE_COMMAND {
            continue;
        }
        let Some(row) = e.row().and_then(|v| row_from_value(v).ok()) else { continue };
        let Ok(d) = row_content_digest(&row) else { continue };
        let pair = (row.key().as_str().to_owned(), d);
        if e.principal_kind() == PrincipalKind::Human {
            human.insert(pair);
        } else {
            agent.insert(pair);
        }
    }
    agent.difference(&human).cloned().collect()
}

impl RegisterView {
    /// No register loaded: every rule is `provisional`/`no_register`. The ROWFLOW journal is
    /// still required: it decides which rows are agent drafts.
    pub fn none(journal: &JournalExtract) -> RegisterView {
        RegisterView { view: View::None, drafts: agent_drafts(journal) }
    }

    /// Refuses an empty pinned prior (`empty_pinned_prior`), loads `gdt-published/v1` with
    /// [`Published::load_extending`] against it (`published_unreadable` when it is malformed
    /// or not append-only), then checks `file` with [`verify_register`] for `purpose`. Any
    /// failure gives an unverified view.
    pub fn load(file: &[u8], sources: &RegisterSources<'_>, purpose: Purpose) -> RegisterView {
        let drafts = agent_drafts(sources.journal);
        let file_digest = sha(file);
        let unverified = |code, drafts| RegisterView { view: View::Unverified { file: file_digest, code }, drafts };
        if sources.pinned_prior.entries().is_empty() {
            return unverified("empty_pinned_prior", drafts);
        }
        let Ok(published) = Published::load_extending(sources.published, sources.pinned_prior) else {
            return unverified("published_unreadable", drafts);
        };
        let checked = verify_register(file, sources.journal, sources.rosters, sources.coi, &published, purpose);
        match checked.verification() {
            Verification::Verified { digest } if checked.register().is_some() => {
                RegisterView { view: View::Verified { checked: Box::new(checked), digest }, drafts }
            }
            v => unverified(v.code(), drafts),
        }
    }

    /// Digest of the register file consulted, or `None` when none was loaded.
    pub fn digest(&self) -> Option<Digest> {
        match &self.view {
            View::None => None,
            View::Unverified { file, .. } => Some(*file),
            View::Verified { digest, .. } => Some(*digest),
        }
    }

    /// `none`, `verified`, or the reason it did not verify.
    pub fn code(&self) -> &'static str {
        match &self.view {
            View::None => "none",
            View::Unverified { code, .. } => code,
            View::Verified { .. } => "verified",
        }
    }

    /// Whether the register verified (for [`RegisterView::purpose`]).
    pub fn is_verified(&self) -> bool {
        matches!(self.view, View::Verified { .. })
    }

    /// The purpose a verified register was checked for.
    pub fn purpose(&self) -> Option<Purpose> {
        match &self.view {
            View::Verified { checked, .. } => Some(checked.purpose()),
            _ => None,
        }
    }

    /// The status of one rule for an evaluation that needs `purpose`: only a register that
    /// verified for exactly that purpose is asked, through [`status_of`].
    fn answer(
        &self,
        purpose: Purpose,
        kind: RuleKind,
        rule: &str,
        digest: Digest,
    ) -> (Status, Option<Reason>, Option<u32>) {
        let fallback = if kind == RuleKind::Profile { Some(1) } else { None };
        match &self.view {
            View::None => (Status::Provisional, Some(Reason::NoRegister), fallback),
            View::Unverified { .. } => (Status::Provisional, Some(Reason::UnverifiedRegister), fallback),
            View::Verified { checked, .. } if checked.purpose() != purpose => {
                (Status::Provisional, Some(Reason::UnverifiedRegister), fallback)
            }
            View::Verified { checked, .. } => {
                let a = status_of(Some(checked.as_ref()), rule, digest);
                match a.entry {
                    // Defence in depth: an entry of another kind is not this rule.
                    Some(e) if e.kind != kind => (Status::Provisional, Some(Reason::NotInRegister), fallback),
                    Some(e) => (a.status, a.reason, Some(e.revision)),
                    // `status_of` never says signed_off without an entry; never trust it here.
                    None => (Status::Provisional, a.reason.or(Some(Reason::NotInRegister)), fallback),
                }
            }
        }
    }
}

fn sha(bytes: &[u8]) -> Digest {
    Digest::from_bytes(fid_canon::digest_bytes(bytes).0)
}

// ------------------------------------------------------------------ the journal pin

/// How the register an evaluation consulted verified, as pinned in its journal entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PinState {
    None,
    Unverified(&'static str),
    Verified,
}

/// What the evaluating command writes into its journal entry (REQ-007): the register digest
/// (`interpretation_register`) and how it verified, so replay uses the same register and
/// never relabels a record. Canonical form:
/// `{"interpretation_register": "sha256:..." | "none", "register_verification": "verified" |
/// "<reason>" | "none", "schema": "fid.gdt-interp-pin/1"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterpretationPin {
    register: Option<Digest>,
    state: PinState,
}

impl InterpretationPin {
    fn of(view: &RegisterView) -> InterpretationPin {
        let state = match &view.view {
            View::None => PinState::None,
            View::Unverified { code, .. } => PinState::Unverified(code),
            View::Verified { .. } => PinState::Verified,
        };
        InterpretationPin { register: view.digest(), state }
    }

    /// The pinned register digest (`None` for `none`).
    pub fn register(&self) -> Option<Digest> {
        self.register
    }

    /// `verified`, the reason it did not verify, or `none`.
    pub fn verification(&self) -> &'static str {
        match self.state {
            PinState::None => "none",
            PinState::Unverified(c) => c,
            PinState::Verified => "verified",
        }
    }

    /// The canonical journal fragment.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("interpretation_register".to_owned(), register_cvalue(self.register));
        m.insert("register_verification".to_owned(), CValue::Str(self.verification().to_owned()));
        m.insert("schema".to_owned(), CValue::Str(PIN_SCHEMA.to_owned()));
        CValue::Obj(m)
    }

    /// RFC 8785 bytes of [`InterpretationPin::to_cvalue`].
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, RecordError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| RecordError::CanonicalEncoding)
    }

    /// Reads a pin back; `None` for anything but the exact canonical form.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Option<InterpretationPin> {
        let v = fid_canon::parse_with_limits(bytes, 1024, 1).ok()?;
        let CValue::Obj(m) = &v else { return None };
        if m.len() != 3 || m.get("schema") != Some(&CValue::Str(PIN_SCHEMA.to_owned())) {
            return None;
        }
        let register = match m.get("interpretation_register")? {
            CValue::Str(s) if s == "none" => None,
            CValue::Str(s) => Some(Digest::parse(s).ok()?),
            _ => return None,
        };
        let state = match m.get("register_verification")? {
            CValue::Str(s) if s == "none" => PinState::None,
            CValue::Str(s) if s == "verified" => PinState::Verified,
            CValue::Str(s) => PinState::Unverified(UNVERIFIED_CODES.iter().find(|c| **c == s.as_str())?),
            _ => return None,
        };
        // A register exactly when one was consulted.
        if register.is_none() != (state == PinState::None) {
            return None;
        }
        let pin = InterpretationPin { register, state };
        (pin.canonical_bytes().ok()?.as_slice() == bytes).then_some(pin)
    }
}

pub(crate) fn register_cvalue(d: Option<Digest>) -> CValue {
    CValue::Str(match d {
        Some(d) => d.as_str().to_owned(),
        None => "none".to_owned(),
    })
}

// ------------------------------------------------------------------ the block

/// The interpretation input a builder holds after
/// [`ResultRecordBuilder::interpretation`](super::record::ResultRecordBuilder::interpretation):
/// rule uses with recomputed digests and looked-up statuses, the row keys and profile
/// reference for the completeness checks of `finish`, and the journal pin. Fields are
/// private; it is made only by the builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interpreted {
    rules: Vec<RuleUse>,
    row_keys: BTreeSet<String>,
    /// (row key, chain digest) of every variant of every row passed.
    chains: BTreeSet<(String, Digest)>,
    profile: Option<ProfileRef>,
    drafts: BTreeSet<String>,
    pin: InterpretationPin,
    error: Option<RecordError>,
}

/// The type state of a builder that has no interpretation block yet: it has no `finish`, so
/// a record without its `interpretation` field cannot be produced, let alone serialised
/// (REQ-008, AC-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Uninterpreted;

/// The finished block, checked by `finish` against the record's primary row and spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) interpretation: Interpretation,
    pub(crate) rules: Vec<RuleUse>,
    pub(crate) register: Option<Digest>,
    pub(crate) pin: InterpretationPin,
}

impl Interpreted {
    /// Computes rule uses for `rows` and `profile` against `view`, for an evaluation needing
    /// a register verified for `purpose` (REQ-001 .. REQ-003).
    pub(crate) fn compute(
        rows: &[&DmtRow],
        profile: Option<&ProfileDoc>,
        view: &RegisterView,
        purpose: Purpose,
    ) -> Interpreted {
        let mut out = Interpreted {
            rules: Vec::new(),
            row_keys: BTreeSet::new(),
            chains: BTreeSet::new(),
            profile: None,
            drafts: BTreeSet::new(),
            pin: InterpretationPin::of(view),
            error: None,
        };
        if rows.len() > MAX_RULE_ROWS {
            out.error = Some(RecordError::InvalidField { field: FIELD });
            return out;
        }
        // (kind, rule, digest), deduplicated and sorted.
        let mut uses: BTreeSet<(RuleKind, String, Digest)> = BTreeSet::new();
        for row in rows {
            let Ok(d) = row_content_digest(row) else {
                out.error = Some(RecordError::InvalidField { field: FIELD });
                return out;
            };
            let key = row.key().as_str().to_owned();
            if view.drafts.contains(&(key.clone(), d)) {
                out.drafts.insert(key.clone());
            }
            for c in row.variants() {
                out.chains.insert((key.clone(), c.digest()));
            }
            out.row_keys.insert(key.clone());
            uses.insert((RuleKind::DmtRow, key, d));
        }
        if let Some(doc) = profile {
            // Recomputed from the canonical bytes, never the digest the document carries.
            let d = sha(doc.canonical());
            out.profile = Some(ProfileRef::new(doc.profile().id(), d));
            uses.insert((RuleKind::Profile, doc.profile().id().to_string(), d));
        }
        for (kind, rule, digest) in uses {
            let (status, reason, revision) = view.answer(purpose, kind, &rule, digest);
            let status = match status {
                Status::Draft => {
                    out.drafts.insert(rule);
                    continue;
                }
                Status::Provisional => RuleStatus::Provisional,
                Status::SignedOff => RuleStatus::SignedOff,
            };
            let reason = if status == RuleStatus::SignedOff { None } else { reason };
            out.rules.push(RuleUse { rule, kind, revision, content_digest: digest, status, reason });
        }
        out
    }

    /// The checks of `finish` (REQ-005, REQ-006), in order: the setter error; rows present,
    /// the primary row among them, and a primary-key row with a variant whose chain digest is
    /// the record's `operator_chain_digest`; the profile present and equal to the spec's
    /// (all `incomplete_result{interpretation_rules}`); then no draft rule
    /// (`rule_not_admitted{rule}`, the first in byte order).
    pub(crate) fn finish(self, primary: &str, chain: Digest, spec_profile: &ProfileRef) -> Result<Block, RecordError> {
        if let Some(e) = self.error {
            return Err(e);
        }
        if self.row_keys.is_empty() || !self.row_keys.contains(primary) {
            return Err(incomplete());
        }
        if !self.chains.contains(&(primary.to_owned(), chain)) {
            return Err(incomplete());
        }
        match &self.profile {
            Some(p) if p == spec_profile => {}
            _ => return Err(incomplete()),
        }
        if let Some(rule) = self.drafts.into_iter().next() {
            return Err(RecordError::RuleNotAdmitted { rule });
        }
        Ok(Block {
            interpretation: Interpretation::of(&self.rules),
            rules: self.rules,
            register: self.pin.register,
            pin: self.pin,
        })
    }

    /// The block without the record checks (replay compares only the three fields).
    fn fields(&self) -> Option<(CValue, CValue, CValue)> {
        if self.error.is_some() || !self.drafts.is_empty() {
            return None;
        }
        Some((
            CValue::Str(Interpretation::of(&self.rules).as_str().to_owned()),
            register_cvalue(self.pin.register),
            CValue::Arr(self.rules.iter().map(RuleUse::to_cvalue).collect()),
        ))
    }
}

// ------------------------------------------------------------------ register store and replay

/// The registers replay can find, by digest (a content-addressed, in-memory stand-in for the
/// project store that also holds pinned DMT tables). A register is stored under the SHA-256
/// of its bytes, so a lookup never returns other bytes than the digest names.
#[derive(Debug, Clone, Default)]
pub struct RegisterStore {
    by_digest: BTreeMap<Digest, Vec<u8>>,
}

impl RegisterStore {
    /// An empty store.
    pub fn new() -> RegisterStore {
        RegisterStore::default()
    }

    /// Stores `bytes` under their digest and returns it. `invalid_field{register}` over
    /// [`MAX_REGISTER_BYTES`]; `store_full` at [`MAX_STORED_REGISTERS`].
    pub fn put(&mut self, bytes: &[u8]) -> Result<Digest, RecordError> {
        if bytes.len() > MAX_REGISTER_BYTES {
            return Err(RecordError::InvalidField { field: "register" });
        }
        let d = sha(bytes);
        if !self.by_digest.contains_key(&d) {
            if self.by_digest.len() >= MAX_STORED_REGISTERS {
                return Err(RecordError::StoreFull);
            }
            self.by_digest.insert(d, bytes.to_vec());
        }
        Ok(d)
    }

    /// The register with this digest.
    pub fn get(&self, digest: &Digest) -> Option<&[u8]> {
        self.by_digest.get(digest).map(Vec::as_slice)
    }

    /// Removes a register (for example when it is pruned); returns its bytes.
    pub fn remove(&mut self, digest: &Digest) -> Option<Vec<u8>> {
        self.by_digest.remove(digest)
    }

    /// Number of registers held.
    pub fn len(&self) -> usize {
        self.by_digest.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.by_digest.is_empty()
    }
}

/// The outcome of [`replay_interpretation`]: the replay classification and, for
/// `NOT_REPLAYABLE`, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterpretationReplay {
    /// `IDENTICAL`, `DIVERGENT` or `NOT_REPLAYABLE`.
    pub outcome: ReplayOutcome,
    /// [`REGISTER_UNAVAILABLE`], `bad_pin` or `bad_record` for `NOT_REPLAYABLE`.
    pub reason: Option<&'static str>,
}

impl InterpretationReplay {
    fn not_replayable(reason: &'static str) -> InterpretationReplay {
        InterpretationReplay { outcome: ReplayOutcome::NotReplayable, reason: Some(reason) }
    }
    fn of(outcome: ReplayOutcome) -> InterpretationReplay {
        InterpretationReplay { outcome, reason: None }
    }
}

/// Replays the interpretation block of a recorded ResultRecord (REQ-007, AC-8, AC-13).
///
/// `recorded` is the record's canonical JSON, `pin` the [`InterpretationPin`] of its journal
/// entry, `rows` and `profile` the inputs the evaluation used. Whenever the pin names a
/// register, replay loads it from `store` by that digest; if it is not there (or its bytes do
/// not hash to the digest), the outcome is `NOT_REPLAYABLE` with [`REGISTER_UNAVAILABLE`]. A
/// `verified` pin's register is then checked with [`RegisterView::load`] for
/// [`Purpose::Replay`] (an older published register is valid for replay) and must verify
/// (else [`REGISTER_UNAVAILABLE`] too); a pin recording an unverified register recomputes the
/// same fail-safe `unverified_register` labels, and a pin of `none` the `no_register` ones. The recomputed `interpretation`,
/// `interpretation_register` and `interpretation_rules` must equal the recorded ones
/// (`IDENTICAL`), else `DIVERGENT`.
pub fn replay_interpretation(
    recorded: &[u8],
    pin: &[u8],
    rows: &[&DmtRow],
    profile: Option<&ProfileDoc>,
    store: &RegisterStore,
    sources: &RegisterSources<'_>,
) -> InterpretationReplay {
    let Some(pin) = InterpretationPin::from_canonical_bytes(pin) else {
        return InterpretationReplay::not_replayable("bad_pin");
    };
    let Ok(CValue::Obj(rec)) = fid_canon::parse(recorded) else {
        return InterpretationReplay::not_replayable("bad_record");
    };
    let (Some(i), Some(reg), Some(rules)) =
        (rec.get("interpretation"), rec.get("interpretation_register"), rec.get("interpretation_rules"))
    else {
        return InterpretationReplay::not_replayable("bad_record");
    };
    let view = match (pin.state, pin.register) {
        (PinState::None, None) => RegisterView::none(sources.journal),
        (PinState::None, Some(_)) | (_, None) => return InterpretationReplay::not_replayable("bad_pin"),
        (state, Some(d)) => {
            let Some(bytes) = store.get(&d).filter(|b| sha(b) == d) else {
                return InterpretationReplay::not_replayable(REGISTER_UNAVAILABLE);
            };
            if let PinState::Unverified(code) = state {
                RegisterView { view: View::Unverified { file: d, code }, drafts: agent_drafts(sources.journal) }
            } else {
                let v = RegisterView::load(bytes, sources, Purpose::Replay);
                if !v.is_verified() || v.digest() != Some(d) {
                    return InterpretationReplay::not_replayable(REGISTER_UNAVAILABLE);
                }
                v
            }
        }
    };
    let again = Interpreted::compute(rows, profile, &view, Purpose::Replay);
    match again.fields() {
        Some((a, b, c)) if &a == i && &b == reg && &c == rules => InterpretationReplay::of(ReplayOutcome::Identical),
        _ => InterpretationReplay::of(ReplayOutcome::Divergent),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn rule(status: RuleStatus) -> RuleUse {
        RuleUse {
            rule: "dmt:asme/flatness/none@1".to_owned(),
            kind: RuleKind::DmtRow,
            revision: Some(1),
            content_digest: Digest::TOMBSTONE,
            status,
            reason: None,
        }
    }

    // REQ-004: the aggregate, including the signed_off branch that no register can reach
    // today (every profile is provisional/no_signoff_path under GDT.RES.STATUS REQ-006).
    #[test]
    fn signed_off_exactly_when_every_rule_is() {
        assert_eq!(Interpretation::of(&[]), Interpretation::Provisional);
        let s = rule(RuleStatus::SignedOff);
        let p = rule(RuleStatus::Provisional);
        assert_eq!(Interpretation::of(std::slice::from_ref(&s)), Interpretation::SignedOff);
        assert_eq!(Interpretation::of(&[s.clone(), s.clone()]), Interpretation::SignedOff);
        assert_eq!(Interpretation::of(&[s.clone(), p.clone()]), Interpretation::Provisional);
        assert_eq!(Interpretation::of(&[p]), Interpretation::Provisional);
        assert_eq!(Interpretation::SignedOff.line(), "interpretation: signed_off");
        assert_eq!(Interpretation::Provisional.line(), PROVISIONAL_LABEL_TEXT);
    }

    #[test]
    fn a_none_view_answers_no_register_and_pins_none() {
        let v = RegisterView { view: View::None, drafts: BTreeSet::new() };
        let (st, r, rev) = v.answer(Purpose::Evaluate, RuleKind::DmtRow, "x", Digest::TOMBSTONE);
        assert_eq!((st, r, rev), (Status::Provisional, Some(Reason::NoRegister), None));
        let (_, _, rev) = v.answer(Purpose::Evaluate, RuleKind::Profile, "x", Digest::TOMBSTONE);
        assert_eq!(rev, Some(1));
        let pin = InterpretationPin::of(&v);
        let b = pin.canonical_bytes().unwrap_or_default();
        assert_eq!(InterpretationPin::from_canonical_bytes(&b), Some(pin));
        assert_eq!(pin.verification(), "none");
    }

    #[test]
    fn pins_round_trip_and_refuse_inconsistent_forms() {
        let v = RegisterView {
            view: View::Unverified { file: Digest::TOMBSTONE, code: "stale_register" },
            drafts: BTreeSet::new(),
        };
        let pin = InterpretationPin::of(&v);
        let b = pin.canonical_bytes().unwrap_or_default();
        assert_eq!(InterpretationPin::from_canonical_bytes(&b), Some(pin));
        let zero = Digest::TOMBSTONE;
        for bad in [
            format!(
                r#"{{"interpretation_register":"none","register_verification":"verified","schema":"{PIN_SCHEMA}"}}"#
            ),
            format!(
                r#"{{"interpretation_register":"{}","register_verification":"none","schema":"{PIN_SCHEMA}"}}"#,
                zero.as_str()
            ),
            format!(
                r#"{{"interpretation_register":"{}","register_verification":"odd","schema":"{PIN_SCHEMA}"}}"#,
                zero.as_str()
            ),
            format!(r#"{{ "interpretation_register":"none","register_verification":"none","schema":"{PIN_SCHEMA}"}}"#),
            r#"{"interpretation_register":"none","register_verification":"none","schema":"x"}"#.to_owned(),
        ] {
            assert_eq!(InterpretationPin::from_canonical_bytes(bad.as_bytes()), None, "{bad}");
        }
    }

    /// A complete record with a hand-made signed-off block (AC-9 second half, AC-11 false
    /// case): no verified register can produce one today, because GDT.RES.STATUS labels every
    /// profile provisional/no_signoff_path.
    fn signed_off_record() -> ResultRecord {
        use crate::chars::dprof::CharId;
        use crate::chars::record::{
            AlgorithmRef, EvaluationBasis, InputDigests, KernelVersion, Measurand, Quantity, ResultRecordBuilder,
            RunId, Uncertainty,
        };
        use crate::chars::spec::{GoverningSpec, SpecError};
        fn structural(reason: &'static str) -> SpecError {
            SpecError::BadRequest { reason }
        }
        let pref = "dp:asme-y14.5-2018@1.0.0#sha256:287a04d6c384768b1e44c85b3c4ae851912d700b723dcd28b72e395065ec84dc";
        let spec_json = format!(
            r#"{{"defaults_profile":"{pref}","dialect":"ASME","inherited_from":"explicit","standards":["std:asme:y14.5:2018"]}}"#
        );
        let spec = GoverningSpec::from_cvalue(&fid_canon::parse(spec_json.as_bytes()).unwrap(), structural).unwrap();
        let profile = ProfileRef::parse(pref).unwrap();
        let key = "dmt:asme/flatness/none@1";
        let mut row_keys = BTreeSet::new();
        row_keys.insert(key.to_owned());
        let mut flat = rule(RuleStatus::SignedOff);
        flat.content_digest = Digest::from_bytes([3; 32]);
        let prof = RuleUse {
            rule: profile.id().to_string(),
            kind: RuleKind::Profile,
            revision: Some(1),
            content_digest: *profile.digest(),
            status: RuleStatus::SignedOff,
            reason: None,
        };
        let mut chains = BTreeSet::new();
        chains.insert((key.to_owned(), Digest::from_bytes([1; 32])));
        let interp = Interpreted {
            rules: vec![flat, prof],
            row_keys,
            chains,
            profile: Some(profile),
            drafts: BTreeSet::new(),
            pin: InterpretationPin { register: Some(Digest::from_bytes([9; 32])), state: PinState::Verified },
            error: None,
        };
        let d = Digest::from_bytes([1; 32]);
        ResultRecordBuilder::new(RunId::parse("run-1").unwrap(), CharId::parse("flat-1").unwrap())
            .measurand(Measurand::parse("flatness of face A").unwrap())
            .evaluation_basis(EvaluationBasis::Points)
            .value(Quantity::new(1.0e-5, "si:m").unwrap())
            .uncertainty(Uncertainty::uec0(fid_kunc::Uec0Reason::UMissing))
            .bind_decision(None, None)
            .spec(spec)
            .algorithm(AlgorithmRef::parse("gdt.form.flatness.minzone@1.0.0").unwrap())
            .kernel_version(KernelVersion::parse("0.1.0").unwrap())
            .input_digests(InputDigests::new(vec![d]).unwrap())
            .journal_entry(d)
            .dmt_row(fid_gps::dmt::DmtKey::parse(key).unwrap())
            .operator_chain_digest(d)
            .dmt_digest(d)
            .with_interpreted(interp)
            .finish()
            .unwrap()
    }

    #[test]
    fn a_signed_off_record_shows_signed_off_and_no_provisional_line() {
        let r = signed_off_record();
        assert_eq!(r.interpretation(), Interpretation::SignedOff);
        assert!(!requires_provisional_label(&r));
        let text = crate::chars::record::render_text(&r);
        assert!(text.lines().any(|l| l == "interpretation: signed_off"));
        assert!(!text.contains("provisional"));
        let json = String::from_utf8(r.canonical_json().to_vec()).unwrap();
        assert!(json.contains(r#""interpretation":"signed_off""#));
    }
}
