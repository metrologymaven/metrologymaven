//! The commands of spec node UNC.INST.SCHEMA (AC-1, AC-2, AC-3 against a stand-in device,
//! AC-5, AC-6) and their fail-closed rules, against `fixtures/unc/inst/` written by the
//! independent reference `make_fixtures.py` (events, results and the evidence document
//! hand-labelled from the spec; digests from hashlib).
//!
//! **Stand-in device.** HAL.SIM is not built, so `sim-tracker-1` here is the stand-in record
//! under `fixtures/unc/inst/standin/` (an `instrument-profile/v1` descriptor that
//! `fid-hal-idl` validates, and a `DeviceInstance`). AC-3 asks for the HAL simulator's own
//! descriptor and stays open until that exists.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::principal::{
    Channel, ChannelKind, Identity, OsUser, Principal, PrincipalId, PrincipalKind, PrincipalRef, SessionId, SessionKey,
    SessionTable, UpgradeGrant,
};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, CValue};
use fid_hal_idl::DeviceInstance;
use fid_kci::kci::sha256_digest;
use fid_registry::def::Projection;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use fid_unc::model_store::{BlobKey, ModelBlobs, ModelFiles, RecordedEntry};
use fid_unc_inst::{
    cli_params, command_defs, declarations, invoke_cli, invoke_mcp, mcp_params, replay, BindTarget, DeviceDirectory,
    DeviceRecord, InstCall, InstCtx, InstError, InstEvent, InstJournal, InstrumentStore,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ fixtures

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/unc/inst")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(dir().join(rel)).unwrap()
}

struct Expected(BTreeMap<String, CValue>);

impl Expected {
    fn load() -> Expected {
        let CValue::Obj(o) = parse(&read("expected.json")).unwrap() else { panic!("expected.json") };
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

    fn d(&self, k: &str) -> Digest {
        Digest::parse(&self.s(k)).unwrap()
    }
}

const MAIN: &str = "main";
const AGENT_BRANCH: &str = "agent/claude-client/1";
const TRACKER: &str = "im:tracker-example@1.0.0";

// ------------------------------------------------------------------ seams

/// Import handles name files under `fixtures/unc/inst/`.
struct Files;

impl ModelFiles for Files {
    fn read(&self, handle: &str, limit: usize) -> Result<Vec<u8>, String> {
        if handle.contains("..") {
            return Err("handle leaves the fixture folder".to_owned());
        }
        let bytes = std::fs::read(dir().join(handle)).map_err(|e| e.to_string())?;
        if bytes.len() > limit {
            return Err("file too large".to_owned());
        }
        Ok(bytes)
    }
}

/// An in-memory content-addressed store.
#[derive(Default)]
struct MemBlobs(RefCell<BTreeMap<Digest, Vec<u8>>>);

impl ModelBlobs for MemBlobs {
    fn put(&self, bytes: &[u8]) -> Result<BlobKey, String> {
        let address = sha256_digest(bytes);
        self.0.borrow_mut().entry(address).or_insert_with(|| bytes.to_vec());
        Ok(BlobKey { address, size: bytes.len() as u64 })
    }

