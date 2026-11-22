//! Acceptance tests AC-1..AC-4 and boundary checks for `UNC.RDL.GEN.XCHECK` (`fid-rdl-gen::xcheck`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use fid_canon::{parse_with_limits, CValue};
use fid_rdl::RdlDataset;
use fid_rdl_gen::{
    finalize, finalize_from_json, finalize_rdl_datasets, CrossCheckStatus, Hp, OracleResultFile, RecordOutput,
    XCheckError, XCheckRecord, MAX_ORACLE_FILE_BYTES,
};
use fid_types::Digest;

const RECORDS_JSON: &[u8] = include_bytes!("../../../fixtures/rdl/gen/xcheck/lsnull_seed1_records.json");
const ORACLE_OK_JSON: &[u8] = include_bytes!("../../../fixtures/rdl/gen/xcheck/oracle_ok.json");
const ORACLE_ONE_BAD_JSON: &[u8] = include_bytes!("../../../fixtures/rdl/gen/xcheck/oracle_one_bad.json");
const ORACLE_MISSING_TWO_JSON: &[u8] = include_bytes!("../../../fixtures/rdl/gen/xcheck/oracle_missing_two.json");
const ORACLE_SELF_JSON: &[u8] = include_bytes!("../../../fixtures/rdl/gen/xcheck/oracle_self.json");

fn load_fixture_bytes(name: &str) -> Vec<u8> {
    match name {
        "lsnull_seed1_records.json" => RECORDS_JSON.to_vec(),
        "oracle_ok.json" => ORACLE_OK_JSON.to_vec(),
        "oracle_one_bad.json" => ORACLE_ONE_BAD_JSON.to_vec(),
        "oracle_missing_two.json" => ORACLE_MISSING_TWO_JSON.to_vec(),
        "oracle_self.json" => ORACLE_SELF_JSON.to_vec(),
        other => panic!("unknown fixture: {other}"),
    }
}

fn load_seed1_datasets() -> Vec<RdlDataset> {
    let bytes = load_fixture_bytes("lsnull_seed1_records.json");
    let CValue::Arr(items) = parse_with_limits(&bytes, 4 * 1024 * 1024, 32).expect("valid JSON array") else {
        panic!("expected JSON array of records");
    };
    items.iter().map(|v| RdlDataset::from_value(v).expect("valid rdl-dataset/v1 record")).collect()
}

fn load_seed1_xcheck_records() -> Vec<XCheckRecord> {
    load_seed1_datasets().iter().map(|ds| XCheckRecord::from_rdl_dataset(ds).expect("valid XCheckRecord")).collect()
}

#[test]
fn ac1_all_seed1_records_agree_with_recorded_deviations() {
    let mut records = load_seed1_xcheck_records();
    let oracle_bytes = load_fixture_bytes("oracle_ok.json");

    finalize_from_json(&mut records, &oracle_bytes, true).expect("AC-1 must succeed");
    assert_eq!(records.len(), 6);

    let tol = Hp::parse_decimal("1e-30").unwrap();
    for rec in &records {
        assert_eq!(rec.cross_checks.len(), 1);
        let cc = &rec.cross_checks[0];
        assert_eq!(cc.oracle_id, "fiducial-oracles-mpmath");
        assert_eq!(cc.oracle_version, "0.1.0");
        assert_eq!(cc.status, CrossCheckStatus::Agreed);
        let dev_str = cc.max_dev_decimal.as_deref().expect("agreed record has max_dev_decimal");
        let dev = Hp::parse_decimal(dev_str).expect("valid decimal deviation");
        assert!(dev <= tol, "record {} deviation {} exceeded 1e-30", rec.id, dev_str);
    }

    // Also verify end-to-end RdlDataset integration writes fid_rdl::CrossCheck entries cleanly.
    let mut datasets = load_seed1_datasets();
    let oracle = OracleResultFile::from_json(&oracle_bytes).unwrap();
    let ccs = finalize_rdl_datasets(&mut datasets, &oracle, true).expect("RdlDataset finalize must succeed");
    assert_eq!(ccs.len(), 6);
    for ds in &datasets {
        assert_eq!(ds.cross_check().len(), 1);
        assert_eq!(ds.cross_check()[0].oracle, "fiducial-oracles-mpmath");
        assert_eq!(ds.cross_check()[0].outcome, fid_rdl::CrossOutcome::Agree);
    }
}

