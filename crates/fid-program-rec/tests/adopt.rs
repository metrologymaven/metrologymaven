//! Acceptance criteria of spec node AUTO.REC.ADOPT (AC-1 .. AC-7) and its fail-closed rules,
//! against `fixtures/rec/adopt/`, written by the independent reference
//! `fixtures/rec/adopt/make_fixtures.py` (expected events and results hand-labelled from the
//! spec). Principals are derived by the bus from channels, the only way to build one.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::policy::class::authorise_class;
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
    adopt_steps, cli_params, declarations, invoke_cli, invoke_mcp, mcp_params, replay, step_contents, AdoptCtx,
    AdoptError, AdoptEvent, AdoptJournal, AdoptStepsParams, Programs, ProvenanceStore, RecordedEntry, ScriptId, Target,
    MCP_ADOPT_STEPS, MCP_ADOPT_VERSION, SCRIPT_ID_PATTERN, UUID7_PATTERN,
};
use fid_registry::def::Projection;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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
        let CValue::Obj(o) = parse(&read("fixtures/rec/adopt/expected.json")).unwrap() else { panic!("object") };
        Expected(o)
    }
    fn s(&self, k: &str) -> String {
        match self.0.get(k) {
            Some(CValue::Str(s)) => s.clone(),
            other => panic!("{k}: {other:?}"),
        }
    }
    fn step(&self, k: &str) -> Uuid7 {
        let Some(CValue::Obj(o)) = self.0.get("steps") else { panic!("steps") };
        let Some(CValue::Str(s)) = o.get(k) else { panic!("{k}") };
        Uuid7::parse(s).unwrap()
    }
}

fn draft() -> Program {
    Program::parse(&read("fixtures/rec/adopt/draft-mixed.json")).unwrap()
}

fn edited() -> Program {
    Program::parse(&read("fixtures/rec/adopt/draft-edited.json")).unwrap()
}

struct OneProgram(Program);

impl Programs for OneProgram {
    fn program(&self, id: &Uuid7) -> Option<&Program> {
        (self.0.program_id == *id).then_some(&self.0)
    }
}

/// A journal that numbers its entries from 100, so they never coincide with the authorship
/// entries the tests record by hand.
struct Log {
    entries: Vec<(u64, Vec<u8>)>,
    next: u64,
}

impl Default for Log {
    fn default() -> Log {
        Log { entries: Vec::new(), next: 100 }
    }
}

impl Log {
    fn events(&self) -> Vec<Vec<u8>> {
        self.entries.iter().map(|(_, e)| e.clone()).collect()
    }
    fn recorded(&self) -> Vec<RecordedEntry> {
        self.entries
            .iter()
            .map(|(n, e)| RecordedEntry { journal_entry: *n, principal_kind: PrincipalKind::Human, event: e.clone() })
            .collect()
    }
}

impl AdoptJournal for Log {
    fn commit(&mut self, event: &AdoptEvent) -> Result<u64, String> {
        let n = self.next;
        self.next += 1;
        self.entries.push((n, event.to_canonical_bytes()));
        Ok(n)
    }
}

struct Failing;

impl AdoptJournal for Failing {
    fn commit(&mut self, _event: &AdoptEvent) -> Result<u64, String> {
        Err("disk full".into())
    }
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

// ------------------------------------------------------------------ helpers

fn step(p: &Program, s: Uuid7) -> Target {
    Target::Step { program: p.program_id, step: s }
}

/// The content digest of step `s` in program `p`.
fn content(p: &Program, s: Uuid7) -> Digest {
    *step_contents(p).unwrap().get(&s).unwrap()
}

/// Records `who` as author of step `s` with its content in `p`, in journal entry `entry`.
fn author(store: &mut ProvenanceStore, p: &Program, s: Uuid7, who: &Principal, entry: u64) {
    store.record_authorship(step(p, s), content(p, s), who, entry);
}

/// The AC-1 starting state: S1 by a person, S2 and S3 by an agent (entries 1, 2, 3).
fn mixed_store(table: &mut SessionTable, ex: &Expected, p: &Program) -> ProvenanceStore {
    let human = principal_as(table, PrincipalKind::Human, "bob");
    let agent = principal_as(table, PrincipalKind::Agent, "claude-client");
    let mut store = ProvenanceStore::new();
    author(&mut store, p, ex.step("S1"), &human, 1);
    author(&mut store, p, ex.step("S2"), &agent, 2);
    author(&mut store, p, ex.step("S3"), &agent, 3);
    store
}

fn cli_args<'a>(program: &'a str, steps: &'a str, digest: &'a str) -> Vec<&'a str> {
    vec!["program", "adopt-steps", "--program", program, "--steps", steps, "--reviewed-digest", digest, "--json"]
}

