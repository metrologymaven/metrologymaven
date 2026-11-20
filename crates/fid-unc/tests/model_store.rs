//! Acceptance criteria of spec node UNC.MODEL.GRAPH.STORE (AC-1 .. AC-6) and its fail-closed
//! rules, against the shared model fixtures `fixtures/unc/model/graph/` (`bore_ls.json`,
//! `bore_ls_param.json`, `bad/cycle.json`, `manifest_stub.json`) and the store fixtures
//! `fixtures/unc/model/graph/store/` written by the independent reference
//! `make_store_fixtures.py` (digests from hashlib, events, results and findings hand-labelled
//! from the spec).
//!
//! **Toy manifest, test code only.** No real kernel release exists yet, so the committed
//! kernel constant reports no manifest core digest and every production call refuses
//! (`kernel_manifest_unset`, tested first below). To exercise the commands, these tests pair
//! `store/toy_release_manifest.json` (kernel name `toy-test-only-not-a-release`) with a test
//! kernel table that reports the toy core digest. Nothing outside this file builds that table.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::principal::{
    Channel, ChannelKind, Identity, OsUser, Principal, PrincipalId, PrincipalKind, PrincipalRef, SessionId, SessionKey,
    SessionTable, UpgradeGrant,
};
use fid_canon::{parse, CValue};
use fid_kci::kci::sha256_digest;
use fid_kci::{AlgorithmTable, CardRegistry, KernelCall, KernelError, KernelManifest, KernelResult};
use fid_kunc::model::{validate, UncertaintyModel};
use fid_kunc::ModelRef;
use fid_registry::decl::CommandDecl;
use fid_registry::def::Projection;
use fid_replay::ReplayOutcome;
use fid_store::cas::BlobStore;
use fid_types::Digest;
use fid_unc::model_store::command::MAX_PARAMS_BYTES;
use fid_unc::model_store::event::BadEvent;
use fid_unc::model_store::{
    bind_release_manifest, cli_params, declarations, ensure_running_manifest, invoke_cli, invoke_mcp, mcp_params,
    read_back, replay, resolve, running_core_digest, BlobKey, CreateModelEvent, ModelBlobs, ModelFiles, ModelJournal,
    ModelSelector, ModelStore, RecordedEntry, ReleaseManifestSource, RunningKernel, StoreError, UncCall, UncCtx,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// ------------------------------------------------------------------ fixtures

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn graph(rel: &str) -> Vec<u8> {
    std::fs::read(root().join("fixtures/unc/model/graph").join(rel)).unwrap()
}

struct Expected(BTreeMap<String, CValue>);

impl Expected {
    fn load() -> Expected {
        let CValue::Obj(o) = parse(&graph("store/expected.json")).unwrap() else { panic!("expected.json") };
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

    fn strs(&self, k: &str) -> Vec<String> {
        match self.0.get(k) {
            Some(CValue::Arr(a)) => a
                .iter()
                .map(|x| match x {
                    CValue::Str(s) => s.clone(),
                    other => panic!("{other:?}"),
                })
                .collect(),
            other => panic!("{k}: {other:?}"),
        }
    }
}

const MAIN: &str = "main";
const AGENT_BRANCH: &str = "agent/claude-client/1";

// ------------------------------------------------------------------ seams

/// The release manifest file, as the host would read it.
struct Release(Vec<u8>);

impl ReleaseManifestSource for Release {
    fn release_manifest(&self) -> Result<Vec<u8>, String> {
        Ok(self.0.clone())
    }
}

/// A release manifest source that cannot be read.
struct NoRelease;

impl ReleaseManifestSource for NoRelease {
    fn release_manifest(&self) -> Result<Vec<u8>, String> {
        Err("no release manifest file".to_owned())
    }
}

fn toy() -> Release {
    Release(graph("store/toy_release_manifest.json"))
}

/// The kernel table of this build: no algorithms, and no manifest core digest because the
/// committed constant is `None` (asserted in `ac6_production_kernel_refuses_create_and_validate`).
fn production_kernel() -> AlgorithmTable {
    AlgorithmTable::new(Vec::new(), CardRegistry::default()).unwrap()
}

/// TEST ONLY: a kernel table that reports the toy manifest's core digest.
fn toy_kernel(ex: &Expected) -> AlgorithmTable {
    production_kernel().with_manifest_core_digest(ex.d("toy_core_digest"))
}

/// A kernel that answers every call with one error.
struct FailingKernel(KernelError);

impl RunningKernel for FailingKernel {
    fn evaluate(&self, _call: &KernelCall) -> Result<KernelResult, KernelError> {
        Err(self.0.clone())
    }
}

/// Import handles name files under `fixtures/unc/model/graph/`.
struct Files;

impl ModelFiles for Files {
    fn read(&self, handle: &str, limit: usize) -> Result<Vec<u8>, String> {
        if handle.contains("..") {
            return Err("handle leaves the fixture folder".to_owned());
        }
        let bytes = std::fs::read(root().join("fixtures/unc/model/graph").join(handle)).map_err(|e| e.to_string())?;
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

/// Blobs whose reads come back with one byte changed.
struct Tampered<'a>(&'a MemBlobs);

impl ModelBlobs for Tampered<'_> {
    fn put(&self, bytes: &[u8]) -> Result<BlobKey, String> {
        self.0.put(bytes)
    }

    fn get(&self, key: &BlobKey) -> Result<Vec<u8>, String> {
        let mut b = self.0.get(key)?;
        if let Some(x) = b.iter_mut().find(|x| **x == b'1') {
            *x = b'2';
        }
        Ok(b)
    }
}

/// The journal: committed event bytes with the principal kind of each entry's envelope.
#[derive(Default)]
struct Journal {
    entries: Vec<RecordedEntry>,
    kind: Option<PrincipalKind>,
    fail: bool,
}

impl ModelJournal for Journal {
    fn commit(&mut self, event: &CreateModelEvent) -> Result<u64, String> {
        if self.fail {
            return Err("disk full".to_owned());
        }
        let n = self.entries.len() as u64 + 1;
        let principal_kind = self.kind.unwrap();
        self.entries.push(RecordedEntry { journal_entry: n, principal_kind, event: event.to_canonical_bytes() });
        Ok(n)
    }
}

/// Everything a command reaches.
struct World<B: ModelBlobs> {
    store: ModelStore,
    blobs: B,
    kernel: Box<dyn RunningKernel>,
    release: Box<dyn ReleaseManifestSource>,
    journal: Journal,
}

impl<B: ModelBlobs> World<B> {
    fn new(blobs: B, kernel: Box<dyn RunningKernel>, release: Box<dyn ReleaseManifestSource>) -> World<B> {
        World { store: ModelStore::new(), blobs, kernel, release, journal: Journal::default() }
    }

    fn ctx<'a>(&'a mut self, principal: &'a Principal, branch: &'a str) -> UncCtx<'a> {
        self.journal.kind = Some(principal.kind());
        UncCtx {
            principal,
            branch,
            store: &mut self.store,
            blobs: &self.blobs,
            files: &Files,
            kernel: self.kernel.as_ref(),
            release_manifest: self.release.as_ref(),
            journal: &mut self.journal,
        }
    }

    fn cli(&mut self, principal: &Principal, branch: &str, args: &[&str]) -> Result<Vec<u8>, StoreError> {
        invoke_cli(&mut self.ctx(principal, branch), args)
    }

    fn mcp(&mut self, principal: &Principal, branch: &str, tool: &str, raw: &[u8]) -> Result<Vec<u8>, StoreError> {
        invoke_mcp(&mut self.ctx(principal, branch), tool, raw)
    }
}

