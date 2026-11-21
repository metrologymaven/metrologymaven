//! CORE.JRN.UNDO.SUPER, model half: the planner and the origin-bound fold of
//! `journal.supersede_record@1` against the independent oracle
//! (`fixtures/core/jrn/undo/super/cases.json`, from `make_fixtures.py`): every planned call,
//! every journal folded (the forged cases cover REQ-CORE.RPL.RECON.FOLD-008 (a) to (e)), the
//! reserved event, the definition and determinism (AC-4 at planner and fold level).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_journal::branch::{Actor, EntryOrigin, EventSource};
use fid_journal::supersede::{
    check_reason, command_defs, plan, SupersedeEntry, SupersedeError, SupersedeParams, Supersession, Supersessions,
    MAX_REASON_CHARS, RECORD_SUPERSEDED, SUPERSEDE_RECORD,
};
use fid_journal::PrincipalKind;
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/undo/super");

type Obj = BTreeMap<String, CValue>;
type Events = Vec<(String, CValue)>;

fn load(name: &str) -> CValue {
    parse(&std::fs::read(format!("{ROOT}/{name}")).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("object expected, got {other:?}"),
    }
}

fn get<'a>(o: &'a Obj, k: &str) -> &'a CValue {
    o.get(k).unwrap_or_else(|| panic!("missing {k}"))
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("field {k} is {other:?}"),
    }
}

fn arr<'a>(o: &'a Obj, k: &str) -> &'a [CValue] {
    match o.get(k) {
        Some(CValue::Arr(a)) => a,
        other => panic!("field {k} is {other:?}"),
    }
}

fn digest(s: &str) -> Digest {
    Digest::parse(s).unwrap()
}

fn opt_digest(o: &Obj, k: &str) -> Option<Digest> {
    match get(o, k) {
        CValue::Null => None,
        CValue::Str(s) => Some(digest(s)),
        other => panic!("{k} is {other:?}"),
    }
}

fn source(s: &str) -> EventSource {
    match s {
        "handler" => EventSource::Handler,
        "bus_route" => EventSource::BusRoute,
        "bus_release" => EventSource::BusRelease,
        other => panic!("{other}"),
    }
}

/// One oracle entry as the fold takes it.
fn entry(v: &CValue) -> (SupersedeEntry, Events) {
    let o = obj(v);
    let CValue::Num(major) = get(o, "major") else { panic!() };
    let e = SupersedeEntry {
        origin: EntryOrigin {
            kind: PrincipalKind::parse(st(o, "kind")).unwrap(),
            command: st(o, "command").to_string(),
            major: *major as u32,
            class: SideEffect::parse(st(o, "class")).unwrap(),
            branch: st(o, "branch").to_string(),
            source: source(st(o, "source")),
            human_assertion: None,
        },
        hash: digest(st(o, "hash")),
        promoted_from: opt_digest(o, "promoted_from"),
        reason_commit: opt_digest(o, "reason_commit"),
        outputs: arr(o, "outputs")
            .iter()
            .map(|d| match d {
                CValue::Str(s) => digest(s),
                other => panic!("{other:?}"),
            })
            .collect(),
        head_before: opt_digest(o, "head_before"),
    };
    let events =
        arr(o, "events").iter().map(|x| (st(obj(x), "type").to_string(), get(obj(x), "body").clone())).collect();
    (e, events)
}

fn entries(list: &[CValue]) -> Vec<(SupersedeEntry, Events)> {
    list.iter().map(entry).collect()
}

fn fold(list: &[(SupersedeEntry, Events)]) -> Result<Supersessions, (usize, SupersedeError)> {
    let mut s = Supersessions::new();
    for (i, (e, ev)) in list.iter().enumerate() {
        s.apply_entry(e, ev).map_err(|err| (i, err))?;
    }
    Ok(s)
}

fn logs() -> BTreeMap<String, Vec<(SupersedeEntry, Events)>> {
    let v = load("two_releases.json");
    obj(get(obj(&v), "logs"))
        .iter()
        .map(|(k, l)| {
            let CValue::Arr(l) = l else { panic!() };
            (k.clone(), entries(l))
        })
        .collect()
}

fn actor(s: &str) -> Actor {
    match s {
        "human" => Actor::Human,
        "agent" => Actor::Agent,
        "script" => Actor::Other,
        other => panic!("{other}"),
    }
}

