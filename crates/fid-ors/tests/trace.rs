//! Acceptance criteria of spec node UNC.ORS.TRACE (AC-1 to AC-5, AC-H1, AC-H2).
//!
//! Every fixture and expected result comes from `fixtures/ors/trace/make_fixtures.py`, an
//! independent stdlib Python generator and checker written from the leaf's pinned rules;
//! none come from this crate. The surface, schema and replay checks are hand-labelled.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse_with_limits, CValue};
use fid_gps::dmt::DmtTable;
use fid_ors::{check_suite, invoke_cli, invoke_mcp, replay, ActivityEntry, CheckReport, Suite, SuiteStore, CODES};
use fid_replay::ReplayOutcome;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ors/trace")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(dir().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn json(rel: &str) -> CValue {
    parse_with_limits(&read(rel), 64 << 20, 64).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("no {k}")),
        _ => panic!("not an object"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("not an array"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("not a string"),
    }
}

fn dmt() -> DmtTable {
    let t = DmtTable::load(&read("dmt_stub.json")).unwrap();
    let digest = String::from_utf8(read("dmt_stub.digest")).unwrap();
    assert_eq!(t.digest().as_str(), digest.trim(), "the stub's digest equals the oracle's");
    t
}

fn good() -> Suite {
    let manifest = read("good_suite/suite.json");
    let mut cases: Vec<Vec<u8>> = Vec::new();
    let mut names: Vec<_> =
        std::fs::read_dir(dir().join("good_suite/cases")).unwrap().map(|e| e.unwrap().path()).collect();
    names.sort();
    for p in names {
        cases.push(std::fs::read(p).unwrap());
    }
    let refs: Vec<&[u8]> = cases.iter().map(Vec::as_slice).collect();
    Suite::from_json(&manifest, &refs).unwrap()
}

fn bundle(rel: &str) -> Suite {
    Suite::from_bundle(&read(rel)).unwrap()
}

type Row = (Option<String>, String, String);

fn rows(r: &CheckReport) -> Vec<Row> {
    r.findings.iter().map(|f| (f.case_id.clone(), f.code.to_string(), f.field.clone())).collect()
}

fn want_rows(v: &CValue) -> Vec<Row> {
    arr(v)
        .iter()
        .map(|f| {
            let case = match get(f, "case") {
                CValue::Null => None,
                c => Some(st(c).to_string()),
            };
            (case, st(get(f, "code")).to_string(), st(get(f, "field")).to_string())
        })
        .collect()
}

fn coverage(r: &CheckReport) -> (u64, u64, u64) {
    (r.coverage.closed_form_math, r.coverage.dmt_row, r.coverage.neither)
}

fn want_coverage(v: &CValue) -> (u64, u64, u64) {
    let c = get(v, "coverage");
    let n = |k| match get(c, k) {
        CValue::Num(x) => *x as u64,
        CValue::Str(s) => s.parse().unwrap(),
        other => panic!("{other:?}"),
    };
    (n("closed_form_math"), n("dmt_row"), n("neither"))
}

fn store() -> SuiteStore {
    let mut s = SuiteStore::new();
    s.add_dmt(dmt()).unwrap();
    s.install(good()).unwrap();
    s
}

// ------------------------------------------------------------------ AC-1 .. AC-3

#[test]
fn ac1_good_suite_passes_with_zero_findings() {
    let exp = json("expected.json");
    let r = check_suite(&good(), Some(&dmt()), None).unwrap();
    assert!(r.passed(), "{:?}", r.findings);
    assert_eq!(r.cases, 6);
    assert_eq!(coverage(&r), want_coverage(get(&exp, "good_suite")));
    // Through the CLI, as the AC states.
    let s = store();
    let mut log = Vec::new();
    let out = invoke_cli(&s, &mut log, &["check-suite", "--suite", "ors-trace-good@1.0.0", "--json"]);
    let env = parse_with_limits(&out.stdout, 1 << 20, 16).unwrap();
    assert_eq!(st(get(&env, "outcome")), "computed", "{}", String::from_utf8_lossy(&out.stdout));
    let result = get(&env, "result");
    assert_eq!(get(result, "passed"), &CValue::Bool(true));
    assert!(arr(get(result, "findings")).is_empty());
    assert_eq!(log.len(), 1);
}

#[test]
fn ac2_each_bad_variant_reports_its_code() {
    let exp = json("expected.json");
    let CValue::Obj(bad) = get(&exp, "bad") else { panic!() };
    assert_eq!(bad.len(), 7);
    let table = dmt();
    for (name, want) in bad {
        let r = check_suite(&bundle(&format!("bad/{name}.json")), Some(&table), None).unwrap();
        assert_eq!(rows(&r), want_rows(get(want, "findings")), "{name}");
        assert_eq!(coverage(&r), want_coverage(want), "{name}");
        assert!(!r.passed());
    }
}

