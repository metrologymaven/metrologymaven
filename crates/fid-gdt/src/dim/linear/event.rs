//! Journal event and exact replay for `check.eval_distance@1` and `check.eval_caliper@1`
//! (`GDT.DIM.LINEAR`, `AC-7`).

use super::{
    commit, eval_caliper_for, eval_distance_for, hex, is_id, obj, CaliperRequest, DistanceMeasurandSpec,
    DistanceRequest, EvalError, EvalInputs, Evaluated, FrameAxis, LinearOutcome, EVAL_CALIPER_COMMAND,
    EVAL_DISTANCE_COMMAND,
};
use crate::chars::dprof::{Branch, CharId};
use crate::chars::interp::{InterpretationPin, RegisterView};
use crate::chars::record::{EvaluationBasis, ResultStore, RunId};
use crate::dmt_flow::status::Purpose;
use crate::drf::build::ReplayRegisters;
use fid_canon::CValue;
use fid_gps::dim::linear::{DistanceFlag, LengthUnit, ReferenceSegment};
use fid_io_reg::Principal;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;

const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_EVENT_DEPTH: usize = 16;

/// Event-specific parameters and result details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinearEventKind {
    Distance {
        a: String,
        b: String,
        measurand: DistanceMeasurandSpec,
        segment_start_bits: [u64; 3],
        segment_end_bits: [u64; 3],
        non_parallel_angle_bits: Option<u64>,
    },
    Caliper {
        points: String,
        direction_bits: [u64; 3],
        min_index: u64,
        max_index: u64,
    },
}

/// One committed `check.eval_distance@1` or `check.eval_caliper@1` journal event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearEvent {
    pub command: &'static str,
    pub principal: Principal,
    pub branch: Branch,
    pub run: RunId,
    pub char: CharId,
    pub variant: Option<String>,
    pub basis: EvaluationBasis,
    pub authored_unit: LengthUnit,
    pub value_si_bits: u64,
    pub authored_value_bits: u64,
    pub authored_decimal: String,
    pub conversion_factor: String,
    pub kind: LinearEventKind,
    pub input_digests: Vec<Digest>,
    pub row: String,
    pub chain_variant: String,
    pub chain_digest: Digest,
    pub dmt_digest: Digest,
    pub interpretation: String,
    pub pin: InterpretationPin,
}

impl LinearEvent {
    pub(crate) fn of(ev: &Evaluated) -> LinearEvent {
        let (authored_unit, value_si_bits, authored_value_bits, authored_decimal, conversion_factor, kind) =
            match &ev.outcome {
                LinearOutcome::Distance { a, b, measurand, value } => {
                    let ReferenceSegment { start, end } = value.reference_segment;
                    let non_parallel_angle_bits = value.flags.first().map(|f| match f {
                        DistanceFlag::NonParallelPlanes { angle_rad } => angle_rad.to_bits(),
                    });
                    (
                        value.authored_unit,
                        value.value_si.to_bits(),
                        value.value_authored.to_bits(),
                        value.reported.value.clone(),
                        value.reported.exact_conversion.factor.to_owned(),
                        LinearEventKind::Distance {
                            a: a.clone(),
                            b: b.clone(),
                            measurand: measurand.clone(),
                            segment_start_bits: [start[0].to_bits(), start[1].to_bits(), start[2].to_bits()],
                            segment_end_bits: [end[0].to_bits(), end[1].to_bits(), end[2].to_bits()],
                            non_parallel_angle_bits,
                        },
                    )
                }
                LinearOutcome::Caliper { points, direction, value } => (
                    value.authored_unit,
                    value.value_si.to_bits(),
                    value.value_authored.to_bits(),
                    value.reported.value.clone(),
                    value.reported.exact_conversion.factor.to_owned(),
                    LinearEventKind::Caliper {
                        points: points.clone(),
                        direction_bits: [direction[0].to_bits(), direction[1].to_bits(), direction[2].to_bits()],
                        min_index: value.min_index,
                        max_index: value.max_index,
                    },
                ),
            };
        LinearEvent {
            command: ev.command,
            principal: ev.principal,
            branch: ev.branch,
            run: ev.run.clone(),
            char: ev.char.clone(),
            variant: ev.variant.clone(),
            basis: ev.basis,
            authored_unit,
            value_si_bits,
            authored_value_bits,
            authored_decimal,
            conversion_factor,
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
        m.insert("authored_decimal".to_owned(), CValue::Str(self.authored_decimal.clone()));
        m.insert("authored_unit".to_owned(), CValue::Str(self.authored_unit.as_str().to_owned()));
        m.insert("authored_value".to_owned(), CValue::Str(format!("0x{:016x}", self.authored_value_bits)));
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
        m.insert("conversion_factor".to_owned(), CValue::Str(self.conversion_factor.clone()));
        m.insert("evaluation_basis".to_owned(), CValue::Str(self.basis.wire().to_owned()));
        m.insert(
            "input_digests".to_owned(),
            CValue::Arr(self.input_digests.iter().map(|d| CValue::Str(d.as_str().to_owned())).collect()),
        );
        m.insert("interpretation".to_owned(), CValue::Str(self.interpretation.clone()));
        m.insert("interpretation_pin".to_owned(), self.pin.to_cvalue());
        m.insert("principal".to_owned(), CValue::Str(principal_str(self.principal).to_owned()));
        m.insert("run".to_owned(), CValue::Str(self.run.as_str().to_owned()));
        m.insert("value_si".to_owned(), CValue::Str(format!("0x{:016x}", self.value_si_bits)));
        m.insert("variant".to_owned(), self.variant.as_ref().map_or(CValue::Null, |v| CValue::Str(v.clone())));
        match &self.kind {
            LinearEventKind::Distance {
                a,
                b,
                measurand,
                segment_start_bits,
                segment_end_bits,
                non_parallel_angle_bits,
            } => {
                m.insert("a".to_owned(), CValue::Str(a.clone()));
                m.insert("b".to_owned(), CValue::Str(b.clone()));
                m.insert("measurand".to_owned(), measurand.to_value());
                m.insert(
                    "non_parallel_angle_rad".to_owned(),
                    non_parallel_angle_bits.map_or(CValue::Null, |bits| CValue::Str(format!("0x{bits:016x}"))),
                );
                m.insert(
                    "reference_segment".to_owned(),
                    obj([("end", bits3_val(*segment_end_bits)), ("start", bits3_val(*segment_start_bits))]),
                );
            }
            LinearEventKind::Caliper { points, direction_bits, min_index, max_index } => {
                m.insert("direction".to_owned(), bits3_val(*direction_bits));
                m.insert(
                    "extreme_indices".to_owned(),
                    CValue::Arr(vec![CValue::Num(*min_index as f64), CValue::Num(*max_index as f64)]),
                );
                m.insert("points".to_owned(), CValue::Str(points.clone()));
            }
        }
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes of this event.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, EvalError> {
        fid_canon::to_canonical_bytes(&self.to_value())
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })
    }

