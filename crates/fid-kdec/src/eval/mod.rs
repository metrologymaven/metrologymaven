//! Rule evaluation and conformance probability, algorithm [`ALGORITHM_ID`]
//! (`dec.rule.evaluate@1.0.0`). Spec node `UNC.DEC.EVAL`.
//!
//! [`evaluate`] turns (value, uncertainty statement, tolerance, rule) into one
//! [`DecisionRecord`]: a checked [`Decision`] (outcome, rule reference, evidence basis), the
//! conformance probability `p` with conservative bounds and the expanded uncertainty `U`
//! when they exist, and flags.
//!
//! # Monte Carlo samples (maintainer decision 2026-11-21)
//!
//! [`evaluate`] never uses samples: until `UNC.UEC.STMT` carries a `samples_digest`, Monte
//! Carlo samples count as unavailable, so a conformance-probability rule on an MCM
//! statement is `NOT_DECIDABLE(pdf_unavailable)`. [`evaluate_with_samples`] is the
//! digest-checked path for when the statement carries that field: it takes the encoded
//! `mcm-samples@1` bytes and the digest they must have (the statement's, once it exists)
//! and refuses any other bytes. Callers must not pass a digest computed from the bytes
//! themselves; `UNC.DEC.DEFAULT` must not call it before the statement field exists.
//!
//! # Fail closed
//!
//! Invalid input is an error, never an outcome; an outcome that cannot be justified is
//! `NOT_DECIDABLE` with its reason or `INDETERMINATE`, never `CONFORMS`. Every comparison
//! of a value with a limit is exact ([`exact`] for sums of products; `fid_num::kdec::cmp`
//! for a single value); no decimal limit is converted to binary64 to compare it.
//!
//! # Order (pinned; the first applicable step decides)
//!
//! Errors ([`EvalError`]):
//! 1. `invalid_number{value}`: the value is NaN or infinite.
//! 2. `invalid_tolerance`: no limit, or `LSL >= USL` (a zero-width tolerance is refused
//!    too: `U/T` is undefined on it).
//! 3. `invalid_parameter{warn_ratio}`: a negative capability threshold.
//! 4. `unit_mismatch`: the statement has a unit and the tolerance's differs.
//! 5. The evidence basis of the statement (`EvidenceBasis::of`, `invalid_number{field}`).
//! 6. `invalid_rule{r}`: the unbound guard-band template `dr:guard-band@v` (only a resolved
//!    instance names its guard band).
//! 7. Samples ([`evaluate_with_samples`] only): `samples_unbound` (the SHA-256 of the
//!    encoded bytes is not the expected digest, the statement's method is not `MCM`, or the
//!    sample set's model digest is not the statement's), `samples_invalid{decode}` (the bytes
//!    are not a valid `mcm-samples@1` set), `samples_output` (the named output is not in the
//!    set).
//!
//! Outcomes:
//! 8. A `label_only` rule: `NOT_DECIDABLE(label_only)` (REQ-UNC.DEC.EVAL-009).
//! 9. A UEC-0 statement: `NOT_DECIDABLE(u_missing)` under every other rule kind
//!    (REQ-UNC.DEC.EVAL-002; under simple acceptance too, see the NODE.md deviation).
//! 10. `p` and its source: the fraction of samples inside the tolerance limits (each sample
//!     compared exactly), else the normal approximation `N(y, u_std)` when the rule's
//!     `pdf_source` is `mc_or_validated_normal`, `u_std` exists and either
//!     `lpu_validated_by_mc` is true or the statement was selected as LPU by
//!     [`LPU_SELECTION_RULE`] with a `linear_adequate` nonlinearity verdict; else none.
//! 11. The capability check `U / (USL - LSL) > warn_ratio`, exact; flag
//!     `capability_warning`, or `capability_unchecked` when `U` is absent or the tolerance
//!     is one-sided.
//! 12. By kind. `conformance_probability`: no `p` gives `NOT_DECIDABLE(pdf_unavailable)`;
//!     else, with conservative bounds `p_lo <= p <= p_hi`, `CONFORMS` if `p_lo >= p_min`,
//!     `NONCONFORMS` if `p_hi <= 1 - p_min`, else `INDETERMINATE`.
//!     - Normal approximation: each limit is split exactly as `f + delta` (`f` its binary64
//!       floor bracket, `delta` bracketed by two adjacent binary64 values), every
//!       subtraction, addition and division of the `z` bounds is moved one ulp outward,
//!       each tail is widened by [`TAIL_ERR`] (the erfc bound) and `p_lo`, `p_hi` by
//!       [`ARITH_ERR`] (the rounding of `1 - a - b`). `p` itself uses `f + dlo`, so it is
//!       within the bounds; it is reported, never decided on.
//!     - Samples: `p` is reported over the successful trials (`failed_trials` when some
//!       failed), but the decision uses the exact bounds `k / T` and `(k + F) / T`, so an
//!       unknown trial never helps either side. With fewer than [`MIN_TRIALS`] trials, or
//!       fewer than `20 / (1 - p_min)` [unverified], the outcome is `INDETERMINATE` with flag
//!       `samples_too_few` (the `samples_invalid{too_few}` case).
//!     Other kinds. `simple_acceptance`: `LSL <= y <= USL` exactly. `guard_band`:
//!     needs `U` (else `uncertainty_missing{expanded.U}`); `w = r U`; two-sided with
//!     `USL - LSL - 2w <= 0`: `NONCONFORMS` with `empty_acceptance_zone`; else `CONFORMS`
//!     iff `LSL + w <= y <= USL - w`, exactly.
//!
//! # Labels come from the statement (D-045, "claimed values never look measured")
//!
//! The flags `provisional_uncertainty` (UEC-1, or any of the three below),
//! `claimed_uncertainty`, `declared_input` and `uncharacterized_input` are copied from the
//! evidence basis, which only `EvidenceBasis::of` builds from the stored statement. The
//! caller cannot set or clear them, and they stay on `CONFORMS`.

