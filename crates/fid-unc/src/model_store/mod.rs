//! The project store of Uncertainty Model versions (spec node `UNC.MODEL.GRAPH.STORE`,
//! REQ-UNC.MODEL.GRAPH.STORE-001 .. 005, REQ-UNC.MODEL.GRAPH-006, -007).
//!
//! - [`ModelStore`]: the index of stored versions of one journal branch, keyed by
//!   `(name, semver)` and by digest. Each entry is a [`StoredModel`]: the model reference
//!   (`um:<name>`, semver, digest), the author principal kind recorded at creation (read by
//!   `UNC.MODEL.BIND` for adoption), the journal entry that created it, the content-addressed
//!   blob holding its canonical bytes, and the digest of the kernel manifest it was validated
//!   against. The store is a derived view of the journal: an entry is added only after its
//!   `unc.create_model@1` event is committed, and nothing removes or replaces one.
//! - [`ModelBlobs`]: the blob seam. The model bytes are the canonical bytes of
//!   `fid-kunc` `model`, stored once in the CORE.STO.CAS blob store
//!   ([`fid_store::cas::BlobStore`] implements the seam), never edited in place.
//! - [`resolve`]: `resolve(ModelRef) -> ValidModel` for engines. It finds the stored version,
//!   reads its bytes back, checks they hash to the stored digest, and validates the model
//!   against the running kernel's release manifest, so a [`ValidModel`] from here always
//!   carries the running manifest digest. [`ensure_running_manifest`] is the check `UNC.PROP`
//!   runs on every `ValidModel` it is given.
//! - [`manifest`]: release-manifest provenance (AC-6). [`command`]: the three commands with
//!   their CLI and MCP surfaces. [`event`]: the journal event. [`mod@replay`]: exact replay.
//!
//! **Immutability** (REQ-004). A `(name, semver)` once stored always means the same digest:
//! creating it again with another digest is `version_conflict`, with the same digest it is an
//! idempotent success that stores and journals nothing. No command deletes or overwrites a
//! version, and the in-memory index has no removal method. Every read of a stored model
//! re-verifies the bytes against the digest (`digest_mismatch` otherwise).
//!
//! **Kernel manifest provenance** (AC-6). `unc.create_model@1` and `unc.validate_model@1`
//! validate only against the release manifest: the bytes the host's
//! [`ReleaseManifestSource`] returns, read with [`fid_kci::KernelManifest::parse_canonical`],
//! whose core digest must equal the core digest the running kernel reports through the
//! reserved query `kernel.manifest.core_digest@1.0.0` ([`RunningKernel`]). No command
//! parameter names, supplies or replaces a manifest. While the kernel reports no digest (the
//! committed constant of `fid-kernel-wasm` is `None` until a real release), both commands
//! refuse with `kernel_manifest_unset`; every check is fail-closed.
//!
//! No floating point is used in decisions (D0), maps are `BTreeMap`, and nothing here panics.

pub mod command;
pub mod event;
pub mod manifest;
pub mod replay;

pub use command::{
    cli_params, create_model, declarations, invoke_cli, invoke_mcp, mcp_params, show_model, validate_model,
    CreateModelParams, CreateModelResult, CreateOutcome, ModelFiles, ModelJournal, ShowModelParams, ShowModelResult,
    UncCall, UncCtx, ValidateModelParams, ValidateModelResult, CREATE_MODEL, SHOW_MODEL, VALIDATE_MODEL,
};
pub use event::CreateModelEvent;
pub use manifest::{bind_release_manifest, running_core_digest, ReleaseManifest, ReleaseManifestSource, RunningKernel};
pub use replay::{replay, ModelReplayed, RecordedEntry, ReplayedModels};

use fid_bus::principal::PrincipalKind;
use fid_canon::CValue;
use fid_kci::kci::sha256_digest;
use fid_kunc::model::{is_model_name, is_semver, validate, ModelError, UncertaintyModel, ValidModel};
use fid_kunc::ModelRef;
use fid_registry::FidSchema;
use fid_store::cas::{Address, BlobRef, BlobStore, ObjectRef, PutOpts, CHUNK_SIZE};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Longest text echoed back inside an error, in bytes.
const MAX_ECHO: usize = 160;

