//! Acceptance criteria of spec node KRN.SOLV.CERT.KKT (AC-1 to AC-4), the three-valued
//! verdicts and the fail-closed paths.
//!
//! Expected values come from `fixtures/solv/cert/kkt-v1/make_cases.py` (exact rational KKT
//! quantities; independent of this crate and of any solver). The test `ActiveSetProblem`
//! implementations for straightness, flatness and minimum-zone circle live here, with the
//! enclosure methods the proven check needs. `OUTPUT_PIN` is a regression pin of this crate's
//! own output (not an oracle); CI checks it on x86-64 Linux and Windows, ARM64 macOS and
//! wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::f;
use fid_num::la::{lstsq, Mat};
use fid_solv::cert::active::{extract, Proposal, RowLabel, Thresholds};
use fid_solv::cert::kkt::{check, Condition, KktFail, KktReport, KktStatus, KktTolerances, Verdict};
use fid_solv::cert::recomp::{recompute, ActiveSetProblem, Interval, RecompConfig, Recomputed};
use fid_solv::cert::{BoundFailure, CertError};
use fid_solv::conic::{Cone, ConicProblem, ConicProblemBuilder, ConicResiduals, ConicSolution, Csc};
use fid_testkit::Sha256Hasher;

const FIXTURE: &str = include_str!("../../../fixtures/solv/cert/kkt-v1/cases.txt");

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

// ------------------------------------------------------------------ test problems

/// Straightness (`dim = 1`) or flatness (`dim = 2`), as in `cert_recomp.rs`, with the KKT
/// enclosures. Row `(i, 1)`: `g = dev_i - t/2`; row `(i, 0)`: `g = -dev_i - t/2`. Flags:
/// `enclosures = false` provides none; `degenerate` makes every constraint gradient of a set
/// equal to the first one (rank 1); `wide_grad` widens the gradient enclosures by 1e3.
struct LinZone {
    dim: usize,
    pts: Vec<Vec<f64>>,
    enclosures: bool,
    degenerate: bool,
    wide_grad: bool,
}

impl LinZone {
    fn new(dim: usize, flat: &[f64]) -> LinZone {
        LinZone {
            dim,
            pts: flat.chunks(dim + 1).map(<[f64]>::to_vec).collect(),
            enclosures: true,
            degenerate: false,
            wide_grad: false,
        }
    }
    fn sgn(side: u8) -> f64 {
        if side == 1 {
            1.0
        } else {
            -1.0
        }
    }
    fn dev(&self, i: usize, p: &[f64]) -> f64 {
        let q = &self.pts[i];
        let mut d = q[self.dim];
        for (pk, qk) in p.iter().zip(&q[..self.dim]) {
            d -= pk * qk;
        }
        d - p[self.dim]
    }
    fn grad_row(&self, l: RowLabel) -> Vec<f64> {
        let s = Self::sgn(l.side);
        let q = &self.pts[l.point as usize];
        let mut r: Vec<f64> = q[..self.dim].iter().map(|v| -s * v).collect();
        r.push(-s);
        r.push(-0.5);
        r
    }
    fn grad_rows(&self, set: &[RowLabel]) -> Vec<Vec<f64>> {
        set.iter().map(|&l| self.grad_row(if self.degenerate { set[0] } else { l })).collect()
    }
}

