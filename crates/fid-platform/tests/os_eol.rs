//! Spec leaf PLAT.MATRIX.POLICY.EOL (AC-1 .. AC-5): tests `fid-platform plat-matrix eol-check`,
//! shipped `platform/os-eol.toml` against `platform/support-matrix.toml`, warning window,
//! `PastEndOfSupport` and `demoted: <date> eol`, Windows 10 `extended_until`, `MissingEolEntry`,
//! `MissingSource`, 64 KiB cap, D0 determinism and exit-2 usage errors.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_platform::os_eol::{parse, EolError, MAX_BYTES};
use std::path::PathBuf;
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fx(name: &str) -> String {
    format!("fixtures/plat/matrix/{name}")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-platform"))
        .current_dir(root())
        .arg("plat-matrix")
        .arg("eol-check")
        .args(args)
        .output()
        .expect("run fid-platform")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// AC-1: `eol-valid.toml` + `valid.toml` at `--today 2026-01-01` exits 0 with no warnings.
/// Also verifies AC-4 (Windows 10 `eol` 2025-10-14 passed, `extended_until` 2028-10-10 in future).
#[test]
fn ac1_and_ac4_valid_and_windows10_extended_until() {
    let args = ["--today", "2026-01-01", "--matrix", &fx("valid.toml"), "--eol", &fx("eol-valid.toml")];
    let o1 = run(&args);
    let o2 = run(&args);
    assert_eq!(o1.status.code(), Some(0), "{}{}", out(&o1), String::from_utf8_lossy(&o1.stderr));
    assert_eq!(o1.stdout, o2.stdout, "eol-check must be D0 deterministic");
    assert!(out(&o1).contains("0 warning(s), 0 error(s)"), "{}", out(&o1));
}

/// AC-2: entry ending `2026-03-01` with `--today 2026-01-15` emits a warning naming the row
/// and exits 0.
#[test]
fn ac2_warning_within_90_days_exits_zero() {
    let o = run(&["--today", "2026-01-15", "--matrix", &fx("valid.toml"), "--eol", &fx("eol-ending-2026-03-01.toml")]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), String::from_utf8_lossy(&o.stderr));
    let s = out(&o);
    assert!(s.contains("warning:") && s.contains("macos") && s.contains("2026-03-01"), "{s}");
    assert!(s.contains("1 warning(s), 0 error(s)"), "{s}");
}

/// AC-3: `--today 2026-03-02` with that row still Tier 1 exits 1 with `PastEndOfSupport`;
/// with a `demoted: 2026-03-02 eol` note it exits 0.
#[test]
fn ac3_past_eol_fails_unless_demoted_note_present() {
    let fail =
        run(&["--today", "2026-03-02", "--matrix", &fx("valid.toml"), "--eol", &fx("eol-ending-2026-03-01.toml")]);
    assert_eq!(fail.status.code(), Some(1), "{}", out(&fail));
    assert!(out(&fail).contains("PastEndOfSupport") && out(&fail).contains("macos"), "{}", out(&fail));

    let demoted = run(&[
        "--today",
        "2026-03-02",
        "--matrix",
        &fx("eol-demoted-matrix.toml"),
        "--eol",
        &fx("eol-ending-2026-03-01.toml"),
    ]);
    assert_eq!(demoted.status.code(), Some(0), "{}{}", out(&demoted), String::from_utf8_lossy(&demoted.stderr));
    assert!(out(&demoted).contains("0 error(s)"), "{}", out(&demoted));
}

/// AC-5: Tier 2 version without an EOL entry fails with `MissingEolEntry`.
#[test]
fn ac5_missing_tier2_entry_fails() {
    let o =
        run(&["--today", "2026-01-01", "--matrix", &fx("eol-missing-t2-matrix.toml"), "--eol", &fx("eol-valid.toml")]);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert!(
        out(&o).contains("MissingEolEntry") && out(&o).contains("ubuntu") && out(&o).contains("tier2"),
        "{}",
        out(&o)
    );
}

/// Shipped `platform/os-eol.toml` passes against shipped `platform/support-matrix.toml` and
/// marks its vendor dates unverified in a comment (REQ-005).
#[test]
fn shipped_os_eol_passes_default_paths() {
    let o = run(&["--today", "2026-11-22"]);
    assert_eq!(o.status.code(), Some(0), "{}{}", out(&o), String::from_utf8_lossy(&o.stderr));
    assert!(out(&o).contains("0 warning(s), 0 error(s)"), "{}", out(&o));

    let raw = std::fs::read_to_string(root().join("platform/os-eol.toml")).unwrap();
    assert!(raw.contains("[unverified]"), "shipped os-eol.toml must mark dates [unverified] in comment");
    assert!(raw.contains("extended_until"), "shipped os-eol.toml must carry Windows 10 extended_until per D-010");
}

/// Missing source URL reports `MissingSource`, and files over 64 KiB report `TooLarge`.
#[test]
fn missing_source_and_size_cap() {
    let no_src = br#"
schema_version = 1
[[entry]]
os = "ubuntu"
version = "24.04"
eol = "2029-05-31"
"#;
    let errs = parse(no_src).unwrap_err();
    assert!(errs.iter().any(|e| matches!(e, EolError::MissingSource { .. })), "{errs:?}");

    let big = vec![b' '; MAX_BYTES + 1];
    let big_errs = parse(&big).unwrap_err();
    assert!(matches!(big_errs.as_slice(), [EolError::TooLarge { .. }]), "{big_errs:?}");
}

/// Exit 2 on missing `--today`, invalid date, or missing files.
#[test]
fn exit_two_on_bad_usage_and_unreadable_files() {
    for bad in [
        &[][..],
        &["--today", "2026-02-29"][..],
        &["--today", "not-a-date"][..],
        &["--today", "2026-01-01", "--eol", "fixtures/plat/matrix/nonexistent.toml"][..],
        &["--today", "2026-01-01", "--matrix", "fixtures/plat/matrix/nonexistent.toml"][..],
    ] {
        let o = run(bad);
        assert_eq!(o.status.code(), Some(2), "expected exit 2 for {bad:?}, got {:?}", o.status.code());
    }
}
