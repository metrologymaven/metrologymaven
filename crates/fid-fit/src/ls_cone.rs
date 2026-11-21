//! Least-squares cone. Spec node `KRN.LSQ.AXIS.CONE`.
//!
//! [`ConeLs`] implements [`Fit`] for `fit.cone.ls@1.0.0`, the orthogonal-distance least-squares
//! cone: it minimises `sum_i e_i^2` with `e_i` the signed distance of point `i` from the cone's
//! generator line in its meridian plane. [`fit_cone`] returns the same result with the
//! multi-start record and the cone's own report ([`ConeFit`]), under explicit [`ConeOptions`]
//! (fixed half-angle, ill-posedness thresholds, apex report limit, condition limit);
//! [`fit_cone_from`] runs it from a caller-supplied [`AxialStart`] (the `StartSpec` override of
//! the command).
//!
//! # Pinned procedure
//!
//! 1. **Parameters.** [`FitParams::validate`]; the method must be least squares, the weights
//!    uniform, and there must be no contract constraint ([`FitError::InvalidParams`] `method` /
//!    `weights` / `constraints`): the cone's only constraint, a fixed half-angle, is
//!    [`ConeOptions::fixed_half_angle`]. [`ConeOptions::validate`] checks the options.
//! 2. **Checks and data.** [`check_points`] for the cone under `params.thresholds` (so 5 points
//!    are `TooFewPoints` need 6, coplanar data `Coplanar` with its condition number); the fit
//!    runs on the centred, scaled data `q_i` of [`Centred`].
//! 3. **Starts.** [`init::cone_start`] with the caller's seed (or [`DEFAULT_SEED`]): its frame
//!    `(P0, A, E1, E2)` (`A` points from the apex into the cone, `P0` at the data's mid-height)
//!    and its `StartSpec` `[a, b, u, v, r, alpha]`, run by `fid_solv::mstart::run`. With a fixed
//!    half-angle the `alpha` entry is dropped from the spec (primary, half widths, tolerances),
//!    which changes the random starts (they depend on the parameter count). There is no apex
//!    parameter (REQ-001).
//! 4. **Model.** For parameters `(a, b, u, v, r, alpha)` relative to a frame:
//!    `D = A + a E1 + b E2`, `n = red::norm2(D)`, `d = D / n`, `P = P0 + u E1 + v E2` (component
//!    formulas, no FMA); `r` is the radius at `P` and `alpha` the half-angle, so the generator in
//!    the meridian plane is `rho = r + t tan(alpha)`. Per point `w = q_i - P`,
//!    `t = red::dot(w, d)`, `perp = w - t d`, `rho = red::norm2(perp)`, and with
//!    `(s, c) = (f::sin(alpha), f::cos(alpha))` the signed orthogonal distance to that line is
//!    `e_i = (rho - r) c - t s` (positive outside). Analytic Jacobian with
//!    `g1 = red::dot(perp, E1) / rho`, `g2` likewise (both 0 when `rho == 0`):
//!    `de/da = -g1 (t c + rho s) / n`, `de/db` with `g2`, `de/du = -g1 c + s (E1 . d)`,
//!    `de/dv = -g2 c + s (E2 . d)`, `de/dr = -c`, `de/dalpha = -(rho - r) s - t c`; the column
//!    of a fixed half-angle is left out.
//! 5. **Solve and re-parameterisation.** `fid_solv::nlls::solve` under `params.nlls` from the
//!    start. The converged cone is then put in canonical form: `alpha` is brought into
//!    `(-pi/2, pi/2]` by at most one shift of `pi` (which describes the same surface with every
//!    residual negated; more than one is a numeric error), then `alpha < 0` is turned into
//!    `alpha > 0` by reversing `d` (the same surface, the same residuals), so `d` points from
//!    the apex into the cone. The axis becomes the new frame: `A' = d`, `P0'` the foot of the
//!    perpendicular from the centroid (the origin) onto the axis, the radius moved there
//!    (`r' = r + s tan(alpha)` with `s` the signed shift along `d`), `(E1', E2')` by the basis
//!    rule of `KRN.LSQ.INIT` step 3, and the solve is repeated from `a = b = u = v = 0`. This
//!    stops after a round with no accepted step, or after [`REPARAM_ROUNDS`] rounds; a later
//!    round's ill-conditioned or rank-deficient solve fails the start with
//!    [`FitError::IllPosedCone`]; any other later-round solver error (not a numeric one) keeps
//!    the last converged round, as in `ls_cyl`.
//!    Residuals of a round whose `alpha` was shifted by `pi` are negated so that they are the
//!    canonical cone's.
//! 6. **Comparable parameters.** Each start's outcome is reported to `mstart` as its
//!    parameters in the start frame, in SI units: with `d` reversed and `alpha` negated when
//!    `d . A < 0` (so the parameters stay continuous through a flip), `a = (d . E1) / (d . A)`,
//!    `b = (d . E2) / (d . A)`, the axis point `X` where the axis meets the plane through `P0`
//!    normal to `A` as `u`, `v` (metres), the radius at `X` (metres) and `alpha`. A start whose
//!    canonical radius at the foot is not positive (the centroid's foot at or beyond the apex)
//!    fails with a numeric `nonpositive_reference_radius`. Selection, agreement and
//!    `local_optimum_only` are `mstart`'s rules with INIT's tolerances.
//! 7. **Ill-posedness (REQ-003).** For the selected cone, with `cond` its scaled-Jacobian
//!    condition estimate: `alpha <= near_zero` (near-cylinder), `alpha >= pi/2 - near_ninety`
//!    (near-plane) or `cond > cond_limit` (a short axial extent or a narrow band: tilting and
//!    shifting the axis become indistinguishable) is [`FitError::IllPosedCone`] with `alpha` and
//!    `cond`. No ill-posed cone is ever returned.
//! 8. **Result.** The canonical cone: reference point `p` (the foot of the centroid), `d`, `r`
//!    there and `alpha`, mapped back to metres; the contract's feature
//!    `FittedFeature::Cone { apex, d, half_angle }` with `apex = p - (r / tan(alpha)) d`
//!    (computed only for output) is canonicalised by [`FitResult::assemble`]; residuals in metres.
//!    [`ConeFit::apex`] reports the apex only when its distance `r / tan(alpha)` from `p` is at
//!    most [`ConeOptions::apex_report_limit_m`] (REQ-005).
//! 9. **Wrong-axis check.** As `ls_cyl` rule 8: every plane is a limit of cones (with a fixed
//!    half-angle too: a cone far from its apex is locally a plane of any orientation), so a fit
//!    with `rms >= (1 + CERT_MARGIN) plane_rms` is not the global optimum, or none exists:
//!    [`FitError::ImplausibleFit`]; `rms / plane_rms > SUSPECT_RATIO` sets
//!    [`ConeFit::suspect_axis`] (the constants are `ls_cyl`'s).
//! 10. **Trait (fail closed).** [`Fit::fit`] runs with [`ConeOptions::default`] and returns
//!     only clean fits: `local_optimum_only` or `suspect_axis` is [`FitError::FlaggedFit`], and
//!     then a weak axis (rule 11) is [`FitError::IllPosedCone`] (code `ill_conditioned`).
//! 11. **Uncertainty.** [`ConeFit::uncertainty`] holds the standard uncertainties
//!     `sigma * sqrt(diag((J^T J)^-1))` of the axis tilts, the reference point across the axis,
//!     the radius and the half-angle about the final cone (see `uncertainty`). The condition
//!     limit of rule 7 is a numeric guard only; a well-conditioned fit can still have a weak
//!     axis when the data is noisy or short, so the axis uncertainty above
//!     [`ConeOptions::max_axis_u_rad`] sets [`ConeFit::weak_axis`] and the trait refuses it, so
//!     a weak axis never silently becomes a datum. [`fit_cone`] returns it with the flag.
//!
//! # Determinism
//!
//! Every sum, dot product and norm is a `fid_num::red` reduction; square roots, sines, cosines
//! and tangents go through `fid_num::f`; comparisons use total order; there is no FMA and no hash
//! container, and every loop is bounded. The result is bit-identical on x86-64, ARM64 and wasm32
//! for a given seed (D0). Nothing panics.

