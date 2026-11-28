//! Acceptance criteria of spec node UNC.PROP.MVAR.VMCM (AC-1 .. AC-3) plus requirement and
//! rule checks.
//!
//! Expected values come from the independent Python reference
//! `fixtures/unc/mvar/vmcm/make_fixtures.py` (exact rationals, its own `mcm-samples@1` writer
//! and SHA-256 from `hashlib`), never from this crate, or are hand-labelled from the rules in
//! the module documentation (the error cases and the tie case).
//!
//! AC-1 draws 10^6 trials of the fixture's linear model with the `UNC.MODEL.DIST` samplers at
//! the `UNC.PROP.MCM.BLOCK` address `(seed, stream, t)`; the model itself (two binary64
//! additions) is evaluated here. `UNC_MVAR_VMCM_QUICK=1` runs it at 10^5 trials with the
//! tolerances widened by sqrt(10), for slow local targets; CI runs the full size.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kci::kci::sha256_digest;
use fid_kunc::dist::{Distribution, Quantity, UnitCode};
use fid_kunc::prop::mcm::block::{BlockOut, TrialFailure, FAIL_ALGORITHM};
use fid_kunc::prop::mcm::samples::{encode, SampleSetHeader, DEFAULT_CAP};
use fid_kunc::prop::mcm::McmError;
use fid_kunc::prop::mvar::vmcm::{propagate_vector_mcm, CovarianceRefusal, VectorMcmResult, VmcmError, ALGORITHM_ID};
use fid_types::Digest;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/mvar/vmcm/");

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
        match self {
            J::O(m) => &m.iter().find(|(k, _)| k == key).unwrap_or_else(|| panic!("missing {key}")).1,
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
    fn o(&self) -> &[(String, J)] {
        match self {
            J::O(m) => m,
            _ => panic!("not an object"),
        }
    }
    fn u(&self) -> u64 {
        self.s().parse().unwrap()
    }
    fn f(&self) -> f64 {
        f64::from_bits(u64::from_str_radix(self.s(), 16).unwrap())
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

// ------------------------------------------------------------------ sample sets

fn label_digest(label: &str) -> Digest {
    sha256_digest(label.as_bytes())
}

fn header(outputs: &[&str], trials: u64, failed: u64, seed: u64) -> SampleSetHeader {
    SampleSetHeader {
        model_digest: label_digest("vmcm-fixture-model"),
        inputs_digest: label_digest("vmcm-fixture-inputs"),
        seed,
        profile_digest: label_digest("vmcm-fixture-profile"),
        outputs: outputs.iter().map(|s| (*s).to_owned()).collect(),
        trials,
        failed,
    }
}

/// Blocks of `size` trials holding, per trial, either the values of every output or a
/// failure.
fn blocks_of(trials: &[Result<Vec<f64>, TrialFailure>], size: usize, k: usize) -> Vec<BlockOut> {
    trials
        .chunks(size)
        .enumerate()
        .map(|(b, chunk)| {
            let mut values = vec![Vec::new(); k];
            let mut failed = Vec::new();
            for t in chunk {
                match t {
                    Ok(v) => {
                        for (col, x) in values.iter_mut().zip(v) {
                            col.push(*x);
                        }
                    }
                    Err(f) => failed.push(*f),
                }
            }
            BlockOut { block_index: b as u64, values, failed }
        })
        .collect()
}

/// Encodes per-trial rows of the outputs `outputs` (ascending) into `mcm-samples@1` bytes.
fn encode_rows(
    outputs: &[&str],
    rows: &[Result<Vec<f64>, TrialFailure>],
    size: usize,
    seed: u64,
    reverse: bool,
) -> Vec<u8> {
    let mut blocks = blocks_of(rows, size, outputs.len());
    if reverse {
        blocks.reverse();
    }
    let failed = rows.iter().filter(|r| r.is_err()).count() as u64;
    let h = header(outputs, rows.len() as u64, failed, seed);
    encode(&blocks, &h, DEFAULT_CAP).unwrap().bytes
}

fn rel_close(got: f64, want: f64, rel: f64) -> bool {
    (got - want).abs() <= rel * want.abs()
}

// ------------------------------------------------------------------ AC-1 model

/// `mcm-samples@1` bytes of the fixture's linear model y1 = x1 + x2, y2 = x2 + x3 at
/// `trials` trials of `seed`, in blocks of `size` (taken in reverse order if `reverse`).
fn bivariate_bytes(fix: &J, seed: u64, trials: u64, size: u64, reverse: bool) -> Vec<u8> {
    let inputs = fix.get("inputs").a();
    assert_eq!(inputs.len(), 3);
    let mut dists = Vec::new();
    for (n, x) in inputs.iter().enumerate() {
        assert_eq!(x.get("stream").u(), n as u64);
        let d = Distribution::normal(
            Quantity::new(x.get("mu").f(), UnitCode::Metre),
            Quantity::new(x.get("sigma").f(), UnitCode::Metre),
        )
        .unwrap();
        dists.push(d);
    }
    let outs = fix.get("outputs").a();
    assert_eq!(outs[0].get("id").s(), "y1");
    assert_eq!(outs[1].get("id").s(), "y2");
    assert_eq!(outs[0].get("terms").a().iter().map(J::s).collect::<Vec<_>>(), ["x1", "x2"]);
    assert_eq!(outs[1].get("terms").a().iter().map(J::s).collect::<Vec<_>>(), ["x2", "x3"]);
    assert_eq!(trials % size, 0);
    let mut blocks = Vec::new();
    for b in 0..trials / size {
        let range = b * size..(b + 1) * size;
        let x: Vec<Vec<f64>> =
            dists.iter().enumerate().map(|(n, d)| d.sample(seed, n as u64, range.clone()).unwrap()).collect();
        let y1: Vec<f64> = x[0].iter().zip(&x[1]).map(|(a, b)| a + b).collect();
        let y2: Vec<f64> = x[1].iter().zip(&x[2]).map(|(a, b)| a + b).collect();
        blocks.push(BlockOut { block_index: b, values: vec![y1, y2], failed: Vec::new() });
    }
    if reverse {
        blocks.reverse();
    }
    encode(&blocks, &header(&["y1", "y2"], trials, 0, seed), DEFAULT_CAP).unwrap().bytes
}

/// AC-1: the sample covariance of 10^6 trials (seed 11) is within 1 % of the known values.
#[test]
fn ac1_bivariate_known_covariance() {
    let fix = parse_json(&read("bivariate.json"));
    let quick = std::env::var("UNC_MVAR_VMCM_QUICK").map(|v| v == "1").unwrap_or(false);
    let full = fix.get("trials").u();
    assert_eq!(full, 1_000_000);
    assert_eq!(fix.get("seed").u(), 11);
    let (trials, widen) = if quick { (full / 10, 10f64.sqrt()) } else { (full, 1.0) };
    let size = fix.get("block_size").u();
    let bytes = bivariate_bytes(&fix, fix.get("seed").u(), trials, if quick { size / 10 } else { size }, false);
    let p = fix.get("p").f();
    let r = propagate_vector_mcm(&bytes, DEFAULT_CAP, &["y1", "y2"], p).unwrap();
    assert_eq!(r.m(), trials);
    assert_eq!(r.trials(), trials);
    assert!(r.excluded().is_empty());
    assert_eq!(r.samples_ref(), sha256_digest(&bytes));

    let rel = fix.get("rel_tol").f() * widen;
    let known = fix.get("known_cov").a();
    assert_eq!(known.len(), 2);
    for (i, row) in known.iter().enumerate() {
        for (j, w) in row.a().iter().enumerate() {
            let want = w.f();
            let got = r.cov().get(i, j).unwrap();
            assert!(rel_close(got, want, rel), "cov[{i}][{j}] = {got}, known {want}, rel tol {rel}");
        }
    }
    for (i, (m, t)) in fix.get("known_mean").a().iter().zip(fix.get("mean_tol").a()).enumerate() {
        let (want, tol) = (m.f(), t.f() * widen);
        assert!((r.mean()[i] - want).abs() <= tol, "mean[{i}] = {}, known {want}", r.mean()[i]);
    }
    // Not an AC: the Mahalanobis radius of a bivariate normal approaches the chi-square(2)
    // quantile.
    let q = fix.get("chi2_quantile").f();
    let qrel = fix.get("quantile_rel_tol").f() * widen;
    assert!(rel_close(r.region().r2_p(), q, qrel), "r2_p = {}, chi2 quantile {q}", r.region().r2_p());
    assert!(!r.check().rank_deficient);
    assert!(r.region().contained() >= r.region().rank());
    eprintln!(
        "AC-1 ({trials} trials): cov {:?}, mean {:?}, r2_p {} (chi2 {q}), contained {} of rank {}",
        r.cov().data(),
        r.mean(),
        r.region().r2_p(),
        r.region().contained(),
        r.region().rank()
    );
}

// ------------------------------------------------------------------ hand cases (oracle)

fn cases() -> Vec<J> {
    parse_json(&read("cases.json")).get("cases").a().to_vec()
}

/// Rows and bytes of a hand case; failed trials fail at the operator of the first output.
fn case_bytes(c: &J) -> Vec<u8> {
    let outputs: Vec<&str> = c.get("outputs").a().iter().map(J::s).collect();
    let trials = c.get("trials").u();
    let failed: Vec<u64> = c.get("failed").a().iter().map(J::u).collect();
    let cols = c.get("columns");
    let mut next = vec![0usize; outputs.len()];
    let mut rows = Vec::new();
    for t in 0..trials {
        if failed.contains(&t) {
            rows.push(Err(TrialFailure { index: t, code: FAIL_ALGORITHM, detail: Some("negative_radius") }));
            continue;
        }
        let mut v = Vec::new();
        for (k, o) in outputs.iter().enumerate() {
            v.push(cols.get(o).a()[next[k]].f());
            next[k] += 1;
        }
        rows.push(Ok(v));
    }
    encode_rows(&outputs, &rows, c.get("block_size").u() as usize, c.get("seed").u(), false)
}

fn run_case(c: &J) -> (Vec<u8>, VectorMcmResult) {
    let bytes = case_bytes(c);
    let group: Vec<&str> = c.get("group").a().iter().map(J::s).collect();
    let r = propagate_vector_mcm(&bytes, DEFAULT_CAP, &group, c.get("p").f()).unwrap();
    (bytes, r)
}

fn check_case(c: &J, bytes: &[u8], r: &VectorMcmResult) {
    let name = c.get("name").s();
    let e = c.get("expected");
    // The set the oracle wrote byte for byte, and the result references it.
    assert_eq!(sha256_digest(bytes).as_str(), e.get("samples").s(), "{name}: samples bytes");
    assert_eq!(r.samples_ref().as_str(), e.get("samples").s(), "{name}");
    let group: Vec<&str> = c.get("group").a().iter().map(J::s).collect();
    assert_eq!(r.outputs(), group.as_slice(), "{name}");
    assert_eq!(r.m(), e.get("m").u(), "{name}");
    assert_eq!(r.trials(), c.get("trials").u(), "{name}");
    let failed: Vec<u64> = c.get("failed").a().iter().map(J::u).collect();
    assert_eq!(r.excluded(), failed.as_slice(), "{name}");
    // Means are correctly rounded quotients of exact sums: bit for bit.
    let mean: Vec<u64> = e.get("mean").a().iter().map(|x| x.f().to_bits()).collect();
    assert_eq!(r.mean().iter().map(|x| x.to_bits()).collect::<Vec<_>>(), mean, "{name}: mean");
    let cov = e.get("cov").a();
    let k = group.len();
    assert_eq!(cov.len(), k);
    let scale = cov.iter().enumerate().map(|(i, row)| row.a()[i].f()).fold(0.0_f64, |a, x| if x > a { x } else { a });
    for (i, row) in cov.iter().enumerate() {
        assert_eq!(row.a().len(), k);
        for (j, w) in row.a().iter().enumerate() {
            let (got, want) = (r.cov().get(i, j).unwrap(), w.f());
            assert!((got - want).abs() <= 1e-13 * scale, "{name}: cov[{i}][{j}] = {got}, want {want}");
            assert_eq!(got.to_bits(), r.cov().get(j, i).unwrap().to_bits(), "{name}: symmetric");
        }
    }
    let reg = r.region();
    assert_eq!(reg.rank(), e.get("rank").u(), "{name}");
    assert_eq!(reg.m(), e.get("m").u(), "{name}");
    assert_eq!(reg.p().to_bits(), c.get("p").f().to_bits(), "{name}");
    assert!(rel_close(reg.r2_p(), e.get("r2_p").f(), 1e-12), "{name}: r2_p {} want {}", reg.r2_p(), e.get("r2_p").f());
    assert_eq!(reg.contained(), e.get("contained").u(), "{name}");
    assert_eq!(reg.at_radius(), e.get("at_radius").u(), "{name}");
    assert_eq!(reg.r_p().to_bits(), reg.r2_p().sqrt().to_bits(), "{name}");
}

/// AC-2: trial 5 failed at the operator of output `a` only; it is excluded for both outputs
/// and the statistics are those of the other 11 trials (oracle).
#[test]
fn ac2_failed_trial_excluded_for_every_output() {
    let c = cases().into_iter().find(|c| c.get("name").s() == "joint_fail").unwrap();
    assert_eq!(c.get("failed").a().iter().map(J::u).collect::<Vec<_>>(), [5]);
    assert!(c.get("failed_would_be").o().iter().any(|(t, _)| t == "5"));
    let (bytes, r) = run_case(&c);
    check_case(&c, &bytes, &r);
    assert_eq!(r.excluded(), [5]);
    assert_eq!(r.m(), 11);
    // The same holds for each output on its own: both columns have 11 values.
    for id in ["a", "b"] {
        let one = propagate_vector_mcm(&bytes, DEFAULT_CAP, &[id], c.get("p").f()).unwrap();
        assert_eq!(one.m(), 11, "{id}");
        assert_eq!(one.excluded(), [5], "{id}");
    }
    let json = String::from_utf8(r.canonical_json().unwrap()).unwrap();
    assert!(json.contains(r#""excluded":["5"]"#), "{json}");
}

/// REQ-001/-002 on a three-output group in non-sorted order with three failed trials.
#[test]
fn oracle_cases() {
    for c in cases() {
        let (bytes, r) = run_case(&c);
        check_case(&c, &bytes, &r);
    }
}

/// AC-3: the same seed gives identical region and covariance bytes; blocks in any order give
/// the same bytes; another seed gives different ones.
#[test]
fn ac3_same_seed_identical_bytes() {
    let fix = parse_json(&read("bivariate.json"));
    let p = fix.get("p").f();
    let run = |seed: u64, reverse: bool| {
        let bytes = bivariate_bytes(&fix, seed, 20_000, 5_000, reverse);
        propagate_vector_mcm(&bytes, DEFAULT_CAP, &["y1", "y2"], p).unwrap()
    };
    let a = run(11, false);
    let b = run(11, false);
    let c = run(11, true);
    assert_eq!(a.canonical_json().unwrap(), b.canonical_json().unwrap());
    assert_eq!(a.canonical_json().unwrap(), c.canonical_json().unwrap());
    assert_eq!(a, b);
    let other = run(12, false);
    assert_ne!(a.canonical_json().unwrap(), other.canonical_json().unwrap());
    assert_ne!(a.samples_ref(), other.samples_ref());
}

/// The canonical JSON members, hand-written from the module documentation.
#[test]
fn canonical_json_members() {
    let c = cases().into_iter().find(|c| c.get("name").s() == "joint_fail").unwrap();
    let (_, r) = run_case(&c);
    let json = String::from_utf8(r.canonical_json().unwrap()).unwrap();
    let hex = |x: f64| format!("{:016x}", x.to_bits());
    let reg = r.region();
    let mut cov = String::new();
    for i in 0..2 {
        let row: Vec<String> = (0..2).map(|j| format!("\"{}\"", hex(r.cov().get(i, j).unwrap()))).collect();
        cov.push_str(&format!("{}[{}]", if i == 0 { "" } else { "," }, row.join(",")));
    }
    let want = format!(
        concat!(
            r#"{{"algorithm":"{}","basis":"simulated","cov":[{}],"excluded":["5"],"#,
            r#""interpretation":"provisional","mean":["{}","{}"],"#,
            r#""outputs":["a","b"],"region":{{"at_radius":"{}","contained":"{}","m":"11","p":"{}","r2_p":"{}","#,
            r#""r_p":"{}","rank":"{}"}},"samples_ref":"{}","trials":"12"}}"#
        ),
        ALGORITHM_ID,
        cov,
        hex(r.mean()[0]),
        hex(r.mean()[1]),
        reg.at_radius(),
        reg.contained(),
        hex(reg.p()),
        hex(reg.r2_p()),
        hex(reg.r_p()),
        reg.rank(),
        r.samples_ref().as_str(),
    );
    assert_eq!(json, want);
    // The fixed members the oracle lists (D-045).
    for (key, value) in parse_json(&read("cases.json")).get("json_members").o() {
        assert!(json.contains(&format!("\"{key}\":\"{}\"", value.s())), "{key}");
    }
}

// ------------------------------------------------------------------ hand-labelled rules

fn simple(outputs: &[&str], rows: &[Vec<f64>]) -> Vec<u8> {
    let rows: Vec<Result<Vec<f64>, TrialFailure>> = rows.iter().cloned().map(Ok).collect();
    encode_rows(outputs, &rows, rows.len(), 1, false)
}

/// Ties at the radius are counted, not broken: x = +-1 .. +-5 (mean exactly 0), p = 0.5,
/// rank 5; the 5th smallest squared distance belongs to x = +-3, so 6 trials are contained
/// and 2 lie on the boundary. r2_p = 9 / s^2 with s^2 = 110 / 9.
#[test]
fn ties_at_the_radius_are_reported() {
    let xs = [-1.0, 1.0, -2.0, 2.0, -3.0, 3.0, -4.0, 4.0, -5.0, 5.0];
    let rows: Vec<Vec<f64>> = xs.iter().map(|&x| vec![x]).collect();
    let bytes = simple(&["x"], &rows);
    let r = propagate_vector_mcm(&bytes, DEFAULT_CAP, &["x"], 0.5).unwrap();
    assert_eq!(r.mean(), [0.0]);
    assert!(rel_close(r.cov().get(0, 0).unwrap(), 110.0 / 9.0, 1e-15));
    let reg = r.region();
    assert_eq!((reg.rank(), reg.contained(), reg.at_radius()), (5, 6, 2));
    assert!(rel_close(reg.r2_p(), 81.0 / 110.0, 1e-14), "{}", reg.r2_p());
}

fn pair_bytes() -> Vec<u8> {
    let c = cases().into_iter().find(|c| c.get("name").s() == "joint_fail").unwrap();
    case_bytes(&c)
}

#[test]
fn invalid_p_is_refused() {
    let bytes = pair_bytes();
    for p in [0.0, 1.0, -0.5, 1.5, f64::NAN, f64::INFINITY, -0.0] {
        let e = propagate_vector_mcm(&bytes, DEFAULT_CAP, &["a", "b"], p).unwrap_err();
        assert_eq!(e, VmcmError::InvalidP, "{p}");
        assert_eq!(e.code(), "invalid_p");
    }
}

#[test]
fn invalid_groups_are_refused() {
    let bytes = pair_bytes();
    let run = |g: &[&str]| propagate_vector_mcm(&bytes, DEFAULT_CAP, g, 0.75).unwrap_err();
    assert_eq!(run(&[]), VmcmError::Group { detail: "size", index: None });
    assert_eq!(run(&["a"; 8]), VmcmError::Group { detail: "size", index: None });
    assert_eq!(run(&["a", "b", "a"]), VmcmError::Group { detail: "duplicate", index: Some(2) });
    assert_eq!(run(&["a", "q"]), VmcmError::Group { detail: "unknown_output", index: Some(1) });
    assert_eq!(run(&["b", "a\u{feff}"]), VmcmError::Group { detail: "unknown_output", index: Some(1) });
    assert_eq!(run(&["a\n"]), VmcmError::Group { detail: "unknown_output", index: Some(0) });
    assert_eq!(run(&["q"]).code(), "invalid_group");
}

#[test]
fn bad_sample_bytes_are_refused() {
    let bytes = pair_bytes();
    let e = propagate_vector_mcm(&bytes[..bytes.len() - 1], DEFAULT_CAP, &["a"], 0.75).unwrap_err();
    assert_eq!(e, VmcmError::Samples(McmError::SamplesInvalid { detail: "length" }));
    let e = propagate_vector_mcm(&bytes, 16, &["a"], 0.75).unwrap_err();
    assert_eq!(e, VmcmError::Samples(McmError::SamplesInvalid { detail: "too_large" }));
    assert_eq!(e.code(), "samples_invalid");
}

/// The minimum is max(k + 1, n_min) with n_min the smallest M with ceil(p M) < M.
#[test]
fn too_few_trials() {
    let bytes = pair_bytes();
    // 0.95 is just below 19/20 in binary64: n_min = 20.
    let e = propagate_vector_mcm(&bytes, DEFAULT_CAP, &["a", "b"], 0.95).unwrap_err();
    assert_eq!(e, VmcmError::TooFewTrials { m: 11, min: 20 });
    assert_eq!(e.code(), "too_few_trials");
    // k + 1 dominates at p = 0.5 with 2 trials and 2 outputs.
    let bytes = simple(&["a", "b"], &[vec![1.0, 2.0], vec![2.0, 1.0]]);
    let e = propagate_vector_mcm(&bytes, DEFAULT_CAP, &["a", "b"], 0.5).unwrap_err();
    assert_eq!(e, VmcmError::TooFewTrials { m: 2, min: 3 });
    // Every trial failed.
    let rows: Vec<Result<Vec<f64>, TrialFailure>> =
        (0..6).map(|t| Err(TrialFailure { index: t, code: FAIL_ALGORITHM, detail: None })).collect();
    let bytes = encode_rows(&["a"], &rows, 6, 1, false);
    let e = propagate_vector_mcm(&bytes, DEFAULT_CAP, &["a"], 0.5).unwrap_err();
    assert_eq!(e, VmcmError::TooFewTrials { m: 0, min: 2 });
}

/// A singular sample covariance is refused with the flagged check, never returned.
#[test]
fn rank_deficient_covariance_is_refused() {
    // b = 2 a exactly.
    let rows: Vec<Vec<f64>> = (0..8).map(|t| f64::from(t * t % 7)).map(|a| vec![a, 2.0 * a]).collect();
    let bytes = simple(&["a", "b"], &rows);
    let e = propagate_vector_mcm(&bytes, DEFAULT_CAP, &["a", "b"], 0.5).unwrap_err();
    assert_eq!(e.code(), "invalid_covariance");
    match e {
        VmcmError::InvalidCovariance(CovarianceRefusal::RankDeficient { check: Some(c), constant_output: None }) => {
            assert!(c.rank_deficient)
        }
        other => panic!("{other:?}"),
    }
    // Each output on its own is fine.
    assert!(propagate_vector_mcm(&bytes, DEFAULT_CAP, &["b"], 0.5).is_ok());
    // A constant output is named by its group index (rule 5a).
    let rows: Vec<Vec<f64>> = (0..8).map(|t| vec![f64::from(t), 3.0]).collect();
    let bytes = simple(&["a", "b"], &rows);
    for (g, at) in [(&["a", "b"][..], 1), (&["b"][..], 0), (&["b", "a"][..], 0)] {
        let e = propagate_vector_mcm(&bytes, DEFAULT_CAP, g, 0.5).unwrap_err();
        let want = CovarianceRefusal::RankDeficient { check: None, constant_output: Some(at) };
        assert_eq!(e, VmcmError::InvalidCovariance(want), "{g:?}");
    }
}

/// Review of #676: a constant column whose rounded mean differs from the constant leaves
/// deviations of about 1e-17 that the correlation scaling would hide; it is refused before
/// any mean is computed (oracle `refusals`, k = 1 and k = 2, 3 and 99999 trials).
#[test]
fn constant_outputs_are_refused() {
    let fix = parse_json(&read("cases.json"));
    let refusals = fix.get("refusals").a();
    assert_eq!(refusals.len(), 4);
    for c in refusals {
        let name = c.get("name").s();
        let trials = c.get("trials").u();
        let outputs: Vec<&str> = c.get("outputs").a().iter().map(J::s).collect();
        assert_eq!(outputs, ["c", "v"]);
        let g = c.get("generator");
        let constant = g.get("c").get("constant").f();
        let (modulus, offset): (i64, i64) =
            (g.get("v").get("mod").s().parse().unwrap(), g.get("v").get("offset").s().parse().unwrap());
        let rows: Vec<Vec<f64>> = (0..trials as i64).map(|t| vec![constant, (t % modulus + offset) as f64]).collect();
        let bytes = simple(&outputs, &rows);
        let group: Vec<&str> = c.get("group").a().iter().map(J::s).collect();
        let e = c.get("expected");
        assert_eq!(e.get("rounded_mean_differs").s(), "true", "{name}");
        let err = propagate_vector_mcm(&bytes, DEFAULT_CAP, &group, c.get("p").f()).unwrap_err();
        assert_eq!(err.code(), e.get("code").s(), "{name}");
        let want = CovarianceRefusal::RankDeficient {
            check: None,
            constant_output: Some(e.get("constant_output").u() as usize),
        };
        assert_eq!(err, VmcmError::InvalidCovariance(want), "{name}");
        // The varying output alone is accepted.
        assert!(propagate_vector_mcm(&bytes, DEFAULT_CAP, &["v"], c.get("p").f()).is_ok(), "{name}");
    }
}
