//! Least-squares circle and sphere fits. Spec node `KRN.LSQ.RND`.
//!
//! Three orthogonal-distance least-squares fits implementing [`crate::contract::Fit`]:
//! [`Circle2Ls`] (`fit.circle2.ls@1.0.0`, a circle in a projection plane), [`Circle3Ls`]
//! (`fit.circle3.ls@1.0.0`, a circle in space) and [`SphereLs`] (`fit.sphere.ls@1.0.0`). The
//! algorithm-ID feature segments are the contract's primitive slugs (`circle2`, `circle3`),
//! which `FitResult::assemble` requires (the leaf's text says `circle2d` / `circle3d`; the
//! same choice as `KRN.LSQ.LIN` made for its lines).
//!
//! # Pinned rules
//!
//! Every fit works on the centred, scaled data `q` of [`Centred`] (scale `s = 2^e`). Every sum,
//! dot product and norm is a `fid_num::red` call, square roots, `hypot` and `atan2` go through
//! `fid_num::f`, comparisons use total order; products and sums outside them are single
//! IEEE-754 operations (no FMA).
//!
//! 1. **Parameters.** [`FitParams::validate`] first; then the method must be least squares
//!    and the weights uniform ([`FitError::InvalidParams`] `method` / `weights`). Accepted
//!    constraints: a fixed radius (all three) and a projection plane (circle2 only); any other
//!    is [`FitError::InvalidParams`] `constraints`. A fixed radius `R` (metres) enters the
//!    fit as `R / s` and is not a parameter (REQ-006).
//! 2. **Checks.** `contract::check_points` for the primitive: count (circle 3, sphere 4 points,
//!    also with a fixed radius), rank, and for the circles the contract's angular coverage under
//!    `thresholds.min_coverage` (REQ-005). For the sphere, which the contract does not cover,
//!    the coverage is [`sphere_coverage`] about the algebraic centre of rule 3, refused the same
//!    way ([`FitError::InsufficientCoverage`]). [`DEFAULT_MIN_COVERAGE`] (30 degrees, the
//!    leaf's default) is set by [`default_params`]; the fits use whatever `params` holds.
//! 3. **Algebraic starts** (Kasa). Circle: rows `[u, v, 1, -(u u + v v)]` folded by the
//!    contract's blocked QR to a `4 x 4` factor, its leading `3 x 3` block and column 3 to
//!    `fid_num::la::lstsq` with `rel_tol = thresholds.rank_tol`; centre `(-D / 2, -E / 2)`,
//!    radius `sqrt((D D + E E) / 4 - F)`. Sphere: rows `[x, y, z, 1, -(x x + y y + z z)]`, a
//!    `5 x 5` factor, `4 x 4` solve, centre `-(D, E, F) / 2`, radius
//!    `sqrt((D D + E E + F F) / 4 - G)`. The 3D circle starts from the normal `N0` of
//!    `ls_lin::PlaneLs` on the same points (REQ-002) and `init`'s algebraic circle in the plane
//!    of `N0` (its basis `E1`, `E2` of `N0`; the start centre `C0 = u_c E1 + v_c E2`). A rank
//!    below full, a non-positive or non-finite squared radius, or a failed plane fit is
//!    [`FitError::IllConditioned`] (`cond: None`).
//! 4. **Models.** circle2 `(u_c, v_c, r)`: `rho = red::norm2(q_uv - c)`, residual `rho - r`,
//!    Jacobian `[-g_u, -g_v, -1]` with `g = (q_uv - c) / rho` (0 when `rho == 0`). Sphere
//!    `(x_c, y_c, z_c, r)`: the same in three components. circle3 `(a, b, x, y, z, r)` about the
//!    start frame (tangent-plane parameters of the normal, REQ-004): `D = N0 + a E1 + b E2`,
//!    `t = red::norm2(D)`, `n = D / t`, `c = C0 + x E1 + y E2 + z N0`; per point `w = q - c`,
//!    `h = red::dot(w, n)`, `perp = w - h n`, `rho = red::norm2(perp)`, `g = perp / rho` (0 when
//!    `rho == 0`); two residuals `h` and `rho - r` (their squares sum to the squared 3D distance
//!    to the circle), with Jacobian rows
//!    `[(w.E1 - h n.E1) / t, (w.E2 - h n.E2) / t, -n.E1, -n.E2, -n.N0, 0]` and
//!    `[-h g.E1 / t, -h g.E2 / t, -g.E1, -g.E2, -g.N0, -1]`. A fixed radius drops the `r`
//!    column. The normal stays within the reach of the tangent-plane parameters because the
//!    start is the plane fit of the same points.
//! 5. **Solve.** One `fid_solv::nlls::solve` under `params.nlls` (Levenberg-Marquardt by
//!    default) from the start; solver errors map by `FitError::from_solver` (iteration limit
//!    `NotConverged`, condition above `nlls.cond_limit` `IllConditioned`, REQ-005).
//! 6. **Result.** circle2 `Circle2 { c, r }` in the projection plane's coordinates (metres, the
//!    frame of `ls_lin::projection_frame`, as `Line2`); circle3 `Circle3 { c, n, r }`; sphere
//!    `Sphere { c, r }`, mapped back with `Normalisation::inverse_feature` and canonicalised
//!    by `FitResult::assemble`. Residuals in metres: `rho - r` (circle2, sphere; positive
//!    outside), and for circle3 the signed 3D distance `hypot(h, rho - r)`, negative when
//!    `rho < r`. Diagnostics are the solver's (`FitDiagnostics::from_nlls`).
//!
//! The fits are deterministic (D0), ignore the seed and never set `local_optimum_only`: a
//! single start from a closed-form estimate, no multi-start.

