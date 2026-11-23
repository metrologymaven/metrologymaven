//! Acceptance tests for `ECO.RULES.VOCAB.RULESET` (`spec/eco/rules/vocab/ruleset/NODE.md`):
//! AC-1 (`fid-vocab test-rules --rules house-rules/vocab/product-rules.toml` exits 0),
//! AC-2 (every regulated term in `fixtures/vocab/ruleset/regulated-terms.txt` is in VC001, and
//!       removing `traceable` fails naming `traceable`),
//! AC-3 (every product on the title line of competitor `seeds/*.md` inventories is in
//!       `house-rules/vocab/incumbents.txt`),
//! AC-4 (`io.import_pcdmis.title` allowed by VC002, `ui.home.tagline` rejected by VC002),
//! AC-5 (`"Conforms (rule ISO 14253-1:2017 default)"` passes VC001; REQ-007 excluded words
//!       absent from VC001).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_vocab::{lint, parse_units, test_rules, RuleSet, Severity, Unit};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn product_rules_path() -> PathBuf {
    repo_root().join("house-rules/vocab/product-rules.toml")
}

fn incumbents_path() -> PathBuf {
    repo_root().join("house-rules/vocab/incumbents.txt")
}

fn read_non_comment_lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Extracts VC001's configured word list directly from the rule TOML file.
fn vc001_words_from_toml(toml_text: &str) -> BTreeSet<String> {
    let tbl: toml::Table = toml_text.parse().unwrap();
    let rules = tbl.get("rule").unwrap().as_array().unwrap();
    let vc001 =
        rules.iter().find(|r| r.get("id").and_then(toml::Value::as_str) == Some("VC001")).expect("VC001 present");
    vc001.get("words").unwrap().as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_lowercase()).collect()
}

fn missing_regulated_terms(vc001_words: &BTreeSet<String>, regulated_terms: &[String]) -> Vec<String> {
    regulated_terms.iter().filter(|term| !vc001_words.contains(&term.to_lowercase())).cloned().collect()
}

