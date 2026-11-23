//! Distance and caliper checks (spec node `GDT.DIM.LINEAR`): commands `check.eval_distance@1`
//! and `check.eval_caliper@1` (W2, dry-run R1; CLI `fid check eval-distance`,
//! `fid check eval-caliper`; MCP `check_eval_distance`, `check_eval_caliper`), journal events,
//! ResultRecord construction and exact replay (`REQ-001`..`REQ-006`, `AC-1`..`AC-7`).

pub mod command;
pub mod event;

pub use command::{
    caliper_command_def, distance_command_def, invoke_cli, invoke_mcp, EVAL_CALIPER_CLI, EVAL_CALIPER_MCP,
    EVAL_DISTANCE_CLI, EVAL_DISTANCE_MCP,
};
pub use event::{replay, LinearEvent, LinearReplayed};

use crate::chars::dprof::{Branch, CharId, DprofState};
use crate::chars::interp::{InterpretationPin, Interpreted, RegisterView, PROVISIONAL_LABEL_TEXT, SIGNED_OFF_LINE};
use crate::chars::record::{
    AlgorithmRef, EvaluationBasis, InputDigests, KernelVersion, Measurand, Quantity, RecordError, ResultRecord,
    ResultRecordBuilder, ResultStore, RunId, Uncertainty,
};
use crate::chars::spec::{SpecError, SpecState};
use crate::dmt_flow::status::Purpose;
use crate::resolve::{resolve_chain, CharacteristicRef, CommandError, CHAR_SCHEMA};
use fid_canon::CValue;
use fid_gps::dim::linear::{
    evaluate_caliper, evaluate_distance, CaliperValue, DistanceFlag, DistanceMeasurand, DistanceValue, LengthUnit,
    LinearElement, LinearError, ReferenceSegment,
};
use fid_gps::dmt::DmtTable;
use fid_gps::resolve::Resolution;
use fid_io_reg::Principal;
use fid_kunc::Uec0Reason;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Registry command ID for distance checks (`GDT.DIM.LINEAR`).
pub const EVAL_DISTANCE_COMMAND: &str = "check.eval_distance@1";
/// Registry command ID for caliper checks (`GDT.DIM.LINEAR`).
pub const EVAL_CALIPER_COMMAND: &str = "check.eval_caliper@1";
/// Schema of the command result JSON object.
pub const RESULT_SCHEMA: &str = "fid.check-linear-result/1";
/// Internal record unit: SI metres (`si:m`).
pub const UNIT: &str = "si:m";
/// Maximum elements, frames, or point sets in one [`LinearStore`].
pub const MAX_ENTRIES: usize = 4096;
/// Maximum points in a single caliper point set.
pub const MAX_POINT_SET_LEN: usize = 1 << 20;
/// Maximum identifier length in bytes.
pub const MAX_ID_LEN: usize = 128;
const MAX_ECHO: usize = 160;

pub(crate) fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

pub(crate) fn is_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_ID_LEN
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
        && s.as_bytes()[0].is_ascii_alphanumeric()
}

pub(crate) fn hex(v: f64) -> String {
    format!("0x{:016x}", v.to_bits())
}

pub(crate) fn vec3_hex(v: [f64; 3]) -> CValue {
    CValue::Arr(vec![CValue::Str(hex(v[0])), CValue::Str(hex(v[1])), CValue::Str(hex(v[2]))])
}

pub(crate) fn obj<const N: usize>(items: [(&str, CValue); N]) -> CValue {
    CValue::Obj(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// Named coordinate-frame axis for `DistanceMeasurandSpec::Component`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FrameAxis {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl FrameAxis {
    pub const fn as_str(self) -> &'static str {
        match self {
            FrameAxis::PosX => "x",
            FrameAxis::NegX => "-x",
            FrameAxis::PosY => "y",
            FrameAxis::NegY => "-y",
            FrameAxis::PosZ => "z",
            FrameAxis::NegZ => "-z",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "x" | "+x" | "X" | "+X" => Some(FrameAxis::PosX),
            "-x" | "-X" => Some(FrameAxis::NegX),
            "y" | "+y" | "Y" | "+Y" => Some(FrameAxis::PosY),
            "-y" | "-Y" => Some(FrameAxis::NegY),
            "z" | "+z" | "Z" | "+Z" => Some(FrameAxis::PosZ),
            "-z" | "-Z" => Some(FrameAxis::NegZ),
            _ => None,
        }
    }
}

