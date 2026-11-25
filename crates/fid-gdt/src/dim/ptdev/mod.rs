//! Point deviations and feature-versus-nominal checks (spec node `GDT.DIM.PTDEV`): command
//! `check.eval_deviation@1` (W2, dry-run R1; CLI `fid check eval-deviation`, MCP
//! `check_eval_deviation`), columnar attachment above 10,000 points, journal event,
//! ResultRecord construction and exact replay (`REQ-001`..`REQ-005`, `AC-1`..`AC-5`).

pub mod command;
pub mod event;

pub use command::{deviation_command_def, invoke_cli, invoke_mcp, EVAL_DEVIATION_CLI, EVAL_DEVIATION_MCP};
pub use event::{replay, PtDevEvent, PtDevReplayed};

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
use fid_gps::dim::ptdev::{
    encode_columnar, evaluate_feature_deviation, evaluate_point_deviations, DevFeature, FeatureDeviation, NominalPoint,
    PointDeviationResult, PtDevError, Rigid, Transform, COLUMNAR_FORMAT, COLUMNAR_THRESHOLD, SIGN_CONVENTION,
};
use fid_gps::dmt::DmtTable;
use fid_gps::resolve::Resolution;
use fid_io_reg::Principal;
use fid_kunc::Uec0Reason;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Registry command ID for point/feature deviation checks (`GDT.DIM.PTDEV`).
pub const EVAL_DEVIATION_COMMAND: &str = "check.eval_deviation@1";
/// Schema of the command result JSON object.
pub const RESULT_SCHEMA: &str = "fid.check-deviation-result/1";
/// Internal record unit: SI metres (`si:m`).
pub const UNIT: &str = "si:m";
/// Maximum entries per table in [`PtDevStore`].
pub const MAX_ENTRIES: usize = 4096;
/// Maximum points in one point/nominal set.
pub const MAX_POINTS: usize = 2_000_000;
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

/// Stored named coordinate frame (`frame_to_world` rigid transform + precomputed inverse).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredFrame {
    pub frame_to_world: Rigid,
    pub world_to_frame: Transform,
    pub digest: Digest,
}

/// Stored measured point set.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredPointSet {
    pub points: Vec<[f64; 3]>,
    pub digest: Digest,
}

/// Stored nominal surface point set.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredNominalSet {
    pub nominals: Vec<NominalPoint>,
    pub digest: Digest,
}

/// Stored geometric feature (actual or nominal).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredFeature {
    pub feature: DevFeature,
    pub digest: Digest,
}

/// Read-only store of frames, point sets, nominal sets, and features for `GDT.DIM.PTDEV`.
#[derive(Debug, Clone, Default)]
pub struct PtDevStore {
    frames: BTreeMap<String, StoredFrame>,
    point_sets: BTreeMap<String, StoredPointSet>,
    nominal_sets: BTreeMap<String, StoredNominalSet>,
    features: BTreeMap<String, StoredFeature>,
}

impl PtDevStore {
    pub fn new() -> PtDevStore {
        PtDevStore::default()
    }

    /// Inserts a named rigid frame (`frame_to_world`) and returns its canonical digest.
    pub fn insert_frame(&mut self, id: &str, frame_to_world: Rigid) -> Result<Digest, EvalError> {
        if !is_id(id) {
            return Err(EvalError::BadParams { reason: "bad_frame_id" });
        }
        if !self.frames.contains_key(id) && self.frames.len() >= MAX_ENTRIES {
            return Err(EvalError::BadParams { reason: "too_many_frames" });
        }
        let world_to_frame =
            Transform::from(frame_to_world).inverse().map_err(|_| EvalError::BadParams { reason: "invalid_frame" })?;
        let r = frame_to_world.r();
        let t = frame_to_world.t();
        let canon = obj([
            ("id", CValue::Str(id.to_owned())),
            ("r0", vec3_hex(r[0])),
            ("r1", vec3_hex(r[1])),
            ("r2", vec3_hex(r[2])),
            ("t", vec3_hex(t)),
        ]);
        let bytes = fid_canon::to_canonical_bytes(&canon)
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
        let digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
        self.frames.insert(id.to_owned(), StoredFrame { frame_to_world, world_to_frame, digest });
        Ok(digest)
    }