    fn get(&self, key: &BlobKey) -> Result<Vec<u8>, String> {
        self.0.borrow().get(&key.address).cloned().ok_or_else(|| "not found".to_owned())
    }
}

impl MemBlobs {
    fn len(&self) -> usize {
        self.0.borrow().len()
    }
}

/// The STAND-IN device directory: `sim-tracker-1` from `fixtures/unc/inst/standin/`.
struct StandIn {
    descriptor: Vec<u8>,
    fail: bool,
}

impl StandIn {
    fn new() -> StandIn {
        StandIn { descriptor: read("standin/sim-tracker-1.profile.json"), fail: false }
    }
}

impl DeviceDirectory for StandIn {
    fn device(&self, device: &str) -> Result<Option<DeviceRecord>, String> {
        if self.fail {
            return Err("directory offline".to_owned());
        }
        if device != "sim-tracker-1" {
            return Ok(None);
        }
        let instance = DeviceInstance::from_json(&read("standin/sim-tracker-1.instance.json")).unwrap();
        Ok(Some(DeviceRecord { instance, descriptor: self.descriptor.clone() }))
    }
}

/// The journal: committed event bytes with the principal kind of each entry's envelope.
#[derive(Default)]
struct Journal {
    entries: Vec<RecordedEntry>,
    kind: Option<PrincipalKind>,
    fail: bool,
}

impl InstJournal for Journal {
    fn commit(&mut self, event: &InstEvent) -> Result<u64, String> {
        if self.fail {
            return Err("disk full".to_owned());
        }
        let n = self.entries.len() as u64 + 1;
        let principal_kind = self.kind.unwrap();
        self.entries.push(RecordedEntry {
            journal_entry: n,
            principal_kind,
            event: event.to_canonical_bytes().unwrap(),
        });
        Ok(n)
    }
}

/// Everything a command reaches.
struct World {
    store: InstrumentStore,
    blobs: MemBlobs,
    devices: StandIn,
    journal: Journal,
}

impl World {
    fn new() -> World {
        World {
            store: InstrumentStore::new(),
            blobs: MemBlobs::default(),
            devices: StandIn::new(),
            journal: Journal::default(),
        }
    }

    fn ctx<'a>(&'a mut self, principal: &'a Principal, branch: &'a str) -> InstCtx<'a> {
        self.journal.kind = Some(principal.kind());
        InstCtx {
            principal,
            branch,
            store: &mut self.store,
            blobs: &self.blobs,
            files: &Files,
            devices: &self.devices,
            journal: &mut self.journal,
        }
    }

    fn cli(&mut self, principal: &Principal, branch: &str, args: &[&str]) -> Result<Vec<u8>, InstError> {
        invoke_cli(&mut self.ctx(principal, branch), args)
    }

    fn mcp(&mut self, principal: &Principal, branch: &str, tool: &str, raw: &[u8]) -> Result<Vec<u8>, InstError> {
        invoke_mcp(&mut self.ctx(principal, branch), tool, raw)
    }

    fn import(&mut self, p: &Principal, file: &str) -> Result<Vec<u8>, InstError> {
        self.cli(p, MAIN, &["unc", "import-instrument-model", "--file", file, "--json"])
    }

    fn bind_device(&mut self, p: &Principal) -> Result<Vec<u8>, InstError> {
        self.cli(p, MAIN, &["unc", "bind-instrument-model", "--model", TRACKER, "--device", "sim-tracker-1", "--json"])
    }
}

// ------------------------------------------------------------------ principals

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
    fn upgrade_grant(&self) -> Option<&UpgradeGrant> {
        None
    }
}

/// A principal of `kind` derived by the bus from a channel (the only way to build one).
fn principal(table: &mut SessionTable, kind: PrincipalKind, subject: &str) -> Principal {
    let ch = match kind {
        PrincipalKind::Human => ChannelKind::Gui,
        PrincipalKind::Agent => ChannelKind::Mcp,
        PrincipalKind::Script => ChannelKind::Cli,
        PrincipalKind::Service => ChannelKind::Service,
        PrincipalKind::Device => ChannelKind::Device,
        PrincipalKind::System => ChannelKind::Internal,
    };
    let mut identity = Identity::new(PrincipalId::new(subject).unwrap());
    identity.os_user = Some(OsUser::new(subject).unwrap());
    if matches!(ch, ChannelKind::ServerSession | ChannelKind::Internal) {
        identity.acting_for =
            Some(PrincipalRef { kind: PrincipalKind::Human, id: PrincipalId::new("trigger").unwrap() });
    }
    let session = table.open_session(SessionId::new(&format!("{}-{subject}", ch.as_str())).unwrap()).unwrap();
    let p = table.derive_principal(&TestChannel { kind: ch, identity, session }).unwrap();
    assert_eq!(p.kind(), kind);
    p
}

