//! Least-squares point, line and plane fits. Spec node `KRN.LSQ.LIN`.
//!
//! Four closed-form orthogonal-distance least-squares fits implementing
//! [`crate::contract::Fit`]: [`PointLs`] (`fit.point.ls@1.0.0`), [`Line2Ls`]
//! (`fit.line2.ls@1.0.0`), [`Line3Ls`] (`fit.line3.ls@1.0.0`) and [`PlaneLs`]
//! (`fit.plane.ls@1.0.0`). The algorithm-ID feature segments are the contract's primitive
//! slugs (`line2`, `line3`), which `FitResult::assemble` requires.
//!
//! # Pinned rules
//!
//! Every fit works on the centred, scaled data `q` of [`Centred`] (scale `s = 2^e`), and every
//! sum and dot product is a `fid_num::red` call; products and sums outside them are single
//! IEEE-754 operations (no FMA).
//!
//! 1. **Parameters.** [`FitParams::validate`] first; then the method must be least squares
//!    and the weights uniform ([`FitError::InvalidParams`] `method` / `weights`); the only
//!    constraints accepted are a fixed axis direction (line2, line3: the line direction;
//!    plane: the normal) and, for line2, a projection plane. Any other constraint is
//!    [`FitError::InvalidParams`] `constraints`.
//! 2. **Checks.** Without a fixed direction, `contract::check_points` for the primitive
//!    (count, rank, so `TooFewPoints` / `Collinear` come from there). With a fixed direction
//!    only the count `n >= 1` is checked (`TooFewPoints { need: 1 }`): the fit is then
//!    defined for any non-empty point set.
//! 3. **Scatter.** `S[j][k] = red::dot(col_j, col_k)` for `j <= k` over the coordinate columns
//!    of `q` in point order (`3 x 3`; `2 x 2` for line2). Eigenpairs by [`sym_eig`].
//! 4. **Direction and conditioning.** line3: `d` = eigenvector of the largest eigenvalue,
//!    relevant pair `(la, lb) = (l_2, l_1)`; plane: `n` = eigenvector of the smallest,
//!    `(la, lb) = (l_1, l_0)`; line2: `d` = eigenvector of the larger, `(la, lb) = (l_1, l_0)`.
//!    The eigenvalue ratio is `ratio = total_max(0, lb) / la`; `ratio >
//!    thresholds.eig_ratio_max` (or `la <= 0`, or a non-finite ratio) is
//!    [`FitError::IllConditioned`] carrying the ratio (`None` when it is not finite): the
//!    competing eigenvalue is too close to the defining one for a unique direction.
//!    Why a ratio, and the default `0.9`: for a perturbation `E` of the scatter matrix (rounding,
//!    or the noise of the data) with `||E||_2 = eps * la`, the direction turns by at most
//!    `eps * la / (la - lb) = eps / (1 - ratio)` radians (Davis-Kahan sin-theta bound, to
//!    first order). `ratio <= 0.9` bounds that amplification by 10. The ratio of the
//!    eigenvalues (second moments) is unaffected by the size or units of the data, and a
//!    shape whose two moments are equal in the noise-free geometry (a circle or ring for a
//!    line, an isotropic blob or a circle for a plane's in-plane pair) has a ratio near 1
//!    whatever the noise, while a long line or a flat patch has a ratio near 0. Directions
//!    go through `normalize_direction` (`normalize_direction2` for line2). A fixed direction
//!    is `normalize_direction` of the constraint (for line2 see rule 7) and its ratio is 0
//!    (no competing direction).
//! 5. **Feature.** Every feature passes through the centroid: the scaled feature has its
//!    point at the origin and is mapped back with `Normalisation::inverse_feature`, so the
//!    point is `0 * s + c` = the `red::mean` centroid.
//! 6. **Residuals** (metres, scaled value times `s` by `Normalisation::inverse_length`):
//!    point: `red::norm2(q_i)` (distance to the centroid, never negative); line3:
//!    `t_i = red::dot(q_i, d)`, `w_i = q_i - t_i d` per component, residual
//!    `red::norm2(w_i)` (distance, never negative); plane: `red::dot(q_i, n)` (signed, positive
//!    on the normal's side); line2: `d_0 q_iv - d_1 q_iu` (signed in-plane distance, positive
//!    to the left of `d`), with `t_i` and `w_i` as for line3 in two components.
//! 7. **Line2 projection.** The plane is the projection-plane constraint `(p0, n0)`, or the
//!    plane through the origin with normal `(0, 0, 1)` when there is none. `n` =
//!    `normalize_direction(n0)`; `k` = the axis with the smallest `|n_k|` (lowest index on
//!    ties); `e1 = normalize_direction(a - n_k n)` with `a` the unit vector of axis `k`;
//!    `e2 = unit_direction(n x e1)` (cross product component by component). For `n = (0,0,1)`
//!    the frame is `e1 = (1,0,0)`, `e2 = (0,1,0)`. Each point maps to
//!    `u = red::dot(x - p0, e1)`, `v = red::dot(x - p0, e2)` (a non-zero `|u|` or `|v|` below
//!    `2^-1022` becomes `+0.0`), and the fit runs on the points `(u, v, 0)`; the result
//!    `Line2 { p, d }` is in these plane coordinates (metres). A fixed direction `f` is
//!    projected: `g = (red::dot(f', e1), red::dot(f', e2))` with `f' =
//!    normalize_direction(f)`; `red::norm2(g) <= thresholds.rank_tol` (the direction is
//!    within that sine of the plane normal) is [`FitError::InvalidParams`]
//!    `constraints.fixed_axis_direction`; else `d = normalize_direction2(g)`.
//! 8. **Diagnostics** (scaled units, objective `1/2 sum r_i^2`). The gradient over the free
//!    parameters is reported by its `red::norm2`: point `[sum q_x, sum q_y, sum q_z]`;
//!    line3 `[sum w_x, sum w_y, sum w_z, dot(t, w_x), dot(t, w_y), dot(t, w_z)]` (the last
//!    three dropped with a fixed direction); line2 the same in two components; plane
//!    `[sum r, dot(r, P_x), dot(r, P_y), dot(r, P_z)]` with `P_i = q_i - r_i n` (the last
//!    three dropped with a fixed normal). The rank is the number of free parameters (point 3;
//!    line3 4, fixed 2; line2 2, fixed 1; plane 3, fixed 1), full whenever the checks and the
//!    conditioning test passed; `cond` is the eigenvalue ratio of rule 4 (0 for the point and
//!    for fixed directions); iterations 0; stop
//!    `closed_form`.
//!
//! The fits are deterministic (D0) and ignore the seed.

