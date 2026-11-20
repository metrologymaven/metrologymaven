//! Acceptance criteria of spec node KRN.SOLV.SEQ (AC-1 to AC-6) and the fail-closed paths.
//!
//! Expected values come from `fixtures/solv/seq/ref-v1/make_cases.py` (exact rational
//! exhaustive contact-set search for the circles, a 90-digit Newton polish with a strict
//! local-optimality check for the cylinders; independent of this crate, of any solver and of
//! the platform libm). The test `LinearisableProblem` implementations (minimum-zone circle,
//! maximum-inscribed circle and minimum-zone cylinder axis, with every enclosure the
//! certificate needs) live here, not in `KRN.MZ` or `KRN.ENV`. `OUTPUT_PIN` is a regression
//! pin of this crate's own output (not an oracle); CI checks it on x86-64 Linux and Windows,
//! ARM64 macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::f;
use fid_num::la::{lstsq, Mat};
use fid_solv::cert::recomp::{ActiveSetProblem, Interval};
use fid_solv::cert::{CertError, CertKind, Certificate, RowLabel};
use fid_solv::conic::{Cone, ConicProblem, ConicProblemBuilder, Csc};
use fid_solv::mstart::{generate_starts, StartSpec, StartSummary};
use fid_solv::seq::{solve, solve_multi, LinearisableProblem, SeqCause, SeqConfig, SeqOutcome, SolverError};
use fid_testkit::Sha256Hasher;

const FIXTURE: &str = include_str!("../../../fixtures/solv/seq/ref-v1/cases.txt");

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| f64::from_bits(u64::from_str_radix(t, 16).unwrap())).collect()
}

fn labels(s: &str) -> Vec<RowLabel> {
    s.split_whitespace()
        .map(|t| {
            let (p, sd) = t.split_once(':').unwrap();
            RowLabel { point: p.parse().unwrap(), side: sd.parse().unwrap() }
        })
        .collect()
}

fn ip(x: f64) -> Interval {
    Interval::point(x).unwrap()
}

// ------------------------------------------------------------------ double-double

/// An unevaluated sum `hi + lo` (double-double), built from error-free transformations with
/// IEEE `+`, `-`, `*` only (no FMA). The residuals of the active-set equations are evaluated in
/// it, so the recomputation's Gauss-Newton steps reach the representation limit of the
/// parameters instead of the rounding noise of a plain `f64` distance.
#[derive(Debug, Clone, Copy)]
struct Dd {
    hi: f64,
    lo: f64,
}

fn two_sum(a: f64, b: f64) -> Dd {
    let s = a + b;
    let bb = s - a;
    Dd { hi: s, lo: (a - (s - bb)) + (b - bb) }
}

fn quick(a: f64, b: f64) -> Dd {
    let s = a + b;
    Dd { hi: s, lo: b - (s - a) }
}

fn split(a: f64) -> (f64, f64) {
    let c = 134_217_729.0 * a;
    let hi = c - (c - a);
    (hi, a - hi)
}

fn two_prod(a: f64, b: f64) -> Dd {
    let p = a * b;
    let ((ah, al), (bh, bl)) = (split(a), split(b));
    Dd { hi: p, lo: ((ah * bh - p) + ah * bl + al * bh) + al * bl }
}

impl Dd {
    fn of(x: f64) -> Dd {
        Dd { hi: x, lo: 0.0 }
    }
    fn add(self, o: Dd) -> Dd {
        let s = two_sum(self.hi, o.hi);
        quick(s.hi, s.lo + self.lo + o.lo)
    }
    fn neg(self) -> Dd {
        Dd { hi: -self.hi, lo: -self.lo }
    }
    fn sub(self, o: Dd) -> Dd {
        self.add(o.neg())
    }
    fn mul(self, o: Dd) -> Dd {
        let p = two_prod(self.hi, o.hi);
        quick(p.hi, p.lo + self.hi * o.lo + self.lo * o.hi)
    }
    fn div(self, o: Dd) -> Dd {
        let q1 = self.hi / o.hi;
        let r = self.sub(o.mul(Dd::of(q1)));
        quick(q1, r.hi / o.hi)
    }
    fn sqrt(self) -> Dd {
        let s = f::sqrt(self.hi).unwrap();
        let r = self.sub(two_prod(s, s));
        quick(s, r.hi / (2.0 * s))
    }
}

// ------------------------------------------------------------------ test problems