    /// Strictly parses a [`LinearEvent`] from RFC 8785 canonical JSON bytes.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<LinearEvent, EvalError> {
        let bad = || EvalError::BadParams { reason: "malformed_event" };
        let v = fid_canon::parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad())?;
        let CValue::Obj(o) = &v else { return Err(bad()) };
        let command = match req_str(o, "command")? {
            EVAL_DISTANCE_COMMAND => EVAL_DISTANCE_COMMAND,
            EVAL_CALIPER_COMMAND => EVAL_CALIPER_COMMAND,
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
        let authored_unit = LengthUnit::parse(req_str(o, "authored_unit")?).ok_or_else(bad)?;
        let value_si_bits = parse_hex_bits(req_str(o, "value_si")?)?;
        let authored_value_bits = parse_hex_bits(req_str(o, "authored_value")?)?;
        let authored_decimal = req_str(o, "authored_decimal")?.to_owned();
        let conversion_factor = req_str(o, "conversion_factor")?.to_owned();
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

        let kind = if command == EVAL_DISTANCE_COMMAND {
            let a = req_str(o, "a")?.to_owned();
            let b = req_str(o, "b")?.to_owned();
            if !is_id(&a) || !is_id(&b) {
                return Err(bad());
            }
            let measurand = parse_measurand(o.get("measurand").ok_or_else(bad)?)?;
            let Some(CValue::Obj(seg)) = o.get("reference_segment") else { return Err(bad()) };
            let segment_start_bits = parse_bits3(seg.get("start").ok_or_else(bad)?)?;
            let segment_end_bits = parse_bits3(seg.get("end").ok_or_else(bad)?)?;
            let non_parallel_angle_bits = match o.get("non_parallel_angle_rad") {
                Some(CValue::Null) => None,
                Some(CValue::Str(s)) => Some(parse_hex_bits(s)?),
                _ => return Err(bad()),
            };
            LinearEventKind::Distance { a, b, measurand, segment_start_bits, segment_end_bits, non_parallel_angle_bits }
        } else {
            let points = req_str(o, "points")?.to_owned();
            if !is_id(&points) {
                return Err(bad());
            }
            let direction_bits = parse_bits3(o.get("direction").ok_or_else(bad)?)?;
            let Some(CValue::Arr(idx)) = o.get("extreme_indices") else { return Err(bad()) };
            if idx.len() != 2 {
                return Err(bad());
            }
            let min_index = parse_u64_num(&idx[0])?;
            let max_index = parse_u64_num(&idx[1])?;
            LinearEventKind::Caliper { points, direction_bits, min_index, max_index }
        };

        Ok(LinearEvent {
            command,
            principal,
            branch,
            run,
            char,
            variant,
            basis,
            authored_unit,
            value_si_bits,
            authored_value_bits,
            authored_decimal,
            conversion_factor,
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

/// Result of exact replay over a sequence of `GDT.DIM.LINEAR` journal entries (`AC-7`).
#[derive(Debug, Clone)]
pub struct LinearReplayed {
    pub outcome: ReplayOutcome,
    pub steps: usize,
    pub store: ResultStore,
}

/// Replays `(entry_hash, principal, canonical_event_bytes)` entries in order (`AC-7`).
pub fn replay(
    inp: &EvalInputs<'_>,
    regs: &ReplayRegisters<'_>,
    entries: &[(Digest, Principal, &[u8])],
) -> LinearReplayed {
    let mut results = ResultStore::new();
    let mut steps = 0usize;
    for &(entry_hash, entry_principal, raw) in entries {
        let Ok(recorded) = LinearEvent::from_canonical_bytes(raw) else {
            return LinearReplayed { outcome: ReplayOutcome::NotReplayable, steps, store: results };
        };
        if recorded.principal != entry_principal {
            return LinearReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        }
        let Ok(register) = RegisterView::for_replay(&recorded.pin, regs.store, regs.sources) else {
            return LinearReplayed { outcome: ReplayOutcome::NotReplayable, steps, store: results };
        };
        let replay_inputs =
            EvalInputs { spec: inp.spec, dprof: inp.dprof, table: inp.table, store: inp.store, register: &register };
        let ev_res = match &recorded.kind {
            LinearEventKind::Distance { a, b, measurand, .. } => {
                let req = DistanceRequest {
                    run: recorded.run.as_str(),
                    char: recorded.char.as_str(),
                    a: a.as_str(),
                    b: b.as_str(),
                    measurand: Some(measurand.clone()),
                    authored_unit: recorded.authored_unit,
                    variant: recorded.variant.as_deref(),
                };
                eval_distance_for(&results, &replay_inputs, entry_principal, &req, false, Purpose::Replay)
            }
            LinearEventKind::Caliper { points, direction_bits, .. } => {
                let direction = [
                    f64::from_bits(direction_bits[0]),
                    f64::from_bits(direction_bits[1]),
                    f64::from_bits(direction_bits[2]),
                ];
                let req = CaliperRequest {
                    run: recorded.run.as_str(),
                    char: recorded.char.as_str(),
                    points: points.as_str(),
                    direction,
                    authored_unit: recorded.authored_unit,
                    variant: recorded.variant.as_deref(),
                };
                eval_caliper_for(&results, &replay_inputs, entry_principal, &req, false, Purpose::Replay)
            }
        };
        let Ok(ev) = ev_res else {
            return LinearReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        };
        let Some(rebuilt_event) = ev.event() else {
            return LinearReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        };
        let Ok(rebuilt_bytes) = rebuilt_event.to_canonical_bytes() else {
            return LinearReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        };
        if rebuilt_bytes != raw {
            return LinearReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        }
        if commit(&mut results, entry_principal, &ev, entry_hash).is_err() {
            return LinearReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        }
        steps += 1;
    }
    LinearReplayed { outcome: ReplayOutcome::Identical, steps, store: results }
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

fn bits3_val(b: [u64; 3]) -> CValue {
    CValue::Arr(vec![
        CValue::Str(hex(f64::from_bits(b[0]))),
        CValue::Str(hex(f64::from_bits(b[1]))),
        CValue::Str(hex(f64::from_bits(b[2]))),
    ])
}

fn parse_bits3(v: &CValue) -> Result<[u64; 3], EvalError> {
    let bad = || EvalError::BadParams { reason: "malformed_event" };
    let CValue::Arr(a) = v else { return Err(bad()) };
    if a.len() != 3 {
        return Err(bad());
    }
    let get = |x: &CValue| match x {
        CValue::Str(s) => parse_hex_bits(s),
        _ => Err(bad()),
    };
    Ok([get(&a[0])?, get(&a[1])?, get(&a[2])?])
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

fn parse_measurand(v: &CValue) -> Result<DistanceMeasurandSpec, EvalError> {
    let bad = || EvalError::BadParams { reason: "malformed_event" };
    let CValue::Obj(o) = v else { return Err(bad()) };
    match req_str(o, "kind")? {
        "3d" => Ok(DistanceMeasurandSpec::ThreeD),
        "projected" => {
            let plane = req_str(o, "plane")?.to_owned();
            if !is_id(&plane) {
                return Err(bad());
            }
            Ok(DistanceMeasurandSpec::Projected { plane })
        }
        "component" => {
            let frame = req_str(o, "frame")?.to_owned();
            if !is_id(&frame) {
                return Err(bad());
            }
            let axis = FrameAxis::parse(req_str(o, "axis")?).ok_or_else(bad)?;
            Ok(DistanceMeasurandSpec::Component { frame, axis })
        }
        _ => Err(bad()),
    }
}
