//! Cylindricity evaluation (`GDT.FORM.CYL`, `spec/gdt/form/cyl/NODE.md`,
//! `REQ-001`..`REQ-003`, `AC-1`..`AC-4`).
//!
//! Pure KNP-1 kernel math: no I/O, no `unsafe`, no panics, no `HashMap`/`HashSet`, no FMA,
//! `fid_num::f` for square roots, `fid_num::la` for small active-set linear solves,
//! `f64::total_cmp` with lowest-index tie breaking, NaN rejected.

use crate::dmt::{OperatorChain, OperatorKind};
use crate::form::flat::FilterProvider;
use crate::form::{
    evaluate_form, signed_residuals_of, FitOutcome, FitProvider, FormElement, FormError, FormPayload, KrnFitProvider,
};
use core::cmp::Ordering;
use fid_fit::contract::{normalize_direction, FitParams, FittedFeature, Method, PointsView};
use fid_fit::ls_cyl;
use fid_num::{
    f as numf,
    la::{lstsq, Mat},
};

/// Algorithm ID for the deterministic multi-start KNP-1 minimum-zone cylinder fit (`GDT.FORM.CYL`).
pub const ALG_CYLINDER_MZ: &str = "fit.cylinder.mz@1.0.0";

/// Pure result of [`evaluate_cylindricity`].
#[derive(Debug, Clone, PartialEq)]
pub struct CylindricityResult {
    /// Shared `GDT.FORM.FCORE` form payload (`FormElement::Cylinder`).
    pub payload: FormPayload,
    /// Ordered algorithm IDs of any filtration/resampling steps applied before fitting.
    pub filter_algorithms: Vec<String>,
}

/// Default [`FitProvider`] for `GDT.FORM.CYL`: supplies deterministic multi-start minimum-zone
/// cylinder fitting (`fit.cylinder.mz@1.0.0`) and delegates all other elements and least-squares
/// fits to [`KrnFitProvider`].
#[derive(Debug, Clone, Copy, Default)]
pub struct CylindricityKrnProvider;

impl FitProvider for CylindricityKrnProvider {
    fn mz_available(&self, element: FormElement) -> bool {
        element == FormElement::Cylinder || KrnFitProvider.mz_available(element)
    }

    fn fit_minimum_zone(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        if element == FormElement::Cylinder {
            return mz_cylinder_fit(points);
        }
        KrnFitProvider.fit_minimum_zone(element, points)
    }

    fn fit_least_squares(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        KrnFitProvider.fit_least_squares(element, points)
    }
}

/// Evaluates cylindricity on `points` under resolved `chain`: runs optional filtration/resampling
/// steps only if named by `chain`, then evaluates `FormElement::Cylinder` via `GDT.FORM.FCORE`
/// (`REQ-001`..`REQ-003`).
pub fn evaluate_cylindricity(
    points: &[[f64; 3]],
    chain: &OperatorChain,
    filters: &dyn FilterProvider,
    krn: &dyn FitProvider,
) -> Result<CylindricityResult, FormError> {
    for p in points {
        if !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite() {
            return Err(FormError::NanInput);
        }
    }

    let mut current: Vec<[f64; 3]> = points.to_vec();
    let mut filter_algorithms: Vec<String> = Vec::new();
    for step in chain.steps() {
        let is_filter_or_resample = step.kind() == OperatorKind::Filtration
            || (matches!(step.kind(), OperatorKind::Extraction | OperatorKind::Reduction)
                && step.alg().as_str().contains("resampl"));
        if is_filter_or_resample {
            current = filters.apply(step, &current)?;
            filter_algorithms.push(step.alg().as_str().to_owned());
        }
    }

    let payload = evaluate_form(FormElement::Cylinder, &current, chain, krn)?;
    Ok(CylindricityResult { payload, filter_algorithms })
}

// -------------------------------------------------- deterministic multi-start KNP-1 MZ cylinder

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm3(v: [f64; 3]) -> Result<f64, FormError> {
    numf::sqrt((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2]).map_err(|_| FormError::NanInput)
}

fn fmax(a: f64, b: f64) -> f64 {
    if a.total_cmp(&b) == Ordering::Greater {
        a
    } else {
        b
    }
}

fn fmin(a: f64, b: f64) -> f64 {
    if a.total_cmp(&b) == Ordering::Less {
        a
    } else {
        b
    }
}

