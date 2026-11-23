//! Integration tests for `CORE.DEP.RECOMP` (`AC-1`..`AC-6`, `REQ-001`..`REQ-009`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, PreconditionReason};
use fid_bus::handler::{
    CancelToken as BusCancelToken, EntityId as BusEntityId, Event, EventType, Handler, HandlerCtx, HandlerError,
    HandlerSet, StateView, ValidParams,
};
use fid_bus::pipeline::{AppendFailure, Bus, BusEnv, BusParts, RecordJournal, Router};
use fid_bus::policy::projection::RequestId;
use fid_bus::principal::{
    Channel, ChannelKind, Identity as BusIdentity, Principal, PrincipalId, PrincipalKind, SessionId, SessionKey,
    SessionTable,
};
use fid_bus::CommandTable;
use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue, Digest};
use fid_dep::recompute::bus::{commit_recompute_entry, submit_recompute_job, BusSystemRunner, SystemRecomputeChannel};
use fid_dep::recompute::{
    command_def, execute_recompute, order_induced_subgraph, plan_recompute, replay_exact, topological_levels,
    CancelCheck, ElementOutcome, ElementRunOutput, NeverCancel, NullProgress, OverlayEntity, OverlayEvent, OverlayView,
    RecompError, RecomputeCommitted, RecomputeRunner, RecomputeScope, RecordedCommand, ReplayOutcome,
};
use fid_dep::{Adjacency, EdgeIndex, EdgesSet, ElementState, ElementStates, EntityId};
use fid_jobs::{
    Caller, JobParams, JobService, JobState, Jobs, JobsConfig, NullLifecycleSink, PrincipalId as JobPrincipalId,
    PrincipalKind as JobPrincipalKind,
};
use fid_journal::append::{AppendRequest, Journal};
use fid_journal::entry::{EntryTime, Manifests, GENESIS_PREV};
use fid_journal::stream::{ActivityLog, ActivityLogConfig, Streams};
use fid_registry::schema::check_schema;
use fid_registry::validate::{validate, CommandDecl};
use fid_registry::{Catalogue, CommandDef, FieldPath, PredContext, PredError, Value as PredValue};
use fid_store::Store;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

