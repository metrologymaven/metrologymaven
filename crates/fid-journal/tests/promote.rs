//! Acceptance criteria of spec node CORE.JRN.BR.PROMO and the pinned rules of module
//! `promote`: every planned call, every forged or replayed event log, the reserved event
//! types, the definitions, and the v3 mapping of every refusal.
//!
//! Expected outcomes come from the independent Python reference
//! `fixtures/core/jrn/br/promo/make_fixtures.py` (which reuses the independent BR.DIFF
//! reference for the verified diff). The Rebaser and AssertionCheck are fakes driven by the
//! oracle's cases.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_journal::branch::{
    Actor, BranchEvent, BranchName, Branches, EntryOrigin, EventSource, ProtectCause, CREATE_BRANCH, MAIN,
    PROTECT_BRANCH,
};
use fid_journal::diff::{diff_rows, DiffOptions};
use fid_journal::promote::{
    command_defs, plan, AssertionCheck, PromoEvent, PromoteError, PromoteOp, PromoteParams, Promotions, RebaseStep,
    Rebaser, StepOutcome, BRANCH_PROMOTED, MAX_STEPS, PROMOTE_BRANCH, PROMOTE_PROTECTED_BRANCH, PROMOTION_REQUESTED,
    PROTECTED_BRANCH_PROMOTED, REQUEST_PROMOTE_BRANCH,
};
use fid_journal::verify::{ReadError, StoredEntry};
use fid_journal::PrincipalKind;
use fid_types::{Digest, SideEffect};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const STREAM: &str = "record";
const BUS_ERRORS_V3: &[u8] = include_bytes!("../../../fixtures/core/bus/pipe/err/bus-errors.v3.json");

type Obj = BTreeMap<String, CValue>;

fn load(rel: &str) -> CValue {
    parse(&std::fs::read(Path::new(ROOT).join(rel)).unwrap()).unwrap()
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

fn num(v: &CValue) -> u64 {
    match v {
        CValue::Num(n) if n.fract() == 0.0 => *n as u64,
        other => panic!("integer expected, got {other:?}"),
    }
}

fn digest(s: &str) -> Digest {
    Digest::parse(s).unwrap()
}

// ------------------------------------------------------------------ the fixture

struct Fixture {
    journals: BTreeMap<String, Vec<StoredEntry>>,
    table: Branches,
}

fn fixture() -> Fixture {
    let v = load("fixtures/core/jrn/br/promo/journals.json");
    let f = obj(&v);
    let mut journals = BTreeMap::new();
    for (name, rows) in obj(get(f, "journals")) {
        let CValue::Arr(rows) = rows else { panic!() };
        let rows = rows
            .iter()
            .map(|r| {
                let r = obj(r);
                let header = st(r, "header").as_bytes().to_vec();
                StoredEntry {
                    seq: num(get(r, "seq")) as i64,
                    hash: st(r, "hash").into(),
                    stream: st(r, "stream").into(),
                    branch: st(r, "branch").into(),
                    kind: st(r, "kind").into(),
                    prev_hash: Some(st(r, "prev_hash").into()),
                    header_len: header.len() as u64,
                    header,
                    refs: Vec::new(),
                }
            })
            .collect::<Vec<_>>();
        journals.insert(name.clone(), rows);
    }
    let first = digest(&journals["ff"][0].hash);
    let mut table = Branches::genesis(first);
    let human = |command: &str, class: SideEffect| EntryOrigin {
        kind: PrincipalKind::Human,
        command: command.into(),
        major: 1,
        class,
        branch: MAIN.into(),
        source: EventSource::Handler,
        human_assertion: None,
    };
    for b in arr(f, "branches") {
        let b = obj(b);
        let name = BranchName::parse(st(b, "name")).unwrap();
        if name.as_str() != MAIN {
            let from = digest(st(b, "created_from"));
            let origin = if name.as_str().starts_with("agent/") {
                EntryOrigin {
                    kind: PrincipalKind::Agent,
                    command: "feature.set_nominal".into(),
                    major: 1,
                    class: SideEffect::W2,
                    branch: name.as_str().into(),
                    source: EventSource::BusRoute,
                    human_assertion: None,
                }
            } else {
                human(CREATE_BRANCH, SideEffect::W2)
            };
            table.apply(&origin, &BranchEvent::Created { name: name.clone(), from }).unwrap();
        }
        if matches!(get(b, "protected"), CValue::Bool(true)) {
            let e = BranchEvent::Protected { name, cause: ProtectCause::Manual };
            table.apply(&human(PROTECT_BRANCH, SideEffect::W3), &e).unwrap();
        }
    }
    Fixture { journals, table }
}

fn cases() -> Obj {
    obj(&load("fixtures/core/jrn/br/promo/cases.json")).clone()
}

// ------------------------------------------------------------------ fakes

struct FakeRebaser {
    steps: Vec<RebaseStep>,
    calls: AtomicUsize,
}

impl Rebaser for FakeRebaser {
    fn rebase(&self, _: &Digest, _: Option<&Digest>, _: &dyn Fn() -> bool) -> Result<Vec<RebaseStep>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.steps.clone())
    }
}

