//! Linearisable minimum-zone 3D line problem for `fid_solv::seq` and `fid_solv::cert`,
//! plus exact fixed-axis SOCP initialisation via `fid_solv::conic::solve_socp`
//! (spec node `KRN.MZ.LIN.LINE3`).
//!
//! Parameters relative to a fixed orthonormal frame `(P0, A, E1, E2)` in the centred,
//! power-of-two scaled coordinates of `contract::Centred`:
//! - Free direction: `p = (a, b, u, v, R)` (5 parameters)
//! - Fixed direction: `p = (u, v, R)` (3 parameters)
//!
//! The axis direction is `D = A + a E1 + b E2` (unnormalised) and axis point
//! `P = P0 + u E1 + v E2`. For point `q_i`, `w = q_i - P` and orthogonal distance to the axis
//! is `rho_i = |w x D| / |D|`. Each constraint row `(i, 0)` is `g_i = rho_i - R <= 0`; the
//! objective is `R` (half the zone diameter, so the reported zone diameter is `2 * R * scale`).
//!
//! Unlike a hollow cylinder, a 3D line cloud may contain interior points right on the axis
//! (`rho_i == 0`). Distance-only evaluation and interval enclosures use `|w x D| / |D|` with
//! `Interval::sqr()` (never dividing by `rho`), while `local_problem` uses the valid
//! subgradient `[0, ..., 0, -1]` whenever `rho_i` is negligible so interior points on the axis
//! never cause division by zero.

#![allow(clippy::needless_range_loop)]

use fid_num::f::{total_max, total_min};
use fid_num::la::Mat;
use fid_num::red;
use fid_solv::cert::recomp::{ActiveSetProblem, Interval};
use fid_solv::cert::{CertError, RowLabel};
use fid_solv::conic::{solve_socp, Cone, ConicProblem, ConicProblemBuilder, Csc};
use fid_solv::seq::LinearisableProblem;

use crate::contract::{FitError, V3};

/// Step-bound scale of `R` relative to `len_scale` so `R` can always absorb infeasibility.
pub const R_SCALE: f64 = 1_048_576.0;

/// Exact-penalty factor of the merit function (dominates multiplier sum 1).
pub const PENALTY: f64 = 2.0;

/// Relative distance threshold below which a point is treated as lying on the axis in
/// `local_problem` (subgradient `[0, ..., 0, -1]`).
const ON_AXIS_REL: f64 = 1e-14;

fn cb(reason: &'static str) -> CertError {
    CertError::Callback { reason }
}

fn num(e: fid_num::NumError) -> CertError {
    CertError::Numeric(e)
}

fn ip(x: f64) -> Result<Interval, CertError> {
    Interval::point(x)
}

/// Local orthonormal frame in centred, scaled coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    pub p0: V3,
    pub a: V3,
    pub e1: V3,
    pub e2: V3,
}

