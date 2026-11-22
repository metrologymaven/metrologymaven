//! Acceptance and error-path tests for `AUTO.SURF.CHECK` (`fid_surface_gen::check`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use fid_canon::{parse, CValue};
use fid_surface_gen::{check_surfaces, CheckError, CommandRef, Finding, FindingKind};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("crates/").parent().expect("repo root").to_path_buf()
}

fn manifest_path() -> PathBuf {
    repo_root().join("fixtures/manifest/min-classes.json")
}

fn manifest_bytes() -> Vec<u8> {
    fs::read(manifest_path()).expect("read min-classes.json")
}

fn surfaces_dir(name: &str) -> PathBuf {
    repo_root().join("fixtures/surfaces").join(name)
}

fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fid-surface-check"))
}

static TEST_TMP: AtomicU64 = AtomicU64::new(0);

fn fresh_temp_copy(src: &Path) -> PathBuf {
    let n = TEST_TMP.fetch_add(1, Ordering::Relaxed);
    let dst = std::env::temp_dir().join(format!("fid-surf-check-test-{}-{n}", std::process::id()));
    if dst.exists() {
        fs::remove_dir_all(&dst).expect("clean old test dir");
    }
    copy_dir_recursive(src, &dst);
    dst
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("mkdir");
    for entry in fs::read_dir(src).expect("read_dir") {
        let entry = entry.expect("dir entry");
        let sp = entry.path();
        let dp = dst.join(entry.file_name());
        if sp.is_dir() {
            copy_dir_recursive(&sp, &dp);
        } else {
            fs::copy(&sp, &dp).expect("copy file");
        }
    }
}

