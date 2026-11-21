//! Acceptance criteria of spec leaf MIG.SRC.REG through the library: the sources check
//! (AC-1 .. AC-4, REQ-005, schema refusals), the listing and the page (AC-5, AC-6), the
//! registry definition and its schemas (validated with `fid_bus::validate::Compiled`).
//! Fixtures and expected outputs are written by `fixtures/mig/src/reg/make_fixtures.py`, an
//! independent reference; expected finding lines are hand-labelled from the spec.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::CValue;
use fid_mig::src::reg::{
    check, command_defs, embedded, list_sources, load, run_list_sources, ListError, ListSources, Status, LIST_SOURCES,
};
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mig/src/reg").join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn manifest() -> Vec<u8> {
    fixture("manifest-formats.json")
}

fn lines(name: &str) -> Vec<String> {
    check(&fixture(name), &manifest()).findings.iter().map(ToString::to_string).collect()
}

fn params(text: &str) -> CValue {
    fid_canon::parse(text.as_bytes()).unwrap()
}

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn assert_valid(schema: &Compiled, v: &CValue, what: &str) {
    if let Err(f) = schema.evaluate(v, &NoHooks) {
        panic!("{what}: {} at {} ({})", f.keyword, f.pointer, f.message);
    }
}

#[test]
fn ac1_register_ok_passes() {
    let r = check(&fixture("register-ok.json"), &manifest());
    assert!(r.is_ok(), "{:?}", r.findings);
    let reg = r.register.unwrap();
    assert_eq!(reg.version().to_string(), "0.3.0");
    assert_eq!(reg.entries().len(), 5);
    // Sorted by ID whatever the file order.
    let ids: Vec<&str> = reg.entries().iter().map(|e| e.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);
}

#[test]
fn ac2_admitted_without_review() {
    assert_eq!(lines("admitted-noreview.json"), ["SRC-REVIEW src.demoscan-points-csv missing review record"]);
}

#[test]
fn ac3_native_admitted_under_d033a() {
    assert_eq!(lines("native-admitted.json"), ["SRC-POLICY src.democmm-program-native D-033-A"]);
}

#[test]
fn ac4_duplicate_overlapping_versions() {
    // Product compared case-insensitively ("DemoCMM" vs "democmm"), "*..*" overlaps "2..3".
    assert_eq!(lines("dup.json"), ["SRC-DUP src.democmm-results-qif src.democmm-results-qif-2"]);
}

#[test]
fn req005_attestation_missing() {
    assert_eq!(lines("attest-missing.json"), ["SRC-ATTEST src.democmm-results-qif"]);
}

#[test]
fn schema_refuses_markup_in_text() {
    let f = lines("schema-bad.json");
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].starts_with("SRC-SCHEMA entries[2].product"), "{f:?}");
}

#[test]
fn open_formats_must_be_registered() {
    assert_eq!(
        lines("unregistered.json"),
        ["SRC-SCHEMA src.democmm-program-native.format_id: democmm-prog is not a registered format"]
    );
    // A manifest without a format catalogue registers nothing: every non-native format and
    // export format of register-ok.json is refused, in ID order.
    let r = check(&fixture("register-ok.json"), br#"{"commands":[]}"#);
    assert!(r.register.is_none());
    assert_eq!(
        r.findings.iter().map(ToString::to_string).collect::<Vec<_>>(),
        [
            "SRC-SCHEMA src.democmm-program-native.export_route.format: dmis is not a registered format",
            "SRC-SCHEMA src.democmm-results-qif.format_id: qif is not a registered format",
            "SRC-SCHEMA src.demoscan-form-native.export_route.format: results-csv is not a registered format",
            "SRC-SCHEMA src.demoscan-points-csv.format_id: points-csv is not a registered format",
        ]
    );
    for bad in [&b"[]"[..], br#"{"formats":{}}"#, br#"{"formats":[{"id":"Bad Id"}]}"#, br#"{"formats":["qif"]}"#] {
        let r = check(&fixture("register-ok.json"), bad);
        assert_eq!(r.findings.len(), 1, "{:?}", r.findings);
        assert!(r.findings[0].to_string().starts_with("SRC-SCHEMA manifest"), "{:?}", r.findings);
    }
}

#[test]
fn links_are_refused() {
    let f = lines("link-note.json");
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].starts_with("SRC-SCHEMA entries[1].export_route.note"), "{f:?}");
}

#[test]
fn review_needs_every_field_and_a_real_date() {
    let text = String::from_utf8(fixture("register-ok.json")).unwrap();
    let broken = text.replacen("\"date\": \"2026-11-20\"", "\"date\": \"2026-02-30\"", 1);
    let r = check(broken.as_bytes(), &manifest());
    assert_eq!(
        r.findings.iter().map(ToString::to_string).collect::<Vec<_>>(),
        ["SRC-REVIEW src.demoscan-points-csv missing or invalid date"]
    );
    let broken = text.replacen("\"reviewer\": \"maintainer-a\"", "\"reviewer\": \"\"", 1);
    assert_eq!(check(broken.as_bytes(), &manifest()).findings[0].code(), "SRC-REVIEW");
}

