//! Specific and global conformity risk (spec node `UNC.DEC.RISK`, algorithm
//! `dec.risk.compute@1.0.0`).
//!
//! Pure Rust, KNP-1, no I/O, no `unsafe`, no panics, no `f64::min`/`f64::max`, no FMA,
//! no `HashMap`/`HashSet`. Transcendentals go exclusively through `fid-num` (`exp` and
//! `eval::normal::upper_tail`), and quadrature sums go through `fid_num::red::sum` in
//! fixed left-to-right order so outputs are bit-identical on x86-64, ARM64 and wasm32.

use core::fmt;

use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::sha256_digest;
use fid_num::NumError;
use fid_types::Digest;

use crate::eval::normal::upper_tail;
use crate::eval::{DecisionRecord, Tolerance};
use crate::rule::{DecisionRule, Outcome, RiskReporting, RuleParams};

/// Algorithm identifier (CONSTITUTION §4, spec card `dec.risk.compute@1.0.0.toml`).
pub const ALGORITHM_ID: &str = "dec.risk.compute@1.0.0";

/// Maximum byte length of a declared prior's untrusted source string.
pub const MAX_PRIOR_SOURCE_BYTES: usize = 512;

/// `1 / sqrt(2 * pi)`, exact binary64 nearest (`0x3fd9884533d43651`).
const FRAC_1_SQRT_2PI: f64 = 0.398_942_280_401_432_7;

/// Number of equal sub-panels per sub-interval in the composite Gauss-Legendre rule.
const SUB_PANELS: usize = 4;

/// Fixed iteration count for bisection when deriving a normal conformance-probability
/// acceptance zone (exceeds 53 bits of binary64 significand; no data-dependent loop).
const BISECT_ITERS: usize = 64;

/// 16-point Gauss-Legendre nodes on `[-1, 1]` (computed at 65 decimal digits in
/// `fixtures/unc/dec/risk/make_cases.py --gl16`).
const GL16_NODES: [f64; 16] = [
    -0.9894009349916499,  // 0xbfefa92c264d787e
    -0.9445750230732326,  // 0xbfee39f56616f9b0
    -0.8656312023878318,  // 0xbfebb3403514e483
    -0.755404408355003,   // 0xbfe82c45dda4726b
    -0.6178762444026438,  // 0xbfe3c5a466d5e8b8
    -0.45801677765722737, // 0xbfdd50259a43a772
    -0.2816035507792589,  // 0xbfd205cae642337c
    -0.09501250983763744, // 0xbfb852bd6676a9f9
    0.09501250983763744,  // 0x3fb852bd6676a9f9
    0.2816035507792589,   // 0x3fd205cae642337c
    0.45801677765722737,  // 0x3fdd50259a43a772
    0.6178762444026438,   // 0x3fe3c5a466d5e8b8
    0.755404408355003,    // 0x3fe82c45dda4726b
    0.8656312023878318,   // 0x3febb3403514e483
    0.9445750230732326,   // 0x3fee39f56616f9b0
    0.9894009349916499,   // 0x3fefa92c264d787e
];

/// 16-point Gauss-Legendre weights on `[-1, 1]`.
const GL16_WEIGHTS: [f64; 16] = [
    0.027152459411754096, // 0x3f9bcddab4b7c211
    0.062253523938647894, // 0x3fafdfb1a2c1265d
    0.09515851168249279,  // 0x3fb85c4ee79cc258
    0.12462897125553388,  // 0x3fbfe7af2bad386a
    0.14959598881657674,  // 0x3fc325f61bca3cbf
    0.16915651939500254,  // 0x3fc5a6ebbb5a75fc
    0.18260341504492358,  // 0x3fc75f8c77e0c00f
    0.1894506104550685,   // 0x3fc83feae80e4dfc
    0.1894506104550685,   // 0x3fc83feae80e4dfc
    0.18260341504492358,  // 0x3fc75f8c77e0c00f
    0.16915651939500254,  // 0x3fc5a6ebbb5a75fc
    0.14959598881657674,  // 0x3fc325f61bca3cbf
    0.12462897125553388,  // 0x3fbfe7af2bad386a
    0.09515851168249279,  // 0x3fb85c4ee79cc258
    0.062253523938647894, // 0x3fafdfb1a2c1265d
    0.027152459411754096, // 0x3f9bcddab4b7c211
];

