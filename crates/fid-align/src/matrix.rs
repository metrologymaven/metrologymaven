//! Known-matrix and TRS transform entry command `align.set_matrix@1` (W2, D0; spec node
//! `ALN.XFM.MATRIX`, `REQ-001`..`REQ-006`, `AC-1`..`AC-7`).

use crate::command::{ID_PATTERN, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH};
use crate::element::{
    id_value, obj, parse_id, Alignment, AlignmentStatus, ElementRef, ParamSet, ParentRef, PrincipalKind, Provenance,
};
use crate::error::AlignError;
use crate::model::{AlignmentModel, MAX_ALIGNMENTS};
use crate::residuals::ResidualStore;
use fid_align_solve::matrix::{
    solve_matrix, MatrixError, MatrixInput, MatrixOrder, MatrixSolution, RotationInput, TransformDirection,
    MATRIX_ALGORITHM_ID,
};
use fid_align_solve::{Disposition, Entry, EntryKind, EntryRef, ReportBuilder};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_dep::{EdgeError, EdgeIndex, EdgesSet};
use fid_geo_frame::{AngleUnit, AuthoredQuantity, EulerKind, EulerSeq, LengthUnit, Transform, Unit, UnitsError};
use fid_kci::AlgorithmId;
use fid_project::IdService;
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Registry command ID and journal event type of `align.set_matrix@1`.
pub const SET_MATRIX: &str = "align.set_matrix@1";
/// CLI subcommand name (`fid align set-matrix`).
pub const CLI_SET_MATRIX: &str = "set-matrix";
/// MCP tool name (`align_set_matrix`).
pub const MCP_SET_MATRIX: &str = "align_set_matrix";
/// Maximum byte length of the untrusted source label (Design).
pub const MAX_SOURCE_CHARS: usize = 256;
/// Maximum CLI arguments.
const MAX_CLI_ARGS: usize = 48;
/// Largest serialised event accepted by [`SetMatrixEvent::from_canonical_bytes`].
pub const MAX_MATRIX_EVENT_BYTES: usize = 256 * 1024;
const MAX_EVENT_DEPTH: usize = 12;
const LIFECYCLE_SINCE: &str = "2026-11-23";
/// Fixed namespace for deterministic zero-entry report UUIDv5 derivation.
const REPORT_NS: Uuid =
    Uuid::from_bytes([0x6a, 0x4b, 0x2c, 0x81, 0x90, 0x3e, 0x4d, 0x12, 0x9b, 0x55, 0x10, 0x22, 0x33, 0x44, 0x55, 0x66]);

/// Single journaled conversion record for an authored numeric parameter (CONSTITUTION §4, `REQ-005`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionRecord {
    pub name: String,
    pub text: String,
    pub unit: String,
    pub si_hex: String,
}

impl ConversionRecord {
    fn from_qty(name: &str, q: &AuthoredQuantity) -> Self {
        Self {
            name: name.to_owned(),
            text: q.text().to_owned(),
            unit: q.unit().symbol().to_owned(),
            si_hex: q.si_hex(),
        }
    }

    pub fn to_value(&self) -> CValue {
        obj([
            ("name", CValue::Str(self.name.clone())),
            ("si_hex", CValue::Str(self.si_hex.clone())),
            ("text", CValue::Str(self.text.clone())),
            ("unit", CValue::Str(self.unit.clone())),
        ])
    }
}

/// Rotation specification as authored by the caller in `trs` form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrsRotationSpec {
    Quaternion { w: String, x: String, y: String, z: String },
    AxisAngle { axis: [String; 3], angle: String, angle_unit: String },
    Euler { sequence: EulerSeq, kind: EulerKind, angles: [String; 3], angle_unit: String },
}

/// Authored transform form of `align.set_matrix@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetMatrixForm {
    Matrix4 { entries: [String; 16], order: MatrixOrder, length_unit: String, scale_declared: bool },
    Trs { translation: [String; 3], length_unit: String, rotation: TrsRotationSpec, scale: Option<String> },
}

/// Validated parameters of `align.set_matrix@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetMatrixParams {
    pub form: SetMatrixForm,
    pub direction: TransformDirection,
    pub source: String,
    pub parent: Option<Uuid>,
    pub principal: String,
    pub principal_kind: PrincipalKind,
}