impl ActiveSetProblem for LinZone {
    fn n_params(&self) -> usize {
        self.dim + 2
    }
    fn expected_active(&self) -> usize {
        self.dim + 2
    }
    fn row_labels(&self) -> Vec<RowLabel> {
        (0..self.pts.len() as u32).flat_map(|point| [0, 1].map(|side| RowLabel { point, side })).collect()
    }
    fn is_linear(&self) -> bool {
        true
    }
    fn start_from_set(&self, _: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        Err(CertError::Callback { reason: "linear problem has no start" })
    }
    fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
        let t = p[self.dim + 1];
        let r = set.iter().map(|l| self.dev(l.point as usize, p) - Self::sgn(l.side) * t / 2.0).collect();
        let rows: Vec<Vec<f64>> = set
            .iter()
            .map(|&l| {
                let q = &self.pts[l.point as usize];
                let mut r: Vec<f64> = q[..self.dim].iter().map(|v| -v).collect();
                r.push(-1.0);
                r.push(-Self::sgn(l.side) / 2.0);
                r
            })
            .collect();
        Ok((r, Mat::from_rows(&rows)?))
    }
    fn value(&self, p: &[f64]) -> Result<f64, CertError> {
        Ok(p[self.dim + 1])
    }
    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        let t = p[self.dim + 1];
        Ok(self.row_labels().iter().map(|l| Self::sgn(l.side) * self.dev(l.point as usize, p) - t / 2.0).collect())
    }
    fn objective_gradient(&self, _: &[f64]) -> Result<Vec<f64>, CertError> {
        let mut g = vec![0.0; self.dim + 2];
        g[self.dim + 1] = 1.0;
        Ok(g)
    }
    fn constraint_gradients(&self, set: &[RowLabel], _: &[f64]) -> Result<Mat, CertError> {
        Ok(Mat::from_rows(&self.grad_rows(set))?)
    }
    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        Ok(Some(b[self.dim + 1]))
    }
    fn constraint_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        if !self.enclosures {
            return Ok(None);
        }
        let mut out = Vec::new();
        for l in self.row_labels() {
            let q = &self.pts[l.point as usize];
            let mut dev = ip(q[self.dim]);
            for (bk, &qk) in b.iter().zip(&q[..self.dim]) {
                dev = dev.sub(bk.mul(ip(qk))?)?;
            }
            dev = dev.sub(b[self.dim])?;
            let sdev = if l.side == 1 { dev } else { dev.neg() };
            out.push(sdev.sub(b[self.dim + 1].mul(ip(0.5))?)?);
        }
        Ok(Some(out))
    }
    fn objective_gradient_enclosure(&self, _: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        if !self.enclosures {
            return Ok(None);
        }
        Ok(Some(self.objective_gradient(&[])?.into_iter().map(ip).collect()))
    }
    fn constraint_gradient_enclosure(
        &self,
        set: &[RowLabel],
        _: &[Interval],
    ) -> Result<Option<Vec<Interval>>, CertError> {
        if !self.enclosures {
            return Ok(None);
        }
        let w = if self.wide_grad { 1e3 } else { 0.0 };
        Ok(Some(self.grad_rows(set).into_iter().flatten().map(|v| Interval::new(v - w, v + w).unwrap()).collect()))
    }
}

/// Minimum-zone circle, as in `cert_recomp.rs`, with the KKT enclosures. Row `(i, 0)`:
/// `g = r_in - d_i`; row `(i, 1)`: `g = d_i - r_out`; `f = r_out - r_in`.
struct Circle {
    pts: Vec<(f64, f64)>,
}

impl Circle {
    fn new(flat: &[f64]) -> Circle {
        Circle { pts: flat.chunks(2).map(|c| (c[0], c[1])).collect() }
    }
    fn dist(&self, i: usize, c: (f64, f64)) -> f64 {
        let (x, y) = self.pts[i];
        let (dx, dy) = (x - c.0, y - c.1);
        f::sqrt(dx * dx + dy * dy).unwrap()
    }
    fn dist_iv(&self, i: usize, cx: Interval, cy: Interval) -> Result<(Interval, Interval, Interval), CertError> {
        let (x, y) = self.pts[i];
        let dx = ip(x).sub(cx)?;
        let dy = ip(y).sub(cy)?;
        Ok((dx, dy, dx.sqr()?.add(dy.sqr()?)?.sqrt()?))
    }
}

