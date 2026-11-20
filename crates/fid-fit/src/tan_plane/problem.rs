//! The linearisable tangent-plane problem handed to `fid_solv::seq` and certified by
//! `fid_solv::cert` (module documentation of [`super`], "Formulation").
//!
//! Local frame: `u`, `v` in the least-squares plane, `w` along the outward least-squares normal,
//! all in the centred, scaled units of `contract::Centred`, with `w` further divided by the
//! power of two `k` (the *form scale*, about the range of `w`), so that the form and the slopes
//! are of order 1 whatever the plate's flatness. In these units the tangent plane is
//! `w = c + a u + b v`; in the centred units it is `k w = k c + k a u + k b v`, its outward unit
//! normal is `(-k a, -k b, 1) / s` with `s = sqrt(1 + k^2 (a^2 + b^2))`, and the depth of point
//! `i` below it is `k (c + a u_i + b v_i - w_i) / s`. Multiplying or dividing by `k` is exact.
//!
//! Every constraint is **linear** in the parameters, so the active-set equations are linear and
//! their coefficients are the data themselves (the recomputation and its error bound are exact);
//! only the objective carries the factor `1 / s`, so it is nonlinear.

// Indexed loops over the parallel `u`, `v`, `w` columns mirror the pinned formulas term by term.
#![allow(clippy::needless_range_loop)]

use fid_num::f::{sqrt, total_max};
use fid_num::la::Mat;
use fid_num::red;
use fid_solv::cert::recomp::{ActiveSetProblem, Interval};
use fid_solv::cert::{CertError, RowLabel};
use fid_solv::conic::{Cone, ConicProblem, ConicProblemBuilder, Csc};
use fid_solv::seq::LinearisableProblem;

use crate::contract::FitError;

/// Step-bound scale of the two slope parameters (`|da| <= radius * SLOPE_SCALE`), in the
/// form-scaled units (slopes of order 1).
pub const SLOPE_SCALE: f64 = 1.0;
/// Step-bound scale of the offsets (form-scaled units: the range of `w` is in `[1, 2)`).
pub const OFFSET_SCALE: f64 = 1.0;

/// Which objective the problem carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Form {
    /// `p = (a, b, c, e)`: tangent plane `w = c + a u + b v`, far plane `w = e + a u + b v`;
    /// rows `(i, 0)`: `w_i - c - a u_i - b v_i <= 0`, `(i, 1)`: `e + a u_i + b v_i - w_i <= 0`;
    /// value `(c - e) / s` (the largest depth over `k`).
    Minimax,
    /// `p = (a, b, c)`; rows `(i, 0)` only; value `(N c + a U + b V - W) / s` with `U, V, W`
    /// the `red::sum` of the `u, v, w` columns (the sum of the depths over `k`).
    L1,
}

fn cb(reason: &'static str) -> CertError {
    CertError::Callback { reason }
}

fn num(e: fid_num::NumError) -> CertError {
    CertError::Numeric(e)
}

fn ip(x: f64) -> Result<Interval, CertError> {
    Interval::point(x)
}

/// The problem data.
#[derive(Debug, Clone)]
pub(crate) struct PlaneProblem {
    form: Form,
    u: Vec<f64>,
    v: Vec<f64>,
    w: Vec<f64>,
    su: f64,
    sv: f64,
    sw: f64,
    nf: f64,
    k2: f64,
    data_scale: f64,
    a_csc: Csc,
    labels: Vec<RowLabel>,
}

fn alloc<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| {
        crate::contract::FitError::Num(fid_num::NumError::Domain { op: "fit.tan_plane", detail: "allocation_failed" })
    })?;
    Ok(v)
}

