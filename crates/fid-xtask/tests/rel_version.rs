//! Acceptance criteria of spec node ECO.REL.PROC.VERSION (AC-1 .. AC-6). Expected outcomes
//! come from `fixtures/rel/version/make_cases.py` (an independent Python reference) or are
//! hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::rel::version::{
    check_tag, ordering_word, parse_any, parse_tag, version_check, Channel, Date, Pre, Version, VersionError,
};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/rel/version").join(name)
}

/// Runs `fid-xtask rel <args>` from the workspace root (what `cargo xtask rel ...` runs);
/// returns (exit code, stdout).
fn cli(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask")).current_dir(root()).arg("rel").args(args).output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

fn csv_rows(name: &str) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(fixture(name)).unwrap();
    let mut lines = text.lines();
    lines.next(); // header
    lines.map(|l| l.split(',').map(String::from).collect()).collect()
}

/// AC-1: a beta tag on the beta channel against a matching workspace exits 0.
#[test]
fn ac1_beta_tag_passes() {
    let (code, out) = cli(&[
        "version-check",
        "--tag",
        "v1.4.0-beta.2",
        "--channel",
        "beta",
        "--workspace",
        "fixtures/rel/version/ws-1.4.0-beta.2",
    ]);
    assert_eq!((code, out.as_str()), (0, "version-check: ok 1.4.0-beta.2 (beta)\n"));
}

/// AC-2: `v1.4` and `v01.4.0` fail with VERSION-TAG-FORMAT.
#[test]
fn ac2_bad_tag_format() {
    for tag in ["v1.4", "v01.4.0"] {
        let (code, out) = cli(&["version-check", "--tag", tag, "--channel", "stable"]);
        assert_eq!(code, 1, "{tag}");
        assert!(out.starts_with("VERSION-TAG-FORMAT: "), "{tag}: {out}");
    }
}

/// AC-3: a beta tag on the stable channel fails with VERSION-CHANNEL-SUFFIX.
#[test]
fn ac3_channel_suffix() {
    let (code, out) = cli(&[
        "version-check",
        "--tag",
        "v1.4.0-beta.2",
        "--channel",
        "stable",
        "--workspace",
        "fixtures/rel/version/ws-1.4.0-beta.2",
    ]);
    assert_eq!(code, 1);
    assert!(out.starts_with("VERSION-CHANNEL-SUFFIX: "), "{out}");
}

/// AC-4: month 13 fails with VERSION-DATE.
#[test]
fn ac4_bad_nightly_date() {
    let (code, out) = cli(&["version-check", "--tag", "v1.4.0-nightly.20261332", "--channel", "nightly"]);
    assert_eq!(code, 1);
    assert!(out.starts_with("VERSION-DATE: "), "{out}");
}

