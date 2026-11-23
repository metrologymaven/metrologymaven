//! Minimum-zone (Chebyshev) 2D line and 3D plane fits. Spec node `KRN.MZ.LIN.FLAT`.
//!
//! Computes the pair of parallel lines (in a caller-supplied or default XY projection plane) or
//! parallel planes with the smallest perpendicular separation containing all points:
//!
//! - [`Line2Mz`] / [`fit_mz_line2`] (`fit.line2.mz@1.0.0`, command row `fit.mz_line2@1`)
//! - [`PlaneMz`] / [`fit_mz_plane`] (`fit.plane.mz@1.0.0`, command row `fit.mz_plane@1`)
//!
//! # Pinned procedure
//!
//! 1. **Parameters.** [`FitParams::validate`]; method must be [`Method::MinimumZone`], weights
//!    [`Weights::Uniform`]; constraints accepted are none, `FixedAxisDirection`, and (for 2D
//!    lines) `ProjectionPlane` (`InvalidParams` otherwise).
//! 2. **Projection & normalisation.** For 2D lines, points are projected into
//!    [`ls_lin::projection_frame`] using the same rule as [`Line2Ls`] (subnormals flushed to
//!    `+0.0`). Free-direction fits check point count and numerical rank via [`check_points`]
//!    (giving `TooFewPoints` / `Collinear` on degenerate input); fixed-direction fits require
//!    at least 1 point. Points are centred and power-of-two scaled by [`Centred::of`].
//! 3. **Fixed direction / normal.** Closed-form exact global optimum: project centred points
//!    onto the unit normal, take total-order `max` and `min`, set mid-zone offset to their
//!    midpoint and `width = scale * (max - min)`.
//! 4. **Free direction / normal.**
//!    - Start from the orthogonal least-squares solution ([`Line2Ls`] or [`PlaneLs`]).
//!    - Build the orthonormal local frame about the LS direction/normal, compute local
//!      coordinates `(u, w)` (or `(u, v, w)`), and extract the exact power-of-two form scale
//!      `k` from `max(w) - min(w)`.
//!    - If `max(w) - min(w) == 0` (exact collinear 2D points or exact coplanar 3D points),
//!      return the exact zero-width closed-form mid-zone feature (`width = 0.0`,
//!      `local_optimum_only = false`).
//!    - Otherwise form [`MzLine2Problem`] (3 parameters) or [`MzPlaneProblem`] (4 parameters)
//!      in form-scaled units, solve via [`fid_solv::seq::solve`] (`CertKind::Zone`, sequential
//!      LP through `fid_solv::conic::solve_lp`) starting from slope 0 lifted onto the data (with
//!      deterministic ring-slope fallbacks if the primary start fails to certify), and obtain
//!      the active-set certificate from `fid_solv::cert::certify`.
//!    - Recompute the mid-zone feature, signed residuals and `width = scale * k * cert.value()`
//!      directly from the certified active-set parameters; fail closed with
//!      [`FitError::SideViolation`] if any point exceeds the certified zone by more than
//!      [`SIDE_TOL`].
//!    - `local_optimum_only` is `false` when the certificate check passes unflagged, `true`
//!      otherwise (REQ-KRN.MZ.LIN.FLAT-004).
//!
//! # Determinism
//!
//! D0: single thread, no FMA, no `f64::min`/`max` (`fid_num::f` total-order helpers only),
//! every sum/dot/norm through `fid_num::red`, KNP-1 conic solver, no hash containers, `seed`
//! ignored (always `null` in canonical output). Bit-identical on x86-64, ARM64 and wasm32.

#![allow(clippy::needless_range_loop)]

mod encode;
mod problem;

pub use encode::{encode, output_digest};
pub use problem::{OFFSET_SCALE, SLOPE_SCALE};

use fid_kci::AlgorithmId;
use fid_num::f::{total_max, total_min};
use fid_num::la::lstsq;
use fid_num::{red, NumError};
use fid_solv::cert::recomp::{ActiveSetProblem, RecompConfig};
use fid_solv::cert::{CertKind, Certificate, Certified, RowLabel};
use fid_solv::seq::{self, SeqConfig};
use fid_types::Determinism;

use crate::contract::{
    self, algorithm_id, check_points, normalize_direction, normalize_direction2, unit_direction, Centred, CommandRow,
    Constraint, Field, Fit, FitDiagnostics, FitError, FitParams, FitResult, FittedFeature, Method, Normalisation,
    PointsView, Primitive, ResidualStats, StopReason, ValueType, Weights, V2, V3,
};
use crate::ls_lin::{self, Line2Ls, PlaneLs};
use problem::{MzLine2Problem, MzPlaneProblem};

