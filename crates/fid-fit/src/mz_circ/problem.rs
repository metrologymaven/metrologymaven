//! Linearisable minimum-zone 2D circle problem for `fid_solv::seq` and `fid_solv::cert`
//! (spec node `KRN.MZ.RND.CIRC`).
//!
//! Parameters in centred, power-of-two scaled projected plane coordinates `(u_i, v_i)`:
//! `p = (cx, cy, r_outer, r_inner)` (4 parameters).
//!
//! For each point `i`:
//! - `rho_i = sqrt((u_i - cx)^2 + (v_i - cy)^2)`
//! - Side 0 (outer circle): `g_{i, 0}(p) = rho_i - r_outer <= 0`
//! - Side 1 (inner circle): `g_{i, 1}(p) = r_inner - rho_i <= 0`
//!
//! Objective: exact linear zone width `value(p) = r_outer - r_inner`, gradient `[0, 0, 1, -1]`.
//! Expected active set size: 4 (generically 2 outer + 2 inner alternating contacts, or 3-1 / 1-3).
//! All interval enclosures use `Interval::sqr()` for exact outward-rounded Krawczyk certification.

#![allow(clippy::needless_range_loop)]

use fid_num::f::{total_max, total_min};
use fid_num::la::Mat;
use fid_num::red;
use fid_solv::cert::recomp::{ActiveSetProblem, Interval};
use fid_solv::cert::{CertError, RowLabel};
use fid_solv::conic::{Cone, ConicProblem, ConicProblemBuilder, Csc};
use fid_solv::seq::LinearisableProblem;

/// Step-bound scale factor for `r_outer` and `r_inner` so offsets can always absorb infeasibility.
pub const R_SCALE: f64 = 1_048_576.0;

/// Exact-penalty factor of the merit function (dominates multiplier sum `1 + 1 = 2`).
pub const PENALTY: f64 = 4.0;

const ON_CENTRE_REL: f64 = 1e-14;

fn cb(reason: &'static str) -> CertError {
    CertError::Callback { reason }
}

fn num(e: fid_num::NumError) -> CertError {
    CertError::Numeric(e)
}

fn ip(x: f64) -> Result<Interval, CertError> {
    Interval::point(x)
}

/// Minimum-zone 2D circle problem in centred, scaled coordinates.
#[derive(Debug, Clone)]
pub(crate) struct MzCircle2Problem {
    pub u: Vec<f64>,
    pub v: Vec<f64>,
    /// Least-squares starting centre `(cx0, cy0)` and radii `(r_outer0, r_inner0)` for `start_from_set`.
    pub start0: [f64; 4],
    /// Step-bound scale for `cx`, `cy`.
    pub len_scale: f64,
    /// KKT data scale.
    pub data_scale: f64,
    labels: Vec<RowLabel>,
}

impl MzCircle2Problem {
    pub(crate) fn new(
        u: Vec<f64>,
        v: Vec<f64>,
        start0: [f64; 4],
        len_scale: f64,
        data_scale: f64,
    ) -> Result<Self, fid_num::NumError> {
        let n = u.len();
        let mut labels = Vec::new();
        labels
            .try_reserve_exact(2 * n)
            .map_err(|_| fid_num::NumError::Domain { op: "fit.mz_circ", detail: "allocation_failed" })?;
        for i in 0..n {
            labels.push(RowLabel { point: i as u32, side: 0 });
            labels.push(RowLabel { point: i as u32, side: 1 });
        }
        Ok(Self { u, v, start0, len_scale, data_scale, labels })
    }

    pub(crate) fn n_pts(&self) -> usize {
        self.u.len()
    }

    fn check_p(p: &[f64]) -> Result<[f64; 4], CertError> {
        match p {
            [cx, cy, ro, ri] => Ok([*cx, *cy, *ro, *ri]),
            _ => Err(CertError::Shape { field: "p", expected: 4, got: p.len() }),
        }
    }

