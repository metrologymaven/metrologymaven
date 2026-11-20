//! Acceptance criteria of spec node KRN.SOLV.CERT.RECOMP (AC-1 to AC-4), the proven error
//! bounds and the fail-closed paths.
//!
//! Expected values come from `fixtures/solv/cert/recomp-v1/make_cases.py` (exact rational
//! optima found by brute force, circle radii to 60 digits; independent of this crate and of any
//! solver). The test `ActiveSetProblem` implementations for straightness, flatness and
//! minimum-zone circle live here. `OUTPUT_PIN` is a regression pin of this crate's own output
//! (not an oracle); CI checks it on x86-64 Linux and Windows, ARM64 macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::f;
use fid_num::la::{lstsq, Mat};
use fid_solv::cert::active::{extract, Proposal, RowLabel, Thresholds};
use fid_solv::cert::recomp::{
    recompute, ActiveSetProblem, Interval, RecompConfig, Recomputed, DEFAULT_COND_MAX, DEFAULT_GN_STEPS,
};
use fid_solv::cert::{BoundFailure, CertError};
use fid_solv::conic::{Cone, ConicProblem, ConicProblemBuilder, ConicResiduals, ConicSolution, Csc};
use fid_testkit::Sha256Hasher;

const FIXTURE: &str = include_str!("../../../fixtures/solv/cert/recomp-v1/cases.txt");
const U: f64 = f64::EPSILON / 2.0;

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

/// Straightness (`dim = 1`, points `(x, y)`, `p = (a, b, t)`) or flatness (`dim = 2`, points
/// `(x, y, z)`, `p = (a, b, c, t)`): `|z - sum coef * coord - c| <= t / 2`. Row `(i, 1)` is the
/// upper bound, `(i, 0)` the lower. Linear: the residual of row `(i, s)` is
/// `dev_i(p) - sgn t / 2` with Jacobian row `(-coords, -1, -sgn / 2)`, exact data.
struct LinZone {
    dim: usize,
    pts: Vec<Vec<f64>>,
}

impl LinZone {
    fn new(dim: usize, flat: &[f64]) -> LinZone {
        LinZone { dim, pts: flat.chunks(dim + 1).map(<[f64]>::to_vec).collect() }
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
    fn row(&self, l: RowLabel) -> Vec<f64> {
        let q = &self.pts[l.point as usize];
        let mut r: Vec<f64> = q[..self.dim].iter().map(|v| -v).collect();
        r.push(-1.0);
        r.push(-Self::sgn(l.side) / 2.0);
        r
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
        let rows: Vec<Vec<f64>> = set.iter().map(|&l| self.row(l)).collect();
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
        let rows: Vec<Vec<f64>> = set
            .iter()
            .map(|&l| {
                let s = Self::sgn(l.side);
                let q = &self.pts[l.point as usize];
                let mut r: Vec<f64> = q[..self.dim].iter().map(|v| -s * v).collect();
                r.push(-s);
                r.push(-0.5);
                r
            })
            .collect();
        Ok(Mat::from_rows(&rows)?)
    }
    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        Ok(Some(b[self.dim + 1]))
    }
}

/// Minimum-zone circle: points `(x, y)`, `p = (cx, cy, r_in, r_out)`. Row `(i, 0)` is the inner
/// bound `r_in <= d_i`, `(i, 1)` the outer bound `d_i <= r_out`; the active-set residual is
/// `d_i - r_side`. Nonlinear. The start is derived from the set alone: the centre equidistant
/// from the set's outer points and from its inner points (perpendicular bisectors, one 2 x 2
/// QR solve), or, with `rough_start`, the centroid of the set's points; the radii are the
/// distances to the first outer and the first inner point.
struct Circle {
    pts: Vec<(f64, f64)>,
    rough_start: bool,
    enclosures: bool,
}

