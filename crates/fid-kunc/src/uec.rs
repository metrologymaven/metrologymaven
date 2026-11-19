//! Evidence class assignment, rule set `uec-assign@1` (algorithm `unc.uec.assign@1.0.0`).
//! Spec node `UNC.UEC.CLASS`.
//!
//! [`assign`] is the only place that grants UEC-2 or UEC-3. It is a pure, total function
//! of its input and of a [`SourceRecords`] lookup: no I/O, no arithmetic, no panics (D0).
//!
//! # Source verification
//!
//! A [`SourceClaim`] is what a producer *says* a contributor rests on; its kind is
//! self-asserted, exactly like [`SourceKind`] in a statement. [`assign`] looks every claim
//! up by content digest in the caller's [`SourceRecords`] and counts a contributor as
//! characterized only when a record exists and its kind and origin equal the claim. A claim
//! with no record, or whose record says anything else, is *unverified* and treated as
//! uncharacterized: the record never upgrades a claim (a `declared` claim whose record says
//! `dcc` stays uncharacterized). Whether a record is genuine or signed is owned by `TRUST`.
//!
//! # Checks (pinned order; the first failure is returned)
//!
//! 1. More than [`MAX_CONTRIBUTORS`] contributors: `invalid_input{contributors}`.
//! 2. Each `input_id` must be a statement token (1..=256 printable ASCII bytes without
//!    space, `"` or `\`): `invalid_input{input_id}`.
//! 3. Contributors are sorted by `input_id` bytes; two equal IDs: `duplicate_input`.
//! 4. In that order, a claim of kind `instrument_model` needs an [`Origin`] and every other
//!    kind must carry none: `inconsistent_input{origin}`.
//! 5. Basis `none`: contributors (`inconsistent_input{contributors}`) or a validation
//!    record (`inconsistent_input{validation_record}`) contradict "nothing evaluated".
//!    Otherwise UEC-0 with `model_missing` (no model bound), `instrument_missing` (model
//!    bound, an instrument input unbound) or `u_missing`.
//! 6. Basis `declared` or `spec_derived`: a validation record is
//!    `inconsistent_input{validation_record}`; otherwise UEC-1, never more, whatever the
//!    sources (REQ-UNC.UEC.CLASS-005). A claim is never promoted.
//! 7. Evaluated bases (`modelled_lpu`, `modelled_mcm`, `iso15530_3`, `iso15530_4`): model
//!    not bound `inconsistent_input{model_bound}`; an instrument input unbound
//!    `inconsistent_input{instruments_bound}`; no contributors `empty_model`; a validation
//!    record on a modelled basis `inconsistent_input{validation_record}`.
//! 8. Contributors are scanned in `input_id` order. The first one that is not a verified
//!    characterized source (`dcc`, `qualification`, `type_a`) gives UEC-1 with a
//!    [`Downgrade`] naming it (REQ-UNC.UEC-005, REQ-UNC.UEC.CLASS-003):
//!    `unsourced_contributor` (no source, a `declared` claim, or a specification-origin
//!    instrument model claim), `uncharacterized_contributor` (an IEMPS-origin instrument
//!    model claim) or `unverified_source` (a `dcc`, `qualification` or `type_a` claim with no
//!    record for its digest, or whose record has another kind).
//! 9. `iso15530_3` / `iso15530_4`: UEC-3 when the validation record is verified (a record
//!    of kind [`RecordFacts::Validation`]); otherwise UEC-2 with downgrade `not_validated`
//!    (REQ-UNC.UEC-006). `modelled_lpu` / `modelled_mcm`: UEC-2.
//!
//! # Labels carried through
//!
//! [`AssignOutput`] carries the three labelling facts of `UNC.UEC.STMT` and
//! `UNC.DEC.TYPE`: [`AssignOutput::is_claimed`] (basis `declared` or `spec_derived`),
//! [`AssignOutput::rests_on_declared_input`] (a claim or its record has kind `declared`) and
//! [`AssignOutput::rests_on_uncharacterized_input`] (any contributor not a verified
//! characterized source). [`AssignOutput::statement`] builds the statement and refuses one
//! that would drop any of them, so a claimed or unverified value never looks measured.
//!
//! # Honest UEC-1 statements over unverified claims
//!
//! A contributor whose claim no record confirmed cannot be cited as a plain source, and
//! leaving it out drops a label. A UEC-0/1 statement cites it instead as a source marked
//! `unverified` (see [`AssignOutput::unverified_claims`]), with the claimed kind or, when a
//! source record of another kind exists, the record's kind (so a `dcc` claim whose record
//! says `declared` can carry the declared label). Such a source counts as uncharacterized.
//! The mark is refused on UEC-2/3, by [`AssignOutput::statement`] and by `UNC.UEC.STMT`'s
//! validation (constructor and parser alike). A contributor with no claim at all has no
//! digest to cite, so its statement still fails `label_dropped` unless another cited source
//! carries the uncharacterized label.

