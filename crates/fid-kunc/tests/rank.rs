//! Acceptance criteria of spec node UNC.PROP.RANK (AC-1 .. AC-5) plus the rule checks of
//! `fid_kunc::prop::rank`.
//!
//! Expected values come from the independent Python reference
//! `fixtures/unc/rank/make_fixtures.py` (its own emulation of the fixed-order reductions,
//! exact-rational square roots and references, its own Philox4x32-10, and the canonical
//! JSON written with `json.dumps`), never from this crate, or are hand-labelled from the rules
//! in the module documentation (the refusal cases). AC-5 (bit identity on x86-64, ARM64 and
//! wasm32) is the byte comparison with the Python canonical JSON, run by CI on every target.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kci::kci::sha256_digest;
use fid_kunc::dist::{Distribution, Quantity, UnitCode};
use fid_kunc::prop::rank::{
    rank, Degenerate, RankError, RankInput, RankMethod, RankOutput, Ranked, ScoreAbsent, Shares, ALGORITHM_ID,
    SHARE_SUM_TOL,
};
use fid_num::NumError;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/rank/");

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

// ------------------------------------------------------------------ a small JSON reader

/// The fixtures hold only objects, arrays and strings without escapes.
#[derive(Debug, Clone)]
enum J {
    S(String),
    A(Vec<J>),
    O(Vec<(String, J)>),
}

impl J {
    fn get(&self, key: &str) -> &J {
        self.opt(key).unwrap_or_else(|| panic!("missing {key}"))
    }
    fn opt(&self, key: &str) -> Option<&J> {
        match self {
            J::O(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => panic!("not an object at {key}"),
        }
    }
    fn s(&self) -> &str {
        match self {
            J::S(s) => s,
            _ => panic!("not a string"),
        }
    }
    fn a(&self) -> &[J] {
        match self {
            J::A(v) => v,
            _ => panic!("not an array"),
        }
    }
    fn f(&self) -> f64 {
        f64::from_bits(u64::from_str_radix(self.s(), 16).unwrap())
    }
    fn strs(&self) -> Vec<String> {
        self.a().iter().map(|x| x.s().to_owned()).collect()
    }
    fn fs(&self) -> Vec<f64> {
        self.a().iter().map(J::f).collect()
    }
    /// An array of hex doubles or `"absent"`.
    fn opt_fs(&self) -> Vec<Option<f64>> {
        self.a().iter().map(|x| if x.s() == "absent" { None } else { Some(x.f()) }).collect()
    }
}

fn parse_json(b: &[u8]) -> J {
    fn ws(b: &[u8], i: &mut usize) {
        while *i < b.len() && b[*i].is_ascii_whitespace() {
            *i += 1;
        }
    }
    fn string(b: &[u8], i: &mut usize) -> String {
        assert_eq!(b[*i], b'"');
        let start = *i + 1;
        let end = start + b[start..].iter().position(|&c| c == b'"').unwrap();
        assert!(!b[start..end].contains(&b'\\'));
        *i = end + 1;
        String::from_utf8(b[start..end].to_vec()).unwrap()
    }
    fn value(b: &[u8], i: &mut usize) -> J {
        ws(b, i);
        match b[*i] {
            b'"' => J::S(string(b, i)),
            b'[' => {
                *i += 1;
                let mut v = Vec::new();
                loop {
                    ws(b, i);
                    if b[*i] == b']' {
                        *i += 1;
                        return J::A(v);
                    }
                    v.push(value(b, i));
                    ws(b, i);
                    if b[*i] == b',' {
                        *i += 1;
                    }
                }
            }
            b'{' => {
                *i += 1;
                let mut m = Vec::new();
                loop {
                    ws(b, i);
                    if b[*i] == b'}' {
                        *i += 1;
                        return J::O(m);
                    }
                    let k = string(b, i);
                    ws(b, i);
                    assert_eq!(b[*i], b':');
                    *i += 1;
                    m.push((k, value(b, i)));
                    ws(b, i);
                    if b[*i] == b',' {
                        *i += 1;
                    }
                }
            }
            c => panic!("unexpected byte {c}"),
        }
    }
    let mut i = 0;
    value(b, &mut i)
}

/// The Python reference's canonical JSON of each case.
fn expected_canonical(case: &str) -> String {
    let text = String::from_utf8(read("expected_canonical.txt")).unwrap();
    assert!(!text.contains('\r'), "expected_canonical.txt must have LF line endings");
    let line = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .find(|l| l.split_once(' ').map(|(k, _)| k) == Some(case))
        .unwrap_or_else(|| panic!("no canonical line for {case}"));
    line.split_once(' ').unwrap().1.to_owned()
}

fn assert_canonical(case: &str, out: &RankOutput) {
    let got = String::from_utf8(out.canonical_json().unwrap()).unwrap();
    assert_eq!(got, expected_canonical(case), "{case}: canonical JSON (AC-5)");
    assert_eq!(out.digest().unwrap(), sha256_digest(got.as_bytes()));
}

fn order(out: &RankOutput) -> Vec<&str> {
    out.contributors().iter().map(Ranked::input_id).collect()
}

fn ties(out: &RankOutput) -> Vec<String> {
    out.contributors().iter().map(|r| r.tie_broken_by_id().to_string()).collect()
}

fn degenerate_code(out: &RankOutput) -> &'static str {
    out.degenerate().map_or("none", Degenerate::code)
}