/// Errors of `align.set_matrix@1`. [`SetMatrixError::code`] is stable.
#[derive(Debug, Clone, PartialEq)]
pub enum SetMatrixError {
    ConventionUnspecified(&'static str),
    NotRigid { ortho_err: f64, det: f64 },
    NotHomogeneous,
    UnitUnknown(String),
    NanInput,
    BadParams(&'static str),
    Align(AlignError),
    Edge(EdgeError),
}

impl SetMatrixError {
    pub fn code(&self) -> &str {
        match self {
            SetMatrixError::ConventionUnspecified(_) => "convention_unspecified",
            SetMatrixError::NotRigid { .. } => "not_rigid",
            SetMatrixError::NotHomogeneous => "not_homogeneous",
            SetMatrixError::UnitUnknown(_) => "unit_unknown",
            SetMatrixError::NanInput => "nan_input",
            SetMatrixError::BadParams(_) => "bad_params",
            SetMatrixError::Align(e) => e.code(),
            SetMatrixError::Edge(e) => e.code(),
        }
    }

    pub fn to_value(&self) -> CValue {
        match self {
            SetMatrixError::NotRigid { ortho_err, det } => obj([
                ("code", CValue::Str("not_rigid".to_owned())),
                ("det", CValue::Str(format!("0x{:016x}", det.to_bits()))),
                ("message", CValue::Str(self.to_string())),
                ("ortho_err", CValue::Str(format!("0x{:016x}", ortho_err.to_bits()))),
            ]),
            _ => obj([("code", CValue::Str(self.code().to_owned())), ("message", CValue::Str(self.to_string()))]),
        }
    }
}

impl fmt::Display for SetMatrixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetMatrixError::ConventionUnspecified(d) => write!(f, "convention_unspecified{{{d}}}"),
            SetMatrixError::NotRigid { ortho_err, det } => {
                write!(f, "not_rigid{{ortho_err={ortho_err:e}, det={det:e}}}")
            }
            SetMatrixError::NotHomogeneous => f.write_str("not_homogeneous"),
            SetMatrixError::UnitUnknown(u) => write!(f, "unit_unknown{{{u}}}"),
            SetMatrixError::NanInput => f.write_str("nan_input"),
            SetMatrixError::BadParams(d) => write!(f, "bad_params{{{d}}}"),
            SetMatrixError::Align(e) => e.fmt(f),
            SetMatrixError::Edge(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for SetMatrixError {}

impl From<MatrixError> for SetMatrixError {
    fn from(e: MatrixError) -> Self {
        match e {
            MatrixError::ConventionUnspecified { detail } => SetMatrixError::ConventionUnspecified(detail),
            MatrixError::NotRigid { ortho_err, det } => SetMatrixError::NotRigid { ortho_err, det },
            MatrixError::NotHomogeneous => SetMatrixError::NotHomogeneous,
            MatrixError::NanInput => SetMatrixError::NanInput,
        }
    }
}

impl From<AlignError> for SetMatrixError {
    fn from(e: AlignError) -> Self {
        SetMatrixError::Align(e)
    }
}

/// Journal event emitted on a committed `align.set_matrix@1` (`REQ-005`, `AC-2`, `AC-7`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetMatrixEvent {
    pub id: Uuid,
    pub parent: Option<Uuid>,
    pub principal: String,
    pub principal_kind: PrincipalKind,
    pub form: SetMatrixForm,
    pub convention: String,
    pub direction: TransformDirection,
    pub length_unit: String,
    pub angle_unit: Option<String>,
    pub scale_declared: bool,
    pub source_untrusted: String,
    pub conversions: Vec<ConversionRecord>,
    pub alignment_digest: Digest,
}

impl SetMatrixEvent {
    /// RFC 8785 `CValue` representation (source stored as untrusted data per C6).
    pub fn to_value(&self) -> CValue {
        let form_val = match &self.form {
            SetMatrixForm::Matrix4 { entries, order, length_unit, scale_declared } => obj([
                ("entries", CValue::Arr(entries.iter().map(|s| CValue::Str(s.clone())).collect())),
                ("kind", CValue::Str("matrix4".to_owned())),
                ("length_unit", CValue::Str(length_unit.clone())),
                ("order", CValue::Str(order.as_str().to_owned())),
                ("scale_declared", CValue::Bool(*scale_declared)),
            ]),
            SetMatrixForm::Trs { translation, length_unit, rotation, scale } => {
                let rot_val = match rotation {
                    TrsRotationSpec::Quaternion { w, x, y, z } => obj([
                        ("kind", CValue::Str("quaternion".to_owned())),
                        ("w", CValue::Str(w.clone())),
                        ("x", CValue::Str(x.clone())),
                        ("y", CValue::Str(y.clone())),
                        ("z", CValue::Str(z.clone())),
                    ]),
                    TrsRotationSpec::AxisAngle { axis, angle, angle_unit } => obj([
                        ("angle", CValue::Str(angle.clone())),
                        ("angle_unit", CValue::Str(angle_unit.clone())),
                        ("axis", CValue::Arr(axis.iter().map(|s| CValue::Str(s.clone())).collect())),
                        ("kind", CValue::Str("axis_angle".to_owned())),
                    ]),
                    TrsRotationSpec::Euler { sequence, kind, angles, angle_unit } => obj([
                        ("angle_unit", CValue::Str(angle_unit.clone())),
                        ("angles", CValue::Arr(angles.iter().map(|s| CValue::Str(s.clone())).collect())),
                        ("euler_kind", CValue::Str(euler_kind_str(*kind).to_owned())),
                        ("kind", CValue::Str("euler".to_owned())),
                        ("sequence", CValue::Str(sequence.name())),
                    ]),
                };
                obj([
                    ("kind", CValue::Str("trs".to_owned())),
                    ("length_unit", CValue::Str(length_unit.clone())),
                    ("rotation", rot_val),
                    ("scale", scale.as_ref().map_or(CValue::Null, |s| CValue::Str(s.clone()))),
                    ("translation", CValue::Arr(translation.iter().map(|s| CValue::Str(s.clone())).collect())),
                ])
            }
        };
        obj([
            ("alignment_digest", CValue::Str(self.alignment_digest.as_str().to_owned())),
            ("angle_unit", self.angle_unit.as_ref().map_or(CValue::Null, |u| CValue::Str(u.clone()))),
            ("command", CValue::Str(SET_MATRIX.to_owned())),
            ("convention", CValue::Str(self.convention.clone())),
            ("conversions", CValue::Arr(self.conversions.iter().map(ConversionRecord::to_value).collect())),
            ("direction", CValue::Str(self.direction.as_str().to_owned())),
            ("form", form_val),
            ("id", id_value(&self.id)),
            ("length_unit", CValue::Str(self.length_unit.clone())),
            ("parent", self.parent.as_ref().map_or(CValue::Null, id_value)),
            ("principal", CValue::Str(self.principal.clone())),
            ("principal_kind", CValue::Str(self.principal_kind.as_str().to_owned())),
            ("scale_declared", CValue::Bool(self.scale_declared)),
            ("source", obj([("text", CValue::Str(self.source_untrusted.clone())), ("untrusted", CValue::Bool(true))])),
        ])
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, SetMatrixError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| SetMatrixError::BadParams("canonical_encode_failed"))
    }

    /// Parses a [`SetMatrixEvent`] strictly from canonical JSON bytes (`AC-7`).
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<SetMatrixEvent, SetMatrixError> {
        let v = parse_with_limits(bytes, MAX_MATRIX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => SetMatrixError::BadParams("event too large or too deep"),
            _ => SetMatrixError::BadParams("malformed event JSON"),
        })?;
        let CValue::Obj(o) = &v else { return Err(SetMatrixError::BadParams("event is not an object")) };
        if req_str(o, "command")? != SET_MATRIX {
            return Err(SetMatrixError::BadParams("unexpected command in event"));
        }
        let id = parse_id(req_str(o, "id")?).ok_or(SetMatrixError::BadParams("bad id"))?;
        let parent = match o.get("parent") {
            Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(parse_id(s).ok_or(SetMatrixError::BadParams("bad parent"))?),
            _ => return Err(SetMatrixError::BadParams("bad parent")),
        };
        let principal = req_str(o, "principal")?.to_owned();
        let principal_kind = PrincipalKind::parse(req_str(o, "principal_kind")?)?;
        let direction = TransformDirection::parse(req_str(o, "direction")?)
            .ok_or(SetMatrixError::ConventionUnspecified("direction"))?;
        let convention = req_str(o, "convention")?.to_owned();
        let length_unit = req_str(o, "length_unit")?.to_owned();
        let angle_unit = opt_str(o, "angle_unit")?.map(str::to_owned);
        let Some(CValue::Bool(scale_declared)) = o.get("scale_declared") else {
            return Err(SetMatrixError::BadParams("bad scale_declared"));
        };
        let Some(CValue::Obj(src)) = o.get("source") else {
            return Err(SetMatrixError::BadParams("bad source"));
        };
        if !matches!(src.get("untrusted"), Some(CValue::Bool(true))) {
            return Err(SetMatrixError::BadParams("source must be marked untrusted"));
        }
        let source_untrusted = req_str(src, "text")?.to_owned();
        let alignment_digest = Digest::parse(req_str(o, "alignment_digest")?)
            .map_err(|_| SetMatrixError::BadParams("bad alignment_digest"))?;
        let form = parse_event_form(o.get("form").ok_or(SetMatrixError::BadParams("missing form"))?)?;
        let Some(CValue::Arr(raw_conv)) = o.get("conversions") else {
            return Err(SetMatrixError::BadParams("missing conversions"));
        };
        let mut conversions = Vec::with_capacity(raw_conv.len());
        for c in raw_conv {
            let CValue::Obj(co) = c else { return Err(SetMatrixError::BadParams("bad conversion")) };
            conversions.push(ConversionRecord {
                name: req_str(co, "name")?.to_owned(),
                text: req_str(co, "text")?.to_owned(),
                unit: req_str(co, "unit")?.to_owned(),
                si_hex: req_str(co, "si_hex")?.to_owned(),
            });
        }
        Ok(SetMatrixEvent {
            id,
            parent,
            principal,
            principal_kind,
            form,
            convention,
            direction,
            length_unit,
            angle_unit,
            scale_declared: *scale_declared,
            source_untrusted,
            conversions,
            alignment_digest,
        })
    }
}

