//! Acceptance criteria of spec node AUTO.RUN.EXEC.RETGT (AC-1 .. AC-4) and the guard's edges.
//!
//! Plans are real: `prepare_run` on `fixtures/program/retarget/manual-pick.json` (a person's
//! trial run of the draft), with inputs hashed from `fixtures/scans/`. Step IDs, the step marked
//! not re-targetable, scan digests and every decision come from
//! `fixtures/program/retarget/expected.json`, written by the independent Python reference
//! `fixtures/program/retarget/make_fixtures.py` (digests from hashlib, decisions hand-labelled
//! from the leaf's REQ-001..003).
//!
//! Partial build (NODE.md). `AUTO.PROG.FMT` has no per-step `retargetable` field yet, so
//! `prepare_run` lists every step re-targetable; each test applies the fixture's marking of `s4`
//! to the plan, standing in for `AUTO.REC.LIFT`. No stored program carries a recorded default
//! digest either, so `prepare_run` records none and every case is checked twice: `today` (every
//! bound input re-targets the run) and `recorded` (the fixture's digest put into the plan,
//! standing in for FMT).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{catalogue, json, prepare, read, Counting, World};
use fid_bus::principal::Principal;
use fid_canon::CValue;
use fid_program::{binding_from_json, Program, Uuid7};
use fid_run::{
    prepare_run, BindCtx, BindingSource, GateDecision, InputSource, NotRetargetable, ProgramRef, RetargetGuard,
    RunFlags, RunPlan, StepGate, StepInfo, StepKey, StepResult, NOT_RETARGETABLE,
};
use fid_types::{CommandId, SideEffect};
use std::collections::BTreeMap;
use std::io::Read;

const TRIAL: RunFlags = RunFlags { trial: true, override_deprecated: false };

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("object: {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("array: {other:?}"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("string: {other:?}"),
    }
}

fn num(v: &CValue) -> u32 {
    match v {
        CValue::Num(n) => *n as u32,
        CValue::I64(n) => u32::try_from(*n).unwrap(),
        CValue::U64(n) => u32::try_from(*n).unwrap(),
        other => panic!("number: {other:?}"),
    }
}

struct Fixture {
    ex: BTreeMap<String, CValue>,
}

impl Fixture {
    fn load() -> Fixture {
        Fixture { ex: obj(&json("fixtures/program/retarget/expected.json")).clone() }
    }
    fn step(&self, name: &str) -> Uuid7 {
        Uuid7::parse(st(&obj(&self.ex["steps"])[name])).unwrap()
    }
    fn blocked(&self) -> Vec<Uuid7> {
        arr(&self.ex["not_retargetable"]).iter().map(|s| self.step(st(s))).collect()
    }
    fn case(&self, name: &str) -> &BTreeMap<String, CValue> {
        arr(&self.ex["cases"]).iter().map(obj).find(|c| st(&c["name"]) == name).unwrap_or_else(|| panic!("{name}"))
    }
    fn scan(&self, name: &str) -> String {
        st(&obj(&self.ex["scan_sha256"])[name]).to_owned()
    }
}

fn program() -> Program {
    Program::parse(&read("fixtures/program/retarget/manual-pick.json")).unwrap()
}

/// The fixture's marking of not-re-targetable steps, applied to the plan (AUTO.REC.LIFT's job).
fn mark(fx: &Fixture, plan: &mut RunPlan) {
    let blocked = fx.blocked();
    for (id, ok) in &mut plan.steps_retargetable {
        if blocked.contains(id) {
            *ok = false;
        }
    }
}

fn binding_json(v: &CValue) -> String {
    String::from_utf8(fid_canon::to_canonical_bytes(v).unwrap()).unwrap()
}

/// A person's trial run of the draft with the case's binding, s4 marked.
fn plan_for(fx: &Fixture, w: &World, binding: &CValue) -> RunPlan {
    let p = program();
    let b = binding_from_json(binding_json(binding).as_bytes()).unwrap();
    let mut plan = prepare(w, ProgramRef::Draft(&p), BindingSource::Binding(b), &w.alice, TRIAL).unwrap();
    // prepare_run marks every step re-targetable (no FMT field yet), in document order.
    let ids: Vec<Uuid7> = plan.steps_retargetable.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, ["s1", "s2", "s3", "s4"].map(|s| fx.step(s)).to_vec());
    assert!(plan.steps_retargetable.iter().all(|(_, ok)| *ok));
    mark(fx, &mut plan);
    plan
}

