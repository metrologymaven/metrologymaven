//! Outlier policy types, validation and guards. Spec node `KRN.OUT.POL`.
//!
//! A policy is a typed operator with explicit parameters that maps per-point evidence
//! (signed residuals, signed distances to nominal) to an [`ExclusionMask`]. This module holds
//! what every method shares:
//!
//! - [`OutlierPolicy`]: one variant per method, every parameter a typed field, lengths in SI
//!   metres (REQ-KRN.OUT.POL-001); [`PolicySpec`] adds the two envelope parameters every
//!   method obeys, `max_excluded_fraction` and the `target` primitive. Nothing has a hidden
//!   default (REQ-KRN.OUT-001): [`DEFAULT_MAX_EXCLUDED_FRACTION`] is documentation for callers
//!   that must still pass it explicitly.
//! - [`PolicySpec::validate`] and the canonical encoding [`PolicySpec::encode`] /
//!   [`PolicySpec::decode`] (module `canon`).
//! - [`PolicyMethod`]: the trait each method implements (REQ-KRN.OUT.POL-002).
//! - [`ExclusionMask`]: sorted, unique, range-checked indices with their digest
//!   (REQ-KRN.OUT.POL-005, REQ-KRN.OUT-002); a repeated index is refused, not merged.
//! - [`check_guards`]: the minimum-points and maximum-fraction guards
//!   (REQ-KRN.OUT.POL-003, -004).
//! - [`apply`]: the only public function in the kernel that takes a [`PointsView`] and returns
//!   a reduced point set (REQ-KRN.OUT.POL-006, REQ-KRN.OUT-004). Its result,
//!   [`ReducedPoints`], cannot be built any other way and always carries its [`Exclusion`]:
//!   the policy's algorithm ID, its canonical parameters, the excluded index set and that
//!   set's digest. A fit computed on the kept points therefore always has the record of what
//!   was removed next to it; excluded points never silently change a result that looks
//!   measured (D-045's "no silent wrong results", REQ-KRN.OUT-002).
//!
//! # Validation (pinned)
//!
//! [`PolicySpec::validate`] checks, in this order, and returns the first failure as
//! [`PolicyError::InvalidParameter`] `{ field, reason }`:
//!
//! 1. the policy's fields in declaration order:
//!    - positive lengths and factors (`k`, `threshold_m`, `max_distance_m`): `non_finite`
//!      (NaN or infinite), then `subnormal` (non-zero subnormal of either sign), then
//!      `out_of_range` unless strictly greater than zero (so `0.0` and `-0.0` are out of range);
//!    - `iterations`: `out_of_range` unless `1 ..= MAX_RANSAC_ITERATIONS`;
//!    - `min_inliers`: `out_of_range` when zero;
//!    - signed band limits (`lower_m`, `upper_m`): `non_finite`, then `subnormal`, then
//!      `negative_zero` (`-0.0` is refused so that equal parameter sets have one encoding);
//!    - `side`, `scale` and `seed` take every value of their type;
//! 2. for a tolerance band, `lower_m > upper_m`: field `lower_m`, reason `above_upper`;
//! 3. `max_excluded_fraction`: `non_finite`, `subnormal`, `negative_zero`, then `out_of_range`
//!    unless `0 <= f < 1` (zero allows no exclusion at all);
//! 4. for RANSAC, `min_inliers` below the target primitive's minimum point count
//!    (`fid_fit::contract::Primitive::requirement`): reason `below_min_points`.
//!
//! Decoding adds the reasons `malformed`, `too_long`, `missing`, `wrong_type`, `bad_float`,
//! `bad_integer`, `unknown_value`, `unexpected_key`, `duplicate_key` and `not_canonical`
//! (see `PolicySpec::decode`).
//!
//! # Guards (pinned)
//!
//! For a mask over `n` points excluding `e` of them, [`check_guards`] returns, in this order:
//!
//! 1. [`PolicyError::TooFewRemaining`] when `n - e` is below the target primitive's minimum
//!    point count (REQ-KRN.OUT.POL-003);
//! 2. [`PolicyError::ExcessiveExclusion`] when `e > 0` and `(e as f64) / (n as f64)`, one
//!    correctly rounded IEEE-754 division of two exactly represented integers, is greater
//!    than `max_excluded_fraction` (REQ-KRN.OUT.POL-004). Exactly at the limit passes.
//!
//! Comparisons only: there is no reduction, no transcendental function and no fused
//! multiply-add anywhere in this module, so every outcome is D0 (bit-identical on every
//! target).

