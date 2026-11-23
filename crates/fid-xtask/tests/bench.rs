//! Acceptance tests AC-1..AC-5 for `ECO.DATA.BENCH.RUN` (`fid_xtask::bench`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use fid_xtask::bench::{BenchRecord, ERR_PROFILE, ERR_REPS, REASON_NOT_IN_CATALOGUE};
use sha2::{Digest as _, Sha256};

const TEST_REV: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bench_fixtures() -> PathBuf {
    repo_root().join("fixtures/bench")
}

fn xtask_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fid-xtask"))
}

fn fid_data_bin() -> &'static PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let status = Command::new(&cargo)
            .args(["build", "-q", "-p", "fid-data", "--bin", "fid-data"])
            .current_dir(repo_root())
            .status()
            .expect("build fid-data");
        assert!(status.success(), "cargo build -p fid-data must succeed");
        // Resolve sibling binary next to fid-xtask in target/debug.
        let mut p = xtask_bin();
        p.set_file_name(if cfg!(windows) { "fid-data.exe" } else { "fid-data" });
        assert!(p.exists(), "fid-data binary not found at {}", p.display());
        p
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in d {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Populates a temporary `fid-data` cache with `bench-ds-a` and `bench-ds-b`.
fn populate_cache(cache_dir: &Path) {
    let fx = bench_fixtures();
    for (id, fname, blob_name) in
        [("bench-ds-a", "bench-ds-a.dat", "blob-a.dat"), ("bench-ds-b", "bench-ds-b.dat", "blob-b.dat")]
    {
        let bytes = fs::read(fx.join(blob_name)).expect("read blob");
        let hex = sha256_hex(&bytes);
        let dir = cache_dir.join(id).join(&hex);
        fs::create_dir_all(&dir).expect("mkdir cache");
        fs::write(dir.join(fname), bytes).expect("write cached blob");
    }
}

fn fresh_tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fid_xtask_bench_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir tmp");
    dir
}