impl ActiveSetProblem for Circle {
    fn n_params(&self) -> usize {
        4
    }
    fn expected_active(&self) -> usize {
        4
    }
    fn row_labels(&self) -> Vec<RowLabel> {
        (0..self.pts.len() as u32).flat_map(|point| [0, 1].map(|side| RowLabel { point, side })).collect()
    }
    fn is_linear(&self) -> bool {
        false
    }
    fn start_from_set(&self, set: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        let outer: Vec<usize> = set.iter().filter(|l| l.side == 1).map(|l| l.point as usize).collect();
        let inner: Vec<usize> = set.iter().filter(|l| l.side == 0).map(|l| l.point as usize).collect();
        if outer.is_empty() || inner.is_empty() {
            return Err(CertError::Callback { reason: "circle set needs an inner and an outer point" });
        }
        let mut rows = Vec::new();
        let mut rhs = Vec::new();
        for g in [&outer, &inner] {
            let (x0, y0) = self.pts[g[0]];
            for &k in &g[1..] {
                let (x1, y1) = self.pts[k];
                rows.push(vec![2.0 * (x1 - x0), 2.0 * (y1 - y0)]);
                rhs.push((x1 * x1 + y1 * y1) - (x0 * x0 + y0 * y0));
            }
        }
        let s = lstsq(&Mat::from_rows(&rows)?, &rhs, 0.0)?;
        let c = (s.x[0], s.x[1]);
        Ok(vec![c.0, c.1, self.dist(inner[0], c), self.dist(outer[0], c)])
    }
    fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
        let mut r = Vec::new();
        let mut rows = Vec::new();
        for l in set {
            let i = l.point as usize;
            let d = self.dist(i, (p[0], p[1]));
            let (x, y) = self.pts[i];
            r.push(d - p[2 + l.side as usize]);
            let mut row = vec![-(x - p[0]) / d, -(y - p[1]) / d, 0.0, 0.0];
            row[2 + l.side as usize] = -1.0;
            rows.push(row);
        }
        Ok((r, Mat::from_rows(&rows)?))
    }
    fn value(&self, p: &[f64]) -> Result<f64, CertError> {
        Ok(p[3] - p[2])
    }
    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        Ok(self
            .row_labels()
            .iter()
            .map(|l| {
                let d = self.dist(l.point as usize, (p[0], p[1]));
                if l.side == 0 {
                    p[2] - d
                } else {
                    d - p[3]
                }
            })
            .collect())
    }
    fn objective_gradient(&self, _: &[f64]) -> Result<Vec<f64>, CertError> {
        Ok(vec![0.0, 0.0, -1.0, 1.0])
    }
    fn constraint_gradients(&self, set: &[RowLabel], p: &[f64]) -> Result<Mat, CertError> {
        let rows: Vec<Vec<f64>> = set
            .iter()
            .map(|l| {
                let i = l.point as usize;
                let d = self.dist(i, (p[0], p[1]));
                let (x, y) = self.pts[i];
                let (ux, uy) = ((x - p[0]) / d, (y - p[1]) / d);
                if l.side == 0 {
                    vec![ux, uy, 1.0, 0.0]
                } else {
                    vec![-ux, -uy, 0.0, -1.0]
                }
            })
            .collect();
        Ok(Mat::from_rows(&rows)?)
    }
    fn residual_enclosure(&self, set: &[RowLabel], p: &[f64]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        for l in set {
            let (_, _, d) = self.dist_iv(l.point as usize, ip(p[0]), ip(p[1]))?;
            out.push(d.sub(ip(p[2 + l.side as usize]))?);
        }
        Ok(Some(out))
    }
    fn jacobian_enclosure(&self, set: &[RowLabel], b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        for l in set {
            let (dx, dy, d) = self.dist_iv(l.point as usize, b[0], b[1])?;
            out.push(dx.div(d)?.neg());
            out.push(dy.div(d)?.neg());
            out.push(ip(if l.side == 0 { -1.0 } else { 0.0 }));
            out.push(ip(if l.side == 1 { -1.0 } else { 0.0 }));
        }
        Ok(Some(out))
    }
    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        Ok(Some(b[3].sub(b[2])?))
    }
    fn constraint_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        for l in self.row_labels() {
            let (_, _, d) = self.dist_iv(l.point as usize, b[0], b[1])?;
            out.push(if l.side == 0 { b[2].sub(d)? } else { d.sub(b[3])? });
        }
        Ok(Some(out))
    }
    fn objective_gradient_enclosure(&self, _: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        Ok(Some(vec![ip(0.0), ip(0.0), ip(-1.0), ip(1.0)]))
    }
    fn constraint_gradient_enclosure(
        &self,
        set: &[RowLabel],
        b: &[Interval],
    ) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        for l in set {
            let (dx, dy, d) = self.dist_iv(l.point as usize, b[0], b[1])?;
            let (ux, uy) = (dx.div(d)?, dy.div(d)?);
            if l.side == 0 {
                out.extend([ux, uy, ip(1.0), ip(0.0)]);
            } else {
                out.extend([ux.neg(), uy.neg(), ip(0.0), ip(-1.0)]);
            }
        }
        Ok(Some(out))
    }
}