/// Output of committing `align.set_matrix@1`.
#[derive(Debug, Clone, PartialEq)]
pub struct SetMatrixCommit {
    pub alignment: Alignment,
    pub edges: EdgesSet,
    pub event: SetMatrixEvent,
    pub solution: MatrixSolution,
}

impl SetMatrixCommit {
    /// Canonical JSON result payload returned by CLI `--json` and MCP `align_set_matrix`.
    pub fn to_value(&self) -> CValue {
        obj([
            ("alignment_digest", CValue::Str(self.alignment.digest().as_str().to_owned())),
            ("command", CValue::Str(SET_MATRIX.to_owned())),
            ("convention", CValue::Str(self.event.convention.clone())),
            ("conversions", CValue::Arr(self.event.conversions.iter().map(ConversionRecord::to_value).collect())),
            ("direction", CValue::Str(self.event.direction.as_str().to_owned())),
            ("id", id_value(&self.alignment.id())),
            ("length_unit", CValue::Str(self.event.length_unit.clone())),
            ("scale", CValue::Str(format!("0x{:016x}", self.solution.scale.to_bits()))),
            (
                "source",
                obj([("text", CValue::Str(self.event.source_untrusted.clone())), ("untrusted", CValue::Bool(true))]),
            ),
            ("transform", transform_value(self.alignment.transform())),
        ])
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, SetMatrixError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| SetMatrixError::BadParams("canonical_encode_failed"))
    }
}

/// Executes `align.set_matrix@1`, updating `model`, `edges`, and `reports` atomically.
/// On any error nothing is mutated and no UUID is drawn (`REQ-001`..`REQ-006`).
pub fn set_matrix(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    ids: &mut IdService<'_>,
    params: &SetMatrixParams,
) -> Result<SetMatrixCommit, SetMatrixError> {
    let prepared = prepare(model, edges, params)?;
    let id = ids.new_v7().map_err(|e| SetMatrixError::Align(AlignError::Id(e)))?;
    commit_prepared(model, edges, reports, id, params, prepared)
}

/// Replays a committed `SetMatrixEvent` in exact mode (`AC-7`), verifying bit-identical
/// transform and alignment digest.
pub fn replay_set_matrix(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    raw_event: &[u8],
) -> Result<SetMatrixCommit, SetMatrixError> {
    let recorded = SetMatrixEvent::from_canonical_bytes(raw_event)?;
    let params = SetMatrixParams {
        form: recorded.form.clone(),
        direction: recorded.direction,
        source: recorded.source_untrusted.clone(),
        parent: recorded.parent,
        principal: recorded.principal.clone(),
        principal_kind: recorded.principal_kind,
    };
    let prepared = prepare(model, edges, &params)?;
    let committed = commit_prepared(model, edges, reports, recorded.id, &params, prepared)?;
    let rebuilt_bytes = committed.event.to_canonical_bytes()?;
    if rebuilt_bytes != raw_event || committed.alignment.digest() != recorded.alignment_digest {
        return Err(SetMatrixError::BadParams("replay_divergent"));
    }
    Ok(committed)
}

struct Prepared {
    provenance: Provenance,
    parent_ref: Option<ParentRef>,
    param_set: ParamSet,
    conversions: Vec<ConversionRecord>,
    convention: String,
    length_unit: String,
    angle_unit: Option<String>,
    scale_declared: bool,
    solution: MatrixSolution,
}

