//! Scalar property and min/max checks (spec node `GDT.DIM.SCALAR`): commands
//! `check.eval_scalar@1` and `check.eval_minmax@1` (W2, dry-run R1; CLI `fid check eval-scalar`,
//! `fid check eval-minmax`; MCP `check_eval_scalar`, `check_eval_minmax`), journal events,
//! ResultRecord construction and exact replay (`REQ-001`..`REQ-005`, `AC-1`..`AC-6`).

pub mod command;
pub mod event;

pub use command::{
    invoke_cli, invoke_mcp, minmax_command_def, scalar_command_def, EVAL_MINMAX_CLI, EVAL_MINMAX_MCP, EVAL_SCALAR_CLI,
    EVAL_SCALAR_MCP,
};
pub use event::{replay, ScalarEvent, ScalarReplayed};

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
use fid_gps::dim::scalar::{
    evaluate_minmax, evaluate_scalar, MinMaxStat, MinMaxValue, QuantityKind, ScalarElement, ScalarError, ScalarValue,
};
use fid_gps::dmt::DmtTable;
use fid_gps::resolve::Resolution;
use fid_io_reg::Principal;
use fid_kunc::Uec0Reason;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Registry command ID for scalar property checks (`GDT.DIM.SCALAR`).
pub const EVAL_SCALAR_COMMAND: &str = "check.eval_scalar@1";
/// Registry command ID for min/max/range checks (`GDT.DIM.SCALAR`).
pub const EVAL_MINMAX_COMMAND: &str = "check.eval_minmax@1";
/// Schema of the command result JSON object.
pub const RESULT_SCHEMA: &str = "fid.check-scalar-result/1";
/// Maximum elements or value sets stored in one [`ScalarStore`].
pub const MAX_ENTRIES: usize = 4096;
/// Maximum values in a single value set (`GDT.DIM.SCALAR`).
pub const MAX_VALUES: usize = 10_000_000;
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

/// Stored element with its canonical digest.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredElement {
    pub element: ScalarElement,
    pub digest: Digest,
}

/// Stored value set for `check.eval_minmax@1` with quantity kind and canonical digest.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredValueSet {
    pub quantity_kind: QuantityKind,
    pub values: Vec<f64>,
    pub digest: Digest,
}

/// Read-only store of named elements and value sets for `GDT.DIM.SCALAR`.
#[derive(Debug, Clone, Default)]
pub struct ScalarStore {
    elements: BTreeMap<String, StoredElement>,
    value_sets: BTreeMap<String, StoredValueSet>,
}

impl ScalarStore {
    pub fn new() -> ScalarStore {
        ScalarStore::default()
    }