mod exact;
pub mod normal;

use core::cmp::Ordering;
use core::fmt;

use fid_kunc::prop::mcm::samples::{decode, SampleSet, DEFAULT_CAP};
use fid_kunc::{EvidenceClass, Method, NlinVerdict, UncertaintyStatement};
use fid_num::kdec::{self, Decimal};
use fid_num::NumError;
use fid_types::Digest;

use crate::decision::{Decision, EvidenceBasis};
use crate::rule::{DecError, DecisionRule, NotDecidableReason, Outcome, PdfSource, RuleKind, RuleParams};
use exact::{sign_of_sum, ExactError, Factor};

/// Algorithm ID of [`evaluate`].
pub const ALGORITHM_ID: &str = "dec.rule.evaluate@1.0.0";
/// Replay equivalence class (`rep-1` field kind) of the outcome field: `exact`, so any
/// outcome difference in replay is `DIVERGENT` (REQ-UNC.DEC.EVAL-008). The spec card
/// `cards/dec.rule.evaluate@1.0.0.toml` declares the same.
pub const OUTCOME_REPLAY_KIND: &str = "exact";
/// Default capability threshold for `U / T` [unverified].
pub const DEFAULT_WARN_RATIO: &str = "0.25";
/// The selection rule whose LPU choice permits the normal approximation.
pub const LPU_SELECTION_RULE: &str = "unc-select@1";

/// A tolerance: exact decimal limits in one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tolerance {
    /// Lower specification limit.
    pub lsl: Option<Decimal>,
    /// Upper specification limit.
    pub usl: Option<Decimal>,
    /// Unit of the limits (and of the value), for example `si:m`.
    pub unit: String,
}

/// Encoded Monte Carlo samples, the digest they must have and the output column to use
/// (see [`evaluate_with_samples`]).
#[derive(Debug, Clone, Copy)]
pub struct SampleBytes<'a> {
    /// The encoded `mcm-samples@1` set.
    pub encoded: &'a [u8],
    /// The SHA-256 digest the bytes must have (the statement's `samples_digest`, once
    /// `UNC.UEC.STMT` carries it).
    pub expected_digest: &'a Digest,
    /// Output node ID of the column.
    pub output: &'a str,
}

/// Fewest Monte Carlo trials a decision may rest on [unverified]; a conformance-probability
/// rule also needs `T (1 - p_min) >= 20` [unverified].
pub const MIN_TRIALS: u64 = 100;
/// Absolute error bound used for each normal tail `P(Z > z)` [unverified beyond the
/// reference grid of `erfc.csv`, where the measured erfc error is at most 2e-15].
pub const TAIL_ERR: f64 = 4e-15;
/// Bound on the rounding of `1 - a - b - c` in the probability bounds.
pub const ARITH_ERR: f64 = 1e-15;

/// Everything an evaluation reads.
#[derive(Debug, Clone, Copy)]
pub struct EvalInput<'a> {
    /// The reported value `y`, in the tolerance unit.
    pub value: f64,
    /// The uncertainty statement of the value.
    pub statement: &'a UncertaintyStatement,
    /// The tolerance.
    pub tolerance: &'a Tolerance,
    /// The rule (validated by construction).
    pub rule: &'a DecisionRule,
    /// Capability threshold for `U / T` (see [`DEFAULT_WARN_RATIO`]).
    pub warn_ratio: Decimal,
}

/// Where `p` came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PSource {
    /// Fraction of Monte Carlo samples inside the limits.
    Samples,
    /// Normal approximation `N(y, u_std)`.
    NormalApproximation,
}

impl PSource {
    /// Wire name.
    pub const fn wire(self) -> &'static str {
        match self {
            PSource::Samples => "samples",
            PSource::NormalApproximation => "normal",
        }
    }
}

