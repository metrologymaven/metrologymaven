//! Acceptance criteria of spec node CORE.RPL.BISECT (AC-1 .. AC-5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_replay::{
    attribute, bisect, first_divergent, first_divergent_by_seq, report_fragment, Attribution, BisectError,
    BisectResult, Cause, EntryFacts, ReplayOutcome,
};
use fid_types::{CommandId, Digest};
use std::collections::BTreeMap;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/rpl/bisect/");

fn load(name: &str) -> CValue {
    let bytes = std::fs::read(format!("{DIR}{name}")).unwrap();
    parse(&bytes).unwrap()
}
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
fn s(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}
fn n(v: &CValue) -> u64 {
    match v {
        CValue::Num(x) => {
            assert!(*x >= 0.0 && x.fract() == 0.0, "not a count: {x}");
            *x as u64
        }
        CValue::U64(x) => *x,
        CValue::I64(x) => u64::try_from(*x).unwrap(),
        other => panic!("expected number, got {other:?}"),
    }
}
fn opt_n(v: &CValue) -> Option<u64> {
    match v {
        CValue::Null => None,
        other => Some(n(other)),
    }
}
fn b(v: &CValue) -> bool {
    match v {
        CValue::Bool(b) => *b,
        other => panic!("expected bool, got {other:?}"),
    }
}

fn outcome(name: &str) -> ReplayOutcome {
    match name {
        "IDENTICAL" => ReplayOutcome::Identical,
        "DIVERGENT" => ReplayOutcome::Divergent,
        other => panic!("unexpected outcome {other}"),
    }
}

fn ceil_log2(x: u64) -> u64 {
    u64::from(64 - (x - 1).leading_zeros()) * u64::from(x > 1)
}

struct Span {
    recorded: Vec<String>,
    replayed: Vec<String>,
    outcomes: Vec<(u64, ReplayOutcome)>,
    expect_first: u64,
    max_evals: u64,
}

fn span() -> Span {
    let doc = load("span_1000.json");
    let doc = obj(&doc);
    let mut sp = Span {
        recorded: Vec::new(),
        replayed: Vec::new(),
        outcomes: Vec::new(),
        expect_first: n(&obj(&doc["expect"])["first_divergent"]),
        max_evals: n(&obj(&doc["expect"])["max_evaluations"]),
    };
    for (i, e) in arr(&doc["entries"]).iter().enumerate() {
        let e = obj(e);
        assert_eq!(n(&e["seq"]), i as u64);
        sp.recorded.push(s(&e["recorded_state"]).to_string());
        sp.replayed.push(s(&e["replayed_state"]).to_string());
        sp.outcomes.push((i as u64, outcome(s(&e["outcome"]))));
    }
    assert_eq!(sp.recorded.len(), 1000);
    sp
}

#[test]
fn ac1_bisection_finds_637_within_11_evaluations() {
    let sp = span();
    assert_eq!(sp.max_evals, 11);
    assert_eq!(ceil_log2(1000) + 1, 11);
    let mut calls = 0u64;
    let res = bisect(sp.recorded.len() as u64, |k| -> Result<bool, ()> {
        calls += 1;
        let k = k as usize;
        Ok(sp.recorded[k] != sp.replayed[k])
    })
    .unwrap();
    assert_eq!(res.first, Some(sp.expect_first));
    assert_eq!(res.first, Some(637));
    assert!(!res.non_monotone);
    assert_eq!(res.evaluations, calls);
    assert!(calls <= 11, "{calls} evaluations");
}

#[test]
fn req001_linear_first_divergent_from_outcomes() {
    let sp = span();
    let plain: Vec<ReplayOutcome> = sp.outcomes.iter().map(|(_, o)| *o).collect();
    assert_eq!(first_divergent(&plain), Some(637));
    let mut shuffled = sp.outcomes.clone();
    shuffled.reverse();
    assert_eq!(first_divergent_by_seq(&shuffled), Some(637));
    assert_eq!(first_divergent(&plain[..637]), None);
    assert_eq!(first_divergent(&[]), None);
    let mixed = [ReplayOutcome::Equivalent, ReplayOutcome::NotReplayable, ReplayOutcome::Divergent];
    assert_eq!(first_divergent(&mixed), Some(2));
}

fn interval_pred(intervals: &[(u64, u64)]) -> impl Fn(u64) -> bool + '_ {
    move |k| intervals.iter().any(|&(a, z)| a <= k && k < z)
}

#[test]
fn ac4_non_monotone_reported_and_linear_fallback_correct() {
    let doc = load("non_monotone.json");
    let mut flagged = 0;
    for c in arr(&obj(&doc)["cases"]) {
        let c = obj(c);
        let name = s(&c["name"]);
        let size = n(&c["n"]);
        let ivs: Vec<(u64, u64)> = arr(&c["true_intervals"])
            .iter()
            .map(|iv| {
                let iv = arr(iv);
                (n(&iv[0]), n(&iv[1]))
            })
            .collect();
        let exp = obj(&c["expect"]);
        let p = interval_pred(&ivs);
        let mut calls = 0u64;
        let res = bisect(size, |k| -> Result<bool, ()> {
            calls += 1;
            Ok(p(k))
        })
        .unwrap();
        assert_eq!(res.first, opt_n(&exp["first_divergent"]), "{name}");
        assert_eq!(res.non_monotone, b(&exp["non_monotone"]), "{name}");
        assert_eq!(res.evaluations, calls, "{name}");
        if res.non_monotone {
            flagged += 1;
            assert_eq!(res.notes(), vec!["non_monotone".to_string()], "{name}");
        } else {
            assert!(calls <= n(&exp["max_evaluations"]), "{name}: {calls}");
            assert!(calls <= ceil_log2(size) + 1, "{name}: {calls}");
        }
    }
    assert!(flagged >= 1);
}

