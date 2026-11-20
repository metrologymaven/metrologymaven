//! The three model commands, declared with the registry macros, with their CLI and MCP entry
//! points.
//!
//! | Command | Class | Journal | MCP |
//! |---|---|---|---|
//! | `unc.create_model@1` | W2 | one `unc.create_model@1` event per new version | `unc_create_model`, exposed |
//! | `unc.validate_model@1` | R1 | none | `unc_validate_model`, exposed |
//! | `unc.show_model@1` | R0 | none | `unc_show_model`, exposed |
//!
//! CLI: `fid unc create-model (--file <handle> | --inline <json>) [--dry-run] [--json]`,
//! `fid unc validate-model (--file <handle> | --inline <json>) [--json]`,
//! `fid unc show-model --ref <reference> [--json]`. Output is canonical JSON either way.
//!
//! **Create** (REQ-001 .. 003). In order, each step fail-closed and committing nothing on
//! failure: the bus class check of the declared definition, the author-kind rule (people,
//! agents and scripts author models), the branch rule (a person writes off agent branches,
//! an agent or script only on its agent branch `agent/<client>/<n>`); the release manifest
//! bound to the running kernel ([`super::manifest`]); exactly one of `file` and `inline`;
//! the document parsed into its canonical form ([`fid_kunc::model::UncertaintyModel`]);
//! validation against the release manifest (`model_invalid` with every finding); then the
//! version rule: a `(name, semver)` stored with the same digest is an idempotent success
//! (`already_stored`, nothing stored or journaled), with another digest `version_conflict`.
//! `dry_run` stops here (`dry_run`). Otherwise the canonical bytes are put in the
//! content-addressed store and read back (they must be identical), the event is committed
//! through [`ModelJournal`], and only then is the version added to the [`ModelStore`]. If the
//! commit fails, the store is unchanged; the content-addressed blob may remain, unreferenced.
//!
//! **Validate** (REQ-005) runs the class check, the manifest binding, the input and parsing
//! steps and validation, and returns the digest and the findings (empty when valid). It
//! stores and journals nothing.
//!
//! **Show** returns a stored version by reference (`um:<name>@<semver>`, optionally with
//! `#sha256:<hex>`) or by digest, with its bytes read back and re-verified
//! ([`super::read_back`]). It needs no kernel manifest.
//!
//! The principal kind and the branch come from the context the bus builds, never from a
//! parameter; the kernel manifest comes from the host's release-manifest seam, never from a
//! parameter. The readers refuse every key the schemas do not declare.

use super::event::CreateModelEvent;
use super::manifest::{bind_release_manifest, ReleaseManifest, ReleaseManifestSource, RunningKernel};
use super::{
    echo, ensure_running_manifest, read_back, AuthorKind, ModelBlobs, ModelSelector, ModelStore, StoreError,
    StoredModel,
};
use fid_bus::principal::Principal;
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_kci::kci::sha256_digest;
use fid_kunc::model::{validate, UncertaintyModel, MAX_INPUT_BYTES};
use fid_registry::{fid_command, fid_commands, CommandDef, FidSchema};
use fid_store::settings::is_agent_branch;
use fid_types::Digest;
use std::collections::BTreeMap;

/// `unc.create_model@1`.
pub const CREATE_MODEL: &str = "unc.create_model@1";
/// `unc.validate_model@1`.
pub const VALIDATE_MODEL: &str = "unc.validate_model@1";
/// `unc.show_model@1`.
pub const SHOW_MODEL: &str = "unc.show_model@1";

/// Longest import handle, in bytes.
pub const MAX_HANDLE_BYTES: usize = 4096;
/// Longest model reference, in bytes.
pub const MAX_REF_BYTES: usize = 256;
/// Largest MCP parameter payload, in bytes: an inline document as a JSON string (each byte at
/// most two) plus room for the other members.
pub const MAX_PARAMS_BYTES: usize = 2 * MAX_INPUT_BYTES + 8 * 1024;
const MAX_PARAMS_DEPTH: usize = 2;
/// Most CLI words.
const MAX_CLI_ARGS: usize = 8;