impl World<MemBlobs> {
    fn toy(ex: &Expected) -> World<MemBlobs> {
        World::new(MemBlobs::default(), Box::new(toy_kernel(ex)), Box::new(toy()))
    }
}

fn create_args(file: &str) -> Vec<&str> {
    vec!["unc", "create-model", "--file", file, "--json"]
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

fn code(r: Result<Vec<u8>, StoreError>) -> &'static str {
    r.unwrap_err().code()
}

// ------------------------------------------------------------------ temp dir

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let name = format!("fid-unc-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::env::temp_dir().join(name);
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

// ------------------------------------------------------------------ AC-6 first: production refuses

#[test]
fn ac6_production_kernel_refuses_create_and_validate() {
    // The committed constant of the assembly crate holds no digest until a real release.
    let constant = std::fs::read_to_string(root().join("crates/fid-kernel-wasm/src/manifest_core_digest.rs")).unwrap();
    assert!(constant.contains("pub const MANIFEST_CORE_DIGEST: Option<&str> = None;"), "{constant}");
    assert_eq!(running_core_digest(&production_kernel()).unwrap_err(), StoreError::KernelManifestUnset);

    let ex = Expected::load();
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    // Even with the toy manifest file and with no manifest file at all: the kernel is asked
    // first, and it reports no digest.
    for release in [Box::new(toy()) as Box<dyn ReleaseManifestSource>, Box::new(NoRelease)] {
        let mut w = World::new(MemBlobs::default(), Box::new(production_kernel()), release);
        assert_eq!(code(w.cli(&a, MAIN, &create_args("bore_ls.json"))), "kernel_manifest_unset");
        let dry = ["unc", "create-model", "--file", "bore_ls.json", "--dry-run"];
        assert_eq!(code(w.cli(&a, MAIN, &dry)), "kernel_manifest_unset");
        let validate = ["unc", "validate-model", "--file", "bore_ls.json", "--json"];
        assert_eq!(code(w.cli(&a, MAIN, &validate)), "kernel_manifest_unset");
        assert!(w.store.is_empty() && w.journal.entries.is_empty() && w.blobs.0.borrow().is_empty());
    }
    let msg = StoreError::KernelManifestUnset.to_string();
    assert!(msg.contains("no manifest core digest"), "{msg}");
    // The toy digests are what the reference script computed.
    let toy_bytes = toy().0;
    let m = KernelManifest::parse_canonical(&toy_bytes).unwrap();
    assert_eq!(m.core().name, "toy-test-only-not-a-release");
    assert_eq!(*m.core_digest(), ex.d("toy_core_digest"));
    assert_eq!(m.manifest_digest(), ex.d("toy_manifest_digest"));
    assert_eq!(sha256_digest(&toy_bytes), ex.d("toy_manifest_digest"));
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_create_stores_in_the_blob_store_and_journals_one_event_then_commits_nothing_again() {
    let ex = Expected::load();
    let dir = TempDir::new("ac1");
    let blobs = BlobStore::open(&dir.0).unwrap();
    let mut w = World::new(blobs, Box::new(toy_kernel(&ex)), Box::new(toy()));
    let mut table = SessionTable::new();
    let a = alice(&mut table);

    let out = w.cli(&a, MAIN, &create_args("bore_ls.json")).unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), ex.s("ac1_result"));
    assert_eq!(w.journal.entries.len(), 1);
    assert_eq!(w.journal.entries[0].event, ex.b("ac1_event"));
    let stored = w.store.get("bore-ls", "1.0.0").unwrap().clone();
    assert_eq!(stored.model_ref.digest, ex.d("bore_ls_digest"));
    assert_eq!(stored.model_ref.id, "um:bore-ls");
    assert_eq!(stored.author_kind.as_str(), "human");
    assert_eq!(stored.created_entry, 1);
    assert_eq!(stored.kernel_manifest_digest, ex.d("toy_manifest_digest"));
    // CORE.STO.CAS: one blob addressed by the model digest.
    assert_eq!(stored.blob, BlobKey { address: ex.d("bore_ls_digest"), size: 4252 });
    assert!(w.blobs.has_blob(&ex.d("bore_ls_digest")).unwrap());
    assert_eq!(w.store.by_digest(&ex.d("bore_ls_digest")), Some(&stored));

    // Again: idempotent, nothing new committed or stored.
    let again = w.cli(&a, MAIN, &create_args("bore_ls.json")).unwrap();
    assert_eq!(String::from_utf8(again).unwrap(), ex.s("ac1_again_result"));
    // The shuffled spelling of the same model has the same digest: also nothing new.
    let shuffled = w.cli(&a, MAIN, &create_args("bore_ls_shuffled.json")).unwrap();
    assert_eq!(String::from_utf8(shuffled).unwrap(), ex.s("ac1_again_result"));
    assert_eq!(w.journal.entries.len(), 1);
    assert_eq!(w.store.len(), 1);

    // Read-back through the real blob store re-verifies the digest.
    let m = read_back(&stored, &w.blobs).unwrap();
    assert_eq!(m.digest(), ex.d("bore_ls_digest"));
}

