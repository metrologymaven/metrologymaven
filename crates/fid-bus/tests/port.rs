//! Acceptance criteria AC-1 .. AC-10 of spec node CORE.BUS.PORT, plus the bus error catalogue
//! check of its Design. Expected values are hand-labelled from the leaf's requirements, from
//! `fixtures/core/bus/principal/presence.json` (hand-labelled, CORE.BUS.PRIN) and from
//! `schema/bus-errors.json`. The commands are those of the CORE.BUS.PIPE.STAGE probe registry
//! (`fixtures/core/bus/pipe/stage/probes/registry.json`, made by its own Python script) and
//! four test commands defined here. The Record Journal is in memory and holds real
//! `fid-journal` entries; the bus's commits and the port's `principal_upgrade` entries go to
//! the same chain.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{ConflictReason, InternalReason, NotFoundReason};
use fid_bus::handler::{EntityId, EntityRef, Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet};
use fid_bus::handler::{SettingKey, StateView, ValidParams};
use fid_bus::pipeline::{AppendFailure, AppendOutcome, Bus, BusEnv, BusParts, RecordJournal};
use fid_bus::policy::projection::RequestId;
use fid_bus::port::{
    channel_kind, to_api_error, CallParts, JournalId, LocalBus, LocalPort, SessionHost, UpgradeJournal,
    NO_PRESENCE_PROVIDER, OTHER_JOURNAL,
};
use fid_bus::principal::{
    Channel, ChannelKind, ConfirmMethod, HumanPresence, JournalAppend, OsUser, PresenceOutcome, SessionId,
    UpgradeClock, UpgradeContext, UpgradeRequest,
};
use fid_bus::route::SessionRouter;
use fid_bus::validate::{CommandTable, NoRoots};
use fid_bus::{BusError, Detail, MAX_DETAIL_CHARS};
use fid_bus_api::{ApiError, ApiReply, ApiStage, BusCall, BusPort, PortKind, PrincipalKind};
use fid_canon::{parse, CValue, Digest};
use fid_journal::branch::BranchName;
use fid_journal::stream::{format_utc, ActivityLog, ActivityLogConfig, Streams};
use fid_journal::{
    build, AppendRequest, Entry, EntryDraft, EntryHash, EntryKind, EntryTime, Manifests, OsSalts, FIRST_SEQ,
    GENESIS_PREV,
};
use fid_registry::{Catalogue, CommandDef, ContextTypes, PredContext};
use fid_types::CommandId;
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type Obj = BTreeMap<String, CValue>;

const MANIFEST: Digest = Digest([7; 32]);
const STREAM: &str = "record";
const MAIN: &str = "main";
const USER: &str = "user-a";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(rel: &str) -> CValue {
    let bytes = std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    parse(&bytes).unwrap()
}

fn json(s: &str) -> CValue {
    parse(s.as_bytes()).unwrap()
}

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("not an object: {other:?}"),
    }
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: {other:?}"),
    }
}

fn cid(s: &str) -> CommandId {
    CommandId::parse(s).unwrap()
}

fn label(s: &str) -> SessionId {
    SessionId::new(s).unwrap()
}

fn user(s: &str) -> OsUser {
    OsUser::new(s).unwrap()
}

fn manifests() -> Manifests {
    Manifests { registry: MANIFEST, kernel: Digest([2; 32]) }
}

// ------------------------------------------------------------------ the in-memory journal

/// One Record Journal chain in memory, of real entries. The bus commits to it through
/// [`MemRecord`] and the port records upgrades on it through [`MemUpgrades`].
#[derive(Default)]
struct Store {
    entries: Mutex<Vec<Entry>>,
}

impl Store {
    fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    fn kinds(&self) -> Vec<(EntryKind, PrincipalKind)> {
        self.entries.lock().unwrap().iter().map(|e| (e.header.kind(), e.header.principal.kind)).collect()
    }

    fn head(&self, branch: &str) -> Option<Digest> {
        self.entries.lock().unwrap().iter().rev().find(|e| e.header.branch == branch).map(|e| e.header.hash)
    }

    fn next(entries: &[Entry]) -> (u64, Digest) {
        match entries.last() {
            Some(e) => (e.header.seq + 1, e.header.hash),
            None => (FIRST_SEQ, GENESIS_PREV),
        }
    }
}

struct MemRecord(Arc<Store>);

impl RecordJournal for MemRecord {
    fn tail(&self) -> Result<Option<(u64, Digest)>, BusError> {
        Ok(self.0.entries.lock().unwrap().last().map(|e| (e.header.seq, e.header.hash)))
    }

