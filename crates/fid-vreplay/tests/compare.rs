//! TRUST.REPLAY.COMPARE AC-1 .. AC-5 and REQ-007 over `fixtures/replay/compare/`, whose labels
//! come from the independent reference `fixtures/replay/compare/make_cases.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_num::kdec::Decimal;
use fid_rep::{NotReplayableCause, Outcome};
use fid_vreplay::compare::{
    aggregate, compare_package, compare_result, CompareError, Decision, DecisionOutcome, Determinism, Rep1,
    ReplayContext, ResultFields, ResultInput, ResultPair,
};

const TABLE: &[u8] = include_bytes!("../../../fixtures/replay/compare/rep1-test.json");
const TABLE_DIGEST: &str = include_str!("../../../fixtures/replay/compare/rep1-test.sha256");
const CASES: &[u8] = include_bytes!("../../../fixtures/replay/compare/cases.json");

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    let CValue::Obj(m) = v else { panic!("not an object: {v:?}") };
    m.get(k).unwrap_or_else(|| panic!("no key {k}"))
}

fn str_of(v: &CValue) -> Option<&str> {
    match v {
        CValue::Str(s) => Some(s),
        CValue::Null => None,
        other => panic!("not a string: {other:?}"),
    }
}

fn f64_of(v: &CValue) -> Option<f64> {
    str_of(v).map(|h| f64::from_bits(u64::from_str_radix(h, 16).unwrap()))
}

fn side(v: &CValue) -> ResultFields {
    let CValue::Num(uec) = get(v, "uec") else { panic!("uec") };
    let decision = match get(v, "decision") {
        CValue::Null => None,
        d => Some(Decision {
            rule_id: str_of(get(d, "rule_id")).unwrap().to_string(),
            outcome: DecisionOutcome::parse(str_of(get(d, "outcome")).unwrap()).unwrap(),
        }),
    };
    ResultFields {
        value: f64_of(get(v, "value")),
        uec: *uec as u8,
        expanded_uncertainty: f64_of(get(v, "u")),
        decision,
        status: str_of(get(v, "status")).unwrap().to_string(),
    }
}

fn bool_of(v: &CValue) -> bool {
    let CValue::Bool(b) = v else { panic!("not a bool") };
    *b
}

struct Case {
    name: String,
    ctx: ReplayContext,
    pair: ResultPair,
    want: (String, Option<String>, Option<String>, bool),
}

fn cases() -> Vec<Case> {
    let doc = fid_canon::parse(CASES).unwrap();
    let CValue::Arr(list) = get(&doc, "cases") else { panic!("cases") };
    list.iter()
        .map(|c| {
            let name = str_of(get(c, "name")).unwrap().to_string();
            let ctx = get(c, "context");
            let limit = match c {
                CValue::Obj(m) => m.get("limit").map(|l| Decimal::parse(str_of(l).unwrap()).unwrap()),
                _ => None,
            };
            let e = get(c, "expect");
            Case {
                ctx: ReplayContext {
                    same_manifest: bool_of(get(ctx, "same_manifest")),
                    same_platform_class: bool_of(get(ctx, "same_platform_class")),
                },
                pair: ResultPair {
                    record_id: name.clone(),
                    determinism: Determinism::parse(str_of(get(c, "determinism")).unwrap()).unwrap(),
                    quantity_kind: str_of(get(c, "quantity_kind")).unwrap().to_string(),
                    limit,
                    packaged: side(get(c, "packaged")),
                    recomputed: side(get(c, "recomputed")),
                },
                want: (
                    str_of(get(e, "outcome")).unwrap().to_string(),
                    str_of(get(e, "first_diff")).map(String::from),
                    str_of(get(e, "reason")).map(String::from),
                    bool_of(get(e, "marginal")),
                ),
                name,
            }
        })
        .collect()
}

fn test_table() -> Rep1 {
    Rep1::load(TABLE).unwrap()
}

fn run(c: &Case) -> (String, Option<String>, Option<String>, bool) {
    let o = compare_result(&test_table(), &c.ctx, &c.pair);
    assert_eq!(o.record_id, c.name);
    (
        o.outcome.as_str().to_string(),
        o.first_diff.map(|f| f.as_str().to_string()),
        o.reason.map(|r| r.to_wire()),
        o.marginal,
    )
}

fn case(name: &str) -> Case {
    cases().into_iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no case {name}"))
}

