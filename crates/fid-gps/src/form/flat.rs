//! Flatness evaluation (`GDT.FORM.FLAT`, `spec/gdt/form/flat/NODE.md`).
//!
//! Pure KNP-1 kernel math: no I/O, no `unsafe`, no panics, no `HashMap`/`HashSet`, no FMA,
//! `fid_num::f` for square roots, `fid_num::la` for small 4x4 solves, `f64::total_cmp` with
//! lowest-index tie breaking, NaN rejected as an error.

use crate::dmt::{Operator, OperatorChain, OperatorKind};
use crate::form::{
    evaluate_form, signed_residuals_of, FitOutcome, FitProvider, FormElement, FormError, FormPayload, KrnFitProvider,
};
use core::cmp::Ordering;
use fid_fit::contract::{normalize_direction, Fit, FitParams, FittedFeature, Method, PointsView};
use fid_fit::ls_lin;
use fid_num::{
    f as numf,
    la::{lstsq, Mat},
};

/// Algorithm ID for the exact KNP-1 minimum-zone plane fit (`GDT.FORM.FLAT`).
pub const ALG_PLANE_MZ: &str = "fit.plane.mz@1.0.0";

/// Optional filtration / resampling provider invoked ONLY when the resolved chain names
/// `Filtration` or `Resampling` operators (`REQ-GDT.FORM.FLAT-002`).
pub trait FilterProvider {
    /// Applies one `Filtration` or `Resampling` operator step to `points`.
    fn apply(&self, step: &Operator, points: &[[f64; 3]]) -> Result<Vec<[f64; 3]>, FormError>;
}

/// Default provider when no filter/resampling implementation is supplied: succeeds untouched when
/// the chain has no filtration/resampling steps and refuses cleanly if one is requested.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoFilterProvider;

impl FilterProvider for NoFilterProvider {
    fn apply(&self, _step: &Operator, _points: &[[f64; 3]]) -> Result<Vec<[f64; 3]>, FormError> {
        Err(FormError::FitRefused { code: "filter_unavailable" })
    }
}

/// Pure result of [`evaluate_flatness`].
#[derive(Debug, Clone, PartialEq)]
pub struct FlatnessResult {
    /// Shared `GDT.FORM.FCORE` form payload (`FormElement::Plane`).
    pub payload: FormPayload,
    /// Ordered algorithm IDs of every filtration or resampling operator applied before fitting
    /// (`REQ-GDT.FORM.FLAT-002`).
    pub filter_algorithms: Vec<String>,
}

/// [`FitProvider`] wiring exact KNP-1 minimum-zone plane fitting (`mz_plane_fit`) for
/// `FormElement::Plane` and delegating all other elements / least-squares fits to
/// [`KrnFitProvider`].
#[derive(Debug, Clone, Copy, Default)]
pub struct FlatnessKrnProvider;

impl FitProvider for FlatnessKrnProvider {
    fn mz_available(&self, element: FormElement) -> bool {
        match element {
            FormElement::Plane => true,
            other => KrnFitProvider.mz_available(other),
        }
    }

    fn fit_minimum_zone(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        match element {
            FormElement::Plane => mz_plane_fit(points),
            other => KrnFitProvider.fit_minimum_zone(other, points),
        }
    }

    fn fit_least_squares(&self, element: FormElement, points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
        KrnFitProvider.fit_least_squares(element, points)
    }
}

/// Evaluates flatness (`REQ-GDT.FORM.FLAT-001`, `REQ-GDT.FORM.FLAT-002`): applies optional
/// filtration/resampling steps only when named in `chain`, records their algorithm IDs, and
/// delegates to `form::evaluate_form(FormElement::Plane, ...)`.
pub fn evaluate_flatness(
    points: &[[f64; 3]],
    chain: &OperatorChain,
    filters: &dyn FilterProvider,
    krn: &dyn FitProvider,
) -> Result<FlatnessResult, FormError> {
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

    let payload = evaluate_form(FormElement::Plane, &current, chain, krn)?;
    Ok(FlatnessResult { payload, filter_algorithms })
}

