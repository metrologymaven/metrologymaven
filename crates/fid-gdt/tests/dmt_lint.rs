//! Integration tests for spec node `GDT.RES.LINT` (AC-1 to AC-6, REQ-001 to REQ-006). The
//! expected findings come from the independent Python reference
//! `fixtures/gdt/lint/make_cases.py`; schema refusals are hand-labelled there from
//! `GDT.RES.SCHEMA`'s rules. The first test is the CI target over the committed synthetic DMT
//! and ORS index.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_gdt::dmt_lint::{
    case_id_ok, is_clause_number, lint, lint_bytes, lint_table, LintError, Mode, NotRunReason, OrsIndex, Rule,
    SchemaCheck, Severity, FREE_TEXT_FIELDS, FREE_TEXT_LIMIT, L5_BASIS, REPORT_SCHEMA,
};
use fid_gps::dmt::DmtTable;
use std::collections::BTreeMap;
use std::fs;

const LINT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/gdt/lint/");

fn read(name: &str) -> Vec<u8> {
    fs::read(format!("{LINT_DIR}{name}")).unwrap()
}

fn index() -> OrsIndex {
    OrsIndex::load(&read("ors_index.json")).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("expected object, got {other:?}"),
    }
}

fn s<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match m.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("expected string at {k}, got {other:?}"),
    }
}

fn arr<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> &'a [CValue] {
    match m.get(k) {
        Some(CValue::Arr(a)) => a,
        other => panic!("expected array at {k}, got {other:?}"),
    }
}

fn strs(a: &[CValue]) -> Vec<String> {
    a.iter()
        .map(|v| match v {
            CValue::Str(s) => s.clone(),
            other => panic!("expected string, got {other:?}"),
        })
        .collect()
}

fn names(rules: &[Rule]) -> Vec<String> {
    rules.iter().map(|r| r.as_str().to_owned()).collect()
}

fn not_run(report: &fid_gdt::dmt_lint::LintReport) -> Vec<(String, String)> {
    report.rules_not_run().iter().map(|(r, why)| (r.as_str().to_owned(), why.as_str().to_owned())).collect()
}

fn num(v: Option<&CValue>) -> usize {
    match v {
        Some(CValue::Num(n)) => format!("{n}").parse().unwrap(),
        other => panic!("expected a count, got {other:?}"),
    }
}

fn mode_of(m: &BTreeMap<String, CValue>) -> Mode {
    Mode::parse(s(m, "mode")).unwrap()
}

fn cases() -> Vec<BTreeMap<String, CValue>> {
    let v = parse(&read("expected.json")).unwrap();
    arr(obj(&v), "cases").iter().map(|c| obj(c).clone()).collect()
}

fn case(name: &str) -> BTreeMap<String, CValue> {
    cases().into_iter().find(|c| s(c, "case") == name).unwrap()
}

fn triples(report: &fid_gdt::dmt_lint::LintReport) -> Vec<(String, String, String)> {
    report.findings().iter().map(|f| (f.rule().as_str().to_owned(), f.key().to_owned(), f.field().to_owned())).collect()
}

fn run(c: &BTreeMap<String, CValue>) -> fid_gdt::dmt_lint::LintReport {
    lint_bytes(&read(s(c, "table")), &index(), mode_of(c)).unwrap()
}

// ---------------------------------------------------------------------------- CI target / AC-1

#[test]
fn ci_target_committed_synthetic_dmt_lints_clean_in_draft_mode() {
    let report = lint_bytes(&read("../dmt/synthetic_table_v1.json"), &index(), Mode::Draft).unwrap();
    assert!(report.findings().is_empty(), "findings: {:?}", report.findings());
    assert!(matches!(report.schema(), SchemaCheck::Ok { .. }));
    assert_eq!(report.rules_run(), &[Rule::L1, Rule::L2, Rule::L3, Rule::L4]);
    assert!(report.rules_not_run().is_empty());
    assert!(report.passed());
    assert_eq!(report.exit_code(), 0);
}

