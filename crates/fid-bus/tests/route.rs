//! Acceptance criteria of spec node CORE.BUS.ROUTE, part 1: AC-3 (non-agent principals commit
//! to the session's open branch), AC-4 (`expected_head` conflict with the current head) and
//! AC-5 (one FIFO writer per branch), plus the lock order between the per-branch tickets and
//! the bus's commit lock, and the agent fail-closed refusal that stays until part 2.
//!
//! Commands come from `fixtures/core/bus/pipe/stage/probes/registry.json` (the synthetic
//! `test.write@1` W2 and `test.noop_read@1` R0 of CORE.BUS.PIPE.STAGE); expected outcomes are
//! hand-labelled from the spec's rules. Commits go to a real `fid_journal::Journal`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::BusError;
use fid_bus::handler::{EntityId, EntityRef, Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet};
use fid_bus::handler::{SettingKey, StateView, ValidParams};
use fid_bus::pipeline::{Bus, BusEnv, BusParts, Call, CommandResult, Invocation, RecordJournal, Router};
use fid_bus::policy::projection::RequestId;
use fid_bus::principal::{Channel, ChannelKind, ConfirmMethod, HumanPresence, Identity, JournalAppend, OsUser};
use fid_bus::principal::{PresenceOutcome, PrincipalId, PrincipalKind, SessionId, SessionKey, SessionTable};
use fid_bus::principal::{UpgradeClock, UpgradeContext, UpgradeGrant, UpgradeRequest};
use fid_bus::route::{check_expected_head, BranchLocks, HeadConflict, SessionRouter};
use fid_bus::validate::{CommandTable, NoRoots};
use fid_canon::{parse, CValue, Digest};
use fid_journal::branch::BranchName;
use fid_journal::stream::{format_utc, ActivityLog, ActivityLogConfig, RecordingFlag, Streams};
use fid_journal::{EntryDraft, EntryHash, EntryTime, Journal, Manifests, VerifyOptions, GENESIS_PREV};
use fid_registry::{Catalogue, CommandDef, ContextTypes};
use fid_store::ProjectLocation;
use fid_types::{CommandId, SideEffect};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type Obj = BTreeMap<String, CValue>;

const MANIFEST: Digest = Digest([7; 32]);
const STREAM: &str = "record";
const MAIN: &str = "main";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn json(s: &str) -> CValue {
    parse(s.as_bytes()).unwrap()
}

fn cid(s: &str) -> CommandId {
    CommandId::parse(s).unwrap()
}

fn bname(s: &str) -> BranchName {
    BranchName::parse(s).unwrap()
}

fn manifests() -> Manifests {
    Manifests { registry: MANIFEST, kernel: Digest([2; 32]) }
}

fn assert_err<T: std::fmt::Debug>(r: Result<T, BusError>, code: &str, reason: &str) -> BusError {
    let e = r.unwrap_err();
    assert_eq!((e.code().to_string().as_str(), e.reason()), (code, reason), "{e}");
    e
}

fn defs() -> Vec<CommandDef> {
    let r = parse(&std::fs::read(root().join("fixtures/core/bus/pipe/stage/probes/registry.json")).unwrap()).unwrap();
    let CValue::Obj(o) = r else { panic!("registry is not an object") };
    let Some(CValue::Arr(cmds)) = o.get("commands") else { panic!("no commands") };
    cmds.iter().map(|c| CommandDef::from_value(c.clone()).unwrap()).collect()
}