// Indexed loops mirror the pinned rules (and the Python model in
// `fixtures/fit/make_lin_fixtures.py`) term by term.
#![allow(clippy::needless_range_loop)]

mod eig;

pub use eig::{sym_eig, SymEig, EIG_MAX_SWEEPS};

use fid_num::{f, red, NumError};
use fid_types::Determinism;

use crate::contract::{
    algorithm_id, check_points, command_row, normalize_direction, normalize_direction2, unit_direction, AlgorithmId,
    Centred, CommandRow, Constraint, Fit, FitDiagnostics, FitError, FitParams, FitResult, FittedFeature, Method,
    PointsView, Primitive, StopReason, Weights, V2, V3,
};

/// Version of all four algorithms.
pub const VERSION: (u32, u32, u32) = (1, 0, 0);

/// The primitives fitted by this module, in the order of [`command_rows`].
pub const PRIMITIVES: [Primitive; 4] = [Primitive::Point, Primitive::Line2, Primitive::Line3, Primitive::Plane];

const OP: &str = "fit.ls_lin";

/// Least-squares point (centroid), `fit.point.ls@1.0.0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PointLs;

/// Least-squares 2D line in a projection plane, `fit.line2.ls@1.0.0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Line2Ls;

/// Least-squares 3D line, `fit.line3.ls@1.0.0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Line3Ls;

/// Least-squares plane, `fit.plane.ls@1.0.0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlaneLs;

/// The command data rows of `fit.ls_point@1`, `fit.ls_line2@1`, `fit.ls_line3@1` and
/// `fit.ls_plane@1` (W2 with R1 dry run, D0), in [`PRIMITIVES`] order.
pub fn command_rows() -> Result<Vec<CommandRow>, FitError> {
    let mut rows = alloc(PRIMITIVES.len())?;
    for p in PRIMITIVES {
        rows.push(command_row(p, Method::LeastSquares, VERSION, Determinism::D0)?);
    }
    Ok(rows)
}