    fn dist_only(&self, i: usize, cx: f64, cy: f64) -> Result<f64, CertError> {
        let dx = self.u[i] - cx;
        let dy = self.v[i] - cy;
        let rho = red::norm2(&[dx, dy]).map_err(num)?;
        if rho.is_nan() {
            return Err(cb("non-finite radius"));
        }
        Ok(rho)
    }

    fn dist_grad(&self, i: usize, cx: f64, cy: f64) -> Result<(f64, [f64; 2]), CertError> {
        let dx = self.u[i] - cx;
        let dy = self.v[i] - cy;
        let rho = red::norm2(&[dx, dy]).map_err(num)?;
        if rho.is_nan() || rho <= 0.0 {
            return Err(cb("point at centre"));
        }
        Ok((rho, [-dx / rho, -dy / rho]))
    }

    fn dist_subgrad(&self, i: usize, cx: f64, cy: f64) -> Result<(f64, [f64; 2]), CertError> {
        let dx = self.u[i] - cx;
        let dy = self.v[i] - cy;
        let rho = red::norm2(&[dx, dy]).map_err(num)?;
        if rho.is_nan() {
            return Err(cb("non-finite radius"));
        }
        if rho <= ON_CENTRE_REL * self.len_scale {
            return Ok((rho, [0.0, 0.0]));
        }
        Ok((rho, [-dx / rho, -dy / rho]))
    }

    fn row_grad(side: u8, g: [f64; 2]) -> [f64; 4] {
        if side == 0 {
            [g[0], g[1], -1.0, 0.0]
        } else {
            [-g[0], -g[1], 0.0, 1.0]
        }
    }

    fn g_val(&self, l: RowLabel, x: [f64; 4]) -> Result<f64, CertError> {
        let i = l.point as usize;
        if i >= self.n_pts() || l.side > 1 {
            return Err(cb("unknown row label"));
        }
        let rho = self.dist_only(i, x[0], x[1])?;
        Ok(if l.side == 0 { rho - x[2] } else { x[3] - rho })
    }

    fn dist_iv(&self, i: usize, b: &[Interval]) -> Result<Interval, CertError> {
        let dx = ip(self.u[i])?.sub(b[0])?;
        let dy = ip(self.v[i])?.sub(b[1])?;
        dx.sqr()?.add(dy.sqr()?)?.sqrt()
    }

    fn dist_grad_iv(&self, i: usize, b: &[Interval]) -> Result<(Interval, [Interval; 2]), CertError> {
        let dx = ip(self.u[i])?.sub(b[0])?;
        let dy = ip(self.v[i])?.sub(b[1])?;
        let rho = dx.sqr()?.add(dy.sqr()?)?.sqrt()?;
        Ok((rho, [dx.div(rho)?.neg(), dy.div(rho)?.neg()]))
    }

    /// Lifts `(cx, cy)` onto the data: `(cx, cy, max_i rho_i, min_i rho_i)`.
    pub(crate) fn lift(&self, cx: f64, cy: f64) -> Result<Vec<f64>, CertError> {
        let mut hi = 0.0_f64;
        let mut lo = f64::INFINITY;
        for i in 0..self.n_pts() {
            let rho = self.dist_only(i, cx, cy)?;
            hi = total_max(hi, rho).map_err(num)?;
            lo = total_min(lo, rho).map_err(num)?;
        }
        Ok(vec![cx, cy, hi, lo])
    }

    /// All radial distances `rho_i` from `(cx, cy)` in scaled coordinates.
    pub(crate) fn radii(&self, cx: f64, cy: f64) -> Result<Vec<f64>, CertError> {
        let mut out = Vec::with_capacity(self.n_pts());
        for i in 0..self.n_pts() {
            out.push(self.dist_only(i, cx, cy)?);
        }
        Ok(out)
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
                Stage::Start => "fit.mz_circ.start.start",
                Stage::Residual => "fit.mz_circ.start.residual",
                Stage::Jacobian => "fit.mz_circ.start.jacobian",
                Stage::Cost => "fit.mz_circ.start.cost",
                Stage::Step => "fit.mz_circ.start.step",
            };
            Err(CertError::Numeric(fid_num::NumError::Infinite { op, index }))
        }
        Err(_) => Ok(x0),
    }
}