/// Untrusted free-text string carried from a declared prior source (CONSTITUTION C6).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UntrustedString(String);

impl UntrustedString {
    /// Wraps a prior source string; rejects empty/whitespace-only strings or strings
    /// exceeding [`MAX_PRIOR_SOURCE_BYTES`].
    pub fn new(raw: impl Into<String>) -> Result<Self, RiskError> {
        let s = raw.into();
        if s.trim().is_empty() {
            return Err(RiskError::InvalidPrior { reason: "empty_source" });
        }
        if s.len() > MAX_PRIOR_SOURCE_BYTES {
            return Err(RiskError::InvalidPrior { reason: "source_too_long" });
        }
        Ok(Self(s))
    }

    /// Raw string as supplied (never render unescaped to terminal or UI).
    pub fn as_raw(&self) -> &str {
        &self.0
    }

    /// Safe display form with ASCII control characters escaped as `\u00XX`.
    pub fn as_escaped(&self) -> String {
        let mut out = String::with_capacity(self.0.len());
        for ch in self.0.chars() {
            if ch.is_control() {
                let code = ch as u32;
                let _ = fmt::Write::write_fmt(&mut out, format_args!("\\u{code:04x}"));
            } else {
                out.push(ch);
            }
        }
        out
    }
}

/// Declared normal process prior `N(mu, sigma^2)` with its source (REQ-UNC.DEC.RISK-002, -005).
#[derive(Debug, Clone, PartialEq)]
pub struct Prior {
    /// Process mean `mu_0` in the tolerance unit.
    pub mu: f64,
    /// Process standard deviation `sigma_0 > 0` in the tolerance unit.
    pub sigma: f64,
    /// Declared provenance of the prior (untrusted text).
    pub source: UntrustedString,
}

impl Prior {
    /// Validates and constructs a declared normal process prior.
    pub fn new(mu: f64, sigma: f64, source: impl Into<String>) -> Result<Self, RiskError> {
        let source = UntrustedString::new(source)?;
        let p = Self { mu, sigma, source };
        p.validate()?;
        Ok(p)
    }

    /// Validates `mu`, `sigma`, and `source`.
    pub fn validate(&self) -> Result<(), RiskError> {
        if !self.mu.is_finite() {
            return Err(RiskError::InvalidPrior { reason: "non_finite_mu" });
        }
        if !self.sigma.is_finite() {
            return Err(RiskError::InvalidPrior { reason: "non_finite_sigma" });
        }
        if self.sigma <= 0.0 {
            return Err(RiskError::InvalidPrior { reason: "non_positive_sigma" });
        }
        if self.source.as_raw().trim().is_empty() {
            return Err(RiskError::InvalidPrior { reason: "empty_source" });
        }
        if self.source.as_raw().len() > MAX_PRIOR_SOURCE_BYTES {
            return Err(RiskError::InvalidPrior { reason: "source_too_long" });
        }
        Ok(())
    }
}

/// Flags attached to [`RiskFigures`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RiskFlag {
    /// Global risk was requested by the rule, but no process prior was declared
    /// (REQ-UNC.DEC.RISK-003).
    PriorMissing,
    /// The rule's acceptance zone is empty (`lower >= upper`), so consumer risk is 0.
    EmptyAcceptanceZone,
}

impl RiskFlag {
    /// Stable wire tag.
    pub const fn wire(self) -> &'static str {
        match self {
            RiskFlag::PriorMissing => "prior_missing",
            RiskFlag::EmptyAcceptanceZone => "empty_acceptance_zone",
        }
    }
}

impl fmt::Display for RiskFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire())
    }
}

/// Binary64 tolerance interval (specification zone) `[lsl, usl]`, one- or two-sided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToleranceF64 {
    /// Lower specification limit, if present.
    pub lsl: Option<f64>,
    /// Upper specification limit, if present.
    pub usl: Option<f64>,
}

impl ToleranceF64 {
    /// Validates and constructs a tolerance interval.
    pub fn new(lsl: Option<f64>, usl: Option<f64>) -> Result<Self, RiskError> {
        let t = Self { lsl, usl };
        t.validate()?;
        Ok(t)
    }