// ------------------------------------------------------------------ context

/// The import-handle seam the host wires (the import service). Returns the file's bytes, at
/// most `limit` of them (more is an error).
pub trait ModelFiles {
    /// Reads the file an import handle names.
    fn read(&self, handle: &str, limit: usize) -> Result<Vec<u8>, String>;
}

/// The journal seam the host wires (the Record Journal behind the bus). `Ok` holds the
/// sequence number of the committed, durable entry; `Err` means nothing was committed.
pub trait ModelJournal {
    /// Commits one event on the branch it names.
    fn commit(&mut self, event: &CreateModelEvent) -> Result<u64, String>;
}

/// The context every model command gets.
pub struct UncCtx<'a> {
    /// The bus-derived principal of the call.
    pub principal: &'a Principal,
    /// The branch the bus routed the call to.
    pub branch: &'a str,
    /// The model store of that branch.
    pub store: &'a mut ModelStore,
    /// The content-addressed blob store.
    pub blobs: &'a dyn ModelBlobs,
    /// The import-handle seam.
    pub files: &'a dyn ModelFiles,
    /// The running kernel (answers the manifest core digest query).
    pub kernel: &'a dyn RunningKernel,
    /// The release manifest file of the running kernel.
    pub release_manifest: &'a dyn ReleaseManifestSource,
    /// The journal seam.
    pub journal: &'a mut dyn ModelJournal,
}

// ------------------------------------------------------------------ params and results

const FILE_DOC: &str =
    "Import handle of the model document (uncertainty-model/v1). Give exactly one of file and inline.";
const INLINE_DOC: &str = "The model document (uncertainty-model/v1) as JSON text. Give exactly one of file and inline.";

/// Parameters of `unc.create_model@1`.
#[derive(FidSchema, Debug, Clone, Default, PartialEq, Eq)]
pub struct CreateModelParams {
    #[fid(description = FILE_DOC, min_length = 1, max_length = MAX_HANDLE_BYTES)]
    pub file: Option<String>,
    #[fid(description = INLINE_DOC, min_length = 1, max_length = MAX_INPUT_BYTES, untrusted = true)]
    pub inline: Option<String>,
    #[fid(description = "Run every check and report the outcome without storing or journaling anything.")]
    pub dry_run: Option<bool>,
}

/// Parameters of `unc.validate_model@1`.
#[derive(FidSchema, Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidateModelParams {
    #[fid(description = FILE_DOC, min_length = 1, max_length = MAX_HANDLE_BYTES)]
    pub file: Option<String>,
    #[fid(description = INLINE_DOC, min_length = 1, max_length = MAX_INPUT_BYTES, untrusted = true)]
    pub inline: Option<String>,
}

/// Parameters of `unc.show_model@1`.
#[derive(FidSchema, Debug, Clone, Default, PartialEq, Eq)]
pub struct ShowModelParams {
    #[fid(
        description = "The stored version: um:<name>@<semver>, optionally followed by #sha256:<hex> (the expected digest), or a bare sha256:<hex> digest.",
        min_length = 1,
        max_length = MAX_REF_BYTES
    )]
    pub r#ref: String,
}

/// What `unc.create_model@1` did.
#[derive(FidSchema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateOutcome {
    /// The version was stored and one event journaled.
    Stored,
    /// The same version (same digest) was already stored; nothing was committed.
    AlreadyStored,
    /// Dry run: every check passed; nothing was committed.
    DryRun,
}

impl CreateOutcome {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            CreateOutcome::Stored => "stored",
            CreateOutcome::AlreadyStored => "already_stored",
            CreateOutcome::DryRun => "dry_run",
        }
    }
}