#[test]
fn bisect_errors() {
    let r = bisect(0, |_| -> Result<bool, ()> { Ok(true) });
    assert_eq!(r.as_ref().map_err(BisectError::code), Err("empty_span"));
    let r = bisect(10, |k| if k == 4 { Err("boom") } else { Ok(k >= 3) });
    assert_eq!(r, Err(BisectError::Predicate("boom")));
}

#[test]
fn bisect_budget_exhaustive_small_spans() {
    // Every monotone predicate on spans of 1..=130 entries: right answer, within budget.
    for size in 1..=130u64 {
        for d in 0..=size {
            let mut calls = 0u64;
            let res = bisect(size, |k| -> Result<bool, ()> {
                calls += 1;
                Ok(k >= d)
            })
            .unwrap();
            let want = if d == size { None } else { Some(d) };
            assert_eq!(res.first, want, "size {size} d {d}");
            assert!(!res.non_monotone);
            assert!(calls <= ceil_log2(size) + 1, "size {size} d {d}: {calls}");
        }
    }
}

fn facts(v: &CValue) -> EntryFacts {
    let o = obj(v);
    let digests = |k: &str| arr(&o[k]).iter().map(|d| Digest::parse(s(d)).unwrap()).collect::<Vec<_>>();
    EntryFacts {
        seq: n(&o["seq"]),
        inputs: digests("inputs"),
        arguments: digests("arguments"),
        defaults_profile: Digest::parse(s(&o["defaults_profile"])).unwrap(),
        command: CommandId::parse(s(&o["command"])).unwrap(),
        migrator: match &o["migrator"] {
            CValue::Null => None,
            other => Some(s(other).to_string()),
        },
        kernel_manifest: Digest::parse(s(&o["kernel_manifest"])).unwrap(),
    }
}

fn expected(v: &CValue) -> Attribution {
    let e = obj(v);
    Attribution {
        entry_seq: n(&e["entry_seq"]),
        primary: Cause::parse(s(&e["primary"])).unwrap(),
        secondary: arr(&e["secondary"]).iter().map(|c| Cause::parse(s(c)).unwrap()).collect(),
        notes: arr(&e["notes"]).iter().map(|x| s(x).to_string()).collect(),
    }
}

fn attribution_cases() -> Vec<(String, String, EntryFacts, EntryFacts, Attribution)> {
    let doc = load("attribution.json");
    arr(&obj(&doc)["cases"])
        .iter()
        .map(|c| {
            let c = obj(c);
            (
                s(&c["name"]).to_string(),
                s(&c["ac"]).to_string(),
                facts(&c["recorded"]),
                facts(&c["replayed"]),
                expected(&c["expect"]),
            )
        })
        .collect()
}

#[test]
fn ac2_one_case_per_cause() {
    let cases = attribution_cases();
    let mut causes = Vec::new();
    for (name, ac, rec, rep, want) in &cases {
        if ac != "AC-2" {
            continue;
        }
        let got = attribute(rec, rep);
        assert_eq!(got.primary, want.primary, "{name}");
        assert_eq!(got, *want, "{name}");
        causes.push(got.primary);
    }
    causes.sort();
    assert_eq!(causes, Cause::ALL.to_vec());
}

#[test]
fn ac3_input_beats_kernel() {
    let cases = attribution_cases();
    let (name, _, rec, rep, want) = cases.iter().find(|c| c.1 == "AC-3").unwrap();
    let got = attribute(rec, rep);
    assert_eq!(got.primary, Cause::Input, "{name}");
    assert_eq!(got.secondary, vec![Cause::Kernel], "{name}");
    assert_eq!(got, *want);
}

#[test]
fn req004_req005_remaining_cases() {
    for (name, _, rec, rep, want) in attribution_cases() {
        assert_eq!(attribute(&rec, &rep), want, "{name}");
    }
    let cases = attribution_cases();
    let (_, _, rec, rep, got) = cases.iter().find(|c| c.1 == "REQ-005").unwrap();
    assert_eq!(rec, rep);
    assert_eq!(got.primary, Cause::Kernel);
    assert_eq!(got.notes, vec!["no_input_difference".to_string()]);
}

fn run_once() -> Vec<u8> {
    let sp = span();
    let res: BisectResult = bisect(1000, |k| -> Result<bool, ()> {
        let k = k as usize;
        Ok(sp.recorded[k] != sp.replayed[k])
    })
    .unwrap();
    let mut out = Vec::new();
    for (_, _, rec, rep, _) in attribution_cases() {
        let att = attribute(&rec, &rep);
        out.extend(report_fragment(Some(&res), &att).unwrap());
        out.push(b'\n');
        out.extend(report_fragment(None, &att).unwrap());
        out.push(b'\n');
    }
    out
}

#[test]
fn ac5_byte_identical_outputs() {
    let a = run_once();
    let b = run_once();
    assert_eq!(a, b);
    let first = a.split(|c| *c == b'\n').next().unwrap();
    assert_eq!(
        std::str::from_utf8(first).unwrap(),
        r#"{"first_divergence":{"attribution":{"notes":[],"primary":"input","secondary":[]},"entry_seq":"637","search":{"evaluations":"11","notes":[]}}}"#
    );
}