fn prepare(model: &AlignmentModel, edges: &EdgeIndex, params: &SetMatrixParams) -> Result<Prepared, SetMatrixError> {
    if model.len() >= MAX_ALIGNMENTS {
        return Err(SetMatrixError::Align(AlignError::InputLimit("alignments")));
    }
    if params.source.chars().count() > MAX_SOURCE_CHARS {
        return Err(SetMatrixError::BadParams("source label exceeds 256 chars"));
    }
    let provenance = Provenance::for_creation(&params.principal, params.principal_kind)?;

    let parent_ref = match params.parent {
        None => None,
        Some(pid) => {
            let p = model.get(&pid).ok_or(SetMatrixError::Align(AlignError::NotFound(pid)))?;
            if !edges.contains(&pid) {
                return Err(SetMatrixError::Align(AlignError::NotFound(pid)));
            }
            if p.status() != AlignmentStatus::Ok {
                return Err(SetMatrixError::Align(AlignError::ParentUnusable(pid)));
            }
            let next_depth = crate::hier::chain_depth(model, pid)
                .map_err(|_| SetMatrixError::Align(AlignError::Internal("parent hierarchy invalid")))?
                + 1;
            if next_depth > crate::hier::MAX_HIERARCHY_DEPTH {
                return Err(SetMatrixError::Align(AlignError::HierarchyTooDeep(next_depth)));
            }
            Some(ParentRef { id: pid, digest: p.digest() })
        }
    };

    let mut param_set = ParamSet::new();
    let mut conversions = Vec::new();
    let (kernel_input, convention, length_unit, angle_unit, scale_declared) = match &params.form {
        SetMatrixForm::Matrix4 { entries, order, length_unit, scale_declared } => {
            let lu = parse_length_unit(length_unit)?;
            let mut parsed = [0.0_f64; 16];
            for (i, s) in entries.iter().enumerate() {
                let v: f64 = s.trim().parse().map_err(|_| SetMatrixError::BadParams("matrix entry not a number"))?;
                if !v.is_finite() {
                    return Err(SetMatrixError::NanInput);
                }
                parsed[i] = v;
            }
            // Convert the 3 translation components from authored length_unit to SI metres via GEO.FRAME.UNITS.
            let t_indices = match order {
                MatrixOrder::RowMajor => [3usize, 7, 11],
                MatrixOrder::ColumnMajor => [12usize, 13, 14],
            };
            let t_names = ["tx", "ty", "tz"];
            for (&idx, &name) in t_indices.iter().zip(t_names.iter()) {
                let qty = parse_authored(&entries[idx], Unit::Length(lu))?;
                parsed[idx] = qty.si();
                param_set.insert(name, qty.text(), qty.unit().symbol())?;
                conversions.push(ConversionRecord::from_qty(name, &qty));
            }
            (
                MatrixInput::Matrix4 { entries: parsed, order: *order, scale_declared: *scale_declared },
                format!("matrix4:{}", order.as_str()),
                lu.symbol().to_owned(),
                None,
                *scale_declared,
            )
        }
        SetMatrixForm::Trs { translation, length_unit, rotation, scale } => {
            let lu = parse_length_unit(length_unit)?;
            let mut t_m = [0.0_f64; 3];
            for (i, name) in ["tx", "ty", "tz"].iter().enumerate() {
                let qty = parse_authored(&translation[i], Unit::Length(lu))?;
                t_m[i] = qty.si();
                param_set.insert(name, qty.text(), qty.unit().symbol())?;
                conversions.push(ConversionRecord::from_qty(name, &qty));
            }
            let (rot_in, conv, ang_unit) = match rotation {
                TrsRotationSpec::Quaternion { w, x, y, z } => {
                    let pw = parse_finite_f64(w)?;
                    let px = parse_finite_f64(x)?;
                    let py = parse_finite_f64(y)?;
                    let pz = parse_finite_f64(z)?;
                    (RotationInput::Quaternion { w: pw, x: px, y: py, z: pz }, "trs:quaternion".to_owned(), None)
                }
                TrsRotationSpec::AxisAngle { axis, angle, angle_unit } => {
                    let au = parse_angle_unit(angle_unit)?;
                    let ax = [parse_finite_f64(&axis[0])?, parse_finite_f64(&axis[1])?, parse_finite_f64(&axis[2])?];
                    let aq = parse_authored(angle, Unit::Angle(au))?;
                    param_set.insert("angle", aq.text(), aq.unit().symbol())?;
                    conversions.push(ConversionRecord::from_qty("angle", &aq));
                    (
                        RotationInput::AxisAngle { axis: ax, angle_rad: aq.si() },
                        "trs:axis_angle".to_owned(),
                        Some(au.symbol().to_owned()),
                    )
                }
                TrsRotationSpec::Euler { sequence, kind, angles, angle_unit } => {
                    let au = parse_angle_unit(angle_unit)?;
                    let mut a_rad = [0.0_f64; 3];
                    for (i, name) in ["angle_0", "angle_1", "angle_2"].iter().enumerate() {
                        let aq = parse_authored(&angles[i], Unit::Angle(au))?;
                        a_rad[i] = aq.si();
                        param_set.insert(name, aq.text(), aq.unit().symbol())?;
                        conversions.push(ConversionRecord::from_qty(name, &aq));
                    }
                    (
                        RotationInput::Euler { seq: *sequence, kind: *kind, angles_rad: a_rad },
                        format!("trs:euler:{}:{}", sequence.name(), euler_kind_str(*kind)),
                        Some(au.symbol().to_owned()),
                    )
                }
            };
            let parsed_scale = match scale {
                None => None,
                Some(s) => Some(parse_finite_f64(s)?),
            };
            (
                MatrixInput::Trs { translation_m: t_m, rotation: rot_in, scale: parsed_scale },
                conv,
                lu.symbol().to_owned(),
                ang_unit,
                parsed_scale.is_some(),
            )
        }
    };

    let solution = solve_matrix(&kernel_input, params.direction)?;
    Ok(Prepared {
        provenance,
        parent_ref,
        param_set,
        conversions,
        convention,
        length_unit,
        angle_unit,
        scale_declared,
        solution,
    })
}