pub(crate) fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

// ------------------------------------------------------------------ author kind

/// The kind of principal that created a model version (`UNC.MODEL.BIND` reads it: a model
/// authored by an agent is a proposal until a person adopts its digest).
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthorKind {
    /// A person.
    Human,
    /// An AI agent.
    Agent,
    /// A script.
    Script,
}

impl AuthorKind {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            AuthorKind::Human => "human",
            AuthorKind::Agent => "agent",
            AuthorKind::Script => "script",
        }
    }

    /// Reads a wire name.
    pub fn parse(s: &str) -> Option<AuthorKind> {
        [AuthorKind::Human, AuthorKind::Agent, AuthorKind::Script].into_iter().find(|k| k.as_str() == s)
    }

    /// The author kind of a bus principal kind. Models are authored by people, agents and
    /// scripts only; a service, device or system principal is not an author (`None`), so its
    /// create call is refused.
    pub fn of(kind: PrincipalKind) -> Option<AuthorKind> {
        match kind {
            PrincipalKind::Human => Some(AuthorKind::Human),
            PrincipalKind::Agent => Some(AuthorKind::Agent),
            PrincipalKind::Script => Some(AuthorKind::Script),
            PrincipalKind::Service | PrincipalKind::Device | PrincipalKind::System => None,
        }
    }
}

// ------------------------------------------------------------------ errors