#[test]
fn create_through_mcp_with_an_inline_document_gives_the_same_event() {
    let ex = Expected::load();
    let mut w = World::toy(&ex);
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let doc = String::from_utf8(graph("bore_ls.json")).unwrap();
    let mut o = BTreeMap::new();
    o.insert("inline".to_owned(), CValue::Str(doc));
    let raw = fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap();
    let out = w.mcp(&a, MAIN, "unc_create_model", &raw).unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), ex.s("ac1_result"));
    assert_eq!(w.journal.entries[0].event, ex.b("ac1_event"));
}

#[test]
fn dry_run_checks_everything_and_commits_nothing() {
    let ex = Expected::load();
    let mut w = World::toy(&ex);
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let out = w.cli(&a, MAIN, &["unc", "create-model", "--file", "bore_ls.json", "--dry-run", "--json"]).unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), ex.s("dry_run_result"));
    assert!(w.store.is_empty() && w.journal.entries.is_empty() && w.blobs.0.borrow().is_empty());
    // A dry run of an invalid model still fails.
    let bad = ["unc", "create-model", "--file", "bad/cycle.json", "--dry-run"];
    assert_eq!(code(w.cli(&a, MAIN, &bad)), "model_invalid");
}

// ------------------------------------------------------------------ AC-2, AC-3

#[test]
fn ac2_same_name_and_semver_with_another_digest_is_a_version_conflict() {
    let ex = Expected::load();
    let mut w = World::toy(&ex);
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    w.cli(&a, MAIN, &create_args("bore_ls.json")).unwrap();
    let before = (w.journal.entries.len(), w.store.clone(), w.blobs.0.borrow().len());

    let err = w.cli(&a, MAIN, &create_args("bore_ls_param.json")).unwrap_err();
    assert_eq!(
        err,
        StoreError::VersionConflict {
            model: "um:bore-ls@1.0.0".to_owned(),
            stored: ex.d("bore_ls_digest"),
            given: ex.d("bore_ls_param_digest"),
        }
    );
    // The journal head is unchanged and nothing was stored.
    assert_eq!((w.journal.entries.len(), w.store.clone(), w.blobs.0.borrow().len()), before);
    // A dry run reports the conflict too.
    let dry = ["unc", "create-model", "--file", "bore_ls_param.json", "--dry-run"];
    assert_eq!(code(w.cli(&a, MAIN, &dry)), "version_conflict");
}

#[test]
fn ac3_an_invalid_model_is_refused_with_every_finding_and_nothing_committed() {
    let ex = Expected::load();
    let mut w = World::toy(&ex);
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let err = w.cli(&a, MAIN, &create_args("bad/cycle.json")).unwrap_err();
    assert_eq!(err, StoreError::ModelInvalid { findings: ex.strs("ac3_findings") });
    assert!(w.store.is_empty() && w.journal.entries.is_empty() && w.blobs.0.borrow().is_empty());
    // The error JSON carries the findings.
    let v = err.to_value();
    let CValue::Obj(o) = v else { panic!() };
    assert_eq!(o.get("code"), Some(&CValue::Str("model_invalid".into())));
    assert_eq!(o.get("findings"), Some(&CValue::Arr(vec![CValue::Str("cycle n3 n3,n5,n7".into())])));
}