// Indexed loops mirror the pinned rules term by term.
#![allow(clippy::needless_range_loop)]

use core::cmp::Ordering;

use fid_kci::AlgorithmId;
use fid_num::la::{lstsq, LaError, Mat};
use fid_num::{f, red, NumError};
use fid_solv::nlls::{self, NllsProblem};
use fid_types::Determinism;

use crate::contract::{
    algorithm_id, check_points, command_row, tsqr, Centred, CommandRow, Constraint, Fit, FitDiagnostics, FitError,
    FitParams, FitResult, FittedFeature, Method, PointsView, Primitive, Weights, TWO_PI, V3,
};
use crate::init;
use crate::ls_lin::{projection_frame, PlaneLs};

/// Version of all three algorithms.
pub const VERSION: (u32, u32, u32) = (1, 0, 0);

/// The primitives fitted by this module, in the order of [`command_rows`].
pub const PRIMITIVES: [Primitive; 3] = [Primitive::Circle2, Primitive::Circle3, Primitive::Sphere];

/// The leaf's default minimum angular coverage, 30 degrees in radians
/// (`core::f64::consts::FRAC_PI_6`) [unverified]. The contract's own default is 20 degrees
/// (`contract::DEFAULT_MIN_COVERAGE`); see [`default_params`].
pub const DEFAULT_MIN_COVERAGE: f64 = core::f64::consts::FRAC_PI_6;

const OP: &str = "fit.ls_rnd";

/// Least-squares circle in a projection plane, `fit.circle2.ls@1.0.0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Circle2Ls;

/// Least-squares circle in space, `fit.circle3.ls@1.0.0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Circle3Ls;

/// Least-squares sphere, `fit.sphere.ls@1.0.0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SphereLs;

/// Least-squares parameters with the leaf's coverage default: `FitParams::new(LeastSquares)`
/// with `thresholds.min_coverage = DEFAULT_MIN_COVERAGE`.
pub fn default_params() -> FitParams {
    let mut p = FitParams::new(Method::LeastSquares);
    p.thresholds.min_coverage = DEFAULT_MIN_COVERAGE;
    p
}

/// The command data rows of `fit.ls_circle2@1`, `fit.ls_circle3@1` and `fit.ls_sphere@1` (W2
/// with R1 dry run, D0), in [`PRIMITIVES`] order.
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
    // Least squares is defined for every primitive, so this cannot fail; the fallback keeps the
    // function total without a panic path.
    algorithm_id(p, Method::LeastSquares, VERSION).unwrap_or_else(|_| AlgorithmId {
        module: "fit".to_string(),
        feature: p.slug().to_string(),
        method: Method::LeastSquares.slug().to_string(),
        version: VERSION,
    })
}

/// Rule 1. Returns the fixed radius in metres, if any.
fn params_for(p: Primitive, pts: &PointsView<'_>, params: &FitParams) -> Result<Option<f64>, FitError> {
    params.validate(pts)?;
    if params.method != Method::LeastSquares {
        return Err(FitError::InvalidParams { field: "method" });
    }
    if !matches!(params.weights, Weights::Uniform) {
        return Err(FitError::InvalidParams { field: "weights" });
    }
    for c in &params.constraints {
        let ok = match c {
            Constraint::FixedRadius(_) => true,
            Constraint::ProjectionPlane { .. } => p == Primitive::Circle2,
            Constraint::FixedAxisDirection(_) => false,
        };
        if !ok {
            return Err(FitError::InvalidParams { field: "constraints" });
        }
    }
    Ok(params.fixed_radius())
}