struct SetSystem<'a> {
    prob: &'a MzCircle2Problem,
    set: &'a [RowLabel],
}

impl fid_solv::nlls::NllsProblem for SetSystem<'_> {
    fn n_params(&self) -> usize {
        4
    }

    fn n_resid(&self) -> usize {
        self.set.len()
    }

    fn eval(&self, p: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), fid_num::NumError> {
        let dom = |detail| fid_num::NumError::Domain { op: "fit.mz_circ.start", detail };
        let x = MzCircle2Problem::check_p(p).map_err(|_| dom("params"))?;
        for (k, l) in self.set.iter().enumerate() {
            let (rho, g) = self.prob.dist_grad(l.point as usize, x[0], x[1]).map_err(|_| dom("distance"))?;
            let rv = if l.side == 0 { rho - x[2] } else { x[3] - rho };
            *r.get_mut(k).ok_or(dom("resid"))? = rv;
            let row = MzCircle2Problem::row_grad(l.side, g);
            j.get_mut(k * 4..(k + 1) * 4).ok_or(dom("jac"))?.copy_from_slice(&row);
        }
        Ok(())
    }
}

impl ActiveSetProblem for MzCircle2Problem {
    fn n_params(&self) -> usize {
        4
    }

    fn expected_active(&self) -> usize {
        4
    }

    fn row_labels(&self) -> Vec<RowLabel> {
        self.labels.clone()
    }

    fn is_linear(&self) -> bool {
        false
    }

    fn start_from_set(&self, set: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        let x0 = self.start0.to_vec();
        let sys = SetSystem { prob: self, set };
        start_or_error(fid_solv::nlls::solve(&sys, &x0, &fid_solv::nlls::NllsConfig::default()), x0)
    }

    fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
        let x = Self::check_p(p)?;
        let mut res = Vec::with_capacity(set.len());
        let mut rows = Vec::with_capacity(set.len());
        for &l in set {
            let (rho, g) = self.dist_grad(l.point as usize, x[0], x[1])?;
            res.push(if l.side == 0 { rho - x[2] } else { x[3] - rho });
            rows.push(Self::row_grad(l.side, g).to_vec());
        }
        Ok((res, Mat::from_rows(&rows).map_err(num)?))
    }

    fn value(&self, p: &[f64]) -> Result<f64, CertError> {
        let x = Self::check_p(p)?;
        fid_num::check_finite("fit.mz_circ.value", x[2] - x[3]).map_err(num)
    }

    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        let x = Self::check_p(p)?;
        let mut out = Vec::with_capacity(self.labels.len());
        for &l in &self.labels {
            out.push(self.g_val(l, x)?);
        }
        Ok(out)
    }

    fn objective_gradient(&self, _p: &[f64]) -> Result<Vec<f64>, CertError> {
        Ok(vec![0.0, 0.0, 1.0, -1.0])
    }

    fn constraint_gradients(&self, set: &[RowLabel], p: &[f64]) -> Result<Mat, CertError> {
        Ok(self.residuals_and_jacobian(set, p)?.1)
    }

    fn residual_enclosure(&self, set: &[RowLabel], p: &[f64]) -> Result<Option<Vec<Interval>>, CertError> {
        let x = Self::check_p(p)?;
        let b = [ip(x[0])?, ip(x[1])?, ip(x[2])?, ip(x[3])?];
        let mut out = Vec::with_capacity(set.len());
        for &l in set {
            let rho = self.dist_iv(l.point as usize, &b)?;
            out.push(if l.side == 0 { rho.sub(b[2])? } else { b[3].sub(rho)? });
        }
        Ok(Some(out))
    }

    fn jacobian_enclosure(&self, set: &[RowLabel], b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        if b.len() != 4 {
            return Err(CertError::Shape { field: "box", expected: 4, got: b.len() });
        }
        let z = ip(0.0)?;
        let p1 = ip(1.0)?;
        let m1 = ip(-1.0)?;
        let mut out = Vec::with_capacity(set.len() * 4);
        for &l in set {
            let (_, g) = self.dist_grad_iv(l.point as usize, b)?;
            if l.side == 0 {
                out.extend_from_slice(&[g[0], g[1], m1, z]);
            } else {
                out.extend_from_slice(&[g[0].neg(), g[1].neg(), z, p1]);
            }
        }
        Ok(Some(out))
    }

    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        if b.len() != 4 {
            return Err(CertError::Shape { field: "box", expected: 4, got: b.len() });
        }
        Ok(Some(b[2].sub(b[3])?))
    }

    fn data_scale(&self) -> Result<f64, CertError> {
        Ok(self.data_scale)
    }

    fn constraint_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        if b.len() != 4 {
            return Err(CertError::Shape { field: "box", expected: 4, got: b.len() });
        }
        let mut out = Vec::with_capacity(self.labels.len());
        for &l in &self.labels {
            let rho = self.dist_iv(l.point as usize, b)?;
            out.push(if l.side == 0 { rho.sub(b[2])? } else { b[3].sub(rho)? });
        }
        Ok(Some(out))
    }

    fn objective_gradient_enclosure(&self, _b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        Ok(Some(vec![ip(0.0)?, ip(0.0)?, ip(1.0)?, ip(-1.0)?]))
    }

    fn constraint_gradient_enclosure(
        &self,
        set: &[RowLabel],
        b: &[Interval],
    ) -> Result<Option<Vec<Interval>>, CertError> {
        self.jacobian_enclosure(set, b)
    }
}