/// A flag of a decision record. Records keep them in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Flag {
    /// `U / T` exceeds the threshold.
    CapabilityWarning,
    /// `U / T` could not be checked (no `U`, or a one-sided tolerance).
    CapabilityUnchecked,
    /// The guard-banded acceptance zone is empty.
    EmptyAcceptanceZone,
    /// The sample set has failed trials.
    FailedTrials,
    /// Too few trials for a decision (`samples_invalid{too_few}`): the outcome is
    /// `INDETERMINATE`.
    SamplesTooFew,
    /// The uncertainty is UEC-1 or rests on a claim, a declared or an uncharacterized
    /// input: the decision is not a fully measured one and must be labelled.
    ProvisionalUncertainty,
    /// The uncertainty is claimed (`DECLARED`, `SPEC_DERIVED`).
    ClaimedUncertainty,
    /// The uncertainty rests on a declared input.
    DeclaredInput,
    /// The uncertainty rests on an uncharacterized input.
    UncharacterizedInput,
}

impl Flag {
    /// Every flag, in declaration order.
    pub const ALL: [Flag; 9] = [
        Flag::CapabilityWarning,
        Flag::CapabilityUnchecked,
        Flag::EmptyAcceptanceZone,
        Flag::FailedTrials,
        Flag::SamplesTooFew,
        Flag::ProvisionalUncertainty,
        Flag::ClaimedUncertainty,
        Flag::DeclaredInput,
        Flag::UncharacterizedInput,
    ];

    /// Wire name.
    pub const fn wire(self) -> &'static str {
        match self {
            Flag::CapabilityWarning => "capability_warning",
            Flag::CapabilityUnchecked => "capability_unchecked",
            Flag::EmptyAcceptanceZone => "empty_acceptance_zone",
            Flag::FailedTrials => "failed_trials",
            Flag::SamplesTooFew => "samples_too_few",
            Flag::ProvisionalUncertainty => "provisional_uncertainty",
            Flag::ClaimedUncertainty => "claimed_uncertainty",
            Flag::DeclaredInput => "declared_input",
            Flag::UncharacterizedInput => "uncharacterized_input",
        }
    }

    /// Parses a wire name.
    pub fn from_wire(s: &str) -> Option<Flag> {
        Flag::ALL.into_iter().find(|f| f.wire() == s)
    }
}

/// The result of an evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionRecord {
    decision: Decision,
    p_conformance: Option<f64>,
    p_bounds: Option<(f64, f64)>,
    p_source: Option<PSource>,
    expanded_u: Option<f64>,
    flags: Vec<Flag>,
}

impl DecisionRecord {
    /// The checked decision (outcome, rule reference, evidence basis).
    pub fn decision(&self) -> &Decision {
        &self.decision
    }
    /// The outcome.
    pub fn outcome(&self) -> Outcome {
        self.decision.outcome()
    }
    /// The conformance probability (over the successful trials when samples were used).
    pub fn p_conformance(&self) -> Option<f64> {
        self.p_conformance
    }
    /// Conservative bounds `(p_lo, p_hi)` on the conformance probability; the outcome of a
    /// conformance-probability rule is decided from them. For samples they are `k / T` and
    /// `(k + F) / T`, rounded outward (the decision itself uses the exact rationals).
    pub fn p_bounds(&self) -> Option<(f64, f64)> {
        self.p_bounds
    }
    /// Where `p` came from.
    pub fn p_source(&self) -> Option<PSource> {
        self.p_source
    }
    /// The expanded uncertainty `U` of the statement.
    pub fn expanded_u(&self) -> Option<f64> {
        self.expanded_u
    }
    /// The flags, in declaration order, without duplicates.
    pub fn flags(&self) -> &[Flag] {
        &self.flags
    }
    /// True if the record carries `flag`.
    pub fn has_flag(&self, flag: Flag) -> bool {
        self.flags.contains(&flag)
    }
    /// True when surfaces must label the decision as resting on a provisional uncertainty.
    pub fn is_provisional(&self) -> bool {
        self.has_flag(Flag::ProvisionalUncertainty)
    }
}