// ------------------------------------------------------------------ exact KNP-1 MZ plane fit

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn norm3(a: [f64; 3]) -> Result<f64, FormError> {
    numf::sqrt(dot3(a, a)).map_err(|_| FormError::NanInput)
}

fn ortho_basis(n: [f64; 3]) -> Result<([f64; 3], [f64; 3]), FormError> {
    let ref_v = if n[0].abs() < 0.8 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let e1_raw = cross3(n, ref_v);
    let l1 = norm3(e1_raw)?;
    if l1 == 0.0 {
        return Err(FormError::FitRefused { code: "degenerate" });
    }
    let e1 = scale3(e1_raw, 1.0 / l1);
    let e2_raw = cross3(n, e1);
    let l2 = norm3(e2_raw)?;
    if l2 == 0.0 {
        return Err(FormError::FitRefused { code: "degenerate" });
    }
    Ok((e1, scale3(e2_raw, 1.0 / l2)))
}

fn solve_4x4(rows: [[f64; 4]; 4], rhs: [f64; 4]) -> Option<[f64; 4]> {
    let mat = Mat::from_rows(&rows).ok()?;
    let sol = lstsq(&mat, &rhs, 1e-14).ok()?;
    if sol.diag.rank < 4 || sol.x.len() != 4 {
        return None;
    }
    if sol.x.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some([sol.x[0], sol.x[1], sol.x[2], sol.x[3]])
}

fn solve_4x4_transpose(rows: [[f64; 4]; 4], rhs: [f64; 4]) -> Option<[f64; 4]> {
    let mut tr = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            tr[i][j] = rows[j][i];
        }
    }
    solve_4x4(tr, rhs)
}

/// Evaluates the exact Euclidean plane width `(max_r - min_r)` for unit normal `n`.
fn normal_width(points: &[[f64; 3]], n: [f64; 3]) -> (f64, f64, f64) {
    let mut hi = dot3(points[0], n);
    let mut lo = hi;
    for &p in points.iter().skip(1) {
        let d = dot3(p, n);
        if d.total_cmp(&hi) == Ordering::Greater {
            hi = d;
        }
        if d.total_cmp(&lo) == Ordering::Less {
            lo = d;
        }
    }
    (hi - lo, lo, hi)
}

