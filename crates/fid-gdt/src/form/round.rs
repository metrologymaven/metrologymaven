//! Roundness (circularity) check command (`GDT.FORM.ROUND`): `check.eval_roundness@1`
//! (W2, dry-run R1; CLI `fid check eval-roundness`, MCP `check_eval_roundness`), governing-spec
//! gate, `gdt.resolve_chain@1`, explicit 2D sections or 3D cross-section extraction,
//! `ResultRecord` construction, journal event and exact D0 replay (`AC-1`..`AC-4`).

use crate::chars::dprof::store::parse_principal;
use crate::chars::dprof::{Branch, CharId, DprofState};
use crate::chars::interp::{InterpretationPin, Interpreted, RegisterView, PROVISIONAL_LABEL_TEXT, SIGNED_OFF_LINE};
use crate::chars::record::{
    AlgorithmRef, EvaluationBasis, InputDigests, KernelVersion, Measurand, Quantity, RecordError, ResultRecord,
    ResultRecordBuilder, ResultStore, RunId, Uncertainty,
};
use crate::chars::spec::{SpecError, SpecState};
use crate::dmt_flow::roster::Dialect;
use crate::dmt_flow::status::Purpose;
use crate::drf::build::ReplayRegisters;
use crate::form::flat::FormPointSets;
use crate::resolve::{resolve_chain, CharacteristicRef, CommandError, CHAR_SCHEMA};
use fid_canon::{CValue, CanonError};
use fid_gps::dmt::DmtTable;
use fid_gps::form::round::{
    evaluate_roundness_3d, evaluate_roundness_sections, RoundnessError, RoundnessResult, SectionPoints,
};
use fid_gps::form::{FitProvider, FormAssociation, KrnFitProvider};
use fid_gps::resolve::Resolution;
use fid_io_reg::Principal;
use fid_kunc::Uec0Reason;
use fid_registry::{CommandDef, DefError};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Registry command ID for roundness (`GDT.FORM.ROUND`).
pub const EVAL_ROUNDNESS_COMMAND: &str = "check.eval_roundness@1";
/// CLI subcommand under `fid check`.
pub const EVAL_ROUNDNESS_CLI: &str = "eval-roundness";
/// MCP tool name.
pub const EVAL_ROUNDNESS_MCP: &str = "check_eval_roundness";
/// Schema tag of the command result JSON object.
pub const RESULT_SCHEMA: &str = "fid.check-roundness-result/1";
/// Record unit: SI metres.
pub const UNIT: &str = "si:m";
/// Maximum MCP parameter payload in bytes.
pub const MAX_PARAMS_BYTES: usize = 4 * 1024;
const MAX_PARAMS_DEPTH: usize = 3;
const MAX_CLI_ARGS: usize = 16;
const MAX_ID_LEN: usize = 128;
const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_EVENT_DEPTH: usize = 5;
const LIFECYCLE_SINCE: &str = "2026-11-23";
const ID_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";

fn hex(v: f64) -> String {
    format!("0x{:016x}", v.to_bits())
}

fn obj<const N: usize>(items: [(&str, CValue); N]) -> CValue {
    CValue::Obj(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn is_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_ID_LEN
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
        && s.as_bytes()[0].is_ascii_alphanumeric()
}

fn principal_str(p: Principal) -> &'static str {
    match p {
        Principal::Human => "human",
        Principal::Agent => "agent",
        Principal::Script => "script",
    }
}

fn parse_association(s: &str) -> Option<FormAssociation> {
    match s {
        "minimum_zone" => Some(FormAssociation::MinimumZone),
        "least_squares" => Some(FormAssociation::LeastSquares),
        _ => None,
    }
}

fn parse_hex_bits(s: &str) -> Result<u64, EvalError> {
    let bad = || EvalError::BadParams { reason: "malformed_event" };
    let rest = s.strip_prefix("0x").ok_or_else(bad)?;
    if rest.len() != 16 {
        return Err(bad());
    }
    u64::from_str_radix(rest, 16).map_err(|_| bad())
}

/// Stored explicit 2D section set with precomputed canonical content digest.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredSectionSet {
    pub sections: Vec<SectionPoints>,
    pub digest: Digest,
}

/// Read-only store of explicit 2D cross-section sets for `check.eval_roundness@1`.
#[derive(Debug, Clone, Default)]
pub struct FormSectionSets {
    sets: BTreeMap<String, StoredSectionSet>,
}