    /// Converts an exact [`Tolerance`] from `UNC.DEC.EVAL` into binary64 limits.
    pub fn from_tolerance(tol: &Tolerance) -> Result<Self, RiskError> {
        let lsl = match tol.lsl {
            Some(d) => Some(d.to_f64()?),
            None => None,
        };
        let usl = match tol.usl {
            Some(d) => Some(d.to_f64()?),
            None => None,
        };
        Self::new(lsl, usl)
    }

    /// Validates that at least one limit is present, all present limits are finite, and
    /// `lsl < usl` when both are present.
    pub fn validate(&self) -> Result<(), RiskError> {
        if self.lsl.is_none() && self.usl.is_none() {
            return Err(RiskError::InvalidTolerance { reason: "no_limits" });
        }
        if let Some(l) = self.lsl {
            if !l.is_finite() {
                return Err(RiskError::InvalidTolerance { reason: "non_finite_lsl" });
            }
        }
        if let Some(u) = self.usl {
            if !u.is_finite() {
                return Err(RiskError::InvalidTolerance { reason: "non_finite_usl" });
            }
        }
        if let (Some(l), Some(u)) = (self.lsl, self.usl) {
            if l >= u {
                return Err(RiskError::InvalidTolerance { reason: "inverted_limits" });
            }
        }
        Ok(())
    }
}

/// Acceptance zone `[lower, upper]` defined by a decision rule (empty if `lower >= upper`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcceptanceZone {
    /// Lower acceptance limit, if present.
    pub lower: Option<f64>,
    /// Upper acceptance limit, if present.
    pub upper: Option<f64>,
}

impl AcceptanceZone {
    /// Applies a uniform guard band `w` (positive = inward, negative = outward) to `tol`.
    pub fn from_guard_band(tol: ToleranceF64, w: f64) -> Result<Self, RiskError> {
        tol.validate()?;
        if !w.is_finite() {
            return Err(RiskError::InvalidTolerance { reason: "non_finite_guard_band" });
        }
        let lower = match tol.lsl {
            Some(l) => {
                let v = l + w;
                if !v.is_finite() {
                    return Err(RiskError::InvalidTolerance { reason: "overflow" });
                }
                Some(v)
            }
            None => None,
        };
        let upper = match tol.usl {
            Some(u) => {
                let v = u - w;
                if !v.is_finite() {
                    return Err(RiskError::InvalidTolerance { reason: "overflow" });
                }
                Some(v)
            }
            None => None,
        };
        Ok(Self { lower, upper })
    }

    /// Returns `true` when the acceptance zone contains no interior (`lower >= upper`).
    pub fn is_empty(&self) -> bool {
        matches!((self.lower, self.upper), (Some(l), Some(u)) if l >= u)
    }
}

/// Computed conformity risk figures returned by [`compute`].
#[derive(Debug, Clone, PartialEq)]
pub struct RiskFigures {
    /// Specific consumer risk `1 - p` (`CONFORMS` and `INDETERMINATE`).
    pub specific_consumer: Option<f64>,
    /// Specific producer risk `p` (`NONCONFORMS` and `INDETERMINATE`).
    pub specific_producer: Option<f64>,
    /// Global consumer risk `R_C` under the declared prior.
    pub global_consumer: Option<f64>,
    /// Global producer risk `R_P` under the declared prior.
    pub global_producer: Option<f64>,
    /// Recorded provenance string of the declared prior (REQ-UNC.DEC.RISK-005).
    pub prior_source: Option<UntrustedString>,
    /// Flags (such as [`RiskFlag::PriorMissing`]).
    pub flags: Vec<RiskFlag>,
}

impl RiskFigures {
    /// Returns `true` if `flag` is present.
    pub fn has_flag(&self, flag: RiskFlag) -> bool {
        self.flags.contains(&flag)
    }

