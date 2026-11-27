//! Residual-statistics policies: n-sigma and robust scale. Spec node `KRN.OUT.STAT`.
//!
//! Both methods decide by the fit's signed residuals alone (metres, in point order); the
//! fitted feature and the points are not read.
//!
//! - [`NSigma`] (`filter.outlier.nsigma@1.0.0`) excludes each point whose signed residual lies
//!   beyond `k` residual standard deviations from zero (REQ-KRN.OUT.STAT-001).
//! - [`RobustScale`] (`filter.outlier.robust_scale@1.0.0`) excludes each point whose residual
//!   lies beyond `k` robust scales from the residuals' median, the scale being
//!   1.4826 times the median absolute deviation or the Qn estimator (REQ-KRN.OUT.STAT-002).
//!
//! Both take a required [`Side`] (REQ-KRN.OUT.STAT-003): which signs of the deviation may be
//! excluded. The scale is always computed from every residual; the side only restricts which
//! points beyond the limit are excluded.
//!
//! # Pinned rules
//!
//! Every value below is one plain IEEE-754 binary64 operation, a `fid_num::red` reduction or
//! the correctly rounded `fid_num::f::sqrt` (no fused multiply-add, no `f64::min`/`max`, total
//! order for every sort), so every mask is D0: bit-identical on every target.
//!
//! 1. **Checks**, in this order: at least one residual
//!    ([`PolicyError::Num`] `EmptyInput`); at most `fid_fit::contract::MAX_POINTS`
//!    (`InvalidParameter { field: "point_count", reason: "out_of_range" }`); every residual
//!    finite ([`PolicyError::Num`]: KNP-1, NaN is an error). Qn additionally needs two
//!    residuals (`InvalidParameter { field: "residuals", reason: "too_few_for_qn" }`): with
//!    one there is no pair, so no scale exists.
//! 2. **Sorting.** A copy of the residuals is sorted ascending by `fid_num::f::total_cmp`
//!    (stable), so `-0.0` sorts before `+0.0`. The median of a sorted list `y` of length
//!    `n` is `y[n / 2]` for odd `n` and `y[n / 2 - 1] * 0.5 + y[n / 2] * 0.5` for even `n`
//!    (each half exact unless subnormal, one rounded addition, no overflow).
//! 3. **Centre and scale.**
//!    - n-sigma: centre `0`; `mean = red::mean(r)`, `e_i = r_i - mean`,
//!      `scale = red::norm2(e) / f::sqrt(n)`, the population standard deviation of
//!      `fid_fit::contract::ResidualStats::std_dev` (divisor `n`).
//!    - robust scale: centre `m` = the median of `r`; `d_i = r_i - m`.
//!      - MAD: `scale = 1.4826 * median(|d_i|)`.
//!      - Qn: `scale = 2.2219 * q`, where `q` is the `k`-th smallest (1-based) of the
//!        `n (n - 1) / 2` pairwise differences `y_j - y_i` (`i < j`) of the sorted list,
//!        `k = h (h - 1) / 2`, `h = floor(n / 2) + 1` (Rousseeuw and Croux 1993); no
//!        finite-sample correction factor. `q` is found exactly: the smallest non-negative
//!        `f64` `t` (searched over its bit pattern, which orders non-negative values) for
//!        which at least `k` differences are `<= t`, counted with two pointers. Each
//!        difference is the rounded `y_j - y_i`, which is monotone in `j` and in `i`, so the
//!        count is exact and `q` is one of the differences.
//! 4. **Zero scale** (REQ-KRN.OUT.STAT-004). If every residual is equal (`==`, so `-0.0` and
//!    `+0.0` are equal) or the scale is `0`, nothing is excluded and the status is
//!    [`MethodStatus::ZeroScale`]. The first test catches equal residuals whose rounded mean
//!    differs from them by an ulp, which would otherwise give a tiny positive scale.
//! 5. **Limit.** `threshold = k * scale`. A deviation, scale or threshold that is not finite
//!    is [`PolicyError::Num`] `Infinite`: an overflow never silently widens or narrows a
//!    limit.
//! 6. **Decision**, with `x` the deviation from the centre (`r_i` for n-sigma, `d_i` for
//!    robust scale): side both excludes `|x| > threshold`, positive `x > threshold`,
//!    negative `x < -threshold`. A point exactly at the limit is kept.
//!
//! Through [`PolicyMethod::exclude`], nominal distances must not be given
//! ([`PolicyError::NominalDistances`] `{ required: false }`: the methods never silently ignore
//! evidence) and there must be one residual per point ([`PolicyError::LengthMismatch`]
//! `residuals`); then the rules above apply. [`crate::policy::apply`] additionally runs the
//! guards and records the exclusion with its [`MethodStatus`].
//!
//! The constants 1.4826 (MAD) and 2.2219 (Qn) are the normal-consistency factors as usually
//! printed [unverified]; they are part of each method's version.