impl PlaneProblem {
    /// Builds the problem from the local columns (equal lengths, at least 1, finite; `w`
    /// already divided by the form scale `k`, a power of two).
    pub(crate) fn new(form: Form, u: Vec<f64>, v: Vec<f64>, w: Vec<f64>, k: f64) -> Result<PlaneProblem, FitError> {
        let n = u.len();
        if n == 0 || v.len() != n || w.len() != n || n > u32::MAX as usize {
            return Err(FitError::TooFewPoints { n, need: 1 });
        }
        let su = red::sum(&u)?;
        let sv = red::sum(&v)?;
        let sw = red::sum(&w)?;
        let mut data_scale = 0.0_f64;
        for i in 0..n {
            data_scale = total_max(data_scale, u[i].abs())?;
            data_scale = total_max(data_scale, v[i].abs())?;
            data_scale = total_max(data_scale, w[i].abs())?;
        }
        let sides: &[u8] = match form {
            Form::Minimax => &[0, 1],
            Form::L1 => &[0],
        };
        let m = n * sides.len();
        let mut labels = alloc::<RowLabel>(m)?;
        for i in 0..n {
            for &side in sides {
                // n <= u32::MAX was checked above.
                labels.push(RowLabel { point: i as u32, side });
            }
        }
        let np = n_params_of(form);
        // Column-major constant constraint matrix, exact zeros dropped.
        let mut colptr = alloc::<usize>(np + 1)?;
        let mut rowval = alloc::<usize>(m * np)?;
        let mut nzval = alloc::<f64>(m * np)?;
        colptr.push(0);
        for j in 0..np {
            for (r, l) in labels.iter().enumerate() {
                let x = coef(l.side, j, u[l.point as usize], v[l.point as usize]);
                if x != 0.0 {
                    rowval.push(r);
                    nzval.push(x);
                }
            }
            colptr.push(rowval.len());
        }
        let a_csc = Csc::new(m, np, colptr, rowval, nzval);
        let k2 = k * k;
        if !(k2.is_normal() && k2 > 0.0) {
            return Err(FitError::Num(fid_num::NumError::Domain { op: "fit.tan_plane", detail: "form_scale" }));
        }
        Ok(PlaneProblem { form, u, v, w, su, sv, sw, nf: n as f64, k2, data_scale, a_csc, labels })
    }

    /// Number of points.
    pub(crate) fn len(&self) -> usize {
        self.u.len()
    }

    pub(crate) fn columns(&self) -> (&[f64], &[f64], &[f64]) {
        (&self.u, &self.v, &self.w)
    }

    /// The start lifted onto the data: for slopes `(a, b)`, `c = max_i (w_i - a u_i - b v_i)`
    /// (and `e` = the minimum for the minimax form), each term `(w_i - a u_i) - b v_i`.
    pub(crate) fn lift(&self, a: f64, b: f64) -> Result<Vec<f64>, FitError> {
        let mut hi = f64::NEG_INFINITY;
        let mut lo = f64::INFINITY;
        for i in 0..self.len() {
            let t = (self.w[i] - a * self.u[i]) - b * self.v[i];
            if !t.is_finite() {
                return Err(FitError::Num(fid_num::NumError::Infinite { op: "fit.tan_plane.start", index: Some(i) }));
            }
            hi = total_max(hi, t)?;
            lo = fid_num::f::total_min(lo, t)?;
        }
        let mut p = alloc::<f64>(n_params_of(self.form))?;
        p.extend_from_slice(&[a, b, hi]);
        if self.form == Form::Minimax {
            p.push(lo);
        }
        Ok(p)
    }

    /// `s = sqrt(1 + k^2 (a^2 + b^2))`.
    pub(crate) fn slope_norm(&self, a: f64, b: f64) -> Result<f64, fid_num::NumError> {
        let s = sqrt(1.0 + self.k2 * (a * a + b * b))?;
        fid_num::check_finite("fit.tan_plane.slope", s)
    }

    /// Depth of every point below the tangent plane of `p` over the form scale:
    /// `(((c + a u_i) + b v_i) - w_i) / s` (times `k` it is the depth in centred units).
    pub(crate) fn depths(&self, p: &[f64]) -> Result<Vec<f64>, FitError> {
        let (a, b, c) = (p[0], p[1], p[2]);
        let s = self.slope_norm(a, b).map_err(FitError::Num)?;
        let mut out = alloc::<f64>(self.len())?;
        for i in 0..self.len() {
            out.push((((c + a * self.u[i]) + b * self.v[i]) - self.w[i]) / s);
        }
        fid_num::check_finite_slice("fit.tan_plane.depths", &out)?;
        Ok(out)
    }

    fn check_p(&self, p: &[f64]) -> Result<(), CertError> {
        if p.len() != n_params_of(self.form) {
            return Err(CertError::Shape { field: "p", expected: n_params_of(self.form), got: p.len() });
        }
        Ok(())
    }

    fn row(&self, l: RowLabel) -> Result<(f64, f64, f64), CertError> {
        let i = l.point as usize;
        if i >= self.len() || l.side > 1 || (self.form == Form::L1 && l.side != 0) {
            return Err(cb("unknown row label"));
        }
        Ok((self.u[i], self.v[i], self.w[i]))
    }

    /// `g` of one row at `p` (fixed operation order, see [`Form`]).
    fn g(&self, l: RowLabel, p: &[f64]) -> Result<f64, CertError> {
        let (u, v, w) = self.row(l)?;
        let (a, b, c) = (p[0], p[1], p[2]);
        Ok(if l.side == 0 { ((w - c) - a * u) - b * v } else { ((p[3] + a * u) + b * v) - w })
    }