/// AC-1: `fid-vocab test-rules --rules house-rules/vocab/product-rules.toml` exits 0 and
/// library `test_rules` reports zero failures.
#[test]
fn ac1_product_rules_test_rules_cli_and_lib_pass() {
    let rules_p = product_rules_path();
    let rules = RuleSet::load(&rules_p).expect("product-rules.toml loads cleanly");
    let failures = test_rules(&rules);
    assert!(failures.is_empty(), "test_rules failed: {failures:?}");

    let out = Command::new(env!("CARGO_BIN_EXE_fid-vocab"))
        .args(["test-rules", "--rules"])
        .arg(&rules_p)
        .output()
        .expect("run fid-vocab test-rules");
    assert!(
        out.status.success(),
        "fid-vocab test-rules failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// AC-2 + REQ-001 + REQ-005 + REQ-007: Every regulated term in
/// `fixtures/vocab/ruleset/regulated-terms.txt` appears in VC001; removing `"traceable"` from
/// VC001 causes the coverage check to fail naming `traceable`; and REQ-007 decision nouns
/// (`conformance`, `compliance`, `conforms`, `nonconforms`) are absent from VC001.
#[test]
fn ac2_regulated_term_coverage_and_decision_word_exemption() {
    let toml_text = std::fs::read_to_string(product_rules_path()).unwrap();
    let vc001 = vc001_words_from_toml(&toml_text);
    let regulated = read_non_comment_lines(&repo_root().join("fixtures/vocab/ruleset/regulated-terms.txt"));
    assert!(!regulated.is_empty());

    let missing = missing_regulated_terms(&vc001, &regulated);
    assert!(missing.is_empty(), "regulated terms absent from VC001: {missing:?}");

    // Mutation check: removing "traceable" fails naming `traceable`.
    let mut mutated = vc001.clone();
    assert!(mutated.remove("traceable"));
    let mutated_missing = missing_regulated_terms(&mutated, &regulated);
    assert_eq!(mutated_missing, vec!["traceable".to_string()]);

    // REQ-007: decision vocabulary must not appear in VC001.
    for excluded in ["conformance", "compliance", "conforms", "nonconforms"] {
        assert!(!vc001.contains(excluded), "REQ-007 violation: '{excluded}' must not be in VC001");
    }
}

/// AC-3 + REQ-006: Every competitor product named on the title line of `seeds/*.md`
/// (excluding non-competitor files `README.md`, `out-of-scope.md`, `devices.md`,
/// `journeys.md`) appears in `house-rules/vocab/incumbents.txt`.
#[test]
fn ac3_every_seed_title_product_is_in_incumbents() {
    let incumbents: BTreeSet<String> =
        read_non_comment_lines(&incumbents_path()).into_iter().map(|s| s.to_lowercase()).collect();
    assert!(!incumbents.is_empty());

    let excluded: BTreeSet<&str> = ["README.md", "out-of-scope.md", "devices.md", "journeys.md"].into_iter().collect();
    let seeds_dir = repo_root().join("seeds");
    let mut checked_products = 0usize;

    for entry in std::fs::read_dir(&seeds_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let fname = path.file_name().unwrap().to_str().unwrap();
        if excluded.contains(fname) {
            continue;
        }
        let first_line = std::fs::read_to_string(&path).unwrap().lines().next().unwrap_or("").trim().to_string();
        let title = first_line.strip_prefix("# ").unwrap_or(&first_line);
        // Strip parenthetical sub-product list (e.g. "PolyWorks (Inspector, Modeler, DataLoop)")
        // and split slash-separated alternatives (e.g. "GOM Inspect / ZEISS INSPECT Optical 3D").
        for part in title.split('/') {
            let main_name = part.split('(').next().unwrap_or(part).trim();
            if main_name.is_empty() {
                continue;
            }
            assert!(
                incumbents.contains(&main_name.to_lowercase()),
                "seed {fname} title product {main_name:?} missing from house-rules/vocab/incumbents.txt"
            );
            checked_products += 1;
        }
    }
    assert!(checked_products >= 4, "expected at least 4 seed title products, found {checked_products}");
}

/// AC-4 + AC-5 + REQ-002 + REQ-003:
/// - `io.import_pcdmis.title` naming an incumbent passes VC002; `ui.home.tagline` naming the
///   same incumbent fails VC002 with error severity.
/// - `"Conforms (rule ISO 14253-1:2017 default)"` produces zero VC001 findings.
/// - VC003 rejects incumbent + comparison word even with a benchmark link (`link_prefixes = []`).
#[test]
fn ac4_ac5_sample_units_vc001_vc002_vc003() {
    let rules = RuleSet::load(&product_rules_path()).unwrap();
    let raw_units = std::fs::read(repo_root().join("fixtures/vocab/ruleset/sample-units.jsonl")).unwrap();
    let units = parse_units(&raw_units).unwrap();
    assert_eq!(units.len(), 3);

    let report = lint(&rules, &units);

    // AC-4: io.import_pcdmis.title has NO VC002 finding; ui.home.tagline HAS a VC002 error.
    assert!(
        !report.findings.iter().any(|f| f.unit == "io.import_pcdmis.title" && f.rule == "VC002"),
        "io.import_pcdmis.title should be allowed by VC002: {:?}",
        report.findings
    );
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.unit == "ui.home.tagline" && f.rule == "VC002" && f.severity == Severity::Error),
        "ui.home.tagline must trigger VC002 error: {:?}",
        report.findings
    );

    // AC-5: "Conforms (rule ISO 14253-1:2017 default)" produces zero VC001 findings.
    assert!(
        !report.findings.iter().any(|f| f.unit == "gdt.decision.conforms" && f.rule == "VC001"),
        "Conforms decision text must not trigger VC001: {:?}",
        report.findings
    );

    // REQ-003: VC003 has empty link_prefixes so even a linked comparison fails.
    let cmp_unit = Unit {
        id: "ui.home.compare".into(),
        file: "ui/home.ftl".into(),
        line: 9,
        col: 1,
        text: "Faster than PolyWorks on large point clouds.".into(),
        links: vec!["https://metrologymaven.org/bench/align-1".into()],
        suppress: Vec::new(),
    };
    let cmp_report = lint(&rules, &[cmp_unit]);
    assert!(
        cmp_report.findings.iter().any(|f| f.rule == "VC003" && f.severity == Severity::Error),
        "VC003 must reject incumbent comparisons even when a link is present"
    );
}
