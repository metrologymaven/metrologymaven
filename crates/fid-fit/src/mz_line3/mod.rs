//! Minimum-zone 3D line (axis straightness in space). Spec node `KRN.MZ.LIN.LINE3`.
//!
//! Computes the smallest circular cylinder containing all points and reports its axis
//! (`FittedFeature::Line3`) and its **zone diameter** (`2 * R`, SI metres).
//!
//! # Pinned procedure
//!
//! 1. **Parameters.** [`FitParams::validate`]; method must be [`Method::MinimumZone`], weights
//!    [`Weights::Uniform`]; accepted constraints are none or `FixedAxisDirection`.
//!    `aspect_min` must be finite and nonnegative (`DEFAULT_ASPECT_MIN = 2.0`).
//! 2. **Checks & normalisation.** Free direction runs [`check_points`] for `Primitive::Line3`
//!    (`n >= 2`, rank >= 1); fixed direction requires `n >= 1`. Points are centred and
//!    power-of-two scaled by [`Centred::new`].
//! 3. **Least-squares axis & aspect guard (REQ-KRN.MZ.LIN.LINE3-003).** Free direction starts
//!    from [`Line3Ls`] axis `A` (errors pass through). Build the orthonormal frame
//!    `(P0 = 0, A, E1, E2)` and measure the axial extent `L = max_i(q_i . A) - min_i(q_i . A)`
//!    and radial spread `rho_max = max_i |q_i - (q_i . A) A|`.
//!    - If `rho_max == 0` (exact collinear 3D line), return the exact zero-diameter closed-form
//!      line (`diameter = 0.0`, `local_optimum_only = false`, `certificate = None`).
//!    - If `L < aspect_min * rho_max` (not line-like, e.g. a disc), return
//!      [`FitError::IllConditioned`] with `cond = Some(rho_max / L)` (or `None` when `L == 0`).
//! 4. **SOCP initial centre.** Solve the exact fixed-axis 2D minimum enclosing circle in
//!    `(E1, E2)` via `fid_solv::conic::solve_socp`, shifting `P0` to `(cx, cy)` and setting
//!    feasible radius `r0 = max_i rho_i(P0, A)`.
//! 5. **Sequential linearisation & certification.** Run `fid_solv::seq::solve` (`CertKind::Zone`)
//!    on [`MzLine3Problem`] from primary start `[0, 0, 0, 0, r0]` (fixed direction `[0, 0, r0]`)
//!    plus 4 deterministic small-tilt ring starts when free, picking the certified minimum
//!    radius. If starts disagree (or any converged start failed certification), mark
//!    `local_optimum_only = true` on the certificate.
//! 6. **Recomputed diameter (REQ-KRN.MZ.LIN.LINE3-002).** From the certified parameters and
//!    critical points `contacts` (active-set point indices, ascending), recompute the radius
//!    `r = max_{k in contacts} rho_k(p*)` at the final axis and set `diameter = 2 * r * scale`.
//!    Fail closed with [`FitError::SideViolation`] if any point exceeds `r` by more than
//!    [`INSIDE_TOL`].
//!
//! # Determinism
//!
//! D0: single thread, no FMA, no `f64::min`/`max`, `fid_num::red` for all sums/dots/norms,
//! KNP-1 `fid-clarabel` conic solver, no hash containers, `seed` ignored (`null`). Bit-identical
//! on x86-64, ARM64 and wasm32.

#![allow(clippy::needless_range_loop)]

mod encode;
mod problem;

pub use encode::{encode, output_digest};
pub use problem::{PENALTY, R_SCALE};

use core::cmp::Ordering;

use fid_kci::AlgorithmId;
use fid_num::{f, red, NumError};
use fid_solv::cert::{CertKind, Certificate, Certified};
use fid_solv::seq::{self, SeqConfig};
use fid_types::Determinism;

use crate::contract::{
    self, algorithm_id, check_points, normalize_direction, unit_direction, Centred, CommandRow, Constraint, Field, Fit,
    FitDiagnostics, FitError, FitParams, FitResult, FittedFeature, Method, Normalisation, PointsView, Primitive,
    ResidualStats, StopReason, ValueType, Weights, V3,
};
use crate::ls_lin::Line3Ls;
use problem::{socp_fixed_axis_mec, Frame, MzLine3Problem};

/// Algorithm version of `fit.line3.mz`.
pub const VERSION: (u32, u32, u32) = (1, 0, 0);

