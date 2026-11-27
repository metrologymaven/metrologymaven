//! The linearisable maximum-inscribed circle and sphere problem handed to `fid_solv::seq` and
//! certified by `fid_solv::cert` (module documentation of [`super`], rules 5 and 6).
//!
//! Parameters in the centred, scaled units of `contract::Centred`: the centre `c` (`dim` = 2
//! or 3 components) and the radius `r`, `p = (c, r)`. The rows are those of the working set
//! `idx` (ascending point indices; every point for a small cloud, rule 8 of [`super`]). Every
//! working-set point `q_i` owns the row
//! `g_i = r - rho_i <= 0` (label `(i, 0)`), `rho_i = |q_i - c|`, with gradient
//! `[(q_i - c) / rho_i, 1]`. Every trust-region facet `k` (`mic_tr`, mapped to the scaled
//! frame) owns the linear row `a_k . c - b_k <= 0` (label `(k, 1)`), gradient `[a_k, 0]`. Rows
//! are ordered points first, then facets. The objective is `-r` (minimised).
//!
//! `rho_i` is convex in `c`, so its tangent plane lies below it: a step that satisfies the
//! linearised row `r + dr <= rho_i + grad . dc` also satisfies the exact row. Every LP step is
//! therefore feasible, and the merit (`objective`) equals the model at every accepted point.

// Indexed loops over the parameter columns mirror the formulas term by term.
#![allow(clippy::needless_range_loop)]

use fid_num::f::{total_max, total_min};
use fid_num::la::{lstsq, Mat};
use fid_num::red;
use fid_solv::cert::recomp::{ActiveSetProblem, Interval};
use fid_solv::cert::{CertError, RowLabel};
use fid_solv::conic::{Cone, ConicProblem, ConicProblemBuilder, Csc};
use fid_solv::seq::LinearisableProblem;

use crate::contract::V3;

/// Exact-penalty factor of the merit: above the point rows' multiplier sum, which is 1
/// (stationarity in `r`). Every LP step is feasible (module note), so the penalty only meets
/// rounding-sized violations.
pub const PENALTY: f64 = 2.0;

/// The side of a trust-region facet row's label (point rows have side 0).
pub const TRUST_SIDE: u8 = 1;

/// The rank tolerance of the closed-form start (`start_from_set`).
const START_RANK_TOL: f64 = 1e-12;

fn cb(reason: &'static str) -> CertError {
    CertError::Callback { reason }
}

fn num(e: fid_num::NumError) -> CertError {
    CertError::Numeric(e)
}

fn ip(x: f64) -> Result<Interval, CertError> {
    Interval::point(x)
}

/// One row of the problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Row {
    Point(usize),
    Trust(usize),
}

/// The problem: scaled points, the scaled trust rows and the scales.
#[derive(Debug, Clone)]
pub(crate) struct Mic<'a> {
    /// Every scaled point, stride 3 (`z = 0` for the circle).
    pub q: &'a [f64],
    /// The working set: the indices of the points that own rows, ascending.
    pub idx: Vec<u32>,
    /// 2 (circle) or 3 (sphere).
    pub dim: usize,
    /// Trust facet normals and offsets in the scaled frame.
    pub a: Vec<V3>,
    pub b: Vec<f64>,
    /// Step-bound scale of every parameter (the trust circumradius, scaled).
    pub len_scale: f64,
    /// KKT data scale: the largest `|q_ij|` and `|b_k|`.
    pub data_scale: f64,
    /// The primary start, used by `start_from_set` when the closed form has no solution.
    pub start: Vec<f64>,
}

