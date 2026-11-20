//! Acceptance criteria of spec node KRN.SOLV.CERT.CT (AC-1 to AC-5), the fail-closed paths
//! and the wire form.
//!
//! Expected values come from `fixtures/solv/cert/ct-v1/make_cases.py` (exact rational optima,
//! multipliers and KKT verdicts found by brute force, and an independently written and hashed
//! golden certificate; independent of this crate, of any solver and of the platform libm).
//! The test `ActiveSetProblem` implementations (minimum-zone and envelope straightness and
//! flatness, with every enclosure and the data scale) live here. `OUTPUT_PIN` is a regression
//! pin of this crate's own output (not an oracle); CI checks it on x86-64 Linux and Windows,
//! ARM64 macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::la::Mat;
use fid_solv::cert::active::{extract, RowLabel, Thresholds};
use fid_solv::cert::ct::{ALGORITHM, SCHEMA};
use fid_solv::cert::kkt::{check, KktFail, KktStatus, KktTolerances};
use fid_solv::cert::recomp::{recompute, ActiveSetProblem, Interval, RecompConfig};
use fid_solv::cert::{certify, CertConfig, CertError, CertKind, Certificate, Certified};
use fid_solv::conic::{solve_lp, Cone, ConicProblem, ConicProblemBuilder, ConicResiduals, ConicSolution, Csc};
use fid_testkit::Sha256Hasher;

const FIXTURE: &str = include_str!("../../../fixtures/solv/cert/ct-v1/cases.txt");
const GOLDEN: &[u8] = include_bytes!("../../../fixtures/solv/cert/ct-v1/golden-cert.json");

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| f64::from_bits(u64::from_str_radix(t, 16).unwrap())).collect()
}

fn ints(s: &str) -> Vec<i64> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
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

/// Minimum-zone (`zone`) or envelope (`!zone`) straightness (`dim = 1`) or flatness
/// (`dim = 2`). Parameters `(a, [b,] c, t)`, `dev_i = z_i - a x_i [- b y_i] - c`. Rows in the
/// order (0, 0), (0, 1), (1, 0), ...: zone `(i, 1)`: `g = dev_i - t/2`, `(i, 0)`:
/// `g = -dev_i - t/2`; envelope `(i, 1)`: `g = dev_i`, `(i, 0)`: `g = -dev_i - t`. Every `g`
/// is affine, `g = A_row p - b_row`, the conic form solved below. `f = t`. The data scale is
/// the largest absolute coordinate (the units of `g`).
#[derive(Clone)]
struct Lin {
    zone: bool,
    dim: usize,
    pts: Vec<Vec<f64>>,
    scale_unset: bool,
}

impl Lin {
    fn new(zone: bool, dim: usize, flat: &[f64]) -> Lin {
        Lin { zone, dim, pts: flat.chunks(dim + 1).map(<[f64]>::to_vec).collect(), scale_unset: false }
    }
    fn n(&self) -> usize {
        self.dim + 2
    }
    fn labels(&self) -> Vec<RowLabel> {
        (0..self.pts.len() as u32).flat_map(|point| [0, 1].map(|side| RowLabel { point, side })).collect()
    }
    fn a_row(&self, l: RowLabel) -> Vec<f64> {
        let q = &self.pts[l.point as usize][..self.dim];
        let upper = l.side == 1;
        let mut r: Vec<f64> = q.iter().map(|&v| if upper { -v } else { v }).collect();
        r.push(if upper { -1.0 } else { 1.0 });
        r.push(match (self.zone, upper) {
            (true, _) => -0.5,
            (false, true) => 0.0,
            (false, false) => -1.0,
        });
        r
    }
    fn b_row(&self, l: RowLabel) -> f64 {
        let z = self.pts[l.point as usize][self.dim];
        if l.side == 1 {
            -z
        } else {
            z
        }
    }
    fn g(&self, l: RowLabel, p: &[f64]) -> f64 {
        let mut acc = 0.0;
        for (a, x) in self.a_row(l).iter().zip(p) {
            acc += a * x;
        }
        acc - self.b_row(l)
    }
    fn conic(&self) -> ConicProblem {
        let rows: Vec<Vec<f64>> = self.labels().iter().map(|&l| self.a_row(l)).collect();
        let b: Vec<f64> = self.labels().iter().map(|&l| self.b_row(l)).collect();
        conic_from(&rows, &b, self.n())
    }
    fn kind(&self) -> CertKind {
        if self.zone {
            CertKind::Zone
        } else {
            CertKind::Envelope
        }
    }
}

