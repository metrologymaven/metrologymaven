//! Frame, transform and unit commands (`GEO.FRAME.CMD`, `spec/geo/frame/cmd/NODE.md`,
//! `REQ-001`..`REQ-006`, `AC-1`..`AC-6`).
//!
//! Six D0 commands registered in `CORE.REG`, exposed on CLI and MCP (`mcp_projection: "exposed"`):
//!
//! - `geo.create_frame@1` (W2): creates a frame with authored length translation records
//!   `{ text, unit, si_hex }` and rotation (matrix, quaternion, axis-angle, or Euler).
//! - `geo.set_transform@1` (W2): updates a frame's transform (and optional parent, rejecting
//!   cycles) and propagates `outdated` via `CORE.DEP` to derived elements.
//! - `geo.delete_frame@1` (W2): deletes an unused frame; fails `frame_in_use` (listing up to
//!   20 referencing IDs) and journals nothing if any child frame or element references it.
//! - `geo.apply_transform@1` (W2): transforms a geometric element into a target frame via
//!   `GEO.FRAME.APPLY` and registers it as a derived UUIDv5 element with dependency edges on
//!   the source element and both frames.
//! - `geo.resolve_transform@1` (R1): resolves the 4x4 matrix (decimal + IEEE hex) and scale
//!   between two frames; writes no Record Journal entry.
//! - `geo.convert_units@1` (R1): parses an authored quantity with single rounding and
//!   optionally formats into a target unit; writes no Record Journal entry.

use std::collections::{BTreeMap, BTreeSet};

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_dep::{propagate, EdgesSet, ElementState, Propagation};
use fid_geo_frame::apply::transform_element_with_provenance;
use fid_geo_frame::rotation::{AxisAngle, Euler, EulerKind, EulerSeq, Quaternion};
use fid_geo_frame::tree::{Frame, FrameError, FrameId, FrameKind, FrameTree};
use fid_geo_frame::units::{format as format_unit, AngleUnit, AuthoredQuantity, LengthUnit, Unit};
use fid_geo_frame::xform::{Matrix3, Rigid, Similarity, Transform, XformError};
use fid_geo_types::{FrameRef, GeometryElement};
use fid_project::IdService;
use fid_registry::{CommandDef, DefError};
use fid_types::{CommandId, Digest};
use fid_uuid::Uuid;

use crate::describe::{ActivityEntry, Surface};
use crate::error::{kept, GeoCmdError};
use crate::register::{register_element, Derivation, EntityId, GeoTx, IdOrigin, TreeEntry};

/// Command IDs (`REQ-001`, `REQ-005`).
pub const CREATE_FRAME: &str = "geo.create_frame@1";
pub const SET_TRANSFORM: &str = "geo.set_transform@1";
pub const DELETE_FRAME: &str = "geo.delete_frame@1";
pub const APPLY_TRANSFORM: &str = "geo.apply_transform@1";
pub const RESOLVE_TRANSFORM: &str = "geo.resolve_transform@1";
pub const CONVERT_UNITS: &str = "geo.convert_units@1";

/// CLI subcommand names (`fid geo <subcommand>`).
pub const CLI_CREATE_FRAME: &str = "create-frame";
pub const CLI_SET_TRANSFORM: &str = "set-transform";
pub const CLI_DELETE_FRAME: &str = "delete-frame";
pub const CLI_APPLY_TRANSFORM: &str = "apply-transform";
pub const CLI_RESOLVE_TRANSFORM: &str = "resolve-transform";
pub const CLI_CONVERT_UNITS: &str = "convert-units";

/// MCP tool names (`AC-6`).
pub const MCP_CREATE_FRAME: &str = "geo_create_frame";
pub const MCP_SET_TRANSFORM: &str = "geo_set_transform";
pub const MCP_DELETE_FRAME: &str = "geo_delete_frame";
pub const MCP_APPLY_TRANSFORM: &str = "geo_apply_transform";
pub const MCP_RESOLVE_TRANSFORM: &str = "geo_resolve_transform";
pub const MCP_CONVERT_UNITS: &str = "geo_convert_units";

/// Maximum referencing entity IDs returned in `frame_in_use` (`REQ-GEO.FRAME.CMD-003`).
pub const MAX_FRAME_REFS_REPORTED: usize = 20;
const MAX_CLI_ARGS: usize = 32;
const MAX_FRAME_PARAMS_BYTES: usize = 64 * 1024;
const MAX_FRAME_PARAMS_DEPTH: usize = 10;
const LIFECYCLE_SINCE: &str = "2026-11-23";
const ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";

/// Queries which elements in the project tree currently reference a given frame
/// (`REQ-GEO.FRAME.CMD-003`).
pub trait FrameUsageIndex {
    fn elements_in_frame(&self, frame: FrameRef) -> Vec<EntityId>;
}

impl FrameUsageIndex for BTreeMap<EntityId, TreeEntry> {
    fn elements_in_frame(&self, frame: FrameRef) -> Vec<EntityId> {
        self.iter()
            .filter_map(|(&id, entry)| match entry {
                TreeEntry::Geometry(g) if g.facts.frame == frame => Some(id),
                _ => None,
            })
            .collect()
    }
}

/// Caller principal context for W2 branch routing (`REQ-GEO.FRAME.CMD-006`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramePrincipal {
    /// Human operator committing to the main branch.
    Human { id: String },
    /// Agent principal committing W2 commands to its dedicated branch (`REQ-006`).
    Agent { id: String, branch: String },
}

impl FramePrincipal {
    pub fn branch(&self) -> Option<&str> {
        match self {
            FramePrincipal::Human { .. } => None,
            FramePrincipal::Agent { branch, .. } => Some(branch.as_str()),
        }
    }

    pub fn principal_id(&self) -> &str {
        match self {
            FramePrincipal::Human { id } | FramePrincipal::Agent { id, .. } => id.as_str(),
        }
    }
}

/// Authored length specification `{ text, unit }` parsed once to SI (`REQ-002`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredLengthSpec {
    pub text: String,
    pub unit: String,
}

impl AuthoredLengthSpec {
    pub fn new(text: impl Into<String>, unit: impl Into<String>) -> Self {
        Self { text: text.into(), unit: unit.into() }
    }

    /// Parses `"10 mm"` or `"5mm"` into `AuthoredLengthSpec`.
    pub fn parse_combined(s: &str) -> Result<Self, GeoCmdError> {
        let trimmed = s.trim();
        let idx = trimmed
            .find(|c: char| c.is_ascii_alphabetic() || c == 'µ' || c == 'μ' || c == '°' || c == '′' || c == '″')
            .ok_or_else(|| GeoCmdError::BadQuantity(kept(trimmed)))?;
        let text = trimmed[..idx].trim();
        let unit = trimmed[idx..].trim();
        if text.is_empty() || unit.is_empty() {
            return Err(GeoCmdError::BadQuantity(kept(trimmed)));
        }
        Ok(Self { text: text.to_owned(), unit: unit.to_owned() })
    }

    pub fn to_authored(&self) -> Result<AuthoredQuantity, GeoCmdError> {
        let Some(Unit::Length(lu)) = Unit::from_symbol(&self.unit) else {
            return Err(GeoCmdError::BadQuantity(format!("unknown length unit {}", kept(&self.unit))));
        };
        AuthoredQuantity::parse(&self.text, Unit::Length(lu)).map_err(|e| GeoCmdError::BadQuantity(format!("{e:?}")))
    }
}

/// Recorded single-rounding conversion entry `{ text, unit, si_hex }` (`REQ-002`, `AC-1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionRecord {
    pub text: String,
    pub unit: String,
    pub si_hex: String,
}

impl ConversionRecord {
    pub fn from_authored(q: &AuthoredQuantity) -> Self {
        Self { text: q.text().to_owned(), unit: q.unit().symbol().to_owned(), si_hex: q.si_hex() }
    }

    pub fn to_value(&self) -> CValue {
        obj([("si_hex", text(&self.si_hex)), ("text", text(&self.text)), ("unit", text(&self.unit))])
    }
}

/// Tagged union of supported rotation parameterisations (`GEO.FRAME.CMD` Design).
#[derive(Debug, Clone, PartialEq)]
pub enum RotationParam {
    Identity,
    Matrix(Matrix3),
    Quaternion { w: f64, x: f64, y: f64, z: f64 },
    AxisAngle { axis: [f64; 3], angle_rad: f64 },
    Euler { seq: EulerSeq, kind: EulerKind, angles_rad: [f64; 3] },
}