fn ortho_basis(a: [f64; 3]) -> Result<([f64; 3], [f64; 3]), FormError> {
    let ax = a[0].abs();
    let ay = a[1].abs();
    let az = a[2].abs();
    let seed = if ax.total_cmp(&ay) != Ordering::Greater && ax.total_cmp(&az) != Ordering::Greater {
        [1.0, 0.0, 0.0]
    } else if ay.total_cmp(&az) != Ordering::Greater {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let u = cross3(a, seed);
    let nu = norm3(u)?;
    if !nu.is_finite() || nu <= 0.0 {
        return Err(FormError::FitRefused { code: "degenerate_axis" });
    }
    let e1 = [u[0] / nu, u[1] / nu, u[2] / nu];
    let v = cross3(a, e1);
    let nv = norm3(v)?;
    if !nv.is_finite() || nv <= 0.0 {
        return Err(FormError::FitRefused { code: "degenerate_axis" });
    }
    let e2 = [v[0] / nv, v[1] / nv, v[2] / nv];
    Ok((e1, e2))
}

#[derive(Clone, Copy)]
struct CylFrame {
    p0: [f64; 3],
    a: [f64; 3],
    e1: [f64; 3],
    e2: [f64; 3],
}

impl CylFrame {
    fn axis_of(&self, x: [f64; 4]) -> Result<([f64; 3], [f64; 3]), FormError> {
        let mut p = [0.0; 3];
        let mut d_raw = [0.0; 3];
        for k in 0..3 {
            p[k] = (self.p0[k] + x[2] * self.e1[k]) + x[3] * self.e2[k];
            d_raw[k] = (self.a[k] + x[0] * self.e1[k]) + x[1] * self.e2[k];
        }
        let nd = norm3(d_raw)?;
        if !nd.is_finite() || nd <= 0.0 {
            return Err(FormError::FitRefused { code: "degenerate_axis" });
        }
        let d = [d_raw[0] / nd, d_raw[1] / nd, d_raw[2] / nd];
        Ok((p, d))
    }

    /// Evaluates coaxial zone `(width, r_min, r_max)` for parameter vector `x = [a, b, u, v]`.
    fn zone_width(&self, points: &[[f64; 3]], x: [f64; 4]) -> Result<(f64, f64, f64), FormError> {
        let (p, d) = self.axis_of(x)?;
        let mut r_min = f64::INFINITY;
        let mut r_max = f64::NEG_INFINITY;
        for &q in points {
            let w = sub3(q, p);
            let t = dot3(w, d);
            let perp = [w[0] - t * d[0], w[1] - t * d[1], w[2] - t * d[2]];
            let rho = norm3(perp)?;
            if rho.total_cmp(&r_min) == Ordering::Less {
                r_min = rho;
            }
            if rho.total_cmp(&r_max) == Ordering::Greater {
                r_max = rho;
            }
        }
        Ok((r_max - r_min, r_min, r_max))
    }

    /// Computes linearized radial distances `rho_i` and exact gradients `grad_i = d(rho_i)/dx`
    /// with respect to `x = [a, b, u, v]` around the current axis `x`.
    fn linearize(&self, points: &[[f64; 3]], x: [f64; 4]) -> Result<(Vec<f64>, Vec<[f64; 4]>), FormError> {
        let mut p = [0.0; 3];
        let mut d_raw = [0.0; 3];
        for k in 0..3 {
            p[k] = (self.p0[k] + x[2] * self.e1[k]) + x[3] * self.e2[k];
            d_raw[k] = (self.a[k] + x[0] * self.e1[k]) + x[1] * self.e2[k];
        }
        let dd = dot3(d_raw, d_raw);
        let nd = numf::sqrt(dd).map_err(|_| FormError::NanInput)?;
        if !nd.is_finite() || nd <= 0.0 {
            return Err(FormError::FitRefused { code: "degenerate_axis" });
        }
        let d_unit = [d_raw[0] / nd, d_raw[1] / nd, d_raw[2] / nd];
        let mut rhos = Vec::with_capacity(points.len());
        let mut grads = Vec::with_capacity(points.len());
        for &q in points {
            let w = sub3(q, p);
            let t_raw = dot3(w, d_raw) / dd;
            let t_u = dot3(w, d_unit);
            let perp = [w[0] - t_u * d_unit[0], w[1] - t_u * d_unit[1], w[2] - t_u * d_unit[2]];
            let rho = norm3(perp)?;
            if !rho.is_finite() || rho <= 1e-15 {
                return Err(FormError::FitRefused { code: "point_on_axis" });
            }
            let g1 = dot3(perp, self.e1) / rho;
            let g2 = dot3(perp, self.e2) / rho;
            // Exact derivative of rho = |w - ((w.D)/(D.D)) D| wrt (a, b, u, v):
            // drho/da = -t_raw * g1, drho/db = -t_raw * g2, drho/du = -g1, drho/dv = -g2.
            rhos.push(rho);
            grads.push([-(t_raw * g1), -(t_raw * g2), -g1, -g2]);
        }
        Ok((rhos, grads))
    }
}

/// Solves the 5-variable linear Chebyshev minimax subproblem:
///   min_{c = [da, db, du, dv, r_mid]} max_i | (rho_i + grad_i . dx) - r_mid |
/// i.e. min_{c, h} h  s.t.  | row_i . c - rho_i | <= h, where row_i = [-grad_i, 1.0],
/// via exact Stiefel exchange on 6-point dual-feasible bases (`fid_num::la::lstsq`).
/// Returns `Some(([da, db, du, dv], r_mid, h))` when a full-rank basis converges.
fn stiefel_chebyshev_5(rhos: &[f64], grads: &[[f64; 4]]) -> Option<([f64; 4], f64, f64)> {
    let n = rhos.len();
    if n < 6 {
        return None;
    }

    // Build rows: A_i = [-g0, -g1, -g2, -g3, 1.0], rhs b_i = rho_i,
    // so residual r_i(c) = b_i - A_i . c = rho_i + g_i . dx - r_mid.
    let row = |i: usize| -> [f64; 5] { [-grads[i][0], -grads[i][1], -grads[i][2], -grads[i][3], 1.0] };

    // Deterministic seed pool: extremes in rho and in each gradient component + combinations.
    fn push_unique(seeds: &mut Vec<usize>, idx: usize) {
        if !seeds.contains(&idx) {
            seeds.push(idx);
        }
    }
    let mut seeds: Vec<usize> = Vec::with_capacity(20);
    let mut best_min = [0usize; 7];
    let mut best_max = [0usize; 7];
    let keys = |i: usize| {
        let g = grads[i];
        [rhos[i], g[0], g[1], g[2], g[3], g[0] + g[2], g[1] + g[3]]
    };
    let mut lo = keys(0);
    let mut hi = lo;
    for i in 1..n {
        let k = keys(i);
        for d in 0..7 {
            if k[d].total_cmp(&lo[d]) == Ordering::Less {
                lo[d] = k[d];
                best_min[d] = i;
            }
            if k[d].total_cmp(&hi[d]) == Ordering::Greater {
                hi[d] = k[d];
                best_max[d] = i;
            }
        }
    }
    for d in 0..7 {
        push_unique(&mut seeds, best_min[d]);
        push_unique(&mut seeds, best_max[d]);
    }
    let step = (n / 6).max(1);
    let mut j = 0usize;
    while seeds.len() < 18 && j < n {
        push_unique(&mut seeds, j);
        j += step;
    }
    for i in 0..n {
        if seeds.len() >= 18 {
            break;
        }
        push_unique(&mut seeds, i);
    }

    // Build an initial 6-point dual-feasible basis by picking 5 independent rows via greedy
    // pivoted norm selection + 1 violating/extreme point.
    let mut picked: Vec<usize> = Vec::with_capacity(6);
    // Start with min-rho and max-rho points.
    picked.push(best_max[0]);
    if best_min[0] != best_max[0] {
        picked.push(best_min[0]);
    }
    for &s in &seeds {
        if picked.len() >= 6 {
            break;
        }
        if picked.contains(&s) {
            continue;
        }
        // Check if adding `s` increases row rank (up to 5) or completes the 6-point basis.
        picked.push(s);
        if picked.len() <= 5 {
            let k = picked.len();
            let mut data = Vec::with_capacity(5 * k);
            for r in 0..5 {
                for &pi in &picked {
                    data.push(row(pi)[r]);
                }
            }
            let rhs = [0.0; 5];
            let ok_rank = Mat::new(5, k, data)
                .ok()
                .and_then(|m| lstsq(&m, &rhs, 1e-13).ok())
                .is_some_and(|sol| sol.diag.rank == k);
            if !ok_rank {
                picked.pop();
            }
        }
    }
    if picked.len() < 6 {
        return None;
    }

    let mut basis_idx = [picked[0], picked[1], picked[2], picked[3], picked[4], picked[5]];
    let mut basis_sgn = [1.0_f64; 6];

    // Compute initial null-space vector of A_basis^T (5x6 matrix) to set dual-feasible signs.
    let init_signs = |bidx: &[usize; 6]| -> Option<[f64; 6]> {
        let mut m5_rows = [[0.0_f64; 5]; 5];
        let mut r5 = [0.0_f64; 5];
        let last = row(bidx[5]);
        for col in 0..5 {
            let rv = row(bidx[col]);
            for k in 0..5 {
                m5_rows[k][col] = rv[k];
            }
        }
        for k in 0..5 {
            r5[k] = -last[k];
        }
        let m5 = Mat::from_rows(&m5_rows).ok()?;
        let sol = lstsq(&m5, &r5, 1e-14).ok()?;
        if sol.diag.rank < 5 || sol.x.len() != 5 {
            return None;
        }
        let u = [sol.x[0], sol.x[1], sol.x[2], sol.x[3], sol.x[4], 1.0];
        let mut sgn = [1.0_f64; 6];
        let mut num = 0.0;
        let mut den = 0.0;
        for k in 0..6 {
            if u[k].abs() < 1e-14 {
                return None;
            }
            sgn[k] = if u[k] >= 0.0 { 1.0 } else { -1.0 };
            num += u[k] * rhos[bidx[k]];
            den += u[k].abs();
        }
        if den <= 1e-15 {
            return None;
        }
        if num < 0.0 {
            for s in &mut sgn {
                *s = -*s;
            }
        }
        Some(sgn)
    };

    if let Some(sgn) = init_signs(&basis_idx) {
        basis_sgn = sgn;
    } else {
        // Try replacing the 6th point from seeds until we get a non-degenerate dual-feasible basis.
        let mut found = false;
        for &cand in &seeds {
            if basis_idx[..5].contains(&cand) {
                continue;
            }
            basis_idx[5] = cand;
            if let Some(sgn) = init_signs(&basis_idx) {
                basis_sgn = sgn;
                found = true;
                break;
            }
        }
        if !found {
            return None;
        }
    }

    // Stiefel exchange loop (bounded to 80 pivots; typically converges in 6-15).
    let mut prev_h = -1.0_f64;
    for _iter in 0..80 {
        let mut m6_rows = [[0.0_f64; 6]; 6];
        let mut rhs6 = [0.0_f64; 6];
        for r in 0..6 {
            let rv = row(basis_idx[r]);
            m6_rows[r][..5].copy_from_slice(&rv);
            m6_rows[r][5] = basis_sgn[r];
            rhs6[r] = rhos[basis_idx[r]];
        }
        let m6 = Mat::from_rows(&m6_rows).ok()?;
        let sol6 = lstsq(&m6, &rhs6, 1e-14).ok()?;
        if sol6.diag.rank < 6 || sol6.x.len() != 6 {
            return None;
        }
        let c = [sol6.x[0], sol6.x[1], sol6.x[2], sol6.x[3], sol6.x[4]];
        let h = sol6.x[5];
        if !h.is_finite() || h < -1e-13 {
            return None;
        }

        // Find point with maximum absolute residual over all points (lowest index on ties).
        let mut worst_idx = 0usize;
        let mut worst_abs = -1.0_f64;
        let mut worst_res = 0.0_f64;
        for (i, &rho_i) in rhos.iter().enumerate() {
            let rv = row(i);
            let pred = ((rv[0] * c[0] + rv[1] * c[1]) + (rv[2] * c[2] + rv[3] * c[3])) + rv[4] * c[4];
            let res = rho_i - pred;
            let a = res.abs();
            if a.total_cmp(&worst_abs) == Ordering::Greater {
                worst_abs = a;
                worst_idx = i;
                worst_res = res;
            }
        }

        let tol = 1e-14 * (1.0 + h.abs());
        if worst_abs <= h + tol || (prev_h >= 0.0 && h < prev_h - 1e-13) {
            return Some(([c[0], c[1], c[2], c[3]], c[4], fmax(h, 0.0)));
        }
        prev_h = h;
        if basis_idx.contains(&worst_idx) {
            return Some(([c[0], c[1], c[2], c[3]], c[4], worst_abs));
        }

        // Compute exact unsigned dual multipliers lambda >= 0 and exchange vector mu:
        // Primal row r is (s_r * A_r) . c + 1 * h = s_r * b_r, so P^T has column r = [s_r * A_r^T, 1]^T.
        // Dual feasibility: P^T * lambda = [0, 0, 0, 0, 0, 1]^T with lambda_r >= 0, sum lambda_r = 1.
        // Incoming column: col_new = [s_new * A_new^T, 1]^T, solve P^T * mu = col_new.
        let mut pt_rows = [[0.0_f64; 6]; 6];
        for r in 0..6 {
            let rv = row(basis_idx[r]);
            let sr = basis_sgn[r];
            for col in 0..5 {
                pt_rows[col][r] = sr * rv[col];
            }
            pt_rows[5][r] = 1.0;
        }
        let pt = Mat::from_rows(&pt_rows).ok()?;
        let lam_sol = lstsq(&pt, &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0], 1e-14).ok()?;
        if lam_sol.diag.rank < 6 {
            return None;
        }
        let s_new = if worst_res >= 0.0 { 1.0 } else { -1.0 };
        let rv_new = row(worst_idx);
        let mu_sol = lstsq(
            &pt,
            &[s_new * rv_new[0], s_new * rv_new[1], s_new * rv_new[2], s_new * rv_new[3], s_new * rv_new[4], 1.0],
            1e-14,
        )
        .ok()?;
        if mu_sol.diag.rank < 6 {
            return None;
        }

        let mut leave: Option<usize> = None;
        let mut best_ratio = f64::INFINITY;
        for k in 0..6 {
            let mk = mu_sol.x[k];
            let lk = lam_sol.x[k];
            if mk > 1e-14 && lk >= -1e-12 {
                let ratio = fmax(lk, 0.0) / mk;
                if ratio.total_cmp(&best_ratio) == Ordering::Less {
                    best_ratio = ratio;
                    leave = Some(k);
                }
            }
        }
        let Some(k_leave) = leave else {
            return Some(([c[0], c[1], c[2], c[3]], c[4], worst_abs));
        };
        basis_idx[k_leave] = worst_idx;
        basis_sgn[k_leave] = s_new;
    }

    None
}