impl ActiveSetProblem for Lin {
    fn n_params(&self) -> usize {
        self.n()
    }
    fn expected_active(&self) -> usize {
        self.n()
    }
    fn row_labels(&self) -> Vec<RowLabel> {
        self.labels()
    }
    fn is_linear(&self) -> bool {
        true
    }
    fn start_from_set(&self, _: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        Err(CertError::Callback { reason: "linear problem has no start" })
    }
    fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
        let r = set.iter().map(|&l| self.g(l, p)).collect();
        let rows: Vec<Vec<f64>> = set.iter().map(|&l| self.a_row(l)).collect();
        Ok((r, Mat::from_rows(&rows)?))
    }
    fn value(&self, p: &[f64]) -> Result<f64, CertError> {
        Ok(p[self.n() - 1])
    }
    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        Ok(self.labels().iter().map(|&l| self.g(l, p)).collect())
    }
    fn objective_gradient(&self, _: &[f64]) -> Result<Vec<f64>, CertError> {
        let mut g = vec![0.0; self.n()];
        g[self.n() - 1] = 1.0;
        Ok(g)
    }
    fn constraint_gradients(&self, set: &[RowLabel], _: &[f64]) -> Result<Mat, CertError> {
        let rows: Vec<Vec<f64>> = set.iter().map(|&l| self.a_row(l)).collect();
        Ok(Mat::from_rows(&rows)?)
    }
    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        Ok(Some(b[self.n() - 1]))
    }
    fn data_scale(&self) -> Result<f64, CertError> {
        if self.scale_unset {
            return Err(CertError::InvalidInput { field: "data_scale_unset" });
        }
        Ok(self.pts.iter().flatten().fold(0.0, |a: f64, v| if v.abs() > a { v.abs() } else { a }))
    }
    fn constraint_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        for l in self.labels() {
            let mut acc = ip(0.0);
            for (&a, bk) in self.a_row(l).iter().zip(b) {
                acc = acc.add(ip(a).mul(*bk)?)?;
            }
            out.push(acc.sub(ip(self.b_row(l)))?);
        }
        Ok(Some(out))
    }
    fn objective_gradient_enclosure(&self, _: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        Ok(Some(self.objective_gradient(&[])?.into_iter().map(ip).collect()))
    }
    fn constraint_gradient_enclosure(
        &self,
        set: &[RowLabel],
        _: &[Interval],
    ) -> Result<Option<Vec<Interval>>, CertError> {
        Ok(Some(set.iter().flat_map(|&l| self.a_row(l)).map(ip).collect()))
    }
}

fn conic_from(rows: &[Vec<f64>], b: &[f64], n: usize) -> ConicProblem {
    let (mut colptr, mut rowval, mut nzval) = (vec![0], Vec::new(), Vec::new());
    for j in 0..n {
        for (i, r) in rows.iter().enumerate() {
            if r[j] != 0.0 {
                rowval.push(i);
                nzval.push(r[j]);
            }
        }
        colptr.push(rowval.len());
    }
    let mut q = vec![0.0; n];
    q[n - 1] = 1.0;
    ConicProblemBuilder::new(q, Csc::new(rows.len(), n, colptr, rowval, nzval), b.to_vec(), vec![Cone::Nonneg(b.len())])
        .build()
        .unwrap()
}

/// Path B: the rows permuted (`perm[j]` is the row of path A that becomes row `j`) and row `r`
/// scaled by `2^scale[r]`, solved, then mapped back exactly: `s_r = s'_j 2^-e`,
/// `y_r = y'_j 2^e`.
fn solve_path_b(p: &Lin, perm: &[i64], scale: &[i64]) -> ConicSolution {
    let labels = p.labels();
    let (mut rows, mut b) = (Vec::new(), Vec::new());
    for &r in perm {
        let f = 2f64.powi(scale[r as usize] as i32);
        let l = labels[r as usize];
        rows.push(p.a_row(l).iter().map(|v| v * f).collect::<Vec<f64>>());
        b.push(p.b_row(l) * f);
    }
    let sol = solve_lp(&conic_from(&rows, &b, p.n())).unwrap();
    let m = labels.len();
    let (mut s, mut y) = (vec![0.0; m], vec![0.0; m]);
    for (j, &r) in perm.iter().enumerate() {
        let f = 2f64.powi(scale[r as usize] as i32);
        s[r as usize] = sol.s[j] / f;
        y[r as usize] = sol.y[j] * f;
    }
    ConicSolution { s, y, ..sol }
}

// ------------------------------------------------------------------ fixture

struct Case {
    name: String,
    problem: Lin,
    fields: Vec<(String, String)>,
}

impl Case {
    fn get(&self, k: &str) -> &str {
        self.fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str()).unwrap_or_else(|| panic!("{k}"))
    }
    fn all(&self, k: &str) -> Vec<&str> {
        self.fields.iter().filter(|(key, _)| key == k).map(|(_, v)| v.as_str()).collect()
    }
}

