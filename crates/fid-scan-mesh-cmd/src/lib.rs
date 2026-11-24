//! The SCAN mesh-operation command adapter of Metrology Maven: the bridge between the kernel
//! mesh operations (`fid-scan-mesh`, kernel graph) and the project document, as `fid-geo-cmd`
//! is for GEO.
//!
//! Implemented: `mesh.invert_normals@1` of `SCAN.MESHOP.CUT.INVERT`
//! (`spec/scan/meshop/cut/invert/NODE.md`), W2, D0, CLI `fid mesh invert-normals --mesh <h>
//! [--selection <h>] [--json]`, MCP `mesh_invert_normals` with `{"mesh": "<h>",
//! "selection": "<h>"}` (`selection` optional). Both surfaces build the same parameters and
//! call [`invert_normals`], so they return the same result bytes and the same journal entry.
//!
//! # Mesh versions
//!
//! The project document has no store for derived mesh versions yet (a `SCAN.MESHOP.PROV`
//! output is a mesh with its `synthesized` flags, which is not a GEO.TYPES element). Until it
//! does, [`MeshStore`] holds them: each version under its entity ID with the version digest
//! recorded when it was stored, against which [`invert_normals`] verifies the content before
//! the run (`scan.mesh.digest_mismatch` otherwise). A new version's ID is the UUIDv5 of the
//! GEO.TYPES.REG derivation (`fid_geo_cmd::derive_id`) over the command, its canonical
//! parameters, the output role `mesh`, and the input mesh and selection with their digests.
//!
//! # Refusals journal nothing
//!
//! Every check and the whole kernel run happen before the store or the journal is touched;
//! a refused call (unknown mesh or selection, a selection on another element, a stale
//! selection `scan.mesh.stale_selection`, an ID collision) leaves both unchanged. Nothing
//! here panics or uses `unsafe`.

use std::collections::BTreeMap;
use std::fmt;

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_geo_cmd::{derive_id, Derivation, FramePrincipal, GeoCmdError, SelectionRegistry};
use fid_geo_types::Mesh;
use fid_registry::{CommandDef, DefError};
use fid_scan_mesh::cut::invert::{plan, INVERT_NORMALS as INVERT_NORMALS_OP};
use fid_scan_mesh::{InvertParams, MeshOpError, MeshOpOutput, MeshView};
use fid_types::{CommandId, Digest};
use fid_uuid::Uuid;

/// Command ID of `mesh.invert_normals@1`.
pub const INVERT_NORMALS: &str = INVERT_NORMALS_OP;
/// CLI subcommand of `fid mesh`.
pub const CLI_INVERT_NORMALS: &str = "invert-normals";
/// MCP tool name.
pub const MCP_INVERT_NORMALS: &str = "mesh_invert_normals";
/// Largest MCP parameter payload, in bytes.
pub const MAX_PARAMS_BYTES: usize = 4 * 1024;
/// Deepest MCP parameter nesting (the object itself).
pub const MAX_PARAMS_DEPTH: usize = 1;
const MAX_CLI_ARGS: usize = 8;
const LIFECYCLE_SINCE: &str = "2026-11-24";
const ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";
const MAX_KEPT_BYTES: usize = 64;

/// Entity IDs (CONSTITUTION §5).
pub type EntityId = Uuid;

// ================================================================== Errors

/// Every way the adapter refuses. A refused call leaves the store and the journal unchanged.
#[derive(Debug, Clone, PartialEq)]
pub enum MeshCmdError {
    /// No mesh version has this ID. Code `scan.mesh.not_found`.
    NotFound(EntityId),
    /// No selection has this ID. Code `scan.mesh.selection_not_found`.
    SelectionNotFound(EntityId),
    /// The selection targets another element. Code `scan.mesh.selection_target`.
    SelectionTarget { selection: EntityId, mesh: EntityId },
    /// The derived ID of the new version is already taken (the same command on the same
    /// input was already run). Code `scan.mesh.id_collision`.
    IdCollision(EntityId),
    /// Parameters do not match the pinned shape. Code `scan.mesh.bad_params`.
    BadParams(&'static str),
    /// A tool or CLI subcommand this crate does not serve (name cut to 64 bytes). Code
    /// `scan.mesh.unknown_command`.
    UnknownCommand(String),
    /// The kernel operation refused (its own code, for example `scan.mesh.stale_selection`).
    Op(MeshOpError),
    /// The ID derivation refused (code `scan.mesh.bad_derivation`).
    Derivation(GeoCmdError),
    /// Canonical encoding failed. Code `scan.mesh.internal`.
    Internal(&'static str),
}

impl MeshCmdError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            MeshCmdError::NotFound(_) => "scan.mesh.not_found",
            MeshCmdError::SelectionNotFound(_) => "scan.mesh.selection_not_found",
            MeshCmdError::SelectionTarget { .. } => "scan.mesh.selection_target",
            MeshCmdError::IdCollision(_) => "scan.mesh.id_collision",
            MeshCmdError::BadParams(_) => "scan.mesh.bad_params",
            MeshCmdError::UnknownCommand(_) => "scan.mesh.unknown_command",
            MeshCmdError::Op(e) => e.code(),
            MeshCmdError::Derivation(_) => "scan.mesh.bad_derivation",
            MeshCmdError::Internal(_) => "scan.mesh.internal",
        }
    }
}