    /// Encodes the risk figures as RFC 8785 canonical JSON with binary64 numbers as
    /// `"0x..."` IEEE-754 hex strings for cross-target bit-identity verification (AC-5).
    /// Absent optional fields are omitted (kernel JSON carries strings, arrays and objects).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, RiskError> {
        let mut fields = Vec::with_capacity(7);
        fields.push(("algorithm".to_owned(), Json::Str(ALGORITHM_ID.to_owned())));
        fields
            .push(("flags".to_owned(), Json::Arr(self.flags.iter().map(|f| Json::Str(f.wire().to_owned())).collect())));
        if let Some(x) = self.global_consumer {
            fields.push(("global_consumer".to_owned(), Json::Str(format!("0x{:016x}", x.to_bits()))));
        }
        if let Some(x) = self.global_producer {
            fields.push(("global_producer".to_owned(), Json::Str(format!("0x{:016x}", x.to_bits()))));
        }
        if let Some(s) = &self.prior_source {
            fields.push(("prior_source".to_owned(), Json::Str(s.as_raw().to_owned())));
        }
        if let Some(x) = self.specific_consumer {
            fields.push(("specific_consumer".to_owned(), Json::Str(format!("0x{:016x}", x.to_bits()))));
        }
        if let Some(x) = self.specific_producer {
            fields.push(("specific_producer".to_owned(), Json::Str(format!("0x{:016x}", x.to_bits()))));
        }
        to_canonical(&Json::Obj(fields)).map_err(|_| RiskError::InvalidTolerance { reason: "canonical_json" })
    }

    /// SHA-256 digest of [`Self::to_canonical_bytes`].
    pub fn digest(&self) -> Result<Digest, RiskError> {
        let bytes = self.to_canonical_bytes()?;
        Ok(sha256_digest(&bytes))
    }
}

/// Typed errors of conformity risk computation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskError {
    /// `invalid_prior`: prior mean, standard deviation or source is invalid.
    InvalidPrior { reason: &'static str },
    /// `invalid_uncertainty`: measurement standard uncertainty `u` is non-finite or `<= 0`.
    InvalidUncertainty { reason: &'static str },
    /// `invalid_tolerance`: tolerance limits or guard band are missing, inverted or non-finite.
    InvalidTolerance { reason: &'static str },
    /// `invalid_probability`: conformance probability `p` is non-finite or outside `[0, 1]`.
    InvalidProbability { reason: &'static str },
    /// `unbound_rule`: rule cannot define an acceptance zone (unbound guard-band template or
    /// `label_only`).
    UnboundRule,
    /// Numeric error from `fid-num`.
    Num(NumError),
}

impl RiskError {
    /// Stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            RiskError::InvalidPrior { .. } => "invalid_prior",
            RiskError::InvalidUncertainty { .. } => "invalid_uncertainty",
            RiskError::InvalidTolerance { .. } => "invalid_tolerance",
            RiskError::InvalidProbability { .. } => "invalid_probability",
            RiskError::UnboundRule => "unbound_rule",
            RiskError::Num(_) => "numeric",
        }
    }
}

impl fmt::Display for RiskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RiskError::InvalidPrior { reason } => write!(f, "invalid_prior: {reason}"),
            RiskError::InvalidUncertainty { reason } => write!(f, "invalid_uncertainty: {reason}"),
            RiskError::InvalidTolerance { reason } => write!(f, "invalid_tolerance: {reason}"),
            RiskError::InvalidProbability { reason } => write!(f, "invalid_probability: {reason}"),
            RiskError::UnboundRule => f.write_str("unbound_rule"),
            RiskError::Num(e) => write!(f, "numeric: {e}"),
        }
    }
}

impl std::error::Error for RiskError {}

impl From<NumError> for RiskError {
    fn from(e: NumError) -> Self {
        RiskError::Num(e)
    }
}

/// Computes specific consumer and producer risks from an outcome and conformance
/// probability `p` (REQ-UNC.DEC.RISK-001, AC-1):
///
/// - `CONFORMS` -> `(Some(1.0 - p), None)` (exact `f64` subtraction)
/// - `NONCONFORMS` -> `(None, Some(p))`
/// - `INDETERMINATE` -> `(Some(1.0 - p), Some(p))`
/// - `NOT_DECIDABLE` -> `(None, None)`
pub fn specific_risk(outcome: Outcome, p: Option<f64>) -> Result<(Option<f64>, Option<f64>), RiskError> {
    if let Some(pv) = p {
        if !pv.is_finite() {
            return Err(RiskError::InvalidProbability { reason: "non_finite" });
        }
        if !(0.0..=1.0).contains(&pv) {
            return Err(RiskError::InvalidProbability { reason: "out_of_range" });
        }
    }
    Ok(match outcome {
        Outcome::Conforms => (p.map(|pv| 1.0 - pv), None),
        Outcome::Nonconforms => (None, p),
        Outcome::Indeterminate => (p.map(|pv| 1.0 - pv), p),
        Outcome::NotDecidable(_) => (None, None),
    })
}