fn parse() -> (Thresholds, Vec<Case>, String) {
    let (mut th, mut golden, mut out) = (None, String::new(), Vec::new());
    let mut head: Vec<String> = Vec::new();
    let mut fields = Vec::new();
    for l in FIXTURE.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if let Some(rest) = l.strip_prefix("thresholds ") {
            let t = hexes(rest);
            th = Some(Thresholds { slack_rel: t[0], mult_rel: t[1] });
        } else if let Some(rest) = l.strip_prefix("golden_digest ") {
            golden = rest.to_string();
        } else if let Some(rest) = l.strip_prefix("case ") {
            head = rest.split_whitespace().map(String::from).collect();
            fields.clear();
        } else if l == "end" {
            let get = |k: &str| fields.iter().find(|(key, _): &&(String, String)| key == k).unwrap().1.clone();
            let problem = Lin::new(head[2] == "zone", head[4].parse().unwrap(), &hexes(&get("points")));
            out.push(Case { name: head[0].clone(), problem, fields: fields.clone() });
        } else {
            let (k, v) = l.split_once(' ').unwrap_or((l, ""));
            fields.push((k.to_string(), v.to_string()));
        }
    }
    (th.unwrap(), out, golden)
}

/// The proven enclosure `got +- bound` contains the exact value whose nearest double is
/// `want`: rounding is monotone, so `fl(got - bound) <= want <= fl(got + bound)`.
fn encloses(got: f64, bound: f64, want: f64) -> bool {
    got - bound <= want && want <= got + bound
}

fn check_expected(c: &Case, cert: &Certificate) {
    assert_eq!(cert.active_set(), &labels(c.get("set"))[..], "{}: active set", c.name);
    for ((&g, &k), &w) in cert.params().iter().zip(cert.param_bounds()).zip(&hexes(c.get("params"))) {
        assert!(encloses(g, k, w), "{}: param {g} +- {k} vs {w}", c.name);
    }
    let v = hexes(c.get("value"))[0];
    assert!(encloses(cert.value(), cert.value_bound(), v), "{}: value", c.name);
    for ((&g, &k), &w) in cert.multipliers().iter().zip(cert.multiplier_bounds()).zip(&hexes(c.get("multipliers"))) {
        assert!(encloses(g, k, w), "{}: multiplier {g} +- {k} vs {w}", c.name);
    }
}

fn certify_with(c: &Case, sol: &ConicSolution, th: &Thresholds) -> Result<Certified, CertError> {
    certify(&c.problem, &c.problem.conic(), sol, c.problem.kind(), &CertConfig::new(*th))
}

fn generated(cases: &[Case]) -> impl Iterator<Item = &Case> {
    cases.iter().filter(|c| c.fields.iter().any(|(k, _)| k == "perm"))
}

// ------------------------------------------------------------------ AC-1

/// AC-1: 60 minimum-zone and envelope problems, each solved on two seeded solver paths; both
/// certify the oracle's active set, and params, value and digest are bit-identical.
#[test]
fn ac1_two_paths_bit_identical() {
    let (th, cases, _) = parse();
    let mut count = [0usize; 4];
    let mut differ = 0;
    for c in generated(&cases) {
        let p = &c.problem;
        count[usize::from(!p.zone) * 2 + p.dim - 1] += 1;
        let sol_a = solve_lp(&p.conic()).unwrap();
        let sol_b = solve_path_b(p, &ints(c.get("perm")), &ints(c.get("scale")));
        if sol_a.s.iter().zip(&sol_b.s).chain(sol_a.y.iter().zip(&sol_b.y)).any(|(a, b)| a.to_bits() != b.to_bits()) {
            differ += 1;
        }
        let a = certify_with(c, &sol_a, &th).unwrap_or_else(|e| panic!("{}: {e}", c.name));
        let b = certify_with(c, &sol_b, &th).unwrap_or_else(|e| panic!("{}: path B: {e}", c.name));
        check_expected(c, a.certificate());
        assert_eq!(a.certificate().active_set(), b.certificate().active_set(), "{}", c.name);
        let bits = |x: &Certified| x.params().iter().map(|v| v.to_bits()).collect::<Vec<u64>>();
        assert_eq!(bits(&a), bits(&b), "{}: params", c.name);
        assert_eq!(a.value().to_bits(), b.value().to_bits(), "{}: value", c.name);
        assert_eq!(a.certificate().digest(), b.certificate().digest(), "{}: digest", c.name);
        assert_eq!(a.certificate().to_canonical_json().unwrap(), b.certificate().to_canonical_json().unwrap());
        assert_eq!(a.certificate().kind(), p.kind());
        assert!(!a.tie_broken() && !b.tie_broken() && !a.certificate().flags().local_optimum_only);
        assert!(a.certificate().is_linear());
        assert_eq!(a.params(), a.certificate().params());
    }
    assert_eq!(count, [15, 15, 15, 15]);
    // the two paths really differ (in the bits of their slacks or multipliers)
    assert!(differ >= 55, "only {differ} of 60 solution pairs differ");
}

// ------------------------------------------------------------------ AC-2

