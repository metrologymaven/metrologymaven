//! End-to-end integration test for the Day-1 2D PDF Auto-Ballooning wedge wired into `fid-slice`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fid_report::balloon::auto_propose::DIAG_RASTER_NEEDS_OCR;
use fid_report::balloon::char::{AuthoredUnit, Dialect};
use fid_slice::balloon::{run_balloon_demo, run_balloon_file};

struct TmpDir(PathBuf);

impl TmpDir {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let dir = std::env::temp_dir().join(format!("fid_slice_balloon_{tag}_{}_{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TmpDir(dir)
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn balloon_demo_produces_deterministic_ballooned_pdf_and_form3_csv() {
    let d1 = TmpDir::new("run1");
    let d2 = TmpDir::new("run2");

    let out1 = run_balloon_demo(&d1.0).expect("run_balloon_demo 1");
    let out2 = run_balloon_demo(&d2.0).expect("run_balloon_demo 2");

    assert_eq!(out1.sheet_count, 1);
    assert_eq!(out1.rows.len(), 15);
    assert!(out1.diagnostics.is_empty());
    let pack1 = out1.pack.as_ref().expect("pack 1");
    let pack2 = out2.pack.as_ref().expect("pack 2");
    assert_eq!(pack1.balloon_count, 15);
    assert_eq!(out1.branch_digest, out2.branch_digest);
    assert_eq!(pack1.pdf_digest, pack2.pdf_digest);
    assert_eq!(pack1.csv_digest, pack2.csv_digest);

    let pdf1 = std::fs::read(d1.0.join("ballooned.pdf")).expect("ballooned.pdf 1");
    let pdf2 = std::fs::read(d2.0.join("ballooned.pdf")).expect("ballooned.pdf 2");
    assert_eq!(pdf1, pdf2);

    let csv1 = std::fs::read(d1.0.join("form3-starter.csv")).expect("form3-starter.csv 1");
    let csv2 = std::fs::read(d2.0.join("form3-starter.csv")).expect("form3-starter.csv 2");
    assert_eq!(csv1, csv2);
}

#[test]
fn balloon_scanned_only_returns_zero_balloons_and_raster_diagnostic() {
    let d = TmpDir::new("scanned_only");
    let pdf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/balloon/auto/scanned_only.pdf");

    let out = run_balloon_file(&pdf, &d.0, Dialect::Asme, AuthoredUnit::Mm).expect("scanned_only must succeed");

    assert_eq!(out.sheet_count, 1);
    assert!(out.rows.is_empty());
    assert!(out.pack.is_none());
    assert!(
        out.diagnostics.iter().any(|diag| diag.code == DIAG_RASTER_NEEDS_OCR),
        "expected {DIAG_RASTER_NEEDS_OCR}, got {:?}",
        out.diagnostics
    );
    assert!(!d.0.join("ballooned.pdf").exists());
    assert!(!d.0.join("form3-starter.csv").exists());
}

#[test]
fn balloon_rejects_parent_dir_escape_without_creating_directory() {
    let d = TmpDir::new("escape");
    let safe = d.0.join("safe");
    std::fs::create_dir_all(&safe).expect("create safe dir");
    let escaped = safe.join("../outside");
    let outside_target = d.0.join("outside");
    let pdf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/balloon/auto/vector_bracket.pdf");

    let err = run_balloon_file(&pdf, &escaped, Dialect::Asme, AuthoredUnit::Mm)
        .expect_err(".. path escape must fail with io.root_violation");
    assert_eq!(err.code, "io.root_violation");
    assert!(!outside_target.exists(), "must not create escaped directory {}", outside_target.display());

    let rel_err = run_balloon_file(&pdf, Path::new("../outside"), Dialect::Asme, AuthoredUnit::Mm)
        .expect_err("relative ../outside must fail with io.root_violation");
    assert_eq!(rel_err.code, "io.root_violation");

    let demo_escaped = safe.join("../outside_demo");
    let demo_err = run_balloon_demo(&demo_escaped).expect_err("demo .. escape must fail");
    assert_eq!(demo_err.code, "io.root_violation");
    assert!(!d.0.join("outside_demo").exists());
}

#[test]
fn tier5_balloon_demo_does_not_clobber_existing_pdf_in_out_dir() {
    let d = TmpDir::new("no_clobber");
    let sentinel = d.0.join("vector_bracket.pdf");
    std::fs::write(&sentinel, b"%SENTINEL-DO-NOT-CLOBBER").expect("write sentinel");

    let out = run_balloon_demo(&d.0).expect("run_balloon_demo");
    assert_eq!(out.rows.len(), 15);
    assert_eq!(
        std::fs::read(&sentinel).expect("read sentinel"),
        b"%SENTINEL-DO-NOT-CLOBBER",
        "run_balloon_demo must not clobber pre-existing vector_bracket.pdf in out_dir"
    );
}
