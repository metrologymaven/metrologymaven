//! `accept.show_report@1` against its published schemas (spec node `FLOW.RUNNER.VERDICT`):
//! real `show_report` output (report and trend) against `result_schema`, and command inputs
//! against `params_schema`, both checked with the bus validator (`fid_bus::validate::Compiled`).
//! The definition itself is validated against the workspace quantity catalogue.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use fid_accept::report::{
    command_def, show_report, AcceptanceReport, AcceptanceStore, ShowReportParams, ShowReportResult,
};
use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, CValue};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/flow/verdict/");

static SEQ: AtomicU64 = AtomicU64::new(0);

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantity literals here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn params() -> Compiled {
    Compiled::compile(&command_def().unwrap().params_schema).unwrap()
}

fn result() -> Compiled {
    Compiled::compile(&command_def().unwrap().result_schema).unwrap()
}

fn assert_valid(schema: &Compiled, v: &CValue, what: &str) {
    if let Err(f) = schema.evaluate(v, &NoHooks) {
        panic!("{what}: {} at {} ({})", f.keyword, f.pointer, f.message);
    }
}

fn assert_invalid(schema: &Compiled, v: &CValue, what: &str) {
    assert!(schema.evaluate(v, &NoHooks).is_err(), "{what} should be refused");
}

fn json(text: &str) -> CValue {
    parse(text.as_bytes()).unwrap()
}

fn temp_store(tag: &str) -> (PathBuf, AcceptanceStore) {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("fid-accept-verdict-schema-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    (p.clone(), AcceptanceStore::new(p))
}

fn read_fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIXTURE_DIR}{name}")).unwrap()
}

#[test]
fn command_def_is_valid_against_the_workspace_catalogue() {
    let def = command_def().unwrap();
    assert_eq!(def.id, "accept.show_report");
    assert_eq!(def.major, 1);
    let catalogue = fid_registry_build::workspace_catalogue().unwrap();
    let decl = fid_registry::CommandDecl(def.to_value());
    let errs = fid_registry::validate::validate_with(&[decl], &fid_registry::validate::Limits::default(), &catalogue);
    assert!(errs.is_empty(), "validation errors: {errs:?}");
}

#[test]
fn report_output_matches_result_schema() {
    let (dir, store) = temp_store("report");
    let report = AcceptanceReport::parse_json(&read_fixture("r1.json")).unwrap();
    let d = store.put_report(&report).unwrap();
    let out = show_report(&store, &ShowReportParams { digest: Some(d), trend: None, last: None }).unwrap();
    assert!(matches!(out, ShowReportResult::Report { .. }));
    let schema = result();
    let value = out.to_value();
    assert_valid(&schema, &value, "show_report report output");

    // The schema is not vacuous: a missing report field, an unknown field and a wrong kind fail.
    let CValue::Obj(top) = &value else { panic!("object") };
    let mut no_digest = top.clone();
    no_digest.remove("digest");
    assert_invalid(&schema, &CValue::Obj(no_digest), "report output without digest");
    let mut extra = top.clone();
    extra.insert("store".to_string(), CValue::Str("x".to_string()));
    assert_invalid(&schema, &CValue::Obj(extra), "report output with an unknown field");
    let mut wrong_kind = top.clone();
    wrong_kind.insert("kind".to_string(), CValue::Str("trend".to_string()));
    assert_invalid(&schema, &CValue::Obj(wrong_kind), "report output labelled trend");
    let mut bad_verdict = top.clone();
    if let Some(CValue::Obj(rep)) = bad_verdict.get_mut("report") {
        if let Some(CValue::Arr(pkgs)) = rep.get_mut("packages") {
            if let Some(CValue::Obj(p)) = pkgs.get_mut(0) {
                p.insert("verdict".to_string(), CValue::Str("maybe".to_string()));
            }
        }
    }
    assert_invalid(&schema, &CValue::Obj(bad_verdict), "report output with an unknown verdict");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn trend_output_matches_result_schema() {
    let (dir, store) = temp_store("trend");
    std::fs::write(store.trend_path(), read_fixture("trend-40.jsonl")).unwrap();
    let reports = store.root().join("reports");
    std::fs::create_dir_all(&reports).unwrap();
    for entry in std::fs::read_dir(format!("{FIXTURE_DIR}reports-40")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), reports.join(entry.file_name())).unwrap();
    }
    let schema = result();
    for last in [1, 30, 365] {
        let out = show_report(
            &store,
            &ShowReportParams { digest: None, trend: Some("JOB.ACC.02".to_string()), last: Some(last) },
        )
        .unwrap();
        assert!(matches!(out, ShowReportResult::Trend { .. }));
        assert_valid(&schema, &out.to_value(), &format!("show_report trend output, last {last}"));
    }

    // A record without its report digest fails.
    let out =
        show_report(&store, &ShowReportParams { digest: None, trend: Some("JOB.ACC.01".to_string()), last: Some(2) })
            .unwrap();
    let CValue::Obj(mut top) = out.to_value() else { panic!("object") };
    if let Some(CValue::Arr(recs)) = top.get_mut("records") {
        if let Some(CValue::Obj(r)) = recs.get_mut(0) {
            r.remove("report");
        }
    }
    assert_invalid(&schema, &CValue::Obj(top), "trend record without report digest");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inputs_match_params_schema() {
    let schema = params();
    let digest = format!("sha256:{}", "ab".repeat(32));
    for ok in [
        format!(r#"{{"digest":"{digest}"}}"#),
        r#"{"trend":"JOB.ACC.02"}"#.to_string(),
        r#"{"trend":"JOB.ACC.02","last":30}"#.to_string(),
        r#"{"trend":"JOB.ACC.02","last":1}"#.to_string(),
        r#"{"trend":"JOB.ACC.02","last":365}"#.to_string(),
    ] {
        assert_valid(&schema, &json(&ok), &ok);
    }
    for bad in [
        "{}".to_string(),
        format!(r#"{{"digest":"{digest}","trend":"JOB.ACC.02"}}"#),
        format!(r#"{{"digest":"{digest}","last":3}}"#),
        r#"{"last":3}"#.to_string(),
        r#"{"trend":"JOB.ACC.02","last":0}"#.to_string(),
        r#"{"trend":"JOB.ACC.02","last":366}"#.to_string(),
        r#"{"trend":"JOB.ACC.02","last":2.5}"#.to_string(),
        r#"{"trend":"job.acc.02"}"#.to_string(),
        r#"{"digest":"sha256:XYZ"}"#.to_string(),
        // The store is never a parameter.
        r#"{"trend":"JOB.ACC.02","store":"/srv/acceptance"}"#.to_string(),
    ] {
        assert_invalid(&schema, &json(&bad), &bad);
    }
}
