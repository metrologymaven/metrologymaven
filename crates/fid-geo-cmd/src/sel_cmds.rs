//! Selection registry commands (`GEO.SEL.CMD`, `spec/geo/sel/cmd/NODE.md`).
//!
//! Five D0 commands (four W2 mutating commands producing one Record Journal entry each, and one
//! R0 read command logged only to the Activity Log; agent W2 calls route to their branch):
//!
//! - `geo.create_selection@1` (`fid geo create-selection` / `geo_create_selection`, W2):
//!   creates an index-set selection from an explicit inline index list (capped at 10^6 indexes,
//!   `REQ-005`).
//! - `geo.resolve_selection@1` (`fid geo resolve-selection` / `geo_resolve_selection`, W2):
//!   resolves explicit camera parameters plus a pick or lasso polygon on the CPU via
//!   `fid-geo-sel::resolve` before journaling; the stored selection holds only the index set and
//!   digests, never camera or pixel fields (`REQ-002`).
//! - `geo.combine_selection@1` (`fid geo combine-selection` / `geo_combine_selection`, W2):
//!   set algebra (`union`, `intersection`, `difference`, `sym_difference`, `invert`) via
//!   `fid-geo-sel::algebra`, rejecting stale inputs with `stale_selection` (`REQ-003`).
//! - `geo.grow_selection@1` (`fid geo grow-selection` / `geo_grow_selection`, W2):
//!   expands or shrinks a selection by mesh rings (`edge`/`vertex`) or authored-length radius.
//! - `geo.describe_selection@1` (`fid geo describe-selection` / `geo_describe_selection`, R0):
//!   returns domain, count, ranges count, target, digests and staleness without journaling.
//!
//! Every created selection is registered in `CORE.DEP` as derived from its target element so
//! replacing the target marks the selection `outdated` (`REQ-004`). Nothing here panics or uses
//! `unsafe`.

use std::collections::BTreeMap;

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_dep::{EdgesSet, ElementState};
use fid_geo_frame::{
    units::{AuthoredQuantity, Unit},
    Rigid,
};
use fid_geo_index::PrimitiveBvh;
use fid_geo_sel::{
    apply, difference_selections, domain_count, grow_selection, intersection_selections, invert_selection,
    resolve_lasso, resolve_pick, shrink_selection, sym_difference_selections, union_selections, Adjacency, Camera,
    Domain, EntityId as SelEntityId, GrowSpec, IndexSet, Lasso, Projection, ResolveMode, SelError, Selection,
    SelectionBody, TriangleRule,
};
use fid_geo_types::{digest as elem_digest, GeometryElement};
use fid_registry::{CommandDef, DefError};
use fid_types::{CommandId, Digest};
use fid_uuid::Uuid;

use crate::describe::{ActivityEntry, Surface};
use crate::error::{kept, GeoCmdError};
use crate::frame_cmds::{AuthoredLengthSpec, FramePrincipal};
use crate::register::{derive_id, Derivation, ElementTree, EntityId, GeoTx, TreeEntry};

/// Command ID for `geo.create_selection@1`.
pub const CREATE_SELECTION: &str = "geo.create_selection@1";
/// Command ID for `geo.resolve_selection@1`.
pub const RESOLVE_SELECTION: &str = "geo.resolve_selection@1";
/// Command ID for `geo.combine_selection@1`.
pub const COMBINE_SELECTION: &str = "geo.combine_selection@1";
/// Command ID for `geo.grow_selection@1`.
pub const GROW_SELECTION: &str = "geo.grow_selection@1";
/// Command ID for `geo.describe_selection@1`.
pub const DESCRIBE_SELECTION: &str = "geo.describe_selection@1";

/// CLI subcommand for `geo.create_selection@1`.
pub const CLI_CREATE_SELECTION: &str = "create-selection";
/// CLI subcommand for `geo.resolve_selection@1`.
pub const CLI_RESOLVE_SELECTION: &str = "resolve-selection";
/// CLI subcommand for `geo.combine_selection@1`.
pub const CLI_COMBINE_SELECTION: &str = "combine-selection";
/// CLI subcommand for `geo.grow_selection@1`.
pub const CLI_GROW_SELECTION: &str = "grow-selection";
/// CLI subcommand for `geo.describe_selection@1`.
pub const CLI_DESCRIBE_SELECTION: &str = "describe-selection";

/// MCP tool name for `geo.create_selection@1`.
pub const MCP_CREATE_SELECTION: &str = "geo_create_selection";
/// MCP tool name for `geo.resolve_selection@1`.
pub const MCP_RESOLVE_SELECTION: &str = "geo_resolve_selection";
/// MCP tool name for `geo.combine_selection@1`.
pub const MCP_COMBINE_SELECTION: &str = "geo_combine_selection";
/// MCP tool name for `geo.grow_selection@1`.
pub const MCP_GROW_SELECTION: &str = "geo_grow_selection";
/// MCP tool name for `geo.describe_selection@1`.
pub const MCP_DESCRIBE_SELECTION: &str = "geo_describe_selection";

/// Maximum inline explicit indices accepted by `geo.create_selection@1` (`REQ-GEO.SEL.CMD-005`).
pub const MAX_INLINE_INDICES: usize = 1_000_000;

/// Maximum JSON parameter payload for selection commands (covers 10^6 inline integers).
pub const MAX_SEL_PARAMS_BYTES: usize = 16 * 1024 * 1024;
/// Maximum JSON parameter nesting depth for selection commands.
pub const MAX_SEL_PARAMS_DEPTH: usize = 12;
const MAX_CLI_ARGS: usize = 32;
const LIFECYCLE_SINCE: &str = "2026-11-23";
const ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";

// ================================================================== Document storage & journal

/// A stored selection element in the project document.
///
/// Holds ONLY the geometry-bound [`Selection`] (target ID, target digest, domain, canonical
/// ranges) and its element ID — never any camera, viewport or pixel fields (`REQ-GEO.SEL.CMD-002`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSelection {
    pub id: EntityId,
    pub selection: Selection,
}

impl StoredSelection {
    /// Serialised JSON representation of the stored selection. Contains no camera or pixel fields.
    pub fn to_serialised_value(&self) -> Result<CValue, GeoCmdError> {
        let SelectionBody::IndexSet(set) = self.selection.body() else {
            return Err(GeoCmdError::Selection("selection is not an index set".to_owned()));
        };
        let sel_digest = self
            .selection
            .content_digest()
            .ok_or_else(|| GeoCmdError::Selection("missing selection digest".to_owned()))?;
        let ranges = CValue::Arr(
            set.ranges()
                .iter()
                .map(|&(a, b)| CValue::Arr(vec![CValue::Num(a as f64), CValue::Num(b as f64)]))
                .collect(),
        );
        Ok(obj([
            ("digest", text(sel_digest.to_digest().as_str())),
            ("domain", text(set.domain().as_str())),
            ("id", text(&self.id.to_string())),
            ("ranges", ranges),
            ("target", text(&Uuid::from_bytes(self.selection.target().to_bytes()).to_string())),
            ("target_digest", text(self.selection.target_digest().to_digest().as_str())),
        ]))
    }
}

/// Deterministic store of selection elements keyed by entity ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionRegistry {
    entries: BTreeMap<EntityId, StoredSelection>,
}

impl SelectionRegistry {
    /// Empty registry.
    pub fn new() -> Self {
        Self { entries: BTreeMap::new() }
    }

    /// Looks up a stored selection by ID.
    pub fn get(&self, id: &EntityId) -> Option<&StoredSelection> {
        self.entries.get(id)
    }

    /// Inserts or updates a stored selection.
    pub fn insert(&mut self, stored: StoredSelection) {
        self.entries.insert(stored.id, stored);
    }

    /// Number of stored selections.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if no selections are stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One committed Record Journal entry for a mutating `GEO.SEL.CMD` command (`REQ-001`, `REQ-002`).
#[derive(Debug, Clone, PartialEq)]
pub struct SelJournalEntry {
    pub command: String,
    pub principal: String,
    pub branch: Option<String>,
    pub args: CValue,
    pub selection_id: EntityId,
    pub target_digest: Digest,
    pub selection_digest: Digest,
    pub result_digest: Digest,
}

impl SelJournalEntry {
    /// Canonical RFC 8785 representation of the journal entry.
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("args".to_owned(), self.args.clone());
        if let Some(b) = &self.branch {
            m.insert("branch".to_owned(), text(b));
        }
        m.insert("command".to_owned(), text(&self.command));
        m.insert("principal".to_owned(), text(&self.principal));
        m.insert("result_digest".to_owned(), text(self.result_digest.as_str()));
        m.insert("selection_digest".to_owned(), text(self.selection_digest.as_str()));
        m.insert("selection_id".to_owned(), text(&self.selection_id.to_string()));
        m.insert("target_digest".to_owned(), text(self.target_digest.as_str()));
        CValue::Obj(m)
    }

