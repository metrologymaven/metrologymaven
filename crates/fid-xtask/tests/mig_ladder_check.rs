//! Acceptance criteria of spec node MIG.LADDER.DEF (AC-1 .. AC-5), run through the
//! `fid-xtask mig ladder-check` binary. Fixtures and the reference digest are written by
//! `fixtures/mig/ladder/def/make_fixtures.py`; expected lines are hand-labelled from the spec.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const DIR: &str = "fixtures/mig/ladder/def";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `fid-xtask mig ladder-check --ladder <l> --manifest <m>`; returns (exit code, stdout).
fn run(ladder: &str, manifest: &str) -> (i32, String) {
    let go = || {
        let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
            .current_dir(repo_root())
            .args(["mig", "ladder-check", "--ladder", &format!("{DIR}/{ladder}"), "--manifest"])
            .arg(format!("{DIR}/{manifest}"))
            .output()
            .unwrap();
        (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap().replace("\r\n", "\n"))
    };
    let got = go();
    // D0: identical output across runs.
    assert_eq!(go(), got);
    got
}

/// AC-1: exits 0 and prints the ladder version and digest.
#[test]
fn ac1_ok() {
    let (code, out) = run("ladder-ok.json", "manifest-ok.json");
    assert_eq!(code, 0, "{out}");
    let digest = std::fs::read_to_string(repo_root().join(DIR).join("ladder-ok.digest")).unwrap();
    assert_eq!(out, format!("ladder-check: ok mig-ladder@1 version 0.9.0 digest {}\n", digest.trim_end()));
}

/// AC-2: exits 1 with `LADDER-FORWARD templates re_evaluate`.
#[test]
fn ac2_forward() {
    let (code, out) = run("forward.json", "manifest-ok.json");
    assert_eq!(code, 1, "{out}");
    assert_eq!(
        out,
        "LADDER-FORWARD templates re_evaluate input:incumbent_point_export\nladder-check: 1 findings\n"
    );
}

/// AC-3: exits 1 with `LADDER-MISSING-CMD migrate.import_form@1`.
#[test]
fn ac3_missing_command() {
    let (code, out) = run("ladder-ok.json", "manifest-missing.json");
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "LADDER-MISSING-CMD migrate.import_form@1 templates\nladder-check: 1 findings\n");
}

/// AC-4: stage 5 commands are planned and absent from the manifest; the check passes.
#[test]
fn ac4_planned_skipped() {
    let manifest = std::fs::read_to_string(repo_root().join(DIR).join("manifest-ok.json")).unwrap();
    assert!(!manifest.contains("migrate.prove_equivalence"));
    assert_eq!(run("ladder-ok.json", "manifest-ok.json").0, 0);
}

/// AC-5: exits 1 with `LADDER-BASIS`.
#[test]
fn ac5_basis() {
    let (code, out) = run("basis.json", "manifest-ok.json");
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "LADDER-BASIS history POINTS\nladder-check: 1 findings\n");
}

/// Usage errors and unreadable files exit 2.
#[test]
fn usage_errors() {
    let bin = env!("CARGO_BIN_EXE_fid-xtask");
    let code = |args: &[&str]| Command::new(bin).current_dir(repo_root()).args(args).output().unwrap().status.code();
    assert_eq!(code(&["mig", "ladder-check"]), Some(2));
    assert_eq!(code(&["mig", "ladder-check", "--ladder", "x.json"]), Some(2));
    assert_eq!(code(&["mig", "ladder-check", "--ladder", "no/such.json", "--manifest", "no/such.json"]), Some(2));
}