/// Result of `unc.create_model@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct CreateModelResult {
    #[fid(description = "The model, um:<name>@<semver>.")]
    pub model: String,
    #[fid(description = "SHA-256 of the model's canonical bytes.", pattern = Digest::PATTERN)]
    pub digest: String,
    #[fid(description = "Who created the stored version (for a dry run: who would).")]
    pub author_kind: AuthorKind,
    #[fid(description = "What the command did.")]
    pub outcome: CreateOutcome,
}

impl CreateModelResult {
    /// The JSON value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("author_kind".to_owned(), text(self.author_kind.as_str()));
        o.insert("digest".to_owned(), text(&self.digest));
        o.insert("model".to_owned(), text(&self.model));
        o.insert("outcome".to_owned(), text(self.outcome.as_str()));
        CValue::Obj(o)
    }
}

/// Result of `unc.validate_model@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ValidateModelResult {
    #[fid(description = "The model, um:<name>@<semver>.")]
    pub model: String,
    #[fid(description = "SHA-256 of the model's canonical bytes.", pattern = Digest::PATTERN)]
    pub digest: String,
    #[fid(description = "True when there are no findings.")]
    pub valid: bool,
    #[fid(description = "Every validation finding, one line each, in the pinned pass order.")]
    pub findings: Vec<String>,
    #[fid(description = "Digest of the release kernel manifest validated against.", pattern = Digest::PATTERN)]
    pub kernel_manifest_digest: String,
}

impl ValidateModelResult {
    /// The JSON value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("digest".to_owned(), text(&self.digest));
        o.insert("findings".to_owned(), CValue::Arr(self.findings.iter().map(|f| text(f)).collect()));
        o.insert("kernel_manifest_digest".to_owned(), text(&self.kernel_manifest_digest));
        o.insert("model".to_owned(), text(&self.model));
        o.insert("valid".to_owned(), CValue::Bool(self.valid));
        CValue::Obj(o)
    }
}

/// Result of `unc.show_model@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct ShowModelResult {
    #[fid(description = "The model, um:<name>@<semver>.")]
    pub model: String,
    #[fid(description = "SHA-256 of the model's canonical bytes, re-verified on read.", pattern = Digest::PATTERN)]
    pub digest: String,
    #[fid(description = "Who created the version.")]
    pub author_kind: AuthorKind,
    #[fid(description = "Digest of the kernel manifest it was validated against at creation.", pattern = Digest::PATTERN)]
    pub kernel_manifest_digest: String,
    #[fid(description = "The stored canonical model document (RFC 8785 JSON text).")]
    pub canonical: String,
}

impl ShowModelResult {
    /// The JSON value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("author_kind".to_owned(), text(self.author_kind.as_str()));
        o.insert("canonical".to_owned(), text(&self.canonical));
        o.insert("digest".to_owned(), text(&self.digest));
        o.insert("kernel_manifest_digest".to_owned(), text(&self.kernel_manifest_digest));
        o.insert("model".to_owned(), text(&self.model));
        CValue::Obj(o)
    }
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn canonical(v: &CValue) -> Result<Vec<u8>, StoreError> {
    to_canonical_bytes(v).map_err(|_| StoreError::Internal { reason: "a result has no canonical form" })
}

// ------------------------------------------------------------------ shared checks

/// The class check for `def` (the declared definition of the running command).
fn authorise_class(principal: &Principal, def: &CommandDef) -> Result<(), StoreError> {
    fid_bus::policy::class::authorise_class(principal, def)
        .map_err(|e| StoreError::PolicyRefused { reason: e.reason() })
}

/// The caller rules of a write: class, author kind, branch.
fn authorise_write(principal: &Principal, branch: &str, def: &CommandDef) -> Result<AuthorKind, StoreError> {
    authorise_class(principal, def)?;
    let kind = AuthorKind::of(principal.kind()).ok_or(StoreError::PolicyRefused { reason: "author_kind" })?;
    branch_allowed(kind, branch)?;
    Ok(kind)
}

