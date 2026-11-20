//! KRN.SOLV.CERT.KKT stationarity on cylinders far from the origin: proven from the multiplier
//! enclosure, so a true optimum certifies even when the residual at the float multipliers is
//! above its threshold, and non-optimal sets still do not certify.
//!
//! Expected values come from `fixtures/solv/cert/kkt-far-v1/make_cases.py` (the 16 `cyl-far`
//! cylinders of `recomp-floor-v1`, 90-digit optima and exact multipliers, and exchanged sets
//! whose primal or dual violation is decided with a wide margin; independent of this crate, of
//! any solver and of the platform libm). The test problem (plain-f64 residuals, every enclosure)
//! is shared with `cert_recomp_floor` in `common`. `OUTPUT_PIN` is a regression pin of this
//! crate's own output (not an oracle); CI checks it on x86-64 Linux and Windows, ARM64 macOS and
//! wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_solv::cert::active::RowLabel;
use fid_solv::cert::kkt::{check, Condition, KktFail, KktReport, KktStatus, KktTolerances, Verdict};
use fid_solv::cert::recomp::{recompute, ActiveSetProblem, RecompConfig, Recomputed};
use fid_solv::cert::{certify, CertConfig, CertError, CertKind};
use fid_testkit::Sha256Hasher;

mod common;

use common::*;

const FIXTURE: &str = include_str!("../../../fixtures/solv/cert/kkt-far-v1/cases.txt");

struct Case {
    name: String,
    points: Vec<f64>,
    start: Vec<f64>,
    set: Vec<RowLabel>,
    params: Vec<f64>,
    multipliers: Vec<f64>,
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
            let get = |k: &str| fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default();
            out.push(Case {
                name: head.split_whitespace().next().unwrap().to_string(),
                points: hexes(&get("points")),
                start: hexes(&get("start")),
                set: labels(&get("set")),
                params: hexes(&get("params")),
                multipliers: hexes(&get("multipliers")),
                expect: get("expect"),
            });
        } else {
            let (k, v) = l.split_once(' ').unwrap_or((l, ""));
            fields.push((k.to_string(), v.to_string()));
        }
    }
    out
}

fn problem(c: &Case) -> Geo {
    Geo::new("cyl", &c.points, c.start.clone())
}

fn recomputed(prob: &Geo, set: &[RowLabel]) -> Recomputed {
    recompute(prob, &proposal(&prob.row_labels(), set), 0, &RecompConfig::default()).unwrap()
}

fn report(c: &Case) -> (Geo, Recomputed, KktReport) {
    let prob = problem(c);
    let rec = recomputed(&prob, &c.set);
    let rep = check(&prob, &rec, &KktTolerances::default()).unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
    (prob, rec, rep)
}

#[test]
fn fixture_counts() {
    let cases = parse();
    let n = |e: &str| cases.iter().filter(|c| c.expect == e).count();
    assert_eq!((n("certified"), n("violated primal"), n("violated dual")), (16, 3, 3));
}

/// The 16 cylinders 300 mm from the origin certify: the multipliers are proven, so stationarity
/// is `[0, 0]` at `lambda*`, and the proven multiplier box holds the exact multipliers. The
/// float-multiplier residual is recorded for information; for these cases it is above the
/// threshold (which is why they failed before), and it does not decide the verdict.
#[test]
fn far_cylinders_certify() {
    let mut float_above = 0;
    for c in parse().iter().filter(|c| c.expect == "certified") {
        let (prob, rec, rep) = report(c);
        assert_eq!(rep.status(), KktStatus::Certified, "{}: {rep:?}", c.name);
        assert_eq!(rep.reason(), None, "{}", c.name);
        let st = rep.stationarity();
        assert_eq!((st.lo(), st.hi(), st.verdict()), (0.0, 0.0, Verdict::Satisfied), "{}", c.name);
        let sf = rep.stationarity_float();
        assert!(sf.lo() >= 0.0 && sf.lo() <= sf.hi() && sf.hi().is_finite(), "{}", c.name);
        assert_eq!(sf.threshold().to_bits(), st.threshold().to_bits(), "{}", c.name);
        if sf.verdict() != Verdict::Satisfied {
            float_above += 1;
        }
        for (i, ((&l, &b), &w)) in rep.multipliers().iter().zip(rep.multiplier_bounds()).zip(&c.multipliers).enumerate()
        {
            check_bound(&c.name, &format!("multiplier {i}"), l, w, b);
        }
        for (i, (&g, &w)) in rec.params().iter().zip(&c.params).enumerate() {
            check_bound(&c.name, &format!("param {i}"), g, w, rec.param_bounds()[i]);
        }

        let (conic_p, sol) = conic(&prob.row_labels(), &c.set);
        let cert = certify(&prob, &conic_p, &sol, CertKind::Zone, &CertConfig::default())
            .unwrap_or_else(|e| panic!("{}: certify {e:?}", c.name));
        let k = cert.certificate().kkt();
        assert_eq!((k.stationarity.lo, k.stationarity.hi), (0.0, 0.0), "{}", c.name);
        assert_eq!(k.stationarity_float.hi.to_bits(), sf.hi().to_bits(), "{}", c.name);
        assert_eq!(bits(cert.params()), bits(rec.params()), "{}", c.name);
    }
    // The old rule (stationarity at the float multipliers) refused every one of them.
    assert_eq!(float_above, 16);
}