    /// Canonical bytes of this journal entry for exact-replay comparison (`AC-5`).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, GeoCmdError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| GeoCmdError::Internal("canonical_encode"))
    }
}

// ================================================================== Parameters & results

/// Parameters for `geo.create_selection@1` (W2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateSelectionParams {
    pub target: EntityId,
    pub domain: Option<Domain>,
    pub indices: Vec<u64>,
}

/// Resolution input: either a pixel pick or a closed screen-space lasso polygon.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveInput {
    Pick { pixel: (f64, f64), tol_px: f64 },
    Lasso(Lasso),
}

/// Parameters for `geo.resolve_selection@1` (W2).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolveSelectionParams {
    pub target: EntityId,
    pub domain: Option<Domain>,
    pub camera: Camera,
    pub input: ResolveInput,
    pub rule: TriangleRule,
    pub mode: ResolveMode,
}

/// Set algebra operation for `geo.combine_selection@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombineOp {
    Union,
    Intersection,
    Difference,
    SymDifference,
    Invert,
}

impl CombineOp {
    /// Wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            CombineOp::Union => "union",
            CombineOp::Intersection => "intersection",
            CombineOp::Difference => "difference",
            CombineOp::SymDifference => "sym_difference",
            CombineOp::Invert => "invert",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "union" => Some(CombineOp::Union),
            "intersection" => Some(CombineOp::Intersection),
            "difference" => Some(CombineOp::Difference),
            "sym_difference" => Some(CombineOp::SymDifference),
            "invert" => Some(CombineOp::Invert),
            _ => None,
        }
    }
}

/// Parameters for `geo.combine_selection@1` (W2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombineSelectionParams {
    pub op: CombineOp,
    pub a: EntityId,
    pub b: Option<EntityId>,
}

/// Growth or shrinkage rule for `geo.grow_selection@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrowParam {
    Rings { rings: u32, adjacency: Adjacency },
    Radius(AuthoredLengthSpec),
}

/// Parameters for `geo.grow_selection@1` (W2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrowSelectionParams {
    pub selection: EntityId,
    pub spec: GrowParam,
    pub shrink: bool,
}

/// Result returned by all four mutating selection commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionResult {
    pub id: EntityId,
    pub target: EntityId,
    pub target_digest: Digest,
    pub domain: Domain,
    pub count: u64,
    pub ranges_count: u64,
    pub digest: Digest,
}

impl SelectionResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("count", CValue::Num(self.count as f64)),
            ("digest", text(self.digest.as_str())),
            ("domain", text(self.domain.as_str())),
            ("id", text(&self.id.to_string())),
            ("ranges_count", CValue::Num(self.ranges_count as f64)),
            ("target", text(&self.target.to_string())),
            ("target_digest", text(self.target_digest.as_str())),
        ])
    }
}

/// Result of `geo.describe_selection@1` (R0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescribeSelectionResult {
    pub id: EntityId,
    pub target: EntityId,
    pub target_digest: Digest,
    pub domain: Domain,
    pub count: u64,
    pub ranges_count: u64,
    pub digest: Digest,
    pub stale: bool,
}

impl DescribeSelectionResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("count", CValue::Num(self.count as f64)),
            ("digest", text(self.digest.as_str())),
            ("domain", text(self.domain.as_str())),
            ("id", text(&self.id.to_string())),
            ("ranges_count", CValue::Num(self.ranges_count as f64)),
            ("stale", CValue::Bool(self.stale)),
            ("target", text(&self.target.to_string())),
            ("target_digest", text(self.target_digest.as_str())),
        ])
    }
}

// ================================================================== Command implementations

/// Executes `geo.create_selection@1` (W2, D0; `REQ-001`, `REQ-004`, `REQ-005`).
pub fn create_selection(
    selections: &mut SelectionRegistry,
    tx: &mut GeoTx<'_>,
    journal: &mut Vec<SelJournalEntry>,
    principal: &FramePrincipal,
    params: &CreateSelectionParams,
) -> Result<SelectionResult, GeoCmdError> {
    if params.indices.len() > MAX_INLINE_INDICES {
        return Err(GeoCmdError::TooManyIndices { count: params.indices.len(), max: MAX_INLINE_INDICES });
    }
    let target_elem = lookup_geometry(tx.tree, params.target)?;
    let domain = params.domain.unwrap_or_else(|| default_domain(target_elem));
    let sel_target = to_sel_entity(params.target);
    let sel = Selection::on_element(sel_target, target_elem, domain, &params.indices).map_err(map_sel_err)?;

    let args = encode_create_args(params.target, domain, &params.indices);
    let params_digest = value_digest(&args)?;
    let inputs = vec![(params.target, sel.target_digest().to_digest())];
    commit_selection(selections, tx, journal, principal, CREATE_SELECTION, args, params_digest, inputs, sel)
}

/// Executes `geo.resolve_selection@1` (W2, D0; `REQ-001`, `REQ-002`, `REQ-004`).
pub fn resolve_selection(
    selections: &mut SelectionRegistry,
    tx: &mut GeoTx<'_>,
    journal: &mut Vec<SelJournalEntry>,
    principal: &FramePrincipal,
    params: &ResolveSelectionParams,
) -> Result<SelectionResult, GeoCmdError> {
    let target_elem = lookup_geometry(tx.tree, params.target)?;
    let domain = params.domain.unwrap_or_else(|| default_domain(target_elem));
    let index_set: IndexSet = match &params.input {
        ResolveInput::Pick { pixel, tol_px } => {
            let bvh = match (target_elem, domain) {
                (GeometryElement::Mesh(mesh), Domain::Triangles) => {
                    let payload: Vec<u64> = (0..mesh.triangle_count()).collect();
                    Some(PrimitiveBvh::build(mesh, &payload, 1).map_err(|e| GeoCmdError::Index(format!("{e:?}")))?)
                }
                _ => None,
            };
            resolve_pick(target_elem, bvh.as_ref(), &params.camera, *pixel, *tol_px, domain).map_err(map_sel_err)?
        }
        ResolveInput::Lasso(lasso) => {
            resolve_lasso(target_elem, &params.camera, lasso, domain, params.rule, params.mode).map_err(map_sel_err)?
        }
    };
    let target_dig = elem_digest(target_elem);
    let sel = Selection::new(to_sel_entity(params.target), target_dig, SelectionBody::IndexSet(index_set));

    let args = encode_resolve_args(params, domain);
    let params_digest = value_digest(&args)?;
    let inputs = vec![(params.target, target_dig.to_digest())];
    commit_selection(selections, tx, journal, principal, RESOLVE_SELECTION, args, params_digest, inputs, sel)
}

/// Executes `geo.combine_selection@1` (W2, D0; `REQ-001`, `REQ-003`, `REQ-004`).
pub fn combine_selection(
    selections: &mut SelectionRegistry,
    tx: &mut GeoTx<'_>,
    journal: &mut Vec<SelJournalEntry>,
    principal: &FramePrincipal,
    params: &CombineSelectionParams,
) -> Result<SelectionResult, GeoCmdError> {
    let stored_a = selections.get(&params.a).ok_or(GeoCmdError::NotFound(params.a))?.clone();
    let target_a = from_sel_entity(stored_a.selection.target());
    let elem_a = lookup_geometry(tx.tree, target_a)?;
    // REQ-003 / AC-4: fail closed with stale_selection before changing anything.
    validate_fresh(params.a, &stored_a.selection, elem_a)?;

    let (new_sel, mut inputs) = match params.op {
        CombineOp::Invert => {
            if params.b.is_some() {
                return Err(GeoCmdError::BadParams("invert takes no second selection"));
            }
            let SelectionBody::IndexSet(set_a) = stored_a.selection.body() else {
                return Err(GeoCmdError::Selection("selection is not an index set".to_owned()));
            };
            let count = domain_count(elem_a, set_a.domain())
                .map_err(map_sel_err)?
                .ok_or_else(|| GeoCmdError::DomainMismatch(format!("no count for {}", set_a.domain())))?;
            let inv = invert_selection(&stored_a.selection, count).map_err(map_sel_err)?;
            let da = sel_content_digest(&stored_a.selection)?;
            (inv, vec![(target_a, stored_a.selection.target_digest().to_digest()), (params.a, da)])
        }
        op => {
            let id_b = params.b.ok_or(GeoCmdError::BadParams("binary combine requires b"))?;
            let stored_b = selections.get(&id_b).ok_or(GeoCmdError::NotFound(id_b))?.clone();
            let target_b = from_sel_entity(stored_b.selection.target());
            let elem_b = lookup_geometry(tx.tree, target_b)?;
            validate_fresh(id_b, &stored_b.selection, elem_b)?;
            if target_a != target_b {
                return Err(GeoCmdError::ElementMismatch(format!("{target_a} != {target_b}")));
            }
            let combined = match op {
                CombineOp::Union => union_selections(&stored_a.selection, &stored_b.selection),
                CombineOp::Intersection => intersection_selections(&stored_a.selection, &stored_b.selection),
                CombineOp::Difference => difference_selections(&stored_a.selection, &stored_b.selection),
                CombineOp::SymDifference => sym_difference_selections(&stored_a.selection, &stored_b.selection),
                CombineOp::Invert => unreachable_combine_err(),
            }
            .map_err(map_sel_err)?;
            let da = sel_content_digest(&stored_a.selection)?;
            let db = sel_content_digest(&stored_b.selection)?;
            let mut ins = vec![(target_a, stored_a.selection.target_digest().to_digest()), (params.a, da)];
            if id_b != params.a {
                ins.push((id_b, db));
            }
            (combined, ins)
        }
    };

    inputs.sort_by_key(|(id, _)| *id);
    let args = encode_combine_args(params);
    let params_digest = value_digest(&args)?;
    commit_selection(selections, tx, journal, principal, COMBINE_SELECTION, args, params_digest, inputs, new_sel)
}