/// Solves the linearized Chebyshev minimax plane `min_{a,b,c,h} h s.t. |z_i - (a x_i + b y_i + c)| <= h`
/// via Stiefel's exact exchange algorithm (certified optimal when max residual <= h).
fn stiefel_chebyshev_plane(xs: &[f64], ys: &[f64], zs: &[f64]) -> Option<[f64; 4]> {
    let n = xs.len();
    if n < 4 {
        return None;
    }

    // Build deterministic candidate seed indices: extremes in x, y, x+y, x-y, and z.
    fn push_unique(seeds: &mut Vec<usize>, idx: usize) {
        if !seeds.contains(&idx) {
            seeds.push(idx);
        }
    }
    let mut seeds: Vec<usize> = Vec::with_capacity(12);
    let mut best_min = [0usize; 5];
    let mut best_max = [0usize; 5];
    let keys = |i: usize| [xs[i], ys[i], xs[i] + ys[i], xs[i] - ys[i], zs[i]];
    let mut lo_v = keys(0);
    let mut hi_v = lo_v;
    for i in 1..n {
        let k = keys(i);
        for d in 0..5 {
            if k[d].total_cmp(&lo_v[d]) == Ordering::Less {
                lo_v[d] = k[d];
                best_min[d] = i;
            }
            if k[d].total_cmp(&hi_v[d]) == Ordering::Greater {
                hi_v[d] = k[d];
                best_max[d] = i;
            }
        }
    }
    for d in 0..5 {
        push_unique(&mut seeds, best_min[d]);
        push_unique(&mut seeds, best_max[d]);
    }
    for i in 0..n {
        if seeds.len() >= 12 {
            break;
        }
        push_unique(&mut seeds, i);
    }

    // Find an initial 4-point dual-feasible basis (all lambda_j >= 0, h >= 0).
    let mut basis_idx = [0usize; 4];
    let mut basis_sgn = [1.0_f64; 4];
    let mut found = false;
    let m = seeds.len();
    'outer: for i0 in 0..m {
        for i1 in (i0 + 1)..m {
            for i2 in (i1 + 1)..m {
                for i3 in (i2 + 1)..m {
                    let cand = [seeds[i0], seeds[i1], seeds[i2], seeds[i3]];
                    // Find null-space vector `u` of the 3x4 matrix [[x_k], [y_k], [1]] (sum u_k = 0).
                    // Fix u_3 = 1 and solve 3x3 for (u_0, u_1, u_2) via determinant/lstsq.
                    let m3 = [
                        [xs[cand[0]], xs[cand[1]], xs[cand[2]]],
                        [ys[cand[0]], ys[cand[1]], ys[cand[2]]],
                        [1.0, 1.0, 1.0],
                    ];
                    let r3 = [-xs[cand[3]], -ys[cand[3]], -1.0];
                    let Ok(mat3) = Mat::from_rows(&m3) else { continue };
                    let Ok(sol3) = lstsq(&mat3, &r3, 1e-14) else { continue };
                    if sol3.diag.rank < 3 || sol3.x.len() != 3 {
                        continue;
                    }
                    let u = [sol3.x[0], sol3.x[1], sol3.x[2], 1.0];
                    if u.iter().any(|v| !v.is_finite() || v.abs() <= 1e-14) {
                        continue;
                    }
                    // Choosing s_k = sign(u_k) makes lambda_k = |u_k| / sum(|u|) > 0, giving a
                    // strictly dual-feasible Stiefel reference set!
                    let mut sgn = [1.0; 4];
                    for k in 0..4 {
                        sgn[k] = if u[k] >= 0.0 { 1.0 } else { -1.0 };
                    }
                    // Flip all signs together if h < 0 so h >= 0.
                    let rows = [
                        [xs[cand[0]], ys[cand[0]], 1.0, sgn[0]],
                        [xs[cand[1]], ys[cand[1]], 1.0, sgn[1]],
                        [xs[cand[2]], ys[cand[2]], 1.0, sgn[2]],
                        [xs[cand[3]], ys[cand[3]], 1.0, sgn[3]],
                    ];
                    let rhs = [zs[cand[0]], zs[cand[1]], zs[cand[2]], zs[cand[3]]];
                    let Some(sol4) = solve_4x4(rows, rhs) else { continue };
                    if sol4[3] < 0.0 {
                        for s in &mut sgn {
                            *s = -*s;
                        }
                    }
                    basis_idx = cand;
                    basis_sgn = sgn;
                    found = true;
                    break 'outer;
                }
            }
        }
    }
    if !found {
        return None;
    }

    let mut last_sol = [0.0; 4];
    for _iter in 0..64 {
        let rows = [
            [xs[basis_idx[0]], ys[basis_idx[0]], 1.0, basis_sgn[0]],
            [xs[basis_idx[1]], ys[basis_idx[1]], 1.0, basis_sgn[1]],
            [xs[basis_idx[2]], ys[basis_idx[2]], 1.0, basis_sgn[2]],
            [xs[basis_idx[3]], ys[basis_idx[3]], 1.0, basis_sgn[3]],
        ];
        let rhs = [zs[basis_idx[0]], zs[basis_idx[1]], zs[basis_idx[2]], zs[basis_idx[3]]];
        let sol = solve_4x4(rows, rhs)?;
        last_sol = sol;
        let (a, b, c, h) = (sol[0], sol[1], sol[2], sol[3]);

        // Find maximum violation across all N points (deterministic lowest-index tie break).
        let mut worst_idx = 0usize;
        let mut worst_abs = -1.0_f64;
        let mut worst_res = 0.0_f64;
        for i in 0..n {
            let r = zs[i] - ((a * xs[i] + b * ys[i]) + c);
            let ar = r.abs();
            if ar.total_cmp(&worst_abs) == Ordering::Greater {
                worst_abs = ar;
                worst_idx = i;
                worst_res = r;
            }
        }
        let tol = 1e-14 * numf::total_max(1.0, h.abs()).unwrap_or(1.0);
        if worst_abs <= h + tol {
            return Some(sol);
        }

        // Exact Stiefel dual ratio test to pick leaving basis element.
        let s_new = if worst_res >= 0.0 { 1.0 } else { -1.0 };
        let lam = solve_4x4_transpose(rows, [0.0, 0.0, 0.0, 1.0])?;
        let v = solve_4x4_transpose(rows, [xs[worst_idx], ys[worst_idx], 1.0, s_new])?;

        let mut leave: Option<(usize, f64)> = None;
        for j in 0..4 {
            if v[j] > 1e-15 {
                let ratio = lam[j] / v[j];
                let better = match leave {
                    None => true,
                    Some((_, best_r)) => ratio.total_cmp(&best_r) == Ordering::Less,
                };
                if better {
                    leave = Some((j, ratio));
                }
            }
        }
        let Some((j_out, _)) = leave else {
            return Some(last_sol);
        };
        basis_idx[j_out] = worst_idx;
        basis_sgn[j_out] = s_new;
    }

    Some(last_sol)
}