    /// Inserts a named measured point set.
    pub fn insert_point_set(&mut self, id: &str, points: Vec<[f64; 3]>) -> Result<Digest, EvalError> {
        if !is_id(id) {
            return Err(EvalError::BadParams { reason: "bad_point_set_id" });
        }
        if points.len() > MAX_POINTS {
            return Err(EvalError::BadParams { reason: "too_many_points" });
        }
        if !self.point_sets.contains_key(id) && self.point_sets.len() >= MAX_ENTRIES {
            return Err(EvalError::BadParams { reason: "too_many_point_sets" });
        }
        let mut buf = Vec::with_capacity(64 + points.len() * 24);
        buf.extend_from_slice(b"fid.gdt-ptdev-points/1\n");
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

    /// Inserts a named nominal point-and-normal set.
    pub fn insert_nominal_set(&mut self, id: &str, nominals: Vec<NominalPoint>) -> Result<Digest, EvalError> {
        if !is_id(id) {
            return Err(EvalError::BadParams { reason: "bad_nominal_set_id" });
        }
        if nominals.len() > MAX_POINTS {
            return Err(EvalError::BadParams { reason: "too_many_nominals" });
        }
        if !self.nominal_sets.contains_key(id) && self.nominal_sets.len() >= MAX_ENTRIES {
            return Err(EvalError::BadParams { reason: "too_many_nominal_sets" });
        }
        let mut buf = Vec::with_capacity(64 + nominals.len() * 48);
        buf.extend_from_slice(b"fid.gdt-ptdev-nominals/1\n");
        buf.extend_from_slice(id.as_bytes());
        buf.push(b'\n');
        for n in &nominals {
            for c in n.point {
                buf.extend_from_slice(&c.to_bits().to_le_bytes());
            }
            for c in n.normal {
                buf.extend_from_slice(&c.to_bits().to_le_bytes());
            }
        }
        let digest = Digest::from_bytes(fid_canon::digest_bytes(&buf).0);
        self.nominal_sets.insert(id.to_owned(), StoredNominalSet { nominals, digest });
        Ok(digest)
    }

    /// Inserts a named actual or nominal feature.
    pub fn insert_feature(&mut self, id: &str, feature: DevFeature) -> Result<Digest, EvalError> {
        if !is_id(id) {
            return Err(EvalError::BadParams { reason: "bad_feature_id" });
        }
        if !self.features.contains_key(id) && self.features.len() >= MAX_ENTRIES {
            return Err(EvalError::BadParams { reason: "too_many_features" });
        }
        let canon = match feature {
            DevFeature::Point { position } => obj([
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("point".to_owned())),
                ("position", vec3_hex(position)),
            ]),
            DevFeature::Circle { center, normal, radius } => obj([
                ("center", vec3_hex(center)),
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("circle".to_owned())),
                ("normal", vec3_hex(normal)),
                ("radius", CValue::Str(hex(radius))),
            ]),
            DevFeature::Sphere { center, radius } => obj([
                ("center", vec3_hex(center)),
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("sphere".to_owned())),
                ("radius", CValue::Str(hex(radius))),
            ]),
            DevFeature::Cylinder { axis_point, axis_dir, radius } => obj([
                ("axis_dir", vec3_hex(axis_dir)),
                ("axis_point", vec3_hex(axis_point)),
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("cylinder".to_owned())),
                ("radius", CValue::Str(hex(radius))),
            ]),
            DevFeature::Plane { centroid, normal } => obj([
                ("centroid", vec3_hex(centroid)),
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("plane".to_owned())),
                ("normal", vec3_hex(normal)),
            ]),
        };
        let bytes = fid_canon::to_canonical_bytes(&canon)
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
        let digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
        self.features.insert(id.to_owned(), StoredFeature { feature, digest });
        Ok(digest)
    }

    pub fn frame(&self, id: &str) -> Option<&StoredFrame> {
        self.frames.get(id)
    }

    pub fn point_set(&self, id: &str) -> Option<&StoredPointSet> {
        self.point_sets.get(id)
    }

    pub fn nominal_set(&self, id: &str) -> Option<&StoredNominalSet> {
        self.nominal_sets.get(id)
    }

    pub fn feature(&self, id: &str) -> Option<&StoredFeature> {
        self.features.get(id)
    }
}

