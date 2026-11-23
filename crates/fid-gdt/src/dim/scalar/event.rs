//! Journal event and exact replay for `check.eval_scalar@1` and `check.eval_minmax@1`
//! (`GDT.DIM.SCALAR`, `AC-6`).

use super::{
    commit, eval_minmax_for, eval_scalar_for, is_id, obj, EvalError, EvalInputs, Evaluated, MinMaxRequest,
    ScalarOutcome, ScalarRequest, EVAL_MINMAX_COMMAND, EVAL_SCALAR_COMMAND,
};
use crate::chars::dprof::{Branch, CharId};
use crate::chars::interp::{InterpretationPin, RegisterView};
use crate::chars::record::{EvaluationBasis, ResultStore, RunId};
use crate::dmt_flow::status::Purpose;
use crate::drf::build::ReplayRegisters;
use fid_canon::CValue;
use fid_gps::dim::scalar::{MinMaxStat, QuantityKind};
use fid_io_reg::Principal;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;

const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_EVENT_DEPTH: usize = 16;

/// Event-specific parameters and result payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScalarEventKind {
    Scalar {
        element: String,
        property: String,
        tolerance_unit: Option<String>,
    },
    MinMax {
        values: String,
        statistic: MinMaxStat,
        min_si_bits: u64,
        max_si_bits: u64,
        range_si_bits: u64,
        min_index: u64,
        max_index: u64,
    },
}

/// One committed `check.eval_scalar@1` or `check.eval_minmax@1` journal event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarEvent {
    pub command: &'static str,
    pub principal: Principal,
    pub branch: Branch,
    pub run: RunId,
    pub char: CharId,
    pub variant: Option<String>,
    pub basis: EvaluationBasis,
    pub quantity_kind: QuantityKind,
    pub value_si_bits: u64,
    pub kind: ScalarEventKind,
    pub input_digests: Vec<Digest>,
    pub row: String,
    pub chain_variant: String,
    pub chain_digest: Digest,
    pub dmt_digest: Digest,
    pub interpretation: String,
    pub pin: InterpretationPin,
}

impl ScalarEvent {
    pub(crate) fn of(ev: &Evaluated) -> ScalarEvent {
        let (quantity_kind, value_si_bits, kind) = match &ev.outcome {
            ScalarOutcome::Scalar { element, property, tolerance_unit, value } => (
                value.quantity_kind,
                value.value_si.to_bits(),
                ScalarEventKind::Scalar {
                    element: element.clone(),
                    property: property.clone(),
                    tolerance_unit: tolerance_unit.clone(),
                },
            ),
            ScalarOutcome::MinMax { values, value } => (
                value.quantity_kind,
                value.value_si.to_bits(),
                ScalarEventKind::MinMax {
                    values: values.clone(),
                    statistic: value.statistic,
                    min_si_bits: value.min_si.to_bits(),
                    max_si_bits: value.max_si.to_bits(),
                    range_si_bits: value.range_si.to_bits(),
                    min_index: value.min_index,
                    max_index: value.max_index,
                },
            ),
        };
        ScalarEvent {
            command: ev.command,
            principal: ev.principal,
            branch: ev.branch,
            run: ev.run.clone(),
            char: ev.char.clone(),
            variant: ev.variant.clone(),
            basis: ev.basis,
            quantity_kind,
            value_si_bits,
            kind,
            input_digests: ev.input_digests.clone(),
            row: ev.row.clone(),
            chain_variant: ev.chain_variant.clone(),
            chain_digest: ev.chain_digest,
            dmt_digest: ev.dmt_digest,
            interpretation: ev.interpretation.to_owned(),
            pin: ev.pin,
        }
    }