fn params(v: &CValue) -> SupersedeParams {
    let o = obj(v);
    SupersedeParams { old: digest(st(o, "old")), new: digest(st(o, "new")), reason: st(o, "reason").to_string() }
}

#[test]
fn every_journal_folds_to_the_oracle_outcome() {
    let c = load("cases.json");
    let folds = arr(obj(&c), "folds");
    assert!(folds.len() >= 30);
    for case in folds {
        let case = obj(case);
        let name = st(case, "name");
        let list = entries(arr(case, "entries"));
        let expect = obj(get(case, "expect"));
        match (fold(&list), st(expect, "outcome")) {
            (Ok(s), "ok") => {
                for x in arr(expect, "superseded") {
                    let x = obj(x);
                    let got = s.superseded_by(st(x, "branch"), &digest(st(x, "old"))).unwrap();
                    assert_eq!(got.new, digest(st(x, "new")), "{name}");
                    assert_eq!(got.entry, digest(st(x, "entry")), "{name}");
                    assert_eq!(got.reason_commit, digest(st(x, "reason_commit")), "{name}");
                }
                let all: usize = ["main", "trial"].iter().map(|b| s.on_branch(b).len()).sum();
                assert_eq!(all, arr(expect, "superseded").len(), "{name}: supersessions");
                for x in arr(expect, "released") {
                    let x = obj(x);
                    assert_eq!(
                        s.released(st(x, "branch"), &digest(st(x, "record"))),
                        Some(digest(st(x, "entry"))),
                        "{name}"
                    );
                }
            }
            (Err((i, e)), "refused") => {
                let CValue::Num(want) = get(expect, "entry") else { panic!() };
                assert_eq!(i, *want as usize, "{name}: refused at the wrong entry ({e})");
                assert_eq!(e.bus_code(), ("internal", "invariant_violated"), "{name}");
            }
            (got, want) => panic!("{name}: expected {want}, got {:?}", got.map(|_| ())),
        }
    }
}

/// A refused entry folds nothing: the state is the one before it.
#[test]
fn a_refused_entry_leaves_the_state_unchanged() {
    let c = load("cases.json");
    for case in arr(obj(&c), "folds") {
        let case = obj(case);
        let list = entries(arr(case, "entries"));
        if let Err((i, _)) = fold(&list) {
            let before = fold(&list[..i]).unwrap();
            let mut s = before.clone();
            assert!(s.apply_entry(&list[i].0, &list[i].1).is_err());
            assert_eq!(s, before, "{}", st(case, "name"));
        }
    }
}

#[test]
fn every_call_plans_to_the_oracle_outcome() {
    let c = load("cases.json");
    let logs = logs();
    let plans = arr(obj(&c), "plans");
    assert!(plans.len() >= 15);
    for case in plans {
        let case = obj(case);
        let name = st(case, "name");
        let state = fold(&logs[st(case, "log")]).unwrap();
        let head = opt_digest(case, "head");
        let got = plan(actor(st(case, "actor")), &state, st(case, "branch"), head, &params(get(case, "params")));
        let expect = obj(get(case, "expect"));
        match (got, expect.get("event"), expect.get("error")) {
            (Ok(s), Some(want), None) => {
                let want = obj(want);
                assert_eq!(st(want, "type"), RECORD_SUPERSEDED, "{name}");
                assert_eq!(&s.body(), get(want, "body"), "{name}");
                assert_eq!(Supersession::from_body(&s.body()).unwrap(), s, "{name}: body round trip");
                // Precommit equals fold: the event folds under the record its entry will have.
                let mut next = state.clone();
                let e = SupersedeEntry {
                    origin: EntryOrigin {
                        kind: PrincipalKind::Human,
                        command: SUPERSEDE_RECORD.into(),
                        major: 1,
                        class: SideEffect::W3,
                        branch: st(case, "branch").into(),
                        source: EventSource::Handler,
                        human_assertion: None,
                    },
                    hash: digest(&format!("sha256:{}", "1".repeat(64))),
                    promoted_from: None,
                    reason_commit: Some(digest(&format!("sha256:{}", "2".repeat(64)))),
                    outputs: Vec::new(),
                    head_before: head,
                };
                next.apply_entry(&e, &[(RECORD_SUPERSEDED.into(), s.body())]).unwrap();
                assert_eq!(next.superseded_by(st(case, "branch"), &s.old).unwrap().new, s.new, "{name}");
            }
            (Err(e), None, Some(CValue::Arr(want))) => {
                let want: Vec<&str> =
                    want.iter().map(|w| if let CValue::Str(s) = w { s.as_str() } else { panic!() }).collect();
                assert_eq!([e.code(), e.reason()], [want[0], want[1]], "{name}: {e}");
            }
            (got, _, _) => panic!("{name}: unexpected {got:?}"),
        }
    }
}

