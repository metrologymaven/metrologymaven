//! CORE.JRN.BR.NAME against the independent oracle `fixtures/core/jrn/br/name/cases.json`
//! (written by make_cases.py from the spec rules): name verdicts, the scripted calls of
//! AC-1, AC-2, AC-3, AC-5 with their exact events or v3 errors, the resulting branch list,
//! replay of the event log with entry origins (AC-6, table level), tampered and forged logs,
//! and the release protection of REQ-006 (AC-4, table level).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{to_canonical_bytes, CValue};
use fid_journal::branch::{
    plan_coediting, plan_create, plan_protect, release_protection, Actor, BranchError, BranchEvent, BranchName,
    Branches, EntryOrigin, EventSource, Owner, CREATE_BRANCH, LIST_BRANCHES, MAIN, PROTECT_BRANCH,
    SET_BRANCH_COEDITING,
};
use fid_journal::PrincipalKind;
use fid_types::{Digest, SideEffect};

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/br/name/cases.json");

fn cases() -> CValue {
    fid_canon::parse(&std::fs::read(CASES).unwrap()).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("missing {k}")),
        _ => panic!("not an object: {v:?}"),
    }
}

fn opt<'a>(v: &'a CValue, k: &str) -> Option<&'a CValue> {
    match v {
        CValue::Obj(o) => o.get(k),
        _ => None,
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("not an array: {v:?}"),
    }
}

fn s(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("not a string: {v:?}"),
    }
}

fn num(v: &CValue) -> u64 {
    match v {
        CValue::Num(n) => *n as u64,
        _ => panic!("not a number: {v:?}"),
    }
}

fn digest(v: &CValue) -> Digest {
    Digest::parse(s(v)).unwrap()
}

fn actor(v: &CValue) -> Actor {
    match s(v) {
        "human" => Actor::Human,
        "agent" => Actor::Agent,
        _ => Actor::Other,
    }
}

fn class(v: &str) -> SideEffect {
    match v {
        "R0" => SideEffect::R0,
        "W2" => SideEffect::W2,
        "W3" => SideEffect::W3,
        other => panic!("class {other}"),
    }
}

/// An entry origin as the oracle writes it.
fn origin(v: &CValue) -> EntryOrigin {
    EntryOrigin {
        kind: PrincipalKind::parse(s(get(v, "kind"))).unwrap(),
        command: s(get(v, "command")).to_string(),
        major: num(get(v, "major")) as u32,
        class: class(s(get(v, "class"))),
        branch: s(get(v, "branch")).to_string(),
        source: match s(get(v, "source")) {
            "handler" => EventSource::Handler,
            "bus_route" => EventSource::BusRoute,
            "bus_release" => EventSource::BusRelease,
            other => panic!("source {other}"),
        },
        human_assertion: None,
    }
}

/// The origin of a handler event for a call by `kind`.
fn handler_origin(kind: PrincipalKind, command: &str, cls: SideEffect) -> EntryOrigin {
    EntryOrigin {
        kind,
        command: command.to_string(),
        major: 1,
        class: cls,
        branch: MAIN.to_string(),
        source: EventSource::Handler,
        human_assertion: None,
    }
}

fn human(command: &str, cls: SideEffect) -> EntryOrigin {
    handler_origin(PrincipalKind::Human, command, cls)
}

struct Project {
    entries: Vec<Digest>,
    head: Digest,
}

fn project(c: &CValue) -> Project {
    let p = get(c, "project");
    Project { entries: arr(get(p, "entries")).iter().map(digest).collect(), head: digest(get(p, "main_head")) }
}

fn event_value(e: &BranchEvent) -> CValue {
    let mut o = std::collections::BTreeMap::new();
    o.insert("type".to_string(), CValue::Str(e.type_id().to_string()));
    o.insert("body".to_string(), e.body());
    CValue::Obj(o)
}

fn parse_event(v: &CValue) -> Result<BranchEvent, BranchError> {
    BranchEvent::from_parts(s(get(v, "type")), get(v, "body"))
}

/// The oracle's log: (origin, event) per committed event.
fn oracle_log(sc: &CValue) -> Vec<(EntryOrigin, BranchEvent)> {
    arr(get(sc, "log")).iter().map(|v| (origin(get(v, "origin")), parse_event(v).unwrap())).collect()
}