use std::fmt;

use fid_types::Digest;

use crate::statement::{
    is_token, EvidenceClass, Method, SourceKind, SourceRef, StatementParts, StmtError, Uec0Reason,
    UncertaintyStatement, MAX_ITEMS,
};

/// Rule-set version recorded in `provenance.rule_sets` (REQ-UNC.UEC.CLASS-006).
pub const RULE_SET: &str = "uec-assign@1";
/// Algorithm ID of [`assign`].
pub const ALGORITHM_ID: &str = "unc.uec.assign@1.0.0";
/// Most contributors one call accepts (the statement's own limit).
pub const MAX_CONTRIBUTORS: usize = MAX_ITEMS;
/// Most records a [`RecordTable`] holds.
pub const MAX_RECORDS: usize = 1 << 20;

/// How the uncertainty of a result was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Basis {
    /// Nothing was evaluated or declared.
    None,
    /// Declared by a person.
    Declared,
    /// Derived from a specification (for example an instrument MPE).
    SpecDerived,
    /// Modelled, law of propagation of uncertainty.
    ModelledLpu,
    /// Modelled, Monte Carlo method.
    ModelledMcm,
    /// Substitution method, ISO 15530-3.
    Iso15530_3,
    /// Simulation, ISO/TS 15530-4.
    Iso15530_4,
}

impl Basis {
    /// Every value, in declaration order.
    pub const ALL: &'static [Basis] = &[
        Basis::None,
        Basis::Declared,
        Basis::SpecDerived,
        Basis::ModelledLpu,
        Basis::ModelledMcm,
        Basis::Iso15530_3,
        Basis::Iso15530_4,
    ];

    /// The wire string.
    pub const fn wire(self) -> &'static str {
        match self {
            Basis::None => "none",
            Basis::Declared => "declared",
            Basis::SpecDerived => "spec_derived",
            Basis::ModelledLpu => "modelled_lpu",
            Basis::ModelledMcm => "modelled_mcm",
            Basis::Iso15530_3 => "iso15530_3",
            Basis::Iso15530_4 => "iso15530_4",
        }
    }

    /// Parses the exact wire string (case-sensitive); anything else is `unknown_basis`.
    pub fn parse(s: &str) -> Result<Basis, AssignError> {
        Basis::ALL.iter().copied().find(|b| b.wire() == s).ok_or(AssignError::UnknownBasis)
    }

    /// The statement method tag of this basis (`None` for [`Basis::None`]).
    pub const fn method(self) -> Option<Method> {
        match self {
            Basis::None => None,
            Basis::Declared => Some(Method::Declared),
            Basis::SpecDerived => Some(Method::SpecDerived),
            Basis::ModelledLpu => Some(Method::Lpu),
            Basis::ModelledMcm => Some(Method::Mcm),
            Basis::Iso15530_3 => Some(Method::Iso15530_3),
            Basis::Iso15530_4 => Some(Method::Iso15530_4),
        }
    }

    /// True for a claim rather than an evaluation (`declared`, `spec_derived`).
    pub const fn is_claimed(self) -> bool {
        matches!(self, Basis::Declared | Basis::SpecDerived)
    }
}

