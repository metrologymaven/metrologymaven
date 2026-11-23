//! Minimum-zone (Chebyshev) 2D circle fit (roundness) and Voronoi agreement checker.
//! Spec node `KRN.MZ.RND.CIRC`.
//!
//! Computes the pair of concentric circles in a projection plane (default XY) with the
//! smallest radial separation `R_outer - R_inner` containing all points, reporting the mean
//! circle (`FittedFeature::Circle2`) and the certified zone width.
//!
//! # Pinned procedure
//!
//! 1. **Parameters.** [`FitParams::validate`]; method must be [`Method::MinimumZone`], weights
//!    [`Weights::Uniform`]; accepted constraints are none or `ProjectionPlane` (`InvalidParams`
//!    otherwise). `starts` must be in `1..=MAX_STARTS`.
//! 2. **Projection & checks.** Project 3D points into [`ls_lin::projection_frame`] (subnormals
//!    flushed to `+0.0`). Run [`check_points`] for `Primitive::Circle2`: checks count (`>= 3`),
//!    rank (`>= 2`, refusing collinear points with `Collinear`) and angular coverage against
//!    `params.thresholds.min_coverage` (`InsufficientCoverage`, REQ-KRN.MZ.RND.CIRC-002).
//! 3. **Least-squares start.** Fit [`Circle2Ls`] on the projected points to obtain the starting
//!    centre `(cx0, cy0)` and radii `(r_outer0, r_inner0)` in centred, power-of-two scaled
//!    coordinates. If `r_outer0 - r_inner0 == 0` (exact perfect circle or 3-point circumcircle),
//!    return the exact zero-width closed-form result (`width = 0.0`, `local_optimum_only = false`).
//! 4. **Multi-start sequential LP & certification.** Generate `starts` deterministic starts via
//!    `fid_solv::mstart::generate_starts` about `(cx0, cy0)` with half-width `max(w0, 0.05 * r_mid0)`
//!    using `seed` and stream [`STREAM`], lift every start onto `(max rho, min rho)`, and run
//!    `fid_solv::seq::solve` (`CertKind::Zone`) through `mstart::run_listed`. Flag
//!    `local_optimum_only` when starts disagree or a converged start failed certification.
//! 5. **Recomputed width & fail-closed checks.** Recompute `r_outer = max_{side 0} rho_k`,
//!    `r_inner = min_{side 1} rho_k` at the certified centre, `width = (r_outer - r_inner) * scale`
//!    (matching `certified.value() * scale`), verify every point lies inside `[r_inner - tol, r_outer + tol]`
//!    (`SideViolation` otherwise).
//!
//! # Determinism
//!
//! D0 for a given seed: single thread, no FMA, no `f64::min`/`max`, `fid_num::red` for sums
//! and norms, KNP-1 `fid-clarabel`, no hash containers. Bit-identical on x86-64, ARM64 and wasm32.

#![allow(clippy::needless_range_loop)]

mod encode;
mod problem;

pub use encode::{encode, output_digest};
pub use problem::{PENALTY, R_SCALE};

use core::cmp::Ordering;

use fid_kci::AlgorithmId;
use fid_num::{f, red, NumError};
use fid_solv::cert::{CertKind, Certificate, Certified, RowLabel};
use fid_solv::mstart::{self, MultiStartError, StartError, StartSpec, StartSummary, MAX_STARTS};
use fid_solv::seq::{self, SeqConfig};
use fid_types::Determinism;

use crate::contract::{
    self, algorithm_id, check_points, Centred, CommandRow, Constraint, Field, Fit, FitDiagnostics, FitError, FitParams,
    FitResult, FittedFeature, Method, Normalisation, PointsView, Primitive, ResidualStats, StopReason, ValueType,
    Weights, V2,
};
use crate::ls_lin;
use crate::ls_rnd::Circle2Ls;
use crate::vor::{mz_circle2d_xy, MzCircle};
use problem::MzCircle2Problem;

/// Algorithm version of `fit.circle2.mz`.
pub const VERSION: (u32, u32, u32) = (1, 0, 0);

/// Default seed when caller passes `None`.
pub const DEFAULT_SEED: u64 = 0;

/// Default deterministic multi-start count (matches Design budget: 4 starts).
pub const DEFAULT_STARTS: u32 = 4;

/// `fid_num::rng` stream for `KRN.MZ.RND.CIRC` multi-start perturbations.
pub const STREAM: u64 = 5;

/// Relative agreement tolerance across starts.
pub const AGREE_TOL: f64 = 1e-9;

