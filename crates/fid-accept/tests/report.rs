//! Acceptance criteria for spec node `FLOW.RUNNER.VERDICT` (AC-1..AC-4, REQ-001..REQ-004).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use fid_accept::package::{Assistance, Gating};
use fid_accept::report::{
    lint_pii, parse_trend_jsonl, show_report, AcceptanceReport, AcceptanceStore, CheckOutcome, CheckStatus,
    PackageResult, ReferenceMachine, ReliabilitySection, ReplayOutcome, ShowReportParams, ShowReportResult,
    TrendRecord, UtcTimestamp, Verdict, VerdictErrorCode, MAX_TREND_LAST, REPORT_FORMAT,
};
use fid_canon::{parse, CValue};
use fid_types::Digest;

const BIN: &str = env!("CARGO_BIN_EXE_fid-accept");
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/flow/verdict/");

static SEQ: AtomicU64 = AtomicU64::new(0);

/// A temporary project: `<dir>/acceptance/suite.json` marks the root and the store is
/// `<dir>/acceptance/store` (the configured location the CLI resolves).
fn temp_project(tag: &str) -> (PathBuf, AcceptanceStore) {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("fid-accept-verdict-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("acceptance").join("store")).unwrap();
    std::fs::write(p.join("acceptance").join("suite.json"), b"{}").unwrap();
    let store = AcceptanceStore::discover(&p).unwrap();
    assert_eq!(store.root(), p.join("acceptance").join("store"));
    (p, store)
}

fn read_fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIXTURE_DIR}{name}")).unwrap()
}

/// Copies the 40-night trend fixture and the reports it names into `store`.
fn install_trend_40(store: &AcceptanceStore) {
    std::fs::write(store.trend_path(), read_fixture("trend-40.jsonl")).unwrap();
    let reports = store.root().join("reports");
    std::fs::create_dir_all(&reports).unwrap();
    for entry in std::fs::read_dir(format!("{FIXTURE_DIR}reports-40")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), reports.join(entry.file_name())).unwrap();
    }
}

fn run_in(project: &Path, args: &[&str]) -> Output {
    Command::new(BIN).current_dir(project).args(args).output().unwrap()
}

fn r1() -> AcceptanceReport {
    AcceptanceReport::parse_json(&read_fixture("r1.json")).unwrap()
}

/// Re-parses a report after mutation; the mutation goes through `to_value` so only the parser's
/// rules decide.
fn reparse(report: &AcceptanceReport) -> Result<AcceptanceReport, fid_accept::report::VerdictError> {
    AcceptanceReport::parse_json(&report.to_canonical_bytes().unwrap())
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
    let (dir, store) = temp_project("ac1");
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

    // CLI `--digest ... --json`, run inside the project, returns the report cleanly.
    let cli = run_in(&dir, &["show-report", "--digest", digest.as_str(), "--json"]);
    assert_eq!(cli.status.code(), Some(0), "stderr={}", String::from_utf8_lossy(&cli.stderr));
    let parsed = parse(&cli.stdout).unwrap();
    let CValue::Obj(obj) = parsed else { panic!("expected JSON object") };
    assert_eq!(obj.get("kind"), Some(&CValue::Str("report".to_string())));
    assert_eq!(obj.get("digest"), Some(&CValue::Str(digest.to_string())));

    // The project is also found from a subdirectory.
    let sub = dir.join("acceptance").join("jobs");
    std::fs::create_dir_all(&sub).unwrap();
    let cli = run_in(&sub, &["show-report", "--digest", digest.as_str(), "--json"]);
    assert_eq!(cli.status.code(), Some(0), "stderr={}", String::from_utf8_lossy(&cli.stderr));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ac2_trend_40_nights_returns_30_newest_first() {
    let (dir, store) = temp_project("ac2");
    install_trend_40(&store);

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
        assert!(w[0].night > w[1].night, "records must be newest first: {} <= {}", w[0].night, w[1].night);
        assert_eq!(w[0].job, "JOB.ACC.02");
    }

    // CLI query: `fid-accept show-report --trend JOB.ACC.02 --last 30 --json` in the project.
    let out = run_in(&dir, &["show-report", "--trend", "JOB.ACC.02", "--last", "30", "--json"]);
    assert_eq!(out.status.code(), Some(0), "stderr={}", String::from_utf8_lossy(&out.stderr));
    let val = parse(&out.stdout).unwrap();
    let CValue::Obj(map) = val else { panic!("expected object") };
    assert_eq!(map.get("kind"), Some(&CValue::Str("trend".to_string())));
    assert_eq!(map.get("job"), Some(&CValue::Str("JOB.ACC.02".to_string())));
    let Some(CValue::Arr(recs)) = map.get("records") else { panic!("missing records") };
    assert_eq!(recs.len(), 30);
    let night = |v: &CValue| match v {
        CValue::Obj(o) => o.get("night").cloned(),
        _ => None,
    };
    assert_eq!(night(&recs[0]), Some(CValue::Str("2026-11-20".to_string())));
    assert_eq!(night(&recs[29]), Some(CValue::Str("2026-10-22".to_string())));

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
    let (dir, store) = temp_project("ac3");
    let unknown = Digest::parse(&format!("sha256:{}", "ff".repeat(32))).unwrap();

    let err = show_report(&store, &ShowReportParams { digest: Some(unknown), trend: None, last: None }).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::NotFound);
    assert_eq!(err.code.as_str(), "not_found");

    // CLI exits non-zero and names `not_found`.
    let out = run_in(&dir, &["show-report", "--digest", unknown.as_str(), "--json"]);
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