impl RotationParam {
    pub fn to_matrix(&self) -> Result<Matrix3, GeoCmdError> {
        match *self {
            RotationParam::Identity => Ok([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
            RotationParam::Matrix(m) => {
                Rigid::new(m, [0.0, 0.0, 0.0]).map_err(map_xform_err)?;
                Ok(m)
            }
            RotationParam::Quaternion { w, x, y, z } => Quaternion { w, x, y, z }.to_matrix().map_err(map_xform_err),
            RotationParam::AxisAngle { axis, angle_rad } => {
                AxisAngle { axis, angle: angle_rad }.to_matrix().map_err(map_xform_err)
            }
            RotationParam::Euler { seq, kind, angles_rad } => {
                Euler { seq, kind, angles: angles_rad }.to_matrix().map_err(map_xform_err)
            }
        }
    }

    /// Parses CLI Euler string `"ZYX:0,0,90deg"` (or `"ZYX:0,0,1.570796rad"`, optional `:intrinsic`/`:extrinsic`).
    pub fn parse_cli_euler(spec: &str) -> Result<Self, GeoCmdError> {
        let (seq_part, rest) = spec.split_once(':').ok_or(GeoCmdError::BadParams("euler needs SEQ:a,b,c"))?;
        let (seq_s, kind) = match seq_part.split_once('/') {
            Some((s, "extrinsic")) => (s, EulerKind::Extrinsic),
            Some((s, "intrinsic")) => (s, EulerKind::Intrinsic),
            Some(_) => return Err(GeoCmdError::BadParams("unknown euler kind")),
            None => (seq_part, EulerKind::Intrinsic),
        };
        let seq = EulerSeq::from_name(seq_s).ok_or(GeoCmdError::BadParams("unknown euler sequence"))?;
        let parts: Vec<&str> = rest.split(',').collect();
        if parts.len() != 3 {
            return Err(GeoCmdError::BadParams("euler needs 3 angles"));
        }
        let mut angles_rad = [0.0_f64; 3];
        for (i, p) in parts.iter().enumerate() {
            angles_rad[i] = parse_angle_str(p.trim())?;
        }
        Ok(RotationParam::Euler { seq, kind, angles_rad })
    }
}

/// One committed Record Journal entry produced by a W2 frame command (`REQ-001`, `REQ-002`, `REQ-006`).
#[derive(Debug, Clone, PartialEq)]
pub struct FrameJournalEntry {
    pub command: String,
    pub principal: String,
    /// `None` on main branch, `Some(branch)` when an agent committed via MCP (`REQ-006`).
    pub branch: Option<String>,
    pub args: CValue,
    pub result_digest: Digest,
}

impl FrameJournalEntry {
    pub fn to_value(&self) -> CValue {
        obj([
            ("args", self.args.clone()),
            ("branch", self.branch.as_deref().map_or(CValue::Null, text)),
            ("command", text(&self.command)),
            ("principal", text(&self.principal)),
            ("result_digest", text(self.result_digest.as_str())),
        ])
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, GeoCmdError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| GeoCmdError::Internal("canonical_encode"))
    }
}

/// Parameters for `geo.create_frame@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateFrameParams {
    pub name: String,
    pub kind: FrameKind,
    pub parent: String,
    pub rotation: RotationParam,
    pub translation: [AuthoredLengthSpec; 3],
    pub scale: Option<f64>,
}

/// Result of `geo.create_frame@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateFrameResult {
    pub id: FrameId,
    pub uuid: Uuid,
    pub name: String,
    pub parent: FrameId,
    pub tree_digest: Digest,
    pub translation_records: [ConversionRecord; 3],
}

impl CreateFrameResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("id", text(&self.uuid.to_string())),
            ("name", text(&self.name)),
            ("parent", text(&frame_uuid(self.parent).to_string())),
            (
                "translation_records",
                CValue::Arr(self.translation_records.iter().map(ConversionRecord::to_value).collect()),
            ),
            ("tree_digest", text(self.tree_digest.as_str())),
        ])
    }
}

/// Parameters for `geo.set_transform@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct SetTransformParams {
    pub frame: String,
    pub parent: Option<String>,
    pub rotation: RotationParam,
    pub translation: [AuthoredLengthSpec; 3],
    pub scale: Option<f64>,
}

/// Result of `geo.set_transform@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct SetTransformResult {
    pub frame: FrameId,
    pub tree_digest: Digest,
    pub marked_outdated: usize,
    pub translation_records: [ConversionRecord; 3],
    pub propagation: Option<Propagation>,
}

impl SetTransformResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("frame", text(&frame_uuid(self.frame).to_string())),
            ("marked_outdated", CValue::Num(self.marked_outdated as f64)),
            (
                "translation_records",
                CValue::Arr(self.translation_records.iter().map(ConversionRecord::to_value).collect()),
            ),
            ("tree_digest", text(self.tree_digest.as_str())),
        ])
    }
}

/// Result of `geo.delete_frame@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeleteFrameResult {
    pub frame: FrameId,
    pub tree_digest: Digest,
}

impl DeleteFrameResult {
    pub fn to_value(&self) -> CValue {
        obj([("frame", text(&frame_uuid(self.frame).to_string())), ("tree_digest", text(self.tree_digest.as_str()))])
    }
}

/// Result of `geo.apply_transform@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyTransformResult {
    pub id: EntityId,
    pub digest: Digest,
    pub source: EntityId,
    pub from: FrameId,
    pub to: FrameId,
}

impl ApplyTransformResult {
    pub fn to_value(&self) -> CValue {
        obj([
            ("digest", text(self.digest.as_str())),
            ("from", text(&frame_uuid(self.from).to_string())),
            ("id", text(&self.id.to_string())),
            ("source", text(&self.source.to_string())),
            ("to", text(&frame_uuid(self.to).to_string())),
        ])
    }
}

/// Result of `geo.resolve_transform@1` (R1).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolveTransformResult {
    pub from: FrameId,
    pub to: FrameId,
    pub matrix: [[f64; 4]; 4],
    pub matrix_hex: [[String; 4]; 4],
    pub scale: f64,
}

impl ResolveTransformResult {
    pub fn to_value(&self) -> CValue {
        let mat_dec = CValue::Arr(
            self.matrix.iter().map(|row| CValue::Arr(row.iter().map(|&v| text(&format!("{v}"))).collect())).collect(),
        );
        let mat_hex =
            CValue::Arr(self.matrix_hex.iter().map(|row| CValue::Arr(row.iter().map(|s| text(s)).collect())).collect());
        obj([
            ("from", text(&frame_uuid(self.from).to_string())),
            ("matrix", mat_dec),
            ("matrix_hex", mat_hex),
            ("scale", text(&format!("{}", self.scale))),
            ("to", text(&frame_uuid(self.to).to_string())),
        ])
    }
}

/// Result of `geo.convert_units@1` (R1).
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertUnitsResult {
    pub record: ConversionRecord,
    pub si: f64,
    pub formatted: Option<String>,
}

impl ConvertUnitsResult {
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        if let Some(f) = &self.formatted {
            m.insert("formatted".to_owned(), text(f));
        }
        m.insert("si".to_owned(), text(&format!("{}", self.si)));
        m.insert("si_hex".to_owned(), text(&self.record.si_hex));
        m.insert("text".to_owned(), text(&self.record.text));
        m.insert("unit".to_owned(), text(&self.record.unit));
        CValue::Obj(m)
    }
}

// ================================================================== Command implementations

/// Executes `geo.create_frame@1` (W2, D0; `REQ-001`, `REQ-002`, `REQ-006`).
pub fn create_frame(
    frames: &mut FrameTree,
    tx: &mut GeoTx<'_>,
    journal: &mut Vec<FrameJournalEntry>,
    principal: &FramePrincipal,
    params: &CreateFrameParams,
) -> Result<CreateFrameResult, GeoCmdError> {
    let parent_id = resolve_frame_target(frames, &params.parent)?;
    let (to_parent, records) = build_transform(&params.rotation, &params.translation, params.scale)?;
    // Validate frame name / duplicate name before drawing UUID.
    if params.name.is_empty() || params.name.eq_ignore_ascii_case("world") {
        return Err(GeoCmdError::BadParams("bad frame name"));
    }
    if frames.find_by_name(&params.name).is_some() {
        return Err(GeoCmdError::BadParams("duplicate frame name"));
    }

    let taken: BTreeSet<Uuid> = frames.frames().map(|f| frame_uuid(f.id)).collect();
    let uuid = {
        let mut ids = IdService::new(tx.namespace, &mut *tx.source, &taken);
        ids.new_v7().map_err(GeoCmdError::Id)?
    };
    commit_create_frame(frames, tx, journal, principal, uuid, parent_id, &params.name, params.kind, to_parent, records)
}