fn alice(table: &mut SessionTable) -> Principal {
    principal(table, PrincipalKind::Human, "alice")
}

fn code(r: Result<Vec<u8>, InstError>) -> &'static str {
    r.unwrap_err().code()
}

fn text(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn import_whose_journal_commit_fails_stores_nothing() {
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let mut w = World::new();
    w.journal.fail = true;
    assert_eq!(code(w.import(&a, "tracker_example.json")), "journal_failed");
    assert!(w.journal.entries.is_empty());
    assert!(w.store.model(TRACKER).is_none());
    // Nothing was committed, so the same import succeeds once the journal works.
    w.journal.fail = false;
    assert_eq!(text(&w.import(&a, "tracker_example.json").unwrap()), Expected::load().s("ac1_result"));
    assert_eq!(w.journal.entries.len(), 1);
}

#[test]
fn ac1_import_stores_the_reference_digest_and_journals_one_event() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let mut w = World::new();
    let out = w.import(&a, "tracker_example.json").unwrap();
    assert_eq!(text(&out), ex.s("ac1_result"));
    assert_eq!(w.journal.entries.len(), 1);
    assert_eq!(text(&w.journal.entries[0].event), ex.s("ac1_event"));
    let digest_line =
        text(&read("digests.txt")).lines().find(|l| l.starts_with("tracker_example.json ")).unwrap().to_owned();
    let stored = w.store.model(TRACKER).unwrap();
    assert_eq!(format!("tracker_example.json {}", stored.digest), digest_line);
    assert_eq!(stored.digest, ex.d("tracker_digest"));
    assert_eq!(stored.imported_entry, 1);
    assert_eq!(w.blobs.get(&stored.blob).unwrap(), read("tracker_example.canonical.json"));

    // The MCP tool gives the same bytes on a fresh world.
    let mut m = World::new();
    let mcp = m.mcp(&a, MAIN, "unc_import_instrument_model", br#"{"file":"tracker_example.json"}"#).unwrap();
    assert_eq!(mcp, out);
    assert_eq!(m.journal.entries[0].event, w.journal.entries[0].event);
}

#[test]
fn ac1_reimport_and_respelling_are_idempotent_and_dry_run_commits_nothing() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let mut dry = World::new();
    let out = dry.cli(&a, MAIN, &["unc", "import-instrument-model", "--file", "tracker_example.json", "--dry-run"]);
    assert_eq!(text(&out.unwrap()), ex.s("dry_run_result"));
    assert!(dry.journal.entries.is_empty() && dry.store.model(TRACKER).is_none() && dry.blobs.len() == 0);

    let mut w = World::new();
    w.import(&a, "tracker_example.json").unwrap();
    for file in ["tracker_example.json", "tracker_example_respelled.json"] {
        assert_eq!(text(&w.import(&a, file).unwrap()), ex.s("ac1_again_result"), "{file}");
    }
    assert_eq!(w.journal.entries.len(), 1);
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_invalid_files_fail_naming_the_field_and_commit_nothing() {
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let mut w = World::new();
    for (file, want_code, want_field) in [
        ("invalid/no_unit.json", "missing_unit", "range_const_sigma"),
        ("invalid/inverted_range.json", "invalid_range", "range"),
        ("invalid/no_source.json", "missing_source", "source"),
        ("invalid/bad_unit.json", "schema_invalid", "/params/0/unit"),
    ] {
        let err = w.import(&a, file).unwrap_err();
        assert_eq!(err.code(), want_code, "{file}");
        let CValue::Obj(o) = err.to_value() else { panic!() };
        assert_eq!(o.get("field"), Some(&CValue::Str(want_field.to_owned())), "{file}");
    }
    assert!(w.journal.entries.is_empty());
    assert!(w.store.models().next().is_none());
    assert_eq!(w.blobs.len(), 0);
    assert_eq!(code(w.import(&a, "missing.json")), "file_unreadable");
    assert_eq!(code(w.import(&a, "../../README.md")), "file_unreadable");
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_same_id_with_other_content_is_a_version_conflict() {
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let mut w = World::new();
    w.import(&a, "tracker_example.json").unwrap();
    let err = w.import(&a, "tracker_example_conflict.json").unwrap_err();
    assert_eq!(err.code(), "version_conflict");
    assert_eq!(w.journal.entries.len(), 1);
    assert_eq!(w.store.model(TRACKER).unwrap().digest, Expected::load().d("tracker_digest"));
}

// ------------------------------------------------------------------ show

#[test]
fn show_returns_the_reverified_canonical_model() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let mut w = World::new();
    assert_eq!(code(w.cli(&a, MAIN, &["unc", "show-instrument-model", "--id", TRACKER])), "model_not_found");
    w.import(&a, "tracker_example.json").unwrap();
    let out = w.cli(&a, MAIN, &["unc", "show-instrument-model", "--id", TRACKER, "--json"]).unwrap();
    assert_eq!(text(&out), ex.s("show_result"));
    let with_digest = format!("{TRACKER}#{}", ex.s("tracker_digest"));
    assert_eq!(
        w.mcp(&a, MAIN, "unc_show_instrument_model", format!("{{\"id\":\"{with_digest}\"}}").as_bytes()).unwrap(),
        out
    );
    let wrong = format!("{TRACKER}#{}", ex.s("descriptor_digest"));
    assert_eq!(code(w.cli(&a, MAIN, &["unc", "show-instrument-model", "--id", &wrong])), "digest_mismatch");
    assert_eq!(code(w.cli(&a, MAIN, &["unc", "show-instrument-model", "--id", "um:x@1.0.0"])), "bad_params");
    // Show is R0: it journals nothing.
    assert_eq!(w.journal.entries.len(), 1);
}

// ------------------------------------------------------------------ AC-3 (stand-in device)

#[test]
fn ac3_standin_bind_records_model_identity_and_descriptor_digests() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let mut w = World::new();
    w.import(&a, "tracker_example.json").unwrap();
    let out = w.bind_device(&a).unwrap();
    assert_eq!(text(&out), ex.s("ac3_result"));
    assert_eq!(w.journal.entries.len(), 2);
    assert_eq!(text(&w.journal.entries[1].event), ex.s("ac3_event"));
    let binding = w.store.binding(&BindTarget::Device("sim-tracker-1".into())).unwrap();
    assert_eq!(binding.model_digest, ex.d("tracker_digest"));
    let device = binding.device.unwrap();
    assert_eq!(device.descriptor_digest, ex.d("descriptor_digest"));
    assert_eq!(device.device_instance_digest, ex.d("device_instance_digest"));
    assert_eq!(device.evidence.address, ex.d("evidence_digest"));
    assert_eq!(w.blobs.get(&device.evidence).unwrap(), ex.b("evidence"));
    // The descriptor digest is the one fid-hal-idl computes.
    assert_eq!(
        fid_hal_idl::profile::digest_json(&read("standin/sim-tracker-1.profile.json")).unwrap(),
        ex.d("descriptor_digest")
    );

    // Again: nothing new.
    assert_eq!(text(&w.bind_device(&a).unwrap()), ex.s("ac3_again_result"));
    assert_eq!(w.journal.entries.len(), 2);

    // The project default.
    let out = w.cli(&a, MAIN, &["unc", "bind-instrument-model", "--model", TRACKER, "--project"]).unwrap();
    assert_eq!(text(&out), ex.s("project_result"));
    assert_eq!(text(&w.journal.entries[2].event), ex.s("project_event"));
    let mcp = br#"{"model":"im:tracker-example@1.0.0","project":true}"#;
    assert_eq!(w.mcp(&a, MAIN, "unc_bind_instrument_model", mcp).unwrap(), {
        let mut again = ex.b("project_result");
        let at = text(&again).find("\"bound\"").unwrap();
        again.splice(at..at + 7, b"\"already_bound\"".iter().copied());
        again
    });
}