// ------------------------------------------------------------------ fixture

struct Case {
    name: String,
    kind: String,
    points: Vec<f64>,
    set: Vec<RowLabel>,
    tol: KktTolerances,
    data_scale: f64,
    multipliers: Vec<f64>,
    primal: f64,
    dual: f64,
    complementarity: f64,
    verdicts: Vec<Verdict>,
    expect: String,
}

fn parse() -> Vec<Case> {
    let mut out = Vec::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut head = String::new();
    for l in FIXTURE.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if let Some(rest) = l.strip_prefix("case ") {
            head = rest.to_string();
            fields.clear();
        } else if l == "end" {
            let get = |k: &str| fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap();
            let h: Vec<&str> = head.split_whitespace().collect();
            let t = hexes(&get("tol"));
            out.push(Case {
                name: h[0].to_string(),
                kind: h[2].to_string(),
                points: hexes(&get("points")),
                set: labels(&get("set")),
                tol: KktTolerances { tol_p: t[0], tol_d: t[1], tol_s: t[2], tol_c: t[3], ..KktTolerances::default() },
                data_scale: hexes(&get("data_scale"))[0],
                multipliers: hexes(&get("multipliers")),
                primal: hexes(&get("primal"))[0],
                dual: hexes(&get("dual"))[0],
                complementarity: hexes(&get("complementarity"))[0],
                verdicts: get("verdicts")
                    .split_whitespace()
                    .map(|v| match v {
                        "S" => Verdict::Satisfied,
                        "V" => Verdict::Violated,
                        o => panic!("verdict {o}"),
                    })
                    .collect(),
                expect: get("expect"),
            });
        } else {
            let (k, v) = l.split_once(' ').unwrap_or((l, ""));
            fields.push((k.to_string(), v.to_string()));
        }
    }
    out
}

fn problem_of(c: &Case) -> Box<dyn ActiveSetProblem> {
    match c.kind.as_str() {
        "straightness" => Box::new(LinZone::new(1, &c.points)),
        "flatness" => Box::new(LinZone::new(2, &c.points)),
        "circle" => Box::new(Circle::new(&c.points)),
        k => panic!("kind {k}"),
    }
}

/// A conic problem with one nonnegative row per label and a solution in which exactly the rows
/// of `set` have small slack (as in `cert_recomp.rs`), so the set reaches the recomputation as
/// the only candidate of an uncertified proposal.
fn proposal(all: &[RowLabel], set: &[RowLabel]) -> Proposal {
    let m = all.len();
    let p: ConicProblem =
        ConicProblemBuilder::new(vec![1.0], Csc::zeros(m, 1), vec![1.0; m], vec![Cone::Nonneg(m)]).build().unwrap();
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
    let th = Thresholds { slack_rel: 1e-9, mult_rel: 1e-9 };
    let pr = extract(&p, &sol, all, set.len(), &th).unwrap();
    assert_eq!(pr.uncertified_sets().len(), 1);
    pr
}