/// Executes `geo.grow_selection@1` (W2, D0; `REQ-001`, `REQ-003`, `REQ-004`).
pub fn grow_or_shrink_selection(
    selections: &mut SelectionRegistry,
    tx: &mut GeoTx<'_>,
    journal: &mut Vec<SelJournalEntry>,
    principal: &FramePrincipal,
    params: &GrowSelectionParams,
) -> Result<SelectionResult, GeoCmdError> {
    let stored = selections.get(&params.selection).ok_or(GeoCmdError::NotFound(params.selection))?.clone();
    let target_id = from_sel_entity(stored.selection.target());
    let target_elem = lookup_geometry(tx.tree, target_id)?;
    validate_fresh(params.selection, &stored.selection, target_elem)?;

    let (grow_spec, spec_args) = match &params.spec {
        GrowParam::Rings { rings, adjacency } => (
            GrowSpec::Rings(*rings, *adjacency),
            obj([("adjacency", text(adj_str(*adjacency))), ("rings", CValue::Num(f64::from(*rings)))]),
        ),
        GrowParam::Radius(len) => {
            let unit = Unit::from_symbol(&len.unit)
                .ok_or_else(|| GeoCmdError::BadQuantity(format!("unknown length unit {}", kept(&len.unit))))?;
            let q = AuthoredQuantity::parse(&len.text, unit).map_err(|e| GeoCmdError::BadQuantity(format!("{e:?}")))?;
            if !matches!(unit, Unit::Length(_)) {
                return Err(GeoCmdError::BadQuantity(format!("unit {} is not a length", kept(&len.unit))));
            }
            (GrowSpec::Radius(q.si()), obj([("radius", obj([("text", text(&len.text)), ("unit", text(&len.unit))]))]))
        }
    };

    let new_sel = if params.shrink {
        shrink_selection(target_elem, &stored.selection, grow_spec)
    } else {
        grow_selection(target_elem, &stored.selection, grow_spec)
    }
    .map_err(map_sel_err)?;

    let da = sel_content_digest(&stored.selection)?;
    let mut inputs = vec![(target_id, stored.selection.target_digest().to_digest()), (params.selection, da)];
    inputs.sort_by_key(|(id, _)| *id);

    let mut args_map = BTreeMap::new();
    args_map.insert("selection".to_owned(), text(&params.selection.to_string()));
    args_map.insert("shrink".to_owned(), CValue::Bool(params.shrink));
    if let CValue::Obj(extra) = spec_args {
        for (k, v) in extra {
            args_map.insert(k, v);
        }
    }
    let args = CValue::Obj(args_map);
    let params_digest = value_digest(&args)?;
    commit_selection(selections, tx, journal, principal, GROW_SELECTION, args, params_digest, inputs, new_sel)
}

/// Executes `geo.describe_selection@1` (R0, D0; writes no Record Journal entry, `REQ-001`).
pub fn describe_selection(
    selections: &SelectionRegistry,
    tree: &dyn ElementTree,
    log: &mut Vec<ActivityEntry>,
    surface: Surface,
    selection_id: EntityId,
) -> Result<DescribeSelectionResult, GeoCmdError> {
    let run = || -> Result<DescribeSelectionResult, GeoCmdError> {
        let stored = selections.get(&selection_id).ok_or(GeoCmdError::NotFound(selection_id))?;
        let SelectionBody::IndexSet(set) = stored.selection.body() else {
            return Err(GeoCmdError::Selection("selection is not an index set".to_owned()));
        };
        let target_id = from_sel_entity(stored.selection.target());
        let stale = match tree.get(&target_id) {
            Some(TreeEntry::Geometry(g)) => apply(&stored.selection, &g.element).is_err(),
            _ => true,
        };
        let dig = sel_content_digest(&stored.selection)?;
        Ok(DescribeSelectionResult {
            id: stored.id,
            target: target_id,
            target_digest: stored.selection.target_digest().to_digest(),
            domain: set.domain(),
            count: set.len(),
            ranges_count: set.ranges().len() as u64,
            digest: dig,
            stale,
        })
    };
    let out = run();
    log.push(ActivityEntry {
        command: DESCRIBE_SELECTION,
        surface,
        outcome: match &out {
            Ok(_) => "ok",
            Err(e) => e.code(),
        },
    });
    out
}

/// Replays a slice of [`SelJournalEntry`] in exact mode (`AC-5`), returning `Ok(replayed)` when
/// every entry reproduces bit-for-bit identically.
pub fn replay_sel_journal(
    selections: &mut SelectionRegistry,
    tx: &mut GeoTx<'_>,
    recorded: &[SelJournalEntry],
) -> Result<Vec<SelJournalEntry>, GeoCmdError> {
    let mut out = Vec::with_capacity(recorded.len());
    for entry in recorded {
        let principal = match &entry.branch {
            Some(b) => FramePrincipal::Agent { id: entry.principal.clone(), branch: b.clone() },
            None => FramePrincipal::Human { id: entry.principal.clone() },
        };
        let CValue::Obj(o) = &entry.args else {
            return Err(GeoCmdError::BadParams("journal args not object"));
        };
        match entry.command.as_str() {
            CREATE_SELECTION => {
                let p = parse_mcp_create_selection(o)?;
                create_selection(selections, tx, &mut out, &principal, &p)?;
            }
            RESOLVE_SELECTION => {
                let p = parse_mcp_resolve_selection(o)?;
                resolve_selection(selections, tx, &mut out, &principal, &p)?;
            }
            COMBINE_SELECTION => {
                let p = parse_mcp_combine_selection(o)?;
                combine_selection(selections, tx, &mut out, &principal, &p)?;
            }
            GROW_SELECTION => {
                let p = parse_mcp_grow_selection(o)?;
                grow_or_shrink_selection(selections, tx, &mut out, &principal, &p)?;
            }
            other => return Err(GeoCmdError::UnknownCommand(kept(other))),
        }
        let Some(replayed) = out.last() else {
            return Err(GeoCmdError::Internal("missing_replayed_entry"));
        };
        if replayed.to_canonical_bytes()? != entry.to_canonical_bytes()? {
            return Err(GeoCmdError::Internal("replay_divergent"));
        }
    }
    Ok(out)
}

// ================================================================== Shared commit helper