/// AC-1: `fixtures/surfaces/complete/` against `fixtures/manifest/min-classes.json`
/// exits 0 with an empty findings list.
#[test]
fn ac1_complete_fixture_is_clean() {
    let report = check_surfaces(&manifest_bytes(), &surfaces_dir("complete")).expect("check complete");
    assert!(report.is_clean(), "expected empty findings, got: {:?}", report.findings);
    assert_eq!(report.to_canonical_json().expect("json"), r#"{"findings":[]}"#);

    let out = Command::new(bin_path())
        .args([
            "--manifest",
            manifest_path().to_str().expect("utf8"),
            "--generated",
            surfaces_dir("complete").to_str().expect("utf8"),
        ])
        .output()
        .expect("run fid-surface-check");
    assert!(out.status.success(), "expected exit 0, stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).expect("utf8 stdout");
    assert_eq!(stdout.trim(), r#"{"findings":[]}"#);
}

/// AC-2: `fixtures/surfaces/missing-py/` exits non-zero with exactly one `missing_surface`
/// finding naming `align.run_bestfit@1` and surface `py`.
#[test]
fn ac2_missing_py_reports_exact_missing_surface() {
    let report = check_surfaces(&manifest_bytes(), &surfaces_dir("missing-py")).expect("check missing-py");
    let missing: Vec<&Finding> = report.findings.iter().filter(|f| f.kind == FindingKind::MissingSurface).collect();
    assert_eq!(missing.len(), 1, "expected exactly 1 missing_surface, got: {:?}", report.findings);
    assert_eq!(missing[0].surface, "py");
    assert_eq!(missing[0].command, Some(CommandRef { command_id: "align.run_bestfit@1".to_string(), major: 1 }));
    assert_eq!(missing[0].path.as_deref(), Some("py/surface-index.json"));
    assert_eq!(report.findings.len(), 1);

    let out = Command::new(bin_path())
        .args([
            "--manifest",
            manifest_path().to_str().expect("utf8"),
            "--generated",
            surfaces_dir("missing-py").to_str().expect("utf8"),
        ])
        .output()
        .expect("run fid-surface-check");
    assert_eq!(out.status.code(), Some(1));
    let val = parse(&out.stdout).expect("stdout must be valid JSON");
    let CValue::Obj(root) = val else { panic!("expected JSON object") };
    let Some(CValue::Arr(arr)) = root.get("findings") else { panic!("missing findings array") };
    assert_eq!(arr.len(), 1);
}

/// AC-3: `fixtures/surfaces/orphan/` whose CLI index names `demo.ghost_cmd@1`
/// reports `orphan_entry` for it (and `stale_file` because `cli/surface-index.json` differs
/// from the registered `CliEmitter` output).
#[test]
fn ac3_orphan_reports_orphan_entry() {
    let report = check_surfaces(&manifest_bytes(), &surfaces_dir("orphan")).expect("check orphan");
    let orphans: Vec<&Finding> = report.findings.iter().filter(|f| f.kind == FindingKind::OrphanEntry).collect();
    assert_eq!(orphans.len(), 1, "expected 1 orphan_entry, got: {:?}", report.findings);
    let f = orphans[0];
    assert_eq!(f.kind, FindingKind::OrphanEntry);
    assert_eq!(f.surface, "cli");
    assert_eq!(f.command, Some(CommandRef { command_id: "demo.ghost_cmd@1".to_string(), major: 1 }));
    assert_eq!(f.path.as_deref(), Some("cli/surface-index.json"));

    let out = Command::new(bin_path())
        .args([
            "--manifest",
            manifest_path().to_str().expect("utf8"),
            "--generated",
            surfaces_dir("orphan").to_str().expect("utf8"),
        ])
        .output()
        .expect("run fid-surface-check");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(stdout.contains(r#""kind":"orphan_entry""#), "stdout: {stdout}");
    assert!(stdout.contains(r#""command_id":"demo.ghost_cmd@1""#), "stdout: {stdout}");
}

/// AC-4: `fixtures/surfaces/m1-listed/` whose MCP index lists the M1 command
/// reports `unexpected_mcp_entry` (and `stale_file` because `mcp/surface-index.json` differs).
#[test]
fn ac4_m1_listed_reports_unexpected_mcp_entry() {
    let report = check_surfaces(&manifest_bytes(), &surfaces_dir("m1-listed")).expect("check m1-listed");
    let unexpected: Vec<&Finding> =
        report.findings.iter().filter(|f| f.kind == FindingKind::UnexpectedMcpEntry).collect();
    assert_eq!(unexpected.len(), 1, "expected 1 unexpected_mcp_entry, got: {:?}", report.findings);
    assert_eq!(unexpected[0].surface, "mcp");
    assert_eq!(unexpected[0].command, Some(CommandRef { command_id: "motion.move_axis@1".to_string(), major: 1 }));

    // Findings must be sorted by kind wire name then path (`stale_file` < `unexpected_mcp_entry`).
    let kinds: Vec<&str> = report.findings.iter().map(|f| f.kind.as_str()).collect();
    let mut sorted = kinds.clone();
    sorted.sort();
    assert_eq!(kinds, sorted, "findings not sorted by kind: {:?}", report.findings);

    let out = Command::new(bin_path())
        .args([
            "--manifest",
            manifest_path().to_str().expect("utf8"),
            "--generated",
            surfaces_dir("m1-listed").to_str().expect("utf8"),
        ])
        .output()
        .expect("run fid-surface-check");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(stdout.contains(r#""kind":"unexpected_mcp_entry""#), "stdout: {stdout}");
    assert!(stdout.contains(r#""command_id":"motion.move_axis@1""#), "stdout: {stdout}");
}

/// AC-5: `complete/` with one byte edited in a generated file reports `stale_file` naming it,
/// and also detects missing and extra files in the generated tree.
#[test]
fn ac5_one_byte_edit_missing_and_extra_files_report_stale_file() {
    // 1. One-byte edit in mcp/mcp-tools.json
    let tmp = fresh_temp_copy(&surfaces_dir("complete"));
    let tools_path = tmp.join("mcp/mcp-tools.json");
    let mut bytes = fs::read(&tools_path).expect("read tools");
    let last = bytes.len() - 1;
    bytes[last] = if bytes[last] == b'\n' { b' ' } else { b'\n' };
    fs::write(&tools_path, &bytes).expect("write mutated tools");

    let report = check_surfaces(&manifest_bytes(), &tmp).expect("check mutated");
    assert_eq!(report.findings.len(), 1, "expected 1 stale_file, got: {:?}", report.findings);
    assert_eq!(report.findings[0].kind, FindingKind::StaleFile);
    assert_eq!(report.findings[0].surface, "mcp");
    assert_eq!(report.findings[0].path.as_deref(), Some("mcp/mcp-tools.json"));
    assert_eq!(report.findings[0].command, None);

    let out = Command::new(bin_path())
        .args(["--manifest", manifest_path().to_str().expect("utf8"), "--generated", tmp.to_str().expect("utf8")])
        .output()
        .expect("run fid-surface-check");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(stdout.contains(r#""kind":"stale_file""#), "stdout: {stdout}");
    assert!(stdout.contains(r#""path":"mcp/mcp-tools.json""#), "stdout: {stdout}");

    // 2. Extra file in mcp/
    let tmp2 = fresh_temp_copy(&surfaces_dir("complete"));
    fs::write(tmp2.join("mcp/extra.json"), b"{}\n").expect("write extra");
    let report2 = check_surfaces(&manifest_bytes(), &tmp2).expect("check extra");
    assert_eq!(report2.findings.len(), 1);
    assert_eq!(report2.findings[0].kind, FindingKind::StaleFile);
    assert_eq!(report2.findings[0].path.as_deref(), Some("mcp/extra.json"));

    // 3. Missing generated file in mcp/
    let tmp3 = fresh_temp_copy(&surfaces_dir("complete"));
    fs::remove_file(tmp3.join("mcp/mcp-tools.json")).expect("remove tools");
    let report3 = check_surfaces(&manifest_bytes(), &tmp3).expect("check missing");
    assert_eq!(report3.findings.len(), 1);
    assert_eq!(report3.findings[0].kind, FindingKind::StaleFile);
    assert_eq!(report3.findings[0].path.as_deref(), Some("mcp/mcp-tools.json"));

    let _ = fs::remove_dir_all(&tmp);
    let _ = fs::remove_dir_all(&tmp2);
    let _ = fs::remove_dir_all(&tmp3);
}

/// Operational errors (`manifest_parse`, `index_parse`, `io_error`) are distinct from findings
/// and exit with status 2.
#[test]
fn operational_errors_are_typed_and_exit_2() {
    // manifest_parse
    let err = check_surfaces(b"{not json", &surfaces_dir("complete")).expect_err("bad manifest");
    assert_eq!(err.code(), "manifest_parse");
    assert!(matches!(err, CheckError::ManifestParse { .. }));

    // index_parse
    let tmp = fresh_temp_copy(&surfaces_dir("complete"));
    fs::write(tmp.join("py/surface-index.json"), b"not-json").expect("corrupt index");
    let err = check_surfaces(&manifest_bytes(), &tmp).expect_err("bad index");
    assert_eq!(err.code(), "index_parse");
    assert!(matches!(err, CheckError::IndexParse { ref surface, .. } if surface == "py"));

    let out = Command::new(bin_path())
        .args(["--manifest", manifest_path().to_str().expect("utf8"), "--generated", tmp.to_str().expect("utf8")])
        .output()
        .expect("run fid-surface-check");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("index_parse"));

    // io_error (missing index file)
    fs::remove_file(tmp.join("py/surface-index.json")).expect("remove index");
    let err = check_surfaces(&manifest_bytes(), &tmp).expect_err("missing index");
    assert_eq!(err.code(), "io_error");
    assert!(matches!(err, CheckError::IoError { .. }));

    // missing manifest file via CLI
    let out_missing = Command::new(bin_path())
        .args(["--manifest", tmp.join("nope.json").to_str().expect("utf8"), "--generated", tmp.to_str().expect("utf8")])
        .output()
        .expect("run fid-surface-check");
    assert_eq!(out_missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out_missing.stderr).contains("io_error"));

    // bad CLI args
    let out_usage = Command::new(bin_path()).output().expect("run fid-surface-check");
    assert_eq!(out_usage.status.code(), Some(2));

    let _ = fs::remove_dir_all(&tmp);
}