#[test]
fn hostile_input_is_refused() {
    for bad in [
        &b"[]"[..],
        b"{\"schema\":\"mig-sources@1\",\"schema\":\"x\"}",
        b"{\"schema\":\"mig-sources@1\",\"version\":\"1.0.0\",\"policy\":\"D-033-A\",\"entries\":[],\"x\":1}",
        b"{\"schema\":\"mig-sources@1\",\"version\":\"1.0.0\",\"policy\":\"D-033-C\",\"entries\":[]}",
        b"\xff",
    ] {
        let r = check(bad, &manifest());
        assert!(r.register.is_none());
        assert!(r.findings.iter().all(|f| f.code() == "SRC-SCHEMA"), "{:?}", r.findings);
    }
    let deep = format!("{}{}", "[".repeat(64), "]".repeat(64));
    assert!(load(deep.as_bytes()).is_err());
    let huge = vec![b' '; fid_mig::src::reg::MAX_REGISTER_BYTES + 1];
    assert!(load(&huge).is_err());
}

#[test]
fn ac5_not_admitted_listing() {
    let reg = load(&fixture("register-ok.json")).unwrap();
    let list = list_sources(&reg, &ListSources { status: Some(Status::NotAdmitted), ..ListSources::default() });
    let ids: Vec<&str> = list.entries.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["src.democmm-program-native", "src.demoscan-form-native"]);
    assert!(list.entries.iter().all(|e| e.export_route.is_some()));
    let got = fid_canon::to_canonical_bytes(&list.to_value()).unwrap();
    let want = fixture("expected/list-not-admitted.json");
    assert_eq!(String::from_utf8(got).unwrap(), String::from_utf8(want).unwrap().trim_end());
    // Through the command entry point too.
    let v = run_list_sources(&fixture("register-ok.json"), &params(r#"{"status":"NOT_ADMITTED"}"#)).unwrap();
    assert_eq!(v, list.to_value());
}

#[test]
fn ac6_page_is_byte_identical() {
    let a = run_list_sources(&fixture("register-ok.json"), &params(r#"{"format":"md"}"#)).unwrap();
    let b = run_list_sources(&fixture("register-ok.json"), &params(r#"{"format":"md"}"#)).unwrap();
    assert_eq!(a, b);
    let CValue::Obj(o) = &a else { panic!() };
    let Some(CValue::Str(page)) = o.get("page") else { panic!("no page") };
    assert_eq!(page.as_bytes(), &fixture("expected/sources.md")[..]);
}

#[test]
fn params_fail_closed() {
    let reg = fixture("register-ok.json");
    for bad in [r#"[]"#, r#"{"kind":"programs"}"#, r#"{"status":"admitted"}"#, r#"{"format":"html"}"#, r#"{"x":1}"#] {
        assert!(matches!(run_list_sources(&reg, &params(bad)), Err(ListError::BadParams(_))), "{bad}");
    }
    assert!(matches!(run_list_sources(b"{}", &params("{}")), Err(ListError::Register(_))));
}

#[test]
fn embedded_register_passes_the_check() {
    let reg = embedded().unwrap();
    // The workspace manifest has no format catalogue yet; the empty register needs none.
    assert!(check(fid_mig::src::reg::EMBEDDED, br#"{"commands":[]}"#).is_ok());
    assert_eq!(reg.policy().as_str(), "D-033-A");
    // The command over the embedded register lists it (moved from the removed `fid` CLI test).
    let v = run_list_sources(fid_mig::src::reg::EMBEDDED, &params("{}")).unwrap();
    let text = String::from_utf8(fid_canon::to_canonical_bytes(&v).unwrap()).unwrap();
    assert!(text.contains("\"policy\":\"D-033-A\""), "{text}");
}

#[test]
fn command_definition_and_schemas() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 1);
    let def = &defs[0];
    assert_eq!(format!("{}@{}", def.id, def.major), LIST_SOURCES);
    let p = Compiled::compile(&def.params_schema).unwrap();
    let r = Compiled::compile(&def.result_schema).unwrap();
    let reg = fixture("register-ok.json");
    for text in [r#"{}"#, r#"{"kind":"program"}"#, r#"{"status":"NOT_ADMITTED","format":"md"}"#] {
        let v = params(text);
        assert_valid(&p, &v, text);
        assert_valid(&r, &run_list_sources(&reg, &v).unwrap(), text);
    }
    assert!(p.evaluate(&params(r#"{"format":"html"}"#), &NoHooks).is_err());
    assert!(p.evaluate(&params(r#"{"x":1}"#), &NoHooks).is_err());
}