impl Mic<'_> {
    pub(crate) fn np(&self) -> usize {
        self.dim + 1
    }

    /// The number of point rows (the working set's size).
    pub(crate) fn n_pts(&self) -> usize {
        self.idx.len()
    }

    /// The point of working-set position `i`.
    fn point(&self, i: usize) -> Result<V3, CertError> {
        let k = *self.idx.get(i).ok_or(cb("point index"))? as usize;
        let s = self.q.get(3 * k..3 * k + 3).ok_or(cb("point index"))?;
        Ok([s[0], s[1], s[2]])
    }

    /// The row of a label: a point row's label carries the point's index in the whole cloud,
    /// which must be in the working set.
    pub(crate) fn row(&self, l: RowLabel) -> Result<Row, CertError> {
        let k = l.point as usize;
        match l.side {
            0 => self.idx.binary_search(&l.point).map(Row::Point).map_err(|_| cb("row label")),
            TRUST_SIDE if k < self.a.len() => Ok(Row::Trust(k)),
            _ => Err(cb("row label")),
        }
    }

    /// The cloud index of working-set position `i`.
    pub(crate) fn index(&self, i: usize) -> Result<u32, CertError> {
        self.idx.get(i).copied().ok_or(cb("point index"))
    }

    /// `(c, r)` of `p`.
    pub(crate) fn split(&self, p: &[f64]) -> Result<(V3, f64), CertError> {
        if p.len() != self.np() {
            return Err(cb("parameter count"));
        }
        let mut c = [0.0; 3];
        c[..self.dim].copy_from_slice(&p[..self.dim]);
        Ok((c, p[self.dim]))
    }

    /// `rho_i` and the unit vector `(q_i - c) / rho_i` (first `dim` components).
    pub(crate) fn dist_dir(&self, i: usize, c: V3) -> Result<(f64, V3), CertError> {
        let q = self.point(i)?;
        let mut w = [0.0; 3];
        for k in 0..self.dim {
            w[k] = q[k] - c[k];
        }
        let rho = red::norm2(&w[..self.dim]).map_err(num)?;
        if rho.is_nan() || rho <= 0.0 {
            return Err(cb("centre on a point"));
        }
        let mut u = [0.0; 3];
        for k in 0..self.dim {
            u[k] = w[k] / rho;
        }
        Ok((rho, u))
    }

    fn trust_value(&self, k: usize, c: V3) -> Result<f64, CertError> {
        let a = self.a.get(k).ok_or(cb("facet index"))?;
        let b = *self.b.get(k).ok_or(cb("facet index"))?;
        Ok(red::dot(&a[..self.dim], &c[..self.dim]).map_err(num)? - b)
    }

    /// Value and gradient of one row at `p`.
    fn row_value_grad(&self, row: Row, c: V3, r: f64) -> Result<(f64, Vec<f64>), CertError> {
        let mut g = vec![0.0; self.np()];
        match row {
            Row::Point(i) => {
                let (rho, u) = self.dist_dir(i, c)?;
                g[..self.dim].copy_from_slice(&u[..self.dim]);
                g[self.dim] = 1.0;
                Ok((r - rho, g))
            }
            Row::Trust(k) => {
                let a = self.a.get(k).ok_or(cb("facet index"))?;
                g[..self.dim].copy_from_slice(&a[..self.dim]);
                Ok((self.trust_value(k, c)?, g))
            }
        }
    }

    /// The smallest `rho_i` over the working set at the centre `c`.
    pub(crate) fn min_dist(&self, c: V3) -> Result<f64, CertError> {
        let mut lo = f64::INFINITY;
        for i in 0..self.n_pts() {
            lo = total_min(lo, self.dist_dir(i, c)?.0).map_err(num)?;
        }
        Ok(lo)
    }

    /// Interval `r - rho_i` and its gradient over the box `b` (all `np` parameters).
    fn point_iv(&self, i: usize, b: &[Interval]) -> Result<(Interval, Vec<Interval>), CertError> {
        if b.len() != self.np() {
            return Err(cb("box length"));
        }
        let q = self.point(i)?;
        let mut w = [ip(0.0)?; 3];
        let mut ss = ip(0.0)?;
        for k in 0..self.dim {
            w[k] = ip(q[k])?.sub(b[k])?;
            ss = ss.add(w[k].sqr()?)?;
        }
        let rho = ss.sqrt()?;
        let mut g = Vec::with_capacity(self.np());
        for k in 0..self.dim {
            g.push(w[k].div(rho)?);
        }
        g.push(ip(1.0)?);
        Ok((b[self.dim].sub(rho)?, g))
    }

    fn trust_iv(&self, k: usize, b: &[Interval]) -> Result<(Interval, Vec<Interval>), CertError> {
        if b.len() != self.np() {
            return Err(cb("box length"));
        }
        let a = self.a.get(k).ok_or(cb("facet index"))?;
        let bk = *self.b.get(k).ok_or(cb("facet index"))?;
        let mut v = ip(0.0)?;
        let mut g = Vec::with_capacity(self.np());
        for j in 0..self.dim {
            let aj = ip(a[j])?;
            v = v.add(aj.mul(b[j])?)?;
            g.push(aj);
        }
        g.push(ip(0.0)?);
        Ok((v.sub(ip(bk)?)?, g))
    }

    fn row_iv(&self, row: Row, b: &[Interval]) -> Result<(Interval, Vec<Interval>), CertError> {
        match row {
            Row::Point(i) => self.point_iv(i, b),
            Row::Trust(k) => self.trust_iv(k, b),
        }
    }

    fn all_rows(&self) -> impl Iterator<Item = Row> + '_ {
        (0..self.n_pts()).map(Row::Point).chain((0..self.a.len()).map(Row::Trust))
    }

    fn points_box(p: &[f64]) -> Result<Vec<Interval>, CertError> {
        let mut b = Vec::with_capacity(p.len());
        for &x in p {
            b.push(ip(x)?);
        }
        Ok(b)
    }
}