use core::cmp::Ordering;
use core::f64::consts::{FRAC_PI_2, PI};

use fid_kci::AlgorithmId;
use fid_num::la::{lstsq, Mat};
use fid_num::{f, red, NumError};
use fid_solv::mstart::{self, MultiStartError, StartError, StartOrigin, StartSpec, StartSummary};
use fid_solv::nlls::{self, NllsConfig, NllsProblem, NllsResult};
use fid_types::Determinism;

use crate::contract::{
    algorithm_id, check_points, DEFAULT_RANK_TOL, command_row, data_shape, unit_direction, Centred, CommandRow, Field, Fit,
    FitDiagnostics, FitError, FitParams, FitResult, FittedFeature, Method, PointsView, Primitive, ValueType, Weights,
    V3,
};
use crate::init::{self, AxialStart, AxisFrame};
use crate::ls_cyl::{CERT_MARGIN, SUSPECT_RATIO};

/// Algorithm version of `fit.cone.ls`.
pub const VERSION: (u32, u32, u32) = (1, 0, 0);

/// The seed used when `seed` is `None`.
pub const DEFAULT_SEED: u64 = 0;

/// Most solve rounds per start (rule 5).
pub const REPARAM_ROUNDS: usize = 8;

/// Default near-cylinder threshold: 0.1 degree in radians (AC-3's value) [unverified].
pub const DEFAULT_NEAR_ZERO: f64 = 0.1 * PI / 180.0;

/// Default near-plane threshold: 0.1 degree in radians (AC-3's value) [unverified].
pub const DEFAULT_NEAR_NINETY: f64 = 0.1 * PI / 180.0;

/// Default apex report limit, metres [unverified].
pub const DEFAULT_APEX_REPORT_LIMIT_M: f64 = 1.0;

/// Default limit on the scaled-Jacobian condition estimate of the selected cone (rule 7)
/// [unverified]. The estimate grows like `r tan(alpha) / h` for an axial extent `h`: the
/// well-posed fixture cones (90 to 360 degrees of coverage, 0.5 to 1.5 radii long) stay below
/// 70, a 30 degree band `1e-3` radii long is about 650.
pub const DEFAULT_COND_LIMIT: f64 = 300.0;

/// Default largest accepted standard uncertainty of the axis direction through the `Fit` trait
/// (radians, rule 11): 1 mrad [unverified].
pub const DEFAULT_MAX_AXIS_U_RAD: f64 = 1e-3;

const OP: &str = "fit.ls_cone";

/// The least-squares cone `fit.cone.ls@1.0.0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConeLs;

/// The cone's own options (the command's parameters beyond the contract's `FitParams`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConeOptions {
    /// A fixed half-angle (radians, in `(0, pi/2)`), or `None` when the half-angle is free.
    pub fixed_half_angle: Option<f64>,
    /// Near-cylinder threshold (radians, finite, not negative): a fitted half-angle at or below
    /// it is [`FitError::IllPosedCone`].
    pub near_zero: f64,
    /// Near-plane threshold (radians, finite, not negative): a fitted half-angle at or above
    /// `pi/2 - near_ninety` is [`FitError::IllPosedCone`]. `near_zero + near_ninety < pi/2`.
    pub near_ninety: f64,
    /// Largest apex distance from the reference point that is reported (metres, not negative;
    /// `+inf` reports every apex).
    pub apex_report_limit_m: f64,
    /// Largest accepted scaled-Jacobian condition estimate of the selected cone (finite, at
    /// least 1).
    pub cond_limit: f64,
    /// Largest accepted standard uncertainty of the axis direction (radians, positive; `+inf`
    /// accepts every axis). Above it [`ConeFit::weak_axis`] is set and the `Fit` trait refuses
    /// the fit with [`FitError::IllPosedCone`] (rule 11) [unverified].
    pub max_axis_u_rad: f64,
}

impl Default for ConeOptions {
    fn default() -> Self {
        ConeOptions {
            fixed_half_angle: None,
            near_zero: DEFAULT_NEAR_ZERO,
            near_ninety: DEFAULT_NEAR_NINETY,
            apex_report_limit_m: DEFAULT_APEX_REPORT_LIMIT_M,
            cond_limit: DEFAULT_COND_LIMIT,
            max_axis_u_rad: DEFAULT_MAX_AXIS_U_RAD,
        }
    }
}