/// Default minimum ratio `axial_extent / radial_spread` (REQ-KRN.MZ.LIN.LINE3-003) [unverified].
pub const DEFAULT_ASPECT_MIN: f64 = 2.0;

/// Largest accepted excess of any point beyond the recomputed zone radius (metres).
pub const INSIDE_TOL: f64 = 1e-11;

/// Agreement tolerance across deterministic starts (relative to `r0` for lengths).
pub const AGREE_TOL: f64 = 1e-9;

const OP: &str = "fit.mz_line3";

/// Minimum-zone 3D line fit (`fit.line3.mz@1.0.0`, spec node `KRN.MZ.LIN.LINE3`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Line3Mz;

/// Configuration options for [`fit_mz_line3`].
#[derive(Debug, Clone, PartialEq)]
pub struct MzLine3Params {
    /// Minimum ratio of axial extent to radial spread (`axial_extent >= aspect_min * radial_spread`).
    pub aspect_min: f64,
    /// Optional sequential-solver configuration override (defaults to `SeqConfig::new(CertKind::Zone)`).
    pub seq: Option<SeqConfig>,
}

impl Default for MzLine3Params {
    fn default() -> Self {
        Self { aspect_min: DEFAULT_ASPECT_MIN, seq: None }
    }
}

/// Full certified result of [`fit_mz_line3`].
#[derive(Debug, Clone, PartialEq)]
pub struct MzLine3Result {
    /// Algorithm ID (`fit.line3.mz@1.0.0`).
    pub algorithm: AlgorithmId,
    /// Canonical mid-zone axis (`FittedFeature::Line3 { p, d }`, metres).
    pub feature: FittedFeature,
    /// Minimum zone **diameter** `2 * R` in SI metres, recomputed from the critical points at the final axis.
    pub diameter: f64,
    /// Radial distances of every point from the fitted axis (metres, non-negative, point order).
    pub residuals: Vec<f64>,
    /// Residual statistics over [`Self::residuals`].
    pub stats: ResidualStats,
    /// Critical point indices touching the enclosing cylinder (sorted ascending).
    pub critical_points: Vec<u32>,
    /// Maximum radial excess of any point over `diameter / 2` (metres).
    pub max_violation: f64,
    /// Active-set KKT certificate (`None` for exact collinear zero-diameter or 2-point fixed-axis SOCP).
    pub certificate: Option<Certificate>,
    /// `true` whenever starts disagreed or a converged start failed certification.
    pub local_optimum_only: bool,
    /// Sequential iterations taken (diagnostic only; excluded from canonical bytes/digest).
    pub iterations: u32,
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
    algorithm_id(Primitive::Line3, Method::MinimumZone, VERSION).unwrap_or_else(|_| AlgorithmId {
        module: "fit".to_string(),
        feature: "line3".to_string(),
        method: "mz".to_string(),
        version: VERSION,
    })
}

fn build_frame(p0: V3, a: V3) -> Result<Frame, FitError> {
    let a = unit_direction(a)?;
    let mut k = 0usize;
    for j in 1..3 {
        if a[j].abs() < a[k].abs() {
            k = j;
        }
    }
    let mut axis_k = [0.0; 3];
    axis_k[k] = 1.0;
    let e1 = unit_direction([axis_k[0] - a[k] * a[0], axis_k[1] - a[k] * a[1], axis_k[2] - a[k] * a[2]])?;
    let e2 = unit_direction([a[1] * e1[2] - a[2] * e1[1], a[2] * e1[0] - a[0] * e1[2], a[0] * e1[1] - a[1] * e1[0]])?;
    Ok(Frame { p0, a, e1, e2 })
}

fn validate_params(pts: &PointsView<'_>, params: &FitParams, extra: &MzLine3Params) -> Result<Option<V3>, FitError> {
    params.validate(pts)?;
    if params.method != Method::MinimumZone {
        return Err(FitError::InvalidParams { field: "method" });
    }
    if params.weights != Weights::Uniform {
        return Err(FitError::InvalidParams { field: "weights" });
    }
    if !(extra.aspect_min.is_finite() && extra.aspect_min >= 0.0) {
        return Err(FitError::InvalidParams { field: "aspect_min" });
    }
    let mut fixed = None;
    for c in &params.constraints {
        match c {
            Constraint::FixedAxisDirection(d) => fixed = Some(*d),
            _ => return Err(FitError::InvalidParams { field: "constraints" }),
        }
    }
    Ok(fixed)
}

fn cert_err(e: fid_solv::cert::CertError) -> FitError {
    match e {
        fid_solv::cert::CertError::Numeric(n) => FitError::Num(n),
        _ => FitError::SeqFailed { cause: "cert", starts: 1 },
    }
}