/// A person writes off agent branches; an agent or script only on an agent branch.
pub(crate) fn branch_allowed(kind: AuthorKind, branch: &str) -> Result<(), StoreError> {
    match (kind == AuthorKind::Human, is_agent_branch(branch)) {
        (true, true) => Err(StoreError::PolicyRefused { reason: "agent_branch" }),
        (false, false) => Err(StoreError::PolicyRefused { reason: "not_agent_branch" }),
        (true, false) | (false, true) => Ok(()),
    }
}

fn read_input(files: &dyn ModelFiles, file: Option<&str>, inline: Option<&str>) -> Result<Vec<u8>, StoreError> {
    match (file, inline) {
        (Some(h), None) => {
            if h.is_empty() || h.len() > MAX_HANDLE_BYTES {
                return Err(StoreError::bad("file must be 1 to 4096 bytes"));
            }
            files.read(h, MAX_INPUT_BYTES).map_err(|d| StoreError::FileUnreadable { detail: echo(&d) })
        }
        (None, Some(t)) if !t.is_empty() => Ok(t.as_bytes().to_vec()),
        (None, Some(_)) => Err(StoreError::bad("inline must not be empty")),
        _ => Err(StoreError::bad("give exactly one of file and inline")),
    }
}

fn parse_model(bytes: &[u8]) -> Result<UncertaintyModel, StoreError> {
    UncertaintyModel::parse(bytes)
        .map_err(|e| StoreError::ModelUnreadable { code: e.code(), detail: echo(&e.to_string()) })
}

/// What a create would do once its checks pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Plan {
    /// The version is stored with the same digest; its recorded author kind.
    AlreadyStored(AuthorKind),
    /// A new version.
    New,
}

/// Validation against the release manifest, then the version rule (REQ-002, REQ-003). Shared
/// with replay.
pub(crate) fn check_create(
    store: &ModelStore,
    model: &UncertaintyModel,
    release: &ReleaseManifest,
) -> Result<Plan, StoreError> {
    let valid = validate(model, release.manifest()).map_err(|e| StoreError::findings(&e))?;
    ensure_running_manifest(&valid, release)?;
    match store.get(model.name(), model.semver()) {
        Some(s) if s.model_ref.digest == model.digest() => Ok(Plan::AlreadyStored(s.author_kind)),
        Some(s) => {
            Err(StoreError::VersionConflict { model: model.id(), stored: s.model_ref.digest, given: model.digest() })
        }
        None => Ok(Plan::New),
    }
}

/// The stored entry of a committed event.
pub(crate) fn stored_from(e: &CreateModelEvent, entry: u64) -> StoredModel {
    StoredModel {
        model_ref: fid_kunc::ModelRef { id: format!("um:{}", e.name), semver: e.semver.clone(), digest: e.digest },
        author_kind: e.author_kind,
        created_entry: entry,
        blob: e.blob,
        kernel_manifest_digest: e.kernel_manifest_digest,
    }
}

/// Puts the canonical bytes in the blob store and reads them back; they must be identical.
pub(crate) fn put_verified(blobs: &dyn ModelBlobs, model: &UncertaintyModel) -> Result<super::BlobKey, StoreError> {
    let bytes = model.canonical_json();
    let key = blobs.put(bytes).map_err(|d| StoreError::BlobStore { detail: echo(&d) })?;
    let back = blobs.get(&key).map_err(|d| StoreError::BlobStore { detail: echo(&d) })?;
    if back.as_slice() != bytes || key.size != bytes.len() as u64 {
        return Err(StoreError::DigestMismatch { expected: model.digest(), found: sha256_digest(&back) });
    }
    Ok(key)
}

