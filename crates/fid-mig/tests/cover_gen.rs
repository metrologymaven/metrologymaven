//! Acceptance criteria of spec leaf MIG.COVER.GEN through the command function
//! `run_generate_coverage` and the library: AC-1 (csv-points table), AC-2 (program table,
//! GOTO in a DIVERGENT program), AC-3 (foreign report), AC-4 (drift), AC-5 (byte-identical
//! output, compared with committed bytes on every CI platform), plus the other error codes and
//! the registry definition (validated with `fid_bus::validate::Compiled`). Fixtures and
//! expected tables come from `fixtures/mig/cover/gen/make_fixtures.py`, an independent
//! reference that never runs this code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::CValue;
use fid_io_reg::{FormatId, Principal, RootAccess, RootId, RootSet};
use fid_mig::cover::gen::{
    self, check, command_def, generate, run_generate_coverage, Drift, GenError, Inputs, RowStatus, GENERATED_LINE,
    GENERATE_COVERAGE,
};
use fid_mig::cover::taxo::SemVer;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mig/cover/gen")
}

fn read(name: &str) -> Vec<u8> {
    let p = dir().join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn roots() -> RootSet {
    let mut r = RootSet::new();
    r.insert(RootId::new("fx").unwrap(), RootAccess::Import, dir());
    r
}

fn params(text: &str) -> CValue {
    fid_canon::parse(text.as_bytes()).unwrap()
}

fn base_params(format: &str, corpus: &str, reports: &str) -> String {
    format!(
        r#"{{"format":"{format}","corpus":"root://fx/{format}/{corpus}","taxonomy":"root://fx/{format}/taxo.json","reports":"root://fx/{format}/{reports}","release":"0.1.0"}}"#
    )
}

fn run(text: &str) -> Result<CValue, GenError> {
    run_generate_coverage(&roots(), Principal::Agent, &params(text))
}

fn field<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(key).unwrap_or_else(|| panic!("missing {key}")),
        _ => panic!("not an object"),
    }
}

fn text(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("not a string"),
    }
}

fn release() -> SemVer {
    SemVer::parse("0.1.0").unwrap()
}

fn format(f: &str) -> FormatId {
    FormatId::new(f).unwrap()
}

/// Generates from in-memory inputs of the csv-points fixture, with `reports` replaced.
fn csv_with_reports(reports: &[u8]) -> Result<gen::CoverageTable, GenError> {
    let f = format("csv-points");
    generate(&Inputs {
        format: &f,
        release: release(),
        corpus: &read("csv-points/corpus.json"),
        taxonomy: &read("csv-points/taxo.json"),
        reports,
    })
}

/// The csv-points report set, parsed, with `edit` applied to its `reports` array.
fn edited_reports(edit: impl FnOnce(&mut Vec<CValue>)) -> Vec<u8> {
    let mut v = fid_canon::parse(&read("csv-points/reports.json")).unwrap();
    if let CValue::Obj(o) = &mut v {
        if let Some(CValue::Arr(items)) = o.get_mut("reports") {
            edit(items);
        }
    }
    fid_canon::to_canonical_bytes(&v).unwrap()
}

#[test]
fn ac1_csv_points_table_equals_expected_bytes() {
    let out = run(&base_params("csv-points", "corpus.json", "reports.json")).unwrap();
    let table = field(&out, "table");
    assert_eq!(fid_canon::to_canonical_bytes(table).unwrap(), read("csv-points/expected/coverage-csv-points.json"));
    assert_eq!(text(field(&out, "markdown")).as_bytes(), read("csv-points/expected/coverage-csv-points.md"));
    let CValue::Arr(rows) = field(table, "rows") else { panic!("rows") };
    let statuses: Vec<&str> = rows.iter().map(|r| text(field(r, "status"))).collect();
    assert_eq!(statuses, ["SUPPORTED", "SUPPORTED", "PARTIAL", "SUPPORTED", "UNSUPPORTED", "UNTESTED"]);
    assert!(matches!(out, CValue::Obj(ref o) if !o.contains_key("check")));
    // Admission is not checked by this command, and private_run is self-declared: both say so.
    assert_eq!(text(field(&out, "admission")), "not_checked");
    let header = field(table, "header");
    assert_eq!(text(field(header, "admission")), "not_checked");
    assert_eq!(text(field(header, "private_run_basis")), "self_declared_unverified");
    let md = text(field(&out, "markdown"));
    assert!(
        md.contains(
            "
- Private run: no (self-declared in the corpus manifest, unverified)
"
        ),
        "{md}"
    );
    assert!(md.contains(
        "
- Corpus and taxonomy admission: not checked by this command; see `cargo xtask mig corpus-check`/`taxonomy-check`
"
    ));
}