// ---------------------------------------------------------------------------- every oracle case

#[test]
fn every_case_matches_the_python_reference() {
    let all = cases();
    assert_eq!(all.len(), 16);
    for c in &all {
        let name = s(c, "case");
        let report = run(c);
        let want: Vec<(String, String, String)> = arr(c, "findings")
            .iter()
            .map(|f| {
                let f = obj(f);
                (s(f, "rule").to_owned(), s(f, "key").to_owned(), s(f, "field").to_owned())
            })
            .collect();
        assert_eq!(triples(&report), want, "{name}: findings");
        assert_eq!(names(report.rules_run()), strs(arr(c, "rules_run")), "{name}: rules_run");
        let want_not_run: Vec<(String, String)> = arr(c, "rules_not_run")
            .iter()
            .map(|x| (s(obj(x), "rule").to_owned(), s(obj(x), "reason").to_owned()))
            .collect();
        assert_eq!(not_run(&report), want_not_run, "{name}: rules_not_run");
        let checks = obj(c.get("checks").unwrap());
        for rule in Rule::ALL {
            match (report.check(rule), checks.get(rule.as_str())) {
                (Some(got), Some(want)) => {
                    let want = obj(want);
                    assert_eq!(got.checked(), num(want.get("checked")), "{name}: {rule} checked");
                    assert_eq!(got.skipped(), num(want.get("skipped")), "{name}: {rule} skipped");
                }
                (None, None) => {}
                (got, want) => panic!("{name}: {rule} counts {got:?} vs {want:?}"),
            }
        }
        assert!(report.findings().iter().all(|f| f.severity() == Severity::Gate), "{name}: severity");
        let sc = obj(c.get("schema_check").unwrap());
        match (report.schema(), s(sc, "status")) {
            (SchemaCheck::Ok { .. }, "ok") => {}
            (SchemaCheck::Refused { code, reason }, "refused") => {
                assert_eq!(*code, s(sc, "code"), "{name}: refusal code");
                assert_eq!(*reason, Some(s(sc, "reason")), "{name}: refusal reason");
            }
            (got, want) => panic!("{name}: schema check {got:?}, expected {want}"),
        }
        assert_eq!(usize::try_from(report.exit_code()).unwrap(), num(c.get("exit_code")), "{name}: exit code");
        assert_eq!(report.passed(), report.exit_code() == 0, "{name}: passed");
        assert_eq!(report.mode(), mode_of(c));
        assert_eq!(report.ors_index_digest(), index().digest());
    }
}