mod canon;
mod error;
mod mask;

pub use canon::MAX_ENCODED_BYTES;
pub use error::PolicyError;
pub use mask::ExclusionMask;

use fid_fit::contract::{AlgorithmId, FittedFeature, PointsView, Primitive};
use fid_num::check_finite_slice;
use fid_types::Digest;

use error::{alloc, invalid};

/// The documented default of `max_excluded_fraction` (REQ-KRN.OUT.POL-004) [unverified]: a
/// placeholder that may move into a defaults profile. Nothing applies it implicitly; callers
/// pass it explicitly in [`PolicySpec`].
pub const DEFAULT_MAX_EXCLUDED_FRACTION: f64 = 0.25;

/// Largest accepted RANSAC `iterations`, so every run is bounded.
pub const MAX_RANSAC_ITERATIONS: u32 = 1_000_000;

/// The first segment of every policy algorithm ID.
pub const MODULE: &str = "filter";
/// The second segment of outlier policy algorithm IDs (`filter.outlier.<method>@<semver>`).
pub const OUTLIER: &str = "outlier";
/// The second segment of point-selection algorithm IDs (`filter.select.<method>@<semver>`).
pub const SELECT: &str = "select";

/// Which residuals a residual-statistics policy may exclude (`KRN.OUT.STAT` REQ-003, which
/// requires the parameter with no default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    /// Both signs.
    Both,
    /// Positive residuals only.
    Positive,
    /// Negative residuals only.
    Negative,
}

impl Side {
    /// Every value, in declaration order.
    pub const ALL: &'static [Side] = &[Side::Both, Side::Positive, Side::Negative];

    /// The encoding slug.
    pub const fn slug(self) -> &'static str {
        match self {
            Side::Both => "both",
            Side::Positive => "positive",
            Side::Negative => "negative",
        }
    }

    /// Parses a slug exactly.
    pub fn parse(s: &str) -> Option<Side> {
        Side::ALL.iter().copied().find(|v| v.slug() == s)
    }
}

/// The robust scale estimator of [`OutlierPolicy::RobustScale`] (`KRN.OUT.STAT` REQ-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScaleEstimator {
    /// 1.4826 times the median absolute deviation.
    Mad,
    /// The Qn estimator of Rousseeuw and Croux (1993).
    Qn,
}

impl ScaleEstimator {
    /// Every value, in declaration order.
    pub const ALL: &'static [ScaleEstimator] = &[ScaleEstimator::Mad, ScaleEstimator::Qn];

    /// The encoding slug.
    pub const fn slug(self) -> &'static str {
        match self {
            ScaleEstimator::Mad => "mad",
            ScaleEstimator::Qn => "qn",
        }
    }

    /// Parses a slug exactly.
    pub fn parse(s: &str) -> Option<ScaleEstimator> {
        ScaleEstimator::ALL.iter().copied().find(|v| v.slug() == s)
    }
}

/// The method of a policy, without its parameters. Its slug is the method segment of the
/// algorithm ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PolicyKind {
    NSigma,
    RobustScale,
    Ransac,
    NominalGate,
    ToleranceBand,
}