#[test]
fn bind_refusals_commit_nothing() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let mut w = World::new();
    assert_eq!(code(w.bind_device(&a)), "model_not_found");
    w.import(&a, "tracker_example.json").unwrap();
    let bind = |w: &mut World, args: &[&str]| {
        let mut v = vec!["unc", "bind-instrument-model"];
        v.extend_from_slice(args);
        w.cli(&a, MAIN, &v)
    };
    assert_eq!(code(bind(&mut w, &["--model", TRACKER, "--device", "sim-tracker-9"])), "device_not_found");
    assert_eq!(code(bind(&mut w, &["--model", TRACKER])), "bad_params");
    assert_eq!(code(bind(&mut w, &["--model", TRACKER, "--device", "sim-tracker-1", "--project"])), "bad_params");
    assert_eq!(code(bind(&mut w, &["--model", TRACKER, "--device", "Bad/Id"])), "bad_params");
    let wrong = format!("{TRACKER}#{}", ex.s("descriptor_digest"));
    assert_eq!(code(bind(&mut w, &["--model", &wrong, "--device", "sim-tracker-1"])), "digest_mismatch");
    let dry = bind(&mut w, &["--model", TRACKER, "--device", "sim-tracker-1", "--dry-run"]).unwrap();
    assert!(text(&dry).contains("\"outcome\":\"dry_run\""));
    w.devices.fail = true;
    assert_eq!(code(w.bind_device(&a)), "device_unavailable");
    w.devices.fail = false;
    w.devices.descriptor = br#"{"schema":"instrument-profile/v1"}"#.to_vec();
    assert_eq!(code(w.bind_device(&a)), "descriptor_invalid");
    w.devices.descriptor = read("standin/sim-tracker-1.profile.json");
    w.journal.fail = true;
    assert_eq!(code(w.bind_device(&a)), "journal_failed");
    assert_eq!(w.journal.entries.len(), 1);
    assert!(w.store.bindings().next().is_none());
}