impl ConeOptions {
    /// Checks every field ([`FitError::InvalidParams`] naming it).
    ///
    /// # Errors
    ///
    /// `InvalidParams` with field `cone.fixed_half_angle`, `cone.near_zero`, `cone.near_ninety`,
    /// `cone.thresholds` (their sum not below `pi/2`), `cone.apex_report_limit_m` or
    /// `cone.cond_limit` or `cone.max_axis_u_rad`.
    pub fn validate(&self) -> Result<(), FitError> {
        let bad = |field| Err(FitError::InvalidParams { field });
        if let Some(h) = self.fixed_half_angle {
            if !(h.is_finite() && h > 0.0 && h < FRAC_PI_2) {
                return bad("cone.fixed_half_angle");
            }
        }
        if !(self.near_zero.is_finite() && self.near_zero >= 0.0) {
            return bad("cone.near_zero");
        }
        if !(self.near_ninety.is_finite() && self.near_ninety >= 0.0) {
            return bad("cone.near_ninety");
        }
        if self.near_zero + self.near_ninety >= FRAC_PI_2 {
            return bad("cone.thresholds");
        }
        if self.apex_report_limit_m.is_nan() || self.apex_report_limit_m < 0.0 {
            return bad("cone.apex_report_limit_m");
        }
        if !(self.cond_limit.is_finite() && self.cond_limit >= 1.0) {
            return bad("cone.cond_limit");
        }
        if self.max_axis_u_rad.is_nan() || self.max_axis_u_rad <= 0.0 {
            return bad("cone.max_axis_u_rad");
        }
        Ok(())
    }
}

/// A cone fit with its multi-start record and the cone's report (REQ-002, -005).
#[derive(Debug, Clone, PartialEq)]
pub struct ConeFit {
    /// The contract result; its feature is `Cone { apex, d, half_angle }` (the contract's form,
    /// whose apex is always present: see [`ConeFit::apex`] for the reported one).
    pub result: FitResult,
    /// The reference point on the axis: the foot of the perpendicular from the centroid (metres).
    pub point: V3,
    /// The unit axis direction, from the apex into the cone.
    pub axis: V3,
    /// The radius at `point` (metres).
    pub radius: f64,
    /// The half-angle (radians, in `(0, pi/2)`).
    pub half_angle: f64,
    /// Distance from `point` to the apex along the axis, `radius / tan(half_angle)` (metres).
    pub apex_distance: f64,
    /// The apex (metres), only when `apex_distance <= apex_report_limit_m` (REQ-005).
    pub apex: Option<V3>,
    /// The options the fit ran under.
    pub options: ConeOptions,
    /// The frame the start and outcome parameters refer to (INIT's, SI metres).
    pub frame: AxisFrame,
    /// Parameter names of `starts` and of each converged outcome, in order (`a, b, u, v, r`
    /// and, unless fixed, `alpha`).
    pub param_names: Vec<&'static str>,
    /// The starts, in index order (SI: tangents, metres and radians).
    pub starts: Vec<Vec<f64>>,
    /// One outcome per start, in index order: objective and parameters, or the start's error.
    pub per_start: Vec<StartSummary<FitError>>,
    /// The selected start.
    pub best_index: u32,
    /// The converged starts disagree beyond the tolerances (REQ-002).
    pub local_optimum_only: bool,
    /// Seed and stream of the starts.
    pub origin: StartOrigin,
    /// RMS distance of the points to their least-squares plane (metres, rule 9): the bound.
    pub plane_rms: f64,
    /// `result.stats.rms / plane_rms` (`+inf` when `plane_rms` is 0).
    pub rms_ratio: f64,
    /// `rms_ratio > SUSPECT_RATIO`: the axis is weakly determined or wrong (rule 9).
    pub suspect_axis: bool,
    /// Standard uncertainties of the fit (rule 11).
    pub uncertainty: ConeUncertainty,
    /// `uncertainty.axis_rad > options.max_axis_u_rad` (rule 11): the `Fit` trait refuses it.
    pub weak_axis: bool,
}

/// Standard uncertainties `sigma * sqrt(diag((J^T J)^-1))` of the parameters about the final
/// cone (rule 11), `sigma^2 = sum e_i^2 / (m - n)` the residual variance. They describe the
/// data's noise propagated through the linearised fit, not the measurement uncertainty.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConeUncertainty {
    /// The unit vectors the axis tilts and the point shifts are measured along (a right-handed
    /// frame `(e1, e2, axis)` by `KRN.LSQ.INIT`'s basis rule).
    pub e1: V3,
    pub e2: V3,
    /// Residual standard deviation `sigma` (metres).
    pub sigma_m: f64,
    /// Axis tilt towards `e1` and towards `e2` (radians).
    pub axis_tilt_rad: [f64; 2],
    /// `sqrt(tilt_1^2 + tilt_2^2)`: the axis direction's standard uncertainty (radians).
    pub axis_rad: f64,
    /// The reference point across the axis, along `e1` and `e2` (metres).
    pub point_m: [f64; 2],
    /// The radius at the reference point (metres).
    pub radius_m: f64,
    /// The half-angle (radians); `None` when it is fixed.
    pub half_angle_rad: Option<f64>,
}

fn alloc<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    Ok(v)
}

fn id() -> AlgorithmId {
    // Least squares is defined for the cone, so this cannot fail; the fallback keeps the
    // function total without a panic path.
    algorithm_id(Primitive::Cone, Method::LeastSquares, VERSION).unwrap_or_else(|_| AlgorithmId {
        module: "fit".to_string(),
        feature: Primitive::Cone.slug().to_string(),
        method: Method::LeastSquares.slug().to_string(),
        version: VERSION,
    })
}