fn fit_mz_line3_inner(
    pts: &PointsView<'_>,
    params: &FitParams,
    extra: &MzLine3Params,
) -> Result<(MzLine3Result, Normalisation), FitError> {
    let fixed = validate_params(pts, params, extra)?;
    if fixed.is_none() {
        check_points(Primitive::Line3, pts, &params.thresholds)?;
    } else if pts.is_empty() {
        return Err(FitError::TooFewPoints { n: 0, need: 1 });
    }

    let centred = Centred::new(pts)?;
    let map = *centred.map();
    let dir_free = fixed.is_none();

    let (axis_a, ls_feature) = match fixed {
        Some(f) => {
            let d = normalize_direction(f)?;
            (d, FittedFeature::Line3 { p: map.centroid(), d })
        }
        None => {
            let mut ls_p = FitParams::new(Method::LeastSquares);
            ls_p.thresholds = params.thresholds;
            let ls = Line3Ls.fit(pts, &ls_p, None)?;
            let d = match ls.feature {
                FittedFeature::Line3 { d, .. } => d,
                _ => return Err(FitError::InvalidParams { field: "feature" }),
            };
            (d, ls.feature)
        }
    };

    let fr0 = build_frame([0.0; 3], axis_a)?;

    // Measure axial extent and radial spread about the initial axis.
    let mut t_lo = f64::INFINITY;
    let mut t_hi = f64::NEG_INFINITY;
    let mut rho_max = 0.0_f64;
    for q in centred.xyz().chunks_exact(3) {
        let t = red::dot(q, &fr0.a)?;
        t_lo = f::total_min(t_lo, t)?;
        t_hi = f::total_max(t_hi, t)?;
        let perp = [q[0] - t * fr0.a[0], q[1] - t * fr0.a[1], q[2] - t * fr0.a[2]];
        let rho = red::norm2(&perp)?;
        rho_max = f::total_max(rho_max, rho)?;
    }
    let axial_extent = t_hi - t_lo;

    // Exact collinear 3D line -> closed-form zero diameter.
    if !(rho_max.is_normal() && rho_max > 0.0) {
        let residuals = vec![0.0_f64; pts.len()];
        let stats = ResidualStats::of(&residuals)?;
        return Ok((
            MzLine3Result {
                algorithm: id(),
                feature: ls_feature.canonical()?,
                diameter: 0.0,
                residuals,
                stats,
                critical_points: Vec::new(),
                max_violation: 0.0,
                certificate: None,
                local_optimum_only: false,
                iterations: 0,
            },
            map,
        ));
    }

    // REQ-KRN.MZ.LIN.LINE3-003: aspect-ratio guard (disc-like clouds rejected).
    if dir_free && axial_extent < extra.aspect_min * rho_max {
        let cond = if axial_extent > 0.0 {
            let c = rho_max / axial_extent;
            if c.is_finite() {
                Some(c)
            } else {
                None
            }
        } else {
            None
        };
        return Err(FitError::IllConditioned { cond });
    }

    // Exact SOCP fixed-axis 2D MEC to place P0 at the exact optimal centre for axis A.
    let (cx, cy, _r_socp) = socp_fixed_axis_mec(centred.xyz(), &fr0)?;
    let p0_shifted = [
        (fr0.p0[0] + cx * fr0.e1[0]) + cy * fr0.e2[0],
        (fr0.p0[1] + cx * fr0.e1[1]) + cy * fr0.e2[1],
        (fr0.p0[2] + cx * fr0.e1[2]) + cy * fr0.e2[2],
    ];
    let fr = Frame { p0: p0_shifted, a: fr0.a, e1: fr0.e1, e2: fr0.e2 };

    let mut data_scale = 0.0_f64;
    for &x in centred.xyz() {
        data_scale = f::total_max(data_scale, x.abs())?;
    }
    let mut prob = MzLine3Problem { q: centred.xyz(), fr, dir: dir_free, len_scale: 1.0, data_scale, r0: 1.0 };
    let zero: Vec<f64> = if dir_free { vec![0.0; 5] } else { vec![0.0; 3] };
    let (r0, _lo0) = prob.extremes(&zero).map_err(cert_err)?;
    if !(r0.is_normal() && r0 > 0.0) {
        let residuals = vec![0.0_f64; pts.len()];
        let stats = ResidualStats::of(&residuals)?;
        let (foot, d) = prob.line(&zero).map_err(cert_err)?;
        let feature = map.inverse_feature(&FittedFeature::Line3 { p: foot, d })?.canonical()?;
        return Ok((
            MzLine3Result {
                algorithm: id(),
                feature,
                diameter: 0.0,
                residuals,
                stats,
                critical_points: Vec::new(),
                max_violation: 0.0,
                certificate: None,
                local_optimum_only: false,
                iterations: 0,
            },
            map,
        ));
    }
    prob.r0 = r0;
    prob.len_scale = r0;
    prob.data_scale = f::total_max(data_scale, r0)?;

    let default_cfg = seq_config();
    let cfg = extra.seq.as_ref().unwrap_or(&default_cfg);

    // Deterministic starts: primary SOCP-centred LS axis + 4 small-tilt ring starts when free.
    let tilt = if axial_extent > 0.0 { r0 / axial_extent } else { 0.0 };
    let mut starts: Vec<Vec<f64>> = Vec::new();
    if dir_free {
        let raw = [(0.0, 0.0), (tilt, 0.0), (-tilt, 0.0), (0.0, tilt), (0.0, -tilt)];
        for &(a, b) in &raw {
            let mut st = vec![a, b, 0.0, 0.0, r0];
            let (hi, _) = prob.extremes(&st).map_err(cert_err)?;
            st[4] = hi;
            starts.push(st);
        }
    } else {
        starts.push(vec![0.0, 0.0, r0]);
    }

    let mut best: Option<(Certified, u32)> = None;
    let mut any_disagree = false;
    let mut converged_but_failed = false;
    let mut first_cause: Option<&'static str> = None;

    for st in &starts {
        match seq::solve(&prob, st, cfg) {
            Ok(out) => {
                let val = out.certified.value();
                match &best {
                    None => best = Some((out.certified, out.iterations)),
                    Some((cur, _)) => {
                        let diff = (val - cur.value()).abs();
                        if diff > AGREE_TOL * r0 {
                            any_disagree = true;
                        }
                        if f::total_cmp(val, cur.value())? == Ordering::Less {
                            best = Some((out.certified, out.iterations));
                        }
                    }
                }
            }
            Err(e) => {
                if e.converged {
                    converged_but_failed = true;
                }
                if first_cause.is_none() {
                    first_cause = Some(e.cause.code());
                }
            }
        }
    }

    // Special case: fixed direction with 2 antipodal active points cannot form a 3x3 active set
    // (expected_active = 3), so fall back to the exact SOCP solution!
    let Some((certified, iterations)) = best else {
        if !dir_free {
            let dists = prob.distances(&zero).map_err(cert_err)?;
            let mut r_sc = 0.0_f64;
            for &d in &dists {
                r_sc = f::total_max(r_sc, d)?;
            }
            let mut contacts = Vec::new();
            let mut residuals = alloc(dists.len())?;
            for (i, &d) in dists.iter().enumerate() {
                if (r_sc - d).abs() <= 1e-9 * r_sc {
                    contacts.push(i as u32);
                }
                residuals.push(map.inverse_length(d)?);
            }
            let (foot, d) = prob.line(&zero).map_err(cert_err)?;
            let feature = map.inverse_feature(&FittedFeature::Line3 { p: foot, d })?.canonical()?;
            let diameter = map.inverse_length(2.0 * r_sc)?;
            let stats = ResidualStats::of(&residuals)?;
            return Ok((
                MzLine3Result {
                    algorithm: id(),
                    feature,
                    diameter,
                    residuals,
                    stats,
                    critical_points: contacts,
                    max_violation: 0.0,
                    certificate: None,
                    local_optimum_only: false,
                    iterations: 0,
                },
                map,
            ));
        }
        return Err(FitError::SeqFailed { cause: first_cause.unwrap_or("solver"), starts: starts.len() as u32 });
    };

    let p_cert = certified.params().to_vec();
    let mut contacts: Vec<u32> = alloc(certified.certificate().active_set().len())?;
    contacts.extend(certified.certificate().active_set().iter().map(|l| l.point));
    contacts.sort_unstable();

    // REQ-KRN.MZ.LIN.LINE3-002: recompute radius directly from critical points at the final axis.
    let dists = prob.distances(&p_cert).map_err(cert_err)?;
    let mut r_recomp = 0.0_f64;
    for &idx in &contacts {
        let dk = *dists.get(idx as usize).ok_or(FitError::InvalidParams { field: "contacts" })?;
        r_recomp = f::total_max(r_recomp, dk)?;
    }

    let mut residuals = alloc(dists.len())?;
    let mut worst_idx = 0usize;
    let mut worst_viol = 0.0_f64;
    for (i, &d) in dists.iter().enumerate() {
        let viol = map.inverse_length(d - r_recomp)?;
        if f::total_cmp(viol, worst_viol)? == Ordering::Greater {
            worst_viol = viol;
            worst_idx = i;
        }
        residuals.push(map.inverse_length(d)?);
    }
    if f::total_cmp(worst_viol, INSIDE_TOL)? == Ordering::Greater {
        return Err(FitError::SideViolation { index: worst_idx, violation: worst_viol });
    }

    let diameter = map.inverse_length(2.0 * r_recomp)?;
    let (foot, d_unit) = prob.line(&p_cert).map_err(cert_err)?;
    let feature = map.inverse_feature(&FittedFeature::Line3 { p: foot, d: d_unit })?.canonical()?;
    let stats = ResidualStats::of(&residuals)?;

    let local_optimum_only =
        dir_free && (any_disagree || converged_but_failed || certified.certificate().flags().local_optimum_only);
    let mut cert = certified.into_certificate();
    if local_optimum_only && !cert.flags().local_optimum_only {
        cert.set_local_optimum_only().map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "cert_flag" }))?;
    }

    Ok((
        MzLine3Result {
            algorithm: id(),
            feature,
            diameter,
            residuals,
            stats,
            critical_points: contacts,
            max_violation: f::total_max(0.0, worst_viol)?,
            certificate: Some(cert),
            local_optimum_only,
            iterations,
        },
        map,
    ))
}

