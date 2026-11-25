//! Capability index kernel `stats.cap.index@1.0.0` (spec node `RPT.STATS.CAP.INDEX`,
//! REQ-RPT.STATS.CAP.INDEX-001 to -008).
//!
//! [`capability`] turns a mean `μ`, a spread estimate `σ`, the sample size `n` and the
//! specification limits into a potential index and a centred index. The same normal-theory
//! formulas serve both index families; only the spread differs and the caller names it:
//! within-subgroup `σ` gives the C family (Cp, Cpk, Cpu, Cpl), overall `σ` the P family
//! (Pp, Ppk, Ppu, Ppl). The estimator ID the caller supplies is carried on the result
//! unchanged.
//!
//! # Formulas
//!
//! | Limits | Potential index | Centred index |
//! |---|---|---|
//! | both | `(USL − LSL) / (6σ)` | `min(USL − μ, μ − LSL) / (3σ)` |
//! | upper only | `NOT_APPLICABLE` | `(USL − μ) / (3σ)` |
//! | lower only | `NOT_APPLICABLE` | `(μ − LSL) / (3σ)` |
//!
//! The index definitions follow the process-capability parts of the ISO 22514 series
//! [unverified part, edition and clause]; no standards text is reproduced here
//! (REQ-RPT.STATS.CAP.INDEX-008). A centred index is negative when the mean lies outside the
//! limit it is measured against.
//!
//! # Order of decisions
//!
//! 1. **Errors** (`stats.bad_capability_input`, [`BadInput`]), checked in this order: a
//!    non-finite `mean`, `sigma`, `lsl` or `usl` (in that order); a negative `sigma`; no limit
//!    at all; `USL ≤ LSL`; `min_n < 2`. An error wins over every status, so bad input never
//!    hides behind `INSUFFICIENT_DATA`.
//! 2. **`INSUFFICIENT_DATA`** when `n < min_n`, for every applicable index.
//! 3. **`UNDEFINED_ZERO_SPREAD`** when `σ` is zero (`+0.0` or `-0.0`; `-0.0` is a zero spread,
//!    not a negative one), for every applicable index.
//! 4. Otherwise a number. If the quotient overflows (for example `USL − LSL` beyond the `f64`
//!    range, or a subnormal `σ`), the call fails with `result_not_finite` rather than return an
//!    infinite index.
//!
//! The potential index of a one-sided tolerance is `NOT_APPLICABLE` in every branch: it does
//! not exist for that tolerance, whatever the data (REQ-RPT.STATS.CAP-002).
//!
//! # Arithmetic (D0)
//!
//! Exactly the IEEE 754 basic operations, in this pinned order and never fused:
//! `d = USL − LSL; s6 = 6.0 * σ; potential = d / s6` and
//! `a = USL − μ; b = μ − LSL; s3 = 3.0 * σ; centred = total_min(a, b) / s3`, with
//! `total_min` from `fid-num` (KRN.NUM.FN), then `+ 0.0` on each index, which turns a `-0.0`
//! (from a signed-zero limit or mean) into `+0.0` and changes nothing else. Each index is therefore within a few ulp of the
//! exact value and bit-identical on every target (REQ-RPT.STATS.CAP.INDEX-007).

use core::fmt;

use fid_num::f::total_min;

/// The algorithm ID every [`IndexPair`] carries (REQ-RPT.STATS.CAP.INDEX-006).
pub const CAP_INDEX_ALGORITHM_ID: &str = "stats.cap.index@1.0.0";

/// The default minimum sample size for an index (REQ-RPT.STATS.CAP.INDEX-003). The value
/// 25 is a flagged default awaiting review [unverified]; callers pass their configured
/// minimum to [`capability`].
pub const DEFAULT_MIN_N: u64 = 25;

/// The longest estimator ID, in bytes.
pub const MAX_ESTIMATOR_ID_BYTES: usize = 64;

/// The index family: which spread the caller supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Within-subgroup spread: Cp, Cpk, Cpu, Cpl.
    C,
    /// Overall spread: Pp, Ppk, Ppu, Ppl.
    P,
}

impl Family {
    /// `"C"` or `"P"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Family::C => "C",
            Family::P => "P",
        }
    }
}

/// Which specification limits the tolerance has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Both limits.
    Both,
    /// Only an upper limit.
    UpperOnly,
    /// Only a lower limit.
    LowerOnly,
}

impl Side {
    /// `"BOTH"`, `"UPPER_ONLY"` or `"LOWER_ONLY"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Both => "BOTH",
            Side::UpperOnly => "UPPER_ONLY",
            Side::LowerOnly => "LOWER_ONLY",
        }
    }
}