use core::cmp::Ordering;

use fid_fit::contract::{FittedFeature, PointsView, MAX_POINTS};
use fid_num::{check_finite_slice, f, red, NumError};

use crate::policy::{ExclusionMask, MethodStatus, OutlierPolicy, PolicyError, PolicyMethod, ScaleEstimator, Side};

/// The semantic version of both methods, the `<semver>` of their algorithm IDs.
pub const VERSION: (u32, u32, u32) = (1, 0, 0);

/// The normal-consistency factor of the median absolute deviation.
pub const MAD_FACTOR: f64 = 1.4826;

/// The normal-consistency factor of the Qn estimator (Rousseeuw and Croux 1993).
pub const QN_FACTOR: f64 = 2.2219;

/// Everything a residual-statistics method decided, with the values it decided by.
#[derive(Debug, Clone, PartialEq)]
pub struct StatOutcome {
    /// The excluded points.
    pub mask: ExclusionMask,
    /// [`MethodStatus::ZeroScale`] when the scale is zero (nothing is excluded), else
    /// [`MethodStatus::Decided`].
    pub status: MethodStatus,
    /// The centre deviations are measured from: `0` for n-sigma, the median for robust scale.
    pub center: f64,
    /// The scale: the standard deviation, `1.4826 * MAD` or `2.2219 * Qn` (metres); `0` on a
    /// zero scale.
    pub scale: f64,
    /// `k * scale` (metres); `0` on a zero scale.
    pub threshold: f64,
}

/// Excludes residuals beyond `k` residual standard deviations from zero
/// (REQ-KRN.OUT.STAT-001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NSigma {
    k: f64,
    side: Side,
}

impl NSigma {
    /// The policy with factor `k` and `side`, validated with the `KRN.OUT.POL` rules (`k` a
    /// positive normal finite number).
    pub fn new(k: f64, side: Side) -> Result<NSigma, PolicyError> {
        OutlierPolicy::NSigma { k, side }.validate_fields()?;
        Ok(NSigma { k, side })
    }

    /// The factor `k`.
    pub fn k(&self) -> f64 {
        self.k
    }

    /// Which signs may be excluded.
    pub fn side(&self) -> Side {
        self.side
    }

    /// Decides on `residuals` by the pinned rules (see the [module documentation](self)).
    pub fn evaluate(&self, residuals: &[f64]) -> Result<StatOutcome, PolicyError> {
        check_residuals(residuals)?;
        let n = residuals.len();
        let mean = red::mean(residuals)?;
        let mut e = Vec::new();
        e.try_reserve_exact(n).map_err(|_| PolicyError::Alloc { op: "stat.nsigma" })?;
        for (i, &r) in residuals.iter().enumerate() {
            e.push(finite("filter.stat.deviation", r - mean, Some(i))?);
        }
        // Slice lengths are at most MAX_POINTS (2^24), so the conversion is exact.
        let scale = finite("filter.stat.scale", red::norm2(&e)? / f::sqrt(n as f64)?, None)?;
        decide(residuals, 0.0, scale, self.k, self.side)
    }
}

impl PolicyMethod for NSigma {
    fn policy(&self) -> OutlierPolicy {
        OutlierPolicy::NSigma { k: self.k, side: self.side }
    }

    fn version(&self) -> (u32, u32, u32) {
        VERSION
    }

    fn exclude(
        &self,
        pts: &PointsView<'_>,
        fit: &FittedFeature,
        residuals: &[f64],
        nominal_dist: Option<&[f64]>,
    ) -> Result<ExclusionMask, PolicyError> {
        Ok(self.exclude_reported(pts, fit, residuals, nominal_dist)?.0)
    }

    fn exclude_reported(
        &self,
        pts: &PointsView<'_>,
        _fit: &FittedFeature,
        residuals: &[f64],
        nominal_dist: Option<&[f64]>,
    ) -> Result<(ExclusionMask, MethodStatus), PolicyError> {
        per_point(pts, residuals, nominal_dist)?;
        let o = self.evaluate(residuals)?;
        Ok((o.mask, o.status))
    }
}

/// Excludes residuals beyond `k` robust scales from the median (REQ-KRN.OUT.STAT-002).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RobustScale {
    k: f64,
    scale: ScaleEstimator,
    side: Side,
}

impl RobustScale {
    /// The policy with factor `k`, estimator `scale` and `side`, validated with the
    /// `KRN.OUT.POL` rules (`k` a positive normal finite number).
    pub fn new(k: f64, scale: ScaleEstimator, side: Side) -> Result<RobustScale, PolicyError> {
        OutlierPolicy::RobustScale { k, scale, side }.validate_fields()?;
        Ok(RobustScale { k, scale, side })
    }