fn commit_prepared(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    id: Uuid,
    params: &SetMatrixParams,
    prep: Prepared,
) -> Result<SetMatrixCommit, SetMatrixError> {
    if parse_id(&id.to_string()).is_none() || model.get(&id).is_some() || edges.contains(&id) {
        return Err(SetMatrixError::Align(AlignError::IdCollision(id)));
    }
    let alg = AlgorithmId::parse(MATRIX_ALGORITHM_ID)
        .map_err(|_| SetMatrixError::Align(AlignError::Internal("bad matrix algorithm id")))?;
    let mut rb = ReportBuilder::new();
    rb.push(
        Entry {
            reference: EntryRef { input: 0, item: 0 },
            kind: EntryKind::Feature,
            residual_m: 0.0,
            vector_m: Some([0.0, 0.0, 0.0]),
            weight: None,
        },
        Disposition::Used,
    )
    .map_err(|_| SetMatrixError::Align(AlignError::Internal("residual report entry failed")))?;
    let report =
        rb.finish().map_err(|_| SetMatrixError::Align(AlignError::Internal("residual report build failed")))?;
    let report_id = fid_uuid::uuid_v5(&REPORT_NS, format!("matrix-report:{id}").as_bytes());
    let report_ref = ElementRef { id: report_id, digest: report.digest() };

    let alignment = Alignment::assemble(
        id,
        alg,
        Vec::new(),
        prep.param_set,
        prep.solution.transform,
        prep.parent_ref,
        report_ref,
        prep.provenance,
        AlignmentStatus::Ok,
        None,
    )?;

    let mut from: BTreeSet<Uuid> = BTreeSet::new();
    if let Some(p) = &prep.parent_ref {
        from.insert(p.id);
    }
    let set = EdgesSet::from_set(id, &from).map_err(SetMatrixError::Edge)?;
    edges.apply(std::slice::from_ref(&set)).map_err(SetMatrixError::Edge)?;
    reports.insert(report_id, report)?;
    model.insert_stored(alignment.clone())?;

    let event = SetMatrixEvent {
        id,
        parent: params.parent,
        principal: params.principal.clone(),
        principal_kind: params.principal_kind,
        form: params.form.clone(),
        convention: prep.convention,
        direction: params.direction,
        length_unit: prep.length_unit,
        angle_unit: prep.angle_unit,
        scale_declared: prep.scale_declared,
        source_untrusted: params.source.clone(),
        conversions: prep.conversions,
        alignment_digest: alignment.digest(),
    };
    Ok(SetMatrixCommit { alignment, edges: set, event, solution: prep.solution })
}

/// Runs MCP tool `align_set_matrix` on raw JSON parameters.
pub fn invoke_set_matrix_mcp(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    ids: &mut IdService<'_>,
    caller: (&str, PrincipalKind),
    tool: &str,
    raw: &[u8],
) -> Result<SetMatrixCommit, SetMatrixError> {
    if tool != MCP_SET_MATRIX {
        return Err(SetMatrixError::BadParams("unknown tool"));
    }
    let v = parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => SetMatrixError::BadParams("params too large or too deep"),
        CanonError::DuplicateKey(_) => SetMatrixError::BadParams("duplicate key"),
        _ => SetMatrixError::BadParams("params are not valid JSON"),
    })?;
    let params = parse_params_value(&v, caller.0, caller.1)?;
    set_matrix(model, edges, reports, ids, &params)
}