#[allow(clippy::too_many_arguments)]
fn commit_selection(
    selections: &mut SelectionRegistry,
    tx: &mut GeoTx<'_>,
    journal: &mut Vec<SelJournalEntry>,
    principal: &FramePrincipal,
    command: &'static str,
    args: CValue,
    params_digest: Digest,
    inputs: Vec<(EntityId, Digest)>,
    sel: Selection,
) -> Result<SelectionResult, GeoCmdError> {
    let SelectionBody::IndexSet(set) = sel.body() else {
        return Err(GeoCmdError::Selection("selection is not an index set".to_owned()));
    };
    let sel_digest = sel_content_digest(&sel)?;
    let cmd_id = CommandId::parse(command).map_err(|_| GeoCmdError::Internal("bad_command_id"))?;
    let derivation =
        Derivation { command: cmd_id, params: params_digest, output: "selection".to_owned(), inputs: inputs.clone() };
    let sel_id = derive_id(tx.namespace, &derivation)?;

    // Register dependency edges in CORE.DEP so replacing the target marks this selection outdated (REQ-004).
    let mut dep_ids: Vec<EntityId> = inputs.iter().map(|(id, _)| *id).collect();
    dep_ids.sort();
    dep_ids.dedup();
    let edges = EdgesSet::new(sel_id, dep_ids).map_err(GeoCmdError::Edge)?;
    tx.edges.apply(&[edges]).map_err(GeoCmdError::Edge)?;
    tx.states.insert(sel_id, ElementState::Current);

    let res = SelectionResult {
        id: sel_id,
        target: from_sel_entity(sel.target()),
        target_digest: sel.target_digest().to_digest(),
        domain: set.domain(),
        count: set.len(),
        ranges_count: set.ranges().len() as u64,
        digest: sel_digest,
    };

    selections.insert(StoredSelection { id: sel_id, selection: sel });
    let entry = SelJournalEntry {
        command: command.to_owned(),
        principal: principal.principal_id().to_owned(),
        branch: principal.branch().map(str::to_owned),
        args,
        selection_id: sel_id,
        target_digest: res.target_digest,
        selection_digest: res.digest,
        result_digest: value_digest(&res.to_value())?,
    };
    journal.push(entry);
    Ok(res)
}

// ================================================================== CLI & MCP dispatch

/// Runs `fid geo <args>` for the five `GEO.SEL.CMD` subcommands (`AC-1`..`AC-4`).
#[allow(clippy::too_many_arguments)]
pub fn invoke_sel_cli(
    selections: &mut SelectionRegistry,
    tx: &mut GeoTx<'_>,
    journal: &mut Vec<SelJournalEntry>,
    log: &mut Vec<ActivityEntry>,
    principal: &FramePrincipal,
    args: &[&str],
) -> Result<Vec<u8>, GeoCmdError> {
    if args.is_empty() || args.len() > MAX_CLI_ARGS {
        return Err(GeoCmdError::BadParams("bad argument count"));
    }
    let sub = args[0];
    let rest = &args[1..];
    match sub {
        CLI_CREATE_SELECTION => {
            let p = parse_cli_create_selection(rest)?;
            let res = create_selection(selections, tx, journal, principal, &p)?;
            to_canonical_bytes(&res.to_value()).map_err(|_| GeoCmdError::Internal("canonical_encode"))
        }
        CLI_RESOLVE_SELECTION => {
            let p = parse_cli_resolve_selection(rest)?;
            let res = resolve_selection(selections, tx, journal, principal, &p)?;
            to_canonical_bytes(&res.to_value()).map_err(|_| GeoCmdError::Internal("canonical_encode"))
        }
        CLI_COMBINE_SELECTION => {
            let p = parse_cli_combine_selection(rest)?;
            let res = combine_selection(selections, tx, journal, principal, &p)?;
            to_canonical_bytes(&res.to_value()).map_err(|_| GeoCmdError::Internal("canonical_encode"))
        }
        CLI_GROW_SELECTION => {
            let p = parse_cli_grow_selection(rest)?;
            let res = grow_or_shrink_selection(selections, tx, journal, principal, &p)?;
            to_canonical_bytes(&res.to_value()).map_err(|_| GeoCmdError::Internal("canonical_encode"))
        }
        CLI_DESCRIBE_SELECTION => {
            let id = parse_cli_describe_selection(rest)?;
            let res = describe_selection(selections, tx.tree, log, Surface::Cli, id)?;
            to_canonical_bytes(&res.to_value()).map_err(|_| GeoCmdError::Internal("canonical_encode"))
        }
        other => Err(GeoCmdError::UnknownCommand(kept(other))),
    }
}

/// Runs an MCP tool call for the five `GEO.SEL.CMD` tools.
#[allow(clippy::too_many_arguments)]
pub fn invoke_sel_mcp(
    selections: &mut SelectionRegistry,
    tx: &mut GeoTx<'_>,
    journal: &mut Vec<SelJournalEntry>,
    log: &mut Vec<ActivityEntry>,
    principal: &FramePrincipal,
    tool: &str,
    params_json: &[u8],
) -> Result<Vec<u8>, GeoCmdError> {
    if params_json.len() > MAX_SEL_PARAMS_BYTES {
        return Err(GeoCmdError::InputLimit("params_bytes"));
    }
    let val = parse_with_limits(params_json, MAX_SEL_PARAMS_BYTES, MAX_SEL_PARAMS_DEPTH).map_err(map_canon_err)?;
    let CValue::Obj(o) = &val else {
        return Err(GeoCmdError::BadParams("params must be an object"));
    };
    match tool {
        MCP_CREATE_SELECTION => {
            let p = parse_mcp_create_selection(o)?;
            let res = create_selection(selections, tx, journal, principal, &p)?;
            to_canonical_bytes(&res.to_value()).map_err(|_| GeoCmdError::Internal("canonical_encode"))
        }
        MCP_RESOLVE_SELECTION => {
            let p = parse_mcp_resolve_selection(o)?;
            let res = resolve_selection(selections, tx, journal, principal, &p)?;
            to_canonical_bytes(&res.to_value()).map_err(|_| GeoCmdError::Internal("canonical_encode"))
        }
        MCP_COMBINE_SELECTION => {
            let p = parse_mcp_combine_selection(o)?;
            let res = combine_selection(selections, tx, journal, principal, &p)?;
            to_canonical_bytes(&res.to_value()).map_err(|_| GeoCmdError::Internal("canonical_encode"))
        }
        MCP_GROW_SELECTION => {
            let p = parse_mcp_grow_selection(o)?;
            let res = grow_or_shrink_selection(selections, tx, journal, principal, &p)?;
            to_canonical_bytes(&res.to_value()).map_err(|_| GeoCmdError::Internal("canonical_encode"))
        }
        MCP_DESCRIBE_SELECTION => {
            let id = parse_uuid_field(o, "selection")?;
            let res = describe_selection(selections, tx.tree, log, Surface::Mcp, id)?;
            to_canonical_bytes(&res.to_value()).map_err(|_| GeoCmdError::Internal("canonical_encode"))
        }
        other => Err(GeoCmdError::UnknownCommand(kept(other))),
    }
}

// ================================================================== Registry definitions