fn run_cli(
    principal: &Principal,
    store: &mut ProvenanceStore,
    programs: &dyn Programs,
    journal: &mut dyn AdoptJournal,
    args: &[&str],
) -> Result<Vec<u8>, AdoptError> {
    let mut ctx = AdoptCtx { principal, store, programs, journal };
    invoke_cli(&mut ctx, args)
}

fn run_mcp(
    principal: &Principal,
    store: &mut ProvenanceStore,
    programs: &dyn Programs,
    journal: &mut dyn AdoptJournal,
    tool: &str,
    raw: &str,
) -> Result<Vec<u8>, AdoptError> {
    let mut ctx = AdoptCtx { principal, store, programs, journal };
    invoke_mcp(&mut ctx, tool, raw.as_bytes())
}

fn ac1_mcp_params(ex: &Expected) -> String {
    format!(
        r#"{{"program":"{}","steps":["{}","{}"],"reviewed_digest":"{}"}}"#,
        ex.s("program"),
        ex.step("S2"),
        ex.step("S3"),
        ex.s("draft_digest")
    )
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_human_adopts_agent_steps() {
    let ex = Expected::load();
    let p = draft();
    assert_eq!(p.digest().unwrap().as_str(), ex.s("draft_digest"));
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    assert_eq!(store.unadopted_steps(&p), vec![ex.step("S2"), ex.step("S3")]);

    let human = cli_session(&mut table, "alice", true);
    assert_eq!(human.kind(), PrincipalKind::Human);
    let steps = format!("{},{}", ex.step("S2"), ex.step("S3"));
    let digest = ex.s("draft_digest");
    let prog = ex.s("program");
    let mut log = Log::default();
    let out = run_cli(&human, &mut store, &OneProgram(p.clone()), &mut log, &cli_args(&prog, &steps, &digest)).unwrap();
    assert_eq!(out, ex.s("ac1_result").into_bytes());
    assert_eq!(log.events(), vec![ex.s("ac1_event").into_bytes()]);
    assert!(store.unadopted_steps(&p).is_empty());
    assert_eq!(store.adoptions().len(), 2);
    for a in store.adoptions() {
        assert_eq!((a.human_id.as_str(), a.digest.as_str(), a.journal_entry), ("alice", digest.as_str(), 100));
    }
    assert_eq!(store.adoptions()[0].content, content(&p, ex.step("S2")));
}

/// Steps named out of document order are journaled and returned in document order.
#[test]
fn steps_are_adopted_in_document_order() {
    let ex = Expected::load();
    let p = draft();
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    let human = principal_as(&mut table, PrincipalKind::Human, "alice");
    let steps = format!("{},{}", ex.step("S3"), ex.step("S2"));
    let (prog, digest) = (ex.s("program"), ex.s("draft_digest"));
    let mut log = Log::default();
    let out = run_cli(&human, &mut store, &OneProgram(p), &mut log, &cli_args(&prog, &steps, &digest)).unwrap();
    assert_eq!(out, ex.s("ac1_result").into_bytes());
    assert_eq!(log.events(), vec![ex.s("ac1_event").into_bytes()]);
}

// ------------------------------------------------------------------ AC-2 and the MCP surface

#[test]
fn ac2_agent_and_script_refused_store_unchanged() {
    let ex = Expected::load();
    let p = draft();
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    let before = store.clone();
    let programs = OneProgram(p.clone());
    let steps = format!("{},{}", ex.step("S2"), ex.step("S3"));
    let (prog, digest) = (ex.s("program"), ex.s("draft_digest"));
    let mut log = Log::default();

    // MCP agent session.
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let e = run_mcp(&agent, &mut store, &programs, &mut log, MCP_ADOPT_STEPS, &ac1_mcp_params(&ex)).unwrap_err();
    assert_eq!(e, AdoptError::PolicyRefused { reason: "human_only" });

    // CLI session that was never upgraded: a script principal.
    let script = cli_session(&mut table, "alice", false);
    assert_eq!(script.kind(), PrincipalKind::Script);
    let e = run_cli(&script, &mut store, &programs, &mut log, &cli_args(&prog, &steps, &digest)).unwrap_err();
    assert_eq!(e, AdoptError::PolicyRefused { reason: "human_only" });

    assert_eq!(store, before);
    assert!(log.entries.is_empty());
    assert_eq!(store.unadopted_steps(&p), vec![ex.step("S2"), ex.step("S3")]);
}

/// MCP is request-only: even a person's call through MCP adopts nothing and is
/// `policy_refused/request_only`; the person adopts through the CLI or the GUI.
#[test]
fn mcp_never_adopts_even_for_a_person() {
    let ex = Expected::load();
    let p = draft();
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    let before = store.clone();
    let programs = OneProgram(p);
    let mut log = Log::default();
    for human in [principal_as(&mut table, PrincipalKind::Human, "alice"), cli_session(&mut table, "carol", true)] {
        assert_eq!(human.kind(), PrincipalKind::Human);
        let e = run_mcp(&human, &mut store, &programs, &mut log, MCP_ADOPT_STEPS, &ac1_mcp_params(&ex)).unwrap_err();
        assert_eq!(e, AdoptError::PolicyRefused { reason: "request_only" });
    }
    assert_eq!(store, before);
    assert!(log.entries.is_empty());
}

/// A principal kind or principal field in the MCP parameters is `bad_params` (no parameter
/// can name the caller), for an agent and for a person, and the store is unchanged.
#[test]
fn spoofed_kind_in_params_is_bad_params() {
    let ex = Expected::load();
    let p = draft();
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    let before = store.clone();
    let programs = OneProgram(p);
    let mut log = Log::default();
    let base = ac1_mcp_params(&ex);
    let spoofs = [
        base.replacen('{', r#"{"kind":"human","#, 1),
        base.replacen('{', r#"{"principal":{"id":"alice","kind":"human"},"#, 1),
        base.replacen('{', r#"{"principal_kind":"human","#, 1),
    ];
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let human = principal_as(&mut table, PrincipalKind::Human, "alice");
    for raw in &spoofs {
        for who in [&agent, &human] {
            let e = run_mcp(who, &mut store, &programs, &mut log, MCP_ADOPT_STEPS, raw).unwrap_err();
            assert_eq!(e.code(), "bad_params", "{raw}");
        }
    }
    let version =
        format!(r#"{{"kind":"human","script":"{}","version_digest":"{}"}}"#, ex.s("script"), ex.s("script_v1"));
    let e = run_mcp(&agent, &mut store, &programs, &mut log, MCP_ADOPT_VERSION, &version).unwrap_err();
    assert_eq!(e.code(), "bad_params");
    assert_eq!(store, before);
    assert!(log.entries.is_empty());
}

/// Every non-human kind is refused by both commands, at the bus class check and in the
/// handler, whatever the parameters say.
#[test]
fn every_non_human_kind_refused() {
    let ex = Expected::load();
    let p = draft();
    let programs = OneProgram(p.clone());
    let mut table = SessionTable::new();
    let decls = declarations();
    let mut store = mixed_store(&mut table, &ex, &p);
    let v1 = Digest::parse(&ex.s("script_v1")).unwrap();
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let script_id = ScriptId::new(&ex.s("script")).unwrap();
    store.record_authorship(Target::ScriptVersion { script: script_id, version: v1 }, v1, &agent, 4);
    let before = store.clone();
    for kind in PrincipalKind::ALL {
        let who = principal_as(&mut table, *kind, &format!("who-{}", kind.as_str()));
        for d in &decls {
            let r = authorise_class(&who, &d.def);
            assert_eq!(r.is_ok(), *kind == PrincipalKind::Human, "{kind:?} {}", d.def.id);
        }
        if *kind == PrincipalKind::Human {
            continue;
        }
        let mut log = Log::default();
        let mut ctx = AdoptCtx { principal: &who, store: &mut store, programs: &programs, journal: &mut log };
        let params = AdoptStepsParams {
            program: ex.s("program"),
            steps: None,
            all_unadopted: Some(true),
            reviewed_digest: ex.s("draft_digest"),
        };
        assert_eq!(adopt_steps(&mut ctx, params).unwrap_err(), AdoptError::PolicyRefused { reason: "human_only" });
        let raw = format!(r#"{{"script":"{}","version_digest":"{}"}}"#, ex.s("script"), ex.s("script_v1"));
        assert_eq!(
            invoke_mcp(&mut ctx, MCP_ADOPT_VERSION, raw.as_bytes()).unwrap_err(),
            AdoptError::PolicyRefused { reason: "human_only" }
        );
        let args = ["script", "adopt-version", "--script", "probe-sweep", "--version-digest", v1.as_str()];
        assert_eq!(invoke_cli(&mut ctx, &args).unwrap_err(), AdoptError::PolicyRefused { reason: "human_only" });
        assert!(log.entries.is_empty());
    }
    assert_eq!(store, before);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_stale_review() {
    let ex = Expected::load();
    let p = draft();
    let now = edited();
    assert_eq!(now.program_id, p.program_id);
    assert_eq!(now.digest().unwrap().as_str(), ex.s("edited_digest"));
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    let before = store.clone();
    let human = principal_as(&mut table, PrincipalKind::Human, "alice");
    let mut log = Log::default();
    let steps = format!("{},{}", ex.step("S2"), ex.step("S3"));
    let (prog, digest) = (ex.s("program"), ex.s("draft_digest"));
    let e = run_cli(&human, &mut store, &OneProgram(now), &mut log, &cli_args(&prog, &steps, &digest)).unwrap_err();
    assert_eq!(e, AdoptError::StaleReview { current: ex.s("edited_digest") });
    assert_eq!(store, before);
    assert!(log.entries.is_empty());
}

// ------------------------------------------------------------------ AC-4 and ordering

#[test]
fn ac4_agent_edit_drops_adoption() {
    let ex = Expected::load();
    let p = draft();
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    let human = principal_as(&mut table, PrincipalKind::Human, "alice");
    let steps = format!("{},{}", ex.step("S2"), ex.step("S3"));
    let (prog, digest) = (ex.s("program"), ex.s("draft_digest"));
    run_cli(&human, &mut store, &OneProgram(p.clone()), &mut Log::default(), &cli_args(&prog, &steps, &digest))
        .unwrap();
    assert!(store.unadopted_steps(&p).is_empty());

    // The agent's edit is a later journal entry than the adoption (100).
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    author(&mut store, &p, ex.step("S2"), &agent, 101);
    assert_eq!(store.unadopted_steps(&p), vec![ex.step("S2")]);
    // The agent's edit adds a record; it removes none.
    assert_eq!((store.authorships().len(), store.adoptions().len()), (4, 2));
    // A person editing S3 keeps it adopted; a person editing S2 settles it.
    author(&mut store, &p, ex.step("S3"), &human, 102);
    assert_eq!(store.unadopted_steps(&p), vec![ex.step("S2")]);
    author(&mut store, &p, ex.step("S2"), &human, 103);
    assert!(store.unadopted_steps(&p).is_empty());
}

/// An agent's authorship record arrives (entry 50) before its content commit lands in the
/// program. The person reviews the program as it still is and adopts S3 in a later entry
/// (100). Once the agent's content lands, S3 is unadopted again: the adoption covers only the
/// content the person saw, whatever the entry order.
#[test]
fn authorship_in_the_gap_before_its_content_commit() {
    let ex = Expected::load();
    let seen = draft();
    let landed = edited();
    let s3 = ex.step("S3");
    assert_ne!(content(&seen, s3), content(&landed, s3));
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &seen);
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    author(&mut store, &landed, s3, &agent, 50);

    let human = principal_as(&mut table, PrincipalKind::Human, "alice");
    let (prog, digest) = (ex.s("program"), ex.s("draft_digest"));
    let mut log = Log::default();
    run_cli(&human, &mut store, &OneProgram(seen.clone()), &mut log, &cli_args(&prog, &s3.to_string(), &digest))
        .unwrap();
    assert_eq!(log.entries[0].0, 100);
    assert_eq!(store.unadopted_steps(&seen), vec![ex.step("S2")]);
    assert_eq!(store.unadopted_steps(&landed), vec![ex.step("S2"), s3]);
    // Content no record describes is unadopted even for a step a person wrote.
    assert!(store.is_unadopted(&step(&seen, ex.step("S1")), &content(&landed, s3)));
}

/// Order is the journal entry number, not arrival: an adoption that arrives after an agent
/// authorship but carries an earlier entry number does not cover it.
#[test]
fn order_is_the_journal_entry_not_arrival() {
    let ex = Expected::load();
    let p = draft();
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let human = principal_as(&mut table, PrincipalKind::Human, "alice");
    // The agent's re-edit of S2 is entry 200; the adoption is committed as entry 100.
    author(&mut store, &p, ex.step("S2"), &agent, 200);
    let (prog, digest) = (ex.s("program"), ex.s("draft_digest"));
    run_cli(
        &human,
        &mut store,
        &OneProgram(p.clone()),
        &mut Log::default(),
        &cli_args(&prog, &ex.step("S2").to_string(), &digest),
    )
    .unwrap();
    assert_eq!(store.unadopted_steps(&p), vec![ex.step("S2"), ex.step("S3")]);
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_script_version_adoption() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let human = cli_session(&mut table, "alice", true);
    let script = ScriptId::new(&ex.s("script")).unwrap();
    let v1 = Digest::parse(&ex.s("script_v1")).unwrap();
    let v2 = Digest::parse(&ex.s("script_v2")).unwrap();
    let mut store = ProvenanceStore::new();
    store.record_authorship(Target::ScriptVersion { script: script.clone(), version: v1 }, v1, &agent, 1);
    assert!(store.unadopted_script(&script, &v1));

    let mut log = Log::default();
    let programs = OneProgram(draft());
    let args = ["script", "adopt-version", "--script", "probe-sweep", "--version-digest", v1.as_str(), "--json"];
    let out = run_cli(&human, &mut store, &programs, &mut log, &args).unwrap();
    assert_eq!(out, ex.s("ac5_result").into_bytes());
    assert_eq!(log.events(), vec![ex.s("ac5_event").into_bytes()]);
    assert!(!store.unadopted_script(&script, &v1));

    store.record_authorship(Target::ScriptVersion { script: script.clone(), version: v2 }, v2, &agent, 101);
    assert!(store.unadopted_script(&script, &v2));
    assert!(!store.unadopted_script(&script, &v1));
    // Adopting v1 again: nothing to adopt. An unknown version: unknown_version.
    let e = run_cli(&human, &mut store, &programs, &mut log, &args).unwrap_err();
    assert_eq!(e.code(), "nothing_to_adopt");
    let v3 = "sha256:".to_string() + &"ab".repeat(32);
    let args3 = ["script", "adopt-version", "--script", "probe-sweep", "--version-digest", v3.as_str()];
    assert_eq!(run_cli(&human, &mut store, &programs, &mut log, &args3).unwrap_err().code(), "unknown_version");
    assert_eq!(log.entries.len(), 1);
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_nothing_to_adopt_and_unknown_step() {
    let ex = Expected::load();
    let p = draft();
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    let before = store.clone();
    let human = principal_as(&mut table, PrincipalKind::Human, "alice");
    let programs = OneProgram(p.clone());
    let mut log = Log::default();
    let (prog, digest) = (ex.s("program"), ex.s("draft_digest"));

    // S1 is human-authored; naming it with S2 adopts nothing.
    let steps = format!("{},{}", ex.step("S2"), ex.step("S1"));
    let e = run_cli(&human, &mut store, &programs, &mut log, &cli_args(&prog, &steps, &digest)).unwrap_err();
    assert_eq!(e, AdoptError::NothingToAdopt { target: ex.step("S1").to_string() });

    let stranger = "019a0000-0009-7000-8000-00000000a009";
    let steps = format!("{},{stranger}", ex.step("S2"));
    let e = run_cli(&human, &mut store, &programs, &mut log, &cli_args(&prog, &steps, &digest)).unwrap_err();
    assert_eq!(e, AdoptError::UnknownStep { step: stranger.to_string() });

    let other = "019a0000-0000-7000-8000-00000000b000";
    let e = run_cli(&human, &mut store, &programs, &mut log, &cli_args(other, &steps, &digest)).unwrap_err();
    assert_eq!(e.code(), "program_not_found");

    // A step named twice, both selections, neither: bad_params.
    let twice = format!("{},{}", ex.step("S2"), ex.step("S2"));
    let e = run_cli(&human, &mut store, &programs, &mut log, &cli_args(&prog, &twice, &digest)).unwrap_err();
    assert_eq!(e.code(), "bad_params");
    let mut both = cli_args(&prog, &twice, &digest);
    both.push("--all-unadopted");
    assert_eq!(run_cli(&human, &mut store, &programs, &mut log, &both).unwrap_err().code(), "bad_params");
    let neither = ["program", "adopt-steps", "--program", prog.as_str(), "--reviewed-digest", digest.as_str()];
    assert_eq!(run_cli(&human, &mut store, &programs, &mut log, &neither).unwrap_err().code(), "bad_params");

    assert_eq!(store, before);
    assert!(log.entries.is_empty());
}

// ------------------------------------------------------------------ AC-7

#[test]
fn ac7_replay_identical() {
    let ex = Expected::load();
    let p = draft();
    let programs = OneProgram(p.clone());
    let mut table = SessionTable::new();
    let start = mixed_store(&mut table, &ex, &p);
    let mut live = start.clone();
    let human = principal_as(&mut table, PrincipalKind::Human, "alice");
    let mut log = Log::default();
    let steps = format!("{},{}", ex.step("S2"), ex.step("S3"));
    let (prog, digest) = (ex.s("program"), ex.s("draft_digest"));
    run_cli(&human, &mut live, &programs, &mut log, &cli_args(&prog, &steps, &digest)).unwrap();

    let r = replay(&start, &programs, &log.recorded());
    assert_eq!(r.outcome, ReplayOutcome::Identical);
    assert_eq!(r.steps, vec![ReplayOutcome::Identical]);
    assert!(r.provenance.matches(&live));
    assert!(r.provenance.unadopted_steps(&p).is_empty());

    // An entry whose envelope principal is not human does not replay.
    for kind in PrincipalKind::ALL.iter().filter(|k| **k != PrincipalKind::Human) {
        let mut entries = log.recorded();
        entries[0].principal_kind = *kind;
        let r = replay(&start, &programs, &entries);
        assert_eq!(r.outcome, ReplayOutcome::Divergent, "{kind:?}");
        assert!(r.provenance.matches(&start));
    }
    // A recorded event naming the human-authored S1 does not replay; garbage is not replayable.
    let forged = ex.s("ac1_event").replace(&ex.step("S2").to_string(), &ex.step("S1").to_string());
    let entry = |event: Vec<u8>| RecordedEntry { journal_entry: 100, principal_kind: PrincipalKind::Human, event };
    let r = replay(&start, &programs, &[entry(forged.into_bytes())]);
    assert_eq!(r.outcome, ReplayOutcome::Divergent);
    assert!(r.provenance.matches(&start));
    let r = replay(&start, &programs, &[entry(b"{\"command\":\"x\"}".to_vec())]);
    assert_eq!(r.outcome, ReplayOutcome::NotReplayable);
}

// ------------------------------------------------------------------ fail closed

/// Steps with no provenance record count as unadopted; a person can still adopt them.
#[test]
fn unknown_provenance_is_unadopted() {
    let ex = Expected::load();
    let p = draft();
    let mut store = ProvenanceStore::new();
    assert_eq!(store.unadopted_steps(&p), p.step_ids());
    let script = ScriptId::new("never-seen").unwrap();
    assert!(store.unadopted_script(&script, &Digest::parse(&ex.s("script_v1")).unwrap()));

    let mut table = SessionTable::new();
    let human = principal_as(&mut table, PrincipalKind::Human, "alice");
    let programs = OneProgram(p.clone());
    let mut log = Log::default();
    let (prog, dig) = (ex.s("program"), ex.s("draft_digest"));
    let args =
        ["program", "adopt-steps", "--program", prog.as_str(), "--all-unadopted", "--reviewed-digest", dig.as_str()];
    run_cli(&human, &mut store, &programs, &mut log, &args).unwrap();
    assert!(store.unadopted_steps(&p).is_empty());
    // Any later non-human authorship supersedes that adoption.
    let service = principal_as(&mut table, PrincipalKind::Service, "svc");
    author(&mut store, &p, ex.step("S1"), &service, 101);
    assert_eq!(store.unadopted_steps(&p), vec![ex.step("S1")]);
}

#[test]
fn journal_failure_adopts_nothing() {
    let ex = Expected::load();
    let p = draft();
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    let before = store.clone();
    let human = principal_as(&mut table, PrincipalKind::Human, "alice");
    let steps = format!("{},{}", ex.step("S2"), ex.step("S3"));
    let (prog, digest) = (ex.s("program"), ex.s("draft_digest"));
    let e = run_cli(&human, &mut store, &OneProgram(p), &mut Failing, &cli_args(&prog, &steps, &digest)).unwrap_err();
    assert_eq!(e.code(), "journal");
    assert_eq!(store, before);
}

// ------------------------------------------------------------------ declarations and schemas

#[test]
fn declarations_are_human_only_w2_request_only() {
    let decls = declarations();
    let ids: Vec<(&str, u32)> = decls.iter().map(|d| (d.def.id.as_str(), d.def.major)).collect();
    assert_eq!(ids, vec![("program.adopt_steps", 1), ("script.adopt_version", 1)]);
    for d in &decls {
        assert!(d.def.human_only, "{}", d.def.id);
        assert_eq!(d.def.side_effect.as_str(), "W2");
        assert_eq!(d.def.effective_projection(), Projection::RequestOnly);
    }
    let raw: Vec<_> = decls.iter().map(|d| d.to_validation()).collect();
    assert_eq!(fid_registry::validate(&raw), vec![]);
}

/// The real AC-1 and AC-5 parameters, as the MCP and CLI surfaces read them, conform to each
/// command's `params_schema`; malformed parameters do not.
#[test]
fn params_conform_to_params_schema() {
    let ex = Expected::load();
    let decls = declarations();
    let (steps_schema, version_schema) = (&decls[0].def.params_schema, &decls[1].def.params_schema);

    let mcp_raw = ac1_mcp_params(&ex);
    conforms(steps_schema, &parse(mcp_raw.as_bytes()).unwrap(), "").unwrap();
    let read = mcp_params(MCP_ADOPT_STEPS, mcp_raw.as_bytes()).unwrap();
    assert_eq!(read.to_value(), parse(mcp_raw.as_bytes()).unwrap());

    let steps = format!("{},{}", ex.step("S2"), ex.step("S3"));
    let (prog, digest) = (ex.s("program"), ex.s("draft_digest"));
    let cli = cli_params(&cli_args(&prog, &steps, &digest)).unwrap();
    assert_eq!(cli, read);
    conforms(steps_schema, &cli.to_value(), "").unwrap();
    let all =
        ["program", "adopt-steps", "--program", prog.as_str(), "--all-unadopted", "--reviewed-digest", digest.as_str()];
    conforms(steps_schema, &cli_params(&all).unwrap().to_value(), "").unwrap();

    let v1 = ex.s("script_v1");
    let version_args = ["script", "adopt-version", "--script", "probe-sweep", "--version-digest", v1.as_str()];
    let version = cli_params(&version_args).unwrap();
    conforms(version_schema, &version.to_value(), "").unwrap();
    let version_raw = format!(r#"{{"script":"{}","version_digest":"{v1}"}}"#, ex.s("script"));
    assert_eq!(mcp_params(MCP_ADOPT_VERSION, version_raw.as_bytes()).unwrap(), version);

    let s2 = ex.step("S2");
    let bad_steps = [
        format!(r#"{{"program":"{prog}","steps":["{s2}"],"reviewed_digest":"sha256:ABC"}}"#),
        format!(r#"{{"program":"{prog}","steps":["S2"],"reviewed_digest":"{digest}"}}"#),
        format!(r#"{{"program":"P","steps":["{s2}"],"reviewed_digest":"{digest}"}}"#),
        format!(r#"{{"program":"{prog}","steps":[],"reviewed_digest":"{digest}"}}"#),
        format!(r#"{{"kind":"human","program":"{prog}","steps":["{s2}"],"reviewed_digest":"{digest}"}}"#),
        format!(r#"{{"program":"{prog}","steps":["{s2}"]}}"#),
    ];
    for bad in &bad_steps {
        assert!(conforms(steps_schema, &parse(bad.as_bytes()).unwrap(), "").is_err(), "{bad}");
    }
    let bad_versions = [
        format!(r#"{{"script":"-x","version_digest":"{v1}"}}"#),
        format!(r#"{{"script":"a b","version_digest":"{v1}"}}"#),
        format!(r#"{{"script":"{}","version_digest":"{v1}"}}"#, "x".repeat(129)),
        r#"{"script":"probe-sweep","version_digest":"sha1:00"}"#.to_string(),
    ];
    for bad in &bad_versions {
        assert!(conforms(version_schema, &parse(bad.as_bytes()).unwrap(), "").is_err(), "{bad}");
    }
}

/// The AC results conform to each command's declared `result_schema`, and a malformed
/// result does not.
#[test]
fn results_conform_to_result_schema() {
    let ex = Expected::load();
    let decls = declarations();
    let steps_schema = &decls[0].def.result_schema;
    let version_schema = &decls[1].def.result_schema;
    conforms(steps_schema, &parse(ex.s("ac1_result").as_bytes()).unwrap(), "").unwrap();
    conforms(version_schema, &parse(ex.s("ac5_result").as_bytes()).unwrap(), "").unwrap();
    for bad in
        [r#"{"adopted":[]}"#, r#"{"adopted":["S2"]}"#, r#"{"adopted":["019a0000-0002-7000-8000-00000000a002"],"x":1}"#]
    {
        assert!(conforms(steps_schema, &parse(bad.as_bytes()).unwrap(), "").is_err(), "{bad}");
    }
    assert!(conforms(version_schema, &parse(br#"{"adopted":"yes"}"#).unwrap(), "").is_err());
}

/// The three patterns the schemas use, matched by hand (no regex engine in the workspace).
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
            ("type", CValue::Str(t)) => matches!(
                (t.as_str(), v),
                ("object", CValue::Obj(_))
                    | ("array", CValue::Arr(_))
                    | ("string", CValue::Str(_))
                    | ("boolean", CValue::Bool(_))
            ),
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
            ("items", item) => {
                if let CValue::Arr(a) = v {
                    for (i, x) in a.iter().enumerate() {
                        conforms(item, x, &format!("{at}/{i}"))?;
                    }
                }
                true
            }
            ("minItems", CValue::Num(n)) => matches!(v, CValue::Arr(a) if a.len() as f64 >= *n),
            ("maxItems", CValue::Num(n)) => matches!(v, CValue::Arr(a) if a.len() as f64 <= *n),
            ("uniqueItems", CValue::Bool(true)) => {
                matches!(v, CValue::Arr(a) if a.iter().enumerate().all(|(i, x)| !a[..i].contains(x)))
            }
            (other, _) => return Err(format!("{at}: keyword {other} not supported by the test checker")),
        };
        if !ok {
            return Err(format!("{at}: fails {k}"));
        }
    }
    Ok(())
}