/// AC-1: after the supersession, R1's release entry is unchanged and still visible, and
/// `superseded_by(R1)` names R2 and the supersession entry.
#[test]
fn ac1_the_original_stays_visible() {
    let c = load("cases.json");
    let case = arr(obj(&c), "folds").iter().map(obj).find(|c| st(c, "name") == "ac1_supersede").unwrap();
    let list = entries(arr(case, "entries"));
    let s = fold(&list).unwrap();
    let records = load("two_releases.json");
    let r = obj(get(obj(&records), "records"));
    let (r1, r2) = (digest(st(r, "R1")), digest(st(r, "R2")));
    assert_eq!(s.released("main", &r1), Some(list[0].0.hash));
    assert_eq!(s.released("main", &r2), Some(list[1].0.hash));
    let by = s.superseded_by("main", &r1).unwrap();
    assert_eq!((by.new, by.entry), (r2, list[2].0.hash));
    assert!(s.superseded_by("main", &r2).is_none());
    assert_eq!(fold(&list[..2]).unwrap().released("main", &r1), Some(list[0].0.hash));
}

/// AC-4 at planner and fold level: the same call against the same journal gives the same
/// event bytes, and folding the same journal twice gives the same state.
#[test]
fn ac4_planning_and_folding_are_deterministic() {
    let c = load("cases.json");
    let logs = logs();
    for case in arr(obj(&c), "plans") {
        let case = obj(case);
        let state = fold(&logs[st(case, "log")]).unwrap();
        let run = || {
            plan(
                actor(st(case, "actor")),
                &state,
                st(case, "branch"),
                opt_digest(case, "head"),
                &params(get(case, "params")),
            )
            .map(|s| to_canonical_bytes(&s.body()).unwrap())
        };
        assert_eq!(run(), run(), "{}", st(case, "name"));
    }
    for case in arr(obj(&c), "folds") {
        let list = entries(arr(obj(case), "entries"));
        assert_eq!(fold(&list).ok(), fold(&list).ok());
    }
}

#[test]
fn the_reason_has_1_to_2000_characters() {
    assert!(check_reason("").is_err());
    assert!(check_reason("x").is_ok());
    assert!(check_reason(&"\u{e9}".repeat(MAX_REASON_CHARS)).is_ok());
    assert_eq!(check_reason(&"x".repeat(MAX_REASON_CHARS + 1)).unwrap_err().bus_code(), ("validation", "schema"));
}

#[test]
fn the_event_is_reserved_and_declared() {
    assert!(fid_registry::RESERVED_EFFECTS.contains(&(RECORD_SUPERSEDED, Some((SUPERSEDE_RECORD, 1)))));
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 1);
    let d = &defs[0];
    assert_eq!(d.id, SUPERSEDE_RECORD);
    let v = d.to_value();
    let o = obj(&v);
    assert_eq!(st(o, "side_effect"), "W3");
    assert_eq!(st(o, "mcp_projection"), "request_only");
    assert_eq!(st(o, "determinism"), "D0");
    assert_eq!(st(o, "undo"), "not_undoable");
    assert_eq!(get(o, "effects"), &CValue::Arr(vec![CValue::Str(RECORD_SUPERSEDED.into())]));
}

#[test]
fn error_codes() {
    let all = [
        (SupersedeError::ClassForbidden, ("policy", "class_forbidden")),
        (SupersedeError::Schema("x".into()), ("validation", "schema")),
        (SupersedeError::NotSupersedable("x".into()), ("precondition", "not_supersedable")),
        (SupersedeError::Tampered("x".into()), ("internal", "invariant_violated")),
    ];
    for (e, want) in all {
        assert_eq!(e.bus_code(), want);
        assert!(e.to_string().starts_with(&format!("{}/{}", want.0, want.1)));
    }
}
