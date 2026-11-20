//! The convergence rule of spec node KRN.SOLV.CERT.RECOMP at the rounding floor: nonlinear
//! problems whose residuals are evaluated in plain `f64` (no double-double) must recompute and
//! certify when their Gauss-Newton steps stall at the rounding floor, and an unconverged
//! problem must still be refused.
//!
//! Expected values come from `fixtures/solv/cert/recomp-floor-v1/make_cases.py` (90-digit
//! Newton-polished optima of minimum-zone partial arcs, long cylinders and cylinders 300 mm from
//! the origin, each verified a strict local minimum; independent of this crate, of any solver
//! and of the platform libm). All 64 generated candidates are emitted (no selection). The test
//! replays the recomputation's Gauss-Newton iteration bit for bit and counts the floor cases,
//! whose last step exceeds `4 u ||p||` (the step-only rule before the fix refused them); at
//! least `MIN_FLOOR` must be floor cases, and every candidate must recompute. `OUTPUT_PIN` is a
//! regression pin of this crate's own output (not an oracle); CI checks it on x86-64 Linux and
//! Windows, ARM64 macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::la::lstsq;
use fid_num::red;
use fid_solv::cert::active::RowLabel;
use fid_solv::cert::recomp::{recompute, ActiveSetProblem, RecompConfig, Recomputed, DEFAULT_REL_TOL, STEP_TOL};
use fid_solv::cert::{certify, CertConfig, CertError, CertKind};
use fid_testkit::Sha256Hasher;

const FIXTURE: &str = include_str!("../../../fixtures/solv/cert/recomp-floor-v1/cases.txt");

mod common;

use common::*;

// ------------------------------------------------------------------ fixture

struct Case {
    name: String,
    kind: String,
    points: Vec<f64>,
    start: Vec<f64>,
    set: Vec<RowLabel>,
    params: Vec<f64>,
    value: f64,
    expect: String,
    gn_steps: Option<u32>,
}

impl Case {
    fn problem(&self) -> Geo {
        Geo::new(&self.kind, &self.points, self.start.clone())
    }
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
            let steps = get("gn_steps");
            out.push(Case {
                name: h[0].to_string(),
                kind: h[2].to_string(),
                points: hexes(&get("points")),
                start: hexes(&get("start")),
                set: labels(&get("set")),
                params: hexes(&get("params")),
                value: hexes(get("value").split_whitespace().next().unwrap_or("")).first().copied().unwrap_or(0.0),
                expect: get("expect"),
                gn_steps: if steps.is_empty() { None } else { Some(steps.parse().unwrap()) },
            });
        } else {
            let (k, v) = l.split_once(' ').unwrap_or((l, ""));
            fields.push((k.to_string(), v.to_string()));
        }
    }
    out
}

fn run(prob: &Geo, set: &[RowLabel], cfg: &RecompConfig) -> Result<Recomputed, CertError> {
    recompute(prob, &proposal(&prob.row_labels(), set), 0, cfg)
}

/// The recomputation's Gauss-Newton iteration replayed with the same calls in the same order
/// (sorted set, `start_from_set`, `lstsq(J, r, rel_tol)`, `p_j -= x_j`): the final parameters
/// and the last step, bit for bit.
fn replay(prob: &Geo, set: &[RowLabel], steps: u32) -> (Vec<f64>, Vec<f64>) {
    let mut sorted = set.to_vec();
    sorted.sort_unstable();
    let mut p = prob.start_from_set(&sorted).unwrap();
    let mut step = Vec::new();
    for _ in 0..steps {
        let (r, j) = prob.residuals_and_jacobian(&sorted, &p).unwrap();
        step = lstsq(&j, &r, DEFAULT_REL_TOL).unwrap().x;
        for (pi, xi) in p.iter_mut().zip(&step) {
            *pi -= xi;
        }
    }
    (p, step)
}

// ------------------------------------------------------------------ tests

#[test]
fn fixture_counts() {
    let cases = parse();
    let count = |p: &str| cases.iter().filter(|c| c.name.starts_with(p)).count();
    assert_eq!((count("arc-"), count("arc-far-"), count("cyl-long-"), count("cyl-far-")), (32, 16, 16, 16));
    assert_eq!(cases.iter().filter(|c| c.expect == "no_convergence").count(), 1);
    assert_eq!(cases.len(), 65);
}