/// Non-optimal exchanged sets still do not certify, with the failing condition the oracle
/// decided: primal (a row outside the zone by at least 1e-5) or dual (a multiplier at most
/// -1e-3 with every other row inside by at least 1e-5). Stationarity is proven there too, so
/// only the other conditions can refuse them, and they do.
#[test]
fn exchanged_sets_do_not_certify() {
    for c in parse().iter().filter(|c| c.expect.starts_with("violated")) {
        let (prob, rec, rep) = report(c);
        for (i, (&g, &w)) in rec.params().iter().zip(&c.params).enumerate() {
            check_bound(&c.name, &format!("param {i}"), g, w, rec.param_bounds()[i]);
        }
        assert_eq!(rep.status(), KktStatus::Violated, "{}: {rep:?}", c.name);
        let (want, cond): (KktFail, Condition) = match c.expect.as_str() {
            "violated primal" => (KktFail::Primal, rep.primal()),
            "violated dual" => (KktFail::Dual, rep.dual()),
            e => panic!("{e}"),
        };
        assert_eq!(rep.reason(), Some(want), "{}", c.name);
        assert_eq!(cond.verdict(), Verdict::Violated, "{}", c.name);
        if want == KktFail::Dual {
            assert_eq!(rep.primal().verdict(), Verdict::Satisfied, "{}", c.name);
            // The exact negative multiplier lies in the proven box.
            for (i, ((&l, &b), &w)) in
                rep.multipliers().iter().zip(rep.multiplier_bounds()).zip(&c.multipliers).enumerate()
            {
                check_bound(&c.name, &format!("multiplier {i}"), l, w, b);
            }
        }
        let (conic_p, sol) = conic(&prob.row_labels(), &c.set);
        match certify(&prob, &conic_p, &sol, CertKind::Zone, &CertConfig::default()) {
            Err(CertError::CertificateFailed { tried: 1, first_report: Some(r), first_error: None }) => {
                assert_eq!(r.reason(), Some(want), "{}", c.name)
            }
            other => panic!("{}: certify {other:?}", c.name),
        }
    }
}

fn put(h: &mut Sha256Hasher, c: Condition) {
    for v in [c.lo(), c.hi(), c.threshold()] {
        h.update(&v.to_bits().to_le_bytes());
    }
    h.update(format!("{:?}", c.verdict()).as_bytes());
}

/// Output-bit pin of every report, recorded from a local x86-64 run and checked by CI on x86-64
/// Linux and Windows, ARM64 macOS and wasm32 (REQ-KRN.SOLV.CERT.KKT-005).
const OUTPUT_PIN: &str = "sha256:a9de63b01c4517fa722de6814dfee914ce86da9d0e52cb0037324e6e0e7c0a27";

#[test]
fn output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for c in &parse() {
        let (_, _, r) = report(c);
        h.update(c.name.as_bytes());
        h.update(format!("{:?} {:?}", r.status(), r.reason()).as_bytes());
        for v in r.multipliers().iter().chain(r.multiplier_bounds()).chain(r.dual_thresholds()) {
            h.update(&v.to_bits().to_le_bytes());
        }
        for cond in [r.primal(), r.dual(), r.stationarity(), r.stationarity_float(), r.complementarity()] {
            put(&mut h, cond);
        }
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
