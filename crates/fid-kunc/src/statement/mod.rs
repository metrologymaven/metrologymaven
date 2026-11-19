//! The uncertainty statement every ResultRecord carries. Spec node `UNC.UEC.STMT`.
//!
//! [`UncertaintyStatement`] values come from two places only: the validating constructor
//! [`UncertaintyStatement::new`] and the canonical parser
//! [`UncertaintyStatement::parse_canonical`]. Both run the same checks, so every value in
//! memory satisfies the invariants below, and every value has one canonical byte form and
//! one digest.
//!
//! # Validation order (pinned, so the first reported error is deterministic)
//!
//! 1. **Class.** A UEC-0 statement carries no evaluation output: the first of `method`,
//!    `u_std`, `expanded`, `interval`, `contributors` (non-empty), `lpu_validated_by_mc` and
//!    `nonlinearity` that is set fails with `uec0_has_value{field}`. A UEC-1..3 statement
//!    carrying a `uec0_reason` fails with `class_mismatch{uec0_reason}`.
//! 2. **Numbers**, in declaration order, with `invalid_number{field}`: `u_std`,
//!    `expanded.U`, `expanded.k`, `interval.low`, `interval.high`, `interval.p`, then per
//!    contributor `contributors.sensitivity`, `contributors.u_contrib`,
//!    `contributors.variance_share`, then `nonlinearity.v1`, `nonlinearity.v2`,
//!    `nonlinearity.ratio`. See [Numbers](#numbers).
//! 3. **Completeness**, with `statement_incomplete{field}`: UEC-0 needs `uec0_reason`;
//!    UEC-1..3 need `method`, `unit`, `coverage` (an `expanded` `{U, k}` or an `interval`
//!    with its `p`) and `model`, in that order.
//! 4. **Text fields**, with `invalid_field{field}`: unit, model ID and semver, source IDs,
//!    contributor input IDs, selection rule, estimate convention, nonlinearity version and
//!    reason, provenance strings; then vector sizes and duplicate IDs.
//! 5. **Class consistency**, with `class_mismatch{field}`: claimed methods (`DECLARED`,
//!    `SPEC_DERIVED`) only with UEC-1, so a claimed value never looks measured; UEC-3 only
//!    with `ISO15530-3` or `ISO15530-4` (REQ-UNC.UEC-006); UEC-2 and UEC-3 need at least one
//!    source and only characterized ones (`dcc`, `qualification`, `type_a`;
//!    REQ-UNC.UEC-005). Provenance: an agent principal or an adopter implies a proposal
//!    (CONSTITUTION C5).
//!
//! # Numbers
//!
//! Checks look at IEEE-754 bit patterns and use comparisons only; nothing is computed (D0).
//! NaN and infinities always fail. Negative zero always fails (each value has one bit
//! pattern, so equal statements have equal digests). `u_std`, `U` and `k` must be strictly
//! positive: a zero uncertainty is never a valid claim, and an absent one is UEC-0
//! (REQ-UNC.UEC-002). `p` lies in the open interval (0, 1). `low < high` strictly.
//! `u_contrib`, `v1`, `v2` and `ratio` are non-negative, `variance_share` lies in [0, 1],
//! `sensitivity` is any finite value.

mod reader;
mod wire;

use std::fmt;

use fid_types::Digest;

/// Schema tag written as the `schema` member of every canonical statement.
pub const SCHEMA: &str = "uncertainty_statement@1";
/// Largest canonical document the parser reads, in bytes.
pub const MAX_INPUT_BYTES: usize = 1 << 20;
/// Most entries in each of `sources`, `contributors` and `provenance.rule_sets`.
pub const MAX_ITEMS: usize = 10_000;
/// Longest text field, in bytes.
pub const MAX_TEXT_BYTES: usize = 256;

macro_rules! wire_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $v:ident => $w:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $($(#[$vm])* $v),+ }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$v),+];

            /// The wire string.
            pub const fn wire(self) -> &'static str {
                match self { $($name::$v => $w),+ }
            }

            /// Parses the exact wire string (case-sensitive).
            pub fn from_wire(s: &str) -> Option<$name> {
                match s { $($w => Some($name::$v),)+ _ => None }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.wire())
            }
        }
    };
}