/// Version of `fit.line2.mz` and `fit.plane.mz`.
pub const VERSION: (u32, u32, u32) = (1, 0, 0);

/// Largest accepted orthogonal violation of any point outside the certified zone (metres).
pub const SIDE_TOL: f64 = 1e-11;

const OP_LINE2: &str = "fit.mz_line2";
const OP_PLANE: &str = "fit.mz_plane";

/// Minimum-zone 2D line fit (`fit.line2.mz@1.0.0`, spec node `KRN.MZ.LIN.FLAT`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Line2Mz;

/// Minimum-zone 3D plane fit (`fit.plane.mz@1.0.0`, spec node `KRN.MZ.LIN.FLAT`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlaneMz;

/// Full certified result of [`fit_mz_line2`] or [`fit_mz_plane`].
#[derive(Debug, Clone, PartialEq)]
pub struct MzFlatResult {
    /// Algorithm ID (`fit.line2.mz@1.0.0` or `fit.plane.mz@1.0.0`).
    pub algorithm: AlgorithmId,
    /// Canonical mid-zone feature (`FittedFeature::Line2` or `FittedFeature::Plane`, metres).
    pub feature: FittedFeature,
    /// Minimum zone width in SI metres, recomputed from the certified critical point set.
    pub width: f64,
    /// Signed perpendicular residuals from the mid-zone feature (metres, point order).
    pub residuals: Vec<f64>,
    /// Residual statistics over [`Self::residuals`].
    pub stats: ResidualStats,
    /// Critical point labels (`point:side`, ascending) defining the active-set system.
    pub critical_points: Vec<RowLabel>,
    /// Active-set KKT certificate (`None` only for exact zero-form or fixed-direction fits).
    pub certificate: Option<Certificate>,
    /// `false` when certified unflagged or exact closed-form; `true` otherwise.
    pub local_optimum_only: bool,
    /// Exact power-of-two form scale used in local coordinates.
    pub form_scale: f64,
    /// Sequential LP iterations taken (diagnostic only; excluded from canonical bytes/digest).
    pub iterations: u32,
}

/// Default sequential LP configuration (`CertKind::Zone`).
pub fn seq_config() -> SeqConfig {
    SeqConfig::new(CertKind::Zone)
}

fn alloc<T>(op: &'static str, n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| FitError::Num(NumError::Domain { op, detail: "allocation_failed" }))?;
    Ok(v)
}

fn id(p: Primitive) -> AlgorithmId {
    algorithm_id(p, Method::MinimumZone, VERSION).unwrap_or_else(|_| AlgorithmId {
        module: "fit".to_string(),
        feature: p.slug().to_string(),
        method: "mz".to_string(),
        version: VERSION,
    })
}

fn form_scale(range: f64) -> f64 {
    if range.is_normal() && range > 0.0 {
        f64::from_bits(range.to_bits() & 0xFFF0_0000_0000_0000)
    } else {
        1.0
    }
}

fn flush(x: f64) -> f64 {
    if x.is_subnormal() {
        0.0
    } else {
        x
    }
}

fn unit_direction2(v: V2) -> Result<V2, FitError> {
    let n = red::norm2(&v)?;
    if !n.is_normal() {
        return Err(FitError::InvalidDirection);
    }
    Ok([v[0] / n, v[1] / n])
}

struct PlaneFrame {
    e1: V3,
    e2: V3,
    n: V3,
}

fn plane_frame(n: V3) -> Result<PlaneFrame, FitError> {
    let mut k = 0usize;
    for j in 1..3 {
        if n[j].abs() < n[k].abs() {
            k = j;
        }
    }
    let mut a = [0.0; 3];
    a[k] = 1.0;
    let e1 = unit_direction([a[0] - n[k] * n[0], a[1] - n[k] * n[1], a[2] - n[k] * n[2]])?;
    let e2 = unit_direction([n[1] * e1[2] - n[2] * e1[1], n[2] * e1[0] - n[0] * e1[2], n[0] * e1[1] - n[1] * e1[0]])?;
    Ok(PlaneFrame { e1, e2, n })
}

fn validate_params(p: Primitive, pts: &PointsView<'_>, params: &FitParams) -> Result<Option<V3>, FitError> {
    params.validate(pts)?;
    if params.method != Method::MinimumZone {
        return Err(FitError::InvalidParams { field: "method" });
    }
    if params.weights != Weights::Uniform {
        return Err(FitError::InvalidParams { field: "weights" });
    }
    let mut fixed = None;
    for c in &params.constraints {
        match c {
            Constraint::FixedAxisDirection(d) => fixed = Some(*d),
            Constraint::ProjectionPlane { .. } if p == Primitive::Line2 => {}
            _ => return Err(FitError::InvalidParams { field: "constraints" }),
        }
    }
    Ok(fixed)
}

