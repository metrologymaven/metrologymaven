//! CLI integration tests for `cargo xtask mig taxonomy-check` (`MIG.COVER.TAXO` AC-1..AC-5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn xtask_bin() -> &'static str {
    env!("CARGO_BIN_EXE_fid-xtask")
}

fn fix(sub: &str) -> PathBuf {
    repo_root().join("fixtures/mig/cover/taxo").join(sub)
}

#[test]
fn cli_ac1_through_ac5_exit_codes_and_findings() {
    // AC-1: ok/ + ok/dmis-conformance.json -> exit 0
    let out = Command::new(xtask_bin())
        .args(["mig", "taxonomy-check", "--dir"])
        .arg(fix("ok"))
        .arg("--dmis-conf")
        .arg(fix("ok/dmis-conformance.json"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "stdout: {}", String::from_utf8_lossy(&out.stdout));

    // AC-2: dmis-missing/ -> exit 1 with TAXO-INCOMPLETE SNSET
    let out = Command::new(xtask_bin())
        .args(["mig", "taxonomy-check", "--dir"])
        .arg(fix("dmis-missing"))
        .arg("--dmis-conf")
        .arg(fix("ok/dmis-conformance.json"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("TAXO-INCOMPLETE SNSET"), "got: {stdout}");

    // AC-3: ok/ + --reports unknown-report/ -> exit 1 with TAXO-UNKNOWN
    let out = Command::new(xtask_bin())
        .args(["mig", "taxonomy-check", "--dir"])
        .arg(fix("ok"))
        .arg("--reports")
        .arg(fix("unknown-report"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("TAXO-UNKNOWN") && stdout.contains("point_set.weighted"), "got: {stdout}");

    // AC-4: renamed/ + --previous ok/ -> exit 1 with TAXO-BREAKING
    let out = Command::new(xtask_bin())
        .args(["mig", "taxonomy-check", "--dir"])
        .arg(fix("renamed"))
        .arg("--previous")
        .arg(fix("ok"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("TAXO-BREAKING"), "got: {stdout}");

    // AC-5: no-attest/ -> exit 1 with TAXO-ATTEST
    let out =
        Command::new(xtask_bin()).args(["mig", "taxonomy-check", "--dir"]).arg(fix("no-attest")).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("TAXO-ATTEST"), "got: {stdout}");
}