impl fmt::Display for Basis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire())
    }
}

/// Origin of an instrument model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// Evaluated from the instrument's error model and performance tests (IEMPS).
    Iemps,
    /// Derived from a specification (for example a maximum permissible error).
    Specification,
}

/// A producer's claim about the source record a contributor rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceClaim {
    /// Claimed kind (self-asserted until verified).
    pub kind: SourceKind,
    /// Origin; set exactly when `kind` is `instrument_model`.
    pub origin: Option<Origin>,
    /// Content digest of the claimed source record.
    pub digest: Digest,
}

/// One model input and the source it claims to rest on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContributorSource {
    /// Model input ID (unique; scan key).
    pub input_id: String,
    /// The claimed source; `None` means the contributor references no source.
    pub source: Option<SourceClaim>,
}

/// Reference to the validation record (calibrated-workpiece comparison) of a
/// substitution or simulation evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecordRef {
    /// Content digest of the record.
    pub digest: Digest,
}

/// What a real record says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordFacts {
    /// A source record of this kind (and origin, for an instrument model).
    Source { kind: SourceKind, origin: Option<Origin> },
    /// A validation record against calibrated workpieces.
    Validation,
}

/// Lookup of records by content digest. Implementations must be deterministic: the same
/// digest gives the same answer for the duration of a call.
pub trait SourceRecords {
    /// The facts of the record with this digest, or `None` if no record is known.
    fn lookup(&self, digest: &Digest) -> Option<RecordFacts>;
}

/// A sorted, duplicate-free in-memory [`SourceRecords`] (binary search; no hashing).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecordTable {
    entries: Vec<(Digest, RecordFacts)>,
}

impl RecordTable {
    /// Builds a table. More than [`MAX_RECORDS`] entries fail
    /// `invalid_input{records}`; a digest listed twice fails `duplicate_record` even if
    /// both entries agree, so a lookup is never ambiguous.
    pub fn new(mut entries: Vec<(Digest, RecordFacts)>) -> Result<RecordTable, AssignError> {
        if entries.len() > MAX_RECORDS {
            return Err(AssignError::InvalidInput { field: "records" });
        }
        entries.sort_unstable_by_key(|e| e.0);
        if entries.windows(2).any(|w| matches!(w, [a, b] if a.0 == b.0)) {
            return Err(AssignError::DuplicateRecord);
        }
        Ok(RecordTable { entries })
    }

    /// Number of records.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the table holds no record.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl SourceRecords for RecordTable {
    fn lookup(&self, digest: &Digest) -> Option<RecordFacts> {
        let i = self.entries.binary_search_by(|e| e.0.cmp(digest)).ok()?;
        self.entries.get(i).map(|e| e.1)
    }
}

/// Input of [`assign`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssignInput {
    /// How the uncertainty was obtained.
    pub basis: Basis,
    /// Every model contributor and its claimed source (any order; scanned sorted).
    pub contributor_sources: Vec<ContributorSource>,
    /// Validation record (substitution and simulation bases only).
    pub validation_record: Option<RecordRef>,
    /// A measurement model is bound.
    pub model_bound: bool,
    /// Every instrument input of the bound model is bound.
    pub instruments_bound: bool,
}

/// Why the class is lower than the basis alone would allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DowngradeReason {
    /// A contributor has no source, a `declared` source or a specification-derived
    /// instrument model (REQ-UNC.UEC.CLASS-003).
    UnsourcedContributor,
    /// A contributor rests on an IEMPS-origin instrument model, which is not a
    /// characterized source in `uec-assign@1` (REQ-UNC.UEC-005).
    UncharacterizedContributor,
    /// A contributor's claimed kind or origin is not confirmed by a record.
    UnverifiedSource,
    /// Substitution or simulation without a verified validation record
    /// (REQ-UNC.UEC.CLASS-004).
    NotValidated,
}