fn recomputed(prob: &dyn ActiveSetProblem, set: &[RowLabel]) -> Recomputed {
    recompute(prob, &proposal(&prob.row_labels(), set), 0, &RecompConfig::default()).unwrap()
}

fn run(prob: &dyn ActiveSetProblem, c: &Case) -> Result<KktReport, CertError> {
    check(prob, &recomputed(prob, &c.set), &c.tol, c.data_scale)
}

fn ulp(x: f64) -> f64 {
    let a = x.abs();
    f64::from_bits(a.to_bits() + 1) - a
}

/// The proven enclosure must contain the exact value; the reference is the exact value rounded
/// to nearest, so one ulp of slack.
fn contains(name: &str, what: &str, c: Condition, reference: f64) {
    let slack = ulp(reference);
    assert!(
        c.lo() - slack <= reference && reference <= c.hi() + slack,
        "{name}: {what} exact {reference:e} outside proven [{:e}, {:e}]",
        c.lo(),
        c.hi()
    );
}

fn expected_status(e: &str) -> (KktStatus, Option<KktFail>) {
    match e {
        "certified" => (KktStatus::Certified, None),
        "violated primal" => (KktStatus::Violated, Some(KktFail::Primal)),
        "violated dual" => (KktStatus::Violated, Some(KktFail::Dual)),
        o => panic!("expect {o}"),
    }
}

/// AC-1: optimal sets pass with multipliers within 1e-12 (relative to the largest) of the exact
/// ones; non-optimal sets fail with the expected condition; every proven enclosure holds the
/// exact value.
#[test]
fn ac1_fixture() {
    let cases = parse();
    let count = |e: &str| cases.iter().filter(|c| c.expect == e).count();
    assert_eq!((count("certified"), count("violated primal"), count("violated dual")), (30, 24, 24));
    for c in &cases {
        let prob = problem_of(c);
        let r = run(prob.as_ref(), c).unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
        let (status, reason) = expected_status(&c.expect);
        assert_eq!((r.status(), r.reason()), (status, reason), "{}", c.name);
        assert_eq!(r.pass(), status == KktStatus::Certified, "{}", c.name);
        let got = [r.primal().verdict(), r.dual().verdict(), r.stationarity().verdict(), r.complementarity().verdict()];
        assert_eq!(got.to_vec(), c.verdicts, "{}", c.name);

        let scale = c.multipliers.iter().fold(0.0_f64, |a, v| if v.abs() > a { v.abs() } else { a });
        assert_eq!(r.multipliers().len(), c.multipliers.len(), "{}", c.name);
        for (i, (&g, &w)) in r.multipliers().iter().zip(&c.multipliers).enumerate() {
            assert!((g - w).abs() <= 1e-12 * scale, "{}: multiplier {i} {g} vs {w}", c.name);
            let b = r.multiplier_bounds()[i];
            assert!((g - w).abs() <= b + ulp(w), "{}: multiplier {i} outside proven bound {b:e}", c.name);
            assert!(b <= 1e-9 * scale, "{}: multiplier bound {b:e}", c.name);
        }
        contains(&c.name, "primal", r.primal(), c.primal);
        contains(&c.name, "dual", r.dual(), c.dual);
        contains(&c.name, "complementarity", r.complementarity(), c.complementarity);
        assert_eq!(r.scales().data, c.data_scale);
        assert_eq!(r.tolerances(), c.tol);
    }
}