#[test]
fn names_match_the_oracle() {
    let c = cases();
    let names = arr(get(&c, "names"));
    assert!(names.len() >= 40);
    for n in names {
        let name = s(get(n, "name"));
        let ok = get(n, "ok") == &CValue::Bool(true);
        match BranchName::parse(name) {
            Ok(b) => {
                assert!(ok, "{name:?} accepted, oracle refuses");
                assert_eq!(b.as_str().as_bytes(), name.as_bytes(), "canonical bytes");
                let owner = if b.owner() == Owner::Agent { "agent" } else { "human" };
                assert_eq!(owner, s(get(n, "owner")), "{name:?}");
            }
            Err(e) => {
                assert!(!ok, "{name:?} refused ({e}), oracle accepts");
                assert_eq!((e.code(), e.reason()), ("validation", "bad_branch_name"), "{name:?}");
            }
        }
    }
}

/// The event and origin of one oracle call, or its refusal. The routing module's planner is
/// crate-private (unit-tested in `fid_journal::branch`); here its oracle events are taken as
/// committed with their bus_route origin.
fn run_call(
    b: &Branches,
    p: &Project,
    call: &CValue,
    want: &CValue,
) -> Result<Option<(EntryOrigin, BranchEvent)>, BranchError> {
    let is_head = |d: &Digest| p.entries.contains(d);
    let a = actor(get(call, "actor"));
    let kind = match a {
        Actor::Human => PrincipalKind::Human,
        Actor::Agent => PrincipalKind::Agent,
        Actor::Other => PrincipalKind::Script,
    };
    let with = |cmd: &str, cls, e: Option<BranchEvent>| e.map(|e| (handler_origin(kind, cmd, cls), e));
    match s(get(call, "op")) {
        "create" => plan_create(b, a, s(get(call, "name")), s(get(call, "from")), &is_head)
            .map(|e| with(CREATE_BRANCH, SideEffect::W2, Some(e))),
        "protect" => plan_protect(b, a, s(get(call, "branch"))).map(|e| with(PROTECT_BRANCH, SideEffect::W3, e)),
        "coediting" => plan_coediting(
            b,
            a,
            s(get(call, "branch")),
            s(get(call, "agent_session")),
            get(call, "allow") == &CValue::Bool(true),
        )
        .map(|e| with(SET_BRANCH_COEDITING, SideEffect::W2, e)),
        "agent_branch" => match opt(want, "event") {
            Some(e) => Ok(Some((origin(get(want, "origin")), parse_event(e).unwrap()))),
            None => Err(BranchError::BadName("routing refusal (unit-tested)".into())),
        },
        other => panic!("op {other}"),
    }
}

/// AC-1, AC-2, AC-3 (table half), AC-5: every call gives exactly the oracle's event, no-op or
/// v3 error, and the list equals the oracle's.
#[test]
fn scenario_matches_the_oracle() {
    let c = cases();
    let p = project(&c);
    let sc = get(&c, "scenario");
    let mut b = Branches::genesis(p.head);
    let mut log = Vec::new();
    for (i, call) in arr(get(sc, "calls")).iter().enumerate() {
        let want = get(call, "expect");
        let routing = s(get(get(call, "call"), "op")) == "agent_branch";
        let got = run_call(&b, &p, get(call, "call"), want);
        match (got, opt(want, "event"), opt(want, "error")) {
            (Ok(Some((o, e))), Some(w), None) => {
                assert_eq!(&event_value(&e), w, "call {i}");
                assert_eq!(o, origin(get(want, "origin")), "call {i}");
                b.apply(&o, &e).unwrap();
                log.push((o, e));
            }
            (Ok(None), None, None) => assert!(opt(want, "noop").is_some(), "call {i}"),
            (Err(_), None, Some(_)) if routing => {}
            (Err(e), None, Some(w)) => {
                assert_eq!(e.code(), s(get(w, "code")), "call {i}: {e}");
                assert_eq!(e.reason(), s(get(w, "reason")), "call {i}: {e}");
            }
            (got, _, _) => panic!("call {i}: got {got:?}, expected {want:?}"),
        }
    }
    assert_eq!(get(&b.to_value(), "branches"), get(sc, "branches"));
    // AC-1: trial/a has main's head and created_from; AC-5: coediting(main) holds S.
    assert_eq!(b.head("trial/a"), b.head(MAIN));
    assert_eq!(b.get("trial/a").unwrap().created_from, Some(p.head));
    assert!(b.coediting(MAIN).unwrap().iter().any(|s| s.as_str() == "S"));
    assert_eq!(b.is_protected(MAIN), Some(true));
    assert_eq!(oracle_log(sc), log);
}