    /// Canonical `CValue` representation of this journal event.
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("branch".to_owned(), CValue::Str(self.branch.as_str().to_owned()));
        m.insert(
            "chain".to_owned(),
            obj([
                ("chain_digest", CValue::Str(self.chain_digest.as_str().to_owned())),
                ("dmt_digest", CValue::Str(self.dmt_digest.as_str().to_owned())),
                ("row", CValue::Str(self.row.clone())),
                ("variant", CValue::Str(self.chain_variant.clone())),
            ]),
        );
        m.insert("char".to_owned(), CValue::Str(self.char.as_str().to_owned()));
        m.insert("command".to_owned(), CValue::Str(self.command.to_owned()));
        m.insert("evaluation_basis".to_owned(), CValue::Str(self.basis.wire().to_owned()));
        m.insert(
            "input_digests".to_owned(),
            CValue::Arr(self.input_digests.iter().map(|d| CValue::Str(d.as_str().to_owned())).collect()),
        );
        m.insert("interpretation".to_owned(), CValue::Str(self.interpretation.clone()));
        m.insert("interpretation_pin".to_owned(), self.pin.to_cvalue());
        m.insert("principal".to_owned(), CValue::Str(principal_str(self.principal).to_owned()));
        m.insert("quantity_kind".to_owned(), CValue::Str(self.quantity_kind.as_str().to_owned()));
        m.insert("run".to_owned(), CValue::Str(self.run.as_str().to_owned()));
        m.insert("value_si".to_owned(), CValue::Str(format!("0x{:016x}", self.value_si_bits)));
        m.insert("variant".to_owned(), self.variant.as_ref().map_or(CValue::Null, |v| CValue::Str(v.clone())));
        match &self.kind {
            ScalarEventKind::Scalar { element, property, tolerance_unit } => {
                m.insert("element".to_owned(), CValue::Str(element.clone()));
                m.insert("property".to_owned(), CValue::Str(property.clone()));
                m.insert(
                    "tolerance_unit".to_owned(),
                    tolerance_unit.as_ref().map_or(CValue::Null, |u| CValue::Str(u.clone())),
                );
            }
            ScalarEventKind::MinMax {
                values,
                statistic,
                min_si_bits,
                max_si_bits,
                range_si_bits,
                min_index,
                max_index,
            } => {
                m.insert("max_index".to_owned(), CValue::Num(*max_index as f64));
                m.insert("max_si".to_owned(), CValue::Str(format!("0x{max_si_bits:016x}")));
                m.insert("min_index".to_owned(), CValue::Num(*min_index as f64));
                m.insert("min_si".to_owned(), CValue::Str(format!("0x{min_si_bits:016x}")));
                m.insert("range_si".to_owned(), CValue::Str(format!("0x{range_si_bits:016x}")));
                m.insert("statistic".to_owned(), CValue::Str(statistic.as_str().to_owned()));
                m.insert("values".to_owned(), CValue::Str(values.clone()));
            }
        }
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes of this event.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, EvalError> {
        fid_canon::to_canonical_bytes(&self.to_value())
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })
    }

    /// Strictly parses a [`ScalarEvent`] from RFC 8785 canonical JSON bytes.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<ScalarEvent, EvalError> {
        let bad = || EvalError::BadParams { reason: "malformed_event" };
        let v = fid_canon::parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad())?;
        let CValue::Obj(o) = &v else { return Err(bad()) };
        let command = match req_str(o, "command")? {
            EVAL_SCALAR_COMMAND => EVAL_SCALAR_COMMAND,
            EVAL_MINMAX_COMMAND => EVAL_MINMAX_COMMAND,
            _ => return Err(bad()),
        };
        let principal = parse_principal(req_str(o, "principal")?).ok_or_else(bad)?;
        let branch = Branch::for_principal(principal);
        if req_str(o, "branch")? != branch.as_str() {
            return Err(bad());
        }
        let run = RunId::parse(req_str(o, "run")?).map_err(|_| bad())?;
        let char = CharId::parse(req_str(o, "char")?).ok_or_else(bad)?;
        let variant = opt_str(o, "variant")?;
        if let Some(vr) = &variant {
            if !is_id(vr) {
                return Err(bad());
            }
        }
        let basis = EvaluationBasis::from_wire(req_str(o, "evaluation_basis")?).ok_or_else(bad)?;
        let quantity_kind = QuantityKind::parse(req_str(o, "quantity_kind")?).ok_or_else(bad)?;
        let value_si_bits = parse_hex_bits(req_str(o, "value_si")?)?;
        let Some(CValue::Arr(raw_digests)) = o.get("input_digests") else { return Err(bad()) };
        let mut input_digests = Vec::with_capacity(raw_digests.len());
        for d in raw_digests {
            let CValue::Str(ds) = d else { return Err(bad()) };
            input_digests.push(Digest::parse(ds).map_err(|_| bad())?);
        }
        let Some(CValue::Obj(ch)) = o.get("chain") else { return Err(bad()) };
        let row = req_str(ch, "row")?.to_owned();
        let chain_variant = req_str(ch, "variant")?.to_owned();
        let chain_digest = Digest::parse(req_str(ch, "chain_digest")?).map_err(|_| bad())?;
        let dmt_digest = Digest::parse(req_str(ch, "dmt_digest")?).map_err(|_| bad())?;
        let interpretation = req_str(o, "interpretation")?.to_owned();
        let pin_bytes =
            fid_canon::to_canonical_bytes(o.get("interpretation_pin").ok_or_else(bad)?).map_err(|_| bad())?;
        let pin = InterpretationPin::from_canonical_bytes(&pin_bytes).ok_or_else(bad)?;

        let kind = if command == EVAL_SCALAR_COMMAND {
            let element = req_str(o, "element")?.to_owned();
            if !is_id(&element) {
                return Err(bad());
            }
            let property = req_str(o, "property")?.to_owned();
            let tolerance_unit = opt_str(o, "tolerance_unit")?;
            ScalarEventKind::Scalar { element, property, tolerance_unit }
        } else {
            let values = req_str(o, "values")?.to_owned();
            if !is_id(&values) {
                return Err(bad());
            }
            let statistic = MinMaxStat::parse(req_str(o, "statistic")?).ok_or_else(bad)?;
            let min_si_bits = parse_hex_bits(req_str(o, "min_si")?)?;
            let max_si_bits = parse_hex_bits(req_str(o, "max_si")?)?;
            let range_si_bits = parse_hex_bits(req_str(o, "range_si")?)?;
            let min_index = parse_u64_num(o.get("min_index").ok_or_else(bad)?)?;
            let max_index = parse_u64_num(o.get("max_index").ok_or_else(bad)?)?;
            ScalarEventKind::MinMax { values, statistic, min_si_bits, max_si_bits, range_si_bits, min_index, max_index }
        };

        Ok(ScalarEvent {
            command,
            principal,
            branch,
            run,
            char,
            variant,
            basis,
            quantity_kind,
            value_si_bits,
            kind,
            input_digests,
            row,
            chain_variant,
            chain_digest,
            dmt_digest,
            interpretation,
            pin,
        })
    }
}

