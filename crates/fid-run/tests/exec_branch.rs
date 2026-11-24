//! Acceptance criteria of spec node AUTO.RUN.EXEC.BRANCH (AC-1 .. AC-5) and its error paths.
//!
//! Plans are real: `prepare_run` on the bracket program, whose lifecycle is set through the
//! `AUTO.REC.LIFE` commands (the shared harness of AUTO.RUN.BIND), for a bus-derived person and
//! agent. The program digest and input digest the Run entity must carry come from
//! `fixtures/auto/run/bind/expected.json`, written by the independent Python reference
//! `fixtures/auto/run/bind/make_cases.py`; branch names, fork points and verdicts are
//! hand-labelled from the leaf's requirements.
//!
//! The bus is a test bus ([`TestBus`]) that keeps branch heads, entries and Run entities, honours
//! `expected_head`, applies the creation rules a branch and a Run entity have (a new name, a
//! known head; one entity per run; an outcome written once), and can be told to fail one
//! command.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{b120, identified, prepare, Expected, World};
use fid_bus::error::{BusError, ConflictReason, Detail, InternalReason, NotFoundReason, ValidationReason};
use fid_bus::principal::{Principal, PrincipalRef};
use fid_bus::{CommandResult, Invocation};
use fid_canon::{CValue, Digest as EntryHash};
use fid_program::Uuid7;
use fid_run::{
    finalise, open_run_target, run_branch, ExecError, FinalOutcome, Outcome, RunBus, RunFlags, RunRecord, SetupCause,
    SetupStage, CREATE_BRANCH_COMMAND, CREATE_RUN_COMMAND, FINALISE_RUN_COMMAND,
};
use fid_types::CommandId;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

// ------------------------------------------------------------------ test bus

#[derive(Debug, Clone)]
struct Entry {
    branch: String,
    command: String,
    principal: PrincipalRef,
    params: CValue,
    hash: EntryHash,
}

#[derive(Default, Clone)]
struct State {
    heads: BTreeMap<String, EntryHash>,
    created_from: BTreeMap<String, EntryHash>,
    entries: Vec<Entry>,
    /// Run entities by (branch, run ID text).
    runs: BTreeMap<(String, String), RunRecord>,
    /// Working branches by principal ID; `main` otherwise.
    working: BTreeMap<String, String>,
}

struct TestBus {
    state: RefCell<State>,
    fail: Option<&'static str>,
    calls: Cell<usize>,
}

fn h(n: u8) -> EntryHash {
    EntryHash([n; 32])
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("string: {other:?}"),
    }
}

fn member<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("member {k}")),
        other => panic!("object: {other:?}"),
    }
}

fn refuse(why: &str) -> BusError {
    BusError::Validation(ValidationReason::Schema, Detail::new(why))
}

impl TestBus {
    /// `main` at head `h(1)`.
    fn new() -> TestBus {
        let mut s = State::default();
        s.heads.insert("main".into(), h(1));
        TestBus { state: RefCell::new(s), fail: None, calls: Cell::new(0) }
    }

    fn failing(command: &'static str) -> TestBus {
        TestBus { fail: Some(command), ..TestBus::new() }
    }

    /// Adds branch `name` at `head` as `principal_id`'s working branch.
    fn with_working(self, principal_id: &str, name: &str, head: EntryHash) -> TestBus {
        {
            let mut s = self.state.borrow_mut();
            s.heads.insert(name.into(), head);
            s.working.insert(principal_id.into(), name.into());
        }
        self
    }

    fn entries(&self) -> Vec<Entry> {
        self.state.borrow().entries.clone()
    }

    fn on(&self, branch: &str) -> Vec<Entry> {
        self.entries().into_iter().filter(|e| e.branch == branch).collect()
    }

    fn head(&self, branch: &str) -> Option<EntryHash> {
        self.state.borrow().heads.get(branch).copied()
    }

    fn run_entities(&self) -> usize {
        self.state.borrow().runs.len()
    }

