//! Straightness check command (`GDT.FORM.STRAIGHT.CMD`): `check.eval_straightness@1`
//! (W2, dry-run R1; CLI `fid check eval-straightness`, MCP `check_eval_straightness`),
//! governing-spec gate, `gdt.resolve_chain@1`, chain-driven dispatch (`line_elements` vs
//! `derived_median_line`), `ResultRecord` construction, journal event and exact replay
//! (`AC-1`..`AC-4`).

use crate::chars::dprof::store::parse_principal;
use crate::chars::dprof::{Branch, CharId, DprofState};
use crate::chars::interp::{InterpretationPin, Interpreted, RegisterView, PROVISIONAL_LABEL_TEXT, SIGNED_OFF_LINE};
use crate::chars::record::{
    AlgorithmRef, EvaluationBasis, InputDigests, KernelVersion, Measurand, Quantity, RecordError, ResultRecord,
    ResultRecordBuilder, ResultStore, RunId, Uncertainty,
};
use crate::chars::spec::{SpecError, SpecState};
use crate::dmt_flow::status::Purpose;
use crate::drf::build::ReplayRegisters;
use crate::form::flat::FormPointSets;
use crate::resolve::{resolve_chain, CharacteristicRef, CommandError, CHAR_SCHEMA};
use fid_canon::{CValue, CanonError};
use fid_gps::dmt::DmtTable;
use fid_gps::form::straightness::derived::{
    DerivedToleranceSpec, IdentityMedianLineDeriver, MaterialConditionProvider, MaterialModifier, MedianLineDeriver,
    MzLine3dFitProvider, NoMaterialBonusProvider,
};
use fid_gps::form::straightness::line_elements::Line2dKrnProvider;
use fid_gps::form::straightness::{
    evaluate_straightness_dispatch, StraightnessError, StraightnessPath, StraightnessPayload,
};
use fid_gps::form::{FitProvider, FormFlag};
use fid_gps::resolve::Resolution;
use fid_io_reg::Principal;
use fid_kunc::Uec0Reason;
use fid_registry::{CommandDef, DefError};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Registry command ID for straightness (`GDT.FORM.STRAIGHT.CMD`).
pub const EVAL_STRAIGHTNESS_COMMAND: &str = "check.eval_straightness@1";
/// CLI subcommand under `fid check`.
pub const EVAL_STRAIGHTNESS_CLI: &str = "eval-straightness";
/// MCP tool name.
pub const EVAL_STRAIGHTNESS_MCP: &str = "check_eval_straightness";
/// Schema tag of the command result JSON object.
pub const RESULT_SCHEMA: &str = "fid.check-straightness-result/1";
/// Record unit: SI metres.
pub const UNIT: &str = "si:m";
/// Maximum MCP parameter payload in bytes.
pub const MAX_PARAMS_BYTES: usize = 4 * 1024;
const MAX_PARAMS_DEPTH: usize = 3;
const MAX_CLI_ARGS: usize = 16;
const MAX_ID_LEN: usize = 128;
const MAX_EVENT_BYTES: usize = 16 * 1024;
const MAX_EVENT_DEPTH: usize = 3;
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

fn parse_path(s: &str) -> Option<StraightnessPath> {
    match s {
        "line_elements" => Some(StraightnessPath::LineElements),
        "derived_median_line" => Some(StraightnessPath::DerivedMedianLine),
        _ => None,
    }
}