fn ill() -> FitError {
    FitError::IllConditioned { cond: None }
}

/// Solves the `K - 1` leading columns of a `K x K` blocked-QR factor against its last column
/// (rule 3); `None` when rank deficient.
fn solve_factor<const K: usize>(r: &[[f64; K]; K], rank_tol: f64) -> Result<Option<Vec<f64>>, FitError> {
    let m = K - 1;
    let mut data = alloc(m * m)?;
    let mut b = alloc(m)?;
    for row in r.iter().take(m) {
        data.extend_from_slice(&row[..m]);
        b.push(row[m]);
    }
    let a = Mat::new(m, m, data)?;
    match lstsq(&a, &b, rank_tol) {
        Ok(s) if s.diag.rank == m => Ok(Some(s.x)),
        Ok(_) => Ok(None),
        Err(e) if LaError::of(&e).is_some() => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Rule 3, circle: the algebraic circle of the first two coordinates of the scaled data.
fn kasa_circle(c: &Centred, rank_tol: f64) -> Result<([f64; 2], f64), FitError> {
    let xyz = c.xyz();
    let r = tsqr::<4, _>(c.len(), |i, out| {
        let q = xyz.get(3 * i..3 * i + 2).ok_or(FitError::BadLength { len: xyz.len() })?;
        let (u, v) = (q[0], q[1]);
        *out = [u, v, 1.0, -(u * u + v * v)];
        Ok(())
    })?;
    let x = solve_factor(&r, rank_tol)?.ok_or_else(ill)?;
    let (d, e, g) = (x[0], x[1], x[2]);
    let r2 = (d * d + e * e) / 4.0 - g;
    if !(r2.is_finite() && r2 > 0.0) {
        return Err(ill());
    }
    Ok(([-d / 2.0, -e / 2.0], f::sqrt(r2)?))
}

/// Rule 3, sphere: the algebraic sphere of the scaled data.
fn kasa_sphere(c: &Centred, rank_tol: f64) -> Result<(V3, f64), FitError> {
    let xyz = c.xyz();
    let r = tsqr::<5, _>(c.len(), |i, out| {
        let q = xyz.get(3 * i..3 * i + 3).ok_or(FitError::BadLength { len: xyz.len() })?;
        let (x, y, z) = (q[0], q[1], q[2]);
        *out = [x, y, z, 1.0, -((x * x + y * y) + z * z)];
        Ok(())
    })?;
    let s = solve_factor(&r, rank_tol)?.ok_or_else(ill)?;
    let (d, e, ff, g) = (s[0], s[1], s[2], s[3]);
    let r2 = ((d * d + e * e) + ff * ff) / 4.0 - g;
    if !(r2.is_finite() && r2 > 0.0) {
        return Err(ill());
    }
    Ok(([-d / 2.0, -e / 2.0, -ff / 2.0], f::sqrt(r2)?))
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Angular coverage of sphere data about `centre` (scaled coordinates), radians: the apex
/// angle of the smallest cone about the mean direction that holds every point, so a spherical
/// cap of apex angle `2 alpha` has coverage `2 alpha` (a circle arc of the same angle has the
/// same circle coverage) and a full sphere `2 pi`.
///
/// With `u_i = (q_i - centre) / red::norm2(q_i - centre)` (points at the centre skipped), `m`
/// the component-wise `red::sum` of the `u_i` and `m' = m / red::norm2(m)`: coverage =
/// `total_min(2 pi, 2 max_i atan2(red::norm2(u_i x m'), red::dot(u_i, m')))`, and `2 pi` when
/// `red::norm2(m)` is zero or no point remains.
pub fn sphere_coverage(c: &Centred, centre: V3) -> Result<f64, FitError> {
    let n = c.len();
    let mut cols: [Vec<f64>; 3] = [alloc(n)?, alloc(n)?, alloc(n)?];
    for q in c.xyz().chunks_exact(3) {
        let w = [q[0] - centre[0], q[1] - centre[1], q[2] - centre[2]];
        let rho = red::norm2(&w)?;
        if rho > 0.0 {
            for (col, wk) in cols.iter_mut().zip(w) {
                col.push(wk / rho);
            }
        }
    }
    let m = [red::sum(&cols[0])?, red::sum(&cols[1])?, red::sum(&cols[2])?];
    let mn = red::norm2(&m)?;
    if cols[0].is_empty() || mn.is_nan() || mn <= 0.0 {
        return Ok(TWO_PI);
    }
    let mh = [m[0] / mn, m[1] / mn, m[2] / mn];
    let mut widest = 0.0;
    for i in 0..cols[0].len() {
        let u = [cols[0][i], cols[1][i], cols[2][i]];
        let ang = f::atan2(red::norm2(&cross(u, mh))?, red::dot(&u, &mh)?)?;
        widest = f::total_max(widest, ang)?;
    }
    Ok(f::total_min(TWO_PI, 2.0 * widest)?)
}

/// The free parameters of a round model, the fixed radius (scaled) left out.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    /// Shape parameters before the radius (circle2 2, sphere 3, circle3 5).
    shape: usize,
    radius: Option<f64>,
}

impl Layout {
    fn n(&self) -> usize {
        self.shape + usize::from(self.radius.is_none())
    }

    fn radius(&self, x: &[f64]) -> Result<f64, NumError> {
        match self.radius {
            Some(r) => Ok(r),
            None => x.get(self.shape).copied().ok_or(NumError::Domain { op: OP, detail: "param_count" }),
        }
    }

    fn start(&self, shape: &[f64], r: f64) -> Result<Vec<f64>, FitError> {
        let mut v = alloc(self.n())?;
        v.extend_from_slice(shape);
        if self.radius.is_none() {
            v.push(r);
        }
        Ok(v)
    }
}

fn row_mut<'a>(j: &'a mut [f64], i: usize, np: usize) -> Result<&'a mut [f64], NumError> {
    j.get_mut(i * np..(i + 1) * np).ok_or(NumError::Domain { op: OP, detail: "jac_len" })
}