    /// Inserts a named element and computes its canonical SHA-256 digest.
    pub fn insert_element(&mut self, id: &str, element: ScalarElement) -> Result<Digest, EvalError> {
        if !is_id(id) {
            return Err(EvalError::BadParams { reason: "bad_element_id" });
        }
        if !self.elements.contains_key(id) && self.elements.len() >= MAX_ENTRIES {
            return Err(EvalError::BadParams { reason: "too_many_elements" });
        }
        let canon = match element {
            ScalarElement::Circle { diameter, radius } => obj([
                ("diameter", CValue::Str(hex(diameter))),
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("circle".to_owned())),
                ("radius", CValue::Str(hex(radius))),
            ]),
            ScalarElement::Cone { apex_angle_rad, half_angle_rad, length } => obj([
                ("apex_angle_rad", CValue::Str(hex(apex_angle_rad))),
                ("half_angle_rad", CValue::Str(hex(half_angle_rad))),
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("cone".to_owned())),
                ("length", CValue::Str(hex(length))),
            ]),
            ScalarElement::Cylinder { diameter, length, radius } => obj([
                ("diameter", CValue::Str(hex(diameter))),
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("cylinder".to_owned())),
                ("length", CValue::Str(hex(length))),
                ("radius", CValue::Str(hex(radius))),
            ]),
            ScalarElement::Line { length } => obj([
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("line".to_owned())),
                ("length", CValue::Str(hex(length))),
            ]),
            ScalarElement::Plane { area } => obj([
                ("area", CValue::Str(hex(area))),
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("plane".to_owned())),
            ]),
            ScalarElement::Point { x, y, z } => obj([
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("point".to_owned())),
                ("x", CValue::Str(hex(x))),
                ("y", CValue::Str(hex(y))),
                ("z", CValue::Str(hex(z))),
            ]),
            ScalarElement::Slot { length, width } => obj([
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("slot".to_owned())),
                ("length", CValue::Str(hex(length))),
                ("width", CValue::Str(hex(width))),
            ]),
            ScalarElement::Sphere { diameter, radius } => obj([
                ("diameter", CValue::Str(hex(diameter))),
                ("id", CValue::Str(id.to_owned())),
                ("kind", CValue::Str("sphere".to_owned())),
                ("radius", CValue::Str(hex(radius))),
            ]),
        };
        let bytes = fid_canon::to_canonical_bytes(&canon)
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
        let digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
        self.elements.insert(id.to_owned(), StoredElement { element, digest });
        Ok(digest)
    }

    /// Inserts a named value set and computes its canonical SHA-256 digest.
    pub fn insert_value_set(
        &mut self,
        id: &str,
        quantity_kind: QuantityKind,
        values: Vec<f64>,
    ) -> Result<Digest, EvalError> {
        if !is_id(id) {
            return Err(EvalError::BadParams { reason: "bad_value_set_id" });
        }
        if values.len() > MAX_VALUES {
            return Err(EvalError::BadParams { reason: "too_many_values" });
        }
        if !self.value_sets.contains_key(id) && self.value_sets.len() >= MAX_ENTRIES {
            return Err(EvalError::BadParams { reason: "too_many_value_sets" });
        }
        let mut buf = Vec::with_capacity(64 + values.len() * 8);
        buf.extend_from_slice(b"fid.gdt-scalar-values/1\n");
        buf.extend_from_slice(id.as_bytes());
        buf.push(b'\n');
        buf.extend_from_slice(quantity_kind.as_str().as_bytes());
        buf.push(b'\n');
        for v in &values {
            buf.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        let digest = Digest::from_bytes(fid_canon::digest_bytes(&buf).0);
        self.value_sets.insert(id.to_owned(), StoredValueSet { quantity_kind, values, digest });
        Ok(digest)
    }

    pub fn element(&self, id: &str) -> Option<&StoredElement> {
        self.elements.get(id)
    }

    pub fn value_set(&self, id: &str) -> Option<&StoredValueSet> {
        self.value_sets.get(id)
    }
}

/// Errors of `check.eval_scalar@1` and `check.eval_minmax@1`. [`EvalError::code`] is stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    BadParams { reason: &'static str },
    ElementMissing { element: String },
    ValueSetMissing { value_set: String },
    Chain(CommandError),
    ChainNotResolved { outcome: &'static str },
    Scalar(ScalarError),
    Record(RecordError),
}

impl EvalError {
    pub fn code(&self) -> &'static str {
        match self {
            EvalError::BadParams { .. } => "bad_params",
            EvalError::ElementMissing { .. } => "element_missing",
            EvalError::ValueSetMissing { .. } => "value_set_missing",
            EvalError::Chain(e) => e.code(),
            EvalError::ChainNotResolved { .. } => "chain_not_resolved",
            EvalError::Scalar(e) => e.code(),
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
            EvalError::ValueSetMissing { value_set } => write!(f, "value_set_missing{{{value_set}}}"),
            EvalError::Chain(e) => e.fmt(f),
            EvalError::ChainNotResolved { outcome } => write!(f, "chain_not_resolved{{{outcome}}}"),
            EvalError::Scalar(e) => e.fmt(f),
            EvalError::Record(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for EvalError {}

/// Inputs borrowed by [`eval_scalar`] and [`eval_minmax`].
pub struct EvalInputs<'a> {
    pub spec: &'a SpecState,
    pub dprof: &'a DprofState,
    pub table: Option<&'a DmtTable>,
    pub store: &'a ScalarStore,
    pub register: &'a RegisterView,
}

/// Parameters of one `check.eval_scalar@1` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarRequest<'a> {
    pub run: &'a str,
    pub char: &'a str,
    pub element: &'a str,
    pub property: &'a str,
    pub tolerance_unit: Option<&'a str>,
    pub variant: Option<&'a str>,
}

/// Parameters of one `check.eval_minmax@1` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinMaxRequest<'a> {
    pub run: &'a str,
    pub char: &'a str,
    pub values: &'a str,
    pub statistic: MinMaxStat,
    pub variant: Option<&'a str>,
}

