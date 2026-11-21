//! Forged-event oracle of the run ledger (REQ-CORE.RPL.RECON.FOLD-008 (g), AC-7 of
//! CORE.RPL.RECON.FOLD): every case of `fixtures/auto/run/bind/forged.json`, written by the
//! independent reference `fixtures/auto/run/bind/make_cases.py` with verdicts hand-labelled
//! from FOLD-008 (a)-(g) and REQ-AUTO.RUN.BIND-001/002.
//!
//! Each case starts from a real plan (`prepare_run` on a lifecycle set through the
//! `AUTO.REC.LIFE` commands) on its branch `run/<run_id>`. The case's record overrides and
//! events are applied to the `RunStarted` event; the entry record (principal, command, class,
//! adding stage, branch, parameters commitment, `promoted_from`) is the one the case writes out
//! in full, its parameters commitment computed in Python. Then the lifecycle change and replay
//! are applied, and both the fold (`apply_entry`) and the commit-stage check (`precommit`) must
//! give the hand-labelled verdict.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{b120, digest, identified, prepare, World};
use fid_bus::error::BusError;
use fid_bus::handler::{Event, EventType};
use fid_bus::policy::projection::{params_commitment, EntryOrigin};
use fid_bus::principal::{Principal, PrincipalId, PrincipalKind, PrincipalRef};
use fid_canon::CValue;
use fid_journal::branch::{EntryOrigin as JournalOrigin, EventSource};
use fid_run::{
    prepare_run, run_branch, run_params, run_started, Applied, BindCtx, RunEntry, RunFlags, RunLedger, RunPlan,
    RunRecord, RUN_COMMAND, RUN_STARTED,
};
use fid_types::{CommandId, SideEffect};
use std::collections::BTreeMap;

const ENTRY_HASH: &str = "sha256:0101010101010101010101010101010101010101010101010101010101010101";

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("object: {other:?}"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("string: {other:?}"),
    }
}

fn principal_ref(v: &CValue) -> PrincipalRef {
    let o = obj(v);
    PrincipalRef { kind: PrincipalKind::parse(st(&o["kind"])).unwrap(), id: PrincipalId::new(st(&o["id"])).unwrap() }
}

fn class(s: &str) -> SideEffect {
    SideEffect::ALL.iter().copied().find(|c| c.as_str() == s).unwrap_or_else(|| panic!("class {s}"))
}

fn source(s: &str) -> EventSource {
    match s {
        "handler" => EventSource::Handler,
        "bus_route" => EventSource::BusRoute,
        "bus_release" => EventSource::BusRelease,
        other => panic!("source {other}"),
    }
}

/// The world and flags of a base, and whether its principal is the person.
fn base(name: &str) -> (World, bool, RunFlags) {
    match name {
        "agent_released" => (World::at("released"), false, RunFlags::default()),
        "human_trial" => (World::at("registered"), true, RunFlags { trial: true, override_deprecated: false }),
        "human_override" => (World::at("deprecated"), true, RunFlags { trial: false, override_deprecated: true }),
        other => panic!("base {other}"),
    }
}

/// The entry record exactly as the case writes it out.
fn entry_of(e: &BTreeMap<String, CValue>) -> RunEntry {
    let CValue::Num(major) = e["journal_major"] else { panic!("major") };
    RunEntry {
        origin: EntryOrigin::new(
            principal_ref(&e["origin_principal"]),
            CommandId::parse(st(&e["origin_command"])).unwrap(),
            digest(st(&e["params_commitment"])),
        ),
        journal: JournalOrigin {
            kind: PrincipalKind::parse(st(&e["journal_kind"])).unwrap(),
            command: st(&e["journal_command"]).to_owned(),
            major: major as u32,
            class: class(st(&e["class"])),
            branch: st(&e["branch"]).to_owned(),
            source: source(st(&e["source"])),
            human_assertion: None,
        },
        promoted_from: match &e["promoted_from"] {
            CValue::Null => None,
            v => Some(digest(st(v))),
        },
        hash: digest(ENTRY_HASH),
    }
}

fn events_for(c: &BTreeMap<String, CValue>, w: &World, human: bool, flags: RunFlags) -> Vec<Event> {
    let p = if human { &w.alice } else { &w.agent };
    let plan = prepare(w, identified(), b120(), p, flags).unwrap();
    let branch = run_branch(plan.record.run_id());
    let started = run_started(&plan.record.with_branch(&branch)).unwrap();
    let mut body = obj(started.body()).clone();
    let mut rec = obj(&body["record"]).clone();
    for (k, v) in obj(&c["record"]) {
        rec.insert(k.clone(), v.clone());
    }
    body.insert("record".into(), CValue::Obj(rec));
    let started = Event::new(EventType::new(RUN_STARTED).unwrap(), CValue::Obj(body));
    let CValue::Arr(types) = &c["events"] else { panic!("events") };
    types
        .iter()
        .map(|t| match st(t) {
            RUN_STARTED => started.clone(),
            other => Event::new(EventType::new(other).unwrap(), CValue::Obj(BTreeMap::new())),
        })
        .collect()
}

fn apply_word(r: &Result<Applied, BusError>) -> &'static str {
    match r {
        Ok(Applied::Ignored) => "ignored",
        Ok(Applied::Skipped { .. }) => "skipped",
        Ok(Applied::Admitted { .. }) => "admitted",
        Err(BusError::Internal(..)) => "refused",
        Err(BusError::Conflict(..)) => "conflict",
        Err(_) => "other",
    }
}

fn precommit_word(r: &Result<Applied, BusError>) -> &'static str {
    match r {
        Ok(Applied::Admitted { .. }) => "ok",
        other => apply_word(other),
    }
}