impl FormSectionSets {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers explicit 2D cross-sections under `handle` and computes a canonical SHA-256 digest.
    pub fn insert(&mut self, handle: &str, sections: Vec<SectionPoints>) -> Result<Digest, EvalError> {
        if !is_id(handle) {
            return Err(EvalError::BadParams { reason: "bad_sections_handle" });
        }
        let secs_val = CValue::Arr(
            sections
                .iter()
                .map(|s| {
                    let pts = CValue::Arr(
                        s.points2d
                            .iter()
                            .map(|p| CValue::Arr(vec![CValue::Str(hex(p[0])), CValue::Str(hex(p[1]))]))
                            .collect(),
                    );
                    obj([("points2d", pts), ("position_m_hex", CValue::Str(hex(s.position_m)))])
                })
                .collect(),
        );
        let bytes =
            fid_canon::to_canonical_bytes(&obj([("handle", CValue::Str(handle.to_owned())), ("sections", secs_val)]))
                .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
        let digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
        self.sets.insert(handle.to_owned(), StoredSectionSet { sections, digest });
        Ok(digest)
    }

    pub fn get(&self, handle: &str) -> Option<&StoredSectionSet> {
        self.sets.get(handle)
    }
}

/// Input source for `check.eval_roundness@1`: either explicit 2D sections or a 3D point cloud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundnessSource<'a> {
    Sections(&'a str),
    Points3d(&'a str),
}

/// Owned input source recorded in [`RoundnessEvent`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedRoundnessSource {
    Sections(String),
    Points3d(String),
}

impl OwnedRoundnessSource {
    fn as_ref(&self) -> RoundnessSource<'_> {
        match self {
            OwnedRoundnessSource::Sections(s) => RoundnessSource::Sections(s.as_str()),
            OwnedRoundnessSource::Points3d(p) => RoundnessSource::Points3d(p.as_str()),
        }
    }

    fn kind_str(&self) -> &'static str {
        match self {
            OwnedRoundnessSource::Sections(_) => "sections",
            OwnedRoundnessSource::Points3d(_) => "points",
        }
    }

    fn handle(&self) -> &str {
        match self {
            OwnedRoundnessSource::Sections(s) | OwnedRoundnessSource::Points3d(s) => s.as_str(),
        }
    }
}

/// Errors returned by `check.eval_roundness@1` (`REQ-001`..`REQ-004`).
#[derive(Debug, Clone, PartialEq)]
pub enum EvalError {
    BadParams { reason: &'static str },
    InputMissing { handle: String },
    Chain(CommandError),
    ChainNotResolved { outcome: &'static str },
    Roundness(RoundnessError),
    Record(RecordError),
}

impl EvalError {
    pub fn code(&self) -> &'static str {
        match self {
            EvalError::BadParams { .. } => "bad_params",
            EvalError::InputMissing { .. } => "input_missing",
            EvalError::Chain(e) => e.code(),
            EvalError::ChainNotResolved { .. } => "chain_not_resolved",
            EvalError::Roundness(e) => e.code(),
            EvalError::Record(e) => e.code(),
        }
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::BadParams { reason } => write!(f, "bad_params{{{reason}}}"),
            EvalError::InputMissing { handle } => write!(f, "input_missing{{{handle}}}"),
            EvalError::Chain(e) => write!(f, "{e}"),
            EvalError::ChainNotResolved { outcome } => write!(f, "chain_not_resolved{{{outcome}}}"),
            EvalError::Roundness(e) => write!(f, "{e}"),
            EvalError::Record(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for EvalError {}

impl From<RecordError> for EvalError {
    fn from(e: RecordError) -> Self {
        EvalError::Record(e)
    }
}

/// Read-only context required by [`eval_roundness`].
pub struct EvalInputs<'a> {
    pub spec: &'a SpecState,
    pub dprof: &'a DprofState,
    pub table: Option<&'a DmtTable>,
    pub register: &'a RegisterView,
    pub section_sets: &'a FormSectionSets,
    pub point_sets: &'a FormPointSets,
    pub krn: &'a dyn FitProvider,
}

impl<'a> EvalInputs<'a> {
    /// Convenience constructor using [`KrnFitProvider`] (exact Voronoi MZ 2D circle + LS fallback).
    pub fn with_default_krn(
        spec: &'a SpecState,
        dprof: &'a DprofState,
        table: Option<&'a DmtTable>,
        register: &'a RegisterView,
        section_sets: &'a FormSectionSets,
        point_sets: &'a FormPointSets,
    ) -> Self {
        static DEFAULT_KRN: KrnFitProvider = KrnFitProvider;
        Self { spec, dprof, table, register, section_sets, point_sets, krn: &DEFAULT_KRN }
    }
}

/// Typed request for `check.eval_roundness@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalRequest<'a> {
    pub run: &'a str,
    pub char: &'a str,
    pub source: RoundnessSource<'a>,
    pub variant: Option<&'a str>,
}

/// Per-section digest item recorded in [`RoundnessEvent`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionEventSummary {
    pub section_index: usize,
    pub position_m_bits: u64,
    pub value_si_bits: u64,
    pub association: FormAssociation,
    pub peak_index: usize,
    pub valley_index: usize,
}

/// Committed journal event of `check.eval_roundness@1` (`AC-4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundnessEvent {
    principal: Principal,
    branch: Branch,
    run: RunId,
    char: CharId,
    source: OwnedRoundnessSource,
    variant: Option<String>,
    value_si_bits: u64,
    worst_section_index: usize,
    algorithm_id: String,
    sections: Vec<SectionEventSummary>,
    input_digest: Digest,
    row: String,
    chain_variant: String,
    chain_digest: Digest,
    dmt_digest: Digest,
    interpretation: String,
    pin: InterpretationPin,
}