fn ls_params_from(params: &FitParams, include_projection: bool) -> FitParams {
    let mut ls = FitParams::new(Method::LeastSquares);
    ls.thresholds = params.thresholds;
    if include_projection {
        if let Some((p, n)) = params.projection_plane() {
            ls.constraints.push(Constraint::ProjectionPlane { p, n });
        }
    }
    ls
}

fn build_contract_cert(cert: &Certificate, k: f64, map: &Normalisation) -> Result<contract::Certificate, FitError> {
    let mut pts = alloc::<usize>("fit.mz_flat.cert", cert.active_set().len())?;
    for l in cert.active_set() {
        let idx = l.point as usize;
        if !pts.contains(&idx) {
            pts.push(idx);
        }
    }
    pts.sort_unstable();
    let gap = map.inverse_length(k * cert.value_bound())?;
    Ok(contract::Certificate { support: pts, gap })
}

fn to_fit_result(r: MzFlatResult, expected_rank: usize, k: f64, map: &Normalisation) -> Result<FitResult, FitError> {
    let (cond, stop, contract_cert) = match &r.certificate {
        Some(c) => (c.cond(), StopReason::Exchange, Some(build_contract_cert(c, k, map)?)),
        None => (0.0, StopReason::ClosedForm, None),
    };
    let diag = FitDiagnostics { grad_norm: 0.0, rank: expected_rank, cond, iterations: r.iterations as usize, stop };
    FitResult::assemble(&r.feature, r.residuals, diag, contract_cert, r.algorithm)
}

// ---------------------------------------------------------------------------
// 2D Line minimum-zone
// ---------------------------------------------------------------------------

struct PreparedLine2 {
    map: Normalisation,
    e1: V2,
    e2: V2,
    u: Vec<f64>,
    w_raw: Vec<f64>,
    range: f64,
    k: f64,
    max_abs_u: f64,
    ls_feature: FittedFeature,
}

fn project_line2(pts: &PointsView<'_>, params: &FitParams) -> Result<Vec<f64>, FitError> {
    let (p0, e1, e2) = ls_lin::projection_frame(params)?;
    let mut buf = alloc(OP_LINE2, pts.xyz().len())?;
    for x in pts.xyz().chunks_exact(3) {
        let dx = [x[0] - p0[0], x[1] - p0[1], x[2] - p0[2]];
        buf.push(flush(red::dot(&dx, &e1)?));
        buf.push(flush(red::dot(&dx, &e2)?));
        buf.push(0.0);
    }
    Ok(buf)
}

fn prepare_line2(pts: &PointsView<'_>, params: &FitParams) -> Result<PreparedLine2, FitError> {
    let buf = project_line2(pts, params)?;
    let view = PointsView::new(&buf, None)?;
    check_points(Primitive::Line2, &view, &params.thresholds)?;
    let ls_p = ls_params_from(params, false);
    let ls = Line2Ls.fit(&view, &ls_p, None)?;
    let d0 = match ls.feature {
        FittedFeature::Line2 { d, .. } => d,
        _ => return Err(FitError::InvalidParams { field: "feature" }),
    };
    let e1 = d0;
    let e2 = [-d0[1], d0[0]];
    let centred = Centred::new(&view)?;
    let map = *centred.map();
    let n = centred.len();
    let mut u = alloc::<f64>(OP_LINE2, n)?;
    let mut w_raw = alloc::<f64>(OP_LINE2, n)?;
    let mut hi = f64::NEG_INFINITY;
    let mut lo = f64::INFINITY;
    let mut max_abs_u = 0.0_f64;
    for q in centred.xyz().chunks_exact(3) {
        let q2 = [q[0], q[1]];
        let ui = red::dot(&q2, &e1)?;
        let wi = red::dot(&q2, &e2)?;
        u.push(ui);
        w_raw.push(wi);
        hi = total_max(hi, wi)?;
        lo = total_min(lo, wi)?;
        max_abs_u = total_max(max_abs_u, ui.abs())?;
    }
    let range = hi - lo;
    let k = form_scale(range);
    Ok(PreparedLine2 { map, e1, e2, u, w_raw, range, k, max_abs_u, ls_feature: ls.feature })
}