impl DowngradeReason {
    /// Every value, in declaration order.
    pub const ALL: &'static [DowngradeReason] = &[
        DowngradeReason::UnsourcedContributor,
        DowngradeReason::UncharacterizedContributor,
        DowngradeReason::UnverifiedSource,
        DowngradeReason::NotValidated,
    ];

    /// Stable wire code.
    pub const fn code(self) -> &'static str {
        match self {
            DowngradeReason::UnsourcedContributor => "unsourced_contributor",
            DowngradeReason::UncharacterizedContributor => "uncharacterized_contributor",
            DowngradeReason::UnverifiedSource => "unverified_source",
            DowngradeReason::NotValidated => "not_validated",
        }
    }
}

impl fmt::Display for DowngradeReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// A typed downgrade, naming the contributor when one is to blame.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Downgrade {
    /// The reason.
    pub reason: DowngradeReason,
    /// The first offending contributor in `input_id` order (`None` for `not_validated`).
    pub input_id: Option<String>,
}

/// Result of [`assign`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssignOutput {
    basis: Basis,
    class: EvidenceClass,
    uec0_reason: Option<Uec0Reason>,
    downgrade: Option<Downgrade>,
    claimed: bool,
    declared_input: bool,
    uncharacterized_input: bool,
    /// Claims confirmed by their record, as (kind, digest); sorted, unique.
    verified: Vec<(SourceKind, Digest)>,
    /// Claims no record confirmed, as (claimed kind, digest) and, when a source record
    /// exists for the digest, also (record kind, digest); sorted, unique. Citable only as
    /// `unverified` sources of a UEC-0/1 statement.
    unverified: Vec<(SourceKind, Digest)>,
    /// Contributor input IDs of an evaluated basis, sorted; empty for other bases.
    inputs: Vec<String>,
}