    /// The factor `k`.
    pub fn k(&self) -> f64 {
        self.k
    }

    /// The scale estimator.
    pub fn estimator(&self) -> ScaleEstimator {
        self.scale
    }

    /// Which signs may be excluded.
    pub fn side(&self) -> Side {
        self.side
    }

    /// Decides on `residuals` by the pinned rules (see the [module documentation](self)).
    pub fn evaluate(&self, residuals: &[f64]) -> Result<StatOutcome, PolicyError> {
        check_residuals(residuals)?;
        if self.scale == ScaleEstimator::Qn && residuals.len() < 2 {
            return Err(PolicyError::InvalidParameter { field: "residuals", reason: "too_few_for_qn" });
        }
        let y = sorted(residuals)?;
        let m = median_sorted(&y)?;
        let raw = match self.scale {
            ScaleEstimator::Mad => {
                let mut a = Vec::new();
                a.try_reserve_exact(residuals.len()).map_err(|_| PolicyError::Alloc { op: "stat.mad" })?;
                for (i, &r) in residuals.iter().enumerate() {
                    a.push(finite("filter.stat.deviation", r - m, Some(i))?.abs());
                }
                MAD_FACTOR * median_sorted(&sorted(&a)?)?
            }
            ScaleEstimator::Qn => {
                // Every deviation must be finite too, as for MAD; checked before the scale.
                for (i, &r) in residuals.iter().enumerate() {
                    finite("filter.stat.deviation", r - m, Some(i))?;
                }
                QN_FACTOR * qn_order_statistic(&y)?
            }
        };
        let scale = finite("filter.stat.scale", raw, None)?;
        decide(residuals, m, scale, self.k, self.side)
    }
}

impl PolicyMethod for RobustScale {
    fn policy(&self) -> OutlierPolicy {
        OutlierPolicy::RobustScale { k: self.k, scale: self.scale, side: self.side }
    }

    fn version(&self) -> (u32, u32, u32) {
        VERSION
    }

    fn exclude(
        &self,
        pts: &PointsView<'_>,
        fit: &FittedFeature,
        residuals: &[f64],
        nominal_dist: Option<&[f64]>,
    ) -> Result<ExclusionMask, PolicyError> {
        Ok(self.exclude_reported(pts, fit, residuals, nominal_dist)?.0)
    }

    fn exclude_reported(
        &self,
        pts: &PointsView<'_>,
        _fit: &FittedFeature,
        residuals: &[f64],
        nominal_dist: Option<&[f64]>,
    ) -> Result<(ExclusionMask, MethodStatus), PolicyError> {
        per_point(pts, residuals, nominal_dist)?;
        let o = self.evaluate(residuals)?;
        Ok((o.mask, o.status))
    }
}

/// The median of `xs` by the pinned rule (a total-order sorted copy; the mean of the two
/// middle values, halved first, for an even length). [`PolicyError::Num`] for an empty or
/// non-finite input.
pub fn median(xs: &[f64]) -> Result<f64, PolicyError> {
    if xs.is_empty() {
        return Err(PolicyError::Num(NumError::EmptyInput { op: "filter.stat.median" }));
    }
    check_finite_slice("filter.stat.median", xs)?;
    median_sorted(&sorted(xs)?)
}

/// The `k`-th smallest (1-based) pairwise difference `|x_i - x_j|`, `i < j`, with
/// `k = h (h - 1) / 2` and `h = floor(n / 2) + 1`: the Qn order statistic before its
/// consistency factor (rule 3 of the [module documentation](self)). Needs two values
/// (`InvalidParameter { field: "residuals", reason: "too_few_for_qn" }`); non-finite input
/// or an overflowing difference is [`PolicyError::Num`].
pub fn qn_raw(xs: &[f64]) -> Result<f64, PolicyError> {
    if xs.len() < 2 {
        return Err(PolicyError::InvalidParameter { field: "residuals", reason: "too_few_for_qn" });
    }
    if xs.len() > MAX_POINTS {
        return Err(PolicyError::InvalidParameter { field: "point_count", reason: "out_of_range" });
    }
    check_finite_slice("filter.stat.qn", xs)?;
    qn_order_statistic(&sorted(xs)?)
}

/// Rule 1 of the module documentation.
fn check_residuals(residuals: &[f64]) -> Result<(), PolicyError> {
    if residuals.is_empty() {
        return Err(PolicyError::Num(NumError::EmptyInput { op: "filter.stat.residuals" }));
    }
    if residuals.len() > MAX_POINTS {
        return Err(PolicyError::InvalidParameter { field: "point_count", reason: "out_of_range" });
    }
    check_finite_slice("filter.stat.residuals", residuals)?;
    Ok(())
}