fn build_line2_problem(prep: &PreparedLine2) -> Result<MzLine2Problem, FitError> {
    let mut w = alloc::<f64>(OP_LINE2, prep.w_raw.len())?;
    for &wi in &prep.w_raw {
        w.push(wi / prep.k);
    }
    MzLine2Problem::new(prep.u.clone(), w, prep.k)
}

fn fit_mz_line2_fixed(
    pts: &PointsView<'_>,
    params: &FitParams,
    fixed: V3,
) -> Result<(MzFlatResult, Normalisation), FitError> {
    if pts.is_empty() {
        return Err(FitError::TooFewPoints { n: 0, need: 1 });
    }
    let (_, pe1, pe2) = ls_lin::projection_frame(params)?;
    let f = normalize_direction(fixed)?;
    let g = [red::dot(&f, &pe1)?, red::dot(&f, &pe2)?];
    if red::norm2(&g)? <= params.thresholds.rank_tol {
        return Err(FitError::InvalidParams { field: "constraints.fixed_axis_direction" });
    }
    let d = normalize_direction2(g)?;
    let n2 = [-d[1], d[0]];
    let buf = project_line2(pts, params)?;
    let view = PointsView::new(&buf, None)?;
    let centred = Centred::new(&view)?;
    let map = *centred.map();
    let mut t = alloc::<f64>(OP_LINE2, centred.len())?;
    let mut tmax = f64::NEG_INFINITY;
    let mut tmin = f64::INFINITY;
    for q in centred.xyz().chunks_exact(3) {
        let ti = red::dot(&[q[0], q[1]], &n2)?;
        tmax = total_max(tmax, ti)?;
        tmin = total_min(tmin, ti)?;
        t.push(ti);
    }
    let tmid = 0.5 * (tmax + tmin);
    let width = map.inverse_length(tmax - tmin)?;
    let mut residuals = alloc::<f64>(OP_LINE2, t.len())?;
    let mut critical = Vec::new();
    for (i, &ti) in t.iter().enumerate() {
        residuals.push(map.inverse_length(ti - tmid)?);
        if ti == tmax {
            critical
                .try_reserve(1)
                .map_err(|_| FitError::Num(NumError::Domain { op: OP_LINE2, detail: "allocation_failed" }))?;
            critical.push(RowLabel { point: i as u32, side: 0 });
        }
        if ti == tmin {
            critical
                .try_reserve(1)
                .map_err(|_| FitError::Num(NumError::Domain { op: OP_LINE2, detail: "allocation_failed" }))?;
            critical.push(RowLabel { point: i as u32, side: 1 });
        }
    }
    let p = map.inverse2([tmid * n2[0], tmid * n2[1]])?;
    let feature = FittedFeature::Line2 { p, d }.canonical()?;
    let stats = ResidualStats::of(&residuals)?;
    Ok((
        MzFlatResult {
            algorithm: id(Primitive::Line2),
            feature,
            width,
            residuals,
            stats,
            critical_points: critical,
            certificate: None,
            local_optimum_only: false,
            form_scale: 1.0,
            iterations: 0,
        },
        map,
    ))
}

fn solve_line2_starts(prob: &MzLine2Problem, tilt: f64, cfg: &SeqConfig) -> Result<(Certified, u32), FitError> {
    let mut slopes = [0.0_f64; 3];
    let n_starts = if tilt.is_finite() && tilt > 0.0 {
        slopes[1] = tilt;
        slopes[2] = -tilt;
        3
    } else {
        1
    };
    let mut first_cause: Option<&'static str> = None;
    for idx in 0..n_starts {
        let start = prob.lift(slopes[idx])?;
        match seq::solve(prob, &start, cfg) {
            Ok(out) => return Ok((out.certified, out.iterations)),
            Err(e) => {
                if first_cause.is_none() {
                    first_cause = Some(e.cause.code());
                }
            }
        }
    }
    Err(FitError::SeqFailed { cause: first_cause.unwrap_or("solver"), starts: n_starts as u32 })
}

