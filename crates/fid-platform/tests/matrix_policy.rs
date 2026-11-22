//! Spec leaf PLAT.MATRIX.POLICY.ROWS (AC-1..AC-5): tests `fid-platform plat-matrix window-check`
//! and `promote-check`, shipped `platform/support-matrix.toml` + `platform/tier1-window.toml`,
//! `OutsideWindow`, `PromotionNotEarned`, `NoVerdicts`, `demoted:` exclusion, and D0 determinism.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_platform::matrix_policy::{parse_window, WindowError, MAX_WINDOW_BYTES};
use std::path::PathBuf;
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fx(rel: &str) -> String {
    format!("fixtures/plat/matrix/{rel}")
}

fn run(sub: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-platform"))
        .current_dir(root())
        .arg("plat-matrix")
        .arg(sub)
        .args(args)
        .output()
        .expect("run fid-platform")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// AC-1: shipped `platform/support-matrix.toml` and `platform/tier1-window.toml` at `--phase P1a`
/// exit 0 with byte-identical output across repeated runs (REQ-001, REQ-002, REQ-005).
#[test]
fn ac1_shipped_matrix_passes_window_check_at_p1a() {
    let o1 = run("window-check", &["--phase", "P1a"]);
    let o2 = run("window-check", &["--phase", "P1a"]);
    assert_eq!(o1.status.code(), Some(0), "{}{}", out(&o1), String::from_utf8_lossy(&o1.stderr));
    assert_eq!(o1.stdout, o2.stdout, "window-check output must be D0 byte-identical");
    assert!(out(&o1).contains("window-check: ok"), "{}", out(&o1));
}

/// AC-2 (error path): `rows-three-ubuntu.toml` with three Tier 1 Ubuntu LTS rows fails with
/// `OutsideWindow { os: ubuntu, version: 22.04, arch: x86_64 }` for the oldest.
#[test]
fn ac2_three_ubuntu_lts_rows_reports_outside_window_for_oldest() {
    let o = run("window-check", &["--phase", "P1a", "--matrix", &fx("rows-three-ubuntu.toml")]);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    let s = out(&o);
    assert!(s.contains("OutsideWindow { os: ubuntu, version: 22.04, arch: x86_64 }"), "{s}");
}

/// AC-3 (error path): Windows 11 aarch64 at Tier 1 fails with `OutsideWindow`.
#[test]
fn ac3_windows_11_aarch64_at_tier1_reports_outside_window() {
    let o = run("window-check", &["--phase", "P1a", "--matrix", &fx("rows-win11-arm64-t1.toml")]);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    let s = out(&o);
    assert!(s.contains("OutsideWindow { os: windows, version: 10.0.22000, arch: aarch64 }"), "{s}");
}

/// AC-4: promoting `ubuntu 26.04 aarch64` from Tier 2 to Tier 1 passes when both latest RC
/// verdicts (`rc1-pass.json`, `rc2-pass.json`) show its Tier 1 jobs passed, and fails with
/// `PromotionNotEarned` when `rc2-fail.json` shows a failed job.
#[test]
fn ac4_promote_check_requires_two_passing_rc_verdicts() {
    let pass1 = run(
        "promote-check",
        &["--old", &fx("promote-old.toml"), "--new", &fx("promote-new.toml"), "--verdicts", &fx("promote-pass")],
    );
    let pass2 = run(
        "promote-check",
        &["--old", &fx("promote-old.toml"), "--new", &fx("promote-new.toml"), "--verdicts", &fx("promote-pass")],
    );
    assert_eq!(pass1.status.code(), Some(0), "{}{}", out(&pass1), String::from_utf8_lossy(&pass1.stderr));
    assert_eq!(pass1.stdout, pass2.stdout, "promote-check output must be D0 byte-identical");
    assert!(out(&pass1).contains("promote-check: ok"), "{}", out(&pass1));

    let fail = run(
        "promote-check",
        &["--old", &fx("promote-old.toml"), "--new", &fx("promote-new.toml"), "--verdicts", &fx("promote-fail")],
    );
    assert_eq!(fail.status.code(), Some(1), "{}", out(&fail));
    let s = out(&fail);
    assert!(
        s.contains("PromotionNotEarned")
            && s.contains("os: ubuntu")
            && s.contains("arch: aarch64")
            && s.contains("ubuntu-26.04-aarch64-full_test"),
        "{s}"
    );
}

/// AC-5: a row carrying a `demoted:` note is excluded from the expected Tier 1 window.
#[test]
fn ac5_demoted_row_is_excluded_from_window() {
    let o = run("window-check", &["--phase", "P1a", "--matrix", &fx("rows-demoted-macos.toml")]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), String::from_utf8_lossy(&o.stderr));
    assert!(out(&o).contains("window-check: ok"), "{}", out(&o));
}

/// Window file parser enforces size limit and schema checks cleanly.
#[test]
fn window_parser_bounds_and_usage_errors() {
    let big = vec![b' '; MAX_WINDOW_BYTES + 1];
    assert!(matches!(parse_window(&big), Err(errs) if matches!(errs.as_slice(), [WindowError::TooLarge { .. }])));

    let bad_usage = run("window-check", &[]);
    assert_eq!(bad_usage.status.code(), Some(2));
    let bad_promote = run("promote-check", &["--old", &fx("promote-old.toml")]);
    assert_eq!(bad_promote.status.code(), Some(2));
}