/// The oracle's outcome of each candidate, with its set.
type Candidates = Vec<(String, Vec<RowLabel>)>;

fn tie_case(cases: &[Case], name: &str) -> (Lin, Vec<RowLabel>, Candidates) {
    let c = cases.iter().find(|c| c.name == name).unwrap();
    let cands = c
        .all("candidate")
        .iter()
        .map(|v| {
            let (outcome, set) = v.split_once(' ').unwrap();
            (outcome.to_string(), labels(set))
        })
        .collect();
    (c.problem.clone(), labels(c.get("small")), cands)
}

/// A hand-built solution: slack 0 on `small`; elsewhere the exact slack at `x` (`exact`) or 1;
/// multiplier `y` on `small` and 0 elsewhere.
fn hand_solution(p: &Lin, small: &[RowLabel], x: &[f64], y: f64, exact: bool) -> ConicSolution {
    let labels = p.labels();
    let s = labels
        .iter()
        .map(|&l| {
            if small.contains(&l) {
                0.0
            } else if exact {
                -p.g(l, x)
            } else {
                1.0
            }
        })
        .collect();
    let y = labels.iter().map(|l| if small.contains(l) { y } else { 0.0 }).collect();
    ConicSolution {
        x: x.to_vec(),
        y,
        s,
        objective: x[x.len() - 1],
        iterations: 0,
        residuals: ConicResiduals { primal: 0.0, dual: 0.0, gap_abs: 0.0, gap_rel: 0.0 },
    }
}

/// AC-2: the five-point tie of KRN.SOLV.CERT.AS. The first candidate {0:0 1:1 3:1} has a
/// negative multiplier, so the certificate uses {0:0 1:1 4:0} with `tie_broken`. The same
/// certificate comes from the wrapper's interior solution and from a vertex solution.
#[test]
fn ac2_tie_uses_lowest_passing_set() {
    let (th, cases, _) = parse();
    let (p, small, cands) = tie_case(&cases, "ac2-tie");
    let c = cases.iter().find(|c| c.name == "ac2-tie").unwrap();
    let conic = p.conic();
    let solved = solve_lp(&conic).unwrap();
    let vertex = hand_solution(&p, &small, &[0.0, 0.0, 2.0], 0.25, true);
    let mut bytes = Vec::new();
    for sol in [&solved, &vertex] {
        let prop = extract(&conic, sol, &p.labels(), 3, &th).unwrap();
        let want: Vec<Vec<RowLabel>> = cands.iter().map(|(_, s)| s.clone()).collect();
        assert_eq!(prop.uncertified_sets(), &want[..]);
        let got = certify(&p, &conic, sol, CertKind::Zone, &CertConfig::new(th)).unwrap();
        let cert = got.certificate();
        check_expected(c, cert);
        assert_eq!(cert.active_set(), &labels("0:0 1:1 4:0")[..]);
        assert!(got.tie_broken());
        bytes.push(cert.to_canonical_json().unwrap());
    }
    assert_eq!(bytes[0], bytes[1]);
    // the oracle's verdict on every candidate, checked through recompute + check
    let prop = extract(&conic, &solved, &p.labels(), 3, &th).unwrap();
    check_candidates(&p, &prop, &cands);
}

fn check_candidates(p: &Lin, prop: &fid_solv::cert::Proposal, cands: &[(String, Vec<RowLabel>)]) {
    for (i, (outcome, set)) in cands.iter().enumerate() {
        assert_eq!(&prop.uncertified_sets()[i], set);
        let r = recompute(p, prop, i, &RecompConfig::default());
        match outcome.as_str() {
            "singular" => assert!(
                matches!(r, Err(CertError::RankDeficient { .. } | CertError::IllConditioned { .. })),
                "{set:?}: {r:?}"
            ),
            o => {
                let rep = check(p, &r.unwrap(), &KktTolerances::default()).unwrap();
                let want = match o {
                    "certified" => None,
                    "violated_primal" => Some(KktFail::Primal),
                    "violated_dual" => Some(KktFail::Dual),
                    other => panic!("{other}"),
                };
                assert_eq!(rep.reason(), want, "{set:?}");
                assert_eq!(rep.pass(), want.is_none());
                if want.is_some() {
                    assert_eq!(rep.status(), KktStatus::Violated);
                }
            }
        }
    }
}

// ------------------------------------------------------------------ AC-3

