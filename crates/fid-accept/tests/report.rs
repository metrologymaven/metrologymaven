//! Acceptance criteria for spec node `FLOW.RUNNER.VERDICT` (AC-1..AC-4, REQ-001..REQ-004).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use fid_accept::package::{Assistance, Gating};
use fid_accept::report::{
    command_def, lint_pii, parse_trend_jsonl, show_report, AcceptanceReport, AcceptanceStore, CheckOutcome,
    CheckStatus, PackageResult, ReferenceMachine, ReliabilitySection, ReplayOutcome, ShowReportParams,
    ShowReportResult, TrendRecord, Verdict, VerdictErrorCode, MAX_TREND_LAST, REPORT_FORMAT,
};
use fid_canon::{parse, CValue};
use fid_types::Digest;

const BIN: &str = env!("CARGO_BIN_EXE_fid-accept");
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/flow/verdict/");

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_store(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("fid-accept-verdict-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn read_fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIXTURE_DIR}{name}")).unwrap()
}

#[test]
fn ac1_r1_reserialises_byte_identically_with_unchanged_digest() {
    let raw = read_fixture("r1.json");
    let expected_sha = String::from_utf8(read_fixture("r1.sha256")).unwrap().trim().to_string();

    let report = AcceptanceReport::parse_json(&raw).unwrap();
    assert_eq!(report.format, REPORT_FORMAT);
    assert_eq!(report.packages.len(), 3);

    // REQ-002: assisted and tracked packages carry `non_gating: true` separately from verdict.
    assert!(!report.packages[0].non_gating);
    assert_eq!(report.packages[0].verdict, Verdict::Pass);
    assert!(report.packages[1].non_gating);
    assert_eq!(report.packages[1].assistance, Assistance::Assisted);
    assert_eq!(report.packages[1].verdict, Verdict::Fail);
    assert!(report.packages[2].non_gating);
    assert_eq!(report.packages[2].gating, Gating::Tracked);
    assert_eq!(report.packages[2].verdict, Verdict::NotReplayable);

    let reserialized = report.to_canonical_bytes().unwrap();
    assert_eq!(reserialized, raw, "re-serialised canonical bytes must equal r1.json byte-for-byte");

    let digest = report.digest().unwrap();
    assert_eq!(digest.to_string(), expected_sha, "digest must equal independent SHA-256");

    // Round-trip through AcceptanceStore and show_report by digest.
    let dir = temp_store("ac1");
    let store = AcceptanceStore::new(&dir);
    let stored_digest = store.put_report(&report).unwrap();
    assert_eq!(stored_digest, digest);

    let res = show_report(&store, &ShowReportParams { digest: Some(digest), trend: None, last: None }).unwrap();
    match res {
        ShowReportResult::Report { digest: got_d, report: got_r } => {
            assert_eq!(got_d, digest);
            assert_eq!(*got_r, report);
        }
        other => panic!("expected Report, got {other:?}"),
    }

    // CLI `--digest ... --json` returns the report cleanly.
    let cli = Command::new(BIN)
        .args(["show-report", "--store", dir.to_str().unwrap(), "--digest", digest.as_str(), "--json"])
        .output()
        .unwrap();
    assert_eq!(cli.status.code(), Some(0), "stderr={}", String::from_utf8_lossy(&cli.stderr));
    let parsed = parse(&cli.stdout).unwrap();
    let CValue::Obj(obj) = parsed else { panic!("expected JSON object") };
    assert_eq!(obj.get("kind"), Some(&CValue::Str("report".to_string())));
    assert_eq!(obj.get("digest"), Some(&CValue::Str(digest.to_string())));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ac2_trend_40_nights_returns_30_newest_first() {
    let dir = temp_store("ac2");
    let store = AcceptanceStore::new(&dir);
    let trend_bytes = read_fixture("trend-40.jsonl");
    std::fs::write(store.trend_path(), &trend_bytes).unwrap();

    // Library query: 30 newest records for JOB.ACC.02.
    let res =
        show_report(&store, &ShowReportParams { digest: None, trend: Some("JOB.ACC.02".to_string()), last: Some(30) })
            .unwrap();
    let ShowReportResult::Trend { job, records } = res else { panic!("expected Trend") };
    assert_eq!(job, "JOB.ACC.02");
    assert_eq!(records.len(), 30);
    assert_eq!(records[0].night, "2026-11-20");
    assert_eq!(records[29].night, "2026-10-22");
    for w in records.windows(2) {
        assert!(w[0].night >= w[1].night, "records must be newest first: {} < {}", w[0].night, w[1].night);
        assert_eq!(w[0].job, "JOB.ACC.02");
    }

    // CLI query: `fid-accept show-report --store <dir> --trend JOB.ACC.02 --last 30 --json`.
    let out = Command::new(BIN)
        .args(["show-report", "--store", dir.to_str().unwrap(), "--trend", "JOB.ACC.02", "--last", "30", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "stderr={}", String::from_utf8_lossy(&out.stderr));
    let val = parse(&out.stdout).unwrap();
    let CValue::Obj(map) = val else { panic!("expected object") };
    assert_eq!(map.get("kind"), Some(&CValue::Str("trend".to_string())));
    assert_eq!(map.get("job"), Some(&CValue::Str("JOB.ACC.02".to_string())));
    let Some(CValue::Arr(recs)) = map.get("records") else { panic!("missing records") };
    assert_eq!(recs.len(), 30);
    let first_night = match &recs[0] {
        CValue::Obj(o) => o.get("night").cloned(),
        _ => None,
    };
    let last_night = match &recs[29] {
        CValue::Obj(o) => o.get("night").cloned(),
        _ => None,
    };
    assert_eq!(first_night, Some(CValue::Str("2026-11-20".to_string())));
    assert_eq!(last_night, Some(CValue::Str("2026-10-22".to_string())));

    // Reject `last == 0` and `last > 365` (REQ-004).
    for bad_last in [0, MAX_TREND_LAST + 1] {
        let err = show_report(
            &store,
            &ShowReportParams { digest: None, trend: Some("JOB.ACC.02".to_string()), last: Some(bad_last) },
        )
        .unwrap_err();
        assert_eq!(err.code, VerdictErrorCode::Schema);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ac3_unknown_digest_returns_not_found() {
    let dir = temp_store("ac3");
    let store = AcceptanceStore::new(&dir);
    let unknown = Digest::parse(&format!("sha256:{}", "ff".repeat(32))).unwrap();

    let err = show_report(&store, &ShowReportParams { digest: Some(unknown), trend: None, last: None }).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::NotFound);
    assert_eq!(err.code.as_str(), "not_found");

    // CLI exits non-zero and names `not_found`.
    let out = Command::new(BIN)
        .args(["show-report", "--store", dir.to_str().unwrap(), "--digest", unknown.as_str(), "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not_found"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ac4_host_path_and_email_fail_with_verdict_pii() {
    let pii_path = read_fixture("pii-host-path.json");
    let err1 = AcceptanceReport::parse_json(&pii_path).unwrap_err();
    assert_eq!(err1.code, VerdictErrorCode::Pii);
    assert_eq!(err1.code.as_str(), "VERDICT-PII");

    let pii_email = read_fixture("pii-email.json");
    let err2 = AcceptanceReport::parse_json(&pii_email).unwrap_err();
    assert_eq!(err2.code, VerdictErrorCode::Pii);

    // Windows paths, UNC paths, tilde paths, and file:// URIs must also fail VERDICT-PII.
    for hostile in [
        r"C:\Users\bob\part.step",
        r"\\fileserver\share\data.e57",
        "~/measurements/run1.json",
        "file:///var/tmp/secret.bin",
        "/Users/operator/doc.pdf",
        "operator@factory.example",
    ] {
        let mut m = BTreeMap::new();
        m.insert("reason".to_string(), CValue::Str(hostile.to_string()));
        let err = lint_pii(&CValue::Obj(m)).unwrap_err();
        assert_eq!(err.code, VerdictErrorCode::Pii, "expected VERDICT-PII for {hostile:?}");
    }
}

#[test]
fn non_gating_consistency_and_append_only_trend_and_tamper_detection() {
    let raw = read_fixture("r1.json");
    let mut rep = AcceptanceReport::parse_json(&raw).unwrap();

    // REQ-002: assisted package marked `non_gating: false` must fail VERDICT-SCHEMA.
    rep.packages[1].non_gating = false;
    let bad_bytes = rep.to_canonical_bytes().unwrap();
    let err = AcceptanceReport::parse_json(&bad_bytes).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::Schema);

    // Append-only trend store works and round-trips cleanly.
    let dir = temp_store("append");
    let store = AcceptanceStore::new(&dir);
    let rec1 = TrendRecord {
        night: "2026-11-19".to_string(),
        job: "JOB.ACC.05".to_string(),
        variant: "v1".to_string(),
        verdict: Verdict::Pass,
        machine_s: 9.5,
    };
    let rec2 = TrendRecord {
        night: "2026-11-20".to_string(),
        job: "JOB.ACC.05".to_string(),
        variant: "v1".to_string(),
        verdict: Verdict::Fail,
        machine_s: 11.25,
    };
    store.append_trend(&rec1).unwrap();
    store.append_trend(&rec2).unwrap();
    let parsed = parse_trend_jsonl(&std::fs::read(store.trend_path()).unwrap()).unwrap();
    assert_eq!(parsed, vec![rec1.clone(), rec2.clone()]);

    // Tampered stored report fails digest check on get_report.
    let good = AcceptanceReport::parse_json(&raw).unwrap();
    let d = store.put_report(&good).unwrap();
    let mut altered = good.clone();
    altered.build_id = "2026.11.20-nightly.9".to_string();
    std::fs::write(store.report_path(&d), altered.to_canonical_bytes().unwrap()).unwrap();
    let err = store.get_report(&d).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::Schema);

    // Direct struct construction & digest works cleanly.
    let synthetic = AcceptanceReport {
        format: REPORT_FORMAT.to_string(),
        build_id: "build-1".to_string(),
        manifest_digest: Digest::parse(&format!("sha256:{}", "aa".repeat(32))).unwrap(),
        suite_digest: Digest::parse(&format!("sha256:{}", "bb".repeat(32))).unwrap(),
        started_at: "2026-11-21T00:00:00Z".to_string(),
        ended_at: "2026-11-21T00:01:00Z".to_string(),
        machines: vec![ReferenceMachine { id: "ref-01".to_string(), host: "linux-x86_64".to_string() }],
        packages: vec![PackageResult {
            job: "JOB.ACC.01".to_string(),
            variant: "base".to_string(),
            gating: Gating::Gating,
            assistance: Assistance::Unassisted,
            non_gating: false,
            replay_outcome: ReplayOutcome::Identical,
            checks: vec![CheckOutcome {
                id: "chk-1".to_string(),
                kind: "replay_outcome".to_string(),
                outcome: CheckStatus::Pass,
                reason: None,
            }],
            machine_times: BTreeMap::from([("ref-01".to_string(), 3.25)]),
            verdict: Verdict::Pass,
            reasons: Vec::new(),
        }],
        reliability: ReliabilitySection {
            total_packages: 1,
            passed: 1,
            failed: 0,
            not_replayable: 0,
            gating_passed: 1,
            gating_failed: 0,
            non_gating_count: 0,
            crash_recovery_passed: None,
        },
    };
    let d_syn = store.put_report(&synthetic).unwrap();
    assert_eq!(store.get_report(&d_syn).unwrap().0, synthetic);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn accept_show_report_command_def_is_valid() {
    let def = command_def().unwrap();
    assert_eq!(def.id, "accept.show_report");
    assert_eq!(def.major, 1);
    let catalogue = fid_registry::Catalogue::geo_frame().unwrap();
    let decl = fid_registry::CommandDecl(def.to_value());
    let errs = fid_registry::validate::validate_with(&[decl], &fid_registry::validate::Limits::default(), &catalogue);
    assert!(errs.is_empty(), "validation errors: {errs:?}");
}