/// AC-2: an active point that should have been inactive (multiplier -0.2): `pass = false`, dual
/// residual 0.2 recorded; the vertex itself is feasible.
#[test]
fn ac2_negative_multiplier() {
    let c = parse().into_iter().find(|c| c.name == "ac2-negative-multiplier").unwrap();
    let r = run(problem_of(&c).as_ref(), &c).unwrap();
    assert!(!r.pass());
    assert_eq!((r.status(), r.reason()), (KktStatus::Violated, Some(KktFail::Dual)));
    assert_eq!(r.dual().verdict(), Verdict::Violated);
    assert_eq!(r.primal().verdict(), Verdict::Satisfied);
    assert!((r.dual().lo() - 0.2).abs() < 1e-12 && (r.dual().hi() - 0.2).abs() < 1e-12, "{:?}", r.dual());
    assert!((r.multipliers()[0] + 0.2).abs() < 1e-12);
}

/// AC-3: an inactive row violated by 1e-6 with tol_p = 1e-12: `pass = false` on primal
/// feasibility.
#[test]
fn ac3_inactive_violation() {
    let c = parse().into_iter().find(|c| c.name == "ac3-inactive-violation").unwrap();
    assert_eq!(c.tol.tol_p, 1e-12);
    let r = run(problem_of(&c).as_ref(), &c).unwrap();
    assert!(!r.pass());
    assert_eq!((r.status(), r.reason()), (KktStatus::Violated, Some(KktFail::Primal)));
    assert!(r.primal().lo() > 0.9e-6 && r.primal().hi() < 1.1e-6, "{:?}", r.primal());
    assert_eq!(r.dual().verdict(), Verdict::Satisfied);
}

fn first(kind: &str) -> Case {
    parse().into_iter().find(|c| c.kind == kind && c.expect == "certified").unwrap()
}

/// Missing enclosures fail closed with `BoundNotProven { NoEnclosure }`.
#[test]
fn missing_enclosures_fail_closed() {
    let c = first("flatness");
    let full = LinZone::new(2, &c.points);
    let rec = recomputed(&full, &c.set);
    let mut bare = LinZone::new(2, &c.points);
    bare.enclosures = false;
    assert_eq!(
        check(&bare, &rec, &c.tol, c.data_scale),
        Err(CertError::BoundNotProven { reason: BoundFailure::NoEnclosure })
    );
}

/// REQ-004: rank-deficient active gradients give `pass = false`, reason
/// `DegenerateGradients`, no multipliers, and an undecidable (not passed) dual condition.
#[test]
fn degenerate_gradients() {
    let c = first("straightness");
    let mut prob = LinZone::new(1, &c.points);
    let rec = recomputed(&prob, &c.set);
    prob.degenerate = true;
    let r = check(&prob, &rec, &c.tol, c.data_scale).unwrap();
    assert!(!r.pass());
    assert_eq!((r.status(), r.reason()), (KktStatus::Undecidable, Some(KktFail::DegenerateGradients)));
    assert!(r.multipliers().is_empty() && r.multiplier_bounds().is_empty());
    assert_eq!(r.dual().verdict(), Verdict::Undecidable);
    assert_eq!(r.dual().hi(), f64::INFINITY);
    assert_eq!(r.primal().verdict(), Verdict::Satisfied);
}

/// Gradient enclosures too wide to prove the multipliers unique: undecidable, never a pass.
#[test]
fn unproven_multipliers_are_undecidable() {
    let c = first("straightness");
    let mut prob = LinZone::new(1, &c.points);
    let rec = recomputed(&prob, &c.set);
    prob.wide_grad = true;
    let r = check(&prob, &rec, &c.tol, c.data_scale).unwrap();
    assert!(!r.pass());
    assert_eq!((r.status(), r.reason()), (KktStatus::Undecidable, Some(KktFail::MultipliersNotProven)));
    assert_eq!(r.multipliers().len(), 3);
    assert!(r.multiplier_bounds().iter().all(|b| *b == f64::INFINITY));
}

