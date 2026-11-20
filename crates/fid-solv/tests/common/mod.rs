//! Shared by the fid-solv integration tests that use plain-f64 minimum-zone circles and
//! cylinder axes with every enclosure the certificate needs (`cert_recomp_floor`, `cert_kkt_far`).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::f;
use fid_num::la::{lstsq, Mat};
use fid_solv::cert::active::{extract, Proposal, RowLabel, Thresholds};
use fid_solv::cert::recomp::{ActiveSetProblem, Interval};
use fid_solv::cert::CertError;
use fid_solv::conic::{Cone, ConicProblem, ConicProblemBuilder, ConicResiduals, ConicSolution, Csc};

pub fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| f64::from_bits(u64::from_str_radix(t, 16).unwrap())).collect()
}

pub fn labels(s: &str) -> Vec<RowLabel> {
    s.split_whitespace()
        .map(|t| {
            let (p, sd) = t.split_once(':').unwrap();
            RowLabel { point: p.parse().unwrap(), side: sd.parse().unwrap() }
        })
        .collect()
}

pub fn ip(x: f64) -> Interval {
    Interval::point(x).unwrap()
}

// ------------------------------------------------------------------ test problem

/// Minimum-zone circle (`geo = 2`, points `(x, y)`, `p = (cx, cy, r_in, r_out)`) or cylinder axis
/// (`geo = 4`, points `(x, y, z)`, `p = (x0, y0, a, b, r_in, r_out)`, the axis through
/// `(x0, y0, 0)` with direction `(a, b, 1)`). Rows `(i, 0)`: `g = r_in - d_i`, `(i, 1)`:
/// `g = d_i - r_out`; the active-set residual is `d_i - r_side`, **in plain `f64`**. Every
/// enclosure the recomputation and the KKT check need is implemented. The circle start comes
/// from the set (perpendicular bisectors of each contact group, one QR solve); the cylinder
/// start is the fixture's nominal axis (problem data).
pub struct Geo {
    pub geo: usize,
    pub pts: Vec<[f64; 3]>,
    pub start: Vec<f64>,
}

impl Geo {
    pub fn new(kind: &str, flat: &[f64], start: Vec<f64>) -> Geo {
        let (geo, pts) = match kind {
            "arc" => (2, flat.chunks(2).map(|c| [c[0], c[1], 0.0]).collect()),
            "cyl" => (4, flat.chunks(3).map(|c| [c[0], c[1], c[2]]).collect()),
            k => panic!("kind {k}"),
        };
        Geo { geo, pts, start }
    }

    /// Distance of point `i` and its gradient in the geometric parameters.
    pub fn dist(&self, i: usize, p: &[f64]) -> (f64, Vec<f64>) {
        let [x, y, z] = self.pts[i];
        if self.geo == 2 {
            let (dx, dy) = (x - p[0], y - p[1]);
            let d = f::sqrt(dx * dx + dy * dy).unwrap();
            return (d, vec![-dx / d, -dy / d]);
        }
        let (a, b) = (p[2], p[3]);
        let (wx, wy, wz) = (x - p[0], y - p[1], z);
        let (cx, cy, cz) = (wy - wz * b, wz * a - wx, wx * b - wy * a);
        let nc = f::sqrt(cx * cx + cy * cy + cz * cz).unwrap();
        let vv = a * a + b * b + 1.0;
        let nv = f::sqrt(vv).unwrap();
        let k = nc * nv;
        let g = vec![
            (cy - b * cz) / k,
            (a * cz - cx) / k,
            (wz * cy - wy * cz) / k - nc * a / (nv * vv),
            (wx * cz - wz * cx) / k - nc * b / (nv * vv),
        ];
        (nc / nv, g)
    }