/// Solves the exact fixed-axis 2D minimum enclosing circle in the `(E1, E2)` plane of `fr`
/// using `fid_solv::conic::solve_socp`:
///
/// Minimise `r` subject to `||(u_i - cx, v_i - cy)||_2 <= r` for all centred points `q`.
///
/// Returns `(cx, cy, r)` in scaled coordinates.
pub(crate) fn socp_fixed_axis_mec(q: &[f64], fr: &Frame) -> Result<(f64, f64, f64), FitError> {
    let n = q.len() / 3;
    if n == 0 {
        return Err(FitError::TooFewPoints { n: 0, need: 1 });
    }
    // Variables x = [cx, cy, r]. Objective q_obj = [0, 0, 1].
    // For each point i, cone i is SOC(3):
    //   s_{3i+0} = r           -> A row [ 0,  0, -1], b =  0
    //   s_{3i+1} = cx - u_i    -> A row [-1,  0,  0], b = -u_i
    //   s_{3i+2} = cy - v_i    -> A row [ 0, -1,  0], b = -v_i
    // Notice every column has exactly n nonzeros (-1.0).
    let m = 3 * n;
    let mut b = Vec::new();
    b.try_reserve_exact(m).map_err(|_| {
        FitError::Num(fid_num::NumError::Domain { op: "fit.mz_line3.socp", detail: "allocation_failed" })
    })?;
    let mut cones = Vec::new();
    cones.try_reserve_exact(n).map_err(|_| {
        FitError::Num(fid_num::NumError::Domain { op: "fit.mz_line3.socp", detail: "allocation_failed" })
    })?;
    let mut rowval = Vec::new();
    rowval.try_reserve_exact(m).map_err(|_| {
        FitError::Num(fid_num::NumError::Domain { op: "fit.mz_line3.socp", detail: "allocation_failed" })
    })?;
    let mut nzval = Vec::new();
    nzval.try_reserve_exact(m).map_err(|_| {
        FitError::Num(fid_num::NumError::Domain { op: "fit.mz_line3.socp", detail: "allocation_failed" })
    })?;

    for pt in q.chunks_exact(3) {
        let dx = [pt[0] - fr.p0[0], pt[1] - fr.p0[1], pt[2] - fr.p0[2]];
        let ui = red::dot(&dx, &fr.e1)?;
        let vi = red::dot(&dx, &fr.e2)?;
        b.push(0.0);
        b.push(-ui);
        b.push(-vi);
        cones.push(Cone::Soc(3));
    }
    let colptr = vec![0, n, 2 * n, 3 * n];
    // Column 0 (cx): rows 3i + 1
    for i in 0..n {
        rowval.push(3 * i + 1);
        nzval.push(-1.0);
    }
    // Column 1 (cy): rows 3i + 2
    for i in 0..n {
        rowval.push(3 * i + 2);
        nzval.push(-1.0);
    }
    // Column 2 (r): rows 3i + 0
    for i in 0..n {
        rowval.push(3 * i);
        nzval.push(-1.0);
    }
    let a = Csc::new(m, 3, colptr, rowval, nzval);
    let prob = ConicProblemBuilder::new(vec![0.0, 0.0, 1.0], a, b, cones)
        .build()
        .map_err(|_| FitError::SeqFailed { cause: "socp_build", starts: 1 })?;
    let sol = solve_socp(&prob).map_err(|_| FitError::SeqFailed { cause: "socp", starts: 1 })?;
    let (cx, cy, r) = (sol.x[0], sol.x[1], sol.x[2]);
    if !(cx.is_finite() && cy.is_finite() && r.is_finite() && r >= 0.0) {
        return Err(FitError::SeqFailed { cause: "socp_nonfinite", starts: 1 });
    }
    Ok((cx, cy, r))
}

/// Minimum-zone 3D line problem in centred, scaled coordinates.
#[derive(Debug, Clone)]
pub(crate) struct MzLine3Problem<'a> {
    pub q: &'a [f64],
    pub fr: Frame,
    /// `true` for free axis direction `(a, b, u, v, R)`, `false` for fixed direction `(u, v, R)`.
    pub dir: bool,
    /// Step-bound length scale (`u`, `v`, `R` in scaled units).
    pub len_scale: f64,
    /// KKT data scale (largest absolute coordinate / start radius).
    pub data_scale: f64,
    /// Start radius `R0` (scaled), used by `start_from_set`.
    pub r0: f64,
}

