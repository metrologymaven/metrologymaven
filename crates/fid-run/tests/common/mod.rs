//! Shared harness: the bracket program's lifecycle set through the real `AUTO.REC.LIFE`
//! commands by a person on `main`, bus-derived principals, the fixture catalogue, an input
//! source over `fixtures/scans/`, a counting run-ID source, and `expected.json` written by
//! the independent reference `fixtures/auto/run/bind/make_cases.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

use fid_bus::principal::{
    Channel, ChannelKind, ConfirmMethod, HumanPresence, Identity, JournalAppend, OsUser, PresenceOutcome, Principal,
    PrincipalId, PrincipalKind, SessionId, SessionKey, SessionTable, UpgradeClock, UpgradeContext, UpgradeGrant,
    UpgradeRequest,
};
use fid_bus::BusError;
use fid_canon::{parse, CValue, Digest as CanonDigest};
use fid_journal::{EntryDraft, EntryTime, Manifests};
use fid_program::{Program, Uuid7};
use fid_program_rec::adopt::{ProvenanceStore, ScriptId};
use fid_program_rec::life::{
    invoke_cli, CheckVerdict, Drafts, LifeCtx, LifeEvent, LifeJournal, LifeRequest, LifeRequests, LifeStore,
    ProgramChecker, ScriptBundle,
};
use fid_registry::Catalogue;
use fid_run::{prepare_run, BindCtx, BindingSource, InputSource, ProgramRef, RunError, RunFlags, RunIds, RunPlan};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAIN: &str = "main";

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

pub fn json(rel: &str) -> CValue {
    parse(&read(rel)).unwrap()
}

pub struct Expected(pub BTreeMap<String, CValue>);

impl Expected {
    pub fn load() -> Expected {
        let CValue::Obj(o) = json("fixtures/auto/run/bind/expected.json") else { panic!("object") };
        Expected(o)
    }
    pub fn s(&self, k: &str) -> String {
        match self.0.get(k) {
            Some(CValue::Str(s)) => s.clone(),
            other => panic!("{k}: {other:?}"),
        }
    }
    pub fn get(&self, k: &str) -> &CValue {
        self.0.get(k).unwrap_or_else(|| panic!("{k}"))
    }
}

pub fn bracket() -> Program {
    Program::parse(&read("fixtures/program/family/bracket.json")).unwrap()
}

pub fn catalogue() -> Catalogue {
    let CValue::Obj(o) = json("fixtures/program/family/expected.json") else { panic!("object") };
    let Some(CValue::Str(rel)) = o.get("catalogue") else { panic!("catalogue path") };
    Catalogue::from_json(&read(rel)).unwrap()
}

// ------------------------------------------------------------------ lifecycle seams

struct Project(Program);

impl Drafts for Project {
    fn program(&self, id: &Uuid7) -> Option<&Program> {
        (self.0.program_id == *id).then_some(&self.0)
    }
    fn script(&self, _id: &ScriptId) -> Option<&ScriptBundle> {
        None
    }
}

#[derive(Default)]
struct Log(u64);

impl LifeJournal for Log {
    fn commit(&mut self, _event: &LifeEvent) -> Result<u64, String> {
        self.0 += 1;
        Ok(100 + self.0)
    }
}

struct NoRequests;

impl LifeRequests for NoRequests {
    fn file(&mut self, _request: &LifeRequest) -> Result<String, String> {
        Err("no requests in these tests".into())
    }
}

struct Pass;

impl ProgramChecker for Pass {
    fn check(&self, _program: &Program) -> CheckVerdict {
        CheckVerdict::Passed
    }
}

/// The lifecycle of the bracket program on `main`, changed only through the real commands.
pub struct World {
    pub store: LifeStore,
    prov: ProvenanceStore,
    project: Project,
    log: Log,
    pub table: SessionTable,
    pub alice: Principal,
    pub agent: Principal,
}

impl World {
    pub fn new() -> World {
        let mut table = SessionTable::new();
        let alice = cli_session(&mut table, "alice");
        let agent = mcp_agent(&mut table, "agent-1");
        World {
            store: LifeStore::new(),
            prov: ProvenanceStore::new(),
            project: Project(bracket()),
            log: Log::default(),
            table,
            alice,
            agent,
        }
    }