#[test]
fn forged_events_get_the_hand_labelled_verdicts() {
    let CValue::Arr(cases) = common::json("fixtures/auto/run/bind/forged.json") else { panic!("cases") };
    assert!(cases.len() >= 40, "{} cases", cases.len());
    let mut items = std::collections::BTreeSet::new();
    for c in &cases {
        let c = obj(c);
        let name = st(&c["name"]).to_owned();
        items.insert(st(&c["item"]).to_owned());
        let (mut w, human, flags) = base(st(&c["base"]));
        let entry = entry_of(obj(&c["entry"]));
        let events = events_for(c, &w, human, flags);
        match st(&c["lifecycle"]) {
            "as_prepared" => {}
            "deprecated" => w.life("deprecate"),
            other => panic!("lifecycle {other}"),
        }
        let mut ledger = RunLedger::new();
        if c["replay"] == CValue::Bool(true) {
            let first = ledger.apply_entry(&entry, &events, &w.store);
            assert_eq!(apply_word(&first), "admitted", "{name}: first application");
        }
        let before = ledger.clone();
        let pre = ledger.precommit(&entry, &events, &w.store);
        assert_eq!(precommit_word(&pre), st(&c["precommit"]), "{name}: precommit {pre:?}");
        let applied = ledger.apply_entry(&entry, &events, &w.store);
        assert_eq!(apply_word(&applied), st(&c["apply"]), "{name}: apply {applied:?}");
        if !matches!(applied, Ok(Applied::Admitted { .. })) {
            assert_eq!(ledger, before, "{name}: a refused or skipped entry changes nothing");
        }
    }
    for item in ["a", "b", "c", "d", "e"] {
        assert!(items.contains(item), "no case for FOLD-008 ({item})");
    }
}

/// The Rust parameters commitment of a real plan is the one the reference computes for it.
#[test]
fn parameters_commitment_matches_the_reference() {
    let CValue::Arr(cases) = common::json("fixtures/auto/run/bind/forged.json") else { panic!("cases") };
    let c = cases.iter().map(obj).find(|c| st(&c["name"]) == "base_agent_released").unwrap();
    let w = World::at("released");
    let plan = prepare(&w, identified(), b120(), &w.agent, RunFlags::default()).unwrap();
    let record = plan.record.clone().with_branch(&run_branch(plan.record.run_id()));
    let ours = params_commitment(&run_params(&record)).unwrap();
    assert_eq!(ours.as_str(), st(&obj(&c["entry"])["params_commitment"]));
}

fn start(p: &RunPlan, who: &Principal) -> (RunRecord, RunEntry, Vec<Event>) {
    let branch = run_branch(p.record.run_id());
    let record = p.record.clone().with_branch(&branch);
    let commit = params_commitment(&run_params(&record)).unwrap();
    let entry = RunEntry::for_call(who, commit, &branch, digest(ENTRY_HASH)).unwrap();
    let events = vec![run_started(&record).unwrap()];
    (record, entry, events)
}

fn step(ledger: &mut RunLedger, w: &World, entry: &RunEntry, events: &[Event]) -> (&'static str, &'static str) {
    let pre = precommit_word(&ledger.precommit(entry, events, &w.store));
    let applied = apply_word(&ledger.apply_entry(entry, events, &w.store));
    (pre, applied)
}

/// Fold domain: entries of all branches in commit order, each judged against the project's
/// (main) lifecycle as of that entry; the commit stage and the fold give the same verdict at
/// each step. Two runs decided against the same released state are both admitted (a run
/// consumes nothing); one prepared before a deprecation is refused; a person's override after
/// it is admitted.
#[test]
fn fold_over_all_branches_in_commit_order() {
    let mut w = World::at("released");
    let cat = common::catalogue();
    let mut ids = common::Counting::default();
    let plans: Vec<RunPlan> = {
        let mut ctx = BindCtx { lifecycle: &w.store, catalogue: &cat, inputs: &common::Scans, run_ids: &mut ids };
        (0..3).map(|_| prepare_run(&mut ctx, identified(), b120(), &w.agent, RunFlags::default()).unwrap()).collect()
    };
    let mut ledger = RunLedger::new();
    let mut verdicts = Vec::new();
    for p in &plans[..2] {
        let (_, entry, events) = start(p, &w.agent);
        verdicts.push(step(&mut ledger, &w, &entry, &events));
    }
    // A person deprecates the program on main; the third plan was decided against `released`.
    w.life("deprecate");
    let (_, entry, events) = start(&plans[2], &w.agent);
    verdicts.push(step(&mut ledger, &w, &entry, &events));
    // After the deprecation, a person's override run is admitted.
    let mut ids = common::Counting(10);
    let over = {
        let mut ctx = BindCtx { lifecycle: &w.store, catalogue: &cat, inputs: &common::Scans, run_ids: &mut ids };
        let flags = RunFlags { trial: false, override_deprecated: true };
        prepare_run(&mut ctx, identified(), b120(), &w.alice, flags).unwrap()
    };
    let (record, entry, events) = start(&over, &w.alice);
    verdicts.push(step(&mut ledger, &w, &entry, &events));
    assert_eq!(verdicts, vec![("ok", "admitted"), ("ok", "admitted"), ("conflict", "refused"), ("ok", "admitted")]);
    assert_eq!(ledger.len(), 3);
    assert!(ledger.get(&record.run_id()).unwrap().0.override_deprecated());
}

/// `RunStarted` is reserved to `program.run@1` in the registry (FOLD-008 (b)).
#[test]
fn run_started_is_reserved_to_program_run() {
    assert!(fid_registry::RESERVED_EFFECTS.contains(&(RUN_STARTED, Some((RUN_COMMAND, 1)))));
}