fn info() -> StepInfo {
    StepInfo { command: Some(CommandId::parse("geo.select_indices@1").unwrap()), class: Some(SideEffect::R1) }
}

fn expected_decision(d: &CValue) -> GateDecision {
    match d {
        CValue::Str(s) if s == "allow" => GateDecision::Allow,
        CValue::Obj(o) => {
            assert_eq!(st(&o["code"]), NOT_RETARGETABLE);
            let detail = obj(&o["detail"]);
            let step = StepKey {
                step_id: Uuid7::parse(st(&detail["step_id"])).unwrap(),
                iteration_path: arr(&detail["iteration_path"]).iter().map(num).collect(),
            };
            let params = arr(&detail["params"]).iter().map(|p| st(p).to_owned()).collect();
            let typed = NotRetargetable { step, params };
            GateDecision::Stop(NOT_RETARGETABLE, typed.to_value())
        }
        other => panic!("decision: {other:?}"),
    }
}

/// The fixture's recorded default digests put into the plan, standing in for the digest
/// `AUTO.PROG.FMT` is to store with a default (no stored program carries one yet).
fn with_recorded(fx: &Fixture, plan: &mut RunPlan) {
    for (param, d) in obj(&fx.ex["recorded_defaults"]) {
        plan.default_digests.insert(param.clone(), fid_types::Digest::parse(st(d)).unwrap());
    }
}

/// Evaluates the guard at every key of a case in `setting` (`today`: the plan as prepared;
/// `recorded`: with the fixture's recorded default digests) and compares with the
/// hand-labelled decisions.
fn run_case(name: &str, setting: &str) -> RetargetGuard {
    let fx = Fixture::load();
    let w = World::new();
    let case = fx.case(name);
    let mut plan = plan_for(&fx, &w, &case["binding"]);
    // prepare_run records no default digest: no stored program carries one.
    assert!(plan.default_digests.is_empty());
    if setting == "recorded" {
        with_recorded(&fx, &mut plan);
    }
    let label = obj(&case[setting]);
    let guard = RetargetGuard::from_plan(&plan);
    assert_eq!(guard.retargeted(), matches!(label["retargeted"], CValue::Bool(true)), "{name}/{setting}");
    let differing: Vec<String> = arr(&label["differing"]).iter().map(|p| st(p).to_owned()).collect();
    assert_eq!(guard.differing(), differing.as_slice(), "{name}/{setting}");
    for d in arr(&label["decisions"]) {
        let d = obj(d);
        let key = StepKey {
            step_id: fx.step(st(&d["step"])),
            iteration_path: arr(&d["iteration_path"]).iter().map(num).collect(),
        };
        let got = guard.before_step(&key, &info());
        assert_eq!(got, expected_decision(&d["decision"]), "{name}/{setting} at {key:?}");
        // A stop's detail reads back to the step key and the differing parameters.
        if let GateDecision::Stop(code, detail) = &got {
            assert_eq!(*code, "not_retargetable");
            let typed = NotRetargetable::from_value(detail).unwrap();
            assert_eq!(typed.step, key);
            assert_eq!(typed.params, differing);
        }
    }
    // The guard never holds, and it allows the end of the run.
    assert_eq!(guard.before_end(), GateDecision::Allow);
    guard
}

const CASES: [&str; 5] = [
    "ac1_recorded_scan_by_default",
    "ac1_recorded_scan_bound",
    "ac2_shifted_scan",
    "ac3_reference_has_no_default",
    "ac3_shifted_and_no_default",
];