/// Named measurand specification supplied to `check.eval_distance@1` (`REQ-002`, `AC-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistanceMeasurandSpec {
    ThreeD,
    Projected { plane: String },
    Component { frame: String, axis: FrameAxis },
}

impl DistanceMeasurandSpec {
    pub fn to_value(&self) -> CValue {
        match self {
            DistanceMeasurandSpec::ThreeD => obj([("kind", CValue::Str("3d".to_owned()))]),
            DistanceMeasurandSpec::Projected { plane } => {
                obj([("kind", CValue::Str("projected".to_owned())), ("plane", CValue::Str(plane.clone()))])
            }
            DistanceMeasurandSpec::Component { frame, axis } => obj([
                ("axis", CValue::Str(axis.as_str().to_owned())),
                ("frame", CValue::Str(frame.clone())),
                ("kind", CValue::Str("component".to_owned())),
            ]),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            DistanceMeasurandSpec::ThreeD => "3d".to_owned(),
            DistanceMeasurandSpec::Projected { plane } => format!("projected({plane})"),
            DistanceMeasurandSpec::Component { frame, axis } => format!("component({frame}:{})", axis.as_str()),
        }
    }
}

/// Stored geometric element with its canonical digest.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredElement {
    pub element: LinearElement,
    pub digest: Digest,
}

/// Stored coordinate frame axes (unit basis vectors in world coordinates) with canonical digest.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredFrame {
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
    pub digest: Digest,
}

impl StoredFrame {
    pub fn axis_vec(&self, axis: FrameAxis) -> [f64; 3] {
        match axis {
            FrameAxis::PosX => self.x,
            FrameAxis::NegX => [-self.x[0], -self.x[1], -self.x[2]],
            FrameAxis::PosY => self.y,
            FrameAxis::NegY => [-self.y[0], -self.y[1], -self.y[2]],
            FrameAxis::PosZ => self.z,
            FrameAxis::NegZ => [-self.z[0], -self.z[1], -self.z[2]],
        }
    }
}

/// Stored point set for `check.eval_caliper@1` with canonical digest.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredPointSet {
    pub points: Vec<[f64; 3]>,
    pub digest: Digest,
}

/// Read-only lookup of named elements, frames, and point sets for `GDT.DIM.LINEAR` commands.
#[derive(Debug, Clone, Default)]
pub struct LinearStore {
    elements: BTreeMap<String, StoredElement>,
    frames: BTreeMap<String, StoredFrame>,
    point_sets: BTreeMap<String, StoredPointSet>,
}

impl LinearStore {
    pub fn new() -> LinearStore {
        LinearStore::default()
    }

