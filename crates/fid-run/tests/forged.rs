//! Forged-event oracle of the run ledger (REQ-CORE.RPL.RECON.FOLD-008 (g), AC-7 of
//! CORE.RPL.RECON.FOLD): every case of `fixtures/auto/run/bind/forged.json`, generated with its
//! expected verdicts by the independent reference `fixtures/auto/run/bind/make_cases.py`.
//!
//! Each case starts from a real plan (`prepare_run` on a lifecycle set through the
//! `AUTO.REC.LIFE` commands), on branch `run/1`, committed by its own principal under
//! `program.run@1`; then the case's entry and record overrides, events, lifecycle change and
//! replay are applied, and both the fold (`apply_entry`) and the commit-stage check
//! (`precommit`) must give the oracle's verdict.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{b120, digest, identified, prepare, World};
use fid_bus::error::BusError;
use fid_bus::handler::{Event, EventType};
use fid_bus::policy::projection::EntryOrigin;
use fid_bus::principal::{PrincipalId, PrincipalKind, PrincipalRef};
use fid_canon::CValue;
use fid_journal::branch::EventSource;
use fid_run::{run_started, Applied, RunEntry, RunFlags, RunLedger, RUN_COMMAND, RUN_STARTED};
use fid_types::{CommandId, SideEffect};
use std::collections::BTreeMap;

const PARAMS_COMMITMENT: &str = "sha256:abababababababababababababababababababababababababababababababab";
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

struct Case<'a>(&'a BTreeMap<String, CValue>);

impl Case<'_> {
    fn s(&self, k: &str) -> &str {
        st(&self.0[k])
    }
    fn o(&self, k: &str) -> &BTreeMap<String, CValue> {
        obj(&self.0[k])
    }
}

/// The world, principal and flags of a base.
fn base(name: &str) -> (World, bool, RunFlags) {
    match name {
        "agent_released" => (World::at("released"), false, RunFlags::default()),
        "human_trial" => (World::at("registered"), true, RunFlags { trial: true, override_deprecated: false }),
        "human_override" => (World::at("deprecated"), true, RunFlags { trial: false, override_deprecated: true }),
        other => panic!("base {other}"),
    }
}

fn entry_for(c: &Case<'_>, w: &World, human: bool) -> RunEntry {
    let p = if human { &w.alice } else { &w.agent };
    let mut e = RunEntry::for_call(p, digest(PARAMS_COMMITMENT), "run/1", digest(ENTRY_HASH)).unwrap();
    let ov = c.o("entry");
    let mut principal = e.origin.principal().clone();
    let mut command = e.origin.command().clone();
    let mut commitment = e.origin.params_commitment();
    for (k, v) in ov {
        match k.as_str() {
            "origin_principal" => principal = principal_ref(v),
            "origin_command" => command = CommandId::parse(st(v)).unwrap(),
            "params_commitment" => commitment = digest(st(v)),
            "journal_kind" => e.journal.kind = PrincipalKind::parse(st(v)).unwrap(),
            "journal_command" => e.journal.command = st(v).to_owned(),
            "journal_major" => {
                let CValue::Num(n) = v else { panic!("major") };
                e.journal.major = *n as u32;
            }
            "class" => e.journal.class = class(st(v)),
            "source" => e.journal.source = source(st(v)),
            "branch" => e.journal.branch = st(v).to_owned(),
            "promoted_from" => e.promoted_from = Some(digest(st(v))),
            other => panic!("entry override {other}"),
        }
    }
    e.origin = EntryOrigin::new(principal, command, commitment);
    e
}