#[derive(Debug, Clone, Copy, PartialEq)]
enum Family {
    /// Minimum-zone circle: p = (cx, cy, r_in, r_out).
    Mzc,
    /// Maximum-inscribed circle: p = (cx, cy, r), value -r.
    Mic,
    /// Minimum-zone cylinder axis: p = (x0, y0, a, b, r_in, r_out), axis through (x0, y0, 0)
    /// with direction (a, b, 1).
    Cyl,
}

/// One test problem. Rows: (i, 0) `g = r_in - d_i`, (i, 1) `g = d_i - r_out` (MIC: only
/// (i, 0) with `g = r - d_i`), point by point. `objective` (the merit the driver minimises)
/// is the zone width at the centre or axis with the radii eliminated (`max d - min d`; MIC:
/// `-min d`). `nominal` is the cylinder axis the recomputation's Gauss-Newton start uses
/// (problem data: the fixture's least-squares axis, or for the twin the planted direction,
/// mirrored for a set in the mirrored half).
#[derive(Debug, Clone)]
struct Prob {
    family: Family,
    pts: Vec<[f64; 3]>,
    nominal: [f64; 4],
    mirror_half: Option<usize>,
    region: Option<Vec<(f64, f64)>>,
    enclosures: bool,
    infeasible: bool,
}