fn create(
    ctx: &mut UncCtx<'_>,
    file: Option<&str>,
    inline: Option<&str>,
    dry_run: bool,
) -> Result<CreateModelResult, StoreError> {
    let author = authorise_write(ctx.principal, ctx.branch, &__fid_command_create_model().def)?;
    let release = bind_release_manifest(ctx.release_manifest, ctx.kernel)?;
    let bytes = read_input(ctx.files, file, inline)?;
    let model = parse_model(&bytes)?;
    let result = |author_kind: AuthorKind, outcome: CreateOutcome| CreateModelResult {
        model: model.id(),
        digest: model.digest().as_str().to_owned(),
        author_kind,
        outcome,
    };
    match check_create(ctx.store, &model, &release)? {
        Plan::AlreadyStored(kind) => return Ok(result(kind, CreateOutcome::AlreadyStored)),
        Plan::New => {}
    }
    if dry_run {
        return Ok(result(author, CreateOutcome::DryRun));
    }
    let blob = put_verified(ctx.blobs, &model)?;
    let event = CreateModelEvent {
        name: model.name().to_owned(),
        semver: model.semver().to_owned(),
        digest: model.digest(),
        blob,
        author_kind: author,
        by: ctx.principal.id().as_str().to_owned(),
        branch: ctx.branch.to_owned(),
        kernel_manifest_digest: release.manifest_digest(),
    };
    let entry = ctx.journal.commit(&event).map_err(|d| StoreError::Journal { detail: echo(&d) })?;
    ctx.store.insert(stored_from(&event, entry))?;
    Ok(result(author, CreateOutcome::Stored))
}

fn validate_doc(
    ctx: &mut UncCtx<'_>,
    file: Option<&str>,
    inline: Option<&str>,
) -> Result<ValidateModelResult, StoreError> {
    authorise_class(ctx.principal, &__fid_command_validate_model().def)?;
    let release = bind_release_manifest(ctx.release_manifest, ctx.kernel)?;
    let bytes = read_input(ctx.files, file, inline)?;
    let model = parse_model(&bytes)?;
    let findings = match validate(&model, release.manifest()) {
        Ok(valid) => {
            ensure_running_manifest(&valid, &release)?;
            Vec::new()
        }
        Err(errs) => errs.iter().map(ToString::to_string).collect(),
    };
    Ok(ValidateModelResult {
        model: model.id(),
        digest: model.digest().as_str().to_owned(),
        valid: findings.is_empty(),
        findings,
        kernel_manifest_digest: release.manifest_digest().as_str().to_owned(),
    })
}

fn show(ctx: &mut UncCtx<'_>, reference: &str) -> Result<ShowModelResult, StoreError> {
    authorise_class(ctx.principal, &__fid_command_show_model().def)?;
    if reference.is_empty() || reference.len() > MAX_REF_BYTES {
        return Err(StoreError::bad("ref must be 1 to 256 bytes"));
    }
    let stored = ctx.store.lookup(&ModelSelector::parse(reference)?)?;
    let model = read_back(stored, ctx.blobs)?;
    let canonical = String::from_utf8(model.canonical_json().to_vec())
        .map_err(|_| StoreError::Internal { reason: "canonical model bytes are not UTF-8" })?;
    Ok(ShowModelResult {
        model: model.id(),
        digest: model.digest().as_str().to_owned(),
        author_kind: stored.author_kind,
        kernel_manifest_digest: stored.kernel_manifest_digest.as_str().to_owned(),
        canonical,
    })
}

// ------------------------------------------------------------------ the commands

#[fid_command(
    id = "unc.create_model",
    major = 1,
    title = "Create model",
    description = "Validates an Uncertainty Model against the release kernel manifest of the running kernel and stores it immutably under its name, semver and digest, recording the author principal kind. The same name and semver with another digest is a version conflict; with the same digest nothing new is stored. Agents create models on their agent branch.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = ReadRoot,
    undo = HeadMove,
    preconditions = [],
    effects = [CREATE_MODEL],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-20"),
    web = Available,
)]
pub fn create_model(ctx: &mut UncCtx<'_>, params: CreateModelParams) -> Result<CreateModelResult, StoreError> {
    create(ctx, params.file.as_deref(), params.inline.as_deref(), params.dry_run.unwrap_or(false))
}