/// AC-1 (and the `cases.json` half of AC-H1): every outcome, first differing field and reason.
#[test]
fn ac1_every_case_matches() {
    let all = cases();
    assert!(all.len() >= 40, "{} cases", all.len());
    let bad: Vec<String> = all
        .iter()
        .filter_map(|c| {
            let got = run(c);
            (got != c.want).then(|| format!("{}: want {:?}, got {got:?}", c.name, c.want))
        })
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

/// AC-2
#[test]
fn ac2_d3_half_and_twice_tolerance() {
    assert_eq!(run(&case("d3_half_tolerance")).0, "EQUIVALENT");
    assert_eq!(run(&case("d3_twice_tolerance")).0, "DIVERGENT");
}

/// AC-3
#[test]
fn ac3_decision_changed_within_tolerance() {
    let got = run(&case("d3_within_decision_changed"));
    assert_eq!((got.0.as_str(), got.1.as_deref()), ("DIVERGENT", Some("decision.outcome")));
}

/// AC-4
#[test]
fn ac4_d0_last_bit_and_missing_kind() {
    assert_eq!(run(&case("d0_last_bit")).0, "DIVERGENT");
    let got = run(&case("missing_kind_angle"));
    assert_eq!((got.0.as_str(), got.2.as_deref()), ("DIVERGENT", Some("NO_TOLERANCE")));
}

/// AC-5 and REQ-006: the outcome lists of `cases.json`, then whole packages.
#[test]
fn ac5_package_aggregation() {
    let doc = fid_canon::parse(CASES).unwrap();
    let CValue::Arr(pkgs) = get(&doc, "packages") else { panic!("packages") };
    let parse = |s: &str| match s {
        "IDENTICAL" => Outcome::Identical,
        "EQUIVALENT" => Outcome::Equivalent,
        "NOT_REPLAYABLE" => Outcome::NotReplayable(NotReplayableCause::Other),
        "DIVERGENT" => Outcome::Divergent,
        other => panic!("{other}"),
    };
    for p in pkgs {
        let CValue::Arr(list) = get(p, "outcomes") else { panic!("outcomes") };
        let outcomes: Vec<Outcome> = list.iter().map(|o| parse(str_of(o).unwrap())).collect();
        assert_eq!(aggregate(&outcomes), parse(str_of(get(p, "expect")).unwrap()), "{p:?}");
    }

    let rep1 = test_table();
    let rc = ReplayContext { same_manifest: true, same_platform_class: true };
    let replayed = |name: &str| ResultInput::Replayed(Box::new(case(name).pair));
    let nr = ResultInput::NotReplayable { record_id: "r-nr".into(), reason: "RESULTS_ONLY".into() };
    let pkg = compare_package(&rep1, &rc, &[replayed("identical_D3"), replayed("d3_half_tolerance")]);
    assert_eq!(pkg.outcome, Outcome::Equivalent);
    let pkg = compare_package(&rep1, &rc, &[replayed("d3_half_tolerance"), nr.clone()]);
    assert_eq!(pkg.outcome, Outcome::NotReplayable(NotReplayableCause::Other));
    assert_eq!(pkg.results[1].reason.as_ref().unwrap().to_wire(), "NOT_REPLAYABLE:RESULTS_ONLY");
    let pkg = compare_package(&rep1, &rc, &[nr, replayed("d0_last_bit")]);
    assert_eq!(pkg.outcome, Outcome::Divergent);
    let pkg = compare_package(&rep1, &rc, &[]);
    assert_eq!(pkg.outcome, Outcome::NotReplayable(NotReplayableCause::Other));
    assert!(pkg.results.is_empty());
}

/// REQ-007: the digest of the table file used is in the output.
#[test]
fn req007_table_digest_recorded() {
    let rep1 = test_table();
    assert_eq!(rep1.digest().as_str(), TABLE_DIGEST.trim_end());
    assert_eq!(rep1.version(), "test-only.rep@0.0.1");
    let rc = ReplayContext { same_manifest: true, same_platform_class: true };
    let pkg = compare_package(&rep1, &rc, &[ResultInput::Replayed(Box::new(case("d3_half_tolerance").pair))]);
    assert_eq!(pkg.rep1_digest, rep1.digest());
    let json = String::from_utf8(pkg.canonical_json().unwrap()).unwrap();
    assert!(json.contains(&format!("\"rep1_digest\":\"{}\"", TABLE_DIGEST.trim_end())), "{json}");
    assert!(json.contains("\"first_diff\":\"value\""), "{json}");

    let prod = Rep1::embedded().unwrap();
    assert_eq!(prod.digest().as_str(), fid_rep::TABLE_DIGEST);
    assert_eq!(prod.version(), "rep-1@1.0.0");
}

/// `rep1.malformed` for every schema rule.
#[test]
fn rep1_malformed() {
    let good = std::str::from_utf8(TABLE).unwrap();
    let bad = |text: &str, detail: &str| {
        let e = Rep1::load(text.as_bytes()).unwrap_err();
        assert_eq!(e.code(), "rep1.malformed");
        assert_eq!(e.to_string(), format!("rep1.malformed: {detail}"), "{text}");
    };
    bad("", "not_json");
    bad("[]", "not_an_object");
    bad(&format!("{good} "), "not_canonical");
    bad(&good.replace("\"rep-table@1\"", "\"rep-table@2\""), "schema");
    bad(&good.replace("\"rdl.rep.classify@1.0.0\"", "\"rdl.rep.classify@2.0.0\""), "algorithm");
    bad(&good.replace(",{\"kind\":\"exact\",\"mode\":\"exact\"}", ""), "exact_row");
    bad(&good.replace("\"unit\":\"m\"", "\"unit\":\"mm\""), "bad_unit");
    bad(&good.replace("\"kind\":\"length\"", "\"kind\":\"temperature\""), "kind_unknown");
    bad(&good.replace("\"mode\":\"abs\",\"unit\":\"m\"", "\"mode\":\"max\",\"unit\":\"m\""), "mode");
    bad(&good.replace("\"bound\":\"1e-6\",\"kind\":\"length\"", "\"bound\":\"x\",\"kind\":\"length\""), "bound");
    bad(
        &good.replace("\"bound\":\"1e-6\",\"kind\":\"length\"", "\"bound\":\"-1e-6\",\"kind\":\"length\""),
        "negative_bound",
    );
    bad(&good.replace("{\"algorithm\"", "{\"extra\":1,\"algorithm\""), "bad_keys");
    bad(&good.replace("{\"algorithm\"", "{\"version\":\"x\",\"algorithm\""), "duplicate_key");
    bad(&good.replace("test-only.rep@0.0.1", "test\\\"rep@0.0.1"), "bad_text");
    bad(&good.replace("\"kind\":\"dimensionless\"", "\"kind\":\"length\""), "duplicate_kind");
    let big = vec![b' '; fid_vreplay::compare::MAX_TABLE_BYTES + 1];
    assert_eq!(Rep1::load(&big).unwrap_err(), CompareError::Rep1Malformed("too_large"));
}