/// The command data row of `fit.ls_cone@1` (W2, dry run R1, D0 per seed): the contract's common
/// row plus the cone's options as parameters and its report, multi-start summary and check
/// fields as results (all optional, written by the command layer).
pub fn cone_command_row() -> Result<CommandRow, FitError> {
    let mut row = command_row(Primitive::Cone, Method::LeastSquares, VERSION, Determinism::D0)?;
    const C: Option<(&str, &str)> = Some(("count", "1"));
    const D: Option<(&str, &str)> = Some(("dimensionless", "1"));
    const L: Option<(&str, &str)> = Some(("length", "m"));
    const A: Option<(&str, &str)> = Some(("angle", "rad"));
    let params = [
        Field { name: "cone.fixed_half_angle", ty: ValueType::Number, quantity: A, required: false },
        Field { name: "cone.near_zero", ty: ValueType::Number, quantity: A, required: false },
        Field { name: "cone.near_ninety", ty: ValueType::Number, quantity: A, required: false },
        Field { name: "cone.apex_report_limit_m", ty: ValueType::Number, quantity: L, required: false },
        Field { name: "cone.cond_limit", ty: ValueType::Number, quantity: D, required: false },
    ];
    let extra = [
        Field { name: "cone.point", ty: ValueType::NumberArray, quantity: L, required: false },
        Field { name: "cone.radius", ty: ValueType::Number, quantity: L, required: false },
        Field { name: "cone.apex_distance", ty: ValueType::Number, quantity: L, required: false },
        Field { name: "cone.apex_reported", ty: ValueType::Boolean, quantity: None, required: false },
        Field { name: "multistart.best_index", ty: ValueType::Integer, quantity: C, required: false },
        Field { name: "multistart.start_count", ty: ValueType::Integer, quantity: C, required: false },
        Field { name: "multistart.local_optimum_only", ty: ValueType::Boolean, quantity: None, required: false },
        Field { name: "check.plane_rms", ty: ValueType::Number, quantity: L, required: false },
        Field { name: "check.rms_ratio", ty: ValueType::Number, quantity: D, required: false },
        Field { name: "check.suspect_axis", ty: ValueType::Boolean, quantity: None, required: false },
    ];
    let oom = |_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" });
    row.params.try_reserve_exact(params.len()).map_err(oom)?;
    row.params.extend_from_slice(&params);
    row.result.try_reserve_exact(extra.len()).map_err(oom)?;
    row.result.extend_from_slice(&extra);
    Ok(row)
}

/// Which parameters are free.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    /// The fixed half-angle, or `None` when `alpha` is free.
    alpha: Option<f64>,
}

impl Layout {
    fn n(&self) -> usize {
        5 + usize::from(self.alpha.is_none())
    }

    fn names(&self) -> Result<Vec<&'static str>, FitError> {
        let mut v = alloc(6)?;
        v.extend_from_slice(&["a", "b", "u", "v", "r"]);
        if self.alpha.is_none() {
            v.push("alpha");
        }
        Ok(v)
    }

    /// `(a, b, u, v, r, alpha)` from the free parameters `x`.
    fn full(&self, x: &[f64]) -> Option<[f64; 6]> {
        let alpha = match self.alpha {
            Some(h) => h,
            None => *x.get(5)?,
        };
        Some([*x.first()?, *x.get(1)?, *x.get(2)?, *x.get(3)?, *x.get(4)?, alpha])
    }

    /// The free parameters of `(a, b, u, v, r, alpha)`.
    fn free(&self, p: [f64; 6]) -> Result<Vec<f64>, FitError> {
        let mut v = alloc(6)?;
        v.extend_from_slice(&p[..5]);
        if self.alpha.is_none() {
            v.push(p[5]);
        }
        Ok(v)
    }
}

/// A frame in scaled coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame {
    p0: V3,
    a: V3,
    e1: V3,
    e2: V3,
}

/// The axis `(P, d, n)` of the full parameters relative to `fr` (rule 4).
fn axis_of(fr: &Frame, p: &[f64; 6]) -> Result<(V3, V3, f64), NumError> {
    let mut dd = [0.0; 3];
    let mut pp = [0.0; 3];
    for k in 0..3 {
        dd[k] = (fr.a[k] + p[0] * fr.e1[k]) + p[1] * fr.e2[k];
        pp[k] = (fr.p0[k] + p[2] * fr.e1[k]) + p[3] * fr.e2[k];
    }
    let n = red::norm2(&dd)?;
    if n.is_nan() || n <= 0.0 {
        return Err(NumError::Domain { op: OP, detail: "zero_direction" });
    }
    Ok((pp, [dd[0] / n, dd[1] / n, dd[2] / n], n))
}

struct Problem<'a> {
    q: &'a [f64],
    fr: Frame,
    layout: Layout,
}

impl NllsProblem for Problem<'_> {
    fn n_params(&self) -> usize {
        self.layout.n()
    }

    fn n_resid(&self) -> usize {
        self.q.len() / 3
    }

    fn eval(&self, x: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), NumError> {
        let full = self.layout.full(x).ok_or(NumError::Domain { op: OP, detail: "param_count" })?;
        let (pp, d, n) = axis_of(&self.fr, &full)?;
        let (rad, alpha) = (full[4], full[5]);
        let (s, c) = (f::sin(alpha)?, f::cos(alpha)?);
        let e1d = red::dot(&self.fr.e1, &d)?;
        let e2d = red::dot(&self.fr.e2, &d)?;
        let np = self.layout.n();
        for (i, qi) in self.q.chunks_exact(3).enumerate() {
            let w = [qi[0] - pp[0], qi[1] - pp[1], qi[2] - pp[2]];
            let t = red::dot(&w, &d)?;
            let perp = [w[0] - t * d[0], w[1] - t * d[1], w[2] - t * d[2]];
            let rho = red::norm2(&perp)?;
            let (g1, g2) = if rho > 0.0 {
                (red::dot(&perp, &self.fr.e1)? / rho, red::dot(&perp, &self.fr.e2)? / rho)
            } else {
                (0.0, 0.0)
            };
            let ri = r.get_mut(i).ok_or(NumError::Domain { op: OP, detail: "resid_len" })?;
            *ri = (rho - rad) * c - t * s;
            let row = j.get_mut(i * np..(i + 1) * np).ok_or(NumError::Domain { op: OP, detail: "jac_len" })?;
            let tilt = t * c + rho * s;
            row[0] = -(g1 * tilt) / n;
            row[1] = -(g2 * tilt) / n;
            row[2] = -(g1 * c) + s * e1d;
            row[3] = -(g2 * c) + s * e2d;
            row[4] = -c;
            if self.layout.alpha.is_none() {
                row[5] = -((rho - rad) * s) - t * c;
            }
        }
        Ok(())
    }
}

/// One start's converged outcome (scaled units), in canonical form (rule 5).
#[derive(Debug, Clone, PartialEq)]
struct Outcome {
    /// Foot of the centroid on the axis.
    p: V3,
    /// From the apex into the cone.
    d: V3,
    /// Radius at `p`.
    r: f64,
    /// Half-angle, `(0, pi/2]` free, the fixed value otherwise (`0` only for an exact cylinder).
    alpha: f64,
    last: NllsResult,
    iterations: usize,
}

