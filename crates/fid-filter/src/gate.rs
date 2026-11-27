//! Distance-to-nominal and tolerance-band policies. Spec node `KRN.OUT.GATE`.
//!
//! Both methods decide by where a point lies relative to the nominal, not by residual
//! statistics. The signed distance of each point to the nominal surface is supplied by the
//! caller (`GEO.DIST`, `SCAN`), one per point in point order, in metres; this module never
//! evaluates CAD.
//!
//! - [`NominalGate`] (`filter.outlier.nominal_gate@1.0.0`) excludes each point whose absolute
//!   signed distance is greater than `max_distance_m` (REQ-KRN.OUT.GATE-001). A point exactly
//!   at the gate is kept.
//! - [`ToleranceBand`] (`filter.outlier.tolerance_band@1.0.0`) keeps exactly the points with
//!   `lower_m <= d <= upper_m` and excludes all others (REQ-KRN.OUT.GATE-002). Both limits
//!   are inclusive.
//!
//! # Errors (pinned order)
//!
//! [`NominalGate::new`] and [`ToleranceBand::new`] validate with the `KRN.OUT.POL` rules, so
//! a method value always holds valid parameters: `max_distance_m` must be a positive normal
//! finite number; the band limits finite, not subnormal, not `-0.0`, and `lower_m > upper_m`
//! is `InvalidParameter { field: "lower_m", reason: "above_upper" }` (REQ-KRN.OUT.GATE-003).
//!
//! [`PolicyMethod::exclude`] then checks, in this order:
//!
//! 1. nominal distances are given, else [`PolicyError::NominalDistances`] `{ required: true }`;
//! 2. one distance per point, else [`PolicyError::LengthMismatch`] `nominal_distances`;
//! 3. at most `fid_fit::contract::MAX_POINTS` distances, else
//!    `InvalidParameter { field: "point_count", reason: "out_of_range" }`;
//! 4. every distance finite, else [`PolicyError::Num`] (KNP-1: NaN is an error).
//!
//! The residuals and the fitted feature are not read: the decision depends on the nominal
//! distances only. [`crate::policy::apply`] still checks the residuals, runs the guards and
//! records the exclusion, so a caller reduces a point set only through it.
//!
//! Comparisons and `f64::abs` only (exact): no reduction, no fused multiply-add, so every mask
//! is D0, bit-identical on every target.

use fid_fit::contract::{FittedFeature, PointsView, MAX_POINTS};
use fid_num::check_finite_slice;

use crate::policy::{ExclusionMask, OutlierPolicy, PolicyError, PolicyMethod};

/// The semantic version of both methods, the `<semver>` of their algorithm IDs.
pub const VERSION: (u32, u32, u32) = (1, 0, 0);

/// Excludes points farther than `max_distance_m` from nominal, in either direction
/// (REQ-KRN.OUT.GATE-001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NominalGate {
    max_distance_m: f64,
}

impl NominalGate {
    /// A gate of `max_distance_m` metres, validated with the `KRN.OUT.POL` rules.
    pub fn new(max_distance_m: f64) -> Result<NominalGate, PolicyError> {
        OutlierPolicy::NominalGate { max_distance_m }.validate_fields()?;
        Ok(NominalGate { max_distance_m })
    }

    /// The gate, metres.
    pub fn max_distance_m(&self) -> f64 {
        self.max_distance_m
    }

    /// The mask over `nominal_dist.len()` points: every index whose `|d| > max_distance_m`.
    /// Steps 3 and 4 of the pinned error order apply.
    pub fn mask(&self, nominal_dist: &[f64]) -> Result<ExclusionMask, PolicyError> {
        let gate = self.max_distance_m;
        select(nominal_dist, "gate.mask", |d| d.abs() > gate)
    }
}

impl PolicyMethod for NominalGate {
    fn policy(&self) -> OutlierPolicy {
        OutlierPolicy::NominalGate { max_distance_m: self.max_distance_m }
    }