// ------------------------------------------------------------------ validate (REQ-005)

#[test]
fn validate_returns_digest_and_findings_and_stores_nothing() {
    let ex = Expected::load();
    let mut w = World::toy(&ex);
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let ok = w.cli(&a, MAIN, &["unc", "validate-model", "--file", "bore_ls.json", "--json"]).unwrap();
    assert_eq!(String::from_utf8(ok).unwrap(), ex.s("validate_bore_ls_result"));

    let bad = w.cli(&a, MAIN, &["unc", "validate-model", "--file", "bad/cycle.json"]).unwrap();
    let CValue::Obj(o) = parse(&bad).unwrap() else { panic!() };
    assert_eq!(o.get("valid"), Some(&CValue::Bool(false)));
    let want: Vec<CValue> = ex.strs("ac3_findings").into_iter().map(CValue::Str).collect();
    assert_eq!(o.get("findings"), Some(&CValue::Arr(want)));
    let cycle = UncertaintyModel::parse(&graph("bad/cycle.json")).unwrap();
    assert_eq!(o.get("digest"), Some(&CValue::Str(cycle.digest().as_str().to_owned())));
    assert_eq!(o.get("kernel_manifest_digest"), Some(&CValue::Str(ex.s("toy_manifest_digest"))));
    assert!(w.store.is_empty() && w.journal.entries.is_empty() && w.blobs.0.borrow().is_empty());

    // An unreadable document is an error, not a finding.
    let err = w.mcp(&a, MAIN, "unc_validate_model", br#"{"inline":"{\"schema\":\"x\"}"}"#).unwrap_err();
    assert_eq!(err.code(), "model_unreadable");
}

// ------------------------------------------------------------------ AC-4 and REQ-004

#[test]
fn ac4_show_returns_the_stored_model_and_refuses_a_wrong_digest() {
    let ex = Expected::load();
    let mut w = World::toy(&ex);
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    w.cli(&a, MAIN, &create_args("bore_ls.json")).unwrap();
    let show = |w: &mut World<MemBlobs>, r: &str| w.cli(&a, MAIN, &["unc", "show-model", "--ref", r, "--json"]);

    let out = show(&mut w, "um:bore-ls@1.0.0").unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), ex.s("ac4_show_result"));
    let with_digest = format!("um:bore-ls@1.0.0#{}", ex.s("bore_ls_digest"));
    assert_eq!(String::from_utf8(show(&mut w, &with_digest).unwrap()).unwrap(), ex.s("ac4_show_result"));
    assert_eq!(String::from_utf8(show(&mut w, &ex.s("bore_ls_digest")).unwrap()).unwrap(), ex.s("ac4_show_result"));

    let wrong = format!("um:bore-ls@1.0.0#{}", ex.s("bore_ls_param_digest"));
    assert_eq!(
        show(&mut w, &wrong).unwrap_err(),
        StoreError::DigestMismatch { expected: ex.d("bore_ls_param_digest"), found: ex.d("bore_ls_digest") }
    );
    assert_eq!(code(show(&mut w, "um:bore-ls@2.0.0")), "model_not_found");
    assert_eq!(code(show(&mut w, &ex.s("bore_ls_param_digest"))), "model_not_found");
    for bad in ["bore-ls@1.0.0", "um:Bore@1.0.0", "um:bore-ls@1.0", "um:bore-ls@1.0.0#sha256:00", "sha256:XY"] {
        assert_eq!(code(show(&mut w, bad)), "bad_params", "{bad}");
    }
    // MCP spelling of the same call.
    let raw = br#"{"ref":"um:bore-ls@1.0.0"}"#;
    assert_eq!(String::from_utf8(w.mcp(&a, MAIN, "unc_show_model", raw).unwrap()).unwrap(), ex.s("ac4_show_result"));
}

#[test]
fn read_back_re_verifies_the_digest_and_resolve_checks_the_reference() {
    let ex = Expected::load();
    let mut w = World::toy(&ex);
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    w.cli(&a, MAIN, &create_args("bore_ls.json")).unwrap();
    let stored = w.store.get("bore-ls", "1.0.0").unwrap().clone();

    // Bytes that changed under the address are refused, never returned.
    let err = read_back(&stored, &Tampered(&w.blobs)).unwrap_err();
    assert_eq!(err.code(), "digest_mismatch");
    let mut tampered = World {
        store: w.store.clone(),
        blobs: Tampered(&w.blobs),
        kernel: Box::new(toy_kernel(&ex)),
        release: Box::new(toy()),
        journal: Journal::default(),
    };
    assert_eq!(code(tampered.cli(&a, MAIN, &["unc", "show-model", "--ref", "um:bore-ls@1.0.0"])), "digest_mismatch");

    // resolve(ModelRef) -> ValidModel under the running release manifest.
    let release = bind_release_manifest(&toy(), &toy_kernel(&ex)).unwrap();
    let r = ModelRef { id: "um:bore-ls".into(), semver: "1.0.0".into(), digest: ex.d("bore_ls_digest") };
    let valid = resolve(&w.store, &w.blobs, &r, &release).unwrap();
    assert_eq!(valid.model().digest(), ex.d("bore_ls_digest"));
    assert_eq!(valid.manifest_digest(), ex.d("toy_manifest_digest"));
    let wrong = ModelRef { digest: ex.d("bore_ls_param_digest"), ..r.clone() };
    assert_eq!(resolve(&w.store, &w.blobs, &wrong, &release).unwrap_err().code(), "digest_mismatch");
    let missing = ModelRef { semver: "9.9.9".into(), ..r.clone() };
    assert_eq!(resolve(&w.store, &w.blobs, &missing, &release).unwrap_err().code(), "model_not_found");
    assert_eq!(resolve(&w.store, &Tampered(&w.blobs), &r, &release).unwrap_err().code(), "digest_mismatch");

    // Selectors.
    assert_eq!(
        ModelSelector::parse("um:bore-ls@1.0.0").unwrap(),
        ModelSelector::Version { name: "bore-ls".into(), semver: "1.0.0".into(), digest: None }
    );
    assert_eq!(
        ModelSelector::from_model_ref(&r).unwrap(),
        ModelSelector::parse(&format!("um:bore-ls@1.0.0#{}", r.digest)).unwrap()
    );
}

