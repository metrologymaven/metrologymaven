//! Rigid point-pair transform solver (Kabsch/Umeyama closed-form SVD + constrained
//! Gauss-Newton over `ALN.BFIT.CONSTR` DoF locks; spec node `ALN.XFM.PAIRS`,
//! `REQ-001`..`REQ-006`).
//!
//! Pure KNP-1 kernel math: SI metres, no I/O, no `unsafe`, no panics, no `HashMap`/`HashSet`,
//! no FMA, `fid_num::la::{svd, lstsq}` and `fid_num::f::sqrt`, fixed-order reductions, NaN is
//! an error.

use crate::constr::{linearise, pose_from_params, reduced_jacobian, ConstrError, ConstraintSet};
use crate::report::{Disposition, Entry, EntryKind, EntryRef, ReportBuilder, ResidualReport};
use core::cmp::Ordering;
use core::fmt;
use fid_geo_frame::{Matrix3, Rigid};
use fid_num::f as numf;
use fid_num::la::{lstsq, svd, Mat};

/// Algorithm ID recorded on alignments created by `align.run_point_pairs@1`.
pub const PAIRS_ALGORITHM_ID: &str = "align.xfm.pairs@1.0.0";
/// Minimum ratio `sigma_2 / sigma_1` of the weighted centred measured points before refusing
/// with `collinear_pairs` (`REQ-004`).
pub const COLLINEAR_RATIO_MIN: f64 = 1e-6;
/// Maximum Gauss-Newton iterations in the locked case (`REQ-002`).
pub const MAX_LOCKED_ITERS: usize = 50;
/// Stopping threshold on the Gauss-Newton parameter update norm (`REQ-002`).
pub const LOCKED_STEP_TOL: f64 = 1e-14;

/// Residual for one point pair (`REQ-005`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairResidual {
    pub index: u64,
    /// `R * measured + t - reference` in metres.
    pub vector_m: [f64; 3],
    /// Euclidean length `‖vector_m‖` in metres.
    pub length_m: f64,
    /// Non-negative weight applied to this pair (`1.0` when unweighted).
    pub weight: f64,
}

/// Solution returned by [`point_pairs_rigid`] (`REQ-001`..`REQ-005`).
#[derive(Debug, Clone, PartialEq)]
pub struct PairSolution {
    pub transform: Rigid,
    pub residuals: Vec<PairResidual>,
    pub weighted_rms_m: f64,
    pub max_len_m: f64,
    pub iterations: usize,
    pub report: ResidualReport,
}

/// Errors returned by [`point_pairs_rigid`] (`REQ-004`).
#[derive(Debug, Clone, PartialEq)]
pub enum PairsError {
    TooFewPairs { count: usize },
    LengthMismatch { measured: usize, reference: usize },
    InvalidWeight { index: u64 },
    CollinearPairs { ratio: f64 },
    NanInput,
    AllDofLocked,
    Constraint(ConstrError),
    SolveFailed(&'static str),
}

impl PairsError {
    pub fn code(&self) -> &'static str {
        match self {
            PairsError::TooFewPairs { .. } => "too_few_pairs",
            PairsError::LengthMismatch { .. } => "too_few_pairs",
            PairsError::InvalidWeight { .. } => "invalid_weight",
            PairsError::CollinearPairs { .. } => "collinear_pairs",
            PairsError::NanInput => "nan_input",
            PairsError::AllDofLocked => "all_dof_locked",
            PairsError::Constraint(e) => e.code(),
            PairsError::SolveFailed(_) => "solve_failed",
        }
    }
}

impl fmt::Display for PairsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PairsError::TooFewPairs { count } => write!(f, "too_few_pairs{{count={count}}}"),
            PairsError::LengthMismatch { measured, reference } => {
                write!(f, "too_few_pairs{{measured={measured}, reference={reference}}}")
            }
            PairsError::InvalidWeight { index } => write!(f, "invalid_weight{{index={index}}}"),
            PairsError::CollinearPairs { ratio } => write!(f, "collinear_pairs{{ratio={ratio:e}}}"),
            PairsError::NanInput => f.write_str("nan_input"),
            PairsError::AllDofLocked => f.write_str("all_dof_locked"),
            PairsError::Constraint(e) => e.fmt(f),
            PairsError::SolveFailed(msg) => write!(f, "solve_failed{{{msg}}}"),
        }
    }
}