/// One index: a finite number or a typed non-result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IndexValue {
    /// The index, always finite.
    Value(f64),
    /// The index does not exist for this tolerance (the potential index of a one-sided one).
    NotApplicable,
    /// Fewer values than the configured minimum; no number is produced.
    InsufficientData,
    /// The spread estimate is zero; no number is produced.
    UndefinedZeroSpread,
}

impl IndexValue {
    /// The number, if there is one.
    pub fn value(self) -> Option<f64> {
        match self {
            IndexValue::Value(x) => Some(x),
            _ => None,
        }
    }

    /// The status code of a non-result (`NOT_APPLICABLE`, `INSUFFICIENT_DATA`,
    /// `UNDEFINED_ZERO_SPREAD`), or `None` for a number.
    pub fn status(self) -> Option<&'static str> {
        match self {
            IndexValue::Value(_) => None,
            IndexValue::NotApplicable => Some("NOT_APPLICABLE"),
            IndexValue::InsufficientData => Some("INSUFFICIENT_DATA"),
            IndexValue::UndefinedZeroSpread => Some("UNDEFINED_ZERO_SPREAD"),
        }
    }
}

/// The ID of the spread estimator that produced `σ`, for example `rbar_d2@1` or
/// `overall_sd@1`: `<name>@<major>`, the name `[a-z][a-z0-9_.]*`, the major 1 to 9999 without a
/// leading zero, at most [`MAX_ESTIMATOR_ID_BYTES`] bytes. Only the syntax is checked here;
/// which estimators exist is the caller's registry (`RPT.STATS.CAP.SIGMA`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstimatorId(String);

impl EstimatorId {
    /// Validates `s`; `stats.bad_capability_input` (`bad_estimator_id`) if it is malformed.
    pub fn new(s: &str) -> Result<Self, StatsError> {
        let bad = StatsError::BadCapabilityInput { reason: BadInput::BadEstimatorId };
        if s.len() > MAX_ESTIMATOR_ID_BYTES {
            return Err(bad);
        }
        let (name, major) = s.split_once('@').ok_or(bad)?;
        let nb = name.as_bytes();
        let name_ok = nb.first().is_some_and(u8::is_ascii_lowercase)
            && nb.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'.');
        let mb = major.as_bytes();
        let major_ok =
            (1..=4).contains(&mb.len()) && mb.iter().all(u8::is_ascii_digit) && mb.first().is_some_and(|&c| c != b'0');
        if name_ok && major_ok {
            Ok(EstimatorId(s.to_owned()))
        } else {
            Err(bad)
        }
    }

    /// The ID as given.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EstimatorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The result of [`capability`] (REQ-RPT.STATS.CAP.INDEX-006).
#[derive(Debug, Clone, PartialEq)]
pub struct IndexPair {
    /// Cp or Pp; `NotApplicable` for a one-sided tolerance.
    pub potential: IndexValue,
    /// Cpk or Ppk for two limits, otherwise the one-sided Cpu/Ppu or Cpl/Ppl.
    pub centred: IndexValue,
    /// Which limits the tolerance has.
    pub side: Side,
    /// The family the caller named.
    pub family: Family,
    /// The caller's spread estimator, passed through.
    pub estimator: EstimatorId,
    /// Always [`CAP_INDEX_ALGORITHM_ID`].
    pub algorithm: &'static str,
}

/// Why the input was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadInput {
    /// `mean` is NaN or infinite.
    NonFiniteMean,
    /// `sigma` is NaN or infinite.
    NonFiniteSigma,
    /// `lsl` is NaN or infinite.
    NonFiniteLsl,
    /// `usl` is NaN or infinite.
    NonFiniteUsl,
    /// `sigma` is below zero.
    NegativeSigma,
    /// Neither limit is given.
    NoLimits,
    /// Both limits are given and `USL ≤ LSL`.
    LimitsNotOrdered,
    /// `min_n` is below 2.
    MinNTooSmall,
    /// The estimator ID is malformed.
    BadEstimatorId,
    /// An index overflowed the `f64` range.
    ResultNotFinite,
}

impl BadInput {
    /// The reason code, for example `limits_not_ordered`.
    pub fn as_str(self) -> &'static str {
        match self {
            BadInput::NonFiniteMean => "non_finite_mean",
            BadInput::NonFiniteSigma => "non_finite_sigma",
            BadInput::NonFiniteLsl => "non_finite_lsl",
            BadInput::NonFiniteUsl => "non_finite_usl",
            BadInput::NegativeSigma => "negative_sigma",
            BadInput::NoLimits => "no_limits",
            BadInput::LimitsNotOrdered => "limits_not_ordered",
            BadInput::MinNTooSmall => "min_n_too_small",
            BadInput::BadEstimatorId => "bad_estimator_id",
            BadInput::ResultNotFinite => "result_not_finite",
        }
    }
}