/// Rule 5: whether a round's solver error keeps the converged earlier round (as `ls_cyl`).
fn keeps_earlier_round(e: &nlls::SolverError, have_round: bool) -> bool {
    have_round && !matches!(e, nlls::SolverError::Numeric { .. } | nlls::SolverError::NonFinite { .. })
}

/// Rule 5: a later round that is ill-conditioned or rank deficient fails the start with
/// [`FitError::IllPosedCone`] (the converged earlier round's half-angle `alpha`) instead of
/// keeping that round: the cone is ill-posed about its own optimum. `None` for any other error
/// or in the first round (`alpha` is `None`).
fn later_round_ill_posed(e: &nlls::SolverError, alpha: Option<f64>) -> Option<FitError> {
    let alpha = alpha?;
    match e {
        nlls::SolverError::IllConditioned { cond, .. } | nlls::SolverError::RankDeficient { cond, .. } => {
            Some(FitError::IllPosedCone { half_angle: alpha, cond: *cond })
        }
        _ => None,
    }
}

/// Rule 5's canonical form of a converged `(P, d, r, alpha)`: `alpha` into `(-pi/2, pi/2]` (the
/// returned flag says whether a shift of `pi` negated the residuals), then `alpha >= 0` by
/// reversing `d`; `P` moved to the foot of the origin with the radius there.
fn canonical(pp: V3, d: V3, r: f64, alpha: f64, fixed: bool) -> Result<(V3, V3, f64, f64, bool), FitError> {
    let mut alpha = alpha;
    let mut d = d;
    let mut shifted = false;
    if !fixed {
        if alpha > FRAC_PI_2 {
            alpha -= PI;
            shifted = true;
        } else if alpha <= -FRAC_PI_2 {
            alpha += PI;
            shifted = true;
        }
        if !(alpha > -FRAC_PI_2 && alpha <= FRAC_PI_2) {
            return Err(FitError::Num(NumError::Domain { op: OP, detail: "half_angle_range" }));
        }
        if alpha < 0.0 {
            alpha = -alpha;
            d = [-d[0] + 0.0, -d[1] + 0.0, -d[2] + 0.0];
        }
    }
    // Foot of the perpendicular from the centroid (the origin) onto the axis; the radius there.
    let s = -red::dot(&pp, &d)?;
    let foot = [pp[0] + s * d[0], pp[1] + s * d[1], pp[2] + s * d[2]];
    let r = r + s * f::tan(alpha)?;
    Ok((foot, d, r, alpha, shifted))
}

/// Rule 5: solve, canonicalise, re-anchor, repeat.
fn solve_rounds(q: &[f64], fr0: Frame, layout: Layout, x0: Vec<f64>, cfg: &NllsConfig) -> Result<Outcome, FitError> {
    let mut fr = fr0;
    let mut x = x0;
    let mut iterations = 0usize;
    let mut done: Option<Outcome> = None;
    for _ in 0..REPARAM_ROUNDS {
        let prob = Problem { q, fr, layout };
        let mut res = match nlls::solve(&prob, &x, cfg) {
            Ok(res) => res,
            Err(e) => match (later_round_ill_posed(&e, done.as_ref().map(|o| o.alpha)), &done) {
                (Some(ill), _) => return Err(ill),
                (None, Some(_)) if keeps_earlier_round(&e, true) => break,
                _ => return Err(FitError::from_solver(e, cfg)),
            },
        };
        iterations = iterations.saturating_add(res.iterations);
        let full = layout.full(&res.p).ok_or(FitError::InvalidParams { field: "params" })?;
        let (pp, d, _) = axis_of(&fr, &full)?;
        let (foot, d, r, alpha, shifted) = canonical(pp, d, full[4], full[5], layout.alpha.is_some())?;
        if shifted {
            for v in &mut res.residuals {
                *v = -*v + 0.0;
            }
        }
        let stop = res.iterations == 0;
        done = Some(Outcome { p: foot, d, r, alpha, last: res, iterations });
        if stop {
            break;
        }
        let (e1, e2) = init::basis(d)?;
        fr = Frame { p0: foot, a: d, e1, e2 };
        x = layout.free([0.0, 0.0, 0.0, 0.0, r, alpha])?;
    }
    let mut out = done.ok_or(FitError::InvalidParams { field: "reparam_rounds" })?;
    out.iterations = iterations;
    Ok(out)
}

/// Rule 6: the outcome as SI parameters in the start frame `fr` (scaled).
fn comparable(o: &Outcome, fr: &Frame, layout: Layout, c: &Centred) -> Result<Vec<f64>, FitError> {
    if o.r.is_nan() || o.r <= 0.0 {
        return Err(FitError::Num(NumError::Domain { op: OP, detail: "nonpositive_reference_radius" }));
    }
    let (mut d, mut alpha) = (o.d, o.alpha);
    if red::dot(&d, &fr.a)? < 0.0 {
        d = [-d[0] + 0.0, -d[1] + 0.0, -d[2] + 0.0];
        alpha = -alpha + 0.0;
    }
    let da = red::dot(&d, &fr.a)?;
    if da.is_nan() || da <= 0.0 {
        return Err(FitError::Num(NumError::Domain { op: OP, detail: "axis_perpendicular_to_start" }));
    }
    let a = red::dot(&d, &fr.e1)? / da;
    let b = red::dot(&d, &fr.e2)? / da;
    let w = [fr.p0[0] - o.p[0], fr.p0[1] - o.p[1], fr.p0[2] - o.p[2]];
    let s = red::dot(&w, &fr.a)? / da;
    let x = [(o.p[0] + s * d[0]) - fr.p0[0], (o.p[1] + s * d[1]) - fr.p0[1], (o.p[2] + s * d[2]) - fr.p0[2]];
    let rx = o.r + s * f::tan(alpha)?;
    let m = c.map();
    let full = [
        a,
        b,
        m.inverse_length(red::dot(&x, &fr.e1)?)?,
        m.inverse_length(red::dot(&x, &fr.e2)?)?,
        m.inverse_length(rx)?,
        alpha,
    ];
    layout.free(full)
}

