//! Acceptance criteria of spec node AUTO.REC.LIFE (AC-1 .. AC-7) and its fail-closed rules,
//! against `fixtures/program/valid/minimal.json`, `fixtures/program/agent-steps.json`,
//! `fixtures/script/hello/` and `fixtures/rec/life/expected.json`, written by the independent
//! reference `fixtures/rec/life/make_fixtures.py` (digests from hashlib, expected events and
//! results hand-labelled from the spec). Principals are derived by the bus from channels, the
//! only way to build one.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::principal::{
    Channel, ChannelKind, ConfirmMethod, HumanPresence, Identity, JournalAppend, OsUser, PresenceOutcome, Principal,
    PrincipalId, PrincipalKind, PrincipalRef, SessionId, SessionKey, SessionTable, UpgradeClock, UpgradeContext,
    UpgradeGrant, UpgradeRequest,
};
use fid_bus::BusError;
use fid_canon::{parse, CValue, Digest as CanonDigest};
use fid_journal::{EntryDraft, EntryTime, Manifests};
use fid_program::{Program, Uuid7};
use fid_program_rec::adopt::{
    self, step_contents, AdoptCtx, AdoptEvent, AdoptJournal, Programs, ProvenanceStore, RecordedEntry, ScriptId,
    Target, SCRIPT_ID_PATTERN, UUID7_PATTERN,
};
use fid_program_rec::life::command::{context_types, is_agent_branch};
use fid_program_rec::life::{
    cli_params, declarations, invoke_cli, invoke_mcp, mcp_params, replay, transition_allowed, ArtifactId, CheckVerdict,
    CheckerUnavailable, Drafts, LifeCtx, LifeError, LifeEvent, LifeJournal, LifeRequest, LifeRequests, LifeState,
    LifeStore, McpReply, ProgramChecker, RunGate, ScriptBundle, Verb,
};
use fid_registry::def::Projection;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

// ------------------------------------------------------------------ fixtures

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

struct Expected(BTreeMap<String, CValue>);

impl Expected {
    fn load() -> Expected {
        let CValue::Obj(o) = parse(&read("fixtures/rec/life/expected.json")).unwrap() else { panic!("object") };
        Expected(o)
    }
    fn s(&self, k: &str) -> String {
        match self.0.get(k) {
            Some(CValue::Str(s)) => s.clone(),
            other => panic!("{k}: {other:?}"),
        }
    }
    fn b(&self, k: &str) -> Vec<u8> {
        self.s(k).into_bytes()
    }
    fn step(&self, k: &str) -> Uuid7 {
        let Some(CValue::Obj(o)) = self.0.get("agent_steps") else { panic!("agent_steps") };
        let Some(CValue::Str(s)) = o.get(k) else { panic!("{k}") };
        Uuid7::parse(s).unwrap()
    }
    fn strs(&self, k: &str) -> Vec<String> {
        let Some(CValue::Arr(a)) = self.0.get(k) else { panic!("{k}") };
        a.iter()
            .map(|x| match x {
                CValue::Str(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    }
}

fn minimal() -> Program {
    Program::parse(&read("fixtures/program/valid/minimal.json")).unwrap()
}

/// `minimal.json` as edited in a new draft (written by make_fixtures.py).
fn edited() -> Program {
    Program::parse(&read("fixtures/rec/life/minimal-edited.json")).unwrap()
}

fn agent_draft() -> Program {
    Program::parse(&read("fixtures/program/agent-steps.json")).unwrap()
}

fn hello() -> ScriptBundle {
    let dir = root().join("fixtures/script/hello");
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap())
        })
        .collect();
    files.sort();
    assert_eq!(files.len(), 1, "a one-file bundle");
    ScriptBundle::new(files).unwrap()
}

fn script_id() -> ScriptId {
    ScriptId::new("hello").unwrap()
}

// ------------------------------------------------------------------ seams

#[derive(Default)]
struct Project {
    programs: Vec<Program>,
    scripts: Vec<(ScriptId, ScriptBundle)>,
}

impl Drafts for Project {
    fn program(&self, id: &Uuid7) -> Option<&Program> {
        self.programs.iter().find(|p| p.program_id == *id)
    }
    fn script(&self, id: &ScriptId) -> Option<&ScriptBundle> {
        self.scripts.iter().find(|(s, _)| s == id).map(|(_, b)| b)
    }
}

impl Programs for Project {
    fn program(&self, id: &Uuid7) -> Option<&Program> {
        self.programs.iter().find(|p| p.program_id == *id)
    }
}

/// A journal numbering its entries from 100, so they never coincide with the authorship
/// entries the tests record by hand.
struct Log {
    entries: Vec<(u64, Vec<u8>)>,
    next: u64,
    fail: bool,
}

impl Default for Log {
    fn default() -> Log {
        Log { entries: Vec::new(), next: 100, fail: false }
    }
}

impl Log {
    fn events(&self) -> Vec<Vec<u8>> {
        self.entries.iter().map(|(_, e)| e.clone()).collect()
    }
    fn recorded(&self, kind: PrincipalKind) -> Vec<RecordedEntry> {
        self.entries
            .iter()
            .map(|(n, e)| RecordedEntry { journal_entry: *n, principal_kind: kind, event: e.clone() })
            .collect()
    }
    fn take(&mut self) -> u64 {
        let n = self.next;
        self.next += 1;
        n
    }
}

impl LifeJournal for Log {
    fn commit(&mut self, event: &LifeEvent) -> Result<u64, String> {
        if self.fail {
            return Err("disk full".into());
        }
        let n = self.take();
        self.entries.push((n, event.to_canonical_bytes()));
        Ok(n)
    }
}

impl AdoptJournal for Log {
    fn commit(&mut self, event: &AdoptEvent) -> Result<u64, String> {
        let n = self.take();
        self.entries.push((n, event.to_canonical_bytes()));
        Ok(n)
    }
}

#[derive(Default)]
struct Inbox(Vec<LifeRequest>);

impl LifeRequests for Inbox {
    fn file(&mut self, request: &LifeRequest) -> Result<String, String> {
        self.0.push(request.clone());
        Ok(format!("req-{}", self.0.len()))
    }
}

#[derive(Clone, Copy)]
enum Checker {
    Pass,
    Fail,
    Unavailable,
}

impl ProgramChecker for Checker {
    fn check(&self, program: &Program) -> CheckVerdict {
        match self {
            Checker::Pass => CheckVerdict::Passed,
            Checker::Fail => CheckVerdict::Failed { diagnostics: vec!["unknown command feature.fit_plane@1".into()] },
            Checker::Unavailable => CheckerUnavailable.check(program),
        }
    }
}

/// One branch's lifecycle state with the project and the seams around it.
struct World {
    store: LifeStore,
    prov: ProvenanceStore,
    project: Project,
    log: Log,
    inbox: Inbox,
    checker: Checker,
}

impl World {
    fn new(project: Project) -> World {
        World {
            store: LifeStore::new(),
            prov: ProvenanceStore::new(),
            project,
            log: Log::default(),
            inbox: Inbox::default(),
            checker: Checker::Pass,
        }
    }

    fn cli(&mut self, p: &Principal, branch: &str, args: &[&str]) -> Result<Vec<u8>, LifeError> {
        let mut ctx = LifeCtx {
            principal: p,
            branch,
            store: &mut self.store,
            drafts: &self.project,
            provenance: &mut self.prov,
            checker: &self.checker,
            journal: &mut self.log,
            requests: &mut self.inbox,
        };
        invoke_cli(&mut ctx, args)
    }

