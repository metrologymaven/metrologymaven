//! Rank-order statistics over local sizes (spec node `GDT.DIM.SIZEISO.RANK`), pure.
//!
//! [`rank_statistic`] reduces a list of local sizes (metres) to one number with a named
//! statistic. Which statistic an ISO 14405-1 rank-order modifier means, and the variant
//! parameters (the median of an even count, the standard-deviation denominator), come from
//! the caller, who takes them from the resolved DMT row (REQ-GDT.DIM.SIZEISO.RANK-001):
//! [`RankStat`] has no default and no variant parameter is chosen here. Mapping modifiers to
//! statistics is DMT content (`GDT.RES.ROWISO`), producing local sizes is
//! `GDT.DIM.SIZEISO.LOCAL` and dispatch is `GDT.DIM.SIZEISO.DISP`. The value returned is a
//! number, not a conformance decision; deciding conformance is `UNC.DEC`'s job.
//!
//! # Pinned procedure
//!
//! 1. **Checks, in this order.** An empty list is `empty_sizes`. The list is then scanned in
//!    index order and the first non-finite element is `nan_input{index}` (NaN) or
//!    `infinite_input{index}` (an infinity; a local size cannot be infinite). Only then is
//!    `StdDev{NMinus1}` over one value `insufficient_count{needed: 2, got: 1}`.
//! 2. **Order statistics.** `Max` and `Min` are a scan with the IEEE 754 total order
//!    (`fid_num::f::total_max` / `total_min`), so `-0.0 < +0.0` and the result does not depend
//!    on the input order. `Median` sorts a copy with `f64::total_cmp` (a total order on the
//!    NaN-free input, so the sorted sequence of bit patterns is unique). With `n` values and
//!    the sorted copy `v`, an odd `n` gives `v[n / 2]` for every `even` variant; an even `n`
//!    gives `v[n/2 - 1]` (`Lower`), `v[n/2]` (`Upper`) or their midpoint (`MeanOfMiddle`).
//! 3. **Range** is `max - min` (one rounding).
//! 4. **Midpoint** (`MidRange` and `MeanOfMiddle`) of `a` and `b` is `(a + b) * 0.5`; if
//!    `a + b` overflows, it is `a * 0.5 + b * 0.5` instead.
//! 5. **Mean** is `fid_num::red::mean`: the KRN.NUM.RED fixed-order sum (index order,
//!    Neumaier compensation in 256-element blocks combined by a fixed tree) divided by `n`.
//! 6. **StdDev** is two-pass: `m` = the mean of step 5, `d_i = x_i - m` in index order, `S` =
//!    `fid_num::red::sum_sq(d)` (the same fixed-order sum of the plain products `d_i * d_i`),
//!    then `sqrt(S / D)` with `D = n` (`N`) or `n - 1` (`NMinus1`) and the correctly rounded
//!    `fid_num::f::sqrt`.
//! 7. **Overflow.** A sum, product, deviation or range that overflows is `overflow`, even
//!    where the exact result would be finite: no value is made up.
//!
//! # Rounding bounds
//!
//! With `u = 2^-53`:
//!
//! - `Max`, `Min`, odd-count `Median` and even-count `Median{Lower | Upper}` are input values:
//!   exact.
//! - `Range`, `MidRange` and `Median{MeanOfMiddle}` are correctly rounded (one round to
//!   nearest, ties to even, of the exact value). For the midpoint: if `a + b` is finite and
//!   `(a + b) / 2` is normal, halving is exact; if `|a + b| < 2^-1021`, the addition is exact
//!   and the halving is the only rounding; if `a + b` overflows, both halves are exact.
//! - `Mean` is correctly rounded whenever the fixed-order sum is exact (for example sizes
//!   that are multiples of a common power of two whose partial sums stay below `2^53` of
//!   it, as in the fixtures); in general its error is the sum's error divided by `n` plus
//!   half an ulp of the mean.
//! - `StdDev` is not correctly rounded. When the mean, the deviations, their squares and
//!   their sum are all exact, the only roundings are `S / D` and the square root, and the
//!   result is within 1 ulp of the exact value (AC-1). In general, to first order, the
//!   relative error is at most about `4u` (one rounding per deviation, square, the sum,
//!   the division and the root, the square root halving the error of its argument) plus
//!   `n * e^2 / (2 * sum (x_i - mean)^2)` from a mean error `e`, which only overestimates
//!   [unverified: derived here, not independently reviewed].
//!
//! # Determinism
//!
//! D0: plain IEEE 754 binary64 `+`, `-`, `*`, `/` in a pinned order, no fused multiply-add,
//! no `f64::min` / `f64::max`, sums and the square root through `fid-num`, NaN rejected, no
//! `HashMap`. The order statistics, range and mid-range do not depend on the input order;
//! the mean and standard deviation follow the index order (REQ-GDT.DIM.SIZEISO.RANK-002).