#[test]
fn ac1_library_output_matches_the_command() {
    let t = csv_with_reports(&read("csv-points/reports.json")).unwrap();
    assert_eq!(t.to_json().unwrap(), read("csv-points/expected/coverage-csv-points.json"));
    assert_eq!(t.render_md().as_bytes(), read("csv-points/expected/coverage-csv-points.md"));
    assert!(!t.program);
    assert_eq!(t.rows.iter().filter(|r| r.status == RowStatus::Untested).count(), 1);
    assert!(t.rows.iter().all(|r| r.programs_equivalent.is_none() && r.programs_not_equivalent.is_none()));
    assert_eq!(t.header.reports.len(), 4);
}

#[test]
fn ac2_goto_in_a_divergent_program_is_partial() {
    let out = run(&base_params("dmis-prog", "corpus.json", "reports.json")).unwrap();
    let table = field(&out, "table");
    assert_eq!(fid_canon::to_canonical_bytes(table).unwrap(), read("dmis-prog/expected/coverage-dmis-prog.json"));
    assert_eq!(text(field(&out, "markdown")).as_bytes(), read("dmis-prog/expected/coverage-dmis-prog.md"));
    let CValue::Arr(rows) = field(table, "rows") else { panic!("rows") };
    let goto = rows.iter().find(|r| text(field(r, "construct_id")) == "stmt.goto").unwrap();
    assert_eq!(text(field(goto, "status")), "PARTIAL");
    assert_eq!(field(goto, "partial"), &CValue::Num(0.0));
    assert_eq!(field(goto, "unsupported"), &CValue::Num(0.0));
    assert_eq!(field(goto, "programs_not_equivalent"), &CValue::Num(1.0));
    let feat = rows.iter().find(|r| text(field(r, "construct_id")) == "stmt.feat").unwrap();
    assert_eq!(text(field(feat, "status")), "SUPPORTED");
}

#[test]
fn ac3_foreign_report_fails_and_outputs_no_table() {
    let err = run(&base_params("csv-points", "corpus.json", "foreign-reports.json")).unwrap_err();
    assert_eq!(err.code(), "coverage_foreign_report");
    match &err {
        GenError::ForeignReport { report, .. } => assert_eq!(report, "mig-csv-stray.mapreport.json"),
        other => panic!("{other}"),
    }
    assert!(err.to_string().contains("mig-csv-stray.mapreport.json"));
}

fn check_params(dir: &str, release: &str) -> String {
    format!(
        r#"{{"format":"csv-points","corpus":"root://fx/csv-points/corpus.json","taxonomy":"root://fx/csv-points/taxo.json","reports":"root://fx/csv-points/reports.json","release":"{release}","check":"root://fx/csv-points/{dir}/coverage-csv-points.json","check_md":"root://fx/csv-points/{dir}/coverage-csv-points.md"}}"#
    )
}

#[test]
fn ac4_hand_edited_count_is_drift_diff_with_the_line() {
    let err = run(&check_params("drift", "0.1.0")).unwrap_err();
    assert_eq!(err.code(), "DRIFT-DIFF");
    let expected = String::from_utf8(read("csv-points/drift/expected-drift.txt")).unwrap();
    assert_eq!(err.to_string(), expected.trim_end());
}

#[test]
fn ac4_committed_table_that_matches_passes_check() {
    let out = run(&check_params("expected", "0.1.0")).unwrap();
    // A match carries the admission status; there is no bare `checked: true`.
    let c = field(&out, "check");
    assert_eq!(text(field(c, "result")), "match");
    assert_eq!(text(field(c, "admission")), "not_checked");
    assert!(matches!(out, CValue::Obj(ref o) if !o.contains_key("checked")));
}