/// Largest accepted excess of any point outside `[r_inner, r_outer]` (metres).
pub const SIDE_TOL: f64 = 1e-11;

/// Default Voronoi agreement tolerance (REQ-KRN.MZ.RND.CIRC-005: 1e-9 m).
pub const VORONOI_AGREE_TOL: f64 = 1e-9;

const OP: &str = "fit.mz_circ";

/// Minimum-zone 2D circle fit (`fit.circle2.mz@1.0.0`, spec node `KRN.MZ.RND.CIRC`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Circle2Mz;

/// Configuration options for [`fit_mz_circle2`].
#[derive(Debug, Clone, PartialEq)]
pub struct MzCircle2Params {
    /// Number of deterministic starts (primary included; in `1..=MAX_STARTS`).
    pub starts: u32,
    /// Optional sequential-solver configuration override (defaults to `SeqConfig::new(CertKind::Zone)`).
    pub seq: Option<SeqConfig>,
}

impl Default for MzCircle2Params {
    fn default() -> Self {
        Self { starts: DEFAULT_STARTS, seq: None }
    }
}

/// Full certified result of [`fit_mz_circle2`].
#[derive(Debug, Clone, PartialEq)]
pub struct MzCircle2Result {
    /// Algorithm ID (`fit.circle2.mz@1.0.0`).
    pub algorithm: AlgorithmId,
    /// Canonical mean circle (`FittedFeature::Circle2 { c, r }`, metres in projection plane).
    pub feature: FittedFeature,
    /// Minimum radial zone width `r_outer - r_inner` in SI metres, recomputed from critical points.
    pub width: f64,
    /// Inner enclosing circle radius (metres).
    pub r_inner: f64,
    /// Outer enclosing circle radius (metres).
    pub r_outer: f64,
    /// Signed radial residuals `rho_i - r_mid` (metres, point order).
    pub residuals: Vec<f64>,
    /// Residual statistics over [`Self::residuals`].
    pub stats: ResidualStats,
    /// Certified active-set labels (`point:side`, ascending; side 0 = outer, side 1 = inner).
    pub critical_points: Vec<RowLabel>,
    /// Maximum radial excess of any point outside `[r_inner, r_outer]` (metres).
    pub max_violation: f64,
    /// Active-set KKT certificate (`None` only for exact zero-width circles).
    pub certificate: Option<Certificate>,
    /// `true` when multi-start results disagreed or a converged start failed certification.
    pub local_optimum_only: bool,
    /// Seed used for deterministic multi-start.
    pub seed: u64,
    /// Number of starts attempted.
    pub starts: u32,
    /// Number of starts that failed.
    pub failed_starts: u32,
}

/// Disagreement reported by [`check_voronoi_agreement`] (REQ-KRN.MZ.RND.CIRC-005).
#[derive(Debug, Clone, PartialEq)]
pub struct VoronoiDisagreement {
    /// Name/ID of the failing reference case.
    pub case_id: String,
    /// `|mz.width - vor.width|` in metres.
    pub width_diff: f64,
    /// Euclidean distance between `mz.center` and `vor.center` in metres.
    pub center_diff: f64,
    /// Configured tolerance in metres.
    pub tol: f64,
}

/// Default sequential configuration (`CertKind::Zone`).
pub fn seq_config() -> SeqConfig {
    SeqConfig::new(CertKind::Zone)
}

fn alloc<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    Ok(v)
}

fn id() -> AlgorithmId {
    algorithm_id(Primitive::Circle2, Method::MinimumZone, VERSION).unwrap_or_else(|_| AlgorithmId {
        module: "fit".to_string(),
        feature: "circle2".to_string(),
        method: "mz".to_string(),
        version: VERSION,
    })
}

fn flush(x: f64) -> f64 {
    if x.is_subnormal() {
        0.0
    } else {
        x
    }
}

fn project_circle2(pts: &PointsView<'_>, params: &FitParams) -> Result<Vec<f64>, FitError> {
    let (p0, e1, e2) = ls_lin::projection_frame(params)?;
    let mut buf = alloc(pts.xyz().len())?;
    for x in pts.xyz().chunks_exact(3) {
        let dx = [x[0] - p0[0], x[1] - p0[1], x[2] - p0[2]];
        buf.push(flush(red::dot(&dx, &e1)?));
        buf.push(flush(red::dot(&dx, &e2)?));
        buf.push(0.0);
    }
    Ok(buf)
}