/// AC-3: a solution whose four candidates all fail (three violate a KKT condition, one is
/// singular): `certificate_failed` with the count tried and the first candidate's report.
#[test]
fn ac3_all_candidates_fail() {
    let (th, cases, _) = parse();
    let (p, small, cands) = tie_case(&cases, "ac3-all-fail");
    let c = cases.iter().find(|c| c.name == "ac3-all-fail").unwrap();
    let tried: usize = c.get("tried").parse().unwrap();
    let sol = hand_solution(&p, &small, &[0.0, 0.0, 2.0], 0.0, false);
    let conic = p.conic();
    let prop = extract(&conic, &sol, &p.labels(), 3, &th).unwrap();
    check_candidates(&p, &prop, &cands);
    match certify(&p, &conic, &sol, CertKind::Zone, &CertConfig::new(th)) {
        Err(CertError::CertificateFailed { tried: t, first_report: Some(r), first_error: None }) => {
            assert_eq!(t, tried);
            assert_eq!(r.status(), KktStatus::Violated);
            assert_eq!(r.reason(), Some(KktFail::Primal));
        }
        other => panic!("{other:?}"),
    }
}

/// A KKT condition that is only undecidable (tolerance 0: the enclosure of the primal maximum
/// straddles 0 at the tight rows) never yields a certificate.
#[test]
fn undecidable_is_not_certified() {
    let (th, cases, _) = parse();
    let c = generated(&cases).next().unwrap();
    let sol = solve_lp(&c.problem.conic()).unwrap();
    let mut cfg = CertConfig::new(th);
    cfg.kkt = KktTolerances { tol_p: 0.0, tol_c: 0.0, ..KktTolerances::default() };
    match certify(&c.problem, &c.problem.conic(), &sol, c.problem.kind(), &cfg) {
        Err(CertError::CertificateFailed { tried: 1, first_report: Some(r), first_error: None }) => {
            assert_eq!(r.status(), KktStatus::Undecidable);
        }
        other => panic!("{other:?}"),
    }
}

/// The first candidate's recomputation error is reported when it had no KKT report.
#[test]
fn first_error_when_no_report() {
    let (th, cases, _) = parse();
    let (p, _, _) = tie_case(&cases, "ac3-all-fail");
    // only the three upper rows of points 1, 2, 3 are small: one singular candidate
    let small = labels("1:1 2:1 3:1");
    let sol = hand_solution(&p, &small, &[0.0, 0.0, 2.0], 0.0, false);
    match certify(&p, &p.conic(), &sol, CertKind::Zone, &CertConfig::new(th)) {
        Err(CertError::CertificateFailed { tried: 1, first_report: None, first_error: Some(e) }) => {
            assert!(matches!(*e, CertError::RankDeficient { .. } | CertError::IllConditioned { .. }), "{e:?}");
        }
        other => panic!("{other:?}"),
    }
}

/// Fail closed before any candidate: an unset data scale, invalid config, tolerances or
/// thresholds, and the extraction's own errors.
#[test]
fn up_front_errors() {
    let (th, cases, _) = parse();
    let c = generated(&cases).next().unwrap();
    let sol = solve_lp(&c.problem.conic()).unwrap();
    let conic = c.problem.conic();
    let mut unset = c.problem.clone();
    unset.scale_unset = true;
    assert_eq!(
        certify(&unset, &conic, &sol, CertKind::Zone, &CertConfig::new(th)).err(),
        Some(CertError::InvalidInput { field: "data_scale_unset" })
    );
    let mut cfg = CertConfig::new(th);
    cfg.recomp.gn_steps = 0;
    assert_eq!(
        certify(&c.problem, &conic, &sol, CertKind::Zone, &cfg).err(),
        Some(CertError::InvalidInput { field: "config" })
    );
    let mut cfg = CertConfig::new(th);
    cfg.kkt.tol_d = f64::NAN;
    assert_eq!(
        certify(&c.problem, &conic, &sol, CertKind::Zone, &cfg).err(),
        Some(CertError::InvalidInput { field: "tolerances" })
    );
    let cfg = CertConfig::new(Thresholds { slack_rel: -1.0, mult_rel: 0.0 });
    assert_eq!(
        certify(&c.problem, &conic, &sol, CertKind::Zone, &cfg).err(),
        Some(CertError::InvalidInput { field: "thresholds" })
    );
    // every slack large: too few rows
    let mut far = sol.clone();
    far.s.iter_mut().for_each(|v| *v = 1e3);
    assert!(matches!(
        certify(&c.problem, &conic, &far, CertKind::Zone, &CertConfig::new(th)),
        Err(CertError::ActiveSetUndetermined { .. })
    ));
}

// ------------------------------------------------------------------ AC-4

/// AC-4: solutions that differ only in their x, y and s bits (each slack staying on its side
/// of the threshold), objective, iteration count and residuals give the same certificate.
#[test]
fn ac4_iterates_do_not_reach_the_digest() {
    let (th, cases, _) = parse();
    for c in generated(&cases) {
        let sol = solve_lp(&c.problem.conic()).unwrap();
        let mut other = sol.clone();
        other.x.iter_mut().for_each(|v| *v = *v * (1.0 + 2f64.powi(-20)) + 1e-9);
        other.y.iter_mut().for_each(|v| *v *= 1.0 + 2f64.powi(-30));
        let set = labels(c.get("set"));
        for (l, s) in c.problem.labels().iter().zip(other.s.iter_mut()) {
            *s = if set.contains(l) { *s * 0.5 } else { *s * (1.0 + 2f64.powi(-30)) };
        }
        other.objective += 1.0;
        other.iterations += 7;
        other.residuals.gap_abs = 0.125;
        let a = certify_with(c, &sol, &th).unwrap();
        let b = certify_with(c, &other, &th).unwrap();
        assert_eq!(a.certificate().digest(), b.certificate().digest(), "{}", c.name);
        assert_eq!(a, b, "{}", c.name);
    }
}