impl AssignOutput {
    /// The basis the class was assigned for.
    pub fn basis(&self) -> Basis {
        self.basis
    }
    /// The assigned class.
    pub fn class(&self) -> EvidenceClass {
        self.class
    }
    /// The UEC-0 reason (UEC-0 only).
    pub fn uec0_reason(&self) -> Option<Uec0Reason> {
        self.uec0_reason
    }
    /// The downgrade, if the evidence did not support the basis's highest class.
    pub fn downgrade(&self) -> Option<&Downgrade> {
        self.downgrade.as_ref()
    }
    /// The value is a claim (basis `declared` or `spec_derived`), never a measurement.
    pub fn is_claimed(&self) -> bool {
        self.claimed
    }
    /// At least one contributor's claim or record has kind `declared`.
    pub fn rests_on_declared_input(&self) -> bool {
        self.declared_input
    }
    /// At least one contributor is not a verified characterized source.
    pub fn rests_on_uncharacterized_input(&self) -> bool {
        self.uncharacterized_input
    }
    /// The rule-set version, always [`RULE_SET`].
    pub fn rule(&self) -> &'static str {
        RULE_SET
    }
    /// The algorithm ID, always [`ALGORITHM_ID`].
    pub fn algorithm(&self) -> &'static str {
        ALGORITHM_ID
    }
    /// The sources a statement for this assignment may cite: every claim whose record
    /// confirmed its kind and origin, as (kind, digest), sorted and unique.
    pub fn citable_sources(&self) -> &[(SourceKind, Digest)] {
        &self.verified
    }
    /// The sources a UEC-0/1 statement may cite with `unverified: true`: every claim no
    /// record confirmed, as (claimed kind, digest), plus (record kind, digest) when a source
    /// record of another kind exists for that digest; sorted and unique. Empty for UEC-2/3.
    pub fn unverified_claims(&self) -> &[(SourceKind, Digest)] {
        &self.unverified
    }
    /// The contributor input IDs the statement must carry (sorted; empty unless the basis
    /// is evaluated, since a claim carries no contributors).
    pub fn contributor_ids(&self) -> &[String] {
        &self.inputs
    }

    /// Builds the statement for this assignment from `parts`; the only constructor of a
    /// UEC-2 or UEC-3 statement. Sets `class` and `uec0_reason`, adds [`RULE_SET`] to
    /// `provenance.rule_sets` (REQ-UNC.UEC.CLASS-006) and validates like
    /// [`UncertaintyStatement::new`]. Refused, in order: a `method` other than the basis's
    /// (`inconsistent_input{method}`); a source whose (kind, digest) is not one of
    /// [`Self::citable_sources`], or, for a source marked `unverified`, not one of
    /// [`Self::unverified_claims`] or on an assignment above UEC-1 (`source_not_verified`); contributor input IDs other than
    /// [`Self::contributor_ids`] (`contributor_mismatch`); any statement error
    /// (`statement_refused`); a statement whose labels are weaker than this assignment's
    /// (`label_dropped{is_claimed | rests_on_declared_input |
    /// rests_on_uncharacterized_input}`), for example one that leaves out an unverified input.
    pub fn statement(&self, mut parts: StatementParts) -> Result<UncertaintyStatement, AssignError> {
        if parts.method != self.basis.method() {
            return Err(AssignError::Inconsistent { field: "method" });
        }
        let low_class = matches!(self.class, EvidenceClass::Uec0 | EvidenceClass::Uec1);
        let citable = |s: &SourceRef| {
            if s.unverified {
                low_class && self.unverified.binary_search(&(s.kind, s.digest)).is_ok()
            } else {
                self.verified.binary_search(&(s.kind, s.digest)).is_ok()
            }
        };
        if !parts.sources.iter().all(citable) {
            return Err(AssignError::SourceNotVerified);
        }
        let mut ids: Vec<&str> = parts.contributors.iter().map(|c| c.input_id.as_str()).collect();
        ids.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        if !ids.iter().copied().eq(self.inputs.iter().map(String::as_str)) {
            return Err(AssignError::ContributorMismatch);
        }
        parts.class = self.class;
        parts.uec0_reason = self.uec0_reason;
        if !parts.provenance.rule_sets.iter().any(|r| r == RULE_SET) {
            parts.provenance.rule_sets.push(RULE_SET.to_owned());
        }
        let stmt = UncertaintyStatement::new_assigned(parts).map_err(AssignError::Statement)?;
        if stmt.is_claimed() != self.claimed {
            return Err(AssignError::LabelDropped { field: "is_claimed" });
        }
        if self.declared_input && !stmt.rests_on_declared_input() {
            return Err(AssignError::LabelDropped { field: "rests_on_declared_input" });
        }
        if self.uncharacterized_input && !stmt.rests_on_uncharacterized_input() {
            return Err(AssignError::LabelDropped { field: "rests_on_uncharacterized_input" });
        }
        Ok(stmt)
    }
}

/// Why an assignment was refused. No class is returned with an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignError {
    /// An evaluated basis with no contributors.
    EmptyModel,
    /// The basis string is not one of the seven wire names.
    UnknownBasis,
    /// Two contributors share an `input_id`.
    DuplicateInput,
    /// A [`RecordTable`] lists a digest twice.
    DuplicateRecord,
    /// An input field is malformed or too large.
    InvalidInput { field: &'static str },
    /// The input contradicts itself, so the class would be ambiguous.
    Inconsistent { field: &'static str },
    /// The statement built for the assignment was refused by `UNC.UEC.STMT`.
    Statement(StmtError),
    /// The statement would not carry a label the assignment sets.
    LabelDropped { field: &'static str },
    /// The statement cites a source that is not a verified claim of the assignment.
    SourceNotVerified,
    /// The statement's contributors are not exactly the assigned contributors.
    ContributorMismatch,
}

impl AssignError {
    /// Stable error code.
    pub const fn code(self) -> &'static str {
        match self {
            AssignError::EmptyModel => "empty_model",
            AssignError::UnknownBasis => "unknown_basis",
            AssignError::DuplicateInput => "duplicate_input",
            AssignError::DuplicateRecord => "duplicate_record",
            AssignError::InvalidInput { .. } => "invalid_input",
            AssignError::Inconsistent { .. } => "inconsistent_input",
            AssignError::Statement(_) => "statement_refused",
            AssignError::LabelDropped { .. } => "label_dropped",
            AssignError::SourceNotVerified => "source_not_verified",
            AssignError::ContributorMismatch => "contributor_mismatch",
        }
    }

    /// The named field, or the statement error code for `statement_refused`.
    pub const fn detail(self) -> Option<&'static str> {
        match self {
            AssignError::InvalidInput { field }
            | AssignError::Inconsistent { field }
            | AssignError::LabelDropped { field } => Some(field),
            AssignError::Statement(e) => Some(e.code()),
            AssignError::EmptyModel
            | AssignError::UnknownBasis
            | AssignError::DuplicateInput
            | AssignError::DuplicateRecord
            | AssignError::SourceNotVerified
            | AssignError::ContributorMismatch => None,
        }
    }
}