wire_enum! {
    /// Uncertainty evidence class (REQ-UNC.UEC-001).
    EvidenceClass {
        /// Not evaluated. Carries no number.
        Uec0 => "UEC-0",
        /// Declared, specification-derived or not fully characterized.
        Uec1 => "UEC-1",
        /// Evaluated from characterized sources only.
        Uec2 => "UEC-2",
        /// ISO 15530-3 substitution or ISO/TS 15530-4 simulation, validated.
        Uec3 => "UEC-3",
    }
}

wire_enum! {
    /// Why a result is UEC-0. Chosen by `UNC.UEC.CLASS`; fixes listed by `UNC.UEC.EXPL`.
    Uec0Reason {
        /// No uncertainty was evaluated or declared.
        UMissing => "u_missing",
        /// No measurement model is bound.
        ModelMissing => "model_missing",
        /// No instrument model is bound.
        InstrumentMissing => "instrument_missing",
    }
}

wire_enum! {
    /// Evaluation method tag (REQ-UNC.UEC.STMT-004).
    Method {
        /// Law of propagation of uncertainty (JCGM 100:2008, clause 5).
        Lpu => "LPU",
        /// Monte Carlo method (JCGM 101:2008).
        Mcm => "MCM",
        /// Substitution method, ISO 15530-3.
        Iso15530_3 => "ISO15530-3",
        /// Simulation, ISO/TS 15530-4.
        Iso15530_4 => "ISO15530-4",
        /// Declared by a person.
        Declared => "DECLARED",
        /// Derived from a specification (for example an instrument MPE).
        SpecDerived => "SPEC_DERIVED",
    }
}

impl Method {
    /// True for methods whose value is a claim rather than an evaluation (`DECLARED`,
    /// `SPEC_DERIVED`). Surfaces must label such values as claimed.
    pub const fn is_claimed(self) -> bool {
        matches!(self, Method::Declared | Method::SpecDerived)
    }
}

wire_enum! {
    /// Kind of coverage interval.
    IntervalKind {
        /// Shortest coverage interval.
        Shortest => "shortest",
        /// Probabilistically symmetric coverage interval.
        ProbSymmetric => "prob_symmetric",
    }
}

wire_enum! {
    /// Kind of an input source record.
    SourceKind {
        /// Digital calibration certificate.
        Dcc => "dcc",
        /// Qualification record.
        Qualification => "qualification",
        /// Type-A dataset.
        TypeA => "type_a",
        /// Declared by a person.
        Declared => "declared",
        /// Instrument model.
        InstrumentModel => "instrument_model",
    }
}

impl SourceKind {
    /// True for characterized sources (calibration certificate, qualification record,
    /// type-A dataset; REQ-UNC.UEC-005).
    pub const fn is_characterized(self) -> bool {
        matches!(self, SourceKind::Dcc | SourceKind::Qualification | SourceKind::TypeA)
    }
}

wire_enum! {
    /// Outcome of the nonlinearity assessment (`UNC.PROP.NLIN`).
    NlinVerdict {
        /// First-order LPU is adequate.
        LinearAdequate => "linear_adequate",
        /// Second-order terms are needed.
        SecondOrderNeeded => "second_order_needed",
        /// Use the Monte Carlo method.
        UseMcm => "use_mcm",
    }
}

wire_enum! {
    /// Kind of the principal that produced a statement.
    PrincipalKind {
        /// A person.
        Human => "human",
        /// An agent. Its statements are proposals (CONSTITUTION C5).
        Agent => "agent",
    }
}

/// Expanded uncertainty `U` with its coverage factor `k`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Expanded {
    /// The expanded uncertainty `U` (wire member `U`).
    pub value: f64,
    /// The coverage factor `k`.
    pub k: f64,
}

/// A coverage interval with its coverage probability.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverageInterval {
    /// Lower end.
    pub low: f64,
    /// Upper end.
    pub high: f64,
    /// Coverage probability, in (0, 1).
    pub p: f64,
    /// Kind of interval.
    pub kind: IntervalKind,
}