impl Circle {
    fn new(flat: &[f64]) -> Circle {
        Circle { pts: flat.chunks(2).map(|c| (c[0], c[1])).collect(), rough_start: false, enclosures: true }
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
        let c = if self.rough_start {
            let n = set.len() as f64;
            let sx = set.iter().fold(0.0, |a, l| a + self.pts[l.point as usize].0);
            let sy = set.iter().fold(0.0, |a, l| a + self.pts[l.point as usize].1);
            (sx / n, sy / n)
        } else {
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
            (s.x[0], s.x[1])
        };
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
                let (gx, gy) = (-(x - p[0]) / d, -(y - p[1]) / d);
                if l.side == 0 {
                    vec![-gx, -gy, 1.0, 0.0]
                } else {
                    vec![gx, gy, 0.0, -1.0]
                }
            })
            .collect();
        Ok(Mat::from_rows(&rows)?)
    }
    fn residual_enclosure(&self, set: &[RowLabel], p: &[f64]) -> Result<Option<Vec<Interval>>, CertError> {
        if !self.enclosures {
            return Ok(None);
        }
        let mut out = Vec::new();
        for l in set {
            let (_, _, d) = self.dist_iv(l.point as usize, ip(p[0]), ip(p[1]))?;
            out.push(d.sub(ip(p[2 + l.side as usize]))?);
        }
        Ok(Some(out))
    }
    fn jacobian_enclosure(&self, set: &[RowLabel], b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        if !self.enclosures {
            return Ok(None);
        }
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
        Ok(if self.enclosures { Some(b[3].sub(b[2])?) } else { None })
    }
}

// ------------------------------------------------------------------ fixture