/// Confirms exactly one (pre-image, assertion) pair.
struct FakeCheck {
    pre_image: Digest,
    assertion: String,
}

impl AssertionCheck for FakeCheck {
    fn confirm(&self, pre_image: &Digest, assertion: &str) -> bool {
        *pre_image == self.pre_image && assertion == self.assertion
    }
}

fn rebaser_of(c: &Obj) -> Option<FakeRebaser> {
    match get(c, "rebaser") {
        CValue::Null => None,
        CValue::Arr(steps) => Some(FakeRebaser {
            steps: steps
                .iter()
                .map(|s| {
                    let s = obj(s);
                    RebaseStep {
                        command: st(s, "command").into(),
                        digest: digest(st(s, "digest")),
                        outcome: StepOutcome::parse(st(s, "outcome")).unwrap(),
                    }
                })
                .collect(),
            calls: AtomicUsize::new(0),
        }),
        other => panic!("{other:?}"),
    }
}

fn check_of(c: &Obj) -> Option<FakeCheck> {
    match get(c, "assertion_check") {
        CValue::Null => None,
        CValue::Obj(o) => {
            let CValue::Arr(pair) = get(o, "confirms") else { panic!() };
            let (CValue::Str(p), CValue::Str(a)) = (&pair[0], &pair[1]) else { panic!() };
            Some(FakeCheck { pre_image: digest(p), assertion: a.clone() })
        }
        other => panic!("{other:?}"),
    }
}

fn op_of(command: &str) -> PromoteOp {
    match command {
        PROMOTE_BRANCH => PromoteOp::Promote,
        PROMOTE_PROTECTED_BRANCH => PromoteOp::Protected,
        REQUEST_PROMOTE_BRANCH => PromoteOp::Request,
        other => panic!("{other}"),
    }
}

fn params_of(p: &Obj) -> PromoteParams {
    PromoteParams {
        source: st(p, "source").into(),
        source_head: digest(st(p, "source_head")),
        target: st(p, "target").into(),
        target_head: match get(p, "target_head") {
            CValue::Null => None,
            CValue::Str(s) => Some(digest(s)),
            other => panic!("{other:?}"),
        },
        diff: digest(st(p, "diff")),
        assertion: p.get("assertion").map(|a| if let CValue::Str(s) = a { s.clone() } else { panic!() }),
        note: p.get("note").map(|a| if let CValue::Str(s) = a { s.clone() } else { panic!() }),
    }
}

fn run_case(fx: &Fixture, c: &Obj) -> (Result<PromoEvent, PromoteError>, usize) {
    let rows = &fx.journals[st(c, "journal")];
    let p = params_of(obj(get(c, "params")));
    let actor = Actor::of(PrincipalKind::parse(st(c, "actor")).unwrap());
    let rebaser = rebaser_of(c);
    let check = check_of(c);
    let table = &fx.table;
    let mut diff_of = || {
        diff_rows(
            STREAM,
            rows.iter().cloned().map(Ok::<_, ReadError>),
            table,
            &p.source,
            &p.target,
            &DiffOptions::default(),
        )
    };
    let got = plan(
        op_of(st(c, "command")),
        actor,
        table,
        &p,
        &mut diff_of,
        rebaser.as_ref().map(|r| r as &dyn Rebaser),
        check.as_ref().map(|c| c as &dyn AssertionCheck),
        &|| false,
    );
    (got, rebaser.map_or(0, |r| r.calls.load(Ordering::SeqCst)))
}