#[test]
fn no_command_deletes_or_overwrites_a_version() {
    let decls = declarations();
    let ids: Vec<&str> = decls.iter().map(|d| d.def.id.as_str()).collect();
    assert_eq!(ids, ["unc.create_model", "unc.validate_model", "unc.show_model"]);
    for d in &decls {
        let words = format!("{} {}", d.def.id, d.def.title).to_lowercase();
        for verb in ["delete", "remove", "overwrite", "replace", "update", "edit"] {
            assert!(!words.contains(verb), "{words}");
        }
    }
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_replay_of_the_journal_is_identical_and_rebuilds_the_store() {
    let ex = Expected::load();
    let mut w = World::toy(&ex);
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let agent = principal(&mut table, PrincipalKind::Agent, "claude-client");
    w.cli(&a, MAIN, &create_args("bore_ls.json")).unwrap();
    w.cli(&a, MAIN, &create_args("bore_ls.json")).unwrap();
    // A second version under another semver, by an agent on its branch.
    let doc =
        String::from_utf8(graph("bore_ls.json")).unwrap().replace("\"semver\": \"1.0.0\"", "\"semver\": \"1.1.0\"");
    assert!(doc.contains("\"1.1.0\""));
    let mut o = BTreeMap::new();
    o.insert("inline".to_owned(), CValue::Str(doc));
    let raw = fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap();
    w.mcp(&agent, AGENT_BRANCH, "unc_create_model", &raw).unwrap();
    assert_eq!(w.journal.entries.len(), 2);

    let release = bind_release_manifest(&toy(), &toy_kernel(&ex)).unwrap();
    let r = replay(&ModelStore::new(), &w.blobs, &release, &w.journal.entries);
    assert_eq!(r.outcome, ReplayOutcome::Identical);
    assert_eq!(r.steps, vec![ReplayOutcome::Identical; 2]);
    assert!(r.models.matches(&w.store));
    assert_eq!(r.models.len(), 2);

    // Tampered event bytes are not replayable; a forged principal kind diverges.
    let mut forged = w.journal.entries.clone();
    forged[0].event.push(b' ');
    assert_eq!(replay(&ModelStore::new(), &w.blobs, &release, &forged).outcome, ReplayOutcome::NotReplayable);
    let mut forged = w.journal.entries.clone();
    forged[0].principal_kind = PrincipalKind::Agent;
    assert_eq!(replay(&ModelStore::new(), &w.blobs, &release, &forged).outcome, ReplayOutcome::Divergent);
    // Replaying an entry onto a store that already holds it diverges (no overwrite).
    assert_eq!(replay(&w.store, &w.blobs, &release, &w.journal.entries[..1]).outcome, ReplayOutcome::Divergent);
    // A missing blob is not replayable.
    assert_eq!(
        replay(&ModelStore::new(), &MemBlobs::default(), &release, &w.journal.entries).outcome,
        ReplayOutcome::NotReplayable
    );
    // Under another kernel manifest the replay diverges.
    let stub = Release(graph("manifest_stub.json"));
    let stub_core = *KernelManifest::parse_canonical(&stub.0).unwrap().core_digest();
    let other = bind_release_manifest(&stub, &production_kernel().with_manifest_core_digest(stub_core)).unwrap();
    assert_eq!(replay(&ModelStore::new(), &w.blobs, &other, &w.journal.entries).outcome, ReplayOutcome::Divergent);
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_a_release_manifest_whose_core_differs_from_the_running_kernel_is_refused() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    // The kernel reports the CHECK stub's core digest; the release file is the toy manifest.
    let stub = graph("manifest_stub.json");
    let stub_core = *KernelManifest::parse_canonical(&stub).unwrap().core_digest();
    let mut w = World::new(
        MemBlobs::default(),
        Box::new(production_kernel().with_manifest_core_digest(stub_core)),
        Box::new(toy()),
    );
    let err = w.cli(&a, MAIN, &create_args("bore_ls.json")).unwrap_err();
    assert_eq!(err, StoreError::KernelManifestMismatch { running: stub_core, manifest: ex.d("toy_core_digest") });
    assert_eq!(code(w.cli(&a, MAIN, &["unc", "validate-model", "--file", "bore_ls.json"])), "kernel_manifest_mismatch");
    assert!(w.store.is_empty() && w.journal.entries.is_empty() && w.blobs.0.borrow().is_empty());

    // Manifest bytes that are not exactly canonical are refused by the strict reader.
    let mut spaced = toy().0;
    spaced.push(b'\n');
    let mut w = World::new(MemBlobs::default(), Box::new(toy_kernel(&ex)), Box::new(Release(spaced)));
    assert_eq!(
        w.cli(&a, MAIN, &create_args("bore_ls.json")).unwrap_err(),
        StoreError::KernelManifestInvalid { code: "non_canonical" }
    );
    // No manifest file.
    let mut w = World::new(MemBlobs::default(), Box::new(toy_kernel(&ex)), Box::new(NoRelease));
    assert_eq!(code(w.cli(&a, MAIN, &create_args("bore_ls.json"))), "kernel_manifest_unavailable");
    // A kernel that cannot answer.
    let mut w =
        World::new(MemBlobs::default(), Box::new(FailingKernel(KernelError::ManifestCoreDigestUnset)), Box::new(toy()));
    assert_eq!(code(w.cli(&a, MAIN, &create_args("bore_ls.json"))), "kernel_manifest_unset");
    let unknown = fid_kci::AlgorithmId::parse(fid_kci::MANIFEST_CORE_DIGEST_QUERY).unwrap();
    let mut w = World::new(
        MemBlobs::default(),
        Box::new(FailingKernel(KernelError::UnknownAlgorithm(unknown))),
        Box::new(toy()),
    );
    assert_eq!(
        w.cli(&a, MAIN, &create_args("bore_ls.json")).unwrap_err(),
        StoreError::KernelUnavailable { code: "unknown_algorithm" }
    );
    assert!(w.store.is_empty() && w.journal.entries.is_empty());
}

#[test]
fn ac6_no_command_parameter_can_supply_a_manifest() {
    let ex = Expected::load();
    let mut w = World::toy(&ex);
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let stub = String::from_utf8(graph("manifest_stub.json")).unwrap();
    for (tool, extra) in [
        ("unc_create_model", "manifest"),
        ("unc_validate_model", "manifest"),
        ("unc_create_model", "kernel_manifest_digest"),
        ("unc_validate_model", "kernel"),
        ("unc_create_model", "principal"),
        ("unc_create_model", "branch"),
    ] {
        let mut o = BTreeMap::new();
        o.insert("file".to_owned(), CValue::Str("bore_ls.json".into()));
        o.insert(extra.to_owned(), CValue::Str(stub.clone()));
        let raw = fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap();
        assert_eq!(code(w.mcp(&a, MAIN, tool, &raw)), "bad_params", "{tool} {extra}");
    }
    for flag in ["--manifest", "--kernel-manifest", "--release-manifest"] {
        let args = ["unc", "create-model", "--file", "bore_ls.json", flag, "x.json"];
        assert_eq!(code(w.cli(&a, MAIN, &args)), "bad_params", "{flag}");
    }
    assert!(w.store.is_empty() && w.journal.entries.is_empty());
    // The schemas declare no manifest member.
    for d in declarations() {
        let CValue::Obj(s) = &d.def.params_schema else { panic!() };
        let Some(CValue::Obj(props)) = s.get("properties") else { panic!() };
        assert!(props.keys().all(|k| ["dry_run", "file", "inline", "ref"].contains(&k.as_str())), "{props:?}");
        assert_eq!(s.get("additionalProperties"), Some(&CValue::Bool(false)));
    }
}

#[test]
fn ac6_prop_refuses_a_valid_model_checked_against_another_manifest() {
    let ex = Expected::load();
    let release = bind_release_manifest(&toy(), &toy_kernel(&ex)).unwrap();
    assert_eq!(release.manifest_digest(), ex.d("toy_manifest_digest"));
    assert_eq!(release.core_digest(), ex.d("toy_core_digest"));
    let model = UncertaintyModel::parse(&graph("bore_ls.json")).unwrap();

    let under_release = validate(&model, release.manifest()).unwrap();
    assert_eq!(ensure_running_manifest(&under_release, &release), Ok(()));

    let stub = KernelManifest::parse_canonical(&graph("manifest_stub.json")).unwrap();
    let under_stub = validate(&model, &stub).unwrap();
    assert_eq!(
        ensure_running_manifest(&under_stub, &release).unwrap_err(),
        StoreError::ModelManifestStale { model: stub.manifest_digest(), running: ex.d("toy_manifest_digest") }
    );
}

// ------------------------------------------------------------------ caller rules

#[test]
fn agents_create_on_their_branch_and_people_off_agent_branches() {
    let ex = Expected::load();
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let agent = principal(&mut table, PrincipalKind::Agent, "claude-client");

    let mut w = World::toy(&ex);
    assert_eq!(
        w.cli(&agent, MAIN, &create_args("bore_ls.json")).unwrap_err(),
        StoreError::PolicyRefused { reason: "not_agent_branch" }
    );
    assert_eq!(
        w.cli(&a, AGENT_BRANCH, &create_args("bore_ls.json")).unwrap_err(),
        StoreError::PolicyRefused { reason: "agent_branch" }
    );
    assert!(w.store.is_empty() && w.journal.entries.is_empty());
    let out = w.cli(&agent, AGENT_BRANCH, &create_args("bore_ls.json")).unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), ex.s("agent_result"));
    assert_eq!(w.journal.entries[0].event, ex.b("agent_event"));
    assert_eq!(w.store.get("bore-ls", "1.0.0").unwrap().author_kind.as_str(), "agent");

    // Services, devices and the system are not model authors.
    for kind in [PrincipalKind::Service, PrincipalKind::Device, PrincipalKind::System] {
        let p = principal(&mut table, kind, "svc");
        let mut w = World::toy(&ex);
        for branch in [MAIN, AGENT_BRANCH] {
            let err = w.cli(&p, branch, &create_args("bore_ls.json")).unwrap_err();
            assert_eq!(err.code(), "policy_refused", "{kind:?} {branch}");
        }
        assert!(w.store.is_empty() && w.journal.entries.is_empty());
    }
    // Reading needs no write branch.
    let mut w = World::toy(&ex);
    w.cli(&a, MAIN, &create_args("bore_ls.json")).unwrap();
    w.cli(&agent, MAIN, &["unc", "show-model", "--ref", "um:bore-ls@1.0.0"]).unwrap();
    w.cli(&agent, MAIN, &["unc", "validate-model", "--file", "bore_ls.json"]).unwrap();
}