    fn apply(&self, s: &mut State, branch: &str, inv: &Invocation) -> Result<(), BusError> {
        let p = &inv.params;
        match inv.command.as_str() {
            c if c == CREATE_BRANCH_COMMAND => {
                let name = st(member(p, "name")).to_owned();
                let from = st(member(p, "from"));
                if s.heads.contains_key(&name) {
                    return Err(refuse("branch exists"));
                }
                let known = s.heads.values().chain(s.created_from.values()).copied().find(|d| d.to_string() == from);
                let from = known.ok_or_else(|| refuse("unknown head"))?;
                s.heads.insert(name.clone(), from);
                s.created_from.insert(name, from);
            }
            c if c == CREATE_RUN_COMMAND => {
                let CValue::Obj(o) = p else { return Err(refuse("params")) };
                if o.len() != 1 {
                    return Err(refuse("params"));
                }
                let record = RunRecord::from_value(member(p, "record")).map_err(|_| refuse("record"))?;
                if record.branch() != Some(branch) {
                    return Err(refuse("the record names another branch"));
                }
                let key = (branch.to_owned(), record.run_id().to_string());
                if s.runs.contains_key(&key) {
                    return Err(BusError::Conflict(ConflictReason::IdCollision, Detail::new("run exists")));
                }
                s.runs.insert(key, record);
            }
            c if c == FINALISE_RUN_COMMAND => {
                let key = (branch.to_owned(), st(member(p, "run")).to_owned());
                let old = s.runs.get(&key).ok_or_else(|| refuse("no run"))?;
                if old.outcome() != Outcome::Pending {
                    return Err(refuse("final already"));
                }
                let mut v = old.to_value();
                if let CValue::Obj(o) = &mut v {
                    o.insert("outcome".into(), member(p, "outcome").clone());
                }
                let new = RunRecord::from_value(&v).map_err(|_| refuse("outcome"))?;
                s.runs.insert(key, new);
            }
            _ => {}
        }
        Ok(())
    }
}

impl RunBus for TestBus {
    fn working_branch(&self, principal: &Principal) -> Result<String, BusError> {
        let s = self.state.borrow();
        Ok(s.working.get(principal.id().as_str()).cloned().unwrap_or_else(|| "main".into()))
    }

    fn branch_head(&self, branch: &str) -> Result<Option<EntryHash>, BusError> {
        Ok(self.head(branch))
    }

    fn run_record(&self, branch: &str, run_id: Uuid7) -> Result<Option<RunRecord>, BusError> {
        Ok(self.state.borrow().runs.get(&(branch.to_owned(), run_id.to_string())).cloned())
    }

    fn execute(&self, principal: &Principal, branch: &str, inv: &Invocation) -> Result<CommandResult, BusError> {
        self.calls.set(self.calls.get() + 1);
        if self.fail == Some(inv.command.as_str()) {
            return Err(BusError::Internal(InternalReason::HandlerFailed, Detail::new("test bus fails this command")));
        }
        let mut s = self.state.borrow_mut();
        let head = *s
            .heads
            .get(branch)
            .ok_or_else(|| BusError::NotFound(NotFoundReason::UnknownBranch, Detail::new(branch)))?;
        if inv.expected_head.is_some_and(|e| e != head) {
            return Err(BusError::Conflict(ConflictReason::ExpectedHeadMismatch, Detail::new(branch)));
        }
        // All or nothing: apply to a copy, keep it only on success.
        let mut next = s.clone();
        self.apply(&mut next, branch, inv)?;
        let n = u8::try_from(next.entries.len() + 100).unwrap();
        let hash = h(n);
        next.entries.push(Entry {
            branch: branch.to_owned(),
            command: inv.command.as_str().to_owned(),
            principal: principal.reference(),
            params: inv.params.clone(),
            hash,
        });
        next.heads.insert(branch.to_owned(), hash);
        *s = next;
        Ok(CommandResult::Done { result: None, events: Vec::new(), entry: Some(hash), activity: None })
    }
}

