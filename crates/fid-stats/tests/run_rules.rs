//! RPT.STATS.SPC.RULES tests. The cases in `fixtures/rpt/stats/rule_cases.json` and
//! `fixtures/rpt/stats/spc_imr_30.json` are hand-chosen and hand-labelled; the expected
//! violations, thresholds (IEEE 754 bits), load verdicts and digests come from the independent
//! Python reference `fixtures/rpt/stats/make_rule_cases.py` (a whole-window reading of the rules,
//! not the kernel's sliding counters), never from running this crate. CI runs this file on
//! x86-64, ARM64 and wasm32 (wasmtime), so the digest tests are the cross-target identity of AC-6.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{parse_json, Json};
use fid_num::rng::{Address, Cursor};
use fid_stats::{
    default_rule_set, evaluate_rules, shipped_rule_set, Lines, LinesReason, Primitive, ReviewStatus, Rule,
    RuleEvaluation, RuleSet, RuleStatus, RulesetReason, StatsError, DEFAULT_RULE_SET, MAX_RULES, MAX_RULESET_BYTES,
    SHIPPED_RULE_SETS,
};

fn fixture(name: &str) -> Json {
    let file = format!("{}/fixtures/rpt/stats/{name}", fid_testkit::WORKSPACE_ROOT);
    parse_json(&std::fs::read_to_string(file).unwrap())
}