    /// Inserts a named element (point, line, or plane) and returns its canonical digest.
    pub fn insert_element(&mut self, id: &str, element: LinearElement) -> Result<Digest, EvalError> {
        if !is_id(id) {
            return Err(EvalError::BadParams { reason: "bad_element_id" });
        }
        if !self.elements.contains_key(id) && self.elements.len() >= MAX_ENTRIES {
            return Err(EvalError::BadParams { reason: "too_many_elements" });
        }
        let canon = match element {
            LinearElement::Point { position, raw_point } => obj([
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("point".to_owned())),
                ("position", vec3_hex(position)),
                ("raw_point", CValue::Bool(raw_point)),
            ]),
            LinearElement::Line { point, direction } => obj([
                ("direction", vec3_hex(direction)),
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("line".to_owned())),
                ("point", vec3_hex(point)),
            ]),
            LinearElement::Plane { centroid, normal } => obj([
                ("centroid", vec3_hex(centroid)),
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("plane".to_owned())),
                ("normal", vec3_hex(normal)),
            ]),
        };
        let bytes = fid_canon::to_canonical_bytes(&canon)
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
        let digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
        self.elements.insert(id.to_owned(), StoredElement { element, digest });
        Ok(digest)
    }

    /// Inserts a named coordinate frame with axes `x`, `y`, `z` and returns its canonical digest.
    pub fn insert_frame(&mut self, id: &str, x: [f64; 3], y: [f64; 3], z: [f64; 3]) -> Result<Digest, EvalError> {
        if !is_id(id) {
            return Err(EvalError::BadParams { reason: "bad_frame_id" });
        }
        if !self.frames.contains_key(id) && self.frames.len() >= MAX_ENTRIES {
            return Err(EvalError::BadParams { reason: "too_many_frames" });
        }
        let canon =
            obj([("id", CValue::Str(id.to_owned())), ("x", vec3_hex(x)), ("y", vec3_hex(y)), ("z", vec3_hex(z))]);
        let bytes = fid_canon::to_canonical_bytes(&canon)
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
        let digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
        self.frames.insert(id.to_owned(), StoredFrame { x, y, z, digest });
        Ok(digest)
    }

    /// Inserts a named point set for `check.eval_caliper@1` and returns its canonical digest.
    pub fn insert_point_set(&mut self, id: &str, points: Vec<[f64; 3]>) -> Result<Digest, EvalError> {
        if !is_id(id) {
            return Err(EvalError::BadParams { reason: "bad_point_set_id" });
        }
        if points.len() > MAX_POINT_SET_LEN {
            return Err(EvalError::BadParams { reason: "too_many_points" });
        }
        if !self.point_sets.contains_key(id) && self.point_sets.len() >= MAX_ENTRIES {
            return Err(EvalError::BadParams { reason: "too_many_point_sets" });
        }
        let mut buf = Vec::with_capacity(64 + points.len() * 24);
        buf.extend_from_slice(b"fid.gdt-linear-points/1\n");
        buf.extend_from_slice(id.as_bytes());
        buf.push(b'\n');
        for p in &points {
            buf.extend_from_slice(&p[0].to_bits().to_le_bytes());
            buf.extend_from_slice(&p[1].to_bits().to_le_bytes());
            buf.extend_from_slice(&p[2].to_bits().to_le_bytes());
        }
        let digest = Digest::from_bytes(fid_canon::digest_bytes(&buf).0);
        self.point_sets.insert(id.to_owned(), StoredPointSet { points, digest });
        Ok(digest)
    }

    pub fn element(&self, id: &str) -> Option<&StoredElement> {
        self.elements.get(id)
    }

    pub fn frame(&self, id: &str) -> Option<&StoredFrame> {
        self.frames.get(id)
    }

    pub fn point_set(&self, id: &str) -> Option<&StoredPointSet> {
        self.point_sets.get(id)
    }
}

/// Errors of `check.eval_distance@1` and `check.eval_caliper@1`. [`EvalError::code`] is stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    BadParams { reason: &'static str },
    MeasurandMissing,
    MeasurandUnknown { detail: String },
    ElementMissing { element: String },
    PointSetMissing { point_set: String },
    Chain(CommandError),
    ChainNotResolved { outcome: &'static str },
    Linear(LinearError),
    Record(RecordError),
}

impl EvalError {
    pub fn code(&self) -> &'static str {
        match self {
            EvalError::BadParams { .. } => "bad_params",
            EvalError::MeasurandMissing => "measurand_missing",
            EvalError::MeasurandUnknown { .. } => "measurand_unknown",
            EvalError::ElementMissing { .. } => "element_missing",
            EvalError::PointSetMissing { .. } => "point_set_missing",
            EvalError::Chain(e) => e.code(),
            EvalError::ChainNotResolved { .. } => "chain_not_resolved",
            EvalError::Linear(e) => e.code(),
            EvalError::Record(e) => e.code(),
        }
    }

    pub fn to_value(&self) -> CValue {
        obj([("code", CValue::Str(self.code().to_owned())), ("message", CValue::Str(self.to_string()))])
    }
}

