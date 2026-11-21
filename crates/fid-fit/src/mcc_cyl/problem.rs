//! The linearisable minimum-circumscribed-cylinder problem handed to `fid_solv::seq` and
//! certified by `fid_solv::cert` (module documentation of [`super`], rules 4 and 5).
//!
//! Parameters relative to a fixed frame `(P0, A, E1, E2)` in the centred, scaled units of
//! `contract::Centred`: free direction `(a, b, u, v, R)`, fixed direction `(u, v, R)`. The axis
//! direction is `D = A + a E1 + b E2` (not normalised), the axis point `P = P0 + u E1 + v E2`.
//! For point `q_i`, `w = q_i - P` and the distance to the axis is
//! `rho_i = |w x D| / |D|`. Row `(i, 0)` is `g_i = rho_i - R <= 0`; the objective is `R`.
//! The gradient of `rho_i` is, with `t = (w . D) / |D|^2`, `perp = w - t D` and
//! `g1 = (perp . E1) / rho`, `g2 = (perp . E2) / rho`:
//! `[-t g1, -t g2, -g1, -g2]` in `(a, b, u, v)`; the `R` column of every row is `-1`.

// Indexed loops over the parameter columns mirror the formulas term by term.
#![allow(clippy::needless_range_loop)]

use fid_num::f::{total_max, total_min};
use fid_num::la::Mat;
use fid_num::red;
use fid_solv::cert::recomp::{ActiveSetProblem, Interval};
use fid_solv::cert::{CertError, RowLabel};
use fid_solv::conic::{Cone, ConicProblem, ConicProblemBuilder, Csc};
use fid_solv::seq::LinearisableProblem;

use crate::contract::V3;

/// Step-bound scale of `R` relative to `len_scale`: large, so the radius can always absorb the
/// violation of an infeasible iterate whatever the trust radius (otherwise a small trust region
/// at a slightly infeasible point makes the LP infeasible); the objective bounds `dR` below
/// through the rows, so the LP stays bounded.
pub const R_SCALE: f64 = 1_048_576.0;

/// Exact-penalty factor of the merit (`objective`): above the multiplier sum, which is 1 for
/// the minimum circumscribed cylinder (stationarity in `R`).
pub const PENALTY: f64 = 2.0;

fn cb(reason: &'static str) -> CertError {
    CertError::Callback { reason }
}

fn num(e: fid_num::NumError) -> CertError {
    CertError::Numeric(e)
}

fn ip(x: f64) -> Result<Interval, CertError> {
    Interval::point(x)
}

/// A frame in scaled coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    pub p0: V3,
    pub a: V3,
    pub e1: V3,
    pub e2: V3,
}

/// The problem: scaled points, the frame, which parameters are free and the step scales.
#[derive(Debug, Clone)]
pub(crate) struct Mcc<'a> {
    pub q: &'a [f64],
    pub fr: Frame,
    /// `a`, `b` free.
    pub dir: bool,
    /// The step-bound scale of `u`, `v` and `R` (scaled units).
    pub len_scale: f64,
    /// Largest absolute scaled coordinate (KKT data scale).
    pub data_scale: f64,
    /// The start radius `R0` (scaled), used by `start_from_set`.
    pub r0: f64,
}