fn table() -> Arc<CommandTable> {
    let catalogue =
        Catalogue::from_json(&std::fs::read(root().join("fixtures/core/bus/unit/catalogue.json")).unwrap()).unwrap();
    let t = CommandTable::build(MANIFEST, defs(), catalogue, &ContextTypes::new()).unwrap();
    assert!(t.compile_errors().is_empty(), "{:?}", t.compile_errors());
    Arc::new(t)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let name = format!("fid-route-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::path::absolute(std::env::temp_dir().join(name)).unwrap();
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
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
    grant: Option<UpgradeGrant>,
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
    fn upgrade_grant(&self) -> Option<&UpgradeGrant> {
        self.grant.as_ref()
    }
}

fn channel(table: &mut SessionTable, kind: ChannelKind, label: &str, subject: &str) -> TestChannel {
    let mut identity = Identity::new(PrincipalId::new(subject).unwrap());
    identity.os_user = Some(OsUser::new(subject).unwrap());
    let session = table.open_session(SessionId::new(label).unwrap()).unwrap();
    TestChannel { kind, identity, session, grant: None }
}

// ------------------------------------------------------------------ session upgrade (D-012)

struct Confirms(&'static str);

impl HumanPresence for Confirms {
    fn confirm(&self, _req: &UpgradeRequest, _deadline: Duration) -> PresenceOutcome {
        PresenceOutcome::Confirmed { os_user: OsUser::new(self.0).unwrap(), method: ConfirmMethod::GuiPrompt }
    }
}

struct StepClock(u64);

impl UpgradeClock for StepClock {
    fn now(&mut self) -> EntryTime {
        self.0 += 1_000_000;
        EntryTime { utc: "2026-11-21T10:00:00Z".into(), mono_ns: self.0, sync: "ntp".into() }
    }
}

/// The upgrade entry is not part of these tests: it is dropped.
struct Discard;

impl JournalAppend for Discard {
    fn append(&mut self, _draft: &EntryDraft) -> Result<(), BusError> {
        Ok(())
    }
}

fn upgrade(table: &mut SessionTable, ch: &mut TestChannel, os_user: &'static str) {
    let pending = table.prepare_upgrade(&*ch).unwrap();
    let confirmed = pending.confirm(&Confirms(os_user), &mut StepClock(1)).unwrap();
    let ctx = UpgradeContext { stream: STREAM.into(), branch: MAIN.into(), manifests: manifests() };
    ch.grant = Some(table.commit_upgrade(confirmed, &mut Discard, &ctx).unwrap());
}

// ------------------------------------------------------------------ state, env, handler

/// A view that reads its branch's head from the journal when asked, so a call decided under
/// its branch ticket sees the head as it is then (AC-5).
struct LiveView {
    journal: Arc<Journal>,
    branch: String,
}

impl StateView for LiveView {
    fn entity(&self, _id: &EntityId) -> Option<EntityRef<'_>> {
        None
    }
    fn head(&self) -> Digest {
        RecordJournal::branch_head(self.journal.as_ref(), &self.branch).unwrap().unwrap_or(GENESIS_PREV)
    }
    fn setting(&self, _key: &SettingKey) -> Option<CValue> {
        None
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

/// A gate the handler of a `"block"` call waits at, after saying it entered.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    turn: Condvar,
}

impl Gate {
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.turn.notify_all();
    }
    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.turn.wait(open).unwrap();
        }
    }
}

/// Emits `TestWritten` for a write. A call with target `"block"` reports that it entered the
/// handler and waits at the gate.
struct WriteHandler {
    calls: Arc<AtomicUsize>,
    emits: bool,
    gate: Arc<Gate>,
    entered: mpsc::Sender<String>,
}

impl Handler for WriteHandler {
    fn run(&self, _view: &dyn StateView, params: &ValidParams, _ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.emits {
            return Ok(Vec::new());
        }
        let CValue::Obj(p) = params.value() else { panic!("params are not an object") };
        let target = match p.get("target") {
            Some(CValue::Str(s)) => s.clone(),
            other => panic!("target {other:?}"),
        };
        if target.starts_with("block") {
            self.entered.send(target.clone()).unwrap();
            self.gate.wait();
        }
        let mut body = Obj::new();
        body.insert("target".into(), CValue::Str(target));
        body.insert("count".into(), p.get("count").cloned().unwrap_or(CValue::Null));
        Ok(vec![Event::new(EventType::new("TestWritten").unwrap(), CValue::Obj(body))])
    }
}

// ------------------------------------------------------------------ the harness

struct Harness {
    _dir: TempDir,
    journal: Arc<Journal>,
    router: Arc<SessionRouter>,
    calls: Arc<AtomicUsize>,
    gate: Arc<Gate>,
    entered: Mutex<mpsc::Receiver<String>>,
    bus: Bus,
    sessions: SessionTable,
}