fn res_mut(r: &mut [f64], i: usize) -> Result<&mut f64, NumError> {
    r.get_mut(i).ok_or(NumError::Domain { op: OP, detail: "resid_len" })
}

/// Rule 4 for circle2 (`K = 2`, first two coordinates) and the sphere (`K = 3`).
struct Round<'a, const K: usize> {
    q: &'a [f64],
    layout: Layout,
}

impl<const K: usize> NllsProblem for Round<'_, K> {
    fn n_params(&self) -> usize {
        self.layout.n()
    }

    fn n_resid(&self) -> usize {
        self.q.len() / 3
    }

    fn eval(&self, x: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), NumError> {
        let cen = x.get(..K).ok_or(NumError::Domain { op: OP, detail: "param_count" })?;
        let rad = self.layout.radius(x)?;
        let np = self.layout.n();
        for (i, qi) in self.q.chunks_exact(3).enumerate() {
            let mut w = [0.0; K];
            for k in 0..K {
                w[k] = qi[k] - cen[k];
            }
            let rho = red::norm2(&w)?;
            *res_mut(r, i)? = rho - rad;
            let row = row_mut(j, i, np)?;
            for k in 0..K {
                row[k] = if rho > 0.0 { -(w[k] / rho) } else { 0.0 };
            }
            if self.layout.radius.is_none() {
                row[K] = -1.0;
            }
        }
        Ok(())
    }
}

/// The 3D circle's start frame (scaled): centre `c0`, normal `n0`, its basis `e1`, `e2`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame {
    c0: V3,
    n0: V3,
    e1: V3,
    e2: V3,
}

impl Frame {
    /// Rule 4: `(c, n, t)` of the parameters `(a, b, x, y, z)`.
    fn circle(&self, p: &[f64]) -> Result<(V3, V3, f64), NumError> {
        let [a, b, x, y, z] = match p.get(..5) {
            Some(&[a, b, x, y, z]) => [a, b, x, y, z],
            _ => return Err(NumError::Domain { op: OP, detail: "param_count" }),
        };
        let mut dd = [0.0; 3];
        let mut c = [0.0; 3];
        for k in 0..3 {
            dd[k] = (self.n0[k] + a * self.e1[k]) + b * self.e2[k];
            c[k] = ((self.c0[k] + x * self.e1[k]) + y * self.e2[k]) + z * self.n0[k];
        }
        let t = red::norm2(&dd)?;
        if t.is_nan() || t <= 0.0 {
            return Err(NumError::Domain { op: OP, detail: "zero_normal" });
        }
        Ok((c, [dd[0] / t, dd[1] / t, dd[2] / t], t))
    }
}