impl std::error::Error for PairsError {}

impl From<ConstrError> for PairsError {
    fn from(e: ConstrError) -> Self {
        PairsError::Constraint(e)
    }
}

/// Solves for the rigid transform mapping `measured` onto `reference` minimizing weighted sum
/// of squared distances (`REQ-001`..`REQ-006`).
///
/// When `locks` is `None`, solves in closed form via weighted cross-covariance SVD with
/// Umeyama reflection correction (`det R = +1`). When `locks` is `Some(cs)`, runs
/// fixed-schedule Gauss-Newton over `cs`'s free parameters (at most 50 iterations, stopping
/// below `1e-14`), holding locked components at exactly `0`.
pub fn point_pairs_rigid(
    measured: &[[f64; 3]],
    reference: &[[f64; 3]],
    weights: Option<&[f64]>,
    locks: Option<&ConstraintSet>,
) -> Result<PairSolution, PairsError> {
    if measured.len() != reference.len() {
        return Err(PairsError::LengthMismatch { measured: measured.len(), reference: reference.len() });
    }
    let n = measured.len();
    if n < 3 {
        return Err(PairsError::TooFewPairs { count: n });
    }
    if let Some(ws) = weights {
        if ws.len() != n {
            return Err(PairsError::InvalidWeight { index: u64::try_from(ws.len()).unwrap_or(u64::MAX) });
        }
    }

    // Validate coordinates & weights in fixed order.
    let mut w_vec = Vec::with_capacity(n);
    let mut w_sum = 0.0_f64;
    let mut active_count = 0usize;
    for i in 0..n {
        check_vec3(measured[i])?;
        check_vec3(reference[i])?;
        let w = match weights {
            Some(ws) => ws[i],
            None => 1.0,
        };
        if !w.is_finite() || w.total_cmp(&0.0) == Ordering::Less {
            return Err(PairsError::InvalidWeight { index: u64::try_from(i).unwrap_or(u64::MAX) });
        }
        if w.total_cmp(&0.0) == Ordering::Greater {
            active_count += 1;
            w_sum += w;
        }
        w_vec.push(w);
    }
    if !w_sum.is_finite() || w_sum.total_cmp(&0.0) != Ordering::Greater || active_count < 3 {
        return Err(PairsError::CollinearPairs { ratio: 0.0 });
    }

    // Weighted centroids.
    let mut p_bar = [0.0_f64; 3];
    let mut q_bar = [0.0_f64; 3];
    for i in 0..n {
        let w = w_vec[i];
        if w == 0.0 {
            continue;
        }
        for k in 0..3 {
            p_bar[k] += w * measured[i][k];
            q_bar[k] += w * reference[i][k];
        }
    }
    for k in 0..3 {
        p_bar[k] /= w_sum;
        q_bar[k] /= w_sum;
    }
    check_vec3(p_bar)?;
    check_vec3(q_bar)?;

    // Centring and RMS radius scaling.
    let mut rms_sq = 0.0_f64;
    for i in 0..n {
        let w = w_vec[i];
        if w == 0.0 {
            continue;
        }
        let pc = sub(measured[i], p_bar);
        rms_sq += w * dot(pc, pc);
    }
    if !rms_sq.is_finite() || rms_sq.total_cmp(&0.0) != Ordering::Greater {
        return Err(PairsError::CollinearPairs { ratio: 0.0 });
    }
    let s_rms = numf::sqrt(rms_sq / w_sum).map_err(|_| PairsError::NanInput)?;
    if !s_rms.is_finite() || s_rms.total_cmp(&0.0) != Ordering::Greater {
        return Err(PairsError::CollinearPairs { ratio: 0.0 });
    }
    let inv_s = 1.0 / s_rms;

    // Collinearity check via 3x3 weighted centred scatter matrix S_p = (1/W) sum w_i (pc_i/s)(pc_i/s)^T.
    let mut sp = [0.0_f64; 9];
    for i in 0..n {
        let w = w_vec[i];
        if w == 0.0 {
            continue;
        }
        let pc = scale(sub(measured[i], p_bar), inv_s);
        let wn = w / w_sum;
        for a in 0..3 {
            for b in 0..3 {
                sp[a * 3 + b] += wn * (pc[a] * pc[b]);
            }
        }
    }
    let sp_mat = Mat::new(3, 3, sp.to_vec()).map_err(|_| PairsError::SolveFailed("scatter_mat"))?;
    let sp_svd = svd(&sp_mat, 1e-15).map_err(|_| PairsError::SolveFailed("scatter_svd"))?;
    let sig = sp_svd.sigma();
    if sig.len() < 2 || sig[0].total_cmp(&0.0) != Ordering::Greater {
        return Err(PairsError::CollinearPairs { ratio: 0.0 });
    }
    let ratio_sq = if sig[1].total_cmp(&0.0) == Ordering::Greater { sig[1] / sig[0] } else { 0.0 };
    let ratio = numf::sqrt(ratio_sq).map_err(|_| PairsError::NanInput)?;
    if ratio.total_cmp(&COLLINEAR_RATIO_MIN) == Ordering::Less {
        return Err(PairsError::CollinearPairs { ratio });
    }

    let (transform, iterations) = match locks {
        None => (solve_unconstrained(measured, reference, &w_vec, w_sum, p_bar, q_bar, inv_s)?, 0usize),
        Some(cs) => solve_locked(measured, reference, &w_vec, cs)?,
    };

    build_solution(transform, measured, reference, &w_vec, iterations)
}