#[allow(clippy::too_many_arguments)]
fn commit_create_frame(
    frames: &mut FrameTree,
    tx: &mut GeoTx<'_>,
    journal: &mut Vec<FrameJournalEntry>,
    principal: &FramePrincipal,
    uuid: Uuid,
    parent_id: FrameId,
    name: &str,
    kind: FrameKind,
    to_parent: Transform,
    records: [ConversionRecord; 3],
) -> Result<CreateFrameResult, GeoCmdError> {
    let fid = FrameId::from_bytes(*uuid.as_bytes());
    let frame = Frame { id: fid, name: name.to_owned(), kind, parent: Some(parent_id), to_parent };
    frames.insert(frame).map_err(map_frame_err)?;
    ensure_frame_dep_node(tx, fid)?;

    let tree_digest = frames.digest();
    let res = CreateFrameResult {
        id: fid,
        uuid,
        name: name.to_owned(),
        parent: parent_id,
        tree_digest,
        translation_records: records.clone(),
    };
    let args = obj([
        ("id", text(&uuid.to_string())),
        ("kind", text(frame_kind_str(kind))),
        ("name", text(name)),
        ("parent", text(&frame_uuid(parent_id).to_string())),
        ("transform", transform_hex_value(&to_parent)),
        ("translation", CValue::Arr(records.iter().map(ConversionRecord::to_value).collect())),
    ]);
    let entry = FrameJournalEntry {
        command: CREATE_FRAME.to_owned(),
        principal: principal.principal_id().to_owned(),
        branch: principal.branch().map(str::to_owned),
        args,
        result_digest: value_digest(&res.to_value())?,
    };
    journal.push(entry);
    Ok(res)
}

/// Executes `geo.set_transform@1` (W2, D0; `REQ-001`, `REQ-002`, `REQ-004`).
pub fn set_transform(
    frames: &mut FrameTree,
    tx: &mut GeoTx<'_>,
    journal: &mut Vec<FrameJournalEntry>,
    principal: &FramePrincipal,
    params: &SetTransformParams,
) -> Result<SetTransformResult, GeoCmdError> {
    let fid = resolve_frame_target(frames, &params.frame)?;
    let new_parent = match &params.parent {
        Some(p) => Some(resolve_frame_target(frames, p)?),
        None => None,
    };
    let (to_parent, records) = build_transform(&params.rotation, &params.translation, params.scale)?;

    commit_set_transform(frames, tx, journal, principal, fid, new_parent, to_parent, records)
}

#[allow(clippy::too_many_arguments)]
fn commit_set_transform(
    frames: &mut FrameTree,
    tx: &mut GeoTx<'_>,
    journal: &mut Vec<FrameJournalEntry>,
    principal: &FramePrincipal,
    fid: FrameId,
    new_parent: Option<FrameId>,
    to_parent: Transform,
    records: [ConversionRecord; 3],
) -> Result<SetTransformResult, GeoCmdError> {
    match new_parent {
        Some(pid) => frames.reparent(fid, pid, to_parent).map_err(map_frame_err)?,
        None => frames.set_transform(fid, to_parent).map_err(map_frame_err)?,
    }
    ensure_frame_dep_node(tx, fid)?;

    let f_uuid = frame_uuid(fid);
    let prop = propagate(tx.edges, tx.states, tx.mode, &[f_uuid], &[]).map_err(GeoCmdError::Prop)?;
    let marked_outdated = prop.event.as_ref().map_or(0, |e| e.elements().len());

    let tree_digest = frames.digest();
    let res = SetTransformResult {
        frame: fid,
        tree_digest,
        marked_outdated,
        translation_records: records.clone(),
        propagation: Some(prop),
    };
    let args = obj([
        ("frame", text(&f_uuid.to_string())),
        ("parent", new_parent.map_or(CValue::Null, |p| text(&frame_uuid(p).to_string()))),
        ("transform", transform_hex_value(&to_parent)),
        ("translation", CValue::Arr(records.iter().map(ConversionRecord::to_value).collect())),
    ]);
    let entry = FrameJournalEntry {
        command: SET_TRANSFORM.to_owned(),
        principal: principal.principal_id().to_owned(),
        branch: principal.branch().map(str::to_owned),
        args,
        result_digest: value_digest(&res.to_value())?,
    };
    journal.push(entry);
    Ok(res)
}

/// Executes `geo.delete_frame@1` (W2, D0; `REQ-003`, `AC-3`).
pub fn delete_frame(
    frames: &mut FrameTree,
    usage: &dyn FrameUsageIndex,
    journal: &mut Vec<FrameJournalEntry>,
    principal: &FramePrincipal,
    frame_target: &str,
) -> Result<DeleteFrameResult, GeoCmdError> {
    let fid = resolve_frame_target(frames, frame_target)?;
    let f_uuid = frame_uuid(fid);
    if fid == frames.world() {
        return Err(GeoCmdError::FrameInUse { frame: f_uuid, refs: vec![f_uuid] });
    }
    let mut refs: BTreeSet<Uuid> = BTreeSet::new();
    for child in frames.children(fid).map_err(map_frame_err)? {
        refs.insert(frame_uuid(child));
    }
    for el_id in usage.elements_in_frame(FrameRef::from_bytes(fid.to_bytes())) {
        refs.insert(el_id);
    }
    if !refs.is_empty() {
        let capped: Vec<Uuid> = refs.into_iter().take(MAX_FRAME_REFS_REPORTED).collect();
        return Err(GeoCmdError::FrameInUse { frame: f_uuid, refs: capped });
    }

    frames.remove(fid).map_err(map_frame_err)?;
    let tree_digest = frames.digest();
    let res = DeleteFrameResult { frame: fid, tree_digest };
    let args = obj([("frame", text(&f_uuid.to_string()))]);
    let entry = FrameJournalEntry {
        command: DELETE_FRAME.to_owned(),
        principal: principal.principal_id().to_owned(),
        branch: principal.branch().map(str::to_owned),
        args,
        result_digest: value_digest(&res.to_value())?,
    };
    journal.push(entry);
    Ok(res)
}

/// Executes `geo.apply_transform@1` (W2, D0; `REQ-004`, `AC-2`).
pub fn apply_transform(
    frames: &FrameTree,
    tx: &mut GeoTx<'_>,
    journal: &mut Vec<FrameJournalEntry>,
    principal: &FramePrincipal,
    element_id: EntityId,
    to_target: &str,
) -> Result<ApplyTransformResult, GeoCmdError> {
    let to_id = resolve_frame_target(frames, to_target)?;
    let src_entry = match tx.tree.get(&element_id) {
        Some(TreeEntry::Geometry(g)) => g.clone(),
        Some(TreeEntry::Other { .. }) => return Err(GeoCmdError::NotGeometry(element_id)),
        None => return Err(GeoCmdError::NotFound(element_id)),
    };
    let from_id = FrameId::from_bytes(src_entry.facts.frame.to_bytes());
    let xform = frames.resolve(from_id, to_id).map_err(map_frame_err)?;
    let to_ref = FrameRef::from_bytes(to_id.to_bytes());
    let transformed = transform_element_with_provenance(&src_entry.element, &xform, to_ref)
        .map_err(|e| GeoCmdError::Internal(e.code()))?;

    ensure_frame_dep_node(tx, from_id)?;
    ensure_frame_dep_node(tx, to_id)?;

    let params_val = obj([("element", text(&element_id.to_string())), ("to", text(&frame_uuid(to_id).to_string()))]);
    let params_digest = value_digest(&params_val)?;
    let cmd_id = CommandId::parse(APPLY_TRANSFORM).map_err(|_| GeoCmdError::Internal("bad_cmd"))?;
    let mut inputs = vec![(element_id, src_entry.facts.digest.to_digest())];
    let from_uuid = frame_uuid(from_id);
    let to_uuid = frame_uuid(to_id);
    let tree_digest = frames.digest();
    inputs.push((from_uuid, tree_digest));
    if to_uuid != from_uuid {
        inputs.push((to_uuid, tree_digest));
    }

    let origin = IdOrigin::Derived(Derivation {
        command: cmd_id,
        params: params_digest,
        output: "transformed".to_owned(),
        inputs,
    });
    let reg = register_element(tx, transformed.element, origin)?;
    let res = ApplyTransformResult {
        id: reg.id,
        digest: reg.digest.to_digest(),
        source: element_id,
        from: from_id,
        to: to_id,
    };
    let args = obj([
        ("element", text(&element_id.to_string())),
        ("from", text(&from_uuid.to_string())),
        ("to", text(&to_uuid.to_string())),
    ]);
    let entry = FrameJournalEntry {
        command: APPLY_TRANSFORM.to_owned(),
        principal: principal.principal_id().to_owned(),
        branch: principal.branch().map(str::to_owned),
        args,
        result_digest: value_digest(&res.to_value())?,
    };
    journal.push(entry);
    Ok(res)
}

