//! Acceptance criteria of spec node `IO.CORP.FUZZ` (AC-1 .. AC-4) and REQ-001 .. REQ-004.
//!
//! Planted test-only readers (`test.panic_on_0xff` for AC-1 and `test.hang_on_empty` for AC-4)
//! live exclusively in this integration test binary and never enter production builds.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_corp::{
    check_coverage, drive_reader, fuzz_limits, generate_targets, load_manifests, load_seeds_dir, minimize_panic_input,
    parse_format_listing, render_fuzz_cargo_toml, render_target_source, run_fuzz_suite, run_fuzz_target, stage_seeds,
    target_name, CorpError, FormatId, FuzzConfig, FuzzFindingKind, FuzzReader, PlyFuzzReader, StlFuzzReader,
    FUZZ_LIMIT_BYTES, FUZZ_LIMIT_DEPTH, FUZZ_MERGE_BUDGET_SECS, FUZZ_PER_INPUT_TIMEOUT_SECS, FUZZ_RSS_LIMIT_MIB,
    FUZZ_WEEKLY_BUDGET_SECS,
};
use fid_io_reg::{BoundedSource, IoError, LimitKind};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn fix(rel: &str) -> PathBuf {
    Path::new(ROOT).join("fixtures/io/corp/manif").join(rel)
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-io-fuzz-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_corpus-fuzz")).args(args).output().unwrap()
}

// ------------------------------------------------------------------ planted test-only readers

/// AC-1 planted reader: panics whenever the input contains byte `0xFF`.
struct PanicOn0xffReader;

impl FuzzReader for PanicOn0xffReader {
    fn reader_id(&self) -> &str {
        "test.panic_on_0xff"
    }

    fn read_bounded(&self, source: &mut BoundedSource<&[u8]>) -> Result<(), IoError> {
        let mut buf = Vec::new();
        source.read_to_end_bounded(&mut buf)?;
        if buf.contains(&0xff) {
            panic!("planted panic: encountered byte 0xFF");
        }
        Ok(())
    }
}

/// AC-4 planted reader: loops forever on empty input (until `stop` is set on test cleanup so the
/// detached worker exits cleanly after the timeout finding has been recorded).
struct HangOnEmptyReader {
    stop: Arc<AtomicBool>,
}

impl FuzzReader for HangOnEmptyReader {
    fn reader_id(&self) -> &str {
        "test.hang_on_empty"
    }

