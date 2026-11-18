//! Acceptance criteria of spec node CORE.RPL.CLASS (AC-1 .. AC-6).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_replay::{
    aggregate, classify, Annotation, ClassifyError, Decision, Determinism, Limit, Output, ReplayOutcome,
    StepComparison, Tolerance,
};
use std::collections::BTreeMap;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/rpl/classify/cases.json");

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}
fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}
fn st<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: expected string, got {other:?}"),
    }
}
fn num(o: &BTreeMap<String, CValue>, k: &str) -> f64 {
    match o.get(k) {
        Some(CValue::Num(n)) => *n,
        other => panic!("{k}: expected number, got {other:?}"),
    }
}
fn boolean(o: &BTreeMap<String, CValue>, k: &str) -> bool {
    match o.get(k) {
        Some(CValue::Bool(b)) => *b,
        other => panic!("{k}: expected bool, got {other:?}"),
    }
}

fn annotation_str(a: &Annotation) -> String {
    match a {
        Annotation::Marginal => "MARGINAL".into(),
        Annotation::ToleranceMissing(k) => format!("TOLERANCE_MISSING:{k}"),
    }
}

fn step_from(c: &BTreeMap<String, CValue>) -> StepComparison {
    StepComparison {
        class: Determinism::parse(st(c, "class")).unwrap(),
        same_kernel: boolean(c, "same_kernel"),
        same_platform_class: boolean(c, "same_platform_class"),
        outputs: arr(&c["outputs"])
            .iter()
            .map(|o| {
                let o = obj(o);
                Output {
                    kind: st(o, "kind").into(),
                    recorded: num(o, "recorded"),
                    replayed: num(o, "replayed"),
                    recorded_digest: st(o, "recorded_digest").into(),
                    replayed_digest: st(o, "replayed_digest").into(),
                }
            })
            .collect(),
        decisions: arr(&c["decisions"])
            .iter()
            .map(|d| {
                let d = obj(d);
                Decision { recorded: st(d, "recorded").into(), replayed: st(d, "replayed").into() }
            })
            .collect(),
        limits: arr(&c["limits"])
            .iter()
            .map(|l| {
                let l = obj(l);
                Limit { kind: st(l, "kind").into(), value: num(l, "value") }
            })
            .collect(),
        missing: match c.get("missing") {
            Some(CValue::Str(s)) => Some(s.clone()),
            _ => None,
        },
    }
}

fn table() -> (BTreeMap<String, Tolerance>, Vec<CValue>) {
    let root = parse(&std::fs::read(CASES).unwrap()).unwrap();
    let root = obj(&root).clone();
    let tol = obj(&root["tolerances"])
        .iter()
        .map(|(k, v)| (k.clone(), Tolerance { abs: num(obj(v), "abs"), rel: num(obj(v), "rel") }))
        .collect();
    (tol, arr(&root["cases"]).clone())
}

/// AC-1 (and AC-2, AC-3, AC-4, AC-6 through named cases): every case matches.
#[test]
fn ac1_fixture_cases() {
    let (tol, cases) = table();
    assert!(cases.len() >= 40, "need at least 40 cases");
    let mut failures = Vec::new();
    for c in &cases {
        let c = obj(c);
        let name = st(c, "name");
        let expect = obj(&c["expect"]);
        let got = classify(&step_from(c), &tol).unwrap();
        let mut got_ann: Vec<String> = got.annotations.iter().map(annotation_str).collect();
        got_ann.sort();
        let want_ann: Vec<String> = arr(&expect["annotations"])
            .iter()
            .map(|a| match a {
                CValue::Str(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        let want_o = st(expect, "outcome");
        if got.outcome.as_str() != want_o || got_ann != want_ann {
            failures.push(format!("{name}: got {} {got_ann:?}, want {want_o} {want_ann:?}", got.outcome));
        }
    }
    assert!(failures.is_empty(), "{} case(s) failed:\n{}", failures.len(), failures.join("\n"));
}

/// AC-5: aggregation order.
#[test]
fn ac5_aggregate() {
    use ReplayOutcome::*;
    assert_eq!(aggregate(&[Identical, Equivalent, NotReplayable]), NotReplayable);
    assert_eq!(aggregate(&[Identical, Equivalent, NotReplayable, Divergent]), Divergent);
    assert_eq!(aggregate(&[Identical, Equivalent]), Equivalent);
    assert_eq!(aggregate(&[Identical]), Identical);
    assert_eq!(aggregate(&[]), Identical);
}

#[test]
fn bad_tolerance_is_rejected() {
    let mut tol: BTreeMap<String, Tolerance> = BTreeMap::new();
    tol.insert("length_mm".into(), Tolerance { abs: -1e-5, rel: 0.0 });
    let step = StepComparison {
        class: Determinism::D3,
        same_kernel: false,
        same_platform_class: false,
        outputs: vec![Output {
            kind: "length_mm".into(),
            recorded: 1.0,
            replayed: 1.000001,
            recorded_digest: "a".into(),
            replayed_digest: "b".into(),
        }],
        decisions: vec![],
        limits: vec![],
        missing: None,
    };
    assert_eq!(classify(&step, &tol).unwrap_err().code(), "comparison_invalid");
    tol.insert("length_mm".into(), Tolerance { abs: f64::NAN, rel: 0.0 });
    assert_eq!(classify(&step, &tol).unwrap_err().code(), "comparison_invalid");
}

#[test]
fn non_finite_input_is_rejected() {
    let tol: BTreeMap<String, Tolerance> = BTreeMap::new();
    let step = StepComparison {
        class: Determinism::D3,
        same_kernel: true,
        same_platform_class: true,
        outputs: vec![Output {
            kind: "length_mm".into(),
            recorded: 1.0,
            replayed: f64::NAN,
            recorded_digest: "a".into(),
            replayed_digest: "b".into(),
        }],
        decisions: vec![],
        limits: vec![],
        missing: None,
    };
    let err = classify(&step, &tol).unwrap_err();
    assert_eq!(err.code(), "comparison_invalid");
    assert!(matches!(err, ClassifyError::ComparisonInvalid(_)));
}