impl RoundnessEvent {
    fn of(ev: &RoundnessEvaluated) -> Self {
        let sections = ev
            .result
            .sections
            .iter()
            .map(|s| SectionEventSummary {
                section_index: s.section_index,
                position_m_bits: s.position_m.to_bits(),
                value_si_bits: s.payload.value_si.to_bits(),
                association: s.payload.association,
                peak_index: s.payload.peak_index,
                valley_index: s.payload.valley_index,
            })
            .collect();
        let worst_alg = ev
            .result
            .sections
            .get(ev.result.worst_section_index)
            .map(|s| s.payload.algorithm_id.clone())
            .unwrap_or_default();
        Self {
            principal: ev.principal,
            branch: ev.branch,
            run: ev.run.clone(),
            char: ev.char.clone(),
            source: ev.source.clone(),
            variant: ev.variant.clone(),
            value_si_bits: ev.result.value_si.to_bits(),
            worst_section_index: ev.result.worst_section_index,
            algorithm_id: worst_alg,
            sections,
            input_digest: ev.input_digest,
            row: ev.row.clone(),
            chain_variant: ev.chain_variant.clone(),
            chain_digest: ev.chain_digest,
            dmt_digest: ev.dmt_digest,
            interpretation: ev.interpretation.to_owned(),
            pin: ev.pin,
        }
    }

    /// IEEE-754 binary64 bits of the worst-section roundness in metres.
    pub fn value_si_bits(&self) -> u64 {
        self.value_si_bits
    }