/// Rule 1.
fn check_params(pts: &PointsView<'_>, params: &FitParams, opts: &ConeOptions) -> Result<(), FitError> {
    params.validate(pts)?;
    if params.method != Method::LeastSquares {
        return Err(FitError::InvalidParams { field: "method" });
    }
    if params.weights != Weights::Uniform {
        return Err(FitError::InvalidParams { field: "weights" });
    }
    if !params.constraints.is_empty() {
        return Err(FitError::InvalidParams { field: "constraints" });
    }
    opts.validate()
}

/// Rule 9's bound: RMS distance of the scaled points to their least-squares plane.
fn plane_rms(c: &Centred) -> Result<f64, FitError> {
    let n = data_shape(c)?.axes[2];
    let mut r = alloc(c.len())?;
    for q in c.xyz().chunks_exact(3) {
        r.push(red::dot(q, &n)?);
    }
    Ok(red::norm2(&r)? / f::sqrt(c.len() as f64)?)
}

/// The multi-start error as a fit error.
fn ms_error(e: MultiStartError<FitError>) -> FitError {
    match e {
        MultiStartError::InvalidSpec(_) => FitError::InvalidParams { field: "start_spec" },
        MultiStartError::AllFailed(errs) => {
            let first = match errs.first() {
                Some(StartError::Solver(e)) => e.code(),
                Some(StartError::Numeric(_)) => "numeric",
                Some(StartError::ParamLength { .. }) => "param_length",
                None => "none",
            };
            FitError::MultiStartFailed { starts: errs.len() as u32, first }
        }
    }
}

/// The cone fit with its multi-start record, starts from [`init::cone_start`] (`seed`, or
/// [`DEFAULT_SEED`]).
///
/// # Errors
///
/// `InvalidParams` (rule 1), the checks' `TooFewPoints` / `Collinear` / `Coplanar`, INIT's
/// `IllConditioned`, [`FitError::MultiStartFailed`] when every start fails,
/// [`FitError::IllPosedCone`] (rule 7) and [`FitError::ImplausibleFit`] (rule 9). Flagged fits
/// are returned with their flags (the trait refuses them, rule 10).
pub fn fit_cone(
    pts: &PointsView<'_>,
    params: &FitParams,
    opts: &ConeOptions,
    seed: Option<u64>,
) -> Result<ConeFit, FitError> {
    check_params(pts, params, opts)?;
    check_points(Primitive::Cone, pts, &params.thresholds)?;
    let start = init::cone_start(pts, seed.unwrap_or(DEFAULT_SEED))?;
    run(pts, params, opts, &start)
}

/// The cone fit from a caller-supplied start (the command's `StartSpec` override; its spec must
/// be INIT's `[a, b, u, v, r, alpha]` relative to its frame).
///
/// # Errors
///
/// As [`fit_cone`], and [`FitError::InvalidParams`] `start.primitive` for a start that is not
/// a cone start.
pub fn fit_cone_from(
    pts: &PointsView<'_>,
    params: &FitParams,
    opts: &ConeOptions,
    start: &AxialStart,
) -> Result<ConeFit, FitError> {
    if start.primitive != Primitive::Cone {
        return Err(FitError::InvalidParams { field: "start.primitive" });
    }
    run(pts, params, opts, start)
}

fn run(pts: &PointsView<'_>, params: &FitParams, opts: &ConeOptions, st: &AxialStart) -> Result<ConeFit, FitError> {
    check_params(pts, params, opts)?;
    let (c, _) = check_points(Primitive::Cone, pts, &params.thresholds)?;
    let map = *c.map();
    let layout = Layout { alpha: opts.fixed_half_angle };
    let names = layout.names()?;
    let fr = Frame { p0: map.forward(st.frame.point)?, a: st.frame.axis, e1: st.frame.e1, e2: st.frame.e2 };
    let spec = spec_for(&st.spec, layout)?;
    let outcome = match mstart::run(&spec, |s: &[f64]| solve_start(&c, fr, layout, s, &params.nlls)) {
        Ok(o) => o,
        Err(MultiStartError::AllFailed(errs)) => {
            // Rule 7's probe: say why when the data is a near-plane cone.
            if let Some((alpha, cond)) = plane_probe(&c, layout, &params.nlls)? {
                if ill_posed(alpha, cond, opts)? {
                    return Err(ill_posed_error(alpha, cond));
                }
            }
            return Err(ms_error(MultiStartError::AllFailed(errs)));
        }
        Err(e) => return Err(ms_error(e)),
    };
    let best = &outcome.best;
    let alpha = best.alpha;
    let cond = best.last.cond;
    // Rule 7: never return an ill-posed cone.
    if ill_posed(alpha, cond, opts)? {
        return Err(ill_posed_error(alpha, cond));
    }
    // Rule 8.
    let point = map.inverse(best.p)?;
    let radius = map.inverse_length(best.r)?;
    let d = unit_direction(best.d)?;
    let apex_distance = radius / f::tan(alpha)?;
    let apex_si = [point[0] - apex_distance * d[0], point[1] - apex_distance * d[1], point[2] - apex_distance * d[2]];
    let feature = FittedFeature::Cone { apex: apex_si, d, half_angle: alpha };
    let mut res = alloc(c.len())?;
    for &ri in &best.last.residuals {
        res.push(map.inverse_length(ri)?);
    }
    let mut diag = FitDiagnostics::from_nlls(&best.last);
    diag.iterations = best.iterations;
    let result = FitResult::assemble(&feature, res, diag, None, id())?;
    let apex = if f::total_cmp(apex_distance, opts.apex_report_limit_m)? == Ordering::Greater {
        None
    } else {
        match result.feature {
            FittedFeature::Cone { apex, .. } => Some(apex),
            _ => None,
        }
    };
    // Rule 9.
    let plane = map.inverse_length(plane_rms(&c)?)?;
    let ratio = if plane > 0.0 { result.stats.rms / plane } else { f64::INFINITY };
    let limit = (1.0 + CERT_MARGIN) * plane;
    if f::total_cmp(result.stats.rms, limit)? != Ordering::Less {
        return Err(FitError::ImplausibleFit { rms: result.stats.rms, bound: plane });
    }
    let suspect = f::total_cmp(ratio, SUSPECT_RATIO)? == Ordering::Greater;
    let uncertainty = uncertainty(&c, layout, best)?;
    let weak_axis = f::total_cmp(uncertainty.axis_rad, opts.max_axis_u_rad)? == Ordering::Greater;
    Ok(ConeFit {
        result,
        point,
        axis: d,
        radius,
        half_angle: alpha,
        apex_distance,
        apex,
        options: *opts,
        frame: st.frame,
        param_names: names,
        starts: outcome.starts,
        per_start: outcome.per_start,
        best_index: outcome.best_index,
        local_optimum_only: outcome.local_optimum_only,
        origin: outcome.origin,
        plane_rms: plane,
        rms_ratio: ratio,
        suspect_axis: suspect,
        uncertainty,
        weak_axis,
    })
}