impl PolicyKind {
    /// Every value, in declaration order.
    pub const ALL: &'static [PolicyKind] = &[
        PolicyKind::NSigma,
        PolicyKind::RobustScale,
        PolicyKind::Ransac,
        PolicyKind::NominalGate,
        PolicyKind::ToleranceBand,
    ];

    /// The method segment (`[a-z][a-z0-9_]*`).
    pub const fn slug(self) -> &'static str {
        match self {
            PolicyKind::NSigma => "nsigma",
            PolicyKind::RobustScale => "robust_scale",
            PolicyKind::Ransac => "ransac",
            PolicyKind::NominalGate => "nominal_gate",
            PolicyKind::ToleranceBand => "tolerance_band",
        }
    }

    /// Parses a slug exactly.
    pub fn parse(s: &str) -> Option<PolicyKind> {
        PolicyKind::ALL.iter().copied().find(|v| v.slug() == s)
    }

    /// Whether the method needs signed nominal distances (the gate policies).
    pub const fn needs_nominal(self) -> bool {
        matches!(self, PolicyKind::NominalGate | PolicyKind::ToleranceBand)
    }
}

/// An outlier policy with all its parameters (REQ-KRN.OUT.POL-001). Lengths are SI metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutlierPolicy {
    /// Exclude residuals beyond `k` residual standard deviations (`KRN.OUT.STAT`).
    NSigma { k: f64, side: Side },
    /// Exclude residuals beyond `k` robust scales from the median (`KRN.OUT.STAT`).
    RobustScale { k: f64, scale: ScaleEstimator, side: Side },
    /// Seeded RANSAC consensus (`KRN.OUT.RANSAC`): `iterations` rounds, inlier distance
    /// `threshold_m`, at least `min_inliers` in the winning consensus.
    Ransac { threshold_m: f64, iterations: u32, min_inliers: u32, seed: u64 },
    /// Exclude points farther than `max_distance_m` from nominal (`KRN.OUT.GATE`).
    NominalGate { max_distance_m: f64 },
    /// Keep points whose signed nominal distance is in `[lower_m, upper_m]` (`KRN.OUT.GATE`).
    ToleranceBand { lower_m: f64, upper_m: f64 },
}

impl OutlierPolicy {
    /// The method of this policy.
    pub const fn kind(&self) -> PolicyKind {
        match self {
            OutlierPolicy::NSigma { .. } => PolicyKind::NSigma,
            OutlierPolicy::RobustScale { .. } => PolicyKind::RobustScale,
            OutlierPolicy::Ransac { .. } => PolicyKind::Ransac,
            OutlierPolicy::NominalGate { .. } => PolicyKind::NominalGate,
            OutlierPolicy::ToleranceBand { .. } => PolicyKind::ToleranceBand,
        }
    }

    /// Checks the policy's own fields (steps 1 and 2 of the pinned validation order).
    fn validate_fields(&self) -> Result<(), PolicyError> {
        match *self {
            OutlierPolicy::NSigma { k, .. } | OutlierPolicy::RobustScale { k, .. } => positive("k", k),
            OutlierPolicy::Ransac { threshold_m, iterations, min_inliers, .. } => {
                positive("threshold_m", threshold_m)?;
                if iterations == 0 || iterations > MAX_RANSAC_ITERATIONS {
                    return Err(invalid("iterations", "out_of_range"));
                }
                if min_inliers == 0 {
                    return Err(invalid("min_inliers", "out_of_range"));
                }
                Ok(())
            }
            OutlierPolicy::NominalGate { max_distance_m } => positive("max_distance_m", max_distance_m),
            OutlierPolicy::ToleranceBand { lower_m, upper_m } => {
                signed("lower_m", lower_m)?;
                signed("upper_m", upper_m)?;
                if lower_m > upper_m {
                    return Err(invalid("lower_m", "above_upper"));
                }
                Ok(())
            }
        }
    }
}

/// A policy with its envelope: everything that decides an exclusion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolicySpec {
    pub policy: OutlierPolicy,
    /// Largest fraction of the input the policy may exclude, `0 <= f < 1`
    /// (REQ-KRN.OUT.POL-004). No implicit default; see [`DEFAULT_MAX_EXCLUDED_FRACTION`].
    pub max_excluded_fraction: f64,
    /// The primitive that will be fitted to the kept points; its minimum point count is the
    /// floor of the remaining-points guard (REQ-KRN.OUT.POL-003).
    pub target: Primitive,
}