fn fit_mz_line2_inner(
    pts: &PointsView<'_>,
    params: &FitParams,
    cfg: Option<&SeqConfig>,
) -> Result<(MzFlatResult, Normalisation), FitError> {
    let fixed = validate_params(Primitive::Line2, pts, params)?;
    if let Some(f) = fixed {
        return fit_mz_line2_fixed(pts, params, f);
    }
    let prep = prepare_line2(pts, params)?;
    if !(prep.range.is_normal() && prep.range > 0.0) {
        let residuals = vec![0.0_f64; pts.len()];
        let stats = ResidualStats::of(&residuals)?;
        return Ok((
            MzFlatResult {
                algorithm: id(Primitive::Line2),
                feature: prep.ls_feature.canonical()?,
                width: 0.0,
                residuals,
                stats,
                critical_points: Vec::new(),
                certificate: None,
                local_optimum_only: false,
                form_scale: 1.0,
                iterations: 0,
            },
            prep.map,
        ));
    }
    let prob = build_line2_problem(&prep)?;
    let default_cfg = seq_config();
    let cfg = cfg.unwrap_or(&default_cfg);
    let tilt = if prep.max_abs_u > 0.0 { (prep.range / prep.k) / prep.max_abs_u } else { 0.0 };
    let (certified, iterations) = solve_line2_starts(&prob, tilt, cfg)?;
    let p_cert = certified.params();
    let (a, c, e) = (p_cert[0], p_cert[1], p_cert[2]);
    let (viol_scaled, worst) = prob.max_scaled_violation(p_cert)?;
    let viol_m = prep.map.inverse_length(prep.k * viol_scaled)?;
    if viol_m > SIDE_TOL {
        return Err(FitError::SideViolation { index: worst, violation: viol_m });
    }
    let s = prob.slope_norm(a)?;
    let ka = prep.k * a;
    let n_raw = unit_direction2([prep.e2[0] - ka * prep.e1[0], prep.e2[1] - ka * prep.e1[1]])?;
    let d_raw = [prep.e1[0] + ka * prep.e2[0], prep.e1[1] + ka * prep.e2[1]];
    let d = normalize_direction2(d_raw)?;
    let n_canon = [-d[1], d[0]];
    let sign = if red::dot(&n_canon, &n_raw)? >= 0.0 { 1.0 } else { -1.0 };
    let d_mid = (prep.k * (0.5 * (c + e))) / s;
    let point = prep.map.inverse2([d_mid * n_raw[0], d_mid * n_raw[1]])?;
    let scaled_res = prob.signed_residuals(p_cert)?;
    let mut residuals = alloc::<f64>(OP_LINE2, scaled_res.len())?;
    for &r in &scaled_res {
        residuals.push(prep.map.inverse_length(sign * (prep.k * r))?);
    }
    let width = prep.map.inverse_length(prep.k * certified.value())?;
    let feature = FittedFeature::Line2 { p: point, d }.canonical()?;
    let stats = ResidualStats::of(&residuals)?;
    let local_optimum_only = certified.certificate().flags().local_optimum_only;
    let critical_points = certified.certificate().active_set().to_vec();
    let certificate = Some(certified.into_certificate());
    Ok((
        MzFlatResult {
            algorithm: id(Primitive::Line2),
            feature,
            width,
            residuals,
            stats,
            critical_points,
            certificate,
            local_optimum_only,
            form_scale: prep.k,
            iterations,
        },
        prep.map,
    ))
}

/// Fits a certified minimum-zone 2D line (in the projection plane of `params`, defaulting to XY).
pub fn fit_mz_line2(
    pts: &PointsView<'_>,
    params: &FitParams,
    cfg: Option<&SeqConfig>,
) -> Result<MzFlatResult, FitError> {
    Ok(fit_mz_line2_inner(pts, params, cfg)?.0)
}

/// Independently recomputes the minimum-zone 2D line width from ONLY the critical point set
/// `critical_points` by solving its linear equal-deviation system (AC-3).
///
/// Matches [`MzFlatResult::width`] bit for bit for any certified free-direction fit.
pub fn recompute_mz_line2_width(
    pts: &PointsView<'_>,
    params: &FitParams,
    critical_points: &[RowLabel],
) -> Result<f64, FitError> {
    let _ = validate_params(Primitive::Line2, pts, params)?;
    let prep = prepare_line2(pts, params)?;
    if !(prep.range.is_normal() && prep.range > 0.0) {
        return Ok(0.0);
    }
    let prob = build_line2_problem(&prep)?;
    let zeros = [0.0_f64; 3];
    let (r0, j) = prob
        .residuals_and_jacobian(critical_points, &zeros)
        .map_err(|_| FitError::InvalidParams { field: "critical_points" })?;
    let neg_r0: Vec<f64> = r0.iter().map(|x| -x).collect();
    let sol = lstsq(&j, &neg_r0, RecompConfig::default().rel_tol)?;
    let val = prob.value(&sol.x).map_err(|_| FitError::InvalidParams { field: "critical_points" })?;
    prep.map.inverse_length(prep.k * val)
}