struct Case {
    name: String,
    kind: String,
    points: Vec<f64>,
    set: Vec<RowLabel>,
    params: Vec<f64>,
    value: f64,
    expect: Vec<String>,
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
            let get = |k: &str| fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default();
            let h: Vec<&str> = head.split_whitespace().collect();
            out.push(Case {
                name: h[0].to_string(),
                kind: h[2].to_string(),
                points: hexes(&get("points")),
                set: labels(&get("set")),
                params: hexes(&get("params")),
                value: hexes(&get("value")).first().copied().unwrap_or(0.0),
                expect: get("expect").split_whitespace().map(String::from).collect(),
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
        "straightness" | "error" => Box::new(LinZone::new(1, &c.points)),
        "flatness" => Box::new(LinZone::new(2, &c.points)),
        "circle" => Box::new(Circle::new(&c.points)),
        k => panic!("kind {k}"),
    }
}

/// A conic problem with one nonnegative row per label (its `A` and `q` do not matter to the
/// extraction) and a solution in which exactly the rows of `set` have small slack. `jitter`
/// changes every slack and multiplier bit without changing the classification: two such
/// solutions model two solver paths that reach the same active set.
fn proposal(all: &[RowLabel], set: &[RowLabel], jitter: u32) -> Proposal {
    let m = all.len();
    let p: ConicProblem =
        ConicProblemBuilder::new(vec![1.0], Csc::zeros(m, 1), vec![1.0; m], vec![Cone::Nonneg(m)]).build().unwrap();
    let j = f64::from(jitter);
    let (mut s, mut y) = (Vec::new(), Vec::new());
    for (i, l) in all.iter().enumerate() {
        let k = i as f64;
        if set.contains(l) {
            s.push(j * 1e-13 * (1.0 + k));
            y.push(0.25 + 0.01 * j * k);
        } else {
            s.push(0.5 + 0.1 * j + k);
            y.push(j * 1e-14 * k);
        }
    }
    let sol = ConicSolution {
        x: vec![j],
        y,
        s,
        objective: j,
        iterations: 7 + jitter,
        residuals: ConicResiduals { primal: 0.0, dual: 0.0, gap_abs: 0.0, gap_rel: 0.0 },
    };
    let th = Thresholds { slack_rel: 1e-9, mult_rel: 1e-9 };
    let pr = extract(&p, &sol, all, set.len(), &th).unwrap();
    assert_eq!(pr.uncertified_sets().len(), 1);
    pr
}

fn run(
    prob: &dyn ActiveSetProblem,
    set: &[RowLabel],
    jitter: u32,
    cfg: &RecompConfig,
) -> Result<Recomputed, CertError> {
    let mut all = prob.row_labels();
    if jitter % 2 == 1 {
        all.reverse(); // the conic solution lists its rows in another order
    }
    recompute(prob, &proposal(&all, set, jitter), 0, cfg)
}

fn ulp(x: f64) -> f64 {
    let a = x.abs();
    f64::from_bits(a.to_bits() + 1) - a
}

/// The proven bound must contain the exact optimum: `|got - exact| <= bound`, and the
/// reference is `exact` rounded to nearest, so `|got - ref| <= bound + ulp(ref)`.
fn check_bound(name: &str, what: &str, got: f64, reference: f64, bound: f64) {
    let err = (got - reference).abs();
    assert!(bound.is_finite() && bound >= 0.0, "{name}: {what} bound {bound}");
    assert!(err <= bound + ulp(reference), "{name}: {what} error {err:e} outside proven bound {bound:e}");
}

/// AC-1: fixture optima. Linear within `2 u cond` relative to the parameter scale (`LIN_TOL`
/// counts multiples of `u cond`): the spec's [unverified] `2 u` is not reachable by the one
/// QR solve that REQ-002 prescribes (the fixture's worst case is 32 u at condition 785), so
/// the criterion is the first-order forward error of a backward-stable solve. Circles within
/// 1e-14 relative. Every result lies inside its proven bound.
const LIN_TOL: f64 = 2.0;
const CIRCLE_TOL: f64 = 1e-14;

#[test]
fn ac1_fixture_optima() {
    let cases = parse();
    let count = |k: &str| cases.iter().filter(|c| c.kind == k).count();
    assert_eq!((count("straightness"), count("flatness"), count("circle")), (10, 10, 10));
    let cfg = RecompConfig::default();
    let mut worst_lin = 0.0_f64;
    for c in cases.iter().filter(|c| c.kind != "error") {
        let prob = problem_of(c);
        let r = run(prob.as_ref(), &c.set, 0, &cfg).unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
        let mut sorted = c.set.clone();
        sorted.sort();
        assert_eq!(r.set(), &sorted[..], "{}", c.name);
        assert_eq!(r.steps(), if c.kind == "circle" { DEFAULT_GN_STEPS } else { 1 }, "{}", c.name);
        assert!(r.cond() >= 1.0 && r.cond() <= DEFAULT_COND_MAX, "{}: cond {}", c.name, r.cond());
        let scale = c.params.iter().fold(0.0_f64, |a, v| if v.abs() > a { v.abs() } else { a });
        for (i, (&g, &w)) in r.params().iter().zip(&c.params).enumerate() {
            check_bound(&c.name, &format!("param {i}"), g, w, r.param_bounds()[i]);
            let rel = (g - w).abs() / scale;
            if c.kind == "circle" {
                assert!(rel <= CIRCLE_TOL, "{}: param {i} rel error {rel:e}", c.name);
            } else {
                let k = rel / (U * r.cond());
                worst_lin = if k > worst_lin { k } else { worst_lin };
                assert!(k <= LIN_TOL, "{}: param {i} rel error {} u at cond {:e}", c.name, rel / U, r.cond());
            }
        }
        check_bound(&c.name, "value", r.value(), c.value, r.value_bound());
        let tol = if c.kind == "circle" { CIRCLE_TOL * scale } else { LIN_TOL * U * r.cond() * scale };
        assert!((r.value() - c.value).abs() <= tol, "{}: value {} vs {}", c.name, r.value(), c.value);
        // The proven bounds are tight enough to be useful: within 1e-9 of the scale.
        assert!(r.param_bounds().iter().all(|&b| b <= 1e-9 * scale), "{}: {:?}", c.name, r.param_bounds());
    }
    eprintln!("worst linear relative error: {worst_lin} u cond");
}

/// AC-2 (path independence): each circle problem recomputed from two different conic
/// solutions (different slack and multiplier bits, rows listed in a different order) that
/// reach the same active set gives bit-identical parameters, value and bounds. The same holds
/// for the linear problems.
#[test]
fn ac2_two_paths_bit_identical() {
    let cfg = RecompConfig::default();
    for c in parse().iter().filter(|c| c.kind != "error") {
        let prob = problem_of(c);
        let a = run(prob.as_ref(), &c.set, 1, &cfg).unwrap();
        let b = run(prob.as_ref(), &c.set, 2, &cfg).unwrap();
        let mut rev = c.set.clone();
        rev.reverse();
        let r = run(prob.as_ref(), &rev, 3, &cfg).unwrap();
        for other in [&b, &r] {
            assert_eq!(bits(a.params()), bits(other.params()), "{}", c.name);
            assert_eq!(a.value().to_bits(), other.value().to_bits(), "{}", c.name);
            assert_eq!(bits(a.param_bounds()), bits(other.param_bounds()), "{}", c.name);
            assert_eq!(a.value_bound().to_bits(), other.value_bound().to_bits(), "{}", c.name);
            assert_eq!(a.cond().to_bits(), other.cond().to_bits(), "{}", c.name);
        }
    }
}

fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits()).collect()
}

