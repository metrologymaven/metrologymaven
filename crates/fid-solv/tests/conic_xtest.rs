//! Acceptance criteria of spec node KRN.SOLV.CONIC.XTEST (AC-1..AC-3) and requirements
//! REQ-KRN.SOLV.CONIC.XTEST-001..004.
//!
//! Uses `include_bytes!` for the frozen fixture set so the full 165-problem suite also runs
//! on `wasm32-wasip1` under wasmtime (`ci/wasm32-crates.txt`) without needing host paths.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_solv::conic::xtest::{
    compare_result_bytes, encode_result_list, parse_manifest, parse_problems, parse_result_list, run_suite,
    ExpectedStatus, XtestError, DEFAULT_REL_TOL, MIN_PER_CLASS,
};
use fid_solv::conic::{solve_lp, solve_qp, solve_socp, ConicCause, ConicError, ProblemClass};

const MANIFEST_BYTES: &[u8] = include_bytes!("../../../fixtures/solv/conic-xtest-v1/manifest.json");
const PROBLEMS_BYTES: &[u8] = include_bytes!("../../../fixtures/solv/conic-xtest-v1/problems.json");

/// Regression pin of the canonical result list SHA-256 across all 165 problems.
/// Checked by CI on x86-64, ARM64 and wasm32 (wasmtime); any cross-target bit divergence fails
/// both this pin and the `conic-xtest.yml` three-target comparison.
const PINNED_RESULT_DIGEST: &str = "sha256:3987f33b136e5d941424d86f97ff9c8aadb985ee98b3a045da7e3476d574d0e3";

/// AC-1 & REQ-001..004: all 165 problems (>= 50 LP, 50 QP, 50 SOCP) solve within 1e-8 relative
/// of the independent offline reference optima, produce canonical JSON with IEEE-754 hex
/// strings, and match the pinned cross-target SHA-256 digest.
#[test]
fn ac1_frozen_suite_objectives_and_pinned_digest() {
    let manifest = parse_manifest(MANIFEST_BYTES).unwrap();
    assert_eq!(manifest.version, "conic-xtest-v1");

    let run = run_suite(MANIFEST_BYTES, PROBLEMS_BYTES, DEFAULT_REL_TOL).unwrap();
    assert!(run.lp_count >= MIN_PER_CLASS, "lp_count={}", run.lp_count);
    assert!(run.qp_count >= MIN_PER_CLASS, "qp_count={}", run.qp_count);
    assert!(run.socp_count >= MIN_PER_CLASS, "socp_count={}", run.socp_count);
    assert_eq!(run.problem_count, 165);
    assert_eq!(run.results.len(), 165);

    // Every float vector entry is a 16-char lowercase IEEE-754 hex string (REQ-004).
    for r in &run.results {
        for hex in r.x.iter().chain(&r.y).chain(&r.s) {
            assert_eq!(hex.len(), 16, "{}: hex float {hex:?} must be 16 chars", r.id);
            assert!(
                hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                "{}: hex float {hex:?} must be lowercase hex",
                r.id
            );
        }
    }

    // Canonical JSON round-trip preserves bytes and digest identically.
    let parsed = parse_result_list(&run.canonical_json).unwrap();
    assert_eq!(parsed, run.results);
    let (reencoded, redigest) = encode_result_list(&parsed).unwrap();
    assert_eq!(reencoded, run.canonical_json);
    assert_eq!(redigest, run.digest);

    // Bit-identity pin across x86-64, ARM64 and wasm32.
    assert_eq!(run.digest.as_str(), PINNED_RESULT_DIGEST, "conic-xtest digest diverged on {}", std::env::consts::ARCH);
}

/// AC-2 & REQ-002: perturbing a single rounding bit (or status / iteration count) on one target
/// fails the comparator and names the exact first differing problem ID and field.
#[test]
fn ac2_perturbed_rounding_fails_naming_problem_and_field() {
    let run = run_suite(MANIFEST_BYTES, PROBLEMS_BYTES, DEFAULT_REL_TOL).unwrap();
    // Self-comparison passes and returns the shared digest.
    assert_eq!(compare_result_bytes(&run.canonical_json, &run.canonical_json).unwrap(), run.digest);

    // 1. Flip the least-significant IEEE-754 rounding bit of x[0] on lp-001.
    let mut perturbed = run.results.clone();
    let p0 = &mut perturbed[0];
    assert_eq!(p0.id, "lp-001");
    let orig_bits = u64::from_str_radix(&p0.x[0], 16).unwrap();
    p0.x[0] = format!("{:016x}", orig_bits ^ 1);
    let (bad_bytes, _) = encode_result_list(&perturbed).unwrap();
    let err = compare_result_bytes(&run.canonical_json, &bad_bytes).unwrap_err();
    match err {
        XtestError::Mismatch(d) => {
            assert_eq!(d.problem_id, "lp-001");
            assert_eq!(d.field, "x[0]");
            assert_ne!(d.expected, d.actual);
        }
        other => panic!("expected Mismatch, got {other:?}"),
    }

    // 2. Flip 1 bit of y[2] on a QP problem.
    let mut perturbed_y = run.results.clone();
    let qp = perturbed_y.iter_mut().find(|r| r.id == "qp-001").unwrap();
    let y_bits = u64::from_str_radix(&qp.y[2], 16).unwrap();
    qp.y[2] = format!("{:016x}", y_bits ^ 1);
    let (bad_y, _) = encode_result_list(&perturbed_y).unwrap();
    let err_y = compare_result_bytes(&run.canonical_json, &bad_y).unwrap_err();
    match err_y {
        XtestError::Mismatch(d) => {
            assert_eq!(d.problem_id, "qp-001");
            assert_eq!(d.field, "y[2]");
        }
        other => panic!("expected Mismatch, got {other:?}"),
    }

    // 3. Perturb iterations on a SOCP problem.
    let mut perturbed_it = run.results.clone();
    let socp = perturbed_it.iter_mut().find(|r| r.id == "socp-001").unwrap();
    socp.iterations += 1;
    let (bad_it, _) = encode_result_list(&perturbed_it).unwrap();
    let err_it = compare_result_bytes(&run.canonical_json, &bad_it).unwrap_err();
    match err_it {
        XtestError::Mismatch(d) => {
            assert_eq!(d.problem_id, "socp-001");
            assert_eq!(d.field, "iterations");
        }
        other => panic!("expected Mismatch, got {other:?}"),
    }
}