    /// Interval distance of point `i` and its gradient over the box `b`.
    fn dist_iv(&self, i: usize, b: &[Interval]) -> Result<(Interval, Vec<Interval>), CertError> {
        let [x, y, z] = self.pts[i];
        if self.geo == 2 {
            let dx = ip(x).sub(b[0])?;
            let dy = ip(y).sub(b[1])?;
            let d = dx.sqr()?.add(dy.sqr()?)?.sqrt()?;
            return Ok((d, vec![dx.div(d)?.neg(), dy.div(d)?.neg()]));
        }
        let (a, bb) = (b[2], b[3]);
        let (wx, wy, wz) = (ip(x).sub(b[0])?, ip(y).sub(b[1])?, ip(z));
        let cx = wy.sub(wz.mul(bb)?)?;
        let cy = wz.mul(a)?.sub(wx)?;
        let cz = wx.mul(bb)?.sub(wy.mul(a)?)?;
        let nc = cx.sqr()?.add(cy.sqr()?)?.add(cz.sqr()?)?.sqrt()?;
        let vv = a.sqr()?.add(bb.sqr()?)?.add(ip(1.0))?;
        let nv = vv.sqrt()?;
        let k = nc.mul(nv)?;
        let nvv = nv.mul(vv)?;
        let g = vec![
            cy.sub(bb.mul(cz)?)?.div(k)?,
            a.mul(cz)?.sub(cx)?.div(k)?,
            wz.mul(cy)?.sub(wy.mul(cz)?)?.div(k)?.sub(nc.mul(a)?.div(nvv)?)?,
            wx.mul(cz)?.sub(wz.mul(cx)?)?.div(k)?.sub(nc.mul(bb)?.div(nvv)?)?,
        ];
        Ok((nc.div(nv)?, g))
    }

    fn grad_row(&self, side: u8, gd: &[Interval]) -> Vec<Interval> {
        let mut row = vec![ip(0.0); self.n_params()];
        for (k, g) in gd.iter().enumerate() {
            row[k] = if side == 0 { g.neg() } else { *g };
        }
        row[self.geo + side as usize] = ip(if side == 0 { 1.0 } else { -1.0 });
        row
    }

    fn obj_grad(&self) -> Vec<f64> {
        let mut g = vec![0.0; self.n_params()];
        g[self.geo] = -1.0;
        g[self.geo + 1] = 1.0;
        g
    }
}

impl ActiveSetProblem for Geo {
    fn n_params(&self) -> usize {
        self.geo + 2
    }
    fn expected_active(&self) -> usize {
        self.geo + 2
    }
    fn row_labels(&self) -> Vec<RowLabel> {
        (0..self.pts.len() as u32).flat_map(|point| [0, 1].map(|side| RowLabel { point, side })).collect()
    }
    fn is_linear(&self) -> bool {
        false
    }
    fn start_from_set(&self, set: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        if self.geo == 4 {
            return Ok(self.start.clone());
        }
        let mut rows = Vec::new();
        let mut rhs = Vec::new();
        let mut radii = Vec::new();
        for side in [0, 1] {
            let group: Vec<usize> = set.iter().filter(|l| l.side == side).map(|l| l.point as usize).collect();
            let Some((&g0, rest)) = group.split_first() else {
                return Err(CertError::Callback { reason: "arc set needs an inner and an outer point" });
            };
            radii.push(g0);
            let [x0, y0, _] = self.pts[g0];
            for &k in rest {
                let [x1, y1, _] = self.pts[k];
                rows.push(vec![2.0 * (x1 - x0), 2.0 * (y1 - y0)]);
                rhs.push((x1 * x1 + y1 * y1) - (x0 * x0 + y0 * y0));
            }
        }
        let s = lstsq(&Mat::from_rows(&rows)?, &rhs, 0.0)?;
        let c = [s.x[0], s.x[1]];
        let mut p = c.to_vec();
        for i in radii {
            p.push(self.dist(i, &c).0);
        }
        Ok(p)
    }
    fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
        let mut r = Vec::new();
        let mut rows = Vec::new();
        for l in set {
            let (d, gd) = self.dist(l.point as usize, p);
            r.push(d - p[self.geo + l.side as usize]);
            let mut row = vec![0.0; self.n_params()];
            row[..gd.len()].copy_from_slice(&gd);
            row[self.geo + l.side as usize] = -1.0;
            rows.push(row);
        }
        Ok((r, Mat::from_rows(&rows)?))
    }
    fn value(&self, p: &[f64]) -> Result<f64, CertError> {
        Ok(p[self.geo + 1] - p[self.geo])
    }
    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        Ok(self
            .row_labels()
            .iter()
            .map(|l| {
                let d = self.dist(l.point as usize, p).0;
                let r = p[self.geo + l.side as usize];
                if l.side == 0 {
                    r - d
                } else {
                    d - r
                }
            })
            .collect())
    }
    fn objective_gradient(&self, _: &[f64]) -> Result<Vec<f64>, CertError> {
        Ok(self.obj_grad())
    }
    fn constraint_gradients(&self, set: &[RowLabel], p: &[f64]) -> Result<Mat, CertError> {
        let rows: Vec<Vec<f64>> = set
            .iter()
            .map(|l| {
                let gd = self.dist(l.point as usize, p).1;
                let mut row = vec![0.0; self.n_params()];
                for (k, g) in gd.iter().enumerate() {
                    row[k] = if l.side == 0 { -g } else { *g };
                }
                row[self.geo + l.side as usize] = if l.side == 0 { 1.0 } else { -1.0 };
                row
            })
            .collect();
        Ok(Mat::from_rows(&rows)?)
    }
    fn residual_enclosure(&self, set: &[RowLabel], p: &[f64]) -> Result<Option<Vec<Interval>>, CertError> {
        let b: Vec<Interval> = p.iter().map(|&x| ip(x)).collect();
        let mut out = Vec::new();
        for l in set {
            let (d, _) = self.dist_iv(l.point as usize, &b)?;
            out.push(d.sub(b[self.geo + l.side as usize])?);
        }
        Ok(Some(out))
    }
    fn jacobian_enclosure(&self, set: &[RowLabel], b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        for l in set {
            let (_, gd) = self.dist_iv(l.point as usize, b)?;
            let mut row = vec![ip(0.0); self.n_params()];
            row[..gd.len()].copy_from_slice(&gd);
            row[self.geo + l.side as usize] = ip(-1.0);
            out.extend(row);
        }
        Ok(Some(out))
    }
    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        Ok(Some(b[self.geo + 1].sub(b[self.geo])?))
    }
    fn data_scale(&self) -> Result<f64, CertError> {
        let mut m = 0.0_f64;
        for v in self.pts.iter().flatten() {
            m = f::total_max(m, v.abs())?;
        }
        Ok(m)
    }
    fn constraint_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        for l in self.row_labels() {
            let (d, _) = self.dist_iv(l.point as usize, b)?;
            let r = b[self.geo + l.side as usize];
            out.push(if l.side == 0 { r.sub(d)? } else { d.sub(r)? });
        }
        Ok(Some(out))
    }
    fn objective_gradient_enclosure(&self, _: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        Ok(Some(self.obj_grad().into_iter().map(ip).collect()))
    }
    fn constraint_gradient_enclosure(
        &self,
        set: &[RowLabel],
        b: &[Interval],
    ) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        for l in set {
            let (_, gd) = self.dist_iv(l.point as usize, b)?;
            out.extend(self.grad_row(l.side, &gd));
        }
        Ok(Some(out))
    }
}