// ------------------------------------------------------------------ AC-5

fn all_certificates() -> Vec<Certificate> {
    let (th, cases, _) = parse();
    let mut out: Vec<Certificate> = generated(&cases)
        .map(|c| certify_with(c, &solve_lp(&c.problem.conic()).unwrap(), &th).unwrap().into_certificate())
        .collect();
    let (p, _, _) = tie_case(&cases, "ac2-tie");
    out.push(
        certify(&p, &p.conic(), &solve_lp(&p.conic()).unwrap(), CertKind::Zone, &CertConfig::new(th))
            .unwrap()
            .into_certificate(),
    );
    out
}

/// AC-5: serialise-parse-serialise gives identical bytes, the digest matches a recomputation
/// from the parsed fields, and `set_local_optimum_only` changes only the flag and the digest.
#[test]
fn ac5_round_trip_and_local_flag() {
    for cert in all_certificates() {
        let bytes = cert.to_canonical_json().unwrap();
        let parsed = Certificate::parse(&bytes).unwrap();
        assert_eq!(parsed, cert);
        assert_eq!(parsed.to_canonical_json().unwrap(), bytes);
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains(&format!("\"schema\":\"{SCHEMA}\"")));
        assert!(text.contains(&format!("\"algorithm\":\"{ALGORITHM}\"")));
        assert!(text.contains(&format!("\"digest\":\"{}\"", cert.digest())));
        for banned in ["\"x\"", "\"y\"", "\"s\"", "iterations", "objective", "tie_broken"] {
            assert!(!text.contains(banned), "{banned}");
        }

        let mut local = cert.clone();
        local.set_local_optimum_only().unwrap();
        assert!(local.flags().local_optimum_only);
        assert_ne!(local.digest(), cert.digest());
        let lbytes = local.to_canonical_json().unwrap();
        let back = String::from_utf8(lbytes.clone())
            .unwrap()
            .replace("\"local_optimum_only\":\"true\"", "\"local_optimum_only\":\"false\"")
            .replace(local.digest().as_str(), cert.digest().as_str());
        assert_eq!(back.as_bytes(), &bytes[..], "only the flag and the digest differ");
        assert_eq!(Certificate::parse(&lbytes).unwrap(), local);
        let mut twice = local.clone();
        twice.set_local_optimum_only().unwrap();
        assert_eq!(twice, local);
    }
}

/// The wire form and digest rule against the independent writer and hashlib of make_cases.py.
#[test]
fn golden_certificate_from_the_oracle() {
    let (_, _, golden_digest) = parse();
    let cert = Certificate::parse(GOLDEN).unwrap();
    assert_eq!(cert.digest().as_str(), golden_digest);
    assert_eq!(cert.to_canonical_json().unwrap(), GOLDEN);
    assert_eq!(cert.kind(), CertKind::Zone);
    assert_eq!(cert.active_set(), &labels("1:0 4:1 6:0")[..]);
    let thr: Vec<u64> = cert.dual_thresholds().iter().map(|v| v.to_bits()).collect();
    assert_eq!(thr, [0x3e0b7cdfd9d7bdb7, 0x3e112e0be826d692, 0x3e06e80fe033c8c3]);
    assert_eq!(cert.gradient_norms_hi(), &[1.25, 1.0, 1.5]);
    assert!(cert.is_linear() && !cert.flags().local_optimum_only);
    assert_eq!(cert.kkt().scales.data, 12.5);
    assert_eq!(cert.recomp().gn_steps, 8);
    assert_eq!(cert.steps(), 1);
}

fn tampered(from: &str, to: &str) -> Result<Certificate, CertError> {
    let text = std::str::from_utf8(GOLDEN).unwrap();
    assert!(text.contains(from), "{from}");
    Certificate::parse(text.replacen(from, to, 1).as_bytes())
}

/// The golden text with `from` replaced by `to` and the digest recomputed over the new body,
/// so only the consistency check can refuse it.
fn tampered_redigested(from: &str, to: &str) -> Result<Certificate, CertError> {
    let text = std::str::from_utf8(GOLDEN).unwrap().replacen(from, to, 1);
    let at = text.find("\"digest\":\"").unwrap();
    let member = &text[at..at + "\"digest\":\"sha256:\",".len() + 64];
    let body = text.replacen(member, "", 1);
    let mut h = Sha256Hasher::new();
    h.update(body.as_bytes());
    let fresh = format!("\"digest\":\"{}\",", h.finish());
    Certificate::parse(text.replacen(member, &fresh, 1).as_bytes())
}