impl From<RecordError> for EvalError {
    fn from(e: RecordError) -> Self {
        EvalError::Record(e)
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::BadParams { reason } => write!(f, "bad_params{{{reason}}}"),
            EvalError::MeasurandMissing => f.write_str("measurand_missing"),
            EvalError::MeasurandUnknown { detail } => write!(f, "measurand_unknown{{{detail}}}"),
            EvalError::ElementMissing { element } => write!(f, "element_missing{{{element}}}"),
            EvalError::PointSetMissing { point_set } => write!(f, "point_set_missing{{{point_set}}}"),
            EvalError::Chain(e) => e.fmt(f),
            EvalError::ChainNotResolved { outcome } => write!(f, "chain_not_resolved{{{outcome}}}"),
            EvalError::Linear(e) => e.fmt(f),
            EvalError::Record(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for EvalError {}

/// Inputs borrowed by [`eval_distance`] and [`eval_caliper`].
pub struct EvalInputs<'a> {
    pub spec: &'a SpecState,
    pub dprof: &'a DprofState,
    pub table: Option<&'a DmtTable>,
    pub store: &'a LinearStore,
    pub register: &'a RegisterView,
}

/// Parameters of one `check.eval_distance@1` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistanceRequest<'a> {
    pub run: &'a str,
    pub char: &'a str,
    pub a: &'a str,
    pub b: &'a str,
    pub measurand: Option<DistanceMeasurandSpec>,
    pub authored_unit: LengthUnit,
    pub variant: Option<&'a str>,
}

/// Parameters of one `check.eval_caliper@1` invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct CaliperRequest<'a> {
    pub run: &'a str,
    pub char: &'a str,
    pub points: &'a str,
    pub direction: [f64; 3],
    pub authored_unit: LengthUnit,
    pub variant: Option<&'a str>,
}

/// Concrete evaluated payload for either distance or caliper.
#[derive(Debug, Clone, PartialEq)]
pub enum LinearOutcome {
    Distance { a: String, b: String, measurand: DistanceMeasurandSpec, value: DistanceValue },
    Caliper { points: String, direction: [f64; 3], value: CaliperValue },
}

/// A validated evaluation ready to commit (or inspect on `--dry-run`).
#[derive(Debug, Clone)]
pub struct Evaluated {
    pub command: &'static str,
    pub principal: Principal,
    pub branch: Branch,
    pub run: RunId,
    pub char: CharId,
    pub variant: Option<String>,
    pub basis: EvaluationBasis,
    pub outcome: LinearOutcome,
    pub input_digests: Vec<Digest>,
    pub row: String,
    pub chain_variant: String,
    pub chain_digest: Digest,
    pub dmt_digest: Digest,
    pub pin: InterpretationPin,
    pub interpretation: &'static str,
    pub label: &'static str,
    pub dry_run: bool,
    builder: ResultRecordBuilder<Interpreted>,
    event: Option<LinearEvent>,
}

impl Evaluated {
    /// The journal event to append (`None` on `--dry-run`).
    pub fn event(&self) -> Option<&LinearEvent> {
        self.event.as_ref()
    }

    /// Finishes the `ResultRecord` naming `journal_entry`.
    pub fn record(&self, journal_entry: Digest) -> Result<ResultRecord, EvalError> {
        if self.dry_run {
            return Err(EvalError::BadParams { reason: "cannot commit dry_run" });
        }
        Ok(self.builder.clone().journal_entry(journal_entry).finish()?)
    }