/// A conic problem with one nonnegative row per label and a solution in which exactly the rows
/// of `set` have zero slack and positive multipliers (the extraction only classifies rows).
pub fn conic(all: &[RowLabel], set: &[RowLabel]) -> (ConicProblem, ConicSolution) {
    let m = all.len();
    let p = ConicProblemBuilder::new(vec![1.0], Csc::zeros(m, 1), vec![1.0; m], vec![Cone::Nonneg(m)]).build().unwrap();
    let (mut s, mut y) = (Vec::new(), Vec::new());
    for (i, l) in all.iter().enumerate() {
        if set.contains(l) {
            s.push(0.0);
            y.push(0.25);
        } else {
            s.push(0.5 + i as f64);
            y.push(0.0);
        }
    }
    let sol = ConicSolution {
        x: vec![0.0],
        y,
        s,
        objective: 0.0,
        iterations: 7,
        residuals: ConicResiduals { primal: 0.0, dual: 0.0, gap_abs: 0.0, gap_rel: 0.0 },
    };
    (p, sol)
}

pub fn proposal(all: &[RowLabel], set: &[RowLabel]) -> Proposal {
    let (p, sol) = conic(all, set);
    let th = Thresholds { slack_rel: 1e-9, mult_rel: 1e-9 };
    let pr = extract(&p, &sol, all, set.len(), &th).unwrap();
    assert_eq!(pr.uncertified_sets().len(), 1);
    pr
}

pub fn ulp(x: f64) -> f64 {
    let a = x.abs();
    f64::from_bits(a.to_bits() + 1) - a
}

/// The proven bound must contain the exact optimum; the reference is the exact optimum rounded
/// to nearest, so `|got - ref| <= bound + ulp(ref)`.
pub fn check_bound(name: &str, what: &str, got: f64, reference: f64, bound: f64) {
    let err = (got - reference).abs();
    assert!(bound.is_finite() && bound >= 0.0, "{name}: {what} bound {bound}");
    assert!(err <= bound + ulp(reference), "{name}: {what} error {err:e} outside proven bound {bound:e}");
}

pub fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits()).collect()
}
