//! Acceptance and unit tests for `MIG.COVER.TAXO` (`spec/mig/cover/taxo/NODE.md`):
//! AC-1..AC-5 plus `TAXO-DUP`, `TAXO-SCHEMA`, major version bump allowing rename, and
//! validation of the committed `taxonomies/` directory.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use fid_mig::cover::taxo::{check_all, check_paths, parse_taxonomy, FindingCode};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn fix_dir() -> PathBuf {
    repo_root().join("fixtures/mig/cover/taxo")
}

/// AC-1: `fixtures/mig/cover/taxo/ok/` with `ok/dmis-conformance.json` passes with 0 findings.
#[test]
fn ac1_ok_taxonomies_and_dmis_conformance_pass() {
    let ok = fix_dir().join("ok");
    let conf = ok.join("dmis-conformance.json");
    let rep = check_paths(&ok, Some(&conf), None, None).expect("check ok/");
    assert!(rep.is_ok(), "unexpected findings: {:?}", rep.findings);
    assert_eq!(rep.taxonomies.len(), 4);
    assert!(rep.taxonomies.contains_key("qif-project"));
    assert!(rep.taxonomies.contains_key("dmis-output"));
    assert!(rep.taxonomies.contains_key("csv-points"));
    assert!(rep.taxonomies.contains_key("form-xlsx"));

    // Also verify the committed `taxonomies/` folder passes cleanly.
    let committed = repo_root().join("taxonomies");
    let rep2 = check_paths(&committed, Some(&conf), None, None).expect("check taxonomies/");
    assert!(rep2.is_ok(), "committed taxonomies/ findings: {:?}", rep2.findings);
}

/// AC-2 (error path): `dmis-missing/` omits `SNSET` -> fails with `TAXO-INCOMPLETE SNSET`.
#[test]
fn ac2_missing_dmis_keyword_fails_with_taxo_incomplete_snset() {
    let missing = fix_dir().join("dmis-missing");
    let conf = fix_dir().join("ok/dmis-conformance.json");
    let rep = check_paths(&missing, Some(&conf), None, None).expect("check dmis-missing/");
    assert!(!rep.is_ok());
    assert!(
        rep.findings.iter().any(|f| f.code == FindingCode::Incomplete && f.subject == "SNSET"),
        "expected TAXO-INCOMPLETE SNSET, got {:?}",
        rep.findings
    );
    assert!(rep.findings.iter().any(|f| f.to_string().starts_with("TAXO-INCOMPLETE SNSET")));
}

/// AC-3 (error path): `ok/` with `--reports unknown-report/` naming `point_set.weighted`
/// fails with `TAXO-UNKNOWN`.
#[test]
fn ac3_unknown_construct_in_report_fails_with_taxo_unknown() {
    let ok = fix_dir().join("ok");
    let reports = fix_dir().join("unknown-report");
    let rep = check_paths(&ok, None, Some(&reports), None).expect("check unknown-report/");
    assert!(!rep.is_ok());
    assert!(
        rep.findings.iter().any(|f| f.code == FindingCode::Unknown && f.subject.contains("point_set.weighted")),
        "expected TAXO-UNKNOWN naming point_set.weighted, got {:?}",
        rep.findings
    );
}

/// AC-4 (error path): `renamed/` renames a construct with only a minor version bump against
/// `--previous ok/` -> fails with `TAXO-BREAKING`; a major bump (`2.0.0`) succeeds.
#[test]
fn ac4_renamed_construct_without_major_bump_fails_with_taxo_breaking() {
    let ok = fix_dir().join("ok");
    let renamed = fix_dir().join("renamed");
    let rep = check_paths(&renamed, None, None, Some(&ok)).expect("check renamed/");
    assert!(!rep.is_ok());
    assert!(
        rep.findings.iter().any(|f| f.code == FindingCode::Breaking && f.subject == "csv-points"),
        "expected TAXO-BREAKING for csv-points, got {:?}",
        rep.findings
    );

    // Major bump 2.0.0 allows removal/rename.
    let prev_bytes = std::fs::read(ok.join("csv-points.json")).unwrap();
    let curr_text = std::fs::read_to_string(renamed.join("csv-points.json")).unwrap().replace("1.1.0", "2.0.0");
    let rep_major = check_all(
        &[("csv-points.json".to_string(), curr_text.into_bytes())],
        None,
        &[],
        Some(&[("csv-points.json".to_string(), prev_bytes)]),
    );
    assert!(rep_major.is_ok(), "major bump must allow rename: {:?}", rep_major.findings);
}

/// AC-5 (error path): `no-attest/` has empty attestation fields -> fails with `TAXO-ATTEST`.
#[test]
fn ac5_empty_attestation_fails_with_taxo_attest() {
    let no_attest = fix_dir().join("no-attest");
    let rep = check_paths(&no_attest, None, None, None).expect("check no-attest/");
    assert!(!rep.is_ok());
    assert!(rep.findings.iter().any(|f| f.code == FindingCode::Attest), "expected TAXO-ATTEST, got {:?}", rep.findings);
}

/// REQ-001: duplicate construct IDs, invalid construct IDs, and descriptions > 200 chars fail.
#[test]
fn duplicate_id_bad_id_and_overlong_description_refused() {
    let long_desc = "a".repeat(201);
    let doc = format!(
        r#"{{
          "schema": "mig-taxonomy@1",
          "format_id": "test-fmt",
          "version": "1.0.0",
          "reviewer_attestation": {{"reviewer":"R","date":"2026-11-22","statement":"Original text."}},
          "constructs": [
            {{"id":"good_one","kind":"record","description":"Valid construct."}},
            {{"id":"good_one","kind":"record","description":"Duplicate construct."}},
            {{"id":"BAD_UPPER","kind":"record","description":"Bad ID."}},
            {{"id":"too_long","kind":"field","description":"{long_desc}"}}
          ]
        }}"#
    );
    let (taxo, findings) = parse_taxonomy("bad.json", doc.as_bytes());
    assert!(taxo.is_none());
    assert!(findings.iter().any(|f| f.code == FindingCode::Dup));
    assert!(findings.iter().any(|f| f.code == FindingCode::Schema && f.detail.contains("construct id")));
    assert!(findings.iter().any(|f| f.code == FindingCode::Schema && f.detail.contains("description")));
}
