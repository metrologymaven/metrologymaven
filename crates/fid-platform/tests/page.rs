//! Acceptance criteria of spec node PLAT.MATRIX.PAGE (AC-1 .. AC-4) and the page module's
//! escaping and diff rules. Expected pages come from the independent Python reference
//! `fixtures/plat/matrix/make_page.py` (hand-checked once, then golden); the diff and escape
//! expectations below are hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_platform::page::{check, escape_cell, render, unified_diff};
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/plat/matrix").join(name)
}

fn repo(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(path)
}

/// A fresh, empty scratch directory for one test.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fid-platform-page-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Runs `fid-platform plat-matrix page <args>` (what `cargo xtask plat-matrix page` runs; the
/// xtask wrapper itself is exercised by the rust.yml step); returns (exit code, stdout, stderr).
fn cli(args: &[&std::ffi::OsStr]) -> (i32, String, String) {
    let out =
        Command::new(env!("CARGO_BIN_EXE_fid-platform")).args(["plat-matrix", "page"]).args(args).output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}

fn os(s: &str) -> &std::ffi::OsStr {
    s.as_ref()
}

fn p(path: &Path) -> &std::ffi::OsStr {
    path.as_os_str()
}

#[test]
fn ac1_valid_fixture_renders_expected_page_byte_for_byte() {
    let dir = scratch("ac1");
    let out = dir.join("page.md");
    let (code, stdout, stderr) = cli(&[os("--matrix"), p(&fixture("valid.toml")), os("--out"), p(&out)]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let expected = std::fs::read(fixture("expected-page.md")).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), expected);
    assert_eq!(render(&std::fs::read(fixture("valid.toml")).unwrap()).unwrap().as_bytes(), &expected[..]);
}

#[test]
fn escaping_family_order_and_row_order_match_reference() {
    let page = render(&std::fs::read(fixture("escape.toml")).unwrap()).unwrap();
    assert_eq!(page.as_bytes(), &std::fs::read(fixture("expected-escape-page.md")).unwrap()[..]);
    assert!(!page.contains('\r'), "REQ-004: LF only");
    assert!(page.contains("| 22.04 to before 22.10 |"), "versions are shown as written");
}

#[test]
fn header_names_source_and_sha256_of_raw_bytes() {
    let page = render(&std::fs::read(fixture("valid.toml")).unwrap()).unwrap();
    let first = page.lines().next().unwrap();
    // sha256 of fixtures/plat/matrix/valid.toml, from the Python reference (hashlib).
    assert_eq!(
        first,
        "<!-- generated from platform/support-matrix.toml \
         sha256:04d3142d796bb6f78af7703bf327a9f217b9173284a59b91d2e1322e8e3e3e71; do not edit -->"
    );
}

#[test]
fn committed_project_page_is_in_sync_with_the_matrix() {
    let page = render(&std::fs::read(repo("platform/support-matrix.toml")).unwrap()).unwrap();
    let committed = std::fs::read(repo("docs/platform/support-matrix.md")).unwrap();
    if let Some(diff) = check(&committed, &page, "docs/platform/support-matrix.md") {
        panic!("regenerate with `cargo xtask plat-matrix page --out docs/platform/support-matrix.md`:\n{diff}");
    }
}