/// Reference to the measurement model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    /// Model ID, `um:<name>`.
    pub id: String,
    /// Exact semver `MAJOR.MINOR.PATCH`.
    pub semver: String,
    /// Content digest of the model.
    pub digest: Digest,
}

/// Reference to an input source record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    /// Kind of source.
    pub kind: SourceKind,
    /// Source ID (unique within a statement; sort key).
    pub id: String,
    /// Content digest of the source record.
    pub digest: Digest,
}

/// One uncertainty contributor (data supplied by `UNC.PROP`).
#[derive(Debug, Clone, PartialEq)]
pub struct Contributor {
    /// Model input ID (unique within a statement; sort key).
    pub input_id: String,
    /// Sensitivity coefficient, when the method has one.
    pub sensitivity: Option<f64>,
    /// Contribution to the standard uncertainty.
    pub u_contrib: f64,
    /// Share of the output variance, in [0, 1].
    pub variance_share: f64,
}

/// Recorded nonlinearity assessment (`UNC.PROP.NLIN`).
#[derive(Debug, Clone, PartialEq)]
pub struct Nonlinearity {
    /// Criteria version, for example `nlin@1`.
    pub version: String,
    /// The outcome.
    pub verdict: NlinVerdict,
    /// First-order variance estimate.
    pub v1: f64,
    /// Second-order variance estimate.
    pub v2: f64,
    /// `V2 / V1` as computed by `UNC.PROP.NLIN`, when defined.
    pub ratio: Option<f64>,
    /// Number of probed inputs.
    pub probed_inputs: u32,
    /// Number of probed input pairs.
    pub probed_pairs: u32,
    /// Whether cross terms were truncated.
    pub cross_terms_truncated: bool,
    /// Reason code, for example `probe_failed`.
    pub reason: Option<String>,
}

impl Nonlinearity {
    /// LPU is reportable only when the verdict is `linear_adequate`.
    pub fn lpu_reportable(&self) -> bool {
        self.verdict == NlinVerdict::LinearAdequate
    }
}

/// Who produced a statement and under which rule sets.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Provenance {
    /// Rule-set versions applied, for example `uec-assign@1` (kept sorted, unique).
    pub rule_sets: Vec<String>,
    /// Principal that produced the statement.
    pub principal: Option<String>,
    /// Kind of that principal; set exactly when `principal` is set.
    pub principal_kind: Option<PrincipalKind>,
    /// True when the statement is a proposal that needs human adoption.
    pub proposal: bool,
    /// The person who adopted a proposal.
    pub adopted_by: Option<String>,
    /// Digest of the statement this one supersedes.
    pub supersedes: Option<Digest>,
}

/// Every field of a statement, as given to [`UncertaintyStatement::new`]. The class is not
/// optional, so no statement exists without one (REQ-UNC.UEC.STMT-001).
#[derive(Debug, Clone, PartialEq)]
pub struct StatementParts {
    pub class: EvidenceClass,
    pub uec0_reason: Option<Uec0Reason>,
    pub method: Option<Method>,
    /// Unit URI, `<scheme>:<rest>`.
    pub unit: Option<String>,
    pub u_std: Option<f64>,
    pub expanded: Option<Expanded>,
    pub interval: Option<CoverageInterval>,
    pub model: Option<ModelRef>,
    pub sources: Vec<SourceRef>,
    pub contributors: Vec<Contributor>,
    pub lpu_validated_by_mc: Option<bool>,
    pub nonlinearity: Option<Nonlinearity>,
    pub selection_rule: Option<String>,
    pub estimate_convention: Option<String>,
    pub provenance: Provenance,
}

impl StatementParts {
    /// Parts with the given class and every other field empty.
    pub fn new(class: EvidenceClass) -> StatementParts {
        StatementParts {
            class,
            uec0_reason: None,
            method: None,
            unit: None,
            u_std: None,
            expanded: None,
            interval: None,
            model: None,
            sources: Vec::new(),
            contributors: Vec::new(),
            lpu_validated_by_mc: None,
            nonlinearity: None,
            selection_rule: None,
            estimate_convention: None,
            provenance: Provenance::default(),
        }
    }
}