impl PolicySpec {
    /// Validates every parameter in the pinned order (see the [module documentation](self)).
    pub fn validate(&self) -> Result<(), PolicyError> {
        self.policy.validate_fields()?;
        let f = self.max_excluded_fraction;
        finite_normal("max_excluded_fraction", f)?;
        if is_negative_zero(f) {
            return Err(invalid("max_excluded_fraction", "negative_zero"));
        }
        if !(0.0..1.0).contains(&f) {
            return Err(invalid("max_excluded_fraction", "out_of_range"));
        }
        if let OutlierPolicy::Ransac { min_inliers, .. } = self.policy {
            if (min_inliers as usize) < self.target.requirement().min_points {
                return Err(invalid("min_inliers", "below_min_points"));
            }
        }
        Ok(())
    }

    /// The canonical byte form (pinned layout in `canon`); validates first, so only a valid
    /// spec has an encoding.
    pub fn encode(&self) -> Result<Vec<u8>, PolicyError> {
        self.validate()?;
        canon::encode(self)
    }

    /// Parses and validates a canonical byte form. Anything that is not exactly the encoding
    /// of a valid spec is refused, so `decode(b)` succeeds only when `encode(decode(b)) == b`.
    pub fn decode(bytes: &[u8]) -> Result<PolicySpec, PolicyError> {
        canon::decode(bytes)
    }

    /// The outlier algorithm ID of this spec's method at `version`.
    pub fn algorithm_id(&self, version: (u32, u32, u32)) -> AlgorithmId {
        outlier_algorithm_id(self.policy.kind(), version)
    }
}

fn is_negative_zero(x: f64) -> bool {
    x == 0.0 && x.is_sign_negative()
}

fn finite_normal(field: &'static str, x: f64) -> Result<(), PolicyError> {
    if !x.is_finite() {
        return Err(invalid(field, "non_finite"));
    }
    if x.is_subnormal() {
        return Err(invalid(field, "subnormal"));
    }
    Ok(())
}

fn positive(field: &'static str, x: f64) -> Result<(), PolicyError> {
    finite_normal(field, x)?;
    if x > 0.0 {
        Ok(())
    } else {
        Err(invalid(field, "out_of_range"))
    }
}

fn signed(field: &'static str, x: f64) -> Result<(), PolicyError> {
    finite_normal(field, x)?;
    if is_negative_zero(x) {
        return Err(invalid(field, "negative_zero"));
    }
    Ok(())
}

/// `filter.outlier.<method>@<semver>` for a policy method.
pub fn outlier_algorithm_id(kind: PolicyKind, version: (u32, u32, u32)) -> AlgorithmId {
    AlgorithmId { module: MODULE.to_string(), feature: OUTLIER.to_string(), method: kind.slug().to_string(), version }
}

/// `filter.select.<method>@<semver>` for a point-selection method (`KRN.OUT.SEL`).
/// `InvalidParameter { field: "method", reason: "bad_segment" }` unless `method` is an ID
/// segment (`[a-z][a-z0-9_]*`, the rule of `fid_kci::AlgorithmId::parse`).
pub fn select_algorithm_id(method: &str, version: (u32, u32, u32)) -> Result<AlgorithmId, PolicyError> {
    let (a, b, c) = version;
    let text = format!("{MODULE}.{SELECT}.{method}@{a}.{b}.{c}");
    let id = AlgorithmId::parse(&text).map_err(|_| invalid("method", "bad_segment"))?;
    if id.method != method {
        return Err(invalid("method", "bad_segment"));
    }
    Ok(id)
}

/// A method that turns per-point evidence into an exclusion mask (REQ-KRN.OUT.POL-002).
/// Implemented by `KRN.OUT.STAT`, `KRN.OUT.RANSAC` and `KRN.OUT.GATE`. Callers never use
/// [`PolicyMethod::exclude`] to reduce a point set themselves: [`apply`] runs it under the
/// guards and returns the reduced set with its record.
pub trait PolicyMethod {
    /// The policy, with the parameters this method runs with.
    fn policy(&self) -> OutlierPolicy;

    /// The method's semantic version, the `<semver>` of its algorithm ID.
    fn version(&self) -> (u32, u32, u32);

    /// `filter.outlier.<method>@<semver>`.
    fn algorithm_id(&self) -> AlgorithmId {
        outlier_algorithm_id(self.policy().kind(), self.version())
    }