// ------------------------------------------------------------------ LPU cases

fn run_lpu(case: &J) -> Result<RankOutput, RankError> {
    let ids = case.get("ids").strs();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let c = case.get("c").fs();
    let cov = case.get("cov").fs();
    rank(&RankInput::Lpu { c: &c, cov: &cov, ids: &ids })
}

/// Checks an LPU case against every expected member of the fixture.
fn check_lpu_case(case: &J) -> Option<RankOutput> {
    let name = case.get("name").s().to_owned();
    let res = run_lpu(case);
    if let Some(err) = case.opt("error") {
        let e = res.expect_err(&name);
        assert_eq!(e.code(), err.s(), "{name}");
        if let Some(pair) = case.opt("pair") {
            let p: Vec<usize> = pair.strs().iter().map(|x| x.parse().unwrap()).collect();
            assert_eq!(e, RankError::NotPositiveSemidefinite { i: p[0], j: p[1] }, "{name}");
        }
        return None;
    }
    let out = res.unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(out.method(), RankMethod::Lpu);
    assert_eq!(out.method().code(), "lpu");
    assert_eq!(out.trials(), None);
    assert_eq!(order(&out), case.get("order").strs(), "{name}: order (REQ-003)");
    assert_eq!(ties(&out), case.get("tie_broken_by_id").strs(), "{name}: ties");
    let u: Vec<u64> = out.contributors().iter().map(|r| r.u_contrib().unwrap().to_bits()).collect();
    assert_eq!(u, case.get("u_contrib").fs().iter().map(|x| x.to_bits()).collect::<Vec<_>>(), "{name}: u_i(y)");
    let shares: Vec<Option<u64>> = out.contributors().iter().map(|r| r.share().map(f64::to_bits)).collect();
    let want: Vec<Option<u64>> = case.get("share").opt_fs().iter().map(|x| x.map(f64::to_bits)).collect();
    assert_eq!(shares, want, "{name}: shares");
    assert_eq!(degenerate_code(&out), case.get("degenerate").s(), "{name}");
    assert_eq!(out.shares().unwrap().code(), case.get("shares").s(), "{name}");
    let sum = case.get("share_sum");
    assert_eq!(out.share_sum().map(f64::to_bits), (sum.s() != "absent").then(|| sum.f().to_bits()), "{name}");
    for r in out.contributors() {
        assert_eq!(r.r2(), None);
        assert_eq!(r.r2_absent(), None);
    }
    assert_canonical(&name, &out);
    Some(out)
}

/// `|got - ref| <= tol` against a 50-digit decimal reference (parsed correctly rounded).
fn near_ref(got: f64, reference: &str, tol: f64) -> bool {
    let r: f64 = reference.parse().unwrap();
    (got - r).abs() <= tol
}