/// AC-6 at table level: the committed events of every entry, in append order with their
/// origins, replay to the same table, byte for byte, every time.
#[test]
fn replay_of_the_log_is_identical() {
    let c = cases();
    let p = project(&c);
    let sc = get(&c, "scenario");
    let log = oracle_log(sc);
    let pairs = || log.iter().map(|(o, e)| (o, e));
    let once = Branches::replay(p.head, pairs()).unwrap();
    let twice = Branches::replay(p.head, pairs()).unwrap();
    assert_eq!(once, twice);
    assert_eq!(to_canonical_bytes(&once.to_value()).unwrap(), to_canonical_bytes(&twice.to_value()).unwrap());
    assert_eq!(get(&once.to_value(), "branches"), get(sc, "branches"));
    for (_, e) in &log {
        assert_eq!(&BranchEvent::from_parts(e.type_id(), &e.body()).unwrap(), e);
    }
    // A replayed (duplicated) entry is refused, not folded twice.
    let mut dup = log.clone();
    dup.push(log[0].clone());
    let r = Branches::replay(p.head, dup.iter().map(|(o, e)| (o, e)));
    assert_eq!(r.unwrap_err().reason(), "invariant_violated");
}

/// Tampered and forged logs fail closed at the oracle's position with
/// internal/invariant_violated: malformed or contradictory events, and well-formed events
/// whose entry origin (principal, command, class, branch, adding stage) could not produce them.
#[test]
fn tampered_and_forged_logs_are_refused() {
    let c = cases();
    let p = project(&c);
    let tamper = arr(get(&c, "tamper"));
    assert!(
        tamper.iter().filter(|t| s(get(t, "why")).contains("by a") || s(get(t, "why")).contains("from")).count() >= 10
    );
    for t in tamper {
        let why = s(get(t, "why"));
        let at = num(get(t, "at")) as usize;
        let mut b = Branches::genesis(p.head);
        let mut failed_at = None;
        for (i, v) in arr(get(t, "log")).iter().enumerate() {
            let o = origin(get(v, "origin"));
            match parse_event(v).and_then(|e| b.apply(&o, &e)) {
                Ok(()) => {}
                Err(e) => {
                    assert_eq!((e.code(), e.reason()), ("internal", "invariant_violated"), "{why}: {e}");
                    failed_at = Some(i);
                    break;
                }
            }
        }
        assert_eq!(failed_at, Some(at), "{why}");
    }
}

