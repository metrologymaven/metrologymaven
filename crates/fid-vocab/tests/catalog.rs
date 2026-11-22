//! Integration tests for `ECO.RULES.VOCAB.CATALOG` (product string catalogue adapter).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_vocab::{format_units_jsonl, load_catalog_units, parse_units, replace_placeholders, CatalogError};
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn fixture(name: &str) -> PathBuf {
    repo_root().join("fixtures/vocab/catalog").join(name)
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fid-vocab"))
}

/// AC-1: given `messages.en.json` (4 messages, one with plural variants `one` and `other`),
/// `fid-vocab catalog-units --export ... --suppressions none.toml` emits 5 sorted units with
/// expected IDs, source file/line, and `{arg}` placeholder replacement matching the Python oracle.
#[test]
fn ac1_catalog_units_emits_five_expected_units() {
    let out = bin()
        .args([
            "catalog-units",
            "--export",
            fixture("messages.en.json").to_str().unwrap(),
            "--suppressions",
            fixture("none.toml").to_str().unwrap(),
        ])
        .output()
        .expect("run fid-vocab catalog-units");

    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let expected = std::fs::read(fixture("expected-units.jsonl")).expect("read expected jsonl");
    assert_eq!(out.stdout, expected);

    // Every emitted JSONL line must parse cleanly back through fid_vocab::parse_units.
    let parsed = parse_units(&out.stdout).expect("parse emitted units");
    assert_eq!(parsed.len(), 5);
    let ids: Vec<&str> = parsed.iter().map(|u| u.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "cli.align.done",
            "dialog.confirm.title",
            "report.footer.note",
            "ui.part.summary#one",
            "ui.part.summary#other",
        ]
    );
    // REQ-002: placeholder `{certified_count}` was replaced by `{arg}`, so no claim word leaks.
    assert_eq!(parsed[3].text, "Loaded {arg} feature from {arg}.");
    assert_eq!(parsed[4].text, "Loaded {arg} features from {arg}.");
    // Escaped braces are preserved.
    assert_eq!(parsed[0].text, "Alignment completed for {arg} with {{literal}} braces.");
    // REQ-005: source file and line preserved.
    assert_eq!(parsed[2].file, "report/footer.ftl");
    assert_eq!(parsed[2].line, 4);
    assert_eq!(parsed[3].file, "ui/part.ftl");
    assert_eq!(parsed[3].line, 12);
}

