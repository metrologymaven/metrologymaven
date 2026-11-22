//! Acceptance and error-path tests for `UNC.RDL.EXT.LIC` (`fid_rdl::ext::lic`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_rdl::ext::lic::{is_allowlisted, visibility_for, LicError, LicenceEvidence, ALLOWLIST_VERSION, RDL_REDIST_V1};
use fid_rdl::Visibility;

const TABLE_CSV: &str = include_str!("../../../fixtures/rdl/ext/lic/table.csv");
const VALID_DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn full_evidence(spdx_or_ref: &str) -> LicenceEvidence {
    LicenceEvidence {
        spdx_or_ref: spdx_or_ref.to_string(),
        evidence_url: "https://example.org/datasets/terms.html".to_string(),
        read_on: "2026-11-22".to_string(),
        confirmed_by: "maintainer-a".to_string(),
        digest_of_terms: VALID_DIGEST.to_string(),
    }
}

/// AC-1: Every row (12 rows) of `fixtures/rdl/ext/lic/table.csv` matches `visibility_for`.
#[test]
fn ac1_table_csv_all_12_rows_match() {
    let rows: Vec<&str> = TABLE_CSV.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(rows.len(), 13, "expected header + 12 rows");
    assert_eq!(rows[0], "licence,evidence_present,expected_visibility");

    for (idx, line) in rows.iter().skip(1).enumerate() {
        let cols: Vec<&str> = line.split(',').collect();
        assert_eq!(cols.len(), 3, "row {idx}: {line}");
        let licence = cols[0];
        let has_ev = match cols[1] {
            "true" => true,
            "false" => false,
            other => panic!("row {idx}: bad boolean {other}"),
        };
        let want = Visibility::parse(cols[2]).expect("expected visibility");
        let ev = has_ev.then(|| full_evidence(licence));
        let dec = visibility_for(licence, ev.as_ref()).expect("valid evidence");
        assert_eq!(dec.visibility, want, "row {idx} ({licence}, has_ev={has_ev})");
        assert_eq!(dec.allowlist_version, ALLOWLIST_VERSION);
    }
}

/// AC-2: Evidence without `confirmed_by` fails with `licence_evidence_incomplete{field: "confirmed_by"}`,
/// and every other missing/malformed field is named accurately.
#[test]
fn ac2_incomplete_evidence_names_missing_field() {
    let mut no_confirm = full_evidence("CC-BY-4.0");
    no_confirm.confirmed_by = String::new();
    let err = visibility_for("CC-BY-4.0", Some(&no_confirm)).unwrap_err();
    assert_eq!(err, LicError::EvidenceIncomplete { field: "confirmed_by" });
    assert_eq!(err.code(), "licence_evidence_incomplete");
    assert_eq!(err.field(), "confirmed_by");

    let mut bad_url = full_evidence("CC-BY-4.0");
    bad_url.evidence_url = "ftp://invalid.example/terms".to_string();
    assert_eq!(
        visibility_for("CC-BY-4.0", Some(&bad_url)).unwrap_err(),
        LicError::EvidenceIncomplete { field: "evidence_url" }
    );

    let mut bad_date = full_evidence("CC-BY-4.0");
    bad_date.read_on = "2026-02-30".to_string();
    assert_eq!(
        visibility_for("CC-BY-4.0", Some(&bad_date)).unwrap_err(),
        LicError::EvidenceIncomplete { field: "read_on" }
    );

    let mut bad_digest = full_evidence("CC-BY-4.0");
    bad_digest.digest_of_terms = "sha256:short".to_string();
    assert_eq!(
        visibility_for("CC-BY-4.0", Some(&bad_digest)).unwrap_err(),
        LicError::EvidenceIncomplete { field: "digest_of_terms" }
    );

    let mut bad_spdx = full_evidence("CC-BY-4.0");
    bad_spdx.spdx_or_ref = String::new();
    assert_eq!(
        visibility_for("CC-BY-4.0", Some(&bad_spdx)).unwrap_err(),
        LicError::EvidenceIncomplete { field: "spdx_or_ref" }
    );
}

/// AC-3: Public result records `rdl-redist@1` both on the decision and in canonical JSON, and
/// mismatched evidence licence fails closed to `reference_only`.
#[test]
fn ac3_public_result_records_allowlist_version_and_mismatch_fails_closed() {
    let ev = full_evidence("CC-BY-4.0");
    let dec = visibility_for("CC-BY-4.0", Some(&ev)).unwrap();
    assert_eq!(dec.visibility, Visibility::Public);
    assert_eq!(dec.allowlist_version, "rdl-redist@1");
    let rec = dec.recorded_evidence.expect("recorded evidence present");
    assert_eq!(rec.allowlist_version, "rdl-redist@1");
    let json = String::from_utf8(rec.to_canonical_bytes().unwrap()).unwrap();
    assert!(json.contains(r#""allowlist_version":"rdl-redist@1""#), "json: {json}");

    // Mismatched licence between dataset and evidence fails closed to reference_only.
    let mismatch = visibility_for("CC0-1.0", Some(&ev)).unwrap();
    assert_eq!(mismatch.visibility, Visibility::ReferenceOnly);
    assert_eq!(mismatch.allowlist_version, "rdl-redist@1");

    // Allowlist sanity check.
    assert!(is_allowlisted("CC-BY-4.0"));
    assert!(is_allowlisted("CC0-1.0"));
    assert!(!is_allowlisted("GPL-3.0-only"));
    assert!(!RDL_REDIST_V1.is_empty());
}