/// Rule 11: the Jacobian at the canonical cone in its own frame (axis `d`, point the foot,
/// basis of `d`, parameters `(0, 0, 0, 0, r, alpha)`, so the tilt parameters are angles to first
/// order), `J^T J` by `red::dot` of its columns, its inverse column by column with
/// `fid_num::la::lstsq`, and `sigma^2 = red::sum_sq(e) / (m - n)`; lengths mapped to metres.
fn uncertainty(c: &Centred, layout: Layout, o: &Outcome) -> Result<ConeUncertainty, FitError> {
    let (e1, e2) = init::basis(o.d)?;
    let fr = Frame { p0: o.p, a: o.d, e1, e2 };
    let prob = Problem { q: c.xyz(), fr, layout };
    let (m, n) = (prob.n_resid(), prob.n_params());
    let x = layout.free([0.0, 0.0, 0.0, 0.0, o.r, o.alpha])?;
    let mut r = alloc(m)?;
    r.resize(m, 0.0);
    let mut j = alloc(m.checked_mul(n).ok_or(FitError::TooLarge { n: m, max: m })?)?;
    j.resize(m * n, 0.0);
    prob.eval(&x, &mut r, &mut j)?;
    if m <= n {
        return Err(FitError::TooFewPoints { n: m, need: n + 1 });
    }
    let sigma = f::sqrt(red::sum_sq(&r)? / (m - n) as f64)?;
    let mut cols: Vec<Vec<f64>> = alloc(n)?;
    for k in 0..n {
        let mut col = alloc(m)?;
        for row in j.chunks_exact(n) {
            col.push(*row.get(k).ok_or(FitError::BadLength { len: j.len() })?);
        }
        cols.push(col);
    }
    let mut jtj = [[0.0; 6]; 6];
    for (row, ca) in jtj.iter_mut().zip(&cols) {
        for (v, cb) in row.iter_mut().zip(&cols) {
            *v = red::dot(ca, cb)?;
        }
    }
    let rows: Vec<&[f64]> = jtj[..n].iter().map(|row| &row[..n]).collect();
    let mat = Mat::from_rows(&rows)?;
    let mut var = [0.0; 6];
    for (k, v) in var.iter_mut().enumerate().take(n) {
        let mut e = [0.0; 6];
        e[k] = 1.0;
        let sol = lstsq(&mat, &e[..n], DEFAULT_RANK_TOL)?;
        if sol.diag.rank < n {
            return Err(FitError::IllConditioned { cond: None });
        }
        let x = *sol.x.get(k).ok_or(FitError::InvalidParams { field: "params" })?;
        if x.is_nan() || x < 0.0 {
            return Err(FitError::IllConditioned { cond: None });
        }
        *v = sigma * f::sqrt(x)?;
    }
    let map = c.map();
    let axis_rad = f::hypot(var[0], var[1])?;
    Ok(ConeUncertainty {
        e1,
        e2,
        sigma_m: map.inverse_length(sigma)?,
        axis_tilt_rad: [var[0], var[1]],
        axis_rad,
        point_m: [map.inverse_length(var[2])?, map.inverse_length(var[3])?],
        radius_m: map.inverse_length(var[4])?,
        half_angle_rad: if layout.alpha.is_none() { Some(var[5]) } else { None },
    })
}

/// Rule 7's test.
fn ill_posed(alpha: f64, cond: f64, opts: &ConeOptions) -> Result<bool, FitError> {
    Ok(f::total_cmp(alpha, opts.near_zero)? != Ordering::Greater
        || f::total_cmp(alpha, FRAC_PI_2 - opts.near_ninety)? != Ordering::Less
        || !cond.is_finite()
        || f::total_cmp(cond, opts.cond_limit)? == Ordering::Greater)
}

fn ill_posed_error(alpha: f64, cond: f64) -> FitError {
    FitError::IllPosedCone { half_angle: alpha, cond: if cond.is_finite() { Some(cond) } else { None } }
}

/// Rule 7's near-plane probe, run only when every start failed: one solve from the plane
/// start (axis the least-squares plane normal `n`, oriented so that the height `t_i = q_i . n`
/// grows with the distance `rho_i` from the axis; the axis point INIT's algebraic circle centre
/// of `n`; the radius that circle's; the half-angle `f::atan2(sxx, |sxt|)` from the scatter
/// `sxx = sum (rho_i - rho_m)^2`, `sxt = sum (rho_i - rho_m) t_i` with `rho_m` the mean, or the
/// fixed one). Returns the converged half-angle and condition estimate, or `None` when the
/// probe has no start or does not converge. Its cone is never returned as a fit.
fn plane_probe(c: &Centred, layout: Layout, cfg: &NllsConfig) -> Result<Option<(f64, f64)>, FitError> {
    let mut n = data_shape(c)?.axes[2];
    let Some((e1, e2, cen, r)) = init::kasa(c, n)? else {
        return Ok(None);
    };
    let p0 = [cen[0] * e1[0] + cen[1] * e2[0], cen[0] * e1[1] + cen[1] * e2[1], cen[0] * e1[2] + cen[1] * e2[2]];
    let len = c.len();
    let (mut ts, mut rhos) = (alloc(len)?, alloc(len)?);
    for q in c.xyz().chunks_exact(3) {
        let w = [q[0] - p0[0], q[1] - p0[1], q[2] - p0[2]];
        let t = red::dot(&w, &n)?;
        let perp = [w[0] - t * n[0], w[1] - t * n[1], w[2] - t * n[2]];
        ts.push(t);
        rhos.push(red::norm2(&perp)?);
    }
    let rho_m = red::sum(&rhos)? / len as f64;
    for v in &mut rhos {
        *v -= rho_m;
    }
    let (sxx, mut sxt) = (red::dot(&rhos, &rhos)?, red::dot(&rhos, &ts)?);
    if sxt < 0.0 {
        n = [-n[0] + 0.0, -n[1] + 0.0, -n[2] + 0.0];
        sxt = -sxt;
    }
    let alpha = match layout.alpha {
        Some(h) => h,
        None => f::atan2(sxx, sxt)?,
    };
    let (e1, e2) = init::basis(n)?;
    let fr = Frame { p0, a: n, e1, e2 };
    let x0 = layout.free([0.0, 0.0, 0.0, 0.0, r, alpha])?;
    match solve_rounds(c.xyz(), fr, layout, x0, cfg) {
        Ok(o) => Ok(Some((o.alpha, o.last.cond))),
        Err(FitError::Num(e)) => Err(FitError::Num(e)),
        Err(_) => Ok(None),
    }
}