    pub fn to_value(&self) -> CValue {
        let secs = CValue::Arr(
            self.sections
                .iter()
                .map(|s| {
                    obj([
                        ("association", CValue::Str(s.association.as_str().to_owned())),
                        ("peak_index", CValue::Str(s.peak_index.to_string())),
                        ("position_m_hex", CValue::Str(hex(f64::from_bits(s.position_m_bits)))),
                        ("section_index", CValue::Str(s.section_index.to_string())),
                        ("valley_index", CValue::Str(s.valley_index.to_string())),
                        ("value_si_hex", CValue::Str(hex(f64::from_bits(s.value_si_bits)))),
                    ])
                })
                .collect(),
        );
        obj([
            ("algorithm_id", CValue::Str(self.algorithm_id.clone())),
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
            ("command", CValue::Str(EVAL_ROUNDNESS_COMMAND.to_owned())),
            ("input_digest", CValue::Str(self.input_digest.as_str().to_owned())),
            ("interpretation", CValue::Str(self.interpretation.clone())),
            ("interpretation_pin", self.pin.to_cvalue()),
            ("principal", CValue::Str(principal_str(self.principal).to_owned())),
            ("run", CValue::Str(self.run.as_str().to_owned())),
            ("sections", secs),
            ("source_handle", CValue::Str(self.source.handle().to_owned())),
            ("source_kind", CValue::Str(self.source.kind_str().to_owned())),
            ("value_si_hex", CValue::Str(hex(f64::from_bits(self.value_si_bits)))),
            ("variant", self.variant.as_ref().map_or(CValue::Null, |v| CValue::Str(v.clone()))),
            ("worst_section_index", CValue::Str(self.worst_section_index.to_string())),
        ])
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, EvalError> {
        fid_canon::to_canonical_bytes(&self.to_value())
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })
    }

    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, EvalError> {
        let bad = || EvalError::BadParams { reason: "malformed_event" };
        let v = fid_canon::parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad())?;
        let CValue::Obj(o) = &v else { return Err(bad()) };
        if o.len() != 16 || req_str(o, "command")? != EVAL_ROUNDNESS_COMMAND {
            return Err(bad());
        }
        let principal = parse_principal(req_str(o, "principal")?).ok_or_else(bad)?;
        let branch = Branch::for_principal(principal);
        if req_str(o, "branch")? != branch.as_str() {
            return Err(bad());
        }
        let run = RunId::parse(req_str(o, "run")?).map_err(|_| bad())?;
        let char = CharId::parse(req_str(o, "char")?).ok_or_else(bad)?;
        let source_handle = req_str(o, "source_handle")?.to_owned();
        if !is_id(&source_handle) {
            return Err(bad());
        }
        let source = match req_str(o, "source_kind")? {
            "sections" => OwnedRoundnessSource::Sections(source_handle),
            "points" => OwnedRoundnessSource::Points3d(source_handle),
            _ => return Err(bad()),
        };
        let variant = opt_str(o, "variant")?.map(str::to_owned);
        if let Some(vr) = &variant {
            if !is_id(vr) {
                return Err(bad());
            }
        }
        let value_si_bits = parse_hex_bits(req_str(o, "value_si_hex")?)?;
        let worst_section_index: usize = req_str(o, "worst_section_index")?.parse().map_err(|_| bad())?;
        let algorithm_id = req_str(o, "algorithm_id")?.to_owned();
        let Some(CValue::Arr(raw_secs)) = o.get("sections") else { return Err(bad()) };
        let mut sections = Vec::with_capacity(raw_secs.len());
        for s in raw_secs {
            let CValue::Obj(so) = s else { return Err(bad()) };
            sections.push(SectionEventSummary {
                section_index: req_str(so, "section_index")?.parse().map_err(|_| bad())?,
                position_m_bits: parse_hex_bits(req_str(so, "position_m_hex")?)?,
                value_si_bits: parse_hex_bits(req_str(so, "value_si_hex")?)?,
                association: parse_association(req_str(so, "association")?).ok_or_else(bad)?,
                peak_index: req_str(so, "peak_index")?.parse().map_err(|_| bad())?,
                valley_index: req_str(so, "valley_index")?.parse().map_err(|_| bad())?,
            });
        }
        let input_digest = Digest::parse(req_str(o, "input_digest")?).map_err(|_| bad())?;
        let Some(CValue::Obj(ch)) = o.get("chain") else { return Err(bad()) };
        let row = req_str(ch, "row")?.to_owned();
        let chain_variant = req_str(ch, "variant")?.to_owned();
        let chain_digest = Digest::parse(req_str(ch, "chain_digest")?).map_err(|_| bad())?;
        let dmt_digest = Digest::parse(req_str(ch, "dmt_digest")?).map_err(|_| bad())?;
        let interpretation = req_str(o, "interpretation")?.to_owned();
        let pin_bytes =
            fid_canon::to_canonical_bytes(o.get("interpretation_pin").ok_or_else(bad)?).map_err(|_| bad())?;
        let pin = InterpretationPin::from_canonical_bytes(&pin_bytes).ok_or_else(bad)?;

        Ok(Self {
            principal,
            branch,
            run,
            char,
            source,
            variant,
            value_si_bits,
            worst_section_index,
            algorithm_id,
            sections,
            input_digest,
            row,
            chain_variant,
            chain_digest,
            dmt_digest,
            interpretation,
            pin,
        })
    }
}