/// Errors of `check.eval_deviation@1`. [`EvalError::code`] is stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    BadParams { reason: &'static str },
    FrameUnknown { frame: String },
    PointSetMissing { point_set: String },
    NominalSetMissing { nominal_set: String },
    FeatureMissing { feature: String },
    Chain(CommandError),
    ChainNotResolved { outcome: &'static str },
    PtDev(PtDevError),
    Record(RecordError),
}

impl EvalError {
    pub fn code(&self) -> &'static str {
        match self {
            EvalError::BadParams { .. } => "bad_params",
            EvalError::FrameUnknown { .. } => "frame_unknown",
            EvalError::PointSetMissing { .. } => "point_set_missing",
            EvalError::NominalSetMissing { .. } => "nominal_set_missing",
            EvalError::FeatureMissing { .. } => "feature_missing",
            EvalError::Chain(e) => e.code(),
            EvalError::ChainNotResolved { .. } => "chain_not_resolved",
            EvalError::PtDev(e) => e.code(),
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
            EvalError::FrameUnknown { frame } => write!(f, "frame_unknown{{{frame}}}"),
            EvalError::PointSetMissing { point_set } => write!(f, "point_set_missing{{{point_set}}}"),
            EvalError::NominalSetMissing { nominal_set } => write!(f, "nominal_set_missing{{{nominal_set}}}"),
            EvalError::FeatureMissing { feature } => write!(f, "feature_missing{{{feature}}}"),
            EvalError::Chain(e) => e.fmt(f),
            EvalError::ChainNotResolved { outcome } => write!(f, "chain_not_resolved{{{outcome}}}"),
            EvalError::PtDev(e) => e.fmt(f),
            EvalError::Record(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for EvalError {}

/// Inputs borrowed by [`eval_deviation`].
pub struct EvalInputs<'a> {
    pub spec: &'a SpecState,
    pub dprof: &'a DprofState,
    pub table: Option<&'a DmtTable>,
    pub store: &'a PtDevStore,
    pub register: &'a RegisterView,
}

/// Target mode of `check.eval_deviation@1`: either point set vs nominals or feature vs nominal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviationTarget {
    Points { points: String, nominals: String },
    Feature { feature: String, nominal: String },
}

/// Parameters of one `check.eval_deviation@1` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviationRequest<'a> {
    pub run: &'a str,
    pub char: &'a str,
    pub target: DeviationTarget,
    pub frame: &'a str,
    pub variant: Option<&'a str>,
}

/// Columnar attachment emitted when point count exceeds [`COLUMNAR_THRESHOLD`] (`AC-4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnarAttachment {
    pub format: &'static str,
    pub count: usize,
    pub digest: Digest,
    pub bytes: Vec<u8>,
}

/// Evaluated payload for point or feature deviation mode.
#[derive(Debug, Clone, PartialEq)]
pub enum DeviationOutcome {
    /// `items_digest` is the SHA-256 of the `fid-ptdev-col/1` encoding of every per-point
    /// deviation, computed at every size so the event and the record bind the items, not only
    /// the summary. Above [`COLUMNAR_THRESHOLD`] the same bytes are the attachment.
    Points {
        result: PointDeviationResult,
        items_digest: Digest,
        attachment: Option<ColumnarAttachment>,
    },
    Feature(FeatureDeviation),
}