/// Runs sequential Stiefel-exchange + exact backtracking line search + deterministic coordinate
/// polish from one start `x0`.
fn optimize_cylinder_from(
    frame: &CylFrame,
    points: &[[f64; 3]],
    x0: [f64; 4],
    scale_ang: f64,
    scale_pos: f64,
) -> Result<([f64; 4], f64, f64), FormError> {
    let mut x = x0;
    let (mut best_w, mut best_rmin, mut best_rmax) = frame.zone_width(points, x)?;

    for _outer in 0..15 {
        let (rhos, grads) = frame.linearize(points, x)?;
        let Some((dx, _rmid, _h)) = stiefel_chebyshev_5(&rhos, &grads) else {
            break;
        };
        // Clamp step to keep the linearized trust region well-behaved on extreme geometries.
        let max_ang = fmax(0.25_f64, 10.0 * scale_ang);
        let max_pos = 10.0 * fmax(scale_pos, 1e-3);
        let step_ratio_a = fmax(dx[0].abs() / max_ang, dx[1].abs() / max_ang);
        let step_ratio_p = fmax(dx[2].abs() / max_pos, dx[3].abs() / max_pos);
        let clamp = fmax(step_ratio_a, step_ratio_p);
        let scale = if clamp > 1.0 { 1.0 / clamp } else { 1.0 };
        let sdx = [dx[0] * scale, dx[1] * scale, dx[2] * scale, dx[3] * scale];

        let mut improved = false;
        let mut alpha = 1.0_f64;
        for _ls in 0..16 {
            let cand = [x[0] + alpha * sdx[0], x[1] + alpha * sdx[1], x[2] + alpha * sdx[2], x[3] + alpha * sdx[3]];
            if let Ok((cw, crmin, crmax)) = frame.zone_width(points, cand) {
                if cw.total_cmp(&(best_w - 1e-16)) == Ordering::Less {
                    let delta = best_w - cw;
                    x = cand;
                    best_w = cw;
                    best_rmin = crmin;
                    best_rmax = crmax;
                    improved = true;
                    if delta <= 1e-15 * (1.0 + best_w) {
                        return Ok((x, best_w, 0.5 * (best_rmin + best_rmax)));
                    }
                    break;
                }
            }
            alpha *= 0.5;
        }
        if !improved {
            break;
        }
    }

    Ok((x, best_w, 0.5 * (best_rmin + best_rmax)))
}