impl Prob {
    fn new(family: Family, flat: &[f64]) -> Prob {
        let pts = match family {
            Family::Cyl => flat.chunks(3).map(|c| [c[0], c[1], c[2]]).collect(),
            _ => flat.chunks(2).map(|c| [c[0], c[1], 0.0]).collect(),
        };
        Prob { family, pts, nominal: [0.0; 4], mirror_half: None, region: None, enclosures: true, infeasible: false }
    }
    /// Number of geometric parameters (before the radii).
    fn geo(&self) -> usize {
        if self.family == Family::Cyl {
            4
        } else {
            2
        }
    }
    fn radius_index(&self, side: u8) -> usize {
        match self.family {
            Family::Mic => 2,
            _ => self.geo() + side as usize,
        }
    }
    /// Distance of point `i` and its gradient in the geometric parameters.
    fn dist(&self, i: usize, p: &[f64]) -> (f64, Vec<f64>) {
        let [x, y, z] = self.pts[i];
        match self.family {
            Family::Cyl => {
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
            _ => {
                let (dx, dy) = (x - p[0], y - p[1]);
                let d = f::sqrt(dx * dx + dy * dy).unwrap();
                (d, vec![-dx / d, -dy / d])
            }
        }
    }
    /// The distance of point `i` in double-double (same formulas as [`Prob::dist`]).
    fn dist_dd(&self, i: usize, p: &[f64]) -> Dd {
        let [x, y, z] = self.pts[i];
        let (wx, wy) = (two_sum(x, -p[0]), two_sum(y, -p[1]));
        match self.family {
            Family::Cyl => {
                let (a, b, wz) = (Dd::of(p[2]), Dd::of(p[3]), Dd::of(z));
                let cx = wy.sub(wz.mul(b));
                let cy = wz.mul(a).sub(wx);
                let cz = wx.mul(b).sub(wy.mul(a));
                let nc2 = cx.mul(cx).add(cy.mul(cy)).add(cz.mul(cz));
                let vv = a.mul(a).add(b.mul(b)).add(Dd::of(1.0));
                nc2.div(vv).sqrt()
            }
            _ => wx.mul(wx).add(wy.mul(wy)).sqrt(),
        }
    }
    /// Interval distance of point `i` and its gradient over the box `b` of geometric
    /// parameters.
    fn dist_iv(&self, i: usize, b: &[Interval]) -> Result<(Interval, Vec<Interval>), CertError> {
        let [x, y, z] = self.pts[i];
        match self.family {
            Family::Cyl => {
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
            _ => {
                let dx = ip(x).sub(b[0])?;
                let dy = ip(y).sub(b[1])?;
                let d = dx.sqr()?.add(dy.sqr()?)?.sqrt()?;
                Ok((d, vec![dx.div(d)?.neg(), dy.div(d)?.neg()]))
            }
        }
    }
    fn sides(&self) -> &'static [u8] {
        if self.family == Family::Mic {
            &[0]
        } else {
            &[0, 1]
        }
    }
    /// Gradient row of constraint `l` from the distance gradient.
    fn grad_row(&self, l: RowLabel, gd: &[f64]) -> Vec<f64> {
        let mut row = vec![0.0; self.n_params()];
        for (k, g) in gd.iter().enumerate() {
            row[k] = if l.side == 0 { -g } else { *g };
        }
        row[self.radius_index(l.side)] = if l.side == 0 { 1.0 } else { -1.0 };
        row
    }
    fn grad_row_iv(&self, l: RowLabel, gd: &[Interval]) -> Vec<Interval> {
        let mut row = vec![ip(0.0); self.n_params()];
        for (k, g) in gd.iter().enumerate() {
            row[k] = if l.side == 0 { g.neg() } else { *g };
        }
        row[self.radius_index(l.side)] = ip(if l.side == 0 { 1.0 } else { -1.0 });
        row
    }
    fn obj_grad(&self) -> Vec<f64> {
        let mut g = vec![0.0; self.n_params()];
        match self.family {
            Family::Mic => g[2] = -1.0,
            _ => {
                g[self.geo()] = -1.0;
                g[self.geo() + 1] = 1.0;
            }
        }
        g
    }
}

impl ActiveSetProblem for Prob {
    fn n_params(&self) -> usize {
        match self.family {
            Family::Mzc => 4,
            Family::Mic => 3,
            Family::Cyl => 6,
        }
    }
    fn expected_active(&self) -> usize {
        self.n_params()
    }
    fn row_labels(&self) -> Vec<RowLabel> {
        let sides = self.sides();
        (0..self.pts.len() as u32).flat_map(|point| sides.iter().map(move |&side| RowLabel { point, side })).collect()
    }
    fn is_linear(&self) -> bool {
        false
    }
    fn start_from_set(&self, set: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        let first = |side: u8| set.iter().find(|l| l.side == side).map(|l| l.point as usize);
        if self.family == Family::Cyl {
            let mut axis = self.nominal.to_vec();
            if let Some(half) = self.mirror_half {
                if set.iter().all(|l| l.point as usize >= half) {
                    axis[0] = -axis[0];
                    axis[2] = -axis[2];
                }
            }
            let (Some(i), Some(o)) = (first(0), first(1)) else {
                return Err(CertError::Callback { reason: "cylinder set needs an inner and an outer point" });
            };
            let (ri, ro) = (self.dist(i, &axis).0, self.dist(o, &axis).0);
            axis.extend([ri, ro]);
            return Ok(axis);
        }
        // Circles: the centre from the perpendicular bisectors within each group of equal
        // radius (least squares), then the radii at that centre.
        let mut rows = Vec::new();
        let mut rhs = Vec::new();
        for side in self.sides() {
            let group: Vec<usize> = set.iter().filter(|l| l.side == *side).map(|l| l.point as usize).collect();
            if let Some((&g0, rest)) = group.split_first() {
                let [x0, y0, _] = self.pts[g0];
                for &k in rest {
                    let [x1, y1, _] = self.pts[k];
                    rows.push(vec![2.0 * (x1 - x0), 2.0 * (y1 - y0)]);
                    rhs.push((x1 * x1 + y1 * y1) - (x0 * x0 + y0 * y0));
                }
            }
        }
        let s = lstsq(&Mat::from_rows(&rows)?, &rhs, 0.0)?;
        let c = [s.x[0], s.x[1]];
        let mut p = c.to_vec();
        for &side in self.sides() {
            let Some(i) = first(side) else {
                return Err(CertError::Callback { reason: "circle set needs an inner and an outer point" });
            };
            p.push(self.dist(i, &c).0);
        }
        Ok(p)
    }
    fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
        let mut r = Vec::new();
        let mut rows = Vec::new();
        for l in set {
            let (_, gd) = self.dist(l.point as usize, p);
            let d = self.dist_dd(l.point as usize, p);
            r.push(d.sub(Dd::of(p[self.radius_index(l.side)])).hi);
            let mut row = vec![0.0; self.n_params()];
            row[..gd.len()].copy_from_slice(&gd);
            row[self.radius_index(l.side)] = -1.0;
            rows.push(row);
        }
        Ok((r, Mat::from_rows(&rows)?))
    }
    fn value(&self, p: &[f64]) -> Result<f64, CertError> {
        Ok(match self.family {
            Family::Mic => -p[2],
            _ => p[self.geo() + 1] - p[self.geo()],
        })
    }
    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        Ok(self
            .row_labels()
            .iter()
            .map(|l| {
                let d = self.dist(l.point as usize, p).0;
                let r = p[self.radius_index(l.side)];
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
        let rows: Vec<Vec<f64>> = set.iter().map(|&l| self.grad_row(l, &self.dist(l.point as usize, p).1)).collect();
        Ok(Mat::from_rows(&rows)?)
    }
    fn residual_enclosure(&self, set: &[RowLabel], p: &[f64]) -> Result<Option<Vec<Interval>>, CertError> {
        if !self.enclosures {
            return Ok(None);
        }
        let b: Vec<Interval> = p.iter().map(|&x| ip(x)).collect();
        let mut out = Vec::new();
        for l in set {
            let (d, _) = self.dist_iv(l.point as usize, &b)?;
            out.push(d.sub(b[self.radius_index(l.side)])?);
        }
        Ok(Some(out))
    }
    fn jacobian_enclosure(&self, set: &[RowLabel], b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        if !self.enclosures {
            return Ok(None);
        }
        let mut out = Vec::new();
        for l in set {
            let (_, gd) = self.dist_iv(l.point as usize, b)?;
            let mut row = vec![ip(0.0); self.n_params()];
            row[..gd.len()].copy_from_slice(&gd);
            row[self.radius_index(l.side)] = ip(-1.0);
            out.extend(row);
        }
        Ok(Some(out))
    }
    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        if !self.enclosures {
            return Ok(None);
        }
        Ok(Some(match self.family {
            Family::Mic => b[2].neg(),
            _ => b[self.geo() + 1].sub(b[self.geo()])?,
        }))
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
            let r = b[self.radius_index(l.side)];
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
        for &l in set {
            let (_, gd) = self.dist_iv(l.point as usize, b)?;
            out.extend(self.grad_row_iv(l, &gd));
        }
        Ok(Some(out))
    }
}

/// A dense row-major matrix as CSC, dropping exact zeros.
fn csc(rows: &[Vec<f64>], n: usize) -> Csc {
    let mut colptr = vec![0];
    let (mut rowval, mut nzval) = (Vec::new(), Vec::new());
    for j in 0..n {
        for (i, r) in rows.iter().enumerate() {
            if r[j] != 0.0 {
                rowval.push(i);
                nzval.push(r[j]);
            }
        }
        colptr.push(rowval.len());
    }
    Csc::new(rows.len(), n, colptr, rowval, nzval)
}

impl LinearisableProblem for Prob {
    fn scales(&self) -> Vec<f64> {
        let mut s = vec![1.0; self.geo()];
        if self.family == Family::Cyl {
            s[2] = 0.125;
            s[3] = 0.125;
        }
        s.resize(self.n_params(), 1024.0);
        s
    }
    /// The linearisation `g(p) + G(p) dp <= 0`, i.e. `G dp + s = -g(p)`, and the model
    /// `value(p) + grad f' dp`.
    fn local_problem(&self, p: &[f64]) -> Result<(ConicProblem, f64), CertError> {
        let labels = self.row_labels();
        let g = self.constraint_values(p)?;
        let gm = self.constraint_gradients(&labels, p)?;
        let mut rows: Vec<Vec<f64>> = (0..labels.len()).map(|i| gm.row(i).unwrap().to_vec()).collect();
        let mut b: Vec<f64> = g.iter().map(|v| -v).collect();
        if self.infeasible {
            // Test-only: the first row demands dp_0 <= -1e6, beyond any step bound.
            rows[0] = vec![0.0; self.n_params()];
            rows[0][0] = 1.0;
            b[0] = -1e6;
        }
        let n = self.n_params();
        let prob = ConicProblemBuilder::new(self.obj_grad(), csc(&rows, n), b, vec![Cone::Nonneg(labels.len())])
            .build()
            .map_err(|_| CertError::Callback { reason: "local problem" })?;
        Ok((prob, self.value(p)?))
    }
    fn objective(&self, p: &[f64]) -> Result<f64, CertError> {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for i in 0..self.pts.len() {
            let d = self.dist(i, p).0;
            lo = f::total_min(lo, d)?;
            hi = f::total_max(hi, d)?;
        }
        Ok(if self.family == Family::Mic { -lo } else { hi - lo })
    }
    fn outer_region(&self) -> Option<Vec<(f64, f64)>> {
        self.region.clone()
    }
}

// ------------------------------------------------------------------ fixture

#[derive(Debug, Clone)]
struct Case {
    name: String,
    family: String,
    pts: Vec<f64>,
    start: Vec<f64>,
    set: Vec<RowLabel>,
    params: Vec<f64>,
    value: f64,
    value_digits: String,
    set2: Vec<RowLabel>,
    params2: Vec<f64>,
    value2: f64,
}

fn parse() -> Vec<Case> {
    let mut out = Vec::new();
    let mut cur: Option<Case> = None;
    for line in FIXTURE.lines().filter(|l| !l.starts_with('#')) {
        let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
        if key == "case" {
            let t: Vec<&str> = rest.split_whitespace().collect();
            cur = Some(Case {
                name: t[0].to_string(),
                family: t[2].to_string(),
                pts: Vec::new(),
                start: Vec::new(),
                set: Vec::new(),
                params: Vec::new(),
                value: 0.0,
                value_digits: String::new(),
                set2: Vec::new(),
                params2: Vec::new(),
                value2: 0.0,
            });
            continue;
        }
        let c = cur.as_mut().unwrap();
        match key {
            "points" => c.pts = hexes(rest),
            "start" => c.start = hexes(rest),
            "set" => c.set = labels(rest),
            "params" => c.params = hexes(rest),
            "value" => {
                let (h, d) = rest.split_once(' ').unwrap();
                c.value = hexes(h)[0];
                c.value_digits = d.to_string();
            }
            "set2" => c.set2 = labels(rest),
            "params2" => c.params2 = hexes(rest),
            "value2" => c.value2 = hexes(rest.split_once(' ').unwrap().0)[0],
            "end" => out.push(cur.take().unwrap()),
            _ => panic!("unknown fixture line {line}"),
        }
    }
    out
}

fn problem_of(c: &Case) -> Prob {
    let family = match c.family.as_str() {
        "mzc" => Family::Mzc,
        "mic" => Family::Mic,
        _ => Family::Cyl,
    };
    let mut p = Prob::new(family, &c.pts);
    if family == Family::Cyl {
        if c.family == "twin" {
            p.nominal = [0.0, 0.0, 1.0, 0.0];
            p.mirror_half = Some(c.pts.len() / 3 / 2);
        } else {
            p.nominal = [c.start[0], c.start[1], c.start[2], c.start[3]];
        }
    }
    p
}

fn kind_of(p: &Prob) -> CertKind {
    if p.family == Family::Mic {
        CertKind::Envelope
    } else {
        CertKind::Zone
    }
}

fn config(p: &Prob) -> SeqConfig {
    SeqConfig::new(kind_of(p))
}

fn reference_cases() -> Vec<Case> {
    parse().into_iter().filter(|c| c.family != "twin").collect()
}

/// `|got - want| <= rel * |want|`.
fn close(got: f64, want: f64, rel: f64) -> bool {
    (got - want).abs() <= rel * want.abs()
}

fn check_reference(c: &Case, out: &SeqOutcome) {
    let cert = out.certified.certificate();
    assert_eq!(
        cert.active_set(),
        c.set.as_slice(),
        "{}: active set (value {:e} vs {:e})",
        c.name,
        out.certified.value(),
        c.value
    );
    let v = out.certified.value();
    assert!(close(v, c.value, 1e-12), "{}: value {v:e} vs {:e}", c.name, c.value);
    // The proven bound of the certificate encloses the reference (up to its rounding).
    assert!((v - c.value).abs() <= cert.value_bound() + c.value.abs() * f64::EPSILON, "{}: value bound", c.name);
    let scale = cert.kkt().scales.data;
    for (k, (got, want)) in out.certified.params().iter().zip(&c.params).enumerate() {
        assert!((got - want).abs() <= 1e-9 * scale, "{}: param {k} {got:e} vs {want:e}", c.name);
    }
    assert!(!out.on_trust_boundary);
    assert!(!cert.flags().local_optimum_only);
}

fn solve_case(c: &Case) -> SeqOutcome {
    let p = problem_of(c);
    match solve(&p, &c.start, &config(&p)) {
        Ok(o) => o,
        Err(e) => panic!("{}: {e}", c.name),
    }
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_reference_optima() {
    let cases = reference_cases();
    assert_eq!(cases.len(), 60);
    for c in &cases {
        // The 50-digit reference and its nearest double agree.
        assert_eq!(c.value_digits.parse::<f64>().unwrap().to_bits(), c.value.to_bits(), "{}", c.name);
        let out = solve_case(c);
        check_reference(c, &out);
        assert!(out.iterations as usize == out.history.len() && out.iterations >= 4, "{}", c.name);
    }
}

// ------------------------------------------------------------------ AC-2

/// Two further seeded starts per problem, perturbed around the least-squares start (geometry
/// only; the radii are recomputed so the start is feasible).
fn seeded_starts(c: &Case, p: &Prob) -> Vec<Vec<f64>> {
    let geo = p.geo();
    let mut half = vec![0.0; p.n_params()];
    for (j, h) in half.iter_mut().enumerate().take(geo) {
        *h = if p.family == Family::Cyl && j >= 2 { 0.002 } else { 0.05 };
    }
    let spec = StartSpec {
        primary: c.start.clone(),
        half_widths: half,
        count: 3,
        seed: 0x5e9,
        stream: 7,
        tolerances: vec![0.0; p.n_params()],
    };
    generate_starts(&spec)
        .unwrap()
        .into_iter()
        .skip(1)
        .map(|mut s| {
            let ds: Vec<f64> = (0..p.pts.len()).map(|i| p.dist(i, &s).0).collect();
            let lo = ds.iter().fold(f64::INFINITY, |a, &d| f::total_min(a, d).unwrap());
            let hi = ds.iter().fold(f64::NEG_INFINITY, |a, &d| f::total_max(a, d).unwrap());
            s[geo] = lo;
            if p.family != Family::Mic {
                s[geo + 1] = hi;
            }
            s
        })
        .collect()
}

#[test]
fn ac2_two_starts_bit_identical() {
    for c in &reference_cases() {
        let p = problem_of(c);
        let base = solve_case(c);
        for (k, s) in seeded_starts(c, &p).iter().enumerate() {
            assert_ne!(s, &c.start);
            let out = solve(&p, s, &config(&p)).unwrap_or_else(|e| panic!("{} start {k}: {e}", c.name));
            let (a, b) = (base.certified.certificate(), out.certified.certificate());
            assert_eq!(a.active_set(), b.active_set(), "{} start {k}", c.name);
            assert_eq!(out.certified.value().to_bits(), base.certified.value().to_bits(), "{} start {k}", c.name);
            assert_eq!(a.digest(), b.digest(), "{} start {k}", c.name);
            assert_eq!(a.to_canonical_json().unwrap(), b.to_canonical_json().unwrap());
        }
        // A different initial radius changes the iterate path, not the certificate.
        let cfg = SeqConfig { radius_0: 0.25, ..config(&p) };
        let out = solve(&p, &c.start, &cfg).unwrap();
        assert_eq!(out.certified.certificate().digest(), base.certified.certificate().digest(), "{}", c.name);
    }
}

// ------------------------------------------------------------------ AC-3

fn bits_of(xs: &[f64]) -> String {
    xs.iter().map(|v| format!("0x{:016x}", v.to_bits())).collect::<Vec<_>>().join(",")
}

fn labels_of(set: &[RowLabel]) -> String {
    set.iter().map(|l| format!("{}:{}", l.point, l.side)).collect::<Vec<_>>().join(",")
}

/// A stable encoding of a driver error: codes and fields, never `Debug` output.
fn encode_err(e: &SolverError) -> String {
    let mut out = format!(
        "{};it={};radius={};set={};otb={}",
        e.cause.code(),
        e.iterations,
        bits_of(&[e.radius]),
        labels_of(&e.active_set),
        e.on_trust_boundary
    );
    match &e.cause {
        SeqCause::Cert(c) | SeqCause::Callback(c) => {
            out.push_str(&format!(";cert={}", c.code()));
            match c {
                CertError::CertificateFailed { tried, first_error, .. } => {
                    out.push_str(&format!(";tried={tried}"));
                    if let Some(fe) = first_error {
                        out.push_str(&format!(";first={}", fe.code()));
                    }
                }
                CertError::ActiveSetUndetermined { strongly_active, ambiguous, .. } => {
                    out.push_str(&format!(";strong={};amb={}", labels_of(strongly_active), labels_of(ambiguous)));
                }
                _ => {}
            }
        }
        SeqCause::InvalidInput { field } => out.push_str(&format!(";field={field}")),
        _ => {}
    }
    out
}

fn first_case(family: &str) -> Case {
    parse().into_iter().find(|c| c.family == family).unwrap()
}

fn ac3_errors() -> Vec<SolverError> {
    let c = first_case("mzc");
    let p = problem_of(&c);
    let mut errs = Vec::new();

    let bad = Prob { infeasible: true, ..p.clone() };
    let e = solve(&bad, &c.start, &config(&bad)).unwrap_err();
    assert_eq!(e.cause, SeqCause::SubproblemInfeasible, "{e}");
    assert_eq!((e.iterations, e.active_set.as_slice()), (0, &[][..]));
    errs.push(e);

    let e = solve(&p, &c.start, &SeqConfig { max_iter: 2, ..config(&p) }).unwrap_err();
    assert_eq!(e.cause, SeqCause::IterationLimit, "{e}");
    assert_eq!(e.iterations, 2);
    assert!(!e.active_set.is_empty());
    errs.push(e);

    let e = solve(&p, &c.start, &SeqConfig { radius_min: 2.0, ..config(&p) }).unwrap_err();
    assert_eq!(e.cause, SeqCause::StepBoundCollapsed, "{e}");
    assert_eq!((e.iterations, e.radius), (0, 1.0));
    errs.push(e);
    errs
}

#[test]
fn ac3_error_paths() {
    let errs = ac3_errors();
    assert_eq!(errs.len(), 3);
}

#[test]
fn certificate_failure_is_a_failure() {
    // Fail closed: without the enclosures the bound cannot be proven, so certification fails
    // and the converged iterate is not returned.
    let c = first_case("mzc");
    let p = Prob { enclosures: false, ..problem_of(&c) };
    let e = solve(&p, &c.start, &config(&p)).unwrap_err();
    match &e.cause {
        SeqCause::Cert(CertError::CertificateFailed { first_error: Some(fe), .. }) => {
            assert!(matches!(**fe, CertError::BoundNotProven { .. }), "{e}");
        }
        _ => panic!("{e}"),
    }
    assert_eq!(e.active_set, c.set);
}

#[test]
fn input_checks() {
    let c = first_case("mzc");
    let p = problem_of(&c);
    let field = |r: Result<SeqOutcome, SolverError>| match r {
        Err(SolverError { cause: SeqCause::InvalidInput { field }, .. }) => field,
        other => panic!("{other:?}"),
    };
    assert_eq!(field(solve(&p, &c.start[..3], &config(&p))), "start");
    assert_eq!(field(solve(&p, &[f64::NAN, 0.0, 1.0, 2.0], &config(&p))), "start");
    assert_eq!(field(solve(&p, &c.start, &SeqConfig { max_iter: 0, ..config(&p) })), "max_iter");
    let boxed = Prob { region: Some(vec![(0.0, 1.0); 3]), ..p.clone() };
    assert_eq!(field(solve(&boxed, &c.start, &config(&p))), "outer_region");
    let outside = Prob { region: Some(vec![(1e9, 2e9); 4]), ..p.clone() };
    assert_eq!(field(solve(&outside, &c.start, &config(&p))), "start");
    let nan = Prob { region: Some(vec![(f64::NAN, 1.0); 4]), ..p };
    assert_eq!(field(solve(&nan, &c.start, &config(&nan))), "outer_region");
}

// ------------------------------------------------------------------ AC-4

/// The MIC centre region: a box on the centre's x coordinate with `hi` (or `lo`) at `edge`,
/// on the side of the start.
fn mic_boxed(c: &Case, edge: f64) -> Prob {
    let inf = f64::INFINITY;
    let x = if c.start[0] < c.params[0] { (-inf, edge) } else { (edge, inf) };
    Prob { region: Some(vec![x, (-inf, inf), (-inf, inf)]), ..problem_of(c) }
}

fn ac4_outcomes() -> (SolverError, SeqOutcome) {
    let c = first_case("mic");
    assert_ne!(c.start[0], c.params[0]);
    // The true centre lies outside the box: the run converges onto the box edge, where the
    // unconstrained problem has no KKT point, so certification fails closed with the flag.
    let mid = 0.5 * (c.start[0] + c.params[0]);
    let p = mic_boxed(&c, mid);
    let e = solve(&p, &c.start, &config(&p)).unwrap_err();
    assert!(e.on_trust_boundary, "{e}");
    assert!(matches!(e.cause, SeqCause::Cert(_)), "{e}");
    // The box edge passes through the true centre: certified, and on the boundary.
    let p = mic_boxed(&c, c.params[0]);
    let out = solve(&p, &c.start, &config(&p)).unwrap();
    assert!(out.on_trust_boundary);
    let free = solve_case(&c);
    assert_eq!(out.certified.certificate().digest(), free.certified.certificate().digest());
    (e, out)
}

#[test]
fn ac4_outer_region() {
    let (_, out) = ac4_outcomes();
    assert_eq!(out.certified.certificate().active_set(), first_case("mic").set.as_slice());
    // A wide box: not on the boundary.
    let c = first_case("mic");
    let wide = mic_boxed(&c, if c.start[0] < c.params[0] { c.params[0] + 1.0 } else { c.params[0] - 1.0 });
    let out = solve(&wide, &c.start, &config(&wide)).unwrap();
    assert!(!out.on_trust_boundary);
}

// ------------------------------------------------------------------ AC-5

fn twin_spec() -> StartSpec {
    let r = 10.0;
    StartSpec {
        primary: vec![0.0, 0.0, 0.5, 0.0, 0.5 * r, 1.5 * r],
        half_widths: vec![0.25, 0.25, 1.25, 0.125, 0.0, 0.0],
        count: 8,
        seed: 2026,
        stream: 5,
        tolerances: vec![1e-6; 6],
    }
}

fn ac5_twin() -> SeqOutcome {
    let c = first_case("twin");
    let p = problem_of(&c);
    let out = solve_multi(&p, &twin_spec(), &config(&p)).unwrap();
    assert!(out.local_optimum_only);
    assert!(!out.agreement);
    let cert = out.best.certified.certificate();
    assert!(cert.flags().local_optimum_only);
    // Every converged start reached one of the two reference optima, and both were reached.
    let (mut first, mut second) = (0, 0);
    for s in &out.per_start {
        if let StartSummary::Converged { objective, params } = s {
            let (want, wp) = if params[2] > 0.0 { (c.value, &c.params) } else { (c.value2, &c.params2) };
            assert!(close(*objective, want, 1e-12), "{objective:e} vs {want:e}");
            for (g, w) in params.iter().zip(wp) {
                assert!((g - w).abs() <= 1e-9 * 20.0, "{g:e} vs {w:e}");
            }
            if params[2] > 0.0 {
                first += 1;
            } else {
                second += 1;
            }
        }
    }
    assert!(first > 0 && second > 0, "{first} / {second}");
    let set = cert.active_set();
    assert!(set == c.set.as_slice() || set == c.set2.as_slice());
    out.best
}

#[test]
fn ac5_multi_start() {
    let best = ac5_twin();
    // Only the flag and the digest changed: the flagged certificate parses and re-serialises.
    let bytes = best.certified.certificate().to_canonical_json().unwrap();
    assert_eq!(Certificate::parse(&bytes).unwrap(), *best.certified.certificate());

    // All starts agree: the flag stays false and the certificate equals the single run's.
    let c = first_case("mzcyl");
    let p = problem_of(&c);
    let spec = StartSpec {
        primary: c.start.clone(),
        half_widths: vec![0.01, 0.01, 0.0005, 0.0005, 0.0, 0.0],
        count: 8,
        seed: 11,
        stream: 3,
        tolerances: vec![0.0; 6],
    };
    let out = solve_multi(&p, &spec, &config(&p)).unwrap();
    assert!(!out.local_optimum_only && out.agreement);
    assert!(out.per_start.iter().all(|s| matches!(s, StartSummary::Converged { .. })));
    let single = solve_case(&c);
    assert_eq!(out.best.certified.certificate(), single.certified.certificate());
    assert!(!out.best.certified.certificate().flags().local_optimum_only);
}

// ------------------------------------------------------------------ AC-6 output pin

/// Output-bit pin: regression pin of this crate's own output (every AC-1 certificate's bytes,
/// the AC-5 flagged certificate, then the AC-3 and AC-4 errors in the stable encoding of
/// `encode_err`), recorded from a local x86-64 run and checked by CI on every target. One value
/// serves all targets because they agree bit for bit; a target that diverged would get its own
/// `#[cfg(target_arch)]` value here.
const OUTPUT_PIN: &str = "sha256:0a9e00d2fb0ba5391845b46ac8aef57edd2704738936f0635badb3fc4388554e";

#[test]
fn ac6_output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for c in &reference_cases() {
        let out = solve_case(c);
        h.update(&out.certified.certificate().to_canonical_json().unwrap());
    }
    h.update(&ac5_twin().certified.certificate().to_canonical_json().unwrap());
    for e in ac3_errors() {
        h.update(encode_err(&e).as_bytes());
    }
    let (e, out) = ac4_outcomes();
    h.update(encode_err(&e).as_bytes());
    h.update(&out.certified.certificate().to_canonical_json().unwrap());
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