    fn mcp(&mut self, p: &Principal, branch: &str, tool: &str, raw: &str) -> Result<McpReply, LifeError> {
        let mut ctx = LifeCtx {
            principal: p,
            branch,
            store: &mut self.store,
            drafts: &self.project,
            provenance: &mut self.prov,
            checker: &self.checker,
            journal: &mut self.log,
            requests: &mut self.inbox,
        };
        invoke_mcp(&mut ctx, tool, raw.as_bytes())
    }

    fn adopt_cli(&mut self, p: &Principal, args: &[&str]) -> Result<Vec<u8>, adopt::AdoptError> {
        let mut ctx = AdoptCtx { principal: p, store: &mut self.prov, programs: &self.project, journal: &mut self.log };
        adopt::invoke_cli(&mut ctx, args)
    }
}

fn args<'a>(kind: &'a str, verb: &'a str, id: &'a str, digest: &'a str) -> Vec<&'a str> {
    let flag = if kind == "program" { "--program" } else { "--script" };
    vec![kind, verb, flag, id, "--expected-digest", digest, "--json"]
}

// ------------------------------------------------------------------ principals

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

fn channel(table: &mut SessionTable, kind: ChannelKind, subject: &str) -> TestChannel {
    let mut identity = Identity::new(PrincipalId::new(subject).unwrap());
    identity.os_user = Some(OsUser::new(subject).unwrap());
    if matches!(kind, ChannelKind::ServerSession | ChannelKind::Internal) {
        identity.acting_for =
            Some(PrincipalRef { kind: PrincipalKind::Human, id: PrincipalId::new("trigger").unwrap() });
    }
    let session = table.open_session(SessionId::new(&format!("{}-{subject}", kind.as_str())).unwrap()).unwrap();
    TestChannel { kind, identity, session, grant: None }
}

fn principal_as(table: &mut SessionTable, kind: PrincipalKind, subject: &str) -> Principal {
    let ch = match kind {
        PrincipalKind::Human => ChannelKind::Gui,
        PrincipalKind::Agent => ChannelKind::Mcp,
        PrincipalKind::Script => ChannelKind::Cli,
        PrincipalKind::Service => ChannelKind::Service,
        PrincipalKind::Device => ChannelKind::Device,
        PrincipalKind::System => ChannelKind::Internal,
    };
    let c = channel(table, ch, subject);
    let p = table.derive_principal(&c).unwrap();
    assert_eq!(p.kind(), kind);
    p
}

struct Confirms(OsUser);

impl HumanPresence for Confirms {
    fn confirm(&self, _req: &UpgradeRequest, _deadline: Duration) -> PresenceOutcome {
        PresenceOutcome::Confirmed { os_user: self.0.clone(), method: ConfirmMethod::OsPresence }
    }
}

struct StepClock(u64);

impl UpgradeClock for StepClock {
    fn now(&mut self) -> EntryTime {
        self.0 += 1_000_000;
        EntryTime { utc: "2026-11-19T10:00:00Z".into(), mono_ns: self.0, sync: "ntp".into() }
    }
}

struct NullJournal;

impl JournalAppend for NullJournal {
    fn append(&mut self, _draft: &EntryDraft) -> Result<(), BusError> {
        Ok(())
    }
}

/// A CLI session of `subject`: `script` until upgraded, then `human` (D-012).
fn cli_session(table: &mut SessionTable, subject: &str, upgrade: bool) -> Principal {
    let mut ch = channel(table, ChannelKind::Cli, subject);
    if upgrade {
        let pending = table.prepare_upgrade(&ch).unwrap();
        let confirmed = pending.confirm(&Confirms(OsUser::new(subject).unwrap()), &mut StepClock(1_000)).unwrap();
        let ctx = UpgradeContext {
            stream: "record".into(),
            branch: "main".into(),
            manifests: Manifests { registry: CanonDigest([0x11; 32]), kernel: CanonDigest([0x22; 32]) },
        };
        ch.grant = Some(table.commit_upgrade(confirmed, &mut NullJournal, &ctx).unwrap());
    }
    table.derive_principal(&ch).unwrap()
}

const MAIN: &str = "main";
const AGENT_BRANCH: &str = "agent/claude-client/1";

fn digest_of(bytes: &[u8]) -> String {
    Digest::from_bytes(fid_canon::digest_bytes(bytes).0).as_str().to_owned()
}

/// A world holding the minimal program, registered and released by `alice` on `main`.
fn released_minimal(table: &mut SessionTable, ex: &Expected) -> (World, Principal) {
    let mut w = World::new(Project { programs: vec![minimal()], scripts: Vec::new() });
    let alice = cli_session(table, "alice", true);
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    w.cli(&alice, MAIN, &args("program", "register", &p, &d)).unwrap();
    w.cli(&alice, MAIN, &args("program", "release", &p, &d)).unwrap();
    (w, alice)
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_register_then_release() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let mut w = World::new(Project { programs: vec![minimal()], scripts: Vec::new() });
    let alice = cli_session(&mut table, "alice", true);
    assert_eq!(alice.kind(), PrincipalKind::Human);
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    let id = ArtifactId::Program(Uuid7::parse(&p).unwrap());

    let out = w.cli(&alice, MAIN, &args("program", "register", &p, &d)).unwrap();
    assert_eq!(out, ex.b("ac1_register_result"));
    let lc = w.store.lifecycle(&id, 1).unwrap();
    assert_eq!((lc.state, lc.version, lc.digest.as_str()), (LifeState::Registered, 1, d.as_str()));
    assert_eq!(w.store.guard_edit(&id).unwrap_err().code(), "immutable");

    let out = w.cli(&alice, MAIN, &args("program", "release", &p, &d)).unwrap();
    assert_eq!(out, ex.b("ac1_release_result"));
    let lc = w.store.lifecycle(&id, 1).unwrap();
    assert_eq!((lc.state, lc.version, lc.source_digest), (LifeState::Released, 1, None));

    // Identity: the SHA-256 of the frozen canonical bytes, which are the fixture's bytes.
    let frozen = w.store.frozen_bytes(&id, 1).unwrap();
    assert_eq!(frozen, read("fixtures/program/valid/minimal.json").as_slice());
    assert_eq!(digest_of(frozen), d);
    assert_eq!(minimal().digest().unwrap().as_str(), d);
    assert_eq!(w.log.events(), vec![ex.b("ac1_register_event"), ex.b("ac1_release_event")]);
    assert_eq!(
        w.store.run_gate(&id, &Digest::parse(&d).unwrap()),
        RunGate::Runnable { version: 1, state: LifeState::Released }
    );
}

// ------------------------------------------------------------------ AC-2 and the transition table

#[test]
fn ac2_released_program_refuses_register_and_edits() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let (mut w, alice) = released_minimal(&mut table, &ex);
    let before = w.store.clone();
    let n = w.log.entries.len();
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    let id = ArtifactId::Program(Uuid7::parse(&p).unwrap());

    let e = w.cli(&alice, MAIN, &args("program", "register", &p, &d)).unwrap_err();
    assert_eq!(
        e,
        LifeError::IllegalTransition { artifact: p.clone(), from: LifeState::Released, to: LifeState::Registered }
    );
    // The edit guard program.edit_insert_step@1 (AUTO.PROG.EDIT.OPS) must call.
    assert_eq!(w.store.guard_edit(&id).unwrap_err(), LifeError::Immutable { artifact: p.clone() });
    // Every other transition from released is refused too.
    for verb in ["unregister", "release"] {
        let e = w.cli(&alice, MAIN, &args("program", verb, &p, &d)).unwrap_err();
        assert_eq!(e.code(), "illegal_transition", "{verb}");
    }
    assert_eq!(w.store, before);
    assert_eq!(w.log.entries.len(), n);
}