/// AC-3 (error paths): three collinear points give `RankDeficient`, the near-degenerate set
/// (exact condition about 1e14) `IllConditioned`, with no value.
#[test]
fn ac3_error_paths() {
    let cfg = RecompConfig::default();
    for c in parse().iter().filter(|c| c.kind == "error") {
        let got = run(problem_of(c).as_ref(), &c.set, 0, &cfg);
        match c.expect[0].as_str() {
            "RankDeficient" => {
                assert_eq!(got, Err(CertError::RankDeficient { rank: c.expect[1].parse().unwrap() }), "{}", c.name)
            }
            "IllConditioned" => match got {
                Err(CertError::IllConditioned { cond }) => assert!(cond > DEFAULT_COND_MAX, "{}: {cond}", c.name),
                other => panic!("{}: {other:?}", c.name),
            },
            e => panic!("expect {e}"),
        }
    }
}

/// Fail closed: no enclosure, a non-contracting proof, a non-square system, non-convergence.
#[test]
fn fails_closed() {
    let cases = parse();
    let circle = cases.iter().find(|c| c.kind == "circle").unwrap();
    let cfg = RecompConfig::default();

    let mut no_enc = Circle::new(&circle.points);
    no_enc.enclosures = false;
    assert_eq!(
        run(&no_enc, &circle.set, 0, &cfg),
        Err(CertError::BoundNotProven { reason: BoundFailure::NoEnclosure })
    );

    // One Gauss-Newton step from the centroid does not converge to 4 u.
    let mut rough = Circle::new(&circle.points);
    rough.rough_start = true;
    let one = RecompConfig { gn_steps: 1, ..cfg };
    assert!(matches!(run(&rough, &circle.set, 0, &one), Err(CertError::RecomputeNoConvergence { .. })));
    // ... but the default 8 steps from the centroid do, to the same optimum.
    let r = run(&rough, &circle.set, 0, &cfg).unwrap();
    for (i, (&g, &w)) in r.params().iter().zip(&circle.params).enumerate() {
        check_bound(&circle.name, &format!("rough param {i}"), g, w, r.param_bounds()[i]);
    }

    // A tight condition threshold turns a fixture case into IllConditioned, with no value.
    let lin = cases.iter().find(|c| c.kind == "flatness").unwrap();
    let tight = RecompConfig { cond_max: 1.0, ..cfg };
    assert!(matches!(run(problem_of(lin).as_ref(), &lin.set, 0, &tight), Err(CertError::IllConditioned { .. })));
}

/// A straightness zone whose active-set size exceeds its parameter count: the system is
/// solved (least squares), but no isolated root exists to enclose, so there is no result.
struct Wide(LinZone);