fn parse_flag(s: &str) -> Option<FormFlag> {
    match s {
        "conservative_upper_bound" => Some(FormFlag::ConservativeUpperBound),
        "local_optimum_only" => Some(FormFlag::LocalOptimumOnly),
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

// ------------------------------------------------------------------ errors

/// Typed errors of `check.eval_straightness@1` (`REQ-002`: errors journal nothing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    BadParams { reason: &'static str },
    Chain(CommandError),
    ChainNotResolved { outcome: &'static str },
    PointsMissing { handle: String },
    Straightness(StraightnessError),
    Record(RecordError),
}

impl EvalError {
    pub fn code(&self) -> &'static str {
        match self {
            EvalError::BadParams { .. } => "bad_params",
            EvalError::Chain(e) => e.code(),
            EvalError::ChainNotResolved { outcome } => outcome,
            EvalError::PointsMissing { .. } => "points_not_found",
            EvalError::Straightness(e) => e.code(),
            EvalError::Record(e) => e.code(),
        }
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::BadParams { reason } => write!(f, "bad_params: {reason}"),
            EvalError::Chain(e) => write!(f, "{e}"),
            EvalError::ChainNotResolved { outcome } => write!(f, "chain resolution returned {outcome}"),
            EvalError::PointsMissing { handle } => write!(f, "point set {handle:?} not found"),
            EvalError::Straightness(e) => write!(f, "{e}"),
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

// ------------------------------------------------------------------ request, inputs, event

/// Request parameters for `check.eval_straightness@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalRequest<'a> {
    pub run: &'a str,
    pub char: &'a str,
    pub points: &'a str,
    pub variant: Option<&'a str>,
}

/// Dependencies required to evaluate `check.eval_straightness@1`.
pub struct EvalInputs<'a> {
    pub spec: &'a SpecState,
    pub dprof: &'a DprofState,
    pub table: Option<&'a DmtTable>,
    pub register: &'a RegisterView,
    pub point_sets: &'a FormPointSets,
    pub deriver: &'a dyn MedianLineDeriver,
    pub krn_line_el: &'a dyn FitProvider,
    pub krn_derived: &'a dyn FitProvider,
    pub tolerance: DerivedToleranceSpec,
    pub matl: &'a dyn MaterialConditionProvider,
}

impl<'a> EvalInputs<'a> {
    /// Standard providers: exact KNP-1 2D line-element MZ (`Line2dKrnProvider`) and 3D
    /// derived median-line MZ (`MzLine3dFitProvider`), identity deriver, RFS tolerance.
    pub fn with_defaults(
        spec: &'a SpecState,
        dprof: &'a DprofState,
        table: Option<&'a DmtTable>,
        register: &'a RegisterView,
        point_sets: &'a FormPointSets,
    ) -> Self {
        static DERIVER: IdentityMedianLineDeriver = IdentityMedianLineDeriver;
        static KRN_2D: Line2dKrnProvider = Line2dKrnProvider;
        static KRN_3D: MzLine3dFitProvider = MzLine3dFitProvider;
        static MATL: NoMaterialBonusProvider = NoMaterialBonusProvider;
        Self {
            spec,
            dprof,
            table,
            register,
            point_sets,
            deriver: &DERIVER,
            krn_line_el: &KRN_2D,
            krn_derived: &KRN_3D,
            tolerance: DerivedToleranceSpec { stated_si: 0.0, modifier: MaterialModifier::Rfs },
            matl: &MATL,
        }
    }
}

/// Journal event payload for `check.eval_straightness@1` (`AC-4` exact replay).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StraightnessEvent {
    pub command: String,
    pub principal: Principal,
    pub branch: Branch,
    pub run: RunId,
    pub char: CharId,
    pub points_handle: String,
    pub variant: Option<String>,
    pub path: StraightnessPath,
    pub determinism: String,
    pub input_digest: Digest,
    pub row: String,
    pub chain_variant: String,
    pub chain_digest: Digest,
    pub dmt_digest: Digest,
    pub pin: InterpretationPin,
    pub algorithm_id: String,
    pub value_bits: u64,
    pub flags: Vec<FormFlag>,
}

impl StraightnessEvent {
    pub fn of(ev: &StraightnessEvaluated) -> Self {
        Self {
            command: ev.command.to_owned(),
            principal: ev.principal,
            branch: ev.branch,
            run: ev.run.clone(),
            char: ev.char.clone(),
            points_handle: ev.points_handle.clone(),
            variant: ev.variant.clone(),
            path: ev.payload.path,
            determinism: ev.payload.determinism.to_owned(),
            input_digest: ev.input_digest,
            row: ev.row.clone(),
            chain_variant: ev.chain_variant.clone(),
            chain_digest: ev.chain_digest,
            dmt_digest: ev.dmt_digest,
            pin: ev.pin,
            algorithm_id: ev.payload.algorithm_id.clone(),
            value_bits: ev.payload.value_si.to_bits(),
            flags: ev.payload.flags.iter().copied().collect(),
        }
    }

