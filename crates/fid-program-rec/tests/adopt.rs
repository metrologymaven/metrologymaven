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
    adopt_steps, declarations, invoke_cli, invoke_mcp, replay, AdoptCtx, AdoptError, AdoptEvent, AdoptJournal,
    AdoptStepsParams, Programs, ProvenanceStore, ScriptId, Target, MCP_ADOPT_STEPS, MCP_ADOPT_VERSION,
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

#[derive(Default)]
struct Log(Vec<Vec<u8>>);

impl AdoptJournal for Log {
    fn commit(&mut self, event: &AdoptEvent) -> Result<(), String> {
        self.0.push(event.to_canonical_bytes());
        Ok(())
    }
}

struct Failing;

impl AdoptJournal for Failing {
    fn commit(&mut self, _event: &AdoptEvent) -> Result<(), String> {
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

/// The AC-1 starting state: S1 by a person, S2 and S3 by an agent.
fn mixed_store(table: &mut SessionTable, ex: &Expected, p: &Program) -> ProvenanceStore {
    let human = principal_as(table, PrincipalKind::Human, "bob");
    let agent = principal_as(table, PrincipalKind::Agent, "claude-client");
    let mut store = ProvenanceStore::new();
    store.record_authorship(step(p, ex.step("S1")), &human);
    store.record_authorship(step(p, ex.step("S2")), &agent);
    store.record_authorship(step(p, ex.step("S3")), &agent);
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
    let mut log = Log::default();
    let out =
        run_cli(&human, &mut store, &OneProgram(p.clone()), &mut log, &cli_args(&ex.s("program"), &steps, &digest))
            .unwrap();
    assert_eq!(out, ex.s("ac1_result").into_bytes());
    assert_eq!(log.0, vec![ex.s("ac1_event").into_bytes()]);
    assert!(store.unadopted_steps(&p).is_empty());
    assert_eq!(store.adoptions().len(), 2);
    assert!(store.adoptions().iter().all(|a| a.human_id == "alice" && a.digest.as_str() == digest));
}

/// The same adoption through MCP from a human principal returns the same bytes and event;
/// steps named out of document order are journaled in document order.
#[test]
fn mcp_matches_cli_and_orders_steps() {
    let ex = Expected::load();
    let p = draft();
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    let human = principal_as(&mut table, PrincipalKind::Human, "alice");
    let raw = format!(
        r#"{{"program":"{}","steps":["{}","{}"],"reviewed_digest":"{}"}}"#,
        ex.s("program"),
        ex.step("S3"),
        ex.step("S2"),
        ex.s("draft_digest")
    );
    let mut log = Log::default();
    let programs = OneProgram(p);
    let mut ctx = AdoptCtx { principal: &human, store: &mut store, programs: &programs, journal: &mut log };
    let out = invoke_mcp(&mut ctx, MCP_ADOPT_STEPS, raw.as_bytes()).unwrap();
    assert_eq!(out, ex.s("ac1_result").into_bytes());
    assert_eq!(log.0, vec![ex.s("ac1_event").into_bytes()]);
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_agent_and_script_refused_store_unchanged() {
    let ex = Expected::load();
    let p = draft();
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    let before = store.clone();
    let programs = OneProgram(p.clone());
    let steps = format!("{},{}", ex.step("S2"), ex.step("S3"));
    let digest = ex.s("draft_digest");
    let mut log = Log::default();

    // MCP agent session.
    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    let raw = format!(r#"{{"program":"{}","all_unadopted":true,"reviewed_digest":"{digest}"}}"#, ex.s("program"));
    let mut ctx = AdoptCtx { principal: &agent, store: &mut store, programs: &programs, journal: &mut log };
    let e = invoke_mcp(&mut ctx, MCP_ADOPT_STEPS, raw.as_bytes()).unwrap_err();
    assert_eq!(e, AdoptError::PolicyRefused { reason: "human_only" });

    // CLI session that was never upgraded: a script principal.
    let script = cli_session(&mut table, "alice", false);
    assert_eq!(script.kind(), PrincipalKind::Script);
    let e =
        run_cli(&script, &mut store, &programs, &mut log, &cli_args(&ex.s("program"), &steps, &digest)).unwrap_err();
    assert_eq!(e, AdoptError::PolicyRefused { reason: "human_only" });

    assert_eq!(store, before);
    assert!(log.0.is_empty());
    assert_eq!(store.unadopted_steps(&p), vec![ex.step("S2"), ex.step("S3")]);
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
    store.record_authorship(Target::ScriptVersion { script: script_id, version: v1 }, &agent);
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
        assert_eq!(adopt_steps(&mut ctx, params).unwrap_err().code(), "policy_refused", "{kind:?}");
        let raw = format!(r#"{{"script":"{}","version_digest":"{}"}}"#, ex.s("script"), ex.s("script_v1"));
        assert_eq!(invoke_mcp(&mut ctx, MCP_ADOPT_VERSION, raw.as_bytes()).unwrap_err().code(), "policy_refused");
        assert!(log.0.is_empty());
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
    let e = run_cli(
        &human,
        &mut store,
        &OneProgram(now),
        &mut log,
        &cli_args(&ex.s("program"), &steps, &ex.s("draft_digest")),
    )
    .unwrap_err();
    assert_eq!(e, AdoptError::StaleReview { current: ex.s("edited_digest") });
    assert_eq!(store, before);
    assert!(log.0.is_empty());
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_agent_edit_drops_adoption() {
    let ex = Expected::load();
    let p = draft();
    let mut table = SessionTable::new();
    let mut store = mixed_store(&mut table, &ex, &p);
    let human = principal_as(&mut table, PrincipalKind::Human, "alice");
    let steps = format!("{},{}", ex.step("S2"), ex.step("S3"));
    run_cli(
        &human,
        &mut store,
        &OneProgram(p.clone()),
        &mut Log::default(),
        &cli_args(&ex.s("program"), &steps, &ex.s("draft_digest")),
    )
    .unwrap();
    assert!(store.unadopted_steps(&p).is_empty());

    let agent = principal_as(&mut table, PrincipalKind::Agent, "claude-client");
    store.record_authorship(step(&p, ex.step("S2")), &agent);
    assert_eq!(store.unadopted_steps(&p), vec![ex.step("S2")]);
    // The agent's edit adds a record; it removes none.
    assert_eq!((store.authorships().len(), store.adoptions().len()), (4, 2));
    // A person editing S3 keeps it adopted; a person editing S2 settles it.
    store.record_authorship(step(&p, ex.step("S3")), &human);
    assert_eq!(store.unadopted_steps(&p), vec![ex.step("S2")]);
    store.record_authorship(step(&p, ex.step("S2")), &human);
    assert!(store.unadopted_steps(&p).is_empty());
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
    store.record_authorship(Target::ScriptVersion { script: script.clone(), version: v1 }, &agent);
    assert!(store.unadopted_script(&script, &v1));

    let mut log = Log::default();
    let programs = OneProgram(draft());
    let args = ["script", "adopt-version", "--script", "probe-sweep", "--version-digest", v1.as_str(), "--json"];
    let out = run_cli(&human, &mut store, &programs, &mut log, &args).unwrap();
    assert_eq!(out, ex.s("ac5_result").into_bytes());
    assert_eq!(log.0, vec![ex.s("ac5_event").into_bytes()]);
    assert!(!store.unadopted_script(&script, &v1));

    store.record_authorship(Target::ScriptVersion { script: script.clone(), version: v2 }, &agent);
    assert!(store.unadopted_script(&script, &v2));
    assert!(!store.unadopted_script(&script, &v1));
    // Adopting v1 again: nothing to adopt. An unknown version: unknown_version.
    let e = run_cli(&human, &mut store, &programs, &mut log, &args).unwrap_err();
    assert_eq!(e.code(), "nothing_to_adopt");
    let v3 = "sha256:".to_string() + &"ab".repeat(32);
    let args3 = ["script", "adopt-version", "--script", "probe-sweep", "--version-digest", v3.as_str()];
    assert_eq!(run_cli(&human, &mut store, &programs, &mut log, &args3).unwrap_err().code(), "unknown_version");
    assert_eq!(log.0.len(), 1);
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
    let digest = ex.s("draft_digest");

    // S1 is human-authored; naming it with S2 adopts nothing.
    let steps = format!("{},{}", ex.step("S2"), ex.step("S1"));
    let e = run_cli(&human, &mut store, &programs, &mut log, &cli_args(&ex.s("program"), &steps, &digest)).unwrap_err();
    assert_eq!(e, AdoptError::NothingToAdopt { target: ex.step("S1").to_string() });

    let stranger = "019a0000-0009-7000-8000-00000000a009";
    let steps = format!("{},{stranger}", ex.step("S2"));
    let e = run_cli(&human, &mut store, &programs, &mut log, &cli_args(&ex.s("program"), &steps, &digest)).unwrap_err();
    assert_eq!(e, AdoptError::UnknownStep { step: stranger.to_string() });

    let other = "019a0000-0000-7000-8000-00000000b000";
    let e = run_cli(&human, &mut store, &programs, &mut log, &cli_args(other, &steps, &digest)).unwrap_err();
    assert_eq!(e.code(), "program_not_found");

    // A step named twice, both selections, neither: bad_params.
    let twice = format!("{},{}", ex.step("S2"), ex.step("S2"));
    let e = run_cli(&human, &mut store, &programs, &mut log, &cli_args(&ex.s("program"), &twice, &digest)).unwrap_err();
    assert_eq!(e.code(), "bad_params");
    let mut both = cli_args(&ex.s("program"), &twice, &digest);
    both.push("--all-unadopted");
    assert_eq!(run_cli(&human, &mut store, &programs, &mut log, &both).unwrap_err().code(), "bad_params");
    let prog = ex.s("program");
    let neither = ["program", "adopt-steps", "--program", prog.as_str(), "--reviewed-digest", digest.as_str()];
    assert_eq!(run_cli(&human, &mut store, &programs, &mut log, &neither).unwrap_err().code(), "bad_params");

    assert_eq!(store, before);
    assert!(log.0.is_empty());
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
    run_cli(&human, &mut live, &programs, &mut log, &cli_args(&ex.s("program"), &steps, &ex.s("draft_digest")))
        .unwrap();

    let r = replay(&start, &programs, &log.0);
    assert_eq!(r.outcome, ReplayOutcome::Identical);
    assert_eq!(r.steps, vec![ReplayOutcome::Identical]);
    assert_eq!(r.store, live);
    assert!(r.store.unadopted_steps(&p).is_empty());

    // A recorded event naming the human-authored S1 does not replay; garbage is not replayable.
    let forged = ex.s("ac1_event").replace(&ex.step("S2").to_string(), &ex.step("S1").to_string());
    let r = replay(&start, &programs, &[forged.into_bytes()]);
    assert_eq!(r.outcome, ReplayOutcome::Divergent);
    assert_eq!(r.store, start);
    let r = replay(&start, &programs, &[b"{\"command\":\"x\"}".to_vec()]);
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
    store.record_authorship(step(&p, ex.step("S1")), &service);
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
    let e = run_cli(
        &human,
        &mut store,
        &OneProgram(p),
        &mut Failing,
        &cli_args(&ex.s("program"), &steps, &ex.s("draft_digest")),
    )
    .unwrap_err();
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
                assert_eq!(p, fid_program_rec::adopt::UUID7_PATTERN, "test checker knows only the UUIDv7 pattern");
                Uuid7::parse(x).is_some()
            }
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
            (other, _) => return Err(format!("{at}: keyword {other} not supported by the test checker")),
        };
        if !ok {
            return Err(format!("{at}: fails {k}"));
        }
    }
    Ok(())
}