/// Executes `geo.resolve_transform@1` (R1, D0; writes no Record Journal entry, `REQ-005`, `AC-4`).
pub fn resolve_transform(
    frames: &FrameTree,
    log: &mut Vec<ActivityEntry>,
    surface: Surface,
    from_target: &str,
    to_target: &str,
) -> Result<ResolveTransformResult, GeoCmdError> {
    let run = || -> Result<ResolveTransformResult, GeoCmdError> {
        let from_id = resolve_frame_target(frames, from_target)?;
        let to_id = resolve_frame_target(frames, to_target)?;
        let xform = frames.resolve(from_id, to_id).map_err(map_frame_err)?;
        let (matrix, scale) = transform_matrix4(&xform);
        let mut matrix_hex: [[String; 4]; 4] = Default::default();
        for (i, row) in matrix.iter().enumerate() {
            for (j, &val) in row.iter().enumerate() {
                matrix_hex[i][j] = format!("0x{:016x}", val.to_bits());
            }
        }
        Ok(ResolveTransformResult { from: from_id, to: to_id, matrix, matrix_hex, scale })
    };
    let out = run();
    log.push(ActivityEntry {
        command: RESOLVE_TRANSFORM,
        surface,
        outcome: match &out {
            Ok(_) => "ok",
            Err(e) => e.code(),
        },
    });
    out
}

/// Executes `geo.convert_units@1` (R1, D0; writes no Record Journal entry, `REQ-005`).
pub fn convert_units(
    log: &mut Vec<ActivityEntry>,
    surface: Surface,
    text_in: &str,
    unit_sym: &str,
    to_unit_sym: Option<&str>,
    digits: Option<u8>,
) -> Result<ConvertUnitsResult, GeoCmdError> {
    let run = || -> Result<ConvertUnitsResult, GeoCmdError> {
        let unit = Unit::from_symbol(unit_sym)
            .ok_or_else(|| GeoCmdError::BadQuantity(format!("unknown unit {}", kept(unit_sym))))?;
        let q = AuthoredQuantity::parse(text_in, unit).map_err(|e| GeoCmdError::BadQuantity(format!("{e:?}")))?;
        let formatted = match to_unit_sym {
            None => None,
            Some(ts) => {
                let target = Unit::from_symbol(ts)
                    .ok_or_else(|| GeoCmdError::BadQuantity(format!("unknown target unit {}", kept(ts))))?;
                let d = digits.unwrap_or(6);
                let s = format_unit(q.si(), target, d).map_err(|e| GeoCmdError::BadQuantity(format!("{e:?}")))?;
                Some(s)
            }
        };
        Ok(ConvertUnitsResult { record: ConversionRecord::from_authored(&q), si: q.si(), formatted })
    };
    let out = run();
    log.push(ActivityEntry {
        command: CONVERT_UNITS,
        surface,
        outcome: match &out {
            Ok(_) => "ok",
            Err(e) => e.code(),
        },
    });
    out
}

/// Replays a committed slice of [`FrameJournalEntry`] in exact mode (`AC-5`), returning
/// `Ok(replayed)` when every replayed entry matches bit-for-bit.
pub fn replay_frame_journal(
    frames: &mut FrameTree,
    tx: &mut GeoTx<'_>,
    usage: &dyn FrameUsageIndex,
    recorded: &[FrameJournalEntry],
) -> Result<Vec<FrameJournalEntry>, GeoCmdError> {
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
            CREATE_FRAME => {
                let uuid = parse_uuid_field(o, "id")?;
                let name = req_str(o, "name")?;
                let kind = parse_frame_kind(req_str(o, "kind")?)?;
                let parent = resolve_frame_target(frames, req_str(o, "parent")?)?;
                let to_parent = parse_transform_hex(o.get("transform").ok_or(GeoCmdError::BadParams("no transform"))?)?;
                let records = parse_records(o.get("translation").ok_or(GeoCmdError::BadParams("no translation"))?)?;
                commit_create_frame(frames, tx, &mut out, &principal, uuid, parent, name, kind, to_parent, records)?;
            }
            SET_TRANSFORM => {
                let fid = resolve_frame_target(frames, req_str(o, "frame")?)?;
                let parent = match o.get("parent") {
                    Some(CValue::Null) | None => None,
                    Some(CValue::Str(s)) => Some(resolve_frame_target(frames, s)?),
                    _ => return Err(GeoCmdError::BadParams("bad parent")),
                };
                let to_parent = parse_transform_hex(o.get("transform").ok_or(GeoCmdError::BadParams("no transform"))?)?;
                let records = parse_records(o.get("translation").ok_or(GeoCmdError::BadParams("no translation"))?)?;
                commit_set_transform(frames, tx, &mut out, &principal, fid, parent, to_parent, records)?;
            }
            DELETE_FRAME => {
                let frame_s = req_str(o, "frame")?;
                delete_frame(frames, usage, &mut out, &principal, frame_s)?;
            }
            APPLY_TRANSFORM => {
                let el = parse_uuid_field(o, "element")?;
                let to_s = req_str(o, "to")?;
                apply_transform(frames, tx, &mut out, &principal, el, to_s)?;
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

// ================================================================== CLI & MCP dispatch

/// Runs `fid geo <args>` for the six `GEO.FRAME.CMD` subcommands (`AC-1`..`AC-4`).
#[allow(clippy::too_many_arguments)]
pub fn invoke_frame_cli(
    frames: &mut FrameTree,
    tx: &mut GeoTx<'_>,
    usage: &dyn FrameUsageIndex,
    journal: &mut Vec<FrameJournalEntry>,
    log: &mut Vec<ActivityEntry>,
    principal: &FramePrincipal,
    args: &[&str],
) -> Result<Vec<u8>, GeoCmdError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(GeoCmdError::InputLimit("args"));
    }
    let (sub, rest) = args.split_first().ok_or(GeoCmdError::BadParams("missing subcommand"))?;
    let val = match *sub {
        CLI_CREATE_FRAME => {
            let p = parse_cli_create_frame(rest)?;
            create_frame(frames, tx, journal, principal, &p)?.to_value()
        }
        CLI_SET_TRANSFORM => {
            let p = parse_cli_set_transform(rest)?;
            set_transform(frames, tx, journal, principal, &p)?.to_value()
        }
        CLI_DELETE_FRAME => {
            let frame_s = parse_cli_single_target(rest, "--frame")?;
            delete_frame(frames, usage, journal, principal, &frame_s)?.to_value()
        }
        CLI_APPLY_TRANSFORM => {
            let (el_id, to_s) = parse_cli_apply_transform(rest)?;
            apply_transform(frames, tx, journal, principal, el_id, &to_s)?.to_value()
        }
        CLI_RESOLVE_TRANSFORM => {
            let (from_s, to_s) = parse_cli_resolve(rest)?;
            resolve_transform(frames, log, Surface::Cli, &from_s, &to_s)?.to_value()
        }
        CLI_CONVERT_UNITS => {
            let (txt, unit, to_u, digits) = parse_cli_convert(rest)?;
            convert_units(log, Surface::Cli, &txt, &unit, to_u.as_deref(), digits)?.to_value()
        }
        other => return Err(GeoCmdError::UnknownCommand(kept(other))),
    };
    to_canonical_bytes(&val).map_err(|_| GeoCmdError::Internal("canonical_encode"))
}

