//! Linearisable minimum-zone 2D line and plane problems for `fid_solv::seq` and
//! `fid_solv::cert` (spec node `KRN.MZ.LIN.FLAT`).
//!
//! # Formulations
//!
//! Both problems work in a centred, power-of-two scaled local frame about the least-squares
//! solution, with the out-of-line/out-of-plane coordinate `w` further divided by the exact
//! power-of-two form scale `k` so slopes and offsets are O(1) regardless of flatness:
//!
//! - **`MzLine2Problem`** (3 parameters `p = (a, c, e)`):
//!   - Upper line `w = c + a u` (side 0: `w_i - c - a u_i <= 0`)
//!   - Lower line `w = e + a u` (side 1: `e + a u_i - w_i <= 0`)
//!   - Objective: orthogonal zone width over `k * scale`, `(c - e) / sqrt(1 + k^2 a^2)`
//!   - Expected active set size: 3 (generically 2-1 or 1-2 critical points).
//!
//! - **`MzPlaneProblem`** (4 parameters `p = (a, b, c, e)`):
//!   - Upper plane `w = c + a u + b v` (side 0: `w_i - c - a u_i - b v_i <= 0`)
//!   - Lower plane `w = e + a u + b v` (side 1: `e + a u_i + b v_i - w_i <= 0`)
//!   - Objective: orthogonal zone width over `k * scale`, `(c - e) / sqrt(1 + k^2 (a^2 + b^2))`
//!   - Expected active set size: 4 (generically 3-1, 1-3, or 2-2 critical points).
//!
//! Every constraint is exact linear in the parameters (`is_linear() == true`), while the
//! objective carries the nonlinear normal factor `1 / s`. Exact interval enclosures are
//! supplied for every callback required by `fid_solv::cert::certify`.

#![allow(clippy::needless_range_loop)]

use fid_num::f::{sqrt, total_max, total_min};
use fid_num::la::Mat;
use fid_num::NumError;
use fid_solv::cert::recomp::{ActiveSetProblem, Interval};
use fid_solv::cert::{CertError, RowLabel};
use fid_solv::conic::{Cone, ConicProblem, ConicProblemBuilder, Csc};
use fid_solv::seq::LinearisableProblem;

use crate::contract::FitError;

/// Step-bound scale of slope parameters in form-scaled units.
pub const SLOPE_SCALE: f64 = 1.0;
/// Step-bound scale of offset parameters in form-scaled units.
pub const OFFSET_SCALE: f64 = 1.0;
/// Exact-penalty weight for the merit function (`2 / s <= 2`, so 4 dominates every multiplier sum).
const MERIT_PENALTY: f64 = 4.0;

fn cb(reason: &'static str) -> CertError {
    CertError::Callback { reason }
}

fn num(e: NumError) -> CertError {
    CertError::Numeric(e)
}

fn ip(x: f64) -> Result<Interval, CertError> {
    Interval::point(x)
}

fn alloc<T>(op: &'static str, n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| FitError::Num(NumError::Domain { op, detail: "allocation_failed" }))?;
    Ok(v)
}

// ---------------------------------------------------------------------------
// 2D Line minimum-zone problem: p = (a, c, e)
// ---------------------------------------------------------------------------

/// Linearisable minimum-zone 2D line problem in form-scaled local coordinates `(u, w)`.
#[derive(Debug, Clone)]
pub(crate) struct MzLine2Problem {
    u: Vec<f64>,
    w: Vec<f64>,
    k2: f64,
    data_scale: f64,
    a_csc: Csc,
    labels: Vec<RowLabel>,
}

const OP_LINE2: &str = "fit.mz_line2";

fn line2_coef(side: u8, j: usize, u: f64) -> f64 {
    match (side, j) {
        (0, 0) => -u,
        (0, 1) => -1.0,
        (0, _) => 0.0,
        (_, 0) => u,
        (_, 2) => 1.0,
        _ => 0.0,
    }
}