/// AC-1: order x3 then x1/x2 by ID, shares (0.01, 0.01, 0.04) / 0.06 within 1e-15, summing
/// to 1.
#[test]
fn ac1_lpu_3in() {
    let case = parse_json(&read("lpu_3in.json"));
    let out = check_lpu_case(&case).unwrap();
    assert_eq!(order(&out), ["x3", "x1", "x2"]);
    assert_eq!(ties(&out), ["false", "false", "true"], "x1/x2 tie broken by ID");
    let tol = case.get("share_tol").f();
    assert_eq!(tol, 1e-15);
    let want = [0.04 / 0.06, 0.01 / 0.06, 0.01 / 0.06];
    for ((r, w), reference) in out.contributors().iter().zip(want).zip(case.get("share_ref50").strs()) {
        let s = r.share().unwrap();
        assert!((s - w).abs() <= tol, "{} share {s} vs {w}", r.input_id());
        assert!(near_ref(s, &reference, tol), "{} share {s} vs {reference}", r.input_id());
    }
    assert_eq!(out.shares(), Some(Shares::Present));
    assert!((out.share_sum().unwrap() - 1.0).abs() <= SHARE_SUM_TOL);
    assert_eq!(out.degenerate(), None);
    let u: Vec<f64> = out.contributors().iter().map(|r| r.u_contrib().unwrap()).collect();
    assert_eq!(u, [0.2, 0.1, 0.1]);
    let c: Vec<f64> = out.contributors().iter().map(|r| r.sensitivity().unwrap()).collect();
    assert_eq!(c, [-1.0, 1.0, 2.0]);
}

/// AC-2: two inputs with rho = -0.9; shares match the 50-digit reference within 1e-14 and
/// sum to 1 (one share is negative: the Euler share is correlation-aware).
#[test]
fn ac2_lpu_corr() {
    let case = parse_json(&read("lpu_corr.json"));
    let out = check_lpu_case(&case).unwrap();
    let tol = case.get("share_tol").f();
    assert_eq!(tol, 1e-14);
    for (r, reference) in out.contributors().iter().zip(case.get("share_ref50").strs()) {
        let s = r.share().unwrap();
        assert!(near_ref(s, &reference, tol), "{} share {s} vs {reference}", r.input_id());
    }
    assert!((out.share_sum().unwrap() - 1.0).abs() <= SHARE_SUM_TOL);
    assert!(out.contributors().iter().any(|r| r.share().unwrap() < 0.0));
}

/// AC-4 and the other degenerate, tie and tolerance rules of the LPU form.
#[test]
fn ac4_and_lpu_rules() {
    let rules = parse_json(&read("rules.json"));
    let mut seen = Vec::new();
    for case in rules.get("cases").a() {
        let name = case.get("name").s();
        let out = check_lpu_case(case);
        seen.push(name.to_owned());
        match name {
            "all_zero_c" => {
                // AC-4: all sensitivities zero: shares absent and degenerate set.
                let out = out.unwrap();
                assert_eq!(out.degenerate(), Some(Degenerate::ZeroVariance));
                assert_eq!(out.shares(), Some(Shares::Degenerate));
                assert_eq!(out.share_sum(), None);
                assert!(out.contributors().iter().all(|r| r.share().is_none()));
            }
            "rounding_resolution" => {
                assert_eq!(out.unwrap().degenerate(), Some(Degenerate::VarianceAtRoundingResolution));
            }
            "not_psd" | "rho_1_5" | "indefinite_3x3" => assert!(out.is_none()),
            "subnormal_variance" => {
                // Review of #685: a subnormal c^T S c is never a confident pair of shares.
                let out = out.unwrap();
                assert_eq!(out.degenerate(), Some(Degenerate::VarianceAtRoundingResolution));
                assert_eq!(out.shares(), Some(Shares::Degenerate));
            }
            "share_sum_out_of_tolerance" => {
                let out = out.unwrap();
                assert_eq!(out.shares(), Some(Shares::SumOutOfTolerance));
                assert!((out.share_sum().unwrap() - 1.0).abs() > SHARE_SUM_TOL);
                assert!(out.contributors().iter().all(|r| r.share().is_none()));
                assert_eq!(out.degenerate(), None);
            }
            _ => {}
        }
    }
    for want in [
        "all_zero_c",
        "perfect_cancel",
        "rounding_resolution",
        "not_psd",
        "rho_1_5",
        "indefinite_3x3",
        "subnormal_variance",
        "negative_share",
        "single_input",
        "share_sum_out_of_tolerance",
    ] {
        assert!(seen.iter().any(|s| s == want), "case {want} missing");
    }
    let not_psd = rules.get("cases").a().iter().find(|c| c.get("name").s() == "not_psd").unwrap();
    assert_eq!(run_lpu(not_psd).unwrap_err(), RankError::NotPositiveSemidefinite { i: 0, j: 1 });
}