/// REQ-006 at table level: only the first W3 record on main adds BranchProtected{release},
/// never `journal.protect_branch@1` itself; the bus-added event folds only with its
/// bus_release origin.
#[test]
fn release_protects_main_once() {
    let c = cases();
    let p = project(&c);
    for r in arr(get(&c, "release")) {
        let mut b = Branches::genesis(p.head);
        let branch = s(get(r, "branch"));
        if branch != MAIN {
            let e = plan_create(&b, Actor::Human, branch, p.head.as_str(), &|_: &Digest| true).unwrap();
            b.apply(&human(CREATE_BRANCH, SideEffect::W2), &e).unwrap();
        }
        if get(r, "protected_before") == &CValue::Bool(true) {
            let e = plan_protect(&b, Actor::Human, MAIN).unwrap().unwrap();
            b.apply(&human(PROTECT_BRANCH, SideEffect::W3), &e).unwrap();
        }
        let command = s(get(r, "command"));
        let cls = class(s(get(r, "class")));
        let got = release_protection(&b, branch, cls, command);
        assert_eq!(got.is_some(), get(r, "adds") == &CValue::Bool(true), "{r:?}");
        if let Some(e) = got {
            let want = BranchEvent::from_parts(
                "BranchProtected",
                &fid_canon::parse(br#"{"cause":"release","name":"main"}"#).unwrap(),
            )
            .unwrap();
            assert_eq!(e, want);
            let bus = EntryOrigin {
                kind: PrincipalKind::Human,
                command: command.to_string(),
                major: 1,
                class: cls,
                branch: branch.to_string(),
                source: EventSource::BusRelease,
                human_assertion: None,
            };
            b.apply(&bus, &e).unwrap();
            assert_eq!(b.is_protected(MAIN), Some(true));
            assert!(release_protection(&b, MAIN, SideEffect::W3, command).is_none());
        }
    }
}

/// Decision on #356: a human `protect-branch main` gives exactly one event, the manual one;
/// the commit stage adds no release protection for it.
#[test]
fn protect_branch_main_gives_exactly_one_event() {
    let p = project(&cases());
    let mut b = Branches::genesis(p.head);
    let mut events = Vec::new();
    events.extend(plan_protect(&b, Actor::Human, MAIN).unwrap());
    events.extend(release_protection(&b, MAIN, SideEffect::W3, PROTECT_BRANCH));
    assert_eq!(events.len(), 1);
    b.apply(&human(PROTECT_BRANCH, SideEffect::W3), &events[0]).unwrap();
    assert!(release_protection(&b, MAIN, SideEffect::W3, PROTECT_BRANCH).is_none());
    assert_eq!(b.is_protected(MAIN), Some(true));
}

/// REQ-005: no API clears a protection; a protected branch stays protected under any
/// later event.
#[test]
fn protection_is_permanent() {
    let p = project(&cases());
    let mut b = Branches::genesis(p.head);
    b.apply(&human(PROTECT_BRANCH, SideEffect::W3), &plan_protect(&b, Actor::Human, MAIN).unwrap().unwrap()).unwrap();
    assert_eq!(plan_protect(&b, Actor::Human, MAIN).unwrap(), None);
    let e = plan_coediting(&b, Actor::Human, MAIN, "S", true).unwrap().unwrap();
    b.apply(&human(SET_BRANCH_COEDITING, SideEffect::W2), &e).unwrap();
    b.set_head(&BranchName::parse(MAIN).unwrap(), p.entries[0]).unwrap();
    assert_eq!(b.is_protected(MAIN), Some(true));
}

/// The four definitions parse, carry the spec's classes, and keep human-only and W3 commands
/// off the plain MCP surface (request only).
#[test]
fn command_definitions() {
    let defs = fid_journal::branch::command_defs().unwrap();
    let ids: Vec<&str> = defs.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, [CREATE_BRANCH, LIST_BRANCHES, PROTECT_BRANCH, SET_BRANCH_COEDITING]);
    let class: Vec<SideEffect> = defs.iter().map(|d| d.side_effect).collect();
    assert_eq!(class, [SideEffect::W2, SideEffect::R0, SideEffect::W3, SideEffect::W2]);
    assert!(defs[3].human_only);
    assert!(!defs[0].human_only && !defs[2].human_only);
    // The registry's checks accept them (reserved effects included: each command declares
    // exactly the event its entry may carry).
    let decls: Vec<fid_registry::CommandDecl> = defs.iter().map(|d| fid_registry::CommandDecl(d.to_value())).collect();
    assert_eq!(fid_registry::validate(&decls), vec![]);
}

/// Listing is sorted by name bytes and stays fast for 10,000 branches (budget < 20 ms in
/// release [unverified]; this debug-build test only bounds it loosely and prints it).
#[test]
fn list_ten_thousand_branches() {
    let p = project(&cases());
    let mut b = Branches::genesis(p.head);
    let o = human(CREATE_BRANCH, SideEffect::W2);
    for i in (0..10_000).rev() {
        let e = plan_create(&b, Actor::Human, &format!("load/{i:05}"), p.head.as_str(), &|_: &Digest| true).unwrap();
        b.apply(&o, &e).unwrap();
    }
    let start = std::time::Instant::now();
    let v = b.to_value();
    let took = start.elapsed();
    eprintln!("listed 10001 branches in {took:?}");
    let names: Vec<&str> = arr(get(&v, "branches")).iter().map(|x| s(get(x, "name"))).collect();
    assert_eq!(names.len(), 10_001);
    assert!(names.windows(2).all(|w| w[0].as_bytes() < w[1].as_bytes()));
    assert!(took < std::time::Duration::from_secs(5));
}