/// AC-3: every infeasible problem in the frozen set yields its exact expected `SolverError`
/// cause (`PrimalInfeasible` or `DualInfeasible`) across LP, QP and SOCP.
#[test]
fn ac3_infeasible_problems_yield_expected_solver_error_causes() {
    let problems = parse_problems(PROBLEMS_BYTES).unwrap();
    let infeasible: Vec<_> = problems.iter().filter(|p| p.expected_status != ExpectedStatus::Solved).collect();
    assert!(infeasible.len() >= 12, "expected infeasible cases across all classes, got {}", infeasible.len());

    let mut primal_count = 0usize;
    let mut dual_count = 0usize;
    for p in infeasible {
        let res = match p.class {
            ProblemClass::Lp => solve_lp(&p.problem),
            ProblemClass::Qp => solve_qp(&p.problem),
            ProblemClass::Socp => solve_socp(&p.problem),
        };
        let Err(ConicError::Solver(se)) = res else {
            panic!("{}: expected ConicError::Solver, got {res:?}", p.id);
        };
        match p.expected_status {
            ExpectedStatus::PrimalInfeasible => {
                assert_eq!(se.cause, ConicCause::PrimalInfeasible, "{}", p.id);
                primal_count += 1;
            }
            ExpectedStatus::DualInfeasible => {
                assert_eq!(se.cause, ConicCause::DualInfeasible, "{}", p.id);
                dual_count += 1;
            }
            ExpectedStatus::Solved => unreachable!(),
        }
    }
    assert!(primal_count >= 9, "primal_count={primal_count}");
    assert!(dual_count >= 6, "dual_count={dual_count}");
}

/// REQ-001: changing any byte of the frozen problems file without updating the manifest digest
/// fails closed with `DigestMismatch`.
#[test]
fn req001_tampered_problems_file_fails_digest_check() {
    let mut tampered = PROBLEMS_BYTES.to_vec();
    tampered.push(b' ');
    let err = run_suite(MANIFEST_BYTES, &tampered, DEFAULT_REL_TOL).unwrap_err();
    assert!(matches!(err, XtestError::DigestMismatch { .. }), "expected DigestMismatch, got {err:?}");
}

/// CLI end-to-end check on native targets: `conic-xtest run` and `conic-xtest compare` succeed
/// on matching outputs and fail naming problem ID and field when one bit is perturbed.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cli_binary_run_and_compare_end_to_end() {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "fid-conic-xtest-cli-{}-{nanos}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();

    let bin = env!("CARGO_BIN_EXE_conic-xtest");
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/solv/conic-xtest-v1");
    let out_a = dir.join("results_a.json");
    let out_b = dir.join("results_b.json");
    let dig_a = dir.join("digest_a.txt");

    let run_a = Command::new(bin)
        .args(["run", "--fixtures"])
        .arg(&fixtures)
        .arg("--out")
        .arg(&out_a)
        .arg("--digest-out")
        .arg(&dig_a)
        .output()
        .unwrap();
    assert!(run_a.status.success(), "run failed: {}", String::from_utf8_lossy(&run_a.stderr));
    assert_eq!(fs::read_to_string(&dig_a).unwrap().trim(), PINNED_RESULT_DIGEST);

    fs::copy(&out_a, &out_b).unwrap();
    let cmp_ok = Command::new(bin).arg("compare").arg(&out_a).arg(&out_b).output().unwrap();
    assert!(cmp_ok.status.success(), "compare ok failed: {}", String::from_utf8_lossy(&cmp_ok.stderr));

    // Perturb 1 bit of x[0] on lp-001 and verify compare exits non-zero naming lp-001 and x[0].
    let mut results = parse_result_list(&fs::read(&out_a).unwrap()).unwrap();
    let orig = u64::from_str_radix(&results[0].x[0], 16).unwrap();
    results[0].x[0] = format!("{:016x}", orig ^ 1);
    let (bad_bytes, _) = encode_result_list(&results).unwrap();
    fs::write(&out_b, bad_bytes).unwrap();

    let cmp_fail = Command::new(bin).arg("compare").arg(&out_a).arg(&out_b).output().unwrap();
    assert!(!cmp_fail.status.success(), "compare must fail on perturbed result");
    let stderr = String::from_utf8_lossy(&cmp_fail.stderr);
    assert!(stderr.contains("lp-001") && stderr.contains("x[0]"), "stderr missing id/field: {stderr}");

    let _ = fs::remove_dir_all(&dir);
}