    fn read_bounded(&self, source: &mut BoundedSource<&[u8]>) -> Result<(), IoError> {
        let mut buf = Vec::new();
        source.read_to_end_bounded(&mut buf)?;
        if buf.is_empty() {
            while !self.stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_planted_panic_on_0xff_fails_names_reader_and_saves_minimised_input() {
    let work = tmp("ac1");
    let fuzz_dir = work.join("fuzz");
    let seeds_dir = work.join("seeds");
    let artifacts_dir = work.join("artifacts");

    let stl_id = FormatId::parse("mesh.stl").unwrap();
    let panic_id = FormatId::parse("test.panic_on_0xff").unwrap();
    let formats = vec![stl_id.clone(), panic_id.clone()];
    generate_targets(&formats, &fuzz_dir, &seeds_dir).unwrap();

    // Seed `mesh.stl` from manifest and `test.panic_on_0xff` with a multi-byte non-crashing seed.
    let (all_manifests, errs) = load_manifests(&fix("ok"));
    assert!(errs.is_empty());
    let stl_manifest = vec![all_manifests.get(&stl_id).unwrap().clone()];
    let sr = stage_seeds(&fix("ok"), &stl_manifest, None, &seeds_dir);
    assert!(sr.is_ok());
    let panic_seeds = seeds_dir.join(target_name(&panic_id));
    fs::create_dir_all(&panic_seeds).unwrap();
    fs::write(panic_seeds.join("seed0"), b"hello world without ff").unwrap();

    let mut readers: BTreeMap<String, Arc<dyn FuzzReader>> = BTreeMap::new();
    readers.insert("mesh.stl".to_string(), Arc::new(StlFuzzReader));
    readers.insert("test.panic_on_0xff".to_string(), Arc::new(PanicOn0xffReader));

    let cfg = FuzzConfig {
        budget: Duration::from_secs(60),
        per_input_timeout: Duration::from_secs(FUZZ_PER_INPUT_TIMEOUT_SECS),
        max_iterations: Some(128),
        seed: 7,
        minimize: true,
    };

    let report =
        run_fuzz_suite(&formats, &readers, &seeds_dir, &artifacts_dir, &fuzz_dir.join("fuzz_targets"), &cfg).unwrap();

    assert!(report.coverage_errors.is_empty());
    assert!(!report.is_ok(), "suite must fail on planted panic reader");

    let findings = report.findings();
    assert_eq!(findings.len(), 1);
    let f = findings[0];
    assert_eq!(f.reader_id, "test.panic_on_0xff");
    assert_eq!(f.kind, FuzzFindingKind::Panic);
    assert!(f.artifact_path.is_file(), "minimised crash input must be written for upload");

    let saved = fs::read(&f.artifact_path).unwrap();
    assert!(saved.contains(&0xff), "saved crashing input must contain 0xFF");
    assert_eq!(saved, vec![0xff], "minimiser should shrink input down to [0xFF]");

    // Deterministic replay of saved input must reproduce the panic.
    let replayed = std::panic::catch_unwind(|| drive_reader(&PanicOn0xffReader, &saved));
    assert!(replayed.is_err());

    // Structured finding.json must exist alongside the crash artefact.
    let finding_json = f.artifact_path.parent().unwrap().join("finding.json");
    assert!(finding_json.is_file());
    let text = fs::read_to_string(&finding_json).unwrap();
    assert!(text.contains("\"reader_id\":\"test.panic_on_0xff\""));
    assert!(text.contains("\"kind\":\"panic\""));

    // Verify minimiser directly on a large input containing 0xFF in the middle.
    let big = [0x41u8; 64].into_iter().chain([0xff]).chain([0x42u8; 64]).collect::<Vec<u8>>();
    let shrunk = minimize_panic_input(&PanicOn0xffReader, &big, 1024);
    assert_eq!(shrunk, vec![0xff]);
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_missing_fuzz_target_fails_with_fuzz_target_missing() {
    let work = tmp("ac2");
    let targets_dir = work.join("fuzz_targets");
    fs::create_dir_all(&targets_dir).unwrap();
    // Create target only for `mesh.stl`, leaving `mesh.ply` uncovered.
    let stl_id = FormatId::parse("mesh.stl").unwrap();
    fs::write(targets_dir.join("mesh_stl.rs"), render_target_source(&stl_id)).unwrap();

    let listing = fs::read(fix("registry_ok.json")).unwrap();
    let formats = parse_format_listing(&listing).unwrap();
    assert_eq!(formats.len(), 2); // mesh.ply and mesh.stl (vendor.jt is plugin, skipped)

    let cov = check_coverage(&formats, &targets_dir);
    assert!(!cov.is_ok());
    assert_eq!(cov.missing.len(), 1);
    assert_eq!(cov.missing[0].code(), "fuzz_target_missing");
    assert_eq!(cov.missing[0], CorpError::FuzzTargetMissing { format: "mesh.ply".to_string() });

    // CLI `corpus-fuzz check` must exit 1 and name `fuzz_target_missing{mesh.ply}` on stderr.
    let out = run_cli(&[
        "check",
        "--formats",
        fix("registry_ok.json").to_str().unwrap(),
        "--targets",
        targets_dir.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("fuzz_target_missing{mesh.ply}"), "stderr was: {stderr}");

    // Once committed targets dir is checked, coverage passes with exit 0.
    let committed_targets = Path::new(ROOT).join("crates/fid-io-corp/fuzz/fuzz_targets");
    let ok_out = run_cli(&[
        "check",
        "--formats",
        fix("registry_ok.json").to_str().unwrap(),
        "--targets",
        committed_targets.to_str().unwrap(),
    ]);
    assert!(ok_out.status.success(), "stderr: {}", String::from_utf8_lossy(&ok_out.stderr));
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_stl_and_ply_readers_with_manifest_seeds_report_zero_failures() {
    let work = tmp("ac3");
    let seeds_root = work.join("seeds");
    let artifacts_dir = work.join("artifacts");

    let listing = fs::read(fix("registry_ok.json")).unwrap();
    let formats = parse_format_listing(&listing).unwrap();
    let (all_manifests, errs) = load_manifests(&fix("ok"));
    assert!(errs.is_empty());
    let manifests: Vec<_> = formats.iter().map(|f| all_manifests.get(f).unwrap().clone()).collect();

    let staged = stage_seeds(&fix("ok"), &manifests, None, &seeds_root);
    assert!(staged.is_ok());
    assert_eq!(staged.targets, 2);
    assert!(staged.seeds_staged >= 2);

    // Valid manifest seeds must parse cleanly under IO.REG.LIMITS.
    let tetra = fs::read(fix("ok/files/tetra.stl")).unwrap();
    assert!(drive_reader(&StlFuzzReader, &tetra).is_ok());
    let tri = fs::read(fix("ok/files/tri.ply")).unwrap();
    assert!(drive_reader(&PlyFuzzReader, &tri).is_ok());

    // Truncated remote binary STL fixture must be cleanly rejected without panicking.
    let trunc = fs::read(fix("remote/truncated_binary.stl")).unwrap();
    assert!(matches!(drive_reader(&StlFuzzReader, &trunc), Err(IoError::TruncatedFile { .. })));

    // Run deterministic regression fuzzing over staged seeds for STL and PLY.
    let stl_seeds = load_seeds_dir(&seeds_root.join("mesh_stl")).unwrap();
    assert!(!stl_seeds.is_empty());
    let cfg = FuzzConfig {
        budget: Duration::from_secs(10),
        per_input_timeout: Duration::from_secs(FUZZ_PER_INPUT_TIMEOUT_SECS),
        max_iterations: Some(300),
        seed: 42,
        minimize: true,
    };
    let stl_outcome = run_fuzz_target(Arc::new(StlFuzzReader), &stl_seeds, &artifacts_dir, &cfg).unwrap();
    assert_eq!(stl_outcome.executions, 300);
    assert!(stl_outcome.finding.is_none(), "STL fuzz regression failed: {:?}", stl_outcome.finding);

    let ply_seeds = load_seeds_dir(&seeds_root.join("mesh_ply")).unwrap();
    assert!(!ply_seeds.is_empty());
    let ply_outcome = run_fuzz_target(Arc::new(PlyFuzzReader), &ply_seeds, &artifacts_dir, &cfg).unwrap();
    assert_eq!(ply_outcome.executions, 300);
    assert!(ply_outcome.finding.is_none(), "PLY fuzz regression failed: {:?}", ply_outcome.finding);

    // End-to-end `corpus-fuzz run` CLI smoke check.
    let cli_art = work.join("cli-artifacts");
    let committed_targets = Path::new(ROOT).join("crates/fid-io-corp/fuzz/fuzz_targets");
    let out = run_cli(&[
        "run",
        "--formats",
        fix("registry_ok.json").to_str().unwrap(),
        "--targets",
        committed_targets.to_str().unwrap(),
        "--seeds",
        seeds_root.to_str().unwrap(),
        "--artifacts",
        cli_art.to_str().unwrap(),
        "--max-iters",
        "50",
        "--budget-secs",
        "10",
        "--timeout-secs",
        "10",
    ]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_planted_infinite_loop_on_empty_input_fails_with_timeout_within_10s() {
    let work = tmp("ac4");
    let artifacts_dir = work.join("artifacts");
    let stop = Arc::new(AtomicBool::new(false));
    let reader: Arc<dyn FuzzReader> = Arc::new(HangOnEmptyReader { stop: Arc::clone(&stop) });

    // Use a 500 ms per-input timeout (strictly <= 10 s REQ-003 budget) so tests stay fast while
    // proving hard external timeout enforcement against a reader that never checks Budget.
    let cfg = FuzzConfig {
        budget: Duration::from_secs(10),
        per_input_timeout: Duration::from_millis(500),
        max_iterations: Some(10),
        seed: 99,
        minimize: true,
    };

    let t0 = Instant::now();
    let outcome = run_fuzz_target(reader, &[], &artifacts_dir, &cfg).unwrap();
    let elapsed = t0.elapsed();
    stop.store(true, Ordering::Relaxed);

    assert!(elapsed < Duration::from_secs(FUZZ_PER_INPUT_TIMEOUT_SECS), "timeout took {elapsed:?} (>= 10 s)");
    let finding = outcome.finding.expect("infinite loop on empty input must produce a timeout finding");
    assert_eq!(finding.reader_id, "test.hang_on_empty");
    assert_eq!(finding.kind, FuzzFindingKind::Timeout);
    assert_eq!(finding.minimised_len, 0);
    assert!(finding.artifact_path.is_file());
    assert_eq!(fs::read(&finding.artifact_path).unwrap(), b"");
}

// ------------------------------------------------------------------ REQ-002 / REQ-003 / REQ-004

#[test]
fn req002_req003_req004_limits_budgets_and_generated_fuzz_package_match_spec() {
    // REQ-002 budgets
    assert_eq!(FUZZ_MERGE_BUDGET_SECS, 600);
    assert_eq!(FUZZ_WEEKLY_BUDGET_SECS, 28_800);
    // REQ-003 external limits
    assert_eq!(FUZZ_PER_INPUT_TIMEOUT_SECS, 10);
    assert_eq!(FUZZ_RSS_LIMIT_MIB, 2_048);
    // REQ-004 small IO.REG.LIMITS
    let lim = fuzz_limits().unwrap();
    assert_eq!(lim.bytes(), FUZZ_LIMIT_BYTES);
    assert_eq!(lim.bytes(), 16 * 1024 * 1024);
    assert_eq!(lim.depth(), FUZZ_LIMIT_DEPTH);
    assert_eq!(lim.depth(), 64);

    // Hostile binary STL declaring u32::MAX facets hits LimitExceeded{elements}, never OOM or panic.
    let mut hostile_stl = vec![0u8; 84];
    hostile_stl[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    let err = drive_reader(&StlFuzzReader, &hostile_stl).unwrap_err();
    assert!(
        matches!(err, IoError::LimitExceeded { limit: LimitKind::Elements, .. }),
        "expected LimitExceeded{{elements}}, got {err:?}"
    );

    // Input larger than 16 MiB hits LimitExceeded{bytes}.
    let huge = vec![0u8; (FUZZ_LIMIT_BYTES as usize) + 1];
    let err = drive_reader(&StlFuzzReader, &huge).unwrap_err();
    assert!(
        matches!(err, IoError::LimitExceeded { limit: LimitKind::Bytes, .. }),
        "expected LimitExceeded{{bytes}}, got {err:?}"
    );

    // Committed crates/fid-io-corp/fuzz/ files match deterministic generator output byte-for-byte.
    let listing = fs::read(fix("registry_ok.json")).unwrap();
    let formats = parse_format_listing(&listing).unwrap();
    let fuzz_root = Path::new(ROOT).join("crates/fid-io-corp/fuzz");
    assert_eq!(fs::read_to_string(fuzz_root.join("Cargo.toml")).unwrap(), render_fuzz_cargo_toml(&formats));
    for fmt in &formats {
        let p = fuzz_root.join("fuzz_targets").join(format!("{}.rs", target_name(fmt)));
        assert_eq!(fs::read_to_string(&p).unwrap(), render_target_source(fmt));
    }
}
