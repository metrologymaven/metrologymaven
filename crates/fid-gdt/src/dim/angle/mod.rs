//! Angle checks between directed elements or an element and a frame axis, with optional
//! projection onto a named plane (spec node `GDT.DIM.ANGLE`): command `check.eval_angle@1`
//! (W2, dry-run R1; CLI `fid check eval-angle`, MCP `check_eval_angle`), journal event,
//! ResultRecord construction and exact replay (`REQ-001`..`REQ-005`, `AC-1`..`AC-5`).

pub mod command;
pub mod event;

pub use command::{angle_command_def, invoke_cli, invoke_mcp, EVAL_ANGLE_CLI, EVAL_ANGLE_MCP};
pub use event::{replay, AngleEvent, AngleReplayed};

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
use fid_gps::dim::angle::{
    evaluate_angle, AngleError, AngleInput, AngleMode, AngleValue, AuthoredUnit, ElementDirectionKind, FrameAxis,
};
use fid_gps::dmt::DmtTable;
use fid_gps::resolve::Resolution;
use fid_io_reg::Principal;
use fid_kunc::Uec0Reason;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Registry command ID of the angle check (`GDT.DIM.ANGLE`).
pub const EVAL_ANGLE_COMMAND: &str = "check.eval_angle@1";
/// Schema of the command result JSON object.
pub const RESULT_SCHEMA: &str = "fid.check-angle-result/1";
/// Internal record unit: radians (`si:rad`).
pub const UNIT: &str = "si:rad";
/// Maximum elements stored in one [`AngleElements`] lookup.
pub const MAX_ELEMENTS: usize = 4096;
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