/// Runs one of the six `GEO.FRAME.CMD` MCP tools (`REQ-006`, `AC-6`).
#[allow(clippy::too_many_arguments)]
pub fn invoke_frame_mcp(
    frames: &mut FrameTree,
    tx: &mut GeoTx<'_>,
    usage: &dyn FrameUsageIndex,
    journal: &mut Vec<FrameJournalEntry>,
    log: &mut Vec<ActivityEntry>,
    principal: &FramePrincipal,
    tool: &str,
    raw: &[u8],
) -> Result<Vec<u8>, GeoCmdError> {
    let v = parse_with_limits(raw, MAX_FRAME_PARAMS_BYTES, MAX_FRAME_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => GeoCmdError::InputLimit("params"),
        CanonError::DuplicateKey(_) => GeoCmdError::BadParams("duplicate key"),
        _ => GeoCmdError::BadParams("params not JSON"),
    })?;
    let CValue::Obj(o) = &v else {
        return Err(GeoCmdError::BadParams("params not object"));
    };
    let val = match tool {
        MCP_CREATE_FRAME => {
            let p = parse_mcp_create_frame(o)?;
            create_frame(frames, tx, journal, principal, &p)?.to_value()
        }
        MCP_SET_TRANSFORM => {
            let p = parse_mcp_set_transform(o)?;
            set_transform(frames, tx, journal, principal, &p)?.to_value()
        }
        MCP_DELETE_FRAME => {
            let f = req_str(o, "frame")?;
            delete_frame(frames, usage, journal, principal, f)?.to_value()
        }
        MCP_APPLY_TRANSFORM => {
            let el = parse_uuid_field(o, "element")?;
            let to = req_str(o, "to")?;
            apply_transform(frames, tx, journal, principal, el, to)?.to_value()
        }
        MCP_RESOLVE_TRANSFORM => {
            let from = req_str(o, "from")?;
            let to = req_str(o, "to")?;
            resolve_transform(frames, log, Surface::Mcp, from, to)?.to_value()
        }
        MCP_CONVERT_UNITS => {
            let txt = req_str(o, "text")?;
            let unit = req_str(o, "unit")?;
            let to_u = match o.get("to_unit") {
                None | Some(CValue::Null) => None,
                Some(CValue::Str(s)) => Some(s.as_str()),
                _ => return Err(GeoCmdError::BadParams("bad to_unit")),
            };
            let digits = match o.get("digits") {
                None | Some(CValue::Null) => None,
                Some(CValue::Num(n)) if *n >= 0.0 && *n <= 15.0 && n.fract() == 0.0 => Some(*n as u8),
                _ => return Err(GeoCmdError::BadParams("bad digits")),
            };
            convert_units(log, Surface::Mcp, txt, unit, to_u, digits)?.to_value()
        }
        other => return Err(GeoCmdError::UnknownCommand(kept(other))),
    };
    to_canonical_bytes(&val).map_err(|_| GeoCmdError::Internal("canonical_encode"))
}

// ================================================================== Registry definitions (AC-6)