impl MzLine3Problem<'_> {
    fn np(&self) -> usize {
        if self.dir {
            5
        } else {
            3
        }
    }

    fn n_pts(&self) -> usize {
        self.q.len() / 3
    }

    fn point(&self, i: usize) -> Result<V3, CertError> {
        let s = self.q.get(3 * i..3 * i + 3).ok_or(cb("point index"))?;
        Ok([s[0], s[1], s[2]])
    }

    fn full(&self, p: &[f64]) -> Result<[f64; 5], CertError> {
        match (self.dir, p) {
            (true, [a, b, u, v, r]) => Ok([*a, *b, *u, *v, *r]),
            (false, [u, v, r]) => Ok([0.0, 0.0, *u, *v, *r]),
            _ => Err(cb("parameter count")),
        }
    }

    fn axis(&self, p: &[f64]) -> Result<(V3, V3, f64), CertError> {
        let x = self.full(p)?;
        let mut d = [0.0; 3];
        let mut pp = [0.0; 3];
        for k in 0..3 {
            d[k] = (self.fr.a[k] + x[0] * self.fr.e1[k]) + x[1] * self.fr.e2[k];
            pp[k] = (self.fr.p0[k] + x[2] * self.fr.e1[k]) + x[3] * self.fr.e2[k];
        }
        Ok((pp, d, x[4]))
    }

    /// Distance `rho_i = |w x D| / |D|` (works cleanly even when `rho_i == 0`).
    fn dist_only(&self, i: usize, pp: V3, d: V3) -> Result<f64, CertError> {
        let q = self.point(i)?;
        let w = [q[0] - pp[0], q[1] - pp[1], q[2] - pp[2]];
        let dd = red::dot(&d, &d).map_err(num)?;
        let t = red::dot(&w, &d).map_err(num)? / dd;
        let perp = [w[0] - t * d[0], w[1] - t * d[1], w[2] - t * d[2]];
        let rho = red::norm2(&perp).map_err(num)?;
        if rho.is_nan() {
            return Err(cb("non-finite distance"));
        }
        Ok(rho)
    }

    /// Distance and exact gradient for active points (`rho_i > 0`).
    fn dist_grad(&self, i: usize, pp: V3, d: V3) -> Result<(f64, [f64; 4]), CertError> {
        let q = self.point(i)?;
        let w = [q[0] - pp[0], q[1] - pp[1], q[2] - pp[2]];
        let dd = red::dot(&d, &d).map_err(num)?;
        let t = red::dot(&w, &d).map_err(num)? / dd;
        let perp = [w[0] - t * d[0], w[1] - t * d[1], w[2] - t * d[2]];
        let rho = red::norm2(&perp).map_err(num)?;
        if rho.is_nan() || rho <= 0.0 {
            return Err(cb("point on the axis"));
        }
        let g1 = red::dot(&perp, &self.fr.e1).map_err(num)? / rho;
        let g2 = red::dot(&perp, &self.fr.e2).map_err(num)? / rho;
        Ok((rho, [-(t * g1), -(t * g2), -g1, -g2]))
    }

    /// Distance and subgradient for `local_problem` (handles `rho_i == 0` gracefully).
    fn dist_subgrad(&self, i: usize, pp: V3, d: V3) -> Result<(f64, [f64; 4]), CertError> {
        let q = self.point(i)?;
        let w = [q[0] - pp[0], q[1] - pp[1], q[2] - pp[2]];
        let dd = red::dot(&d, &d).map_err(num)?;
        let t = red::dot(&w, &d).map_err(num)? / dd;
        let perp = [w[0] - t * d[0], w[1] - t * d[1], w[2] - t * d[2]];
        let rho = red::norm2(&perp).map_err(num)?;
        if rho.is_nan() {
            return Err(cb("non-finite distance"));
        }
        if rho <= ON_AXIS_REL * self.len_scale {
            return Ok((rho, [0.0; 4]));
        }
        let g1 = red::dot(&perp, &self.fr.e1).map_err(num)? / rho;
        let g2 = red::dot(&perp, &self.fr.e2).map_err(num)? / rho;
        Ok((rho, [-(t * g1), -(t * g2), -g1, -g2]))
    }

    fn row(&self, g: [f64; 4]) -> Vec<f64> {
        if self.dir {
            vec![g[0], g[1], g[2], g[3], -1.0]
        } else {
            vec![g[2], g[3], -1.0]
        }
    }

    /// Interval distance enclosure `rho = |w x D| / |D|` using `Interval::sqr()` so `rho == 0`
    /// never fails interval square-root!
    fn dist_iv(&self, i: usize, b: &[Interval]) -> Result<Interval, CertError> {
        let z = ip(0.0)?;
        let (a, bb, u, v) = match (self.dir, b) {
            (true, [a, bb, u, v, _]) => (*a, *bb, *u, *v),
            (false, [u, v, _]) => (z, z, *u, *v),
            _ => return Err(cb("box length")),
        };
        let q = self.point(i)?;
        let mut d = [z; 3];
        let mut w = [z; 3];
        for k in 0..3 {
            d[k] = ip(self.fr.a[k])?.add(a.mul(ip(self.fr.e1[k])?)?)?.add(bb.mul(ip(self.fr.e2[k])?)?)?;
            let pk = ip(self.fr.p0[k])?.add(u.mul(ip(self.fr.e1[k])?)?)?.add(v.mul(ip(self.fr.e2[k])?)?)?;
            w[k] = ip(q[k])?.sub(pk)?;
        }
        let c = [
            w[1].mul(d[2])?.sub(w[2].mul(d[1])?)?,
            w[2].mul(d[0])?.sub(w[0].mul(d[2])?)?,
            w[0].mul(d[1])?.sub(w[1].mul(d[0])?)?,
        ];
        let c_sq = c[0].sqr()?.add(c[1].sqr()?)?.add(c[2].sqr()?)?;
        let d_sq = d[0].sqr()?.add(d[1].sqr()?)?.add(d[2].sqr()?)?;
        c_sq.sqrt()?.div(d_sq.sqrt()?)
    }

    /// Interval `(rho, grad)` for active points (`rho > 0`).
    fn dist_grad_iv(&self, i: usize, b: &[Interval]) -> Result<(Interval, [Interval; 4]), CertError> {
        let z = ip(0.0)?;
        let (a, bb, u, v) = match (self.dir, b) {
            (true, [a, bb, u, v, _]) => (*a, *bb, *u, *v),
            (false, [u, v, _]) => (z, z, *u, *v),
            _ => return Err(cb("box length")),
        };
        let q = self.point(i)?;
        let mut d = [z; 3];
        let mut w = [z; 3];
        for k in 0..3 {
            d[k] = ip(self.fr.a[k])?.add(a.mul(ip(self.fr.e1[k])?)?)?.add(bb.mul(ip(self.fr.e2[k])?)?)?;
            let pk = ip(self.fr.p0[k])?.add(u.mul(ip(self.fr.e1[k])?)?)?.add(v.mul(ip(self.fr.e2[k])?)?)?;
            w[k] = ip(q[k])?.sub(pk)?;
        }
        let dot = |x: &[Interval; 3], y: &[Interval; 3]| -> Result<Interval, CertError> {
            x[0].mul(y[0])?.add(x[1].mul(y[1])?)?.add(x[2].mul(y[2])?)
        };
        let c = [
            w[1].mul(d[2])?.sub(w[2].mul(d[1])?)?,
            w[2].mul(d[0])?.sub(w[0].mul(d[2])?)?,
            w[0].mul(d[1])?.sub(w[1].mul(d[0])?)?,
        ];
        let c_sq = c[0].sqr()?.add(c[1].sqr()?)?.add(c[2].sqr()?)?;
        let d_sq = d[0].sqr()?.add(d[1].sqr()?)?.add(d[2].sqr()?)?;
        let rho = c_sq.sqrt()?.div(d_sq.sqrt()?)?;
        let t = dot(&w, &d)?.div(d_sq)?;
        let mut perp = [z; 3];
        for k in 0..3 {
            perp[k] = w[k].sub(t.mul(d[k])?)?;
        }
        let e1 = [ip(self.fr.e1[0])?, ip(self.fr.e1[1])?, ip(self.fr.e1[2])?];
        let e2 = [ip(self.fr.e2[0])?, ip(self.fr.e2[1])?, ip(self.fr.e2[2])?];
        let g1 = dot(&perp, &e1)?.div(rho)?;
        let g2 = dot(&perp, &e2)?.div(rho)?;
        Ok((rho, [t.mul(g1)?.neg(), t.mul(g2)?.neg(), g1.neg(), g2.neg()]))
    }

    fn r_of(&self, b: &[Interval]) -> Result<Interval, CertError> {
        b.last().copied().ok_or(cb("box length"))
    }

    fn row_iv(&self, g: [Interval; 4]) -> Result<Vec<Interval>, CertError> {
        let m1 = ip(-1.0)?;
        Ok(if self.dir { vec![g[0], g[1], g[2], g[3], m1] } else { vec![g[2], g[3], m1] })
    }

    /// `(max_i rho_i, min_i rho_i)` at `p` (scaled).
    pub(crate) fn extremes(&self, p: &[f64]) -> Result<(f64, f64), CertError> {
        let (pp, d, _) = self.axis(p)?;
        let mut hi = 0.0_f64;
        let mut lo = f64::INFINITY;
        for i in 0..self.n_pts() {
            let rho = self.dist_only(i, pp, d)?;
            hi = total_max(hi, rho).map_err(num)?;
            lo = total_min(lo, rho).map_err(num)?;
        }
        Ok((hi, lo))
    }

    /// All radial distances `rho_i` at `p` (scaled).
    pub(crate) fn distances(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        let (pp, d, _) = self.axis(p)?;
        let mut out = Vec::with_capacity(self.n_pts());
        for i in 0..self.n_pts() {
            out.push(self.dist_only(i, pp, d)?);
        }
        Ok(out)
    }

    /// Axis point (foot of perpendicular from origin/centroid) and unit direction of `p`.
    pub(crate) fn line(&self, p: &[f64]) -> Result<(V3, V3), CertError> {
        let (pp, d, _) = self.axis(p)?;
        let n = red::norm2(&d).map_err(num)?;
        let u = [d[0] / n, d[1] / n, d[2] / n];
        let s = red::dot(&pp, &u).map_err(num)?;
        Ok(([pp[0] - s * u[0], pp[1] - s * u[1], pp[2] - s * u[2]], u))
    }
}