fn alloc<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    Ok(v)
}

fn id(p: Primitive) -> AlgorithmId {
    // `least squares` is defined for every primitive, so this cannot fail; the fallback keeps
    // the function total without a panic path.
    algorithm_id(p, Method::LeastSquares, VERSION).unwrap_or_else(|_| AlgorithmId {
        module: "fit".to_string(),
        feature: p.slug().to_string(),
        method: Method::LeastSquares.slug().to_string(),
        version: VERSION,
    })
}

/// Rule 1. Returns the fixed direction, if any.
fn params_for(p: Primitive, pts: &PointsView<'_>, params: &FitParams) -> Result<Option<V3>, FitError> {
    params.validate(pts)?;
    if params.method != Method::LeastSquares {
        return Err(FitError::InvalidParams { field: "method" });
    }
    if !matches!(params.weights, Weights::Uniform) {
        return Err(FitError::InvalidParams { field: "weights" });
    }
    for c in &params.constraints {
        let ok = match c {
            Constraint::FixedAxisDirection(_) => p != Primitive::Point,
            Constraint::ProjectionPlane { .. } => p == Primitive::Line2,
            Constraint::FixedRadius(_) => false,
        };
        if !ok {
            return Err(FitError::InvalidParams { field: "constraints" });
        }
    }
    Ok(params.fixed_axis_direction())
}

/// Rule 2.
fn centre(p: Primitive, pts: &PointsView<'_>, params: &FitParams, fixed: bool) -> Result<Centred, FitError> {
    if fixed {
        if pts.is_empty() {
            return Err(FitError::TooFewPoints { n: 0, need: 1 });
        }
        Centred::new(pts)
    } else {
        Ok(check_points(p, pts, &params.thresholds)?.0)
    }
}

/// The first `K` coordinate columns of the scaled data, in point order.
fn columns<const K: usize>(c: &Centred) -> Result<[Vec<f64>; K], FitError> {
    let n = c.len();
    let mut cols: [Vec<f64>; K] = core::array::from_fn(|_| Vec::new());
    for col in cols.iter_mut() {
        *col = alloc(n)?;
    }
    for q in c.xyz().chunks_exact(3) {
        for (col, &x) in cols.iter_mut().zip(q) {
            col.push(x);
        }
    }
    Ok(cols)
}

/// Rule 3: the scatter matrix of the columns (upper triangle filled, lower mirrored).
fn scatter<const K: usize>(cols: &[Vec<f64>; K]) -> Result<[[f64; K]; K], FitError> {
    let mut s = [[0.0; K]; K];
    for j in 0..K {
        for k in j..K {
            let v = red::dot(&cols[j], &cols[k])?;
            s[j][k] = v;
            s[k][j] = v;
        }
    }
    Ok(s)
}

/// Rule 4: the eigenvalue ratio `total_max(0, lb) / la` of the pair `(la, lb)`, `la >= lb`,
/// accepted when at most `max`.
fn eig_ratio(la: f64, lb: f64, max: f64) -> Result<f64, FitError> {
    if la <= 0.0 || !la.is_finite() {
        return Err(FitError::IllConditioned { cond: None });
    }
    let ratio = f::total_max(0.0, lb)? / la;
    if ratio.is_finite() && ratio <= max {
        Ok(ratio)
    } else {
        Err(FitError::IllConditioned { cond: if ratio.is_finite() { Some(ratio) } else { None } })
    }
}

fn diagnostics(grad: &[f64], rank: usize, cond: f64) -> Result<FitDiagnostics, FitError> {
    Ok(FitDiagnostics { grad_norm: red::norm2(grad)?, rank, cond, iterations: 0, stop: StopReason::ClosedForm })
}

/// Residual and gradient parts shared by the 3D and 2D lines (rules 6 and 8): for each point
/// `t_i = red::dot(q_i, d)` and `w_i = q_i - t_i d`; returns `(t, w columns)`.
fn line_parts<const K: usize>(c: &Centred, d: &[f64; K]) -> Result<(Vec<f64>, [Vec<f64>; K]), FitError> {
    let n = c.len();
    let mut t = alloc(n)?;
    let mut w: [Vec<f64>; K] = core::array::from_fn(|_| Vec::new());
    for col in w.iter_mut() {
        *col = alloc(n)?;
    }
    for q in c.xyz().chunks_exact(3) {
        let ti = red::dot(&q[..K], d)?;
        t.push(ti);
        for k in 0..K {
            w[k].push(q[k] - ti * d[k]);
        }
    }
    Ok((t, w))
}