#[test]
fn caller_rules_follow_the_branch_policy() {
    let mut table = SessionTable::new();
    let agent = principal(&mut table, PrincipalKind::Agent, "claude");
    let service = principal(&mut table, PrincipalKind::Service, "svc");
    let a = alice(&mut table);
    let mut w = World::new();
    let args = ["unc", "import-instrument-model", "--file", "tracker_example.json"];
    assert_eq!(code(w.cli(&agent, MAIN, &args)), "policy_refused");
    assert_eq!(code(w.cli(&a, AGENT_BRANCH, &args)), "policy_refused");
    assert_eq!(code(w.cli(&service, MAIN, &args)), "policy_refused");
    let out = w.cli(&agent, AGENT_BRANCH, &args).unwrap();
    assert!(text(&out).contains("\"author_kind\":\"agent\""));
    let e = InstEvent::from_canonical_bytes(&w.journal.entries[0].event).unwrap();
    let InstEvent::Import(e) = e else { panic!() };
    assert_eq!((e.by.as_str(), e.branch.as_str()), ("claude", AGENT_BRANCH));
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_exact_replay_of_import_and_bind_is_identical() {
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let mut w = World::new();
    w.import(&a, "tracker_example.json").unwrap();
    w.bind_device(&a).unwrap();
    w.cli(&a, MAIN, &["unc", "bind-instrument-model", "--model", TRACKER, "--project"]).unwrap();
    let entries = w.journal.entries.clone();
    assert_eq!(entries.len(), 3);
    let r = replay(&InstrumentStore::new(), &w.blobs, &entries);
    assert_eq!(r.outcome, ReplayOutcome::Identical);
    assert_eq!(r.steps, vec![ReplayOutcome::Identical; 3]);
    assert!(r.store.matches(&w.store));
    let r = replay(&InstrumentStore::new(), &w.blobs, &entries[..2]);
    assert_eq!(r.outcome, ReplayOutcome::Identical);
    assert!(r.store.binding(&BindTarget::Device("sim-tracker-1".into())).is_some());
}

#[test]
fn replay_diverges_on_forged_or_missing_inputs() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let mut w = World::new();
    w.import(&a, "tracker_example.json").unwrap();
    w.bind_device(&a).unwrap();
    let entries = w.journal.entries.clone();
    let start = InstrumentStore::new();
    let run = |e: &[RecordedEntry], blobs: &MemBlobs| replay(&start, blobs, e).outcome;

    // A bind without its import: the model is unknown.
    assert_eq!(run(&entries[1..], &w.blobs), ReplayOutcome::Divergent);
    // The same import twice: the live command would have journaled nothing.
    assert_eq!(run(&[entries[0].clone(), entries[0].clone()], &w.blobs), ReplayOutcome::Divergent);
    // The same binding twice.
    assert_eq!(run(&[entries[0].clone(), entries[1].clone(), entries[1].clone()], &w.blobs), ReplayOutcome::Divergent);
    // The envelope says agent, the event says human.
    let mut forged = entries.clone();
    forged[0].principal_kind = PrincipalKind::Agent;
    assert_eq!(run(&forged, &w.blobs), ReplayOutcome::Divergent);
    // A recorded descriptor digest that the evidence does not give.
    let mut forged = entries.clone();
    let bad = text(&forged[1].event).replace(&ex.s("descriptor_digest"), &ex.s("device_instance_digest"));
    forged[1].event = bad.into_bytes();
    assert_eq!(run(&forged, &w.blobs), ReplayOutcome::Divergent);
    // A blob key the bytes do not have: replay derives the keys from the bytes read back.
    let model_size = ex.b("ac1_event").len(); // any wrong size; the right one is checked below
    let right_size = read("tracker_example.canonical.json").len();
    assert_ne!(model_size, right_size);
    let mut forged = entries.clone();
    let bad =
        text(&forged[0].event).replace(&format!("\"blob_size\":{right_size}"), &format!("\"blob_size\":{model_size}"));
    assert_ne!(bad.as_bytes(), forged[0].event.as_slice());
    forged[0].event = bad.into_bytes();
    assert_eq!(run(&forged, &w.blobs), ReplayOutcome::Divergent);
    let mut forged = entries.clone();
    let evidence_size = ex.b("evidence").len();
    let bad = text(&forged[1].event)
        .replace(&format!("\"evidence_size\":{evidence_size}"), &format!("\"evidence_size\":{}", evidence_size + 1));
    assert_ne!(bad.as_bytes(), forged[1].event.as_slice());
    forged[1].event = bad.into_bytes();
    assert_eq!(run(&forged, &w.blobs), ReplayOutcome::Divergent);
    // Not an event.
    let mut forged = entries.clone();
    forged[0].event = b"{}".to_vec();
    assert_eq!(run(&forged, &w.blobs), ReplayOutcome::NotReplayable);
    // The blobs are gone.
    assert_eq!(run(&entries, &MemBlobs::default()), ReplayOutcome::NotReplayable);
    // Evidence blob gone, model blob present.
    let only_model = MemBlobs::default();
    only_model.put(&read("tracker_example.canonical.json")).unwrap();
    let r = replay(&start, &only_model, &entries);
    assert_eq!(r.steps, vec![ReplayOutcome::Identical, ReplayOutcome::NotReplayable]);
}