#[test]
fn ac2_one_bad_reference_fails_atomically_and_leaves_records_unchanged() {
    let mut records = load_seed1_xcheck_records();
    let snapshot = records.clone();
    let oracle_bytes = load_fixture_bytes("oracle_one_bad.json");

    let err = finalize_from_json(&mut records, &oracle_bytes, true).expect_err("AC-2 must fail");
    assert_eq!(err.code(), "oracle_disagreement");
    assert_eq!(err, XCheckError::OracleDisagreement { ids: vec!["rdl:G/ls.circle.gn/seed1/0003".to_string()] });
    assert_eq!(records, snapshot, "records must remain completely unchanged on oracle_disagreement");

    // Verify multiple disagreeing records are ALL listed.
    let mut oracle = OracleResultFile::from_json(&oracle_bytes).unwrap();
    let first_digest = records[0].digest;
    oracle.entries.get_mut(&first_digest).unwrap().insert("origin_x".to_string(), "0.5".to_string());
    let err2 = finalize(&mut records, &oracle, true).expect_err("two bad records must fail");
    assert_eq!(
        err2,
        XCheckError::OracleDisagreement {
            ids: vec!["rdl:G/ls.line.svd/seed1/0001".to_string(), "rdl:G/ls.circle.gn/seed1/0003".to_string()]
        }
    );
    assert_eq!(records, snapshot);
}

#[test]
fn ac3_missing_two_records_pending_when_optional_and_error_when_required() {
    let oracle_bytes = load_fixture_bytes("oracle_missing_two.json");

    // require_all = false -> matched records are Agreed, two missing records are Pending.
    let mut records_opt = load_seed1_xcheck_records();
    finalize_from_json(&mut records_opt, &oracle_bytes, false).expect("require_all=false must succeed");
    for rec in &records_opt[..4] {
        assert_eq!(rec.cross_checks.len(), 1);
        assert_eq!(rec.cross_checks[0].status, CrossCheckStatus::Agreed);
        assert!(rec.cross_checks[0].max_dev_decimal.is_some());
    }
    for rec in &records_opt[4..] {
        assert_eq!(rec.cross_checks.len(), 1);
        assert_eq!(rec.cross_checks[0].status, CrossCheckStatus::Pending);
        assert_eq!(rec.cross_checks[0].oracle_id, "fiducial-oracles-mpmath");
        assert_eq!(rec.cross_checks[0].oracle_version, "0.1.0");
        assert!(rec.cross_checks[0].max_dev_decimal.is_none());
    }

    // require_all = true -> fails with oracle_missing naming both missing IDs, leaving records untouched.
    let mut records_req = load_seed1_xcheck_records();
    let snapshot = records_req.clone();
    let err = finalize_from_json(&mut records_req, &oracle_bytes, true).expect_err("require_all=true must fail");
    assert_eq!(err.code(), "oracle_missing");
    assert_eq!(
        err,
        XCheckError::OracleMissing {
            ids: vec!["rdl:G/ls.cylinder.gn/seed1/0005".to_string(), "rdl:G/ls.cone.gn/seed1/0006".to_string()]
        }
    );
    assert_eq!(records_req, snapshot);
}

#[test]
fn ac4_generator_tool_id_rejected_as_not_independent() {
    let mut records = load_seed1_xcheck_records();
    let snapshot = records.clone();
    let oracle_bytes = load_fixture_bytes("oracle_self.json");

    let err = finalize_from_json(&mut records, &oracle_bytes, false).expect_err("AC-4 must fail");
    assert_eq!(err.code(), "oracle_not_independent");
    assert_eq!(err, XCheckError::OracleNotIndependent { oracle_id: "fid-rdl-gen".to_string() });
    assert_eq!(records, snapshot);

    // Also reject when oracle_id matches a record's custom generator prefix (`custom-gen:lsnull@p256d50`).
    records[0].generator_id = "custom-gen:lsnull@p256d50".to_string();
    let mut oracle = OracleResultFile::from_json(&load_fixture_bytes("oracle_ok.json")).unwrap();
    oracle.oracle_id = "custom-gen".to_string();
    let err2 = finalize(&mut records, &oracle, false).expect_err("matching custom generator prefix must fail");
    assert_eq!(err2.code(), "oracle_not_independent");
}

