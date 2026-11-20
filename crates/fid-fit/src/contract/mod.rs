//! The fit contract. Spec node `KRN.LSQ.TRAIT`.
//!
//! Every fitting method in the kernel implements [`Fit`]: it takes a validated
//! [`PointsView`] (SI metres), explicit [`FitParams`] and an optional seed, and returns a
//! [`FitResult`] or a typed [`FitError`] (REQ-KRN.LSQ.TRAIT-001). The shared pieces live here:
//!
//! - [`PointsView`]: borrowed xyz triples with optional unit normals, refusing NaN,
//!   infinities and subnormal coordinates at construction (REQ-002);
//! - [`Normalisation`] / [`Centred`]: centring on the `red::mean` centroid and isotropic
//!   scaling by a power of two near the RMS distance, with its inverse map (REQ-003);
//! - [`check_points`] / [`check`]: point count, numerical rank (collinear, coplanar) and
//!   angular coverage, under thresholds passed in [`Thresholds`] (REQ-004);
//! - [`ResidualStats`] and [`FitDiagnostics`] (REQ-005, REQ-006);
//! - [`FittedFeature`] with unit, sign-normalised directions (REQ-007);
//! - [`encode`]: the canonical byte form, and [`algorithm_id`] (REQ-008);
//! - [`command_row`]: the data row that declares a `fit.<method>_<primitive>@1` command for
//!   the registry (REQ-009).
//!
//! Every value-affecting choice is explicit: weights, constraints, the rank rule's tolerance,
//! the coverage threshold and the NLLS convergence configuration are fields of
//! [`FitParams`]; the remaining rules (centroid, scale, factorisation order, Jacobi sweep
//! order, sign rule, encoding) are pinned in the documentation of each module. Arithmetic is
//! plain IEEE-754 binary64 with every sum, dot product and norm through `fid_num::red` and every
//! square root, arctangent, minimum and maximum through `fid_num::f` (no FMA, no `f64::min` or
//! `f64::max`), so results are bit-identical on x86-64, ARM64 and wasm32 (D0). Nothing panics;
//! every buffer is reserved fallibly and every loop is bounded.

mod degen;
mod encode;
mod error;
mod normalise;
mod params;
mod points;
mod primitive;
mod rows;
mod stats;

pub use degen::{
    angular_coverage, check, check_points, data_shape, DataShape, Degeneracy, JACOBI_MAX_SWEEPS, JACOBI_TOL,
};
pub use encode::{encode, encode_feature};
pub use error::{FitError, NotConvergedReason};
pub use normalise::{Centred, Normalisation};
pub use params::{
    Constraint, FitParams, Thresholds, Weights, DEFAULT_EIG_RATIO_MAX, DEFAULT_MIN_COVERAGE, DEFAULT_RANK_TOL, TWO_PI,
};
pub use points::{PointsView, MAX_POINTS, UNIT_NORMAL_TOL};
pub use primitive::{
    algorithm_id, foot_of_perpendicular, normalize_direction, normalize_direction2, unit_direction, FittedFeature,
    Method, Primitive, Requirement, V2, V3,
};
pub use rows::{
    command_name, command_row, feature_fields, CommandRow, Field, RowError, ValueType, QUANTITIES, ROW_FORMAT,
};
pub use stats::{Certificate, FitDiagnostics, ResidualStats, StopReason};

pub use fid_kci::AlgorithmId;

/// The result of a successful fit (REQ-KRN.LSQ.TRAIT-001).
#[derive(Debug, Clone, PartialEq)]
pub struct FitResult {
    /// The fitted feature in SI metres, canonical ([`FittedFeature::canonical`]).
    pub feature: FittedFeature,
    /// Signed residual of each point, in point order (metres).
    pub residuals: Vec<f64>,
    pub stats: ResidualStats,
    pub diagnostics: FitDiagnostics,
    pub certificate: Option<Certificate>,
    /// `fit.<primitive>.<method>@<semver>`.
    pub algorithm: AlgorithmId,
}

impl FitResult {
    /// Builds a result: canonicalises the feature and computes the residual statistics.
    /// [`FitError::InvalidParams`] (`algorithm`) when the algorithm ID's feature segment is
    /// not the feature's primitive or its module is not `fit`; [`FitError::Num`] for an empty
    /// or non-finite residual list.
    pub fn assemble(
        feature: &FittedFeature,
        residuals: Vec<f64>,
        diagnostics: FitDiagnostics,
        certificate: Option<Certificate>,
        algorithm: AlgorithmId,
    ) -> Result<FitResult, FitError> {
        if algorithm.module != "fit" || algorithm.feature != feature.primitive().slug() {
            return Err(FitError::InvalidParams { field: "algorithm" });
        }
        let feature = feature.canonical()?;
        let stats = ResidualStats::of(&residuals)?;
        Ok(FitResult { feature, residuals, stats, diagnostics, certificate, algorithm })
    }
}

/// A fitting method (REQ-KRN.LSQ.TRAIT-001).
pub trait Fit {
    /// The algorithm ID `fit.<primitive>.<method>@<semver>`.
    fn algorithm_id(&self) -> AlgorithmId;

    /// Fits `pts` under `params`. `seed` drives any randomised start (multi-start); a
    /// deterministic method ignores it. Implementations validate `params`
    /// ([`FitParams::validate`]), run [`check_points`] for their primitive and work on the
    /// centred, scaled data, mapping the feature back with [`Normalisation::inverse_feature`].
    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, seed: Option<u64>) -> Result<FitResult, FitError>;
}