use fid_num::{f, red, NumError};
use std::fmt;

/// How the median of an even count of values is taken (a parameter of the DMT row).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MedianEven {
    /// The midpoint of the two middle values (`mean_of_middle`).
    MeanOfMiddle,
    /// The lower of the two middle values (`lower`).
    Lower,
    /// The upper of the two middle values (`upper`).
    Upper,
}

/// The standard-deviation denominator (a parameter of the DMT row).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StdDenominator {
    /// Divide the sum of squared deviations by `n` (`n`).
    N,
    /// Divide by `n - 1` (`n_minus_1`); needs at least two values.
    NMinus1,
}

/// The closed set of rank-order statistics, with every variant parameter explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RankStat {
    /// The largest value in total order.
    Max,
    /// The smallest value in total order.
    Min,
    /// The arithmetic mean.
    Mean,
    /// The median, with the even-count rule.
    Median {
        /// How an even count is resolved.
        even: MedianEven,
    },
    /// The midpoint of the largest and smallest values.
    MidRange,
    /// The largest minus the smallest value.
    Range,
    /// The standard deviation, with the denominator.
    StdDev {
        /// `n` or `n - 1`.
        denominator: StdDenominator,
    },
}

impl RankStat {
    /// Every statistic, each variant parameter spelt out (ten in all).
    pub const ALL: [RankStat; 10] = [
        RankStat::Max,
        RankStat::Min,
        RankStat::Mean,
        RankStat::Median { even: MedianEven::MeanOfMiddle },
        RankStat::Median { even: MedianEven::Lower },
        RankStat::Median { even: MedianEven::Upper },
        RankStat::MidRange,
        RankStat::Range,
        RankStat::StdDev { denominator: StdDenominator::N },
        RankStat::StdDev { denominator: StdDenominator::NMinus1 },
    ];

    /// The stable key: `max`, `min`, `mean`, `median.mean_of_middle`, `median.lower`,
    /// `median.upper`, `mid_range`, `range`, `std_dev.n` or `std_dev.n_minus_1`.
    pub fn key(self) -> &'static str {
        match self {
            RankStat::Max => "max",
            RankStat::Min => "min",
            RankStat::Mean => "mean",
            RankStat::Median { even: MedianEven::MeanOfMiddle } => "median.mean_of_middle",
            RankStat::Median { even: MedianEven::Lower } => "median.lower",
            RankStat::Median { even: MedianEven::Upper } => "median.upper",
            RankStat::MidRange => "mid_range",
            RankStat::Range => "range",
            RankStat::StdDev { denominator: StdDenominator::N } => "std_dev.n",
            RankStat::StdDev { denominator: StdDenominator::NMinus1 } => "std_dev.n_minus_1",
        }
    }

    /// The statistic with stable key `key` (the inverse of [`RankStat::key`]), if any.
    pub fn from_key(key: &str) -> Option<RankStat> {
        RankStat::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// Why no statistic was computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankError {
    /// `empty_sizes`: the list is empty.
    EmptySizes,
    /// `nan_input`: the first non-finite element, at `index`, is NaN.
    NanInput {
        /// Index of the element.
        index: usize,
    },
    /// `infinite_input`: the first non-finite element, at `index`, is an infinity.
    InfiniteInput {
        /// Index of the element.
        index: usize,
    },
    /// `insufficient_count`: the statistic needs `needed` values and has `got`.
    InsufficientCount {
        /// Values needed.
        needed: usize,
        /// Values given.
        got: usize,
    },
    /// `overflow`: an intermediate sum, product, deviation or the range overflowed.
    Overflow,
    /// `numeric`: any other `fid-num` refusal (not expected after the input scan; passed
    /// through rather than hidden).
    Numeric(NumError),
}