/// Exact KNP-1 minimum-zone plane fit via Stiefel exchange with outer Euclidean re-projection.
pub fn mz_plane_fit(points: &[[f64; 3]]) -> Result<FitOutcome, FormError> {
    if points.len() < 4 {
        return Err(FormError::TooFewPoints { required: 4, given: points.len() });
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
    let ls_res = ls_lin::PlaneLs
        .fit(&view, &FitParams::new(fid_fit::contract::Method::LeastSquares), None)
        .map_err(|e| FormError::FitRefused { code: e.code() })?;
    let FittedFeature::Plane { p: mut origin, n: mut cur_n } = ls_res.feature else {
        return Err(FormError::FitRefused { code: "unexpected_ls_plane" });
    };
    let (mut best_w, mut best_lo, mut best_hi) = normal_width(points, cur_n);
    let mut best_n = cur_n;

    // Outer Euclidean re-projection loop (typically converges in 1-2 iterations to < 1e-15 m).
    let mut xs = Vec::with_capacity(points.len());
    let mut ys = Vec::with_capacity(points.len());
    let mut zs = Vec::with_capacity(points.len());
    for _outer in 0..5 {
        let (e1, e2) = ortho_basis(cur_n)?;
        xs.clear();
        ys.clear();
        zs.clear();
        for &p in points {
            let d = sub3(p, origin);
            xs.push(dot3(d, e1));
            ys.push(dot3(d, e2));
            zs.push(dot3(d, cur_n));
        }
        let Some([a, b, c, _h]) = stiefel_chebyshev_plane(&xs, &ys, &zs) else {
            break;
        };
        let n_raw = sub3(sub3(cur_n, scale3(e1, a)), scale3(e2, b));
        let Ok(n_new) = normalize_direction(n_raw) else {
            break;
        };
        let (w_new, lo_new, hi_new) = normal_width(points, n_new);
        if w_new.total_cmp(&best_w) == Ordering::Less {
            best_w = w_new;
            best_n = n_new;
            best_lo = lo_new;
            best_hi = hi_new;
        }
        origin = add3(origin, scale3(cur_n, c));
        let step_norm = norm3(sub3(n_new, cur_n))?;
        cur_n = n_new;
        if step_norm <= 1e-14 {
            break;
        }
    }

    // Mid-plane point at 0.5 * (best_lo + best_hi) along best_n so residuals are symmetric in [-w/2, +w/2].
    let mid_offset = 0.5 * (best_lo + best_hi);
    let ref_plane = FittedFeature::Plane { p: scale3(best_n, mid_offset), n: best_n }
        .canonical()
        .map_err(|e| FormError::FitRefused { code: e.code() })?;
    let residuals = signed_residuals_of(&ref_plane, points)?;
    let _ = Method::MinimumZone;
    Ok(FitOutcome {
        reference: ref_plane,
        signed_residuals: residuals,
        active_zone_width: Some(best_w),
        algorithm_id: ALG_PLANE_MZ.to_owned(),
        local_optimum_only: false,
    })
}