/// Parsing fails closed: a changed value (digest mismatch), a changed digest, non-canonical
/// bytes, an inconsistent record, an unknown schema or kind, a float not in `F64Bits` form.
#[test]
fn parse_fails_closed() {
    let mal = |detail| Err(CertError::Malformed { detail });
    let inc = |detail| Err(CertError::Inconsistent { detail });
    assert_eq!(tampered("\"0x3fd8000000000000\"", "\"0x3fd8000000000001\""), mal("digest_mismatch"));
    assert_eq!(tampered("sha256:8", "sha256:9"), mal("digest_mismatch"));
    assert_eq!(tampered("\"is_linear\":\"true\"", "\"is_linear\":\"false\""), inc("steps"));
    assert_eq!(tampered("\"0x3fd8000000000000\"", "\"3fd8000000000000\""), mal("float"));
    assert_eq!(tampered("\"0x3fd8000000000000\"", "\"0X3fd8000000000000\""), mal("float"));
    assert_eq!(tampered("{\"active_set\"", "{ \"active_set\""), mal("syntax"));
    assert_eq!(tampered("\"schema\":\"fid.cert@1\"", "\"schema\":\"fid.cert@2\""), mal("schema"));
    assert_eq!(tampered("\"kind\":\"zone\"", "\"kind\":\"Zone\""), mal("kind"));
    assert_eq!(tampered("\"status\":\"certified\"", "\"status\":\"undecidable\""), mal("status"));
    assert_eq!(tampered("\"steps\":\"1\"", "\"steps\":\"01\""), mal("integer"));
    assert_eq!(tampered("\"steps\":\"1\"", "\"steps\":\"2\""), inc("steps"));
    assert_eq!(tampered("\"flags\":{\"local", "\"flags\":{\"tie_broken\":\"true\",\"local"), mal("unknown_member"));
    // primal hi above its threshold
    assert_eq!(
        tampered("\"hi\":\"0x3d60000000000000\",\"lo\":\"0xbd6", "\"hi\":\"0x3f60000000000000\",\"lo\":\"0xbd6"),
        inc("primal")
    );
    // a negative multiplier beyond its row threshold
    assert_eq!(
        tampered("\"multipliers\":[\"0x3fd0000000000000\"", "\"multipliers\":[\"0xbfd0000000000000\""),
        inc("dual")
    );
    // NaN
    assert_eq!(tampered("\"value\":\"0x3fea000000000000\"", "\"value\":\"0x7ff8000000000000\""), inc("not_finite"));
    assert_eq!(tampered("\"active_set\":[\"1:0\",\"4:1\"", "\"active_set\":[\"4:1\",\"1:0\""), inc("active_set"));
    assert!(Certificate::parse(&[]).is_err());
}

/// Dual thresholds are rechecked bit for bit from `tol_d`, `gradient_lo` and the recorded
/// `hi||grad g_i||`, even when the digest is recomputed to match the tampered body: raising a
/// threshold that is not the minimum (so the recorded dual minimum still agrees) is refused,
/// as is a changed divisor or a nonzero threshold when `gradient_lo` is 0.
#[test]
fn tampered_dual_thresholds_are_refused() {
    let inc = |detail| Err(CertError::Inconsistent { detail });
    // the untouched golden body re-digested parses (the helper itself is sound)
    assert!(tampered_redigested("\"kind\":\"zone\"", "\"kind\":\"zone\"").is_ok());
    assert_eq!(
        tampered_redigested(
            "\"dual_thresholds\":[\"0x3e0b7cdfd9d7bdb7\"",
            "\"dual_thresholds\":[\"0x3e0b7cdfd9d7bdb8\""
        ),
        inc("dual_thresholds")
    );
    assert_eq!(
        tampered_redigested(
            "\"dual_thresholds\":[\"0x3e0b7cdfd9d7bdb7\"",
            "\"dual_thresholds\":[\"0x3e20000000000000\""
        ),
        inc("dual_thresholds")
    );
    assert_eq!(
        tampered_redigested(
            "\"gradient_norms_hi\":[\"0x3ff4000000000000\"",
            "\"gradient_norms_hi\":[\"0x3ff2000000000000\""
        ),
        inc("dual_thresholds")
    );
    assert_eq!(
        tampered_redigested("\"gradient_lo\":\"0x3fefffffffffffff\"", "\"gradient_lo\":\"0x0000000000000000\""),
        inc("dual_thresholds")
    );
    assert_eq!(
        tampered_redigested(
            "\"gradient_norms_hi\":[\"0x3ff4000000000000\"",
            "\"gradient_norms_hi\":[\"0x0000000000000000\""
        ),
        inc("gradient_norms_hi")
    );
}