impl LinearisableProblem for MzCircle2Problem {
    fn scales(&self) -> Vec<f64> {
        vec![self.len_scale, self.len_scale, R_SCALE * self.len_scale, R_SCALE * self.len_scale]
    }

    fn local_problem(&self, p: &[f64]) -> Result<(ConicProblem, f64), CertError> {
        let x = Self::check_p(p)?;
        let m = self.labels.len();
        let mut cols: [Vec<f64>; 4] =
            [Vec::with_capacity(m), Vec::with_capacity(m), Vec::with_capacity(m), Vec::with_capacity(m)];
        let mut rhs = Vec::with_capacity(m);
        for i in 0..self.n_pts() {
            let (rho, g) = self.dist_subgrad(i, x[0], x[1])?;
            // side 0: rho - r_outer <= 0
            let r0 = Self::row_grad(0, g);
            for j in 0..4 {
                cols[j].push(r0[j]);
            }
            rhs.push(x[2] - rho);
            // side 1: r_inner - rho <= 0
            let r1 = Self::row_grad(1, g);
            for j in 0..4 {
                cols[j].push(r1[j]);
            }
            rhs.push(rho - x[3]);
        }
        let mut colptr = Vec::with_capacity(5);
        let mut rowval = Vec::with_capacity(m * 3);
        let mut nzval = Vec::with_capacity(m * 3);
        colptr.push(0);
        for col in &cols {
            for (r, &val) in col.iter().enumerate() {
                if val != 0.0 {
                    rowval.push(r);
                    nzval.push(val);
                }
            }
            colptr.push(rowval.len());
        }
        let a = Csc::new(m, 4, colptr, rowval, nzval);
        let q = self.objective_gradient(p)?;
        let prob =
            ConicProblemBuilder::new(q, a, rhs, vec![Cone::Nonneg(m)]).build().map_err(|_| cb("local problem"))?;
        Ok((prob, x[2] - x[3]))
    }

    fn objective(&self, p: &[f64]) -> Result<f64, CertError> {
        let x = Self::check_p(p)?;
        let mut viol = 0.0_f64;
        for &l in &self.labels {
            viol = total_max(viol, self.g_val(l, x)?).map_err(num)?;
        }
        fid_num::check_finite("fit.mz_circ.merit", (x[2] - x[3]) + PENALTY * viol).map_err(num)
    }
}