    /// The points to exclude, given the input `pts`, the feature fitted to all of them,
    /// the signed residual of each point (in point order, metres) and, for the gate
    /// policies, the signed distance of each point to nominal. The mask's point count is
    /// `pts.len()`.
    fn exclude(
        &self,
        pts: &PointsView<'_>,
        fit: &FittedFeature,
        residuals: &[f64],
        nominal_dist: Option<&[f64]>,
    ) -> Result<ExclusionMask, PolicyError>;
}

/// The minimum-points and maximum-fraction guards, in the pinned order (see the
/// [module documentation](self)). Validates `spec` first.
pub fn check_guards(spec: &PolicySpec, mask: &ExclusionMask) -> Result<(), PolicyError> {
    spec.validate()?;
    let required = spec.target.requirement().min_points;
    let remaining = mask.remaining();
    if remaining < required {
        return Err(PolicyError::TooFewRemaining { remaining, required });
    }
    if !mask.is_empty() {
        // Both counts are at most MAX_POINTS (2^24), so the conversions are exact.
        let fraction = mask.len() as f64 / mask.point_count() as f64;
        if fraction > spec.max_excluded_fraction {
            return Err(PolicyError::ExcessiveExclusion { fraction, limit: spec.max_excluded_fraction });
        }
    }
    Ok(())
}

/// What a policy removed from one input set: the record that travels with every reduced
/// point set (REQ-KRN.OUT-002). `KRN.OUT.DISC` builds the full `ExclusionRecord` (with the
/// impact disclosure) from it.
#[derive(Debug, Clone, PartialEq)]
pub struct Exclusion {
    algorithm: AlgorithmId,
    params: Vec<u8>,
    mask: ExclusionMask,
    digest: Digest,
}

impl Exclusion {
    /// The policy's algorithm ID, `filter.outlier.<method>@<semver>`.
    pub fn algorithm(&self) -> &AlgorithmId {
        &self.algorithm
    }

    /// The canonical encoding of the [`PolicySpec`] that decided the exclusion.
    pub fn params(&self) -> &[u8] {
        &self.params
    }

    /// The excluded index set.
    pub fn mask(&self) -> &ExclusionMask {
        &self.mask
    }

    /// The excluded indices, ascending.
    pub fn excluded(&self) -> &[u32] {
        self.mask.indices()
    }

    /// Number of excluded points (zero is recorded too).
    pub fn excluded_count(&self) -> usize {
        self.mask.len()
    }

    /// Number of points of the input set.
    pub fn point_count(&self) -> usize {
        self.mask.point_count()
    }

    /// The digest of the excluded set ([`ExclusionMask::digest`]).
    pub fn digest(&self) -> &Digest {
        &self.digest
    }
}

/// The kept points of an input set together with the record of what was excluded. Only
/// [`apply`] builds one, and the points cannot be taken out without the record: a fit run on
/// [`ReducedPoints::view`] has [`ReducedPoints::exclusion`] beside it.
#[derive(Debug, Clone, PartialEq)]
#[must_use = "the reduced point set carries the record of the excluded points"]
pub struct ReducedPoints {
    exclusion: Exclusion,
    kept: Vec<u32>,
    xyz: Vec<f64>,
    normals: Option<Vec<f64>>,
}

impl ReducedPoints {
    /// The record of the exclusion.
    pub fn exclusion(&self) -> &Exclusion {
        &self.exclusion
    }

    /// The original index of each kept point, ascending (kept points stay in input order).
    pub fn kept_indices(&self) -> &[u32] {
        &self.kept
    }

    /// Whether at least one point was excluded.
    pub fn is_reduced(&self) -> bool {
        !self.exclusion.mask.is_empty()
    }

    /// A view over the kept points (and their normals, if the input had them).
    pub fn view(&self) -> Result<PointsView<'_>, PolicyError> {
        PointsView::new(&self.xyz, self.normals.as_deref()).map_err(|e| invalid("points", e.code()))
    }
}