/// Zero tolerances: the exact optimum lies on the active constraints, so the proven
/// enclosures of primal feasibility and complementarity straddle the zero threshold and an
/// optimal set is never rounded into a pass. (Stationarity at the float multipliers may be
/// certainly violated at tolerance 0, since they are not the exact ones: e.g. `flat-08`.) A
/// certain violation wins over undecidable conditions.
#[test]
fn borderline_is_never_a_pass() {
    let zero = KktTolerances { tol_p: 0.0, tol_d: 0.0, tol_s: 0.0, tol_c: 0.0, ..KktTolerances::default() };
    let mut undecidable = 0;
    for c in parse().iter().filter(|c| c.kind != "circle") {
        let prob = problem_of(c);
        let r = check(prob.as_ref(), &recomputed(prob.as_ref(), &c.set), &zero, c.data_scale).unwrap();
        assert_ne!(r.status(), KktStatus::Certified, "{}", c.name);
        assert!(!r.pass(), "{}", c.name);
        assert_eq!(r.complementarity().verdict(), Verdict::Undecidable, "{}", c.name);
        if c.expect == "certified" {
            assert_eq!(r.primal().verdict(), Verdict::Undecidable, "{}", c.name);
            assert_eq!(r.dual().verdict(), Verdict::Satisfied, "{}", c.name);
            if r.status() == KktStatus::Undecidable {
                assert_eq!(r.reason(), Some(KktFail::Primal), "{}", c.name);
                undecidable += 1;
            } else {
                assert_eq!(r.reason(), Some(KktFail::Stationarity), "{}", c.name);
            }
        } else {
            assert_eq!(r.status(), KktStatus::Violated, "{}", c.name);
        }
    }
    assert!(undecidable > 0);
}

#[test]
fn input_checks() {
    let c = first("straightness");
    let prob = LinZone::new(1, &c.points);
    let rec = recomputed(&prob, &c.set);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(check(&prob, &rec, &c.tol, bad), Err(CertError::InvalidInput { field: "data_scale" }));
    }
    let bad = KktTolerances { tol_s: f64::NAN, ..c.tol };
    assert_eq!(check(&prob, &rec, &bad, c.data_scale), Err(CertError::InvalidInput { field: "tolerances" }));
    let bad = KktTolerances { tol_p: f64::MAX, ..c.tol };
    assert_eq!(check(&prob, &rec, &bad, 4.0), Err(CertError::InvalidInput { field: "tolerances" }));
    // A recomputation whose set names rows this (smaller) problem does not have.
    assert!(rec.set().iter().any(|l| l.point >= 2));
    let other = LinZone::new(1, &c.points[..4]);
    assert_eq!(check(&other, &rec, &c.tol, c.data_scale), Err(CertError::InvalidInput { field: "set" }));
}

fn put(h: &mut Sha256Hasher, c: Condition) {
    for v in [c.lo(), c.hi(), c.threshold()] {
        h.update(&v.to_bits().to_le_bytes());
    }
    h.update(format!("{:?}", c.verdict()).as_bytes());
}

/// AC-4: output-bit pin of every fixture report, recorded from a local x86-64 run and checked
/// by CI on x86-64 Linux and Windows, ARM64 macOS and wasm32 under wasmtime. One value serves
/// all targets: REQ-KRN.SOLV.CERT.KKT-005 requires them to agree bit for bit, so a target that
/// diverges is a failure, not a new pin.
const OUTPUT_PIN: &str = "sha256:763b7d096eebabba03da85e75e079b7db720a4e4290061605c04bc994ad297b5";

#[test]
fn ac4_output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for c in &parse() {
        h.update(c.name.as_bytes());
        match run(problem_of(c).as_ref(), c) {
            Ok(r) => {
                h.update(format!("{:?} {:?}", r.status(), r.reason()).as_bytes());
                for v in r.multipliers().iter().chain(r.multiplier_bounds()) {
                    h.update(&v.to_bits().to_le_bytes());
                }
                for cond in [r.primal(), r.dual(), r.stationarity(), r.complementarity()] {
                    put(&mut h, cond);
                }
                let s = r.scales();
                for v in [s.data, s.multiplier, s.gradient] {
                    h.update(&v.to_bits().to_le_bytes());
                }
            }
            Err(e) => h.update(format!("{e:?}").as_bytes()),
        }
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
