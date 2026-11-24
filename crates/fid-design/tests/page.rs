//! Spec leaf UX.REPORT.PAGE: `fid-design page-check` on the shipped `design/report/pages.json`
//! (AC-1), on `fixtures/ux/report/page/{narrow-margin,bad-width}.json` (AC-2, AC-3), plus
//! missing-grid, schema, tolerance, D0 determinism and exit-2 paths.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_design::page::{check, check_file, check_text, GRID_SUM, MARGIN, MISSING, SCHEMA};
use std::path::PathBuf;
use std::process::{Command, Output};

const FX: &str = "fixtures/ux/report/page";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).current_dir(root()).args(args).output().expect("run fid-design")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// AC-1: shipped `design/report/pages.json` passes `page-check --root design --json`, lists
/// four grids (A4 210 x 297 mm, Letter 215.9 x 279.4 mm), and is D0 byte-identical.
#[test]
fn ac1_shipped_pages_pass_and_are_d0() {
    let args = ["page-check", "--root", "design", "--json"];
    let o1 = run(&args);
    let o2 = run(&args);
    assert_eq!(o1.status.code(), Some(0), "{}{}", out(&o1), String::from_utf8_lossy(&o1.stderr));
    assert_eq!(o1.stdout, o2.stdout, "page-check --json must be byte-identical (D0)");
    let s = out(&o1);
    assert!(s.contains("\"ok\":true"), "{s}");
    assert!(s.contains("\"id\":\"a4-portrait\""), "{s}");
    assert!(s.contains("\"id\":\"a4-landscape\""), "{s}");
    assert!(s.contains("\"id\":\"letter-portrait\""), "{s}");
    assert!(s.contains("\"id\":\"letter-landscape\""), "{s}");
    assert!(s.contains("\"width_mm\":\"210.0\""), "{s}");
    assert!(s.contains("\"height_mm\":\"297.0\""), "{s}");
    assert!(s.contains("\"width_mm\":\"215.9\""), "{s}");
    assert!(s.contains("\"height_mm\":\"279.4\""), "{s}");

    let r = check(&root().join("design")).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
    assert_eq!(r.grids.len(), 4);
}

/// AC-2: `narrow-margin.json` (8 mm) exits 1 with `PAGE-MARGIN a4-portrait`.
#[test]
fn ac2_narrow_margin_fails() {
    let f = format!("{FX}/narrow-margin.json");
    let o = run(&["page-check", "--file", &f]);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert!(out(&o).contains("PAGE-MARGIN a4-portrait"), "{}", out(&o));

    let j = run(&["page-check", "--file", &f, "--json"]);
    assert_eq!(j.status.code(), Some(1));
    assert!(out(&j).contains("\"code\":\"PAGE-MARGIN\""), "{}", out(&j));

    let r = check_file(&root().join(&f)).unwrap();
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].code, MARGIN);
    assert_eq!(r.findings[0].subject, "a4-portrait");
}

/// AC-3: `bad-width.json` (columns + gutters != content width) exits 1 with `PAGE-GRID-SUM`.
#[test]
fn ac3_bad_width_fails() {
    let f = format!("{FX}/bad-width.json");
    let o = run(&["page-check", "--file", &f]);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert!(out(&o).contains("PAGE-GRID-SUM a4-portrait"), "{}", out(&o));

    let j = run(&["page-check", "--file", &f, "--json"]);
    assert_eq!(j.status.code(), Some(1));
    assert!(out(&j).contains("\"code\":\"PAGE-GRID-SUM\""), "{}", out(&j));

    let r = check_file(&root().join(&f)).unwrap();
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].code, GRID_SUM);
    assert_eq!(r.findings[0].subject, "a4-portrait");
}

/// Missing grids report `PAGE-MISSING`; malformed JSON or non-12 columns report `PAGE-SCHEMA`.
#[test]
fn missing_grid_and_schema_errors() {
    let missing = check_text(r#"{"format":"page-grids@1","grids":[]}"#);
    assert_eq!(missing.findings.len(), 4);
    assert!(missing.findings.iter().all(|f| f.code == MISSING));

    let bad_json = check_text("{not json");
    assert_eq!(bad_json.findings.len(), 1);
    assert_eq!(bad_json.findings[0].code, SCHEMA);

    let bad_cols = check_text(
        r#"{
  "format": "page-grids@1",
  "grids": [
    {
      "id": "a4-portrait",
      "page": {"width_mm": "210.0", "height_mm": "297.0"},
      "margins": {"top_mm": "15.0", "right_mm": "15.0", "bottom_mm": "15.0", "left_mm": "18.0"},
      "columns": 10,
      "column_width_mm": "15.0",
      "gutter_mm": "3.0",
      "content_width_mm": "177.0",
      "header_band_mm": "12.0",
      "footer_band_mm": "10.0",
      "content_height_mm": "245.0"
    }
  ]
}"#,
    );
    assert!(
        bad_cols.findings.iter().any(|f| f.code == SCHEMA && f.subject == "a4-portrait"),
        "{:?}",
        bad_cols.findings
    );
    assert!(bad_cols.findings.iter().any(|f| f.code == MISSING && f.subject == "letter-portrait"));
}

/// Exit 2 on bad CLI usage or missing paths.
#[test]
fn exit_two_on_bad_usage_and_missing_inputs() {
    for bad in [
        &["page-check"][..],
        &["page-check", "--root", "design", "--file", &format!("{FX}/narrow-margin.json")][..],
        &["page-check", "--root", "fixtures/ux/report/page/nonexistent"][..],
        &["page-check", "--file", "fixtures/ux/report/page/nonexistent.json"][..],
    ] {
        let o = run(bad);
        assert_eq!(o.status.code(), Some(2), "expected exit 2 for {bad:?}, got {:?}", o.status.code());
    }
}