    /// Canonical JSON result payload (`fid.check-linear-result/1`).
    pub fn to_value(&self) -> CValue {
        let (value_si, authored_value, authored_unit, reported, algorithm, ref_seg, flags, extreme_indices) =
            match &self.outcome {
                LinearOutcome::Distance { value, .. } => (
                    value.value_si,
                    value.value_authored,
                    value.authored_unit,
                    &value.reported,
                    value.algorithm(),
                    Some(value.reference_segment),
                    flags_value(&value.flags),
                    CValue::Null,
                ),
                LinearOutcome::Caliper { value, .. } => (
                    value.value_si,
                    value.value_authored,
                    value.authored_unit,
                    &value.reported,
                    value.algorithm(),
                    None,
                    CValue::Arr(Vec::new()),
                    CValue::Arr(vec![CValue::Num(value.min_index as f64), CValue::Num(value.max_index as f64)]),
                ),
            };
        let seg_val = match ref_seg {
            Some(ReferenceSegment { start, end }) => obj([("end", vec3_hex(end)), ("start", vec3_hex(start))]),
            None => CValue::Null,
        };
        let conv = obj([
            ("factor", CValue::Str(reported.exact_conversion.factor.to_owned())),
            ("formatter", CValue::Str(reported.formatter.to_string())),
            ("from", CValue::Str(reported.exact_conversion.from.symbol().to_owned())),
            ("to", CValue::Str(reported.exact_conversion.to.symbol().to_owned())),
        ]);
        obj([
            ("algorithm", CValue::Str(algorithm.to_owned())),
            ("authored_decimal", CValue::Str(reported.value.clone())),
            ("authored_unit", CValue::Str(authored_unit.as_str().to_owned())),
            ("authored_value", CValue::Str(hex(authored_value))),
            ("branch", CValue::Str(self.branch.as_str().to_owned())),
            (
                "chain",
                obj([
                    ("chain_digest", CValue::Str(self.chain_digest.as_str().to_owned())),
                    ("dmt_digest", CValue::Str(self.dmt_digest.as_str().to_owned())),
                    ("row", CValue::Str(self.row.clone())),
                    ("variant", CValue::Str(self.chain_variant.clone())),
                ]),
            ),
            ("char", CValue::Str(self.char.as_str().to_owned())),
            ("command", CValue::Str(self.command.to_owned())),
            ("conversion", conv),
            ("dry_run", CValue::Bool(self.dry_run)),
            ("evaluation_basis", CValue::Str(self.basis.wire().to_owned())),
            ("extreme_indices", extreme_indices),
            ("flags", flags),
            ("interpretation", CValue::Str(self.interpretation.to_owned())),
            ("label", CValue::Str(self.label.to_owned())),
            ("reference_segment", seg_val),
            ("run", CValue::Str(self.run.as_str().to_owned())),
            ("schema", CValue::Str(RESULT_SCHEMA.to_owned())),
            ("unit", CValue::Str(UNIT.to_owned())),
            ("value_si", CValue::Str(hex(value_si))),
        ])
    }

    /// RFC 8785 canonical JSON bytes of [`Self::to_value`].
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, EvalError> {
        fid_canon::to_canonical_bytes(&self.to_value())
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })
    }
}

pub(crate) fn flags_value(flags: &[DistanceFlag]) -> CValue {
    CValue::Arr(
        flags
            .iter()
            .map(|f| match f {
                DistanceFlag::NonParallelPlanes { angle_rad } => obj([
                    ("angle_rad", CValue::Str(hex(*angle_rad))),
                    ("code", CValue::Str("non_parallel_planes".to_owned())),
                ]),
            })
            .collect(),
    )
}

fn chain_characteristic(char: &CharId, kind: &str, variant: Option<&str>) -> Result<CharacteristicRef, EvalError> {
    let doc = obj([
        ("char", CValue::Str(char.as_str().to_owned())),
        ("characteristic", CValue::Str(kind.to_owned())),
        ("modifiers", CValue::Arr(Vec::new())),
        ("override", variant.map_or(CValue::Null, |v| CValue::Str(v.to_owned()))),
        ("schema", CValue::Str(CHAR_SCHEMA.to_owned())),
    ]);
    let bytes =
        fid_canon::to_canonical_bytes(&doc).map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
    CharacteristicRef::parse(&bytes).map_err(EvalError::Chain)
}