/// Fits a certified minimum-zone 3D line (axis straightness in space).
pub fn fit_mz_line3(
    pts: &PointsView<'_>,
    params: &FitParams,
    extra: &MzLine3Params,
) -> Result<MzLine3Result, FitError> {
    Ok(fit_mz_line3_inner(pts, params, extra)?.0)
}

impl Fit for Line3Mz {
    fn algorithm_id(&self) -> AlgorithmId {
        id()
    }

    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, _seed: Option<u64>) -> Result<FitResult, FitError> {
        let (res, map) = fit_mz_line3_inner(pts, params, &MzLine3Params::default())?;
        let (rank, cond, stop, cert) = match &res.certificate {
            Some(c) => {
                let mut support = alloc(res.critical_points.len())?;
                support.extend(res.critical_points.iter().map(|&i| i as usize));
                let gap = map.inverse_length(2.0 * c.value_bound())?;
                (c.params().len(), c.cond(), StopReason::Exchange, Some(contract::Certificate { support, gap }))
            }
            None => (3, 0.0, StopReason::ClosedForm, None),
        };
        let diag = FitDiagnostics { grad_norm: 0.0, rank, cond, iterations: 0, stop };
        FitResult::assemble(&res.feature, res.residuals, diag, cert, res.algorithm)
    }
}