/// The single entry point through which a fit's input set is reduced (REQ-KRN.OUT.POL-006,
/// REQ-KRN.OUT-004). Steps, in order, each returning its error:
///
/// 1. `spec.validate()`;
/// 2. `method.policy()` must encode to the same bytes as `spec.policy` (field `policy`,
///    reason `method_mismatch`), and `method.algorithm_id()` must be
///    `filter.outlier.<spec's method>@..` (field `algorithm`, reason `method_mismatch`);
/// 3. `nominal_dist` is given exactly when the spec's method needs it
///    ([`PolicyKind::needs_nominal`]), else [`PolicyError::NominalDistances`] `{ required }`:
///    a gate method never runs without its evidence, and other methods are never handed
///    evidence they would silently ignore;
/// 4. one residual per point ([`PolicyError::LengthMismatch`] `residuals`), every residual
///    finite ([`PolicyError::Num`]); the same for `nominal_dist` when given
///    (`nominal_distances`);
/// 5. `method.exclude(..)`, whose mask must cover `pts.len()` points (`LengthMismatch` `mask`);
/// 6. [`check_guards`];
/// 7. the kept points are copied in input order and returned with their [`Exclusion`].
pub fn apply(
    spec: &PolicySpec,
    method: &dyn PolicyMethod,
    pts: &PointsView<'_>,
    fit: &FittedFeature,
    residuals: &[f64],
    nominal_dist: Option<&[f64]>,
) -> Result<ReducedPoints, PolicyError> {
    let params = spec.encode()?;
    let declared = PolicySpec { policy: method.policy(), ..*spec };
    if declared.encode().ok().as_deref() != Some(params.as_slice()) {
        return Err(invalid("policy", "method_mismatch"));
    }
    let algorithm = method.algorithm_id();
    if algorithm.module != MODULE || algorithm.feature != OUTLIER || algorithm.method != spec.policy.kind().slug() {
        return Err(invalid("algorithm", "method_mismatch"));
    }
    let required = spec.policy.kind().needs_nominal();
    if nominal_dist.is_some() != required {
        return Err(PolicyError::NominalDistances { required });
    }
    let n = pts.len();
    if residuals.len() != n {
        return Err(PolicyError::LengthMismatch { what: "residuals", expected: n, got: residuals.len() });
    }
    check_finite_slice("filter.policy.residuals", residuals)?;
    if let Some(d) = nominal_dist {
        if d.len() != n {
            return Err(PolicyError::LengthMismatch { what: "nominal_distances", expected: n, got: d.len() });
        }
        check_finite_slice("filter.policy.nominal_distances", d)?;
    }
    let mask = method.exclude(pts, fit, residuals, nominal_dist)?;
    if mask.point_count() != n {
        return Err(PolicyError::LengthMismatch { what: "mask", expected: n, got: mask.point_count() });
    }
    check_guards(spec, &mask)?;
    let keep = mask.remaining();
    let mut kept = Vec::new();
    kept.try_reserve_exact(keep).map_err(|_| alloc("policy.apply"))?;
    let mut xyz = Vec::new();
    xyz.try_reserve_exact(keep.saturating_mul(3)).map_err(|_| alloc("policy.apply"))?;
    let mut normals = match pts.normals() {
        Some(_) => {
            let mut v = Vec::new();
            v.try_reserve_exact(keep.saturating_mul(3)).map_err(|_| alloc("policy.apply"))?;
            Some(v)
        }
        None => None,
    };
    let mut excluded = mask.indices().iter().peekable();
    for (i, p) in pts.xyz().chunks_exact(3).enumerate() {
        let i = u32::try_from(i).map_err(|_| invalid("point_count", "out_of_range"))?;
        if excluded.peek() == Some(&&i) {
            excluded.next();
            continue;
        }
        kept.push(i);
        xyz.extend_from_slice(p);
        if let (Some(out), Some(src)) = (normals.as_mut(), pts.normal(i as usize)) {
            out.extend_from_slice(&src);
        }
    }
    let digest = mask.digest();
    Ok(ReducedPoints { exclusion: Exclusion { algorithm, params, mask, digest }, kept, xyz, normals })
}