/// REQ-001, hand-labelled: exactly four of the sixteen pairs are allowed.
#[test]
fn transition_table_is_exactly_the_four_edges() {
    use LifeState::{Deprecated as X, Draft as D, Registered as G, Released as R};
    let allowed = [(D, G), (G, D), (G, R), (R, X)];
    for from in [D, G, R, X] {
        for to in [D, G, R, X] {
            assert_eq!(transition_allowed(from, to), allowed.contains(&(from, to)), "{from:?} -> {to:?}");
        }
    }
}

#[test]
fn illegal_transitions_from_draft_and_registered() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let mut w = World::new(Project { programs: vec![minimal()], scripts: Vec::new() });
    let alice = cli_session(&mut table, "alice", true);
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    // Draft: only register.
    for verb in ["unregister", "release", "deprecate"] {
        let e = w.cli(&alice, MAIN, &args("program", verb, &p, &d)).unwrap_err();
        assert_eq!(e.code(), "illegal_transition", "{verb}");
    }
    // Registered: not register again, not deprecate.
    w.cli(&alice, MAIN, &args("program", "register", &p, &d)).unwrap();
    for verb in ["register", "deprecate"] {
        let e = w.cli(&alice, MAIN, &args("program", verb, &p, &d)).unwrap_err();
        assert_eq!(e.code(), "illegal_transition", "{verb}");
    }
    assert_eq!(w.log.entries.len(), 1);
    // Unknown artifact.
    let stranger = "019a0000-0000-7000-8000-00000000ffff";
    let e = w.cli(&alice, MAIN, &args("program", "register", stranger, &d)).unwrap_err();
    assert_eq!(e.code(), "artifact_not_found");
    let e = w.cli(&alice, MAIN, &args("program", "release", stranger, &d)).unwrap_err();
    assert_eq!(e.code(), "artifact_not_found");
}