    fn g_iv(&self, l: RowLabel, b: &[Interval]) -> Result<Interval, CertError> {
        let (u, v, w) = self.row(l)?;
        let (ia, ib, ic) = (b[0], b[1], b[2]);
        if l.side == 0 {
            ip(w)?.sub(ic)?.sub(ia.mul(ip(u)?)?)?.sub(ib.mul(ip(v)?)?)
        } else {
            b[3].add(ia.mul(ip(u)?)?)?.add(ib.mul(ip(v)?)?)?.sub(ip(w)?)
        }
    }

    fn grad_row(&self, l: RowLabel) -> Result<Vec<f64>, CertError> {
        let (u, v, _) = self.row(l)?;
        let np = n_params_of(self.form);
        Ok((0..np).map(|j| coef(l.side, j, u, v)).collect())
    }

    fn penalty(&self) -> f64 {
        match self.form {
            // Multiplier sum at a KKT point: 2 / s <= 2.
            Form::Minimax => 4.0,
            // Multiplier sum N / s <= N.
            Form::L1 => 2.0 * self.nf,
        }
    }
}

const fn n_params_of(form: Form) -> usize {
    match form {
        Form::Minimax => 4,
        Form::L1 => 3,
    }
}

/// Coefficient of parameter `j` in the row of `side` for a point with `(u, v)`: side 0
/// `(-u, -v, -1, 0)`, side 1 `(u, v, 0, 1)`.
fn coef(side: u8, j: usize, u: f64, v: f64) -> f64 {
    match (side, j) {
        (0, 0) => -u,
        (0, 1) => -v,
        (0, 2) => -1.0,
        (0, _) => 0.0,
        (_, 0) => u,
        (_, 1) => v,
        (_, 3) => 1.0,
        _ => 0.0,
    }
}

fn s2_iv(k2: f64, a: Interval, b: Interval) -> Result<Interval, CertError> {
    ip(1.0)?.add(ip(k2)?.mul(a.sqr()?.add(b.sqr()?)?)?)
}

impl ActiveSetProblem for PlaneProblem {
    fn n_params(&self) -> usize {
        n_params_of(self.form)
    }

    fn expected_active(&self) -> usize {
        n_params_of(self.form)
    }

    fn row_labels(&self) -> Vec<RowLabel> {
        self.labels.clone()
    }

    fn is_linear(&self) -> bool {
        true
    }

    fn start_from_set(&self, _set: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        Err(cb("linear problem has no Gauss-Newton start"))
    }

    fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
        self.check_p(p)?;
        let mut r = Vec::new();
        let mut rows = Vec::new();
        for &l in set {
            r.push(self.g(l, p)?);
            rows.push(self.grad_row(l)?);
        }
        Ok((r, Mat::from_rows(&rows).map_err(num)?))
    }

    fn value(&self, p: &[f64]) -> Result<f64, CertError> {
        self.check_p(p)?;
        let (a, b, c) = (p[0], p[1], p[2]);
        let s = self.slope_norm(a, b).map_err(num)?;
        let v = match self.form {
            Form::Minimax => (c - p[3]) / s,
            Form::L1 => (((self.nf * c + a * self.su) + b * self.sv) - self.sw) / s,
        };
        fid_num::check_finite("fit.tan_plane.value", v).map_err(num)
    }

    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        self.check_p(p)?;
        let mut out = Vec::new();
        out.try_reserve_exact(self.labels.len()).map_err(|_| cb("allocation failed"))?;
        for &l in &self.labels {
            out.push(self.g(l, p)?);
        }
        Ok(out)
    }

    fn objective_gradient(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        self.check_p(p)?;
        let (a, b, c) = (p[0], p[1], p[2]);
        let s2 = 1.0 + self.k2 * (a * a + b * b);
        let s = sqrt(s2).map_err(num)?;
        let s3 = s2 * s;
        let (ka, kb) = (self.k2 * a, self.k2 * b);
        let g = match self.form {
            Form::Minimax => {
                let wd = c - p[3];
                vec![-(wd * ka) / s3, -(wd * kb) / s3, 1.0 / s, -1.0 / s]
            }
            Form::L1 => {
                let nm = ((self.nf * c + a * self.su) + b * self.sv) - self.sw;
                vec![self.su / s - (nm * ka) / s3, self.sv / s - (nm * kb) / s3, self.nf / s]
            }
        };
        fid_num::check_finite_slice("fit.tan_plane.gradient", &g).map_err(num)?;
        Ok(g)
    }

    fn constraint_gradients(&self, set: &[RowLabel], p: &[f64]) -> Result<Mat, CertError> {
        self.check_p(p)?;
        let mut rows = Vec::new();
        for &l in set {
            rows.push(self.grad_row(l)?);
        }
        Mat::from_rows(&rows).map_err(num)
    }

    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        if b.len() != n_params_of(self.form) {
            return Err(CertError::Shape { field: "box", expected: n_params_of(self.form), got: b.len() });
        }
        let s = s2_iv(self.k2, b[0], b[1])?.sqrt()?;
        let nm = match self.form {
            Form::Minimax => b[2].sub(b[3])?,
            Form::L1 => {
                ip(self.nf)?.mul(b[2])?.add(b[0].mul(ip(self.su)?)?)?.add(b[1].mul(ip(self.sv)?)?)?.sub(ip(self.sw)?)?
            }
        };
        Ok(Some(nm.div(s)?))
    }

    fn data_scale(&self) -> Result<f64, CertError> {
        if self.data_scale.is_finite() && self.data_scale > 0.0 {
            Ok(self.data_scale)
        } else {
            Err(CertError::InvalidInput { field: "data_scale" })
        }
    }

    fn constraint_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        if b.len() != n_params_of(self.form) {
            return Err(CertError::Shape { field: "box", expected: n_params_of(self.form), got: b.len() });
        }
        let mut out = Vec::new();
        out.try_reserve_exact(self.labels.len()).map_err(|_| cb("allocation failed"))?;
        for &l in &self.labels {
            out.push(self.g_iv(l, b)?);
        }
        Ok(Some(out))
    }

    fn objective_gradient_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        if b.len() != n_params_of(self.form) {
            return Err(CertError::Shape { field: "box", expected: n_params_of(self.form), got: b.len() });
        }
        let (ia, ib, ic) = (b[0], b[1], b[2]);
        let s2 = s2_iv(self.k2, ia, ib)?;
        let s = s2.sqrt()?;
        let s3 = s2.mul(s)?;
        let one = ip(1.0)?;
        let k2 = ip(self.k2)?;
        let (ka, kb) = (k2.mul(ia)?, k2.mul(ib)?);
        Ok(Some(match self.form {
            Form::Minimax => {
                let wd = ic.sub(b[3])?;
                vec![wd.mul(ka)?.div(s3)?.neg(), wd.mul(kb)?.div(s3)?.neg(), one.div(s)?, one.div(s)?.neg()]
            }
            Form::L1 => {
                let nm =
                    ip(self.nf)?.mul(ic)?.add(ia.mul(ip(self.su)?)?)?.add(ib.mul(ip(self.sv)?)?)?.sub(ip(self.sw)?)?;
                vec![
                    ip(self.su)?.div(s)?.sub(nm.mul(ka)?.div(s3)?)?,
                    ip(self.sv)?.div(s)?.sub(nm.mul(kb)?.div(s3)?)?,
                    ip(self.nf)?.div(s)?,
                ]
            }
        }))
    }

    fn constraint_gradient_enclosure(
        &self,
        set: &[RowLabel],
        b: &[Interval],
    ) -> Result<Option<Vec<Interval>>, CertError> {
        if b.len() != n_params_of(self.form) {
            return Err(CertError::Shape { field: "box", expected: n_params_of(self.form), got: b.len() });
        }
        let mut out = Vec::new();
        for &l in set {
            for x in self.grad_row(l)? {
                out.push(ip(x)?);
            }
        }
        Ok(Some(out))
    }
}

impl LinearisableProblem for PlaneProblem {
    fn scales(&self) -> Vec<f64> {
        let mut s = vec![SLOPE_SCALE, SLOPE_SCALE, OFFSET_SCALE];
        if self.form == Form::Minimax {
            s.push(OFFSET_SCALE);
        }
        s
    }

    /// The exact linearisation: the constraints are linear, so `G dp + s = -g(p)` with the
    /// constant `G`; the model is `value(p) + grad f(p)' dp`.
    fn local_problem(&self, p: &[f64]) -> Result<(ConicProblem, f64), CertError> {
        let q = self.objective_gradient(p)?;
        let g = self.constraint_values(p)?;
        let rhs: Vec<f64> = g.iter().map(|x| -x).collect();
        let prob = ConicProblemBuilder::new(q, self.a_csc.clone(), rhs, vec![Cone::Nonneg(self.labels.len())])
            .build()
            .map_err(|_| cb("local problem"))?;
        Ok((prob, self.value(p)?))
    }

    /// The merit: `value(p) + M max(0, max_i g_i(p))` with `M` above the multiplier sum
    /// (4 for minimax, `2 N` for L1).
    fn objective(&self, p: &[f64]) -> Result<f64, CertError> {
        let mut viol = 0.0_f64;
        for &l in &self.labels {
            viol = total_max(viol, self.g(l, p)?).map_err(num)?;
        }
        let f = self.value(p)? + self.penalty() * viol;
        fid_num::check_finite("fit.tan_plane.merit", f).map_err(num)
    }
}