#[fid_command(
    id = "unc.validate_model",
    major = 1,
    title = "Validate model",
    description = "Validates an Uncertainty Model against the release kernel manifest of the running kernel and returns its digest and every finding, without storing anything.",
    side_effect = R1,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = ReadRoot,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-20"),
    web = Available,
)]
pub fn validate_model(ctx: &mut UncCtx<'_>, params: ValidateModelParams) -> Result<ValidateModelResult, StoreError> {
    validate_doc(ctx, params.file.as_deref(), params.inline.as_deref())
}

#[fid_command(
    id = "unc.show_model",
    major = 1,
    title = "Show model",
    description = "Returns a stored Uncertainty Model version by reference or digest, with its bytes re-verified against the stored digest. A reference whose digest differs from the stored one is a digest mismatch.",
    side_effect = R0,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-20"),
    web = Available,
)]
pub fn show_model(ctx: &mut UncCtx<'_>, params: ShowModelParams) -> Result<ShowModelResult, StoreError> {
    show(ctx, &params.r#ref)
}

fid_commands![create_model, validate_model, show_model];

// ------------------------------------------------------------------ parameter readers

/// One parsed call of a model command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UncCall {
    /// `unc.create_model@1`.
    Create(CreateModelParams),
    /// `unc.validate_model@1`.
    Validate(ValidateModelParams),
    /// `unc.show_model@1`.
    Show(ShowModelParams),
}

impl UncCall {
    /// The command ID with major.
    pub fn command(&self) -> &'static str {
        match self {
            UncCall::Create(_) => CREATE_MODEL,
            UncCall::Validate(_) => VALIDATE_MODEL,
            UncCall::Show(_) => SHOW_MODEL,
        }
    }

    /// The parameter object (the shape the command's schema describes).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        let (file, inline) = match self {
            UncCall::Create(p) => {
                if let Some(d) = p.dry_run {
                    o.insert("dry_run".to_owned(), CValue::Bool(d));
                }
                (&p.file, &p.inline)
            }
            UncCall::Validate(p) => (&p.file, &p.inline),
            UncCall::Show(p) => {
                o.insert("ref".to_owned(), text(&p.r#ref));
                return CValue::Obj(o);
            }
        };
        if let Some(f) = file {
            o.insert("file".to_owned(), text(f));
        }
        if let Some(i) = inline {
            o.insert("inline".to_owned(), text(i));
        }
        CValue::Obj(o)
    }
}

/// Which command a tool or subcommand names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    Create,
    Validate,
    Show,
}