/// Evaluated payload for either scalar property or min/max/range check.
#[derive(Debug, Clone, PartialEq)]
pub enum ScalarOutcome {
    Scalar { element: String, property: String, tolerance_unit: Option<String>, value: ScalarValue },
    MinMax { values: String, value: MinMaxValue },
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
    pub outcome: ScalarOutcome,
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
    event: Option<ScalarEvent>,
}

impl Evaluated {
    /// The journal event to append (`None` on `--dry-run`).
    pub fn event(&self) -> Option<&ScalarEvent> {
        self.event.as_ref()
    }

    /// Finishes the `ResultRecord` naming `journal_entry`.
    pub fn record(&self, journal_entry: Digest) -> Result<ResultRecord, EvalError> {
        if self.dry_run {
            return Err(EvalError::BadParams { reason: "cannot commit dry_run" });
        }
        Ok(self.builder.clone().journal_entry(journal_entry).finish()?)
    }

    /// Canonical JSON result payload (`fid.check-scalar-result/1`).
    pub fn to_value(&self) -> CValue {
        let (value_si, si_unit, qkind, algorithm, attained_at, summary) = match &self.outcome {
            ScalarOutcome::Scalar { value, .. } => {
                (value.value_si, value.si_unit, value.quantity_kind, value.algorithm(), CValue::Null, CValue::Null)
            }
            ScalarOutcome::MinMax { value, .. } => {
                let att = match value.statistic {
                    MinMaxStat::Min => obj([("index", CValue::Num(value.min_index as f64))]),
                    MinMaxStat::Max => obj([("index", CValue::Num(value.max_index as f64))]),
                    MinMaxStat::Range => obj([
                        ("max_index", CValue::Num(value.max_index as f64)),
                        ("min_index", CValue::Num(value.min_index as f64)),
                    ]),
                };
                let sum = obj([
                    ("max_index", CValue::Num(value.max_index as f64)),
                    ("max_si", CValue::Str(hex(value.max_si))),
                    ("min_index", CValue::Num(value.min_index as f64)),
                    ("min_si", CValue::Str(hex(value.min_si))),
                    ("range_si", CValue::Str(hex(value.range_si))),
                    ("statistic", CValue::Str(value.statistic.as_str().to_owned())),
                ]);
                (value.value_si, value.si_unit, value.quantity_kind, value.algorithm(), att, sum)
            }
        };
        obj([
            ("algorithm", CValue::Str(algorithm.to_owned())),
            ("attained_at", attained_at),
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
            ("interpretation", CValue::Str(self.interpretation.to_owned())),
            ("label", CValue::Str(self.label.to_owned())),
            ("minmax", summary),
            ("quantity_kind", CValue::Str(qkind.as_str().to_owned())),
            ("run", CValue::Str(self.run.as_str().to_owned())),
            ("schema", CValue::Str(RESULT_SCHEMA.to_owned())),
            ("unit", CValue::Str(si_unit.to_owned())),
            ("value_si", CValue::Str(hex(value_si))),
        ])
    }

    /// RFC 8785 canonical JSON bytes of [`Self::to_value`].
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, EvalError> {
        fid_canon::to_canonical_bytes(&self.to_value())
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })
    }
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

/// Evaluates `check.eval_scalar@1` (`REQ-001`, `REQ-002`, `REQ-005`).
pub fn eval_scalar(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &ScalarRequest<'_>,
    dry_run: bool,
) -> Result<Evaluated, EvalError> {
    eval_scalar_for(results, inp, principal, req, dry_run, Purpose::Evaluate)
}

