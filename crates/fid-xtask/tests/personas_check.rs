//! Acceptance criteria of spec node FLOW.PERSONA.PERS (AC-1 .. AC-3), run through the
//! `fid-xtask personas-check` binary. Expected findings are hand-labelled in
//! `fixtures/flow/personas/expected.txt`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const APPS: &str = "fixtures/flow/personas/apps.toml";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `fid-xtask personas-check <args>` from the repository root; returns (exit code, stdout).
fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .current_dir(repo_root())
        .arg("personas-check")
        .args(args)
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap().replace("\r\n", "\n"))
}

fn fixture(name: &str) -> (i32, String) {
    let path = format!("fixtures/flow/personas/{name}");
    let got = run(&["--personas", &path, "--apps", APPS]);
    // D0: the output is identical across runs.
    assert_eq!(run(&["--personas", &path, "--apps", APPS]), got, "{name}");
    got
}

/// AC-1: the shipped file and the app list pass and report 9 personas.
#[test]
fn ac1_shipped_file_reports_nine_personas() {
    let (code, out) = run(&["--apps", APPS]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "personas-check: 9 personas\n");
}

/// AC-2: `missing-07.json` exits 1 with `PERS-MISSING JOB.PER.07`.
#[test]
fn ac2_missing_07() {
    let (code, out) = fixture("missing-07.json");
    assert_eq!(code, 1, "{out}");
    assert!(out.lines().any(|l| l.starts_with("PERS-MISSING JOB.PER.07")), "{out}");
}

/// AC-3: `bad-app.json` (`primary_app: "measure"`) exits 1 with `PERS-APP`.
#[test]
fn ac3_bad_app() {
    let (code, out) = fixture("bad-app.json");
    assert_eq!(code, 1, "{out}");
    assert!(out.lines().any(|l| l.starts_with("PERS-APP JOB.PER.01") && l.contains("measure")), "{out}");
}

/// Every hand-labelled fixture: exit code and the `<CODE> <subject>` of each output line.
#[test]
fn fixtures_match_hand_labels() {
    let text = std::fs::read_to_string(repo_root().join("fixtures/flow/personas/expected.txt")).unwrap();
    let mut names: Vec<&str> =
        text.lines().filter(|l| !l.starts_with('#')).filter_map(|l| l.split_once('\t')).map(|(f, _)| f).collect();
    names.dedup();
    assert!(names.len() >= 9);
    for name in names {
        let want: Vec<&str> = text
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .filter(|(f, v)| *f == name && *v != "ok")
            .map(|(_, v)| v)
            .collect();
        let (code, out) = fixture(name);
        let mut lines: Vec<&str> = out.lines().collect();
        let summary = lines.pop().unwrap();
        let got: Vec<String> = lines
            .iter()
            .map(|l| {
                let (head, _) = l.split_once(": ").unwrap();
                head.to_string()
            })
            .collect();
        assert_eq!(got, want, "{name}: {out}");
        if want.is_empty() {
            assert_eq!((code, summary), (0, "personas-check: 9 personas"), "{name}");
        } else {
            assert_eq!(code, 1, "{name}");
            assert_eq!(summary, format!("personas-check: {} findings", want.len()), "{name}");
        }
    }
}

#[test]
fn missing_app_list_fails_and_bad_usage_is_2() {
    let (code, out) = run(&["--apps", "fixtures/flow/personas/no-such.toml"]);
    assert_eq!(code, 1);
    assert!(out.starts_with("PERS-APP -: cannot read app list"), "{out}");
    assert_eq!(run(&["--bogus"]).0, 2);
}