    fn branch_head(&self, branch: &str) -> Result<Option<Digest>, BusError> {
        Ok(self.0.head(branch))
    }

    fn append(&self, req: AppendRequest) -> Result<EntryHash, AppendFailure> {
        let mut entries = self.0.entries.lock().unwrap();
        let (seq, prev) = Store::next(&entries);
        if req.expected_prev.is_some_and(|p| p != prev) {
            return Err(AppendFailure {
                error: BusError::Conflict(ConflictReason::HeadMoved, Detail::new("tail moved")),
                outcome: AppendOutcome::NotAppended,
            });
        }
        let built = match req.salts {
            Some(mut salts) => build(&req.draft, seq, prev, &mut salts),
            None => build(&req.draft, seq, prev, &mut OsSalts),
        };
        let entry = built.map_err(|e| AppendFailure {
            error: BusError::Internal(InternalReason::InvariantViolated, Detail::new(&e.to_string())),
            outcome: AppendOutcome::NotAppended,
        })?;
        let hash = entry.header.hash;
        entries.push(entry);
        Ok(hash)
    }
}

/// The upgrade append of a [`Store`]; with `fail`, every append fails and nothing lands.
struct MemUpgrades {
    store: Arc<Store>,
    fail: bool,
}

impl JournalAppend for MemUpgrades {
    fn append(&mut self, draft: &EntryDraft) -> Result<(), BusError> {
        if self.fail {
            return Err(BusError::Internal(InternalReason::JournalUnavailable, Detail::new("test journal is down")));
        }
        let mut entries = self.store.entries.lock().unwrap();
        let (seq, prev) = Store::next(&entries);
        let entry = build(draft, seq, prev, &mut OsSalts)
            .map_err(|e| BusError::Internal(InternalReason::InvariantViolated, Detail::new(&e.to_string())))?;
        entries.push(entry);
        Ok(())
    }
}

// ------------------------------------------------------------------ state, env, parts

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

/// Each call runs against the head of `main` in its store, with no precondition roots.
struct Parts(Arc<Store>);

impl CallParts for Parts {
    fn inputs(&self, _channel: &dyn Channel) -> Result<(Box<dyn StateView>, Box<dyn PredContext>), BusError> {
        Ok((Box::new(View { head: self.0.head(MAIN).unwrap_or(GENESIS_PREV) }), Box::new(NoRoots)))
    }
}

struct Env {
    mono: AtomicU64,
}

impl BusEnv for Env {
    fn now(&self) -> EntryTime {
        let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let mono_ns = self.mono.fetch_add(1, Ordering::SeqCst) + 1;
        EntryTime { utc: format_utc(since), mono_ns, sync: "ntp".into() }
    }
    fn seed(&self) -> u64 {
        42
    }
    fn request_id(&self) -> Result<RequestId, BusError> {
        let n = u128::from(self.mono.fetch_add(1, Ordering::SeqCst));
        RequestId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000u128 | (n & 0xffff_ffff)))
    }
}

/// Each `now` advances the monotonic clock by the next step (nanoseconds; 1 ms after the
/// listed steps), as in the CORE.BUS.PRIN tests: `prompt_takes(ms)` makes the prompt take `ms`.
struct FakeClock {
    mono: i128,
    steps: Vec<i128>,
    calls: usize,
}

impl FakeClock {
    fn prompt_takes(elapsed_ms: i128) -> Box<FakeClock> {
        Box::new(FakeClock { mono: 5_000_000_000, steps: vec![0, elapsed_ms * 1_000_000], calls: 0 })
    }
}

impl UpgradeClock for FakeClock {
    fn now(&mut self) -> EntryTime {
        self.mono += self.steps.get(self.calls).copied().unwrap_or(1_000_000);
        self.calls += 1;
        EntryTime {
            utc: format!("2026-11-25T10:00:{:02}Z", self.calls.min(59)),
            mono_ns: u64::try_from(self.mono).unwrap(),
            sync: "ntp".into(),
        }
    }
}

// ------------------------------------------------------------------ presence providers

/// A scripted presence provider that counts its calls; with `block`, it sleeps inside
/// `confirm` and sets `returned` just before answering.
struct Scripted {
    outcome: PresenceOutcome,
    block: Duration,
    calls: AtomicUsize,
    returned: AtomicBool,
}

impl Scripted {
    fn new(outcome: PresenceOutcome) -> Arc<Scripted> {
        Arc::new(Scripted {
            outcome,
            block: Duration::ZERO,
            calls: AtomicUsize::new(0),
            returned: AtomicBool::new(false),
        })
    }