/// Exact deterministic multi-start KNP-1 minimum-zone cylinder fit (`GDT.FORM.CYL`).
pub fn mz_cylinder_fit(points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
    if points.len() < 5 {
        return Err(FormError::TooFewPoints { required: 5, given: points.len() });
    }
    let mut flat = Vec::with_capacity(points.len() * 3);
    for p in points {
        if !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite() {
            return Err(FormError::NanInput);
        }
        flat.push(p[0]);
        flat.push(p[1]);
        flat.push(p[2]);
    }
    let view = PointsView::new(&flat, None).map_err(|e| FormError::FitRefused { code: e.code() })?;
    let ls_params = FitParams::new(Method::LeastSquares);
    let ls = ls_cyl::fit_cylinder(&view, &ls_params, Some(0)).map_err(|e| FormError::FitRefused { code: e.code() })?;
    let FittedFeature::Cylinder { p: p0, d: a0, r: r0 } = ls.result.feature else {
        return Err(FormError::FitRefused { code: "unexpected_ls_cylinder" });
    };
    let (e1, e2) = ortho_basis(a0)?;
    let frame = CylFrame { p0, a: a0, e1, e2 };
    let (w0, _, _) = frame.zone_width(points, [0.0; 4])?;

    // Axial span L for start perturbation scaling.
    let mut s_min = f64::INFINITY;
    let mut s_max = f64::NEG_INFINITY;
    for &q in points {
        let s = dot3(sub3(q, p0), a0);
        if s.total_cmp(&s_min) == Ordering::Less {
            s_min = s;
        }
        if s.total_cmp(&s_max) == Ordering::Greater {
            s_max = s;
        }
    }
    let span = fmax(fmax(s_max - s_min, r0 * 1e-3), 1e-9);
    let da = fmin(0.25 * w0 / span, 0.05);
    let du = fmin(0.25 * w0, 0.05 * r0);

    let starts: [[f64; 4]; 5] =
        [[0.0, 0.0, 0.0, 0.0], [da, 0.0, du, 0.0], [-da, 0.0, -du, 0.0], [0.0, da, 0.0, du], [0.0, -da, 0.0, -du]];

    let mut best_x = [0.0; 4];
    let mut best_w = f64::INFINITY;
    let mut best_r = r0;
    let mut outcomes: Vec<f64> = Vec::with_capacity(starts.len());
    let mut any_failed = false;

    for &st in &starts {
        match optimize_cylinder_from(&frame, points, st, fmax(da, 1e-6), fmax(du, 1e-6)) {
            Ok((cx, cw, cr)) => {
                outcomes.push(cw);
                if cw.total_cmp(&best_w) == Ordering::Less {
                    best_w = cw;
                    best_x = cx;
                    best_r = cr;
                }
            }
            Err(_) => {
                any_failed = true;
            }
        }
    }

    if !best_w.is_finite() {
        return Err(FormError::FitRefused { code: "mz_cylinder_failed" });
    }

    // Check deterministic multi-start agreement (`REQ-GDT.FORM.CYL-002`).
    let agree_tol = 1e-9 * (1.0 + r0.abs());
    let disagree = outcomes.iter().any(|&w| (w - best_w).abs() > agree_tol);
    let local_optimum_only = ls.local_optimum_only || ls.suspect_axis || any_failed || disagree;

    let (p_raw, d_raw) = frame.axis_of(best_x)?;
    let d_canon = normalize_direction(d_raw).map_err(|e| FormError::FitRefused { code: e.code() })?;
    // Foot of centroid onto canonical axis.
    let mut c_sum = [0.0; 3];
    for &q in points {
        c_sum[0] += q[0];
        c_sum[1] += q[1];
        c_sum[2] += q[2];
    }
    let inv_n = 1.0 / (points.len() as f64);
    let cent = [c_sum[0] * inv_n, c_sum[1] * inv_n, c_sum[2] * inv_n];
    let t_c = dot3(sub3(cent, p_raw), d_canon);
    let p_canon = [p_raw[0] + t_c * d_canon[0], p_raw[1] + t_c * d_canon[1], p_raw[2] + t_c * d_canon[2]];

    let feature = FittedFeature::Cylinder { p: p_canon, d: d_canon, r: best_r };
    let residuals = signed_residuals_of(&feature, points)?;
    Ok(FitOutcome {
        reference: feature,
        signed_residuals: residuals,
        active_zone_width: Some(best_w),
        algorithm_id: ALG_CYLINDER_MZ.to_owned(),
        local_optimum_only,
    })
}