/// Returns the six `GEO.FRAME.CMD` registry definitions (`AC-6`).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let conv_rec = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["si_hex", "text", "unit"])),
        (
            "properties",
            obj([
                ("si_hex", obj([("type", text("string"))])),
                ("text", obj([("type", text("string"))])),
                ("unit", obj([("type", text("string"))])),
            ]),
        ),
    ]);
    let authored_len = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["text", "unit"])),
        ("properties", obj([("text", obj([("type", text("string"))])), ("unit", obj([("type", text("string"))]))])),
    ]);
    let trans_arr = obj([
        ("type", text("array")),
        ("items", authored_len),
        ("minItems", CValue::Num(3.0)),
        ("maxItems", CValue::Num(3.0)),
    ]);
    let rot_schema = obj([
        ("type", text("object")),
        ("description", text("Tagged rotation: identity, matrix, quaternion, axis_angle or euler.")),
    ]);
    let uuid_schema = || obj([("type", text("string")), ("pattern", text(ID_PATTERN))]);
    let scale_schema = || obj([("type", text("string"))]);

    let create_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["name", "parent", "translation"])),
        (
            "properties",
            obj([
                ("kind", obj([("type", text("string")), ("enum", strs(&["part", "instrument", "alignment", "user"]))])),
                ("name", obj([("type", text("string"))])),
                ("parent", obj([("type", text("string"))])),
                ("rotation", rot_schema.clone()),
                ("scale", scale_schema()),
                ("translation", trans_arr.clone()),
            ]),
        ),
    ]);
    let create_result = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["id", "name", "parent", "translation_records", "tree_digest"])),
        (
            "properties",
            obj([
                ("id", uuid_schema()),
                ("name", obj([("type", text("string"))])),
                ("parent", uuid_schema()),
                ("translation_records", obj([("type", text("array")), ("items", conv_rec.clone())])),
                ("tree_digest", obj([("type", text("string"))])),
            ]),
        ),
    ]);

    let set_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["frame", "translation"])),
        (
            "properties",
            obj([
                ("frame", obj([("type", text("string"))])),
                ("parent", obj([("type", text("string"))])),
                ("rotation", rot_schema),
                ("scale", scale_schema()),
                ("translation", trans_arr),
            ]),
        ),
    ]);
    let set_result = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["frame", "marked_outdated", "translation_records", "tree_digest"])),
        (
            "properties",
            obj([
                ("frame", uuid_schema()),
                ("marked_outdated", obj([("type", text("integer")), ("minimum", CValue::Num(0.0))])),
                ("translation_records", obj([("type", text("array")), ("items", conv_rec)])),
                ("tree_digest", obj([("type", text("string"))])),
            ]),
        ),
    ]);

    let del_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["frame"])),
        ("properties", obj([("frame", obj([("type", text("string"))]))])),
    ]);
    let del_result = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["frame", "tree_digest"])),
        ("properties", obj([("frame", uuid_schema()), ("tree_digest", obj([("type", text("string"))]))])),
    ]);

    let apply_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["element", "to"])),
        (
            "properties",
            obj([
                ("element", obj([("type", text("string")), ("x-ref", text("element")), ("pattern", text(ID_PATTERN))])),
                ("to", obj([("type", text("string"))])),
            ]),
        ),
    ]);
    let apply_result = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["digest", "from", "id", "source", "to"])),
        (
            "properties",
            obj([
                ("digest", obj([("type", text("string"))])),
                ("from", uuid_schema()),
                ("id", uuid_schema()),
                ("source", uuid_schema()),
                ("to", uuid_schema()),
            ]),
        ),
    ]);

    let resolve_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["from", "to"])),
        ("properties", obj([("from", obj([("type", text("string"))])), ("to", obj([("type", text("string"))]))])),
    ]);
    let mat4_num = obj([
        ("type", text("array")),
        ("items", obj([("type", text("array")), ("items", obj([("type", text("string"))]))])),
    ]);
    let mat4_hex = obj([
        ("type", text("array")),
        ("items", obj([("type", text("array")), ("items", obj([("type", text("string"))]))])),
    ]);
    let resolve_result = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["from", "matrix", "matrix_hex", "scale", "to"])),
        (
            "properties",
            obj([
                ("from", uuid_schema()),
                ("matrix", mat4_num),
                ("matrix_hex", mat4_hex),
                ("scale", scale_schema()),
                ("to", uuid_schema()),
            ]),
        ),
    ]);

    let conv_params = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["text", "unit"])),
        (
            "properties",
            obj([
                (
                    "digits",
                    obj([("type", text("integer")), ("minimum", CValue::Num(0.0)), ("maximum", CValue::Num(15.0))]),
                ),
                ("text", obj([("type", text("string"))])),
                ("to_unit", obj([("type", text("string"))])),
                ("unit", obj([("type", text("string"))])),
            ]),
        ),
    ]);
    let conv_result = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["si", "si_hex", "text", "unit"])),
        (
            "properties",
            obj([
                ("formatted", obj([("type", text("string"))])),
                ("si", obj([("type", text("string"))])),
                ("si_hex", obj([("type", text("string"))])),
                ("text", obj([("type", text("string"))])),
                ("unit", obj([("type", text("string"))])),
            ]),
        ),
    ]);

    Ok(vec![
        build_cmd(
            "geo.create_frame",
            "Create coordinate frame",
            "Creates a coordinate frame with authored length translation records and rotation.",
            "W2",
            "head_move",
            &[CREATE_FRAME],
            create_params,
            create_result,
        )?,
        build_cmd(
            "geo.set_transform",
            "Set frame transform",
            "Updates a coordinate frame's transform and optional parent, propagating outdated state to derived elements.",
            "W2",
            "head_move",
            &[SET_TRANSFORM],
            set_params,
            set_result,
        )?,
        build_cmd(
            "geo.delete_frame",
            "Delete coordinate frame",
            "Deletes an unreferenced coordinate frame; fails with frame_in_use if any child frame or element references it.",
            "W2",
            "head_move",
            &[DELETE_FRAME],
            del_params,
            del_result,
        )?,
        build_cmd(
            "geo.apply_transform",
            "Apply transform to element",
            "Transforms a geometric element into a target frame and registers it as a derived element.",
            "W2",
            "head_move",
            &[APPLY_TRANSFORM],
            apply_params,
            apply_result,
        )?,
        build_cmd(
            "geo.resolve_transform",
            "Resolve transform between frames",
            "Resolves the exact 4x4 transform matrix (decimal and IEEE hex) and scale between two coordinate frames.",
            "R1",
            "not_undoable",
            &[],
            resolve_params,
            resolve_result,
        )?,
        build_cmd(
            "geo.convert_units",
            "Convert authored units",
            "Converts an authored length or angle quantity to SI with single rounding and optional formatted display.",
            "R1",
            "not_undoable",
            &[],
            conv_params,
            conv_result,
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
        ("idempotency", text(if side_effect == "R1" { "natural" } else { "none" })),
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

// ================================================================== Helpers

fn frame_uuid(id: FrameId) -> Uuid {
    Uuid::from_bytes(id.to_bytes())
}

fn ensure_frame_dep_node(tx: &mut GeoTx<'_>, fid: FrameId) -> Result<(), GeoCmdError> {
    let u = frame_uuid(fid);
    if !tx.edges.contains(&u) {
        let empty = BTreeSet::new();
        let set = EdgesSet::from_set(u, &empty).map_err(GeoCmdError::Edge)?;
        tx.edges.apply(std::slice::from_ref(&set)).map_err(GeoCmdError::Edge)?;
        tx.states.insert(u, ElementState::Current);
    }
    Ok(())
}

fn resolve_frame_target(frames: &FrameTree, target: &str) -> Result<FrameId, GeoCmdError> {
    if target.eq_ignore_ascii_case("world") {
        return Ok(frames.world());
    }
    if let Some(id) = frames.find_by_name(target) {
        return Ok(id);
    }
    if let Ok(u) = Uuid::parse(target) {
        let fid = FrameId::from_bytes(*u.as_bytes());
        if frames.get(fid).is_some() {
            return Ok(fid);
        }
    }
    if let Ok(fid) = FrameId::parse(target) {
        if frames.get(fid).is_some() {
            return Ok(fid);
        }
    }
    Err(GeoCmdError::UnknownFrame(kept(target)))
}

fn build_transform(
    rot: &RotationParam,
    translation: &[AuthoredLengthSpec; 3],
    scale: Option<f64>,
) -> Result<(Transform, [ConversionRecord; 3]), GeoCmdError> {
    let r = rot.to_matrix()?;
    let q0 = translation[0].to_authored()?;
    let q1 = translation[1].to_authored()?;
    let q2 = translation[2].to_authored()?;
    let t = [q0.si(), q1.si(), q2.si()];
    let rigid = Rigid::new(r, t).map_err(map_xform_err)?;
    let xform = match scale {
        None => Transform::Rigid(rigid),
        Some(s) => Transform::Similarity(Similarity::new(rigid, s).map_err(map_xform_err)?),
    };
    Ok((
        xform,
        [
            ConversionRecord::from_authored(&q0),
            ConversionRecord::from_authored(&q1),
            ConversionRecord::from_authored(&q2),
        ],
    ))
}

fn transform_matrix4(t: &Transform) -> ([[f64; 4]; 4], f64) {
    let (r, tr, s) = match *t {
        Transform::Rigid(rg) => (rg.r(), rg.t(), 1.0),
        Transform::Similarity(sm) => (sm.rigid().r(), sm.rigid().t(), sm.s()),
    };
    let m = [
        [r[0][0] * s, r[0][1] * s, r[0][2] * s, tr[0]],
        [r[1][0] * s, r[1][1] * s, r[1][2] * s, tr[1]],
        [r[2][0] * s, r[2][1] * s, r[2][2] * s, tr[2]],
        [0.0, 0.0, 0.0, 1.0],
    ];
    (m, s)
}

fn transform_hex_value(t: &Transform) -> CValue {
    let (r, tr, s) = match *t {
        Transform::Rigid(rg) => (rg.r(), rg.t(), None),
        Transform::Similarity(sm) => (sm.rigid().r(), sm.rigid().t(), Some(sm.s())),
    };
    let hex = |v: f64| text(&format!("0x{:016x}", v.to_bits()));
    let row = |x: [f64; 3]| CValue::Arr(vec![hex(x[0]), hex(x[1]), hex(x[2])]);
    obj([
        ("r", CValue::Arr(vec![row(r[0]), row(r[1]), row(r[2])])),
        ("scale", s.map_or(CValue::Null, hex)),
        ("t", row(tr)),
    ])
}

fn parse_transform_hex(v: &CValue) -> Result<Transform, GeoCmdError> {
    let CValue::Obj(o) = v else { return Err(GeoCmdError::BadParams("bad transform hex")) };
    let Some(CValue::Arr(rows)) = o.get("r") else { return Err(GeoCmdError::BadParams("bad r")) };
    if rows.len() != 3 {
        return Err(GeoCmdError::BadParams("r len != 3"));
    }
    let r = [parse_hex_vec3(&rows[0])?, parse_hex_vec3(&rows[1])?, parse_hex_vec3(&rows[2])?];
    let t = parse_hex_vec3(o.get("t").ok_or(GeoCmdError::BadParams("no t"))?)?;
    let rigid = Rigid::new(r, t).map_err(map_xform_err)?;
    match o.get("scale") {
        None | Some(CValue::Null) => Ok(Transform::Rigid(rigid)),
        Some(CValue::Str(s)) => {
            let sc = parse_hex_f64(s)?;
            Ok(Transform::Similarity(Similarity::new(rigid, sc).map_err(map_xform_err)?))
        }
        _ => Err(GeoCmdError::BadParams("bad scale")),
    }
}

fn parse_hex_vec3(v: &CValue) -> Result<[f64; 3], GeoCmdError> {
    let CValue::Arr(a) = v else { return Err(GeoCmdError::BadParams("hex vec3 not array")) };
    if a.len() != 3 {
        return Err(GeoCmdError::BadParams("hex vec3 len != 3"));
    }
    let mut out = [0.0; 3];
    for (i, item) in a.iter().enumerate() {
        let CValue::Str(s) = item else { return Err(GeoCmdError::BadParams("hex coord not string")) };
        out[i] = parse_hex_f64(s)?;
    }
    Ok(out)
}

fn parse_hex_f64(s: &str) -> Result<f64, GeoCmdError> {
    let rest = s.strip_prefix("0x").ok_or(GeoCmdError::BadParams("bad hex"))?;
    if rest.len() != 16 {
        return Err(GeoCmdError::BadParams("bad hex len"));
    }
    let bits = u64::from_str_radix(rest, 16).map_err(|_| GeoCmdError::BadParams("bad hex digits"))?;
    Ok(f64::from_bits(bits))
}

fn parse_records(v: &CValue) -> Result<[ConversionRecord; 3], GeoCmdError> {
    let CValue::Arr(a) = v else { return Err(GeoCmdError::BadParams("records not array")) };
    if a.len() != 3 {
        return Err(GeoCmdError::BadParams("records len != 3"));
    }
    let one = |item: &CValue| -> Result<ConversionRecord, GeoCmdError> {
        let CValue::Obj(o) = item else { return Err(GeoCmdError::BadParams("record not obj")) };
        Ok(ConversionRecord {
            text: req_str(o, "text")?.to_owned(),
            unit: req_str(o, "unit")?.to_owned(),
            si_hex: req_str(o, "si_hex")?.to_owned(),
        })
    };
    Ok([one(&a[0])?, one(&a[1])?, one(&a[2])?])
}

fn parse_angle_str(s: &str) -> Result<f64, GeoCmdError> {
    if let Ok(v) = s.parse::<f64>() {
        // Plain numeric zero is unambiguous (0 deg == 0 rad); otherwise treat as radians.
        if !v.is_finite() {
            return Err(GeoCmdError::BadQuantity(kept(s)));
        }
        return Ok(v);
    }
    let idx = s
        .find(|c: char| c.is_ascii_alphabetic() || c == '°' || c == '′' || c == '″')
        .ok_or_else(|| GeoCmdError::BadQuantity(kept(s)))?;
    let num = s[..idx].trim();
    let unit_s = s[idx..].trim();
    let unit = match unit_s {
        "deg" | "°" => Unit::Angle(AngleUnit::Deg),
        "rad" => Unit::Angle(AngleUnit::Rad),
        "gon" => Unit::Angle(AngleUnit::Gon),
        "arcmin" | "′" => Unit::Angle(AngleUnit::Arcmin),
        "arcsec" | "″" => Unit::Angle(AngleUnit::Arcsec),
        _ => return Err(GeoCmdError::BadQuantity(kept(s))),
    };
    let q = AuthoredQuantity::parse(num, unit).map_err(|e| GeoCmdError::BadQuantity(format!("{e:?}")))?;
    Ok(q.si())
}

fn frame_kind_str(k: FrameKind) -> &'static str {
    match k {
        FrameKind::World => "world",
        FrameKind::Part => "part",
        FrameKind::Instrument => "instrument",
        FrameKind::Alignment => "alignment",
        FrameKind::User => "user",
    }
}

fn parse_frame_kind(s: &str) -> Result<FrameKind, GeoCmdError> {
    match s {
        "part" => Ok(FrameKind::Part),
        "instrument" => Ok(FrameKind::Instrument),
        "alignment" => Ok(FrameKind::Alignment),
        "user" => Ok(FrameKind::User),
        _ => Err(GeoCmdError::BadParams("unknown frame kind")),
    }
}

fn map_xform_err(e: XformError) -> GeoCmdError {
    match e {
        XformError::NotOrthonormal { .. }
        | XformError::Improper { .. }
        | XformError::NotUnit { .. }
        | XformError::ZeroAxis => GeoCmdError::NotOrthonormal(format!("{e:?}")),
        XformError::BadScale => GeoCmdError::BadParams("bad scale"),
        XformError::NonFinite => GeoCmdError::BadParams("non-finite transform"),
        XformError::Numeric(_) => GeoCmdError::BadParams("numeric transform error"),
    }
}

fn map_frame_err(e: FrameError) -> GeoCmdError {
    match e {
        FrameError::UnknownFrame { id } => GeoCmdError::UnknownFrame(frame_uuid(id).to_string()),
        FrameError::Cycle { frames } => GeoCmdError::Cycle(frames.into_iter().map(frame_uuid).collect()),
        FrameError::BadTransform { .. } | FrameError::Handedness { .. } => {
            GeoCmdError::NotOrthonormal(format!("{e:?}"))
        }
        FrameError::HasChildren { id, .. } => GeoCmdError::FrameInUse { frame: frame_uuid(id), refs: Vec::new() },
        FrameError::DuplicateName { .. } => GeoCmdError::BadParams("duplicate frame name"),
        FrameError::DuplicateId { id } => GeoCmdError::IdCollision(frame_uuid(id)),
        _ => GeoCmdError::BadParams("frame tree error"),
    }
}

fn parse_cli_translation(s: &str) -> Result<[AuthoredLengthSpec; 3], GeoCmdError> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 3 {
        return Err(GeoCmdError::BadQuantity("translation must have 3 comma-separated lengths".to_owned()));
    }
    Ok([
        AuthoredLengthSpec::parse_combined(parts[0])?,
        AuthoredLengthSpec::parse_combined(parts[1])?,
        AuthoredLengthSpec::parse_combined(parts[2])?,
    ])
}

fn parse_cli_create_frame(rest: &[&str]) -> Result<CreateFrameParams, GeoCmdError> {
    let mut map: BTreeMap<&str, &str> = BTreeMap::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--json" => {}
            f @ ("--name" | "--parent" | "--kind" | "--translation" | "--rotation-euler" | "--scale") => {
                i += 1;
                let v = *rest.get(i).ok_or(GeoCmdError::BadParams("option missing value"))?;
                if map.insert(f, v).is_some() {
                    return Err(GeoCmdError::BadParams("repeated option"));
                }
            }
            _ => return Err(GeoCmdError::BadParams("unknown option")),
        }
        i += 1;
    }
    let name = map.get("--name").ok_or(GeoCmdError::BadParams("missing --name"))?.to_string();
    let parent = map.get("--parent").copied().unwrap_or("world").to_owned();
    let kind = match map.get("--kind") {
        None => FrameKind::User,
        Some(k) => parse_frame_kind(k)?,
    };
    let translation = match map.get("--translation") {
        None => {
            [AuthoredLengthSpec::new("0", "m"), AuthoredLengthSpec::new("0", "m"), AuthoredLengthSpec::new("0", "m")]
        }
        Some(s) => parse_cli_translation(s)?,
    };
    let rotation = match map.get("--rotation-euler") {
        None => RotationParam::Identity,
        Some(s) => RotationParam::parse_cli_euler(s)?,
    };
    let scale = match map.get("--scale") {
        None => None,
        Some(s) => Some(s.parse::<f64>().map_err(|_| GeoCmdError::BadParams("bad --scale"))?),
    };
    Ok(CreateFrameParams { name, kind, parent, rotation, translation, scale })
}