/// Derives the acceptance zone of `rule` for tolerance `tol` and normal measurement
/// standard uncertainty `u` (and optional expanded uncertainty `expanded_u`; defaults to
/// `2 * u` for guard-band rules when `expanded_u` is `None`).
pub fn acceptance_zone_for_rule(
    tol: ToleranceF64,
    rule: &DecisionRule,
    u: f64,
    expanded_u: Option<f64>,
) -> Result<AcceptanceZone, RiskError> {
    tol.validate()?;
    validate_u(u)?;
    match rule.params() {
        RuleParams::SimpleAcceptance => Ok(AcceptanceZone { lower: tol.lsl, upper: tol.usl }),
        RuleParams::GuardBand { r: Some(r_dec) } => {
            let r = r_dec.to_f64("r").map_err(|_| RiskError::InvalidTolerance { reason: "bad_guard_band_r" })?;
            let u_exp = match expanded_u {
                Some(ue) => {
                    if !ue.is_finite() || ue <= 0.0 {
                        return Err(RiskError::InvalidUncertainty { reason: "bad_expanded_u" });
                    }
                    ue
                }
                None => 2.0 * u,
            };
            AcceptanceZone::from_guard_band(tol, r * u_exp)
        }
        RuleParams::GuardBand { r: None } | RuleParams::LabelOnly => Err(RiskError::UnboundRule),
        RuleParams::ConformanceProbability { p_min } => {
            let p_target = p_min.to_f64("p_min").map_err(|_| RiskError::InvalidProbability { reason: "bad_p_min" })?;
            acceptance_zone_conformance_prob(tol, u, p_target)
        }
    }
}