/// A validated evaluation ready to commit (or inspect on `--dry-run`).
#[derive(Debug, Clone)]
pub struct Evaluated {
    pub command: &'static str,
    pub principal: Principal,
    pub branch: Branch,
    pub run: RunId,
    pub char: CharId,
    pub frame: String,
    pub target: DeviationTarget,
    pub variant: Option<String>,
    pub basis: EvaluationBasis,
    pub outcome: DeviationOutcome,
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
    event: Option<PtDevEvent>,
}

impl Evaluated {
    /// The journal event to append (`None` on `--dry-run`).
    pub fn event(&self) -> Option<&PtDevEvent> {
        self.event.as_ref()
    }

    /// The columnar attachment if point count exceeded [`COLUMNAR_THRESHOLD`] (`AC-4`).
    pub fn attachment(&self) -> Option<&ColumnarAttachment> {
        match &self.outcome {
            DeviationOutcome::Points { attachment, .. } => attachment.as_ref(),
            DeviationOutcome::Feature(_) => None,
        }
    }

    /// Finishes the `ResultRecord` naming `journal_entry`.
    pub fn record(&self, journal_entry: Digest) -> Result<ResultRecord, EvalError> {
        if self.dry_run {
            return Err(EvalError::BadParams { reason: "cannot commit dry_run" });
        }
        Ok(self.builder.clone().journal_entry(journal_entry).finish()?)
    }