impl Fit for Line2Mz {
    fn algorithm_id(&self) -> AlgorithmId {
        id(Primitive::Line2)
    }

    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, _seed: Option<u64>) -> Result<FitResult, FitError> {
        let (res, map) = fit_mz_line2_inner(pts, params, None)?;
        let k = res.form_scale;
        to_fit_result(res, 3, k, &map)
    }
}

// ---------------------------------------------------------------------------
// 3D Plane minimum-zone
// ---------------------------------------------------------------------------

struct PreparedPlane {
    map: Normalisation,
    fr: PlaneFrame,
    u: Vec<f64>,
    v: Vec<f64>,
    w_raw: Vec<f64>,
    range: f64,
    k: f64,
    max_abs_uv: f64,
    ls_feature: FittedFeature,
}

fn prepare_plane(pts: &PointsView<'_>, params: &FitParams) -> Result<PreparedPlane, FitError> {
    check_points(Primitive::Plane, pts, &params.thresholds)?;
    let ls_p = ls_params_from(params, false);
    let ls = PlaneLs.fit(pts, &ls_p, None)?;
    let n0 = match ls.feature {
        FittedFeature::Plane { n, .. } => n,
        _ => return Err(FitError::InvalidParams { field: "feature" }),
    };
    let fr = plane_frame(n0)?;
    let centred = Centred::new(pts)?;
    let map = *centred.map();
    let n = centred.len();
    let mut u = alloc::<f64>(OP_PLANE, n)?;
    let mut v = alloc::<f64>(OP_PLANE, n)?;
    let mut w_raw = alloc::<f64>(OP_PLANE, n)?;
    let mut hi = f64::NEG_INFINITY;
    let mut lo = f64::INFINITY;
    let mut max_abs_uv = 0.0_f64;
    for q in centred.xyz().chunks_exact(3) {
        let ui = red::dot(q, &fr.e1)?;
        let vi = red::dot(q, &fr.e2)?;
        let wi = red::dot(q, &fr.n)?;
        u.push(ui);
        v.push(vi);
        w_raw.push(wi);
        hi = total_max(hi, wi)?;
        lo = total_min(lo, wi)?;
        max_abs_uv = total_max(max_abs_uv, ui.abs())?;
        max_abs_uv = total_max(max_abs_uv, vi.abs())?;
    }
    let range = hi - lo;
    let k = form_scale(range);
    Ok(PreparedPlane { map, fr, u, v, w_raw, range, k, max_abs_uv, ls_feature: ls.feature })
}

fn build_plane_problem(prep: &PreparedPlane) -> Result<MzPlaneProblem, FitError> {
    let mut w = alloc::<f64>(OP_PLANE, prep.w_raw.len())?;
    for &wi in &prep.w_raw {
        w.push(wi / prep.k);
    }
    MzPlaneProblem::new(prep.u.clone(), prep.v.clone(), w, prep.k)
}

fn fit_mz_plane_fixed(pts: &PointsView<'_>, fixed: V3) -> Result<(MzFlatResult, Normalisation), FitError> {
    if pts.is_empty() {
        return Err(FitError::TooFewPoints { n: 0, need: 1 });
    }
    let n = normalize_direction(fixed)?;
    let centred = Centred::new(pts)?;
    let map = *centred.map();
    let mut t = alloc::<f64>(OP_PLANE, centred.len())?;
    let mut tmax = f64::NEG_INFINITY;
    let mut tmin = f64::INFINITY;
    for q in centred.xyz().chunks_exact(3) {
        let ti = red::dot(q, &n)?;
        tmax = total_max(tmax, ti)?;
        tmin = total_min(tmin, ti)?;
        t.push(ti);
    }
    let tmid = 0.5 * (tmax + tmin);
    let width = map.inverse_length(tmax - tmin)?;
    let mut residuals = alloc::<f64>(OP_PLANE, t.len())?;
    let mut critical = Vec::new();
    for (i, &ti) in t.iter().enumerate() {
        residuals.push(map.inverse_length(ti - tmid)?);
        if ti == tmax {
            critical
                .try_reserve(1)
                .map_err(|_| FitError::Num(NumError::Domain { op: OP_PLANE, detail: "allocation_failed" }))?;
            critical.push(RowLabel { point: i as u32, side: 0 });
        }
        if ti == tmin {
            critical
                .try_reserve(1)
                .map_err(|_| FitError::Num(NumError::Domain { op: OP_PLANE, detail: "allocation_failed" }))?;
            critical.push(RowLabel { point: i as u32, side: 1 });
        }
    }
    let p = map.inverse([tmid * n[0], tmid * n[1], tmid * n[2]])?;
    let feature = FittedFeature::Plane { p, n }.canonical()?;
    let stats = ResidualStats::of(&residuals)?;
    Ok((
        MzFlatResult {
            algorithm: id(Primitive::Plane),
            feature,
            width,
            residuals,
            stats,
            critical_points: critical,
            certificate: None,
            local_optimum_only: false,
            form_scale: 1.0,
            iterations: 0,
        },
        map,
    ))
}