fn solve_unconstrained(
    measured: &[[f64; 3]],
    reference: &[[f64; 3]],
    w_vec: &[f64],
    w_sum: f64,
    p_bar: [f64; 3],
    q_bar: [f64; 3],
    inv_s: f64,
) -> Result<Rigid, PairsError> {
    let mut h_data = [0.0_f64; 9];
    for i in 0..measured.len() {
        let w = w_vec[i];
        if w == 0.0 {
            continue;
        }
        let pc = scale(sub(measured[i], p_bar), inv_s);
        let qc = scale(sub(reference[i], q_bar), inv_s);
        let wn = w / w_sum;
        for a in 0..3 {
            for b in 0..3 {
                h_data[a * 3 + b] += wn * (pc[a] * qc[b]);
            }
        }
    }
    let h_mat = Mat::new(3, 3, h_data.to_vec()).map_err(|_| PairsError::SolveFailed("h_mat"))?;
    let dec = svd(&h_mat, 1e-15).map_err(|_| PairsError::SolveFailed("h_svd"))?;
    let u = mat3_from_mat(dec.u())?;
    let v = mat3_from_mat(dec.v())?;
    let ut = transpose(&u);
    let r_raw = mat_mul(&v, &ut);
    let sign = if det3(&r_raw).total_cmp(&0.0) == Ordering::Less { -1.0 } else { 1.0 };
    let v_adj =
        [[v[0][0], v[0][1], v[0][2] * sign], [v[1][0], v[1][1], v[1][2] * sign], [v[2][0], v[2][1], v[2][2] * sign]];
    let r = canonical_mat3(mat_mul(&v_adj, &ut));
    let rp = mat_vec(&r, p_bar);
    let t = [canonical_zero(q_bar[0] - rp[0]), canonical_zero(q_bar[1] - rp[1]), canonical_zero(q_bar[2] - rp[2])];
    Rigid::new(r, t).map_err(|_| PairsError::SolveFailed("non_rigid_svd"))
}