    /// Canonical JSON result payload (`fid.check-deviation-result/1`).
    pub fn to_value(&self) -> CValue {
        let (mode, value_si, items, attachment, feature_val, summary) = match &self.outcome {
            DeviationOutcome::Points { result, items_digest, attachment } => {
                let (items_val, att_val) = match attachment {
                    Some(att) => (
                        CValue::Null,
                        obj([
                            ("count", CValue::Num(att.count as f64)),
                            ("digest", CValue::Str(att.digest.as_str().to_owned())),
                            ("format", CValue::Str(att.format.to_owned())),
                        ]),
                    ),
                    None => (
                        CValue::Arr(
                            result
                                .deviations
                                .iter()
                                .map(|d| {
                                    obj([
                                        ("dn", CValue::Str(hex(d.dn))),
                                        ("dx", CValue::Str(hex(d.dx))),
                                        ("dy", CValue::Str(hex(d.dy))),
                                        ("dz", CValue::Str(hex(d.dz))),
                                    ])
                                })
                                .collect(),
                        ),
                        CValue::Null,
                    ),
                };
                let sum = obj([
                    ("count", CValue::Num(result.deviations.len() as f64)),
                    ("items_digest", CValue::Str(items_digest.as_str().to_owned())),
                    ("max_abs_dn_si", CValue::Str(hex(result.max_abs_dn_si))),
                    ("max_dn_si", CValue::Str(hex(result.max_dn_si))),
                    ("min_dn_si", CValue::Str(hex(result.min_dn_si))),
                ]);
                ("points", result.max_abs_dn_si, items_val, att_val, CValue::Null, sum)
            }
            DeviationOutcome::Feature(fd) => {
                let fv = obj([
                    ("direction_rad", fd.direction_rad.map_or(CValue::Null, |a| CValue::Str(hex(a)))),
                    ("feature_kind", CValue::Str(fd.feature_kind.to_owned())),
                    ("location", vec3_hex(fd.location)),
                    ("size", fd.size.map_or(CValue::Null, |s| CValue::Str(hex(s)))),
                ]);
                ("feature", fd.location_norm_si, CValue::Null, CValue::Null, fv, CValue::Null)
            }
        };
        obj([
            ("algorithm", CValue::Str(fid_gps::dim::ptdev::PTDEV_ALGORITHM_ID.to_owned())),
            ("attachment", attachment),
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
            ("dry_run", CValue::Bool(self.dry_run)),
            ("evaluation_basis", CValue::Str(self.basis.wire().to_owned())),
            ("feature", feature_val),
            ("frame", CValue::Str(self.frame.clone())),
            ("interpretation", CValue::Str(self.interpretation.to_owned())),
            ("items", items),
            ("label", CValue::Str(self.label.to_owned())),
            ("mode", CValue::Str(mode.to_owned())),
            ("run", CValue::Str(self.run.as_str().to_owned())),
            ("schema", CValue::Str(RESULT_SCHEMA.to_owned())),
            ("sign_convention", CValue::Str(SIGN_CONVENTION.to_owned())),
            ("summary", summary),
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

fn chain_characteristic(char: &CharId, variant: Option<&str>) -> Result<CharacteristicRef, EvalError> {
    let doc = obj([
        ("char", CValue::Str(char.as_str().to_owned())),
        ("characteristic", CValue::Str("deviation".to_owned())),
        ("modifiers", CValue::Arr(Vec::new())),
        ("override", variant.map_or(CValue::Null, |v| CValue::Str(v.to_owned()))),
        ("schema", CValue::Str(CHAR_SCHEMA.to_owned())),
    ]);
    let bytes =
        fid_canon::to_canonical_bytes(&doc).map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
    CharacteristicRef::parse(&bytes).map_err(EvalError::Chain)
}

/// Evaluates `check.eval_deviation@1` (`REQ-001`..`REQ-005`).
pub fn eval_deviation(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &DeviationRequest<'_>,
    dry_run: bool,
) -> Result<Evaluated, EvalError> {
    eval_deviation_for(results, inp, principal, req, dry_run, Purpose::Evaluate)
}

pub(crate) fn eval_deviation_for(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &DeviationRequest<'_>,
    dry_run: bool,
    purpose: Purpose,
) -> Result<Evaluated, EvalError> {
    let run = RunId::parse(req.run).map_err(|_| EvalError::BadParams { reason: "bad_run_id" })?;
    let char = CharId::parse(req.char).ok_or(EvalError::BadParams { reason: "bad_char_id" })?;
    if !is_id(req.frame) {
        return Err(EvalError::FrameUnknown { frame: echo(req.frame) });
    }
    if let Some(v) = req.variant {
        if !is_id(v) {
            return Err(EvalError::BadParams { reason: "bad_variant" });
        }
    }

    // 1. Governing-spec gate and DMT chain resolution.
    let char_ref = chain_characteristic(&char, req.variant)?;
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
    if row.characteristic() != "deviation" || !row.modifiers().is_empty() {
        return Err(EvalError::ChainNotResolved { outcome: "UNRESOLVED" });
    }

    // 2. Look up frame.
    let frame = inp.store.frame(req.frame).ok_or_else(|| EvalError::FrameUnknown { frame: echo(req.frame) })?;
    let mut input_digests = vec![frame.digest];

    // 3. Evaluate point or feature deviations.
    let (outcome, basis, value_si, measurand_text) = match &req.target {
        DeviationTarget::Points { points, nominals } => {
            if !is_id(points) || !is_id(nominals) {
                return Err(EvalError::BadParams { reason: "bad_target_id" });
            }
            let ps =
                inp.store.point_set(points).ok_or_else(|| EvalError::PointSetMissing { point_set: echo(points) })?;
            let ns = inp
                .store
                .nominal_set(nominals)
                .ok_or_else(|| EvalError::NominalSetMissing { nominal_set: echo(nominals) })?;
            input_digests.push(ps.digest);
            input_digests.push(ns.digest);
            let result =
                evaluate_point_deviations(&ps.points, &ns.nominals, &frame.world_to_frame).map_err(EvalError::PtDev)?;
            // The items digest binds every dx, dy, dz, dn at every size (event and record inputs);
            // above the threshold the same bytes become the columnar attachment (AC-4).
            let bytes = encode_columnar(&result.deviations);
            let items_digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
            input_digests.push(items_digest);
            let attachment = if result.deviations.len() > COLUMNAR_THRESHOLD {
                Some(ColumnarAttachment {
                    format: COLUMNAR_FORMAT,
                    count: result.deviations.len(),
                    digest: items_digest,
                    bytes,
                })
            } else {
                None
            };
            let v = result.max_abs_dn_si;
            (
                DeviationOutcome::Points { result, items_digest, attachment },
                EvaluationBasis::Points,
                v,
                format!("deviation(points={points}, nominals={nominals}, frame={})", req.frame),
            )
        }
        DeviationTarget::Feature { feature, nominal } => {
            if !is_id(feature) || !is_id(nominal) {
                return Err(EvalError::BadParams { reason: "bad_target_id" });
            }
            let fa = inp.store.feature(feature).ok_or_else(|| EvalError::FeatureMissing { feature: echo(feature) })?;
            let fnm = inp.store.feature(nominal).ok_or_else(|| EvalError::FeatureMissing { feature: echo(nominal) })?;
            input_digests.push(fa.digest);
            input_digests.push(fnm.digest);
            let fd = evaluate_feature_deviation(&fa.feature, &fnm.feature, &frame.world_to_frame)
                .map_err(EvalError::PtDev)?;
            let v = fd.location_norm_si;
            (
                DeviationOutcome::Feature(fd),
                EvaluationBasis::Features,
                v,
                format!("deviation(feature={feature}, nominal={nominal}, frame={})", req.frame),
            )
        }
    };

    // 4. Enforce one record per (run, characteristic).
    match results.get(principal, &run, &char) {
        Ok(_) => return Err(EvalError::Record(RecordError::DuplicateResult)),
        Err(RecordError::NotFound) => {}
        Err(e) => return Err(EvalError::Record(e)),
    }

    // 5. Build and validate ResultRecord.
    let spec_err = |e: SpecError| EvalError::Chain(CommandError::Spec(e));
    let gs = inp.spec.require_governing_spec(principal, char.as_str(), inp.dprof).map_err(spec_err)?;
    let stored =
        inp.dprof.resolve(Branch::for_principal(principal), gs.defaults_profile()).map_err(|e| spec_err(e.into()))?;
    let measurand = Measurand::parse(&measurand_text)?;
    let builder = ResultRecordBuilder::new(run.clone(), char.clone())
        .measurand(measurand)
        .evaluation_basis(basis)
        .value(Quantity::new(value_si, UNIT)?)
        .uncertainty(Uncertainty::uec0(Uec0Reason::UMissing))
        .bind_decision(None, None)
        .spec(gs.clone())
        .algorithm(AlgorithmRef::parse(fid_gps::dim::ptdev::PTDEV_ALGORITHM_ID)?)
        .kernel_version(KernelVersion::parse(env!("CARGO_PKG_VERSION"))?)
        .input_digests(InputDigests::new(input_digests.clone())?)
        .resolved(&res)
        .interpretation_for(&[row], Some(stored.doc()), inp.register, purpose);
    let trial = builder.clone().journal_entry(Digest::TOMBSTONE).finish()?;
    let pin = *trial.interpretation_pin();
    let interpretation = trial.interpretation().as_str();
    let label = if crate::requires_provisional_label(&trial) { PROVISIONAL_LABEL_TEXT } else { SIGNED_OFF_LINE };

    let mut ev = Evaluated {
        command: EVAL_DEVIATION_COMMAND,
        principal,
        branch: Branch::for_principal(principal),
        run,
        char,
        frame: req.frame.to_owned(),
        target: req.target.clone(),
        variant: req.variant.map(str::to_owned),
        basis,
        outcome,
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
        ev.event = Some(PtDevEvent::of(&ev));
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