/// Result of exact replay over a sequence of `GDT.DIM.SCALAR` journal entries (`AC-6`).
#[derive(Debug, Clone)]
pub struct ScalarReplayed {
    pub outcome: ReplayOutcome,
    pub steps: usize,
    pub store: ResultStore,
}

/// Replays `(entry_hash, principal, canonical_event_bytes)` entries in order (`AC-6`).
pub fn replay(
    inp: &EvalInputs<'_>,
    regs: &ReplayRegisters<'_>,
    entries: &[(Digest, Principal, &[u8])],
) -> ScalarReplayed {
    let mut results = ResultStore::new();
    let mut steps = 0usize;
    for &(entry_hash, entry_principal, raw) in entries {
        let Ok(recorded) = ScalarEvent::from_canonical_bytes(raw) else {
            return ScalarReplayed { outcome: ReplayOutcome::NotReplayable, steps, store: results };
        };
        if recorded.principal != entry_principal {
            return ScalarReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        }
        let Ok(register) = RegisterView::for_replay(&recorded.pin, regs.store, regs.sources) else {
            return ScalarReplayed { outcome: ReplayOutcome::NotReplayable, steps, store: results };
        };
        let replay_inputs =
            EvalInputs { spec: inp.spec, dprof: inp.dprof, table: inp.table, store: inp.store, register: &register };
        let ev_res = match &recorded.kind {
            ScalarEventKind::Scalar { element, property, tolerance_unit } => {
                let req = ScalarRequest {
                    run: recorded.run.as_str(),
                    char: recorded.char.as_str(),
                    element: element.as_str(),
                    property: property.as_str(),
                    tolerance_unit: tolerance_unit.as_deref(),
                    variant: recorded.variant.as_deref(),
                };
                eval_scalar_for(&results, &replay_inputs, entry_principal, &req, false, Purpose::Replay)
            }
            ScalarEventKind::MinMax { values, statistic, .. } => {
                let req = MinMaxRequest {
                    run: recorded.run.as_str(),
                    char: recorded.char.as_str(),
                    values: values.as_str(),
                    statistic: *statistic,
                    variant: recorded.variant.as_deref(),
                };
                eval_minmax_for(&results, &replay_inputs, entry_principal, &req, false, Purpose::Replay)
            }
        };
        let Ok(ev) = ev_res else {
            return ScalarReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        };
        let Some(rebuilt_event) = ev.event() else {
            return ScalarReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        };
        let Ok(rebuilt_bytes) = rebuilt_event.to_canonical_bytes() else {
            return ScalarReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        };
        if rebuilt_bytes != raw {
            return ScalarReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        }
        if commit(&mut results, entry_principal, &ev, entry_hash).is_err() {
            return ScalarReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        }
        steps += 1;
    }
    ScalarReplayed { outcome: ReplayOutcome::Identical, steps, store: results }
}

fn principal_str(p: Principal) -> &'static str {
    match p {
        Principal::Human => "human",
        Principal::Agent => "agent",
        Principal::Script => "script",
    }
}

fn parse_principal(s: &str) -> Option<Principal> {
    match s {
        "human" => Some(Principal::Human),
        "agent" => Some(Principal::Agent),
        "script" => Some(Principal::Script),
        _ => None,
    }
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, EvalError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(EvalError::BadParams { reason: "malformed_event" }),
    }
}

fn opt_str(o: &BTreeMap<String, CValue>, k: &str) -> Result<Option<String>, EvalError> {
    match o.get(k) {
        Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        _ => Err(EvalError::BadParams { reason: "malformed_event" }),
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

fn parse_u64_num(v: &CValue) -> Result<u64, EvalError> {
    let bad = || EvalError::BadParams { reason: "malformed_event" };
    let CValue::Num(n) = v else { return Err(bad()) };
    if !n.is_finite() || *n < 0.0 || n.fract() != 0.0 || *n > (1u64 << 53) as f64 {
        return Err(bad());
    }
    Ok(*n as u64)
}
