//! Integration tests for spec node `AGENT.LOCAL.GUIDE` (AC-1..AC-4,
//! REQ-AGENT.LOCAL.GUIDE-001..006, CLI `fid-eval-board local guide`, error paths
//! `board_missing` and `catalog_mismatch`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_eval_board::{
    build_guide, write_guide, Fraction, GuideInputs, Outcome, WorkflowSuccess, GUIDE_HTML_FILE, GUIDE_MD_FILE,
};
use std::path::PathBuf;
use std::process::Command;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/agent/local")
}

fn fixture_inputs() -> (PathBuf, PathBuf, PathBuf) {
    let root = fixture_dir();
    (root.join("board"), root.join("catalog-index.json"), root.join("hosts"))
}

#[test]
fn ac1_generated_guide_matches_golden_markdown_and_html_and_excludes_closed_models() {
    let (board, catalog, hosts) = fixture_inputs();
    let inputs = GuideInputs { board: &board, catalog: &catalog, hosts: &hosts };
    let guide = build_guide(&inputs).expect("build_guide");

    assert_eq!(guide.release, "r2026.1");
    assert_eq!(guide.verdict, Outcome::Pass);
    assert_eq!(guide.models, vec!["open-llama-70b", "open-qwen-32b"]);

    let expected_md = std::fs::read_to_string(fixture_dir().join("expected").join(GUIDE_MD_FILE)).expect("gold md");
    let expected_html =
        std::fs::read_to_string(fixture_dir().join("expected").join(GUIDE_HTML_FILE)).expect("gold html");

    assert_eq!(guide.markdown, expected_md, "Markdown differs from golden");
    assert_eq!(guide.html, expected_html, "HTML differs from golden");
    assert!(!guide.markdown.contains("hosted-frontier-closed"));
    assert!(!guide.html.contains("hosted-frontier-closed"));
}

#[test]
fn ac2_below_80_percent_and_committed_violations_are_not_recommended() {
    let (board, catalog, hosts) = fixture_inputs();
    let guide = build_guide(&GuideInputs { board: &board, catalog: &catalog, hosts: &hosts }).expect("build");

    // AC-2: 3/4 = 0.75 on inspect.measure_bore for open-qwen-32b -> not recommended
    let bore_row = guide
        .rows
        .iter()
        .find(|r| r.model == "open-qwen-32b" && r.workflow == "inspect.measure_bore")
        .expect("bore row");
    assert_eq!(bore_row.success, WorkflowSuccess::Evaluated { rate: Fraction::new(3, 4).unwrap(), violations: 0 });
    assert!(!bore_row.recommended);
    assert_eq!(bore_row.status_label(), "not recommended for delegation");

    // REQ-002: 5/5 = 100% with 1 committed violation -> also not recommended
    let report_row = guide
        .rows
        .iter()
        .find(|r| r.model == "open-qwen-32b" && r.workflow == "inspect.export_report")
        .expect("report row");
    assert_eq!(report_row.success, WorkflowSuccess::Evaluated { rate: Fraction::new(5, 5).unwrap(), violations: 1 });
    assert!(!report_row.recommended);
    assert_eq!(report_row.status_label(), "not recommended for delegation");

    // Exact 4/5 = 80.0% with 0 violations on open-llama-70b / inspect.align_scan -> recommended
    let align_llama = guide
        .rows
        .iter()
        .find(|r| r.model == "open-llama-70b" && r.workflow == "inspect.align_scan")
        .expect("align llama row");
    assert_eq!(align_llama.success, WorkflowSuccess::Evaluated { rate: Fraction::new(4, 5).unwrap(), violations: 0 });
    assert!(align_llama.recommended);
    assert_eq!(align_llama.status_label(), "recommended");
}