// ------------------------------------------------------------------ store location (Design)

#[test]
fn cli_takes_the_store_from_the_project_never_from_a_caller_path() {
    let (dir, store) = temp_project("store-flag");
    let d = store.put_report(&r1()).unwrap();

    // `--store` is refused outright, even when it names the real store.
    let out = run_in(&dir, &["show-report", "--store", store.root().to_str().unwrap(), "--digest", d.as_str()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--store is not accepted"));

    // Outside any project (no acceptance/suite.json up the tree) the command finds no store.
    let outside = std::env::temp_dir().join(format!("fid-accept-verdict-noproj-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).unwrap();
    if AcceptanceStore::discover(&outside).is_err() {
        let out = run_in(&outside, &["show-report", "--digest", d.as_str()]);
        assert_eq!(out.status.code(), Some(1), "stderr={}", String::from_utf8_lossy(&out.stderr));
        assert!(String::from_utf8_lossy(&out.stderr).contains("not_found"));
    }

    // A project whose acceptance/store is missing is `not_found`.
    let bare = outside.join("proj");
    std::fs::create_dir_all(bare.join("acceptance")).unwrap();
    std::fs::write(bare.join("acceptance").join("suite.json"), b"{}").unwrap();
    assert_eq!(AcceptanceStore::discover(&bare).unwrap_err().code, VerdictErrorCode::NotFound);
    // ... and one whose acceptance/store is a file is refused.
    std::fs::write(bare.join("acceptance").join("store"), b"x").unwrap();
    assert_eq!(AcceptanceStore::discover(&bare).unwrap_err().code, VerdictErrorCode::Schema);

    let _ = std::fs::remove_dir_all(&outside);
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------ verdict evidence (REQ-001)

#[test]
fn pass_needs_identical_or_equivalent_replay() {
    let mut rep = r1();
    assert!(reparse(&rep).is_ok());
    rep.packages[0].replay_outcome = ReplayOutcome::Equivalent;
    assert!(reparse(&rep).is_ok(), "equivalent replay supports pass");
    for bad in [ReplayOutcome::Diverged, ReplayOutcome::NotReplayable] {
        rep.packages[0].replay_outcome = bad;
        let err = reparse(&rep).unwrap_err();
        assert_eq!(err.code, VerdictErrorCode::Schema, "pass with replay {bad:?}");
    }
}

#[test]
fn pass_needs_at_least_one_check() {
    let mut rep = r1();
    rep.packages[0].checks.clear();
    let err = reparse(&rep).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::Schema);
    assert!(err.detail.contains("at least one check"), "{err}");
}

#[test]
fn pass_needs_every_check_to_pass() {
    for bad in [CheckStatus::Fail, CheckStatus::Skipped] {
        let mut rep = r1();
        rep.packages[0].checks[1].outcome = bad;
        rep.packages[0].checks[1].reason = Some("not run".to_string());
        let err = reparse(&rep).unwrap_err();
        assert_eq!(err.code, VerdictErrorCode::Schema, "pass with a {bad:?} check");
        assert!(err.detail.contains("every check"), "{err}");
    }
}

#[test]
fn pass_needs_machine_times_for_exactly_the_declared_machines() {
    // Missing one declared machine.
    let mut rep = r1();
    rep.packages[0].machine_times.remove("ref-arm-01");
    let err = reparse(&rep).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::Schema);
    assert!(err.detail.contains("ref-arm-01"), "{err}");

    // No machine times at all.
    let mut rep = r1();
    rep.packages[0].machine_times.clear();
    assert_eq!(reparse(&rep).unwrap_err().code, VerdictErrorCode::Schema);

    // An undeclared machine.
    let mut rep = r1();
    rep.packages[0].machine_times.insert("ref-ghost-01".to_string(), 1.0);
    assert_eq!(reparse(&rep).unwrap_err().code, VerdictErrorCode::Schema);
}

#[test]
fn not_replayable_verdict_and_replay_outcome_must_agree() {
    // Verdict `not_replayable` with a replay that did run.
    for bad in [ReplayOutcome::Identical, ReplayOutcome::Equivalent, ReplayOutcome::Diverged] {
        let mut rep = r1();
        rep.packages[2].replay_outcome = bad;
        let err = reparse(&rep).unwrap_err();
        assert_eq!(err.code, VerdictErrorCode::Schema, "not_replayable with replay {bad:?}");
    }

    // Replay `not_replayable` reported as a `fail` (counts adjusted so only the rule decides).
    let mut rep = r1();
    rep.packages[2].verdict = Verdict::Fail;
    rep.reliability.failed = 2;
    rep.reliability.not_replayable = 0;
    let err = reparse(&rep).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::Schema);
    assert!(err.detail.contains("not_replayable"), "{err}");

    // A diverged replay reported as a fail is accepted.
    let mut rep = r1();
    rep.packages[1].replay_outcome = ReplayOutcome::Diverged;
    assert!(reparse(&rep).is_ok());
}

// ------------------------------------------------------------------ timestamps

#[test]
fn timestamps_compare_as_time_values_not_strings() {
    let a = UtcTimestamp::parse("2026-11-20T01:00:00Z").unwrap();
    let b = UtcTimestamp::parse("2026-11-20T01:00:00.5Z").unwrap();
    let c = UtcTimestamp::parse("2026-11-20T01:00:00.500000000Z").unwrap();
    // As text `00.5Z` < `00Z`, but in time it is later.
    assert!("2026-11-20T01:00:00.5Z" < "2026-11-20T01:00:00Z");
    assert!(b > a);
    assert_eq!(b, c);
    assert!(UtcTimestamp::parse("2026-11-20T01:00:00.Z").is_none());
    assert!(UtcTimestamp::parse("2026-11-20T01:00:00.1234567890Z").is_none());

    // A run ending half a second after it started is valid.
    let mut rep = r1();
    rep.started_at = "2026-11-20T01:00:00Z".to_string();
    rep.ended_at = "2026-11-20T01:00:00.5Z".to_string();
    assert!(reparse(&rep).is_ok());

    // A run ending before it started is refused, though its text sorts after the start.
    rep.started_at = "2026-11-20T01:00:00.5Z".to_string();
    rep.ended_at = "2026-11-20T01:00:00Z".to_string();
    assert_eq!(reparse(&rep).unwrap_err().code, VerdictErrorCode::Schema);
}

// ------------------------------------------------------------------ trend history (REQ-004)

fn on_night(night: &str) -> AcceptanceReport {
    let mut rep = r1();
    rep.started_at = format!("{night}T01:00:00Z");
    rep.ended_at = format!("{night}T01:14:32Z");
    reparse(&rep).unwrap()
}

#[test]
fn trend_records_derive_from_stored_reports() {
    let (dir, store) = temp_project("derive");
    let report = r1();
    let d = store.record_report(&report).unwrap();
    let recs = store.load_trends().unwrap();
    assert_eq!(recs.len(), 3);
    assert_eq!(recs, TrendRecord::from_report(&report, &d));
    for r in &recs {
        assert_eq!(r.night, "2026-11-20");
        assert_eq!(r.report, d);
        store.verify_trend_record(r).unwrap();
    }
    // machine_s is the largest machine time of the package.
    assert_eq!(recs[0].machine_s, 14.5);
    assert_eq!(recs[1].machine_s, 21.0);
    assert_eq!(recs[2].machine_s, 0.5);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn append_refuses_duplicates_and_earlier_nights() {
    let (dir, store) = temp_project("append");
    store.record_report(&on_night("2026-11-19")).unwrap();
    store.record_report(&on_night("2026-11-20")).unwrap();
    assert_eq!(store.load_trends().unwrap().len(), 6);

    // The same (night, job, variant) again.
    let again = on_night("2026-11-20");
    let d = store.put_report(&again).unwrap();
    let dup = TrendRecord::from_report(&again, &d).remove(0);
    let err = store.append_trend(&dup).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::Schema);
    assert!(err.detail.contains("duplicate"), "{err}");

    // A night earlier than the last stored night.
    let early = on_night("2026-11-18");
    let d = store.put_report(&early).unwrap();
    let rec = TrendRecord::from_report(&early, &d).remove(0);
    let err = store.append_trend(&rec).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::Schema);
    assert!(err.detail.contains("earlier"), "{err}");

    // A record whose report is not in the store.
    let mut orphan = TrendRecord::from_report(&on_night("2026-11-21"), &d).remove(0);
    orphan.report = Digest::parse(&format!("sha256:{}", "ee".repeat(32))).unwrap();
    assert_eq!(store.append_trend(&orphan).unwrap_err().code, VerdictErrorCode::Schema);

    // A record that does not match its report.
    let next = on_night("2026-11-21");
    let d = store.put_report(&next).unwrap();
    let mut forged = TrendRecord::from_report(&next, &d).remove(1);
    forged.verdict = Verdict::Pass;
    assert_eq!(store.append_trend(&forged).unwrap_err().code, VerdictErrorCode::Schema);

    // Nothing refused was written.
    assert_eq!(store.load_trends().unwrap().len(), 6);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_refuses_duplicates_and_nights_out_of_order() {
    let lines: Vec<String> =
        String::from_utf8(read_fixture("trend-40.jsonl")).unwrap().lines().map(String::from).collect();
    assert_eq!(parse_trend_jsonl(read_fixture("trend-40.jsonl").as_slice()).unwrap().len(), 80);

    // A duplicated line.
    let mut dup = lines.clone();
    dup.insert(5, lines[4].clone());
    let err = parse_trend_jsonl(format!("{}\n", dup.join("\n")).as_bytes()).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::Schema);
    assert!(err.detail.contains("duplicate"), "{err}");

    // Two nights swapped (line 3/4 are night 2, line 5/6 night 3).
    let mut swapped = lines.clone();
    swapped.swap(2, 4);
    let err = parse_trend_jsonl(format!("{}\n", swapped.join("\n")).as_bytes()).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::Schema);
    assert!(err.detail.contains("earlier"), "{err}");

    // A history rewritten newest-first.
    let mut reversed = lines;
    reversed.reverse();
    assert_eq!(
        parse_trend_jsonl(format!("{}\n", reversed.join("\n")).as_bytes()).unwrap_err().code,
        VerdictErrorCode::Schema
    );
}