/// Output of [`eval_roundness`].
#[derive(Debug, Clone)]
pub struct RoundnessEvaluated {
    pub command: &'static str,
    pub principal: Principal,
    pub branch: Branch,
    pub run: RunId,
    pub char: CharId,
    pub source: OwnedRoundnessSource,
    pub variant: Option<String>,
    pub result: RoundnessResult,
    pub input_digest: Digest,
    pub row: String,
    pub chain_variant: String,
    pub chain_digest: Digest,
    pub dmt_digest: Digest,
    pub pin: InterpretationPin,
    pub interpretation: &'static str,
    pub label: &'static str,
    pub dry_run: bool,
    builder: ResultRecordBuilder<Interpreted>,
    event: Option<RoundnessEvent>,
}

impl RoundnessEvaluated {
    pub fn event(&self) -> Option<&RoundnessEvent> {
        self.event.as_ref()
    }

    pub fn value_si_bits(&self) -> u64 {
        self.result.value_si.to_bits()
    }

    pub fn record(&self, journal_entry: Digest) -> Result<ResultRecord, EvalError> {
        if self.dry_run {
            return Err(EvalError::BadParams { reason: "cannot commit dry_run" });
        }
        Ok(self.builder.clone().journal_entry(journal_entry).finish()?)
    }

    pub fn to_value(&self) -> Result<CValue, EvalError> {
        let mut secs_val = Vec::with_capacity(self.result.sections.len());
        for s in &self.result.sections {
            secs_val.push(s.to_cvalue().map_err(|e| {
                EvalError::Roundness(RoundnessError::Form { section: Some(s.section_index), source: e })
            })?);
        }
        let extracted_val = self.result.extracted.as_ref().map_or(CValue::Null, |ex| ex.to_cvalue());
        Ok(obj([
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
            ("evaluation_basis", CValue::Str("POINTS".to_owned())),
            ("extracted", extracted_val),
            ("interpretation", CValue::Str(self.interpretation.to_owned())),
            ("label", CValue::Str(self.label.to_owned())),
            ("run", CValue::Str(self.run.as_str().to_owned())),
            ("schema", CValue::Str(RESULT_SCHEMA.to_owned())),
            ("sections", CValue::Arr(secs_val)),
            ("source_handle", CValue::Str(self.source.handle().to_owned())),
            ("source_kind", CValue::Str(self.source.kind_str().to_owned())),
            ("unit", CValue::Str(UNIT.to_owned())),
            ("value_si_hex", CValue::Str(hex(self.result.value_si))),
            ("worst_section_index", CValue::Str(self.result.worst_section_index.to_string())),
        ]))
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, EvalError> {
        let v = self.to_value()?;
        fid_canon::to_canonical_bytes(&v).map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })
    }
}

fn chain_characteristic(char: &CharId, dmt_name: &str, variant: Option<&str>) -> Result<CharacteristicRef, EvalError> {
    let doc = obj([
        ("char", CValue::Str(char.as_str().to_owned())),
        ("characteristic", CValue::Str(dmt_name.to_owned())),
        ("modifiers", CValue::Arr(Vec::new())),
        ("override", variant.map_or(CValue::Null, |v| CValue::Str(v.to_owned()))),
        ("schema", CValue::Str(CHAR_SCHEMA.to_owned())),
    ]);
    let bytes =
        fid_canon::to_canonical_bytes(&doc).map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
    CharacteristicRef::parse(&bytes).map_err(EvalError::Chain)
}

/// Evaluates `check.eval_roundness@1` (`REQ-001`..`REQ-005`).
pub fn eval_roundness(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &EvalRequest<'_>,
    dry_run: bool,
) -> Result<RoundnessEvaluated, EvalError> {
    eval_for(store, inp, principal, req, dry_run, Purpose::Evaluate)
}