fn events_for(c: &Case<'_>, w: &World, human: bool, flags: RunFlags) -> Vec<Event> {
    let p = if human { &w.alice } else { &w.agent };
    let plan = prepare(w, identified(), b120(), p, flags).unwrap();
    let record = plan.record.with_branch("run/1");
    let started = run_started(&record, digest(PARAMS_COMMITMENT)).unwrap();
    // Record overrides on the event body.
    let mut body = obj(started.body()).clone();
    let mut rec = obj(&body["record"]).clone();
    for (k, v) in c.o("record") {
        rec.insert(k.clone(), v.clone());
    }
    body.insert("record".into(), CValue::Obj(rec));
    let started = Event::new(EventType::new(RUN_STARTED).unwrap(), CValue::Obj(body));
    let CValue::Arr(types) = &c.0["events"] else { panic!("events") };
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
fn forged_events_get_the_oracles_verdicts() {
    let cases = common::json("fixtures/auto/run/bind/forged.json");
    let CValue::Arr(cases) = cases else { panic!("cases") };
    assert!(cases.len() >= 30, "at least the 30 cases of #378");
    let mut items = std::collections::BTreeSet::new();
    for c in &cases {
        let c = Case(obj(c));
        let name = c.s("name").to_owned();
        items.insert(c.s("item").to_owned());
        let (mut w, human, flags) = base(c.s("base"));
        let entry = entry_for(&c, &w, human);
        let events = events_for(&c, &w, human, flags);
        match c.s("lifecycle") {
            "as_prepared" => {}
            "deprecated" => w.life("deprecate"),
            other => panic!("lifecycle {other}"),
        }
        let mut ledger = RunLedger::new();
        if c.0["replay"] == CValue::Bool(true) {
            let first = ledger.apply_entry(&entry, &events, &w.store);
            assert_eq!(apply_word(&first), "admitted", "{name}: first application");
        }
        let before = ledger.clone();
        let pre = ledger.precommit(&entry, &events, &w.store);
        assert_eq!(precommit_word(&pre), c.s("precommit"), "{name}: precommit {pre:?}");
        let applied = ledger.apply_entry(&entry, &events, &w.store);
        assert_eq!(apply_word(&applied), c.s("apply"), "{name}: apply {applied:?}");
        if !matches!(applied, Ok(Applied::Admitted { .. })) {
            assert_eq!(ledger, before, "{name}: a refused or skipped entry changes nothing");
        }
    }
    for item in ["a", "b", "c", "d", "e"] {
        assert!(items.contains(item), "no case for FOLD-008 ({item})");
    }
}

/// Two runs of one program decided against the same released state are both admitted (a run
/// consumes nothing); a third prepared before a deprecation is refused at commit.
#[test]
fn concurrent_runs_and_a_deprecation_in_between() {
    let mut w = World::at("released");
    let cat = common::catalogue();
    let mut ids = common::Counting::default();
    let mut plans = Vec::new();
    {
        let mut ctx =
            fid_run::BindCtx { lifecycle: &w.store, catalogue: &cat, inputs: &common::Scans, run_ids: &mut ids };
        for _ in 0..3 {
            plans.push(fid_run::prepare_run(&mut ctx, identified(), b120(), &w.agent, RunFlags::default()).unwrap());
        }
    }
    let mut ledger = RunLedger::new();
    for (i, plan) in plans.iter().enumerate() {
        let branch = format!("run/{}", i + 1);
        let record = plan.record.clone().with_branch(&branch);
        let entry = RunEntry::for_call(&w.agent, digest(PARAMS_COMMITMENT), &branch, digest(ENTRY_HASH)).unwrap();
        let events = vec![run_started(&record, digest(PARAMS_COMMITMENT)).unwrap()];
        if i == 2 {
            w.life("deprecate");
            let pre = ledger.precommit(&entry, &events, &w.store);
            assert!(matches!(pre, Err(BusError::Conflict(..))), "{pre:?}");
            assert!(ledger.apply_entry(&entry, &events, &w.store).is_err());
        } else {
            ledger.precommit(&entry, &events, &w.store).unwrap();
            let applied = ledger.apply_entry(&entry, &events, &w.store).unwrap();
            assert_eq!(applied, Applied::Admitted { run_id: record.run_id() });
            assert_eq!(ledger.get(&record.run_id()).unwrap().0, &record);
        }
    }
    assert_eq!(ledger.len(), 2);
}

/// `RunStarted` is reserved to `program.run@1` in the registry (FOLD-008 (b)).
#[test]
fn run_started_is_reserved_to_program_run() {
    assert!(fid_registry::RESERVED_EFFECTS.contains(&(RUN_STARTED, Some((RUN_COMMAND, 1)))));
}