/// registered → draft retires the version number; editing reopens; a new registration gets
/// the next number. A new draft copied from a released version records its source digest.
#[test]
fn unregister_and_new_drafts() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let mut w = World::new(Project { programs: vec![minimal()], scripts: Vec::new() });
    let alice = cli_session(&mut table, "alice", true);
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    let id = ArtifactId::Program(Uuid7::parse(&p).unwrap());
    let dg = Digest::parse(&d).unwrap();

    w.cli(&alice, MAIN, &args("program", "register", &p, &d)).unwrap();
    let out = w.cli(&alice, MAIN, &args("program", "unregister", &p, &d)).unwrap();
    assert_eq!(out, format!(r#"{{"digest":"{d}","state":"draft","version":1}}"#).into_bytes());
    assert!(w.store.guard_edit(&id).is_ok());
    assert_eq!(w.store.run_gate(&id, &dg), RunGate::NotIdentified);
    let out = w.cli(&alice, MAIN, &args("program", "register", &p, &d)).unwrap();
    assert_eq!(out, format!(r#"{{"digest":"{d}","state":"registered","version":2}}"#).into_bytes());
    assert_eq!(w.store.lifecycle(&id, 1).unwrap().state, LifeState::Draft);
    w.cli(&alice, MAIN, &args("program", "release", &p, &d)).unwrap();

    // open_draft: a new draft that copies the released bytes and records the source; the
    // released version is untouched.
    let out = w.cli(&alice, MAIN, &args("program", "open-draft", &p, &d)).unwrap();
    assert_eq!(out, format!(r#"{{"digest":"{d}","state":"draft","version":2}}"#).into_bytes());
    assert_eq!(w.store.frozen_bytes(&id, 2).unwrap(), read("fixtures/program/valid/minimal.json").as_slice());
    assert!(w.store.guard_edit(&id).is_ok());
    assert_eq!(w.store.draft_source(&id), Some(dg));
    assert_eq!(w.cli(&alice, MAIN, &args("program", "open-draft", &p, &d)).unwrap_err().code(), "conflict");
    // Registering the copy unchanged is a conflict: version 2 already has this content.
    assert_eq!(w.cli(&alice, MAIN, &args("program", "register", &p, &d)).unwrap_err().code(), "conflict");
    // The edit path writes changed content into the draft; that registers as version 3.
    w.project.programs = vec![edited()];
    let e = ex.s("edited_digest");
    let out = w.cli(&alice, MAIN, &args("program", "register", &p, &e)).unwrap();
    assert_eq!(out, format!(r#"{{"digest":"{e}","state":"registered","version":3}}"#).into_bytes());
    assert_eq!(w.store.lifecycle(&id, 3).unwrap().source_digest, Some(dg));
    assert_eq!(w.store.lifecycle(&id, 2).unwrap().state, LifeState::Released);
    let events = w.log.events();
    assert_eq!(events.len(), 6);
    let open = LifeEvent::from_canonical_bytes(&events[4]).unwrap();
    assert_eq!(
        (open.verb, open.version, open.source_digest, open.state),
        (Verb::OpenDraft, 2, Some(dg), LifeState::Draft)
    );
    let last = LifeEvent::from_canonical_bytes(&events[5]).unwrap();
    assert_eq!((last.verb, last.version, last.source_digest), (Verb::Register, 3, Some(dg)));
    assert_eq!(last.content.unwrap().into_bytes(), read("fixtures/rec/life/minimal-edited.json"));
}

/// Review of #309: deprecate v1, open a draft from it, register the same bytes. That must not
/// give a runnable copy of a deprecated version, for a person or for an agent.
#[test]
fn deprecation_cannot_be_undone_by_reregistering_the_same_bytes() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let (mut w, alice) = released_minimal(&mut table, &ex);
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    let id = ArtifactId::Program(Uuid7::parse(&p).unwrap());
    let dg = Digest::parse(&d).unwrap();
    w.cli(&alice, MAIN, &args("program", "deprecate", &p, &d)).unwrap();

    // A person.
    w.cli(&alice, MAIN, &args("program", "open-draft", &p, &d)).unwrap();
    let e = w.cli(&alice, MAIN, &args("program", "register", &p, &d)).unwrap_err();
    assert_eq!(e, LifeError::Conflict { detail: "version 1 already has this content and is deprecated".into() });
    assert_eq!(w.store.run_gate(&id, &dg), RunGate::Deprecated { version: 1 });
    assert_eq!(w.store.versions(&id).len(), 1);

    // An agent on its branch (same state, as if promoted from there).
    let mut a = World::new(Project { programs: vec![minimal()], scripts: Vec::new() });
    a.store = w.store.clone();
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let raw = format!(r#"{{"expected_digest":"{d}","program":"{p}"}}"#);
    let e = a.mcp(&agent, AGENT_BRANCH, "program_register", &raw).unwrap_err();
    assert_eq!(e.code(), "conflict");
    assert_eq!(a.store.run_gate(&id, &dg), RunGate::Deprecated { version: 1 });
    assert_eq!(a.store.versions(&id).len(), 1);
    // Opening a draft is itself journaled and follows the branch rule.
    assert_eq!(
        a.mcp(&agent, MAIN, "program_open_draft", &raw).unwrap_err(),
        LifeError::PolicyRefused { reason: "not_agent_branch" }
    );
}

#[test]
fn conflict_on_a_wrong_expected_digest() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let mut w = World::new(Project { programs: vec![minimal()], scripts: Vec::new() });
    let alice = cli_session(&mut table, "alice", true);
    let p = ex.s("program");
    let wrong = ex.s("agent_digest");
    assert_eq!(w.cli(&alice, MAIN, &args("program", "register", &p, &wrong)).unwrap_err().code(), "conflict");
    w.cli(&alice, MAIN, &args("program", "register", &p, &ex.s("minimal_digest"))).unwrap();
    assert_eq!(w.cli(&alice, MAIN, &args("program", "release", &p, &wrong)).unwrap_err().code(), "conflict");
    assert_eq!(w.log.entries.len(), 1);
}

// ------------------------------------------------------------------ AC-3 and the caller rules

#[test]
fn ac3_mcp_release_files_a_request_and_changes_nothing() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let mut w = World::new(Project { programs: vec![minimal()], scripts: Vec::new() });
    let alice = cli_session(&mut table, "alice", true);
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    let id = ArtifactId::Program(Uuid7::parse(&p).unwrap());
    w.cli(&alice, MAIN, &args("program", "register", &p, &d)).unwrap();
    let before = w.store.clone();

    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let raw = format!(r#"{{"expected_digest":"{d}","program":"{p}"}}"#);
    let reply = w.mcp(&agent, AGENT_BRANCH, "program_release", &raw).unwrap();
    assert_eq!(reply, McpReply::RequestFiled { request_id: "req-1".into() });
    assert_eq!(reply.outcome(), "request_filed");
    assert_eq!(w.store.lifecycle(&id, 1).unwrap().state, LifeState::Registered);
    assert_eq!(w.store, before);
    assert_eq!(w.log.entries.len(), 1);
    let req = &w.inbox.0[0];
    assert_eq!(req.command, "program.release@1");
    assert_eq!(
        req.requester,
        PrincipalRef { kind: PrincipalKind::Agent, id: PrincipalId::new("claude-client").unwrap() }
    );
    assert_eq!(req.params, raw.into_bytes());

    // Deprecate is request-only too, and so is a person's MCP call.
    let raw = format!(r#"{{"expected_digest":"{d}","program":"{p}"}}"#);
    let human = principal_as(&mut table, PrincipalKind::Human, "bob");
    assert_eq!(w.mcp(&human, MAIN, "program_release", &raw).unwrap().outcome(), "request_filed");
    assert_eq!(w.mcp(&agent, AGENT_BRANCH, "program_deprecate", &raw).unwrap().outcome(), "request_filed");
    assert_eq!(w.store, before);
    assert_eq!(w.inbox.0.len(), 3);
}

/// Only a person performs release and deprecate; every other principal kind is refused, on
/// every branch, and nothing is journaled.
#[test]
fn every_non_human_kind_is_refused_w3() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let mut w = World::new(Project { programs: vec![minimal()], scripts: Vec::new() });
    let alice = cli_session(&mut table, "alice", true);
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    w.cli(&alice, MAIN, &args("program", "register", &p, &d)).unwrap();
    let before = w.store.clone();
    let mut kinds: Vec<Principal> = PrincipalKind::ALL
        .iter()
        .filter(|k| **k != PrincipalKind::Human)
        .map(|k| principal_as(&mut table, *k, &format!("p-{}", k.as_str())))
        .collect();
    kinds.push(cli_session(&mut table, "carol", false));
    for who in &kinds {
        for branch in [MAIN, AGENT_BRANCH] {
            for verb in ["release", "deprecate"] {
                let e = w.cli(who, branch, &args("program", verb, &p, &d)).unwrap_err();
                assert_eq!(e.code(), "policy_refused", "{:?} {verb} on {branch}", who.kind());
            }
        }
    }
    assert_eq!(w.store, before);
    assert_eq!(w.log.entries.len(), 1);
}

/// Agents work only on agent branches; a person never writes lifecycle state on one.
#[test]
fn branch_rule_for_register() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let mut w = World::new(Project { programs: vec![minimal()], scripts: Vec::new() });
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let alice = cli_session(&mut table, "alice", true);
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    let raw = format!(r#"{{"expected_digest":"{d}","program":"{p}"}}"#);

    let e = w.mcp(&agent, MAIN, "program_register", &raw).unwrap_err();
    assert_eq!(e, LifeError::PolicyRefused { reason: "not_agent_branch" });
    let e = w.cli(&alice, AGENT_BRANCH, &args("program", "register", &p, &d)).unwrap_err();
    assert_eq!(e, LifeError::PolicyRefused { reason: "agent_branch" });
    assert!(w.log.entries.is_empty());

    let reply = w.mcp(&agent, AGENT_BRANCH, "program_register", &raw).unwrap();
    assert_eq!(reply, McpReply::Committed(ex.b("ac1_register_result")));
    let e = LifeEvent::from_canonical_bytes(&w.log.events()[0]).unwrap();
    assert_eq!((e.branch.as_str(), e.by.as_str()), (AGENT_BRANCH, "claude-client"));
    // A person may not release on the agent branch either.
    let e = w.cli(&alice, AGENT_BRANCH, &args("program", "release", &p, &d)).unwrap_err();
    assert_eq!(e, LifeError::PolicyRefused { reason: "agent_branch" });

    for (b, ok) in [
        ("agent/claude-client/1", true),
        ("agent/a/12", true),
        ("agent/Claude/1", false),
        ("agent/x/0", false),
        ("agent/x/01", false),
        ("agent/x", false),
        ("agent/x/1/y", false),
        ("main", false),
        ("agents/x/1", false),
    ] {
        assert_eq!(is_agent_branch(b), ok, "{b}");
    }
}

/// A spoofed principal kind or branch in the parameters is refused as an unknown parameter.
#[test]
fn spoofed_kind_or_branch_in_params_is_bad_params() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let mut w = World::new(Project { programs: vec![minimal()], scripts: Vec::new() });
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    for extra in [r#""kind":"human","#, r#""principal":"alice","#, r#""branch":"main","#] {
        let raw = format!(r#"{{{extra}"expected_digest":"{d}","program":"{p}"}}"#);
        for tool in ["program_register", "program_release"] {
            assert_eq!(w.mcp(&agent, AGENT_BRANCH, tool, &raw).unwrap_err().code(), "bad_params", "{tool} {extra}");
        }
    }
    let dup = format!(r#"{{"expected_digest":"{d}","program":"{p}","program":"{p}"}}"#);
    assert_eq!(w.mcp(&agent, AGENT_BRANCH, "program_register", &dup).unwrap_err().code(), "bad_params");
    assert_eq!(w.mcp(&agent, AGENT_BRANCH, "program_publish", "{}").unwrap_err().code(), "bad_params");
    assert!(w.log.entries.is_empty());
    assert!(w.inbox.0.is_empty());
}

/// Review of #309: a request-only call is validated fully before a request is filed.
#[test]
fn mcp_w3_params_are_validated_before_filing() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let mut w = World::new(Project { programs: vec![minimal()], scripts: Vec::new() });
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    for raw in [
        format!(r#"{{"expected_digest":"sha256:XYZ","program":"{p}"}}"#),
        format!(r#"{{"expected_digest":"{d}","program":"not-a-uuid"}}"#),
        format!(r#"{{"expected_digest":"{d}","note":"","program":"{p}"}}"#),
        format!(r#"{{"expected_digest":"{d}","note":"a\u0007b","program":"{p}"}}"#),
    ] {
        for tool in ["program_release", "program_deprecate"] {
            assert_eq!(w.mcp(&agent, AGENT_BRANCH, tool, &raw).unwrap_err().code(), "bad_params", "{tool} {raw}");
        }
    }
    let raw = r#"{"expected_digest":"sha256:XYZ","script":"hello"}"#;
    assert_eq!(w.mcp(&agent, AGENT_BRANCH, "script_release", raw).unwrap_err().code(), "bad_params");
    assert!(w.inbox.0.is_empty());
    assert!(w.log.entries.is_empty());
}

#[test]
fn journal_failure_changes_nothing() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let mut w = World::new(Project { programs: vec![minimal()], scripts: vec![(script_id(), hello())] });
    let alice = cli_session(&mut table, "alice", true);
    w.log.fail = true;
    let e = w.cli(&alice, MAIN, &args("program", "register", &ex.s("program"), &ex.s("minimal_digest"))).unwrap_err();
    assert_eq!(e.code(), "journal");
    let e = w.cli(&alice, MAIN, &args("script", "register", "hello", &ex.s("script_digest"))).unwrap_err();
    assert_eq!(e.code(), "journal");
    assert_eq!(w.store, LifeStore::new());
    assert_eq!(w.prov, ProvenanceStore::new());
}

// ------------------------------------------------------------------ AC-4 and the check gate

fn agent_world(table: &mut SessionTable, ex: &Expected) -> (World, Principal) {
    let p = agent_draft();
    let mut w = World::new(Project { programs: vec![p.clone()], scripts: Vec::new() });
    let human = principal_as(table, PrincipalKind::Human, "bob");
    let agent = principal_as(table, PrincipalKind::Agent, "claude-client");
    let contents = step_contents(&p).unwrap();
    let mut author = |s: Uuid7, who: &Principal, entry: u64| {
        let t = Target::Step { program: p.program_id, step: s };
        w.prov.record_authorship(t, *contents.get(&s).unwrap(), who, entry);
    };
    author(ex.step("H1"), &human, 1);
    author(ex.step("A1"), &agent, 2);
    author(ex.step("A2"), &agent, 3);
    let alice = cli_session(table, "alice", true);
    let (id, d) = (ex.s("agent_program"), ex.s("agent_digest"));
    w.cli(&alice, MAIN, &args("program", "register", &id, &d)).unwrap();
    (w, alice)
}

#[test]
fn ac4_unadopted_agent_steps_block_release() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let (mut w, alice) = agent_world(&mut table, &ex);
    let before = w.store.clone();
    let (id, d) = (ex.s("agent_program"), ex.s("agent_digest"));
    let e = w.cli(&alice, MAIN, &args("program", "release", &id, &d)).unwrap_err();
    assert_eq!(e, LifeError::UnadoptedSteps { steps: ex.strs("ac4_unadopted") });
    assert_eq!(w.store, before);
    assert_eq!(w.log.entries.len(), 1);

    // After a person adopts both steps (AUTO.REC.ADOPT), the release goes through.
    let steps = format!("{},{}", ex.step("A1"), ex.step("A2"));
    let adopt = [
        "program",
        "adopt-steps",
        "--program",
        id.as_str(),
        "--steps",
        steps.as_str(),
        "--reviewed-digest",
        d.as_str(),
    ];
    w.adopt_cli(&alice, &adopt).unwrap();
    let out = w.cli(&alice, MAIN, &args("program", "release", &id, &d)).unwrap();
    assert_eq!(out, format!(r#"{{"digest":"{d}","state":"released","version":1}}"#).into_bytes());
}

#[test]
fn check_failed_comes_first_and_fails_closed_without_a_checker() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let (mut w, alice) = agent_world(&mut table, &ex);
    let (id, d) = (ex.s("agent_program"), ex.s("agent_digest"));
    w.checker = Checker::Fail;
    let e = w.cli(&alice, MAIN, &args("program", "release", &id, &d)).unwrap_err();
    assert_eq!(e, LifeError::CheckFailed { diagnostics: vec!["unknown command feature.fit_plane@1".into()] });
    w.checker = Checker::Unavailable;
    let e = w.cli(&alice, MAIN, &args("program", "release", &id, &d)).unwrap_err();
    assert_eq!(e, LifeError::CheckFailed { diagnostics: vec!["program.check@1 is not available".into()] });
    assert_eq!(w.log.entries.len(), 1);
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_deprecation_keeps_bytes_and_identity() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let (mut w, alice) = released_minimal(&mut table, &ex);
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    let id = ArtifactId::Program(Uuid7::parse(&p).unwrap());
    let bytes_before = w.store.frozen_bytes(&id, 1).unwrap().to_vec();

    let out = w.cli(&alice, MAIN, &args("program", "deprecate", &p, &d)).unwrap();
    assert_eq!(out, ex.b("ac5_deprecate_result"));
    assert_eq!(w.store.frozen_bytes(&id, 1).unwrap(), bytes_before.as_slice());
    assert_eq!(bytes_before, read("fixtures/program/valid/minimal.json"));
    let dg = Digest::parse(&d).unwrap();
    assert_eq!(w.store.run_gate(&id, &dg), RunGate::Deprecated { version: 1 });
    let lc = w.store.by_digest(&id, &dg).unwrap();
    assert_eq!((lc.state, lc.version, lc.digest), (LifeState::Deprecated, 1, dg));
    // The earlier journal entries are untouched, and deprecated is final.
    assert_eq!(w.log.events()[..2], [ex.b("ac1_register_event"), ex.b("ac1_release_event")]);
    for verb in ["register", "unregister", "release", "deprecate"] {
        assert_eq!(w.cli(&alice, MAIN, &args("program", verb, &p, &d)).unwrap_err().code(), "illegal_transition");
    }
    assert_eq!(w.store.guard_edit(&id).unwrap_err().code(), "immutable");
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_script_commands_behave_identically() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let bundle = hello();
    assert_eq!(bundle.digest().unwrap().as_str(), ex.s("script_digest"));
    let mut w = World::new(Project { programs: Vec::new(), scripts: vec![(script_id(), bundle.clone())] });
    let alice = cli_session(&mut table, "alice", true);
    let d = ex.s("script_digest");
    let id = ArtifactId::Script(script_id());

    let out = w.cli(&alice, MAIN, &args("script", "register", "hello", &d)).unwrap();
    assert_eq!(out, ex.b("ac6_register_result"));
    // Registration records the person as the version's author (AUTO.REC.ADOPT).
    assert!(!w.prov.unadopted_script(&script_id(), &Digest::parse(&d).unwrap()));
    let out = w.cli(&alice, MAIN, &args("script", "release", "hello", &d)).unwrap();
    assert_eq!(out, ex.b("ac6_release_result"));
    assert_eq!(w.log.events(), vec![ex.b("ac6_register_event"), ex.b("ac6_release_event")]);
    let frozen = w.store.frozen_bytes(&id, 1).unwrap();
    assert_eq!(digest_of(frozen), d);
    assert_eq!(ScriptBundle::parse(frozen).unwrap(), bundle);

    // Same refusals as programs.
    let e = w.cli(&alice, MAIN, &args("script", "register", "hello", &d)).unwrap_err();
    assert_eq!(e.code(), "illegal_transition");
    assert_eq!(w.store.guard_edit(&id).unwrap_err().code(), "immutable");
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let raw = format!(r#"{{"expected_digest":"{d}","script":"hello"}}"#);
    assert_eq!(w.mcp(&agent, AGENT_BRANCH, "script_deprecate", &raw).unwrap().outcome(), "request_filed");
    assert_eq!(w.store.lifecycle(&id, 1).unwrap().state, LifeState::Released);
    let out = w.cli(&alice, MAIN, &args("script", "deprecate", "hello", &d)).unwrap();
    assert_eq!(out, ex.b("ac6_deprecate_result"));
    assert_eq!(w.store.run_gate(&id, &Digest::parse(&d).unwrap()), RunGate::Deprecated { version: 1 });
}

/// A script version an agent registered on its branch cannot be released until a person
/// adopts it as a whole.
#[test]
fn agent_registered_script_needs_adoption() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let mut w = World::new(Project { programs: Vec::new(), scripts: vec![(script_id(), hello())] });
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let d = ex.s("script_digest");
    let raw = format!(r#"{{"expected_digest":"{d}","script":"hello"}}"#);
    assert_eq!(w.mcp(&agent, AGENT_BRANCH, "script_register", &raw).unwrap().outcome(), "committed");
    assert!(w.prov.unadopted_script(&script_id(), &Digest::parse(&d).unwrap()));

    // The branch is promoted (CORE.JRN.BR.PROMO, not modelled here): the same state on main.
    let alice = cli_session(&mut table, "alice", true);
    let e = w.cli(&alice, MAIN, &args("script", "release", "hello", &d)).unwrap_err();
    assert_eq!(e, LifeError::UnadoptedVersion { version: d.clone() });
    let adopt = ["script", "adopt-version", "--script", "hello", "--version-digest", d.as_str()];
    w.adopt_cli(&alice, &adopt).unwrap();
    let out = w.cli(&alice, MAIN, &args("script", "release", "hello", &d)).unwrap();
    assert_eq!(out, ex.b("ac6_release_result"));
}

/// Review of #309: registering is not authoring. A person who re-registers content an agent
/// registered before does not become its author; it still needs a person's adoption.
#[test]
fn person_reregistering_agent_content_still_needs_adoption() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let mut w = World::new(Project { programs: Vec::new(), scripts: vec![(script_id(), hello())] });
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let d = ex.s("script_digest");
    let raw = format!(r#"{{"expected_digest":"{d}","script":"hello"}}"#);
    w.mcp(&agent, AGENT_BRANCH, "script_register", &raw).unwrap();
    w.mcp(&agent, AGENT_BRANCH, "script_unregister", &raw).unwrap();

    let alice = cli_session(&mut table, "alice", true);
    let out = w.cli(&alice, MAIN, &args("script", "register", "hello", &d)).unwrap();
    assert_eq!(out, format!(r#"{{"digest":"{d}","state":"registered","version":2}}"#).into_bytes());
    assert!(w.prov.unadopted_script(&script_id(), &Digest::parse(&d).unwrap()));
    let e = w.cli(&alice, MAIN, &args("script", "release", "hello", &d)).unwrap_err();
    assert_eq!(e, LifeError::UnadoptedVersion { version: d.clone() });
    let adopt = ["script", "adopt-version", "--script", "hello", "--version-digest", d.as_str()];
    w.adopt_cli(&alice, &adopt).unwrap();
    let out = w.cli(&alice, MAIN, &args("script", "release", "hello", &d)).unwrap();
    assert_eq!(out, format!(r#"{{"digest":"{d}","state":"released","version":2}}"#).into_bytes());
}

/// Review of #309: LifeJournal and AdoptJournal are two handles to one journal with one entry
/// sequence. With one sequence, an adoption made after an agent's registration orders after
/// it; with two independent counters it would not, and the release would stay refused.
#[test]
fn lifecycle_and_adoption_share_one_journal_sequence() {
    let ex = Expected::load();
    let d = ex.s("script_digest");
    let project = Project { programs: Vec::new(), scripts: vec![(script_id(), hello())] };
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let alice = cli_session(&mut table, "alice", true);
    let raw = format!(r#"{{"expected_digest":"{d}","script":"hello"}}"#);
    let release = args("script", "release", "hello", &d);
    let adopt = ["script", "adopt-version", "--script", "hello", "--version-digest", d.as_str()];

    let run = |shared: bool| -> (Result<Vec<u8>, LifeError>, Vec<(u64, String)>) {
        let one = Rc::new(RefCell::new(Seq::default()));
        let other = if shared { Rc::clone(&one) } else { Rc::new(RefCell::new(Seq::default())) };
        let mut life = LifeHandle(Rc::clone(&one));
        let mut adoption = AdoptHandle(other);
        let (mut store, mut prov, mut inbox) = (LifeStore::new(), ProvenanceStore::new(), Inbox::default());
        {
            let mut c = LifeCtx {
                principal: &agent,
                branch: AGENT_BRANCH,
                store: &mut store,
                drafts: &project,
                provenance: &mut prov,
                checker: &Checker::Pass,
                journal: &mut life,
                requests: &mut inbox,
            };
            invoke_mcp(&mut c, "script_register", raw.as_bytes()).unwrap();
        }
        {
            let mut c = AdoptCtx { principal: &alice, store: &mut prov, programs: &project, journal: &mut adoption };
            adopt::invoke_cli(&mut c, &adopt).unwrap();
        }
        let out = {
            let mut c = LifeCtx {
                principal: &alice,
                branch: MAIN,
                store: &mut store,
                drafts: &project,
                provenance: &mut prov,
                checker: &Checker::Pass,
                journal: &mut life,
                requests: &mut inbox,
            };
            invoke_cli(&mut c, &release)
        };
        let entries = one.borrow().entries.clone();
        (out, entries)
    };

    let (out, entries) = run(true);
    assert_eq!(out.unwrap(), ex.b("ac6_release_result"));
    let want: Vec<(u64, String)> =
        vec![(1, "script.register@1".into()), (2, "script.adopt_version@1".into()), (3, "script.release@1".into())];
    assert_eq!(entries, want);

    let (out, _) = run(false);
    assert_eq!(out.unwrap_err().code(), "unadopted_version");
}

/// One journal with one entry sequence.
#[derive(Default)]
struct Seq {
    next: u64,
    entries: Vec<(u64, String)>,
}

impl Seq {
    fn append(&mut self, command: &str) -> u64 {
        self.next += 1;
        self.entries.push((self.next, command.to_owned()));
        self.next
    }
}

struct LifeHandle(Rc<RefCell<Seq>>);
struct AdoptHandle(Rc<RefCell<Seq>>);

impl LifeJournal for LifeHandle {
    fn commit(&mut self, event: &LifeEvent) -> Result<u64, String> {
        Ok(self.0.borrow_mut().append(event.command()))
    }
}

impl AdoptJournal for AdoptHandle {
    fn commit(&mut self, event: &AdoptEvent) -> Result<u64, String> {
        Ok(self.0.borrow_mut().append(event.command()))
    }
}

#[test]
fn script_bundle_rules() {
    let f = |p: &str| vec![(p.to_string(), b"x\n".to_vec())];
    for bad in ["", "/a.py", "a//b.py", "./a.py", "../a.py", "a/../b.py", "a b.py", "a\\b.py", "\u{e9}.py"] {
        assert_eq!(ScriptBundle::new(f(bad)).unwrap_err().code(), "bad_params", "{bad:?}");
    }
    assert!(ScriptBundle::new(f("lib/util-1.py")).is_ok());
    let twice = vec![("Main.py".to_string(), b"a".to_vec()), ("main.py".to_string(), b"b".to_vec())];
    assert_eq!(ScriptBundle::new(twice).unwrap_err().code(), "bad_params");
    assert_eq!(ScriptBundle::new(vec![("a.py".into(), vec![0xff])]).unwrap_err().code(), "bad_params");
    assert_eq!(ScriptBundle::new(Vec::new()).unwrap_err().code(), "bad_params");
    // Round trip, and only the canonical form parses.
    let b = hello();
    let bytes = b.to_canonical_bytes().unwrap();
    assert_eq!(ScriptBundle::parse(&bytes).unwrap(), b);
    let mut spaced = bytes.clone();
    spaced.insert(1, b' ');
    assert!(ScriptBundle::parse(&spaced).is_err());
}

// ------------------------------------------------------------------ AC-7

#[test]
fn ac7_replay_identical() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let (w, _) = released_minimal(&mut table, &ex);
    let start = LifeStore::new();
    let r = replay(&start, &w.prov, &Checker::Pass, &w.log.recorded(PrincipalKind::Human));
    assert_eq!(r.outcome, ReplayOutcome::Identical);
    assert_eq!(r.steps, vec![ReplayOutcome::Identical, ReplayOutcome::Identical]);
    assert!(r.lifecycle.matches(&w.store));
    let id = ArtifactId::Program(Uuid7::parse(&ex.s("program")).unwrap());
    assert_eq!(r.lifecycle.lifecycle(&id, 1).unwrap().state, LifeState::Released);

    // The release entry under any non-human envelope does not replay.
    for kind in PrincipalKind::ALL.iter().filter(|k| **k != PrincipalKind::Human) {
        let mut entries = w.log.recorded(PrincipalKind::Human);
        entries[1].principal_kind = *kind;
        let r = replay(&start, &w.prov, &Checker::Pass, &entries);
        assert_eq!(r.outcome, ReplayOutcome::Divergent, "{kind:?}");
    }
    // A failing check now: divergent. A forged state: not replayable. Garbage: not replayable.
    let r = replay(&start, &w.prov, &Checker::Fail, &w.log.recorded(PrincipalKind::Human));
    assert_eq!(r.steps, vec![ReplayOutcome::Identical, ReplayOutcome::Divergent]);
    let forged = ex.s("ac1_release_event").replace("\"released\"", "\"deprecated\"");
    let entry = |event: Vec<u8>| RecordedEntry { journal_entry: 100, principal_kind: PrincipalKind::Human, event };
    let r = replay(&start, &w.prov, &Checker::Pass, &[entry(forged.into_bytes())]);
    assert_eq!(r.outcome, ReplayOutcome::NotReplayable);
    let r = replay(&start, &w.prov, &Checker::Pass, &[entry(b"{\"command\":\"x\"}".to_vec())]);
    assert_eq!(r.outcome, ReplayOutcome::NotReplayable);
    // Release replayed without its register: divergent (draft cannot be released).
    let r = replay(&start, &w.prov, &Checker::Pass, &[entry(ex.b("ac1_release_event"))]);
    assert_eq!(r.outcome, ReplayOutcome::Divergent);
    assert!(r.lifecycle.matches(&start));
}

/// The JSON string value that follows `key` in `s`, quotes included.
fn json_string_after(s: &str, key: &str) -> String {
    let start = s.find(key).unwrap() + key.len();
    let b = s.as_bytes();
    assert_eq!(b[start], b'"');
    let mut i = start + 1;
    while b[i] != b'"' {
        i += if b[i] == b'\\' { 2 } else { 1 };
    }
    s[start..=i].to_string()
}

/// Review of #309: frozen bytes are durable. A multi-version lineage (register, release,
/// open a draft, register the edit, release it, deprecate the first) replays as IDENTICAL from
/// the journal alone, with every version's bytes, after the project's drafts are gone.
#[test]
fn multi_version_lineage_replays_from_the_journal_alone() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let (mut w, alice) = released_minimal(&mut table, &ex);
    let (p, d, e) = (ex.s("program"), ex.s("minimal_digest"), ex.s("edited_digest"));
    let id = ArtifactId::Program(Uuid7::parse(&p).unwrap());
    w.cli(&alice, MAIN, &args("program", "open-draft", &p, &d)).unwrap();
    w.project.programs = vec![edited()];
    w.cli(&alice, MAIN, &args("program", "register", &p, &e)).unwrap();
    w.cli(&alice, MAIN, &args("program", "release", &p, &e)).unwrap();
    w.cli(&alice, MAIN, &args("program", "deprecate", &p, &d)).unwrap();
    // A script lineage in the same journal, with an unregister in it.
    w.project.scripts = vec![(script_id(), hello())];
    let sd = ex.s("script_digest");
    w.cli(&alice, MAIN, &args("script", "register", "hello", &sd)).unwrap();
    w.cli(&alice, MAIN, &args("script", "unregister", "hello", &sd)).unwrap();
    w.cli(&alice, MAIN, &args("script", "register", "hello", &sd)).unwrap();
    w.project = Project::default();

    let r = replay(&LifeStore::new(), &w.prov, &Checker::Pass, &w.log.recorded(PrincipalKind::Human));
    assert_eq!(r.outcome, ReplayOutcome::Identical);
    assert_eq!(r.steps.len(), 9);
    assert!(r.lifecycle.matches(&w.store));
    assert_eq!(r.lifecycle.frozen_bytes(&id, 1).unwrap(), read("fixtures/program/valid/minimal.json").as_slice());
    assert_eq!(r.lifecycle.frozen_bytes(&id, 2).unwrap(), read("fixtures/rec/life/minimal-edited.json").as_slice());
    assert_eq!(r.lifecycle.run_gate(&id, &Digest::parse(&d).unwrap()), RunGate::Deprecated { version: 1 });
    let sid = ArtifactId::Script(script_id());
    assert_eq!(r.lifecycle.lifecycle(&sid, 2).unwrap().state, LifeState::Registered);

    // Tampered content (bytes that do not hash to the recorded digest) does not replay.
    let mut entries = w.log.recorded(PrincipalKind::Human);
    let tampered =
        String::from_utf8(entries[0].event.clone()).unwrap().replace("\\\"meta\\\":{}", "\\\"meta\\\":{\\\"x\\\":1}");
    assert_ne!(tampered.as_bytes(), entries[0].event.as_slice());
    entries[0].event = tampered.into_bytes();
    let r = replay(&LifeStore::new(), &w.prov, &Checker::Pass, &entries);
    assert_eq!(r.outcome, ReplayOutcome::Divergent);
}

#[test]
fn event_reader_is_strict() {
    let ex = Expected::load();
    for k in ["ac1_register_event", "ac1_release_event", "ac6_register_event", "ac6_release_event"] {
        let e = LifeEvent::from_canonical_bytes(&ex.b(k)).unwrap();
        assert_eq!(e.to_canonical_bytes(), ex.b(k), "{k}");
    }
    let base = ex.s("ac1_register_event");
    for bad in [
        base.replace("{\"artifact\"", "{\"admin\":true,\"artifact\""),
        base.replace("\"version\":1", "\"version\":0"),
        base.replace("\"version\":1", "\"version\":1.5"),
        base.replace("program.register@1", "program.publish@1"),
        base.replace(",\"by\":\"alice\"", ""),
        base.replace(':', ": "),
        // A register event without its content, and content on another event.
        base.replace(&format!(",\"content\":{}", json_string_after(&base, "\"content\":")), ""),
        ex.s("ac1_release_event").replace(",\"digest\"", ",\"content\":\"x\",\"digest\""),
    ] {
        assert!(LifeEvent::from_canonical_bytes(bad.as_bytes()).is_err(), "{bad}");
    }
}

// ------------------------------------------------------------------ declarations and schemas

#[test]
fn declarations_classes_and_projection() {
    let decls = declarations();
    let got: Vec<(String, u32, String, bool, Projection)> = decls
        .iter()
        .map(|d| {
            (
                d.def.id.clone(),
                d.def.major,
                d.def.side_effect.as_str().to_string(),
                d.def.human_only,
                d.def.effective_projection(),
            )
        })
        .collect();
    let want =
        |id: &str, class: &str, human: bool, proj: Projection| (id.to_string(), 1, class.to_string(), human, proj);
    assert_eq!(
        got,
        vec![
            want("program.register", "W2", false, Projection::Exposed),
            want("program.unregister", "W2", false, Projection::Exposed),
            want("program.release", "W3", true, Projection::RequestOnly),
            want("program.deprecate", "W3", true, Projection::RequestOnly),
            want("program.open_draft", "W2", false, Projection::Exposed),
            want("script.register", "W2", false, Projection::Exposed),
            want("script.unregister", "W2", false, Projection::Exposed),
            want("script.release", "W3", true, Projection::RequestOnly),
            want("script.deprecate", "W3", true, Projection::RequestOnly),
            want("script.open_draft", "W2", false, Projection::Exposed),
        ]
    );
    let raw: Vec<_> = decls.iter().map(|d| d.to_validation()).collect();
    assert_eq!(fid_registry::validate(&raw), vec![]);
    // The release preconditions, hand-labelled from REQ-004 and the Design: each expression and
    // message key, and they compile against the declared context types.
    let unadopted = "size(artifact.unadopted) == 0";
    for d in &decls {
        let got: Vec<(&str, &str)> =
            d.def.preconditions.iter().map(|p| (p.expr.as_str(), p.message_key.as_str())).collect();
        let want: Vec<(&str, &str)> = match d.def.id.as_str() {
            "program.release" => vec![("artifact.check_passed", "check_failed"), (unadopted, "unadopted_steps")],
            "script.release" => vec![(unadopted, "unadopted_version")],
            _ => Vec::new(),
        };
        assert_eq!(got, want, "{}", d.def.id);
        let preds = fid_registry::compile_command(&d.def, &context_types()).unwrap();
        assert_eq!(preds.len(), want.len(), "{}", d.def.id);
    }
    // Command IDs of the events match the declarations.
    for d in &decls {
        let (kind, verb) = Verb::from_command(&format!("{}@1", d.def.id)).unwrap();
        assert_eq!(verb.command(kind), format!("{}@1", d.def.id));
    }
}

/// The real AC parameters, as the CLI and MCP surfaces read them, conform to each command's
/// `params_schema`; malformed parameters do not.
#[test]
fn params_conform_to_params_schema() {
    let ex = Expected::load();
    let decls = declarations();
    let schema = |id: &str| decls.iter().find(|d| d.def.id == id).unwrap().def.params_schema.clone();
    let (p, d, sd) = (ex.s("program"), ex.s("minimal_digest"), ex.s("script_digest"));
    for (verb, word) in [
        ("register", "register"),
        ("unregister", "unregister"),
        ("release", "release"),
        ("deprecate", "deprecate"),
        ("open_draft", "open-draft"),
    ] {
        let ps = schema(&format!("program.{verb}"));
        let call = cli_params(&args("program", word, &p, &d)).unwrap();
        conforms(&ps, &call.to_value(), "").unwrap();
        let raw = format!(r#"{{"expected_digest":"{d}","note":"for the bracket line","program":"{p}"}}"#);
        let m = mcp_params(&format!("program_{verb}"), raw.as_bytes()).unwrap();
        assert_eq!(m.to_value(), parse(raw.as_bytes()).unwrap());
        conforms(&ps, &m.to_value(), "").unwrap();

        let ss = schema(&format!("script.{verb}"));
        let call = cli_params(&args("script", word, "hello", &sd)).unwrap();
        conforms(&ss, &call.to_value(), "").unwrap();
    }
    let ps = schema("program.release");
    for bad in [
        format!(r#"{{"expected_digest":"sha256:ABC","program":"{p}"}}"#),
        format!(r#"{{"expected_digest":"{d}","program":"P"}}"#),
        format!(r#"{{"expected_digest":"{d}"}}"#),
        format!(r#"{{"expected_digest":"{d}","kind":"human","program":"{p}"}}"#),
        format!(r#"{{"expected_digest":"{d}","note":"","program":"{p}"}}"#),
        format!(r#"{{"expected_digest":"{d}","note":"{}","program":"{p}"}}"#, "n".repeat(1025)),
    ] {
        assert!(conforms(&ps, &parse(bad.as_bytes()).unwrap(), "").is_err(), "{bad}");
    }
    let ss = schema("script.release");
    for bad in [
        format!(r#"{{"expected_digest":"{sd}","script":"-x"}}"#),
        format!(r#"{{"expected_digest":"{sd}","program":"{p}"}}"#),
    ] {
        assert!(conforms(&ss, &parse(bad.as_bytes()).unwrap(), "").is_err(), "{bad}");
    }
}

/// Real results of the AC runs conform to every command's declared `result_schema`, and
/// malformed results do not.
#[test]
fn results_conform_to_result_schema() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let (mut w, alice) = released_minimal(&mut table, &ex);
    let (p, d) = (ex.s("program"), ex.s("minimal_digest"));
    let deprecated = w.cli(&alice, MAIN, &args("program", "deprecate", &p, &d)).unwrap();
    let decls = declarations();
    let mut results = vec![ex.b("ac1_register_result"), ex.b("ac1_release_result"), deprecated];
    results.push(format!(r#"{{"digest":"{d}","state":"draft","version":1}}"#).into_bytes());
    results.extend(["ac6_register_result", "ac6_release_result", "ac6_deprecate_result"].map(|k| ex.b(k)));
    for decl in &decls {
        for r in &results {
            conforms(&decl.def.result_schema, &parse(r).unwrap(), "").unwrap();
        }
        for bad in [
            format!(r#"{{"digest":"{d}","state":"published","version":1}}"#),
            format!(r#"{{"digest":"{d}","state":"released","version":-1}}"#),
            format!(r#"{{"digest":"{d}","state":"released"}}"#),
            format!(r#"{{"digest":"{d}","extra":1,"state":"released","version":1}}"#),
            r#"{"digest":"sha256:00","state":"released","version":1}"#.to_string(),
        ] {
            assert!(conforms(&decl.def.result_schema, &parse(bad.as_bytes()).unwrap(), "").is_err(), "{bad}");
        }
    }
}

/// The patterns the schemas use, matched by hand (no regex engine in the workspace).
fn matches_pattern(p: &str, x: &str) -> bool {
    let b = x.as_bytes();
    if p == UUID7_PATTERN {
        Uuid7::parse(x).is_some()
    } else if p == Digest::PATTERN {
        b.len() == 71 && x.starts_with("sha256:") && b[7..].iter().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
    } else if p == SCRIPT_ID_PATTERN {
        !b.is_empty()
            && b.len() <= 128
            && b[0].is_ascii_alphanumeric()
            && b.iter().all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(c))
    } else {
        panic!("pattern {p} not supported by the test checker")
    }
}

fn conforms(schema: &CValue, v: &CValue, at: &str) -> Result<(), String> {
    let CValue::Obj(s) = schema else { return Err(format!("{at}: schema is not an object")) };
    for (k, kv) in s {
        let ok = match (k.as_str(), kv) {
            ("description" | "title" | "$schema", _) => true,
            ("type", CValue::Str(t)) => match (t.as_str(), v) {
                ("object", CValue::Obj(_)) | ("array", CValue::Arr(_)) | ("string", CValue::Str(_)) => true,
                ("boolean", CValue::Bool(_)) => true,
                ("integer", CValue::Num(n)) => n.fract() == 0.0,
                _ => false,
            },
            ("enum", CValue::Arr(values)) => values.contains(v),
            ("minimum", CValue::Num(m)) => matches!(v, CValue::Num(n) if n >= m),
            ("maximum", CValue::Num(m)) => matches!(v, CValue::Num(n) if n <= m),
            ("pattern", CValue::Str(p)) => {
                let CValue::Str(x) = v else { return Err(format!("{at}: pattern on a non-string")) };
                matches_pattern(p, x)
            }
            ("minLength", CValue::Num(n)) => matches!(v, CValue::Str(x) if x.chars().count() as f64 >= *n),
            ("maxLength", CValue::Num(n)) => matches!(v, CValue::Str(x) if x.chars().count() as f64 <= *n),
            ("required", CValue::Arr(r)) => {
                let CValue::Obj(o) = v else { return Err(format!("{at}: required on a non-object")) };
                r.iter().all(|k| matches!(k, CValue::Str(k) if o.contains_key(k)))
            }
            ("additionalProperties", CValue::Bool(false)) => {
                let (CValue::Obj(o), Some(CValue::Obj(props))) = (v, s.get("properties")) else {
                    return Err(format!("{at}: additionalProperties without properties"));
                };
                o.keys().all(|k| props.contains_key(k))
            }
            ("properties", CValue::Obj(props)) => {
                if let CValue::Obj(o) = v {
                    for (pk, ps) in props {
                        if let Some(pv) = o.get(pk) {
                            conforms(ps, pv, &format!("{at}/{pk}"))?;
                        }
                    }
                }
                true
            }
            (other, _) => return Err(format!("{at}: keyword {other} not supported by the test checker")),
        };
        if !ok {
            return Err(format!("{at}: fails {k}"));
        }
    }
    Ok(())
}