fn parse_cli_set_transform(rest: &[&str]) -> Result<SetTransformParams, GeoCmdError> {
    let mut map: BTreeMap<&str, &str> = BTreeMap::new();
    let mut positional: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--json" => {}
            f @ ("--frame" | "--parent" | "--translation" | "--rotation-euler" | "--scale") => {
                i += 1;
                let v = *rest.get(i).ok_or(GeoCmdError::BadParams("option missing value"))?;
                if map.insert(f, v).is_some() {
                    return Err(GeoCmdError::BadParams("repeated option"));
                }
            }
            s if s.starts_with('-') => return Err(GeoCmdError::BadParams("unknown option")),
            s if positional.is_none() => positional = Some(s),
            _ => return Err(GeoCmdError::BadParams("unexpected positional")),
        }
        i += 1;
    }
    let frame = map.get("--frame").copied().or(positional).ok_or(GeoCmdError::BadParams("missing frame"))?.to_owned();
    let parent = map.get("--parent").map(|s| (*s).to_owned());
    let translation = match map.get("--translation") {
        None => {
            [AuthoredLengthSpec::new("0", "m"), AuthoredLengthSpec::new("0", "m"), AuthoredLengthSpec::new("0", "m")]
        }
        Some(s) => parse_cli_translation(s)?,
    };
    let rotation = match map.get("--rotation-euler") {
        None => RotationParam::Identity,
        Some(s) => RotationParam::parse_cli_euler(s)?,
    };
    let scale = match map.get("--scale") {
        None => None,
        Some(s) => Some(s.parse::<f64>().map_err(|_| GeoCmdError::BadParams("bad --scale"))?),
    };
    Ok(SetTransformParams { frame, parent, rotation, translation, scale })
}

fn parse_cli_single_target(rest: &[&str], flag: &str) -> Result<String, GeoCmdError> {
    let mut target: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--json" => {}
            f if f == flag => {
                i += 1;
                target = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing flag value"))?);
            }
            s if s.starts_with('-') => return Err(GeoCmdError::BadParams("unknown option")),
            s if target.is_none() => target = Some(s),
            _ => return Err(GeoCmdError::BadParams("unexpected arg")),
        }
        i += 1;
    }
    target.map(str::to_owned).ok_or(GeoCmdError::BadParams("missing target"))
}

fn parse_cli_apply_transform(rest: &[&str]) -> Result<(EntityId, String), GeoCmdError> {
    let mut element: Option<&str> = None;
    let mut to: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--json" => {}
            "--to" => {
                i += 1;
                to = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --to"))?);
            }
            "--element" => {
                i += 1;
                element = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --element"))?);
            }
            s if s.starts_with('-') => return Err(GeoCmdError::BadParams("unknown option")),
            s if element.is_none() => element = Some(s),
            _ => return Err(GeoCmdError::BadParams("unexpected arg")),
        }
        i += 1;
    }
    let el_s = element.ok_or(GeoCmdError::BadParams("missing element"))?;
    let el_id = Uuid::parse(el_s).map_err(|_| GeoCmdError::BadParams("bad element uuid"))?;
    let to_s = to.ok_or(GeoCmdError::BadParams("missing --to"))?.to_owned();
    Ok((el_id, to_s))
}