#[test]
fn drift_input_when_the_committed_header_names_another_release() {
    let err = run(&check_params("expected", "0.2.0")).unwrap_err();
    assert_eq!(err.code(), "DRIFT-INPUT");
    assert_eq!(err.to_string(), "DRIFT-INPUT coverage-csv-points.json release");
}

#[test]
fn drift_input_and_header_through_the_library() {
    let t = csv_with_reports(&read("csv-points/reports.json")).unwrap();
    let json = read("csv-points/expected/coverage-csv-points.json");
    let md = read("csv-points/expected/coverage-csv-points.md");
    assert!(check(&t, &json, &md).is_ok());
    // Another set of reports (one fewer): the committed header names other inputs.
    let fewer = csv_with_reports(&edited_reports(|r| {
        r.pop();
    }))
    .unwrap();
    match check(&fewer, &json, &md) {
        Err(GenError::Drift(d)) => {
            assert_eq!(d, vec![Drift::Input { file: "coverage-csv-points.json".into(), field: "reports" }], "{d:?}")
        }
        other => panic!("{other:?}"),
    }
    // A Markdown file without the generated line, and JSON that does not parse.
    let stripped = &md[GENERATED_LINE.len() + 1..];
    match check(&t, b"{", stripped) {
        Err(GenError::Drift(d)) => {
            let codes: Vec<String> = d.iter().map(ToString::to_string).collect();
            assert_eq!(
                codes,
                [
                    "DRIFT-HEADER coverage-csv-points.json not valid JSON",
                    "DRIFT-HEADER coverage-csv-points.md no generated header"
                ]
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn ac5_output_is_byte_identical_and_independent_of_report_order() {
    let a = csv_with_reports(&read("csv-points/reports.json")).unwrap();
    let b = csv_with_reports(&read("csv-points/reports.json")).unwrap();
    assert_eq!(a.to_json().unwrap(), b.to_json().unwrap());
    assert_eq!(a.render_md(), b.render_md());
    let reversed = csv_with_reports(&edited_reports(|r| r.reverse())).unwrap();
    assert_eq!(reversed.to_json().unwrap(), read("csv-points/expected/coverage-csv-points.json"));
    assert_eq!(reversed.render_md().as_bytes(), read("csv-points/expected/coverage-csv-points.md"));
}

#[test]
fn private_corpus_sets_private_run() {
    let out = run(&base_params("csv-points", "private-corpus.json", "reports.json")).unwrap();
    let table = field(&out, "table");
    assert_eq!(field(field(table, "header"), "private_run"), &CValue::Bool(true));
    assert_eq!(
        fid_canon::to_canonical_bytes(table).unwrap(),
        read("csv-points/expected-private/coverage-csv-points.json")
    );
    assert_eq!(text(field(&out, "markdown")).as_bytes(), read("csv-points/expected-private/coverage-csv-points.md"));
}

fn set_construct(r: &mut [CValue], construct: &str) {
    if let Some(CValue::Obj(entry)) = r.first_mut() {
        if let Some(CValue::Obj(report)) = entry.get_mut("report") {
            if let Some(CValue::Arr(items)) = report.get_mut("items") {
                if let Some(CValue::Obj(item)) = items.first_mut() {
                    item.insert("construct_id".into(), CValue::Str(construct.into()));
                }
            }
        }
    }
}

#[test]
fn unknown_construct_is_refused() {
    let err = csv_with_reports(&edited_reports(|r| set_construct(r, "point_set.weighted"))).unwrap_err();
    assert_eq!(err.code(), "coverage_unknown_construct");
    match err {
        GenError::UnknownConstruct { report, construct } => {
            assert_eq!(report, "mig-csv-demo-1.mapreport.json");
            assert_eq!(construct, "point_set.weighted");
        }
        other => panic!("{other}"),
    }
}

#[test]
fn two_reports_for_one_corpus_file_are_refused() {
    let err = csv_with_reports(&edited_reports(|r| {
        let mut copy = r[0].clone();
        if let CValue::Obj(o) = &mut copy {
            o.insert("name".into(), CValue::Str("again.json".into()));
        }
        r.push(copy);
    }))
    .unwrap_err();
    assert_eq!(err.code(), "coverage_schema");
    assert!(err.to_string().contains("both item (mapping or fill) reports for corpus file"), "{err}");
}

/// The corpus digest the first csv-points report was produced from.
fn first_source() -> String {
    let v = fid_canon::parse(&read("csv-points/reports.json")).unwrap();
    let CValue::Arr(r) = field(&v, "reports") else { panic!("reports") };
    text(field(field(field(&r[0], "report"), "header"), "source_sha256")).to_string()
}

fn entry(text: &str) -> CValue {
    fid_canon::parse(text.as_bytes()).unwrap()
}

#[test]
fn a_mapping_and_a_fill_report_for_one_file_are_refused() {
    let src = first_source();
    let fill = format!(
        r#"{{"name":"demo-1.fill.json","report":{{"schema":"mig-fillreport@1","source_workbook_sha256":"{src}","items":[{{"construct_id":"column.header","status":"MAPPED"}}]}}}}"#
    );
    let err = csv_with_reports(&edited_reports(|r| r.push(entry(&fill)))).unwrap_err();
    assert_eq!(err.code(), "coverage_schema");
}

#[test]
fn one_item_report_plus_one_equivalence_report_per_file() {
    let src = first_source();
    // The equivalence report's own items are not counted when an item report exists.
    let eq = |name: &str, verdict: &str| {
        entry(&format!(
            r#"{{"name":"{name}","report":{{"schema":"mig-equivalence@1","header":{{"format_id":"csv-points","source_sha256":"{src}"}},"verdict":"{verdict}","items":[{{"construct_id":"comment.line","status":"MAPPED"}}]}}}}"#
        ))
    };
    let t = csv_with_reports(&edited_reports(|r| r.push(eq("demo-1.eq.json", "EQUIVALENT")))).unwrap();
    assert!(t.program);
    let row = |id: &str| t.rows.iter().find(|r| r.construct_id == id).unwrap().clone();
    // comment.line only appears in the equivalence report's items: still UNTESTED.
    assert_eq!(row("comment.line").status, RowStatus::Untested);
    assert_eq!(row("comment.line").mapped, 0);
    // column.header: 4 MAPPED; file 1 is EQUIVALENT, files 2..4 have no equivalence report.
    let h = row("column.header");
    assert_eq!((h.mapped, h.programs_equivalent, h.programs_not_equivalent), (4, Some(1), Some(3)));
    assert_eq!(h.status, RowStatus::Partial);
    assert_eq!(t.header.reports.len(), 5);
    // A second equivalence report for the same file is refused.
    let err = csv_with_reports(&edited_reports(|r| {
        r.push(eq("demo-1.eq.json", "EQUIVALENT"));
        r.push(eq("demo-1.eq2.json", "DIVERGENT"));
    }))
    .unwrap_err();
    assert_eq!(err.code(), "coverage_schema");
    assert!(err.to_string().contains("both equivalence reports"), "{err}");
}

#[test]
fn mixed_program_set_takes_items_from_mapping_and_verdicts_from_equivalence() {
    let out = run(&base_params("dmis-prog", "corpus.json", "mixed-reports.json")).unwrap();
    let table = field(&out, "table");
    assert_eq!(fid_canon::to_canonical_bytes(table).unwrap(), read("dmis-prog/expected-mixed/coverage-dmis-prog.json"));
    assert_eq!(text(field(&out, "markdown")).as_bytes(), read("dmis-prog/expected-mixed/coverage-dmis-prog.md"));
}

#[test]
fn fill_reports_feed_a_form_table() {
    let out = run(&base_params("form-xlsx", "corpus.json", "reports.json")).unwrap();
    let table = field(&out, "table");
    assert_eq!(fid_canon::to_canonical_bytes(table).unwrap(), read("form-xlsx/expected/coverage-form-xlsx.json"));
    assert_eq!(text(field(&out, "markdown")).as_bytes(), read("form-xlsx/expected/coverage-form-xlsx.md"));
}

#[test]
fn malformed_inputs_are_coverage_schema() {
    for bad in [
        &b"[]"[..],
        br#"{"schema":"mig-report-set@1"}"#,
        br#"{"schema":"other@1","reports":[]}"#,
        br#"{"schema":"mig-report-set@1","reports":[],"extra":1}"#,
        br#"{"schema":"mig-report-set@1","reports":[{"name":"A.json","report":{}}]}"#,
        br#"{"schema":"mig-report-set@1","reports":[{"name":"a.json","report":{"schema":"x@1"}}]}"#,
        br#"{"schema":"mig-report-set@1","reports":[{"name":"a.json","report":{"schema":"mig-mapreport@1","header":{"format_id":"qif","source_sha256":"sha256:00"},"items":[]}}]}"#,
    ] {
        let err = csv_with_reports(bad).unwrap_err();
        assert_eq!(err.code(), "coverage_schema", "{}", String::from_utf8_lossy(bad));
    }
    // An item status outside MAPPED, PARTIAL and UNSUPPORTED.
    let err = csv_with_reports(&edited_reports(|r| {
        if let Some(CValue::Obj(entry)) = r.first_mut() {
            if let Some(CValue::Obj(report)) = entry.get_mut("report") {
                if let Some(CValue::Arr(items)) = report.get_mut("items") {
                    if let Some(CValue::Obj(item)) = items.first_mut() {
                        item.insert("status".into(), CValue::Str("DONE".into()));
                    }
                }
            }
        }
    }))
    .unwrap_err();
    assert_eq!(err.code(), "coverage_schema");
    // A taxonomy or corpus for another format.
    let err =
        run(&base_params("csv-points", "corpus.json", "reports.json").replace("csv-points/taxo", "dmis-prog/taxo"))
            .unwrap_err();
    assert_eq!(err.code(), "coverage_schema");
    // An empty report set gives an all-UNTESTED table.
    let t = csv_with_reports(br#"{"schema":"mig-report-set@1","reports":[]}"#).unwrap();
    assert!(t.rows.iter().all(|r| r.status == RowStatus::Untested));
    assert!(t.render_md().contains("- Input reports: none\n"));
}

#[test]
fn handles_and_params_fail_closed() {
    let raw = base_params("csv-points", "corpus.json", "reports.json")
        .replace("root://fx/csv-points/corpus.json", "/etc/corpus.json");
    assert_eq!(run(&raw).unwrap_err().code(), "raw_path_forbidden");
    let outside = base_params("csv-points", "corpus.json", "reports.json")
        .replace("root://fx/csv-points/corpus.json", "root://fx/../corpus.json");
    assert_eq!(run(&outside).unwrap_err().code(), "path_outside_root");
    for bad in [
        r#"[]"#.to_string(),
        r#"{}"#.to_string(),
        base_params("csv-points", "corpus.json", "reports.json").replace("0.1.0", "0.1"),
        base_params("csv-points", "corpus.json", "reports.json")
            .replace(r#""format":"csv-points""#, r#""format":"CSV""#),
        base_params("csv-points", "corpus.json", "reports.json").replace('}', r#","verbose":true}"#),
        base_params("csv-points", "corpus.json", "reports.json")
            .replace('}', r#","check":"root://fx/csv-points/expected/coverage-csv-points.json"}"#),
    ] {
        assert_eq!(run(&bad).unwrap_err().code(), "bad_params", "{bad}");
    }
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
fn command_definition_and_schemas() {
    let def = command_def().unwrap();
    assert_eq!(format!("{}@{}", def.id, def.major), GENERATE_COVERAGE);
    let ids: Vec<String> = fid_mig::command_defs().unwrap().iter().map(|d| format!("{}@{}", d.id, d.major)).collect();
    assert_eq!(ids, ["migrate.list_sources@1", GENERATE_COVERAGE]);
    let p = Compiled::compile(&def.params_schema).unwrap();
    let r = Compiled::compile(&def.result_schema).unwrap();
    for text in [
        base_params("csv-points", "corpus.json", "reports.json"),
        base_params("dmis-prog", "corpus.json", "reports.json"),
        check_params("expected", "0.1.0"),
        base_params("dmis-prog", "corpus.json", "mixed-reports.json"),
        base_params("form-xlsx", "corpus.json", "reports.json"),
    ] {
        assert_valid(&p, &params(&text), &text);
        let out = run(&text).unwrap();
        assert_valid(&r, &out, &text);
    }
    assert!(p.evaluate(&params(r#"{"format":"csv-points"}"#), &NoHooks).is_err());
}