fn call_from(which: Which, o: &BTreeMap<String, CValue>) -> Result<UncCall, StoreError> {
    let allowed: &[&str] = match which {
        Which::Create => &["dry_run", "file", "inline"],
        Which::Validate => &["file", "inline"],
        Which::Show => &["ref"],
    };
    if o.keys().any(|k| !allowed.contains(&k.as_str())) {
        // A `manifest`, `principal`, `branch` or any other undeclared key lands here.
        return Err(StoreError::bad("unknown parameter"));
    }
    let s = |k: &str| match o.get(k) {
        None => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(StoreError::bad("parameter is not a string")),
    };
    Ok(match which {
        Which::Create => {
            let dry_run = match o.get("dry_run") {
                None => None,
                Some(CValue::Bool(b)) => Some(*b),
                Some(_) => return Err(StoreError::bad("dry_run is not a boolean")),
            };
            UncCall::Create(CreateModelParams { file: s("file")?, inline: s("inline")?, dry_run })
        }
        Which::Validate => UncCall::Validate(ValidateModelParams { file: s("file")?, inline: s("inline")? }),
        Which::Show => UncCall::Show(ShowModelParams { r#ref: s("ref")?.ok_or(StoreError::bad("missing ref"))? }),
    })
}

/// Reads MCP parameters for `tool` (`unc_create_model`, `unc_validate_model`,
/// `unc_show_model`) strictly: at most [`MAX_PARAMS_BYTES`] bytes, duplicate keys refused, and
/// any key the schema does not declare is `bad_params`.
pub fn mcp_params(tool: &str, raw: &[u8]) -> Result<UncCall, StoreError> {
    let which = match tool {
        "unc_create_model" => Which::Create,
        "unc_validate_model" => Which::Validate,
        "unc_show_model" => Which::Show,
        _ => return Err(StoreError::bad("unknown tool")),
    };
    let v = parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|_| StoreError::bad("params are not JSON within the limits"))?;
    let CValue::Obj(o) = v else { return Err(StoreError::bad("params is not an object")) };
    call_from(which, &o)
}

/// Reads `fid <args>`: `unc create-model (--file <H> | --inline <JSON>) [--dry-run] [--json]`,
/// `unc validate-model (--file <H> | --inline <JSON>) [--json]`, `unc show-model --ref <R>
/// [--json]`. Unknown, repeated or value-less options and stray words are `bad_params`.
pub fn cli_params(args: &[&str]) -> Result<UncCall, StoreError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(StoreError::bad("too many arguments"));
    }
    let [group, sub, rest @ ..] = args else { return Err(StoreError::bad("missing subcommand")) };
    if *group != "unc" {
        return Err(StoreError::bad("unknown command group"));
    }
    let which = match *sub {
        "create-model" => Which::Create,
        "validate-model" => Which::Validate,
        "show-model" => Which::Show,
        _ => return Err(StoreError::bad("unknown subcommand")),
    };
    let mut o = BTreeMap::new();
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let key = match (*arg, which) {
            ("--json", _) if !json => {
                json = true;
                continue;
            }
            ("--dry-run", Which::Create) => {
                if o.insert("dry_run".to_owned(), CValue::Bool(true)).is_some() {
                    return Err(StoreError::bad("repeated option"));
                }
                continue;
            }
            ("--file", Which::Create | Which::Validate) => "file",
            ("--inline", Which::Create | Which::Validate) => "inline",
            ("--ref", Which::Show) => "ref",
            _ => return Err(StoreError::bad("unknown or repeated option")),
        };
        let value = it.next().ok_or(StoreError::bad("option needs a value"))?;
        if o.insert(key.to_owned(), CValue::Str((*value).to_owned())).is_some() {
            return Err(StoreError::bad("repeated option"));
        }
    }
    call_from(which, &o)
}

// ------------------------------------------------------------------ surfaces

fn dispatch(ctx: &mut UncCtx<'_>, call: UncCall) -> Result<Vec<u8>, StoreError> {
    match call {
        UncCall::Create(p) => canonical(&create_model(ctx, p)?.to_value()),
        UncCall::Validate(p) => canonical(&validate_model(ctx, p)?.to_value()),
        UncCall::Show(p) => canonical(&show_model(ctx, p)?.to_value()),
    }
}

/// The MCP tools: parameters read strictly ([`mcp_params`]), then the command runs with the
/// caller's principal and branch. Output is the canonical result JSON.
pub fn invoke_mcp(ctx: &mut UncCtx<'_>, tool: &str, raw: &[u8]) -> Result<Vec<u8>, StoreError> {
    let call = mcp_params(tool, raw)?;
    dispatch(ctx, call)
}

/// Runs `fid <args>` (see [`cli_params`]). Output is the canonical result JSON.
pub fn invoke_cli(ctx: &mut UncCtx<'_>, args: &[&str]) -> Result<Vec<u8>, StoreError> {
    let call = cli_params(args)?;
    dispatch(ctx, call)
}