impl MzLine2Problem {
    /// Builds the 2D minimum-zone problem from local columns `(u, w)` and exact power-of-two
    /// form scale `k`.
    pub(crate) fn new(u: Vec<f64>, w: Vec<f64>, k: f64) -> Result<Self, FitError> {
        let n = u.len();
        if n < 2 || w.len() != n || n > u32::MAX as usize {
            return Err(FitError::TooFewPoints { n, need: 2 });
        }
        let mut data_scale = 0.0_f64;
        for i in 0..n {
            data_scale = total_max(data_scale, u[i].abs())?;
            data_scale = total_max(data_scale, w[i].abs())?;
        }
        let m = n * 2;
        let mut labels = alloc::<RowLabel>(OP_LINE2, m)?;
        for i in 0..n {
            labels.push(RowLabel { point: i as u32, side: 0 });
            labels.push(RowLabel { point: i as u32, side: 1 });
        }
        let np = 3usize;
        let mut colptr = alloc::<usize>(OP_LINE2, np + 1)?;
        let mut rowval = alloc::<usize>(OP_LINE2, m * 2)?;
        let mut nzval = alloc::<f64>(OP_LINE2, m * 2)?;
        colptr.push(0);
        for j in 0..np {
            for (r, l) in labels.iter().enumerate() {
                let x = line2_coef(l.side, j, u[l.point as usize]);
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
            return Err(FitError::Num(NumError::Domain { op: OP_LINE2, detail: "form_scale" }));
        }
        Ok(Self { u, w, k2, data_scale, a_csc, labels })
    }

    pub(crate) fn len(&self) -> usize {
        self.u.len()
    }

    /// Lifts slope `a` onto the data: `c = max_i (w_i - a u_i)`, `e = min_i (w_i - a u_i)`.
    pub(crate) fn lift(&self, a: f64) -> Result<Vec<f64>, FitError> {
        let mut hi = f64::NEG_INFINITY;
        let mut lo = f64::INFINITY;
        for i in 0..self.len() {
            let t = self.w[i] - a * self.u[i];
            if !t.is_finite() {
                return Err(FitError::Num(NumError::Infinite { op: "fit.mz_line2.start", index: Some(i) }));
            }
            hi = total_max(hi, t)?;
            lo = total_min(lo, t)?;
        }
        let mut p = alloc::<f64>(OP_LINE2, 3)?;
        p.extend_from_slice(&[a, hi, lo]);
        Ok(p)
    }

    /// `s = sqrt(1 + k^2 a^2)`.
    pub(crate) fn slope_norm(&self, a: f64) -> Result<f64, NumError> {
        let s = sqrt(1.0 + self.k2 * (a * a))?;
        fid_num::check_finite("fit.mz_line2.slope", s)
    }

    /// Form-scaled signed residuals from the mid-zone line `(w_i - mid - a u_i) / s`,
    /// where `mid = 0.5 * (c + e)`.
    pub(crate) fn signed_residuals(&self, p: &[f64]) -> Result<Vec<f64>, FitError> {
        let (a, c, e) = (p[0], p[1], p[2]);
        let mid = 0.5 * (c + e);
        let s = self.slope_norm(a).map_err(FitError::Num)?;
        let mut out = alloc::<f64>(OP_LINE2, self.len())?;
        for i in 0..self.len() {
            let r = ((self.w[i] - mid) - a * self.u[i]) / s;
            out.push(r);
        }
        fid_num::check_finite_slice("fit.mz_line2.residuals", &out)?;
        Ok(out)
    }

    /// Largest orthogonal constraint violation in form-scaled units (`max(0, max_r g_r / s)`).
    pub(crate) fn max_scaled_violation(&self, p: &[f64]) -> Result<(f64, usize), FitError> {
        let s = self.slope_norm(p[0]).map_err(FitError::Num)?;
        let mut worst_v = 0.0_f64;
        let mut worst_idx = 0usize;
        for &l in &self.labels {
            let g = self.g(l, p).map_err(|_| FitError::InvalidParams { field: "active_set" })?;
            let v = g / s;
            if v > worst_v {
                worst_v = v;
                worst_idx = l.point as usize;
            }
        }
        Ok((worst_v, worst_idx))
    }

    fn check_p(&self, p: &[f64]) -> Result<(), CertError> {
        if p.len() != 3 {
            return Err(CertError::Shape { field: "p", expected: 3, got: p.len() });
        }
        Ok(())
    }

    fn row(&self, l: RowLabel) -> Result<(f64, f64), CertError> {
        let i = l.point as usize;
        if i >= self.len() || l.side > 1 {
            return Err(cb("unknown row label"));
        }
        Ok((self.u[i], self.w[i]))
    }

    fn g(&self, l: RowLabel, p: &[f64]) -> Result<f64, CertError> {
        let (u, w) = self.row(l)?;
        let (a, c, e) = (p[0], p[1], p[2]);
        Ok(if l.side == 0 { (w - c) - a * u } else { (e + a * u) - w })
    }

    fn g_iv(&self, l: RowLabel, b: &[Interval]) -> Result<Interval, CertError> {
        let (u, w) = self.row(l)?;
        let (ia, ic, ie) = (b[0], b[1], b[2]);
        if l.side == 0 {
            ip(w)?.sub(ic)?.sub(ia.mul(ip(u)?)?)
        } else {
            ie.add(ia.mul(ip(u)?)?)?.sub(ip(w)?)
        }
    }

    fn grad_row(&self, l: RowLabel) -> Result<Vec<f64>, CertError> {
        let (u, _) = self.row(l)?;
        Ok(vec![line2_coef(l.side, 0, u), line2_coef(l.side, 1, u), line2_coef(l.side, 2, u)])
    }
}

fn s2_line2_iv(k2: f64, a: Interval) -> Result<Interval, CertError> {
    ip(1.0)?.add(ip(k2)?.mul(a.sqr()?)?)
}

impl ActiveSetProblem for MzLine2Problem {
    fn n_params(&self) -> usize {
        3
    }