impl RankError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            RankError::EmptySizes => "empty_sizes",
            RankError::NanInput { .. } => "nan_input",
            RankError::InfiniteInput { .. } => "infinite_input",
            RankError::InsufficientCount { .. } => "insufficient_count",
            RankError::Overflow => "overflow",
            RankError::Numeric(_) => "numeric",
        }
    }
}

impl fmt::Display for RankError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RankError::NanInput { index } | RankError::InfiniteInput { index } => {
                write!(f, "{}: element {index}", self.code())
            }
            RankError::InsufficientCount { needed, got } => {
                write!(f, "insufficient_count: needs {needed} values, got {got}")
            }
            RankError::Numeric(e) => write!(f, "numeric: {e}"),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for RankError {}

fn num(e: NumError) -> RankError {
    match e {
        NumError::Infinite { .. } => RankError::Overflow,
        other => RankError::Numeric(other),
    }
}

/// The statistic `stat` over `sizes` (metres), by the pinned procedure of the module
/// documentation.
pub fn rank_statistic(sizes: &[f64], stat: RankStat) -> Result<f64, RankError> {
    if sizes.is_empty() {
        return Err(RankError::EmptySizes);
    }
    for (index, &x) in sizes.iter().enumerate() {
        if x.is_nan() {
            return Err(RankError::NanInput { index });
        }
        if x.is_infinite() {
            return Err(RankError::InfiniteInput { index });
        }
    }
    match stat {
        RankStat::Max => max(sizes),
        RankStat::Min => min(sizes),
        RankStat::Mean => red::mean(sizes).map_err(num),
        RankStat::Median { even } => median(sizes, even),
        RankStat::MidRange => midpoint(max(sizes)?, min(sizes)?),
        RankStat::Range => finite(max(sizes)? - min(sizes)?),
        RankStat::StdDev { denominator } => std_dev(sizes, denominator),
    }
}

fn finite(x: f64) -> Result<f64, RankError> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(RankError::Overflow)
    }
}

fn max(xs: &[f64]) -> Result<f64, RankError> {
    let (&first, rest) = xs.split_first().ok_or(RankError::EmptySizes)?;
    let mut m = first;
    for &x in rest {
        m = f::total_max(m, x).map_err(num)?;
    }
    Ok(m)
}

fn min(xs: &[f64]) -> Result<f64, RankError> {
    let (&first, rest) = xs.split_first().ok_or(RankError::EmptySizes)?;
    let mut m = first;
    for &x in rest {
        m = f::total_min(m, x).map_err(num)?;
    }
    Ok(m)
}

/// The correctly rounded `(a + b) / 2` of two finite values.
fn midpoint(a: f64, b: f64) -> Result<f64, RankError> {
    let s = a + b;
    if s.is_finite() {
        return Ok(s * 0.5);
    }
    finite(a * 0.5 + b * 0.5)
}

fn median(xs: &[f64], even: MedianEven) -> Result<f64, RankError> {
    let mut v = xs.to_vec();
    v.sort_unstable_by(f64::total_cmp);
    let n = v.len();
    let upper = *v.get(n / 2).ok_or(RankError::EmptySizes)?;
    if n % 2 == 1 {
        return Ok(upper);
    }
    // n is even and at least 2 here, so n / 2 - 1 is a valid index.
    let lower = *v.get(n / 2 - 1).ok_or(RankError::EmptySizes)?;
    match even {
        MedianEven::Lower => Ok(lower),
        MedianEven::Upper => Ok(upper),
        MedianEven::MeanOfMiddle => midpoint(lower, upper),
    }
}

fn std_dev(xs: &[f64], denominator: StdDenominator) -> Result<f64, RankError> {
    let n = xs.len();
    let d = match denominator {
        StdDenominator::N => n,
        StdDenominator::NMinus1 => {
            if n < 2 {
                return Err(RankError::InsufficientCount { needed: 2, got: n });
            }
            n - 1
        }
    };
    let m = red::mean(xs).map_err(num)?;
    let mut dev = Vec::with_capacity(n);
    for &x in xs {
        dev.push(finite(x - m)?);
    }
    let s = red::sum_sq(&dev).map_err(num)?;
    // Slice lengths never exceed 2^53, so the conversion is exact.
    f::sqrt(s / d as f64).map_err(num)
}