/// REQ-003: `tie_broken` depends on the solution's slacks, so it is not in the digest. Two
/// solutions of one problem whose extractions list different candidates (one candidate, and
/// `n + 1` candidates because an inactive row's slack is reported as 0) certify the same set
/// with identical certificates and digests; only the in-memory `tie_broken` differs.
#[test]
fn different_candidate_lists_same_digest() {
    let (th, cases, _) = parse();
    for c in generated(&cases) {
        let p = &c.problem;
        let conic = p.conic();
        let sol = solve_lp(&conic).unwrap();
        let set = labels(c.get("set"));
        let mut other = sol.clone();
        let extra = p.labels().iter().position(|l| !set.contains(l)).unwrap();
        other.s[extra] = 0.0;
        let la = extract(&conic, &sol, &p.labels(), p.n(), &th).unwrap();
        let lb = extract(&conic, &other, &p.labels(), p.n(), &th).unwrap();
        assert_eq!(la.uncertified_sets().len(), 1, "{}", c.name);
        assert_eq!(lb.uncertified_sets().len(), p.n() + 1, "{}", c.name);
        let a = certify_with(c, &sol, &th).unwrap();
        let b = certify_with(c, &other, &th).unwrap_or_else(|e| panic!("{}: {e}", c.name));
        assert!(!a.tie_broken() && b.tie_broken(), "{}", c.name);
        assert_eq!(b.certificate().active_set(), &set[..], "{}", c.name);
        assert_eq!(a.certificate().digest(), b.certificate().digest(), "{}", c.name);
        assert_eq!(a.certificate(), b.certificate(), "{}", c.name);
    }
}

// ------------------------------------------------------------------ output pin

/// Output-bit pin: regression pin of this crate's own output (every certificate's bytes, then
/// the AC-3 error in the stable encoding of `encode_err`), recorded from a local x86-64 run and checked by CI on every target. One
/// value serves all targets because they agree bit for bit; a target that diverged would get
/// its own `#[cfg(target_arch)]` value here.
const OUTPUT_PIN: &str = "sha256:f2970613c0c6b3cfb008ece929490b438efb00a977732b82d46e6f06868c926e";

fn bits_of(xs: &[f64]) -> String {
    xs.iter().map(|v| format!("0x{:016x}", v.to_bits())).collect::<Vec<_>>().join(",")
}

/// A stable encoding of an error: its code and fields (KKT reports by status, reason and
/// every recorded value's bits), never `Debug` output.
fn encode_err(e: &CertError) -> String {
    let mut out = e.code().to_string();
    match e {
        CertError::CertificateFailed { tried, first_report, first_error } => {
            out.push_str(&format!(";tried={tried}"));
            if let Some(r) = first_report {
                let status = match r.status() {
                    KktStatus::Certified => "certified",
                    KktStatus::Violated => "violated",
                    KktStatus::Undecidable => "undecidable",
                };
                let reason = match r.reason() {
                    None => "none",
                    Some(KktFail::Primal) => "primal",
                    Some(KktFail::Dual) => "dual",
                    Some(KktFail::Stationarity) => "stationarity",
                    Some(KktFail::Complementarity) => "complementarity",
                    Some(KktFail::DegenerateGradients) => "degenerate_gradients",
                    Some(KktFail::MultipliersNotProven) => "multipliers_not_proven",
                    Some(_) => "other",
                };
                out.push_str(&format!(";status={status};reason={reason}"));
                for c in [r.primal(), r.dual(), r.stationarity(), r.complementarity()] {
                    out.push_str(&format!(";cond={}", bits_of(&[c.lo(), c.hi(), c.threshold()])));
                }
                out.push_str(&format!(
                    ";mult={};mb={};thr={};ghi={}",
                    bits_of(r.multipliers()),
                    bits_of(r.multiplier_bounds()),
                    bits_of(r.dual_thresholds()),
                    bits_of(r.gradient_norms_hi())
                ));
            }
            if let Some(e) = first_error {
                out.push_str(&format!(";first_error={}", encode_err(e)));
            }
        }
        CertError::RankDeficient { rank } => out.push_str(&format!(";rank={rank}")),
        CertError::IllConditioned { cond } => out.push_str(&format!(";cond={}", bits_of(&[*cond]))),
        CertError::InvalidInput { field } => out.push_str(&format!(";field={field}")),
        _ => {}
    }
    out
}

#[test]
fn output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for cert in all_certificates() {
        h.update(&cert.to_canonical_json().unwrap());
    }
    let (th, cases, _) = parse();
    let (p, small, _) = tie_case(&cases, "ac3-all-fail");
    let sol = hand_solution(&p, &small, &[0.0, 0.0, 2.0], 0.0, false);
    let err = certify(&p, &p.conic(), &sol, CertKind::Zone, &CertConfig::new(th)).unwrap_err();
    h.update(encode_err(&err).as_bytes());
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