/// Computes the exact normal-model acceptance zone `{ y : P(eta in tol | Y_m = y) >= p_min }`
/// using a fixed-iteration bisection (no data-dependent loop count).
fn acceptance_zone_conformance_prob(tol: ToleranceF64, u: f64, p_min: f64) -> Result<AcceptanceZone, RiskError> {
    if !p_min.is_finite() || !(0.5 < p_min && p_min < 1.0) {
        return Err(RiskError::InvalidProbability { reason: "p_min_out_of_range" });
    }
    match (tol.lsl, tol.usl) {
        (Some(lsl), Some(usl)) => {
            let half_t = 0.5 * (usl - lsl);
            let mid = lsl + half_t;
            // Maximum achievable conformance probability occurs at the tolerance midpoint.
            let p_mid = prob_in_interval(mid, lsl, usl, u)?;
            if p_mid < p_min {
                return Ok(AcceptanceZone { lower: Some(mid), upper: Some(mid) });
            }
            // Solve P(eta in [lsl, usl] | Y_m = lsl + w) = p_min for w in [0, half_t].
            let mut lo = 0.0_f64;
            let mut hi = half_t;
            for _ in 0..BISECT_ITERS {
                let m = 0.5 * (lo + hi);
                let pm = prob_in_interval(lsl + m, lsl, usl, u)?;
                if pm < p_min {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            let w = hi;
            Ok(AcceptanceZone { lower: Some(lsl + w), upper: Some(usl - w) })
        }
        (Some(lsl), None) => {
            let z = inverse_normal_cdf(p_min)?;
            Ok(AcceptanceZone { lower: Some(lsl + z * u), upper: None })
        }
        (None, Some(usl)) => {
            let z = inverse_normal_cdf(p_min)?;
            Ok(AcceptanceZone { lower: None, upper: Some(usl - z * u) })
        }
        (None, None) => Err(RiskError::InvalidTolerance { reason: "no_limits" }),
    }
}

/// Fixed-iteration bisection for `Phi^{-1}(p)` on `p in (0.5, 1)` using `upper_tail`.
fn inverse_normal_cdf(p: f64) -> Result<f64, RiskError> {
    let target_tail = 1.0 - p;
    let mut lo = 0.0_f64;
    let mut hi = 40.0_f64;
    for _ in 0..BISECT_ITERS {
        let m = 0.5 * (lo + hi);
        let tail = upper_tail(m)?;
        if tail > target_tail {
            lo = m;
        } else {
            hi = m;
        }
    }
    Ok(0.5 * (lo + hi))
}

/// Computes `(global_consumer_risk, global_producer_risk)` for tolerance `tol`, acceptance
/// zone `zone`, normal measurement standard uncertainty `u`, and declared normal process
/// prior `prior` (REQ-UNC.DEC.RISK-002, REQ-UNC.DEC.RISK-004, AC-2, AC-4, AC-5).
pub fn global_risk(tol: ToleranceF64, zone: AcceptanceZone, u: f64, prior: &Prior) -> Result<(f64, f64), RiskError> {
    tol.validate()?;
    validate_u(u)?;
    prior.validate()?;

    let lo_dom = prior.mu - 10.0 * prior.sigma;
    let hi_dom = prior.mu + 10.0 * prior.sigma;
    if !lo_dom.is_finite() || !hi_dom.is_finite() {
        return Err(RiskError::InvalidPrior { reason: "domain_overflow" });
    }

    // Collect deterministic special points (limits + acceptance limits + prior mean).
    let mut special = Vec::with_capacity(5);
    if let Some(x) = tol.lsl {
        special.push(x);
    }
    if let Some(x) = tol.usl {
        special.push(x);
    }
    if let Some(x) = zone.lower {
        special.push(x);
    }
    if let Some(x) = zone.upper {
        special.push(x);
    }
    special.push(prior.mu);

    // Consumer risk: true value OUTSIDE tol, measured value INSIDE acceptance zone.
    let rc = if zone.is_empty() {
        0.0
    } else {
        let mut parts = Vec::with_capacity(2);
        if let Some(lsl) = tol.lsl {
            let hi = min_f64(hi_dom, lsl);
            if lo_dom < hi {
                parts.push(integrate_region(lo_dom, hi, &special, u, prior, |eta| {
                    let g = prior_pdf(eta, prior)?;
                    let p_in = prob_in_zone(eta, zone, u)?;
                    Ok(g * p_in)
                })?);
            }
        }
        if let Some(usl) = tol.usl {
            let lo = max_f64(lo_dom, usl);
            if lo < hi_dom {
                parts.push(integrate_region(lo, hi_dom, &special, u, prior, |eta| {
                    let g = prior_pdf(eta, prior)?;
                    let p_in = prob_in_zone(eta, zone, u)?;
                    Ok(g * p_in)
                })?);
            }
        }
        clamp_unit(fid_num::red::sum(&parts)?)
    };

    // Producer risk: true value INSIDE tol, measured value OUTSIDE acceptance zone.
    let in_lo = match tol.lsl {
        Some(lsl) => max_f64(lo_dom, lsl),
        None => lo_dom,
    };
    let in_hi = match tol.usl {
        Some(usl) => min_f64(hi_dom, usl),
        None => hi_dom,
    };
    let rp = if in_lo < in_hi {
        let val = integrate_region(in_lo, in_hi, &special, u, prior, |eta| {
            let g = prior_pdf(eta, prior)?;
            let p_out = prob_out_zone(eta, zone, u)?;
            Ok(g * p_out)
        })?;
        clamp_unit(val)
    } else {
        0.0
    };

    Ok((rc, rp))
}

/// Computes specific and (when requested and a prior is declared) global conformity risk
/// for an evaluated decision (algorithm `dec.risk.compute@1.0.0`).
///
/// - `RiskReporting::NotReported`: returns empty [`RiskFigures`].
/// - `RiskReporting::Specific`: returns specific consumer/producer risk from `decision`.
/// - `RiskReporting::SpecificAndGlobal`: returns specific risk plus global risk when
///   `prior` is `Some`, or sets [`RiskFlag::PriorMissing`] with no global figures when
///   `prior` is `None` (never assuming a prior).
pub fn compute(
    decision: &DecisionRecord,
    tol: &Tolerance,
    rule: &DecisionRule,
    u: f64,
    prior: Option<&Prior>,
) -> Result<RiskFigures, RiskError> {
    match rule.risk_reporting() {
        RiskReporting::NotReported => Ok(RiskFigures {
            specific_consumer: None,
            specific_producer: None,
            global_consumer: None,
            global_producer: None,
            prior_source: None,
            flags: Vec::new(),
        }),
        RiskReporting::Specific => {
            let (sc, sp) = specific_risk(decision.outcome(), decision.p_conformance())?;
            Ok(RiskFigures {
                specific_consumer: sc,
                specific_producer: sp,
                global_consumer: None,
                global_producer: None,
                prior_source: None,
                flags: Vec::new(),
            })
        }
        RiskReporting::SpecificAndGlobal => {
            let (sc, sp) = specific_risk(decision.outcome(), decision.p_conformance())?;
            let Some(p) = prior else {
                return Ok(RiskFigures {
                    specific_consumer: sc,
                    specific_producer: sp,
                    global_consumer: None,
                    global_producer: None,
                    prior_source: None,
                    flags: vec![RiskFlag::PriorMissing],
                });
            };
            p.validate()?;
            let tol_f = ToleranceF64::from_tolerance(tol)?;
            let zone = acceptance_zone_for_rule(tol_f, rule, u, decision.expanded_u())?;
            let (gc, gp) = global_risk(tol_f, zone, u, p)?;
            let mut flags = Vec::new();
            if zone.is_empty() {
                flags.push(RiskFlag::EmptyAcceptanceZone);
            }
            Ok(RiskFigures {
                specific_consumer: sc,
                specific_producer: sp,
                global_consumer: Some(gc),
                global_producer: Some(gp),
                prior_source: Some(p.source.clone()),
                flags,
            })
        }
    }
}

fn validate_u(u: f64) -> Result<(), RiskError> {
    if !u.is_finite() {
        return Err(RiskError::InvalidUncertainty { reason: "non_finite_u" });
    }
    if u <= 0.0 {
        return Err(RiskError::InvalidUncertainty { reason: "non_positive_u" });
    }
    Ok(())
}

/// Deterministic total-order min for validated finite `f64` (no `f64::min`).
#[inline]
fn min_f64(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// Deterministic total-order max for validated finite `f64` (no `f64::max`).
#[inline]
fn max_f64(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// KNP-1 deterministic clamp on validated finite `f64` (avoids `f64::clamp` which can panic).
#[inline]
#[allow(clippy::manual_clamp)]
fn clamp_unit(x: f64) -> f64 {
    if x < 0.0 {
        0.0
    } else if x > 1.0 {
        1.0
    } else {
        x
    }
}

fn prior_pdf(eta: f64, prior: &Prior) -> Result<f64, NumError> {
    let z = (eta - prior.mu) / prior.sigma;
    let e = fid_num::f::exp(-0.5 * z * z)?;
    Ok((FRAC_1_SQRT_2PI / prior.sigma) * e)
}

/// Cancellation-free `P(Y_m in [a, b] | eta)` for `a < b` and `Y_m ~ N(eta, u^2)`.
fn prob_in_interval(eta: f64, a: f64, b: f64, u: f64) -> Result<f64, NumError> {
    if a >= b {
        return Ok(0.0);
    }
    let val = if a > eta {
        upper_tail((a - eta) / u)? - upper_tail((b - eta) / u)?
    } else if b < eta {
        upper_tail((eta - b) / u)? - upper_tail((eta - a) / u)?
    } else {
        1.0 - upper_tail((b - eta) / u)? - upper_tail((eta - a) / u)?
    };
    Ok(clamp_unit(val))
}

fn prob_in_zone(eta: f64, zone: AcceptanceZone, u: f64) -> Result<f64, NumError> {
    match (zone.lower, zone.upper) {
        (Some(al), Some(au)) => prob_in_interval(eta, al, au, u),
        (Some(al), None) => upper_tail((al - eta) / u),
        (None, Some(au)) => upper_tail((eta - au) / u),
        (None, None) => Ok(1.0),
    }
}

/// Cancellation-free `P(Y_m not in zone | eta)`: sum of non-negative tail probabilities.
fn prob_out_zone(eta: f64, zone: AcceptanceZone, u: f64) -> Result<f64, NumError> {
    match (zone.lower, zone.upper) {
        (Some(al), Some(au)) => {
            if al >= au {
                return Ok(1.0);
            }
            Ok(clamp_unit(upper_tail((eta - al) / u)? + upper_tail((au - eta) / u)?))
        }
        (Some(al), None) => upper_tail((eta - al) / u),
        (None, Some(au)) => upper_tail((au - eta) / u),
        (None, None) => Ok(0.0),
    }
}

fn integrate_region<F>(a: f64, b: f64, special: &[f64], u: f64, prior: &Prior, mut f: F) -> Result<f64, RiskError>
where
    F: FnMut(f64) -> Result<f64, NumError>,
{
    if a >= b {
        return Ok(0.0);
    }
    // Split at every special point and fixed boundary-layer multiples of u and sigma.
    const U_MULTS: [f64; 4] = [1.0, 4.0, 16.0, 64.0];
    let mut pts = Vec::with_capacity(2 + special.len() * (1 + 2 * U_MULTS.len()) + 8);
    pts.push(a);
    pts.push(b);
    for &sp in special {
        if sp > a && sp < b {
            pts.push(sp);
        }
        for &m in &U_MULTS {
            let p_lo = sp - m * u;
            if p_lo > a && p_lo < b {
                pts.push(p_lo);
            }
            let p_hi = sp + m * u;
            if p_hi > a && p_hi < b {
                pts.push(p_hi);
            }
        }
    }
    // Also add fixed sigma-scale splits around mu so wide regions stay well resolved.
    for k in [-6_i32, -3, -1, 1, 3, 6] {
        let p = prior.mu + f64::from(k) * prior.sigma;
        if p > a && p < b {
            pts.push(p);
        }
    }

    // Total-order sort and deduplicate strictly increasing breakpoints.
    pts.sort_by(f64::total_cmp);
    let mut uniq = Vec::with_capacity(pts.len());
    for p in pts {
        if uniq.last().is_none_or(|&prev| p > prev) {
            uniq.push(p);
        }
    }

    let mut interval_vals = Vec::with_capacity(uniq.len());
    for pair in uniq.windows(2) {
        let lo = pair[0];
        let hi = pair[1];
        if lo >= hi {
            continue;
        }
        let val = integrate_composite_gl16(lo, hi, &mut f)?;
        interval_vals.push(val);
    }
    Ok(fid_num::red::sum(&interval_vals)?)
}

fn integrate_composite_gl16<F>(a: f64, b: f64, f: &mut F) -> Result<f64, NumError>
where
    F: FnMut(f64) -> Result<f64, NumError>,
{
    let width = b - a;
    let h = width / (SUB_PANELS as f64);
    let mut panel_sums = [0.0_f64; SUB_PANELS];
    for (p, slot) in panel_sums.iter_mut().enumerate() {
        let lo = a + h * (p as f64);
        let hi = if p + 1 == SUB_PANELS { b } else { a + h * ((p + 1) as f64) };
        let mid = 0.5 * (lo + hi);
        let half = 0.5 * (hi - lo);
        let mut terms = [0.0_f64; 16];
        for i in 0..16 {
            let x = mid + half * GL16_NODES[i];
            terms[i] = GL16_WEIGHTS[i] * f(x)?;
        }
        *slot = half * fid_num::red::sum(&terms)?;
    }
    fid_num::red::sum(&panel_sums)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn constants_match_pinned_bits() {
        assert_eq!(FRAC_1_SQRT_2PI.to_bits(), 0x3fd9_8845_33d4_3651);
        // GL16 weights sum to 2.0 on [-1, 1].
        let wsum = fid_num::red::sum(&GL16_WEIGHTS).unwrap();
        assert!((wsum - 2.0).abs() < 1e-15);
    }

    #[test]
    fn untrusted_source_escapes_control_chars() {
        let s = UntrustedString::new("SPC\x1b[31m\nlot-7").unwrap();
        let esc = s.as_escaped();
        assert!(!esc.contains('\x1b'));
        assert!(!esc.contains('\n'));
        assert!(esc.contains("\\u001b"));
        assert!(esc.contains("\\u000a"));
    }
}