fn hex_f64(s: &str) -> f64 {
    assert_eq!(s.len(), 16, "{s}");
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn hex(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn digest(text: &str) -> String {
    let got = fid_testkit::sha256_hex(text.as_bytes());
    got.strip_prefix("sha256:").unwrap_or(&got).to_owned()
}

fn lines_of(j: &Json) -> Lines {
    let v = j.arr();
    assert_eq!(v.len(), 3);
    Lines { centre: hex_f64(v[0].str()), ucl: hex_f64(v[1].str()), lcl: hex_f64(v[2].str()) }
}

fn error_token(e: StatsError) -> String {
    match e {
        StatsError::BadLines { reason } => format!("{}:{}", e.code(), reason.as_str()),
        StatsError::RulesetInvalid { reason } => format!("{}:{}", e.code(), reason.as_str()),
        StatsError::NonFinite { index } => format!("{}:{index}", e.code()),
        StatsError::InsufficientData | StatsError::TooManyPoints => e.code().to_owned(),
        other => panic!("unexpected error kind {other}"),
    }
}

fn status_token(s: RuleStatus) -> String {
    match s {
        RuleStatus::Evaluated => "evaluated".to_owned(),
        RuleStatus::TooFewPoints { needed } => format!("too_few_points:{needed}"),
    }
}

/// The digest block of one result, as `make_rule_cases.py` writes it.
fn block(id: &str, r: &Result<RuleEvaluation, StatsError>) -> String {
    match r {
        Err(e) => format!("{id} error:{}\n", error_token(*e)),
        Ok(ev) => {
            let mut s = format!("{id} {}\n", ev.violations.len());
            for v in &ev.violations {
                s.push_str(&format!("v {} {} {} {}\n", v.index, v.rule_id, v.from, v.to));
            }
            for o in &ev.rules {
                let (u, l) = o.zone.map_or(("-".to_owned(), "-".to_owned()), |(u, l)| (hex(u), hex(l)));
                s.push_str(&format!("r {} {} {} {u} {l}\n", o.rule_id, status_token(o.status), o.violations));
            }
            s
        }
    }
}

/// The shape every successful result must have (REQ-005, REQ-006).
fn check_invariants(id: &str, set: &RuleSet, n: usize, ev: &RuleEvaluation) {
    assert_eq!(ev.set_id, set.id(), "{id}");
    assert_eq!(ev.set_version, set.version(), "{id}");
    assert_eq!(ev.review, set.review(), "{id}");
    assert_eq!(ev.n_points as usize, n, "{id}");
    assert_eq!(ev.rules.len(), set.rules().len(), "{id}");
    let pos = |rid: &str| set.rules().iter().position(|r| r.id() == rid).unwrap();
    for w in ev.violations.windows(2) {
        assert!((w[0].index, pos(&w[0].rule_id)) < (w[1].index, pos(&w[1].rule_id)), "{id}: ordered");
    }
    for v in &ev.violations {
        assert_eq!(v.to, v.index, "{id}");
        assert!(v.from <= v.to, "{id}");
        assert_eq!(v.set_id, set.id(), "{id}");
        assert_eq!(v.set_version, set.version(), "{id}");
        assert_eq!(v.review, set.review(), "{id}");
    }
    for (o, r) in ev.rules.iter().zip(set.rules()) {
        assert_eq!(o.rule_id, r.id(), "{id}");
        assert_eq!(o.primitive, r.primitive(), "{id}");
        let raised = ev.violations.iter().filter(|v| v.rule_id == r.id()).count();
        assert_eq!(o.violations as usize, raised, "{id}");
        if let RuleStatus::TooFewPoints { needed } = o.status {
            assert!(n < needed as usize, "{id}");
            assert_eq!(raised, 0, "{id}: a rule with too few points cannot fire");
        }
    }
}

/// AC-1, AC-2, AC-3, REQ-005/006 and the refusals: every fixture case, violations, rule
/// outcomes and thresholds bit for bit, or the refusal with its reason; then the digest.
#[test]
fn eval_cases_and_cross_target_digest() {
    let fx = fixture("rule_cases.json");
    let cases = fx.get("eval_cases").arr();
    assert!(cases.len() >= 35, "{}", cases.len());
    let mut text = String::new();
    for c in cases {
        let id = c.get("id").str();
        let lines = lines_of(c.get("lines"));
        let points: Vec<f64> = c.get("points").arr().iter().map(|v| hex_f64(v.str())).collect();
        let set = RuleSet::parse(c.get("ruleset").str().as_bytes()).unwrap();
        let want = c.get("expect");
        let got = evaluate_rules(&points, &lines, &set);
        match &got {
            Err(e) => {
                assert!(!want.get("error").is_null(), "{id}: unexpected error {e}");
                assert_eq!(error_token(*e), want.get("error").str(), "{id}");
            }
            Ok(ev) => {
                assert!(want.get("error").is_null(), "{id}: expected {:?}, got {ev:?}", want.get("error"));
                let viol: Vec<(u32, String, u32, u32)> = want
                    .get("violations")
                    .arr()
                    .iter()
                    .map(|v| {
                        let v = v.arr();
                        (v[0].num() as u32, v[1].str().to_owned(), v[2].num() as u32, v[3].num() as u32)
                    })
                    .collect();
                let mine: Vec<(u32, String, u32, u32)> =
                    ev.violations.iter().map(|v| (v.index, v.rule_id.clone(), v.from, v.to)).collect();
                assert_eq!(mine, viol, "{id}: violations");
                for (o, w) in ev.rules.iter().zip(want.get("rules").arr()) {
                    let w = w.arr();
                    assert_eq!(o.rule_id, w[0].str(), "{id}");
                    assert_eq!(status_token(o.status), w[1].str(), "{id}");
                    assert_eq!(o.violations as usize, w[2].num() as usize, "{id}");
                    match o.zone {
                        None => assert!(w[3].is_null() && w[4].is_null(), "{id}"),
                        Some((u, l)) => {
                            assert_eq!(hex(u), w[3].str(), "{id}: upper");
                            assert_eq!(hex(l), w[4].str(), "{id}: lower");
                        }
                    }
                }
                check_invariants(id, &set, points.len(), ev);
            }
        }
        text.push_str(&block(id, &got));
    }
    assert_eq!(digest(&text), fx.get("eval_digest").str(), "cross-target result digest");
}

/// REQ-003, REQ-007 and AC-5: every load case is accepted with its canonical form, or refused
/// with `stats.ruleset_invalid` and the hand-labelled reason.
#[test]
fn load_cases() {
    let fx = fixture("rule_cases.json");
    let cases = fx.get("load_cases").arr();
    assert!(cases.len() >= 70, "{}", cases.len());
    for c in cases {
        let id = c.get("id").str();
        let mut bytes = c.get("text").str().as_bytes().to_vec();
        if !c.get("pad_to").is_null() {
            bytes.resize(c.get("pad_to").num() as usize, b' ');
        }
        let want = c.get("expect").str();
        match RuleSet::parse(&bytes) {
            Ok(set) => {
                assert_eq!(want, "ok", "{id}: accepted");
                let canon = set.to_canonical_json();
                assert_eq!(canon, c.get("canonical").str(), "{id}: canonical form");
                let again = RuleSet::parse(canon.as_bytes()).unwrap();
                assert_eq!(again, set, "{id}: round trip");
                assert_eq!(again.to_canonical_json(), canon, "{id}");
            }
            Err(e) => {
                assert_eq!(e.code(), "stats.ruleset_invalid", "{id}");
                match e {
                    StatsError::RulesetInvalid { reason } => assert_eq!(reason.as_str(), want, "{id}"),
                    other => panic!("{id}: {other}"),
                }
            }
        }
    }
}

/// AC-5 written out: a rule naming primitive `MAGIC` is refused, so no rule can be evaluated.
#[test]
fn ac5_magic_primitive_refused() {
    let text = br#"{"id":"x","review":"UNVERIFIED","rules":[{"id":"1","primitive":"MAGIC","z":3}],"schema":"fid.stats.ruleset/1","version":1}"#;
    let e = RuleSet::parse(text).unwrap_err();
    assert_eq!(e, StatsError::RulesetInvalid { reason: RulesetReason::UnknownPrimitive });
    assert_eq!(e.to_string(), "stats.ruleset_invalid: unknown_primitive");
    assert_eq!(
        RuleSet::parse(&[0xff, 0xfe]).unwrap_err(),
        StatsError::RulesetInvalid { reason: RulesetReason::Malformed }
    );
    let big = vec![b' '; MAX_RULESET_BYTES + 1];
    assert_eq!(RuleSet::parse(&big).unwrap_err(), StatsError::RulesetInvalid { reason: RulesetReason::TooLarge });
}

/// REQ-004: the shipped sets, hand-checked against the parameters the leaf names; each embedded
/// file is its own canonical encoding; every set is `UNVERIFIED`.
#[test]
fn shipped_sets() {
    assert_eq!(DEFAULT_RULE_SET, ("beyond_limits", 1));
    let d = default_rule_set().unwrap();
    assert_eq!((d.id(), d.version()), DEFAULT_RULE_SET);
    assert_eq!(d.rules().len(), 1);
    assert_eq!(d.rules()[0].primitive(), Primitive::Beyond { z: 3.0 });
    for (id, version, bytes) in SHIPPED_RULE_SETS {
        let s = shipped_rule_set(id, version).unwrap();
        assert_eq!(s.id(), id);
        assert_eq!(s.version(), version);
        assert_eq!(s.review(), ReviewStatus::Unverified, "{id}");
        assert_eq!(s.to_canonical_json().as_bytes(), bytes, "{id}: embedded file is canonical");
    }
    let we = shipped_rule_set("western_electric", 1).unwrap();
    let we: Vec<Primitive> = we.rules().iter().map(Rule::primitive).collect();
    assert_eq!(
        we,
        vec![
            Primitive::Beyond { z: 3.0 },
            Primitive::KOfMBeyond { k: 2, m: 3, z: 2.0 },
            Primitive::KOfMBeyond { k: 4, m: 5, z: 1.0 },
            Primitive::RunSameSide { len: 8 },
        ]
    );
    let nelson = shipped_rule_set("nelson", 1).unwrap();
    let ids: Vec<&str> = nelson.rules().iter().map(Rule::id).collect();
    assert_eq!(ids, ["1", "2", "3", "4", "5", "6", "7", "8"]);
    let nelson: Vec<Primitive> = nelson.rules().iter().map(Rule::primitive).collect();
    assert_eq!(
        nelson,
        vec![
            Primitive::Beyond { z: 3.0 },
            Primitive::RunSameSide { len: 9 },
            Primitive::Trend { len: 6 },
            Primitive::Alternating { len: 14 },
            Primitive::KOfMBeyond { k: 2, m: 3, z: 2.0 },
            Primitive::KOfMBeyond { k: 4, m: 5, z: 1.0 },
            Primitive::RunWithin { len: 15, z: 1.0 },
            Primitive::RunOutside { len: 8, z: 1.0 },
        ]
    );
    let unknown = StatsError::RulesetInvalid { reason: RulesetReason::UnknownRuleSet };
    assert_eq!(shipped_rule_set("nelson", 2).unwrap_err(), unknown);
    assert_eq!(shipped_rule_set("magic", 1).unwrap_err(), unknown);
}

/// AC-4: `beyond_limits@1` on the 30-point individuals series with one point above its UCL
/// returns exactly one violation.
#[test]
fn ac4_default_set_on_imr_30() {
    let fx = fixture("spc_imr_30.json");
    let values: Vec<f64> = fx.get("value_bits").arr().iter().map(|v| hex_f64(v.str())).collect();
    assert_eq!(values.len(), 30);
    for (v, t) in values.iter().zip(fx.get("values").arr()) {
        assert_eq!(*v, t.str().parse::<f64>().unwrap());
    }
    let l = fx.get("lines");
    let lines = Lines {
        centre: hex_f64(l.get("centre").str()),
        ucl: hex_f64(l.get("ucl").str()),
        lcl: hex_f64(l.get("lcl").str()),
    };
    let set = default_rule_set().unwrap();
    let ev = evaluate_rules(&values, &lines, &set).unwrap();
    assert_eq!(ev.violations.len(), 1);
    let v = &ev.violations[0];
    assert_eq!((v.index, v.rule_id.as_str(), v.from, v.to), (17, "1", 17, 17));
    assert_eq!((v.set_id.as_str(), v.set_version, v.review), ("beyond_limits", 1, ReviewStatus::Unverified));
    assert!(values[17] > lines.ucl);
    assert_eq!(ev.rules[0].zone, Some((lines.ucl, lines.lcl)), "BEYOND{{3}} is the plain control-limit test");
    check_invariants("imr_30", &set, 30, &ev);
}

/// AC-6: 10 000 points from the fid-num Philox stream (seed 11), evaluated with every shipped
/// set (`nelson@1` among them); the violations and the digests equal the reference's on every
/// target.
#[test]
fn ac6_random_10k_cross_target_digest() {
    let fx = fixture("rule_cases.json");
    let rnd = fx.get("random_10k");
    assert_eq!(rnd.get("seed").num() as u64, 11);
    let addr = Address { seed: 11, stream: rnd.get("stream").num() as u64 };
    let n = rnd.get("n").num() as usize;
    assert_eq!(n, 10_000);
    let draws = rnd.get("draws_per_point").num() as usize;
    let mut cur = Cursor::new(addr, 0);
    let points: Vec<f64> = (0..n)
        .map(|_| {
            let mut s = 0.0;
            for _ in 0..draws {
                s += cur.uniform_f64();
            }
            s - 6.0
        })
        .collect();
    for (p, w) in points.iter().zip(rnd.get("first_points").arr()) {
        assert_eq!(hex(*p), w.str());
    }
    let lines = lines_of(rnd.get("lines"));
    let sets = rnd.get("sets").arr();
    assert_eq!(sets.len(), SHIPPED_RULE_SETS.len());
    let mut saw_nelson = false;
    for want in sets {
        let name = want.get("set").str();
        saw_nelson |= name == "nelson";
        let set = shipped_rule_set(name, want.get("version").num() as u32).unwrap();
        let got = evaluate_rules(&points, &lines, &set);
        let ev = got.as_ref().unwrap();
        assert_eq!(ev.violations.len(), want.get("violations").num() as usize, "{name}");
        for (o, w) in ev.rules.iter().zip(want.get("per_rule").arr()) {
            assert_eq!(o.rule_id, w.arr()[0].str(), "{name}");
            assert_eq!(o.violations as usize, w.arr()[1].num() as usize, "{name} rule {}", o.rule_id);
            assert_eq!(o.status, RuleStatus::Evaluated);
        }
        check_invariants(name, &set, n, ev);
        let text = block(&format!("{name}@1"), &got);
        assert_eq!(digest(&text), want.get("digest").str(), "{name}: cross-target digest");
    }
    assert!(saw_nelson);
}

/// The programmatic constructors apply the same rules as the loader: only a valid set exists.
#[test]
fn constructors_refuse_invalid_sets() {
    let reason = |e: StatsError| match e {
        StatsError::RulesetInvalid { reason } => reason,
        other => panic!("{other}"),
    };
    let bad = RulesetReason::BadParameter;
    for p in [
        Primitive::Beyond { z: f64::NAN },
        Primitive::Beyond { z: 0.0 },
        Primitive::Beyond { z: -0.0 },
        Primitive::Beyond { z: f64::INFINITY },
        Primitive::Beyond { z: 1000.5 },
        Primitive::KOfMBeyond { k: 3, m: 2, z: 2.0 },
        Primitive::KOfMBeyond { k: 0, m: 2, z: 2.0 },
        Primitive::RunSameSide { len: 1 },
        Primitive::Trend { len: 2 },
        Primitive::Alternating { len: 2 },
        Primitive::RunWithin { len: 1001, z: 1.0 },
        Primitive::RunOutside { len: 8, z: f64::NAN },
    ] {
        assert_eq!(reason(Rule::new("r", p).unwrap_err()), bad, "{p:?}");
    }
    assert_eq!(reason(Rule::new("R", Primitive::Beyond { z: 3.0 }).unwrap_err()), RulesetReason::BadRuleId);
    assert_eq!(reason(Rule::new("", Primitive::Beyond { z: 3.0 }).unwrap_err()), RulesetReason::BadRuleId);
    let r = Rule::new("r", Primitive::Beyond { z: 3.0 }).unwrap();
    let u = ReviewStatus::Unverified;
    assert_eq!(reason(RuleSet::new("x", 1, u, vec![]).unwrap_err()), RulesetReason::NoRules);
    assert_eq!(reason(RuleSet::new("X", 1, u, vec![r.clone()]).unwrap_err()), RulesetReason::BadId);
    assert_eq!(reason(RuleSet::new("x", 0, u, vec![r.clone()]).unwrap_err()), RulesetReason::BadVersion);
    assert_eq!(
        reason(RuleSet::new("x", 1, u, vec![r.clone(), r.clone()]).unwrap_err()),
        RulesetReason::DuplicateRuleId
    );
    let many: Vec<Rule> =
        (0..=MAX_RULES).map(|i| Rule::new(&format!("r{i}"), Primitive::Beyond { z: 3.0 }).unwrap()).collect();
    assert_eq!(reason(RuleSet::new("x", 1, u, many).unwrap_err()), RulesetReason::TooManyRules);
    let s = RuleSet::new("x", 1, u, vec![r]).unwrap();
    assert_eq!(RuleSet::parse(s.to_canonical_json().as_bytes()).unwrap(), s);
}

/// Codes and messages of the new refusals; a `-0.0` centre is the `+0.0` centre.
#[test]
fn codes_and_signed_zero() {
    let e = StatsError::BadLines { reason: LinesReason::UclNotAboveCentre };
    assert_eq!(e.code(), "stats.bad_lines");
    assert_eq!(e.to_string(), "stats.bad_lines: ucl_not_above_centre");
    assert_eq!(StatsError::TooManyPoints.code(), "stats.too_many_points");
    let set = default_rule_set().unwrap();
    let a = evaluate_rules(&[0.0, 3.5], &Lines { centre: -0.0, ucl: 3.0, lcl: -3.0 }, &set).unwrap();
    let b = evaluate_rules(&[0.0, 3.5], &Lines { centre: 0.0, ucl: 3.0, lcl: -3.0 }, &set).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.violations.len(), 1);
}