#[test]
fn every_case_in_both_settings() {
    for name in CASES {
        for setting in ["today", "recorded"] {
            run_case(name, setting);
        }
    }
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_recorded_scan() {
    let fx = Fixture::load();
    let w = World::new();
    let plan = plan_for(&fx, &w, &fx.case("ac1_recorded_scan_by_default")["binding"]);
    assert_eq!(plan.record.inputs()["scan"].sha256.as_str(), fx.scan("bracket-120"));
    assert_eq!(plan.record.program_digest().as_str(), st(&fx.ex["program_digest"]));
    // Today (open, NODE.md): no recorded default digest, so even the recorded scan re-targets
    // the run and s4 stops (fail closed).
    for name in ["ac1_recorded_scan_by_default", "ac1_recorded_scan_bound"] {
        let g = run_case(name, "today");
        assert!(g.retargeted());
        assert!(matches!(g.decide(&StepKey::top(fx.step("s4"))), GateDecision::Stop(NOT_RETARGETABLE, _)));
    }
    // Given the recorded digest FMT is to store: every step allowed.
    let g = run_case("ac1_recorded_scan_by_default", "recorded");
    assert!(!g.retargeted());
    assert_eq!(g.blocked().iter().copied().collect::<Vec<_>>(), vec![fx.step("s4")]);
    assert!(!run_case("ac1_recorded_scan_bound", "recorded").retargeted());
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_shifted_scan_stops_at_s4() {
    let fx = Fixture::load();
    let w = World::new();
    let plan = plan_for(&fx, &w, &fx.case("ac2_shifted_scan")["binding"]);
    assert_eq!(plan.record.inputs()["scan"].sha256.as_str(), fx.scan("bracket-120-shifted"));
    for setting in ["today", "recorded"] {
        let g = run_case("ac2_shifted_scan", setting);
        assert_eq!(g.differing(), ["scan".to_owned()]);
        let stop = g.decide(&StepKey::top(fx.step("s4")));
        let GateDecision::Stop(code, detail) = stop else { panic!("{stop:?}") };
        assert_eq!(code, NOT_RETARGETABLE);
        assert_eq!(
            String::from_utf8(fid_canon::to_canonical_bytes(&detail).unwrap()).unwrap(),
            format!(r#"{{"iteration_path":[],"params":["scan"],"step_id":"{}"}}"#, fx.step("s4"))
        );
    }
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_parameter_without_recorded_default_retargets() {
    let fx = Fixture::load();
    let w = World::new();
    let plan = plan_for(&fx, &w, &fx.case("ac3_reference_has_no_default")["binding"]);
    // `reference` is bound to the very scan `scan` was recorded on, but it has no default.
    assert_eq!(plan.record.inputs()["reference"].sha256.as_str(), fx.scan("bracket-120"));
    assert_eq!(run_case("ac3_reference_has_no_default", "recorded").differing(), ["reference".to_owned()]);
    assert_eq!(
        run_case("ac3_reference_has_no_default", "today").differing(),
        ["reference".to_owned(), "scan".to_owned()]
    );
    assert_eq!(
        run_case("ac3_shifted_and_no_default", "recorded").differing(),
        ["reference".to_owned(), "scan".to_owned()]
    );
}

#[test]
fn ac3_bracket_scan_without_default_is_retargeted() {
    // The bracket family's `scan` declares no default: a B-120 run is re-targeted, but no step
    // of that program is marked, so every step is allowed (REQ-001 with an empty blocked set).
    let w = World::at("released");
    let plan = prepare(&w, common::identified(), common::b120(), &w.agent, RunFlags::default()).unwrap();
    assert!(plan.default_digests.is_empty());
    let g = RetargetGuard::from_plan(&plan);
    assert!(g.retargeted());
    assert_eq!(g.differing(), ["scan".to_owned()]);
    for (id, _) in &plan.steps_retargetable {
        assert_eq!(g.before_step(&StepKey::top(*id), &info()), GateDecision::Allow);
    }
}

/// Fails reading `input:scans/bracket-120.ply` only; any other `input:scans/<name>` is read
/// from `fixtures/scans/` as the shared harness does.
struct DefaultGone;

impl InputSource for DefaultGone {
    fn open(&self, _principal: &Principal, _param: &str, handle: &str) -> Result<Box<dyn Read + '_>, String> {
        if handle == "input:scans/bracket-120.ply" {
            return Err("not found".into());
        }
        let name = handle.strip_prefix("input:scans/").ok_or("unknown handle")?;
        let f = std::fs::File::open(common::root().join("fixtures/scans").join(name)).map_err(|e| format!("{e}"))?;
        Ok(Box::new(f))
    }
}

#[test]
fn prepare_never_reads_an_unused_default() {
    // The default's dataset is not opened when the run binds another one, and it never
    // becomes a recorded digest.
    let fx = Fixture::load();
    let w = World::new();
    let p = program();
    let b = binding_from_json(br#"{"scan":"input:scans/bracket-120-shifted.ply"}"#).unwrap();
    let cat = catalogue();
    let mut ids = Counting::default();
    let mut ctx = BindCtx { lifecycle: &w.store, catalogue: &cat, inputs: &DefaultGone, run_ids: &mut ids };
    let mut plan = prepare_run(&mut ctx, ProgramRef::Draft(&p), BindingSource::Binding(b), &w.alice, TRIAL).unwrap();
    assert!(plan.default_digests.is_empty());
    mark(&fx, &mut plan);
    let g = RetargetGuard::from_plan(&plan);
    assert_eq!(g.differing(), ["scan".to_owned()]);
    assert!(matches!(g.decide(&StepKey::top(fx.step("s4"))), GateDecision::Stop(NOT_RETARGETABLE, _)));
    assert_eq!(g.decide(&StepKey::top(fx.step("s3"))), GateDecision::Allow);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_same_plan_and_key_same_decision() {
    let fx = Fixture::load();
    let w = World::new();
    for name in ["ac1_recorded_scan_by_default", "ac2_shifted_scan", "ac3_reference_has_no_default"] {
        let plan = plan_for(&fx, &w, &fx.case(name)["binding"]);
        let a = RetargetGuard::from_plan(&plan);
        let mut b = RetargetGuard::from_plan(&plan);
        assert_eq!(a, b);
        for s in ["s1", "s2", "s3", "s4"] {
            let key = StepKey { step_id: fx.step(s), iteration_path: vec![3, 0] };
            let first = a.before_step(&key, &info());
            assert_eq!(first, a.before_step(&key, &info()));
            // `after_step` changes nothing the decision depends on.
            b.after_step(&key, &StepResult { command: info().command, output: CValue::Null });
            assert_eq!(first, b.before_step(&key, &info()));
            // The step's command and class do not enter the decision.
            assert_eq!(first, a.before_step(&key, &StepInfo { command: None, class: None }));
        }
    }
}

// ------------------------------------------------------------------ the hook

/// A gate of another node, written against the trait's defaults (as APPR and pause will be).
struct Counts(u32);

impl StepGate for Counts {
    fn before_step(&self, _key: &StepKey, _info: &StepInfo) -> GateDecision {
        if self.0 >= 2 {
            GateDecision::Hold("two steps done".into())
        } else {
            GateDecision::Allow
        }
    }
    fn after_step(&mut self, _key: &StepKey, _result: &StepResult) {
        self.0 += 1;
    }
}

#[test]
fn gate_chain_runs_through_trait_objects() {
    let fx = Fixture::load();
    let w = World::new();
    let plan = plan_for(&fx, &w, &fx.case("ac2_shifted_scan")["binding"]);
    let mut chain: Vec<Box<dyn StepGate>> = vec![Box::new(RetargetGuard::from_plan(&plan)), Box::new(Counts(0))];
    let mut decisions = Vec::new();
    for s in ["s1", "s2", "s3", "s4"] {
        let key = StepKey::top(fx.step(s));
        // The first decision that is not Allow wins (AUTO.RUN.EXEC.CMD's rule).
        let d = chain
            .iter()
            .map(|g| g.before_step(&key, &info()))
            .find(|d| *d != GateDecision::Allow)
            .unwrap_or(GateDecision::Allow);
        if d == GateDecision::Allow {
            for g in &mut chain {
                g.after_step(&key, &StepResult { command: None, output: CValue::Null });
            }
        }
        decisions.push(d);
    }
    assert_eq!(decisions[0], GateDecision::Allow);
    assert_eq!(decisions[1], GateDecision::Allow);
    assert_eq!(decisions[2], GateDecision::Hold("two steps done".into()));
    assert!(matches!(decisions[3], GateDecision::Stop(NOT_RETARGETABLE, _)));
    assert!(chain.iter().all(|g| g.before_end() == GateDecision::Allow));
}

#[test]
fn stop_detail_reader_is_strict() {
    let id = Uuid7::parse("018f0000-0000-7000-8000-000000000304").unwrap();
    let good =
        NotRetargetable { step: StepKey { step_id: id, iteration_path: vec![0, 7] }, params: vec!["scan".into()] };
    assert_eq!(NotRetargetable::from_value(&good.to_value()), Some(good.clone()));
    let CValue::Obj(mut extra) = good.to_value() else { panic!() };
    extra.insert("approved".into(), CValue::Bool(true));
    assert_eq!(NotRetargetable::from_value(&CValue::Obj(extra)), None);
    for bad in [
        r#"{"iteration_path":[-1],"params":[],"step_id":"018f0000-0000-7000-8000-000000000304"}"#,
        r#"{"iteration_path":[1.5],"params":[],"step_id":"018f0000-0000-7000-8000-000000000304"}"#,
        r#"{"iteration_path":[],"params":[1],"step_id":"018f0000-0000-7000-8000-000000000304"}"#,
        r#"{"iteration_path":[],"params":[],"step_id":"s4"}"#,
        r#"{"iteration_path":[],"params":[]}"#,
        "[]",
    ] {
        assert_eq!(NotRetargetable::from_value(&fid_canon::parse(bad.as_bytes()).unwrap()), None, "{bad}");
    }
}