fn eval_for(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &EvalRequest<'_>,
    dry_run: bool,
    purpose: Purpose,
) -> Result<RoundnessEvaluated, EvalError> {
    let run = RunId::parse(req.run).map_err(|_| EvalError::BadParams { reason: "bad_run_id" })?;
    let char = CharId::parse(req.char).ok_or(EvalError::BadParams { reason: "bad_char_id" })?;
    let owned_source = match req.source {
        RoundnessSource::Sections(h) => {
            if !is_id(h) {
                return Err(EvalError::BadParams { reason: "bad_sections_handle" });
            }
            OwnedRoundnessSource::Sections(h.to_owned())
        }
        RoundnessSource::Points3d(h) => {
            if !is_id(h) {
                return Err(EvalError::BadParams { reason: "bad_points_handle" });
            }
            OwnedRoundnessSource::Points3d(h.to_owned())
        }
    };
    if let Some(v) = req.variant {
        if !is_id(v) {
            return Err(EvalError::BadParams { reason: "bad_variant" });
        }
    }

    // 1. Governing-spec gate + dialect-appropriate DMT lookup ("circularity" for ASME, "roundness" for ISO).
    let spec_err = |e: SpecError| EvalError::Chain(CommandError::Spec(e));
    let gs = inp.spec.require_governing_spec(principal, char.as_str(), inp.dprof).map_err(spec_err)?;
    let dmt_char_name = match gs.dialect() {
        Dialect::Asme => "circularity",
        Dialect::Iso => "roundness",
    };
    let char_ref = chain_characteristic(&char, dmt_char_name, req.variant)?;
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
    if row.characteristic() != dmt_char_name || !row.modifiers().is_empty() {
        return Err(EvalError::ChainNotResolved { outcome: "UNRESOLVED" });
    }

    // 2. Evaluate either explicit 2D sections or 3D cloud extraction.
    let (round_res, input_digest) = match &owned_source {
        OwnedRoundnessSource::Sections(h) => {
            let ss = inp.section_sets.get(h).ok_or_else(|| EvalError::InputMissing { handle: h.clone() })?;
            let r = evaluate_roundness_sections(&ss.sections, res.chain(), inp.krn).map_err(EvalError::Roundness)?;
            (r, ss.digest)
        }
        OwnedRoundnessSource::Points3d(h) => {
            let ps = inp.point_sets.get(h).ok_or_else(|| EvalError::InputMissing { handle: h.clone() })?;
            let r = evaluate_roundness_3d(&ps.points, res.chain(), inp.krn).map_err(EvalError::Roundness)?;
            (r, ps.digest)
        }
    };

    // 3. Enforce one record per (run, characteristic).
    match store.get(principal, &run, &char) {
        Ok(_) => return Err(EvalError::Record(RecordError::DuplicateResult)),
        Err(RecordError::NotFound) => {}
        Err(e) => return Err(EvalError::Record(e)),
    }

    // 4. Build and pre-validate ResultRecord before emitting any journal event.
    let stored =
        inp.dprof.resolve(Branch::for_principal(principal), gs.defaults_profile()).map_err(|e| spec_err(e.into()))?;
    let worst = round_res
        .sections
        .get(round_res.worst_section_index)
        .ok_or(EvalError::Roundness(RoundnessError::NoSections))?;
    let measurand = Measurand::parse(&format!("roundness({})", owned_source.handle()))?;
    let builder = ResultRecordBuilder::new(run.clone(), char.clone())
        .measurand(measurand)
        .evaluation_basis(EvaluationBasis::Points)
        .value(Quantity::new(round_res.value_si, UNIT)?)
        .uncertainty(Uncertainty::uec0(Uec0Reason::UMissing))
        .bind_decision(None, None)
        .spec(gs.clone())
        .algorithm(AlgorithmRef::parse(&worst.payload.algorithm_id)?)
        .kernel_version(KernelVersion::parse(env!("CARGO_PKG_VERSION"))?)
        .input_digests(InputDigests::new(vec![input_digest])?)
        .resolved(&res)
        .interpretation_for(&[row], Some(stored.doc()), inp.register, purpose);
    let trial = builder.clone().journal_entry(Digest::TOMBSTONE).finish()?;
    let pin = *trial.interpretation_pin();
    let interpretation = trial.interpretation().as_str();
    let label = if crate::requires_provisional_label(&trial) { PROVISIONAL_LABEL_TEXT } else { SIGNED_OFF_LINE };

    let mut ev = RoundnessEvaluated {
        command: EVAL_ROUNDNESS_COMMAND,
        principal,
        branch: Branch::for_principal(principal),
        run,
        char,
        source: owned_source,
        variant: req.variant.map(str::to_owned),
        result: round_res,
        input_digest,
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
        ev.event = Some(RoundnessEvent::of(&ev));
    }
    Ok(ev)
}

/// Commits the `ResultRecord` of a non-dry-run evaluation with its journal entry hash.
pub fn commit(
    store: &mut ResultStore,
    principal: Principal,
    ev: &RoundnessEvaluated,
    journal_entry: Digest,
) -> Result<Digest, EvalError> {
    if principal != ev.principal {
        return Err(EvalError::BadParams { reason: "principal_mismatch" });
    }
    let record = ev.record(journal_entry)?;
    Ok(store.commit(principal, record)?)
}