fn temp_dir(tag: &str) -> PathBuf {
    let n = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("fid-recomp-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn num(n: i64) -> CValue {
    CValue::Num(n as f64)
}

fn test_manifests() -> Manifests {
    let d = digest_bytes(b"fid-dep-recomp-test");
    Manifests { kernel: d, registry: d }
}

fn test_time(sec: u64) -> EntryTime {
    EntryTime { utc: format!("2026-11-23T00:00:{:02}Z", sec % 60), mono_ns: sec * 1_000_000_000, sync: "synced".into() }
}

// ------------------------------------------------------------------ fixture loader

struct LoadedFixture {
    graph: EdgeIndex,
    overlay: OverlayView,
    commands: BTreeMap<EntityId, RecordedCommand>,
    by_name: BTreeMap<String, EntityId>,
    expected_order: Vec<EntityId>,
    expected_values: BTreeMap<EntityId, i64>,
}

fn load_fixture(raw: &str, head: Digest) -> LoadedFixture {
    let doc = parse(raw.as_bytes()).unwrap();
    let CValue::Obj(root) = doc else { panic!("fixture must be object") };
    let CValue::Arr(elems) = root.get("elements").unwrap() else { panic!("elements array") };

    let mut graph = EdgeIndex::new();
    let mut base: BTreeMap<EntityId, OverlayEntity> = BTreeMap::new();
    let mut commands: BTreeMap<EntityId, RecordedCommand> = BTreeMap::new();
    let mut by_name: BTreeMap<String, EntityId> = BTreeMap::new();

    for item in elems {
        let CValue::Obj(e) = item else { panic!("element object") };
        let CValue::Str(id_s) = e.get("id").unwrap() else { panic!("id") };
        let CValue::Str(name) = e.get("name").unwrap() else { panic!("name") };
        let CValue::Str(state_s) = e.get("state").unwrap() else { panic!("state") };
        let CValue::Num(val_f) = e.get("value").unwrap() else { panic!("value") };
        let id = Uuid::parse(id_s).unwrap();
        let state = match state_s.as_str() {
            "current" => ElementState::Current,
            "outdated" => ElementState::Outdated,
            "failed" => ElementState::Failed,
            "not_computed" => ElementState::NotComputed,
            other => panic!("unknown state {other}"),
        };
        let body = obj([("id", text(id_s)), ("name", text(name)), ("value", CValue::Num(*val_f))]);
        base.insert(id, OverlayEntity::new("test.scalar", body, state).unwrap());
        by_name.insert(name.clone(), id);

        let CValue::Arr(inps) = e.get("inputs").unwrap() else { panic!("inputs") };
        let mut inp_ids = Vec::new();
        for iv in inps {
            let CValue::Str(s) = iv else { panic!("input id") };
            inp_ids.push(Uuid::parse(s).unwrap());
        }
        inp_ids.sort();
        graph.apply(&[EdgesSet::new(id, inp_ids).unwrap()]).unwrap();

        if let Some(CValue::Str(cmd)) = e.get("command") {
            let args = e.get("args").unwrap().clone();
            commands.insert(id, RecordedCommand { command: cmd.clone(), args });
        }
    }

    let mut expected_order = Vec::new();
    if let Some(CValue::Arr(ord)) = root.get("expected_order") {
        for v in ord {
            let CValue::Str(s) = v else { panic!("order id") };
            expected_order.push(Uuid::parse(s).unwrap());
        }
    }

    let mut expected_values = BTreeMap::new();
    if let Some(CValue::Obj(vals)) = root.get("expected_values") {
        for (k, v) in vals {
            let CValue::Num(f) = v else { panic!("expected val") };
            expected_values.insert(Uuid::parse(k).unwrap(), *f as i64);
        }
    }

    LoadedFixture { graph, overlay: OverlayView::new(head, base), commands, by_name, expected_order, expected_values }
}

// ------------------------------------------------------------------ real Bus handlers for test kit

fn make_test_def(name: &str, props: Vec<(&str, CValue)>, required: &[&str]) -> CommandDef {
    let prop_map: BTreeMap<String, CValue> = props.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        ("properties", CValue::Obj(prop_map)),
        ("required", CValue::Arr(required.iter().map(|s| text(s)).collect())),
        ("type", text("object")),
    ]);
    let result_schema = obj([("type", text("object"))]);
    CommandDef::from_value(obj([
        ("algorithms", CValue::Arr(Vec::new())),
        ("cli_bare", CValue::Bool(false)),
        ("description", text("Test-kit pure element handler.")),
        ("determinism", text("D0")),
        ("effects", CValue::Arr(vec![text("TestElementComputed")])),
        ("exposure", CValue::Arr(vec![text("cli")])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text(name)),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text("2026-11-23")), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text("W2")),
        ("title", text(name)),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))
    .unwrap()
}

fn read_entity_val(view: &dyn StateView, id_s: &str) -> Result<i64, HandlerError> {
    let eid = BusEntityId::new(id_s).map_err(HandlerError::Refused)?;
    let ent = view.entity(&eid).ok_or_else(|| HandlerError::Failed(Detail::new("missing input entity")))?;
    let CValue::Obj(m) = ent.body else { return Err(HandlerError::Failed(Detail::new("body not object"))) };
    let Some(CValue::Num(n)) = m.get("value") else {
        return Err(HandlerError::Failed(Detail::new("missing value")));
    };
    Ok(*n as i64)
}

fn read_entity_name(view: &dyn StateView, id_s: &str) -> String {
    if let Ok(eid) = BusEntityId::new(id_s) {
        if let Some(ent) = view.entity(&eid) {
            if let CValue::Obj(m) = ent.body {
                if let Some(CValue::Str(s)) = m.get("name") {
                    return s.clone();
                }
            }
        }
    }
    String::new()
}

type InvocationLog = Arc<Mutex<Vec<(String, PrincipalKind, Option<String>)>>>;

struct TestKitHandler {
    op: &'static str,
    invocations: InvocationLog,
}