/// Every refusal of the store and its commands, each with a stable code. A refused command
/// commits nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Parameters break the command's schema or surface rules (unknown, repeated or missing
    /// keys, both or neither of `file` and `inline`, a malformed reference).
    BadParams { detail: &'static str },
    /// The class, author-kind or branch rule refuses the caller.
    PolicyRefused { reason: &'static str },
    /// The import handle could not be read.
    FileUnreadable { detail: String },
    /// The document is not an `uncertainty-model/v1` model (`fid-kunc` schema error).
    ModelUnreadable { code: &'static str, detail: String },
    /// The running kernel reports no manifest core digest: it is not a release build, so no
    /// model can be validated or stored.
    KernelManifestUnset,
    /// The running kernel could not answer the manifest query (KCI error code).
    KernelUnavailable { code: &'static str },
    /// The host could not read the release manifest file.
    KernelManifestUnavailable { detail: String },
    /// The release manifest is not a canonical kernel manifest (manifest error code).
    KernelManifestInvalid { code: &'static str },
    /// The release manifest's core digest differs from the running kernel's.
    KernelManifestMismatch { running: Digest, manifest: Digest },
    /// A `ValidModel` was checked against another manifest than the running one (`UNC.PROP`).
    ModelManifestStale { model: Digest, running: Digest },
    /// The model fails validation; every finding, in `fid-kunc` order.
    ModelInvalid { findings: Vec<String> },
    /// The `(name, semver)` is stored with another digest.
    VersionConflict { model: String, stored: Digest, given: Digest },
    /// No stored version matches the reference.
    ModelNotFound { model: String },
    /// A reference or the stored bytes disagree with the stored digest.
    DigestMismatch { expected: Digest, found: Digest },
    /// The blob store failed.
    BlobStore { detail: String },
    /// The journal did not commit the event; nothing was stored.
    Journal { detail: String },
    /// A broken invariant (a bug); nothing was committed.
    Internal { reason: &'static str },
}

impl StoreError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            StoreError::BadParams { .. } => "bad_params",
            StoreError::PolicyRefused { .. } => "policy_refused",
            StoreError::FileUnreadable { .. } => "file_unreadable",
            StoreError::ModelUnreadable { .. } => "model_unreadable",
            StoreError::KernelManifestUnset => "kernel_manifest_unset",
            StoreError::KernelUnavailable { .. } => "kernel_unavailable",
            StoreError::KernelManifestUnavailable { .. } => "kernel_manifest_unavailable",
            StoreError::KernelManifestInvalid { .. } => "kernel_manifest_invalid",
            StoreError::KernelManifestMismatch { .. } => "kernel_manifest_mismatch",
            StoreError::ModelManifestStale { .. } => "model_manifest_stale",
            StoreError::ModelInvalid { .. } => "model_invalid",
            StoreError::VersionConflict { .. } => "version_conflict",
            StoreError::ModelNotFound { .. } => "model_not_found",
            StoreError::DigestMismatch { .. } => "digest_mismatch",
            StoreError::BlobStore { .. } => "blob_store",
            StoreError::Journal { .. } => "journal_failed",
            StoreError::Internal { .. } => "internal",
        }
    }

    pub(crate) fn bad(detail: &'static str) -> StoreError {
        StoreError::BadParams { detail }
    }

    fn findings(errs: &[ModelError]) -> StoreError {
        StoreError::ModelInvalid { findings: errs.iter().map(ToString::to_string).collect() }
    }

    /// The error as JSON: `{"code", "message"}`, plus `findings` for `model_invalid`.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_owned(), CValue::Str(self.code().to_owned()));
        o.insert("message".to_owned(), CValue::Str(self.to_string()));
        if let StoreError::ModelInvalid { findings } = self {
            o.insert("findings".to_owned(), CValue::Arr(findings.iter().map(|f| CValue::Str(f.clone())).collect()));
        }
        CValue::Obj(o)
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            StoreError::BadParams { detail } => write!(f, "{code}: {detail}"),
            StoreError::PolicyRefused { reason } => write!(f, "{code}: {reason}"),
            StoreError::FileUnreadable { detail }
            | StoreError::KernelManifestUnavailable { detail }
            | StoreError::BlobStore { detail }
            | StoreError::Journal { detail } => write!(f, "{code}: {detail}"),
            StoreError::ModelUnreadable { detail, .. } => write!(f, "{code}: {detail}"),
            StoreError::KernelManifestUnset => {
                write!(f, "{code}: the running kernel reports no manifest core digest (not a release build)")
            }
            StoreError::KernelUnavailable { code: c } | StoreError::KernelManifestInvalid { code: c } => {
                write!(f, "{code}: {c}")
            }
            StoreError::KernelManifestMismatch { running, manifest } => {
                write!(f, "{code}: the running kernel reports core {running}, the release manifest has {manifest}")
            }
            StoreError::ModelManifestStale { model, running } => {
                write!(f, "{code}: the model was validated against manifest {model}, the running manifest is {running}")
            }
            StoreError::ModelInvalid { findings } => write!(f, "{code}: {} finding(s)", findings.len()),
            StoreError::VersionConflict { model, stored, given } => {
                write!(f, "{code}: {model} is stored as {stored}, not {given}")
            }
            StoreError::ModelNotFound { model } => write!(f, "{code}: {model}"),
            StoreError::DigestMismatch { expected, found } => write!(f, "{code}: expected {expected}, found {found}"),
            StoreError::Internal { reason } => write!(f, "{code}: {reason}"),
        }
    }
}

impl std::error::Error for StoreError {}

// ------------------------------------------------------------------ blobs

/// Where a model's canonical bytes are stored: the CORE.STO.CAS address (a blob's digest, or
/// for content above [`CHUNK_SIZE`] the digest of its chunk manifest) and the byte length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlobKey {
    /// The content address.
    pub address: Digest,
    /// Content length in bytes.
    pub size: u64,
}

/// The content-addressed blob seam. Implementations store bytes once under their content
/// address and never change them; `get` returns exactly the stored bytes or an error.
pub trait ModelBlobs {
    /// Stores `bytes` (idempotent) and returns where they are.
    fn put(&self, bytes: &[u8]) -> Result<BlobKey, String>;
    /// The bytes stored under `key`.
    fn get(&self, key: &BlobKey) -> Result<Vec<u8>, String>;
}

impl ModelBlobs for BlobStore {
    fn put(&self, bytes: &[u8]) -> Result<BlobKey, String> {
        let a = self.put_bytes(bytes, PutOpts { durable: true }).map_err(|e| e.code().to_owned())?;
        Ok(BlobKey { address: a.digest(), size: a.size() })
    }