/// Per-point parts of the 3D circle model: `(w, h, perp, rho)`.
fn circle3_parts(qi: &[f64], c: &V3, n: &V3) -> Result<(V3, f64, V3, f64), NumError> {
    let w = [qi[0] - c[0], qi[1] - c[1], qi[2] - c[2]];
    let h = red::dot(&w, n)?;
    let perp = [w[0] - h * n[0], w[1] - h * n[1], w[2] - h * n[2]];
    let rho = red::norm2(&perp)?;
    Ok((w, h, perp, rho))
}

struct Circle3Problem<'a> {
    q: &'a [f64],
    fr: Frame,
    layout: Layout,
}

impl NllsProblem for Circle3Problem<'_> {
    fn n_params(&self) -> usize {
        self.layout.n()
    }

    fn n_resid(&self) -> usize {
        2 * (self.q.len() / 3)
    }

    fn eval(&self, x: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), NumError> {
        let (c, n, t) = self.fr.circle(x)?;
        let rad = self.layout.radius(x)?;
        let np = self.layout.n();
        let fr = &self.fr;
        let (ne1, ne2, nn0) = (red::dot(&n, &fr.e1)?, red::dot(&n, &fr.e2)?, red::dot(&n, &fr.n0)?);
        for (i, qi) in self.q.chunks_exact(3).enumerate() {
            let (w, h, perp, rho) = circle3_parts(qi, &c, &n)?;
            let g = if rho > 0.0 { [perp[0] / rho, perp[1] / rho, perp[2] / rho] } else { [0.0; 3] };
            let (we1, we2) = (red::dot(&w, &fr.e1)?, red::dot(&w, &fr.e2)?);
            let (ge1, ge2, gn0) = (red::dot(&g, &fr.e1)?, red::dot(&g, &fr.e2)?, red::dot(&g, &fr.n0)?);
            *res_mut(r, 2 * i)? = h;
            *res_mut(r, 2 * i + 1)? = rho - rad;
            let rh = row_mut(j, 2 * i, np)?;
            rh[0] = (we1 - h * ne1) / t;
            rh[1] = (we2 - h * ne2) / t;
            rh[2] = -ne1;
            rh[3] = -ne2;
            rh[4] = -nn0;
            if self.layout.radius.is_none() {
                rh[5] = 0.0;
            }
            let rr = row_mut(j, 2 * i + 1, np)?;
            rr[0] = -(h * ge1) / t;
            rr[1] = -(h * ge2) / t;
            rr[2] = -ge1;
            rr[3] = -ge2;
            rr[4] = -gn0;
            if self.layout.radius.is_none() {
                rr[5] = -1.0;
            }
        }
        Ok(())
    }
}

fn solve<P: NllsProblem>(prob: &P, x0: &[f64], params: &FitParams) -> Result<nlls::NllsResult, FitError> {
    nlls::solve(prob, x0, &params.nlls).map_err(|e| FitError::from_solver(e, &params.nlls))
}

fn layout(c: &Centred, shape: usize, fixed: Option<f64>) -> Result<Layout, FitError> {
    let radius = match fixed {
        Some(r) => Some(c.map().forward_length(r)?),
        None => None,
    };
    Ok(Layout { shape, radius })
}

fn finish(
    p: Primitive,
    c: &Centred,
    feature: FittedFeature,
    scaled_res: &[f64],
    res: &nlls::NllsResult,
) -> Result<FitResult, FitError> {
    let map = c.map();
    let feature = map.inverse_feature(&feature)?;
    let mut out = alloc(scaled_res.len())?;
    for &ri in scaled_res {
        out.push(map.inverse_length(ri)?);
    }
    FitResult::assemble(&feature, out, FitDiagnostics::from_nlls(res), None, id(p))
}

fn flush(x: f64) -> f64 {
    if x.is_subnormal() {
        0.0
    } else {
        x
    }
}