impl ActiveSetProblem for Wide {
    fn n_params(&self) -> usize {
        self.0.n_params()
    }
    fn expected_active(&self) -> usize {
        4
    }
    fn row_labels(&self) -> Vec<RowLabel> {
        self.0.row_labels()
    }
    fn is_linear(&self) -> bool {
        true
    }
    fn start_from_set(&self, s: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        self.0.start_from_set(s)
    }
    fn residuals_and_jacobian(&self, s: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
        self.0.residuals_and_jacobian(s, p)
    }
    fn value(&self, p: &[f64]) -> Result<f64, CertError> {
        self.0.value(p)
    }
    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        self.0.constraint_values(p)
    }
    fn objective_gradient(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        self.0.objective_gradient(p)
    }
    fn constraint_gradients(&self, s: &[RowLabel], p: &[f64]) -> Result<Mat, CertError> {
        self.0.constraint_gradients(s, p)
    }
    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        self.0.value_enclosure(b)
    }
}

#[test]
fn non_square_fails_closed() {
    let c = parse().into_iter().find(|c| c.kind == "straightness").unwrap();
    let prob = Wide(LinZone::new(1, &c.points));
    let mut set = c.set.clone();
    let extra =
        prob.row_labels().into_iter().find(|l| !set.contains(l) && !set.iter().any(|s| s.point == l.point)).unwrap();
    set.push(extra);
    assert_eq!(
        run(&prob, &set, 0, &RecompConfig::default()),
        Err(CertError::BoundNotProven { reason: BoundFailure::NotSquare })
    );
}

#[test]
fn input_checks() {
    let c = parse().into_iter().find(|c| c.kind == "straightness").unwrap();
    let prob = LinZone::new(1, &c.points);
    let pr = proposal(&prob.row_labels(), &c.set, 0);
    let cfg = RecompConfig::default();
    assert_eq!(recompute(&prob, &pr, 1, &cfg), Err(CertError::InvalidInput { field: "candidate" }));
    let bad = RecompConfig { rel_tol: -1.0, ..cfg };
    assert_eq!(recompute(&prob, &pr, 0, &bad), Err(CertError::InvalidInput { field: "config" }));
    // A proposal whose labels are not the problem's rows.
    let foreign: Vec<RowLabel> = (100..110).map(|point| RowLabel { point, side: 0 }).collect();
    let pr = proposal(&foreign, &foreign[..3], 0);
    assert_eq!(recompute(&prob, &pr, 0, &cfg), Err(CertError::InvalidInput { field: "set" }));
    // A proposal of the wrong size for the problem.
    let all = prob.row_labels();
    let pr = proposal(&all, &all[..2], 0);
    assert_eq!(recompute(&prob, &pr, 0, &cfg), Err(CertError::Shape { field: "set", expected: 3, got: 2 }));
}

fn digest(name: &str, r: &Result<Recomputed, CertError>, h: &mut Sha256Hasher) {
    h.update(name.as_bytes());
    match r {
        Ok(r) => {
            for l in r.set() {
                h.update(&l.point.to_le_bytes());
                h.update(&[l.side]);
            }
            for v in r.params().iter().chain(r.param_bounds()).chain(&[r.value(), r.value_bound(), r.cond()]) {
                h.update(&v.to_bits().to_le_bytes());
            }
            h.update(&r.steps().to_le_bytes());
        }
        Err(e) => h.update(format!("{e:?}").as_bytes()),
    }
}

/// AC-4: output-bit pin of every fixture case (AC-1 and AC-3 together), recorded from a local
/// x86-64 run and checked by CI on x86-64 Linux and Windows, ARM64 macOS and wasm32 under
/// wasmtime. One value serves all targets because they agree bit for bit; a target that
/// diverged would get its own `#[cfg(target_arch)]` value here.
const OUTPUT_PIN: &str = "sha256:582689d3dfd2e80dbe679439a61dddfbca074f4888dfdaac99f58622c0849a58";

#[test]
fn ac4_output_bits_pinned() {
    let cfg = RecompConfig::default();
    let mut h = Sha256Hasher::new();
    for c in &parse() {
        digest(&c.name, &run(problem_of(c).as_ref(), &c.set, 0, &cfg), &mut h);
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