impl fmt::Display for MeshCmdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MeshCmdError::NotFound(id) | MeshCmdError::SelectionNotFound(id) | MeshCmdError::IdCollision(id) => {
                write!(f, "{}: {id}", self.code())
            }
            MeshCmdError::SelectionTarget { selection, mesh } => {
                write!(f, "{}: selection {selection} does not target mesh {mesh}", self.code())
            }
            MeshCmdError::BadParams(r) | MeshCmdError::Internal(r) => write!(f, "{}: {r}", self.code()),
            MeshCmdError::UnknownCommand(n) => write!(f, "{}: {n}", self.code()),
            MeshCmdError::Op(e) => write!(f, "{e}"),
            MeshCmdError::Derivation(e) => write!(f, "{}: {e}", self.code()),
        }
    }
}

impl std::error::Error for MeshCmdError {}

impl From<MeshOpError> for MeshCmdError {
    fn from(e: MeshOpError) -> Self {
        MeshCmdError::Op(e)
    }
}

fn kept(s: &str) -> String {
    let mut end = s.len().min(MAX_KEPT_BYTES);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or("").to_string()
}

// ================================================================== Mesh versions

/// One stored mesh version: a raw GEO.TYPES mesh (measured, no `synthesized` attribute) or
/// the output of a mesh operation (mesh and flags).
#[derive(Debug, Clone)]
pub enum MeshVersion {
    /// A mesh without the attribute; its version digest is the GEO.TYPES element digest.
    Raw(Box<Mesh>),
    /// An operation output; its version digest is the derived-mesh digest.
    Derived(Box<MeshOpOutput>),
}

impl MeshVersion {
    /// The mesh.
    pub fn mesh(&self) -> &Mesh {
        match self {
            MeshVersion::Raw(m) => m,
            MeshVersion::Derived(o) => o.mesh(),
        }
    }

    fn view(&self) -> Result<MeshView<'_>, MeshOpError> {
        match self {
            MeshVersion::Raw(m) => Ok(MeshView::unflagged(m)),
            MeshVersion::Derived(o) => MeshView::of_output(o),
        }
    }
}

/// Deterministic store of mesh versions keyed by entity ID, each with the version digest
/// recorded when it was stored.
#[derive(Debug, Clone, Default)]
pub struct MeshStore {
    versions: BTreeMap<EntityId, (MeshVersion, Digest)>,
}

impl MeshStore {
    /// An empty store.
    pub fn new() -> MeshStore {
        MeshStore::default()
    }

    /// Stores a raw mesh under `id` and returns its version digest (the GEO.TYPES element
    /// digest). An ID already in use is refused.
    pub fn insert_raw(&mut self, id: EntityId, mesh: Mesh) -> Result<Digest, MeshCmdError> {
        if self.versions.contains_key(&id) {
            return Err(MeshCmdError::IdCollision(id));
        }
        let digest = MeshView::unflagged(&mesh).digest();
        self.versions.insert(id, (MeshVersion::Raw(Box::new(mesh)), digest));
        Ok(digest)
    }

    /// The version stored under `id`.
    pub fn get(&self, id: &EntityId) -> Option<&MeshVersion> {
        self.versions.get(id).map(|(v, _)| v)
    }

    /// The version digest recorded for `id`.
    pub fn digest(&self, id: &EntityId) -> Option<Digest> {
        self.versions.get(id).map(|(_, d)| *d)
    }

    /// The number of stored versions.
    pub fn len(&self) -> usize {
        self.versions.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }
}

// ================================================================== Journal, parameters, result