    fn get(&self, key: &BlobKey) -> Result<Vec<u8>, String> {
        // CORE.STO.CAS: content up to CHUNK_SIZE is one blob, longer content an object.
        let addr = if key.size <= CHUNK_SIZE {
            Address::Blob(BlobRef { digest: key.address, size: key.size })
        } else {
            Address::Object(ObjectRef { manifest_digest: key.address, size: key.size })
        };
        BlobStore::get(self, &addr).map_err(|e| e.code().to_owned())
    }
}

// ------------------------------------------------------------------ the store

/// One stored model version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredModel {
    /// `um:<name>`, semver and digest.
    pub model_ref: ModelRef,
    /// Who created it.
    pub author_kind: AuthorKind,
    /// The sequence number of the journal entry that created it.
    pub created_entry: u64,
    /// Where its canonical bytes are.
    pub blob: BlobKey,
    /// Digest of the kernel manifest it was validated against at creation.
    pub kernel_manifest_digest: Digest,
}

impl StoredModel {
    /// The model name (without `um:`).
    pub fn name(&self) -> &str {
        self.model_ref.id.strip_prefix("um:").unwrap_or(&self.model_ref.id)
    }

    /// `um:<name>@<semver>`.
    pub fn id(&self) -> String {
        format!("{}@{}", self.model_ref.id, self.model_ref.semver)
    }
}

/// The stored model versions of one journal branch. Only a committed `unc.create_model@1`
/// adds to it; there is no way to remove or replace a version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelStore {
    versions: BTreeMap<(String, String), StoredModel>,
    by_digest: BTreeMap<Digest, (String, String)>,
}

impl ModelStore {
    /// An empty store.
    pub fn new() -> ModelStore {
        ModelStore::default()
    }

    /// Number of stored versions.
    pub fn len(&self) -> usize {
        self.versions.len()
    }

    /// Whether nothing is stored.
    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }

    /// The version `name@semver`, if stored.
    pub fn get(&self, name: &str, semver: &str) -> Option<&StoredModel> {
        self.versions.get(&(name.to_owned(), semver.to_owned()))
    }

    /// The version with this digest, if stored.
    pub fn by_digest(&self, digest: &Digest) -> Option<&StoredModel> {
        self.by_digest.get(digest).and_then(|k| self.versions.get(k))
    }

    /// Every stored version, by `(name, semver)`.
    pub fn iter(&self) -> impl Iterator<Item = &StoredModel> {
        self.versions.values()
    }

    /// The stored version a selector names. A name and semver that are not stored are
    /// `model_not_found`; with a digest that differs from the stored one, `digest_mismatch`.
    pub fn lookup(&self, selector: &ModelSelector) -> Result<&StoredModel, StoreError> {
        match selector {
            ModelSelector::Version { name, semver, digest } => {
                let stored = self
                    .get(name, semver)
                    .ok_or_else(|| StoreError::ModelNotFound { model: format!("um:{name}@{semver}") })?;
                match digest {
                    Some(d) if *d != stored.model_ref.digest => {
                        Err(StoreError::DigestMismatch { expected: *d, found: stored.model_ref.digest })
                    }
                    _ => Ok(stored),
                }
            }
            ModelSelector::Digest(d) => {
                self.by_digest(d).ok_or_else(|| StoreError::ModelNotFound { model: d.as_str().to_owned() })
            }
        }
    }

    /// Adds a version after its event was committed. A key or digest that is already
    /// stored is never overwritten (`internal`: the command checks before committing).
    pub(crate) fn insert(&mut self, stored: StoredModel) -> Result<(), StoreError> {
        let key = (stored.name().to_owned(), stored.model_ref.semver.clone());
        if self.versions.contains_key(&key) || self.by_digest.contains_key(&stored.model_ref.digest) {
            return Err(StoreError::Internal { reason: "a stored model version is never overwritten" });
        }
        self.by_digest.insert(stored.model_ref.digest, key.clone());
        self.versions.insert(key, stored);
        Ok(())
    }
}

// ------------------------------------------------------------------ references

/// What `unc.show_model@1` and [`ModelStore::lookup`] accept: `um:<name>@<semver>`,
/// optionally followed by `#sha256:<hex>` (the digest the caller expects), or a bare
/// `sha256:<hex>` digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSelector {
    /// By name and semver, with the expected digest if given.
    Version { name: String, semver: String, digest: Option<Digest> },
    /// By digest.
    Digest(Digest),
}