#[test]
fn a_failed_journal_commit_stores_nothing() {
    let ex = Expected::load();
    let mut w = World::toy(&ex);
    w.journal.fail = true;
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    assert_eq!(code(w.cli(&a, MAIN, &create_args("bore_ls.json"))), "journal_failed");
    assert!(w.store.is_empty() && w.journal.entries.is_empty());
}

// ------------------------------------------------------------------ surfaces and schemas

#[test]
fn surface_readers_are_strict() {
    let ex = Expected::load();
    let mut w = World::toy(&ex);
    let mut table = SessionTable::new();
    let a = alice(&mut table);
    let doc = String::from_utf8(graph("bore_ls.json")).unwrap();
    let mut both = BTreeMap::new();
    both.insert("file".to_owned(), CValue::Str("bore_ls.json".into()));
    both.insert("inline".to_owned(), CValue::Str(doc));
    let raw = fid_canon::to_canonical_bytes(&CValue::Obj(both)).unwrap();
    assert_eq!(code(w.mcp(&a, MAIN, "unc_create_model", &raw)), "bad_params");
    for raw in [
        &b"{}"[..],
        br#"{"inline":""}"#,
        br#"{"file":1}"#,
        br#"{"dry_run":"yes","file":"bore_ls.json"}"#,
        b"[]",
        br#"{"file":"a","file":"b"}"#,
    ] {
        assert_eq!(code(w.mcp(&a, MAIN, "unc_create_model", raw)), "bad_params", "{}", String::from_utf8_lossy(raw));
    }
    assert_eq!(code(w.mcp(&a, MAIN, "unc_delete_model", b"{}")), "bad_params");
    assert_eq!(code(w.mcp(&a, MAIN, "unc_show_model", b"{}")), "bad_params");
    assert_eq!(code(w.mcp(&a, MAIN, "unc_validate_model", br#"{"dry_run":true,"file":"bore_ls.json"}"#)), "bad_params");
    for args in [
        &["unc", "create-model"][..],
        &["unc", "create-model", "--file"],
        &["unc", "create-model", "--file", "a", "--file", "b"],
        &["unc", "create-model", "--file", "a", "--json", "--json"],
        &["unc", "validate-model", "--file", "a", "--dry-run"],
        &["unc", "show-model", "--file", "a"],
        &["unc", "delete-model", "--ref", "um:bore-ls@1.0.0"],
        &["program", "create-model", "--file", "a"],
        &["unc"],
    ] {
        assert_eq!(code(w.cli(&a, MAIN, args)), "bad_params", "{args:?}");
    }
    assert_eq!(code(w.cli(&a, MAIN, &create_args("missing.json"))), "file_unreadable");
    assert!(w.store.is_empty() && w.journal.entries.is_empty());
    assert!(MAX_PARAMS_BYTES > 2 * fid_kunc::model::MAX_INPUT_BYTES);

    // Events are read strictly.
    let ev = CreateModelEvent::from_canonical_bytes(&ex.b("ac1_event")).unwrap();
    assert_eq!(ev.to_canonical_bytes(), ex.b("ac1_event"));
    let extra = ex.s("ac1_event").replacen('{', "{\"aa\":\"x\",", 1);
    assert!(CreateModelEvent::from_canonical_bytes(extra.as_bytes()).is_err());
    let spaced = ex.s("ac1_event").replacen(',', ", ", 1);
    assert_eq!(
        CreateModelEvent::from_canonical_bytes(spaced.as_bytes()),
        Err(BadEvent("not the canonical form of an event"))
    );
    let other = ex.s("ac1_event").replace("unc.create_model@1", "unc.delete_model@1");
    assert_eq!(CreateModelEvent::from_canonical_bytes(other.as_bytes()), Err(BadEvent("unknown command")));
}

#[test]
fn declarations_validate_and_calls_and_results_conform_to_their_schemas() {
    let ex = Expected::load();
    let decls = declarations();
    let got: Vec<(String, u32, String, bool, Projection, Vec<String>)> = decls
        .iter()
        .map(|d| {
            (
                d.def.id.clone(),
                d.def.major,
                d.def.side_effect.as_str().to_string(),
                d.def.human_only,
                d.def.effective_projection(),
                d.def.effects.clone(),
            )
        })
        .collect();
    let want = |id: &str, class: &str, effects: &[&str]| {
        (
            id.to_string(),
            1,
            class.to_string(),
            false,
            Projection::Exposed,
            effects.iter().map(|s| s.to_string()).collect(),
        )
    };
    assert_eq!(
        got,
        vec![
            want("unc.create_model", "W2", &["unc.create_model@1"]),
            want("unc.validate_model", "R1", &[]),
            want("unc.show_model", "R0", &[]),
        ]
    );
    let tools: Vec<String> = decls.iter().map(|d| fid_registry::tool_name(&d.def.id, d.def.major)).collect();
    assert_eq!(tools, ["unc_create_model", "unc_validate_model", "unc_show_model"]);
    let raw: Vec<_> = decls.iter().map(CommandDecl::to_validation).collect();
    assert_eq!(fid_registry::validate::validate(&raw), Vec::new());
    let catalogue = fid_registry::Catalogue::geo_frame().unwrap();
    for d in &decls {
        for schema in [&d.def.params_schema, &d.def.result_schema] {
            assert_eq!(fid_registry::check_schema(schema, &catalogue), Vec::new(), "{}", d.def.id);
        }
    }
    let schema = |id: &str| decls.iter().find(|d| d.def.id == id).unwrap().def.clone();

    // Every call the readers accept conforms to its params schema.
    let calls = [
        cli_params(&create_args("bore_ls.json")).unwrap(),
        cli_params(&["unc", "create-model", "--inline", "{}", "--dry-run"]).unwrap(),
        cli_params(&["unc", "validate-model", "--file", "bore_ls.json"]).unwrap(),
        cli_params(&["unc", "show-model", "--ref", "um:bore-ls@1.0.0", "--json"]).unwrap(),
        mcp_params("unc_create_model", br#"{"dry_run":false,"file":"h"}"#).unwrap(),
        mcp_params("unc_validate_model", br#"{"inline":"{}"}"#).unwrap(),
        mcp_params("unc_show_model", br#"{"ref":"sha256:00"}"#).unwrap(),
    ];
    for call in &calls {
        let id = call.command().trim_end_matches("@1");
        conforms(&schema(id).params_schema, &call.to_value(), "").unwrap();
    }
    assert_eq!(
        calls[4],
        UncCall::Create(fid_unc::model_store::CreateModelParams {
            file: Some("h".into()),
            inline: None,
            dry_run: Some(false)
        })
    );
    for bad in [r#"{"file":""}"#, r#"{"manifest":"x"}"#, r#"{"dry_run":"no"}"#] {
        assert!(
            conforms(&schema("unc.create_model").params_schema, &parse(bad.as_bytes()).unwrap(), "").is_err(),
            "{bad}"
        );
    }

    // Every result the fixtures label conforms to its result schema.
    for (id, key) in [
        ("unc.create_model", "ac1_result"),
        ("unc.create_model", "ac1_again_result"),
        ("unc.create_model", "dry_run_result"),
        ("unc.create_model", "agent_result"),
        ("unc.validate_model", "validate_bore_ls_result"),
        ("unc.show_model", "ac4_show_result"),
    ] {
        conforms(&schema(id).result_schema, &parse(&ex.b(key)).unwrap(), "").unwrap();
    }
    let bad = ex.s("ac1_result").replace("\"stored\"", "\"deleted\"");
    assert!(conforms(&schema("unc.create_model").result_schema, &parse(bad.as_bytes()).unwrap(), "").is_err());
    let bad = ex.s("ac1_result").replace(&ex.s("bore_ls_digest"), "sha256:00");
    assert!(conforms(&schema("unc.create_model").result_schema, &parse(bad.as_bytes()).unwrap(), "").is_err());
}

/// A JSON Schema subset checker for the keywords these schemas use. Unknown keywords fail.
fn conforms(schema: &CValue, v: &CValue, at: &str) -> Result<(), String> {
    let CValue::Obj(s) = schema else { return Err(format!("{at}: schema is not an object")) };
    for (k, kv) in s {
        let ok = match (k.as_str(), kv) {
            ("description" | "title" | "$schema" | "x-untrusted", _) => true,
            ("type", CValue::Str(t)) => matches!(
                (t.as_str(), v),
                ("object", CValue::Obj(_))
                    | ("array", CValue::Arr(_))
                    | ("string", CValue::Str(_))
                    | ("boolean", CValue::Bool(_))
            ),
            ("enum", CValue::Arr(e)) => e.contains(v),
            ("pattern", CValue::Str(p)) => {
                let CValue::Str(x) = v else { return Err(format!("{at}: pattern on a non-string")) };
                if p != Digest::PATTERN {
                    return Err(format!("{at}: unknown pattern {p}"));
                }
                Digest::is_valid(x)
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
            (other, _) => return Err(format!("{at}: unknown keyword {other}")),
        };
        if !ok {
            return Err(format!("{at}: fails {k}"));
        }
    }
    Ok(())
}