impl fmt::Display for AssignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.detail() {
            Some(x) => write!(f, "{}: {x}", self.code()),
            None => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for AssignError {}

/// Verified status of one contributor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Characterized,
    Unsourced,
    Uncharacterized,
    Unverified,
}

/// Checks one claim against its record. The record confirms or refutes the claim; it never
/// replaces it. A claim that could never be characterized reports its own weakness whatever
/// the record says; only a claim of a characterized kind needs the record to count.
/// The confirmation rule shared by [`assign`] and
/// [`UncertaintyStatement::parse_canonical_verified`]: a record exists for `digest` and says
/// exactly this kind and origin.
pub(crate) fn confirms(records: &dyn SourceRecords, kind: SourceKind, origin: Option<Origin>, digest: &Digest) -> bool {
    records.lookup(digest) == Some(RecordFacts::Source { kind, origin })
}

fn verify(claim: Option<&SourceClaim>, records: &dyn SourceRecords) -> (Status, bool, bool) {
    let Some(c) = claim else { return (Status::Unsourced, false, false) };
    let record = records.lookup(&c.digest);
    let confirmed = confirms(records, c.kind, c.origin, &c.digest);
    let declared = c.kind == SourceKind::Declared
        || matches!(record, Some(RecordFacts::Source { kind: SourceKind::Declared, .. }));
    let status = match (c.kind, c.origin) {
        (SourceKind::Dcc | SourceKind::Qualification | SourceKind::TypeA, _) => {
            if confirmed {
                Status::Characterized
            } else {
                Status::Unverified
            }
        }
        (SourceKind::InstrumentModel, Some(Origin::Iemps)) => Status::Uncharacterized,
        (SourceKind::Declared | SourceKind::InstrumentModel, _) => Status::Unsourced,
    };
    (status, declared, confirmed)
}

fn inconsistent(field: &'static str) -> AssignError {
    AssignError::Inconsistent { field }
}