fn harness(tag: &str) -> Harness {
    let dir = TempDir::new(tag);
    let (loc, lock) = ProjectLocation::create(&dir.0.join("project")).unwrap();
    let journal = Arc::new(Journal::open(loc, lock, STREAM).unwrap());
    let router = Arc::new(SessionRouter::new(bname(MAIN)));
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Gate::default());
    let (tx, rx) = mpsc::channel();
    let mut handlers = HandlerSet::new();
    for d in defs() {
        let emits = !matches!(d.side_effect, SideEffect::R0 | SideEffect::R1);
        let h = WriteHandler { calls: Arc::clone(&calls), emits, gate: Arc::clone(&gate), entered: tx.clone() };
        handlers.register(d, Box::new(h)).unwrap();
    }
    let activity = Arc::new(ActivityLog::new(dir.0.join("alog"), ActivityLogConfig::default(), manifests()));
    let bus = Bus::new(BusParts {
        table: table(),
        handlers,
        router: Arc::clone(&router) as Arc<dyn Router>,
        journal: Arc::clone(&journal) as Arc<dyn RecordJournal>,
        streams: Streams::with_flag(activity, Arc::new(RecordingFlag::new())),
        env: Arc::new(Env { mono: AtomicU64::new(1) }) as Arc<dyn BusEnv>,
        stream: STREAM.to_string(),
        manifests: manifests(),
    });
    Harness { _dir: dir, journal, router, calls, gate, entered: Mutex::new(rx), bus, sessions: SessionTable::new() }
}

impl Harness {
    fn head(&self, branch: &str) -> Option<Digest> {
        RecordJournal::branch_head(self.journal.as_ref(), branch).unwrap()
    }

    fn seq(&self) -> u64 {
        RecordJournal::tail(self.journal.as_ref()).unwrap().map_or(0, |t| t.0)
    }

    fn view(&self, branch: &str) -> LiveView {
        LiveView { journal: Arc::clone(&self.journal), branch: branch.to_string() }
    }

    /// Runs `inv` on `ch` against the live view of the branch `ch`'s session has open.
    fn run(&self, ch: &TestChannel, inv: &Invocation) -> Result<CommandResult, BusError> {
        let open = self.router.open_branch_of(&ch.session).unwrap();
        let view = self.view(open.as_str());
        self.bus.execute(&Call { sessions: &self.sessions, channel: ch, view: &view, roots: &NoRoots }, inv)
    }

    fn entered(&self) -> String {
        self.entered.lock().unwrap().recv_timeout(Duration::from_secs(60)).expect("a handler entered")
    }
}