#[test]
fn ac3_a_changed_expected_value_needs_a_bump_and_a_changelog_entry() {
    let exp = json("expected.json");
    let CValue::Obj(v) = get(&exp, "versioning") else { panic!() };
    let (table, prev) = (dmt(), good());
    for (name, want) in v {
        let r = check_suite(&bundle(&format!("versioning/{name}.json")), Some(&table), Some(&prev)).unwrap();
        assert_eq!(rows(&r), want_rows(get(want, "findings")), "{name}");
    }
    let r = check_suite(&bundle("versioning/changed_same_version.json"), Some(&table), Some(&prev)).unwrap();
    assert_eq!(r.findings[0].code, "version_not_bumped");
    assert!(check_suite(&bundle("versioning/bumped_with_changelog.json"), Some(&table), Some(&prev)).unwrap().passed());
    // Through the command: the candidate is staged under the released handle.
    let mut s = store();
    s.stage(bundle("versioning/changed_same_version.json")).unwrap();
    let mut log = Vec::new();
    let out = invoke_cli(
        &s,
        &mut log,
        &["check-suite", "--suite", "ors-trace-good@1.0.0", "--staged", "--previous", "ors-trace-good@1.0.0", "--json"],
    );
    let env = parse_with_limits(&out.stdout, 1 << 20, 16).unwrap();
    let f = &arr(get(get(&env, "result"), "findings"))[0];
    assert_eq!(st(get(f, "code")), "version_not_bumped");
    // A previous release of another suite is unreadable input, not a finding.
    let other = Suite::from_bundle(
        &String::from_utf8(read("bad/licence_proprietary.json"))
            .unwrap()
            .replace("ors-trace-good", "ors-other")
            .into_bytes(),
    )
    .unwrap();
    assert_eq!(check_suite(&good(), Some(&table), Some(&other)).unwrap_err().code(), "suite_invalid");
}

#[test]
fn a_dmt_with_another_digest_resolves_no_row() {
    let mut other = String::from_utf8(read("dmt_stub.json")).unwrap();
    other = other.replacen("\"1.0.0\"", "\"1.0.1\"", 1);
    let t = DmtTable::load(other.as_bytes()).unwrap();
    let r = check_suite(&good(), Some(&t), None).unwrap();
    assert_eq!(r.findings.len(), 4);
    assert!(r.findings.iter().all(|f| f.code == "dmt_row_unknown"));
    assert_eq!(check_suite(&good(), None, None).unwrap().findings.len(), 4);
}

// ------------------------------------------------------------------ AC-4, AC-5

#[test]
fn ac4_list_suite_returns_name_version_count_licence_and_dmt_digest() {
    let exp = json("expected.json");
    let s = store();
    let mut log = Vec::new();
    let out = invoke_cli(&s, &mut log, &["list-suite", "--json"]);
    let env = parse_with_limits(&out.stdout, 1 << 20, 16).unwrap();
    let got = arr(get(&env, "result"));
    let want = arr(get(&exp, "list_suite"));
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(want) {
        for k in ["dmt_digest", "licence", "name", "version"] {
            assert_eq!(get(g, k), get(w, k), "{k}");
        }
        assert_eq!(st(get(g, "cases")), "6");
    }
    let mcp = invoke_mcp(&s, &mut log, "rdl_list_suite", b"{}");
    assert_eq!(mcp.encode().unwrap(), out.stdout, "the MCP and CLI forms print the same bytes");
}

