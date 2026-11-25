//! Integration tests for the `GEO.INDEX.BENCH` benchmark harness (`benches/index.rs`).
//!
//! Covers:
//! - AC-1: seeded generator (seed 31), JSON report contains all REQ-GEO.INDEX.BENCH-004 fields
//!   (`profile`, `threads`, `version`, `seeds`, `tiers[]`, `regression`), both `uniform` and
//!   `scan_strip` distributions plus BVH metrics, and `bytes_per_point <= 32`.
//! - AC-2: exact `10^8`-point memory accounting (`overhead_bytes(100_000_000) / 1e8 <= 32.0`)
//!   and large-tier configuration parsing (`--tier large` -> `100_000_000` points, `100_000`
//!   queries).
//! - AC-3: regression path under `--profile reference` with an injected slowdown marks
//!   `"regression":true` and returns non-zero `ExitCode::FAILURE`, while non-reference
//!   profiles do not fail on latency alone.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::ExitCode;

use fid_geo_index::overhead_bytes;

#[allow(dead_code)]
#[path = "../benches/index.rs"]
mod index_bench;

fn args(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn ac1_report_contains_all_required_fields_and_memory_under_32_bpp() {
    let (code, stdout, stderr) = index_bench::run_cli(&args(&[
        "--n",
        "2048",
        "--queries",
        "128",
        "--bvh-triangles",
        "256",
        "--bvh-queries",
        "64",
        "--seed",
        "31",
        "--profile",
        "ci-pr",
        "--threads",
        "1",
        "--json",
    ]));
    assert_eq!(code, ExitCode::SUCCESS, "stderr={stderr}");
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");

    let trimmed = stdout.trim();
    assert!(trimmed.starts_with('{') && trimmed.ends_with('}'), "not JSON: {trimmed}");
    for needle in [
        r#""profile":"ci-pr""#,
        r#""threads":1"#,
        concat!(r#""version":""#, env!("CARGO_PKG_VERSION"), r#"""#),
        r#""seeds":{"base":31,"#,
        r#""distribution":"uniform""#,
        r#""distribution":"scan_strip""#,
        r#""n":2048"#,
        r#""queries":128"#,
        r#""bytes_per_point":"#,
        r#""build_ms":"#,
        r#""knn16_median_ns":"#,
        r#""knn16_p99_ns":"#,
        r#""bvh":{"#,
        r#""triangles":256"#,
        r#""regression":false"#,
    ] {
        assert!(trimmed.contains(needle), "missing {needle} in JSON: {trimmed}");
    }

    // Verify exact report struct values directly too.
    let cfg = index_bench::parse_args(&args(&[
        "--n",
        "2048",
        "--queries",
        "128",
        "--bvh-triangles",
        "256",
        "--bvh-queries",
        "64",
        "--seed",
        "31",
        "--json",
    ]))
    .unwrap();
    let (report, failures) = index_bench::execute(&cfg).unwrap();
    assert!(failures.is_empty(), "unexpected failures: {failures:?}");
    assert!(!report.regression);
    assert_eq!(report.tiers.len(), 2);
    for t in &report.tiers {
        assert_eq!(t.n, 2048);
        assert!(t.bytes_per_point > 0.0 && t.bytes_per_point <= 32.0, "bpp={}", t.bytes_per_point);
        assert!(t.knn16_p99_ns >= t.knn16_median_ns, "p99 < median");
        let bvh = t.bvh.as_ref().expect("bvh metrics present");
        assert_eq!(bvh.triangles, 256);
        assert!(bvh.memory_bytes > 0);
        assert!(bvh.nearest_box_p99_ns >= bvh.nearest_box_median_ns);
    }
}

#[test]
fn ac2_large_tier_config_and_1e8_memory_bound_under_32_bpp() {
    // Tier definitions match REQ-001 / REQ-002 exactly.
    let small = index_bench::parse_args(&args(&["--tier", "small", "--json"])).unwrap();
    assert_eq!(small.tiers, vec![(index_bench::TierKind::Small, 1_000_000)]);
    assert_eq!(small.queries, 100_000);
    assert_eq!(small.seed, 31);

    let medium = index_bench::parse_args(&args(&["--tier", "medium", "--json"])).unwrap();
    assert_eq!(medium.tiers, vec![(index_bench::TierKind::Medium, 10_000_000)]);

    let large = index_bench::parse_args(&args(&["--tier", "large", "--json"])).unwrap();
    assert_eq!(large.tiers, vec![(index_bench::TierKind::Large, 100_000_000)]);
    assert_eq!(large.queries, 100_000);

    // Exact point-index accounting at 10^8 points is well under 32 bytes/point.
    let bytes_1e8 = overhead_bytes(100_000_000).expect("10^8 within MAX_POINTS");
    let bpp_1e8 = bytes_1e8 as f64 / 100_000_000.0;
    assert!(bpp_1e8 <= 32.0, "10^8 point index overhead {bpp_1e8:.6} B/pt exceeds 32 B/pt");
}

#[test]
fn ac3_injected_slowdown_under_reference_profile_flags_regression_and_fails() {
    // Under `--profile reference`, injecting a slowdown past the 10 us (10_000 ns) budget
    // MUST mark `regression: true`, emit the JSON report, and exit non-zero.
    let (code, stdout, stderr) = index_bench::run_cli(&args(&[
        "--n",
        "1024",
        "--queries",
        "64",
        "--bvh-triangles",
        "0",
        "--seed",
        "31",
        "--profile",
        "reference",
        "--slowdown-factor",
        "2",
        "--inject-slowdown-ns",
        "15000",
        "--json",
    ]));
    assert_eq!(code, ExitCode::FAILURE, "expected failure on reference latency regression");
    assert!(stdout.contains(r#""profile":"reference""#), "stdout={stdout}");
    assert!(stdout.contains(r#""regression":true"#), "expected regression:true in {stdout}");
    assert!(stderr.contains("exceeds reference budget 10000 ns"), "stderr={stderr}");

    // The same latency on a non-reference profile does NOT fail or set regression:true.
    let (code_ci, stdout_ci, stderr_ci) = index_bench::run_cli(&args(&[
        "--n",
        "1024",
        "--queries",
        "64",
        "--bvh-triangles",
        "0",
        "--seed",
        "31",
        "--profile",
        "ci-default",
        "--slowdown-factor",
        "2",
        "--inject-slowdown-ns",
        "15000",
        "--json",
    ]));
    assert_eq!(code_ci, ExitCode::SUCCESS, "stderr={stderr_ci}");
    assert!(stdout_ci.contains(r#""regression":false"#), "stdout={stdout_ci}");
}

#[test]
fn out_file_and_invalid_cli_flags() {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp_dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let out_path = tmp_dir.join(format!("fid-geo-index-bench-test-{n}.json"));
    let out_str = out_path.to_str().unwrap().to_string();
    let (code, stdout, stderr) = index_bench::run_cli(&args(&[
        "--n",
        "512",
        "--queries",
        "32",
        "--bvh-triangles",
        "0",
        "--out",
        &out_str,
        "--json",
    ]));
    assert_eq!(code, ExitCode::SUCCESS, "stderr={stderr}");
    let written = std::fs::read_to_string(&out_path).unwrap();
    let _ = std::fs::remove_file(&out_path);
    assert_eq!(written, stdout);

    let (bad_code, _, bad_err) = index_bench::run_cli(&args(&["--tier", "gigantic"]));
    assert_eq!(bad_code, ExitCode::FAILURE);
    assert!(bad_err.contains("unknown tier"), "bad_err={bad_err}");

    let (zero_threads, _, zt_err) = index_bench::run_cli(&args(&["--threads", "0"]));
    assert_eq!(zero_threads, ExitCode::FAILURE);
    assert!(zt_err.contains("--threads"), "zt_err={zt_err}");
}