    /// Runs `fid program <verb>` as alice on `main`.
    pub fn life(&mut self, verb: &str) {
        let p = bracket();
        let id = p.program_id.to_string();
        let d = p.digest().unwrap();
        let mut ctx = LifeCtx {
            principal: &self.alice,
            branch: MAIN,
            store: &mut self.store,
            drafts: &self.project,
            provenance: &mut self.prov,
            checker: &Pass,
            journal: &mut self.log,
            requests: &mut NoRequests,
        };
        invoke_cli(&mut ctx, &["program", verb, "--program", &id, "--expected-digest", d.as_str(), "--json"])
            .unwrap_or_else(|e| panic!("program {verb}: {e}"));
    }

    /// A world whose bracket program is in `state` (version 1).
    pub fn at(state: &str) -> World {
        let mut w = World::new();
        let verbs: &[&str] = match state {
            "draft" => &[],
            "registered" => &["register"],
            "released" => &["register", "release"],
            "deprecated" => &["register", "release", "deprecate"],
            other => panic!("{other}"),
        };
        for v in verbs {
            w.life(v);
        }
        w
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
    let session = table.open_session(SessionId::new(&format!("{}-{subject}", kind.as_str())).unwrap()).unwrap();
    TestChannel { kind, identity, session, grant: None }
}

pub fn mcp_agent(table: &mut SessionTable, subject: &str) -> Principal {
    let c = channel(table, ChannelKind::Mcp, subject);
    let p = table.derive_principal(&c).unwrap();
    assert_eq!(p.kind(), PrincipalKind::Agent);
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
        EntryTime { utc: "2026-11-21T10:00:00Z".into(), mono_ns: self.0, sync: "ntp".into() }
    }
}

struct NullJournal;

impl JournalAppend for NullJournal {
    fn append(&mut self, _draft: &EntryDraft) -> Result<(), BusError> {
        Ok(())
    }
}

/// A CLI session of `subject` upgraded to `human` (D-012).
pub fn cli_session(table: &mut SessionTable, subject: &str) -> Principal {
    let mut ch = channel(table, ChannelKind::Cli, subject);
    let pending = table.prepare_upgrade(&ch).unwrap();
    let confirmed = pending.confirm(&Confirms(OsUser::new(subject).unwrap()), &mut StepClock(1_000)).unwrap();
    let ctx = UpgradeContext {
        stream: "record".into(),
        branch: "main".into(),
        manifests: Manifests { registry: CanonDigest([0x11; 32]), kernel: CanonDigest([0x22; 32]) },
    };
    ch.grant = Some(table.commit_upgrade(confirmed, &mut NullJournal, &ctx).unwrap());
    let p = table.derive_principal(&ch).unwrap();
    assert_eq!(p.kind(), PrincipalKind::Human);
    p
}

// ------------------------------------------------------------------ run seams

/// `input:scans/<name>` → `fixtures/scans/<name>`; anything else is unreadable.
pub struct Scans;

impl InputSource for Scans {
    fn open(&self, _principal: &Principal, _param: &str, handle: &str) -> Result<Box<dyn Read + '_>, String> {
        let name = handle.strip_prefix("input:scans/").ok_or("unknown handle")?;
        if name.contains('/') || name.contains('\\') || name.starts_with('.') {
            return Err("unknown handle".into());
        }
        let f = std::fs::File::open(root().join("fixtures/scans").join(name)).map_err(|e| format!("{}", e.kind()))?;
        Ok(Box::new(f))
    }
}

/// The n-th ID (from 1): `Uuid7::from_parts(1_763_000_000_000 + n, [n; 10])`.
#[derive(Default)]
pub struct Counting(pub u8);

impl RunIds for Counting {
    fn next_run_id(&mut self) -> Result<Uuid7, String> {
        self.0 += 1;
        Ok(Uuid7::from_parts(1_763_000_000_000 + u64::from(self.0), [self.0; 10]))
    }
}

/// `prepare_run` on the world's lifecycle, with a fresh counting ID source.
pub fn prepare(
    w: &World,
    program: ProgramRef<'_>,
    binding: BindingSource,
    principal: &Principal,
    flags: RunFlags,
) -> Result<RunPlan, RunError> {
    let cat = catalogue();
    let mut ids = Counting::default();
    let mut ctx = BindCtx { lifecycle: &w.store, catalogue: &cat, inputs: &Scans, run_ids: &mut ids };
    prepare_run(&mut ctx, program, binding, principal, flags)
}

pub fn identified() -> ProgramRef<'static> {
    let p = bracket();
    ProgramRef::Identified { program_id: p.program_id, digest: p.digest().unwrap() }
}

pub fn b120() -> BindingSource {
    BindingSource::Variant("B-120".into())
}

pub fn digest(s: &str) -> Digest {
    Digest::parse(s).unwrap()
}