fn validate_params(pts: &PointsView<'_>, params: &FitParams, extra: &MzCircle2Params) -> Result<(), FitError> {
    params.validate(pts)?;
    if params.method != Method::MinimumZone {
        return Err(FitError::InvalidParams { field: "method" });
    }
    if params.weights != Weights::Uniform {
        return Err(FitError::InvalidParams { field: "weights" });
    }
    if extra.starts == 0 || extra.starts > MAX_STARTS {
        return Err(FitError::InvalidParams { field: "starts" });
    }
    for c in &params.constraints {
        if !matches!(c, Constraint::ProjectionPlane { .. }) {
            return Err(FitError::InvalidParams { field: "constraints" });
        }
    }
    Ok(())
}

fn cert_err(e: fid_solv::cert::CertError) -> FitError {
    match e {
        fid_solv::cert::CertError::Numeric(n) => FitError::Num(n),
        _ => FitError::SeqFailed { cause: "cert", starts: 1 },
    }
}

fn seq_cause(e: &StartError<seq::SolverError>) -> &'static str {
    match e {
        StartError::Solver(s) => s.cause.code(),
        StartError::Numeric(_) => "numeric",
        StartError::ParamLength { .. } => "param_length",
    }
}

fn fit_mz_circle2_inner(
    pts: &PointsView<'_>,
    params: &FitParams,
    extra: &MzCircle2Params,
    seed: Option<u64>,
) -> Result<(MzCircle2Result, Normalisation), FitError> {
    validate_params(pts, params, extra)?;
    let seed = seed.unwrap_or(DEFAULT_SEED);

    let buf = project_circle2(pts, params)?;
    let view = PointsView::new(&buf, None)?;
    // Enforces min_points >= 3, rank >= 2 (Collinear) and min_coverage (InsufficientCoverage).
    check_points(Primitive::Circle2, &view, &params.thresholds)?;

    // Start from the orthogonal least-squares 2D circle.
    let mut ls_p = FitParams::new(Method::LeastSquares);
    ls_p.thresholds = params.thresholds;
    ls_p.nlls = params.nlls;
    let ls = Circle2Ls.fit(&view, &ls_p, None)?;
    let (ls_c, ls_r) = match ls.feature {
        FittedFeature::Circle2 { c, r } => (c, r),
        _ => return Err(FitError::InvalidParams { field: "feature" }),
    };

    let centred = Centred::new(&view)?;
    let map = *centred.map();
    let n = centred.len();
    let mut u = alloc::<f64>(n)?;
    let mut v = alloc::<f64>(n)?;
    let mut data_scale = 0.0_f64;
    for q in centred.xyz().chunks_exact(3) {
        u.push(q[0]);
        v.push(q[1]);
        data_scale = f::total_max(data_scale, q[0].abs())?;
        data_scale = f::total_max(data_scale, q[1].abs())?;
    }

    let c0_3 = map.forward([ls_c[0], ls_c[1], 0.0])?;
    let (cx0, cy0) = (c0_3[0], c0_3[1]);
    let r_ls_sc = map.forward_length(ls_r)?;

    let mut r_outer0 = 0.0_f64;
    let mut r_inner0 = f64::INFINITY;
    for i in 0..n {
        let rho = red::norm2(&[u[i] - cx0, v[i] - cy0])?;
        r_outer0 = f::total_max(r_outer0, rho)?;
        r_inner0 = f::total_min(r_inner0, rho)?;
    }
    let w0 = r_outer0 - r_inner0;

    // Exact perfect circle (or 3-point circumcircle with zero radial spread).
    if n < 4 || !(w0.is_normal() && w0 > 0.0) {
        let r_mid_sc = 0.5 * (r_outer0 + r_inner0);
        let mut residuals = alloc(n)?;
        for i in 0..n {
            let rho = red::norm2(&[u[i] - cx0, v[i] - cy0])?;
            residuals.push(map.inverse_length(rho - r_mid_sc)?);
        }
        let stats = ResidualStats::of(&residuals)?;
        let feature = FittedFeature::Circle2 { c: ls_c, r: map.inverse_length(r_mid_sc)? }.canonical()?;
        return Ok((
            MzCircle2Result {
                algorithm: id(),
                feature,
                width: 0.0,
                r_inner: map.inverse_length(r_inner0)?,
                r_outer: map.inverse_length(r_outer0)?,
                residuals,
                stats,
                critical_points: Vec::new(),
                max_violation: 0.0,
                certificate: None,
                local_optimum_only: false,
                seed,
                starts: 0,
                failed_starts: 0,
            },
            map,
        ));
    }

    let start0 = [cx0, cy0, r_outer0, r_inner0];
    let len_scale = f::total_max(w0, 0.05 * r_ls_sc)?;
    data_scale = f::total_max(data_scale, r_outer0)?;
    let prob = MzCircle2Problem::new(u, v, start0, len_scale, data_scale)?;

    let perturb = f::total_max(w0, 1e-4 * r_ls_sc)?;
    let tol = AGREE_TOL * f::total_max(1.0, r_ls_sc)?;
    let spec = StartSpec {
        primary: start0.to_vec(),
        half_widths: vec![perturb, perturb, 0.0, 0.0],
        count: extra.starts,
        seed,
        stream: STREAM,
        tolerances: vec![tol, tol, tol, tol],
    };
    let raw_starts = mstart::generate_starts(&spec).map_err(|_| FitError::InvalidParams { field: "starts" })?;
    let mut starts = alloc(raw_starts.len())?;
    for st in raw_starts {
        starts.push(prob.lift(st[0], st[1]).map_err(cert_err)?);
    }

    let default_cfg = seq_config();
    let cfg = extra.seq.as_ref().unwrap_or(&default_cfg);
    let mut certified_list: Vec<Option<Certified>> = alloc(starts.len())?;
    let run = mstart::run_listed(&starts, &spec.tolerances, |start| match seq::solve(&prob, start, cfg) {
        Ok(out) => {
            let val = out.certified.value();
            let ps = out.certified.params().to_vec();
            certified_list.push(Some(out.certified));
            Ok(((), val, ps))
        }
        Err(e) => {
            certified_list.push(None);
            Err(e)
        }
    });

    let out = match run {
        Ok(o) => o,
        Err(MultiStartError::AllFailed(errs)) => {
            let cause = errs.first().map_or("no_starts", seq_cause);
            return Err(FitError::SeqFailed { cause, starts: extra.starts });
        }
        Err(MultiStartError::InvalidSpec(_)) => return Err(FitError::InvalidParams { field: "starts" }),
    };

    let converged_but_failed =
        out.per_start.iter().any(|s| matches!(s, StartSummary::Failed(StartError::Solver(e)) if e.converged));
    let failed_starts = out.per_start.iter().filter(|s| matches!(s, StartSummary::Failed(_))).count() as u32;
    let local_optimum_only = out.local_optimum_only || converged_but_failed;

    let mut chosen = certified_list
        .get_mut(out.best_index as usize)
        .and_then(Option::take)
        .ok_or(FitError::SeqFailed { cause: "no_best", starts: extra.starts })?;
    if local_optimum_only && !chosen.certificate().flags().local_optimum_only {
        chosen.set_local_optimum_only().map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "cert_flag" }))?;
    }

    let p_cert = chosen.params();
    let (cx, cy, r_outer_sc, r_inner_sc) = (p_cert[0], p_cert[1], p_cert[2], p_cert[3]);
    let radii = prob.radii(cx, cy).map_err(cert_err)?;
    let r_mid_sc = 0.5 * (r_outer_sc + r_inner_sc);

    let mut residuals = alloc(n)?;
    let mut worst_idx = 0usize;
    let mut worst_viol = 0.0_f64;
    for (i, &rho) in radii.iter().enumerate() {
        let v_out = map.inverse_length(rho - r_outer_sc)?;
        let v_in = map.inverse_length(r_inner_sc - rho)?;
        let v = f::total_max(v_out, v_in)?;
        if f::total_cmp(v, worst_viol)? == Ordering::Greater {
            worst_viol = v;
            worst_idx = i;
        }
        residuals.push(map.inverse_length(rho - r_mid_sc)?);
    }
    if f::total_cmp(worst_viol, SIDE_TOL)? == Ordering::Greater {
        return Err(FitError::SideViolation { index: worst_idx, violation: worst_viol });
    }

    let center = map.inverse2([cx, cy])?;
    let r_outer = map.inverse_length(r_outer_sc)?;
    let r_inner = map.inverse_length(r_inner_sc)?;
    let r_mid = map.inverse_length(r_mid_sc)?;
    let width = map.inverse_length(chosen.value())?;
    let feature = FittedFeature::Circle2 { c: center, r: r_mid }.canonical()?;
    let stats = ResidualStats::of(&residuals)?;
    let critical_points = chosen.certificate().active_set().to_vec();
    let certificate = Some(chosen.into_certificate());

    Ok((
        MzCircle2Result {
            algorithm: id(),
            feature,
            width,
            r_inner,
            r_outer,
            residuals,
            stats,
            critical_points,
            max_violation: f::total_max(0.0, worst_viol)?,
            certificate,
            local_optimum_only,
            seed,
            starts: extra.starts,
            failed_starts,
        },
        map,
    ))
}