/// The trait path: no nominal distances, one residual per point.
fn per_point(pts: &PointsView<'_>, residuals: &[f64], nominal_dist: Option<&[f64]>) -> Result<(), PolicyError> {
    if nominal_dist.is_some() {
        return Err(PolicyError::NominalDistances { required: false });
    }
    if residuals.len() != pts.len() {
        return Err(PolicyError::LengthMismatch { what: "residuals", expected: pts.len(), got: residuals.len() });
    }
    Ok(())
}

fn finite(op: &'static str, x: f64, index: Option<usize>) -> Result<f64, PolicyError> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(PolicyError::Num(NumError::Infinite { op, index }))
    }
}

/// A copy of finite `xs`, sorted ascending in total order (stable).
fn sorted(xs: &[f64]) -> Result<Vec<f64>, PolicyError> {
    let mut y = Vec::new();
    y.try_reserve_exact(xs.len()).map_err(|_| PolicyError::Alloc { op: "stat.sort" })?;
    y.extend_from_slice(xs);
    // The input was checked finite, so total_cmp never fails; Equal would only keep order.
    y.sort_by(|a, b| f::total_cmp(*a, *b).unwrap_or(Ordering::Equal));
    Ok(y)
}

/// The median of a non-empty sorted list.
fn median_sorted(y: &[f64]) -> Result<f64, PolicyError> {
    let n = y.len();
    let mid = n / 2;
    let Some(&hi) = y.get(mid) else {
        return Err(PolicyError::Num(NumError::EmptyInput { op: "filter.stat.median" }));
    };
    if n % 2 == 1 {
        return Ok(hi);
    }
    let Some(&lo) = mid.checked_sub(1).and_then(|i| y.get(i)) else {
        return Err(PolicyError::Num(NumError::EmptyInput { op: "filter.stat.median" }));
    };
    Ok(lo * 0.5 + hi * 0.5)
}

/// Number of pairs `i < j` of the sorted list `y` with `y[j] - y[i] <= t` (`t >= 0`).
fn count_le(y: &[f64], t: f64) -> u64 {
    let mut lo = 0usize;
    let mut count = 0u64;
    for (j, &yj) in y.iter().enumerate() {
        // `lo` never passes `j`: `yj - yj == 0 <= t`.
        while let Some(&yi) = y.get(lo) {
            if yj - yi > t {
                lo += 1;
            } else {
                break;
            }
        }
        count += j.saturating_sub(lo) as u64;
    }
    count
}

/// The Qn order statistic of a sorted list of at least two finite values.
fn qn_order_statistic(y: &[f64]) -> Result<f64, PolicyError> {
    let n = y.len() as u64;
    let h = n / 2 + 1;
    let k = h * (h - 1) / 2;
    let (first, last) = match (y.first(), y.last()) {
        (Some(&a), Some(&b)) => (a, b),
        _ => return Err(PolicyError::InvalidParameter { field: "residuals", reason: "too_few_for_qn" }),
    };
    let span = finite("filter.stat.qn", last - first, None)?;
    // Non-negative f64 values are ordered as their bit patterns; search the smallest bit
    // pattern whose value has at least k differences at or below it.
    let (mut lo, mut hi) = (0u64, span.to_bits());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if count_le(y, f64::from_bits(mid)) >= k {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Ok(f64::from_bits(lo))
}

/// Rules 4 to 6: zero scale, the limit and the mask.
fn decide(residuals: &[f64], center: f64, scale: f64, k: f64, side: Side) -> Result<StatOutcome, PolicyError> {
    let n = residuals.len();
    let all_equal = match residuals.first() {
        Some(&r0) => residuals.iter().all(|&r| r == r0),
        None => true,
    };
    if all_equal || scale == 0.0 {
        let mask = ExclusionMask::none(n)?;
        return Ok(StatOutcome { mask, status: MethodStatus::ZeroScale, center, scale: 0.0, threshold: 0.0 });
    }
    let threshold = finite("filter.stat.threshold", k * scale, None)?;
    let beyond = |r: f64| {
        let x = r - center;
        match side {
            Side::Both => x.abs() > threshold,
            Side::Positive => x > threshold,
            Side::Negative => x < -threshold,
        }
    };
    let count = residuals.iter().filter(|&&r| beyond(r)).count();
    let mut indices = Vec::new();
    indices.try_reserve_exact(count).map_err(|_| PolicyError::Alloc { op: "stat.mask" })?;
    // At most MAX_POINTS (2^24) residuals, so every index fits in u32.
    for (i, &r) in (0u32..).zip(residuals) {
        if beyond(r) {
            indices.push(i);
        }
    }
    let mask = ExclusionMask::new(n, indices)?;
    Ok(StatOutcome { mask, status: MethodStatus::Decided, center, scale, threshold })
}