#[test]
fn bounded_parse_and_schema_validation_fail_closed() {
    // 16 MiB cap enforced.
    let oversized = vec![b' '; MAX_ORACLE_FILE_BYTES + 1];
    let err = OracleResultFile::from_json(&oversized).expect_err("oversized file must fail");
    assert_eq!(err.code(), "oracle_file_invalid");

    // Wrong schema tag.
    let bad_schema = br#"{"schema":"oracle-result/v2","oracle_id":"ext","oracle_version":"1.0","entries":{}}"#;
    assert_eq!(OracleResultFile::from_json(bad_schema).unwrap_err().code(), "oracle_file_invalid");

    // Unknown root key.
    let unknown_key =
        br#"{"schema":"oracle-result/v1","oracle_id":"ext","oracle_version":"1.0","entries":{},"extra":1}"#;
    assert_eq!(OracleResultFile::from_json(unknown_key).unwrap_err().code(), "oracle_file_invalid");

    // Duplicate key rejected.
    let dup_key =
        br#"{"schema":"oracle-result/v1","oracle_id":"a","oracle_id":"b","oracle_version":"1.0","entries":{}}"#;
    assert_eq!(OracleResultFile::from_json(dup_key).unwrap_err().code(), "oracle_file_invalid");

    // Non-decimal value rejected.
    let bad_dec = br#"{"schema":"oracle-result/v1","oracle_id":"ext","oracle_version":"1.0","entries":{"sha256:0000000000000000000000000000000000000000000000000000000000000000":{"radius":"NaN"}}}"#;
    assert_eq!(OracleResultFile::from_json(bad_dec).unwrap_err().code(), "oracle_file_invalid");
}

#[test]
fn canonical_roundtrip_and_batch_determinism() {
    let oracle_bytes = load_fixture_bytes("oracle_ok.json");
    let parsed = OracleResultFile::from_json(&oracle_bytes).unwrap();
    let c1 = parsed.to_canonical_bytes().unwrap();
    let c2 = OracleResultFile::from_json(&c1).unwrap().to_canonical_bytes().unwrap();
    assert_eq!(c1, c2, "canonical RFC 8785 bytes must be D0 byte-identical");

    // Verify fast batch finalisation across 1,000 synthetic records (scales well below 5 s for 10k).
    let mut records = Vec::with_capacity(1_000);
    let mut entries = BTreeMap::new();
    for i in 0..1_000u32 {
        let mut raw = [0u8; 32];
        raw[..4].copy_from_slice(&i.to_be_bytes());
        let dig = Digest::from_bytes(raw);
        let val = format!("1.{i:06}0000000000000000000000000000000000000000000e-01");
        records.push(XCheckRecord {
            id: format!("rdl:G/ls.circle.gn/batch/{i:04}"),
            digest: dig,
            generator_id: "fid-rdl-gen".to_string(),
            outputs: vec![RecordOutput {
                name: "radius".to_string(),
                value_dec: val.clone(),
                test_uncertainty_dec: "1e-30".to_string(),
            }],
            cross_checks: Vec::new(),
        });
        let mut outs = BTreeMap::new();
        outs.insert("radius".to_string(), val);
        entries.insert(dig, outs);
    }
    let oracle = OracleResultFile {
        oracle_id: "fiducial-oracles-mpmath".to_string(),
        oracle_version: "0.1.0".to_string(),
        entries,
    };
    finalize(&mut records, &oracle, true).expect("1,000-record batch must pass");
    assert!(records.iter().all(|r| r.cross_checks.len() == 1 && r.cross_checks[0].status == CrossCheckStatus::Agreed));
}