/// Fits a certified minimum-zone 2D circle (roundness).
pub fn fit_mz_circle2(
    pts: &PointsView<'_>,
    params: &FitParams,
    extra: &MzCircle2Params,
    seed: Option<u64>,
) -> Result<MzCircle2Result, FitError> {
    Ok(fit_mz_circle2_inner(pts, params, extra, seed)?.0)
}

/// Compares a sequential-LP minimum-zone 2D circle result against the exact Voronoi solution
/// of `KRN.MZ.VOR` (`fid_fit::vor::mz_circle2d_xy`, REQ-KRN.MZ.RND.CIRC-005).
///
/// Fails with [`VoronoiDisagreement`] naming `case_id` if width or centre differs by more
/// than `tol` (metres).
pub fn check_voronoi_agreement(
    case_id: &str,
    mz: &MzCircle2Result,
    pts_xy: &[V2],
    tol: f64,
) -> Result<MzCircle, Result<VoronoiDisagreement, FitError>> {
    let vor = mz_circle2d_xy(pts_xy).map_err(Err)?;
    let FittedFeature::Circle2 { c, .. } = mz.feature else {
        return Err(Err(FitError::InvalidParams { field: "feature" }));
    };
    let width_diff = (mz.width - vor.width).abs();
    let center_diff = red::norm2(&[c[0] - vor.center[0], c[1] - vor.center[1]]).map_err(|e| Err(FitError::Num(e)))?;
    if width_diff > tol || center_diff > tol {
        return Err(Ok(VoronoiDisagreement { case_id: case_id.to_string(), width_diff, center_diff, tol }));
    }
    Ok(vor)
}