/// Gradient of a line fit (rule 8): sums of `w`, then (when the direction is free) dot
/// products of `t` with `w`.
fn line_grad<const K: usize>(t: &[f64], w: &[Vec<f64>; K], free_dir: bool) -> Result<Vec<f64>, FitError> {
    let mut g = alloc(2 * K)?;
    for col in w {
        g.push(red::sum(col)?);
    }
    if free_dir {
        for col in w {
            g.push(red::dot(t, col)?);
        }
    }
    Ok(g)
}

fn finish(
    p: Primitive,
    c: &Centred,
    feature: FittedFeature,
    residuals: Vec<f64>,
    diag: FitDiagnostics,
) -> Result<FitResult, FitError> {
    let feature = c.map().inverse_feature(&feature)?;
    FitResult::assemble(&feature, residuals, diag, None, id(p))
}

impl Fit for PointLs {
    fn algorithm_id(&self) -> AlgorithmId {
        id(Primitive::Point)
    }

    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, _seed: Option<u64>) -> Result<FitResult, FitError> {
        params_for(Primitive::Point, pts, params)?;
        let c = centre(Primitive::Point, pts, params, false)?;
        let map = *c.map();
        let mut res = alloc(c.len())?;
        for q in c.xyz().chunks_exact(3) {
            res.push(map.inverse_length(red::norm2(q)?)?);
        }
        let cols = columns::<3>(&c)?;
        let grad = [red::sum(&cols[0])?, red::sum(&cols[1])?, red::sum(&cols[2])?];
        let diag = diagnostics(&grad, 3, 0.0)?;
        finish(Primitive::Point, &c, FittedFeature::Point { c: [0.0; 3] }, res, diag)
    }
}

impl Fit for Line3Ls {
    fn algorithm_id(&self) -> AlgorithmId {
        id(Primitive::Line3)
    }

    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, _seed: Option<u64>) -> Result<FitResult, FitError> {
        let fixed = params_for(Primitive::Line3, pts, params)?;
        let c = centre(Primitive::Line3, pts, params, fixed.is_some())?;
        let (d, cond) = match fixed {
            Some(f) => (normalize_direction(f)?, 0.0),
            None => {
                let e = sym_eig(&scatter(&columns::<3>(&c)?)?)?;
                let cond = eig_ratio(e.values[2], e.values[1], params.thresholds.eig_ratio_max)?;
                (normalize_direction(e.vectors[2])?, cond)
            }
        };
        let map = *c.map();
        let (t, w) = line_parts::<3>(&c, &d)?;
        let mut res = alloc(c.len())?;
        for i in 0..c.len() {
            res.push(map.inverse_length(red::norm2(&[w[0][i], w[1][i], w[2][i]])?)?);
        }
        let grad = line_grad(&t, &w, fixed.is_none())?;
        let diag = diagnostics(&grad, if fixed.is_some() { 2 } else { 4 }, cond)?;
        finish(Primitive::Line3, &c, FittedFeature::Line3 { p: [0.0; 3], d }, res, diag)
    }
}