fn start_or_error(
    r: Result<fid_solv::nlls::NllsResult, fid_solv::nlls::SolverError>,
    x0: Vec<f64>,
) -> Result<Vec<f64>, CertError> {
    use fid_solv::nlls::{SolverError, Stage};
    match r {
        Ok(r) => Ok(r.p),
        Err(SolverError::Numeric { error, .. }) => Err(CertError::Numeric(error)),
        Err(SolverError::NonFinite { stage, index, .. }) => {
            let op = match stage {
                Stage::Start => "fit.mz_line3.start.start",
                Stage::Residual => "fit.mz_line3.start.residual",
                Stage::Jacobian => "fit.mz_line3.start.jacobian",
                Stage::Cost => "fit.mz_line3.start.cost",
                Stage::Step => "fit.mz_line3.start.step",
            };
            Err(CertError::Numeric(fid_num::NumError::Infinite { op, index }))
        }
        Err(_) => Ok(x0),
    }
}

struct SetSystem<'a, 'b> {
    prob: &'a MzLine3Problem<'b>,
    set: &'a [RowLabel],
}

impl fid_solv::nlls::NllsProblem for SetSystem<'_, '_> {
    fn n_params(&self) -> usize {
        self.prob.np()
    }

    fn n_resid(&self) -> usize {
        self.set.len()
    }