impl Mcc<'_> {
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

    fn point(&self, i: usize) -> Result<[f64; 3], CertError> {
        let s = self.q.get(3 * i..3 * i + 3).ok_or(cb("point index"))?;
        Ok([s[0], s[1], s[2]])
    }

    /// `(a, b, u, v, R)` from `p`.
    fn full(&self, p: &[f64]) -> Result<[f64; 5], CertError> {
        match (self.dir, p) {
            (true, [a, b, u, v, r]) => Ok([*a, *b, *u, *v, *r]),
            (false, [u, v, r]) => Ok([0.0, 0.0, *u, *v, *r]),
            _ => Err(cb("parameter count")),
        }
    }

    /// `(P, D)` of `p`.
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

    /// `rho_i` and its gradient in the free axis parameters (without the `R` column).
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

    /// The row of the constraint gradient of point `i` (free parameters, `R` column `-1`).
    fn row(&self, g: [f64; 4]) -> Vec<f64> {
        if self.dir {
            vec![g[0], g[1], g[2], g[3], -1.0]
        } else {
            vec![g[2], g[3], -1.0]
        }
    }

    /// Interval `(rho, grad)` of point `i` over the box `b` (free parameters).
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
        // rho = |w x D| / |D| (no cancellation-prone subtraction of t D).
        let c = [
            w[1].mul(d[2])?.sub(w[2].mul(d[1])?)?,
            w[2].mul(d[0])?.sub(w[0].mul(d[2])?)?,
            w[0].mul(d[1])?.sub(w[1].mul(d[0])?)?,
        ];
        let dd = dot(&d, &d)?;
        let rho = dot(&c, &c)?.sqrt()?.div(dd.sqrt()?)?;
        let t = dot(&w, &d)?.div(dd)?;
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

    /// The largest `rho_i` and the smallest, at `p` (scaled).
    pub(crate) fn extremes(&self, p: &[f64]) -> Result<(f64, f64), CertError> {
        let (pp, d, _) = self.axis(p)?;
        let mut hi = 0.0_f64;
        let mut lo = f64::INFINITY;
        for i in 0..self.n_pts() {
            let (rho, _) = self.dist_grad(i, pp, d)?;
            hi = total_max(hi, rho).map_err(num)?;
            lo = total_min(lo, rho).map_err(num)?;
        }
        Ok((hi, lo))
    }

    /// Every `rho_i` at `p`.
    pub(crate) fn distances(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        let (pp, d, _) = self.axis(p)?;
        let mut out = Vec::with_capacity(self.n_pts());
        for i in 0..self.n_pts() {
            out.push(self.dist_grad(i, pp, d)?.0);
        }
        Ok(out)
    }

    /// The axis point (foot of the perpendicular from the origin, the centroid) and the unit
    /// direction of `p`.
    pub(crate) fn line(&self, p: &[f64]) -> Result<(V3, V3), CertError> {
        let (pp, d, _) = self.axis(p)?;
        let n = red::norm2(&d).map_err(num)?;
        let u = [d[0] / n, d[1] / n, d[2] / n];
        let s = red::dot(&pp, &u).map_err(num)?;
        Ok(([pp[0] - s * u[0], pp[1] - s * u[1], pp[2] - s * u[2]], u))
    }
}

/// `start_from_set`'s outcome: the solve's parameters, the frame start `x0` when the solve
/// stopped without converging, a numeric error passed through.
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
                Stage::Start => "fit.mcc_cyl.start.start",
                Stage::Residual => "fit.mcc_cyl.start.residual",
                Stage::Jacobian => "fit.mcc_cyl.start.jacobian",
                Stage::Cost => "fit.mcc_cyl.start.cost",
                Stage::Step => "fit.mcc_cyl.start.step",
            };
            Err(CertError::Numeric(fid_num::NumError::Infinite { op, index }))
        }
        Err(_) => Ok(x0),
    }
}

/// The set's equations as a least-squares system for `start_from_set`.
struct SetSystem<'a, 'b> {
    mcc: &'a Mcc<'b>,
    set: &'a [RowLabel],
}

impl fid_solv::nlls::NllsProblem for SetSystem<'_, '_> {
    fn n_params(&self) -> usize {
        self.mcc.np()
    }

    fn n_resid(&self) -> usize {
        self.set.len()
    }

    fn eval(&self, p: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), fid_num::NumError> {
        let dom = |detail| fid_num::NumError::Domain { op: "fit.mcc_cyl.start", detail };
        let (pp, d, rad) = self.mcc.axis(p).map_err(|_| dom("axis"))?;
        let np = self.mcc.np();
        for (i, l) in self.set.iter().enumerate() {
            let (rho, g) = self.mcc.dist_grad(l.point as usize, pp, d).map_err(|_| dom("distance"))?;
            *r.get_mut(i).ok_or(dom("resid"))? = rho - rad;
            let row = self.mcc.row(g);
            j.get_mut(i * np..(i + 1) * np).ok_or(dom("jac"))?.copy_from_slice(&row);
        }
        Ok(())
    }
}