/// Assigns the evidence class under rule set `uec-assign@1` (see the module documentation
/// for the pinned order of checks and rules).
pub fn assign(input: &AssignInput, records: &dyn SourceRecords) -> Result<AssignOutput, AssignError> {
    let cs = &input.contributor_sources;
    if cs.len() > MAX_CONTRIBUTORS {
        return Err(AssignError::InvalidInput { field: "contributors" });
    }
    if !cs.iter().all(|c| is_token(&c.input_id)) {
        return Err(AssignError::InvalidInput { field: "input_id" });
    }
    let mut sorted: Vec<&ContributorSource> = cs.iter().collect();
    sorted.sort_by(|a, b| a.input_id.as_bytes().cmp(b.input_id.as_bytes()));
    if sorted.windows(2).any(|w| matches!(w, [a, b] if a.input_id == b.input_id)) {
        return Err(AssignError::DuplicateInput);
    }
    for c in &sorted {
        if let Some(s) = &c.source {
            if (s.kind == SourceKind::InstrumentModel) != s.origin.is_some() {
                return Err(inconsistent("origin"));
            }
        }
    }

    let basis = input.basis;
    let mut out = AssignOutput {
        basis,
        class: EvidenceClass::Uec0,
        uec0_reason: None,
        downgrade: None,
        claimed: basis.is_claimed(),
        declared_input: false,
        uncharacterized_input: false,
        verified: Vec::new(),
        unverified: Vec::new(),
        inputs: Vec::new(),
    };

    if basis == Basis::None {
        if !cs.is_empty() {
            return Err(inconsistent("contributors"));
        }
        if input.validation_record.is_some() {
            return Err(inconsistent("validation_record"));
        }
        out.uec0_reason = Some(if !input.model_bound {
            Uec0Reason::ModelMissing
        } else if !input.instruments_bound {
            Uec0Reason::InstrumentMissing
        } else {
            Uec0Reason::UMissing
        });
        return Ok(out);
    }

    let substitution = matches!(basis, Basis::Iso15530_3 | Basis::Iso15530_4);
    if basis.is_claimed() {
        if input.validation_record.is_some() {
            return Err(inconsistent("validation_record"));
        }
    } else {
        if !input.model_bound {
            return Err(inconsistent("model_bound"));
        }
        if !input.instruments_bound {
            return Err(inconsistent("instruments_bound"));
        }
        if cs.is_empty() {
            return Err(AssignError::EmptyModel);
        }
        if !substitution && input.validation_record.is_some() {
            return Err(inconsistent("validation_record"));
        }
    }

    let mut first: Option<(Status, &str)> = None;
    for c in &sorted {
        let (status, declared, confirmed) = verify(c.source.as_ref(), records);
        out.declared_input |= declared;
        if let Some(s) = &c.source {
            if confirmed {
                out.verified.push((s.kind, s.digest));
            } else {
                out.unverified.push((s.kind, s.digest));
                if let Some(RecordFacts::Source { kind, .. }) = records.lookup(&s.digest) {
                    out.unverified.push((kind, s.digest));
                }
            }
        }
        if status != Status::Characterized {
            out.uncharacterized_input = true;
            if first.is_none() {
                first = Some((status, c.input_id.as_str()));
            }
        }
    }

    out.verified.sort_unstable();
    out.verified.dedup();
    out.unverified.sort_unstable();
    out.unverified.dedup();

    if basis.is_claimed() {
        out.class = EvidenceClass::Uec1;
        return Ok(out);
    }
    out.inputs = sorted.iter().map(|c| c.input_id.clone()).collect();
    if let Some((status, id)) = first {
        let reason = match status {
            Status::Uncharacterized => DowngradeReason::UncharacterizedContributor,
            Status::Unverified => DowngradeReason::UnverifiedSource,
            Status::Unsourced | Status::Characterized => DowngradeReason::UnsourcedContributor,
        };
        out.class = EvidenceClass::Uec1;
        out.downgrade = Some(Downgrade { reason, input_id: Some(id.to_owned()) });
        return Ok(out);
    }
    if substitution {
        let validated =
            input.validation_record.is_some_and(|r| records.lookup(&r.digest) == Some(RecordFacts::Validation));
        if validated {
            out.class = EvidenceClass::Uec3;
        } else {
            out.class = EvidenceClass::Uec2;
            out.downgrade = Some(Downgrade { reason: DowngradeReason::NotValidated, input_id: None });
        }
        return Ok(out);
    }
    out.class = EvidenceClass::Uec2;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_wire_names_round_trip() {
        for b in Basis::ALL {
            assert_eq!(Basis::parse(b.wire()), Ok(*b));
        }
        assert_eq!(Basis::parse("modelled"), Err(AssignError::UnknownBasis));
        assert_eq!(Basis::parse("NONE"), Err(AssignError::UnknownBasis));
    }

    #[test]
    fn claimed_bases_map_to_claimed_methods() {
        for b in Basis::ALL {
            assert_eq!(b.is_claimed(), b.method().is_some_and(Method::is_claimed), "{b}");
        }
    }
}