/// At least this many candidates are floor cases (the step-only rule refused them).
const MIN_FLOOR: usize = 19;

/// Every generated candidate (no selection) recomputes with the replayed parameters and a proven
/// bound that contains the exact optimum, and at least [`MIN_FLOOR`] of them are floor cases: the
/// step-only rule (`s <= 4 u ||p||`) would refuse them. A floor case is accepted only within the
/// new allowance. Then `certify` runs end to end and passes for every candidate, the cylinders
/// 300 mm from the origin included (KRN.SOLV.CERT.KKT proves stationarity from the multiplier
/// enclosure; the residual at the float multipliers is information only).
#[test]
fn all_candidates_recompute() {
    let cfg = RecompConfig::default();
    let mut floor = Vec::new();
    for c in parse().iter().filter(|c| c.expect == "certify") {
        let prob = c.problem();
        let (p, step) = replay(&prob, &c.set, cfg.gn_steps);
        let s = red::norm2(&step).unwrap();
        let old_thr = STEP_TOL * red::norm2(&p).unwrap();

        let r = run(&prob, &c.set, &cfg).unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
        assert_eq!(bits(r.params()), bits(&p), "{}", c.name);
        let scale = c.params.iter().fold(0.0_f64, |a, v| if v.abs() > a { v.abs() } else { a });
        for (i, (&g, &w)) in r.params().iter().zip(&c.params).enumerate() {
            check_bound(&c.name, &format!("param {i}"), g, w, r.param_bounds()[i]);
        }
        check_bound(&c.name, "value", r.value(), c.value, r.value_bound());
        assert!(r.param_bounds().iter().all(|&b| b <= 1e-9 * scale), "{}: {:?}", c.name, r.param_bounds());
        if s > old_thr {
            floor.push(c.name.clone());
            // Accepted only within the allowance 4 u ||p|| + 2 ||k||.
            let k = red::norm2(r.param_bounds()).unwrap();
            assert!(s <= old_thr + 2.0 * k, "{}", c.name);
        }

        let (conic_p, sol) = conic(&prob.row_labels(), &c.set);
        let cert = certify(&prob, &conic_p, &sol, CertKind::Zone, &CertConfig::default())
            .unwrap_or_else(|e| panic!("{}: certify {e:?}", c.name));
        assert_eq!(bits(cert.params()), bits(r.params()), "{}", c.name);
    }
    eprintln!("floor cases ({}): {floor:?}", floor.len());
    assert!(floor.len() >= MIN_FLOOR, "only {} floor cases: {floor:?}", floor.len());
}

/// The unconverged case: its step budget leaves a last step far above both `4 u ||p||` and twice
/// any proven radius around the final point, so it is still refused.
#[test]
fn unconverged_case_still_fails() {
    let c = parse().into_iter().find(|c| c.expect == "no_convergence").unwrap();
    let cfg = RecompConfig { gn_steps: c.gn_steps.unwrap(), ..RecompConfig::default() };
    let prob = c.problem();
    let (_, step) = replay(&prob, &c.set, cfg.gn_steps);
    let s = red::norm2(&step).unwrap();
    assert_eq!(run(&prob, &c.set, &cfg), Err(CertError::RecomputeNoConvergence { step_norm: s }));
    // With the default budget the same start converges and certifies.
    let r = run(&prob, &c.set, &RecompConfig::default()).unwrap();
    for (i, (&g, &w)) in r.params().iter().zip(&c.params).enumerate() {
        check_bound(&c.name, &format!("param {i}"), g, w, r.param_bounds()[i]);
    }
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

/// Output-bit pin of every case (its own step budget), recorded from a local x86-64 run and
/// checked by CI on x86-64 Linux and Windows, ARM64 macOS and wasm32 under wasmtime
/// (REQ-KRN.SOLV.CERT.RECOMP-005: a target that diverges is a failure, not a new pin).
const OUTPUT_PIN: &str = "sha256:ac75dc549609915e1aee1f471ea8611c44927fdbc046cdaf9aa21621af82b714";

#[test]
fn output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for c in &parse() {
        let cfg = RecompConfig {
            gn_steps: c.gn_steps.unwrap_or(RecompConfig::default().gn_steps),
            ..RecompConfig::default()
        };
        digest(&c.name, &run(&c.problem(), &c.set, &cfg), &mut h);
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