// ------------------------------------------------------------------ plans

#[test]
fn every_planned_call_reaches_the_oracle_outcome() {
    let fx = fixture();
    let c = cases();
    let mut n = 0;
    for case in arr(&c, "plans") {
        let case = obj(case);
        let name = st(case, "name");
        let expect = obj(get(case, "expect"));
        let (got, calls) = run_case(&fx, case);
        match (got, expect.get("event"), expect.get("error")) {
            (Ok(e), Some(want), None) => {
                let want = obj(want);
                assert_eq!(e.type_id(), st(want, "type"), "{name}");
                assert_eq!(&e.body(), get(want, "body"), "{name}");
                // The body round-trips through the strict parser.
                assert_eq!(PromoEvent::from_parts(e.type_id(), &e.body()).unwrap(), e, "{name}");
                // AC-1: a fast-forward never calls the Rebaser.
                if let PromoEvent::Promoted(p) = &e {
                    let ff = p.mode == fid_journal::promote::Mode::FastForward;
                    assert_eq!(calls, usize::from(!ff), "{name}: rebaser calls");
                }
            }
            (Err(e), None, Some(want)) => {
                let want = obj(want);
                assert_eq!((e.code(), e.reason()), (st(want, "code"), st(want, "reason")), "{name}: {e}");
                if let Some(step) = want.get("step") {
                    let PromoteError::DivergentStep { step: got, .. } = e else { panic!("{name}: {e}") };
                    assert_eq!(got as u64, num(step), "{name}");
                    assert!(e.detail().contains(&format!("step {got}")), "{name}: the result names the step");
                }
            }
            (got, _, _) => panic!("{name}: got {got:?}, expected {expect:?}"),
        }
        n += 1;
    }
    assert!(n >= 30);
}

/// AC-6 (planner and fold level): the same call gives the same bytes; folding the same log
/// twice gives the same state.
#[test]
fn ac6_planning_and_folding_are_deterministic() {
    let fx = fixture();
    let c = cases();
    for case in arr(&c, "plans") {
        let case = obj(case);
        let (a, _) = run_case(&fx, case);
        let (b, _) = run_case(&fx, case);
        match (a, b) {
            (Ok(a), Ok(b)) => {
                assert_eq!(to_canonical_bytes(&a.body()).unwrap(), to_canonical_bytes(&b.body()).unwrap())
            }
            (Err(a), Err(b)) => assert_eq!(a, b),
            other => panic!("{other:?}"),
        }
    }
    let genuine = arr(&c, "folds").iter().map(obj).find(|f| st(f, "name") == "genuine_promotions_and_request").unwrap();
    assert_eq!(fold(&fx, genuine).unwrap(), fold(&fx, genuine).unwrap());
}

#[test]
fn a_cancelled_rebase_is_cancelled() {
    let fx = fixture();
    let c = cases();
    let case = arr(&c, "plans").iter().map(obj).find(|f| st(f, "name") == "ac2_rebased_identical").unwrap();
    let rows = &fx.journals["fork"];
    let p = params_of(obj(get(case, "params")));
    let r = rebaser_of(case).unwrap();
    let mut diff_of =
        || diff_rows(STREAM, rows.iter().cloned().map(Ok), &fx.table, &p.source, &p.target, &DiffOptions::default());
    let e = plan(PromoteOp::Promote, Actor::Human, &fx.table, &p, &mut diff_of, Some(&r), None, &|| true).unwrap_err();
    assert_eq!(e.bus_code(), ("cancelled", "requested"));
    // Too many steps.
    let many = FakeRebaser { steps: vec![r.steps[0].clone(); MAX_STEPS + 1], calls: AtomicUsize::new(0) };
    let e =
        plan(PromoteOp::Promote, Actor::Human, &fx.table, &p, &mut diff_of, Some(&many), None, &|| false).unwrap_err();
    assert_eq!(e.bus_code(), ("validation", "input_limit"));
}