fn solve_locked(
    measured: &[[f64; 3]],
    reference: &[[f64; 3]],
    w_vec: &[f64],
    cs: &ConstraintSet,
) -> Result<(Rigid, usize), PairsError> {
    let n_free = cs.n_free();
    if n_free == 0 {
        return Err(PairsError::AllDofLocked);
    }
    let n = measured.len();
    let mut sqrt_w = Vec::with_capacity(n);
    let mut active = Vec::with_capacity(n);
    for (i, &w) in w_vec.iter().enumerate() {
        if w.total_cmp(&0.0) == Ordering::Greater {
            let sw = numf::sqrt(w).map_err(|_| PairsError::NanInput)?;
            sqrt_w.push(sw);
            active.push(i);
        }
    }
    let m = active.len();
    let mut free = vec![0.0_f64; n_free];
    let mut iters = 0usize;

    for iter in 0..MAX_LOCKED_ITERS {
        iters = iter + 1;
        let lin = linearise(cs, &free)?;
        let pose = lin.pose();
        let r_mat = pose.r();
        let t_vec = pose.t();

        let mut neg_res = Vec::with_capacity(m * 3);
        let mut full_rows: Vec<[f64; 6]> = Vec::with_capacity(m * 3);
        let mut obj_cur = 0.0_f64;
        for (k, &idx) in active.iter().enumerate() {
            let sw = sqrt_w[k];
            let p = measured[idx];
            let q = reference[idx];
            let pred = add(mat_vec(&r_mat, p), t_vec);
            let diff = sub(pred, q);
            let j6 = lin.jacobian_rows(&p)?;
            for c in 0..3 {
                let wr = sw * diff[c];
                obj_cur += wr * wr;
                neg_res.push(-wr);
                let mut row = [0.0_f64; 6];
                for col in 0..6 {
                    row[col] = sw * j6[c][col];
                }
                full_rows.push(row);
            }
        }
        let j_red = reduced_jacobian(cs, &full_rows)?;
        let j_mat = Mat::new(j_red.rows(), j_red.cols(), j_red.data().to_vec())
            .map_err(|_| PairsError::SolveFailed("locked_jmat"))?;
        let sol = lstsq(&j_mat, &neg_res, 1e-12).map_err(|_| PairsError::SolveFailed("locked_lstsq"))?;
        let step = &sol.x;

        // Compute step norm.
        let mut step_sq = 0.0_f64;
        for &d in step {
            if !d.is_finite() {
                return Err(PairsError::NanInput);
            }
            step_sq += d * d;
        }
        let step_norm = numf::sqrt(step_sq).map_err(|_| PairsError::NanInput)?;

        // Backtracking damping to guarantee monotonic decrease on large angles.
        let mut alpha = 1.0_f64;
        let mut accepted = false;
        let mut cand = vec![0.0_f64; n_free];
        for _ in 0..16 {
            for k in 0..n_free {
                cand[k] = free[k] + (alpha * step[k]);
            }
            if let Ok(cp) = pose_from_params(cs, &cand) {
                let cpose = cp.pose();
                let cr = cpose.r();
                let ct = cpose.t();
                let mut obj_new = 0.0_f64;
                for (k, &idx) in active.iter().enumerate() {
                    let sw = sqrt_w[k];
                    let diff = sub(add(mat_vec(&cr, measured[idx]), ct), reference[idx]);
                    for &d in &diff {
                        let wr = sw * d;
                        obj_new += wr * wr;
                    }
                }
                if obj_new.total_cmp(&obj_cur) != Ordering::Greater {
                    free.copy_from_slice(&cand);
                    accepted = true;
                    break;
                }
            }
            alpha *= 0.5;
        }
        if !accepted || (step_norm * alpha).total_cmp(&LOCKED_STEP_TOL) == Ordering::Less {
            break;
        }
    }

    let certified = pose_from_params(cs, &free)?;
    Ok((certified.pose(), iters))
}