/// Evaluates `check.eval_distance@1` (`REQ-001`, `REQ-002`, `REQ-004`, `REQ-005`, `REQ-006`).
pub fn eval_distance(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &DistanceRequest<'_>,
    dry_run: bool,
) -> Result<Evaluated, EvalError> {
    eval_distance_for(results, inp, principal, req, dry_run, Purpose::Evaluate)
}

pub(crate) fn eval_distance_for(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &DistanceRequest<'_>,
    dry_run: bool,
    purpose: Purpose,
) -> Result<Evaluated, EvalError> {
    // 1. Validate request identifiers and required measurand (REQ-002, AC-3).
    let run = RunId::parse(req.run).map_err(|_| EvalError::BadParams { reason: "bad_run_id" })?;
    let char = CharId::parse(req.char).ok_or(EvalError::BadParams { reason: "bad_char_id" })?;
    if !is_id(req.a) || !is_id(req.b) {
        return Err(EvalError::BadParams { reason: "bad_element_id" });
    }
    if let Some(v) = req.variant {
        if !is_id(v) {
            return Err(EvalError::BadParams { reason: "bad_variant" });
        }
    }
    let Some(meas_spec) = req.measurand.clone() else {
        return Err(EvalError::MeasurandMissing);
    };

    // 2. Governing-spec gate & DMT chain resolution.
    let char_ref = chain_characteristic(&char, "distance", req.variant)?;
    let resolved = resolve_chain(inp.spec, inp.dprof, principal, &char_ref, inp.table).map_err(EvalError::Chain)?;
    let res = match resolved {
        Resolution::Resolved(r) => r,
        other => return Err(EvalError::ChainNotResolved { outcome: other.outcome() }),
    };
    let Some(table) = inp.table else {
        return Err(EvalError::Chain(CommandError::DmtNotLoaded));
    };
    let Some(row) = table.get(res.row().as_str()) else {
        return Err(EvalError::ChainNotResolved { outcome: "UNRESOLVED" });
    };
    if row.characteristic() != "distance" || !row.modifiers().is_empty() {
        return Err(EvalError::ChainNotResolved { outcome: "UNRESOLVED" });
    }

    // 3. Look up elements and resolve measurand geometry (REQ-002).
    let el_a = inp.store.element(req.a).ok_or_else(|| EvalError::ElementMissing { element: echo(req.a) })?;
    let el_b = inp.store.element(req.b).ok_or_else(|| EvalError::ElementMissing { element: echo(req.b) })?;
    let mut input_digests = vec![el_a.digest, el_b.digest];

    let kernel_meas = match &meas_spec {
        DistanceMeasurandSpec::ThreeD => DistanceMeasurand::ThreeD,
        DistanceMeasurandSpec::Projected { plane } => {
            if !is_id(plane) {
                return Err(EvalError::MeasurandUnknown { detail: echo(plane) });
            }
            let Some(el_p) = inp.store.element(plane) else {
                return Err(EvalError::MeasurandUnknown { detail: format!("plane:{}", echo(plane)) });
            };
            let LinearElement::Plane { normal, .. } = el_p.element else {
                return Err(EvalError::MeasurandUnknown { detail: format!("not_a_plane:{}", echo(plane)) });
            };
            input_digests.push(el_p.digest);
            DistanceMeasurand::Projected { plane_normal: normal }
        }
        DistanceMeasurandSpec::Component { frame, axis } => {
            if !is_id(frame) {
                return Err(EvalError::MeasurandUnknown { detail: echo(frame) });
            }
            let Some(fr) = inp.store.frame(frame) else {
                return Err(EvalError::MeasurandUnknown { detail: format!("frame:{}", echo(frame)) });
            };
            input_digests.push(fr.digest);
            DistanceMeasurand::Component { axis_unit: fr.axis_vec(*axis) }
        }
    };

    // 4. Evaluate distance via KNP-1 kernel (`fid-gps::dim::linear`).
    let value =
        evaluate_distance(el_a.element, el_b.element, kernel_meas, req.authored_unit).map_err(EvalError::Linear)?;
    let basis = if value.from_raw_points { EvaluationBasis::Points } else { EvaluationBasis::Features };

    // 5. One record per (run, characteristic).
    match results.get(principal, &run, &char) {
        Ok(_) => return Err(EvalError::Record(RecordError::DuplicateResult)),
        Err(RecordError::NotFound) => {}
        Err(e) => return Err(EvalError::Record(e)),
    }

    // 6. Build and validate ResultRecord.
    let spec_err = |e: SpecError| EvalError::Chain(CommandError::Spec(e));
    let gs = inp.spec.require_governing_spec(principal, char.as_str(), inp.dprof).map_err(spec_err)?;
    let stored =
        inp.dprof.resolve(Branch::for_principal(principal), gs.defaults_profile()).map_err(|e| spec_err(e.into()))?;
    let measurand_text = format!("distance({}, {}, {})", req.a, req.b, meas_spec.describe());
    let measurand = Measurand::parse(&measurand_text)?;
    let builder = ResultRecordBuilder::new(run.clone(), char.clone())
        .measurand(measurand)
        .evaluation_basis(basis)
        .value(Quantity::new(value.value_si, UNIT)?)
        .uncertainty(Uncertainty::uec0(Uec0Reason::UMissing))
        .bind_decision(None, None)
        .spec(gs.clone())
        .algorithm(AlgorithmRef::parse(value.algorithm())?)
        .kernel_version(KernelVersion::parse(env!("CARGO_PKG_VERSION"))?)
        .input_digests(InputDigests::new(input_digests.clone())?)
        .resolved(&res)
        .interpretation_for(&[row], Some(stored.doc()), inp.register, purpose);
    let trial = builder.clone().journal_entry(Digest::TOMBSTONE).finish()?;
    let pin = *trial.interpretation_pin();
    let interpretation = trial.interpretation().as_str();
    let label = if crate::requires_provisional_label(&trial) { PROVISIONAL_LABEL_TEXT } else { SIGNED_OFF_LINE };

    let mut ev = Evaluated {
        command: EVAL_DISTANCE_COMMAND,
        principal,
        branch: Branch::for_principal(principal),
        run,
        char,
        variant: req.variant.map(str::to_owned),
        basis,
        outcome: LinearOutcome::Distance { a: req.a.to_owned(), b: req.b.to_owned(), measurand: meas_spec, value },
        input_digests,
        row: res.row().as_str().to_owned(),
        chain_variant: res.variant().to_owned(),
        chain_digest: res.chain_digest(),
        dmt_digest: res.dmt_digest(),
        pin,
        interpretation,
        label,
        dry_run,
        builder,
        event: None,
    };
    if !dry_run {
        ev.event = Some(LinearEvent::of(&ev));
    }
    Ok(ev)
}