/// Result of exact replay over a sequence of `check.eval_roundness@1` journal entries (`AC-4`).
#[derive(Debug, Clone)]
pub struct RoundnessReplayed {
    pub outcome: ReplayOutcome,
    pub steps: usize,
    pub store: ResultStore,
}

/// Replays `(entry_hash, principal, canonical_event_bytes)` entries in order (`AC-4`).
pub fn replay(
    inp: &EvalInputs<'_>,
    regs: &ReplayRegisters<'_>,
    entries: &[(Digest, Principal, &[u8])],
) -> RoundnessReplayed {
    let mut store = ResultStore::new();
    let mut steps = 0usize;
    for &(entry_hash, entry_principal, raw) in entries {
        let Ok(recorded) = RoundnessEvent::from_canonical_bytes(raw) else {
            return RoundnessReplayed { outcome: ReplayOutcome::NotReplayable, steps, store };
        };
        if recorded.principal != entry_principal {
            return RoundnessReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        }
        let Ok(register) = RegisterView::for_replay(&recorded.pin, regs.store, regs.sources) else {
            return RoundnessReplayed { outcome: ReplayOutcome::NotReplayable, steps, store };
        };
        let replay_inputs = EvalInputs {
            spec: inp.spec,
            dprof: inp.dprof,
            table: inp.table,
            register: &register,
            section_sets: inp.section_sets,
            point_sets: inp.point_sets,
            krn: inp.krn,
        };
        let req = EvalRequest {
            run: recorded.run.as_str(),
            char: recorded.char.as_str(),
            source: recorded.source.as_ref(),
            variant: recorded.variant.as_deref(),
        };
        let Ok(ev) = eval_for(&store, &replay_inputs, entry_principal, &req, false, Purpose::Replay) else {
            return RoundnessReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        };
        let Some(rebuilt_event) = ev.event() else {
            return RoundnessReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        };
        let Ok(rebuilt_bytes) = rebuilt_event.to_canonical_bytes() else {
            return RoundnessReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        };
        if rebuilt_bytes != raw {
            return RoundnessReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        }
        if commit(&mut store, entry_principal, &ev, entry_hash).is_err() {
            return RoundnessReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        }
        steps += 1;
    }
    RoundnessReplayed { outcome: ReplayOutcome::Identical, steps, store }
}

// ================================================================== CLI, MCP & CommandDef

fn opt_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<Option<&'a str>, EvalError> {
    match o.get(k) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(EvalError::BadParams { reason: "parameter is not a string" }),
    }
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, EvalError> {
    opt_str(o, k)?.ok_or(EvalError::BadParams { reason: "missing parameter" })
}

/// Runs `fid check eval-roundness --run <R> --char <C> (--sections <S> | --points <P>) [--variant <V>] [--dry-run] [--json]`.
pub fn invoke_cli(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    args: &[&str],
) -> Result<RoundnessEvaluated, EvalError> {
    if args.is_empty() || args.len() > MAX_CLI_ARGS || args[0] != EVAL_ROUNDNESS_CLI {
        return Err(EvalError::BadParams { reason: "bad_cli_subcommand" });
    }
    let mut run: Option<&str> = None;
    let mut char_id: Option<&str> = None;
    let mut sections: Option<&str> = None;
    let mut points: Option<&str> = None;
    let mut variant: Option<&str> = None;
    let mut dry_run = false;
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "--json" => {}
            "--dry-run" => dry_run = true,
            "--run" => {
                i += 1;
                run = Some(*args.get(i).ok_or(EvalError::BadParams { reason: "missing --run" })?);
            }
            "--char" => {
                i += 1;
                char_id = Some(*args.get(i).ok_or(EvalError::BadParams { reason: "missing --char" })?);
            }
            "--sections" => {
                i += 1;
                sections = Some(*args.get(i).ok_or(EvalError::BadParams { reason: "missing --sections" })?);
            }
            "--points" => {
                i += 1;
                points = Some(*args.get(i).ok_or(EvalError::BadParams { reason: "missing --points" })?);
            }
            "--variant" => {
                i += 1;
                variant = Some(*args.get(i).ok_or(EvalError::BadParams { reason: "missing --variant" })?);
            }
            _ => return Err(EvalError::BadParams { reason: "unknown_flag" }),
        }
        i += 1;
    }
    let source = match (sections, points) {
        (Some(s), None) => RoundnessSource::Sections(s),
        (None, Some(p)) => RoundnessSource::Points3d(p),
        _ => return Err(EvalError::BadParams { reason: "provide_exactly_one_of_sections_or_points" }),
    };
    let req = EvalRequest {
        run: run.ok_or(EvalError::BadParams { reason: "missing --run" })?,
        char: char_id.ok_or(EvalError::BadParams { reason: "missing --char" })?,
        source,
        variant,
    };
    eval_roundness(store, inp, principal, &req, dry_run)
}