    pub fn to_value(&self) -> CValue {
        obj([
            ("algorithm_id", CValue::Str(self.algorithm_id.clone())),
            ("branch", CValue::Str(self.branch.as_str().to_owned())),
            ("chain_digest", CValue::Str(self.chain_digest.as_str().to_owned())),
            ("chain_variant", CValue::Str(self.chain_variant.clone())),
            ("char", CValue::Str(self.char.as_str().to_owned())),
            ("command", CValue::Str(self.command.clone())),
            ("determinism", CValue::Str(self.determinism.clone())),
            ("dmt_digest", CValue::Str(self.dmt_digest.as_str().to_owned())),
            ("flags", CValue::Arr(self.flags.iter().map(|f| CValue::Str(f.as_str().to_owned())).collect())),
            ("input_digest", CValue::Str(self.input_digest.as_str().to_owned())),
            ("path", CValue::Str(self.path.as_str().to_owned())),
            ("pin", self.pin.to_cvalue()),
            ("points", CValue::Str(self.points_handle.clone())),
            ("principal", CValue::Str(principal_str(self.principal).to_owned())),
            ("row", CValue::Str(self.row.clone())),
            ("run", CValue::Str(self.run.as_str().to_owned())),
            ("value_hex", CValue::Str(hex(f64::from_bits(self.value_bits)))),
            ("variant", self.variant.as_deref().map_or(CValue::Null, |v| CValue::Str(v.to_owned()))),
        ])
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, CanonError> {
        fid_canon::to_canonical_bytes(&self.to_value())
    }

    pub fn from_canonical_bytes(raw: &[u8]) -> Result<Self, EvalError> {
        let bad = || EvalError::BadParams { reason: "malformed_event" };
        let val = fid_canon::parse_with_limits(raw, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad())?;
        let CValue::Obj(o) = val else { return Err(bad()) };
        let req_s = |k: &str| -> Result<&str, EvalError> {
            match o.get(k) {
                Some(CValue::Str(s)) => Ok(s.as_str()),
                _ => Err(bad()),
            }
        };
        let command = req_s("command")?.to_owned();
        if command != EVAL_STRAIGHTNESS_COMMAND {
            return Err(bad());
        }
        let principal = parse_principal(req_s("principal")?).ok_or_else(bad)?;
        let branch = Branch::for_principal(principal);
        if req_s("branch")? != branch.as_str() {
            return Err(bad());
        }
        let run = RunId::parse(req_s("run")?).map_err(|_| bad())?;
        let char = CharId::parse(req_s("char")?).ok_or_else(bad)?;
        let points_handle = req_s("points")?.to_owned();
        let variant = match o.get("variant") {
            None | Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(s.clone()),
            _ => return Err(bad()),
        };
        let path = parse_path(req_s("path")?).ok_or_else(bad)?;
        let determinism = req_s("determinism")?.to_owned();
        let input_digest = Digest::parse(req_s("input_digest")?).map_err(|_| bad())?;
        let row = req_s("row")?.to_owned();
        let chain_variant = req_s("chain_variant")?.to_owned();
        let chain_digest = Digest::parse(req_s("chain_digest")?).map_err(|_| bad())?;
        let dmt_digest = Digest::parse(req_s("dmt_digest")?).map_err(|_| bad())?;
        let pin_val = o.get("pin").ok_or_else(bad)?;
        let pin_bytes = fid_canon::to_canonical_bytes(pin_val).map_err(|_| bad())?;
        let pin = InterpretationPin::from_canonical_bytes(&pin_bytes).ok_or_else(bad)?;
        let algorithm_id = req_s("algorithm_id")?.to_owned();
        let value_bits = parse_hex_bits(req_s("value_hex")?)?;
        let CValue::Arr(flag_arr) = o.get("flags").ok_or_else(bad)? else { return Err(bad()) };
        let mut flags = Vec::with_capacity(flag_arr.len());
        for fv in flag_arr {
            let CValue::Str(fs) = fv else { return Err(bad()) };
            flags.push(parse_flag(fs).ok_or_else(bad)?);
        }
        Ok(Self {
            command,
            principal,
            branch,
            run,
            char,
            points_handle,
            variant,
            path,
            determinism,
            input_digest,
            row,
            chain_variant,
            chain_digest,
            dmt_digest,
            pin,
            algorithm_id,
            value_bits,
            flags,
        })
    }
}

// ------------------------------------------------------------------ evaluated result

/// Evaluation result of `check.eval_straightness@1`.
#[derive(Debug, Clone)]
pub struct StraightnessEvaluated {
    pub command: &'static str,
    pub principal: Principal,
    pub branch: Branch,
    pub run: RunId,
    pub char: CharId,
    pub points_handle: String,
    pub variant: Option<String>,
    pub payload: StraightnessPayload,
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
    event: Option<StraightnessEvent>,
}

impl StraightnessEvaluated {
    pub fn event(&self) -> Option<&StraightnessEvent> {
        self.event.as_ref()
    }

