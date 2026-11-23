//! End-to-end integration test for the Day-1 2D PDF Auto-Ballooning wedge wired into `fid-slice`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use fid_slice::balloon::run_balloon_demo;

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
    assert_eq!(out1.pack.balloon_count, 15);
    assert_eq!(out1.branch_digest, out2.branch_digest);
    assert_eq!(out1.pack.pdf_digest, out2.pack.pdf_digest);
    assert_eq!(out1.pack.csv_digest, out2.pack.csv_digest);

    let pdf1 = std::fs::read(d1.0.join("ballooned.pdf")).expect("ballooned.pdf 1");
    let pdf2 = std::fs::read(d2.0.join("ballooned.pdf")).expect("ballooned.pdf 2");
    assert_eq!(pdf1, pdf2);

    let csv1 = std::fs::read(d1.0.join("form3-starter.csv")).expect("form3-starter.csv 1");
    let csv2 = std::fs::read(d2.0.join("form3-starter.csv")).expect("form3-starter.csv 2");
    assert_eq!(csv1, csv2);
}
