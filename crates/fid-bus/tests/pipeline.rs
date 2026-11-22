//! Acceptance criteria of spec node CORE.BUS.PIPE.STAGE (AC-1 to AC-3 and AC-6 to AC-10),
//! plus the commit-stage compare-and-swap, commit hooks, poisoning, the bus's agent-branch
//! check and the reason parameter, against `fixtures/core/bus/pipe/stage/probes/registry.json`
//! and `cases.json`, written by the independent reference
//! `fixtures/core/bus/pipe/stage/make_fixtures.py` (outcomes hand-labelled from the spec).
//! Commits go to a real `fid_journal::Journal`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, PreconditionReason};
use fid_bus::handler::{EntityId, EntityRef, Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet};
use fid_bus::handler::{SettingKey, StateView, ValidParams};
use fid_bus::pipeline::{AppendFailure, AppendOutcome};
use fid_bus::pipeline::{
    Bus, BusEnv, BusParts, Call, CommandResult, CommitHook, CommitRecord, Invocation, RecordJournal, Router, Stage,
    StageProbe, StagedFold, SystemCall, Watch,
};
use fid_bus::policy::projection::{
    EntryOrigin, RequestBook, RequestId, RequestState, REQUEST_APPROVED, REQUEST_FILED, REQUEST_REJECTED,
    REQUEST_WITHDRAWN,
};
use fid_bus::principal::Principal;
use fid_bus::principal::{Channel, ChannelKind, Identity, OsUser, PrincipalId, PrincipalKind, PrincipalRef};
use fid_bus::principal::{SessionId, SessionKey, SessionTable};
use fid_bus::route::SessionRouter;
use fid_bus::validate::{CommandTable, NoRoots};
use fid_canon::{parse, CValue, Digest};
use fid_journal::branch::BranchName;
use fid_journal::stream::{format_utc, ActivityLog, ActivityLogConfig, RecordingFlag, Streams, CURRENT_FILE};
use fid_journal::{
    AiAssistance, AppendRequest, CommandDraft, DraftBody, EntryDraft, EntryHash, EntryHeader, EntryPayload, EntryTime,
    Field, HeaderBody, Journal, Manifests, PrincipalDraft, TaggedDigest, R1_RESULT_TAG,
};
use fid_registry::{Catalogue, CommandDef, ContextTypes};
use fid_store::ProjectLocation;
use fid_types::{CommandId, Determinism, SideEffect};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

type Obj = BTreeMap<String, CValue>;

const MANIFEST: Digest = Digest([7; 32]);
const STREAM: &str = "record";
const MAIN: &str = "main";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(rel: &str) -> CValue {
    let bytes = std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    parse(&bytes).unwrap()
}

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("not an object: {other:?}"),
    }
}

fn arr<'a>(o: &'a Obj, k: &str) -> &'a [CValue] {
    match o.get(k) {
        Some(CValue::Arr(a)) => a,
        other => panic!("{k}: {other:?}"),
    }
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: {other:?}"),
    }
}

fn strs(o: &Obj, k: &str) -> Vec<String> {
    arr(o, k)
        .iter()
        .map(|v| match v {
            CValue::Str(s) => s.clone(),
            other => panic!("{k}: {other:?}"),
        })
        .collect()
}

fn json(s: &str) -> CValue {
    parse(s.as_bytes()).unwrap()
}

fn cases() -> CValue {
    read_json("fixtures/core/bus/pipe/stage/probes/cases.json")
}

fn defs() -> Vec<CommandDef> {
    let r = read_json("fixtures/core/bus/pipe/stage/probes/registry.json");
    arr(obj(&r), "commands").iter().map(|c| CommandDef::from_value(c.clone()).unwrap()).collect()
}

fn cid(s: &str) -> CommandId {
    CommandId::parse(s).unwrap()
}

fn manifests() -> Manifests {
    Manifests { registry: MANIFEST, kernel: Digest([2; 32]) }
}