/// One committed Record Journal entry of `mesh.invert_normals@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshJournalEntry {
    /// The command ID.
    pub command: String,
    /// The principal that ran it.
    pub principal: String,
    /// The agent branch (`None` for a human on main).
    pub branch: Option<String>,
    /// The canonical parameters.
    pub args: CValue,
    /// The input mesh and its version digest.
    pub input: EntityId,
    pub input_digest: Digest,
    /// The selection and its content digest, if one was given.
    pub selection: Option<(EntityId, Digest)>,
    /// The new mesh version and its version digest.
    pub output: EntityId,
    pub output_digest: Digest,
    /// The SCAN.MESHOP.PROV record digest of the run.
    pub record_digest: Digest,
    /// The digest of the canonical result.
    pub result_digest: Digest,
}

impl MeshJournalEntry {
    /// The canonical representation.
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("args".to_owned(), self.args.clone());
        if let Some(b) = &self.branch {
            m.insert("branch".to_owned(), text(b));
        }
        m.insert("command".to_owned(), text(&self.command));
        m.insert("input".to_owned(), text(&self.input.to_string()));
        m.insert("input_digest".to_owned(), text(self.input_digest.as_str()));
        m.insert("output".to_owned(), text(&self.output.to_string()));
        m.insert("output_digest".to_owned(), text(self.output_digest.as_str()));
        m.insert("principal".to_owned(), text(&self.principal));
        m.insert("record_digest".to_owned(), text(self.record_digest.as_str()));
        m.insert("result_digest".to_owned(), text(self.result_digest.as_str()));
        if let Some((id, d)) = &self.selection {
            m.insert("selection".to_owned(), text(&id.to_string()));
            m.insert("selection_digest".to_owned(), text(d.as_str()));
        }
        CValue::Obj(m)
    }

    /// Canonical bytes of the entry (exact-replay comparison).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, MeshCmdError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| MeshCmdError::Internal("canonical_encode"))
    }
}

/// Parameters of `mesh.invert_normals@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvertNormalsParams {
    /// The mesh version to invert.
    pub mesh: EntityId,
    /// A triangle selection on that version; `None` inverts the whole mesh.
    pub selection: Option<EntityId>,
}

impl InvertNormalsParams {
    fn to_value(self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("mesh".to_owned(), text(&self.mesh.to_string()));
        if let Some(s) = self.selection {
            m.insert("selection".to_owned(), text(&s.to_string()));
        }
        CValue::Obj(m)
    }
}

/// The result of `mesh.invert_normals@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvertNormalsResult {
    /// The new mesh version.
    pub mesh: EntityId,
    /// Its version digest (the derived-mesh digest, flags included).
    pub digest: Digest,
    /// The input mesh version and its digest.
    pub input: EntityId,
    pub input_digest: Digest,
    /// Triangles whose winding was reversed.
    pub inverted_triangles: u64,
    /// Vertices whose stored normal was negated (0 when the mesh stores no normals).
    pub negated_normals: u64,
    /// The SCAN.MESHOP.PROV record digest of the run.
    pub record_digest: Digest,
}

impl InvertNormalsResult {
    /// The canonical result object.
    pub fn to_value(&self) -> CValue {
        obj([
            ("digest", text(self.digest.as_str())),
            ("input", text(&self.input.to_string())),
            ("input_digest", text(self.input_digest.as_str())),
            ("inverted_triangles", CValue::Num(self.inverted_triangles as f64)),
            ("mesh", text(&self.mesh.to_string())),
            ("negated_normals", CValue::Num(self.negated_normals as f64)),
            ("record_digest", text(self.record_digest.as_str())),
        ])
    }
}

// ================================================================== The command