fn solve_plane_starts(prob: &MzPlaneProblem, tilt: f64, cfg: &SeqConfig) -> Result<(Certified, u32), FitError> {
    let mut slopes = [(0.0_f64, 0.0_f64); 5];
    let n_starts = if tilt.is_finite() && tilt > 0.0 {
        slopes[1] = (tilt, 0.0);
        slopes[2] = (0.0, tilt);
        slopes[3] = (-tilt, 0.0);
        slopes[4] = (0.0, -tilt);
        5
    } else {
        1
    };
    let mut first_cause: Option<&'static str> = None;
    for idx in 0..n_starts {
        let (a, b) = slopes[idx];
        let start = prob.lift(a, b)?;
        match seq::solve(prob, &start, cfg) {
            Ok(out) => return Ok((out.certified, out.iterations)),
            Err(e) => {
                if first_cause.is_none() {
                    first_cause = Some(e.cause.code());
                }
            }
        }
    }
    Err(FitError::SeqFailed { cause: first_cause.unwrap_or("solver"), starts: n_starts as u32 })
}

fn fit_mz_plane_inner(
    pts: &PointsView<'_>,
    params: &FitParams,
    cfg: Option<&SeqConfig>,
) -> Result<(MzFlatResult, Normalisation), FitError> {
    let fixed = validate_params(Primitive::Plane, pts, params)?;
    if let Some(f) = fixed {
        return fit_mz_plane_fixed(pts, f);
    }
    let prep = prepare_plane(pts, params)?;
    if !(prep.range.is_normal() && prep.range > 0.0) {
        let residuals = vec![0.0_f64; pts.len()];
        let stats = ResidualStats::of(&residuals)?;
        return Ok((
            MzFlatResult {
                algorithm: id(Primitive::Plane),
                feature: prep.ls_feature.canonical()?,
                width: 0.0,
                residuals,
                stats,
                critical_points: Vec::new(),
                certificate: None,
                local_optimum_only: false,
                form_scale: 1.0,
                iterations: 0,
            },
            prep.map,
        ));
    }
    let prob = build_plane_problem(&prep)?;
    let default_cfg = seq_config();
    let cfg = cfg.unwrap_or(&default_cfg);
    let tilt = if prep.max_abs_uv > 0.0 { (prep.range / prep.k) / prep.max_abs_uv } else { 0.0 };
    let (certified, iterations) = solve_plane_starts(&prob, tilt, cfg)?;
    let p_cert = certified.params();
    let (a, b, c, e) = (p_cert[0], p_cert[1], p_cert[2], p_cert[3]);
    let (viol_scaled, worst) = prob.max_scaled_violation(p_cert)?;
    let viol_m = prep.map.inverse_length(prep.k * viol_scaled)?;
    if viol_m > SIDE_TOL {
        return Err(FitError::SideViolation { index: worst, violation: viol_m });
    }
    let s = prob.slope_norm(a, b)?;
    let (ka, kb) = (prep.k * a, prep.k * b);
    let n_raw = unit_direction([
        (prep.fr.n[0] - ka * prep.fr.e1[0]) - kb * prep.fr.e2[0],
        (prep.fr.n[1] - ka * prep.fr.e1[1]) - kb * prep.fr.e2[1],
        (prep.fr.n[2] - ka * prep.fr.e1[2]) - kb * prep.fr.e2[2],
    ])?;
    let n = normalize_direction(n_raw)?;
    let sign = if red::dot(&n, &n_raw)? >= 0.0 { 1.0 } else { -1.0 };
    let d_mid = (prep.k * (0.5 * (c + e))) / s;
    let point = prep.map.inverse([d_mid * n_raw[0], d_mid * n_raw[1], d_mid * n_raw[2]])?;
    let scaled_res = prob.signed_residuals(p_cert)?;
    let mut residuals = alloc::<f64>(OP_PLANE, scaled_res.len())?;
    for &r in &scaled_res {
        residuals.push(prep.map.inverse_length(sign * (prep.k * r))?);
    }
    let width = prep.map.inverse_length(prep.k * certified.value())?;
    let feature = FittedFeature::Plane { p: point, n }.canonical()?;
    let stats = ResidualStats::of(&residuals)?;
    let local_optimum_only = certified.certificate().flags().local_optimum_only;
    let critical_points = certified.certificate().active_set().to_vec();
    let certificate = Some(certified.into_certificate());
    Ok((
        MzFlatResult {
            algorithm: id(Primitive::Plane),
            feature,
            width,
            residuals,
            stats,
            critical_points,
            certificate,
            local_optimum_only,
            form_scale: prep.k,
            iterations,
        },
        prep.map,
    ))
}