impl Fit for Circle2Ls {
    fn algorithm_id(&self) -> AlgorithmId {
        id(Primitive::Circle2)
    }

    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, _seed: Option<u64>) -> Result<FitResult, FitError> {
        let fixed = params_for(Primitive::Circle2, pts, params)?;
        // Projection onto the plane, as `ls_lin` rule 7.
        let (p0, e1, e2) = projection_frame(params)?;
        let mut buf = alloc(pts.xyz().len())?;
        for x in pts.xyz().chunks_exact(3) {
            let dx = [x[0] - p0[0], x[1] - p0[1], x[2] - p0[2]];
            buf.push(flush(red::dot(&dx, &e1)?));
            buf.push(flush(red::dot(&dx, &e2)?));
            buf.push(0.0);
        }
        let view = PointsView::new(&buf, None)?;
        let (c, _) = check_points(Primitive::Circle2, &view, &params.thresholds)?;
        let (cen, r0) = kasa_circle(&c, params.thresholds.rank_tol)?;
        let lay = layout(&c, 2, fixed)?;
        let x0 = lay.start(&cen, r0)?;
        let res = solve(&Round::<2> { q: c.xyz(), layout: lay }, &x0, params)?;
        let rad = lay.radius(&res.p)?;
        let feature = FittedFeature::Circle2 { c: [res.p[0], res.p[1]], r: rad };
        finish(Primitive::Circle2, &c, feature, &res.residuals, &res)
    }
}

impl Fit for SphereLs {
    fn algorithm_id(&self) -> AlgorithmId {
        id(Primitive::Sphere)
    }

    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, _seed: Option<u64>) -> Result<FitResult, FitError> {
        let fixed = params_for(Primitive::Sphere, pts, params)?;
        let (c, _) = check_points(Primitive::Sphere, pts, &params.thresholds)?;
        let (cen, r0) = kasa_sphere(&c, params.thresholds.rank_tol)?;
        let cov = sphere_coverage(&c, cen)?;
        if f::total_cmp(cov, params.thresholds.min_coverage)? == Ordering::Less {
            return Err(FitError::InsufficientCoverage {
                n: c.len(),
                coverage: cov,
                need: params.thresholds.min_coverage,
            });
        }
        let lay = layout(&c, 3, fixed)?;
        let x0 = lay.start(&cen, r0)?;
        let res = solve(&Round::<3> { q: c.xyz(), layout: lay }, &x0, params)?;
        let rad = lay.radius(&res.p)?;
        let feature = FittedFeature::Sphere { c: [res.p[0], res.p[1], res.p[2]], r: rad };
        finish(Primitive::Sphere, &c, feature, &res.residuals, &res)
    }
}

impl Fit for Circle3Ls {
    fn algorithm_id(&self) -> AlgorithmId {
        id(Primitive::Circle3)
    }

    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, _seed: Option<u64>) -> Result<FitResult, FitError> {
        let fixed = params_for(Primitive::Circle3, pts, params)?;
        let (c, _) = check_points(Primitive::Circle3, pts, &params.thresholds)?;
        // REQ-002: the start normal is the least-squares plane's (`KRN.LSQ.LIN`).
        let mut plane_params = FitParams::new(Method::LeastSquares);
        plane_params.thresholds = params.thresholds;
        let n0 = match PlaneLs.fit(pts, &plane_params, None) {
            Ok(FitResult { feature: FittedFeature::Plane { n, .. }, .. }) => n,
            Ok(_) | Err(FitError::IllConditioned { .. } | FitError::Collinear { .. }) => return Err(ill()),
            Err(e) => return Err(e),
        };
        let (e1, e2, cen, r0) = init::kasa(&c, n0)?.ok_or_else(ill)?;
        let c0 = [cen[0] * e1[0] + cen[1] * e2[0], cen[0] * e1[1] + cen[1] * e2[1], cen[0] * e1[2] + cen[1] * e2[2]];
        let fr = Frame { c0, n0, e1, e2 };
        let lay = layout(&c, 5, fixed)?;
        let x0 = lay.start(&[0.0; 5], r0)?;
        let res = solve(&Circle3Problem { q: c.xyz(), fr, layout: lay }, &x0, params)?;
        let (cc, n, _) = fr.circle(&res.p)?;
        let rad = lay.radius(&res.p)?;
        let mut signed = alloc(c.len())?;
        for qi in c.xyz().chunks_exact(3) {
            let (_, h, _, rho) = circle3_parts(qi, &cc, &n)?;
            let e = rho - rad;
            let d = f::hypot(h, e)?;
            signed.push(if e < 0.0 { -d } else { d });
        }
        let feature = FittedFeature::Circle3 { c: cc, n, r: rad };
        finish(Primitive::Circle3, &c, feature, &signed, &res)
    }
}