/// Runs `mesh.invert_normals@1` (W2, D0) for `principal`: resolves the mesh version and
/// verifies it against its recorded digest, resolves the selection (which must target that
/// mesh and not be stale), runs the kernel operation, stores the new version under its
/// derived ID in `namespace` and appends one journal entry. A refusal changes nothing.
pub fn invert_normals(
    store: &mut MeshStore,
    selections: &SelectionRegistry,
    journal: &mut Vec<MeshJournalEntry>,
    principal: &FramePrincipal,
    namespace: Uuid,
    params: &InvertNormalsParams,
) -> Result<InvertNormalsResult, MeshCmdError> {
    let (version, input_digest) = store.versions.get(&params.mesh).ok_or(MeshCmdError::NotFound(params.mesh))?;
    let input_digest = *input_digest;
    let view = version.view()?.verified(&input_digest)?;
    let mesh = version.mesh();

    let (op_params, selection) = match params.selection {
        None => (InvertParams::whole(), None),
        Some(sel_id) => {
            let stored = selections.get(&sel_id).ok_or(MeshCmdError::SelectionNotFound(sel_id))?;
            if stored.selection.target().to_bytes() != *params.mesh.as_bytes() {
                return Err(MeshCmdError::SelectionTarget { selection: sel_id, mesh: params.mesh });
            }
            let p = InvertParams::from_selection(&stored.selection, mesh)?;
            let d = p.selection_digest().ok_or(MeshCmdError::Internal("selection_digest"))?;
            (p, Some((sel_id, d)))
        }
    };
    let counts = plan(mesh, &op_params)?;
    let has_normals = mesh.vertex_attrs().normal.is_some();
    let out = fid_scan_mesh::invert_normals(view, &op_params)?;

    let args = params.to_value();
    let canonical = to_canonical_bytes(&args).map_err(|_| MeshCmdError::Internal("canonical_encode"))?;
    let command = CommandId::parse(INVERT_NORMALS).map_err(|_| MeshCmdError::Internal("bad_command_id"))?;
    let mut inputs = vec![(params.mesh, input_digest)];
    if let Some((id, d)) = selection {
        inputs.push((id, d));
    }
    let derivation = Derivation {
        command,
        params: Digest::from_bytes(digest_bytes(&canonical).0),
        output: "mesh".to_owned(),
        inputs,
    };
    let new_id = derive_id(namespace, &derivation).map_err(MeshCmdError::Derivation)?;
    if store.versions.contains_key(&new_id) {
        return Err(MeshCmdError::IdCollision(new_id));
    }

    let result = InvertNormalsResult {
        mesh: new_id,
        digest: out.output_digest(),
        input: params.mesh,
        input_digest,
        inverted_triangles: counts.triangle_count(),
        negated_normals: if has_normals { counts.vertex_count() } else { 0 },
        record_digest: out.record_digest(),
    };
    let result_bytes =
        to_canonical_bytes(&result.to_value()).map_err(|_| MeshCmdError::Internal("canonical_encode"))?;
    let entry = MeshJournalEntry {
        command: INVERT_NORMALS.to_owned(),
        principal: principal.principal_id().to_owned(),
        branch: principal.branch().map(str::to_owned),
        args,
        input: params.mesh,
        input_digest,
        selection,
        output: new_id,
        output_digest: result.digest,
        record_digest: result.record_digest,
        result_digest: Digest::from_bytes(digest_bytes(&result_bytes).0),
    };
    // Commit: nothing below can fail.
    let digest = out.output_digest();
    store.versions.insert(new_id, (MeshVersion::Derived(Box::new(out)), digest));
    journal.push(entry);
    Ok(result)
}

// ================================================================== CLI and MCP

fn parse_id(s: &str) -> Result<EntityId, MeshCmdError> {
    Uuid::parse(s).map_err(|_| MeshCmdError::BadParams("not a UUID"))
}

fn run_bytes(
    store: &mut MeshStore,
    selections: &SelectionRegistry,
    journal: &mut Vec<MeshJournalEntry>,
    principal: &FramePrincipal,
    namespace: Uuid,
    params: &InvertNormalsParams,
) -> Result<Vec<u8>, MeshCmdError> {
    let res = invert_normals(store, selections, journal, principal, namespace, params)?;
    to_canonical_bytes(&res.to_value()).map_err(|_| MeshCmdError::Internal("canonical_encode"))
}

/// Runs `fid mesh <args>`: `invert-normals --mesh <h> [--selection <h>] [--json]`. Output is
/// canonical JSON either way. Unknown subcommands, unknown, repeated or value-less options
/// and stray arguments are refused before anything runs.
pub fn invoke_cli(
    store: &mut MeshStore,
    selections: &SelectionRegistry,
    journal: &mut Vec<MeshJournalEntry>,
    principal: &FramePrincipal,
    namespace: Uuid,
    args: &[&str],
) -> Result<Vec<u8>, MeshCmdError> {
    if args.is_empty() || args.len() > MAX_CLI_ARGS {
        return Err(MeshCmdError::BadParams("bad argument count"));
    }
    let (sub, rest) = (args[0], &args[1..]);
    if sub != CLI_INVERT_NORMALS {
        return Err(MeshCmdError::UnknownCommand(kept(sub)));
    }
    let (mut mesh, mut selection, mut json) = (None, None, false);
    let mut it = rest.iter();
    while let Some(&a) = it.next() {
        match a {
            "--mesh" | "--selection" => {
                let v = it.next().ok_or(MeshCmdError::BadParams("option without a value"))?;
                let slot = if a == "--mesh" { &mut mesh } else { &mut selection };
                if slot.is_some() {
                    return Err(MeshCmdError::BadParams("repeated option"));
                }
                *slot = Some(parse_id(v)?);
            }
            "--json" => {
                if json {
                    return Err(MeshCmdError::BadParams("repeated option"));
                }
                json = true;
            }
            _ => return Err(MeshCmdError::BadParams("unknown argument")),
        }
    }
    let mesh = mesh.ok_or(MeshCmdError::BadParams("missing --mesh"))?;
    run_bytes(store, selections, journal, principal, namespace, &InvertNormalsParams { mesh, selection })
}

