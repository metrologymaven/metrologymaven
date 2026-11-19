//! Acceptance criteria of spec node ECO.REPO.ALLOW (AC-1 .. AC-6).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::kernel_allow::{run, ALLOWLIST_FILE};
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/allow")
}

/// Warnings, then findings, as `expected.txt` lists them.
fn check_fixture(name: &str) {
    let ws = fixtures().join(name);
    let want: Vec<String> =
        std::fs::read_to_string(ws.join("expected.txt")).unwrap().lines().map(String::from).collect();
    let report = run(&ws, &ws.join("layers.toml"), &ws.join(ALLOWLIST_FILE)).unwrap_or_else(|e| panic!("{name}: {e}"));
    let got: Vec<String> = report.warnings.iter().chain(&report.findings).cloned().collect();
    assert_eq!(got, want, "{name}");
    let again = run(&ws, &ws.join("layers.toml"), &ws.join(ALLOWLIST_FILE)).unwrap();
    assert_eq!(again, report, "{name}: not deterministic");
}

/// Runs the binary on a fixture: (exit code, stdout lines).
fn cli(name: &str) -> (i32, Vec<String>) {
    let ws = fixtures().join(name);
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .args(["kernel-allow", "--workspace"])
        .arg(&ws)
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap().lines().map(String::from).collect())
}

/// AC-1
#[test]
fn ac1_ok() {
    check_fixture("ok");
    assert_eq!(cli("ok"), (0, vec!["kernel-allow: ok".to_string()]));
}

/// AC-2
#[test]
fn ac2_unlisted() {
    check_fixture("unlisted");
    assert_eq!(cli("unlisted"), (1, vec!["ALLOW-UNLISTED ext-b via fid-krn -> ext-a -> ext-b".to_string()]));
}

/// AC-3
#[test]
fn ac3_version() {
    check_fixture("version");
    assert_eq!(cli("version"), (1, vec!["ALLOW-VERSION ext-a 2.0.0 ^1.2".to_string()]));
}

/// AC-4
#[test]
fn ac4_schema() {
    check_fixture("schema");
    let (code, out) = cli("schema");
    assert_eq!(code, 1);
    assert!(out.iter().any(|l| l.starts_with("ALLOW-SCHEMA")), "{out:?}");
}

/// AC-5
#[test]
fn ac5_unused() {
    check_fixture("unused");
    assert_eq!(cli("unused"), (0, vec!["warn ALLOW-UNUSED ext-z".to_string(), "kernel-allow: ok".to_string()]));
}

/// AC-6
#[test]
fn ac6_viewer_not_checked() {
    check_fixture("viewer");
    assert_eq!(cli("viewer").0, 0);
}

/// REQ-001: build-dependencies count (also through first-party crates); dev-dependencies do not.
#[test]
fn build_edges_count_dev_edges_do_not() {
    check_fixture("edges");
    assert_eq!(cli("edges").0, 1);
}

/// Fail closed: a missing allowlist file is an error, not a pass.
#[test]
fn missing_allowlist_fails() {
    let ws = fixtures().join("ok");
    let err = run(&ws, &ws.join("layers.toml"), &ws.join("no-such-allowlist.toml")).unwrap_err();
    assert!(err.starts_with("ALLOW-FILE"), "{err}");
}

/// The repository's own kernel graph matches its allowlist, with no unused entries.
#[test]
fn repository_kernel_graph_is_allowed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let report = run(&root, &root.join("layers.toml"), &root.join(ALLOWLIST_FILE)).unwrap();
    assert!(report.findings.is_empty() && report.warnings.is_empty(), "{report:?}");
}