#[test]
fn ac1_and_ac4_run_suite_and_second_run_never_overwrites() {
    let tmp = fresh_tmp("ac1_ac4");
    let cache = tmp.join("cache");
    let history = tmp.join("history");
    let state = tmp.join("state");
    populate_cache(&cache);

    let fx = bench_fixtures();
    let run_once = || {
        Command::new(xtask_bin())
            .env("FID_DATA_BIN", fid_data_bin())
            .env("FAKE_FID_STATE_DIR", &state)
            .args([
                "bench",
                "run",
                "--suite",
                fx.join("suite.toml").to_str().unwrap(),
                "--profiles",
                fx.join("profiles.toml").to_str().unwrap(),
                "--catalogue",
                fx.join("datasets.toml").to_str().unwrap(),
                "--cache",
                cache.to_str().unwrap(),
                "--offline",
                "--machine-profile",
                "min-2026",
                "--fid",
                fx.join("fake-fid").to_str().unwrap(),
                "--reps",
                "3",
                "--revision",
                TEST_REV,
                "--history",
                history.to_str().unwrap(),
            ])
            .output()
            .expect("run xtask bench")
    };

    // AC-1: first run creates 2 records with median 100, min 98, max 102, rel_spread 0.04.
    let out1 = run_once();
    assert!(out1.status.success(), "AC-1 stderr: {}", String::from_utf8_lossy(&out1.stderr));

    let hist_dir = history.join(TEST_REV).join("min-2026");
    for wid in ["bench.load", "bench.fit"] {
        let p = hist_dir.join(format!("{wid}.json"));
        assert!(p.exists(), "missing record {}", p.display());
        let rec = BenchRecord::from_json(&fs::read(&p).unwrap()).expect("valid bench-record.json");
        assert_eq!(rec.schema, 1);
        assert_eq!(rec.benchmark_id, wid);
        assert_eq!(rec.revision, TEST_REV);
        assert_eq!(rec.build_profile, "release");
        assert_eq!(rec.machine_profile_id, "min-2026");
        assert_eq!(rec.repetitions, 3);
        assert_eq!(rec.status, "ok");
        assert!(rec.reason.is_none());
        assert_eq!(rec.results.len(), 3);
        assert_eq!(rec.metrics.len(), 1);
        let m = &rec.metrics[0];
        assert_eq!(m.name, "elapsed_ms");
        assert_eq!(m.unit, "ms");
        assert_eq!(m.median, "100");
        assert_eq!(m.min, "98");
        assert_eq!(m.max, "102");
        assert_eq!(m.rel_spread, "0.04");
    }

    // AC-4: second run writes suffixed `-1` records and never overwrites originals.
    let orig_load = fs::read(hist_dir.join("bench.load.json")).unwrap();
    let out2 = run_once();
    assert!(out2.status.success(), "AC-4 stderr: {}", String::from_utf8_lossy(&out2.stderr));

    assert_eq!(
        fs::read(hist_dir.join("bench.load.json")).unwrap(),
        orig_load,
        "original record must not be overwritten"
    );
    assert!(hist_dir.join("bench.load-1.json").exists(), "second run must write bench.load-1.json");
    assert!(hist_dir.join("bench.fit-1.json").exists(), "second run must write bench.fit-1.json");
    let count = fs::read_dir(&hist_dir).unwrap().count();
    assert_eq!(count, 4, "running AC-1 twice must leave exactly 4 records");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac2_missing_dataset_and_corrupted_offline_cache_write_not_run_without_calling_fid() {
    let tmp = fresh_tmp("ac2");
    let cache = tmp.join("cache");
    let history = tmp.join("history");
    let call_log = tmp.join("calls.log");
    let fx = bench_fixtures();

    // Part 1: dataset absent from catalogue -> NOT-IN-CATALOGUE, fake-fid never called.
    let out = Command::new(xtask_bin())
        .env("FID_DATA_BIN", fid_data_bin())
        .env("FAKE_FID_CALL_LOG", &call_log)
        .args([
            "bench",
            "run",
            "--suite",
            fx.join("suite-missing-ds.toml").to_str().unwrap(),
            "--profiles",
            fx.join("profiles.toml").to_str().unwrap(),
            "--catalogue",
            fx.join("datasets.toml").to_str().unwrap(),
            "--cache",
            cache.to_str().unwrap(),
            "--offline",
            "--machine-profile",
            "min-2026",
            "--fid",
            fx.join("fake-fid").to_str().unwrap(),
            "--reps",
            "3",
            "--revision",
            TEST_REV,
            "--history",
            history.to_str().unwrap(),
        ])
        .output()
        .expect("run xtask bench");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!call_log.exists(), "fake-fid must never be invoked for uncatalogued datasets");

    let p = history.join(TEST_REV).join("min-2026").join("bench.missing.json");
    let rec = BenchRecord::from_json(&fs::read(&p).unwrap()).unwrap();
    assert_eq!(rec.status, "not-run");
    assert_eq!(rec.reason.as_deref(), Some(REASON_NOT_IN_CATALOGUE));
    assert!(rec.results.is_empty());

    // Part 2: wrong digest in cache under --offline -> not-run with fid-data fetch error code.
    let bytes = fs::read(fx.join("blob-a.dat")).unwrap();
    let hex = sha256_hex(&bytes);
    let bad_dir = cache.join("bench-ds-a").join(&hex);
    fs::create_dir_all(&bad_dir).unwrap();
    fs::write(bad_dir.join("bench-ds-a.dat"), b"tampered-bytes").unwrap();

    let out2 = Command::new(xtask_bin())
        .env("FID_DATA_BIN", fid_data_bin())
        .env("FAKE_FID_CALL_LOG", &call_log)
        .args([
            "bench",
            "run",
            "--suite",
            fx.join("suite.toml").to_str().unwrap(),
            "--workload",
            "bench.load",
            "--profiles",
            fx.join("profiles.toml").to_str().unwrap(),
            "--catalogue",
            fx.join("datasets.toml").to_str().unwrap(),
            "--cache",
            cache.to_str().unwrap(),
            "--offline",
            "--machine-profile",
            "min-2026",
            "--fid",
            fx.join("fake-fid").to_str().unwrap(),
            "--reps",
            "3",
            "--revision",
            TEST_REV,
            "--history",
            history.to_str().unwrap(),
        ])
        .output()
        .expect("run xtask bench offline bad cache");
    assert!(out2.status.success(), "stderr: {}", String::from_utf8_lossy(&out2.stderr));
    assert!(!call_log.exists(), "fake-fid must not be called when cache verification fails");

    let p2 = history.join(TEST_REV).join("min-2026").join("bench.load.json");
    let rec2 = BenchRecord::from_json(&fs::read(&p2).unwrap()).unwrap();
    assert_eq!(rec2.status, "not-run");
    let reason = rec2.reason.as_deref().expect("not-run record must have fetch error code");
    assert!(reason.starts_with("FETCH-"), "expected FETCH-* error code, got {reason}");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac3_reps_below_3_and_unknown_profile_exit_1_and_write_nothing() {
    let tmp = fresh_tmp("ac3");
    let history = tmp.join("history");
    let fx = bench_fixtures();

    // --reps 2 -> exit 1 with BENCH-REPS.
    let out_reps = Command::new(xtask_bin())
        .args([
            "bench",
            "run",
            "--suite",
            fx.join("suite.toml").to_str().unwrap(),
            "--profiles",
            fx.join("profiles.toml").to_str().unwrap(),
            "--machine-profile",
            "min-2026",
            "--fid",
            fx.join("fake-fid").to_str().unwrap(),
            "--reps",
            "2",
            "--history",
            history.to_str().unwrap(),
        ])
        .output()
        .expect("run reps=2");
    assert_eq!(out_reps.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out_reps.stderr).contains(ERR_REPS));
    assert!(!history.exists(), "reps failure must write nothing");

    // --machine-profile nope -> exit 1 with BENCH-PROFILE and writes nothing.
    let out_prof = Command::new(xtask_bin())
        .args([
            "bench",
            "run",
            "--suite",
            fx.join("suite.toml").to_str().unwrap(),
            "--profiles",
            fx.join("profiles.toml").to_str().unwrap(),
            "--machine-profile",
            "nope",
            "--fid",
            fx.join("fake-fid").to_str().unwrap(),
            "--reps",
            "3",
            "--history",
            history.to_str().unwrap(),
        ])
        .output()
        .expect("run unknown profile");
    assert_eq!(out_prof.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out_prof.stderr).contains(ERR_PROFILE));
    assert!(!history.exists(), "profile failure must write nothing");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn ac5_compare_prints_differences_and_warns_bench_incomparable() {
    let fx = bench_fixtures();
    let out = Command::new(xtask_bin())
        .args([
            "bench",
            "compare",
            fx.join("record-a.json").to_str().unwrap(),
            fx.join("record-b.json").to_str().unwrap(),
        ])
        .output()
        .expect("run bench compare");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("BENCH-INCOMPARABLE machine_profile_id"), "stdout:\n{stdout}");
    assert!(stdout.contains("elapsed_ms"), "stdout:\n{stdout}");
    assert!(stdout.contains("-20 ms"), "stdout:\n{stdout}");
    assert!(stdout.contains("-20%"), "stdout:\n{stdout}");
}