// ------------------------------------------------------------------ samples

fn run_samples(case: &J) -> RankOutput {
    let ids = case.get("ids").strs();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let cols: Vec<Vec<f64>> = case.get("inputs").a().iter().map(J::fs).collect();
    let cols: Vec<&[f64]> = cols.iter().map(Vec::as_slice).collect();
    let output = case.get("output").fs();
    rank(&RankInput::Samples { inputs: &cols, output: &output, ids: &ids }).unwrap()
}

/// The degenerate, constant-input and tie rules of the samples form (REQ-002..004).
#[test]
fn samples_rules() {
    let rules = parse_json(&read("rules.json"));
    let cases = rules.get("samples").a();
    assert_eq!(cases.len(), 5);
    for case in cases {
        let name = case.get("name").s();
        let out = run_samples(case);
        assert_eq!(out.method(), RankMethod::ApproximateSrc2);
        assert_eq!(out.shares(), None);
        assert_eq!(out.share_sum(), None);
        assert_eq!(order(&out), case.get("order").strs(), "{name}");
        assert_eq!(ties(&out), case.get("tie_broken_by_id").strs(), "{name}");
        let r2: Vec<Option<u64>> = out.contributors().iter().map(|r| r.r2().map(f64::to_bits)).collect();
        let want: Vec<Option<u64>> = case.get("r2").opt_fs().iter().map(|x| x.map(f64::to_bits)).collect();
        assert_eq!(r2, want, "{name}");
        let absent: Vec<&str> =
            out.contributors().iter().map(|r| r.r2_absent().map_or("none", ScoreAbsent::code)).collect();
        assert_eq!(absent, case.get("r2_absent").strs(), "{name}");
        assert_eq!(degenerate_code(&out), case.get("degenerate").s(), "{name}");
        for r in out.contributors() {
            assert_eq!((r.sensitivity(), r.u_contrib(), r.share()), (None, None, None));
        }
        if let Some(refs) = case.opt("r2_ref50") {
            for (r, reference) in out.contributors().iter().zip(refs.strs()) {
                assert!(near_ref(r.r2().unwrap(), &reference, 1e-15), "{name}: {}", r.input_id());
            }
        }
        assert_canonical(name, &out);
    }
}