// ---------------------------------------------------------------------------
// Registry command data row (REQ-KRN.MZ.LIN.LINE3-005)
// ---------------------------------------------------------------------------

const fn fld(
    name: &'static str,
    ty: ValueType,
    quantity: Option<(&'static str, &'static str)>,
    required: bool,
) -> Field {
    Field { name, ty, quantity, required }
}

const D: Option<(&str, &str)> = Some(("dimensionless", "1"));
const L: Option<(&str, &str)> = Some(("length", "m"));

/// Extra parameters for `fit.mz_line3@1`.
pub const MZ_LINE3_PARAMS: [Field; 1] = [fld("aspect_min", ValueType::Number, D, true)];

/// Extra result fields for `fit.mz_line3@1`.
pub const MZ_LINE3_RESULT: [Field; 3] = [
    fld("zone_diameter", ValueType::Number, L, true),
    fld("local_optimum_only", ValueType::Boolean, None, true),
    fld("certificate.digest", ValueType::String, None, false),
];

/// Command data row for `fit.mz_line3@1` (W2, dry-run R1, D0).
pub fn command_row() -> Result<CommandRow, FitError> {
    let mut row = contract::command_row(Primitive::Line3, Method::MinimumZone, VERSION, Determinism::D0)?;
    row.params
        .try_reserve_exact(MZ_LINE3_PARAMS.len())
        .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    row.params.extend_from_slice(&MZ_LINE3_PARAMS);
    row.result
        .try_reserve_exact(MZ_LINE3_RESULT.len())
        .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    row.result.extend_from_slice(&MZ_LINE3_RESULT);
    Ok(row)
}