    fn version(&self) -> (u32, u32, u32) {
        VERSION
    }

    fn exclude(
        &self,
        pts: &PointsView<'_>,
        _fit: &FittedFeature,
        _residuals: &[f64],
        nominal_dist: Option<&[f64]>,
    ) -> Result<ExclusionMask, PolicyError> {
        self.mask(per_point(pts, nominal_dist)?)
    }
}

/// Keeps exactly the points whose signed nominal distance lies in `[lower_m, upper_m]`
/// (REQ-KRN.OUT.GATE-002).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToleranceBand {
    lower_m: f64,
    upper_m: f64,
}

impl ToleranceBand {
    /// The band `[lower_m, upper_m]` metres, validated with the `KRN.OUT.POL` rules
    /// (`lower_m > upper_m` is refused, REQ-KRN.OUT.GATE-003).
    pub fn new(lower_m: f64, upper_m: f64) -> Result<ToleranceBand, PolicyError> {
        OutlierPolicy::ToleranceBand { lower_m, upper_m }.validate_fields()?;
        Ok(ToleranceBand { lower_m, upper_m })
    }

    /// The lower limit, metres (inclusive).
    pub fn lower_m(&self) -> f64 {
        self.lower_m
    }

    /// The upper limit, metres (inclusive).
    pub fn upper_m(&self) -> f64 {
        self.upper_m
    }

    /// The mask over `nominal_dist.len()` points: every index whose distance is outside the
    /// band. Steps 3 and 4 of the pinned error order apply.
    pub fn mask(&self, nominal_dist: &[f64]) -> Result<ExclusionMask, PolicyError> {
        let (lower, upper) = (self.lower_m, self.upper_m);
        select(nominal_dist, "band.mask", |d| d < lower || d > upper)
    }
}

impl PolicyMethod for ToleranceBand {
    fn policy(&self) -> OutlierPolicy {
        OutlierPolicy::ToleranceBand { lower_m: self.lower_m, upper_m: self.upper_m }
    }

    fn version(&self) -> (u32, u32, u32) {
        VERSION
    }

    fn exclude(
        &self,
        pts: &PointsView<'_>,
        _fit: &FittedFeature,
        _residuals: &[f64],
        nominal_dist: Option<&[f64]>,
    ) -> Result<ExclusionMask, PolicyError> {
        self.mask(per_point(pts, nominal_dist)?)
    }
}

/// Steps 1 and 2 of the pinned error order.
fn per_point<'a>(pts: &PointsView<'_>, nominal_dist: Option<&'a [f64]>) -> Result<&'a [f64], PolicyError> {
    let d = nominal_dist.ok_or(PolicyError::NominalDistances { required: true })?;
    if d.len() != pts.len() {
        return Err(PolicyError::LengthMismatch { what: "nominal_distances", expected: pts.len(), got: d.len() });
    }
    Ok(d)
}

/// Steps 3 and 4, then the ascending indices for which `excluded` holds.
fn select(
    nominal_dist: &[f64],
    op: &'static str,
    excluded: impl Fn(f64) -> bool,
) -> Result<ExclusionMask, PolicyError> {
    if nominal_dist.len() > MAX_POINTS {
        return Err(PolicyError::InvalidParameter { field: "point_count", reason: "out_of_range" });
    }
    check_finite_slice("filter.gate.nominal_distances", nominal_dist)?;
    let count = nominal_dist.iter().filter(|&&d| excluded(d)).count();
    let mut indices = Vec::new();
    indices.try_reserve_exact(count).map_err(|_| PolicyError::Alloc { op })?;
    // At most MAX_POINTS (2^24) distances, so every index fits in u32.
    for (i, &d) in (0u32..).zip(nominal_dist) {
        if excluded(d) {
            indices.push(i);
        }
    }
    ExclusionMask::new(nominal_dist.len(), indices)
}
