//! Acceptance criteria AC-1, AC-2 and AC-4 of spec node QUAL.VAP.TPL, run through the
//! `fid-xtask qual vap check-templates` binary. The template sets are written by
//! `fixtures/qual/vap/tpl/make_cases.py`; expected lines are hand-labelled from the spec.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const DIR: &str = "fixtures/qual/vap/tpl";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `fid-xtask qual vap check-templates <args>`; returns (exit code, stdout).
fn run(args: &[&str]) -> (i32, String) {
    let go = || {
        let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
            .current_dir(repo_root())
            .args(["qual", "vap", "check-templates"])
            .args(args)
            .output()
            .unwrap();
        (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap().replace("\r\n", "\n"))
    };
    let got = go();
    // D0: identical output across runs.
    assert_eq!(go(), got);
    got
}

/// AC-1: the eight-template set passes.
#[test]
fn ac1_set_ok() {
    let (code, out) = run(&["--dir", &format!("{DIR}/set-ok/")]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "check-templates: ok, 8 templates\n");
}

/// AC-2: without the supplier questionnaire the check exits 1 naming it.
#[test]
fn ac2_missing_supplier_questionnaire() {
    let (code, out) = run(&["--dir", &format!("{DIR}/set-no-supq/")]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "TPL-MISSING supplier-questionnaire\ncheck-templates: 1 findings\n");
}

/// AC-4: a template without a licence notice fails, named.
#[test]
fn ac4_missing_licence() {
    let (code, out) = run(&["--dir", &format!("{DIR}/set-no-licence")]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "TPL-LICENCE risk: no licence notice\ncheck-templates: 1 findings\n");
}

/// An explicit catalogue path works; usage errors and unreadable inputs exit 2.
#[test]
fn usage_and_io_errors() {
    let (code, out) = run(&["--dir", &format!("{DIR}/set-ok"), "--catalogue", "vap/intended-uses.toml"]);
    assert_eq!((code, out.as_str()), (0, "check-templates: ok, 8 templates\n"));
    assert_eq!(run(&[]).0, 2);
    assert_eq!(run(&["--dir"]).0, 2);
    assert_eq!(run(&["--dir", "a", "--dir", "b"]).0, 2);
    assert_eq!(run(&["--dir", &format!("{DIR}/set-ok"), "--catalogue", "no/such/file.toml"]).0, 2);
    assert_eq!(run(&["--dir", &format!("{DIR}/no-such-dir")]).0, 2);
}