/// AC-3: 10^5 trials of Y = X1 + 3 X2 drawn with UNC.MODEL.DIST at seed 0x52414E4B: X2 ranks
/// first and the method is `approximate_src2`. The sample set must equal the Python
/// reference's (SHA-256 of the three columns) before it is ranked.
#[test]
fn ac3_mcm_samples() {
    let case = parse_json(&read("mcm_samples.json"));
    let seed = u64::from_str_radix(case.get("seed").s(), 16).unwrap();
    assert_eq!(seed, 0x5241_4E4B);
    let n: u64 = case.get("trials").s().parse().unwrap();
    assert_eq!(n, 100_000);
    let (a, b) = (case.get("a").f(), case.get("b").f());
    let d = Distribution::rect(Quantity::new(a, UnitCode::Metre), Quantity::new(b, UnitCode::Metre)).unwrap();
    let x1 = d.sample(seed, 0, 0..n).unwrap();
    let x2 = d.sample(seed, 1, 0..n).unwrap();
    let y: Vec<f64> = x1.iter().zip(&x2).map(|(p, q)| p + 3.0 * q).collect();
    let mut bytes = Vec::with_capacity(3 * 8 * x1.len());
    for col in [&x1, &x2, &y] {
        for v in col.iter() {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    assert_eq!(sha256_digest(&bytes).to_string(), format!("sha256:{}", case.get("samples_sha256").s()));

    let ids = ["X1", "X2"];
    let out = rank(&RankInput::Samples { inputs: &[&x1, &x2], output: &y, ids: &ids }).unwrap();
    assert_eq!(order(&out), ["X2", "X1"], "X2 ranks first");
    assert_eq!(out.method(), RankMethod::ApproximateSrc2);
    assert_eq!(out.method().code(), case.get("method").s());
    assert_eq!(out.trials(), Some(n));
    assert_eq!(out.degenerate(), None);
    let r2: Vec<u64> = out.contributors().iter().map(|r| r.r2().unwrap().to_bits()).collect();
    assert_eq!(r2, case.get("r2").fs().iter().map(|x| x.to_bits()).collect::<Vec<_>>());
    for (r, reference) in out.contributors().iter().zip(case.get("r2_ref50").strs()) {
        assert!(near_ref(r.r2().unwrap(), &reference, 1e-13), "{}", r.input_id());
    }
    assert_canonical("mcm_samples", &out);
    let json = String::from_utf8(out.canonical_json().unwrap()).unwrap();
    assert!(json.contains("\"basis\":\"simulated\""), "D-045: sample-based rankings are labelled simulated");
}

// ------------------------------------------------------------------ refusals (hand-labelled)

fn lpu(c: &[f64], cov: &[f64], ids: &[&str]) -> Result<RankOutput, RankError> {
    rank(&RankInput::Lpu { c, cov, ids })
}

fn samples(inputs: &[&[f64]], output: &[f64], ids: &[&str]) -> Result<RankOutput, RankError> {
    rank(&RankInput::Samples { inputs, output, ids })
}

#[test]
fn lpu_refusals() {
    let i2 = [1.0, 0.0, 0.0, 1.0];
    assert_eq!(lpu(&[], &[], &[]).unwrap_err(), RankError::NoInputs);
    assert_eq!(
        lpu(&[1.0, 1.0], &i2, &["a"]).unwrap_err(),
        RankError::DimensionMismatch { what: "ids", expected: 2, found: 1, index: None }
    );
    assert_eq!(
        lpu(&[1.0, 1.0], &[1.0, 0.0, 1.0], &["a", "b"]).unwrap_err(),
        RankError::DimensionMismatch { what: "cov", expected: 4, found: 3, index: None }
    );
    for bad in ["", "a b", "q\"", "tab\t", "x\u{7f}", "\u{feff}x", "é"] {
        assert_eq!(lpu(&[1.0, 1.0], &i2, &["ok", bad]).unwrap_err(), RankError::InvalidId { index: 1 }, "{bad:?}");
    }
    let long = "x".repeat(257);
    assert_eq!(lpu(&[1.0, 1.0], &i2, &[&long, "b"]).unwrap_err(), RankError::InvalidId { index: 0 });
    let id4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    assert_eq!(
        lpu(&[1.0; 4], &id4, &["b", "a", "b", "a"]).unwrap_err(),
        RankError::DuplicateId { index: 2 },
        "lowest index that repeats an earlier ID"
    );
    assert_eq!(
        lpu(&[1.0, f64::NAN], &i2, &["a", "b"]).unwrap_err(),
        RankError::Numeric { input: None, error: NumError::NotANumber { op: "rank.c", index: Some(1) } }
    );
    assert_eq!(
        lpu(&[1.0, 1.0], &[1.0, 0.0, 0.0, f64::INFINITY], &["a", "b"]).unwrap_err(),
        RankError::Numeric { input: None, error: NumError::Infinite { op: "rank.cov", index: Some(3) } }
    );
    assert_eq!(
        lpu(&[1.0, 1.0], &[1.0, 0.5, 0.25, 1.0], &["a", "b"]).unwrap_err(),
        RankError::NotSymmetric { i: 0, j: 1 }
    );
    assert_eq!(
        lpu(&[1.0, 1.0], &[1.0, 0.0, 0.0, -1e-300], &["a", "b"]).unwrap_err(),
        RankError::NegativeVariance { index: Some(1) }
    );
    // An overflowing u_i(y) and an overflowing product inside a dot are numeric, never inf.
    let big = f64::MAX;
    let e = lpu(&[big], &[4.0], &["a"]).unwrap_err();
    assert_eq!(e.code(), "numeric");
    let e = lpu(&[big, 1.0], &[1.0, 2.0, 2.0, 5.0], &["a", "b"]).unwrap_err();
    assert_eq!(e.code(), "numeric");
}

#[test]
fn samples_refusals() {
    let x = [1.0, 2.0, 3.0];
    let y = [2.0, 4.0, 7.0];
    assert_eq!(samples(&[], &y, &[]).unwrap_err(), RankError::NoInputs);
    assert_eq!(
        samples(&[&x], &y, &["a", "b"]).unwrap_err(),
        RankError::DimensionMismatch { what: "ids", expected: 1, found: 2, index: None }
    );
    assert_eq!(samples(&[&x[..1]], &y[..1], &["a"]).unwrap_err(), RankError::TooFewSamples { found: 1 });
    assert_eq!(samples(&[&[]], &[], &["a"]).unwrap_err(), RankError::TooFewSamples { found: 0 });
    assert_eq!(
        samples(&[&x, &x[..2]], &y, &["a", "b"]).unwrap_err(),
        RankError::DimensionMismatch { what: "input", expected: 3, found: 2, index: Some(1) }
    );
    assert_eq!(samples(&[&x, &x], &y, &["a", "a"]).unwrap_err(), RankError::DuplicateId { index: 1 });
    assert_eq!(
        samples(&[&x], &[2.0, f64::NAN, 1.0], &["a"]).unwrap_err(),
        RankError::Numeric { input: None, error: NumError::NotANumber { op: "rank.output", index: Some(1) } }
    );
    assert_eq!(
        samples(&[&x, &[1.0, f64::NEG_INFINITY, 0.0]], &y, &["a", "b"]).unwrap_err(),
        RankError::Numeric { input: Some(1), error: NumError::Infinite { op: "rank.input", index: Some(1) } }
    );
    // Deviations that overflow are numeric, naming the input.
    let e = samples(&[&[f64::MAX, -f64::MAX, 0.0]], &y, &["a"]).unwrap_err();
    assert!(matches!(e, RankError::Numeric { input: Some(0), .. }), "{e:?}");
}

/// Constant columns are caught bitwise, before any mean (module rule 3): the rounded mean of
/// three values 0.1 is not 0.1.
#[test]
fn constant_columns_never_produce_a_correlation() {
    let c = [0.1, 0.1, 0.1];
    let x = [1.0, 2.0, 4.0];
    let out = samples(&[&c, &x], &[3.0, 1.0, 2.0], &["c", "x"]).unwrap();
    assert_eq!(order(&out), ["x", "c"]);
    assert_eq!(out.contributors()[1].r2(), None);
    assert_eq!(out.contributors()[1].r2_absent(), Some(ScoreAbsent::ZeroInputVariance));
    let out = samples(&[&x], &c, &["x"]).unwrap();
    assert_eq!(out.degenerate(), Some(Degenerate::ZeroOutputVariance));
    assert_eq!(out.contributors()[0].r2_absent(), Some(ScoreAbsent::DegenerateOutput));
}

/// A perfectly correlated input has r2 exactly 1 and never above it.
#[test]
fn r2_never_exceeds_one() {
    let x: Vec<f64> = (0..1000).map(|i| f64::from(i) * 0.1 + 0.3).collect();
    let y: Vec<f64> = x.iter().map(|v| -7.0 * v).collect();
    let out = samples(&[&x], &y, &["x"]).unwrap();
    let r2 = out.contributors()[0].r2().unwrap();
    assert!(r2 <= 1.0 && r2 > 1.0 - 1e-12, "{r2}");
}

/// The covariance must be positive semi-definite (review of #685): rho = 1.5 fails the
/// pairwise Cauchy-Schwarz test; a pairwise-valid but indefinite 3x3 fails the Cholesky
/// remainder; singular positive semi-definite matrices pass; extreme scales never let an
/// overflow or underflow decide.
#[test]
fn positive_semidefinite_rules() {
    // u = (1, 2), rho = 1.5: S_12 = 3.
    assert_eq!(
        lpu(&[1.0, 1.0], &[1.0, 3.0, 3.0, 4.0], &["a", "b"]).unwrap_err(),
        RankError::NotPositiveSemidefinite { i: 0, j: 1 }
    );
    // A zero variance with a non-zero covariance.
    assert_eq!(
        lpu(&[1.0, 1.0], &[0.0, 1e-300, 1e-300, 1.0], &["a", "b"]).unwrap_err(),
        RankError::NotPositiveSemidefinite { i: 0, j: 1 }
    );
    // rho = 10 at scales where s * s underflows to zero: decided by the scaled form.
    assert_eq!(
        lpu(&[1.0, 1.0], &[1e-171, 1e-170, 1e-170, 1e-171], &["a", "b"]).unwrap_err(),
        RankError::NotPositiveSemidefinite { i: 0, j: 1 }
    );
    // rho = 2 at scales where s * s overflows.
    assert_eq!(
        lpu(&[1.0, 1.0], &[1e200, 2e200, 2e200, 1e200], &["a", "b"]).unwrap_err(),
        RankError::NotPositiveSemidefinite { i: 0, j: 1 }
    );
    // rho = 0.5 at those scales passes.
    for (d, s) in [(1e-171, 5e-172), (1e200, 5e199)] {
        assert!(lpu(&[1.0, 1.0], &[d, s, s, d], &["a", "b"]).is_ok(), "{d}");
    }
    // Exactly singular positive semi-definite matrices pass (rho = 1, rank 1 of 3).
    let one = [1.0, 2.0, 3.0, 2.0, 4.0, 6.0, 3.0, 6.0, 9.0];
    let out = lpu(&[1.0, -1.0, 0.5], &one, &["a", "b", "c"]).unwrap();
    assert_eq!(out.contributors().len(), 3);
    // Pairwise correlations 0.9, -0.9, 0.9: indefinite, found by the Cholesky remainder.
    let ind = [1.0, 0.9, -0.9, 0.9, 1.0, 0.9, -0.9, 0.9, 1.0];
    assert_eq!(lpu(&[1.0; 3], &ind, &["a", "b", "c"]).unwrap_err().code(), "not_positive_semidefinite");
}

/// Review of #685: c = (1e-155, 9.9999998e-156) with perfectly anti-correlated unit inputs
/// gave a subnormal c^T S c and shares near (404805, -404804); it is degenerate.
#[test]
fn subnormal_variance_is_degenerate() {
    let out = lpu(&[1e-155, 9.9999998e-156], &[1.0, -1.0, -1.0, 1.0], &["x1", "x2"]).unwrap();
    assert_eq!(out.degenerate(), Some(Degenerate::VarianceAtRoundingResolution));
    assert!(out.contributors().iter().all(|r| r.share().is_none()));
}

/// Sums of squares below 2^-1022 carry no r2 (review of #685).
#[test]
fn subnormal_sums_of_squares_carry_no_r2() {
    let t = [0.0, 1e-160, 0.0, 1e-160];
    let x = [1.0, 2.0, 3.0, 5.0];
    let out = samples(&[&t, &x], &[1.0, 2.0, 4.0, 4.0], &["t", "x"]).unwrap();
    assert_eq!(order(&out), ["x", "t"]);
    assert_eq!(out.contributors()[1].r2_absent(), Some(ScoreAbsent::BelowRoundingResolution));
    let out = samples(&[&x], &[0.0, 1e-160, 2e-160, 0.0], &["x"]).unwrap();
    assert_eq!(out.contributors()[0].r2_absent(), Some(ScoreAbsent::BelowRoundingResolution));
    assert_eq!(out.degenerate(), None);
}

// ------------------------------------------------------------------ statement conversion

#[test]
fn statement_contributors() {
    let out = check_lpu_case(&parse_json(&read("lpu_3in.json"))).unwrap();
    let cs = out.statement_contributors().unwrap();
    let ids: Vec<&str> = cs.iter().map(|c| c.input_id.as_str()).collect();
    assert_eq!(ids, ["x1", "x2", "x3"], "sorted by input ID, as the statement requires");
    for c in &cs {
        let r = out.contributors().iter().find(|r| r.input_id() == c.input_id).unwrap();
        assert_eq!(c.sensitivity, r.sensitivity());
        assert_eq!(Some(c.u_contrib), r.u_contrib());
        assert_eq!(Some(c.variance_share), r.share());
    }

    let rules = parse_json(&read("rules.json"));
    let case = |name: &str| rules.get("cases").a().iter().find(|c| c.get("name").s() == name).unwrap().clone();
    let neg = run_lpu(&case("negative_share")).unwrap();
    assert_eq!(
        neg.statement_contributors().unwrap_err(),
        RankError::NotConvertible { reason: "share_outside_unit_interval", input_id: Some("x2".to_owned()) },
        "shares above 1 (x2, first in rank order) and below 0 (x1) are refused, never clamped"
    );
    for name in ["all_zero_c", "share_sum_out_of_tolerance"] {
        assert_eq!(
            run_lpu(&case(name)).unwrap().statement_contributors().unwrap_err(),
            RankError::NotConvertible { reason: "shares_absent", input_id: None },
            "{name}"
        );
    }
    let s = run_samples(rules.get("samples").a().iter().find(|c| c.get("name").s() == "partial").unwrap());
    assert_eq!(
        s.statement_contributors().unwrap_err(),
        RankError::NotConvertible { reason: "approximate_src2", input_id: None }
    );
}

// ------------------------------------------------------------------ card and codes

/// The spec card parses (KRN.MAN.CARD schema and standards-text check) and matches the ID.
#[test]
fn spec_card() {
    let card = fid_kci::parse_card(include_str!("../cards/unc.prop.rank@1.0.0.toml")).unwrap();
    let id = fid_kci::AlgorithmId::parse(ALGORITHM_ID).unwrap();
    let report = fid_kci::check(&[id], std::slice::from_ref(&card.id));
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(card.determinism, "D0");
    // `rank` takes one data input, the `RankInput`; the binding stops compiling if it changes.
    let _: fn(&RankInput<'_>) -> Result<RankOutput, RankError> = rank;
    assert_eq!(card.input_arity, fid_kci::InputArity::exactly(1));
}

#[test]
fn codes() {
    assert_eq!(ALGORITHM_ID, "unc.prop.rank@1.0.0");
    assert_eq!(SHARE_SUM_TOL, 1e-12);
    assert_eq!(RankMethod::ApproximateSrc2.code(), "approximate_src2");
    assert_eq!(ScoreAbsent::BelowRoundingResolution.code(), "below_rounding_resolution");
    let errs = [
        (RankError::NoInputs, "no_inputs"),
        (RankError::DimensionMismatch { what: "ids", expected: 1, found: 0, index: None }, "dimension_mismatch"),
        (RankError::InvalidId { index: 0 }, "invalid_id"),
        (RankError::DuplicateId { index: 1 }, "duplicate_id"),
        (RankError::TooFewSamples { found: 1 }, "too_few_samples"),
        (RankError::NotSymmetric { i: 0, j: 1 }, "not_symmetric"),
        (RankError::NegativeVariance { index: None }, "negative_variance"),
        (RankError::NotPositiveSemidefinite { i: 0, j: 1 }, "not_positive_semidefinite"),
        (RankError::Numeric { input: None, error: NumError::EmptyInput { op: "x" } }, "numeric"),
        (RankError::NotConvertible { reason: "shares_absent", input_id: None }, "not_convertible"),
        (RankError::Encoding, "encoding"),
    ];
    for (e, code) in errs {
        assert_eq!(e.code(), code);
        assert!(!e.to_string().is_empty());
    }
}