/// Runs the MCP tool `mesh_invert_normals` on raw JSON parameters (at most
/// [`MAX_PARAMS_BYTES`] bytes, one level, duplicate keys refused): `{"mesh": "<uuid>",
/// "selection": "<uuid>"}` with `selection` optional and no other key.
pub fn invoke_mcp(
    store: &mut MeshStore,
    selections: &SelectionRegistry,
    journal: &mut Vec<MeshJournalEntry>,
    principal: &FramePrincipal,
    namespace: Uuid,
    tool: &str,
    raw: &[u8],
) -> Result<Vec<u8>, MeshCmdError> {
    if tool != MCP_INVERT_NORMALS {
        return Err(MeshCmdError::UnknownCommand(kept(tool)));
    }
    let v = parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => MeshCmdError::BadParams("params too large or too deep"),
        CanonError::DuplicateKey(_) => MeshCmdError::BadParams("duplicate key"),
        _ => MeshCmdError::BadParams("params are not valid JSON"),
    })?;
    let CValue::Obj(o) = v else { return Err(MeshCmdError::BadParams("params is not an object")) };
    if o.keys().any(|k| k != "mesh" && k != "selection") {
        return Err(MeshCmdError::BadParams("unknown parameter"));
    }
    let id = |k: &str| -> Result<Option<EntityId>, MeshCmdError> {
        match o.get(k) {
            None => Ok(None),
            Some(CValue::Str(s)) => parse_id(s).map(Some),
            Some(_) => Err(MeshCmdError::BadParams("parameter is not a string")),
        }
    };
    let mesh = id("mesh")?.ok_or(MeshCmdError::BadParams("missing mesh"))?;
    let selection = id("selection")?;
    run_bytes(store, selections, journal, principal, namespace, &InvertNormalsParams { mesh, selection })
}

// ================================================================== Registry definition

/// The registry definitions owned by this crate: `mesh.invert_normals@1` (W2, D0, CLI and
/// MCP exposed).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let uuid = || obj([("type", text("string")), ("pattern", text(ID_PATTERN))]);
    let digest = || obj([("type", text("string"))]);
    let count = || obj([("type", text("integer")), ("minimum", CValue::Num(0.0))]);
    let params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["mesh"])),
        (
            "properties",
            obj([
                ("mesh", obj([("type", text("string")), ("x-ref", text("element")), ("pattern", text(ID_PATTERN))])),
                ("selection", uuid()),
            ]),
        ),
    ]);
    let result = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        (
            "required",
            strs(&[
                "digest",
                "input",
                "input_digest",
                "inverted_triangles",
                "mesh",
                "negated_normals",
                "record_digest",
            ]),
        ),
        (
            "properties",
            obj([
                ("digest", digest()),
                ("input", uuid()),
                ("input_digest", digest()),
                ("inverted_triangles", count()),
                ("mesh", uuid()),
                ("negated_normals", count()),
                ("record_digest", digest()),
            ]),
        ),
    ]);
    let def = CommandDef::from_value(obj([
        ("algorithms", CValue::Arr(Vec::new())),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text(
                "Reverses the winding of every triangle of a mesh, or of a triangle selection, and negates stored \
                 vertex normals whose incident triangles are all reversed; creates a new mesh version.",
            ),
        ),
        ("determinism", text("D0")),
        ("effects", strs(&["MeshVersionCreated"])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text("mesh.invert_normals")),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result),
        ("side_effect", text("W2")),
        ("title", text("Invert mesh normals")),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))?;
    Ok(vec![def])
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn obj<const N: usize>(entries: [(&str, CValue); N]) -> CValue {
    CValue::Obj(entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}