/// INIT's spec with a fixed half-angle dropped (rule 3).
fn spec_for(s: &StartSpec, layout: Layout) -> Result<StartSpec, FitError> {
    let pick = |v: &[f64]| -> Result<Vec<f64>, FitError> {
        let arr: [f64; 6] = match v {
            [a, b, u, w, r, h] => [*a, *b, *u, *w, *r, *h],
            _ => return Err(FitError::InvalidParams { field: "start_spec" }),
        };
        layout.free(arr)
    };
    Ok(StartSpec {
        primary: pick(&s.primary)?,
        half_widths: pick(&s.half_widths)?,
        count: s.count,
        seed: s.seed,
        stream: s.stream,
        tolerances: pick(&s.tolerances)?,
    })
}

/// One start (SI parameters in the start frame) through rules 5 and 6.
fn solve_start(
    c: &Centred,
    fr: Frame,
    layout: Layout,
    s: &[f64],
    cfg: &NllsConfig,
) -> mstart::SolveResult<Outcome, FitError> {
    let full = layout.full(s).ok_or(FitError::InvalidParams { field: "start" })?;
    let m = c.map();
    let scaled =
        [full[0], full[1], m.forward_length(full[2])?, m.forward_length(full[3])?, m.forward_length(full[4])?, full[5]];
    let x0 = layout.free(scaled)?;
    let o = solve_rounds(c.xyz(), fr, layout, x0, cfg)?;
    let si = comparable(&o, &fr, layout, c)?;
    let cost = o.last.cost;
    Ok((o, cost, si))
}

impl Fit for ConeLs {
    fn algorithm_id(&self) -> AlgorithmId {
        id()
    }

    /// Rules 10 and 11: runs with [`ConeOptions::default`] and fails closed on a flagged fit
    /// ([`FitError::FlaggedFit`]) and on a weak axis ([`FitError::IllPosedCone`]).
    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, seed: Option<u64>) -> Result<FitResult, FitError> {
        let f = fit_cone(pts, params, &ConeOptions::default(), seed)?;
        if f.local_optimum_only || f.suspect_axis {
            return Err(FitError::FlaggedFit {
                local_optimum_only: f.local_optimum_only,
                suspect_axis: f.suspect_axis,
                rms_ratio: f.rms_ratio,
            });
        }
        if f.weak_axis {
            return Err(FitError::IllPosedCone { half_angle: f.half_angle, cond: Some(f.result.diagnostics.cond) });
        }
        Ok(f.result)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{canonical, keeps_earlier_round, ConeOptions};
    use fid_num::NumError;
    use fid_solv::nlls::{Diagnostics, SolverError, Stage};

    /// Rule 5: a later round's numeric error passes through; any other solver error keeps the
    /// earlier round, and with no earlier round every error fails the start.
    #[test]
    fn numeric_errors_never_keep_the_earlier_round() {
        let last = Diagnostics::default();
        let numeric = SolverError::Numeric { error: NumError::Domain { op: "t", detail: "allocation_failed" }, last };
        let non_finite = SolverError::NonFinite { stage: Stage::Residual, index: Some(0), last };
        let limit = SolverError::IterationLimit { last };
        for e in [&numeric, &non_finite] {
            assert!(!keeps_earlier_round(e, true), "{e:?}");
        }
        assert!(keeps_earlier_round(&limit, true));
        assert!(!keeps_earlier_round(&limit, false));
    }

    /// Rule 5's canonical form: a negative half-angle reverses the axis; a half-angle beyond
    /// `pi/2` is shifted by `pi` (flagged, residuals negated); the radius moves to the foot.
    #[test]
    fn canonical_form() {
        let (p, d, r, a, sh) = canonical([0.0, 0.0, 2.0], [0.0, 0.0, 1.0], 1.0, -0.5, false).unwrap();
        assert_eq!((d, a, sh), ([0.0, 0.0, -1.0], 0.5, false));
        assert_eq!(p, [0.0, 0.0, 0.0]);
        // The foot is 2 along the new d from P, so the radius grows by 2 tan(0.5).
        assert!((r - (1.0 + 2.0 * 0.5f64.tan())).abs() < 1e-12);
        let (_, _, _, a, sh) = canonical([0.0; 3], [1.0, 0.0, 0.0], 1.0, 2.0, false).unwrap();
        assert!(sh && (a - (core::f64::consts::PI - 2.0)).abs() < 1e-15);
        assert!(canonical([0.0; 3], [1.0, 0.0, 0.0], 1.0, 7.0, false).is_err());
    }

    #[test]
    fn options_are_validated() {
        assert!(ConeOptions::default().validate().is_ok());
        let bad = [
            ConeOptions { fixed_half_angle: Some(0.0), ..ConeOptions::default() },
            ConeOptions { fixed_half_angle: Some(core::f64::consts::FRAC_PI_2), ..ConeOptions::default() },
            ConeOptions { near_zero: -1e-3, ..ConeOptions::default() },
            ConeOptions { near_ninety: f64::NAN, ..ConeOptions::default() },
            ConeOptions { near_zero: 0.8, near_ninety: 0.8, ..ConeOptions::default() },
            ConeOptions { apex_report_limit_m: -1.0, ..ConeOptions::default() },
            ConeOptions { cond_limit: 0.5, ..ConeOptions::default() },
        ];
        for o in bad {
            assert_eq!(o.validate().unwrap_err().code(), "invalid_params", "{o:?}");
        }
    }
}