/// Returns the five registry command definitions for `GEO.SEL.CMD` (`AC-6`).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let uuid_schema = || obj([("type", text("string")), ("pattern", text(ID_PATTERN))]);
    let domain_schema = || obj([("type", text("string")), ("enum", strs(&["points", "triangles", "faces", "edges"]))]);
    let authored_len = || {
        obj([
            ("type", text("object")),
            ("additionalProperties", CValue::Bool(false)),
            ("required", strs(&["text", "unit"])),
            ("properties", obj([("text", obj([("type", text("string"))])), ("unit", obj([("type", text("string"))]))])),
        ])
    };
    let sel_result = || {
        obj([
            ("type", text("object")),
            ("additionalProperties", CValue::Bool(false)),
            ("required", strs(&["count", "digest", "domain", "id", "ranges_count", "target", "target_digest"])),
            (
                "properties",
                obj([
                    ("count", obj([("type", text("integer")), ("minimum", CValue::Num(0.0))])),
                    ("digest", obj([("type", text("string"))])),
                    ("domain", domain_schema()),
                    ("id", uuid_schema()),
                    ("ranges_count", obj([("type", text("integer")), ("minimum", CValue::Num(0.0))])),
                    ("target", uuid_schema()),
                    ("target_digest", obj([("type", text("string"))])),
                ]),
            ),
        ])
    };

    let create_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["indices", "target"])),
        (
            "properties",
            obj([
                ("domain", domain_schema()),
                (
                    "indices",
                    obj([
                        ("type", text("array")),
                        ("items", obj([("type", text("integer")), ("minimum", CValue::Num(0.0))])),
                        ("maxItems", CValue::Num(MAX_INLINE_INDICES as f64)),
                    ]),
                ),
                ("target", obj([("type", text("string")), ("x-ref", text("element")), ("pattern", text(ID_PATTERN))])),
            ]),
        ),
    ]);

    let resolve_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["camera", "target"])),
        (
            "properties",
            obj([
                ("camera", obj([("type", text("object")), ("description", text("Explicit CPU camera parameters."))])),
                ("domain", domain_schema()),
                ("lasso", obj([("type", text("object")), ("description", text("Screen-space closed polygon."))])),
                ("mode", obj([("type", text("string")), ("enum", strs(&["through", "visible"]))])),
                (
                    "pick",
                    obj([("type", text("object")), ("description", text("Screen-space pixel pick and tolerance."))]),
                ),
                ("rule", obj([("type", text("string")), ("enum", strs(&["all_vertices", "any_vertex", "centroid"]))])),
                ("target", obj([("type", text("string")), ("x-ref", text("element")), ("pattern", text(ID_PATTERN))])),
            ]),
        ),
    ]);

    let combine_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["a", "op"])),
        (
            "properties",
            obj([
                ("a", uuid_schema()),
                ("b", uuid_schema()),
                (
                    "op",
                    obj([
                        ("type", text("string")),
                        ("enum", strs(&["union", "intersection", "difference", "sym_difference", "invert"])),
                    ]),
                ),
            ]),
        ),
    ]);

    let grow_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["selection"])),
        (
            "properties",
            obj([
                ("adjacency", obj([("type", text("string")), ("enum", strs(&["edge", "vertex"]))])),
                ("radius", authored_len()),
                ("rings", obj([("type", text("integer")), ("minimum", CValue::Num(0.0))])),
                ("selection", uuid_schema()),
                ("shrink", obj([("type", text("boolean"))])),
            ]),
        ),
    ]);

    let desc_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["selection"])),
        ("properties", obj([("selection", uuid_schema())])),
    ]);
    let desc_result = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["count", "digest", "domain", "id", "ranges_count", "stale", "target", "target_digest"])),
        (
            "properties",
            obj([
                ("count", obj([("type", text("integer")), ("minimum", CValue::Num(0.0))])),
                ("digest", obj([("type", text("string"))])),
                ("domain", domain_schema()),
                ("id", uuid_schema()),
                ("ranges_count", obj([("type", text("integer")), ("minimum", CValue::Num(0.0))])),
                ("stale", obj([("type", text("boolean"))])),
                ("target", uuid_schema()),
                ("target_digest", obj([("type", text("string"))])),
            ]),
        ),
    ]);

    Ok(vec![
        build_cmd(
            "geo.create_selection",
            "Create explicit selection",
            "Creates an index-set selection from an explicit list of up to 1 000 000 indices.",
            "W2",
            "head_move",
            &["SelectionCreated"],
            create_params,
            sel_result(),
        )?,
        build_cmd(
            "geo.resolve_selection",
            "Resolve pick or lasso selection",
            "Resolves a pixel pick or lasso polygon on the CPU into a geometry-bound index-set selection.",
            "W2",
            "head_move",
            &["SelectionCreated"],
            resolve_params,
            sel_result(),
        )?,
        build_cmd(
            "geo.combine_selection",
            "Combine selections",
            "Combines selections via set algebra (union, intersection, difference, sym_difference, invert).",
            "W2",
            "head_move",
            &["SelectionCreated"],
            combine_params,
            sel_result(),
        )?,
        build_cmd(
            "geo.grow_selection",
            "Grow or shrink selection",
            "Expands or shrinks a selection by mesh rings or an authored-length radius.",
            "W2",
            "head_move",
            &["SelectionCreated"],
            grow_params,
            sel_result(),
        )?,
        build_cmd(
            "geo.describe_selection",
            "Describe selection",
            "Reports a selection's domain, member count, range count, target, digests and staleness.",
            "R0",
            "not_undoable",
            &[],
            desc_params,
            desc_result,
        )?,
    ])
}

#[allow(clippy::too_many_arguments)]
fn build_cmd(
    id: &str,
    title: &str,
    description: &str,
    side_effect: &str,
    undo: &str,
    effects: &[&str],
    params_schema: CValue,
    result_schema: CValue,
) -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj([
        ("algorithms", CValue::Arr(Vec::new())),
        ("cli_bare", CValue::Bool(false)),
        ("description", text(description)),
        ("determinism", text("D0")),
        ("effects", strs(effects)),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text(id)),
        ("idempotency", text(if side_effect == "R0" || side_effect == "R1" { "natural" } else { "none" })),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text(side_effect)),
        ("title", text(title)),
        ("undo", text(undo)),
        ("web", text("available")),
    ]))
}

// ================================================================== Parsing & encoding helpers

/// Parses compact CLI index specs such as `"1-10,20"` (inclusive ranges) up to `MAX_INLINE_INDICES`.
pub fn parse_index_spec(spec: &str) -> Result<Vec<u64>, GeoCmdError> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for tok in trimmed.split(',') {
        let part = tok.trim();
        if part.is_empty() {
            return Err(GeoCmdError::BadParams("empty index token"));
        }
        if let Some((a_s, b_s)) = part.split_once('-') {
            let a: u64 = a_s.trim().parse().map_err(|_| GeoCmdError::BadParams("bad range start"))?;
            let b: u64 = b_s.trim().parse().map_err(|_| GeoCmdError::BadParams("bad range end"))?;
            if a > b {
                return Err(GeoCmdError::BadParams("reversed index range"));
            }
            let span = b
                .checked_sub(a)
                .and_then(|d| d.checked_add(1))
                .ok_or(GeoCmdError::TooManyIndices { count: usize::MAX, max: MAX_INLINE_INDICES })?;
            let span_usize = usize::try_from(span)
                .map_err(|_| GeoCmdError::TooManyIndices { count: usize::MAX, max: MAX_INLINE_INDICES })?;
            let next_len = out.len().saturating_add(span_usize);
            if next_len > MAX_INLINE_INDICES {
                return Err(GeoCmdError::TooManyIndices { count: next_len, max: MAX_INLINE_INDICES });
            }
            for idx in a..=b {
                out.push(idx);
            }
        } else {
            let v: u64 = part.parse().map_err(|_| GeoCmdError::BadParams("bad index"))?;
            let next_len = out.len().saturating_add(1);
            if next_len > MAX_INLINE_INDICES {
                return Err(GeoCmdError::TooManyIndices { count: next_len, max: MAX_INLINE_INDICES });
            }
            out.push(v);
        }
    }
    Ok(out)
}

fn parse_cli_create_selection(rest: &[&str]) -> Result<CreateSelectionParams, GeoCmdError> {
    let mut target: Option<&str> = None;
    let mut indices_s: Option<&str> = None;
    let mut domain_s: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--json" => {}
            "--indices" => {
                i += 1;
                indices_s = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --indices value"))?);
            }
            "--domain" => {
                i += 1;
                domain_s = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --domain value"))?);
            }
            "--target" => {
                i += 1;
                target = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --target value"))?);
            }
            s if s.starts_with('-') => return Err(GeoCmdError::BadParams("unknown option")),
            s if target.is_none() => target = Some(s),
            _ => return Err(GeoCmdError::BadParams("unexpected positional")),
        }
        i += 1;
    }
    let target = Uuid::parse(target.ok_or(GeoCmdError::BadParams("missing target"))?)
        .map_err(|_| GeoCmdError::BadParams("bad target uuid"))?;
    let domain = match domain_s {
        None => None,
        Some(d) => Some(parse_domain(d)?),
    };
    let indices = parse_index_spec(indices_s.ok_or(GeoCmdError::BadParams("missing --indices"))?)?;
    Ok(CreateSelectionParams { target, domain, indices })
}

fn read_json_arg(arg: &str) -> Result<CValue, GeoCmdError> {
    let trimmed = arg.trim();
    let bytes = if trimmed.starts_with('{') || trimmed.starts_with('[') {
        trimmed.as_bytes().to_vec()
    } else {
        std::fs::read(trimmed).map_err(|_| GeoCmdError::BadParams("cannot read json file"))?
    };
    parse_with_limits(&bytes, MAX_SEL_PARAMS_BYTES, MAX_SEL_PARAMS_DEPTH).map_err(map_canon_err)
}