/// Why a statement was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StmtError {
    /// A UEC-0 statement carries an evaluation output.
    Uec0HasValue { field: &'static str },
    /// A required field is missing (the first in validation order).
    StatementIncomplete { field: &'static str },
    /// The method tag is not one of the six known tags.
    UnknownMethod,
    /// A number is NaN, infinite, negative zero or out of its range.
    InvalidNumber { field: &'static str },
    /// A text field, enum value, size or ID set is invalid.
    InvalidField { field: &'static str },
    /// The class does not fit the method, sources, reason or provenance.
    ClassMismatch { field: &'static str },
    /// The input is not a well-formed statement document (detail is a fixed code).
    Malformed { detail: &'static str },
    /// The input is a valid statement but not its canonical bytes.
    NonCanonical,
}

impl StmtError {
    /// Stable error code.
    pub const fn code(self) -> &'static str {
        match self {
            StmtError::Uec0HasValue { .. } => "uec0_has_value",
            StmtError::StatementIncomplete { .. } => "statement_incomplete",
            StmtError::UnknownMethod => "unknown_method",
            StmtError::InvalidNumber { .. } => "invalid_number",
            StmtError::InvalidField { .. } => "invalid_field",
            StmtError::ClassMismatch { .. } => "class_mismatch",
            StmtError::Malformed { .. } => "malformed",
            StmtError::NonCanonical => "non_canonical",
        }
    }

    /// The named field or detail code, if any.
    pub const fn field(self) -> Option<&'static str> {
        match self {
            StmtError::Uec0HasValue { field }
            | StmtError::StatementIncomplete { field }
            | StmtError::InvalidNumber { field }
            | StmtError::InvalidField { field }
            | StmtError::ClassMismatch { field } => Some(field),
            StmtError::Malformed { detail } => Some(detail),
            StmtError::UnknownMethod | StmtError::NonCanonical => None,
        }
    }
}

impl fmt::Display for StmtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.field() {
            Some(x) => write!(f, "{}: {x}", self.code()),
            None => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for StmtError {}

/// A validated uncertainty statement with its canonical bytes and digest.
#[derive(Debug, Clone, PartialEq)]
pub struct UncertaintyStatement {
    parts: StatementParts,
    canonical: Vec<u8>,
    digest: Digest,
}

impl UncertaintyStatement {
    /// Validates `parts` (see the module documentation for the order) and builds the
    /// statement. `sources` are sorted by `id`, `contributors` by `input_id` and
    /// `provenance.rule_sets` by value (byte order) before the duplicate check.
    pub fn new(mut parts: StatementParts) -> Result<UncertaintyStatement, StmtError> {
        parts.sources.sort_by(|a, b| a.id.as_bytes().cmp(b.id.as_bytes()));
        parts.contributors.sort_by(|a, b| a.input_id.as_bytes().cmp(b.input_id.as_bytes()));
        parts.provenance.rule_sets.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        validate(&parts)?;
        let canonical = wire::to_canonical(&parts)?;
        let digest = fid_kci::kci::sha256_digest(&canonical);
        Ok(UncertaintyStatement { parts, canonical, digest })
    }

    /// Parses canonical statement bytes. Anything that is not exactly the canonical form
    /// of a valid statement is refused (`non_canonical` when only the bytes differ).
    pub fn parse_canonical(input: &[u8]) -> Result<UncertaintyStatement, StmtError> {
        if input.len() > MAX_INPUT_BYTES {
            return Err(StmtError::Malformed { detail: "too_large" });
        }
        let json = reader::read(input)?;
        let parts = wire::from_json(json)?;
        let stmt = UncertaintyStatement::new(parts)?;
        if stmt.canonical != input {
            return Err(StmtError::NonCanonical);
        }
        Ok(stmt)
    }

    /// The canonical RFC 8785 bytes (REQ-UNC.UEC.STMT-005).
    pub fn canonical_json(&self) -> &[u8] {
        &self.canonical
    }

    /// SHA-256 of [`Self::canonical_json`].
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// A copy of every field.
    pub fn to_parts(&self) -> StatementParts {
        self.parts.clone()
    }

    /// The evidence class.
    pub fn class(&self) -> EvidenceClass {
        self.parts.class
    }
    /// The UEC-0 reason (UEC-0 only).
    pub fn uec0_reason(&self) -> Option<Uec0Reason> {
        self.parts.uec0_reason
    }
    /// The method tag.
    pub fn method(&self) -> Option<Method> {
        self.parts.method
    }
    /// True when the value is a claim (`DECLARED` or `SPEC_DERIVED`), never a measurement.
    pub fn is_claimed(&self) -> bool {
        self.parts.method.is_some_and(Method::is_claimed)
    }
    /// The unit URI.
    pub fn unit(&self) -> Option<&str> {
        self.parts.unit.as_deref()
    }
    /// The standard uncertainty.
    pub fn u_std(&self) -> Option<f64> {
        self.parts.u_std
    }
    /// The expanded uncertainty and coverage factor.
    pub fn expanded(&self) -> Option<Expanded> {
        self.parts.expanded
    }
    /// The coverage interval.
    pub fn interval(&self) -> Option<CoverageInterval> {
        self.parts.interval
    }
    /// The model reference.
    pub fn model(&self) -> Option<&ModelRef> {
        self.parts.model.as_ref()
    }
    /// Sources, sorted by ID.
    pub fn sources(&self) -> &[SourceRef] {
        &self.parts.sources
    }
    /// Contributors, sorted by input ID.
    pub fn contributors(&self) -> &[Contributor] {
        &self.parts.contributors
    }
    /// Whether LPU was validated by MCM.
    pub fn lpu_validated_by_mc(&self) -> Option<bool> {
        self.parts.lpu_validated_by_mc
    }
    /// The nonlinearity assessment.
    pub fn nonlinearity(&self) -> Option<&Nonlinearity> {
        self.parts.nonlinearity.as_ref()
    }
    /// The selection-rule version.
    pub fn selection_rule(&self) -> Option<&str> {
        self.parts.selection_rule.as_deref()
    }
    /// The estimate convention.
    pub fn estimate_convention(&self) -> Option<&str> {
        self.parts.estimate_convention.as_deref()
    }
    /// The provenance.
    pub fn provenance(&self) -> &Provenance {
        &self.parts.provenance
    }
}

// ------------------------------------------------------------------ validation

const SIGN: u64 = 1 << 63;
const EXP: u64 = 0x7ff0_0000_0000_0000;
const ONE: u64 = 0x3ff0_0000_0000_0000;

/// Finite and not negative zero.
fn finite(x: f64) -> bool {
    let b = x.to_bits();
    b & EXP != EXP && b != SIGN
}

/// Finite and >= +0.
fn non_negative(x: f64) -> bool {
    finite(x) && x.to_bits() & SIGN == 0
}

/// Finite and > 0.
fn positive(x: f64) -> bool {
    non_negative(x) && x.to_bits() != 0
}

fn check(ok: bool, field: &'static str) -> Result<(), StmtError> {
    if ok {
        Ok(())
    } else {
        Err(StmtError::InvalidNumber { field })
    }
}

fn validate(p: &StatementParts) -> Result<(), StmtError> {
    check_class(p)?;
    check_numbers(p)?;
    check_complete(p)?;
    check_text(p)?;
    check_consistency(p)
}

fn check_class(p: &StatementParts) -> Result<(), StmtError> {
    if p.class != EvidenceClass::Uec0 {
        if p.uec0_reason.is_some() {
            return Err(StmtError::ClassMismatch { field: "uec0_reason" });
        }
        return Ok(());
    }
    let set: [(&'static str, bool); 7] = [
        ("method", p.method.is_some()),
        ("u_std", p.u_std.is_some()),
        ("expanded", p.expanded.is_some()),
        ("interval", p.interval.is_some()),
        ("contributors", !p.contributors.is_empty()),
        ("lpu_validated_by_mc", p.lpu_validated_by_mc.is_some()),
        ("nonlinearity", p.nonlinearity.is_some()),
    ];
    match set.iter().find(|(_, s)| *s) {
        Some((field, _)) => Err(StmtError::Uec0HasValue { field: *field }),
        None => Ok(()),
    }
}

fn check_numbers(p: &StatementParts) -> Result<(), StmtError> {
    if let Some(u) = p.u_std {
        check(positive(u), "u_std")?;
    }
    if let Some(e) = p.expanded {
        check(positive(e.value), "expanded.U")?;
        check(positive(e.k), "expanded.k")?;
    }
    if let Some(i) = p.interval {
        check(finite(i.low), "interval.low")?;
        check(finite(i.high) && i.low < i.high, "interval.high")?;
        check(positive(i.p) && i.p.to_bits() < ONE, "interval.p")?;
    }
    for c in &p.contributors {
        if let Some(s) = c.sensitivity {
            check(finite(s), "contributors.sensitivity")?;
        }
        check(non_negative(c.u_contrib), "contributors.u_contrib")?;
        check(non_negative(c.variance_share) && c.variance_share.to_bits() <= ONE, "contributors.variance_share")?;
    }
    if let Some(n) = &p.nonlinearity {
        check(non_negative(n.v1), "nonlinearity.v1")?;
        check(non_negative(n.v2), "nonlinearity.v2")?;
        if let Some(r) = n.ratio {
            check(non_negative(r), "nonlinearity.ratio")?;
        }
    }
    Ok(())
}

fn check_complete(p: &StatementParts) -> Result<(), StmtError> {
    let missing = if p.class == EvidenceClass::Uec0 {
        [("uec0_reason", p.uec0_reason.is_none())].into_iter().find(|(_, m)| *m)
    } else {
        [
            ("method", p.method.is_none()),
            ("unit", p.unit.is_none()),
            ("coverage", p.expanded.is_none() && p.interval.is_none()),
            ("model", p.model.is_none()),
        ]
        .into_iter()
        .find(|(_, m)| *m)
    };
    match missing {
        Some((field, _)) => Err(StmtError::StatementIncomplete { field }),
        None => Ok(()),
    }
}

/// Text fields are 1..=256 bytes of printable ASCII without space, `"` or `\`, so the
/// canonical form never needs an escape.
pub(crate) fn is_token(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_TEXT_BYTES && s.bytes().all(|b| (0x21..=0x7e).contains(&b) && b != b'"' && b != b'\\')
}

/// `<scheme>:<rest>` with an RFC 3986 scheme and a non-empty rest.
fn is_uri(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once(':') else { return false };
    let mut sb = scheme.bytes();
    is_token(s)
        && !rest.is_empty()
        && sb.next().is_some_and(|b| b.is_ascii_alphabetic())
        && sb.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

/// `MAJOR.MINOR.PATCH`, each 1..=9 digits without leading zeros.
fn is_semver(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|n| {
            !n.is_empty() && n.len() <= 9 && n.bytes().all(|b| b.is_ascii_digit()) && (n.len() == 1 || !n.starts_with('0'))
        })
}

fn text(ok: bool, field: &'static str) -> Result<(), StmtError> {
    if ok {
        Ok(())
    } else {
        Err(StmtError::InvalidField { field })
    }
}

fn opt_token(s: &Option<String>, field: &'static str) -> Result<(), StmtError> {
    match s {
        Some(s) => text(is_token(s), field),
        None => Ok(()),
    }
}

/// Strictly increasing by bytes (sorted, no duplicates).
fn strictly_sorted<'a>(mut ids: impl Iterator<Item = &'a str>) -> bool {
    let Some(mut prev) = ids.next() else { return true };
    for id in ids {
        if prev.as_bytes() >= id.as_bytes() {
            return false;
        }
        prev = id;
    }
    true
}

fn check_text(p: &StatementParts) -> Result<(), StmtError> {
    if let Some(u) = &p.unit {
        text(is_uri(u), "unit")?;
    }
    if let Some(m) = &p.model {
        text(is_token(&m.id) && m.id.len() > 3 && m.id.starts_with("um:"), "model.id")?;
        text(is_semver(&m.semver), "model.semver")?;
    }
    for s in &p.sources {
        text(is_token(&s.id), "sources.id")?;
    }
    for c in &p.contributors {
        text(is_token(&c.input_id), "contributors.input_id")?;
    }
    if let Some(n) = &p.nonlinearity {
        text(is_token(&n.version), "nonlinearity.version")?;
        opt_token(&n.reason, "nonlinearity.reason")?;
    }
    opt_token(&p.selection_rule, "selection_rule")?;
    opt_token(&p.estimate_convention, "estimate_convention")?;
    let pv = &p.provenance;
    for r in &pv.rule_sets {
        text(is_token(r), "provenance.rule_sets")?;
    }
    opt_token(&pv.principal, "provenance.principal")?;
    opt_token(&pv.adopted_by, "provenance.adopted_by")?;
    text(p.sources.len() <= MAX_ITEMS, "sources")?;
    text(p.contributors.len() <= MAX_ITEMS, "contributors")?;
    text(pv.rule_sets.len() <= MAX_ITEMS, "provenance.rule_sets")?;
    text(strictly_sorted(p.sources.iter().map(|s| s.id.as_str())), "sources.id")?;
    text(strictly_sorted(p.contributors.iter().map(|c| c.input_id.as_str())), "contributors.input_id")?;
    text(strictly_sorted(pv.rule_sets.iter().map(String::as_str)), "provenance.rule_sets")
}

fn mismatch(ok: bool, field: &'static str) -> Result<(), StmtError> {
    if ok {
        Ok(())
    } else {
        Err(StmtError::ClassMismatch { field })
    }
}

fn check_consistency(p: &StatementParts) -> Result<(), StmtError> {
    if let Some(m) = p.method {
        mismatch(!m.is_claimed() || p.class == EvidenceClass::Uec1, "method")?;
        mismatch(
            p.class != EvidenceClass::Uec3 || matches!(m, Method::Iso15530_3 | Method::Iso15530_4),
            "method",
        )?;
    }
    if matches!(p.class, EvidenceClass::Uec2 | EvidenceClass::Uec3) {
        mismatch(!p.sources.is_empty() && p.sources.iter().all(|s| s.kind.is_characterized()), "sources")?;
    }
    let pv = &p.provenance;
    mismatch(pv.principal.is_some() == pv.principal_kind.is_some(), "provenance.principal_kind")?;
    let needs_proposal = pv.principal_kind == Some(PrincipalKind::Agent) || pv.adopted_by.is_some();
    mismatch(!needs_proposal || pv.proposal, "provenance.proposal")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_predicates_work_on_bits() {
        assert!(!finite(f64::NAN) && !finite(f64::INFINITY) && !finite(-0.0));
        assert!(finite(0.0) && finite(-1.0));
        assert!(non_negative(0.0) && !non_negative(-1e-300));
        assert!(!positive(0.0) && positive(f64::from_bits(1)));
    }

    #[test]
    fn tokens_exclude_escapes_and_space() {
        assert!(is_token("um:a"));
        for bad in ["", "a b", "a\"b", "a\\b", "\u{e9}", "a\nb"] {
            assert!(!is_token(bad), "{bad:?}");
        }
        assert!(!is_token(&"x".repeat(MAX_TEXT_BYTES + 1)));
    }

    #[test]
    fn semver_and_uri() {
        assert!(is_semver("0.10.3") && !is_semver("01.0.0") && !is_semver("1.0") && !is_semver("1.0.0-rc1"));
        assert!(is_uri("unit:mm") && is_uri("http://qudt.org/vocab/unit/MilliM"));
        assert!(!is_uri("mm") && !is_uri("1x:mm") && !is_uri("unit:"));
    }

    #[test]
    fn enum_wire_names_round_trip() {
        for m in Method::ALL {
            assert_eq!(Method::from_wire(m.wire()), Some(*m));
        }
        assert_eq!(Method::from_wire("lpu"), None);
    }
}
