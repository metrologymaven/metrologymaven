//! CLI tests for `cargo xtask mig corpus-check` (`MIG.COVER.CORPUS` AC-1..AC-6). Fixtures and
//! expected digests come from `fixtures/mig/cover/corpus/make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};

fn fix(sub: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mig/cover/corpus").join(sub)
}

fn run(manifest: &str, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .args(["mig", "corpus-check", "--manifest"])
        .arg(fix(manifest))
        .arg("--catalogue")
        .arg(fix("datasets.toml"))
        .arg("--sources")
        .arg(fix("sources.json"))
        .args(extra)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn expected_digest(name: &str) -> String {
    std::fs::read_to_string(fix(name)).unwrap().trim_end().to_string()
}

#[test]
fn ac1_ok_prints_the_expected_digest() {
    let o = run("ok/corpus.json", &[]);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.lines().any(|l| l == format!("corpus_digest: {}", expected_digest("ok/expected-digest.txt"))), "{out}");
    assert!(out.lines().any(|l| l == "private_run: false"), "{out}");
    // Without --registry-manifest the pass says the registry was not checked; no bare ok.
    assert!(out.lines().any(|l| l == "format_registry: not checked (no --registry-manifest)"), "{out}");
    assert!(!out.lines().any(|l| l == "format_registry: checked"), "{out}");
    let ok_line = out.lines().find(|l| l.starts_with("corpus-check: ok")).unwrap();
    assert!(ok_line.starts_with("corpus-check: ok (format registry not checked) "), "{out}");
}

#[test]
fn ac2_uncatalogued() {
    let o = run("missing.json", &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).lines().any(|l| l == "CORPUS-UNCATALOGUED mig-qif-demo-z"), "{}", stdout(&o));
}

#[test]
fn ac3_public_fetch_only() {
    let o = run("public-fetchonly.json", &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).lines().any(|l| l.starts_with("CORPUS-VISIBILITY mig-qif-fetchonly")), "{}", stdout(&o));
}

#[test]
fn ac4_pending_source() {
    let o = run("pending-source.json", &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).lines().any(|l| l.starts_with("CORPUS-SOURCE points-csv")), "{}", stdout(&o));
}

#[test]
fn ac5_private_run_label() {
    let o = run("mixed.json", &[]);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.lines().any(|l| l == "private_run: true"), "{out}");
    assert!(out.lines().any(|l| l == format!("corpus_digest: {}", expected_digest("expected-digest-mixed.txt"))));
}

#[test]
fn ac6_shuffled_entries_same_digest() {
    let o = run("ok/corpus-shuffled.json", &[]);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.lines().any(|l| l == format!("corpus_digest: {}", expected_digest("ok/expected-digest.txt"))), "{out}");
}

#[test]
fn registry_manifest_and_usage_errors() {
    let o = run("ok/corpus.json", &["--registry-manifest", fix("registry-qif.json").to_str().unwrap()]);
    let out = stdout(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.lines().any(|l| l == "format_registry: checked"), "{out}");
    assert!(!out.lines().any(|l| l.starts_with("format_registry: not checked")), "{out}");
    assert!(out.lines().any(|l| l.starts_with("corpus-check: ok (format registry checked) ")), "{out}");
    let o = run("ok/corpus.json", &["--registry-manifest", fix("registry-empty.json").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("CORPUS-SCHEMA qif not a registered format"), "{}", stdout(&o));
    // Missing file: exit 2. Unknown or repeated option: exit 2.
    assert_eq!(run("no-such-file.json", &[]).status.code(), Some(2));
    assert_eq!(run("ok/corpus.json", &["--bogus", "x"]).status.code(), Some(2));
    assert_eq!(run("ok/corpus.json", &["--sources", "x"]).status.code(), Some(2));
    let o = Command::new(env!("CARGO_BIN_EXE_fid-xtask")).args(["mig", "corpus-check"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
}