    fn expected_active(&self) -> usize {
        3
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
        let (a, c, e) = (p[0], p[1], p[2]);
        let s = self.slope_norm(a).map_err(num)?;
        let v = (c - e) / s;
        fid_num::check_finite("fit.mz_line2.value", v).map_err(num)
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
        let (a, c, e) = (p[0], p[1], p[2]);
        let s2 = 1.0 + self.k2 * (a * a);
        let s = sqrt(s2).map_err(num)?;
        let s3 = s2 * s;
        let ka = self.k2 * a;
        let wd = c - e;
        let g = vec![-(wd * ka) / s3, 1.0 / s, -1.0 / s];
        fid_num::check_finite_slice("fit.mz_line2.gradient", &g).map_err(num)?;
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
        if b.len() != 3 {
            return Err(CertError::Shape { field: "box", expected: 3, got: b.len() });
        }
        let s = s2_line2_iv(self.k2, b[0])?.sqrt()?;
        let nm = b[1].sub(b[2])?;
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
        if b.len() != 3 {
            return Err(CertError::Shape { field: "box", expected: 3, got: b.len() });
        }
        let mut out = Vec::new();
        out.try_reserve_exact(self.labels.len()).map_err(|_| cb("allocation failed"))?;
        for &l in &self.labels {
            out.push(self.g_iv(l, b)?);
        }
        Ok(Some(out))
    }

    fn objective_gradient_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        if b.len() != 3 {
            return Err(CertError::Shape { field: "box", expected: 3, got: b.len() });
        }
        let (ia, ic, ie) = (b[0], b[1], b[2]);
        let s2 = s2_line2_iv(self.k2, ia)?;
        let s = s2.sqrt()?;
        let s3 = s2.mul(s)?;
        let one = ip(1.0)?;
        let ka = ip(self.k2)?.mul(ia)?;
        let wd = ic.sub(ie)?;
        Ok(Some(vec![wd.mul(ka)?.div(s3)?.neg(), one.div(s)?, one.div(s)?.neg()]))
    }