// ------------------------------------------------------------------ folds

fn origin_of(o: &Obj) -> EntryOrigin {
    EntryOrigin {
        kind: PrincipalKind::parse(st(o, "kind")).unwrap(),
        command: st(o, "command").into(),
        major: num(get(o, "major")) as u32,
        class: SideEffect::parse(st(o, "class")).unwrap(),
        branch: st(o, "branch").into(),
        source: match st(o, "source") {
            "handler" => EventSource::Handler,
            "bus_route" => EventSource::BusRoute,
            "bus_release" => EventSource::BusRelease,
            other => panic!("{other}"),
        },
        human_assertion: match get(o, "human_assertion") {
            CValue::Null => None,
            CValue::Str(s) => Some(s.clone()),
            other => panic!("{other:?}"),
        },
    }
}

fn heads_of(entry: &Obj) -> BTreeMap<String, Option<Digest>> {
    obj(get(entry, "heads"))
        .iter()
        .map(|(k, v)| {
            let h = match v {
                CValue::Null => None,
                CValue::Str(s) => Some(digest(s)),
                other => panic!("{other:?}"),
            };
            (k.clone(), h)
        })
        .collect()
}

/// The fold case's AssertionCheck: confirms exactly the listed pairs.
struct PairsCheck(Vec<(Digest, String)>);

impl AssertionCheck for PairsCheck {
    fn confirm(&self, pre_image: &Digest, assertion: &str) -> bool {
        self.0.iter().any(|(p, a)| p == pre_image && a == assertion)
    }
}

fn check_for(case: &Obj) -> Option<PairsCheck> {
    match get(case, "assertion_check") {
        CValue::Null => None,
        CValue::Obj(o) => {
            let CValue::Arr(pairs) = get(o, "confirms") else { panic!() };
            Some(PairsCheck(
                pairs
                    .iter()
                    .map(|p| {
                        let CValue::Arr(p) = p else { panic!() };
                        let (CValue::Str(d), CValue::Str(a)) = (&p[0], &p[1]) else { panic!() };
                        (digest(d), a.clone())
                    })
                    .collect(),
            ))
        }
        other => panic!("{other:?}"),
    }
}

fn fold(fx: &Fixture, case: &Obj) -> Result<Promotions, String> {
    let mut p = Promotions::default();
    for entry in arr(case, "log") {
        let entry = obj(entry);
        let events: Result<Vec<PromoEvent>, _> = arr(entry, "events")
            .iter()
            .map(|e| {
                let e = obj(e);
                PromoEvent::from_parts(st(e, "type"), get(e, "body"))
            })
            .collect();
        let events = events.map_err(|e| e.reason().to_string())?;
        let check = check_for(case);
        p.apply_entry(
            &fx.table,
            &heads_of(entry),
            check.as_ref().map(|c| c as &dyn AssertionCheck),
            &origin_of(obj(get(entry, "origin"))),
            &events,
        )
        .map_err(|e| e.reason().to_string())?;
    }
    Ok(p)
}

#[test]
fn every_forged_and_replayed_log_reaches_the_oracle_outcome() {
    let fx = fixture();
    let c = cases();
    let mut n = 0;
    for case in arr(&c, "folds") {
        let case = obj(case);
        let name = st(case, "name");
        let expect = obj(get(case, "expect"));
        match (fold(&fx, case), expect.get("ok"), expect.get("error")) {
            (Ok(p), Some(want), None) => {
                let want = obj(want);
                assert_eq!(p.promotions.len() as u64, num(get(want, "promotions")), "{name}");
                let kinds: Vec<&str> = p.requests.iter().map(|r| r.requester.as_str()).collect();
                let want_kinds: Vec<&str> = arr(want, "requests")
                    .iter()
                    .map(|k| if let CValue::Str(s) = k { s.as_str() } else { panic!() })
                    .collect();
                assert_eq!(kinds, want_kinds, "{name}: requester from the entry's principal");
            }
            (Err(reason), None, Some(CValue::Str(want))) => assert_eq!(&reason, want, "{name}"),
            (got, _, _) => panic!("{name}: got {got:?}, expected {expect:?}"),
        }
        n += 1;
    }
    assert!(n >= 25);
}