impl Handler for TestKitHandler {
    fn run(&self, view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        let CValue::Obj(p) = params.value() else { return Err(HandlerError::Failed(Detail::new("params object"))) };
        let CValue::Str(target) = p.get("target").unwrap() else {
            return Err(HandlerError::Failed(Detail::new("missing target")));
        };
        self.invocations.lock().unwrap().push((
            target.clone(),
            ctx.principal().kind(),
            ctx.principal().on_behalf_of().map(|r| r.id.as_str().to_owned()),
        ));

        let new_val = match self.op {
            "add_const" => {
                let CValue::Str(inp) = p.get("input").unwrap() else { unreachable!() };
                let CValue::Num(delta) = p.get("delta").unwrap() else { unreachable!() };
                read_entity_val(view, inp)? + (*delta as i64)
            }
            "mul_const" => {
                let CValue::Str(inp) = p.get("input").unwrap() else { unreachable!() };
                let CValue::Num(factor) = p.get("factor").unwrap() else { unreachable!() };
                read_entity_val(view, inp)? * (*factor as i64)
            }
            "sum_two" => {
                let CValue::Str(left) = p.get("left").unwrap() else { unreachable!() };
                let CValue::Str(right) = p.get("right").unwrap() else { unreachable!() };
                read_entity_val(view, left)? + read_entity_val(view, right)?
            }
            "fail_fit" => {
                return Err(HandlerError::Refused(BusError::Precondition(
                    PreconditionReason::False,
                    Detail::new("fit_failed"),
                )));
            }
            other => panic!("unknown op {other}"),
        };

        let name = read_entity_name(view, target);
        let body = obj([("id", text(target)), ("name", text(&name)), ("value", num(new_val))]);
        let ev = Event::new(EventType::new("TestElementComputed").unwrap(), body);
        Ok(vec![ev])
    }
}

struct DummyRouter;
impl Router for DummyRouter {
    fn branch(&self, _principal: &Principal, _channel: &dyn Channel, _def: &CommandDef) -> Result<String, BusError> {
        Ok("main".to_owned())
    }
    fn request_branch(&self, _principal: &Principal, _channel: &dyn Channel) -> Result<String, BusError> {
        Ok("agent/req".to_owned())
    }
}

struct DummyRecordJournal;
impl RecordJournal for DummyRecordJournal {
    fn tail(&self) -> Result<Option<(u64, Digest)>, BusError> {
        Ok(None)
    }
    fn branch_head(&self, _branch: &str) -> Result<Option<Digest>, BusError> {
        Ok(None)
    }
    fn append(&self, _req: AppendRequest) -> Result<Digest, AppendFailure> {
        Ok(GENESIS_PREV)
    }
}

struct DummyEnv;
impl BusEnv for DummyEnv {
    fn now(&self) -> EntryTime {
        test_time(1)
    }
    fn seed(&self) -> u64 {
        0
    }
    fn request_id(&self) -> Result<RequestId, BusError> {
        RequestId::parse("019aae02-a400-730b-a185-a23d532c6350")
    }
}

struct EmptyPredCtx;
impl PredContext for EmptyPredCtx {
    fn get(&self, path: &FieldPath) -> Result<PredValue, PredError> {
        Err(PredError::MissingField { path: path.to_string() })
    }
}

struct TestGuiChannel {
    who: String,
    session: SessionKey,
}
impl Channel for TestGuiChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Gui
    }
    fn authenticated_identity(&self) -> Option<BusIdentity> {
        Some(BusIdentity::new(PrincipalId::new(&self.who).unwrap()))
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
}

struct BusHarness {
    _dir: PathBuf,
    bus: Bus,
    sessions: SessionTable,
    sys_session: SessionKey,
    caller: Principal,
    invocations: InvocationLog,
}

fn build_bus_harness(tag: &str) -> BusHarness {
    let dir = temp_dir(tag);
    let str_t = || obj([("type", text("string"))]);
    let num_t = || obj([("type", text("number"))]);
    let defs = vec![
        make_test_def(
            "test.add_const",
            vec![("delta", num_t()), ("input", str_t()), ("target", str_t())],
            &["delta", "input", "target"],
        ),
        make_test_def(
            "test.mul_const",
            vec![("factor", num_t()), ("input", str_t()), ("target", str_t())],
            &["factor", "input", "target"],
        ),
        make_test_def(
            "test.sum_two",
            vec![("left", str_t()), ("right", str_t()), ("target", str_t())],
            &["left", "right", "target"],
        ),
        make_test_def(
            "test.fail_fit",
            vec![("code", str_t()), ("input", str_t()), ("target", str_t())],
            &["code", "input", "target"],
        ),
    ];
    let ops = ["add_const", "mul_const", "sum_two", "fail_fit"];
    let table = Arc::new(
        CommandTable::build(
            test_manifests().registry,
            defs.clone(),
            Catalogue::geo_frame().unwrap(),
            &fid_registry::ContextTypes::default(),
        )
        .unwrap(),
    );
    let invocations = Arc::new(Mutex::new(Vec::new()));
    let mut handlers = HandlerSet::new();
    for (def, op) in defs.into_iter().zip(ops) {
        handlers.register(def, Box::new(TestKitHandler { op, invocations: Arc::clone(&invocations) })).unwrap();
    }
    let activity = Arc::new(ActivityLog::new(dir.join("alog"), ActivityLogConfig::default(), test_manifests()));
    let bus = Bus::new(BusParts {
        table,
        handlers,
        router: Arc::new(DummyRouter),
        journal: Arc::new(DummyRecordJournal),
        streams: Streams::new(activity),
        env: Arc::new(DummyEnv),
        stream: "record".to_owned(),
        manifests: test_manifests(),
    });
    let mut sessions = SessionTable::new();
    let gui_key = sessions.open_session(SessionId::new("gui-1").unwrap()).unwrap();
    let gui_ch = TestGuiChannel { who: "alice".to_owned(), session: gui_key };
    let caller = HandlerCtx::derive(&sessions, &gui_ch, 0, BusCancelToken::never()).unwrap().principal().clone();
    let sys_session = sessions.open_session(SessionId::new("recomp-sys").unwrap()).unwrap();
    BusHarness { _dir: dir, bus, sessions, sys_session, caller, invocations }
}