/// Runs CLI `fid align set-matrix ... --json`.
pub fn invoke_set_matrix_cli(
    model: &mut AlignmentModel,
    edges: &mut EdgeIndex,
    reports: &mut ResidualStore,
    ids: &mut IdService<'_>,
    principal: &str,
    principal_kind: PrincipalKind,
    args: &[&str],
) -> Result<SetMatrixCommit, SetMatrixError> {
    if args.len() > MAX_CLI_ARGS {
        return Err(SetMatrixError::BadParams("too many arguments"));
    }
    let (sub, rest) = args.split_first().ok_or(SetMatrixError::BadParams("missing subcommand"))?;
    if *sub != CLI_SET_MATRIX {
        return Err(SetMatrixError::BadParams("unknown subcommand"));
    }
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    let mut scale_flag = false;
    let mut json = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match *arg {
            "--json" if !json => {
                json = true;
                continue;
            }
            "--json" => return Err(SetMatrixError::BadParams("repeated --json")),
            "--scale" => {
                scale_flag = true;
                continue;
            }
            "--form" | "--order" | "--matrix" | "--translation" | "--length-unit" | "--rotation-kind" | "--quat"
            | "--axis" | "--angle" | "--euler-seq" | "--euler-kind" | "--angles" | "--angle-unit" | "--scale-value"
            | "--direction" | "--source" | "--parent" => {
                let val = it.next().ok_or(SetMatrixError::BadParams("option needs a value"))?;
                if map.insert((*arg).to_owned(), (*val).to_owned()).is_some() {
                    return Err(SetMatrixError::BadParams("repeated option"));
                }
            }
            s if s.starts_with('-') => return Err(SetMatrixError::BadParams("unknown option")),
            _ => return Err(SetMatrixError::BadParams("unexpected argument")),
        }
    }

    let form_name =
        map.get("--form").map(String::as_str).ok_or(SetMatrixError::ConventionUnspecified("missing form"))?;
    let direction_s =
        map.get("--direction").map(String::as_str).ok_or(SetMatrixError::ConventionUnspecified("missing direction"))?;
    let direction =
        TransformDirection::parse(direction_s).ok_or(SetMatrixError::ConventionUnspecified("invalid direction"))?;
    let source = map.get("--source").cloned().unwrap_or_default();
    let parent = match map.get("--parent") {
        None => None,
        Some(s) => Some(parse_id(s).ok_or(SetMatrixError::BadParams("bad parent id"))?),
    };

    let form = match form_name {
        "matrix4" => {
            let order_s = map
                .get("--order")
                .map(String::as_str)
                .ok_or(SetMatrixError::ConventionUnspecified("missing matrix order"))?;
            let order =
                MatrixOrder::parse(order_s).ok_or(SetMatrixError::ConventionUnspecified("invalid matrix order"))?;
            let mat_s = map.get("--matrix").ok_or(SetMatrixError::BadParams("missing --matrix"))?;
            let entries = split_fixed::<16>(mat_s, "matrix must have 16 comma-separated entries")?;
            let length_unit = map.get("--length-unit").cloned().unwrap_or_else(|| "m".to_owned());
            SetMatrixForm::Matrix4 { entries, order, length_unit, scale_declared: scale_flag }
        }
        "trs" => {
            let t_s = map.get("--translation").ok_or(SetMatrixError::BadParams("missing --translation"))?;
            let translation = split_fixed::<3>(t_s, "translation must have 3 comma-separated entries")?;
            let length_unit = map
                .get("--length-unit")
                .cloned()
                .ok_or(SetMatrixError::ConventionUnspecified("missing length_unit"))?;
            let rot_kind = map
                .get("--rotation-kind")
                .map(String::as_str)
                .ok_or(SetMatrixError::ConventionUnspecified("missing rotation convention"))?;
            let rotation = match rot_kind {
                "quaternion" => {
                    let q_s = map.get("--quat").ok_or(SetMatrixError::BadParams("missing --quat"))?;
                    let [w, x, y, z] = split_fixed::<4>(q_s, "quat must have 4 entries (w,x,y,z)")?;
                    TrsRotationSpec::Quaternion { w, x, y, z }
                }
                "axis_angle" => {
                    let ax_s = map.get("--axis").ok_or(SetMatrixError::BadParams("missing --axis"))?;
                    let axis = split_fixed::<3>(ax_s, "axis must have 3 entries")?;
                    let angle = map.get("--angle").cloned().ok_or(SetMatrixError::BadParams("missing --angle"))?;
                    let angle_unit = map
                        .get("--angle-unit")
                        .cloned()
                        .ok_or(SetMatrixError::ConventionUnspecified("missing angle_unit"))?;
                    TrsRotationSpec::AxisAngle { axis, angle, angle_unit }
                }
                "euler" => {
                    let seq_s = map
                        .get("--euler-seq")
                        .ok_or(SetMatrixError::ConventionUnspecified("missing euler sequence"))?;
                    let sequence = EulerSeq::from_name(seq_s)
                        .ok_or(SetMatrixError::ConventionUnspecified("unknown euler sequence"))?;
                    let ek_s =
                        map.get("--euler-kind").ok_or(SetMatrixError::ConventionUnspecified("missing euler kind"))?;
                    let kind =
                        parse_euler_kind(ek_s).ok_or(SetMatrixError::ConventionUnspecified("invalid euler kind"))?;
                    let ang_s = map.get("--angles").ok_or(SetMatrixError::BadParams("missing --angles"))?;
                    let angles = split_fixed::<3>(ang_s, "angles must have 3 entries")?;
                    let angle_unit = map
                        .get("--angle-unit")
                        .cloned()
                        .ok_or(SetMatrixError::ConventionUnspecified("missing angle_unit"))?;
                    TrsRotationSpec::Euler { sequence, kind, angles, angle_unit }
                }
                _ => return Err(SetMatrixError::ConventionUnspecified("unknown rotation convention")),
            };
            let scale = map.get("--scale-value").cloned();
            SetMatrixForm::Trs { translation, length_unit, rotation, scale }
        }
        _ => return Err(SetMatrixError::ConventionUnspecified("unknown form")),
    };

    let params = SetMatrixParams { form, direction, source, parent, principal: principal.to_owned(), principal_kind };
    set_matrix(model, edges, reports, ids, &params)
}

/// Returns the validated [`CommandDef`] for `align.set_matrix@1`.
pub fn set_matrix_command_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_owned());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("angle_unit", obj([("type", text("string"))])),
                (
                    "direction",
                    obj([("enum", strs(&["data_to_reference", "reference_to_data"])), ("type", text("string"))]),
                ),
                ("form", obj([("enum", strs(&["matrix4", "trs"])), ("type", text("string"))])),
                ("length_unit", obj([("type", text("string"))])),
                ("matrix", obj([("type", text("array"))])),
                ("order", obj([("enum", strs(&["row_major", "column_major"])), ("type", text("string"))])),
                ("parent", obj([("pattern", text(ID_PATTERN)), ("type", text("string"))])),
                ("rotation", obj([("type", text("object"))])),
                ("scale", obj([("type", strs(&["boolean", "string", "number"]))])),
                ("source", obj([("maxLength", CValue::Num(MAX_SOURCE_CHARS as f64)), ("type", text("string"))])),
                ("translation", obj([("type", text("array"))])),
            ]),
        ),
        ("required", strs(&["direction", "form"])),
        ("type", text("object")),
    ]);
    let result_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("alignment_digest", obj([("type", text("string"))])),
                ("command", obj([("enum", strs(&[SET_MATRIX])), ("type", text("string"))])),
                ("convention", obj([("type", text("string"))])),
                ("conversions", obj([("type", text("array"))])),
                (
                    "direction",
                    obj([("enum", strs(&["data_to_reference", "reference_to_data"])), ("type", text("string"))]),
                ),
                ("id", obj([("pattern", text(ID_PATTERN)), ("type", text("string"))])),
                ("length_unit", obj([("type", text("string"))])),
                ("scale", obj([("pattern", text("^0x[0-9a-f]{16}$")), ("type", text("string"))])),
                ("source", obj([("type", text("object"))])),
                ("transform", obj([("type", text("object"))])),
            ]),
        ),
        (
            "required",
            strs(&[
                "alignment_digest",
                "command",
                "convention",
                "conversions",
                "direction",
                "id",
                "length_unit",
                "scale",
                "source",
                "transform",
            ]),
        ),
        ("type", text("object")),
    ]);
    CommandDef::from_value(obj([
        ("algorithms", strs(&[MATRIX_ALGORITHM_ID])),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            text("Creates an alignment from a known 4x4 matrix or translation+rotation in a stated convention, validating rigidity or declared similarity."),
        ),
        ("determinism", text("D0")),
        ("effects", strs(&[SET_MATRIX])),
        ("exposure", strs(&["cli", "mcp"])),
        ("fs_effect", text("none")),
        ("human_only", CValue::Bool(false)),
        ("id", text("align.set_matrix")),
        ("idempotency", text("none")),
        ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", text("exposed")),
        ("network", text("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", text("W2")),
        ("title", text("Create alignment from known matrix or TRS")),
        ("undo", text("head_move")),
        ("web", text("available")),
    ]))
}

// ------------------------------------------------------------------ parsing helpers