/// A refused entry folds nothing: the state is the one before it.
#[test]
fn a_refused_entry_leaves_the_fold_unchanged() {
    let fx = fixture();
    let c = cases();
    let genuine = arr(&c, "folds").iter().map(obj).find(|f| st(f, "name") == "genuine_promotions_and_request").unwrap();
    let mut p = fold(&fx, genuine).unwrap();
    let before = p.clone();
    let first = obj(&arr(genuine, "log")[0]);
    let ev = obj(&arr(first, "events")[0]);
    let e = PromoEvent::from_parts(st(ev, "type"), get(ev, "body")).unwrap();
    let origin = origin_of(obj(get(first, "origin")));
    // The same promotion again, with the heads before the first: a replay.
    assert!(p.apply_entry(&fx.table, &heads_of(first), None, &origin, &[e]).is_err());
    assert_eq!(p, before);
}

// ------------------------------------------------------------------ definitions and errors

#[test]
fn the_three_commands_are_declared_as_the_spec_says() {
    let defs = command_defs().unwrap();
    let ids: Vec<&str> = defs.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, [PROMOTE_BRANCH, PROMOTE_PROTECTED_BRANCH, REQUEST_PROMOTE_BRANCH]);
    let by: BTreeMap<&str, &fid_registry::CommandDef> = defs.iter().map(|d| (d.id.as_str(), d)).collect();
    let p = by[PROMOTE_BRANCH];
    assert_eq!((p.side_effect, p.human_only), (SideEffect::W2, true));
    assert_eq!(p.effects, [BRANCH_PROMOTED]);
    let w3 = by[PROMOTE_PROTECTED_BRANCH];
    assert_eq!(w3.side_effect, SideEffect::W3);
    assert_eq!(w3.effects, [PROTECTED_BRANCH_PROMOTED]);
    let r = by[REQUEST_PROMOTE_BRANCH];
    assert_eq!((r.side_effect, r.human_only), (SideEffect::W1, false));
    assert_eq!(r.effects, [PROMOTION_REQUESTED]);
}

/// The three event types are reserved to their commands in the registry.
#[test]
fn promotion_events_are_reserved_to_their_commands() {
    for (effect, owner) in [
        (BRANCH_PROMOTED, PROMOTE_BRANCH),
        (PROTECTED_BRANCH_PROMOTED, PROMOTE_PROTECTED_BRANCH),
        (PROMOTION_REQUESTED, REQUEST_PROMOTE_BRANCH),
    ] {
        assert!(fid_registry::RESERVED_EFFECTS.contains(&(effect, Some((owner, 1)))), "{effect}");
    }
}

#[test]
fn every_refusal_maps_into_bus_catalogue_v3() {
    let v = parse(BUS_ERRORS_V3).unwrap();
    let cat: BTreeMap<String, BTreeSet<String>> = obj(get(obj(&v), "codes"))
        .iter()
        .map(|(c, rs)| {
            let CValue::Arr(rs) = rs else { panic!() };
            (c.clone(), rs.iter().map(|r| if let CValue::Str(s) = r { s.clone() } else { panic!() }).collect())
        })
        .collect();
    for e in all_errors() {
        let (code, reason) = e.bus_code();
        assert!(cat.get(code).is_some_and(|rs| rs.contains(reason)), "{e:?} -> {code}/{reason}");
    }
}

fn all_errors() -> Vec<PromoteError> {
    vec![
        PromoteError::HumanOnly,
        PromoteError::ClassForbidden,
        PromoteError::Schema("x".into()),
        PromoteError::UnknownBranch("x".into()),
        PromoteError::NotPromotable("x".into()),
        PromoteError::TargetProtected("x".into()),
        PromoteError::HeadMismatch("x".into()),
        PromoteError::DigestMismatch,
        PromoteError::CapabilityMissing("Rebaser"),
        PromoteError::NoAssertion,
        PromoteError::DivergentStep { step: 2, outcome: StepOutcome::Divergent },
        PromoteError::InputLimit("x".into()),
        PromoteError::Rebase("x".into()),
        PromoteError::Cancelled,
        PromoteError::Diff(fid_journal::diff::DiffError::UnknownBranch("x".into())),
    ]
}
