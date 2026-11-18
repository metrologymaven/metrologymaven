//! Acceptance criteria of spec node ECO.CI.BOTS.REG (AC-1 .. AC-5). Fixtures and their
//! expected findings under `fixtures/bots/` are hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::bots::check;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fx(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bots").join(p)
}

fn expected(file: &str) -> Vec<String> {
    std::fs::read_to_string(fx(file)).unwrap().lines().map(String::from).collect()
}

/// Runs the library check on `<dir>/bots.toml` with `<dir>/restricted-paths.txt`.
fn findings(dir: &str, restricted_dir: &str, today: &str, export: Option<&str>) -> Vec<String> {
    let export = export.map(fx);
    let register = fx(&format!("{dir}/bots.toml"));
    let restricted = fx(&format!("{restricted_dir}/restricted-paths.txt"));
    check(&register, &restricted, today, export.as_deref()).unwrap().iter().map(ToString::to_string).collect()
}

/// Runs `fid-xtask bots-check` and returns (exit code, stdout).
fn run_bin(dir: &str, today: &str, export: Option<&str>) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-xtask"));
    cmd.arg("bots-check")
        .arg("--register")
        .arg(fx(&format!("{dir}/bots.toml")))
        .arg("--restricted")
        .arg(fx(&format!("{dir}/restricted-paths.txt")))
        .arg("--today")
        .arg(today);
    if let Some(e) = export {
        cmd.arg("--review-export").arg(fx(e));
    }
    let out = cmd.output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

fn joined(lines: &[String]) -> String {
    lines.iter().map(|l| format!("{l}\n")).collect()
}

/// AC-1
#[test]
fn ac1_ok_register_passes() {
    assert_eq!(findings("ok", "ok", "2027-01-10", None), expected("ok/expected.txt"));
    assert_eq!(run_bin("ok", "2027-01-10", None), (0, "bots-check: ok\n".to_string()));
}

/// AC-2
#[test]
fn ac2_missing_exclusion() {
    let want = expected("missing-exclude/expected.txt");
    assert_eq!(want, ["BOTS-EXCLUDE hosted-x standards/**"]);
    assert_eq!(findings("missing-exclude", "missing-exclude", "2027-01-10", None), want);
    assert_eq!(run_bin("missing-exclude", "2027-01-10", None), (1, joined(&want)));
}

/// AC-3, and the 365-day boundary across a leap day.
#[test]
fn ac3_stale() {
    assert_eq!(findings("ok", "ok", "2028-06-01", None), expected("ok/expected-2028-06-01.txt"));
    assert_eq!(
        findings("stale-boundary", "stale-boundary", "2028-06-01", None),
        expected("stale-boundary/expected.txt")
    );
    assert!(findings("stale-boundary", "stale-boundary", "2028-05-31", None).is_empty());
}

/// AC-4
#[test]
fn ac4_canary_and_unregistered() {
    assert_eq!(findings("no-canary", "no-canary", "2027-01-10", None), expected("no-canary/expected.txt"));
    let got = findings("ok", "ok", "2027-01-10", Some("unregistered/reviews.json"));
    assert_eq!(got, expected("unregistered/expected.txt"));
    let (code, out) = run_bin("ok", "2027-01-10", Some("unregistered/reviews.json"));
    assert_eq!((code, out), (1, "BOTS-UNREGISTERED lint-bot[bot]\n".to_string()));
}

/// AC-5: sorted by code then bot ID, byte-identical across runs.
#[test]
fn ac5_sorted_and_deterministic() {
    let first = run_bin("missing-exclude", "2028-06-01", Some("unregistered/reviews.json"));
    let second = run_bin("missing-exclude", "2028-06-01", Some("unregistered/reviews.json"));
    assert_eq!(first, second);
    assert_eq!(
        first.1,
        "BOTS-EXCLUDE hosted-x standards/**\nBOTS-STALE hosted-x\nBOTS-STALE self-review\n\
         BOTS-UNREGISTERED lint-bot[bot]\n"
    );
    let lines: Vec<&str> = first.1.lines().collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(lines, sorted);
}

/// REQ-001: every malformed field is a BOTS-SCHEMA finding.
#[test]
fn schema_errors() {
    assert_eq!(findings("schema", "schema", "2027-01-10", None), expected("schema/expected.txt"));
}

/// REQ-003: exclusion lists from JSON, TOML and YAML at a dotted key path.
#[test]
fn config_formats() {
    assert_eq!(findings("formats", "formats", "2027-01-10", None), expected("formats/expected.txt"));
}

/// An unreadable config fails closed.
#[test]
fn missing_config_is_io() {
    let got = findings("io-missing", "io-missing", "2027-01-10", None);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].starts_with("BOTS-IO hosted-x config absent.yml: "), "{got:?}");
}

#[test]
fn missing_inputs_and_usage() {
    let got = check(&fx("nope/bots.toml"), &fx("nope/restricted-paths.txt"), "2027-01-10", None).unwrap();
    let codes: Vec<(&str, &str)> = got.iter().map(|f| (f.code, f.subject.as_str())).collect();
    assert_eq!(codes, [("BOTS-IO", "register"), ("BOTS-IO", "restricted")]);
    assert!(check(&fx("ok/bots.toml"), &fx("ok/restricted-paths.txt"), "2027-1-10", None).is_err());
    let out =
        Command::new(env!("CARGO_BIN_EXE_fid-xtask")).args(["bots-check", "--today", "2027-01-10"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .args(["bots-check", "--today", "2027-01-10", "--today", "2027-01-11"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

/// The repository's own register passes (its only entry sends no content, so the
/// restricted list does not matter until `house-rules/restricted-paths.txt` exists).
#[test]
fn repository_register_is_valid() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let got = check(&root.join("bots.toml"), &fx("ok/restricted-paths.txt"), "2026-11-17", None).unwrap();
    assert!(got.is_empty(), "{got:?}");
}