fn parse_params_value(
    v: &CValue,
    principal: &str,
    principal_kind: PrincipalKind,
) -> Result<SetMatrixParams, SetMatrixError> {
    let CValue::Obj(o) = v else { return Err(SetMatrixError::BadParams("params is not an object")) };
    for k in o.keys() {
        if !matches!(
            k.as_str(),
            "form"
                | "direction"
                | "order"
                | "matrix"
                | "translation"
                | "length_unit"
                | "rotation"
                | "angle_unit"
                | "scale"
                | "source"
                | "parent"
        ) {
            return Err(SetMatrixError::BadParams("unknown parameter"));
        }
    }
    let form_s = opt_str(o, "form")?.ok_or(SetMatrixError::ConventionUnspecified("missing form"))?;
    let dir_s = opt_str(o, "direction")?.ok_or(SetMatrixError::ConventionUnspecified("missing direction"))?;
    let direction =
        TransformDirection::parse(dir_s).ok_or(SetMatrixError::ConventionUnspecified("invalid direction"))?;
    let source = opt_str(o, "source")?.unwrap_or("").to_owned();
    let parent = match opt_str(o, "parent")? {
        None => None,
        Some(s) => Some(parse_id(s).ok_or(SetMatrixError::BadParams("bad parent id"))?),
    };

    let form = match form_s {
        "matrix4" => {
            let order_s = opt_str(o, "order")?.ok_or(SetMatrixError::ConventionUnspecified("missing order"))?;
            let order = MatrixOrder::parse(order_s).ok_or(SetMatrixError::ConventionUnspecified("invalid order"))?;
            let entries = parse_str_or_num_array::<16>(o.get("matrix"), "matrix must have 16 entries")?;
            let length_unit = opt_str(o, "length_unit")?.unwrap_or("m").to_owned();
            let scale_declared = match o.get("scale") {
                None | Some(CValue::Null) => false,
                Some(CValue::Bool(b)) => *b,
                _ => return Err(SetMatrixError::BadParams("scale for matrix4 must be boolean")),
            };
            SetMatrixForm::Matrix4 { entries, order, length_unit, scale_declared }
        }
        "trs" => {
            let translation = parse_str_or_num_array::<3>(o.get("translation"), "translation must have 3 entries")?;
            let length_unit = opt_str(o, "length_unit")?
                .ok_or(SetMatrixError::ConventionUnspecified("missing length_unit"))?
                .to_owned();
            let Some(CValue::Obj(ro)) = o.get("rotation") else {
                return Err(SetMatrixError::ConventionUnspecified("missing rotation"));
            };
            let rkind = opt_str(ro, "kind")?.ok_or(SetMatrixError::ConventionUnspecified("missing rotation kind"))?;
            let rotation = match rkind {
                "quaternion" => TrsRotationSpec::Quaternion {
                    w: value_to_num_str(ro.get("w"), "missing w")?,
                    x: value_to_num_str(ro.get("x"), "missing x")?,
                    y: value_to_num_str(ro.get("y"), "missing y")?,
                    z: value_to_num_str(ro.get("z"), "missing z")?,
                },
                "axis_angle" => {
                    let axis = parse_str_or_num_array::<3>(ro.get("axis"), "axis must have 3 entries")?;
                    let angle = value_to_num_str(ro.get("angle"), "missing angle")?;
                    let angle_unit = opt_str(ro, "angle_unit")?
                        .or(opt_str(o, "angle_unit")?)
                        .ok_or(SetMatrixError::ConventionUnspecified("missing angle_unit"))?
                        .to_owned();
                    TrsRotationSpec::AxisAngle { axis, angle, angle_unit }
                }
                "euler" => {
                    let seq_s = opt_str(ro, "sequence")?
                        .ok_or(SetMatrixError::ConventionUnspecified("missing euler sequence"))?;
                    let sequence = EulerSeq::from_name(seq_s)
                        .ok_or(SetMatrixError::ConventionUnspecified("unknown euler sequence"))?;
                    let ek_s = opt_str(ro, "euler_kind")?
                        .or(opt_str(ro, "convention")?)
                        .ok_or(SetMatrixError::ConventionUnspecified("missing euler kind"))?;
                    let kind =
                        parse_euler_kind(ek_s).ok_or(SetMatrixError::ConventionUnspecified("invalid euler kind"))?;
                    let angles = parse_str_or_num_array::<3>(ro.get("angles"), "angles must have 3 entries")?;
                    let angle_unit = opt_str(ro, "angle_unit")?
                        .or(opt_str(o, "angle_unit")?)
                        .ok_or(SetMatrixError::ConventionUnspecified("missing angle_unit"))?
                        .to_owned();
                    TrsRotationSpec::Euler { sequence, kind, angles, angle_unit }
                }
                _ => return Err(SetMatrixError::ConventionUnspecified("unknown rotation kind")),
            };
            let scale = match o.get("scale") {
                None | Some(CValue::Null) | Some(CValue::Bool(false)) => None,
                Some(CValue::Str(s)) => Some(s.clone()),
                Some(CValue::Num(n)) => Some(format!("{n}")),
                _ => return Err(SetMatrixError::BadParams("invalid scale")),
            };
            SetMatrixForm::Trs { translation, length_unit, rotation, scale }
        }
        _ => return Err(SetMatrixError::ConventionUnspecified("unknown form")),
    };
    Ok(SetMatrixParams { form, direction, source, parent, principal: principal.to_owned(), principal_kind })
}