fn parse_cli_resolve_selection(rest: &[&str]) -> Result<ResolveSelectionParams, GeoCmdError> {
    let mut target: Option<&str> = None;
    let mut camera_s: Option<&str> = None;
    let mut lasso_s: Option<&str> = None;
    let mut pick_s: Option<&str> = None;
    let mut domain_s: Option<&str> = None;
    let mut rule_s: Option<&str> = None;
    let mut mode_s: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--json" => {}
            "--target" => {
                i += 1;
                target = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --target"))?);
            }
            "--camera" => {
                i += 1;
                camera_s = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --camera"))?);
            }
            "--lasso" => {
                i += 1;
                lasso_s = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --lasso"))?);
            }
            "--pick" => {
                i += 1;
                pick_s = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --pick"))?);
            }
            "--domain" => {
                i += 1;
                domain_s = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --domain"))?);
            }
            "--rule" => {
                i += 1;
                rule_s = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --rule"))?);
            }
            "--mode" => {
                i += 1;
                mode_s = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --mode"))?);
            }
            s if s.starts_with('-') => return Err(GeoCmdError::BadParams("unknown option")),
            s if target.is_none() => target = Some(s),
            _ => return Err(GeoCmdError::BadParams("unexpected positional")),
        }
        i += 1;
    }
    let target = Uuid::parse(target.ok_or(GeoCmdError::BadParams("missing target"))?)
        .map_err(|_| GeoCmdError::BadParams("bad target uuid"))?;
    let domain = domain_s.map(parse_domain).transpose()?;
    let rule = match rule_s {
        None => TriangleRule::AllVertices,
        Some(s) => parse_triangle_rule(s)?,
    };
    let mode = match mode_s {
        None => ResolveMode::Through,
        Some(s) => parse_resolve_mode(s, None)?,
    };
    let cam_val = read_json_arg(camera_s.ok_or(GeoCmdError::BadParams("missing --camera"))?)?;
    let camera = parse_camera_value(&cam_val)?;
    let input = match (pick_s, lasso_s) {
        (Some(ps), None) => {
            let v = read_json_arg(ps)?;
            parse_pick_value(&v)?
        }
        (None, Some(ls)) => {
            let v = read_json_arg(ls)?;
            ResolveInput::Lasso(parse_lasso_value(&v)?)
        }
        _ => return Err(GeoCmdError::BadParams("specify exactly one of --pick or --lasso")),
    };
    Ok(ResolveSelectionParams { target, domain, camera, input, rule, mode })
}

fn parse_cli_combine_selection(rest: &[&str]) -> Result<CombineSelectionParams, GeoCmdError> {
    let mut op_s: Option<&str> = None;
    let mut pos: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--json" => {}
            "--op" => {
                i += 1;
                op_s = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --op"))?);
            }
            s if s.starts_with('-') => return Err(GeoCmdError::BadParams("unknown option")),
            s => pos.push(s),
        }
        i += 1;
    }
    let op = CombineOp::parse(op_s.ok_or(GeoCmdError::BadParams("missing --op"))?)
        .ok_or(GeoCmdError::BadParams("unknown --op"))?;
    let a = Uuid::parse(pos.first().ok_or(GeoCmdError::BadParams("missing first selection"))?)
        .map_err(|_| GeoCmdError::BadParams("bad selection uuid"))?;
    let b = match pos.get(1) {
        None => None,
        Some(s) => Some(Uuid::parse(s).map_err(|_| GeoCmdError::BadParams("bad second selection uuid"))?),
    };
    if pos.len() > 2 {
        return Err(GeoCmdError::BadParams("too many selection arguments"));
    }
    Ok(CombineSelectionParams { op, a, b })
}

fn parse_cli_grow_selection(rest: &[&str]) -> Result<GrowSelectionParams, GeoCmdError> {
    let mut sel: Option<&str> = None;
    let mut rings: Option<u32> = None;
    let mut adj: Option<Adjacency> = None;
    let mut radius: Option<AuthoredLengthSpec> = None;
    let mut shrink = false;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--json" => {}
            "--shrink" => shrink = true,
            "--selection" => {
                i += 1;
                sel = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --selection"))?);
            }
            "--rings" => {
                i += 1;
                rings = Some(
                    rest.get(i)
                        .ok_or(GeoCmdError::BadParams("missing --rings"))?
                        .parse::<u32>()
                        .map_err(|_| GeoCmdError::BadParams("bad --rings"))?,
                );
            }
            "--adjacency" => {
                i += 1;
                adj = Some(parse_adjacency(rest.get(i).ok_or(GeoCmdError::BadParams("missing --adjacency"))?)?);
            }
            "--radius" => {
                i += 1;
                radius = Some(AuthoredLengthSpec::parse_combined(
                    rest.get(i).ok_or(GeoCmdError::BadParams("missing --radius"))?,
                )?);
            }
            s if s.starts_with('-') => return Err(GeoCmdError::BadParams("unknown option")),
            s if sel.is_none() => sel = Some(s),
            _ => return Err(GeoCmdError::BadParams("unexpected positional")),
        }
        i += 1;
    }
    let selection = Uuid::parse(sel.ok_or(GeoCmdError::BadParams("missing selection"))?)
        .map_err(|_| GeoCmdError::BadParams("bad selection uuid"))?;
    let spec = match (rings, radius) {
        (Some(r), None) => GrowParam::Rings { rings: r, adjacency: adj.unwrap_or(Adjacency::Edge) },
        (None, Some(rad)) => GrowParam::Radius(rad),
        _ => return Err(GeoCmdError::BadParams("specify exactly one of --rings or --radius")),
    };
    Ok(GrowSelectionParams { selection, spec, shrink })
}

fn parse_cli_describe_selection(rest: &[&str]) -> Result<EntityId, GeoCmdError> {
    let mut sel: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--json" => {}
            "--selection" => {
                i += 1;
                sel = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --selection"))?);
            }
            s if s.starts_with('-') => return Err(GeoCmdError::BadParams("unknown option")),
            s if sel.is_none() => sel = Some(s),
            _ => return Err(GeoCmdError::BadParams("unexpected positional")),
        }
        i += 1;
    }
    Uuid::parse(sel.ok_or(GeoCmdError::BadParams("missing selection"))?)
        .map_err(|_| GeoCmdError::BadParams("bad selection uuid"))
}

fn parse_mcp_create_selection(o: &BTreeMap<String, CValue>) -> Result<CreateSelectionParams, GeoCmdError> {
    let target = parse_uuid_field(o, "target")?;
    let domain = match o.get("domain") {
        None | Some(CValue::Null) => None,
        Some(CValue::Str(s)) => Some(parse_domain(s)?),
        _ => return Err(GeoCmdError::BadParams("bad domain")),
    };
    let Some(CValue::Arr(arr)) = o.get("indices") else {
        return Err(GeoCmdError::BadParams("indices must be array"));
    };
    if arr.len() > MAX_INLINE_INDICES {
        return Err(GeoCmdError::TooManyIndices { count: arr.len(), max: MAX_INLINE_INDICES });
    }
    let mut indices = Vec::with_capacity(arr.len());
    for item in arr {
        indices.push(parse_u64_val(item)?);
    }
    Ok(CreateSelectionParams { target, domain, indices })
}

fn parse_mcp_resolve_selection(o: &BTreeMap<String, CValue>) -> Result<ResolveSelectionParams, GeoCmdError> {
    let target = parse_uuid_field(o, "target")?;
    let domain = match o.get("domain") {
        None | Some(CValue::Null) => None,
        Some(CValue::Str(s)) => Some(parse_domain(s)?),
        _ => return Err(GeoCmdError::BadParams("bad domain")),
    };
    let camera = parse_camera_value(o.get("camera").ok_or(GeoCmdError::BadParams("missing camera"))?)?;
    let input = match (o.get("pick"), o.get("lasso")) {
        (Some(p), None) => parse_pick_value(p)?,
        (None, Some(l)) => ResolveInput::Lasso(parse_lasso_value(l)?),
        _ => return Err(GeoCmdError::BadParams("specify exactly one of pick or lasso")),
    };
    let rule = match o.get("rule") {
        None | Some(CValue::Null) => TriangleRule::AllVertices,
        Some(CValue::Str(s)) => parse_triangle_rule(s)?,
        _ => return Err(GeoCmdError::BadParams("bad rule")),
    };
    let depth_tol = match o.get("depth_tol") {
        None | Some(CValue::Null) => None,
        Some(v) => Some(parse_f64_val(v)?),
    };
    let mode = match o.get("mode") {
        None | Some(CValue::Null) => ResolveMode::Through,
        Some(CValue::Str(s)) => parse_resolve_mode(s, depth_tol)?,
        _ => return Err(GeoCmdError::BadParams("bad mode")),
    };
    Ok(ResolveSelectionParams { target, domain, camera, input, rule, mode })
}

fn parse_mcp_combine_selection(o: &BTreeMap<String, CValue>) -> Result<CombineSelectionParams, GeoCmdError> {
    let op = CombineOp::parse(req_str(o, "op")?).ok_or(GeoCmdError::BadParams("unknown op"))?;
    let a = parse_uuid_field(o, "a")?;
    let b = match o.get("b") {
        None | Some(CValue::Null) => None,
        Some(CValue::Str(s)) => Some(Uuid::parse(s).map_err(|_| GeoCmdError::BadParams("bad b uuid"))?),
        _ => return Err(GeoCmdError::BadParams("bad b")),
    };
    Ok(CombineSelectionParams { op, a, b })
}