impl ActiveSetProblem for Mic<'_> {
    fn n_params(&self) -> usize {
        self.np()
    }

    fn expected_active(&self) -> usize {
        self.np()
    }

    fn row_labels(&self) -> Vec<RowLabel> {
        let pts = self.idx.iter().map(|&point| RowLabel { point, side: 0 });
        let facets = (0..self.a.len() as u32).map(|point| RowLabel { point, side: TRUST_SIDE });
        pts.chain(facets).collect()
    }

    fn is_linear(&self) -> bool {
        false
    }

    /// The closed-form point of the set (rule 6): with the set's points `q_0, .., q_m` (at
    /// least one) and facets, `c = q_0 + y` where `y` solves the `dim x dim` linear system
    /// `2 (q_j - q_0) . y = |q_j - q_0|^2` (equal distance to `q_0` and `q_j`) and
    /// `a_k . y = b_k - a_k . q_0` (on facet `k`), by `fid_num::la::lstsq`; `r = |y|`. It reads
    /// only the set and the problem data. A set without a point row, or a rank-deficient
    /// system, gives the problem's primary start (the recomputation then fails closed on the
    /// set's own rank).
    fn start_from_set(&self, set: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        let mut pts = Vec::new();
        let mut facets = Vec::new();
        for l in set {
            match self.row(*l)? {
                Row::Point(i) => pts.push(i),
                Row::Trust(k) => facets.push(k),
            }
        }
        let Some((&i0, rest)) = pts.split_first() else {
            return Ok(self.start.clone());
        };
        let q0 = self.point(i0)?;
        let mut rows: Vec<Vec<f64>> = Vec::with_capacity(self.dim);
        let mut rhs = Vec::with_capacity(self.dim);
        for &j in rest {
            let qj = self.point(j)?;
            let mut w = vec![0.0; self.dim];
            for k in 0..self.dim {
                w[k] = qj[k] - q0[k];
            }
            rhs.push(red::dot(&w, &w).map_err(num)?);
            rows.push(w.iter().map(|x| 2.0 * x).collect());
        }
        for &k in &facets {
            let a = self.a.get(k).ok_or(cb("facet index"))?;
            let bk = *self.b.get(k).ok_or(cb("facet index"))?;
            rhs.push(bk - red::dot(&a[..self.dim], &q0[..self.dim]).map_err(num)?);
            rows.push(a[..self.dim].to_vec());
        }
        if rows.len() != self.dim {
            return Ok(self.start.clone());
        }
        let m = Mat::from_rows(&rows).map_err(num)?;
        let y = match lstsq(&m, &rhs, START_RANK_TOL) {
            Ok(s) if s.diag.rank == self.dim => s.x,
            _ => return Ok(self.start.clone()),
        };
        let r = red::norm2(&y).map_err(num)?;
        let mut p = Vec::with_capacity(self.np());
        for k in 0..self.dim {
            p.push(q0[k] + y[k]);
        }
        p.push(r);
        if p.iter().any(|x| !x.is_finite()) {
            return Ok(self.start.clone());
        }
        Ok(p)
    }

    fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
        let (c, r) = self.split(p)?;
        let mut res = Vec::with_capacity(set.len());
        let mut rows = Vec::with_capacity(set.len());
        for l in set {
            let (v, g) = self.row_value_grad(self.row(*l)?, c, r)?;
            res.push(v);
            rows.push(g);
        }
        Ok((res, Mat::from_rows(&rows).map_err(num)?))
    }

    fn value(&self, p: &[f64]) -> Result<f64, CertError> {
        Ok(-self.split(p)?.1)
    }

    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        let (c, r) = self.split(p)?;
        let mut out = Vec::new();
        out.try_reserve_exact(self.n_pts() + self.a.len()).map_err(|_| cb("allocation"))?;
        for row in self.all_rows() {
            out.push(self.row_value_grad(row, c, r)?.0);
        }
        Ok(out)
    }

    fn objective_gradient(&self, _p: &[f64]) -> Result<Vec<f64>, CertError> {
        let mut g = vec![0.0; self.np()];
        g[self.dim] = -1.0;
        Ok(g)
    }

    fn constraint_gradients(&self, set: &[RowLabel], p: &[f64]) -> Result<Mat, CertError> {
        Ok(self.residuals_and_jacobian(set, p)?.1)
    }

    fn residual_enclosure(&self, set: &[RowLabel], p: &[f64]) -> Result<Option<Vec<Interval>>, CertError> {
        let b = Self::points_box(p)?;
        let mut out = Vec::with_capacity(set.len());
        for l in set {
            out.push(self.row_iv(self.row(*l)?, &b)?.0);
        }
        Ok(Some(out))
    }

    fn jacobian_enclosure(&self, set: &[RowLabel], b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::with_capacity(set.len() * self.np());
        for l in set {
            out.extend(self.row_iv(self.row(*l)?, b)?.1);
        }
        Ok(Some(out))
    }

    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        Ok(Some(b.get(self.dim).ok_or(cb("box length"))?.neg()))
    }

    fn data_scale(&self) -> Result<f64, CertError> {
        Ok(self.data_scale)
    }

    fn constraint_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        out.try_reserve_exact(self.n_pts() + self.a.len()).map_err(|_| cb("allocation"))?;
        for row in self.all_rows() {
            out.push(self.row_iv(row, b)?.0);
        }
        Ok(Some(out))
    }

    fn objective_gradient_enclosure(&self, _b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut g = Vec::with_capacity(self.np());
        for k in 0..self.np() {
            g.push(ip(if k == self.dim { -1.0 } else { 0.0 })?);
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

impl LinearisableProblem for Mic<'_> {
    fn scales(&self) -> Vec<f64> {
        vec![self.len_scale; self.np()]
    }

    /// Minimise `-dr` subject to `[(q_i - c) / rho_i, 1] . (dc, dr) <= rho_i - r` for every
    /// point and `a_k . dc <= b_k - a_k . c` for every facet (all rows of the nonnegative
    /// cone), `m0 = -r`.
    fn local_problem(&self, p: &[f64]) -> Result<(ConicProblem, f64), CertError> {
        let (c, r) = self.split(p)?;
        let m = self.n_pts() + self.a.len();
        let np = self.np();
        let mut cols: Vec<Vec<f64>> = Vec::with_capacity(np);
        for _ in 0..np {
            let mut col = Vec::new();
            col.try_reserve_exact(m).map_err(|_| cb("allocation"))?;
            cols.push(col);
        }
        let mut rhs = Vec::new();
        rhs.try_reserve_exact(m).map_err(|_| cb("allocation"))?;
        for row in self.all_rows() {
            let (v, g) = self.row_value_grad(row, c, r)?;
            for j in 0..np {
                cols[j].push(g[j]);
            }
            rhs.push(-v);
        }
        let mut colptr = Vec::with_capacity(np + 1);
        let mut rowval = Vec::new();
        let mut nzval = Vec::new();
        rowval.try_reserve_exact(m * np).map_err(|_| cb("allocation"))?;
        nzval.try_reserve_exact(m * np).map_err(|_| cb("allocation"))?;
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
        Ok((prob, -r))
    }

    /// The merit `-r + PENALTY * max(0, max_i (r - rho_i), max_k (a_k . c - b_k))`.
    fn objective(&self, p: &[f64]) -> Result<f64, CertError> {
        let (c, r) = self.split(p)?;
        let mut viol = 0.0_f64;
        viol = total_max(viol, r - self.min_dist(c)?).map_err(num)?;
        for k in 0..self.a.len() {
            viol = total_max(viol, self.trust_value(k, c)?).map_err(num)?;
        }
        fid_num::check_finite("fit.mic_cirsph.merit", -r + PENALTY * viol).map_err(num)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn square() -> (Vec<V3>, Vec<f64>) {
        (vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]], vec![0.5; 4])
    }

    #[test]
    fn closed_form_start_of_mixed_sets() {
        // Three points on the unit circle about (0.1, 0): the circumcentre and radius 1.
        let q = [1.1, 0.0, 0.0, 0.1, 1.0, 0.0, -0.9, 0.0, 0.0, 0.1, -1.0, 0.0];
        let (a, b) = square();
        let p = Mic {
            q: &q,
            idx: vec![0, 1, 2, 3],
            dim: 2,
            a,
            b,
            len_scale: 0.5,
            data_scale: 1.1,
            start: vec![0.0, 0.0, 0.5],
        };
        let l = |point, side| RowLabel { point, side };
        let s = p.start_from_set(&[l(0, 0), l(1, 0), l(2, 0)]).unwrap();
        assert!((s[0] - 0.1).abs() < 1e-15 && s[1].abs() < 1e-15 && (s[2] - 1.0).abs() < 1e-15, "{s:?}");
        // Two points and the facet x <= 0.5: on the bisector of points 1 and 3 (y = 0).
        let s = p.start_from_set(&[l(0, TRUST_SIDE), l(1, 0), l(3, 0)]).unwrap();
        assert_eq!((s[0], s[1]), (0.5, 0.0));
        // No point row: the primary start.
        let s = p.start_from_set(&[l(0, TRUST_SIDE), l(1, TRUST_SIDE), l(2, TRUST_SIDE)]).unwrap();
        assert_eq!(s, vec![0.0, 0.0, 0.5]);
        // Labels outside the problem are callback errors, not panics.
        assert!(p.start_from_set(&[l(9, 0)]).is_err());
        assert!(p.start_from_set(&[l(4, TRUST_SIDE)]).is_err());
        assert!(p.start_from_set(&[l(0, 7)]).is_err());
    }

    #[test]
    fn every_lp_step_is_feasible() {
        // The merit at a feasible point is -r; the local problem's rhs at zero step is -g.
        let q = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0, 0.0];
        let (a, b) = square();
        let p = Mic {
            q: &q,
            idx: vec![0, 1, 2, 3],
            dim: 2,
            a,
            b,
            len_scale: 0.5,
            data_scale: 1.0,
            start: vec![0.0, 0.0, 0.5],
        };
        assert_eq!(p.objective(&[0.0, 0.0, 0.5]).unwrap(), -0.5);
        // Infeasible radius: penalised.
        assert_eq!(p.objective(&[0.0, 0.0, 1.5]).unwrap(), -1.5 + PENALTY * 0.5);
        let (_, m0) = p.local_problem(&[0.0, 0.0, 0.5]).unwrap();
        assert_eq!(m0, -0.5);
        assert_eq!(p.row_labels().len(), 8);
    }
}