fn assert_err<T: std::fmt::Debug>(r: Result<T, BusError>, code: &str, reason: &str) -> BusError {
    let e = r.unwrap_err();
    assert_eq!((e.code().to_string().as_str(), e.reason()), (code, reason), "{e}");
    e
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let name = format!("fid-stage-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::path::absolute(std::env::temp_dir().join(name)).unwrap();
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ------------------------------------------------------------------ channels

struct TestChannel {
    kind: ChannelKind,
    identity: Identity,
    session: SessionKey,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        Some(self.identity.clone())
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
}

fn channel(table: &mut SessionTable, wire: &str, subject: &str) -> TestChannel {
    let kind = ChannelKind::from_wire(wire).unwrap();
    let mut identity = Identity::new(PrincipalId::new(subject).unwrap());
    identity.os_user = Some(OsUser::new(subject).unwrap());
    if matches!(kind, ChannelKind::ServerSession | ChannelKind::Internal) {
        identity.acting_for =
            Some(PrincipalRef { kind: PrincipalKind::Human, id: PrincipalId::new("trigger").unwrap() });
    }
    let session = table.open_session(SessionId::new(&format!("{wire}-{subject}")).unwrap()).unwrap();
    TestChannel { kind, identity, session }
}

// ------------------------------------------------------------------ state, env, handler

struct View {
    head: Digest,
}

impl StateView for View {
    fn entity(&self, _id: &EntityId) -> Option<EntityRef<'_>> {
        None
    }
    fn head(&self) -> Digest {
        self.head
    }
    fn setting(&self, _key: &SettingKey) -> Option<CValue> {
        None
    }
}

/// The host inputs. `mono` is a counter every `now` and `request_id` advances (`now` returns
/// the advanced value); with `stepped_back` set, the wall clock reads an hour early.
struct Env {
    mono: AtomicU64,
    stepped_back: AtomicBool,
}

impl BusEnv for Env {
    fn now(&self) -> EntryTime {
        let mut since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        if self.stepped_back.load(Ordering::SeqCst) {
            since -= std::time::Duration::from_secs(3600);
        }
        let mono_ns = self.mono.fetch_add(1, Ordering::SeqCst) + 1;
        EntryTime { utc: format_utc(since), mono_ns, sync: "ntp".into() }
    }
    fn seed(&self) -> u64 {
        42
    }
    fn request_id(&self) -> Result<RequestId, BusError> {
        let n = u128::from(self.mono.fetch_add(1, Ordering::SeqCst));
        // UUIDv7 layout: version nibble 7, variant bits 10.
        let v = (0x0190_0000_0000_7000_8000_0000_0000_0000u128) | (n & 0xffff_ffff);
        RequestId::from_uuid(Uuid::from_u128(v))
    }
}

/// The probe handler: behaviour chosen by `target` (see the reference script).
struct ProbeHandler {
    calls: Arc<AtomicUsize>,
    emits: bool,
}

fn event(t: &str, body: CValue) -> Event {
    Event::new(EventType::new(t).unwrap(), body)
}

impl Handler for ProbeHandler {
    fn run(&self, _view: &dyn StateView, params: &ValidParams, _ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.emits {
            return Ok(Vec::new());
        }
        let p = obj(params.value());
        let target = p.get("target").and_then(|v| if let CValue::Str(s) = v { Some(s.as_str()) } else { None });
        let mut body = Obj::new();
        body.insert("target".into(), p.get("target").cloned().unwrap_or(CValue::Null));
        body.insert("count".into(), p.get("count").cloned().unwrap_or(CValue::Null));
        match target {
            Some("refuse") => Err(HandlerError::Refused(BusError::Precondition(
                PreconditionReason::False,
                Detail::new("probe handler refuses"),
            ))),
            Some("undeclared") => Ok(vec![event("TestOther", CValue::Obj(body))]),
            _ => Ok(vec![event("TestWritten", CValue::Obj(body))]),
        }
    }
}

// ------------------------------------------------------------------ probes and hooks

#[derive(Default)]
struct Recorder {
    log: Mutex<Vec<Stage>>,
    fail_at: Mutex<Option<Stage>>,
}

impl Recorder {
    fn names(&self) -> Vec<String> {
        self.log.lock().unwrap().iter().map(|s| s.as_str().to_string()).collect()
    }
    fn clear(&self) {
        self.log.lock().unwrap().clear();
    }
}

fn probe_error(stage: Stage) -> BusError {
    BusError::Precondition(PreconditionReason::CapabilityMissing, Detail::new(&format!("probe at {stage}")))
}

impl StageProbe for Recorder {
    fn enter(&self, stage: Stage) -> Result<(), BusError> {
        self.log.lock().unwrap().push(stage);
        if *self.fail_at.lock().unwrap() == Some(stage) {
            return Err(probe_error(stage));
        }
        Ok(())
    }
}

/// One entry a hook was called for.
#[derive(Clone)]
struct Seen {
    origin: Option<EntryOrigin>,
    header: EntryHeader,
    payload: EntryPayload,
    decided_at: Digest,
    events: Vec<Event>,
}

/// Records precommits and folds; can refuse either.
struct Counting {
    watch: Watch,
    refuse_precommit: AtomicBool,
    refuse_fold: AtomicBool,
    precommits: Mutex<Vec<Seen>>,
    folds: Mutex<Vec<Seen>>,
}

impl Counting {
    fn new(watch: Watch) -> Arc<Counting> {
        Arc::new(Counting {
            watch,
            refuse_precommit: AtomicBool::new(false),
            refuse_fold: AtomicBool::new(false),
            precommits: Mutex::new(Vec::new()),
            folds: Mutex::new(Vec::new()),
        })
    }
}

fn seen(r: &CommitRecord<'_>, events: &[Event]) -> Seen {
    Seen {
        origin: r.origin().cloned(),
        header: r.header().clone(),
        payload: r.payload().clone(),
        decided_at: r.decided_at(),
        events: events.to_vec(),
    }
}

fn stale() -> BusError {
    BusError::Conflict(fid_bus::error::ConflictReason::HeadMoved, Detail::new("hook: decided-against state moved"))
}

struct CountingStage<'a> {
    hook: &'a Counting,
    staged: Vec<Seen>,
}

impl CommitHook for Counting {
    fn name(&self) -> &str {
        "counting"
    }
    fn watch(&self) -> Watch {
        self.watch.clone()
    }
    fn stage(&self) -> Result<Box<dyn StagedFold + '_>, BusError> {
        Ok(Box::new(CountingStage { hook: self, staged: Vec::new() }))
    }
}

impl StagedFold for CountingStage<'_> {
    fn precommit(&mut self, r: &CommitRecord<'_>, events: &[Event]) -> Result<(), BusError> {
        assert_eq!(r.hash(), r.header().hash);
        assert_eq!(r.promoted_from(), None);
        self.hook.precommits.lock().unwrap().push(seen(r, events));
        if self.hook.refuse_precommit.load(Ordering::SeqCst) {
            return Err(stale());
        }
        self.staged.push(seen(r, events));
        Ok(())
    }
    fn install(self: Box<Self>) -> Result<(), BusError> {
        self.hook.folds.lock().unwrap().extend(self.staged);
        if self.hook.refuse_fold.load(Ordering::SeqCst) {
            return Err(stale());
        }
        Ok(())
    }
}

/// The request book as an origin-bound commit hook: the staged state is a copy of the book.
struct BookHook {
    book: Mutex<RequestBook>,
}

struct BookStage<'a> {
    hook: &'a BookHook,
    copy: RequestBook,
}

impl CommitHook for BookHook {
    fn name(&self) -> &str {
        "requests"
    }
    fn watch(&self) -> Watch {
        Watch::Effects(
            [REQUEST_FILED, REQUEST_APPROVED, REQUEST_WITHDRAWN, REQUEST_REJECTED]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )
    }
    fn stage(&self) -> Result<Box<dyn StagedFold + '_>, BusError> {
        Ok(Box::new(BookStage { hook: self, copy: self.book.lock().unwrap().clone() }))
    }
}

impl StagedFold for BookStage<'_> {
    fn precommit(&mut self, r: &CommitRecord<'_>, events: &[Event]) -> Result<(), BusError> {
        self.copy.apply_entry(r.origin().unwrap(), events)
    }
    fn install(self: Box<Self>) -> Result<(), BusError> {
        *self.hook.book.lock().unwrap() = self.copy;
        Ok(())
    }
}

/// A hook whose install panics (a bug inside a hook, after the append).
struct Panicking;

struct PanickingStage;

impl CommitHook for Panicking {
    fn name(&self) -> &str {
        "panicking"
    }
    fn watch(&self) -> Watch {
        Watch::All
    }
    fn stage(&self) -> Result<Box<dyn StagedFold + '_>, BusError> {
        Ok(Box::new(PanickingStage))
    }
}

impl StagedFold for PanickingStage {
    fn precommit(&mut self, _r: &CommitRecord<'_>, _events: &[Event]) -> Result<(), BusError> {
        Ok(())
    }
    fn install(self: Box<Self>) -> Result<(), BusError> {
        panic!("hook bug during install")
    }
}

// ------------------------------------------------------------------ routers