    pub fn flags(&self) -> &BTreeSet<FormFlag> {
        &self.payload.flags
    }

    pub fn record(&self, journal_entry: Digest) -> Result<ResultRecord, EvalError> {
        if self.dry_run {
            return Err(EvalError::BadParams { reason: "cannot commit dry_run" });
        }
        Ok(self.builder.clone().journal_entry(journal_entry).finish()?)
    }

    pub fn to_value(&self) -> Result<CValue, EvalError> {
        let straight_val = self.payload.to_cvalue().map_err(|source| {
            EvalError::Straightness(StraightnessError::LineElements(
                fid_gps::form::straightness::line_elements::LineElementError::Form { element: 0, source },
            ))
        })?;
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
            ("interpretation", CValue::Str(self.interpretation.to_owned())),
            ("label", CValue::Str(self.label.to_owned())),
            ("path", CValue::Str(self.payload.path.as_str().to_owned())),
            ("points", CValue::Str(self.points_handle.clone())),
            ("run", CValue::Str(self.run.as_str().to_owned())),
            ("schema", CValue::Str(RESULT_SCHEMA.to_owned())),
            ("straightness", straight_val),
            ("unit", CValue::Str(UNIT.to_owned())),
        ]))
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, EvalError> {
        let v = self.to_value()?;
        fid_canon::to_canonical_bytes(&v).map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })
    }
}

fn chain_characteristic(char: &CharId, variant: Option<&str>) -> Result<CharacteristicRef, EvalError> {
    let doc = obj([
        ("char", CValue::Str(char.as_str().to_owned())),
        ("characteristic", CValue::Str("straightness".to_owned())),
        ("modifiers", CValue::Arr(Vec::new())),
        ("override", variant.map_or(CValue::Null, |v| CValue::Str(v.to_owned()))),
        ("schema", CValue::Str(CHAR_SCHEMA.to_owned())),
    ]);
    let bytes =
        fid_canon::to_canonical_bytes(&doc).map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })?;
    CharacteristicRef::parse(&bytes).map_err(EvalError::Chain)
}

/// Evaluates `check.eval_straightness@1` (`REQ-001`..`REQ-003`).
pub fn eval_straightness(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &EvalRequest<'_>,
    dry_run: bool,
) -> Result<StraightnessEvaluated, EvalError> {
    eval_for(store, inp, principal, req, dry_run, Purpose::Evaluate)
}