fn build_solution(
    transform: Rigid,
    measured: &[[f64; 3]],
    reference: &[[f64; 3]],
    w_vec: &[f64],
    iterations: usize,
) -> Result<PairSolution, PairsError> {
    let r = transform.r();
    let t = transform.t();
    let n = measured.len();
    let mut residuals = Vec::with_capacity(n);
    let mut rb = ReportBuilder::new();
    let mut w_sum = 0.0_f64;
    let mut w_sq_sum = 0.0_f64;
    let mut max_len = 0.0_f64;

    for i in 0..n {
        let pred = add(mat_vec(&r, measured[i]), t);
        let diff_raw = sub(pred, reference[i]);
        let vec_m = [canonical_zero(diff_raw[0]), canonical_zero(diff_raw[1]), canonical_zero(diff_raw[2])];
        let len_m = canonical_zero(norm3(vec_m)?);
        let w = w_vec[i];
        let idx = u64::try_from(i).unwrap_or(u64::MAX);
        if w.total_cmp(&0.0) == Ordering::Greater {
            w_sum += w;
            w_sq_sum += w * (len_m * len_m);
            if len_m.total_cmp(&max_len) == Ordering::Greater {
                max_len = len_m;
            }
        }
        residuals.push(PairResidual { index: idx, vector_m: vec_m, length_m: len_m, weight: w });
        let disp = if w.total_cmp(&0.0) == Ordering::Greater {
            Disposition::Used
        } else {
            Disposition::Rejected(
                crate::report::RejectReason::new("zero_weight")
                    .map_err(|_| PairsError::SolveFailed("reject_reason"))?,
            )
        };
        let weight_opt = if w.total_cmp(&0.0) == Ordering::Greater { Some(w) } else { Some(1.0) };
        rb.push(
            Entry {
                reference: EntryRef { input: 0, item: idx },
                kind: EntryKind::Pair,
                residual_m: len_m,
                vector_m: Some(vec_m),
                weight: weight_opt,
            },
            disp,
        )
        .map_err(|_| PairsError::SolveFailed("report_push"))?;
    }
    let weighted_rms_m = if w_sum.total_cmp(&0.0) == Ordering::Greater {
        canonical_zero(numf::sqrt(w_sq_sum / w_sum).map_err(|_| PairsError::NanInput)?)
    } else {
        0.0
    };
    let report = rb.finish().map_err(|_| PairsError::SolveFailed("report_finish"))?;
    Ok(PairSolution { transform, residuals, weighted_rms_m, max_len_m: canonical_zero(max_len), iterations, report })
}

// ------------------------------------------------------------------ KNP-1 helpers

fn check_vec3(v: [f64; 3]) -> Result<(), PairsError> {
    if v[0].is_finite() && v[1].is_finite() && v[2].is_finite() {
        Ok(())
    } else {
        Err(PairsError::NanInput)
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] * b[0]) + (a[1] * b[1])) + (a[2] * b[2])
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(v: [f64; 3], s: f64) -> [f64; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

fn norm3(v: [f64; 3]) -> Result<f64, PairsError> {
    check_vec3(v)?;
    let n2 = dot(v, v);
    if !n2.is_finite() {
        return Err(PairsError::NanInput);
    }
    numf::sqrt(n2).map_err(|_| PairsError::NanInput)
}

fn mat3_from_mat(m: &Mat) -> Result<Matrix3, PairsError> {
    if m.rows() != 3 || m.cols() != 3 {
        return Err(PairsError::SolveFailed("expected_3x3"));
    }
    let d = m.data();
    Ok([[d[0], d[1], d[2]], [d[3], d[4], d[5]], [d[6], d[7], d[8]]])
}

fn transpose(a: &Matrix3) -> Matrix3 {
    [[a[0][0], a[1][0], a[2][0]], [a[0][1], a[1][1], a[2][1]], [a[0][2], a[1][2], a[2][2]]]
}

fn mat_mul(a: &Matrix3, b: &Matrix3) -> Matrix3 {
    let mut c = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            c[i][j] = ((a[i][0] * b[0][j]) + (a[i][1] * b[1][j])) + (a[i][2] * b[2][j]);
        }
    }
    c
}

fn mat_vec(a: &Matrix3, v: [f64; 3]) -> [f64; 3] {
    [dot(a[0], v), dot(a[1], v), dot(a[2], v)]
}

fn det3(r: &Matrix3) -> f64 {
    ((r[0][0] * ((r[1][1] * r[2][2]) - (r[1][2] * r[2][1]))) - (r[0][1] * ((r[1][0] * r[2][2]) - (r[1][2] * r[2][0]))))
        + (r[0][2] * ((r[1][0] * r[2][1]) - (r[1][1] * r[2][0])))
}

fn canonical_mat3(mut r: Matrix3) -> Matrix3 {
    for row in &mut r {
        for v in row {
            *v = canonical_zero(*v);
        }
    }
    r
}

fn canonical_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}