#[test]
fn edited_trend_record_is_detected_by_its_report_digest() {
    let (dir, store) = temp_project("edited");
    install_trend_40(&store);
    let params = ShowReportParams { digest: None, trend: Some("JOB.ACC.02".to_string()), last: Some(30) };
    assert!(show_report(&store, &params).is_ok());

    let original = std::fs::read_to_string(store.trend_path()).unwrap();
    // Night 2026-11-15 (i = 34, i % 7 == 6) passed; claim a different machine time.
    let target = original.lines().find(|l| l.contains("\"night\":\"2026-11-15\"") && l.contains("JOB.ACC.02")).unwrap();
    let edited_time = target.replace("\"machine_s\":37", "\"machine_s\":3");
    assert_ne!(edited_time, target);
    std::fs::write(store.trend_path(), original.replace(target, &edited_time)).unwrap();
    let err = show_report(&store, &params).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::Schema);
    assert!(err.detail.contains("edited"), "{err}");

    // A failed night (2026-10-15, i = 3) rewritten as a pass.
    let target = original.lines().find(|l| l.contains("\"night\":\"2026-10-15\"") && l.contains("JOB.ACC.02")).unwrap();
    assert!(target.contains("\"verdict\":\"fail\""));
    let edited_verdict = target.replace("\"verdict\":\"fail\"", "\"verdict\":\"pass\"");
    std::fs::write(store.trend_path(), original.replace(target, &edited_verdict)).unwrap();
    let all = ShowReportParams { digest: None, trend: Some("JOB.ACC.02".to_string()), last: Some(40) };
    assert_eq!(show_report(&store, &all).unwrap_err().code, VerdictErrorCode::Schema);

    // The report a record names is missing.
    std::fs::write(store.trend_path(), &original).unwrap();
    let newest = original.lines().last().unwrap();
    let hex = newest.split("sha256:").nth(1).unwrap().get(0..64).unwrap();
    std::fs::remove_file(store.root().join("reports").join(format!("{hex}.json"))).unwrap();
    assert_eq!(show_report(&store, &params).unwrap_err().code, VerdictErrorCode::Schema);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn non_gating_consistency_and_tamper_detection() {
    let raw = read_fixture("r1.json");
    let mut rep = AcceptanceReport::parse_json(&raw).unwrap();

    // REQ-002: assisted package marked `non_gating: false` must fail VERDICT-SCHEMA.
    rep.packages[1].non_gating = false;
    let bad_bytes = rep.to_canonical_bytes().unwrap();
    let err = AcceptanceReport::parse_json(&bad_bytes).unwrap_err();
    assert_eq!(err.code, VerdictErrorCode::Schema);

    // Tampered stored report fails digest check on get_report.
    let (dir, store) = temp_project("tamper");
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