fn released_plan(w: &World, principal: &Principal) -> fid_run::RunPlan {
    prepare(w, identified(), b120(), principal, RunFlags::default()).unwrap()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_person_on_main_gets_run_branch_and_run_entity_first() {
    let ex = Expected::load();
    let w = World::at("released");
    let plan = released_plan(&w, &w.alice);
    let bus = TestBus::new();
    let t = open_run_target(&bus, &plan, &w.alice).unwrap();

    let run_id = plan.record.run_id();
    let name = format!("run/{run_id}");
    assert_eq!(t.branch(), name);
    assert_eq!(run_branch(run_id), name);
    assert_eq!(t.run_id(), run_id);
    assert_eq!(t.run_entity().as_str(), run_id.to_string());
    // Forked from main's head at the time of the call.
    assert_eq!(t.fork_point(), h(1));
    assert_eq!(bus.state.borrow().created_from.get(&name), Some(&h(1)));

    // The branch is created on main, under the person, at exactly the head read.
    let on_main = bus.on("main");
    assert_eq!(on_main.len(), 1);
    assert_eq!(on_main[0].command, CREATE_BRANCH_COMMAND);
    assert_eq!(st(member(&on_main[0].params, "name")), name);
    assert_eq!(st(member(&on_main[0].params, "from")), h(1).to_string());

    // The run branch's first (and only) entry is project.create_run@1 with the record.
    let on_run = bus.on(&name);
    assert_eq!(on_run.len(), 1);
    assert_eq!(on_run[0].command, CREATE_RUN_COMMAND);
    assert_eq!(on_run[0].hash, t.run_entry());
    let rec = member(&on_run[0].params, "record");
    assert_eq!(st(member(rec, "program_digest")), ex.s("program_digest"));
    assert_eq!(st(member(member(member(rec, "inputs"), "scan"), "sha256")), ex.s("scan_sha256"));
    assert_eq!(st(member(rec, "branch")), name);
    assert_eq!(st(member(rec, "outcome")), "pending");
    // Every RunRecord field, the branch reference added and nothing else changed.
    let entity = RunRecord::from_value(rec).unwrap();
    assert_eq!(entity, plan.record.clone().with_branch(&name));

    // Branch first, then the entity; both under the run's principal.
    let all = bus.entries();
    assert_eq!(all.iter().map(|e| e.command.as_str()).collect::<Vec<_>>(), [CREATE_BRANCH_COMMAND, CREATE_RUN_COMMAND]);
    assert!(all.iter().all(|e| e.principal == w.alice.reference()));
    assert_eq!(t.principal(), &w.alice);
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_agent_forks_from_its_own_branch_not_main() {
    let w = World::at("released");
    let plan = released_plan(&w, &w.agent);
    let bus = TestBus::new().with_working(w.agent.id().as_str(), "agent/a1", h(7));
    let t = open_run_target(&bus, &plan, &w.agent).unwrap();
    assert_eq!(t.fork_point(), h(7));
    assert_ne!(t.fork_point(), bus.head("main").unwrap());
    assert_eq!(bus.state.borrow().created_from.get(t.branch()), Some(&h(7)));
    // The create_branch entry is on the agent's branch; main is untouched.
    assert_eq!(bus.on("agent/a1").len(), 1);
    assert!(bus.on("main").is_empty());
    assert_eq!(bus.head("main"), Some(h(1)));
    assert!(bus.entries().iter().all(|e| e.principal == w.agent.reference()));
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_failed_run_entity_write_is_run_setup_failed_and_leaves_no_entity() {
    let w = World::at("released");
    let plan = released_plan(&w, &w.alice);
    let bus = TestBus::failing(CREATE_RUN_COMMAND);
    let e = open_run_target(&bus, &plan, &w.alice).unwrap_err();
    assert_eq!(e.code(), "run_setup_failed");
    let name = run_branch(plan.record.run_id());
    match &e {
        ExecError::RunSetupFailed { stage, branch, cause } => {
            assert_eq!(*stage, SetupStage::RunEntity);
            assert_eq!(branch.as_deref(), Some(name.as_str()));
            assert!(matches!(cause, SetupCause::Bus(BusError::Internal(InternalReason::HandlerFailed, _))));
        }
        other => panic!("{other:?}"),
    }
    // No Run entity on any branch; the empty run branch stays at the fork point.
    assert_eq!(bus.run_entities(), 0);
    assert!(bus.on(&name).is_empty());
    assert_eq!(bus.head(&name), Some(h(1)));
    assert!(e.to_string().contains(&name));
}

#[test]
fn ac3_failed_branch_creation_writes_no_entity_at_all() {
    let w = World::at("released");
    let plan = released_plan(&w, &w.alice);
    let bus = TestBus::failing(CREATE_BRANCH_COMMAND);
    let e = open_run_target(&bus, &plan, &w.alice).unwrap_err();
    assert_eq!(e.code(), "run_setup_failed");
    assert!(matches!(e, ExecError::RunSetupFailed { stage: SetupStage::CreateBranch, branch: None, .. }));
    // project.create_run@1 was never called.
    assert_eq!(bus.calls.get(), 1);
    assert_eq!(bus.run_entities(), 0);
    assert!(bus.entries().is_empty());
    assert_eq!(bus.head(&run_branch(plan.record.run_id())), None);
}

#[test]
fn ac3_existing_run_branch_is_run_setup_failed() {
    let w = World::at("released");
    let plan = released_plan(&w, &w.alice);
    let name = run_branch(plan.record.run_id());
    let bus = TestBus::new().with_working("someone-else", &name, h(9));
    let e = open_run_target(&bus, &plan, &w.alice).unwrap_err();
    assert!(matches!(e, ExecError::RunSetupFailed { stage: SetupStage::CreateBranch, branch: None, .. }));
    assert_eq!(bus.run_entities(), 0);
}

#[test]
fn working_branch_moved_since_read_is_refused_by_the_head_guard() {
    // A bus whose reported head is stale: the guard on journal.create_branch@1 refuses.
    struct Stale(TestBus);
    impl RunBus for Stale {
        fn working_branch(&self, p: &Principal) -> Result<String, BusError> {
            self.0.working_branch(p)
        }
        fn branch_head(&self, b: &str) -> Result<Option<EntryHash>, BusError> {
            if b == "main" {
                Ok(Some(h(2)))
            } else {
                self.0.branch_head(b)
            }
        }
        fn run_record(&self, b: &str, id: Uuid7) -> Result<Option<RunRecord>, BusError> {
            self.0.run_record(b, id)
        }
        fn execute(&self, p: &Principal, b: &str, i: &Invocation) -> Result<CommandResult, BusError> {
            self.0.execute(p, b, i)
        }
    }
    let w = World::at("released");
    let plan = released_plan(&w, &w.alice);
    let bus = Stale(TestBus::new());
    let e = open_run_target(&bus, &plan, &w.alice).unwrap_err();
    match e {
        ExecError::RunSetupFailed { stage: SetupStage::CreateBranch, cause: SetupCause::Bus(b), .. } => {
            assert_eq!(b.reason(), "expected_head_mismatch");
        }
        other => panic!("{other:?}"),
    }
    assert!(bus.0.entries().is_empty());
}

#[test]
fn only_the_runs_principal_may_open_it_and_only_a_fresh_plan() {
    let w = World::at("released");
    let plan = released_plan(&w, &w.alice);
    let bus = TestBus::new();
    let e = open_run_target(&bus, &plan, &w.agent).unwrap_err();
    assert!(matches!(e, ExecError::RunSetupFailed { stage: SetupStage::Plan, branch: None, .. }));
    assert_eq!(bus.calls.get(), 0);

    let mut used = plan.clone();
    used.record = plan.record.clone().with_branch("run/elsewhere");
    let e = open_run_target(&bus, &used, &w.alice).unwrap_err();
    assert!(matches!(e, ExecError::RunSetupFailed { stage: SetupStage::Plan, .. }));
    assert_eq!(bus.calls.get(), 0);

    // A second opening of the same plan is refused: the branch name is never reused.
    open_run_target(&bus, &plan, &w.alice).unwrap();
    let e = open_run_target(&bus, &plan, &w.alice).unwrap_err();
    assert!(matches!(e, ExecError::RunSetupFailed { stage: SetupStage::CreateBranch, .. }));
    assert_eq!(bus.run_entities(), 1);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_commit_routed_to_main_is_wrong_branch() {
    let w = World::at("released");
    let plan = released_plan(&w, &w.alice);
    let bus = TestBus::new();
    let t = open_run_target(&bus, &plan, &w.alice).unwrap();
    let before = bus.calls.get();
    let inv = Invocation::new(CommandId::parse("test.step@1").unwrap(), CValue::Obj(BTreeMap::new()));

    for other in ["main", "agent/a1", "run/other", ""] {
        let e = t.commit(&bus, other, &inv).unwrap_err();
        assert_eq!(e.code(), "wrong_branch");
        assert_eq!(e, ExecError::WrongBranch { expected: t.branch().to_owned(), got: other.to_owned() });
        assert_eq!(t.route(other), Err(e));
    }
    // Refused before the bus is called; main keeps the one create_branch entry.
    assert_eq!(bus.calls.get(), before);
    assert_eq!(bus.on("main").len(), 1);

    // The run branch itself is allowed, under the run's principal.
    let r = t.commit(&bus, t.branch(), &inv).unwrap();
    let CommandResult::Done { entry: Some(hash), .. } = &r else { panic!("{r:?}") };
    let on_run = bus.on(t.branch());
    assert_eq!(on_run.len(), 2);
    assert_eq!(on_run[1].hash, *hash);
    assert_eq!(on_run[1].principal, w.alice.reference());
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_second_finalisation_is_already_final_and_changes_nothing() {
    let w = World::at("released");
    let plan = released_plan(&w, &w.alice);
    let bus = TestBus::new();
    let t = open_run_target(&bus, &plan, &w.alice).unwrap();

    let entry = finalise(&bus, &t, FinalOutcome::Succeeded).unwrap();
    let rec = bus.run_record(t.branch(), t.run_id()).unwrap().unwrap();
    assert_eq!(rec.outcome(), Outcome::Succeeded);
    let on_run = bus.on(t.branch());
    assert_eq!(on_run.len(), 2);
    assert_eq!((on_run[1].command.as_str(), on_run[1].hash), (FINALISE_RUN_COMMAND, entry));
    assert_eq!(st(member(&on_run[1].params, "outcome")), "succeeded");
    assert_eq!(st(member(&on_run[1].params, "run")), t.run_id().to_string());
    assert_eq!(on_run[1].principal, w.alice.reference());

    let calls = bus.calls.get();
    for again in [FinalOutcome::Failed, FinalOutcome::Cancelled, FinalOutcome::Succeeded] {
        let e = finalise(&bus, &t, again).unwrap_err();
        assert_eq!(e.code(), "already_final");
        assert_eq!(e, ExecError::AlreadyFinal { outcome: Outcome::Succeeded });
    }
    // The entity is unchanged and nothing was committed.
    assert_eq!(bus.run_record(t.branch(), t.run_id()).unwrap().unwrap(), rec);
    assert_eq!(bus.calls.get(), calls);
    assert_eq!(bus.on(t.branch()).len(), 2);
}

#[test]
fn ac5_each_final_outcome_is_written_as_named() {
    for (o, wire) in [
        (FinalOutcome::Succeeded, "succeeded"),
        (FinalOutcome::Failed, "failed"),
        (FinalOutcome::Cancelled, "cancelled"),
    ] {
        let w = World::at("released");
        let plan = released_plan(&w, &w.alice);
        let bus = TestBus::new();
        let t = open_run_target(&bus, &plan, &w.alice).unwrap();
        finalise(&bus, &t, o).unwrap();
        assert_eq!(bus.run_record(t.branch(), t.run_id()).unwrap().unwrap().outcome().as_str(), wire);
    }
}

#[test]
fn finalising_a_missing_entity_is_unknown_entity() {
    let w = World::at("released");
    let plan = released_plan(&w, &w.alice);
    let bus = TestBus::new();
    let t = open_run_target(&bus, &plan, &w.alice).unwrap();
    bus.state.borrow_mut().runs.clear();
    let e = finalise(&bus, &t, FinalOutcome::Failed).unwrap_err();
    assert_eq!(e.code(), "not_found");
    assert!(matches!(e, ExecError::Bus(BusError::NotFound(NotFoundReason::UnknownEntity, _))));
}

#[test]
fn a_failed_finalisation_passes_the_bus_error_through() {
    let w = World::at("released");
    let plan = released_plan(&w, &w.alice);
    let ok = TestBus::new();
    let t = open_run_target(&ok, &plan, &w.alice).unwrap();
    let bus = TestBus { fail: Some(FINALISE_RUN_COMMAND), ..ok };
    let e = finalise(&bus, &t, FinalOutcome::Succeeded).unwrap_err();
    assert_eq!(e.code(), "internal");
    // Still pending: a later finalisation may succeed.
    assert_eq!(bus.run_record(t.branch(), t.run_id()).unwrap().unwrap().outcome(), Outcome::Pending);
}

// ------------------------------------------------------------------ determinism

#[test]
fn same_plan_same_target_and_entries() {
    let w = World::at("released");
    let plan = released_plan(&w, &w.alice);
    let run = || {
        let bus = TestBus::new();
        let t = open_run_target(&bus, &plan, &w.alice).unwrap();
        finalise(&bus, &t, FinalOutcome::Succeeded).unwrap();
        let entries: Vec<_> = bus.entries().into_iter().map(|e| (e.branch, e.command, e.params, e.hash)).collect();
        (t, entries)
    };
    assert_eq!(run(), run());
}