/// Runs MCP tool `check_eval_roundness` on raw JSON parameters.
pub fn invoke_mcp(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    tool: &str,
    raw: &[u8],
) -> Result<RoundnessEvaluated, EvalError> {
    if tool != EVAL_ROUNDNESS_MCP {
        return Err(EvalError::BadParams { reason: "unknown_tool" });
    }
    let val = fid_canon::parse_with_limits(raw, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => EvalError::BadParams { reason: "params_too_large" },
        CanonError::DuplicateKey(_) => EvalError::BadParams { reason: "duplicate_key" },
        _ => EvalError::BadParams { reason: "invalid_json" },
    })?;
    let CValue::Obj(o) = val else {
        return Err(EvalError::BadParams { reason: "params_not_object" });
    };
    let dry_run = match o.get("dry_run") {
        None | Some(CValue::Null) => false,
        Some(CValue::Bool(b)) => *b,
        _ => return Err(EvalError::BadParams { reason: "bad_dry_run" }),
    };
    let source = match (opt_str(&o, "sections")?, opt_str(&o, "points")?) {
        (Some(s), None) => RoundnessSource::Sections(s),
        (None, Some(p)) => RoundnessSource::Points3d(p),
        _ => return Err(EvalError::BadParams { reason: "provide_exactly_one_of_sections_or_points" }),
    };
    let req =
        EvalRequest { run: req_str(&o, "run")?, char: req_str(&o, "char")?, source, variant: opt_str(&o, "variant")? };
    eval_roundness(store, inp, principal, &req, dry_run)
}

/// Registry command definition for `check.eval_roundness@1` (W2, D0, CLI + MCP).
pub fn roundness_command_def() -> Result<CommandDef, DefError> {
    let id_schema =
        || obj([("pattern", CValue::Str(ID_PATTERN.to_owned())), ("type", CValue::Str("string".to_owned()))]);
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("char", id_schema()),
                ("dry_run", obj([("type", CValue::Str("boolean".to_owned()))])),
                ("points", id_schema()),
                ("run", id_schema()),
                ("sections", id_schema()),
                ("variant", id_schema()),
            ]),
        ),
        ("required", CValue::Arr(vec![CValue::Str("char".to_owned()), CValue::Str("run".to_owned())])),
        ("type", CValue::Str("object".to_owned())),
    ]);
    let result_schema = obj([("additionalProperties", CValue::Bool(true)), ("type", CValue::Str("object".to_owned()))]);
    CommandDef::from_value(obj([
        ("algorithms", CValue::Arr(Vec::new())),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            CValue::Str(
                "Evaluates roundness (circularity) across explicit 2D sections or 3D cross-sections extracted along a reference axis."
                    .to_owned(),
            ),
        ),
        ("determinism", CValue::Str("D0".to_owned())),
        ("effects", CValue::Arr(vec![CValue::Str("ResultRecorded".to_owned())])),
        ("exposure", CValue::Arr(vec![CValue::Str("cli".to_owned()), CValue::Str("mcp".to_owned())])),
        ("fs_effect", CValue::Str("none".to_owned())),
        ("human_only", CValue::Bool(false)),
        ("id", CValue::Str("check.eval_roundness".to_owned())),
        ("idempotency", CValue::Str("none".to_owned())),
        ("lifecycle", obj([("since", CValue::Str(LIFECYCLE_SINCE.to_owned())), ("state", CValue::Str("active".to_owned()))])),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", CValue::Str("exposed".to_owned())),
        ("network", CValue::Str("none".to_owned())),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", CValue::Str("W2".to_owned())),
        ("title", CValue::Str("Evaluate roundness".to_owned())),
        ("undo", CValue::Str("head_move".to_owned())),
        ("web", CValue::Str("available".to_owned())),
    ]))
}