#[test]
fn ac2_hand_edited_tier_cell_fails_check_and_names_the_line() {
    let dir = scratch("ac2");
    let page = dir.join("page.md");
    let good = std::fs::read_to_string(fixture("expected-page.md")).unwrap();
    let line = "| 15 to before 16 | aarch64 | Tier 1 | P1a | fixture |\n";
    let edited_line = "| 15 to before 16 | aarch64 | Tier 2 | P1a | fixture |\n";
    assert_eq!(good.matches(line).count(), 1);
    let edited = good.replace(line, edited_line);
    std::fs::write(&page, &edited).unwrap();

    let (code, stdout, stderr) =
        cli(&[os("--check"), os("--matrix"), p(&fixture("valid.toml")), os("--out"), p(&page)]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    let name = page.to_str().unwrap();
    let expected = format!(
        "--- {name}\n+++ {name} (generated)\n@@ -15,7 +15,7 @@\n \
         | Versions | Arch | Tier | Since | Notes |\n \
         |---|---|---|---|---|\n \
         | 15 to before 16 | x86\\_64 | Tier 3 | later | fixture |\n\
         -| 15 to before 16 | aarch64 | Tier 2 | P1a | fixture |\n\
         +| 15 to before 16 | aarch64 | Tier 1 | P1a | fixture |\n \
         \n \
         ## Ubuntu\n \
         \n"
    );
    assert_eq!(stdout, expected);
    assert_eq!(std::fs::read_to_string(&page).unwrap(), edited, "check mode never writes");
}

#[test]
fn ac3_unedited_page_passes_check() {
    let dir = scratch("ac3");
    let page = dir.join("page.md");
    let good = std::fs::read(fixture("expected-page.md")).unwrap();
    std::fs::write(&page, &good).unwrap();
    let (code, stdout, stderr) =
        cli(&[os("--check"), os("--matrix"), p(&fixture("valid.toml")), os("--out"), p(&page)]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(std::fs::read(&page).unwrap(), good);
}

#[test]
fn ac4_overlapping_matrix_exits_1_and_writes_nothing() {
    let dir = scratch("ac4");
    let out = dir.join("page.md");
    let (code, stdout, stderr) = cli(&[os("--matrix"), p(&fixture("overlap.toml")), os("--out"), p(&out)]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("OverlappingRange: rows 2 and 3: ubuntu x86_64 version ranges overlap"), "{stderr}");
    assert!(!out.exists(), "nothing is written on a validation error");

    std::fs::write(&out, "keep\n").unwrap();
    let (code, _, _) = cli(&[os("--check"), os("--matrix"), p(&fixture("overlap.toml")), os("--out"), p(&out)]);
    assert_eq!(code, 1);
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "keep\n");
}

#[test]
fn check_on_a_missing_page_is_an_io_error_and_creates_nothing() {
    let dir = scratch("missing");
    let out = dir.join("absent.md");
    let (code, _, stderr) = cli(&[os("--check"), os("--matrix"), p(&fixture("valid.toml")), os("--out"), p(&out)]);
    assert_eq!(code, 2, "{stderr}");
    assert!(!out.exists());
}

#[test]
fn bad_usage_exits_2() {
    let m = fixture("valid.toml");
    assert_eq!(cli(&[os("--matrix"), p(&m)]).0, 2, "--out is required");
    assert_eq!(cli(&[os("--check"), os("--check"), os("--out"), os("x.md")]).0, 2);
    assert_eq!(cli(&[os("--out")]).0, 2);
    assert_eq!(cli(&[os("--bogus"), os("--out"), os("x.md")]).0, 2);
    assert!(!Path::new("x.md").exists());
}

#[test]
fn escape_cell_hand_labelled() {
    assert_eq!(escape_cell("plain text, 1.2-3 (ok)"), "plain text, 1.2-3 (ok)");
    assert_eq!(escape_cell("a|b"), "a\\|b");
    assert_eq!(escape_cell("<b>&</b>"), "&lt;b&gt;&amp;&lt;/b&gt;");
    assert_eq!(escape_cell("*x* _y_ `z` ~w~ $v$ [l](u) \\"), "\\*x\\* \\_y\\_ \\`z\\` \\~w\\~ \\$v\\$ \\[l\\](u) \\\\");
    assert_eq!(escape_cell("a\nb\r\nc\td\u{7f}e"), "a b  c d e");
    assert_eq!(escape_cell("café — ok"), "café — ok");
    assert_eq!(escape_cell(""), "");
}

#[test]
fn unified_diff_hand_labelled() {
    assert_eq!(unified_diff("a\nb\n", "a\nb\n", "o", "n"), "");
    assert_eq!(unified_diff("", "a\n", "o", "n"), "--- o\n+++ n\n@@ -0,0 +1,1 @@\n+a\n");
    assert_eq!(unified_diff("a\n", "", "o", "n"), "--- o\n+++ n\n@@ -1,1 +0,0 @@\n-a\n");
    // Missing final newline in the committed page.
    assert_eq!(
        unified_diff("x\ny", "x\ny\n", "o", "n"),
        "--- o\n+++ n\n@@ -1,2 +1,2 @@\n x\n-y\n\\ No newline at end of file\n+y\n"
    );
    // Context is capped at three lines on each side; unchanged lines inside the region stay.
    let old = "1\n2\n3\n4\n5\nX\n7\nY\n9\n10\n11\n12\n";
    let new = "1\n2\n3\n4\n5\nx\n7\ny\n9\n10\n11\n12\n";
    assert_eq!(
        unified_diff(old, new, "o", "n"),
        "--- o\n+++ n\n@@ -3,9 +3,9 @@\n 3\n 4\n 5\n-X\n+x\n 7\n-Y\n+y\n 9\n 10\n 11\n"
    );
    // A CRLF page differs from the LF page on every line.
    let d = unified_diff("a\r\nb\r\n", "a\nb\n", "o", "n");
    assert_eq!(d, "--- o\n+++ n\n@@ -1,2 +1,2 @@\n-a\r\n-b\r\n+a\n+b\n");
}

#[test]
fn check_returns_none_only_for_identical_bytes() {
    assert_eq!(check(b"a\n", "a\n", "p"), None);
    assert!(check(b"a\r\n", "a\n", "p").is_some());
    assert!(check(b"", "a\n", "p").is_some());
}

#[test]
fn render_is_deterministic() {
    let bytes = std::fs::read(fixture("escape.toml")).unwrap();
    assert_eq!(render(&bytes).unwrap(), render(&bytes).unwrap());
}
