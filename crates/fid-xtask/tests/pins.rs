//! Acceptance criteria of spec node ECO.REPO.TOPO (AC-1 .. AC-5), run through the
//! `fid-xtask pins` binary. Expected outputs are hand-labelled in `expected.txt`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `fid-xtask pins <args>` from the repository root; returns (exit code, stdout).
fn pins(args: &[&str]) -> (i32, String) {
    let out =
        Command::new(env!("CARGO_BIN_EXE_fid-xtask")).current_dir(repo_root()).arg("pins").args(args).output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap().replace("\r\n", "\n"))
}

fn check_fixture(name: &str) {
    let dir = format!("fixtures/pins/{name}");
    let want = std::fs::read_to_string(repo_root().join(&dir).join("expected.txt")).unwrap().replace("\r\n", "\n");
    let want_code = if want == "pins: ok\n" { 0 } else { 1 };
    let (code, got) = pins(&["--workspace", &dir, "--repos", "fixtures/pins/repos.toml"]);
    assert_eq!(got, want, "{name}");
    assert_eq!(code, want_code, "{name}");
    // D0: the output is identical across runs.
    assert_eq!(pins(&["--workspace", &dir, "--repos", "fixtures/pins/repos.toml"]), (code, got), "{name}");
}

/// AC-1: a 40-hex `rev`, and a `tag` on a repository with protected tags.
#[test]
fn ac1_ok() {
    check_fixture("ok");
}

/// AC-2
#[test]
fn ac2_branch() {
    check_fixture("branch");
}

/// AC-3
#[test]
fn ac3_tag_unprotected() {
    check_fixture("tag-unprotected");
}

/// AC-4: `HTTPS://Example.org/fid/libs` matches `https://example.org/fid/libs.git`.
#[test]
fn ac4_url_variant() {
    check_fixture("url-variant");
}

/// AC-5: third-party git dependencies are not this check's concern.
#[test]
fn ac5_third_party() {
    check_fixture("third-party");
}

/// `PIN-REV-SHORT`, and a workspace with glob members, an excluded member, `patch`,
/// build-, dev- and target-specific tables; findings are sorted.
#[test]
fn rev_short_and_mixed_workspace() {
    check_fixture("rev-short");
    check_fixture("mixed");
}

/// `PIN-REPOS`: a bad repository map fails the run.
#[test]
fn bad_repository_map() {
    let (code, out) = pins(&["--workspace", "fixtures/pins/ok", "--repos", "fixtures/pins/bad-repos.toml"]);
    assert_eq!(code, 1);
    assert!(out.starts_with("PIN-REPOS "), "{out}");
    assert!(out.contains("protected_tags"), "{out}");
}

/// The repository's own `repos.toml` is valid and this workspace passes the check.
#[test]
fn this_repository_passes() {
    assert_eq!(pins(&[]), (0, "pins: ok\n".to_string()));
}

#[test]
fn usage_error() {
    assert_eq!(pins(&["--workspace"]).0, 2);
    assert_eq!(pins(&["--bogus", "x"]).0, 2);
}