/// Evaluates `check.eval_caliper@1` (`REQ-003`, `REQ-005`, `AC-4`).
pub fn eval_caliper(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &CaliperRequest<'_>,
    dry_run: bool,
) -> Result<Evaluated, EvalError> {
    eval_caliper_for(results, inp, principal, req, dry_run, Purpose::Evaluate)
}

pub(crate) fn eval_caliper_for(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &CaliperRequest<'_>,
    dry_run: bool,
    purpose: Purpose,
) -> Result<Evaluated, EvalError> {
    let run = RunId::parse(req.run).map_err(|_| EvalError::BadParams { reason: "bad_run_id" })?;
    let char = CharId::parse(req.char).ok_or(EvalError::BadParams { reason: "bad_char_id" })?;
    if !is_id(req.points) {
        return Err(EvalError::BadParams { reason: "bad_point_set_id" });
    }
    if let Some(v) = req.variant {
        if !is_id(v) {
            return Err(EvalError::BadParams { reason: "bad_variant" });
        }
    }

    let char_ref = chain_characteristic(&char, "caliper", req.variant)?;
    let resolved = resolve_chain(inp.spec, inp.dprof, principal, &char_ref, inp.table).map_err(EvalError::Chain)?;
    let res = match resolved {
        Resolution::Resolved(r) => r,
        other => return Err(EvalError::ChainNotResolved { outcome: other.outcome() }),
    };
    let Some(table) = inp.table else {
        return Err(EvalError::Chain(CommandError::DmtNotLoaded));
    };
    let Some(row) = table.get(res.row().as_str()) else {
        return Err(EvalError::ChainNotResolved { outcome: "UNRESOLVED" });
    };
    if row.characteristic() != "caliper" || !row.modifiers().is_empty() {
        return Err(EvalError::ChainNotResolved { outcome: "UNRESOLVED" });
    }

    let ps =
        inp.store.point_set(req.points).ok_or_else(|| EvalError::PointSetMissing { point_set: echo(req.points) })?;
    let input_digests = vec![ps.digest];
    let value = evaluate_caliper(&ps.points, req.direction, req.authored_unit).map_err(EvalError::Linear)?;
    let basis = EvaluationBasis::Points;

    match results.get(principal, &run, &char) {
        Ok(_) => return Err(EvalError::Record(RecordError::DuplicateResult)),
        Err(RecordError::NotFound) => {}
        Err(e) => return Err(EvalError::Record(e)),
    }

    let spec_err = |e: SpecError| EvalError::Chain(CommandError::Spec(e));
    let gs = inp.spec.require_governing_spec(principal, char.as_str(), inp.dprof).map_err(spec_err)?;
    let stored =
        inp.dprof.resolve(Branch::for_principal(principal), gs.defaults_profile()).map_err(|e| spec_err(e.into()))?;
    let measurand_text = format!("caliper({})", req.points);
    let measurand = Measurand::parse(&measurand_text)?;
    let builder = ResultRecordBuilder::new(run.clone(), char.clone())
        .measurand(measurand)
        .evaluation_basis(basis)
        .value(Quantity::new(value.value_si, UNIT)?)
        .uncertainty(Uncertainty::uec0(Uec0Reason::UMissing))
        .bind_decision(None, None)
        .spec(gs.clone())
        .algorithm(AlgorithmRef::parse(value.algorithm())?)
        .kernel_version(KernelVersion::parse(env!("CARGO_PKG_VERSION"))?)
        .input_digests(InputDigests::new(input_digests.clone())?)
        .resolved(&res)
        .interpretation_for(&[row], Some(stored.doc()), inp.register, purpose);
    let trial = builder.clone().journal_entry(Digest::TOMBSTONE).finish()?;
    let pin = *trial.interpretation_pin();
    let interpretation = trial.interpretation().as_str();
    let label = if crate::requires_provisional_label(&trial) { PROVISIONAL_LABEL_TEXT } else { SIGNED_OFF_LINE };

    let mut ev = Evaluated {
        command: EVAL_CALIPER_COMMAND,
        principal,
        branch: Branch::for_principal(principal),
        run,
        char,
        variant: req.variant.map(str::to_owned),
        basis,
        outcome: LinearOutcome::Caliper { points: req.points.to_owned(), direction: req.direction, value },
        input_digests,
        row: res.row().as_str().to_owned(),
        chain_variant: res.variant().to_owned(),
        chain_digest: res.chain_digest(),
        dmt_digest: res.dmt_digest(),
        pin,
        interpretation,
        label,
        dry_run,
        builder,
        event: None,
    };
    if !dry_run {
        ev.event = Some(LinearEvent::of(&ev));
    }
    Ok(ev)
}

/// Commits the `ResultRecord` of a non-dry-run evaluation with its journal entry hash.
pub fn commit(
    results: &mut ResultStore,
    principal: Principal,
    ev: &Evaluated,
    journal_entry: Digest,
) -> Result<Digest, EvalError> {
    if principal != ev.principal {
        return Err(EvalError::BadParams { reason: "principal_mismatch" });
    }
    let record = ev.record(journal_entry)?;
    Ok(results.commit(principal, record)?)
}