/// Fits a certified minimum-zone 3D plane (flatness).
pub fn fit_mz_plane(
    pts: &PointsView<'_>,
    params: &FitParams,
    cfg: Option<&SeqConfig>,
) -> Result<MzFlatResult, FitError> {
    Ok(fit_mz_plane_inner(pts, params, cfg)?.0)
}

/// Independently recomputes the minimum-zone plane width from ONLY the critical point set
/// `critical_points` by solving its linear equal-deviation system (AC-3).
///
/// Matches [`MzFlatResult::width`] bit for bit for any certified free-normal fit.
pub fn recompute_mz_plane_width(
    pts: &PointsView<'_>,
    params: &FitParams,
    critical_points: &[RowLabel],
) -> Result<f64, FitError> {
    let _ = validate_params(Primitive::Plane, pts, params)?;
    let prep = prepare_plane(pts, params)?;
    if !(prep.range.is_normal() && prep.range > 0.0) {
        return Ok(0.0);
    }
    let prob = build_plane_problem(&prep)?;
    let zeros = [0.0_f64; 4];
    let (r0, j) = prob
        .residuals_and_jacobian(critical_points, &zeros)
        .map_err(|_| FitError::InvalidParams { field: "critical_points" })?;
    let neg_r0: Vec<f64> = r0.iter().map(|x| -x).collect();
    let sol = lstsq(&j, &neg_r0, RecompConfig::default().rel_tol)?;
    let val = prob.value(&sol.x).map_err(|_| FitError::InvalidParams { field: "critical_points" })?;
    prep.map.inverse_length(prep.k * val)
}

impl Fit for PlaneMz {
    fn algorithm_id(&self) -> AlgorithmId {
        id(Primitive::Plane)
    }

    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, _seed: Option<u64>) -> Result<FitResult, FitError> {
        let (res, map) = fit_mz_plane_inner(pts, params, None)?;
        let k = res.form_scale;
        to_fit_result(res, 4, k, &map)
    }
}

// ---------------------------------------------------------------------------
// Registry command data rows (REQ-KRN.MZ.LIN.FLAT-005)
// ---------------------------------------------------------------------------

const fn fld(
    name: &'static str,
    ty: ValueType,
    quantity: Option<(&'static str, &'static str)>,
    required: bool,
) -> Field {
    Field { name, ty, quantity, required }
}

const L: Option<(&str, &str)> = Some(("length", "m"));

/// Minimum-zone result fields appended to the contract's common result fields.
pub const MZ_FLAT_RESULT: [Field; 3] = [
    fld("zone_width", ValueType::Number, L, true),
    fld("local_optimum_only", ValueType::Boolean, None, true),
    fld("certificate.digest", ValueType::String, None, false),
];

fn row_for(p: Primitive, op: &'static str) -> Result<CommandRow, FitError> {
    let mut row = contract::command_row(p, Method::MinimumZone, VERSION, Determinism::D0)?;
    row.result
        .try_reserve_exact(MZ_FLAT_RESULT.len())
        .map_err(|_| FitError::Num(NumError::Domain { op, detail: "allocation_failed" }))?;
    row.result.extend_from_slice(&MZ_FLAT_RESULT);
    Ok(row)
}

/// Command data row for `fit.mz_line2@1` (W2, dry-run R1, D0).
pub fn command_row_line2() -> Result<CommandRow, FitError> {
    row_for(Primitive::Line2, OP_LINE2)
}

/// Command data row for `fit.mz_plane@1` (W2, dry-run R1, D0).
pub fn command_row_plane() -> Result<CommandRow, FitError> {
    row_for(Primitive::Plane, OP_PLANE)
}

/// Both command data rows of `KRN.MZ.LIN.FLAT` (`[fit.mz_line2@1, fit.mz_plane@1]`).
pub fn command_rows() -> Result<Vec<CommandRow>, FitError> {
    let mut rows = alloc(OP_PLANE, 2)?;
    rows.push(command_row_line2()?);
    rows.push(command_row_plane()?);
    Ok(rows)
}