    fn eval(&self, p: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), fid_num::NumError> {
        let dom = |detail| fid_num::NumError::Domain { op: "fit.mz_line3.start", detail };
        let (pp, d, rad) = self.prob.axis(p).map_err(|_| dom("axis"))?;
        let np = self.prob.np();
        for (i, l) in self.set.iter().enumerate() {
            let (rho, g) = self.prob.dist_grad(l.point as usize, pp, d).map_err(|_| dom("distance"))?;
            *r.get_mut(i).ok_or(dom("resid"))? = rho - rad;
            let row = self.prob.row(g);
            j.get_mut(i * np..(i + 1) * np).ok_or(dom("jac"))?.copy_from_slice(&row);
        }
        Ok(())
    }
}

impl ActiveSetProblem for MzLine3Problem<'_> {
    fn n_params(&self) -> usize {
        self.np()
    }

    fn expected_active(&self) -> usize {
        self.np()
    }

    fn row_labels(&self) -> Vec<RowLabel> {
        (0..self.n_pts() as u32).map(|point| RowLabel { point, side: 0 }).collect()
    }

    fn is_linear(&self) -> bool {
        false
    }

    fn start_from_set(&self, set: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        let x0 = if self.dir { vec![0.0, 0.0, 0.0, 0.0, self.r0] } else { vec![0.0, 0.0, self.r0] };
        let sys = SetSystem { prob: self, set };
        start_or_error(fid_solv::nlls::solve(&sys, &x0, &fid_solv::nlls::NllsConfig::default()), x0)
    }

    fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
        let (pp, d, r) = self.axis(p)?;
        let mut res = Vec::with_capacity(set.len());
        let mut rows = Vec::with_capacity(set.len());
        for l in set {
            let (rho, g) = self.dist_grad(l.point as usize, pp, d)?;
            res.push(rho - r);
            rows.push(self.row(g));
        }
        Ok((res, Mat::from_rows(&rows).map_err(num)?))
    }

    fn value(&self, p: &[f64]) -> Result<f64, CertError> {
        Ok(self.full(p)?[4])
    }

    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        let (pp, d, r) = self.axis(p)?;
        let mut out = Vec::with_capacity(self.n_pts());
        for i in 0..self.n_pts() {
            out.push(self.dist_only(i, pp, d)? - r);
        }
        Ok(out)
    }

    fn objective_gradient(&self, _p: &[f64]) -> Result<Vec<f64>, CertError> {
        let mut g = vec![0.0; self.np()];
        if let Some(x) = g.last_mut() {
            *x = 1.0;
        }
        Ok(g)
    }

    fn constraint_gradients(&self, set: &[RowLabel], p: &[f64]) -> Result<Mat, CertError> {
        Ok(self.residuals_and_jacobian(set, p)?.1)
    }

    fn residual_enclosure(&self, set: &[RowLabel], p: &[f64]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut b = Vec::with_capacity(p.len());
        for &x in p {
            b.push(ip(x)?);
        }
        let r = self.r_of(&b)?;
        let mut out = Vec::with_capacity(set.len());
        for l in set {
            out.push(self.dist_iv(l.point as usize, &b)?.sub(r)?);
        }
        Ok(Some(out))
    }

    fn jacobian_enclosure(&self, set: &[RowLabel], b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::with_capacity(set.len() * self.np());
        for l in set {
            out.extend(self.row_iv(self.dist_grad_iv(l.point as usize, b)?.1)?);
        }
        Ok(Some(out))
    }

    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        Ok(Some(self.r_of(b)?))
    }

    fn data_scale(&self) -> Result<f64, CertError> {
        Ok(self.data_scale)
    }

    fn constraint_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let r = self.r_of(b)?;
        let mut out = Vec::with_capacity(self.n_pts());
        for i in 0..self.n_pts() {
            out.push(self.dist_iv(i, b)?.sub(r)?);
        }
        Ok(Some(out))
    }

    fn objective_gradient_enclosure(&self, _b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut g = Vec::with_capacity(self.np());
        for k in 0..self.np() {
            g.push(ip(if k + 1 == self.np() { 1.0 } else { 0.0 })?);
        }
        Ok(Some(g))
    }

    fn constraint_gradient_enclosure(
        &self,
        set: &[RowLabel],
        b: &[Interval],
    ) -> Result<Option<Vec<Interval>>, CertError> {
        self.jacobian_enclosure(set, b)
    }
}