fn parse_event_form(v: &CValue) -> Result<SetMatrixForm, SetMatrixError> {
    let CValue::Obj(o) = v else { return Err(SetMatrixError::BadParams("bad form")) };
    match req_str(o, "kind")? {
        "matrix4" => {
            let entries = parse_str_or_num_array::<16>(o.get("entries"), "bad matrix entries")?;
            let order = MatrixOrder::parse(req_str(o, "order")?).ok_or(SetMatrixError::BadParams("bad order"))?;
            let length_unit = req_str(o, "length_unit")?.to_owned();
            let Some(CValue::Bool(scale_declared)) = o.get("scale_declared") else {
                return Err(SetMatrixError::BadParams("bad scale_declared"));
            };
            Ok(SetMatrixForm::Matrix4 { entries, order, length_unit, scale_declared: *scale_declared })
        }
        "trs" => {
            let translation = parse_str_or_num_array::<3>(o.get("translation"), "bad translation")?;
            let length_unit = req_str(o, "length_unit")?.to_owned();
            let scale = opt_str(o, "scale")?.map(str::to_owned);
            let Some(CValue::Obj(ro)) = o.get("rotation") else {
                return Err(SetMatrixError::BadParams("bad rotation"));
            };
            let rotation = match req_str(ro, "kind")? {
                "quaternion" => TrsRotationSpec::Quaternion {
                    w: req_str(ro, "w")?.to_owned(),
                    x: req_str(ro, "x")?.to_owned(),
                    y: req_str(ro, "y")?.to_owned(),
                    z: req_str(ro, "z")?.to_owned(),
                },
                "axis_angle" => TrsRotationSpec::AxisAngle {
                    axis: parse_str_or_num_array::<3>(ro.get("axis"), "bad axis")?,
                    angle: req_str(ro, "angle")?.to_owned(),
                    angle_unit: req_str(ro, "angle_unit")?.to_owned(),
                },
                "euler" => TrsRotationSpec::Euler {
                    sequence: EulerSeq::from_name(req_str(ro, "sequence")?)
                        .ok_or(SetMatrixError::BadParams("bad sequence"))?,
                    kind: parse_euler_kind(req_str(ro, "euler_kind")?)
                        .ok_or(SetMatrixError::BadParams("bad euler_kind"))?,
                    angles: parse_str_or_num_array::<3>(ro.get("angles"), "bad angles")?,
                    angle_unit: req_str(ro, "angle_unit")?.to_owned(),
                },
                _ => return Err(SetMatrixError::BadParams("bad rotation kind")),
            };
            Ok(SetMatrixForm::Trs { translation, length_unit, rotation, scale })
        }
        _ => Err(SetMatrixError::BadParams("bad form kind")),
    }
}

fn parse_length_unit(sym: &str) -> Result<LengthUnit, SetMatrixError> {
    match Unit::from_symbol(sym) {
        Some(Unit::Length(u)) => Ok(u),
        _ => Err(SetMatrixError::UnitUnknown(sym.to_owned())),
    }
}

fn parse_angle_unit(sym: &str) -> Result<AngleUnit, SetMatrixError> {
    match Unit::from_symbol(sym) {
        Some(Unit::Angle(u)) => Ok(u),
        _ => Err(SetMatrixError::UnitUnknown(sym.to_owned())),
    }
}

fn parse_authored(text: &str, unit: Unit) -> Result<AuthoredQuantity, SetMatrixError> {
    AuthoredQuantity::parse(text.trim(), unit).map_err(|e| match e {
        UnitsError::NonFinite => SetMatrixError::NanInput,
        other => SetMatrixError::Align(AlignError::BadQuantity(other)),
    })
}

fn parse_finite_f64(text: &str) -> Result<f64, SetMatrixError> {
    let v: f64 = text.trim().parse().map_err(|_| SetMatrixError::BadParams("invalid numeric value"))?;
    if !v.is_finite() {
        return Err(SetMatrixError::NanInput);
    }
    Ok(v)
}

fn euler_kind_str(k: EulerKind) -> &'static str {
    match k {
        EulerKind::Intrinsic => "intrinsic",
        EulerKind::Extrinsic => "extrinsic",
    }
}

fn parse_euler_kind(s: &str) -> Option<EulerKind> {
    match s {
        "intrinsic" => Some(EulerKind::Intrinsic),
        "extrinsic" => Some(EulerKind::Extrinsic),
        _ => None,
    }
}

fn split_fixed<const N: usize>(s: &str, err: &'static str) -> Result<[String; N], SetMatrixError> {
    let parts: Vec<String> = s.split(',').map(|p| p.trim().to_owned()).collect();
    if parts.len() != N {
        return Err(SetMatrixError::BadParams(err));
    }
    let arr: [String; N] = parts.try_into().map_err(|_| SetMatrixError::BadParams(err))?;
    Ok(arr)
}

fn parse_str_or_num_array<const N: usize>(
    v: Option<&CValue>,
    err: &'static str,
) -> Result<[String; N], SetMatrixError> {
    let Some(CValue::Arr(a)) = v else { return Err(SetMatrixError::BadParams(err)) };
    if a.len() != N {
        return Err(SetMatrixError::BadParams(err));
    }
    let mut out = Vec::with_capacity(N);
    for item in a {
        out.push(value_to_num_str(Some(item), err)?);
    }
    out.try_into().map_err(|_| SetMatrixError::BadParams(err))
}

fn value_to_num_str(v: Option<&CValue>, err: &'static str) -> Result<String, SetMatrixError> {
    match v {
        Some(CValue::Str(s)) => Ok(s.clone()),
        Some(CValue::Num(n)) => {
            if !n.is_finite() {
                return Err(SetMatrixError::NanInput);
            }
            Ok(format!("{n}"))
        }
        _ => Err(SetMatrixError::BadParams(err)),
    }
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, SetMatrixError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(SetMatrixError::BadParams("missing or non-string field")),
    }
}

fn opt_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<Option<&'a str>, SetMatrixError> {
    match o.get(k) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.as_str())),
        _ => Err(SetMatrixError::BadParams("field is not a string")),
    }
}

fn transform_value(t: &Transform) -> CValue {
    let r = t.rotation();
    let tr = t.translation();
    let row = |x: [f64; 3]| CValue::Arr(vec![CValue::F64Bits(x[0]), CValue::F64Bits(x[1]), CValue::F64Bits(x[2])]);
    let r_val = CValue::Arr(vec![row(r[0]), row(r[1]), row(r[2])]);
    let t_val = row(tr);
    if t.is_rigid() {
        obj([("kind", CValue::Str("rigid".to_owned())), ("r", r_val), ("t", t_val)])
    } else {
        obj([
            ("kind", CValue::Str("similarity".to_owned())),
            ("r", r_val),
            ("s", CValue::F64Bits(t.scale())),
            ("t", t_val),
        ])
    }
}