impl Fit for Circle2Mz {
    fn algorithm_id(&self) -> AlgorithmId {
        id()
    }

    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, seed: Option<u64>) -> Result<FitResult, FitError> {
        let (res, map) = fit_mz_circle2_inner(pts, params, &MzCircle2Params::default(), seed)?;
        let (rank, cond, stop, cert) = match &res.certificate {
            Some(c) => {
                let mut support = alloc::<usize>(res.critical_points.len())?;
                for l in &res.critical_points {
                    let idx = l.point as usize;
                    if !support.contains(&idx) {
                        support.push(idx);
                    }
                }
                support.sort_unstable();
                let gap = map.inverse_length(c.value_bound())?;
                (c.params().len(), c.cond(), StopReason::Exchange, Some(contract::Certificate { support, gap }))
            }
            None => (3, 0.0, StopReason::ClosedForm, None),
        };
        let diag = FitDiagnostics { grad_norm: 0.0, rank, cond, iterations: 0, stop };
        FitResult::assemble(&res.feature, res.residuals, diag, cert, res.algorithm)
    }
}

// ---------------------------------------------------------------------------
// Registry command data row (REQ-KRN.MZ.RND.CIRC-006)
// ---------------------------------------------------------------------------

const fn fld(
    name: &'static str,
    ty: ValueType,
    quantity: Option<(&'static str, &'static str)>,
    required: bool,
) -> Field {
    Field { name, ty, quantity, required }
}

const C: Option<(&str, &str)> = Some(("count", "1"));
const L: Option<(&str, &str)> = Some(("length", "m"));

/// Extra parameters for `fit.mz_circle2@1`.
pub const MZ_CIRCLE2_PARAMS: [Field; 1] = [fld("starts", ValueType::Integer, C, true)];

/// Extra result fields for `fit.mz_circle2@1`.
pub const MZ_CIRCLE2_RESULT: [Field; 5] = [
    fld("zone_width", ValueType::Number, L, true),
    fld("r_inner", ValueType::Number, L, true),
    fld("r_outer", ValueType::Number, L, true),
    fld("local_optimum_only", ValueType::Boolean, None, true),
    fld("certificate.digest", ValueType::String, None, false),
];

/// Command data row for `fit.mz_circle2@1` (W2, dry-run R1, D0 per seed).
pub fn command_row() -> Result<CommandRow, FitError> {
    let mut row = contract::command_row(Primitive::Circle2, Method::MinimumZone, VERSION, Determinism::D0)?;
    row.params
        .try_reserve_exact(MZ_CIRCLE2_PARAMS.len())
        .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    row.params.extend_from_slice(&MZ_CIRCLE2_PARAMS);
    row.result
        .try_reserve_exact(MZ_CIRCLE2_RESULT.len())
        .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    row.result.extend_from_slice(&MZ_CIRCLE2_RESULT);
    Ok(row)
}
