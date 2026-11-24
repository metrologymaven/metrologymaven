//! Spec leaf OUTR.BRAND.GUIDE.DRESS (AC-1 .. AC-4): validates `brand/guidelines/trade-dress.toml`
//! against `crates/fid-brand-lint/rules/incumbents.txt`, verifies rendered `trade-dress.md`
//! matches `fid-outreach dress-render` and passes `guide-check`, and tests error fixtures
//! `fixtures/outr/brand/dress/{missing,long,image}.toml` plus `[unverified]` summary exclusion.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_outreach::dress::{check_files, render_markdown, ErrorCode};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

fn repo(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

fn fx(name: &str) -> PathBuf {
    repo("fixtures/outr/brand/dress").join(name)
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-outreach"))
        .current_dir(repo("."))
        .args(args)
        .output()
        .expect("run fid-outreach")
}

fn tmp_file(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let id = N.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("fid-outreach-dress-{}-{tag}-{id}.md", std::process::id()));
    let _ = fs::remove_file(&p);
    p
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// AC-1: every incumbent in `crates/fid-brand-lint/rules/incumbents.txt` has a valid entry in
/// `brand/guidelines/trade-dress.toml` with `colours`, `icon_style`, `layout` and `instead`,
/// and the rendered `brand/guidelines/trade-dress.md` matches `dress-render` byte-for-byte.
#[test]
fn ac1_shipped_trade_dress_list_is_complete_and_rendered_page_matches() {
    let list = repo("brand/guidelines/trade-dress.toml");
    let inc = repo("crates/fid-brand-lint/rules/incumbents.txt");
    let res = check_files(&list, Some(&inc)).unwrap();
    assert!(res.findings.is_empty(), "shipped trade-dress findings: {:?}", res.findings);
    assert_eq!(res.entries.len(), 5);
    for e in &res.entries {
        assert!(!e.colours.is_empty());
        assert!(!e.icon_style.is_empty());
        assert!(!e.layout.is_empty());
        assert!(!e.instead.is_empty());
    }

    let rendered = render_markdown(&res.entries);
    let shipped_md = fs::read_to_string(repo("brand/guidelines/trade-dress.md")).unwrap();
    assert_eq!(shipped_md, rendered, "brand/guidelines/trade-dress.md is out of sync with dress-render");
    assert!(shipped_md.contains("{{FAMILY_NAME}}"));

    // Shipped guidelines directory (including trade-dress.md) still passes guide-check.
    let gc = run(&["guide-check", "--root", "brand/guidelines", "--name", "brand/name.toml", "--json"]);
    assert_eq!(gc.status.code(), Some(0), "{}", String::from_utf8_lossy(&gc.stdout));
}

/// AC-2: `missing.toml` fails naming the missing incumbent (`Cinder Probe`).
#[test]
fn ac2_missing_incumbent_fails_naming_it() {
    let res = check_files(&fx("missing.toml"), Some(&fx("incumbents.txt"))).unwrap();
    assert_eq!(res.findings.len(), 1, "{:?}", res.findings);
    assert_eq!(res.findings[0].code, ErrorCode::EntryMissing);
    assert_eq!(res.findings[0].entry, "Cinder Probe");

    let out = tmp_file("missing");
    let o = run(&[
        "dress-render",
        "--list",
        &s(&fx("missing.toml")),
        "--incumbents",
        &s(&fx("incumbents.txt")),
        "--out",
        &s(&out),
    ]);
    assert_eq!(o.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(stdout.contains("entry_missing") && stdout.contains("Cinder Probe"), "{stdout}");
    assert!(!out.exists(), "failing dress-render must not write --out");
}

/// AC-3: `long.toml` (400-char `layout`) fails with `field_too_long` naming entry & field;
/// `image.toml` (image link in `colours`) fails with `media_reference` naming entry & field.
#[test]
fn ac3_long_field_and_image_link_fail() {
    let long_res = check_files(&fx("long.toml"), Some(&fx("incumbents.txt"))).unwrap();
    assert_eq!(long_res.findings.len(), 1, "{:?}", long_res.findings);
    assert_eq!(long_res.findings[0].code, ErrorCode::FieldTooLong);
    assert_eq!(long_res.findings[0].entry, "Acorn Metro");
    assert_eq!(long_res.findings[0].field.as_deref(), Some("layout"));

    let img_res = check_files(&fx("image.toml"), Some(&fx("incumbents.txt"))).unwrap();
    assert_eq!(img_res.findings.len(), 1, "{:?}", img_res.findings);
    assert_eq!(img_res.findings[0].code, ErrorCode::MediaReference);
    assert_eq!(img_res.findings[0].entry, "Beryl Scan");
    assert_eq!(img_res.findings[0].field.as_deref(), Some("colours"));
}

/// AC-4: `dress-render` marks entries without `reviewed_by`/`reviewed_on` as `[unverified]`
/// and excludes them from `## Reviewer summary`, while keeping reviewed entries in the summary.
/// Running twice produces byte-identical output (D0).
#[test]
fn ac4_unreviewed_entries_marked_unverified_and_excluded_from_summary() {
    let out1 = tmp_file("ok1");
    let out2 = tmp_file("ok2");
    let o1 = run(&["dress-render", "--list", &s(&fx("ok.toml")), "--out", &s(&out1)]);
    let o2 = run(&["dress-render", "--list", &s(&fx("ok.toml")), "--out", &s(&out2)]);
    assert_eq!(o1.status.code(), Some(0), "{}", String::from_utf8_lossy(&o1.stderr));
    assert_eq!(o2.status.code(), Some(0));

    let md1 = fs::read_to_string(&out1).unwrap();
    let md2 = fs::read_to_string(&out2).unwrap();
    assert_eq!(md1, md2, "dress-render output must be byte-identical (D0)");

    let (summary, entries) = md1.split_once("## Entries").expect("rendered page must have ## Entries");
    assert!(summary.contains("Acorn Metro"), "reviewed entry must appear in summary:\n{summary}");
    assert!(!summary.contains("Beryl Scan"), "unreviewed entry must be absent from summary:\n{summary}");
    assert!(!summary.contains("Cinder Probe"), "unreviewed entry must be absent from summary:\n{summary}");

    assert!(entries.contains("### Acorn Metro\n"), "reviewed entry must not be marked unverified:\n{entries}");
    assert!(entries.contains("### Beryl Scan [unverified]\n"), "{entries}");
    assert!(entries.contains("### Cinder Probe [unverified]\n"), "{entries}");
    assert!(md1.contains("{{FAMILY_NAME}}"));

    let _ = fs::remove_file(&out1);
    let _ = fs::remove_file(&out2);
}

/// Exit 2 on bad CLI usage or unreadable input.
#[test]
fn exit_two_on_bad_usage_and_missing_input() {
    let out = tmp_file("bad");
    for bad in [
        &["dress-render"][..],
        &["dress-render", "--list", &s(&fx("ok.toml"))][..],
        &["dress-render", "--list", "fixtures/outr/brand/dress/nonexistent.toml", "--out", &s(&out)][..],
    ] {
        let o = run(bad);
        assert_eq!(o.status.code(), Some(2), "expected exit 2 for {bad:?}");
    }
}