fn parse_mcp_grow_selection(o: &BTreeMap<String, CValue>) -> Result<GrowSelectionParams, GeoCmdError> {
    let selection = parse_uuid_field(o, "selection")?;
    let shrink = match o.get("shrink") {
        None | Some(CValue::Null) => false,
        Some(CValue::Bool(b)) => *b,
        _ => return Err(GeoCmdError::BadParams("bad shrink")),
    };
    let spec = match (o.get("rings"), o.get("radius")) {
        (Some(rv), None) => {
            let rings_u64 = parse_u64_val(rv)?;
            let rings = u32::try_from(rings_u64).map_err(|_| GeoCmdError::BadParams("rings overflow"))?;
            let adjacency = match o.get("adjacency") {
                None | Some(CValue::Null) => Adjacency::Edge,
                Some(CValue::Str(s)) => parse_adjacency(s)?,
                _ => return Err(GeoCmdError::BadParams("bad adjacency")),
            };
            GrowParam::Rings { rings, adjacency }
        }
        (None, Some(CValue::Obj(ro))) => {
            GrowParam::Radius(AuthoredLengthSpec::new(req_str(ro, "text")?, req_str(ro, "unit")?))
        }
        _ => return Err(GeoCmdError::BadParams("specify exactly one of rings or radius")),
    };
    Ok(GrowSelectionParams { selection, spec, shrink })
}

fn parse_camera_value(v: &CValue) -> Result<Camera, GeoCmdError> {
    let CValue::Obj(o) = v else {
        return Err(GeoCmdError::BadCamera("camera must be object".to_owned()));
    };
    let vp_arr = match o.get("viewport") {
        Some(CValue::Arr(a)) if a.len() == 2 => a,
        _ => return Err(GeoCmdError::BadCamera("viewport must be [w, h]".to_owned())),
    };
    let vw = u32::try_from(parse_u64_val(&vp_arr[0]).map_err(|_| GeoCmdError::BadCamera("bad viewport w".to_owned()))?)
        .map_err(|_| GeoCmdError::BadCamera("viewport w overflow".to_owned()))?;
    let vh = u32::try_from(parse_u64_val(&vp_arr[1]).map_err(|_| GeoCmdError::BadCamera("bad viewport h".to_owned()))?)
        .map_err(|_| GeoCmdError::BadCamera("viewport h overflow".to_owned()))?;

    let proj = match o.get("projection").and_then(|x| match x {
        CValue::Str(s) => Some(s.as_str()),
        _ => None,
    }) {
        Some("perspective") => Projection::Perspective {
            fov_y: req_f64_cam(o, "fov_y")?,
            aspect: req_f64_cam(o, "aspect")?,
            near: req_f64_cam(o, "near")?,
            far: req_f64_cam(o, "far")?,
        },
        Some("ortho") => Projection::Ortho {
            height: req_f64_cam(o, "height")?,
            aspect: req_f64_cam(o, "aspect")?,
            near: req_f64_cam(o, "near")?,
            far: req_f64_cam(o, "far")?,
        },
        _ => return Err(GeoCmdError::BadCamera("projection must be perspective or ortho".to_owned())),
    };

    let r_rows = match o.get("view_r") {
        Some(CValue::Arr(rows)) if rows.len() == 3 => rows,
        _ => return Err(GeoCmdError::BadCamera("view_r must be 3x3 array".to_owned())),
    };
    let r = [parse_f64_vec3(&r_rows[0])?, parse_f64_vec3(&r_rows[1])?, parse_f64_vec3(&r_rows[2])?];
    let t = parse_f64_vec3(o.get("view_t").ok_or_else(|| GeoCmdError::BadCamera("missing view_t".to_owned()))?)?;
    let view = Rigid::new(r, t).map_err(|e| GeoCmdError::BadCamera(format!("{e:?}")))?;
    Ok(Camera { view, proj, viewport: (vw, vh) })
}

fn parse_pick_value(v: &CValue) -> Result<ResolveInput, GeoCmdError> {
    let CValue::Obj(o) = v else {
        return Err(GeoCmdError::BadParams("pick must be object"));
    };
    let x = parse_f64_val(o.get("x").ok_or(GeoCmdError::BadParams("pick missing x"))?)?;
    let y = parse_f64_val(o.get("y").ok_or(GeoCmdError::BadParams("pick missing y"))?)?;
    let tol_px = match o.get("tol_px") {
        None | Some(CValue::Null) => 4.0,
        Some(tv) => parse_f64_val(tv)?,
    };
    Ok(ResolveInput::Pick { pixel: (x, y), tol_px })
}

fn parse_lasso_value(v: &CValue) -> Result<Lasso, GeoCmdError> {
    let arr = match v {
        CValue::Arr(a) => a,
        CValue::Obj(o) => match o.get("pixels") {
            Some(CValue::Arr(a)) => a,
            _ => return Err(GeoCmdError::BadPolygon("lasso missing pixels array".to_owned())),
        },
        _ => return Err(GeoCmdError::BadPolygon("lasso must be object or array".to_owned())),
    };
    let mut pixels = Vec::with_capacity(arr.len());
    for item in arr {
        let CValue::Arr(pair) = item else {
            return Err(GeoCmdError::BadPolygon("lasso vertex must be [x, y]".to_owned()));
        };
        if pair.len() != 2 {
            return Err(GeoCmdError::BadPolygon("lasso vertex must have 2 coordinates".to_owned()));
        }
        let x = parse_f64_val(&pair[0]).map_err(|_| GeoCmdError::BadPolygon("bad lasso x".to_owned()))?;
        let y = parse_f64_val(&pair[1]).map_err(|_| GeoCmdError::BadPolygon("bad lasso y".to_owned()))?;
        pixels.push((x, y));
    }
    Lasso::new(pixels).map_err(map_sel_err)
}

fn encode_create_args(target: EntityId, domain: Domain, indices: &[u64]) -> CValue {
    obj([
        ("domain", text(domain.as_str())),
        ("indices", CValue::Arr(indices.iter().map(|&i| CValue::Num(i as f64)).collect())),
        ("target", text(&target.to_string())),
    ])
}

fn encode_camera_value(cam: &Camera) -> CValue {
    let r = cam.view.r();
    let t = cam.view.t();
    let mut m = BTreeMap::new();
    match cam.proj {
        Projection::Perspective { fov_y, aspect, near, far } => {
            m.insert("aspect".to_owned(), hex_f64_val(aspect));
            m.insert("far".to_owned(), hex_f64_val(far));
            m.insert("fov_y".to_owned(), hex_f64_val(fov_y));
            m.insert("near".to_owned(), hex_f64_val(near));
            m.insert("projection".to_owned(), text("perspective"));
        }
        Projection::Ortho { height, aspect, near, far } => {
            m.insert("aspect".to_owned(), hex_f64_val(aspect));
            m.insert("far".to_owned(), hex_f64_val(far));
            m.insert("height".to_owned(), hex_f64_val(height));
            m.insert("near".to_owned(), hex_f64_val(near));
            m.insert("projection".to_owned(), text("ortho"));
        }
    }
    m.insert(
        "view_r".to_owned(),
        CValue::Arr(r.iter().map(|row| CValue::Arr(row.iter().map(|&x| hex_f64_val(x)).collect())).collect()),
    );
    m.insert("view_t".to_owned(), CValue::Arr(t.iter().map(|&x| hex_f64_val(x)).collect()));
    m.insert(
        "viewport".to_owned(),
        CValue::Arr(vec![CValue::Num(f64::from(cam.viewport.0)), CValue::Num(f64::from(cam.viewport.1))]),
    );
    CValue::Obj(m)
}

fn encode_resolve_args(params: &ResolveSelectionParams, domain: Domain) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("camera".to_owned(), encode_camera_value(&params.camera));
    m.insert("domain".to_owned(), text(domain.as_str()));
    match &params.input {
        ResolveInput::Pick { pixel, tol_px } => {
            m.insert(
                "pick".to_owned(),
                obj([("tol_px", hex_f64_val(*tol_px)), ("x", hex_f64_val(pixel.0)), ("y", hex_f64_val(pixel.1))]),
            );
        }
        ResolveInput::Lasso(l) => {
            let pts =
                CValue::Arr(l.pixels.iter().map(|&(x, y)| CValue::Arr(vec![hex_f64_val(x), hex_f64_val(y)])).collect());
            m.insert("lasso".to_owned(), obj([("pixels", pts)]));
        }
    }
    match params.mode {
        ResolveMode::Through => {
            m.insert("mode".to_owned(), text("through"));
        }
        ResolveMode::Visible { depth_tol } => {
            m.insert("mode".to_owned(), text("visible"));
            if let Some(dt) = depth_tol {
                m.insert("depth_tol".to_owned(), hex_f64_val(dt));
            }
        }
    }
    m.insert("rule".to_owned(), text(params.rule.as_str()));
    m.insert("target".to_owned(), text(&params.target.to_string()));
    CValue::Obj(m)
}