#[test]
fn typed_lint_agrees_with_lint_bytes_on_every_loadable_case() {
    for c in cases() {
        if s(obj(c.get("schema_check").unwrap()), "status") != "ok" {
            continue;
        }
        let table = DmtTable::load(&read(s(&c, "table"))).unwrap();
        let report = run(&c);
        assert_eq!(lint(&table, &index(), mode_of(&c)), report.findings(), "{}", s(&c, "case"));
        assert_eq!(lint_table(&table, &index(), mode_of(&c)), report, "{}", s(&c, "case"));
        match report.schema() {
            SchemaCheck::Ok { table_digest } => assert_eq!(*table_digest, table.digest()),
            other => panic!("{other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------- AC-2 (L1)

#[test]
fn ac2_long_text_fires_l1_naming_row_and_field_without_echoing_the_text() {
    let report = run(&case("long_text"));
    let [f] = report.findings() else { panic!("{:?}", report.findings()) };
    assert_eq!(f.rule(), Rule::L1);
    assert_eq!(f.key(), "dmt:asme/flatness/none@1");
    assert_eq!(f.field(), "notes");
    assert!(f.detail().contains("300") && f.detail().contains(&FREE_TEXT_LIMIT.to_string()), "{}", f.detail());
    assert!(!f.detail().contains("Lorem") && !f.detail().contains("ipsum"), "{}", f.detail());
    // The schema refuses the same row, so L3 to L5 never ran and the table cannot pass.
    assert_eq!(report.schema(), &SchemaCheck::Refused { code: "field_too_long", reason: Some("notes") });
    assert_eq!(
        report.rules_not_run(),
        &[(Rule::L3, NotRunReason::SchemaRefused), (Rule::L4, NotRunReason::SchemaRefused)]
    );
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn l1_boundary_280_passes_and_281_fires() {
    assert!(run(&case("notes_280")).passed());
    let report = run(&case("notes_281"));
    assert_eq!(triples(&report), vec![("L1".into(), "dmt:asme/flatness/none@1".into(), "notes".into())]);
}

// ---------------------------------------------------------------------------- AC-3 (L3, L4)

#[test]
fn ac3_supported_without_ors_fires_l3_and_dangling_link_fires_l4() {
    let r3 = run(&case("supported_no_ors"));
    assert_eq!(triples(&r3), vec![("L3".into(), "dmt:asme/straightness/none@1".into(), "ors_cases".into())]);
    assert_eq!(r3.exit_code(), 1);
    let r4 = run(&case("dangling_ors"));
    let [f] = r4.findings() else { panic!("{:?}", r4.findings()) };
    assert_eq!((f.rule(), f.key(), f.field()), (Rule::L4, "dmt:asme/size/none@1", "ors_cases[0]"));
    assert!(f.detail().contains("ors:synthetic/missing.001"), "{}", f.detail());
    assert_eq!(r4.exit_code(), 1);
}

// ---------------------------------------------------------------------------- AC-4 (L2)

#[test]
fn ac4_bad_clause_fires_l2_even_though_the_schema_refuses_it() {
    let report = run(&case("bad_clause"));
    let [f] = report.findings() else { panic!("{:?}", report.findings()) };
    assert_eq!((f.rule(), f.key(), f.field()), (Rule::L2, "dmt:asme/angularity/none@1", "clauses[0].clause"));
    assert!(!f.detail().contains("see text"), "{}", f.detail());
    assert_eq!(report.schema(), &SchemaCheck::Refused { code: "malformed_table", reason: Some("bad_clause") });
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn l2_fires_on_a_clause_the_schema_accepts() {
    let report = run(&case("bad_clause_schema_ok"));
    assert!(matches!(report.schema(), SchemaCheck::Ok { .. }));
    assert_eq!(triples(&report), vec![("L2".into(), "dmt:asme/angularity/none@1".into(), "clauses[0].clause".into())]);
}

#[test]
fn clause_number_pattern() {
    for ok in ["1", "90.2", "1a", "1.2a", "10.20.30", "0"] {
        assert!(is_clause_number(ok), "{ok}");
    }
    for bad in ["", "a", "1.", ".1", "1..2", "1a.2", "see text", "1A", "1ab", "1 ", "a.b", "x.1", "\u{0661}", "1-2"] {
        assert!(!is_clause_number(bad), "{bad:?}");
    }
}

// ---------------------------------------------------------------------------- AC-5 (L5)

#[test]
fn ac5_release_mode_on_draft_rows_fires_l5_for_every_row_and_exits_non_zero() {
    let c = case("release_draft");
    let table = DmtTable::load(&read(s(&c, "table"))).unwrap();
    let report = run(&c);
    let mut keys: Vec<&str> = table.rows().keys().map(|k| k.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(report.findings().len(), keys.len());
    for (f, key) in report.findings().iter().zip(keys) {
        assert_eq!((f.rule(), f.key(), f.field()), (Rule::L5, key, "review"));
    }
    assert_eq!(report.exit_code(), 1);
    // The same rows in draft mode: L5 does not run.
    let draft = lint_bytes(&read(s(&c, "table")), &index(), Mode::Draft).unwrap();
    assert!(draft.passed());
}

#[test]
fn release_mode_on_the_synthetic_table_fires_for_rows_not_signed_off_by_two() {
    let report = run(&case("synthetic_release"));
    assert_eq!(report.findings().len(), 17);
    assert!(report.findings().iter().all(|f| f.rule() == Rule::L5));
    assert!(!report.findings().iter().any(|f| f.key() == "dmt:asme/flatness/none@1"));
}

#[test]
fn l5_needs_two_distinct_reviewers() {
    let report = run(&case("signed_one"));
    let [f] = report.findings() else { panic!("{:?}", report.findings()) };
    assert_eq!((f.rule(), f.key()), (Rule::L5, "dmt:asme/flatness/none@1"));
    assert!(f.detail().contains("1 distinct"), "{}", f.detail());
    assert!(run(&case("release_clean")).passed());
}

// ---------------------------------------------------------------------------- AC-6 (determinism)

#[test]
fn ac6_two_runs_give_byte_identical_sorted_reports() {
    for c in cases() {
        let a = run(&c).canonical_bytes().unwrap();
        let b = run(&c).canonical_bytes().unwrap();
        assert_eq!(a, b, "{}", s(&c, "case"));
    }
    let report = run(&case("mixed"));
    let f = report.findings();
    assert!(f.windows(2).all(|w| (w[0].key(), w[0].rule(), w[0].field()) <= (w[1].key(), w[1].rule(), w[1].field())));
    assert_eq!(f.len(), 21);
}

// ---------------------------------------------------------------------------- report wire form

#[test]
fn report_json_carries_every_required_field_and_the_bases() {
    let report = run(&case("long_text"));
    let v = parse(&report.canonical_bytes().unwrap()).unwrap();
    let m = obj(&v);
    let keys: Vec<&str> = m.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "checks",
            "findings",
            "l1_threshold",
            "l5_basis",
            "mode",
            "ors_index_digest",
            "passed",
            "rules_not_run",
            "rules_run",
            "schema",
            "schema_check"
        ]
    );
    assert_eq!(s(m, "schema"), REPORT_SCHEMA);
    assert_eq!(s(m, "l5_basis"), L5_BASIS);
    assert_eq!(s(obj(m.get("l1_threshold").unwrap()), "basis"), "unverified");
    assert_eq!(m.get("passed"), Some(&CValue::Bool(false)));
    let sc = obj(m.get("schema_check").unwrap());
    assert_eq!((s(sc, "status"), s(sc, "code"), s(sc, "reason")), ("refused", "field_too_long", "notes"));
    let not_run = arr(m, "rules_not_run");
    assert_eq!(not_run.len(), 2);
    for r in not_run {
        assert_eq!(s(obj(r), "reason"), "schema_refused");
    }
    let clean = run(&case("synthetic_draft"));
    let v = parse(&clean.canonical_bytes().unwrap()).unwrap();
    let sc = obj(obj(&v).get("schema_check").unwrap()).clone();
    assert_eq!(s(&sc, "status"), "ok");
    assert!(s(&sc, "table_digest").starts_with("sha256:"));
}

// ---------------------------------------------------------------------------- refusals

#[test]
fn a_row_with_an_invalid_key_is_named_by_its_index() {
    let mut text = String::from_utf8(read("long_text.json")).unwrap();
    text = text.replacen("\"dmt:asme/flatness/none@1\"", "\"not a key\"", 1);
    let report = lint_bytes(text.as_bytes(), &index(), Mode::Draft).unwrap();
    let [f] = report.findings() else { panic!("{:?}", report.findings()) };
    assert_eq!((f.rule(), f.field()), (Rule::L1, "notes"));
    assert!(f.key().starts_with("rows[") && f.key().ends_with(']'), "{}", f.key());
    assert_eq!(report.schema(), &SchemaCheck::Refused { code: "bad_dmt_key", reason: None });
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn unreadable_table_bytes_are_errors() {
    let table = read("../dmt/synthetic_table_v1.json");
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(&table);
    let e = lint_bytes(&bom, &index(), Mode::Draft).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("dmt_lint_input_invalid", "bom"));
    let e = lint_bytes(b"{\"rows\": [", &index(), Mode::Draft).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("dmt_lint_input_invalid", "syntax"));
    let e = lint_bytes(b"{\"a\": 1, \"a\": 2}", &index(), Mode::Draft).unwrap_err();
    assert_eq!(e.reason(), "duplicate_key");
    let e = lint_bytes(&[0xff, 0xfe], &index(), Mode::Draft).unwrap_err();
    assert_eq!(e.reason(), "invalid_utf8");
    // Valid JSON that is not a table: the schema refuses it, L1 and L2 could look at no row,
    // and nothing passes.
    let report = lint_bytes(b"[]", &index(), Mode::Release).unwrap();
    assert!(report.findings().is_empty());
    assert!(matches!(report.schema(), SchemaCheck::Refused { .. }));
    assert!(report.rules_run().is_empty());
    assert_eq!(
        report.rules_not_run(),
        &[
            (Rule::L1, NotRunReason::NotEvaluable),
            (Rule::L2, NotRunReason::NotEvaluable),
            (Rule::L3, NotRunReason::SchemaRefused),
            (Rule::L4, NotRunReason::SchemaRefused),
            (Rule::L5, NotRunReason::SchemaRefused),
        ]
    );
    assert!(!report.passed());
    assert_eq!(report.exit_code(), 1);
}

// ---------------------------------------------------------------------------- ORS index

fn index_err(bytes: &[u8]) -> &'static str {
    match OrsIndex::load(bytes) {
        Err(e @ LintError::OrsIndexInvalid { .. }) => e.reason(),
        other => panic!("expected ors_index_invalid, got {other:?}"),
    }
}

#[test]
fn ors_index_refusals() {
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(&read("ors_index.json"));
    assert_eq!(index_err(&bom), "bom");
    assert_eq!(index_err(br#"{"schema":"fid.ors-index/1","cases":["ors:a/b","ors:a/a"]}"#), "not_sorted");
    assert_eq!(index_err(br#"{"schema":"fid.ors-index/1","cases":["ors:a/a","ors:a/a"]}"#), "duplicate_case");
    assert_eq!(index_err(br#"{"schema":"fid.ors-index/1","cases":["ors:A/a"]}"#), "bad_case_id");
    assert_eq!(index_err(br#"{"schema":"fid.ors-index/1","cases":[1]}"#), "wrong_type");
    assert_eq!(index_err(br#"{"schema":"fid.ors-index/1","cases":[],"extra":1}"#), "unknown_field");
    assert_eq!(index_err(br#"{"schema":"fid.ors-index/1"}"#), "missing_field");
    assert_eq!(index_err(br#"{"cases":[]}"#), "missing_field");
    assert_eq!(index_err(br#"{"schema":"fid.ors-index/2","cases":[]}"#), "bad_schema");
    assert_eq!(index_err(br#"{"schema":"fid.ors-index/1","cases":[],"schema":"x"}"#), "duplicate_key");
    assert_eq!(index_err(b"[]"), "wrong_type");
}

#[test]
fn ors_index_new_is_order_independent_and_matches_load() {
    let loaded = index();
    let mut ids: Vec<String> = loaded.cases().map(str::to_owned).collect();
    ids.reverse();
    let built = OrsIndex::new(ids).unwrap();
    assert_eq!(built, loaded);
    assert_eq!(built.digest(), loaded.digest());
    assert_eq!(loaded.len(), 12);
    let e = OrsIndex::new(vec!["ors:a/a".to_owned(), "ors:a/a".to_owned()]).unwrap_err();
    assert_eq!(e.reason(), "duplicate_case");
    let e = OrsIndex::new(vec!["not-a-case".to_owned()]).unwrap_err();
    assert_eq!(e.reason(), "bad_case_id");
    assert!(OrsIndex::new(Vec::new()).unwrap().is_empty());
}

#[test]
fn case_id_grammar() {
    for ok in ["ors:synthetic/flatness.001", "ors:a/b", "ors:a-b_c/d.e-f_g", "ors:0/0"] {
        assert!(case_id_ok(ok), "{ok}");
    }
    let long = format!("ors:a/{}", "b".repeat(91));
    assert!(!case_id_ok(&long));
    assert!(case_id_ok(&long[..96]));
    for bad in ["", "ors:", "ors:/a", "ors:a/", "ors:a", "ors:-a/b", "ors:a/.b", "ors:a.b/c", "ORS:a/b", "ors:a/b c"] {
        assert!(!case_id_ok(bad), "{bad:?}");
    }
}

// ---------------------------------------------------------------------------- counts and empty input

#[test]
fn an_empty_table_is_not_passed() {
    let c = case("empty_rows");
    let bytes = read(s(&c, "table"));
    let table = DmtTable::load(&bytes).unwrap();
    assert!(table.rows().is_empty());
    assert!(lint(&table, &index(), Mode::Release).is_empty());
    for mode in [Mode::Draft, Mode::Release] {
        let report = lint_bytes(&bytes, &index(), mode).unwrap();
        assert_eq!(report, lint_table(&table, &index(), mode));
        assert!(matches!(report.schema(), SchemaCheck::Ok { .. }));
        assert!(report.findings().is_empty() && report.rules_run().is_empty());
        let want: Vec<(Rule, NotRunReason)> = mode.rules().iter().map(|&r| (r, NotRunReason::NoRows)).collect();
        assert_eq!(report.rules_not_run(), want.as_slice());
        assert!(!report.passed());
        assert_eq!(report.exit_code(), 1);
        let v = parse(&report.canonical_bytes().unwrap()).unwrap();
        assert_eq!(obj(&v).get("passed"), Some(&CValue::Bool(false)));
        for r in arr(obj(&v), "rules_not_run") {
            assert_eq!(s(obj(r), "reason"), "no_rows");
        }
    }
}

#[test]
fn a_clean_report_says_how_much_it_checked() {
    let report = run(&case("synthetic_draft"));
    assert!(report.passed());
    let counts: Vec<(usize, usize)> =
        Rule::ALL.iter().filter_map(|&r| report.check(r)).map(|c| (c.checked(), c.skipped())).collect();
    assert_eq!(counts, [(20, 0), (20, 0), (12, 0), (14, 0)]);
    assert_eq!(report.check(Rule::L5), None);
    let v = parse(&report.canonical_bytes().unwrap()).unwrap();
    let checks = obj(obj(&v).get("checks").unwrap());
    assert_eq!(checks.keys().map(String::as_str).collect::<Vec<_>>(), ["L1", "L2", "L3", "L4"]);
    assert_eq!(s(obj(checks.get("L3").unwrap()), "checked"), "12");
}

#[test]
fn rows_l1_and_l2_cannot_read_are_counted_as_skipped() {
    let report = run(&case("raw_skips"));
    assert_eq!(report.schema(), &SchemaCheck::Refused { code: "malformed_table", reason: Some("wrong_type") });
    let l1 = report.check(Rule::L1).unwrap();
    let l2 = report.check(Rule::L2).unwrap();
    assert_eq!((l1.checked(), l1.skipped(), l2.checked(), l2.skipped()), (19, 2, 19, 2));
    // The long notes in a readable row are still found.
    assert_eq!(triples(&report), vec![("L1".into(), "dmt:asme/straightness/none@1".into(), "notes".into())]);
    assert!(!report.passed());
    let report = run(&case("rows_not_array"));
    assert!(report.rules_run().is_empty());
    assert_eq!(report.rules_not_run()[0], (Rule::L1, NotRunReason::NotEvaluable));
    assert_eq!(report.rules_not_run()[1], (Rule::L2, NotRunReason::NotEvaluable));
}

#[test]
fn every_rule_is_a_gate_and_findings_carry_it() {
    for r in Rule::ALL {
        assert_eq!(r.severity(), Severity::Gate);
    }
    let report = run(&case("mixed"));
    let v = parse(&report.canonical_bytes().unwrap()).unwrap();
    for f in arr(obj(&v), "findings") {
        assert_eq!(s(obj(f), "severity"), "gate");
    }
    assert_eq!(FREE_TEXT_FIELDS.join("+"), "notes");
}