pub(crate) fn obj<const N: usize>(items: [(&str, CValue); N]) -> CValue {
    CValue::Obj(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// Operand `b` of `check.eval_angle@1`: either a named element or a coordinate-frame axis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetB {
    Element(String),
    Axis(FrameAxis),
}

/// One element entry in [`AngleElements`].
#[derive(Debug, Clone, PartialEq)]
pub struct StoredElement {
    pub kind: ElementDirectionKind,
    pub direction: Option<[f64; 3]>,
    pub digest: Digest,
}

/// Read-only store of named geometric elements (directed elements and projection planes) used
/// by `check.eval_angle@1`.
#[derive(Debug, Clone, Default)]
pub struct AngleElements {
    elements: BTreeMap<String, StoredElement>,
}

impl AngleElements {
    pub fn new() -> AngleElements {
        AngleElements::default()
    }

    /// Inserts a directed element (line, axis, plane normal, or cone axis) with its direction
    /// vector and computes its canonical SHA-256 input digest.
    pub fn insert_directed(
        &mut self,
        id: &str,
        kind: ElementDirectionKind,
        direction: [f64; 3],
    ) -> Result<Digest, EvalError> {
        if !is_id(id) {
            return Err(EvalError::BadParams { reason: "bad_element_id" });
        }
        if !kind.is_directed() {
            return Err(EvalError::BadParams { reason: "expected_directed_kind" });
        }
        if !self.elements.contains_key(id) && self.elements.len() >= MAX_ELEMENTS {
            return Err(EvalError::BadParams { reason: "too_many_elements" });
        }
        let canon = obj([
            (
                "direction",
                CValue::Arr(vec![
                    CValue::Str(hex(direction[0])),
                    CValue::Str(hex(direction[1])),
                    CValue::Str(hex(direction[2])),
                ]),
            ),
            ("id", CValue::Str(id.to_owned())),
            ("kind", CValue::Str(kind.as_str().to_owned())),
        ]);
        let bytes = fid_canon::to_canonical_bytes(&canon)
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
        let digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
        self.elements.insert(id.to_owned(), StoredElement { kind, direction: Some(direction), digest });
        Ok(digest)
    }

    /// Inserts a non-directed element (e.g. a point or sphere) so calling `eval-angle` on it
    /// yields `not_directed_element`.
    pub fn insert_undirected(&mut self, id: &str) -> Result<Digest, EvalError> {
        if !is_id(id) {
            return Err(EvalError::BadParams { reason: "bad_element_id" });
        }
        if !self.elements.contains_key(id) && self.elements.len() >= MAX_ELEMENTS {
            return Err(EvalError::BadParams { reason: "too_many_elements" });
        }
        let canon = obj([
            ("id", CValue::Str(id.to_owned())),
            ("kind", CValue::Str(ElementDirectionKind::Undirected.as_str().to_owned())),
        ]);
        let bytes = fid_canon::to_canonical_bytes(&canon)
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
        let digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
        self.elements
            .insert(id.to_owned(), StoredElement { kind: ElementDirectionKind::Undirected, direction: None, digest });
        Ok(digest)
    }

    /// Looks up an element by ID.
    pub fn get(&self, id: &str) -> Option<&StoredElement> {
        self.elements.get(id)
    }
}

/// Errors of `check.eval_angle@1`. [`EvalError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    BadParams { reason: &'static str },
    ElementMissing { element: String },
    Chain(CommandError),
    ChainNotResolved { outcome: &'static str },
    Angle(AngleError),
    Record(RecordError),
}

impl EvalError {
    pub fn code(&self) -> &'static str {
        match self {
            EvalError::BadParams { .. } => "bad_params",
            EvalError::ElementMissing { .. } => "element_missing",
            EvalError::Chain(e) => e.code(),
            EvalError::ChainNotResolved { .. } => "chain_not_resolved",
            EvalError::Angle(e) => e.code(),
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
            EvalError::ElementMissing { element } => write!(f, "element_missing{{{element}}}"),
            EvalError::Chain(e) => e.fmt(f),
            EvalError::ChainNotResolved { outcome } => write!(f, "chain_not_resolved{{{outcome}}}"),
            EvalError::Angle(e) => e.fmt(f),
            EvalError::Record(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for EvalError {}

/// Inputs borrowed by [`eval_angle`].
pub struct EvalInputs<'a> {
    pub spec: &'a SpecState,
    pub dprof: &'a DprofState,
    pub table: Option<&'a DmtTable>,
    pub elements: &'a AngleElements,
    pub register: &'a RegisterView,
}

/// Parameters of one `check.eval_angle@1` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalRequest<'a> {
    pub run: &'a str,
    pub char: &'a str,
    pub a: &'a str,
    pub b: TargetB,
    pub mode: AngleMode,
    pub plane: Option<&'a str>,
    pub authored_unit: AuthoredUnit,
    pub variant: Option<&'a str>,
}

/// A validated evaluation ready to commit (or inspect on `--dry-run`).
#[derive(Debug, Clone)]
pub struct Evaluated {
    pub command: &'static str,
    pub principal: Principal,
    pub branch: Branch,
    pub run: RunId,
    pub char: CharId,
    pub a: String,
    pub b: TargetB,
    pub mode: AngleMode,
    pub plane: Option<String>,
    pub variant: Option<String>,
    pub value: AngleValue,
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
    event: Option<AngleEvent>,
}

impl Evaluated {
    /// The journal event to append (`None` on `--dry-run`).
    pub fn event(&self) -> Option<&AngleEvent> {
        self.event.as_ref()
    }

    /// Finishes the `ResultRecord` naming `journal_entry`.
    pub fn record(&self, journal_entry: Digest) -> Result<ResultRecord, EvalError> {
        if self.dry_run {
            return Err(EvalError::BadParams { reason: "cannot commit dry_run" });
        }
        Ok(self.builder.clone().journal_entry(journal_entry).finish()?)
    }

    /// Canonical JSON result payload (`fid.check-angle-result/1`).
    pub fn to_value(&self) -> CValue {
        let b_val = match &self.b {
            TargetB::Element(id) => obj([("element", CValue::Str(id.clone()))]),
            TargetB::Axis(ax) => obj([("axis", CValue::Str(ax.as_str().to_owned()))]),
        };
        obj([
            ("a", CValue::Str(self.a.clone())),
            ("algorithm", CValue::Str(self.value.algorithm().to_owned())),
            ("authored_unit", CValue::Str(self.value.authored_unit.as_str().to_owned())),
            ("authored_value", CValue::Str(hex(self.value.authored_value))),
            ("b", b_val),
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
            ("interpretation", CValue::Str(self.interpretation.to_owned())),
            ("label", CValue::Str(self.label.to_owned())),
            ("mode", CValue::Str(self.mode.as_str().to_owned())),
            ("plane", self.plane.as_ref().map_or(CValue::Null, |p| CValue::Str(p.clone()))),
            ("projected", CValue::Bool(self.value.projected)),
            ("run", CValue::Str(self.run.as_str().to_owned())),
            ("schema", CValue::Str(RESULT_SCHEMA.to_owned())),
            ("unit", CValue::Str(UNIT.to_owned())),
            ("value_rad", CValue::Str(hex(self.value.value_rad))),
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
        ("characteristic", CValue::Str("angle".to_owned())),
        ("modifiers", CValue::Arr(Vec::new())),
        ("override", variant.map_or(CValue::Null, |v| CValue::Str(v.to_owned()))),
        ("schema", CValue::Str(CHAR_SCHEMA.to_owned())),
    ]);
    let bytes =
        fid_canon::to_canonical_bytes(&doc).map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
    CharacteristicRef::parse(&bytes).map_err(EvalError::Chain)
}

/// Evaluates `check.eval_angle@1` (`REQ-001`..`REQ-005`).
pub fn eval_angle(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &EvalRequest<'_>,
    dry_run: bool,
) -> Result<Evaluated, EvalError> {
    eval_for(store, inp, principal, req, dry_run, Purpose::Evaluate)
}

pub(crate) fn eval_for(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &EvalRequest<'_>,
    dry_run: bool,
    purpose: Purpose,
) -> Result<Evaluated, EvalError> {
    // 1. Validate request IDs.
    let run = RunId::parse(req.run).map_err(|_| EvalError::BadParams { reason: "bad_run_id" })?;
    let char = CharId::parse(req.char).ok_or(EvalError::BadParams { reason: "bad_char_id" })?;
    if !is_id(req.a) {
        return Err(EvalError::BadParams { reason: "bad_element_id" });
    }
    if let TargetB::Element(b_id) = &req.b {
        if !is_id(b_id) {
            return Err(EvalError::BadParams { reason: "bad_element_id" });
        }
    }
    if let Some(p) = req.plane {
        if !is_id(p) {
            return Err(EvalError::BadParams { reason: "bad_plane_id" });
        }
    }
    if let Some(v) = req.variant {
        if !is_id(v) {
            return Err(EvalError::BadParams { reason: "bad_variant" });
        }
    }

    // 2. Gate on governing spec and resolve DMT operator chain for `angle`.
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
    if row.characteristic() != "angle" || !row.modifiers().is_empty() {
        return Err(EvalError::ChainNotResolved { outcome: "UNRESOLVED" });
    }

    // 3. Look up directed elements and optional projection plane.
    let el_a = inp.elements.get(req.a).ok_or_else(|| EvalError::ElementMissing { element: echo(req.a) })?;
    let dir_a = match (el_a.kind.is_directed(), el_a.direction) {
        (true, Some(d)) => d,
        _ => return Err(EvalError::Angle(AngleError::NotDirectedElement { element: echo(req.a) })),
    };
    let mut input_digests = vec![el_a.digest];

    let dir_b = match &req.b {
        TargetB::Axis(ax) => ax.unit_vec(),
        TargetB::Element(b_id) => {
            let el_b = inp.elements.get(b_id).ok_or_else(|| EvalError::ElementMissing { element: echo(b_id) })?;
            let d = match (el_b.kind.is_directed(), el_b.direction) {
                (true, Some(d)) => d,
                _ => return Err(EvalError::Angle(AngleError::NotDirectedElement { element: echo(b_id) })),
            };
            input_digests.push(el_b.digest);
            d
        }
    };

    let plane_normal = match req.plane {
        None => None,
        Some(p_id) => {
            let el_p = inp.elements.get(p_id).ok_or_else(|| EvalError::ElementMissing { element: echo(p_id) })?;
            let n = match (el_p.kind, el_p.direction) {
                (ElementDirectionKind::PlaneNormal, Some(n)) => n,
                _ => return Err(EvalError::Angle(AngleError::NotDirectedElement { element: echo(p_id) })),
            };
            input_digests.push(el_p.digest);
            Some(n)
        }
    };

    // 4. Evaluate angle via KNP-1 kernel (`fid-gps::dim::angle`).
    let angle_input = AngleInput { dir_a, dir_b, mode: req.mode, plane_normal, authored_unit: req.authored_unit };
    let value = evaluate_angle(&angle_input).map_err(EvalError::Angle)?;

    // 5. Enforce one record per (run, characteristic).
    match store.get(principal, &run, &char) {
        Ok(_) => return Err(EvalError::Record(RecordError::DuplicateResult)),
        Err(RecordError::NotFound) => {}
        Err(e) => return Err(EvalError::Record(e)),
    }

    // 6. Build and validate ResultRecord before emitting anything.
    let spec_err = |e: SpecError| EvalError::Chain(CommandError::Spec(e));
    let gs = inp.spec.require_governing_spec(principal, char.as_str(), inp.dprof).map_err(spec_err)?;
    let stored =
        inp.dprof.resolve(Branch::for_principal(principal), gs.defaults_profile()).map_err(|e| spec_err(e.into()))?;
    let b_desc = match &req.b {
        TargetB::Element(id) => id.as_str(),
        TargetB::Axis(ax) => ax.as_str(),
    };
    let measurand_text = match req.plane {
        Some(p) => format!("angle({}, {}, plane={p})", req.a, b_desc),
        None => format!("angle({}, {})", req.a, b_desc),
    };
    let measurand = Measurand::parse(&measurand_text)?;
    let builder = ResultRecordBuilder::new(run.clone(), char.clone())
        .measurand(measurand)
        .evaluation_basis(EvaluationBasis::Points)
        .value(Quantity::new(value.value_rad, UNIT)?)
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
        command: EVAL_ANGLE_COMMAND,
        principal,
        branch: Branch::for_principal(principal),
        run,
        char,
        a: req.a.to_owned(),
        b: req.b.clone(),
        mode: req.mode,
        plane: req.plane.map(str::to_owned),
        variant: req.variant.map(str::to_owned),
        value,
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
        ev.event = Some(AngleEvent::of(&ev));
    }
    Ok(ev)
}

/// Commits the `ResultRecord` of a non-dry-run evaluation with its journal entry hash.
pub fn commit(
    store: &mut ResultStore,
    principal: Principal,
    ev: &Evaluated,
    journal_entry: Digest,
) -> Result<Digest, EvalError> {
    if principal != ev.principal {
        return Err(EvalError::BadParams { reason: "principal_mismatch" });
    }
    let record = ev.record(journal_entry)?;
    Ok(store.commit(principal, record)?)
}