impl LinearisableProblem for MzLine3Problem<'_> {
    fn scales(&self) -> Vec<f64> {
        if self.dir {
            vec![1.0, 1.0, self.len_scale, self.len_scale, R_SCALE * self.len_scale]
        } else {
            vec![self.len_scale, self.len_scale, R_SCALE * self.len_scale]
        }
    }

    fn local_problem(&self, p: &[f64]) -> Result<(ConicProblem, f64), CertError> {
        let (pp, d, r) = self.axis(p)?;
        let m = self.n_pts();
        let np = self.np();
        let mut cols: Vec<Vec<f64>> = vec![Vec::with_capacity(m); np];
        let mut rhs = Vec::with_capacity(m);
        for i in 0..m {
            let (rho, g) = self.dist_subgrad(i, pp, d)?;
            let row = self.row(g);
            for j in 0..np {
                cols[j].push(row[j]);
            }
            rhs.push(r - rho);
        }
        let mut colptr = Vec::with_capacity(np + 1);
        let mut rowval = Vec::with_capacity(m * np);
        let mut nzval = Vec::with_capacity(m * np);
        colptr.push(0);
        for col in &cols {
            for (i, &x) in col.iter().enumerate() {
                if x != 0.0 {
                    rowval.push(i);
                    nzval.push(x);
                }
            }
            colptr.push(rowval.len());
        }
        let a = Csc::new(m, np, colptr, rowval, nzval);
        let q = self.objective_gradient(p)?;
        let prob =
            ConicProblemBuilder::new(q, a, rhs, vec![Cone::Nonneg(m)]).build().map_err(|_| cb("local problem"))?;
        Ok((prob, r))
    }

    fn objective(&self, p: &[f64]) -> Result<f64, CertError> {
        let (hi, _) = self.extremes(p)?;
        let r = self.full(p)?[4];
        let viol = total_max(0.0, hi - r).map_err(num)?;
        fid_num::check_finite("fit.mz_line3.merit", r + PENALTY * viol).map_err(num)
    }
}