#[test]
fn ac5_exact_replay_of_ac1_and_ac4_is_identical() {
    let s = store();
    let mut log = Vec::new();
    invoke_cli(&s, &mut log, &["check-suite", "--suite", "ors-trace-good@1.0.0", "--json"]);
    invoke_cli(&s, &mut log, &["list-suite"]);
    invoke_mcp(&s, &mut log, "rdl_check_suite", br#"{"suite":"ors-trace-good@9.9.9"}"#);
    assert_eq!(log.len(), 3);
    for e in &log {
        let again = ActivityEntry::from_json(&e.to_json().unwrap()).unwrap();
        assert_eq!(&again, e);
        assert_eq!(replay(&again, &s).unwrap(), ReplayOutcome::Identical);
    }
    // Replayed against another store state, the check diverges.
    let mut changed = SuiteStore::new();
    changed.install(good()).unwrap();
    assert_eq!(replay(&log[0], &changed).unwrap(), ReplayOutcome::Divergent);
}

#[test]
fn surfaces_refuse_bad_parameters_without_logging_unparsed_calls() {
    let s = store();
    let mut log = Vec::new();
    for args in [
        &["check-suite"][..],
        &["check-suite", "--suite", "trace@1.0.0"],
        &["check-suite", "--suite", "ors-trace-good@1.0"],
        &["check-suite", "--suite", "ors-trace-good@1.0.0", "--suite", "ors-trace-good@1.0.0"],
        &["check-suite", "--suite", "ors-trace-good@1.0.0", "--bogus"],
        &["list-suite", "--suite", "x"],
        &["nope"],
    ] {
        let out = invoke_cli(&s, &mut log, args);
        assert!(out.envelope.error.is_some(), "{args:?}");
        assert!(out.stderr.starts_with("error: bad_params"), "{args:?}: {}", out.stderr);
    }
    assert!(log.is_empty());
    for p in
        [&br#"{"suite":1}"#[..], br#"{"suite":"ors-trace-good@1.0.0","x":1}"#, br#"{"suite":"a","suite":"b"}"#, b"[]"]
    {
        let env = invoke_mcp(&s, &mut log, "rdl_check_suite", p);
        assert!(env.error.is_some());
    }
    assert!(log.is_empty());
    let nf = invoke_mcp(
        &s,
        &mut log,
        "rdl_check_suite",
        br#"{"suite":"ors-trace-good@1.0.0","previous":"ors-trace-good@0.1.0"}"#,
    );
    let details = nf.error.unwrap().details.unwrap();
    assert_eq!(st(get(&details, "field")), "previous");
    assert_eq!(st(get(&details, "ors_error")), "suite_not_found");
}

// ------------------------------------------------------------------ AC-H1, AC-H2

#[test]
fn ach1_every_seeded_mutant_is_reported_exactly() {
    let m = json("mutants.json");
    let mutants = arr(get(&m, "mutants"));
    assert_eq!(mutants.len(), 100);
    let (table, prev) = (dmt(), good());
    let mut codes = std::collections::BTreeSet::new();
    for mu in mutants {
        let name = st(get(mu, "name"));
        let b = get(mu, "suite");
        let suite = Suite::from_values(get(b, "manifest"), arr(get(b, "cases"))).unwrap();
        let previous = matches!(get(mu, "previous"), CValue::Bool(true)).then_some(&prev);
        let r = check_suite(&suite, Some(&table), previous).unwrap();
        let want = want_rows(get(mu, "findings"));
        assert_eq!(rows(&r), want, "{name}");
        let benign = matches!(get(mu, "benign"), CValue::Bool(true));
        assert_eq!(r.findings.is_empty(), benign, "{name}: a seeded fault must be found, a benign change must not");
        codes.extend(r.findings.iter().map(|f| f.code));
    }
    // All seven finding codes of the leaf (plus case_invalid) are exercised.
    for c in CODES {
        assert!(codes.contains(c), "{c} never seeded");
    }
}

#[test]
fn ach2_every_good_case_resolves_to_a_dmt_row_or_closed_form_math() {
    let r = check_suite(&good(), Some(&dmt()), None).unwrap();
    assert_eq!(r.coverage.neither, 0);
    assert_eq!(r.coverage.closed_form_math + r.coverage.dmt_row, r.cases);
}

// ------------------------------------------------------------------ loading and schemas

#[test]
fn unreadable_suites_are_suite_invalid() {
    let g = String::from_utf8(read("good_suite/suite.json")).unwrap();
    let case = read("good_suite/cases/flatness.001.json");
    for (m, why) in [
        (g.replace("ors-suite/v1", "ors-suite/v2"), "schema"),
        (g.replace("\"ors-trace-good\"", "\"trace-good\""), "name"),
        (g.replace("\"1.0.0\"", "\"1.0\""), "version"),
        (g.replace("\"licence\": \"CC-BY-4.0\"", "\"licence\": \"MIT OR Apache-2.0\""), "licence"),
        (g.replacen("{", "{\"extra\": 1, ", 1), "unknown"),
    ] {
        let e = Suite::from_json(m.as_bytes(), &[&case]).unwrap_err();
        assert_eq!(e.code(), "suite_invalid", "{why}");
    }
    // Case documents must match the listed IDs one to one.
    assert_eq!(Suite::from_json(g.as_bytes(), &[&case]).unwrap_err().code(), "suite_invalid");
    // Duplicate keys and oversized input are refused.
    let dup = br#"{"schema":"ors-suite/v1","schema":"ors-suite/v1"}"#;
    assert!(Suite::from_json(dup, &[]).is_err());
    let errors = Suite::from_json(b"x", &[]).unwrap_err().to_string();
    assert!(errors.starts_with("suite_invalid: manifest:"), "{errors}");
}