fn write(target: &str, count: i64) -> Invocation {
    Invocation::new(cid("test.write@1"), json(&format!(r#"{{"target":"{target}","count":{count}}}"#)))
}

fn entry(r: CommandResult) -> EntryHash {
    match r {
        CommandResult::Done { entry: Some(e), .. } => e,
        other => panic!("not a commit: {other:?}"),
    }
}

// ------------------------------------------------------------------ AC-3

/// AC-3: a CLI session upgraded to human (D-012) commits to the branch the session has open;
/// the working branch is unchanged.
#[test]
fn ac3_an_upgraded_cli_session_commits_to_its_open_branch() {
    let mut h = harness("ac3");
    let mut cli = channel(&mut h.sessions, ChannelKind::Cli, "cli-bob", "bob");
    upgrade(&mut h.sessions, &mut cli, "bob");
    assert_eq!(h.sessions.derive_principal(&cli).unwrap().kind(), PrincipalKind::Human);
    h.router.open_branch(&cli.session, bname("feature/fixture-b")).unwrap();

    let e = entry(h.run(&cli, &write("a", 1)).unwrap());
    assert_eq!(h.head("feature/fixture-b"), Some(e));
    assert_eq!(h.head(MAIN), None, "the working branch is unchanged");
    assert_eq!(h.seq(), 1);
}

/// REQ-003 for every non-agent principal: each session commits to its own open branch, a
/// session that opened none to the working branch; reopening the working branch, or closing
/// the session, routes it back there.
#[test]
fn every_session_commits_to_its_own_open_branch() {
    let mut h = harness("open");
    let gui = channel(&mut h.sessions, ChannelKind::Gui, "gui-alice", "alice");
    let script = channel(&mut h.sessions, ChannelKind::Cli, "cli-carol", "carol");
    let service = channel(&mut h.sessions, ChannelKind::Service, "svc-nightly", "nightly");
    h.router.open_branch(&script.session, bname("scratch/carol")).unwrap();

    let g = entry(h.run(&gui, &write("a", 1)).unwrap());
    let s = entry(h.run(&script, &write("b", 1)).unwrap());
    let v = entry(h.run(&service, &write("c", 1)).unwrap());
    assert_eq!(h.sessions.derive_principal(&script).unwrap().kind(), PrincipalKind::Script);
    assert_eq!(h.sessions.derive_principal(&service).unwrap().kind(), PrincipalKind::Service);
    assert_eq!(h.head(MAIN), Some(v), "gui then service on the working branch");
    assert_ne!(g, v);
    assert_eq!(h.head("scratch/carol"), Some(s));

    h.router.open_branch(&script.session, bname(MAIN)).unwrap();
    let s2 = entry(h.run(&script, &write("b", 2)).unwrap());
    assert_eq!(h.head(MAIN), Some(s2));
    h.router.open_branch(&script.session, bname("scratch/carol")).unwrap();
    h.router.close_session(&script.session).unwrap();
    assert_eq!(h.router.open_branch_of(&script.session).unwrap(), bname(MAIN));
}

/// Part 2 is not built: an agent's commit is refused with `policy/agent_write_forbidden` by
/// the router, before any handler runs, whatever branch its session has open; an agent's
/// read still runs; nothing is appended.
#[test]
fn agent_commits_fail_closed_until_part_2() {
    let mut h = harness("agent");
    let mcp = channel(&mut h.sessions, ChannelKind::Mcp, "mcp-claude", "claude");
    assert_err(h.run(&mcp, &write("a", 1)), "policy", "agent_write_forbidden");
    h.router.open_branch(&mcp.session, bname("agent/claude-code/1")).unwrap();
    let e = assert_err(h.run(&mcp, &write("a", 1)), "policy", "agent_write_forbidden");
    assert!(e.detail().as_untrusted_str().contains("part 2"), "{e}");
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    let read = Invocation::new(cid("test.noop_read@1"), json("{}"));
    assert!(matches!(h.run(&mcp, &read).unwrap(), CommandResult::Done { entry: None, .. }));
    assert_eq!(h.seq(), 0);
}

// ------------------------------------------------------------------ AC-4

/// AC-4 (error path): `expected_head` set to an older hash gives
/// `conflict/expected_head_mismatch` naming the current head; nothing is journaled, the
/// handler does not run, and the branch is free again for the next writer.
#[test]
fn ac4_an_old_expected_head_is_a_conflict_with_the_current_head() {
    let mut h = harness("ac4");
    let gui = channel(&mut h.sessions, ChannelKind::Gui, "gui-alice", "alice");
    let first = entry(h.run(&gui, &write("a", 1)).unwrap());
    let second = entry(h.run(&gui, &write("a", 2)).unwrap());
    let calls = h.calls.load(Ordering::SeqCst);

    let mut inv = write("a", 3);
    inv.expected_head = Some(first);
    let e = assert_err(h.run(&gui, &inv), "conflict", "expected_head_mismatch");
    let detail = e.detail().as_untrusted_str();
    assert!(detail.contains(&format!("current head {second}")), "{detail}");
    assert!(detail.contains(&format!("expected head {first}")), "{detail}");
    assert_eq!((h.seq(), h.head(MAIN)), (2, Some(second)), "nothing journaled");
    assert_eq!(h.calls.load(Ordering::SeqCst), calls, "the handler did not run");
    assert_eq!(h.bus.branch_writers(MAIN), 0, "the ticket was released");

    // The same call with the current head commits.
    inv.expected_head = Some(second);
    let third = entry(h.run(&gui, &inv).unwrap());
    assert_eq!(h.head(MAIN), Some(third));
}

/// REQ-004: the typed guard carries the current head; as a bus error it is
/// `conflict/expected_head_mismatch`. No expectation, or the right one, passes.
#[test]
fn the_expected_head_guard_carries_the_current_head() {
    let (old, cur) = (Digest([1; 32]), Digest([2; 32]));
    let c = check_expected_head("main", Some(old), cur).unwrap_err();
    assert_eq!(c, HeadConflict { branch: "main".into(), expected: old, current: cur });
    let e = BusError::from(c);
    assert_eq!((e.code().to_string().as_str(), e.reason()), ("conflict", "expected_head_mismatch"));
    assert!(e.detail().as_untrusted_str().ends_with(&format!("current head {cur}")));
    assert_eq!(check_expected_head("main", None, cur), Ok(()));
    assert_eq!(check_expected_head("main", Some(cur), cur), Ok(()));
    assert!(check_expected_head("main", Some(old), GENESIS_PREV).is_err());
}

// ------------------------------------------------------------------ AC-5

const WRITERS: usize = 8;
const PER_WRITER: usize = 1_000;

/// AC-5: 8 threads committing 1,000 W2 commands each to one branch, each against a view that
/// reads the branch head when the bus asks (under the branch ticket): every call commits (no
/// `conflict/head_moved` from an interleaved writer), the branch holds exactly 8,000 new
/// entries, the chain verifies with gap-free sequence numbers 1..=8000, and the branch head is
/// the tail.
#[test]
fn ac5_eight_writers_on_one_branch() {
    let mut h = harness("ac5");
    let chans: Vec<TestChannel> = (0..WRITERS)
        .map(|i| channel(&mut h.sessions, ChannelKind::Gui, &format!("gui-w{i}"), &format!("writer{i}")))
        .collect();
    let h = &h;
    let hashes: Vec<Vec<EntryHash>> = std::thread::scope(|s| {
        let joins: Vec<_> = chans
            .iter()
            .enumerate()
            .map(|(i, ch)| {
                s.spawn(move || {
                    let view = h.view(MAIN);
                    let call = Call { sessions: &h.sessions, channel: ch, view: &view, roots: &NoRoots };
                    (0..PER_WRITER)
                        .map(|n| {
                            let inv = write(&format!("w{i}"), i64::try_from(n + 1).unwrap());
                            match h.bus.execute(&call, &inv) {
                                Ok(r) => entry(r),
                                Err(e) => panic!("writer {i}, call {n}: {e} ({})", e.detail().as_untrusted_str()),
                            }
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        joins.into_iter().map(|j| j.join().unwrap()).collect()
    });
    let total = WRITERS * PER_WRITER;
    let distinct: BTreeSet<[u8; 32]> = hashes.iter().flatten().map(|h| h.0).collect();
    assert_eq!(distinct.len(), total);
    assert_eq!(h.calls.load(Ordering::SeqCst), total);
    let tail = RecordJournal::tail(h.journal.as_ref()).unwrap().unwrap();
    assert_eq!(tail.0, u64::try_from(total).unwrap());
    assert_eq!(h.head(MAIN), Some(tail.1));
    let report = h.journal.verify_chain(&VerifyOptions::default()).unwrap();
    assert_eq!(report.failure, None);
    assert_eq!(report.checked.entries, u64::try_from(total).unwrap());
    assert_eq!(h.bus.branch_writers(MAIN), 0);
    assert!(!h.bus.is_poisoned());
}

// ------------------------------------------------------------------ FIFO and lock order

/// REQ-005: waiting writers are served in arrival order (the order they took their tickets).
#[test]
fn branch_tickets_are_served_in_arrival_order() {
    let locks = BranchLocks::new();
    let main = bname(MAIN);
    let first = locks.acquire(&main).unwrap();
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut joins = Vec::new();
    for i in 1..=6u64 {
        let (mine, main, order) = (locks.clone(), main.clone(), Arc::clone(&order));
        joins.push(std::thread::spawn(move || {
            let t = mine.acquire(&main).unwrap();
            order.lock().unwrap().push(i);
            drop(t);
        }));
        // Thread i has taken its ticket before thread i + 1 starts.
        while locks.queued(MAIN) < i + 1 {
            std::thread::yield_now();
        }
    }
    // Another branch is not held by `main`'s ticket.
    drop(locks.acquire(&bname("side")).unwrap());
    assert!(order.lock().unwrap().is_empty(), "nobody ran while the first ticket was held");
    drop(first);
    for j in joins {
        j.join().unwrap();
    }
    assert_eq!(*order.lock().unwrap(), vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(locks.queued(MAIN), 0);
}

/// Lock order (route module docs): the branch ticket is held from route through the handler
/// and commit, the bus commit lock is not. While a writer of `main` waits inside its handler:
/// a second writer of `main` queues behind it and does not reach its handler, and a writer of
/// `side` commits (it takes the bus commit lock, which nothing holds across a handler). Then
/// the first and second commit in arrival order.
#[test]
fn the_branch_ticket_is_held_through_the_handler_and_the_commit_lock_is_not() {
    let mut h = harness("order");
    let a = channel(&mut h.sessions, ChannelKind::Gui, "gui-a", "alice");
    let b = channel(&mut h.sessions, ChannelKind::Gui, "gui-b", "bob");
    let c = channel(&mut h.sessions, ChannelKind::Gui, "gui-c", "carol");
    h.router.open_branch(&c.session, bname("side")).unwrap();
    let h = &h;
    std::thread::scope(|s| {
        let first = s.spawn(|| entry(h.run(&a, &write("block-1", 1)).unwrap()));
        assert_eq!(h.entered(), "block-1");
        let second = s.spawn(|| entry(h.run(&b, &write("block-2", 1)).unwrap()));
        while h.bus.branch_writers(MAIN) < 2 {
            std::thread::yield_now();
        }
        // Another branch commits while `main` is held inside a handler.
        let side = entry(h.run(&c, &write("c", 1)).unwrap());
        assert_eq!(h.head("side"), Some(side));
        assert_eq!(h.head(MAIN), None);
        assert_eq!(h.calls.load(Ordering::SeqCst), 2, "the second writer of main did not reach its handler");
        h.gate.release();
        let one = first.join().unwrap();
        assert_eq!(h.entered(), "block-2");
        let two = second.join().unwrap();
        assert_eq!(h.head(MAIN), Some(two));
        assert_ne!(one, two);
    });
    let report = h.journal.verify_chain(&VerifyOptions::default()).unwrap();
    assert_eq!((report.failure, report.checked.entries), (None, 3));
}

/// No deadlock between the per-branch tickets and the bus commit lock: 6 writers spread over
/// 3 branches, each alternating, all finish (a watchdog fails the test instead of hanging),
/// and every branch ends with its writers' entries.
#[test]
fn writers_on_several_branches_never_deadlock() {
    const PER: usize = 60;
    let mut h = harness("nodead");
    let branches = ["main", "side/a", "side/b"];
    let chans: Vec<TestChannel> =
        (0..6).map(|i| channel(&mut h.sessions, ChannelKind::Gui, &format!("gui-n{i}"), &format!("user{i}"))).collect();
    for (i, ch) in chans.iter().enumerate() {
        h.router.open_branch(&ch.session, bname(branches[i % 3])).unwrap();
    }
    let h = &h;
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::scope(|s| {
        for (i, ch) in chans.iter().enumerate() {
            let done = done_tx.clone();
            s.spawn(move || {
                for n in 0..PER {
                    let inv = write(&format!("u{i}"), i64::try_from(n + 1).unwrap());
                    if let Err(e) = h.run(ch, &inv) {
                        panic!("writer {i}: {e} ({})", e.detail().as_untrusted_str());
                    }
                }
                done.send(i).unwrap();
            });
        }
        for _ in 0..chans.len() {
            done_rx.recv_timeout(Duration::from_secs(600)).expect("a writer is stuck: deadlock");
        }
    });
    assert_eq!(h.seq(), u64::try_from(6 * PER).unwrap());
    let report = h.journal.verify_chain(&VerifyOptions::default()).unwrap();
    assert_eq!(report.failure, None);
    for b in branches {
        assert!(h.head(b).is_some(), "{b}");
        assert_eq!(h.bus.branch_writers(b), 0);
    }
}