fn eval_for(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    req: &EvalRequest<'_>,
    dry_run: bool,
    purpose: Purpose,
) -> Result<StraightnessEvaluated, EvalError> {
    let run = RunId::parse(req.run).map_err(|_| EvalError::BadParams { reason: "bad_run_id" })?;
    let char = CharId::parse(req.char).ok_or(EvalError::BadParams { reason: "bad_char_id" })?;
    if !is_id(req.points) {
        return Err(EvalError::BadParams { reason: "bad_points_handle" });
    }
    if let Some(v) = req.variant {
        if !is_id(v) {
            return Err(EvalError::BadParams { reason: "bad_variant" });
        }
    }

    // REQ-002: governing-spec gate and chain resolution first; any error journals nothing.
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
    if row.characteristic() != "straightness" {
        return Err(EvalError::ChainNotResolved { outcome: "UNRESOLVED" });
    }

    let ps =
        inp.point_sets.get(req.points).ok_or_else(|| EvalError::PointsMissing { handle: req.points.to_owned() })?;

    // REQ-001: dispatch purely on what the resolved chain contains.
    let payload = evaluate_straightness_dispatch(
        &ps.points,
        res.chain(),
        inp.deriver,
        inp.krn_line_el,
        inp.krn_derived,
        &inp.tolerance,
        inp.matl,
    )
    .map_err(EvalError::Straightness)?;

    match store.get(principal, &run, &char) {
        Ok(_) => return Err(EvalError::Record(RecordError::DuplicateResult)),
        Err(RecordError::NotFound) => {}
        Err(e) => return Err(EvalError::Record(e)),
    }

    let spec_err = |e: SpecError| EvalError::Chain(CommandError::Spec(e));
    let gs = inp.spec.require_governing_spec(principal, char.as_str(), inp.dprof).map_err(spec_err)?;
    let stored =
        inp.dprof.resolve(Branch::for_principal(principal), gs.defaults_profile()).map_err(|e| spec_err(e.into()))?;
    let measurand = Measurand::parse(&format!("straightness({})", req.points))?;
    let builder = ResultRecordBuilder::new(run.clone(), char.clone())
        .measurand(measurand)
        .evaluation_basis(EvaluationBasis::Points)
        .value(Quantity::new(payload.value_si, UNIT)?)
        .uncertainty(Uncertainty::uec0(Uec0Reason::UMissing))
        .bind_decision(None, None)
        .spec(gs.clone())
        .algorithm(AlgorithmRef::parse(&payload.algorithm_id)?)
        .kernel_version(KernelVersion::parse(env!("CARGO_PKG_VERSION"))?)
        .input_digests(InputDigests::new(vec![ps.digest])?)
        .resolved(&res)
        .interpretation_for(&[row], Some(stored.doc()), inp.register, purpose);
    let trial = builder.clone().journal_entry(Digest::TOMBSTONE).finish()?;
    let pin = *trial.interpretation_pin();
    let interpretation = trial.interpretation().as_str();
    let label = if crate::requires_provisional_label(&trial) { PROVISIONAL_LABEL_TEXT } else { SIGNED_OFF_LINE };

    let mut ev = StraightnessEvaluated {
        command: EVAL_STRAIGHTNESS_COMMAND,
        principal,
        branch: Branch::for_principal(principal),
        run,
        char,
        points_handle: req.points.to_owned(),
        variant: req.variant.map(str::to_owned),
        payload,
        input_digest: ps.digest,
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
        ev.event = Some(StraightnessEvent::of(&ev));
    }
    Ok(ev)
}

/// Commits the `ResultRecord` of a non-dry-run evaluation with its journal entry hash.
pub fn commit(
    store: &mut ResultStore,
    principal: Principal,
    ev: &StraightnessEvaluated,
    journal_entry: Digest,
) -> Result<Digest, EvalError> {
    if principal != ev.principal {
        return Err(EvalError::BadParams { reason: "principal_mismatch" });
    }
    let record = ev.record(journal_entry)?;
    Ok(store.commit(principal, record)?)
}

/// Result of exact replay over a sequence of `check.eval_straightness@1` journal entries (`AC-4`).
#[derive(Debug, Clone)]
pub struct StraightnessReplayed {
    pub outcome: ReplayOutcome,
    pub steps: usize,
    pub store: ResultStore,
}