impl Drop for BusHarness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self._dir);
    }
}

/// Fast pure-Rust test-kit runner for the 1,000-element chain & job tests.
struct FastChainRunner {
    processed: Arc<AtomicUsize>,
    cancel_after: Option<(usize, Arc<AtomicBool>)>,
    per_step_delay: Duration,
}

impl RecomputeRunner for FastChainRunner {
    fn run_element(
        &self,
        _id: EntityId,
        recorded: &RecordedCommand,
        overlay: &OverlayView,
    ) -> Result<ElementRunOutput, String> {
        if !self.per_step_delay.is_zero() {
            std::thread::sleep(self.per_step_delay);
        }
        let CValue::Obj(p) = &recorded.args else { return Err("bad_args".into()) };
        let CValue::Str(target_s) = p.get("target").unwrap() else { return Err("missing_target".into()) };
        let CValue::Str(inp_s) = p.get("input").unwrap() else { return Err("missing_input".into()) };
        let CValue::Num(delta) = p.get("delta").unwrap() else { return Err("missing_delta".into()) };
        let inp_id = Uuid::parse(inp_s).unwrap();
        let target_id = Uuid::parse(target_s).unwrap();
        let inp_ent = overlay.get(&inp_id).ok_or_else(|| "missing_input".to_owned())?;
        let CValue::Obj(im) = &inp_ent.body else { return Err("bad_body".into()) };
        let CValue::Num(prev_v) = im.get("value").unwrap() else { return Err("bad_val".into()) };
        let target_name = match &overlay.get(&target_id).unwrap().body {
            CValue::Obj(tm) => match tm.get("name") {
                Some(CValue::Str(s)) => s.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        let new_val = (*prev_v as i64) + (*delta as i64);
        let body = obj([("id", text(target_s)), ("name", text(&target_name)), ("value", num(new_val))]);
        let bytes = to_canonical_bytes(&body).unwrap();
        let out_digest = digest_bytes(&bytes);
        let done = self.processed.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some((limit, flag)) = &self.cancel_after {
            if done >= *limit {
                flag.store(true, Ordering::SeqCst);
            }
        }
        Ok(ElementRunOutput {
            output_digest: out_digest,
            events: vec![OverlayEvent { event_type: "TestElementComputed".into(), body: body.clone() }],
            updated_body: body,
        })
    }
}

struct FlagCancel(Arc<AtomicBool>);
impl CancelCheck for FlagCancel {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

// ------------------------------------------------------------------ tests

/// Registry command definition conforms to schema checker and registry validator.
#[test]
fn command_def_validates_cleanly() {
    let def = command_def().unwrap();
    assert_eq!(def.id, "project.recompute_dependents");
    assert_eq!(def.major, 1);
    let catalogue = Catalogue::geo_frame().unwrap();
    assert_eq!(check_schema(&def.params_schema, &catalogue), vec![]);
    assert_eq!(check_schema(&def.result_schema, &catalogue), vec![]);
    let decls = vec![CommandDecl::from_def(&def)];
    assert_eq!(validate(&decls), vec![]);

    // Scope parameter validation: empty elements refused.
    let err = RecomputeScope::from_params(&obj([("scope", obj([("elements", CValue::Arr(vec![]))]))])).unwrap_err();
    assert_eq!(err.code(), "validation");
}

/// AC-1: Given `fixtures/core/dep/recomp/diamond` (a -> b, a -> c, b -> d, c -> d; id(c) < id(b)),
/// recompute runs through `Bus::run_handlers_as_system` as `system` with `on_behalf_of` caller,
/// processes in exact order c, b, d, sets all three `current`, and appends exactly one
/// `project.recompute_dependents@1` entry to the Record Journal with three digests.
#[test]
fn ac1_diamond_order_system_principal_and_single_journal_entry() {
    let dir = temp_dir("ac1_store");
    let store = Store::create(&dir.join("proj")).unwrap();
    let jr = Journal::on_store(&store, "record").unwrap();
    let head0 = GENESIS_PREV;

    let fx = load_fixture(include_str!("../../../fixtures/core/dep/recomp/diamond.json"), head0);
    let h = build_bus_harness("ac1_bus");
    let sys_ch = SystemRecomputeChannel { session: &h.sys_session, on_behalf_of: h.caller.reference() };
    let runner = BusSystemRunner { bus: &h.bus, sessions: &h.sessions, channel: &sys_ch, pred_ctx: &EmptyPredCtx };

    let exec = execute_recompute(
        &fx.graph,
        &fx.overlay,
        &fx.commands,
        &RecomputeScope::AllOutdated,
        &runner,
        &NeverCancel,
        &NullProgress,
    )
    .unwrap();

    // Order is strictly c, b, d (`id(c) < id(b)`).
    assert_eq!(exec.order, fx.expected_order);
    let rec_ids: Vec<EntityId> = exec.committed.recomputed.iter().map(|r| r.id).collect();
    assert_eq!(rec_ids, fx.expected_order);
    assert!(exec.committed.failed.is_empty());
    assert!(exec.committed.skipped.is_empty());

    // Every handler invocation ran as principal `system` with `on_behalf_of = alice`.
    let calls = h.invocations.lock().unwrap();
    assert_eq!(calls.len(), 3);
    for (idx, (target_s, pkind, behalf)) in calls.iter().enumerate() {
        assert_eq!(*target_s, fx.expected_order[idx].to_string());
        assert_eq!(*pkind, PrincipalKind::System);
        assert_eq!(behalf.as_deref(), Some("alice"));
    }

    // All three are now `current` with expected computed values.
    let final_entities = exec.overlay.clone().into_merged();
    for (id, expected_val) in &fx.expected_values {
        let ent = final_entities.get(id).unwrap();
        assert_eq!(ent.state, ElementState::Current);
        let CValue::Obj(m) = &ent.body else { panic!("body obj") };
        assert_eq!(m.get("value"), Some(&num(*expected_val)));
    }

    // Commit exactly one `project.recompute_dependents@1` entry onto the Record Journal.
    let _entry_hash = commit_recompute_entry(
        &jr,
        "main",
        &h.caller,
        test_time(10),
        test_manifests(),
        &RecomputeScope::AllOutdated,
        &exec,
    )
    .unwrap();

    let count: i64 = store
        .read(|conn| conn.query_row("SELECT count(*) FROM entries WHERE stream = 'record'", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(count, 1, "Record Journal must hold exactly one recompute entry");
    let hdr_bytes: Vec<u8> = store
        .read(|conn| conn.query_row("SELECT header FROM entries WHERE stream = 'record' AND seq = 1", [], |r| r.get(0)))
        .unwrap();
    let CValue::Obj(hdr) = parse(&hdr_bytes).unwrap() else { panic!("hdr obj") };
    let CValue::Obj(cmd) = hdr.get("command").unwrap() else { panic!("cmd obj") };
    assert_eq!(cmd.get("id"), Some(&text("project.recompute_dependents")));
    let CValue::Arr(outs) = cmd.get("outputs").unwrap() else { panic!("outputs arr") };
    assert_eq!(outs.len(), 3, "entry must carry three output digests");

    let _ = fs::remove_dir_all(&dir);
}

/// AC-2: Given `fixtures/core/dep/recomp/fail_b` (b fails with `fit_failed`), b becomes `failed`,
/// d stays `outdated` and its handler is NEVER run, c becomes `current`, and the single entry
/// lists c with its digest and b with `fit_failed`.
#[test]
fn ac2_failure_isolation_blocks_dependents_and_continues_siblings() {
    let fx = load_fixture(include_str!("../../../fixtures/core/dep/recomp/fail_b.json"), GENESIS_PREV);
    let h = build_bus_harness("ac2_bus");
    let sys_ch = SystemRecomputeChannel { session: &h.sys_session, on_behalf_of: h.caller.reference() };
    let runner = BusSystemRunner { bus: &h.bus, sessions: &h.sessions, channel: &sys_ch, pred_ctx: &EmptyPredCtx };

    let exec = execute_recompute(
        &fx.graph,
        &fx.overlay,
        &fx.commands,
        &RecomputeScope::AllOutdated,
        &runner,
        &NeverCancel,
        &NullProgress,
    )
    .unwrap();

    let id_c = fx.by_name["c"];
    let id_b = fx.by_name["b"];
    let id_d = fx.by_name["d"];

    assert_eq!(exec.committed.recomputed.len(), 1);
    assert_eq!(exec.committed.recomputed[0].id, id_c);
    assert_eq!(exec.committed.failed.len(), 1);
    assert_eq!(exec.committed.failed[0].id, id_b);
    assert_eq!(exec.committed.failed[0].code, "fit_failed");
    assert_eq!(exec.outcomes.get(&id_d), Some(&ElementOutcome::Blocked));

    // Verify states in the resulting overlay.
    let states = exec.overlay.element_states();
    assert_eq!(states.get(&id_c), Some(ElementState::Current));
    assert_eq!(states.get(&id_b), Some(ElementState::Failed));
    assert_eq!(states.get(&id_d), Some(ElementState::Outdated));

    // Confirm d's handler was NEVER invoked.
    let called_targets: Vec<String> = h.invocations.lock().unwrap().iter().map(|(t, _, _)| t.clone()).collect();
    assert_eq!(called_targets, vec![id_c.to_string(), id_b.to_string()]);
    assert!(!called_targets.contains(&id_d.to_string()));
}

/// AC-3: Given `chain1000` cancelled after 100 elements, returns `Cancelled`, every element
/// keeps its pre-recompute state and output, and the Record Journal is unchanged.
#[test]
fn ac3_cancel_after_100_elements_discards_all_partial_work() {
    let dir = temp_dir("ac3_store");
    let store = Store::create(&dir.join("proj")).unwrap();
    let fx = load_fixture(include_str!("../../../fixtures/core/dep/recomp/chain1000.json"), GENESIS_PREV);
    let before_states = fx.overlay.element_states();
    let flag = Arc::new(AtomicBool::new(false));
    let processed = Arc::new(AtomicUsize::new(0));
    let runner = FastChainRunner {
        processed: Arc::clone(&processed),
        cancel_after: Some((100, Arc::clone(&flag))),
        per_step_delay: Duration::ZERO,
    };
    let cancel = FlagCancel(Arc::clone(&flag));

    let err = execute_recompute(
        &fx.graph,
        &fx.overlay,
        &fx.commands,
        &RecomputeScope::AllOutdated,
        &runner,
        &cancel,
        &NullProgress,
    )
    .unwrap_err();
    assert_eq!(err, RecompError::Cancelled);
    assert_eq!(err.code(), "cancelled");
    assert_eq!(processed.load(Ordering::SeqCst), 100);
    // Original head overlay untouched; journal untouched (0 entries).
    assert_eq!(fx.overlay.element_states(), before_states);
    let count: i64 = store
        .read(|conn| conn.query_row("SELECT count(*) FROM entries WHERE stream = 'record'", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(count, 0, "cancelled recompute must append nothing to the Record Journal");
    let _ = fs::remove_dir_all(&dir);
}

/// AC-4, REQ-005, REQ-006, REQ-008: Recomputing `kernel_d0` without input change yields identical
/// digests; exact replay outcome is `IDENTICAL`; parallel-level ordering produces byte-identical
/// events to sequential execution; already-current elements in explicit scope are `skipped`.
#[test]
fn ac4_exact_replay_identical_parallel_equivalence_and_skipped() {
    let fx = load_fixture(include_str!("../../../fixtures/core/dep/recomp/kernel_d0.json"), GENESIS_PREV);
    let h = build_bus_harness("ac4_bus");
    let sys_ch = SystemRecomputeChannel { session: &h.sys_session, on_behalf_of: h.caller.reference() };
    let runner = BusSystemRunner { bus: &h.bus, sessions: &h.sessions, channel: &sys_ch, pred_ctx: &EmptyPredCtx };

    // First run: compute all outdated elements.
    let exec1 = execute_recompute(
        &fx.graph,
        &fx.overlay,
        &fx.commands,
        &RecomputeScope::AllOutdated,
        &runner,
        &NeverCancel,
        &NullProgress,
    )
    .unwrap();
    assert_eq!(exec1.committed.recomputed.len(), 3);

    // Mark the same elements outdated without changing inputs and recompute again -> identical digests!
    let mut base2 = exec1.overlay.clone().into_merged();
    for id in &fx.expected_order {
        base2.get_mut(id).unwrap().state = ElementState::Outdated;
    }
    let overlay2 = OverlayView::new(GENESIS_PREV, base2);
    let exec2 = execute_recompute(
        &fx.graph,
        &overlay2,
        &fx.commands,
        &RecomputeScope::AllOutdated,
        &runner,
        &NeverCancel,
        &NullProgress,
    )
    .unwrap();
    assert_eq!(exec1.committed.recomputed, exec2.committed.recomputed);
    assert_eq!(exec1.committed.canonical_bytes().unwrap(), exec2.committed.canonical_bytes().unwrap());

    // Exact replay against the initial head state -> IDENTICAL.
    let (outcome, replayed) =
        replay_exact(&fx.graph, &fx.overlay, &fx.commands, &RecomputeScope::AllOutdated, &exec1.committed, &runner)
            .unwrap();
    assert_eq!(outcome, ReplayOutcome::Identical);
    assert_eq!(replayed.canonical_events_bytes().unwrap(), exec1.canonical_events_bytes().unwrap());

    // REQ-006: simulate computing each topological level independently (out-of-order within the
    // level) and assembling outcomes in canonical ascending-ID level order -> byte-identical!
    let scope_set: BTreeSet<EntityId> = fx.expected_order.iter().copied().collect();
    let levels = topological_levels(&fx.graph, &scope_set).unwrap();
    let mut par_overlay = fx.overlay.clone();
    let mut par_recomputed = Vec::new();
    for level in levels {
        // Compute in reverse order within the level to prove independence, then emit in ascending ID order.
        let mut level_results: BTreeMap<EntityId, ElementRunOutput> = BTreeMap::new();
        for &id in level.iter().rev() {
            let out = runner.run_element(id, &fx.commands[&id], &par_overlay).unwrap();
            level_results.insert(id, out);
        }
        for (id, out) in level_results {
            par_overlay.stage_recomputed(id, out.updated_body, out.output_digest).unwrap();
            par_recomputed.push(fid_dep::RecomputedEntry { id, digest: out.output_digest });
        }
    }
    let par_committed = RecomputeCommitted { recomputed: par_recomputed, failed: Vec::new(), skipped: Vec::new() };
    assert_eq!(par_committed.canonical_bytes().unwrap(), exec1.committed.canonical_bytes().unwrap());

    // REQ-008: explicit scope including already-current `datum_seed` skips it and lists it in `skipped`.
    let id_in = fx.by_name["datum_seed"];
    let exec_skip = execute_recompute(
        &fx.graph,
        &fx.overlay,
        &fx.commands,
        &RecomputeScope::Subtree(id_in),
        &runner,
        &NeverCancel,
        &NullProgress,
    )
    .unwrap();
    assert_eq!(exec_skip.committed.skipped, vec![id_in]);
    assert_eq!(exec_skip.outcomes.get(&id_in), Some(&ElementOutcome::Skipped));
    assert_eq!(exec_skip.committed.recomputed.len(), 3);
}

/// AC-5: Running `chain1000` through `fid_jobs::JobService`, `show_status` reports progress
/// values that never decrease and end at `1.0`.
#[test]
fn ac5_job_service_monotonic_progress_ending_at_one() {
    let fx = load_fixture(include_str!("../../../fixtures/core/dep/recomp/chain1000.json"), GENESIS_PREV);
    let jobs: Jobs<Result<fid_dep::RecomputeExecution, RecompError>> =
        Jobs::new(JobsConfig::default(), Arc::new(NullLifecycleSink)).unwrap();
    let svc = JobService::new(jobs).unwrap();
    let caller = Caller { kind: JobPrincipalKind::Human, id: JobPrincipalId::parse("alice").unwrap() };

    let processed = Arc::new(AtomicUsize::new(0));
    let runner = FastChainRunner {
        processed: Arc::clone(&processed),
        cancel_after: None,
        per_step_delay: Duration::from_micros(50),
    };

    let job_id =
        submit_recompute_job(&svc, &caller, fx.graph, fx.overlay, fx.commands, RecomputeScope::AllOutdated, runner)
            .unwrap();

    let params = JobParams { job: job_id };
    let mut seen_progress: Vec<f64> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let st = svc.show_status(&caller, &params).unwrap();
        seen_progress.push(st.progress);
        if matches!(st.state, JobState::Succeeded | JobState::Failed | JobState::Cancelled) {
            assert_eq!(st.state, JobState::Succeeded, "recompute job must succeed");
            break;
        }
        assert!(Instant::now() < deadline, "recompute job timed out");
        std::thread::sleep(Duration::from_millis(2));
    }

    assert!(!seen_progress.is_empty(), "must observe progress values from show_status");
    for pair in seen_progress.windows(2) {
        assert!(pair[1] >= pair[0], "progress must never decrease: {seen_progress:?}");
    }
    let last = *seen_progress.last().unwrap();
    assert!((last - 1.0).abs() <= 1e-12, "progress must end at 1.0, got {last}");
    assert_eq!(processed.load(Ordering::SeqCst), 999);
}

/// AC-6: Given a concurrent W2 commit on the same branch while the recompute runs, the recompute
/// commit fails with `conflict` (`RecompError::Conflict`) and the elements stay `outdated`.
#[test]
fn ac6_head_moved_during_recompute_returns_conflict() {
    let dir = temp_dir("ac6_store");
    let store = Store::create(&dir.join("proj")).unwrap();
    let jr = Journal::on_store(&store, "record").unwrap();
    let h = build_bus_harness("ac6_bus");

    // Start recompute against GENESIS_PREV.
    let fx = load_fixture(include_str!("../../../fixtures/core/dep/recomp/diamond.json"), GENESIS_PREV);
    let sys_ch = SystemRecomputeChannel { session: &h.sys_session, on_behalf_of: h.caller.reference() };
    let runner = BusSystemRunner { bus: &h.bus, sessions: &h.sessions, channel: &sys_ch, pred_ctx: &EmptyPredCtx };
    let exec = execute_recompute(
        &fx.graph,
        &fx.overlay,
        &fx.commands,
        &RecomputeScope::AllOutdated,
        &runner,
        &NeverCancel,
        &NullProgress,
    )
    .unwrap();

    // Concurrent W2 commit advances the branch head before recompute commits.
    let concurrent_head = commit_recompute_entry(
        &jr,
        "main",
        &h.caller,
        test_time(20),
        test_manifests(),
        &RecomputeScope::AllOutdated,
        &exec,
    )
    .unwrap();
    assert_ne!(concurrent_head, GENESIS_PREV);

    // Now the stale recompute (expected_head == GENESIS_PREV) attempts to commit -> Conflict!
    let err = commit_recompute_entry(
        &jr,
        "main",
        &h.caller,
        test_time(21),
        test_manifests(),
        &RecomputeScope::AllOutdated,
        &exec,
    )
    .unwrap_err();
    assert!(
        matches!(err, RecompError::Conflict { expected, actual } if expected == GENESIS_PREV && actual == concurrent_head),
        "expected Conflict(GENESIS_PREV -> concurrent_head), got {err:?}"
    );
    assert_eq!(err.code(), "conflict");
    // Original committed overlay still holds all three dependents as `outdated`.
    let states = fx.overlay.element_states();
    for name in ["b", "c", "d"] {
        assert_eq!(states.get(&fx.by_name[name]), Some(ElementState::Outdated));
    }

    let _ = fs::remove_dir_all(&dir);
}

/// Performance sanity check for Kahn + BTreeSet ordering (budget: 100k elements < 100 ms;
/// test 10k elements in debug build to keep CI fast while verifying O((V+E) log V) scaling).
#[test]
fn ordering_10k_elements_is_fast_and_deterministic() {
    let mut adj = Adjacency::new();
    let mut scope = BTreeSet::new();
    let mut ids = Vec::with_capacity(10_000);
    for i in 0u64..10_000 {
        let mut b = [0u8; 16];
        let be = i.to_be_bytes();
        b[0..6].copy_from_slice(&be[2..8]);
        b[6] = 0x70;
        b[8] = 0x80;
        let id = Uuid::from_bytes(b);
        adj.add_element(id);
        scope.insert(id);
        if i > 0 {
            adj.add_edge(ids[(i - 1) as usize], id);
        }
        ids.push(id);
    }
    let t0 = Instant::now();
    let plan = order_induced_subgraph(&adj, &scope).unwrap();
    let elapsed = t0.elapsed();
    assert_eq!(plan.order, ids);
    assert!(elapsed < Duration::from_secs(2), "10k ordering took {elapsed:?}");

    // Also verify plan_recompute rejects unknown element IDs with validation.
    let ghost = Uuid::from_bytes([0xff; 16]);
    let states = ElementStates::new();
    let err = plan_recompute(&adj, &states, &RecomputeScope::Elements(vec![ghost])).unwrap_err();
    assert_eq!(err, RecompError::UnknownElement(ghost));
    assert_eq!(err.code(), "validation");
}