/// AC-5: the `fid-verify` stub reports 1.3.9; the check names it, and only it.
#[test]
fn ac5_binary_mismatch_names_binary() {
    let (code, out) = cli(&[
        "version-check",
        "--tag",
        "v1.4.0",
        "--channel",
        "stable",
        "--workspace",
        "fixtures/rel/version/bin-mismatch",
        "--binaries",
        "fixtures/rel/version/bin-mismatch/bin",
    ]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(out, "VERSION-MISMATCH(fid-verify): reports \"1.3.9\", tag version is 1.4.0\n");
}

/// AC-6: every row of precedence.csv (written by the Python reference) matches, both
/// through the library and through `rel version-compare`.
#[test]
fn ac6_precedence_csv() {
    let rows = csv_rows("precedence.csv");
    assert!(rows.len() > 100);
    for r in &rows {
        let (a, b) = (parse_any(&r[0]).unwrap(), parse_any(&r[1]).unwrap());
        assert_eq!(ordering_word(a.precedence(&b)), r[2], "{} vs {}", r[0], r[1]);
    }
    // The spec's own examples, through the command (with and without the `v`).
    assert_eq!(cli(&["version-compare", "1.4.0-beta.2", "1.4.0"]), (0, "lt\n".into()));
    assert_eq!(cli(&["version-compare", "v1.4.0-beta.10", "v1.4.0-beta.2"]), (0, "gt\n".into()));
    assert_eq!(cli(&["version-compare", "1.4.0", "v1.4.0"]), (0, "eq\n".into()));
    let (code, out) = cli(&["version-compare", "1.4", "1.4.0"]);
    assert_eq!(code, 1);
    assert!(out.starts_with("VERSION-TAG-FORMAT: "), "{out}");
}

/// REQ-001..003: every row of the fixture tag list (labelled by the Python reference).
#[test]
fn tag_list_fixture() {
    let rows = csv_rows("tags.csv");
    assert!(rows.len() > 30);
    for r in &rows {
        let channel = Channel::parse(&r[1]).unwrap();
        let got = match check_tag(&r[0], channel) {
            Ok(_) => "ok",
            Err(e) => e.code(),
        };
        assert_eq!(got, r[2], "tag {:?} channel {}", r[0], r[1]);
    }
}

#[test]
fn parsed_structure_and_display() {
    let v = parse_tag("v1.4.0-nightly.20240229").unwrap();
    let d = Date { year: 2024, month: 2, day: 29 };
    assert_eq!(v, Version { major: 1, minor: 4, patch: 0, pre: Some(Pre::Nightly(d)) });
    assert_eq!(v.to_string(), "1.4.0-nightly.20240229");
    assert_eq!(parse_tag("v2.0.7-beta.3").unwrap().pre, Some(Pre::Beta(3)));
    assert_eq!(parse_tag("v2.0.7").unwrap().to_string(), "2.0.7");
}

/// Hostile input: long tags are refused and the message stays short.
#[test]
fn long_tag_is_bounded() {
    let tag = format!("v1.4.0-beta.{}", "9".repeat(10_000));
    let e = parse_tag(&tag).unwrap_err();
    assert_eq!(e.code(), "VERSION-TAG-FORMAT");
    assert!(e.to_string().len() < 200, "{e}");
}

/// REQ-004: the workspace version is checked too, and named.
#[test]
fn workspace_mismatch_is_named() {
    let r = version_check("v1.4.0", Channel::Stable, &fixture("ws-mismatch"), None);
    let e = r.unwrap_err();
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].code(), "VERSION-MISMATCH");
    assert_eq!(e[0].subject(), Some("workspace"));
}

/// REQ-004: all binaries agree (including an app), so the check passes; `--json` gives a
/// deterministic report.
#[test]
fn binaries_ok_and_json_report() {
    let args = [
        "version-check",
        "--tag",
        "v1.4.0",
        "--channel",
        "lts",
        "--workspace",
        "fixtures/rel/version/bin-ok",
        "--binaries",
        "fixtures/rel/version/bin-ok/bin",
        "--json",
    ];
    let (code, out) = cli(&args);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "{\"channel\":\"lts\",\"findings\":[],\"ok\":true,\"tag\":\"v1.4.0\",\"version\":\"1.4.0\"}\n");
    assert_eq!(cli(&args), (code, out));
}

/// A binary that prints no JSON or exits non-zero is VERSION-BINARY-FAILED, named, in file
/// name order, after the workspace check.
#[test]
fn broken_binaries_fail() {
    let e = version_check(
        "v1.4.0",
        Channel::Stable,
        &fixture("bin-broken"),
        Some(&fixture("bin-broken").join("bin")),
    )
    .unwrap_err();
    let got: Vec<(&str, Option<&str>)> = e.iter().map(|f| (f.code(), f.subject())).collect();
    assert_eq!(
        got,
        vec![("VERSION-BINARY-FAILED", Some("fid")), ("VERSION-BINARY-FAILED", Some("fid-verify"))]
    );
    assert!(matches!(&e[0], VersionError::BinaryFailed { reason, .. } if reason.contains("not JSON")), "{e:?}");
}

/// The check only reads: a missing binaries directory is a finding, not a crash, and the
/// default workspace (this repository, no tags yet) is read without error.
#[test]
fn missing_dir_and_repo_workspace() {
    let e = version_check("v1.4.0", Channel::Stable, &fixture("bin-ok"), Some(&fixture("no-such-dir"))).unwrap_err();
    assert_eq!(e[0].code(), "VERSION-BINARY-FAILED");
    let repo = fid_xtask::rel::version::workspace_version(&root()).unwrap();
    assert!(parse_any(&repo).is_ok(), "{repo}");
}

#[test]
fn usage_errors_exit_2() {
    assert_eq!(cli(&["version-check", "--tag", "v1.4.0"]).0, 2);
    assert_eq!(cli(&["version-check", "--tag", "v1.4.0", "--channel", "rc"]).0, 2);
    assert_eq!(cli(&["version-compare", "1.4.0"]).0, 2);
}