    fn constraint_gradient_enclosure(
        &self,
        set: &[RowLabel],
        b: &[Interval],
    ) -> Result<Option<Vec<Interval>>, CertError> {
        if b.len() != 3 {
            return Err(CertError::Shape { field: "box", expected: 3, got: b.len() });
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

impl LinearisableProblem for MzLine2Problem {
    fn scales(&self) -> Vec<f64> {
        vec![SLOPE_SCALE, OFFSET_SCALE, OFFSET_SCALE]
    }

    fn local_problem(&self, p: &[f64]) -> Result<(ConicProblem, f64), CertError> {
        let q = self.objective_gradient(p)?;
        let g = self.constraint_values(p)?;
        let rhs: Vec<f64> = g.iter().map(|x| -x).collect();
        let prob = ConicProblemBuilder::new(q, self.a_csc.clone(), rhs, vec![Cone::Nonneg(self.labels.len())])
            .build()
            .map_err(|_| cb("local problem"))?;
        Ok((prob, self.value(p)?))
    }

    fn objective(&self, p: &[f64]) -> Result<f64, CertError> {
        let mut viol = 0.0_f64;
        for &l in &self.labels {
            viol = total_max(viol, self.g(l, p)?).map_err(num)?;
        }
        let f = self.value(p)? + MERIT_PENALTY * viol;
        fid_num::check_finite("fit.mz_line2.merit", f).map_err(num)
    }
}

// ---------------------------------------------------------------------------
// 3D Plane minimum-zone problem: p = (a, b, c, e)
// ---------------------------------------------------------------------------

/// Linearisable minimum-zone plane problem in form-scaled local coordinates `(u, v, w)`.
#[derive(Debug, Clone)]
pub(crate) struct MzPlaneProblem {
    u: Vec<f64>,
    v: Vec<f64>,
    w: Vec<f64>,
    k2: f64,
    data_scale: f64,
    a_csc: Csc,
    labels: Vec<RowLabel>,
}

const OP_PLANE: &str = "fit.mz_plane";

fn plane_coef(side: u8, j: usize, u: f64, v: f64) -> f64 {
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

fn s2_plane_iv(k2: f64, a: Interval, b: Interval) -> Result<Interval, CertError> {
    ip(1.0)?.add(ip(k2)?.mul(a.sqr()?.add(b.sqr()?)?)?)
}

impl MzPlaneProblem {
    /// Builds the 3D minimum-zone plane problem from local columns `(u, v, w)` and exact
    /// power-of-two form scale `k`.
    pub(crate) fn new(u: Vec<f64>, v: Vec<f64>, w: Vec<f64>, k: f64) -> Result<Self, FitError> {
        let n = u.len();
        if n < 3 || v.len() != n || w.len() != n || n > u32::MAX as usize {
            return Err(FitError::TooFewPoints { n, need: 3 });
        }
        let mut data_scale = 0.0_f64;
        for i in 0..n {
            data_scale = total_max(data_scale, u[i].abs())?;
            data_scale = total_max(data_scale, v[i].abs())?;
            data_scale = total_max(data_scale, w[i].abs())?;
        }
        let m = n * 2;
        let mut labels = alloc::<RowLabel>(OP_PLANE, m)?;
        for i in 0..n {
            labels.push(RowLabel { point: i as u32, side: 0 });
            labels.push(RowLabel { point: i as u32, side: 1 });
        }
        let np = 4usize;
        let mut colptr = alloc::<usize>(OP_PLANE, np + 1)?;
        let mut rowval = alloc::<usize>(OP_PLANE, m * 3)?;
        let mut nzval = alloc::<f64>(OP_PLANE, m * 3)?;
        colptr.push(0);
        for j in 0..np {
            for (r, l) in labels.iter().enumerate() {
                let x = plane_coef(l.side, j, u[l.point as usize], v[l.point as usize]);
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
            return Err(FitError::Num(NumError::Domain { op: OP_PLANE, detail: "form_scale" }));
        }
        Ok(Self { u, v, w, k2, data_scale, a_csc, labels })
    }

    pub(crate) fn len(&self) -> usize {
        self.u.len()
    }

    /// Lifts slopes `(a, b)` onto the data: `c = max_i (w_i - a u_i - b v_i)`,
    /// `e = min_i (w_i - a u_i - b v_i)`.
    pub(crate) fn lift(&self, a: f64, b: f64) -> Result<Vec<f64>, FitError> {
        let mut hi = f64::NEG_INFINITY;
        let mut lo = f64::INFINITY;
        for i in 0..self.len() {
            let t = (self.w[i] - a * self.u[i]) - b * self.v[i];
            if !t.is_finite() {
                return Err(FitError::Num(NumError::Infinite { op: "fit.mz_plane.start", index: Some(i) }));
            }
            hi = total_max(hi, t)?;
            lo = total_min(lo, t)?;
        }
        let mut p = alloc::<f64>(OP_PLANE, 4)?;
        p.extend_from_slice(&[a, b, hi, lo]);
        Ok(p)
    }

    /// `s = sqrt(1 + k^2 (a^2 + b^2))`.
    pub(crate) fn slope_norm(&self, a: f64, b: f64) -> Result<f64, NumError> {
        let s = sqrt(1.0 + self.k2 * (a * a + b * b))?;
        fid_num::check_finite("fit.mz_plane.slope", s)
    }

    /// Form-scaled signed residuals from the mid-zone plane:
    /// `((w_i - mid) - a u_i - b v_i) / s`, where `mid = 0.5 * (c + e)`.
    pub(crate) fn signed_residuals(&self, p: &[f64]) -> Result<Vec<f64>, FitError> {
        let (a, b, c, e) = (p[0], p[1], p[2], p[3]);
        let mid = 0.5 * (c + e);
        let s = self.slope_norm(a, b).map_err(FitError::Num)?;
        let mut out = alloc::<f64>(OP_PLANE, self.len())?;
        for i in 0..self.len() {
            let r = (((self.w[i] - mid) - a * self.u[i]) - b * self.v[i]) / s;
            out.push(r);
        }
        fid_num::check_finite_slice("fit.mz_plane.residuals", &out)?;
        Ok(out)
    }

    /// Largest orthogonal constraint violation in form-scaled units (`max(0, max_r g_r / s)`).
    pub(crate) fn max_scaled_violation(&self, p: &[f64]) -> Result<(f64, usize), FitError> {
        let s = self.slope_norm(p[0], p[1]).map_err(FitError::Num)?;
        let mut worst_v = 0.0_f64;
        let mut worst_idx = 0usize;
        for &l in &self.labels {
            let g = self.g(l, p).map_err(|_| FitError::InvalidParams { field: "active_set" })?;
            let v = g / s;
            if v > worst_v {
                worst_v = v;
                worst_idx = l.point as usize;
            }
        }
        Ok((worst_v, worst_idx))
    }

    fn check_p(&self, p: &[f64]) -> Result<(), CertError> {
        if p.len() != 4 {
            return Err(CertError::Shape { field: "p", expected: 4, got: p.len() });
        }
        Ok(())
    }

    fn row(&self, l: RowLabel) -> Result<(f64, f64, f64), CertError> {
        let i = l.point as usize;
        if i >= self.len() || l.side > 1 {
            return Err(cb("unknown row label"));
        }
        Ok((self.u[i], self.v[i], self.w[i]))
    }

    fn g(&self, l: RowLabel, p: &[f64]) -> Result<f64, CertError> {
        let (u, v, w) = self.row(l)?;
        let (a, b, c, e) = (p[0], p[1], p[2], p[3]);
        Ok(if l.side == 0 { ((w - c) - a * u) - b * v } else { ((e + a * u) + b * v) - w })
    }

    fn g_iv(&self, l: RowLabel, b: &[Interval]) -> Result<Interval, CertError> {
        let (u, v, w) = self.row(l)?;
        let (ia, ib, ic, ie) = (b[0], b[1], b[2], b[3]);
        if l.side == 0 {
            ip(w)?.sub(ic)?.sub(ia.mul(ip(u)?)?)?.sub(ib.mul(ip(v)?)?)
        } else {
            ie.add(ia.mul(ip(u)?)?)?.add(ib.mul(ip(v)?)?)?.sub(ip(w)?)
        }
    }

    fn grad_row(&self, l: RowLabel) -> Result<Vec<f64>, CertError> {
        let (u, v, _) = self.row(l)?;
        Ok(vec![
            plane_coef(l.side, 0, u, v),
            plane_coef(l.side, 1, u, v),
            plane_coef(l.side, 2, u, v),
            plane_coef(l.side, 3, u, v),
        ])
    }
}

impl ActiveSetProblem for MzPlaneProblem {
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
        let (a, b, c, e) = (p[0], p[1], p[2], p[3]);
        let s = self.slope_norm(a, b).map_err(num)?;
        let v = (c - e) / s;
        fid_num::check_finite("fit.mz_plane.value", v).map_err(num)
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
        let (a, b, c, e) = (p[0], p[1], p[2], p[3]);
        let s2 = 1.0 + self.k2 * (a * a + b * b);
        let s = sqrt(s2).map_err(num)?;
        let s3 = s2 * s;
        let (ka, kb) = (self.k2 * a, self.k2 * b);
        let wd = c - e;
        let g = vec![-(wd * ka) / s3, -(wd * kb) / s3, 1.0 / s, -1.0 / s];
        fid_num::check_finite_slice("fit.mz_plane.gradient", &g).map_err(num)?;
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
        if b.len() != 4 {
            return Err(CertError::Shape { field: "box", expected: 4, got: b.len() });
        }
        let s = s2_plane_iv(self.k2, b[0], b[1])?.sqrt()?;
        let nm = b[2].sub(b[3])?;
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
        if b.len() != 4 {
            return Err(CertError::Shape { field: "box", expected: 4, got: b.len() });
        }
        let mut out = Vec::new();
        out.try_reserve_exact(self.labels.len()).map_err(|_| cb("allocation failed"))?;
        for &l in &self.labels {
            out.push(self.g_iv(l, b)?);
        }
        Ok(Some(out))
    }

    fn objective_gradient_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        if b.len() != 4 {
            return Err(CertError::Shape { field: "box", expected: 4, got: b.len() });
        }
        let (ia, ib, ic, ie) = (b[0], b[1], b[2], b[3]);
        let s2 = s2_plane_iv(self.k2, ia, ib)?;
        let s = s2.sqrt()?;
        let s3 = s2.mul(s)?;
        let one = ip(1.0)?;
        let k2 = ip(self.k2)?;
        let (ka, kb) = (k2.mul(ia)?, k2.mul(ib)?);
        let wd = ic.sub(ie)?;
        Ok(Some(vec![wd.mul(ka)?.div(s3)?.neg(), wd.mul(kb)?.div(s3)?.neg(), one.div(s)?, one.div(s)?.neg()]))
    }

    fn constraint_gradient_enclosure(
        &self,
        set: &[RowLabel],
        b: &[Interval],
    ) -> Result<Option<Vec<Interval>>, CertError> {
        if b.len() != 4 {
            return Err(CertError::Shape { field: "box", expected: 4, got: b.len() });
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

impl LinearisableProblem for MzPlaneProblem {
    fn scales(&self) -> Vec<f64> {
        vec![SLOPE_SCALE, SLOPE_SCALE, OFFSET_SCALE, OFFSET_SCALE]
    }

    fn local_problem(&self, p: &[f64]) -> Result<(ConicProblem, f64), CertError> {
        let q = self.objective_gradient(p)?;
        let g = self.constraint_values(p)?;
        let rhs: Vec<f64> = g.iter().map(|x| -x).collect();
        let prob = ConicProblemBuilder::new(q, self.a_csc.clone(), rhs, vec![Cone::Nonneg(self.labels.len())])
            .build()
            .map_err(|_| cb("local problem"))?;
        Ok((prob, self.value(p)?))
    }

    fn objective(&self, p: &[f64]) -> Result<f64, CertError> {
        let mut viol = 0.0_f64;
        for &l in &self.labels {
            viol = total_max(viol, self.g(l, p)?).map_err(num)?;
        }
        let f = self.value(p)? + MERIT_PENALTY * viol;
        fid_num::check_finite("fit.mz_plane.merit", f).map_err(num)
    }
}