#[test]
fn events_read_strictly() {
    let ex = Expected::load();
    for key in ["ac1_event", "ac3_event", "project_event"] {
        let bytes = ex.b(key);
        let e = InstEvent::from_canonical_bytes(&bytes).unwrap();
        assert_eq!(e.to_canonical_bytes().unwrap(), bytes, "{key}");
    }
    let extra = ex.s("ac1_event").replacen('{', "{\"extra\":1,", 1);
    assert!(InstEvent::from_canonical_bytes(extra.as_bytes()).is_err());
    let other = ex.s("ac1_event").replace("unc.import_instrument_model@1", "unc.delete_instrument_model@1");
    assert!(InstEvent::from_canonical_bytes(other.as_bytes()).is_err());
    let spaced = ex.s("ac1_event").replace(",", ", ");
    assert!(InstEvent::from_canonical_bytes(spaced.as_bytes()).is_err());
}

// ------------------------------------------------------------------ surfaces and schemas

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantity literals here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

#[test]
fn declarations_validate_and_calls_and_results_conform_to_their_schemas() {
    let ex = Expected::load();
    let decls = declarations();
    let got: Vec<(String, String, Projection, Vec<String>)> = decls
        .iter()
        .map(|d| {
            (
                d.def.id.clone(),
                d.def.side_effect.as_str().to_string(),
                d.def.effective_projection(),
                d.def.effects.clone(),
            )
        })
        .collect();
    let want = |id: &str, class: &str, effects: &[&str]| {
        (id.to_string(), class.to_string(), Projection::Exposed, effects.iter().map(|s| s.to_string()).collect())
    };
    assert_eq!(
        got,
        vec![
            want("unc.import_instrument_model", "W2", &["unc.import_instrument_model@1"]),
            want("unc.show_instrument_model", "R0", &[]),
            want("unc.bind_instrument_model", "W2", &["unc.bind_instrument_model@1"]),
        ]
    );
    assert!(decls.iter().all(|d| d.def.major == 1 && !d.def.human_only));
    assert_eq!(command_defs().unwrap().len(), 3);
    let tools: Vec<String> = decls.iter().map(|d| fid_registry::tool_name(&d.def.id, d.def.major)).collect();
    assert_eq!(tools, ["unc_import_instrument_model", "unc_show_instrument_model", "unc_bind_instrument_model"]);
    let raw: Vec<_> = decls.iter().map(fid_registry::decl::CommandDecl::to_validation).collect();
    assert_eq!(fid_registry::validate::validate(&raw), Vec::new());

    let def = |id: &str| decls.iter().find(|d| d.def.id == id).unwrap().def.clone();
    let params = |id: &str| Compiled::compile(&def(id).params_schema).unwrap();
    let results = |id: &str| Compiled::compile(&def(id).result_schema).unwrap();

    let calls = [
        cli_params(&["unc", "import-instrument-model", "--file", "tracker_example.json", "--json"]).unwrap(),
        cli_params(&["unc", "import-instrument-model", "--dry-run", "--file", "h"]).unwrap(),
        cli_params(&["unc", "show-instrument-model", "--id", TRACKER]).unwrap(),
        cli_params(&["unc", "bind-instrument-model", "--model", TRACKER, "--device", "sim-tracker-1"]).unwrap(),
        cli_params(&["unc", "bind-instrument-model", "--model", TRACKER, "--project", "--dry-run"]).unwrap(),
        mcp_params("unc_import_instrument_model", br#"{"dry_run":false,"file":"h"}"#).unwrap(),
        mcp_params("unc_show_instrument_model", br#"{"id":"im:a@1.0.0"}"#).unwrap(),
        mcp_params("unc_bind_instrument_model", br#"{"model":"im:a@1.0.0","project":true}"#).unwrap(),
    ];
    for call in &calls {
        let id = call.command().trim_end_matches("@1");
        if let Err(f) = params(id).evaluate(&call.to_value(), &NoHooks) {
            panic!("{id}: {} at {} ({})", f.keyword, f.pointer, f.message);
        }
    }
    assert!(matches!(&calls[3], InstCall::Bind(p) if p.device.as_deref() == Some("sim-tracker-1")));
    for bad in [r#"{"file":""}"#, r#"{"file":"h","principal":"x"}"#, r#"{"dry_run":"no","file":"h"}"#, "{}"] {
        let v = parse(bad.as_bytes()).unwrap();
        assert!(params("unc.import_instrument_model").evaluate(&v, &NoHooks).is_err(), "{bad}");
    }
    for (id, key) in [
        ("unc.import_instrument_model", "ac1_result"),
        ("unc.import_instrument_model", "ac1_again_result"),
        ("unc.import_instrument_model", "dry_run_result"),
        ("unc.show_instrument_model", "show_result"),
        ("unc.bind_instrument_model", "ac3_result"),
        ("unc.bind_instrument_model", "ac3_again_result"),
        ("unc.bind_instrument_model", "project_result"),
    ] {
        if let Err(f) = results(id).evaluate(&parse(&ex.b(key)).unwrap(), &NoHooks) {
            panic!("{key}: {} at {} ({})", f.keyword, f.pointer, f.message);
        }
    }
    // The dry-run results of both write commands conform too.
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let mut w = World::new();
    let dry = w.cli(&a, MAIN, &["unc", "import-instrument-model", "--file", "tracker_example.json", "--dry-run"]);
    let dry = parse(&dry.unwrap()).unwrap();
    assert!(results("unc.import_instrument_model").evaluate(&dry, &NoHooks).is_ok());
    w.import(&a, "tracker_example.json").unwrap();
    for args in [&["--device", "sim-tracker-1", "--dry-run"][..], &["--project", "--dry-run"]] {
        let mut v = vec!["unc", "bind-instrument-model", "--model", TRACKER];
        v.extend_from_slice(args);
        let out = parse(&w.cli(&a, MAIN, &v).unwrap()).unwrap();
        let CValue::Obj(o) = &out else { panic!() };
        assert_eq!(o.get("outcome"), Some(&CValue::Str("dry_run".into())), "{args:?}");
        if let Err(f) = results("unc.bind_instrument_model").evaluate(&out, &NoHooks) {
            panic!("bind dry run {args:?}: {} at {} ({})", f.keyword, f.pointer, f.message);
        }
    }
    assert_eq!(w.journal.entries.len(), 1);
    let bad = ex.s("ac1_result").replace("\"stored\"", "\"deleted\"");
    assert!(results("unc.import_instrument_model").evaluate(&parse(bad.as_bytes()).unwrap(), &NoHooks).is_err());
}

#[test]
fn surface_readers_refuse_undeclared_and_malformed_input() {
    for args in [
        &["unc"][..],
        &["geo", "import-instrument-model", "--file", "x"],
        &["unc", "import-instrument-model"],
        &["unc", "import-instrument-model", "--file"],
        &["unc", "import-instrument-model", "--file", "a", "--file", "b"],
        &["unc", "import-instrument-model", "--file", "a", "--json", "--json"],
        &["unc", "import-instrument-model", "--file", "a", "--device", "d"],
        &["unc", "show-instrument-model", "--model", TRACKER],
        &["unc", "bind-instrument-model", "--model", TRACKER, "--project", "--project"],
        &["unc", "bind-instrument-model", "--model", TRACKER, "stray"],
        &["unc", "delete-instrument-model", "--id", TRACKER],
    ] {
        assert_eq!(cli_params(args).unwrap_err().code(), "bad_params", "{args:?}");
    }
    for (tool, raw) in [
        ("unc_import_instrument_model", &br#"{"file":"a","principal":"alice"}"#[..]),
        ("unc_import_instrument_model", br#"{"file":"a","file":"b"}"#),
        ("unc_import_instrument_model", br#"{"file":1}"#),
        ("unc_bind_instrument_model", br#"{"model":"im:a@1.0.0","descriptor":"x"}"#),
        ("unc_bind_instrument_model", br#"{"model":"im:a@1.0.0","project":"yes"}"#),
        ("unc_delete_instrument_model", br#"{}"#),
        ("unc_show_instrument_model", br#"[]"#),
    ] {
        assert_eq!(mcp_params(tool, raw).unwrap_err().code(), "bad_params", "{tool}");
    }
}