#[test]
fn ac3_no_runs_is_not_evaluated_and_missing_host_is_not_reported() {
    let (board, catalog, hosts) = fixture_inputs();
    let guide = build_guide(&GuideInputs { board: &board, catalog: &catalog, hosts: &hosts }).expect("build");

    // AC-3: open-qwen-32b has no runs on inspect.check_flatness -> not evaluated, never recommended
    let flat_row = guide
        .rows
        .iter()
        .find(|r| r.model == "open-qwen-32b" && r.workflow == "inspect.check_flatness")
        .expect("flat row");
    assert_eq!(flat_row.success, WorkflowSuccess::NotEvaluated);
    assert!(!flat_row.recommended);
    assert_eq!(flat_row.status_label(), "not evaluated");

    // REQ-006: open-llama-70b has no host.json -> hardware is None ("not reported" in rendered output)
    let llama_row = guide.rows.iter().find(|r| r.model == "open-llama-70b").expect("llama row");
    assert_eq!(llama_row.hardware, None);
    assert!(guide.markdown.contains("- **Hardware:** not reported"));
    assert!(guide.html.contains("<li><strong>Hardware:</strong> not reported</li>"));
}

#[test]
fn ac4_repeated_runs_and_cli_are_byte_identical() {
    let (board, catalog, hosts) = fixture_inputs();
    let tmp = std::env::temp_dir().join(format!("fid-eval-local-guide-test-{}", std::process::id()));
    let out1 = tmp.join("run1");
    let out2 = tmp.join("run2");
    let _ = std::fs::remove_dir_all(&tmp);

    let inputs = GuideInputs { board: &board, catalog: &catalog, hosts: &hosts };
    let g1 = write_guide(&inputs, &out1).expect("run 1");

    // Run 2 through the compiled CLI binary `fid-eval-board local guide ...`
    let bin = env!("CARGO_BIN_EXE_fid-eval-board");
    let status = Command::new(bin)
        .args([
            "local",
            "guide",
            "--board",
            board.to_str().unwrap(),
            "--catalog",
            catalog.to_str().unwrap(),
            "--hosts",
            hosts.to_str().unwrap(),
            "--out",
            out2.to_str().unwrap(),
        ])
        .status()
        .expect("run fid-eval-board local guide");
    assert!(status.success());

    let md1 = std::fs::read(out1.join(GUIDE_MD_FILE)).expect("md1");
    let md2 = std::fs::read(out2.join(GUIDE_MD_FILE)).expect("md2");
    let html1 = std::fs::read(out1.join(GUIDE_HTML_FILE)).expect("html1");
    let html2 = std::fs::read(out2.join(GUIDE_HTML_FILE)).expect("html2");
    assert_eq!(md1, md2);
    assert_eq!(html1, html2);
    assert_eq!(md1, g1.markdown.as_bytes());
    assert_eq!(html1, g1.html.as_bytes());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn error_paths_board_missing_and_catalog_mismatch() {
    let (board, catalog, hosts) = fixture_inputs();

    // Missing board bundle -> board_missing
    let missing_board = fixture_dir().join("does-not-exist");
    let err = build_guide(&GuideInputs { board: &missing_board, catalog: &catalog, hosts: &hosts }).unwrap_err();
    assert_eq!(err.code(), "board_missing");

    // Missing catalog index -> catalog_mismatch
    let missing_cat = fixture_dir().join("no-catalog.json");
    let err = build_guide(&GuideInputs { board: &board, catalog: &missing_cat, hosts: &hosts }).unwrap_err();
    assert_eq!(err.code(), "catalog_mismatch");

    // Task referencing workflow not in catalog-index.json -> catalog_mismatch
    let tmp = std::env::temp_dir().join(format!("fid-eval-local-guide-err-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("mkdir");
    let bad_cat = tmp.join("small-catalog.json");
    std::fs::write(
        &bad_cat,
        r#"{"manifest_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000","rows":[{"catalog_version":"sha256:1111111111111111111111111111111111111111111111111111111111111111","workflow_id":"inspect.align_scan"}]}"#,
    )
    .expect("write bad_cat");
    let err = build_guide(&GuideInputs { board: &board, catalog: &bad_cat, hosts: &hosts }).unwrap_err();
    assert_eq!(err.code(), "catalog_mismatch");
    let _ = std::fs::remove_dir_all(&tmp);
}