fn parse_cli_resolve(rest: &[&str]) -> Result<(String, String), GeoCmdError> {
    let mut from: Option<&str> = None;
    let mut to: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--json" => {}
            "--from" => {
                i += 1;
                from = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --from"))?);
            }
            "--to" => {
                i += 1;
                to = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --to"))?);
            }
            _ => return Err(GeoCmdError::BadParams("unknown option")),
        }
        i += 1;
    }
    Ok((
        from.ok_or(GeoCmdError::BadParams("missing --from"))?.to_owned(),
        to.ok_or(GeoCmdError::BadParams("missing --to"))?.to_owned(),
    ))
}

fn parse_cli_convert(rest: &[&str]) -> Result<(String, String, Option<String>, Option<u8>), GeoCmdError> {
    let mut txt: Option<&str> = None;
    let mut unit: Option<&str> = None;
    let mut to_unit: Option<String> = None;
    let mut digits: Option<u8> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--json" => {}
            "--text" => {
                i += 1;
                txt = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --text"))?);
            }
            "--unit" => {
                i += 1;
                unit = Some(*rest.get(i).ok_or(GeoCmdError::BadParams("missing --unit"))?);
            }
            "--to-unit" => {
                i += 1;
                to_unit = Some((*rest.get(i).ok_or(GeoCmdError::BadParams("missing --to-unit"))?).to_owned());
            }
            "--digits" => {
                i += 1;
                digits = Some(
                    rest.get(i)
                        .ok_or(GeoCmdError::BadParams("missing --digits"))?
                        .parse::<u8>()
                        .map_err(|_| GeoCmdError::BadParams("bad --digits"))?,
                );
            }
            _ => return Err(GeoCmdError::BadParams("unknown option")),
        }
        i += 1;
    }
    Ok((
        txt.ok_or(GeoCmdError::BadParams("missing --text"))?.to_owned(),
        unit.ok_or(GeoCmdError::BadParams("missing --unit"))?.to_owned(),
        to_unit,
        digits,
    ))
}

fn parse_mcp_translation(v: &CValue) -> Result<[AuthoredLengthSpec; 3], GeoCmdError> {
    let CValue::Arr(a) = v else { return Err(GeoCmdError::BadParams("translation not array")) };
    if a.len() != 3 {
        return Err(GeoCmdError::BadParams("translation len != 3"));
    }
    let one = |item: &CValue| -> Result<AuthoredLengthSpec, GeoCmdError> {
        let CValue::Obj(o) = item else { return Err(GeoCmdError::BadParams("length item not object")) };
        Ok(AuthoredLengthSpec::new(req_str(o, "text")?, req_str(o, "unit")?))
    };
    Ok([one(&a[0])?, one(&a[1])?, one(&a[2])?])
}

fn parse_mcp_rotation(v: Option<&CValue>) -> Result<RotationParam, GeoCmdError> {
    let Some(val) = v else { return Ok(RotationParam::Identity) };
    let CValue::Obj(o) = val else { return Err(GeoCmdError::BadParams("rotation not object")) };
    match req_str(o, "kind")? {
        "identity" => Ok(RotationParam::Identity),
        "matrix" => {
            let Some(CValue::Arr(rows)) = o.get("rows") else {
                return Err(GeoCmdError::BadParams("matrix missing rows"));
            };
            if rows.len() != 3 {
                return Err(GeoCmdError::BadParams("matrix rows != 3"));
            }
            Ok(RotationParam::Matrix([parse_num_vec3(&rows[0])?, parse_num_vec3(&rows[1])?, parse_num_vec3(&rows[2])?]))
        }
        "quaternion" => Ok(RotationParam::Quaternion {
            w: req_num(o, "w")?,
            x: req_num(o, "x")?,
            y: req_num(o, "y")?,
            z: req_num(o, "z")?,
        }),
        "axis_angle" => Ok(RotationParam::AxisAngle {
            axis: parse_num_vec3(o.get("axis").ok_or(GeoCmdError::BadParams("missing axis"))?)?,
            angle_rad: req_num(o, "angle_rad")?,
        }),
        "euler" => {
            let seq = EulerSeq::from_name(req_str(o, "seq")?).ok_or(GeoCmdError::BadParams("bad euler seq"))?;
            let kind = match o.get("euler_kind") {
                None => EulerKind::Intrinsic,
                Some(CValue::Str(s)) if s == "intrinsic" => EulerKind::Intrinsic,
                Some(CValue::Str(s)) if s == "extrinsic" => EulerKind::Extrinsic,
                _ => return Err(GeoCmdError::BadParams("bad euler_kind")),
            };
            let angles_rad = parse_num_vec3(o.get("angles_rad").ok_or(GeoCmdError::BadParams("missing angles_rad"))?)?;
            Ok(RotationParam::Euler { seq, kind, angles_rad })
        }
        _ => Err(GeoCmdError::BadParams("unknown rotation kind")),
    }
}

fn parse_mcp_create_frame(o: &BTreeMap<String, CValue>) -> Result<CreateFrameParams, GeoCmdError> {
    let name = req_str(o, "name")?.to_owned();
    let parent = req_str(o, "parent")?.to_owned();
    let kind = match o.get("kind") {
        None | Some(CValue::Null) => FrameKind::User,
        Some(CValue::Str(s)) => parse_frame_kind(s)?,
        _ => return Err(GeoCmdError::BadParams("bad kind")),
    };
    let translation =
        parse_mcp_translation(o.get("translation").ok_or(GeoCmdError::BadParams("missing translation"))?)?;
    let rotation = parse_mcp_rotation(o.get("rotation"))?;
    let scale = match o.get("scale") {
        None | Some(CValue::Null) => None,
        Some(CValue::Num(n)) => Some(*n),
        Some(CValue::Str(s)) => Some(s.parse::<f64>().map_err(|_| GeoCmdError::BadParams("bad scale"))?),
        _ => return Err(GeoCmdError::BadParams("bad scale")),
    };
    Ok(CreateFrameParams { name, kind, parent, rotation, translation, scale })
}

fn parse_mcp_set_transform(o: &BTreeMap<String, CValue>) -> Result<SetTransformParams, GeoCmdError> {
    let frame = req_str(o, "frame")?.to_owned();
    let parent = match o.get("parent") {
        None | Some(CValue::Null) => None,
        Some(CValue::Str(s)) => Some(s.clone()),
        _ => return Err(GeoCmdError::BadParams("bad parent")),
    };
    let translation =
        parse_mcp_translation(o.get("translation").ok_or(GeoCmdError::BadParams("missing translation"))?)?;
    let rotation = parse_mcp_rotation(o.get("rotation"))?;
    let scale = match o.get("scale") {
        None | Some(CValue::Null) => None,
        Some(CValue::Num(n)) => Some(*n),
        Some(CValue::Str(s)) => Some(s.parse::<f64>().map_err(|_| GeoCmdError::BadParams("bad scale"))?),
        _ => return Err(GeoCmdError::BadParams("bad scale")),
    };
    Ok(SetTransformParams { frame, parent, rotation, translation, scale })
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

fn req_num(o: &BTreeMap<String, CValue>, k: &str) -> Result<f64, GeoCmdError> {
    match o.get(k) {
        Some(CValue::Num(n)) => Ok(*n),
        _ => Err(GeoCmdError::BadParams("missing number field")),
    }
}

fn parse_num_vec3(v: &CValue) -> Result<[f64; 3], GeoCmdError> {
    let CValue::Arr(a) = v else { return Err(GeoCmdError::BadParams("vec3 not array")) };
    if a.len() != 3 {
        return Err(GeoCmdError::BadParams("vec3 len != 3"));
    }
    let mut out = [0.0; 3];
    for (i, item) in a.iter().enumerate() {
        match item {
            CValue::Num(n) => out[i] = *n,
            _ => return Err(GeoCmdError::BadParams("vec3 element not num")),
        }
    }
    Ok(out)
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

/// Ensures `LengthUnit` is referenced cleanly for downstream callers.
pub fn supported_length_units() -> &'static [LengthUnit] {
    &[LengthUnit::M, LengthUnit::Mm, LengthUnit::Um, LengthUnit::In]
}

/// Exposes `GeometryElement` helper so tests can assert transformed elements cleanly.
pub fn element_frame(e: &GeometryElement) -> FrameRef {
    e.frame()
}