/// Errors of the statistics kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsError {
    /// `stats.bad_capability_input{reason}` (REQ-RPT.STATS.CAP.INDEX-005).
    BadCapabilityInput {
        /// Which rule the input broke.
        reason: BadInput,
    },
}

impl StatsError {
    /// The error code, `stats.bad_capability_input`.
    pub fn code(self) -> &'static str {
        match self {
            StatsError::BadCapabilityInput { .. } => "stats.bad_capability_input",
        }
    }
}

impl fmt::Display for StatsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatsError::BadCapabilityInput { reason } => write!(f, "{}: {}", self.code(), reason.as_str()),
        }
    }
}

impl core::error::Error for StatsError {}

fn bad(reason: BadInput) -> StatsError {
    StatsError::BadCapabilityInput { reason }
}

fn finite(x: f64, reason: BadInput) -> Result<f64, StatsError> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(bad(reason))
    }
}

/// A finite index with any negative zero made `+0.0`: in round-to-nearest-even `-0.0 + 0.0`
/// is `+0.0` and every other value is unchanged, so a signed-zero limit or mean (for example
/// `μ = -0.0` on `LSL = +0.0`) cannot produce a `-0.0` index.
fn finite_index(x: f64) -> Result<IndexValue, StatsError> {
    finite(x + 0.0, BadInput::ResultNotFinite).map(IndexValue::Value)
}

/// The potential and centred capability indices of one characteristic (see the module
/// documentation for the formulas, the order of decisions and the pinned arithmetic).
///
/// `lsl` and `usl` are the specification limits (at least one), `n` the number of values the
/// mean and spread came from, `min_n` the configured minimum sample size (usually
/// [`DEFAULT_MIN_N`]), `family` and `estimator` name the spread `sigma`.
///
/// # Errors
///
/// `stats.bad_capability_input` with a [`BadInput`] reason.
#[allow(clippy::too_many_arguments)] // the signature is pinned by the leaf's Design
pub fn capability(
    mean: f64,
    sigma: f64,
    n: u64,
    lsl: Option<f64>,
    usl: Option<f64>,
    min_n: u64,
    family: Family,
    estimator: EstimatorId,
) -> Result<IndexPair, StatsError> {
    let mean = finite(mean, BadInput::NonFiniteMean)?;
    let sigma = finite(sigma, BadInput::NonFiniteSigma)?;
    let lsl = lsl.map(|x| finite(x, BadInput::NonFiniteLsl)).transpose()?;
    let usl = usl.map(|x| finite(x, BadInput::NonFiniteUsl)).transpose()?;
    // `-0.0 < 0.0` is false: a negative zero is a zero spread.
    if sigma < 0.0 {
        return Err(bad(BadInput::NegativeSigma));
    }
    let side = match (lsl, usl) {
        (None, None) => return Err(bad(BadInput::NoLimits)),
        (Some(l), Some(u)) if u <= l => return Err(bad(BadInput::LimitsNotOrdered)),
        (Some(_), Some(_)) => Side::Both,
        (None, Some(_)) => Side::UpperOnly,
        (Some(_), None) => Side::LowerOnly,
    };
    if min_n < 2 {
        return Err(bad(BadInput::MinNTooSmall));
    }

    let status = if n < min_n {
        Some(IndexValue::InsufficientData)
    } else if sigma == 0.0 {
        Some(IndexValue::UndefinedZeroSpread)
    } else {
        None
    };
    let (potential, centred) = match (status, lsl, usl) {
        (Some(s), _, _) => (if side == Side::Both { s } else { IndexValue::NotApplicable }, s),
        (None, Some(l), Some(u)) => {
            let s6 = 6.0 * sigma;
            let s3 = 3.0 * sigma;
            let near = total_min(u - mean, mean - l).map_err(|_| bad(BadInput::ResultNotFinite))?;
            (finite_index((u - l) / s6)?, finite_index(near / s3)?)
        }
        (None, None, Some(u)) => (IndexValue::NotApplicable, finite_index((u - mean) / (3.0 * sigma))?),
        (None, Some(l), None) => (IndexValue::NotApplicable, finite_index((mean - l) / (3.0 * sigma))?),
        // Unreachable: `side` above refused a missing pair of limits.
        (None, None, None) => return Err(bad(BadInput::NoLimits)),
    };
    Ok(IndexPair { potential, centred, side, family, estimator, algorithm: CAP_INDEX_ALGORITHM_ID })
}