impl ModelSelector {
    /// Reads the text form (see the type docs); anything else is `bad_params`.
    pub fn parse(s: &str) -> Result<ModelSelector, StoreError> {
        if let Some(rest) = s.strip_prefix("um:") {
            let (version, digest) = match rest.split_once('#') {
                Some((v, d)) => {
                    (v, Some(Digest::parse(d).map_err(|_| StoreError::bad("reference digest is malformed"))?))
                }
                None => (rest, None),
            };
            let (name, semver) = version.split_once('@').ok_or(StoreError::bad("reference has no @<semver>"))?;
            if !is_model_name(name) || !is_semver(semver) {
                return Err(StoreError::bad("reference is not um:<name>@<semver>"));
            }
            return Ok(ModelSelector::Version { name: name.to_owned(), semver: semver.to_owned(), digest });
        }
        Digest::parse(s)
            .map(ModelSelector::Digest)
            .map_err(|_| StoreError::bad("reference is neither um:<name>@<semver>[#<digest>] nor a digest"))
    }

    /// The selector of a full model reference (`id` is `um:<name>`).
    pub fn from_model_ref(r: &ModelRef) -> Result<ModelSelector, StoreError> {
        let name = r.id.strip_prefix("um:").ok_or(StoreError::bad("model reference id is not um:<name>"))?;
        if !is_model_name(name) || !is_semver(&r.semver) {
            return Err(StoreError::bad("model reference is not um:<name>@<semver>"));
        }
        Ok(ModelSelector::Version { name: name.to_owned(), semver: r.semver.clone(), digest: Some(r.digest) })
    }
}

// ------------------------------------------------------------------ reading back

/// Reads a stored version's bytes back and re-verifies them: they must hash to the stored
/// digest and be the canonical form of a model with the stored name and semver; anything
/// else is `digest_mismatch` and no model is returned.
pub fn read_back(stored: &StoredModel, blobs: &dyn ModelBlobs) -> Result<UncertaintyModel, StoreError> {
    let expected = stored.model_ref.digest;
    let bytes = blobs.get(&stored.blob).map_err(|d| StoreError::BlobStore { detail: echo(&d) })?;
    let found = sha256_digest(&bytes);
    if found != expected {
        return Err(StoreError::DigestMismatch { expected, found });
    }
    let model = UncertaintyModel::parse(&bytes).map_err(|_| StoreError::DigestMismatch { expected, found })?;
    if model.digest() != expected
        || model.canonical_json() != bytes.as_slice()
        || model.name() != stored.name()
        || model.semver() != stored.model_ref.semver
    {
        return Err(StoreError::DigestMismatch { expected, found: model.digest() });
    }
    Ok(model)
}

/// `UNC.PROP`'s check on a model it is given: the manifest the model was validated against
/// must be the running release manifest (`model_manifest_stale` otherwise).
pub fn ensure_running_manifest(model: &ValidModel, running: &ReleaseManifest) -> Result<(), StoreError> {
    let (m, r) = (model.manifest_digest(), running.manifest_digest());
    if m == r {
        Ok(())
    } else {
        Err(StoreError::ModelManifestStale { model: m, running: r })
    }
}

/// `resolve(ModelRef) -> ValidModel` for engines (REQ-004): the stored version the reference
/// names (`model_not_found`), with the reference's digest (`digest_mismatch`), its bytes read
/// back and re-verified ([`read_back`]), validated against the running release manifest
/// (`model_invalid`). The result carries that manifest's digest.
pub fn resolve(
    store: &ModelStore,
    blobs: &dyn ModelBlobs,
    reference: &ModelRef,
    running: &ReleaseManifest,
) -> Result<ValidModel, StoreError> {
    let stored = store.lookup(&ModelSelector::from_model_ref(reference)?)?;
    let model = read_back(stored, blobs)?;
    let valid = validate(&model, running.manifest()).map_err(|e| StoreError::findings(&e))?;
    ensure_running_manifest(&valid, running)?;
    Ok(valid)
}