fn encode_combine_args(params: &CombineSelectionParams) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("a".to_owned(), text(&params.a.to_string()));
    if let Some(b) = params.b {
        m.insert("b".to_owned(), text(&b.to_string()));
    }
    m.insert("op".to_owned(), text(params.op.as_str()));
    CValue::Obj(m)
}

fn lookup_geometry(tree: &dyn ElementTree, id: EntityId) -> Result<&GeometryElement, GeoCmdError> {
    match tree.get(&id) {
        None => Err(GeoCmdError::NotFound(id)),
        Some(TreeEntry::Other { .. }) => Err(GeoCmdError::NotGeometry(id)),
        Some(TreeEntry::Geometry(g)) => Ok(&g.element),
    }
}

fn default_domain(el: &GeometryElement) -> Domain {
    match el {
        GeometryElement::PointSet(_) | GeometryElement::PointCloud(_) => Domain::Points,
        GeometryElement::Mesh(_) => Domain::Triangles,
        GeometryElement::CadBody(_) => Domain::Faces,
    }
}

fn validate_fresh(sel_id: EntityId, sel: &Selection, elem: &GeometryElement) -> Result<(), GeoCmdError> {
    match apply(sel, elem) {
        Ok(_) => Ok(()),
        Err(SelError::StaleSelection { .. }) => Err(GeoCmdError::StaleSelection(sel_id)),
        Err(e) => Err(map_sel_err(e)),
    }
}

fn sel_content_digest(sel: &Selection) -> Result<Digest, GeoCmdError> {
    sel.content_digest()
        .map(|d| d.to_digest())
        .ok_or_else(|| GeoCmdError::Selection("selection has no index-set digest".to_owned()))
}

fn to_sel_entity(id: EntityId) -> SelEntityId {
    SelEntityId::from_bytes(*id.as_bytes())
}

fn from_sel_entity(id: SelEntityId) -> EntityId {
    Uuid::from_bytes(id.to_bytes())
}

fn map_sel_err(e: SelError) -> GeoCmdError {
    match e {
        SelError::StaleSelection { .. } => GeoCmdError::StaleSelection(Uuid::from_bytes([0u8; 16])),
        SelError::ElementMismatch { .. } => GeoCmdError::ElementMismatch(format!("{e:?}")),
        SelError::DomainMismatch { .. } | SelError::SetDomainMismatch { .. } => {
            GeoCmdError::DomainMismatch(format!("{e:?}"))
        }
        SelError::BadCamera { reason } => GeoCmdError::BadCamera(reason.to_owned()),
        SelError::BadPolygon { reason } => GeoCmdError::BadPolygon(reason.to_owned()),
        SelError::TooManyIndices { count, limit } => {
            GeoCmdError::TooManyIndices { count: count as usize, max: limit as usize }
        }
        other => GeoCmdError::Selection(format!("{other:?}")),
    }
}

fn map_canon_err(e: CanonError) -> GeoCmdError {
    match e {
        CanonError::InputLimit => GeoCmdError::InputLimit("params"),
        CanonError::DuplicateKey(_) => GeoCmdError::BadParams("duplicate key"),
        _ => GeoCmdError::BadParams("invalid json"),
    }
}

fn parse_domain(s: &str) -> Result<Domain, GeoCmdError> {
    match s {
        "points" => Ok(Domain::Points),
        "triangles" => Ok(Domain::Triangles),
        "faces" => Ok(Domain::Faces),
        "edges" => Ok(Domain::Edges),
        _ => Err(GeoCmdError::DomainMismatch(format!("unknown domain {}", kept(s)))),
    }
}

fn parse_triangle_rule(s: &str) -> Result<TriangleRule, GeoCmdError> {
    match s {
        "all_vertices" => Ok(TriangleRule::AllVertices),
        "any_vertex" => Ok(TriangleRule::AnyVertex),
        "centroid" => Ok(TriangleRule::Centroid),
        _ => Err(GeoCmdError::BadParams("unknown triangle rule")),
    }
}

fn parse_resolve_mode(s: &str, depth_tol: Option<f64>) -> Result<ResolveMode, GeoCmdError> {
    match s {
        "through" => Ok(ResolveMode::Through),
        "visible" => Ok(ResolveMode::Visible { depth_tol }),
        _ => Err(GeoCmdError::BadParams("unknown resolve mode")),
    }
}

fn parse_adjacency(s: &str) -> Result<Adjacency, GeoCmdError> {
    match s {
        "edge" => Ok(Adjacency::Edge),
        "vertex" => Ok(Adjacency::Vertex),
        _ => Err(GeoCmdError::BadParams("unknown adjacency")),
    }
}

fn adj_str(a: Adjacency) -> &'static str {
    match a {
        Adjacency::Edge => "edge",
        Adjacency::Vertex => "vertex",
    }
}

fn hex_f64_val(x: f64) -> CValue {
    text(&format!("0x{:016x}", x.to_bits()))
}

fn parse_f64_val(v: &CValue) -> Result<f64, GeoCmdError> {
    match v {
        CValue::Num(n) if n.is_finite() => Ok(*n),
        CValue::Str(s) => {
            if let Some(hex) = s.strip_prefix("0x") {
                let bits = u64::from_str_radix(hex, 16).map_err(|_| GeoCmdError::BadParams("bad hex f64"))?;
                let x = f64::from_bits(bits);
                if x.is_finite() {
                    Ok(x)
                } else {
                    Err(GeoCmdError::BadParams("non-finite f64"))
                }
            } else {
                let x: f64 = s.parse().map_err(|_| GeoCmdError::BadParams("bad decimal f64"))?;
                if x.is_finite() {
                    Ok(x)
                } else {
                    Err(GeoCmdError::BadParams("non-finite f64"))
                }
            }
        }
        _ => Err(GeoCmdError::BadParams("expected number or hex f64")),
    }
}

fn req_f64_cam(o: &BTreeMap<String, CValue>, k: &str) -> Result<f64, GeoCmdError> {
    let v = o.get(k).ok_or_else(|| GeoCmdError::BadCamera(format!("missing {k}")))?;
    parse_f64_val(v).map_err(|_| GeoCmdError::BadCamera(format!("bad {k}")))
}

fn parse_f64_vec3(v: &CValue) -> Result<[f64; 3], GeoCmdError> {
    let CValue::Arr(a) = v else {
        return Err(GeoCmdError::BadCamera("vec3 must be array".to_owned()));
    };
    if a.len() != 3 {
        return Err(GeoCmdError::BadCamera("vec3 len != 3".to_owned()));
    }
    Ok([
        parse_f64_val(&a[0]).map_err(|_| GeoCmdError::BadCamera("bad vec3[0]".to_owned()))?,
        parse_f64_val(&a[1]).map_err(|_| GeoCmdError::BadCamera("bad vec3[1]".to_owned()))?,
        parse_f64_val(&a[2]).map_err(|_| GeoCmdError::BadCamera("bad vec3[2]".to_owned()))?,
    ])
}

fn parse_u64_val(v: &CValue) -> Result<u64, GeoCmdError> {
    match v {
        CValue::Num(n) if n.is_finite() && *n >= 0.0 && (*n % 1.0 == 0.0) && *n <= ((1u64 << 53) as f64) => {
            Ok(*n as u64)
        }
        _ => Err(GeoCmdError::BadParams("expected non-negative integer")),
    }
}

fn value_digest(v: &CValue) -> Result<Digest, GeoCmdError> {
    let bytes = to_canonical_bytes(v).map_err(|_| GeoCmdError::Internal("canonical_encode"))?;
    Ok(Digest::from_bytes(digest_bytes(&bytes).0))
}

fn parse_uuid_field(o: &BTreeMap<String, CValue>, k: &str) -> Result<Uuid, GeoCmdError> {
    Uuid::parse(req_str(o, k)?).map_err(|_| GeoCmdError::BadParams("bad uuid"))
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, GeoCmdError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(GeoCmdError::BadParams("missing string field")),
    }
}

fn unreachable_combine_err() -> Result<Selection, SelError> {
    Err(SelError::NotIndexSet)
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn obj<const N: usize>(entries: [(&str, CValue); N]) -> CValue {
    let mut m = BTreeMap::new();
    for (k, v) in entries {
        m.insert(k.to_owned(), v);
    }
    CValue::Obj(m)
}