/// AC-2: `fid-vocab catalog-lint` on `messages.en.json` with product rules exits 1 with a VC001
/// finding on unit `report.footer.note` (pointing at `report/footer.ftl:4`), and does NOT flag
/// `{certified_count}` because placeholders were replaced with `{arg}`.
#[test]
fn ac2_catalog_lint_reports_vc001_on_report_footer_note() {
    let out = bin()
        .args([
            "catalog-lint",
            "--export",
            fixture("messages.en.json").to_str().unwrap(),
            "--rules",
            fixture("product-rules-fixture.toml").to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("run fid-vocab catalog-lint");

    assert_eq!(out.status.code(), Some(1), "expected exit 1; stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).expect("utf8 json");
    assert!(stdout.contains("\"unit\":\"report.footer.note\""), "stdout: {stdout}");
    assert!(stdout.contains("\"rule\":\"VC001\""), "stdout: {stdout}");
    assert!(stdout.contains("\"file\":\"report/footer.ftl\""), "stdout: {stdout}");
    assert!(stdout.contains("\"line\":4"), "stdout: {stdout}");
    // Placeholder `{certified_count}` must NOT have caused a finding on `ui.part.summary#*`.
    assert!(!stdout.contains("ui.part.summary"), "placeholder leaked into findings: {stdout}");
}

/// REQ-003: valid suppressions attach to every unit of the named message (including all plural
/// variants) and silence the suppressed finding in `catalog-lint`.
#[test]
fn valid_suppressions_attach_to_all_variants_and_silence_finding() {
    let units = load_catalog_units(&fixture("messages.en.json"), Some(&fixture("suppress-valid.toml")))
        .expect("load with valid suppressions");

    let footer = units.iter().find(|u| u.id == "report.footer.note").unwrap();
    assert_eq!(footer.suppress.len(), 1);
    assert_eq!(footer.suppress[0].rule, "VC001");

    let one = units.iter().find(|u| u.id == "ui.part.summary#one").unwrap();
    let other = units.iter().find(|u| u.id == "ui.part.summary#other").unwrap();
    assert_eq!(one.suppress.len(), 1);
    assert_eq!(other.suppress.len(), 1);
    assert_eq!(one.suppress[0].reason, "Plural suppression applies to all variants.");
    assert_eq!(other.suppress[0].reason, one.suppress[0].reason);

    // Linting with valid suppression for `report.footer.note` exits 0.
    let out = bin()
        .args([
            "catalog-lint",
            "--export",
            fixture("messages.en.json").to_str().unwrap(),
            "--rules",
            fixture("product-rules-fixture.toml").to_str().unwrap(),
            "--suppressions",
            fixture("suppress-valid.toml").to_str().unwrap(),
        ])
        .output()
        .expect("run catalog-lint with suppressions");
    assert_eq!(
        out.status.code(),
        Some(0),
        "expected exit 0 when suppressed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// AC-3 (error path): suppression naming `ui.ghost.message` absent from the export exits 2 with
/// `CATALOG-UNKNOWN-ID ui.ghost.message`.
#[test]
fn ac3_unknown_suppression_id_exits_2() {
    let err = load_catalog_units(&fixture("messages.en.json"), Some(&fixture("suppress-unknown-id.toml")))
        .expect_err("must reject unknown suppression ID");
    assert_eq!(err, CatalogError::UnknownId { id: "ui.ghost.message".to_owned() });
    assert_eq!(err.to_string(), "CATALOG-UNKNOWN-ID ui.ghost.message");

    let out = bin()
        .args([
            "catalog-units",
            "--export",
            fixture("messages.en.json").to_str().unwrap(),
            "--suppressions",
            fixture("suppress-unknown-id.toml").to_str().unwrap(),
        ])
        .output()
        .expect("run catalog-units");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("CATALOG-UNKNOWN-ID ui.ghost.message"), "stderr: {stderr}");
}

/// AC-4 (error path): suppression with `reason = ""` exits 2 with `CATALOG-NO-REASON`.
#[test]
fn ac4_empty_suppression_reason_exits_2() {
    let err = load_catalog_units(&fixture("messages.en.json"), Some(&fixture("suppress-empty-reason.toml")))
        .expect_err("must reject empty reason");
    assert!(matches!(err, CatalogError::NoReason { .. }));
    assert!(err.to_string().contains("CATALOG-NO-REASON"));

    let out = bin()
        .args([
            "catalog-lint",
            "--export",
            fixture("messages.en.json").to_str().unwrap(),
            "--rules",
            fixture("product-rules-fixture.toml").to_str().unwrap(),
            "--suppressions",
            fixture("suppress-empty-reason.toml").to_str().unwrap(),
        ])
        .output()
        .expect("run catalog-lint");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("CATALOG-NO-REASON"), "stderr: {stderr}");
}

/// D0 determinism, placeholder edge cases, and committed `house-rules/vocab/suppressions.toml`.
#[test]
fn determinism_placeholders_and_repo_suppressions_file() {
    assert_eq!(replace_placeholders("Hello {user_name}!"), "Hello {arg}!");
    assert_eq!(replace_placeholders("{{escaped}} and {a.b:0.2}"), "{{escaped}} and {arg}");
    assert_eq!(replace_placeholders("unclosed {brace and { spaced }"), "unclosed {brace and { spaced }");

    let repo_sup = repo_root().join("house-rules/vocab/suppressions.toml");
    let u1 = load_catalog_units(&fixture("messages.en.json"), Some(&repo_sup)).expect("load 1");
    let u2 = load_catalog_units(&fixture("messages.en.json"), Some(&repo_sup)).expect("load 2");
    assert_eq!(format_units_jsonl(&u1).expect("jsonl 1"), format_units_jsonl(&u2).expect("jsonl 2"));
}