/// Why an evaluation was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalError {
    /// A number is NaN or infinite; `field` names it.
    InvalidNumber { field: &'static str },
    /// No limit, or `LSL >= USL`.
    InvalidTolerance,
    /// An evaluation parameter is invalid; `param` names it.
    InvalidParameter { param: &'static str },
    /// The tolerance unit differs from the statement unit.
    UnitMismatch,
    /// The rule needs a number the statement does not carry; `field` names it.
    UncertaintyMissing { field: &'static str },
    /// The samples do not belong to the statement.
    SamplesUnbound,
    /// The named output is not in the sample set.
    SamplesOutput,
    /// The sample set is inconsistent; `detail` names the check.
    SamplesInvalid { detail: &'static str },
    /// A rule or decision error from the rule types.
    Rule(DecError),
    /// A numeric error from `fid-num`.
    Numeric(NumError),
    /// An exact comparison exceeded its bounds (unreachable for valid inputs).
    Capacity,
}

impl EvalError {
    /// Stable error code.
    pub const fn code(self) -> &'static str {
        match self {
            EvalError::InvalidNumber { .. } => "invalid_number",
            EvalError::InvalidTolerance => "invalid_tolerance",
            EvalError::InvalidParameter { .. } => "invalid_parameter",
            EvalError::UnitMismatch => "unit_mismatch",
            EvalError::UncertaintyMissing { .. } => "uncertainty_missing",
            EvalError::SamplesUnbound => "samples_unbound",
            EvalError::SamplesOutput => "samples_output",
            EvalError::SamplesInvalid { .. } => "samples_invalid",
            EvalError::Rule(e) => e.code(),
            EvalError::Numeric(_) => "numeric",
            EvalError::Capacity => "capacity",
        }
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::InvalidNumber { field: x }
            | EvalError::InvalidParameter { param: x }
            | EvalError::UncertaintyMissing { field: x }
            | EvalError::SamplesInvalid { detail: x } => write!(f, "{}: {x}", self.code()),
            EvalError::Rule(e) => write!(f, "{e}"),
            EvalError::Numeric(e) => write!(f, "numeric: {e}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for EvalError {}

impl From<DecError> for EvalError {
    fn from(e: DecError) -> EvalError {
        EvalError::Rule(e)
    }
}

impl From<NumError> for EvalError {
    fn from(e: NumError) -> EvalError {
        EvalError::Numeric(e)
    }
}

impl From<ExactError> for EvalError {
    fn from(e: ExactError) -> EvalError {
        match e {
            ExactError::NonFinite => EvalError::InvalidNumber { field: "exact" },
            ExactError::Capacity => EvalError::Capacity,
        }
    }
}

/// `sign(sum)` as a non-negative test.
fn nonneg(terms: &[(bool, &[Factor])]) -> Result<bool, EvalError> {
    Ok(sign_of_sum(terms)? != Ordering::Less)
}

fn positive(terms: &[(bool, &[Factor])]) -> Result<bool, EvalError> {
    Ok(sign_of_sum(terms)? == Ordering::Greater)
}

/// The largest finite `f64` `<= d`, if any.
fn floor_f64(d: &Decimal) -> Result<Option<f64>, EvalError> {
    match d.to_f64() {
        Ok(x) => {
            if kdec::cmp(x, d)? == Ordering::Greater {
                let y = x.next_down();
                Ok(if y.is_finite() { Some(y) } else { None })
            } else {
                Ok(Some(x))
            }
        }
        Err(NumError::Infinite { .. }) => Ok(if d.is_negative() { None } else { Some(f64::MAX) }),
        Err(e) => Err(e.into()),
    }
}

/// The smallest finite `f64` `>= d`, if any.
fn ceil_f64(d: &Decimal) -> Result<Option<f64>, EvalError> {
    match d.to_f64() {
        Ok(x) => {
            if kdec::cmp(x, d)? == Ordering::Less {
                let y = x.next_up();
                Ok(if y.is_finite() { Some(y) } else { None })
            } else {
                Ok(Some(x))
            }
        }
        Err(NumError::Infinite { .. }) => Ok(if d.is_negative() { Some(-f64::MAX) } else { None }),
        Err(e) => Err(e.into()),
    }
}

/// The tolerance as binary64 brackets, so that for every finite `x`:
/// `x >= LSL` iff `x >= lo` and `x <= USL` iff `x <= hi` (exact, no rounding).
struct Brackets {
    /// `None`: no lower limit. `Some(None)`: no finite value is `>= LSL`.
    lo: Option<Option<f64>>,
    hi: Option<Option<f64>>,
}

impl Brackets {
    fn new(t: &Tolerance) -> Result<Brackets, EvalError> {
        let lo = match &t.lsl {
            Some(l) => Some(ceil_f64(l)?),
            None => None,
        };
        let hi = match &t.usl {
            Some(u) => Some(floor_f64(u)?),
            None => None,
        };
        Ok(Brackets { lo, hi })
    }

    fn inside(&self, x: f64) -> bool {
        let lo_ok = match self.lo {
            None => true,
            Some(None) => false,
            Some(Some(lo)) => x >= lo,
        };
        let hi_ok = match self.hi {
            None => true,
            Some(None) => false,
            Some(Some(hi)) => x <= hi,
        };
        lo_ok && hi_ok
    }
}

/// `LSL <= y <= USL`, exactly.
fn within(y: f64, t: &Tolerance) -> Result<bool, EvalError> {
    if let Some(l) = &t.lsl {
        if kdec::cmp(y, l)? == Ordering::Less {
            return Ok(false);
        }
    }
    if let Some(u) = &t.usl {
        if kdec::cmp(y, u)? == Ordering::Greater {
            return Ok(false);
        }
    }
    Ok(true)
}

fn check_tolerance(t: &Tolerance) -> Result<(), EvalError> {
    match (&t.lsl, &t.usl) {
        (None, None) => Err(EvalError::InvalidTolerance),
        (Some(l), Some(u)) if kdec::cmp_dec(l, u) != Ordering::Less => Err(EvalError::InvalidTolerance),
        _ => Ok(()),
    }
}

/// Sample counts: `k` inside, `n = T - F` kept, `failed = F`, `trials = T`.
struct Counts {
    inside: u64,
    kept: u64,
    failed: u64,
    trials: u64,
}

fn count_samples(s: &SampleBytes<'_>, stmt: &UncertaintyStatement, t: &Tolerance) -> Result<Counts, EvalError> {
    if fid_kci::kci::sha256_digest(s.encoded) != *s.expected_digest {
        return Err(EvalError::SamplesUnbound);
    }
    if stmt.method() != Some(Method::Mcm) {
        return Err(EvalError::SamplesUnbound);
    }
    let set: SampleSet = decode(s.encoded, DEFAULT_CAP).map_err(|_| EvalError::SamplesInvalid { detail: "decode" })?;
    let h = &set.header;
    if stmt.model().map(|m| m.digest) != Some(h.model_digest) {
        return Err(EvalError::SamplesUnbound);
    }
    let col = h.outputs.iter().position(|o| o == s.output).ok_or(EvalError::SamplesOutput)?;
    // `decode` has checked the counts, the column lengths and that every value is finite.
    let kept = h.trials.checked_sub(h.failed).ok_or(EvalError::SamplesInvalid { detail: "counts" })?;
    let values = set.outputs.get(col).ok_or(EvalError::SamplesOutput)?;
    if u64::try_from(values.len()).ok() != Some(kept) {
        return Err(EvalError::SamplesInvalid { detail: "length" });
    }
    let b = Brackets::new(t)?;
    let mut inside = 0u64;
    for &x in values {
        if !x.is_finite() {
            return Err(EvalError::SamplesInvalid { detail: "non_finite" });
        }
        if b.inside(x) {
            inside += 1;
        }
    }
    Ok(Counts { inside, kept, failed: h.failed, trials: h.trials })
}

/// `T >= MIN_TRIALS` and, under a conformance-probability rule, `T (1 - p_min) >= 20`.
fn enough_trials(trials: u64, p_min: Option<Decimal>) -> Result<bool, EvalError> {
    if trials < MIN_TRIALS {
        return Ok(false);
    }
    match p_min {
        // T - p_min T - 20 >= 0
        Some(pm) => nonneg(&[
            (false, &[Factor::I(trials)]),
            (true, &[Factor::D(pm), Factor::I(trials)]),
            (true, &[Factor::I(20)]),
        ]),
        None => Ok(true),
    }
}

fn normal_permitted(rule: &DecisionRule, stmt: &UncertaintyStatement) -> bool {
    if rule.pdf_source() != PdfSource::McOrValidatedNormal || stmt.u_std().is_none() {
        return false;
    }
    if stmt.lpu_validated_by_mc() == Some(true) {
        return true;
    }
    stmt.method() == Some(Method::Lpu)
        && stmt.selection_rule() == Some(LPU_SELECTION_RULE)
        && stmt.nonlinearity().is_some_and(|n| n.verdict == NlinVerdict::LinearAdequate)
}

/// `x` moved one ulp down (an infinity stays put, `+inf` becomes `f64::MAX`).
fn down(x: f64) -> f64 {
    x.next_down()
}

/// `x` moved one ulp up.
fn up(x: f64) -> f64 {
    x.next_up()
}

fn clamp01(x: f64) -> Result<f64, EvalError> {
    Ok(fid_num::f::total_clamp(x, 0.0, 1.0)?)
}

/// A decimal limit `L` as `f + delta`: `f` the largest finite `f64 <= L`, and
/// `dlo <= delta <= dhi` adjacent (or equal) non-negative `f64` values, found by bisection on
/// the bit patterns with exact comparisons. `None` when no finite `f64` is `<= L`.
struct Split {
    f: f64,
    dlo: f64,
    dhi: f64,
}

fn split(l: &Decimal) -> Result<Option<Split>, EvalError> {
    let Some(f) = floor_f64(l)? else { return Ok(None) };
    // sign(L - f - t)
    let rest = |t: f64| sign_of_sum(&[(false, &[Factor::D(*l)]), (true, &[Factor::F(f)]), (true, &[Factor::F(t)])]);
    if rest(f64::MAX)? != Ordering::Less {
        return Ok(Some(Split { f, dlo: f64::MAX, dhi: f64::INFINITY }));
    }
    // Invariant: rest(lo) >= 0 (t = 0, since f <= L), rest(hi) < 0.
    let (mut lo, mut hi) = (0u64, f64::MAX.to_bits());
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if rest(f64::from_bits(mid))? == Ordering::Less {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let dlo = f64::from_bits(lo);
    let dhi = if rest(dlo)? == Ordering::Equal { dlo } else { f64::from_bits(hi) };
    Ok(Some(Split { f, dlo, dhi }))
}

/// Normal approximation `P(LSL <= Y <= USL)` for `Y ~ N(y, u)`: the nominal value and
/// conservative bounds `(p, p_lo, p_hi)`.
///
/// Each limit is `f + delta` ([`split`]); `(L - y) / u` is bounded by binary64 operations
/// each moved one ulp outward, so the bounds hold whatever the rounding of the
/// subtraction, the addition of `delta` and the division. Each tail is widened by
/// [`TAIL_ERR`] and the result by [`ARITH_ERR`].
fn normal_p(y: f64, u: f64, t: &Tolerance) -> Result<(f64, f64, f64), EvalError> {
    let tail = |z: f64| -> Result<f64, EvalError> { Ok(normal::upper_tail(z)?) };
    // (nominal, max, min) of P(Y > USL) = P(Z > (USL - y) / u).
    let (above, above_max, above_min) = match &t.usl {
        None => (0.0, 0.0, 0.0),
        Some(us) => match split(us)? {
            // USL below every finite value: P(Y > USL) = 1.
            None => (1.0, 1.0, 1.0),
            Some(sp) => {
                let a = sp.f - y;
                let z = (a + sp.dlo) / u;
                let z_lo = down(down(down(a) + sp.dlo) / u);
                let z_hi = up(up(up(a) + sp.dhi) / u);
                (tail(z)?, tail(z_lo)? + TAIL_ERR, tail(z_hi)? - TAIL_ERR)
            }
        },
    };
    // P(Y < LSL) = P(Z > (y - LSL) / u).
    let (below, below_max, below_min) = match &t.lsl {
        None => (0.0, 0.0, 0.0),
        Some(ls) => match split(ls)? {
            // LSL below every finite value: P(Y < LSL) = 0.
            None => (0.0, 0.0, 0.0),
            Some(sp) => {
                let a = y - sp.f;
                let z = (a - sp.dlo) / u;
                let z_lo = down(down(down(a) - sp.dhi) / u);
                let z_hi = up(up(up(a) - sp.dlo) / u);
                (tail(z)?, tail(z_lo)? + TAIL_ERR, tail(z_hi)? - TAIL_ERR)
            }
        },
    };
    let p = clamp01(1.0 - above - below)?;
    let p_lo = clamp01(1.0 - above_max - below_max - ARITH_ERR)?;
    let p_hi = clamp01(1.0 - above_min - below_min + ARITH_ERR)?;
    Ok((p, p_lo, p_hi))
}

/// `[x / n]` rounded outward by one ulp and clamped to [0, 1].
fn ratio_bounds(lo: u64, hi: u64, n: u64) -> Result<(f64, f64), EvalError> {
    let n = n as f64;
    Ok((clamp01((lo as f64 / n).next_down())?, clamp01((hi as f64 / n).next_up())?))
}

enum PBasis {
    Samples(Counts),
    /// Nominal, lower bound, upper bound.
    Normal(f64, f64, f64),
}

fn basis_flags(basis: &EvidenceBasis, flags: &mut Vec<Flag>) {
    let claimed = basis.is_claimed();
    let declared = basis.rests_on_declared_input();
    let unchar = basis.rests_on_uncharacterized_input();
    if basis.class() == EvidenceClass::Uec1 || claimed || declared || unchar {
        flags.push(Flag::ProvisionalUncertainty);
    }
    if claimed {
        flags.push(Flag::ClaimedUncertainty);
    }
    if declared {
        flags.push(Flag::DeclaredInput);
    }
    if unchar {
        flags.push(Flag::UncharacterizedInput);
    }
}

/// Evaluates a rule without Monte Carlo samples (see the module documentation for the
/// pinned order).
pub fn evaluate(input: &EvalInput<'_>) -> Result<DecisionRecord, EvalError> {
    run(input, None)
}

/// Evaluates a rule with encoded Monte Carlo samples whose SHA-256 must equal
/// `samples.expected_digest` (see the module documentation: not for values of record until
/// `UNC.UEC.STMT` carries `samples_digest`).
pub fn evaluate_with_samples(input: &EvalInput<'_>, samples: &SampleBytes<'_>) -> Result<DecisionRecord, EvalError> {
    run(input, Some(samples))
}

fn run(input: &EvalInput<'_>, samples: Option<&SampleBytes<'_>>) -> Result<DecisionRecord, EvalError> {
    let y = input.value;
    if !y.is_finite() {
        return Err(EvalError::InvalidNumber { field: "value" });
    }
    let tol = input.tolerance;
    check_tolerance(tol)?;
    if input.warn_ratio.is_negative() {
        return Err(EvalError::InvalidParameter { param: "warn_ratio" });
    }
    let stmt = input.statement;
    if stmt.unit().is_some_and(|u| u != tol.unit) {
        return Err(EvalError::UnitMismatch);
    }
    let basis = EvidenceBasis::of(stmt)?;
    let rule = input.rule;
    let r = match rule.params() {
        RuleParams::GuardBand { r: None } => return Err(DecError::InvalidRule { param: "r" }.into()),
        RuleParams::GuardBand { r: Some(r) } => Some(r.value()),
        _ => None,
    };
    let counts = match samples {
        Some(s) => Some(count_samples(s, stmt, tol)?),
        None => None,
    };

    let mut flags = Vec::new();
    basis_flags(&basis, &mut flags);
    if rule.kind() == RuleKind::LabelOnly {
        return undecided(rule, NotDecidableReason::LabelOnly, flags, basis);
    }
    if basis.class() == EvidenceClass::Uec0 {
        return undecided(rule, NotDecidableReason::UMissing, flags, basis);
    }

    let pb = match counts {
        Some(c) => Some(PBasis::Samples(c)),
        None => match stmt.u_std() {
            Some(u) if normal_permitted(rule, stmt) => {
                let (p, lo, hi) = normal_p(y, u, tol)?;
                Some(PBasis::Normal(p, lo, hi))
            }
            _ => None,
        },
    };
    let p_min = match rule.params() {
        RuleParams::ConformanceProbability { p_min } => Some(p_min.value()),
        _ => None,
    };
    let mut too_few = false;
    let (p, p_bounds, p_source) = match &pb {
        Some(PBasis::Samples(c)) => {
            if c.failed > 0 {
                flags.push(Flag::FailedTrials);
            }
            if !enough_trials(c.trials, p_min)? {
                too_few = true;
                flags.push(Flag::SamplesTooFew);
            }
            // Exact integers below 2^53 in practice; one rounding of the quotient.
            let p = if c.kept > 0 { Some(c.inside as f64 / c.kept as f64) } else { None };
            (p, Some(ratio_bounds(c.inside, c.inside + c.failed, c.trials)?), Some(PSource::Samples))
        }
        Some(PBasis::Normal(p, lo, hi)) => (Some(*p), Some((*lo, *hi)), Some(PSource::NormalApproximation)),
        None => (None, None, None),
    };

    let u_exp = stmt.expanded().map(|e| e.value);
    match (u_exp, &tol.lsl, &tol.usl) {
        (Some(u), Some(l), Some(h)) => {
            let w = input.warn_ratio;
            // U - warn * USL + warn * LSL > 0
            if positive(&[
                (false, &[Factor::F(u)]),
                (true, &[Factor::D(w), Factor::D(*h)]),
                (false, &[Factor::D(w), Factor::D(*l)]),
            ])? {
                flags.push(Flag::CapabilityWarning);
            }
        }
        _ => flags.push(Flag::CapabilityUnchecked),
    }

    let outcome = match rule.params() {
        RuleParams::ConformanceProbability { p_min } => {
            let pm = p_min.value();
            match &pb {
                None => Outcome::NotDecidable(NotDecidableReason::PdfUnavailable),
                Some(PBasis::Samples(_)) if too_few => Outcome::Indeterminate,
                Some(PBasis::Samples(c)) => {
                    let t = c.trials;
                    let hi = c.inside + c.failed;
                    // k - p_min T >= 0
                    let conforms = nonneg(&[(false, &[Factor::I(c.inside)]), (true, &[Factor::D(pm), Factor::I(t)])])?;
                    // T - p_min T - (k + F) >= 0
                    let nonconforms = nonneg(&[
                        (false, &[Factor::I(t)]),
                        (true, &[Factor::D(pm), Factor::I(t)]),
                        (true, &[Factor::I(hi)]),
                    ])?;
                    pick(conforms, nonconforms)
                }
                Some(PBasis::Normal(_, lo, hi)) => {
                    let conforms = kdec::cmp(*lo, &pm)? != Ordering::Less;
                    // 1 - p_min - p_hi >= 0
                    let nonconforms =
                        nonneg(&[(false, &[Factor::I(1)]), (true, &[Factor::D(pm)]), (true, &[Factor::F(*hi)])])?;
                    pick(conforms, nonconforms)
                }
            }
        }
        RuleParams::SimpleAcceptance => {
            if within(y, tol)? {
                Outcome::Conforms
            } else {
                Outcome::Nonconforms
            }
        }
        RuleParams::GuardBand { .. } => {
            let r = r.ok_or(DecError::InvalidRule { param: "r" })?;
            let u = u_exp.ok_or(EvalError::UncertaintyMissing { field: "expanded.U" })?;
            guard_band(y, u, r, tol, &mut flags)?
        }
        RuleParams::LabelOnly => Outcome::NotDecidable(NotDecidableReason::LabelOnly),
    };
    flags.sort();
    flags.dedup();
    let decision = Decision::new(Some(rule), outcome, Some(basis))?;
    Ok(DecisionRecord { decision, p_conformance: p, p_bounds, p_source, expanded_u: u_exp, flags })
}

fn undecided(
    rule: &DecisionRule,
    reason: NotDecidableReason,
    flags: Vec<Flag>,
    basis: EvidenceBasis,
) -> Result<DecisionRecord, EvalError> {
    let decision = Decision::new(Some(rule), Outcome::NotDecidable(reason), Some(basis))?;
    Ok(DecisionRecord { decision, p_conformance: None, p_bounds: None, p_source: None, expanded_u: None, flags })
}

fn pick(conforms: bool, nonconforms: bool) -> Outcome {
    if conforms {
        Outcome::Conforms
    } else if nonconforms {
        Outcome::Nonconforms
    } else {
        Outcome::Indeterminate
    }
}

fn guard_band(y: f64, u: f64, r: Decimal, tol: &Tolerance, flags: &mut Vec<Flag>) -> Result<Outcome, EvalError> {
    let w = [Factor::D(r), Factor::F(u)];
    if let (Some(l), Some(h)) = (&tol.lsl, &tol.usl) {
        // USL - LSL - 2 r U <= 0: the acceptance zone is empty (or a single point).
        let open = positive(&[
            (false, &[Factor::D(*h)]),
            (true, &[Factor::D(*l)]),
            (true, &[Factor::I(2), Factor::D(r), Factor::F(u)]),
        ])?;
        if !open {
            flags.push(Flag::EmptyAcceptanceZone);
            return Ok(Outcome::Nonconforms);
        }
    }
    if let Some(l) = &tol.lsl {
        // y - LSL - r U >= 0
        if !nonneg(&[(false, &[Factor::F(y)]), (true, &[Factor::D(*l)]), (true, &w)])? {
            return Ok(Outcome::Nonconforms);
        }
    }
    if let Some(h) = &tol.usl {
        // USL - r U - y >= 0
        if !nonneg(&[(false, &[Factor::D(*h)]), (true, &w), (true, &[Factor::F(y)])])? {
            return Ok(Outcome::Nonconforms);
        }
    }
    Ok(Outcome::Conforms)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::parse(s).unwrap()
    }

    #[test]
    fn brackets_are_exact() {
        for s in ["0.1", "10.05", "-10.05", "0", "1e-400", "-1e-400", "1e400", "-1e400", "0.5", "1e-320"] {
            let l = d(s);
            let lo = ceil_f64(&l).unwrap();
            let hi = floor_f64(&l).unwrap();
            if let Some(lo) = lo {
                assert_ne!(kdec::cmp(lo, &l).unwrap(), Ordering::Less, "{s}");
                let below = lo.next_down();
                if below.is_finite() {
                    assert_eq!(kdec::cmp(below, &l).unwrap(), Ordering::Less, "{s}");
                }
            }
            if let Some(hi) = hi {
                assert_ne!(kdec::cmp(hi, &l).unwrap(), Ordering::Greater, "{s}");
                let above = hi.next_up();
                if above.is_finite() {
                    assert_eq!(kdec::cmp(above, &l).unwrap(), Ordering::Greater, "{s}");
                }
            }
        }
        assert_eq!(floor_f64(&d("-1e400")).unwrap(), None);
        assert_eq!(ceil_f64(&d("1e400")).unwrap(), None);
        assert_eq!(floor_f64(&d("1e400")).unwrap(), Some(f64::MAX));
    }

    #[test]
    fn split_brackets_the_remainder_exactly() {
        for s in ["1.7985560436", "0.01005", "0.5", "0", "-0.00999", "1e-400", "-1e-400", "1e400", "123456.789"] {
            let l = d(s);
            let sp = split(&l).unwrap().unwrap();
            let lo = sign_of_sum(&[(false, &[Factor::D(l)]), (true, &[Factor::F(sp.f)]), (true, &[Factor::F(sp.dlo)])]);
            assert_ne!(lo.unwrap(), Ordering::Less, "{s}");
            if sp.dhi.is_finite() {
                let hi =
                    sign_of_sum(&[(false, &[Factor::D(l)]), (true, &[Factor::F(sp.f)]), (true, &[Factor::F(sp.dhi)])]);
                assert_ne!(hi.unwrap(), Ordering::Greater, "{s}");
                assert!(sp.dhi == sp.dlo || sp.dhi == sp.dlo.next_up(), "{s}");
            }
        }
        assert!(split(&d("-1e400")).unwrap().is_none());
    }

    #[test]
    fn flag_wire_round_trips() {
        for f in Flag::ALL {
            assert_eq!(Flag::from_wire(f.wire()), Some(f));
        }
        let mut sorted = Flag::ALL;
        sorted.sort();
        assert_eq!(sorted, Flag::ALL);
    }
}
