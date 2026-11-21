//! Acceptance criteria AC-1 .. AC-4 of spec leaf MIG.SRC.REG, run through the
//! `fid-xtask mig sources-check` binary, plus the vocabulary check and the embedded production
//! register. Fixtures are written by `fixtures/mig/src/reg/make_fixtures.py`; expected lines
//! are hand-labelled from the spec.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const DIR: &str = "fixtures/mig/src/reg";
const CLAIM_RULES: &str = "crates/fid-brand-lint/rules/claim-rules.toml";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `fid-xtask mig sources-check --register <file> [extra]`; returns (exit code, stdout).
fn run(register: &str, extra: &[&str]) -> (i32, String) {
    let go = || {
        let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
            .current_dir(repo_root())
            .args(["mig", "sources-check", "--register", register])
            .args(extra)
            .output()
            .unwrap();
        (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap().replace("\r\n", "\n"))
    };
    let got = go();
    // D0: identical output across runs.
    assert_eq!(go(), got);
    got
}

fn fx(name: &str) -> String {
    format!("{DIR}/{name}")
}

/// AC-1: exits 0.
#[test]
fn ac1_ok() {
    let (code, out) = run(&fx("register-ok.json"), &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("sources-check: ok mig-sources@1 version 0.3.0 digest sha256:"), "{out}");
    assert!(out.ends_with(" entries 5\n"), "{out}");
    // The vocabulary check finds nothing in the fixture's own wording.
    let (code, out) = run(&fx("register-ok.json"), &["--vocab-rules", CLAIM_RULES]);
    assert_eq!(code, 0, "{out}");
}

/// AC-2: exits 1 with `SRC-REVIEW <id>`.
#[test]
fn ac2_review() {
    let (code, out) = run(&fx("admitted-noreview.json"), &[]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "SRC-REVIEW src.demoscan-points-csv missing review record\nsources-check: 1 findings\n");
}

/// AC-3: exits 1 with `SRC-POLICY <id>`.
#[test]
fn ac3_policy() {
    let (code, out) = run(&fx("native-admitted.json"), &[]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "SRC-POLICY src.democmm-program-native D-033-A\nsources-check: 1 findings\n");
}

/// AC-4: exits 1 with `SRC-DUP`.
#[test]
fn ac4_dup() {
    let (code, out) = run(&fx("dup.json"), &[]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "SRC-DUP src.democmm-results-qif src.democmm-results-qif-2\nsources-check: 1 findings\n");
}

/// The vocabulary check flags a conformance claim in an export note.
#[test]
fn vocabulary_claim() {
    let (code, out) = run(&fx("vocab-claim.json"), &[]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = run(&fx("vocab-claim.json"), &["--vocab-rules", CLAIM_RULES]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with("SRC-VOCAB src.democmm-program-native CL001 "), "{out}");
}

/// The embedded production register passes, with the claim rules too.
#[test]
fn production_register_passes() {
    let (code, out) = run("data/mig/sources.json", &["--vocab-rules", CLAIM_RULES]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn usage_and_unreadable() {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .current_dir(repo_root())
        .args(["mig", "sources-check"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let (code, _) = run(&fx("no-such-file.json"), &[]);
    assert_eq!(code, 2);
}