    fn confirms(os_user: &str) -> Arc<Scripted> {
        Scripted::new(PresenceOutcome::Confirmed { os_user: user(os_user), method: ConfirmMethod::GuiPrompt })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl HumanPresence for Scripted {
    fn confirm(&self, _req: &UpgradeRequest, _deadline: Duration) -> PresenceOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.block.is_zero() {
            std::thread::sleep(self.block);
        }
        self.returned.store(true, Ordering::SeqCst);
        self.outcome.clone()
    }
}

fn host_as(os_user: &str, presence: Option<&Arc<Scripted>>, clock: Box<FakeClock>) -> Arc<SessionHost> {
    let presence = presence.map(|p| Arc::clone(p) as Arc<dyn HumanPresence + Send + Sync>);
    Arc::new(SessionHost::new(user(os_user), presence, clock).unwrap())
}

fn host(presence: Option<&Arc<Scripted>>) -> Arc<SessionHost> {
    host_as(USER, presence, FakeClock::prompt_takes(1))
}

// ------------------------------------------------------------------ commands and handlers

fn defs() -> Vec<CommandDef> {
    let r = read_json("fixtures/core/bus/pipe/stage/probes/registry.json");
    let CValue::Arr(commands) = obj(&r).get("commands").unwrap() else { panic!("commands") };
    let mut values = commands.clone();
    let read = values.iter().find(|c| st(obj(c), "id") == "test.noop_read").unwrap().clone();
    // The four commands of this test, built from the probe registry's R0 read.
    let extra = |id: &str, title: &str, schema: &str, mcp: bool| {
        let mut o = obj(&read).clone();
        o.insert("id".into(), CValue::Str(id.into()));
        o.insert("title".into(), CValue::Str(title.into()));
        o.insert("params_schema".into(), json(schema));
        if !mcp {
            o.insert("exposure".into(), json(r#"["cli","gui"]"#));
            o.insert("mcp_projection".into(), CValue::Str("never".into()));
        }
        CValue::Obj(o)
    };
    values.push(extra(
        "test.echo_principal",
        "Echo the principal kind",
        r#"{"type":"object","additionalProperties":false,"properties":{
            "actor":{"type":"string"},"on_behalf_of":{"type":"string"},"session":{"type":"string"},
            "principal":{"type":"object","additionalProperties":false,"properties":{"kind":{"type":"string"}}}}}"#,
        true,
    ));
    values.push(extra(
        "test.lookup",
        "Look up a missing entity",
        r#"{"type":"object","additionalProperties":false,"properties":{}}"#,
        true,
    ));
    values.push(extra(
        "test.hidden",
        "Not on MCP",
        r#"{"type":"object","additionalProperties":false,"properties":{}}"#,
        false,
    ));
    values.into_iter().map(|c| CommandDef::from_value(c).unwrap()).collect()
}

fn table() -> Arc<CommandTable> {
    let catalogue =
        Catalogue::from_json(&std::fs::read(root().join("fixtures/core/bus/unit/catalogue.json")).unwrap()).unwrap();
    let t = CommandTable::build(MANIFEST, defs(), catalogue, &ContextTypes::new()).unwrap();
    assert!(t.compile_errors().is_empty(), "{:?}", t.compile_errors());
    Arc::new(t)
}

/// Writes one `TestWritten` event for a commit class (`TestOther`, which no command declares,
/// for target `undeclared`), nothing for a read.
struct Probe {
    emits: bool,
}

impl Handler for Probe {
    fn run(&self, _view: &dyn StateView, params: &ValidParams, _ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        if !self.emits {
            return Ok(Vec::new());
        }
        let p = obj(params.value());
        let mut body = Obj::new();
        body.insert("target".into(), p.get("target").cloned().unwrap_or(CValue::Null));
        body.insert("count".into(), p.get("count").cloned().unwrap_or(CValue::Null));
        let kind = if p.get("target") == Some(&CValue::Str("undeclared".into())) { "TestOther" } else { "TestWritten" };
        Ok(vec![Event::new(EventType::new(kind).unwrap(), CValue::Obj(body))])
    }
}

/// Returns the principal kind it ran as, from the handler context (AC-4).
struct EchoPrincipal;

impl Handler for EchoPrincipal {
    fn run(&self, _view: &dyn StateView, _params: &ValidParams, _ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        Ok(Vec::new())
    }
    fn run_with_result(
        &self,
        _view: &dyn StateView,
        _params: &ValidParams,
        ctx: &HandlerCtx,
    ) -> Result<(Vec<Event>, Option<CValue>), HandlerError> {
        let kind = CValue::Str(ctx.principal().kind().as_str().to_string());
        Ok((Vec::new(), Some(CValue::Obj(BTreeMap::from([("kind".to_string(), kind)])))))
    }
}

/// Fails with `not_found/unknown_entity` (AC-7).
struct Lookup;

impl Handler for Lookup {
    fn run(&self, _view: &dyn StateView, _params: &ValidParams, _ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        Err(HandlerError::Refused(BusError::NotFound(NotFoundReason::UnknownEntity, Detail::new("no entity e-1"))))
    }
}

fn handlers() -> HandlerSet {
    let mut set = HandlerSet::new();
    for d in defs() {
        let h: Box<dyn Handler> = match d.id.as_str() {
            "test.echo_principal" => Box::new(EchoPrincipal),
            "test.lookup" => Box::new(Lookup),
            _ => Box::new(Probe {
                emits: !matches!(d.side_effect, fid_types::SideEffect::R0 | fid_types::SideEffect::R1),
            }),
        };
        set.register(d, h).unwrap();
    }
    set
}

fn bus_over(store: &Arc<Store>) -> Bus {
    // Recording is off, so the Activity Log writes nothing and its folder is never created.
    let alog = std::env::temp_dir().join(format!("fid-port-alog-{}", std::process::id()));
    let activity = Arc::new(ActivityLog::new(alog, ActivityLogConfig::default(), manifests()));
    Bus::new(BusParts {
        table: table(),
        handlers: handlers(),
        router: Arc::new(SessionRouter::new(BranchName::parse(MAIN).unwrap())),
        journal: Arc::new(MemRecord(Arc::clone(store))),
        streams: Streams::new(activity),
        env: Arc::new(Env { mono: AtomicU64::new(1) }),
        stream: STREAM.to_string(),
        manifests: manifests(),
    })
}

fn upgrade_ctx() -> UpgradeContext {
    UpgradeContext { stream: STREAM.into(), branch: MAIN.into(), manifests: manifests() }
}

/// A bus over a project: its store and an upgrade journal on that store named `id`.
fn project_with(id: JournalId, store: &Arc<Store>, fail: bool) -> Arc<LocalBus> {
    let append = Box::new(MemUpgrades { store: Arc::clone(store), fail });
    let upgrades = UpgradeJournal::new(id, append, upgrade_ctx());
    Arc::new(LocalBus::new(bus_over(store), Arc::new(Parts(Arc::clone(store))), Some(upgrades)))
}

fn project() -> (Arc<Store>, Arc<LocalBus>) {
    let store = Arc::new(Store::default());
    let bus = project_with(JournalId::fresh().unwrap(), &store, false);
    (store, bus)
}

/// The no-project bus: no upgrade journal.
fn no_project() -> (Arc<Store>, Arc<LocalBus>) {
    let store = Arc::new(Store::default());
    let bus = Arc::new(LocalBus::new(bus_over(&store), Arc::new(Parts(Arc::clone(&store))), None));
    (store, bus)
}

fn call(command: &str, params: &str) -> BusCall {
    BusCall { command: cid(command), params: json(params), dry_run: false }
}

fn w2() -> BusCall {
    call("test.write@1", r#"{"target":"a","count":1}"#)
}

fn r0() -> BusCall {
    call("test.noop_read@1", "{}")
}

fn failed(reply: ApiReply) -> ApiError {
    match reply {
        ApiReply::Failed(e) => e,
        other => panic!("not failed: {other:?}"),
    }
}

fn parts(e: &ApiError) -> (&str, &str, ApiStage) {
    (e.code(), e.reason(), e.stage())
}

fn committed(reply: ApiReply) -> fid_types::Digest {
    match reply {
        ApiReply::Committed { entry, .. } => entry,
        other => panic!("not committed: {other:?}"),
    }
}

fn open(host: &Arc<SessionHost>, kind: PortKind, name: &str, bus: &Arc<LocalBus>) -> LocalPort {
    host.open(kind, label(name), Arc::clone(bus)).unwrap()
}

// ------------------------------------------------------------------ REQ-001 and threads

#[test]
fn ports_open_only_the_four_kinds_and_none_starts_human() {
    let kinds: Vec<ChannelKind> = PortKind::ALL.iter().map(|k| channel_kind(*k)).collect();
    assert_eq!(kinds, [ChannelKind::Cli, ChannelKind::Python, ChannelKind::Control, ChannelKind::Mcp]);
    for k in &kinds {
        assert_ne!(k.base_kind(), PrincipalKind::Human, "{k}");
    }
    fn send_sync<T: Send + Sync>() {}
    fn send<T: Send>() {}
    send_sync::<SessionHost>();
    send_sync::<LocalBus>();
    send::<LocalPort>();
}

// ------------------------------------------------------------------ AC-1

/// Exactly four arms: a port kind added to `PortKind` fails to build here until it is mapped,
/// and no arm is `human`.
fn expected_kind(kind: PortKind) -> PrincipalKind {
    match kind {
        PortKind::Cli => PrincipalKind::Script,
        PortKind::Python => PrincipalKind::Script,
        PortKind::Control => PrincipalKind::Script,
        PortKind::Mcp => PrincipalKind::Agent,
    }
}

#[test]
fn ac1_each_port_kind_derives_without_an_upgrade() {
    let host = host(None);
    let (_store, bus) = project();
    for kind in PortKind::ALL {
        let port = open(&host, *kind, &format!("{kind}-1"), &bus);
        assert_eq!(port.principal_kind(), expected_kind(*kind), "{kind}");
        let info = port.channel();
        assert_eq!((info.kind, info.identity.as_str()), (*kind, USER));
        assert!(info.session.starts_with(&format!("{kind}-1#")), "{}", info.session);
        assert!(!port.has_pending_upgrade());
    }
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_confirmed_upgrade_is_journaled_before_the_human_call() {
    let presence = Scripted::confirms(USER);
    let host = host(Some(&presence));
    let (store, bus) = project();
    let mut port = open(&host, PortKind::Cli, "cli-1", &bus);
    assert_eq!(port.principal_kind(), PrincipalKind::Script);
    port.confirm_human().unwrap();
    assert_eq!(presence.calls(), 1);
    assert_eq!(port.principal_kind(), PrincipalKind::Human);
    let entry = committed(port.call(&w2()));

    assert_eq!(
        store.kinds(),
        [(EntryKind::PrincipalUpgrade, PrincipalKind::Script), (EntryKind::Command, PrincipalKind::Human)]
    );
    let entries = store.entries.lock().unwrap().clone();
    assert_eq!(entries[1].header.prev, entries[0].header.hash);
    assert_eq!(entry, fid_types::Digest::from_bytes(entries[1].header.hash.0));

    // Another port of the same host is a session of its own: script.
    let mut other = open(&host, PortKind::Cli, "cli-2", &bus);
    assert_eq!(other.principal_kind(), PrincipalKind::Script);
    committed(other.call(&w2()));
    assert_eq!(store.kinds()[2], (EntryKind::Command, PrincipalKind::Script));

    // A new host over the same journal (a restart) opens ports as script.
    let restarted = self::host(Some(&presence));
    let fresh = open(&restarted, PortKind::Cli, "cli-1", &bus);
    assert_eq!(fresh.principal_kind(), PrincipalKind::Script);
    assert_eq!(presence.calls(), 1);
}

// ------------------------------------------------------------------ AC-3

fn outcome_from(o: &Obj) -> PresenceOutcome {
    match st(o, "result") {
        "confirmed" => PresenceOutcome::Confirmed {
            os_user: user(st(o, "os_user")),
            method: match st(o, "method") {
                "gui_prompt" => ConfirmMethod::GuiPrompt,
                "os_presence" => ConfirmMethod::OsPresence,
                other => panic!("method {other}"),
            },
        },
        "declined" => PresenceOutcome::Declined,
        "timed_out" => PresenceOutcome::TimedOut,
        other => panic!("result {other}"),
    }
}

fn elapsed_ms(o: &Obj) -> i128 {
    match o.get("elapsed_ms") {
        Some(CValue::I64(n)) => i128::from(*n),
        Some(CValue::Num(n)) => *n as i128,
        other => panic!("elapsed_ms {other:?}"),
    }
}

#[test]
fn ac3_scripted_presence_outcomes() {
    let fixture = read_json("fixtures/core/bus/principal/presence.json");
    let f = obj(&fixture);
    let os_user = st(f, "session_os_user");
    let CValue::Arr(cases) = f.get("cases").unwrap() else { panic!("cases") };
    let mut refusals = Vec::new();
    for case in cases {
        let case = obj(case);
        let name = st(case, "name");
        let presence = Scripted::new(outcome_from(obj(case.get("outcome").unwrap())));
        let host = host_as(os_user, Some(&presence), FakeClock::prompt_takes(elapsed_ms(case)));
        let (store, bus) = project();
        let mut port = open(&host, PortKind::Cli, "cli-1", &bus);
        let result = port.confirm_human();
        assert_eq!(presence.calls(), 1, "{name}");
        match st(case, "expect") {
            "ok" => {
                result.unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(port.principal_kind(), PrincipalKind::Human, "{name}");
                assert_eq!(store.kinds(), [(EntryKind::PrincipalUpgrade, PrincipalKind::Script)], "{name}");
            }
            expect => {
                let e = result.unwrap_err();
                assert_eq!(format!("{}/{}", e.code(), e.reason()), expect, "{name}");
                assert_eq!(e.stage(), ApiStage::Session, "{name}");
                assert_eq!(port.principal_kind(), PrincipalKind::Script, "{name}");
                assert_eq!(store.len(), 0, "{name}: the journal changed");
                assert!(!port.has_pending_upgrade(), "{name}");
                refusals.push(e.reason().to_string());
            }
        }
    }
    // The leaf's three named outcomes are among the fixture's refusals.
    for reason in ["declined", "timeout", "user_mismatch"] {
        assert!(refusals.iter().any(|r| r == reason), "{reason}");
    }

    // No provider: refused without asking anyone.
    let counting = Scripted::confirms(USER);
    let host = host(None);
    let (store, bus) = project();
    let mut port = open(&host, PortKind::Cli, "cli-1", &bus);
    let e = port.confirm_human().unwrap_err();
    assert_eq!(parts(&e), ("upgrade_refused", "declined", ApiStage::Session));
    assert_eq!(e.detail(), NO_PRESENCE_PROVIDER);
    assert_eq!(counting.calls(), 0);
    assert_eq!(port.principal_kind(), PrincipalKind::Script);
    assert_eq!(store.len(), 0);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_parameters_and_environment_do_not_make_a_principal() {
    // Edition 2021: `set_var` is safe. Nothing in the bus or the port reads it.
    std::env::set_var("FID_PRINCIPAL", "human");
    let host = host(None);
    let (store, bus) = project();
    let mut port = open(&host, PortKind::Cli, "cli-1", &bus);
    let params = r#"{"actor":"user-a","on_behalf_of":"user-a","principal":{"kind":"human"},"session":"s1"}"#;
    let reply = port.call(&call("test.echo_principal@1", params));
    assert_eq!(reply, ApiReply::Computed { result: Some(json(r#"{"kind":"script"}"#)) });
    assert_eq!(port.principal_kind(), PrincipalKind::Script);
    assert_eq!(store.len(), 0);
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_a_prompt_holds_no_session_table_lock() {
    let presence = Arc::new(Scripted {
        outcome: PresenceOutcome::Confirmed { os_user: user(USER), method: ConfirmMethod::OsPresence },
        block: Duration::from_secs(2),
        calls: AtomicUsize::new(0),
        returned: AtomicBool::new(false),
    });
    let host = host(Some(&presence));
    let (_store, bus) = project();
    let mut a = open(&host, PortKind::Cli, "cli-a", &bus);
    let mut b = open(&host, PortKind::Python, "py-b", &bus);
    let asking = std::thread::spawn(move || {
        let r = a.confirm_human();
        (a, r)
    });
    let start = Instant::now();
    while presence.calls() == 0 {
        assert!(start.elapsed() < Duration::from_secs(30), "the provider was never asked");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(b.call(&r0()), ApiReply::Computed { result: None });
    let c = open(&host, PortKind::Cli, "cli-c", &bus);
    assert!(!presence.returned.load(Ordering::SeqCst), "B and C waited for the provider");
    assert_eq!(c.principal_kind(), PrincipalKind::Script);
    let (a, r) = asking.join().unwrap();
    r.unwrap();
    assert_eq!(a.principal_kind(), PrincipalKind::Human);
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_an_upgrade_waits_for_a_journal_to_record_it() {
    let presence = Scripted::confirms(USER);
    let host = host(Some(&presence));
    let (none_store, none_bus) = no_project();
    let (store, bus) = project();
    let mut port = open(&host, PortKind::Cli, "cli-1", &none_bus);
    port.confirm_human().unwrap();
    assert_eq!(port.principal_kind(), PrincipalKind::Script);
    assert!(port.has_pending_upgrade());
    assert_eq!((none_store.len(), store.len()), (0, 0));
    port.attach(Arc::clone(&bus)).unwrap();
    assert_eq!(store.kinds(), [(EntryKind::PrincipalUpgrade, PrincipalKind::Script)]);
    assert_eq!(port.principal_kind(), PrincipalKind::Human);
    assert!(!port.has_pending_upgrade());
    assert_eq!(store.len(), 1);

    // A journal whose append fails: the pending upgrade is dropped and nothing is recorded.
    let mut port = open(&host, PortKind::Cli, "cli-2", &none_bus);
    port.confirm_human().unwrap();
    let broken_store = Arc::new(Store::default());
    let broken = project_with(JournalId::fresh().unwrap(), &broken_store, true);
    let e = port.attach(broken).unwrap_err();
    assert_eq!(parts(&e), ("internal", "journal_unavailable", ApiStage::Session));
    assert_eq!(port.principal_kind(), PrincipalKind::Script);
    assert!(!port.has_pending_upgrade());
    assert_eq!(broken_store.len(), 0);
    // The port stayed on its bus.
    assert_eq!(port.call(&r0()), ApiReply::Computed { result: None });
    let (working_store, working) = project();
    port.attach(working).unwrap();
    assert_eq!(working_store.len(), 0);
    assert_eq!(port.principal_kind(), PrincipalKind::Script);
}

// ------------------------------------------------------------------ AC-7

#[test]
fn ac7_each_failure_reports_its_stage() {
    let host = host(None);
    let (store, bus) = project();
    let mut cli = open(&host, PortKind::Cli, "cli-1", &bus);
    let mut mcp = open(&host, PortKind::Mcp, "mcp-1", &bus);
    let release = r#"{"target":"a","count":1,"reason":"r"}"#;
    let cases = [
        (call("test.nope@1", "{}"), ("not_found", "unknown_command", ApiStage::Resolve)),
        (call("test.write@1", r#"{"target":1,"count":1}"#), ("validation", "schema", ApiStage::Validate)),
        // A script may not run W3 (CORE.BUS.POL.CLASS). The leaf's "W3 from an mcp port" files a
        // request instead (CORE.BUS.POL.PROJ), checked below.
        (call("test.release@1", release), ("policy", "class_forbidden", ApiStage::Authorise)),
        (call("test.write@1", r#"{"target":"a","count":0}"#), ("precondition", "false", ApiStage::Precondition)),
        (call("test.lookup@1", "{}"), ("not_found", "unknown_entity", ApiStage::Handler)),
        (
            call("test.write@1", r#"{"target":"undeclared","count":1}"#),
            ("undeclared_effect", "undeclared_event_type", ApiStage::Effects),
        ),
    ];
    for (c, want) in cases {
        let e = failed(cli.call(&c));
        assert_eq!(parts(&e), want, "{}", c.command);
    }
    let e = failed(cli.call(&call("test.lookup@1", "{}")));
    assert_eq!(e.detail(), "no entity e-1", "detail unchanged");

    // An agent's W3 call becomes a request; the session router refuses an agent's write to
    // `main` at the route stage.
    let e = failed(mcp.call(&call("test.release@1", release)));
    assert_eq!(parts(&e), ("policy", "agent_write_forbidden", ApiStage::Route));

    // D-035: a command hidden from MCP is refused exactly as an unknown one, stage included.
    let hidden = failed(mcp.call(&call("test.hidden@1", "{}")));
    let unknown = failed(mcp.call(&call("test.nope@1", "{}")));
    assert_eq!(parts(&hidden), parts(&unknown));
    assert_eq!(parts(&hidden), ("not_found", "unknown_command", ApiStage::Resolve));
    assert_eq!(hidden.detail(), r#""test.hidden"@1: unknown command"#);
    assert_eq!(unknown.detail(), r#""test.nope"@1: unknown command"#);
    // The same command runs on the CLI.
    assert_eq!(cli.call(&call("test.hidden@1", "{}")), ApiReply::Computed { result: None });
    assert_eq!(store.len(), 0);

    // A dry run is never reported as committed (D-045).
    let mut dry = w2();
    dry.dry_run = true;
    let ApiReply::DryRun { result } = cli.call(&dry) else { panic!("dry run") };
    let CValue::Arr(events) = obj(&result).get("events").unwrap() else { panic!("events") };
    assert_eq!(events.len(), 1);
    assert_eq!(store.len(), 0);
}

// ------------------------------------------------------------------ AC-8

#[test]
fn ac8_dropping_a_port_ends_its_session() {
    let presence = Scripted::confirms(USER);
    let host = host(Some(&presence));
    let (_store, bus) = project();
    let mut port = open(&host, PortKind::Cli, "cli-1", &bus);
    port.confirm_human().unwrap();
    assert_eq!(host.upgraded_sessions(), 1);
    drop(port);
    assert_eq!(host.upgraded_sessions(), 0);
    let again = open(&host, PortKind::Cli, "cli-1", &bus);
    assert_eq!(again.principal_kind(), PrincipalKind::Script);

    let mut closing = open(&host, PortKind::Python, "py-1", &bus);
    closing.confirm_human().unwrap();
    assert_eq!(host.upgraded_sessions(), 1);
    closing.close();
    assert_eq!(host.upgraded_sessions(), 0);
}

// ------------------------------------------------------------------ AC-9

#[test]
fn ac9_a_grant_stays_on_its_journal() {
    let presence = Scripted::confirms(USER);
    let host = host(Some(&presence));
    let (store_a, bus_a) = project();
    let (store_b, bus_b) = project();
    let (store_n, bus_n) = no_project();
    let mut port = open(&host, PortKind::Cli, "cli-1", &bus_a);
    port.confirm_human().unwrap();
    assert_eq!(store_a.len(), 1);
    for target in [&bus_b, &bus_n] {
        let e = port.attach(Arc::clone(target)).unwrap_err();
        assert_eq!(parts(&e), ("precondition", "false", ApiStage::Session));
        assert_eq!(e.detail(), OTHER_JOURNAL);
    }
    assert_eq!((store_b.len(), store_n.len()), (0, 0));
    assert_eq!(port.principal_kind(), PrincipalKind::Human);
    committed(port.call(&w2()));
    assert_eq!(store_a.kinds()[1], (EntryKind::Command, PrincipalKind::Human));

    // Another bus over the same journal (same ID) accepts the grant.
    let same = project_with(bus_a.upgrade_journal().unwrap().clone(), &store_a, false);
    port.attach(same).unwrap();
    assert_eq!(port.principal_kind(), PrincipalKind::Human);

    // A port with no grant and no pending upgrade moves freely.
    let mut plain = open(&host, PortKind::Cli, "cli-2", &bus_n);
    for target in [&bus_a, &bus_b, &bus_n] {
        plain.attach(Arc::clone(target)).unwrap();
        assert_eq!(plain.principal_kind(), PrincipalKind::Script);
    }
    assert_eq!((store_a.len(), store_b.len(), store_n.len()), (2, 0, 0));
}

// ------------------------------------------------------------------ AC-10

#[test]
fn ac10_control_and_mcp_ports_cannot_be_upgraded() {
    let presence = Scripted::confirms(USER);
    let with = host(Some(&presence));
    let without = host(None);
    let (store, bus) = project();
    for h in [&with, &without] {
        for (kind, base) in [(PortKind::Mcp, PrincipalKind::Agent), (PortKind::Control, PrincipalKind::Script)] {
            let mut port = open(h, kind, &format!("{kind}-1"), &bus);
            let e = port.confirm_human().unwrap_err();
            assert_eq!(parts(&e), ("upgrade_refused", "channel", ApiStage::Session), "{kind}");
            assert_eq!(port.principal_kind(), base, "{kind}");
            assert!(!port.has_pending_upgrade());
        }
    }
    assert_eq!(presence.calls(), 0);
    assert_eq!(store.len(), 0);
}

// ------------------------------------------------------------------ the catalogue

/// Every code and reason of `schema/bus-errors.json` builds an `ApiError` with its strings
/// unchanged, at every stage, also with a detail at the 2,000-character limit.
#[test]
fn every_bus_error_fits_an_api_error() {
    assert_eq!(MAX_DETAIL_CHARS, fid_bus_api::MAX_DETAIL_CHARS);
    let catalog = read_json("schema/bus-errors.json");
    let codes = obj(obj(&catalog).get("codes").unwrap());
    let long: String = "\u{e9}".repeat(MAX_DETAIL_CHARS);
    let mut pairs = 0;
    for (code, reasons) in codes {
        let CValue::Arr(reasons) = reasons else { panic!("{code}") };
        for reason in reasons {
            let CValue::Str(reason) = reason else { panic!("{code}") };
            for detail in ["", "x", long.as_str()] {
                let e = BusError::from_wire(code, reason, detail).unwrap_or_else(|| panic!("{code}/{reason}"));
                for stage in ApiStage::ALL {
                    let a = to_api_error(&e, *stage).unwrap();
                    assert_eq!(
                        (a.code(), a.reason(), a.detail(), a.stage()),
                        (code.as_str(), reason.as_str(), detail, *stage)
                    );
                }
            }
            pairs += 1;
        }
    }
    assert!(pairs > 50, "{pairs}");
}