impl ActiveSetProblem for Mcc<'_> {
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

    /// The cylinder through the set's points nearest the frame axis: Levenberg-Marquardt
    /// (`fid_solv::nlls`, default configuration) on the set's equations `rho_k - R = 0` from the
    /// frame axis with the radius `R0`. It reads only the set and the problem data (the frame and
    /// `R0` come from the data), never a solver iterate. When the solve stops without
    /// converging (iteration limit, rank deficiency, ill conditioning, configuration) the frame
    /// start is used; a numeric error (an `eval` error, a non-finite value, an allocation
    /// failure) passes through.
    fn start_from_set(&self, set: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        let x0 = if self.dir { vec![0.0, 0.0, 0.0, 0.0, self.r0] } else { vec![0.0, 0.0, self.r0] };
        let sys = SetSystem { mcc: self, set };
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
            out.push(self.dist_grad(i, pp, d)?.0 - r);
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
            out.push(self.dist_grad_iv(l.point as usize, &b)?.0.sub(r)?);
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
            out.push(self.dist_grad_iv(i, b)?.0.sub(r)?);
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

impl LinearisableProblem for Mcc<'_> {
    fn scales(&self) -> Vec<f64> {
        if self.dir {
            vec![1.0, 1.0, self.len_scale, self.len_scale, R_SCALE * self.len_scale]
        } else {
            vec![self.len_scale, self.len_scale, R_SCALE * self.len_scale]
        }
    }

    /// Minimise `dR` subject to `grad g_i . dp <= -g_i` for every point (all rows of the
    /// nonnegative cone), `m0 = R`.
    fn local_problem(&self, p: &[f64]) -> Result<(ConicProblem, f64), CertError> {
        let (pp, d, r) = self.axis(p)?;
        let m = self.n_pts();
        let np = self.np();
        let mut cols: Vec<Vec<f64>> = vec![Vec::with_capacity(m); np];
        let mut rhs = Vec::with_capacity(m);
        for i in 0..m {
            let (rho, g) = self.dist_grad(i, pp, d)?;
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

    /// The merit `R + PENALTY * max(0, max_i (rho_i - R))`.
    fn objective(&self, p: &[f64]) -> Result<f64, CertError> {
        let (hi, _) = self.extremes(p)?;
        let r = self.full(p)?[4];
        let viol = total_max(0.0, hi - r).map_err(num)?;
        fid_num::check_finite("fit.mcc_cyl.merit", r + PENALTY * viol).map_err(num)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use fid_solv::nlls::{Diagnostics, SolverError, Stage};

    fn mcc(q: &[f64]) -> Mcc<'_> {
        let fr = Frame { p0: [0.0; 3], a: [0.0, 0.0, 1.0], e1: [1.0, 0.0, 0.0], e2: [0.0, 1.0, 0.0] };
        Mcc { q, fr, dir: true, len_scale: 1.0, data_scale: 1.0, r0: 1.0 }
    }

    /// Review of #374: `start_from_set` passes the solve's numeric errors through (here the
    /// set's point 0 lies on the frame axis, so `eval` fails) instead of keeping the frame start.
    #[test]
    fn start_from_set_passes_numeric_errors() {
        let q = [0.0, 0.0, 0.0, 1.0, 0.0, 0.1, 0.0, 1.0, 0.2, -1.0, 0.0, 0.3, 0.0, -1.0, 0.4];
        let p = mcc(&q);
        let set: Vec<RowLabel> = (0..5).map(|point| RowLabel { point, side: 0 }).collect();
        assert!(matches!(p.start_from_set(&set), Err(CertError::Numeric(_))));
        // Without the point on the axis the start is the solve's.
        let q2 = [0.5, 0.0, 0.0, 1.0, 0.0, 0.1, 0.0, 1.0, 0.2, -1.0, 0.0, 0.3, 0.0, -1.0, 0.4];
        assert!(mcc(&q2).start_from_set(&set).is_ok());
    }

    /// `start_or_error`: numeric and non-finite errors pass through; the solver's other stops
    /// keep the frame start.
    #[test]
    fn start_or_error_cases() {
        let last = Diagnostics::default();
        let x0 = vec![0.0, 0.0, 1.0];
        let numeric = SolverError::Numeric { error: fid_num::NumError::Domain { op: "t", detail: "d" }, last };
        let non_finite = SolverError::NonFinite { stage: Stage::Jacobian, index: Some(2), last };
        assert!(matches!(start_or_error(Err(numeric), x0.clone()), Err(CertError::Numeric(_))));
        assert!(matches!(
            start_or_error(Err(non_finite), x0.clone()),
            Err(CertError::Numeric(fid_num::NumError::Infinite { index: Some(2), .. }))
        ));
        for e in [SolverError::IterationLimit { last }, SolverError::RankDeficient { rank: 1, cond: None, last }] {
            assert_eq!(start_or_error(Err(e), x0.clone()).unwrap(), x0);
        }
    }
}