impl Fit for PlaneLs {
    fn algorithm_id(&self) -> AlgorithmId {
        id(Primitive::Plane)
    }

    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, _seed: Option<u64>) -> Result<FitResult, FitError> {
        let fixed = params_for(Primitive::Plane, pts, params)?;
        let c = centre(Primitive::Plane, pts, params, fixed.is_some())?;
        let (n, cond) = match fixed {
            Some(f) => (normalize_direction(f)?, 0.0),
            None => {
                let e = sym_eig(&scatter(&columns::<3>(&c)?)?)?;
                let cond = eig_ratio(e.values[1], e.values[0], params.thresholds.eig_ratio_max)?;
                (normalize_direction(e.vectors[0])?, cond)
            }
        };
        let map = *c.map();
        let len = c.len();
        let mut r = alloc(len)?;
        for q in c.xyz().chunks_exact(3) {
            r.push(red::dot(q, &n)?);
        }
        let mut g = alloc(4)?;
        g.push(red::sum(&r)?);
        if fixed.is_none() {
            let mut pc: [Vec<f64>; 3] = [alloc(len)?, alloc(len)?, alloc(len)?];
            for (q, &ri) in c.xyz().chunks_exact(3).zip(&r) {
                for k in 0..3 {
                    pc[k].push(q[k] - ri * n[k]);
                }
            }
            for col in &pc {
                g.push(red::dot(&r, col)?);
            }
        }
        let mut res = alloc(len)?;
        for &ri in &r {
            res.push(map.inverse_length(ri)?);
        }
        let diag = diagnostics(&g, if fixed.is_some() { 1 } else { 3 }, cond)?;
        finish(Primitive::Plane, &c, FittedFeature::Plane { p: [0.0; 3], n }, res, diag)
    }
}

/// The in-plane frame of rule 7: `(p0, e1, e2)`.
pub fn projection_frame(params: &FitParams) -> Result<(V3, V3, V3), FitError> {
    let (p0, n0) = params.projection_plane().unwrap_or(([0.0; 3], [0.0, 0.0, 1.0]));
    let n = normalize_direction(n0)?;
    let mut k = 0usize;
    for j in 1..3 {
        if n[j].abs() < n[k].abs() {
            k = j;
        }
    }
    let mut a = [0.0; 3];
    a[k] = 1.0;
    let nk = n[k];
    let e1 = normalize_direction([a[0] - nk * n[0], a[1] - nk * n[1], a[2] - nk * n[2]])?;
    let cross = [n[1] * e1[2] - n[2] * e1[1], n[2] * e1[0] - n[0] * e1[2], n[0] * e1[1] - n[1] * e1[0]];
    let e2 = unit_direction(cross)?;
    Ok((p0, e1, e2))
}

fn flush(x: f64) -> f64 {
    if x.is_subnormal() {
        0.0
    } else {
        x
    }
}

impl Fit for Line2Ls {
    fn algorithm_id(&self) -> AlgorithmId {
        id(Primitive::Line2)
    }

    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, _seed: Option<u64>) -> Result<FitResult, FitError> {
        let fixed = params_for(Primitive::Line2, pts, params)?;
        let (p0, e1, e2) = projection_frame(params)?;
        let fixed2 = match fixed {
            Some(f) => {
                let f = normalize_direction(f)?;
                let g = [red::dot(&f, &e1)?, red::dot(&f, &e2)?];
                if red::norm2(&g)? <= params.thresholds.rank_tol {
                    return Err(FitError::InvalidParams { field: "constraints.fixed_axis_direction" });
                }
                Some(normalize_direction2(g)?)
            }
            None => None,
        };
        let mut buf = alloc(pts.xyz().len())?;
        for x in pts.xyz().chunks_exact(3) {
            let dx = [x[0] - p0[0], x[1] - p0[1], x[2] - p0[2]];
            buf.push(flush(red::dot(&dx, &e1)?));
            buf.push(flush(red::dot(&dx, &e2)?));
            buf.push(0.0);
        }
        let view = PointsView::new(&buf, None)?;
        let c = centre(Primitive::Line2, &view, params, fixed2.is_some())?;
        let (d, cond): (V2, f64) = match fixed2 {
            Some(d) => (d, 0.0),
            None => {
                let e = sym_eig(&scatter(&columns::<2>(&c)?)?)?;
                let cond = eig_ratio(e.values[1], e.values[0], params.thresholds.eig_ratio_max)?;
                (normalize_direction2(e.vectors[1])?, cond)
            }
        };
        let map = *c.map();
        let (t, w) = line_parts::<2>(&c, &d)?;
        let mut res = alloc(c.len())?;
        for q in c.xyz().chunks_exact(3) {
            res.push(map.inverse_length(d[0] * q[1] - d[1] * q[0])?);
        }
        let grad = line_grad(&t, &w, fixed2.is_none())?;
        let diag = diagnostics(&grad, if fixed2.is_some() { 1 } else { 2 }, cond)?;
        finish(Primitive::Line2, &c, FittedFeature::Line2 { p: [0.0; 2], d }, res, diag)
    }
}