/// Replays `(entry_hash, principal, canonical_event_bytes)` entries in order (`AC-4`).
pub fn replay(
    inp: &EvalInputs<'_>,
    regs: &ReplayRegisters<'_>,
    entries: &[(Digest, Principal, &[u8])],
) -> StraightnessReplayed {
    let mut store = ResultStore::new();
    let mut steps = 0usize;
    for &(entry_hash, entry_principal, raw) in entries {
        let Ok(recorded) = StraightnessEvent::from_canonical_bytes(raw) else {
            return StraightnessReplayed { outcome: ReplayOutcome::NotReplayable, steps, store };
        };
        if recorded.principal != entry_principal {
            return StraightnessReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        }
        let Ok(register) = RegisterView::for_replay(&recorded.pin, regs.store, regs.sources) else {
            return StraightnessReplayed { outcome: ReplayOutcome::NotReplayable, steps, store };
        };
        let replay_inputs = EvalInputs {
            spec: inp.spec,
            dprof: inp.dprof,
            table: inp.table,
            register: &register,
            point_sets: inp.point_sets,
            deriver: inp.deriver,
            krn_line_el: inp.krn_line_el,
            krn_derived: inp.krn_derived,
            tolerance: inp.tolerance,
            matl: inp.matl,
        };
        let req = EvalRequest {
            run: recorded.run.as_str(),
            char: recorded.char.as_str(),
            points: recorded.points_handle.as_str(),
            variant: recorded.variant.as_deref(),
        };
        let Ok(ev) = eval_for(&store, &replay_inputs, entry_principal, &req, false, Purpose::Replay) else {
            return StraightnessReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        };
        let Some(rebuilt_event) = ev.event() else {
            return StraightnessReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        };
        let Ok(rebuilt_bytes) = rebuilt_event.to_canonical_bytes() else {
            return StraightnessReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        };
        if rebuilt_bytes != raw {
            return StraightnessReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        }
        if commit(&mut store, entry_principal, &ev, entry_hash).is_err() {
            return StraightnessReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        }
        steps += 1;
    }
    StraightnessReplayed { outcome: ReplayOutcome::Identical, steps, store }
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

/// Runs `fid check eval-straightness --run <R> --char <C> --points <H> [--variant <V>] [--dry-run] [--json]`.
pub fn invoke_cli(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    args: &[&str],
) -> Result<StraightnessEvaluated, EvalError> {
    if args.is_empty() || args.len() > MAX_CLI_ARGS || args[0] != EVAL_STRAIGHTNESS_CLI {
        return Err(EvalError::BadParams { reason: "bad_cli_subcommand" });
    }
    let mut run: Option<&str> = None;
    let mut char_id: Option<&str> = None;
    let mut points: Option<&str> = None;
    let mut variant: Option<&str> = None;
    let mut dry_run = false;
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "--json" => {}
            "--dry-run" => dry_run = true,
            "--run" | "--char" | "--points" | "--variant" => {
                let flag = args[i];
                i += 1;
                if i >= args.len() {
                    return Err(EvalError::BadParams { reason: "missing_flag_value" });
                }
                let val = args[i];
                match flag {
                    "--run" => run = Some(val),
                    "--char" => char_id = Some(val),
                    "--points" => points = Some(val),
                    "--variant" => variant = Some(val),
                    _ => {}
                }
            }
            _ => return Err(EvalError::BadParams { reason: "unknown_cli_flag" }),
        }
        i += 1;
    }
    let req = EvalRequest {
        run: run.ok_or(EvalError::BadParams { reason: "missing_run" })?,
        char: char_id.ok_or(EvalError::BadParams { reason: "missing_char" })?,
        points: points.ok_or(EvalError::BadParams { reason: "missing_points" })?,
        variant,
    };
    eval_straightness(store, inp, principal, &req, dry_run)
}

/// Runs MCP tool `check_eval_straightness` from raw canonical JSON parameters.
pub fn invoke_mcp(
    store: &ResultStore,
    inp: &EvalInputs<'_>,
    principal: Principal,
    params_json: &[u8],
) -> Result<StraightnessEvaluated, EvalError> {
    let val = fid_canon::parse_with_limits(params_json, MAX_PARAMS_BYTES, MAX_PARAMS_DEPTH)
        .map_err(|_| EvalError::BadParams { reason: "bad_json" })?;
    let CValue::Obj(o) = val else { return Err(EvalError::BadParams { reason: "params_not_object" }) };
    for k in o.keys() {
        if !matches!(k.as_str(), "run" | "char" | "points" | "variant" | "dry_run") {
            return Err(EvalError::BadParams { reason: "unknown_param" });
        }
    }
    let dry_run = match o.get("dry_run") {
        None | Some(CValue::Null) => false,
        Some(CValue::Bool(b)) => *b,
        Some(_) => return Err(EvalError::BadParams { reason: "dry_run_not_bool" }),
    };
    let req = EvalRequest {
        run: req_str(&o, "run")?,
        char: req_str(&o, "char")?,
        points: req_str(&o, "points")?,
        variant: opt_str(&o, "variant")?,
    };
    eval_straightness(store, inp, principal, &req, dry_run)
}