pub(crate) fn eval_scalar_for(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &ScalarRequest<'_>,
    dry_run: bool,
    purpose: Purpose,
) -> Result<Evaluated, EvalError> {
    let run = RunId::parse(req.run).map_err(|_| EvalError::BadParams { reason: "bad_run_id" })?;
    let char = CharId::parse(req.char).ok_or(EvalError::BadParams { reason: "bad_char_id" })?;
    if !is_id(req.element) {
        return Err(EvalError::BadParams { reason: "bad_element_id" });
    }
    if req.property.is_empty() || req.property.len() > MAX_ID_LEN {
        return Err(EvalError::BadParams { reason: "bad_property" });
    }
    if let Some(v) = req.variant {
        if !is_id(v) {
            return Err(EvalError::BadParams { reason: "bad_variant" });
        }
    }

    let char_ref = chain_characteristic(&char, "scalar", req.variant)?;
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
    if row.characteristic() != "scalar" || !row.modifiers().is_empty() {
        return Err(EvalError::ChainNotResolved { outcome: "UNRESOLVED" });
    }

    let el = inp.store.element(req.element).ok_or_else(|| EvalError::ElementMissing { element: echo(req.element) })?;
    let input_digests = vec![el.digest];
    let value = evaluate_scalar(&el.element, req.property, req.tolerance_unit).map_err(EvalError::Scalar)?;
    let basis = EvaluationBasis::Features;

    match results.get(principal, &run, &char) {
        Ok(_) => return Err(EvalError::Record(RecordError::DuplicateResult)),
        Err(RecordError::NotFound) => {}
        Err(e) => return Err(EvalError::Record(e)),
    }

    let spec_err = |e: SpecError| EvalError::Chain(CommandError::Spec(e));
    let gs = inp.spec.require_governing_spec(principal, char.as_str(), inp.dprof).map_err(spec_err)?;
    let stored =
        inp.dprof.resolve(Branch::for_principal(principal), gs.defaults_profile()).map_err(|e| spec_err(e.into()))?;
    let measurand_text = format!("scalar({}.{}.{})", req.element, value.element_kind, value.property);
    let measurand = Measurand::parse(&measurand_text)?;
    let builder = ResultRecordBuilder::new(run.clone(), char.clone())
        .measurand(measurand)
        .evaluation_basis(basis)
        .value(Quantity::new(value.value_si, value.si_unit)?)
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
        command: EVAL_SCALAR_COMMAND,
        principal,
        branch: Branch::for_principal(principal),
        run,
        char,
        variant: req.variant.map(str::to_owned),
        basis,
        outcome: ScalarOutcome::Scalar {
            element: req.element.to_owned(),
            property: req.property.to_owned(),
            tolerance_unit: req.tolerance_unit.map(str::to_owned),
            value,
        },
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
        ev.event = Some(ScalarEvent::of(&ev));
    }
    Ok(ev)
}

/// Evaluates `check.eval_minmax@1` (`REQ-003`, `REQ-004`, `REQ-005`).
pub fn eval_minmax(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &MinMaxRequest<'_>,
    dry_run: bool,
) -> Result<Evaluated, EvalError> {
    eval_minmax_for(results, inp, principal, req, dry_run, Purpose::Evaluate)
}

pub(crate) fn eval_minmax_for(
    results: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &MinMaxRequest<'_>,
    dry_run: bool,
    purpose: Purpose,
) -> Result<Evaluated, EvalError> {
    let run = RunId::parse(req.run).map_err(|_| EvalError::BadParams { reason: "bad_run_id" })?;
    let char = CharId::parse(req.char).ok_or(EvalError::BadParams { reason: "bad_char_id" })?;
    if !is_id(req.values) {
        return Err(EvalError::BadParams { reason: "bad_value_set_id" });
    }
    if let Some(v) = req.variant {
        if !is_id(v) {
            return Err(EvalError::BadParams { reason: "bad_variant" });
        }
    }

    let char_ref = chain_characteristic(&char, "minmax", req.variant)?;
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
    if row.characteristic() != "minmax" || !row.modifiers().is_empty() {
        return Err(EvalError::ChainNotResolved { outcome: "UNRESOLVED" });
    }

    let vs =
        inp.store.value_set(req.values).ok_or_else(|| EvalError::ValueSetMissing { value_set: echo(req.values) })?;
    let input_digests = vec![vs.digest];
    let value = evaluate_minmax(&vs.values, req.statistic, vs.quantity_kind).map_err(EvalError::Scalar)?;
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
    let measurand_text = format!("minmax({}, {})", req.values, req.statistic.as_str());
    let measurand = Measurand::parse(&measurand_text)?;
    let builder = ResultRecordBuilder::new(run.clone(), char.clone())
        .measurand(measurand)
        .evaluation_basis(basis)
        .value(Quantity::new(value.value_si, value.si_unit)?)
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
        command: EVAL_MINMAX_COMMAND,
        principal,
        branch: Branch::for_principal(principal),
        run,
        char,
        variant: req.variant.map(str::to_owned),
        basis,
        outcome: ScalarOutcome::MinMax { values: req.values.to_owned(), value },
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
        ev.event = Some(ScalarEvent::of(&ev));
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
