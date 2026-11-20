//! Over-cap tied pools of a linear problem (spec node KRN.SOLV.CERT.AS, REQ-007): the
//! certificate pipeline reduces the pool to one exact Bland-order basis instead of failing
//! `OverCap`, whatever the solution's multipliers, and fails closed with `NoBasis` when the pool
//! has no basis with nonnegative multipliers.
//!
//! Expected values come from `fixtures/solv/cert/as-ties-v1/make_cases.py` (the same rule on an
//! exact `Fraction` tableau, the exact optimum; independent of this crate and of any solver).
//! `OUTPUT_PIN` is a regression pin of this crate's own output (not an oracle); CI checks it on
//! x86-64 Linux and Windows, ARM64 macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Instant;

use fid_num::f;
use fid_num::la::Mat;
use fid_solv::cert::active::{extract, RowLabel, Thresholds};
use fid_solv::cert::recomp::{ActiveSetProblem, Interval};
use fid_solv::cert::{certify, CertConfig, CertError, CertKind, Undetermined};
use fid_solv::conic::{Cone, ConicProblem, ConicProblemBuilder, ConicResiduals, ConicSolution, Csc};
use fid_testkit::Sha256Hasher;

const FIXTURE: &str = include_str!("../../../fixtures/solv/cert/as-ties-v1/cases.txt");

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

/// Minimum-zone straightness, `p = (a, c, t)`, rows `(i, 1)`: `z - a x - c - t/2 <= 0` and
/// `(i, 0)`: `-(z - a x - c) - t/2 <= 0`, i.e. `g = A_row p - b_row` with the conic rows of the
/// fixture script. Linear, with every enclosure the certificate needs.
struct Zone {
    pts: Vec<(f64, f64)>,
}

impl Zone {
    fn row(&self, l: RowLabel) -> ([f64; 3], f64) {
        let (x, z) = self.pts[l.point as usize];
        if l.side == 1 {
            ([-x, -1.0, -0.5], -z)
        } else {
            ([x, 1.0, -0.5], z)
        }
    }
    fn g(&self, l: RowLabel, p: &[f64]) -> f64 {
        let (a, b) = self.row(l);
        a[0] * p[0] + a[1] * p[1] + a[2] * p[2] - b
    }
    fn conic(&self) -> ConicProblem {
        let labels = self.row_labels();
        let m = labels.len();
        let (mut colptr, mut rowval, mut nzval) = (vec![0], Vec::new(), Vec::new());
        for j in 0..3 {
            for (i, &l) in labels.iter().enumerate() {
                let v = self.row(l).0[j];
                if v != 0.0 {
                    rowval.push(i);
                    nzval.push(v);
                }
            }
            colptr.push(rowval.len());
        }
        let b: Vec<f64> = labels.iter().map(|&l| self.row(l).1).collect();
        ConicProblemBuilder::new(vec![0.0, 0.0, 1.0], Csc::new(m, 3, colptr, rowval, nzval), b, vec![Cone::Nonneg(m)])
            .build()
            .unwrap()
    }
}

impl ActiveSetProblem for Zone {
    fn n_params(&self) -> usize {
        3
    }
    fn expected_active(&self) -> usize {
        3
    }
    fn row_labels(&self) -> Vec<RowLabel> {
        (0..self.pts.len() as u32).flat_map(|point| [0, 1].map(|side| RowLabel { point, side })).collect()
    }
    fn is_linear(&self) -> bool {
        true
    }
    fn start_from_set(&self, _: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        Err(CertError::Callback { reason: "linear" })
    }
    fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
        let r = set.iter().map(|&l| self.g(l, p)).collect();
        let rows: Vec<[f64; 3]> = set.iter().map(|&l| self.row(l).0).collect();
        Ok((r, Mat::from_rows(&rows)?))
    }
    fn value(&self, p: &[f64]) -> Result<f64, CertError> {
        Ok(p[2])
    }
    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        Ok(self.row_labels().iter().map(|&l| self.g(l, p)).collect())
    }
    fn objective_gradient(&self, _: &[f64]) -> Result<Vec<f64>, CertError> {
        Ok(vec![0.0, 0.0, 1.0])
    }
    fn constraint_gradients(&self, set: &[RowLabel], _: &[f64]) -> Result<Mat, CertError> {
        let rows: Vec<[f64; 3]> = set.iter().map(|&l| self.row(l).0).collect();
        Ok(Mat::from_rows(&rows)?)
    }
    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        Ok(Some(b[2]))
    }
    fn data_scale(&self) -> Result<f64, CertError> {
        let mut m = 0.0_f64;
        for &(x, z) in &self.pts {
            m = f::total_max(m, f::total_max(x.abs(), z.abs())?)?;
        }
        Ok(m)
    }
    fn constraint_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        for l in self.row_labels() {
            let (a, rhs) = self.row(l);
            let mut acc = ip(-rhs);
            for (aj, bj) in a.iter().zip(b) {
                acc = acc.add(ip(*aj).mul(*bj)?)?;
            }
            out.push(acc);
        }
        Ok(Some(out))
    }
    fn objective_gradient_enclosure(&self, _: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        Ok(Some(vec![ip(0.0), ip(0.0), ip(1.0)]))
    }
    fn constraint_gradient_enclosure(
        &self,
        set: &[RowLabel],
        _: &[Interval],
    ) -> Result<Option<Vec<Interval>>, CertError> {
        Ok(Some(set.iter().flat_map(|&l| self.row(l).0.map(ip)).collect()))
    }
}