/// Registry [`CommandDef`] for `check.eval_straightness@1` (W2, dry-run R1, D3).
pub fn command_def() -> Result<CommandDef, DefError> {
    let id_prop = |desc: &str| {
        obj([
            ("description", CValue::Str(desc.to_owned())),
            ("pattern", CValue::Str(ID_PATTERN.to_owned())),
            ("type", CValue::Str("string".to_owned())),
        ])
    };
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("char", id_prop("Characteristic identifier.")),
                (
                    "dry_run",
                    obj([
                        ("description", CValue::Str("If true, evaluate as R1 without journaling.".to_owned())),
                        ("type", CValue::Str("boolean".to_owned())),
                    ]),
                ),
                ("points", id_prop("Point-set handle.")),
                ("run", id_prop("Measurement run identifier.")),
                ("variant", id_prop("Optional DMT operator-chain variant.")),
            ]),
        ),
        (
            "required",
            CValue::Arr(vec![
                CValue::Str("char".to_owned()),
                CValue::Str("points".to_owned()),
                CValue::Str("run".to_owned()),
            ]),
        ),
        ("type", CValue::Str("object".to_owned())),
    ]);
    let result_schema = obj([
        ("additionalProperties", CValue::Bool(true)),
        (
            "properties",
            obj([
                (
                    "evaluation_basis",
                    obj([
                        ("enum", CValue::Arr(vec![CValue::Str("POINTS".to_owned())])),
                        ("type", CValue::Str("string".to_owned())),
                    ]),
                ),
                (
                    "path",
                    obj([
                        (
                            "enum",
                            CValue::Arr(vec![
                                CValue::Str("line_elements".to_owned()),
                                CValue::Str("derived_median_line".to_owned()),
                            ]),
                        ),
                        ("type", CValue::Str("string".to_owned())),
                    ]),
                ),
                ("straightness", obj([("type", CValue::Str("object".to_owned()))])),
            ]),
        ),
        (
            "required",
            CValue::Arr(vec![
                CValue::Str("evaluation_basis".to_owned()),
                CValue::Str("path".to_owned()),
                CValue::Str("straightness".to_owned()),
            ]),
        ),
        ("type", CValue::Str("object".to_owned())),
    ]);

    CommandDef::from_value(obj([
        ("algorithms", CValue::Arr(Vec::new())),
        ("cli_bare", CValue::Bool(false)),
        (
            "description",
            CValue::Str(
                "Evaluates straightness via governing-spec gate and DMT chain resolution, dispatching to \
                 line_elements or derived_median_line by the resolved chain's operators."
                    .to_owned(),
            ),
        ),
        ("determinism", CValue::Str("D3".to_owned())),
        ("effects", CValue::Arr(vec![CValue::Str(EVAL_STRAIGHTNESS_COMMAND.to_owned())])),
        ("exposure", CValue::Arr(vec![CValue::Str("cli".to_owned()), CValue::Str("mcp".to_owned())])),
        ("fs_effect", CValue::Str("none".to_owned())),
        ("human_only", CValue::Bool(false)),
        ("id", CValue::Str("check.eval_straightness".to_owned())),
        ("idempotency", CValue::Str("none".to_owned())),
        (
            "lifecycle",
            obj([("since", CValue::Str(LIFECYCLE_SINCE.to_owned())), ("state", CValue::Str("active".to_owned()))]),
        ),
        ("major", CValue::Num(1.0)),
        ("mcp_projection", CValue::Str("exposed".to_owned())),
        ("network", CValue::Str("none".to_owned())),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", CValue::Str("W2".to_owned())),
        ("title", CValue::Str("Evaluate straightness".to_owned())),
        ("undo", CValue::Str("head_move".to_owned())),
        ("web", CValue::Str("available".to_owned())),
    ]))
}