/// The harness router: commits as [`SessionRouter`] (agent commits refused); requests to the
/// open branch, except an agent's, which go to `agent/<id>` (the request's source branch).
struct Harnessed {
    open: SessionRouter,
}

impl Router for Harnessed {
    fn branch(&self, principal: &Principal, channel: &dyn Channel, def: &CommandDef) -> Result<String, BusError> {
        self.open.branch(principal, channel, def)
    }
    fn request_branch(&self, principal: &Principal, channel: &dyn Channel) -> Result<String, BusError> {
        if principal.kind() == PrincipalKind::Agent {
            return Ok(format!("agent/{}", principal.id().as_str()));
        }
        self.open.request_branch(principal, channel)
    }
}

/// A hostile router: routes everything, an agent's writes and requests included, to one
/// fixed name of its choosing.
struct Hostile(&'static str);

impl Router for Hostile {
    fn branch(&self, _p: &Principal, _c: &dyn Channel, _d: &CommandDef) -> Result<String, BusError> {
        Ok(self.0.into())
    }
    fn request_branch(&self, _p: &Principal, _c: &dyn Channel) -> Result<String, BusError> {
        Ok(self.0.into())
    }
}

// ------------------------------------------------------------------ journal

/// `TestJournal::fail_append` values.
const APPEND_OK: u8 = 0;
/// An append failure with an unknown outcome (`internal/journal_unavailable`).
const APPEND_UNKNOWN: u8 = 1;
/// An append refused with a known outcome (`conflict/head_moved`).
const APPEND_REFUSED: u8 = 2;

/// A real journal with an append fault switch. `tail` records the env's monotonic counter
/// when the commit stage reads the tail (under the commit lock).
struct TestJournal {
    inner: Arc<Journal>,
    fail_append: AtomicU8,
    env: Arc<Env>,
    mono_at_tail: AtomicU64,
}

impl RecordJournal for TestJournal {
    fn tail(&self) -> Result<Option<(u64, Digest)>, BusError> {
        self.mono_at_tail.store(self.env.mono.load(Ordering::SeqCst), Ordering::SeqCst);
        RecordJournal::tail(self.inner.as_ref())
    }
    fn branch_head(&self, branch: &str) -> Result<Option<Digest>, BusError> {
        RecordJournal::branch_head(self.inner.as_ref(), branch)
    }
    fn append(&self, req: AppendRequest) -> Result<EntryHash, AppendFailure> {
        match self.fail_append.load(Ordering::SeqCst) {
            APPEND_UNKNOWN => Err(AppendFailure {
                error: BusError::Internal(
                    fid_bus::error::InternalReason::JournalUnavailable,
                    Detail::new("injected append failure"),
                ),
                outcome: AppendOutcome::Unknown,
            }),
            APPEND_REFUSED => Err(AppendFailure { error: stale(), outcome: AppendOutcome::NotAppended }),
            _ => RecordJournal::append(self.inner.as_ref(), req),
        }
    }
}

// ------------------------------------------------------------------ the harness

struct Harness {
    _dir: TempDir,
    alog: PathBuf,
    journal: Arc<Journal>,
    tj: Arc<TestJournal>,
    recording: Arc<RecordingFlag>,
    calls: Arc<AtomicUsize>,
    probe: Arc<Recorder>,
    env: Arc<Env>,
    bus: Bus,
    sessions: SessionTable,
    gui: TestChannel,
    cli: TestChannel,
    mcp: TestChannel,
    internal: TestChannel,
}

fn table() -> Arc<CommandTable> {
    let catalogue =
        Catalogue::from_json(&std::fs::read(root().join("fixtures/core/bus/unit/catalogue.json")).unwrap()).unwrap();
    let t = CommandTable::build(MANIFEST, defs(), catalogue, &ContextTypes::new()).unwrap();
    assert!(t.compile_errors().is_empty(), "{:?}", t.compile_errors());
    Arc::new(t)
}

fn router(working: &str) -> SessionRouter {
    SessionRouter::new(BranchName::parse(working).unwrap())
}

fn harness(tag: &str) -> Harness {
    harness_with(tag, Arc::new(Harnessed { open: router(MAIN) }))
}

fn harness_with(tag: &str, router: Arc<dyn Router>) -> Harness {
    let dir = TempDir::new(tag);
    let (loc, lock) = ProjectLocation::create(&dir.p("project")).unwrap();
    let journal = Arc::new(Journal::open(loc, lock, STREAM).unwrap());
    let env = Arc::new(Env { mono: AtomicU64::new(1), stepped_back: AtomicBool::new(false) });
    let tj = Arc::new(TestJournal {
        inner: Arc::clone(&journal),
        fail_append: AtomicU8::new(APPEND_OK),
        env: Arc::clone(&env),
        mono_at_tail: AtomicU64::new(0),
    });
    let alog = dir.p("alog");
    let recording = Arc::new(RecordingFlag::new());
    let activity = Arc::new(ActivityLog::new(&alog, ActivityLogConfig::default(), manifests()));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut handlers = HandlerSet::new();
    for d in defs() {
        let emits = !matches!(d.side_effect, SideEffect::R0 | SideEffect::R1);
        handlers.register(d, Box::new(ProbeHandler { calls: Arc::clone(&calls), emits })).unwrap();
    }
    let probe = Arc::new(Recorder::default());
    let mut bus = Bus::new(BusParts {
        table: table(),
        handlers,
        router,
        journal: Arc::clone(&tj) as Arc<dyn RecordJournal>,
        streams: Streams::with_flag(activity, Arc::clone(&recording)),
        env: Arc::clone(&env) as Arc<dyn BusEnv>,
        stream: STREAM.to_string(),
        manifests: manifests(),
    });
    bus.set_probe(Arc::clone(&probe) as Arc<dyn StageProbe>);
    let mut sessions = SessionTable::new();
    let gui = channel(&mut sessions, "gui", "alice");
    let cli = channel(&mut sessions, "cli", "bob");
    let mcp = channel(&mut sessions, "mcp", "claude");
    let internal = channel(&mut sessions, "internal", "replay");
    Harness { _dir: dir, alog, journal, tj, recording, calls, probe, env, bus, sessions, gui, cli, mcp, internal }
}

impl Harness {
    fn head(&self) -> Digest {
        RecordJournal::branch_head(self.journal.as_ref(), MAIN).unwrap().unwrap_or(fid_journal::GENESIS_PREV)
    }

    fn tail(&self) -> Option<(u64, Digest)> {
        RecordJournal::tail(self.journal.as_ref()).unwrap()
    }

    fn chan(&self, wire: &str) -> &TestChannel {
        match wire {
            "gui" => &self.gui,
            "cli" => &self.cli,
            "mcp" => &self.mcp,
            "internal" => &self.internal,
            other => panic!("channel {other}"),
        }
    }

    fn run_on(&self, wire: &str, view: &dyn StateView, inv: &Invocation) -> Result<CommandResult, BusError> {
        let call = Call { sessions: &self.sessions, channel: self.chan(wire), view, roots: &NoRoots };
        self.bus.execute(&call, inv)
    }

    fn run(&self, wire: &str, inv: &Invocation) -> Result<CommandResult, BusError> {
        let view = View { head: self.head() };
        self.run_on(wire, &view, inv)
    }
}

fn write(target: &str, count: i64) -> Invocation {
    Invocation::new(cid("test.write@1"), json(&format!(r#"{{"target":"{target}","count":{count}}}"#)))
}

fn done(r: CommandResult) -> (Vec<Event>, Option<EntryHash>) {
    match r {
        CommandResult::Done { events, entry, .. } => (events, entry),
        other => panic!("not done: {other:?}"),
    }
}

fn stage_of(name: &str) -> Stage {
    *Stage::ALL.iter().find(|s| s.as_str() == name).unwrap_or_else(|| panic!("stage {name}"))
}

// ------------------------------------------------------------------ AC-1

/// AC-1: a W2 call passes the eight stages in the fixture's order, each once, and commits one
/// entry whose hash is the result's.
#[test]
fn ac1_stage_order() {
    let h = harness("ac1");
    let c = cases();
    let (events, entry) = done(h.run("gui", &write("a", 1)).unwrap());
    assert_eq!(h.probe.names(), strs(obj(&c), "order"));
    assert_eq!(Stage::ALL.iter().map(|s| s.as_str().to_string()).collect::<Vec<_>>(), strs(obj(&c), "order"));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].type_id().as_str(), "TestWritten");
    let (seq, tail) = h.tail().unwrap();
    assert_eq!((seq, Some(tail)), (1, entry));
    assert_eq!(h.head(), tail);
    assert_eq!(h.calls.load(Ordering::SeqCst), 1);
}

// ------------------------------------------------------------------ AC-2

/// AC-2 (forced): a probe refusing at each stage in turn gives that error, no later stage
/// runs, and the journal is unchanged.
#[test]
fn ac2_forced_failure_at_each_stage() {
    let c = cases();
    for f in arr(obj(&c), "forced") {
        let f = obj(f);
        let stage = stage_of(st(f, "stage"));
        let h = harness("ac2f");
        // One committed entry first, so "unchanged" compares a real head.
        done(h.run("gui", &write("a", 1)).unwrap());
        let before = (h.tail(), h.head());
        h.probe.clear();
        *h.probe.fail_at.lock().unwrap() = Some(stage);
        let e = h.run("gui", &write("b", 1)).unwrap_err();
        assert_eq!(e, probe_error(stage), "{stage}");
        assert_eq!(h.probe.names(), strs(f, "ran"), "{stage}");
        assert_eq!((h.tail(), h.head()), before, "{stage}: the journal changed");
    }
}

/// AC-2 (real): one real failure per stage, with the hand-labelled code and reason; later
/// stages do not run and the journal is unchanged.
#[test]
fn ac2_real_failure_at_each_stage() {
    let c = cases();
    for case in arr(obj(&c), "real") {
        let case = obj(case);
        let stage = st(case, "stage");
        let mut h = harness("ac2r");
        done(h.run("gui", &write("a", 1)).unwrap());
        let before = (h.tail(), h.head());
        let mut inv = Invocation::new(cid(st(case, "command")), case.get("params").unwrap().clone());
        if case.contains_key("expected_head") {
            inv.expected_head = Some(Digest([9; 32]));
        }
        if case.contains_key("hook_refuses") {
            let hook = Counting::new(Watch::All);
            hook.refuse_precommit.store(true, Ordering::SeqCst);
            h.bus.register_hook(hook.clone());
        }
        h.probe.clear();
        let calls = h.calls.load(Ordering::SeqCst);
        assert_err(h.run(st(case, "channel"), &inv), st(case, "code"), st(case, "reason"));
        assert_eq!(h.probe.names(), strs(case, "ran"), "{stage}");
        assert_eq!((h.tail(), h.head()), before, "{stage}: the journal changed");
        let ran_handler = strs(case, "ran").iter().any(|s| s == "handler");
        assert_eq!(h.calls.load(Ordering::SeqCst), calls + usize::from(ran_handler), "{stage}");
    }
}

/// AC-2: an append refused with a known outcome commits nothing, the hooks install nothing,
/// and the bus stays usable.
#[test]
fn ac2_failed_append_commits_nothing() {
    let mut h = harness("ac2a");
    let hook = Counting::new(Watch::All);
    h.bus.register_hook(hook.clone());
    h.tj.fail_append.store(APPEND_REFUSED, Ordering::SeqCst);
    assert_err(h.run("gui", &write("a", 1)), "conflict", "head_moved");
    assert_eq!(h.tail(), None);
    assert_eq!(hook.precommits.lock().unwrap().len(), 1);
    assert!(hook.folds.lock().unwrap().is_empty());
    assert!(!h.bus.is_poisoned());
    h.tj.fail_append.store(APPEND_OK, Ordering::SeqCst);
    done(h.run("gui", &write("a", 1)).unwrap());
    assert_eq!(h.tail().unwrap().0, 1);
}

/// An append that fails with `internal/journal_unavailable` has an unknown outcome (the entry
/// may have landed unfolded): the bus is poisoned and refuses every later commit.
#[test]
fn an_unknown_append_outcome_poisons_the_bus() {
    let mut h = harness("unknown");
    let hook = Counting::new(Watch::All);
    h.bus.register_hook(hook.clone());
    h.tj.fail_append.store(APPEND_UNKNOWN, Ordering::SeqCst);
    assert_err(h.run("gui", &write("a", 1)), "internal", "journal_unavailable");
    assert!(h.bus.is_poisoned());
    assert!(hook.folds.lock().unwrap().is_empty());
    h.tj.fail_append.store(APPEND_OK, Ordering::SeqCst);
    assert_err(h.run("gui", &write("a", 1)), "internal", "invariant_violated");
    assert_eq!(h.tail(), None);
}

// ------------------------------------------------------------------ AC-3

/// AC-3: an MCP call to a request_only command appends exactly one pending-request entry and
/// never calls the handler.
#[test]
fn ac3_request_only_files_one_request() {
    let mut h = harness("ac3");
    let hook = Counting::new(Watch::All);
    h.bus.register_hook(hook.clone());
    let c = cases();
    let r = obj(obj(&c).get("request").unwrap());
    let inv = Invocation::new(cid(st(r, "command")), r.get("params").unwrap().clone());
    let out = h.run(st(r, "channel"), &inv).unwrap();
    let CommandResult::Pending { entry, .. } = out else { panic!("not pending: {out:?}") };
    assert_eq!(h.probe.names(), strs(r, "ran"));
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    let (seq, tail) = h.tail().unwrap();
    assert_eq!((seq, tail), (1, entry));
    let folds = hook.folds.lock().unwrap();
    assert_eq!(folds.len(), 1);
    let HeaderBody::Command(cmd) = &folds[0].header.body else { panic!("not a command entry") };
    assert_eq!((cmd.id.as_str(), cmd.major, cmd.class), ("request.file_request", 1, SideEffect::W1));
    let commitment = fid_types::Digest::parse(st(r, "params_commitment")).unwrap();
    assert!(cmd.inputs.contains(&TaggedDigest { tag: "param".into(), digest: Digest(commitment.to_bytes()) }));
    assert_eq!(folds[0].events.len(), 1);
    assert_eq!(folds[0].events[0].type_id().as_str(), REQUEST_FILED);
    assert_eq!(folds[0].header.principal.kind, PrincipalKind::Agent);
    assert_eq!(folds[0].header.ai_assistance, AiAssistance::Executed);
}

/// REQ-003 with CORE.BUS.POL.PROJ: a person approves the filed request; the commit carries the
/// `RequestApproved` event and the request book (an origin-bound commit hook) folds it; a
/// second approval is refused, so an approval cannot be replayed.
#[test]
fn approval_commits_once_through_the_request_hook() {
    let mut h = harness("approve");
    let book = Arc::new(BookHook { book: Mutex::new(RequestBook::new()) });
    h.bus.register_hook(book.clone());
    let params = json(r#"{"target":"a","count":2}"#);
    let filed = h.run("mcp", &Invocation::new(cid("test.request@1"), params.clone())).unwrap();
    let CommandResult::Pending { request_id, entry } = filed else { panic!("{filed:?}") };
    assert_eq!(book.book.lock().unwrap().get(&request_id).unwrap().state(), RequestState::Pending);
    // The agent's request is on its own branch; main has not moved.
    assert_eq!(RecordJournal::branch_head(h.journal.as_ref(), "agent/claude").unwrap(), Some(entry));
    assert_eq!(h.head(), fid_journal::GENESIS_PREV);

    let mut inv = Invocation::new(cid("test.request@1"), params.clone());
    inv.approval_of = Some(request_id);
    let result = {
        let b = book.book.lock().unwrap().clone();
        h.run_on("gui", &b.view(h.head()), &inv)
    };
    let (events, _) = done(result.unwrap());
    let types: Vec<&str> = events.iter().map(|e| e.type_id().as_str()).collect();
    assert_eq!(types, ["TestWritten", REQUEST_APPROVED]);
    assert_eq!(book.book.lock().unwrap().get(&request_id).unwrap().state(), RequestState::Approved);
    assert_eq!(h.tail().unwrap().0, 2);

    let again = {
        let b = book.book.lock().unwrap().clone();
        h.run_on("gui", &b.view(h.head()), &inv)
    };
    assert_err(again, "policy", "bad_approval");
    assert_eq!(h.tail().unwrap().0, 2);
}

// ------------------------------------------------------------------ AC-6

/// AC-6: an R0 call writes nothing without a recording session; with one, exactly one
/// Activity Log record and still nothing in the Record Journal.
#[test]
fn ac6_reads_go_to_the_activity_log_only() {
    let h = harness("ac6");
    let read = Invocation::new(cid("test.noop_read@1"), json("{}"));
    let out = h.run("gui", &read).unwrap();
    let CommandResult::Done { entry, activity, .. } = out else { panic!("{out:?}") };
    assert_eq!(entry, None);
    let activity = activity.unwrap();
    assert_eq!((activity.stream, activity.written), (None, None));
    assert!(!h.alog.exists(), "the Activity Log was touched without recording");
    assert_eq!(h.tail(), None);
    assert_eq!(
        h.probe.names(),
        ["resolve", "validate", "authorise", "precondition", "handler", "effects"],
        "a read skips route and commit"
    );

    h.recording.set_recording(true, Some("rec-1"));
    let out = h.run("gui", &read).unwrap();
    let CommandResult::Done { entry, activity, .. } = out else { panic!("{out:?}") };
    assert_eq!(entry, None);
    let activity = activity.unwrap();
    assert_eq!(activity.stream, Some(fid_journal::stream::StreamId::Activity));
    assert!(activity.written.is_some());
    let text = std::fs::read_to_string(h.alog.join(CURRENT_FILE)).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert_eq!(h.tail(), None, "a read reached the Record Journal");
}

/// REQ-006: an R1 read by an MCP agent is not refused by the open-branch router and writes
/// nothing to the Record Journal.
#[test]
fn r1_reads_never_reach_the_record_journal() {
    let h = harness("r1");
    h.recording.set_recording(true, None);
    let out = h.run("mcp", &Invocation::new(cid("test.read_r1@1"), json(r#"{"target":"a","count":1}"#))).unwrap();
    let CommandResult::Done { entry: None, activity: Some(a), .. } = out else { panic!("{out:?}") };
    assert!(a.written.is_some());
    assert_eq!(h.tail(), None);
}

// ------------------------------------------------------------------ AC-7

/// AC-7: `consumed_r1` digests land in the entry's input digests, tagged `r1_result`.
#[test]
fn ac7_consumed_r1_becomes_an_input() {
    let mut h = harness("ac7");
    let hook = Counting::new(Watch::All);
    h.bus.register_hook(hook.clone());
    let mut inv = write("a", 1);
    let r1 = Digest([0x5a; 32]);
    inv.consumed_r1 = vec![r1];
    done(h.run("gui", &inv).unwrap());
    let folds = hook.folds.lock().unwrap();
    let HeaderBody::Command(cmd) = &folds[0].header.body else { panic!() };
    assert!(cmd.inputs.contains(&TaggedDigest { tag: R1_RESULT_TAG.into(), digest: r1 }), "{:?}", cmd.inputs);
}

// ------------------------------------------------------------------ AC-8

/// AC-8: a dry run returns the events of a real run and leaves the journal unchanged.
#[test]
fn ac8_dry_run_returns_the_events_and_commits_nothing() {
    let h = harness("ac8");
    let mut inv = write("a", 3);
    inv.dry_run = true;
    let out = h.run("gui", &inv).unwrap();
    let CommandResult::DryRun { events: dry } = out else { panic!("{out:?}") };
    assert_eq!(h.tail(), None);
    assert_eq!(h.probe.names(), ["resolve", "validate", "authorise", "precondition", "route", "handler", "effects"]);
    let (real, _) = done(h.run("gui", &write("a", 3)).unwrap());
    assert_eq!(dry, real);
    assert_eq!(h.tail().unwrap().0, 1);

    // A dry run of a call that would file a request files nothing. The agent's request goes
    // to its own agent branch, which is still empty.
    let agent_view = View { head: fid_journal::GENESIS_PREV };
    let mut req = Invocation::new(cid("test.request@1"), json(r#"{"target":"a","count":1}"#));
    req.dry_run = true;
    let out = h.run_on("mcp", &agent_view, &req).unwrap();
    assert_eq!(out, CommandResult::WouldFile { command: cid("test.request@1") });
    assert_eq!(h.tail().unwrap().0, 1);
}

/// REQ-008: a dry-run filing runs the route guards, so it reports the same refusal a real
/// filing would: a wrong `expected_head`, a stale view, an idempotency key.
#[test]
fn a_dry_run_filing_reports_the_route_conflicts_of_a_real_one() {
    let h = harness("dryfile");
    let agent_view = View { head: fid_journal::GENESIS_PREV };
    let stale_view = View { head: Digest([3; 32]) };
    let base = Invocation::new(cid("test.request@1"), json(r#"{"target":"a","count":1}"#));
    let mut wrong_head = base.clone();
    wrong_head.expected_head = Some(Digest([9; 32]));
    let mut keyed = base.clone();
    keyed.idempotency_key = Some("k1".into());
    let cases: [(&Invocation, &View, &str, &str); 3] = [
        (&wrong_head, &agent_view, "conflict", "expected_head_mismatch"),
        (&base, &stale_view, "conflict", "head_moved"),
        (&keyed, &agent_view, "validation", "schema"),
    ];
    for (inv, view, code, reason) in cases {
        let mut dry = inv.clone();
        dry.dry_run = true;
        assert_err(h.run_on("mcp", view, &dry), code, reason);
        assert_err(h.run_on("mcp", view, inv), code, reason);
    }
    assert_eq!(h.tail(), None);
}

/// A read carrying an envelope field only a commit can honour is refused, not ignored.
#[test]
fn a_read_refuses_commit_only_envelope_fields() {
    let h = harness("readenv");
    let read = Invocation::new(cid("test.noop_read@1"), json("{}"));
    let mut with_head = read.clone();
    with_head.expected_head = Some(h.head());
    let mut keyed = read.clone();
    keyed.idempotency_key = Some("k1".into());
    let mut consumed = read.clone();
    consumed.consumed_r1 = vec![Digest([0x5a; 32])];
    for inv in [&with_head, &keyed, &consumed] {
        h.probe.clear();
        assert_err(h.run("gui", inv), "validation", "schema");
        assert_eq!(h.probe.names(), ["resolve", "validate"]);
    }
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    done(h.run("gui", &read).unwrap());
}

// ------------------------------------------------------------------ AC-9

/// AC-9: a script principal gets `policy/system_only`; the system principal gets the events a
/// real run would commit, and nothing is committed.
#[test]
fn ac9_run_handlers_as_system_is_system_only() {
    let h = harness("ac9");
    let calls = [SystemCall { command: cid("test.write@1"), params: json(r#"{"target":"a","count":1}"#) }];
    let view = View { head: h.head() };
    assert_err(h.bus.run_handlers_as_system(&h.sessions, &h.cli, &calls, &view, &NoRoots), "policy", "system_only");
    assert_err(h.bus.run_handlers_as_system(&h.sessions, &h.mcp, &calls, &view, &NoRoots), "policy", "system_only");
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    let out = h.bus.run_handlers_as_system(&h.sessions, &h.internal, &calls, &view, &NoRoots).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(h.tail(), None);
    let (real, _) = done(h.run("gui", &write("a", 1)).unwrap());
    assert_eq!(out[0].events(), real.as_slice());
    // A failing precondition in the list is returned.
    let bad = [SystemCall { command: cid("test.write@1"), params: json(r#"{"target":"a","count":0}"#) }];
    assert_err(h.bus.run_handlers_as_system(&h.sessions, &h.internal, &bad, &view, &NoRoots), "precondition", "false");
}

// ------------------------------------------------------------------ AC-10

/// AC-10 (bench_r0_overhead): the median time of 10,000 no-op R0 calls through the whole
/// pipeline is below 1 ms. CI runners are not the reference hardware, so this is a smoke
/// bound, not the measurement the spec asks for [unverified until measured].
#[test]
fn bench_r0_overhead() {
    let h = harness("bench");
    let read = Invocation::new(cid("test.noop_read@1"), json("{}"));
    let view = View { head: h.head() };
    let mut times = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let t = Instant::now();
        h.run_on("gui", &view, &read).unwrap();
        times.push(t.elapsed());
    }
    times.sort();
    let median = times[times.len() / 2];
    println!("bench_r0_overhead: median {median:?} over 10,000 calls");
    assert!(median.as_micros() < 1000, "median {median:?}");
}

// ------------------------------------------------------------------ the compare-and-swap

/// Appends a W2 entry to `main` behind the bus's back (a concurrent committer).
struct Intruder {
    journal: Arc<Journal>,
    branch: String,
    fired: AtomicBool,
}

impl StageProbe for Intruder {
    fn enter(&self, stage: Stage) -> Result<(), BusError> {
        if stage == Stage::Commit && !self.fired.swap(true, Ordering::SeqCst) {
            let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            let draft = EntryDraft {
                stream: STREAM.into(),
                branch: self.branch.clone(),
                logical_parent: None,
                time: EntryTime { utc: format_utc(since), mono_ns: 1, sync: "ntp".into() },
                principal: PrincipalDraft {
                    kind: PrincipalKind::Human,
                    identity: CValue::Str("carol".into()),
                    on_behalf_of: None,
                },
                ai_assistance: AiAssistance::None,
                manifests: manifests(),
                reason: None,
                human_assertion: None,
                agent_coedited: false,
                body: DraftBody::Command(CommandDraft {
                    id: "test.write".into(),
                    major: 1,
                    class: SideEffect::W2,
                    determinism: Determinism::D0,
                    seed: 0,
                    args: json(r#"{"target":"x","count":1}"#),
                    events: Vec::new(),
                    inputs: Vec::new(),
                    consumed_r1: Vec::new(),
                    outputs: Vec::new(),
                    algorithms: Vec::new(),
                    event_sources: Vec::new(),
                }),
            };
            self.journal.append(AppendRequest::new(draft)).unwrap();
        }
        Ok(())
    }
}

/// REQ-002, FOLD-008 (d): a commit on the same branch that lands between the route stage and
/// the commit makes the commit `conflict/head_moved`; nothing of the call is appended and no
/// hook folds it.
#[test]
fn a_concurrent_commit_on_the_branch_is_a_conflict() {
    let mut h = harness("cas");
    let hook = Counting::new(Watch::All);
    h.bus.register_hook(hook.clone());
    h.bus.set_probe(Arc::new(Intruder {
        journal: Arc::clone(&h.journal),
        branch: MAIN.into(),
        fired: AtomicBool::new(false),
    }));
    assert_err(h.run("gui", &write("a", 1)), "conflict", "head_moved");
    assert_eq!(h.tail().unwrap().0, 1, "only the intruder's entry");
    assert!(hook.precommits.lock().unwrap().is_empty());
    assert!(hook.folds.lock().unwrap().is_empty());
    // The retry against the new head commits.
    done(h.run("gui", &write("a", 1)).unwrap());
    assert_eq!(h.tail().unwrap().0, 2);
}

/// A commit on another branch moves the tail but not this branch's head: no conflict, and the
/// entry is appended on the new tail.
#[test]
fn a_commit_on_another_branch_is_not_a_conflict() {
    let mut h = harness("cas2");
    h.bus.set_probe(Arc::new(Intruder {
        journal: Arc::clone(&h.journal),
        branch: "side".into(),
        fired: AtomicBool::new(false),
    }));
    let (_, entry) = done(h.run("gui", &write("a", 1)).unwrap());
    let (seq, tail) = h.tail().unwrap();
    assert_eq!((seq, Some(tail)), (2, entry));
}

/// The route stage refuses a call whose view is not at the branch head.
#[test]
fn a_stale_view_is_refused_at_route() {
    let h = harness("stale");
    done(h.run("gui", &write("a", 1)).unwrap());
    let old = View { head: fid_journal::GENESIS_PREV };
    h.probe.clear();
    assert_err(h.run_on("gui", &old, &write("a", 1)), "conflict", "head_moved");
    assert_eq!(h.probe.names(), ["resolve", "validate", "authorise", "precondition", "route"]);
    assert_eq!(h.tail().unwrap().0, 1);
}

// ------------------------------------------------------------------ hooks

/// The hook sees the entry exactly as appended: same hash, the decided-against head, the
/// events; `Watch::Effects` filters entries; the logical parent is the previous head.
#[test]
fn hooks_see_the_entry_as_appended() {
    let mut h = harness("hooks");
    let all = Counting::new(Watch::All);
    let other = Counting::new(Watch::Effects(BTreeSet::from(["SomethingElse".to_string()])));
    h.bus.register_hook(all.clone());
    h.bus.register_hook(other.clone());
    let (events, first) = done(h.run("gui", &write("a", 1)).unwrap());
    let (_, second) = done(h.run("gui", &write("b", 2)).unwrap());
    let folds = all.folds.lock().unwrap();
    assert_eq!(folds.len(), 2);
    assert_eq!(Some(folds[0].header.hash), first);
    assert_eq!(Some(folds[1].header.hash), second);
    assert_eq!(folds[0].decided_at, fid_journal::GENESIS_PREV);
    assert_eq!(folds[0].header.logical_parent, None);
    assert_eq!(Some(folds[1].decided_at), first);
    assert_eq!(folds[1].header.logical_parent, first);
    assert_eq!(folds[0].events, events);
    let origin = folds[0].origin.clone().unwrap();
    assert_eq!(origin.command().as_str(), "test.write@1");
    assert_eq!(origin.principal().kind, PrincipalKind::Human);
    let HeaderBody::Command(cmd) = &folds[0].header.body else { panic!() };
    assert_eq!(cmd.seed, 42);
    assert_eq!(all.precommits.lock().unwrap().len(), 2);
    assert!(other.precommits.lock().unwrap().is_empty());
    assert!(other.folds.lock().unwrap().is_empty());
}

/// A fold that refuses after the append poisons the bus: the call is
/// `internal/invariant_violated` and every later commit is refused.
#[test]
fn a_fold_refusal_poisons_the_bus() {
    let mut h = harness("poison");
    let hook = Counting::new(Watch::All);
    hook.refuse_fold.store(true, Ordering::SeqCst);
    h.bus.register_hook(hook.clone());
    assert_err(h.run("gui", &write("a", 1)), "internal", "invariant_violated");
    assert!(h.bus.is_poisoned());
    assert_eq!(h.tail().unwrap().0, 1);
    hook.refuse_fold.store(false, Ordering::SeqCst);
    assert_err(h.run("gui", &write("a", 1)), "internal", "invariant_violated");
    assert_eq!(h.tail().unwrap().0, 1);
}

/// A panic inside a hook while the commit lock is held poisons the bus: the entry landed but
/// no hook state was installed, so every later commit is refused.
#[test]
fn a_panicking_hook_poisons_the_bus() {
    let mut h = harness("panic");
    h.bus.register_hook(Arc::new(Panicking));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h.run("gui", &write("a", 1))));
    assert!(r.is_err(), "the hook's panic propagates");
    assert!(h.bus.is_poisoned());
    assert_eq!(h.tail().unwrap().0, 1, "the entry landed before the install");
    assert_err(h.run("gui", &write("a", 1)), "internal", "invariant_violated");
    assert_eq!(h.tail().unwrap().0, 1);
}

// ------------------------------------------------------------------ entry time

/// The entry time is taken after the commit lock is acquired: its monotonic stamp is later
/// than the counter when the commit stage read the tail under the lock (the old code took it
/// when the draft was built, before the commit stage, and fails this). Holds for a command
/// entry and for a filing.
#[test]
fn the_entry_time_is_taken_under_the_commit_lock() {
    let mut h = harness("time");
    let hook = Counting::new(Watch::All);
    h.bus.register_hook(hook.clone());
    done(h.run("gui", &write("a", 1)).unwrap());
    let first = hook.folds.lock().unwrap()[0].header.time.mono_ns;
    let at_tail = h.tj.mono_at_tail.load(Ordering::SeqCst);
    assert!(first > at_tail, "entry time {first} was taken before the tail read ({at_tail})");

    let filed = h.run_on(
        "mcp",
        &View { head: fid_journal::GENESIS_PREV },
        &Invocation::new(cid("test.request@1"), json(r#"{"target":"a","count":1}"#)),
    );
    assert!(matches!(filed.unwrap(), CommandResult::Pending { .. }));
    let second = hook.folds.lock().unwrap()[1].header.time.mono_ns;
    let at_tail = h.tj.mono_at_tail.load(Ordering::SeqCst);
    assert!(second > at_tail, "filing time {second} was taken before the tail read ({at_tail})");
}

/// A clock refusal has a known outcome: the host clock stepped back behind the chain tail,
/// the journal refuses the append (`precondition/false`, detail `clock: ...`), nothing is
/// appended, the bus is not poisoned, and the next commit with a sane clock succeeds.
#[test]
fn a_clock_step_back_refuses_one_commit_and_the_next_succeeds() {
    let mut h = harness("clock");
    let hook = Counting::new(Watch::All);
    h.bus.register_hook(hook.clone());
    done(h.run("gui", &write("a", 1)).unwrap());
    h.env.stepped_back.store(true, Ordering::SeqCst);
    let e = assert_err(h.run("gui", &write("b", 1)), "precondition", "false");
    assert!(e.detail().as_untrusted_str().starts_with("clock: time_regressed"), "{e}");
    assert!(!h.bus.is_poisoned());
    assert_eq!(h.tail().unwrap().0, 1);
    assert_eq!(hook.folds.lock().unwrap().len(), 1, "the refused entry was not installed");
    h.env.stepped_back.store(false, Ordering::SeqCst);
    done(h.run("gui", &write("b", 1)).unwrap());
    assert_eq!(h.tail().unwrap().0, 2);
}

/// A poisoned bus does not report what a commit would do: a commit-class dry run and a filing
/// (dry or real, before its request ID is drawn) are `internal/invariant_violated`; a read
/// still runs.
#[test]
fn a_poisoned_bus_refuses_dry_runs_and_filings() {
    let h = harness("poisondry");
    h.tj.fail_append.store(APPEND_UNKNOWN, Ordering::SeqCst);
    assert_err(h.run("gui", &write("a", 1)), "internal", "journal_unavailable");
    assert!(h.bus.is_poisoned());
    let mut dry = write("a", 1);
    dry.dry_run = true;
    assert_err(h.run("gui", &dry), "internal", "invariant_violated");
    let agent_view = View { head: fid_journal::GENESIS_PREV };
    let request = Invocation::new(cid("test.request@1"), json(r#"{"target":"a","count":1}"#));
    let mut dry_request = request.clone();
    dry_request.dry_run = true;
    let ids = h.env.mono.load(Ordering::SeqCst);
    assert_err(h.run_on("mcp", &agent_view, &dry_request), "internal", "invariant_violated");
    assert_err(h.run_on("mcp", &agent_view, &request), "internal", "invariant_violated");
    assert_eq!(h.env.mono.load(Ordering::SeqCst), ids, "a request ID was drawn on a poisoned bus");
    let read = Invocation::new(cid("test.noop_read@1"), json("{}"));
    done(h.run("gui", &read).unwrap());
}

// ------------------------------------------------------------------ reason

/// CORE.JRN.UNDO.SUPER's need: a registered reason parameter becomes the entry's reason
/// (committed in the header, opened in the payload).
#[test]
fn reason_parameter_is_recorded_as_the_entry_reason() {
    let mut h = harness("reason");
    let hook = Counting::new(Watch::All);
    h.bus.register_hook(hook.clone());
    h.bus.set_reason_param(&cid("test.release@1"), "reason").unwrap();
    let inv =
        Invocation::new(cid("test.release@1"), json(r#"{"target":"a","count":1,"reason":"wrong part revision"}"#));
    done(h.run("gui", &inv).unwrap());
    let folds = hook.folds.lock().unwrap();
    assert!(folds[0].header.reason_commit.is_some());
    assert_eq!(folds[0].payload.values.get(&Field::Reason), Some(&CValue::Str("wrong part revision".into())));
    // A command without a registered reason parameter has none.
    drop(folds);
    done(h.run("gui", &write("a", 1)).unwrap());
    assert!(hook.folds.lock().unwrap()[1].header.reason_commit.is_none());
}

// ------------------------------------------------------------------ fail-closed gaps

/// Until CORE.BUS.ROUTE part 2: an agent's commit is refused at route, and an idempotency key is
/// refused rather than ignored.
#[test]
fn route_gaps_fail_closed() {
    let h = harness("gaps");
    assert_err(h.run("mcp", &write("a", 1)), "policy", "agent_write_forbidden");
    let mut inv = write("a", 1);
    inv.idempotency_key = Some("k1".into());
    assert_err(h.run("gui", &inv), "validation", "schema");
    assert_eq!(h.tail(), None);
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
}

/// The bus's own agent-branch check holds whatever the router says: a hostile router that
/// sends an agent's commit or request to `main` gets `policy/agent_write_forbidden` at the
/// route stage, the handler is not called and nothing is appended; a person's commit still
/// goes through. The shipped `SessionRouter` routes an agent's request to the open branch, and
/// the check refuses that too.
#[test]
fn an_agent_never_writes_outside_an_agent_branch_whatever_the_router() {
    let h = harness_with("hostile", Arc::new(Hostile(MAIN)));
    h.probe.clear();
    assert_err(h.run("mcp", &write("a", 1)), "policy", "agent_write_forbidden");
    assert_eq!(h.probe.names(), ["resolve", "validate", "authorise", "precondition", "route"]);
    let request = Invocation::new(cid("test.request@1"), json(r#"{"target":"a","count":1}"#));
    assert_err(h.run("mcp", &request), "policy", "agent_write_forbidden");
    let mut dry = request.clone();
    dry.dry_run = true;
    assert_err(h.run("mcp", &dry), "policy", "agent_write_forbidden");
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    assert_eq!(h.tail(), None);
    done(h.run("gui", &write("a", 1)).unwrap());
    assert_eq!(h.tail().unwrap().0, 1);

    let open = harness_with("open", Arc::new(router(MAIN)));
    assert_err(open.run("mcp", &request), "policy", "agent_write_forbidden");
    assert_eq!(open.tail(), None);

    // Names that only look like agent branches are not valid branch names at all.
    for name in ["agent/../main", "agent//main", "agent/Main"] {
        let h = harness_with("hostile2", Arc::new(Hostile(name)));
        assert_err(h.run("mcp", &write("a", 1)), "validation", "bad_branch_name");
        assert_err(
            h.run_on("mcp", &View { head: fid_journal::GENESIS_PREV }, &request),
            "validation",
            "bad_branch_name",
        );
        assert_eq!(h.tail(), None, "{name}");
    }
}