struct Case {
    name: String,
    zone: Zone,
    pool: Vec<RowLabel>,
    basis: Option<Vec<RowLabel>>,
    params: Vec<f64>,
}

fn parse() -> Vec<Case> {
    let mut out = Vec::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut name = String::new();
    for l in FIXTURE.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if let Some(rest) = l.strip_prefix("case ") {
            name = rest.split_whitespace().next().unwrap().to_string();
            fields.clear();
        } else if l == "end" {
            let get = |k: &str| fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default();
            let nums = |s: String| s.split_whitespace().map(|t| t.parse::<f64>().unwrap()).collect::<Vec<f64>>();
            let pts = nums(get("points")).chunks(2).map(|c| (c[0], c[1])).collect();
            let expect = get("expect");
            out.push(Case {
                name: name.clone(),
                zone: Zone { pts },
                pool: labels(&get("pool")),
                basis: expect.strip_prefix("basis ").map(labels),
                params: nums(get("params")),
            });
        } else {
            let (k, v) = l.split_once(' ').unwrap_or((l, ""));
            fields.push((k.to_string(), v.to_string()));
        }
    }
    out
}

/// A solution with slack 0 on the pool and 1 elsewhere; multipliers `y` on the pool rows, the
/// rest 0. `vertex` puts them only on the first three pool rows, otherwise spreads them evenly
/// (an interior solution).
fn solution(z: &Zone, pool: &[RowLabel], vertex: bool) -> ConicSolution {
    let labels = z.row_labels();
    let mut s = Vec::new();
    let mut y = Vec::new();
    for l in &labels {
        let at = pool.iter().position(|p| p == l);
        s.push(if at.is_some() { 0.0 } else { 1.0 });
        y.push(match at {
            Some(_) if !vertex => 1.0 / pool.len() as f64,
            Some(k) if k < 3 => 0.5,
            _ => 0.0,
        });
    }
    ConicSolution {
        x: vec![0.125, 0.0, 2.0],
        y,
        s,
        objective: 2.0,
        iterations: 9,
        residuals: ConicResiduals { primal: 0.0, dual: 0.0, gap_abs: 0.0, gap_rel: 0.0 },
    }
}

fn th() -> Thresholds {
    Thresholds { slack_rel: 1e-9, mult_rel: 1e-9 }
}

#[test]
fn fixture_counts() {
    let cases = parse();
    assert_eq!(cases.len(), 3);
    assert_eq!(cases.iter().map(|c| c.pool.len()).collect::<Vec<_>>(), [200, 13, 100]);
}

/// The over-cap pools certify on the oracle's basis, from an interior and a vertex solution
/// alike (identical certificate bytes), the plain extraction still reporting `OverCap`.
#[test]
fn over_cap_pools_certify_on_the_exact_basis() {
    for c in parse().iter().filter(|c| c.basis.is_some()) {
        let conic = c.zone.conic();
        let mut bytes = Vec::new();
        for vertex in [false, true] {
            let sol = solution(&c.zone, &c.pool, vertex);
            assert!(matches!(
                extract(&conic, &sol, &c.zone.row_labels(), 3, &th()),
                Err(CertError::ActiveSetUndetermined { cause: Undetermined::OverCap, .. })
            ));
            let start = Instant::now();
            let got = certify(&c.zone, &conic, &sol, CertKind::Zone, &CertConfig::new(th()))
                .unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
            // Bounded time: one exact phase-1 simplex on 200 columns, not C(200, 3) bases.
            assert!(start.elapsed().as_secs() < 30, "{}: {:?}", c.name, start.elapsed());
            let cert = got.certificate();
            assert_eq!(cert.active_set(), &c.basis.clone().unwrap()[..], "{}", c.name);
            assert!(got.tie_broken());
            for (i, (&g, &w)) in cert.params().iter().zip(&c.params).enumerate() {
                assert!((g - w).abs() <= cert.param_bounds()[i], "{}: param {i}", c.name);
            }
            bytes.push(cert.to_canonical_json().unwrap());
        }
        assert_eq!(bytes[0], bytes[1], "{}", c.name);
    }
}

/// A pool whose multiplier system has no nonnegative solution proposes nothing: fail closed.
#[test]
fn pool_without_a_basis_fails_closed() {
    let c = parse().into_iter().find(|c| c.basis.is_none()).unwrap();
    let conic = c.zone.conic();
    let sol = solution(&c.zone, &c.pool, false);
    match certify(&c.zone, &conic, &sol, CertKind::Zone, &CertConfig::new(th())) {
        Err(CertError::ActiveSetUndetermined { cause: Undetermined::NoBasis, strongly_active, ambiguous }) => {
            assert_eq!(strongly_active.len() + ambiguous.len(), 100);
        }
        other => panic!("{other:?}"),
    }
}

/// Output-bit pin of every outcome, recorded from a local x86-64 run and checked by CI on every
/// target (REQ-KRN.SOLV.CERT.AS-005).
const OUTPUT_PIN: &str = "sha256:979707c415abb6daf46384ba16b60ccb58f46135df804b5ac660550b9b198705";

#[test]
fn output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for c in &parse() {
        h.update(c.name.as_bytes());
        let sol = solution(&c.zone, &c.pool, false);
        match certify(&c.zone, &c.zone.conic(), &sol, CertKind::Zone, &CertConfig::new(th())) {
            Ok(got) => h.update(&got.certificate().to_canonical_json().unwrap()),
            Err(e) => h.update(format!("{e:?}").as_bytes()),
        }
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
