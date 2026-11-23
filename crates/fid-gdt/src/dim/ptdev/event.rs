//! Journal event and exact replay for `check.eval_deviation@1` (`GDT.DIM.PTDEV`, `AC-5`).

use super::{
    commit, eval_deviation_for, hex, is_id, obj, DeviationOutcome, DeviationRequest, DeviationTarget, EvalError,
    EvalInputs, Evaluated, EVAL_DEVIATION_COMMAND,
};
use crate::chars::dprof::{Branch, CharId};
use crate::chars::interp::{InterpretationPin, RegisterView};
use crate::chars::record::{EvaluationBasis, ResultStore, RunId};
use crate::dmt_flow::status::Purpose;
use crate::drf::build::ReplayRegisters;
use fid_canon::CValue;
use fid_gps::dim::ptdev::SIGN_CONVENTION;
use fid_io_reg::Principal;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;

const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_EVENT_DEPTH: usize = 16;

/// Event-specific summary payload committed to the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtDevEventPayload {
    Points {
        points: String,
        nominals: String,
        count: u64,
        min_dn_bits: u64,
        max_dn_bits: u64,
        max_abs_dn_bits: u64,
        attachment_digest: Option<Digest>,
    },
    Feature {
        feature: String,
        nominal: String,
        location_bits: [u64; 3],
        direction_rad_bits: Option<u64>,
        size_bits: Option<u64>,
    },
}

/// One committed `check.eval_deviation@1` journal event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtDevEvent {
    pub principal: Principal,
    pub branch: Branch,
    pub run: RunId,
    pub char: CharId,
    pub frame: String,
    pub variant: Option<String>,
    pub basis: EvaluationBasis,
    pub value_si_bits: u64,
    pub payload: PtDevEventPayload,
    pub input_digests: Vec<Digest>,
    pub row: String,
    pub chain_variant: String,
    pub chain_digest: Digest,
    pub dmt_digest: Digest,
    pub interpretation: String,
    pub pin: InterpretationPin,
}

impl PtDevEvent {
    pub(crate) fn of(ev: &Evaluated) -> PtDevEvent {
        let (value_si_bits, payload) = match (&ev.target, &ev.outcome) {
            (DeviationTarget::Points { points, nominals }, DeviationOutcome::Points { result, attachment }) => (
                result.max_abs_dn_si.to_bits(),
                PtDevEventPayload::Points {
                    points: points.clone(),
                    nominals: nominals.clone(),
                    count: u64::try_from(result.deviations.len()).unwrap_or(u64::MAX),
                    min_dn_bits: result.min_dn_si.to_bits(),
                    max_dn_bits: result.max_dn_si.to_bits(),
                    max_abs_dn_bits: result.max_abs_dn_si.to_bits(),
                    attachment_digest: attachment.as_ref().map(|a| a.digest),
                },
            ),
            (DeviationTarget::Feature { feature, nominal }, DeviationOutcome::Feature(fd)) => (
                fd.location_norm_si.to_bits(),
                PtDevEventPayload::Feature {
                    feature: feature.clone(),
                    nominal: nominal.clone(),
                    location_bits: [fd.location[0].to_bits(), fd.location[1].to_bits(), fd.location[2].to_bits()],
                    direction_rad_bits: fd.direction_rad.map(f64::to_bits),
                    size_bits: fd.size.map(f64::to_bits),
                },
            ),
            _ => (
                0,
                PtDevEventPayload::Points {
                    points: String::new(),
                    nominals: String::new(),
                    count: 0,
                    min_dn_bits: 0,
                    max_dn_bits: 0,
                    max_abs_dn_bits: 0,
                    attachment_digest: None,
                },
            ),
        };
        PtDevEvent {
            principal: ev.principal,
            branch: ev.branch,
            run: ev.run.clone(),
            char: ev.char.clone(),
            frame: ev.frame.clone(),
            variant: ev.variant.clone(),
            basis: ev.basis,
            value_si_bits,
            payload,
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
        m.insert("command".to_owned(), CValue::Str(EVAL_DEVIATION_COMMAND.to_owned()));
        m.insert("evaluation_basis".to_owned(), CValue::Str(self.basis.wire().to_owned()));
        m.insert("frame".to_owned(), CValue::Str(self.frame.clone()));
        m.insert(
            "input_digests".to_owned(),
            CValue::Arr(self.input_digests.iter().map(|d| CValue::Str(d.as_str().to_owned())).collect()),
        );
        m.insert("interpretation".to_owned(), CValue::Str(self.interpretation.clone()));
        m.insert("interpretation_pin".to_owned(), self.pin.to_cvalue());
        m.insert("principal".to_owned(), CValue::Str(principal_str(self.principal).to_owned()));
        m.insert("run".to_owned(), CValue::Str(self.run.as_str().to_owned()));
        m.insert("sign_convention".to_owned(), CValue::Str(SIGN_CONVENTION.to_owned()));
        m.insert("value_si".to_owned(), CValue::Str(format!("0x{:016x}", self.value_si_bits)));
        m.insert("variant".to_owned(), self.variant.as_ref().map_or(CValue::Null, |v| CValue::Str(v.clone())));
        match &self.payload {
            PtDevEventPayload::Points {
                points,
                nominals,
                count,
                min_dn_bits,
                max_dn_bits,
                max_abs_dn_bits,
                attachment_digest,
            } => {
                m.insert(
                    "attachment_digest".to_owned(),
                    attachment_digest.map_or(CValue::Null, |d| CValue::Str(d.as_str().to_owned())),
                );
                m.insert("count".to_owned(), CValue::Num(*count as f64));
                m.insert("max_abs_dn_si".to_owned(), CValue::Str(format!("0x{max_abs_dn_bits:016x}")));
                m.insert("max_dn_si".to_owned(), CValue::Str(format!("0x{max_dn_bits:016x}")));
                m.insert("min_dn_si".to_owned(), CValue::Str(format!("0x{min_dn_bits:016x}")));
                m.insert("mode".to_owned(), CValue::Str("points".to_owned()));
                m.insert("nominals".to_owned(), CValue::Str(nominals.clone()));
                m.insert("points".to_owned(), CValue::Str(points.clone()));
            }
            PtDevEventPayload::Feature { feature, nominal, location_bits, direction_rad_bits, size_bits } => {
                m.insert(
                    "direction_rad".to_owned(),
                    direction_rad_bits.map_or(CValue::Null, |b| CValue::Str(format!("0x{b:016x}"))),
                );
                m.insert("feature".to_owned(), CValue::Str(feature.clone()));
                m.insert(
                    "location".to_owned(),
                    CValue::Arr(vec![
                        CValue::Str(hex(f64::from_bits(location_bits[0]))),
                        CValue::Str(hex(f64::from_bits(location_bits[1]))),
                        CValue::Str(hex(f64::from_bits(location_bits[2]))),
                    ]),
                );
                m.insert("mode".to_owned(), CValue::Str("feature".to_owned()));
                m.insert("nominal".to_owned(), CValue::Str(nominal.clone()));
                m.insert("size".to_owned(), size_bits.map_or(CValue::Null, |b| CValue::Str(format!("0x{b:016x}"))));
            }
        }
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes of this event.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, EvalError> {
        fid_canon::to_canonical_bytes(&self.to_value())
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })
    }

    /// Strictly parses a [`PtDevEvent`] from RFC 8785 canonical JSON bytes.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<PtDevEvent, EvalError> {
        let bad = || EvalError::BadParams { reason: "malformed_event" };
        let v = fid_canon::parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad())?;
        let CValue::Obj(o) = &v else { return Err(bad()) };
        if req_str(o, "command")? != EVAL_DEVIATION_COMMAND {
            return Err(bad());
        }
        if req_str(o, "sign_convention")? != SIGN_CONVENTION {
            return Err(bad());
        }
        let principal = parse_principal(req_str(o, "principal")?).ok_or_else(bad)?;
        let branch = Branch::for_principal(principal);
        if req_str(o, "branch")? != branch.as_str() {
            return Err(bad());
        }
        let run = RunId::parse(req_str(o, "run")?).map_err(|_| bad())?;
        let char = CharId::parse(req_str(o, "char")?).ok_or_else(bad)?;
        let frame = req_str(o, "frame")?.to_owned();
        if !is_id(&frame) {
            return Err(bad());
        }
        let variant = opt_str(o, "variant")?;
        if let Some(vr) = &variant {
            if !is_id(vr) {
                return Err(bad());
            }
        }
        let basis = EvaluationBasis::from_wire(req_str(o, "evaluation_basis")?).ok_or_else(bad)?;
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

        let payload = match req_str(o, "mode")? {
            "points" => {
                let points = req_str(o, "points")?.to_owned();
                let nominals = req_str(o, "nominals")?.to_owned();
                if !is_id(&points) || !is_id(&nominals) {
                    return Err(bad());
                }
                let count = parse_u64_num(o.get("count").ok_or_else(bad)?)?;
                let min_dn_bits = parse_hex_bits(req_str(o, "min_dn_si")?)?;
                let max_dn_bits = parse_hex_bits(req_str(o, "max_dn_si")?)?;
                let max_abs_dn_bits = parse_hex_bits(req_str(o, "max_abs_dn_si")?)?;
                let attachment_digest = match o.get("attachment_digest") {
                    Some(CValue::Null) => None,
                    Some(CValue::Str(s)) => Some(Digest::parse(s).map_err(|_| bad())?),
                    _ => return Err(bad()),
                };
                PtDevEventPayload::Points {
                    points,
                    nominals,
                    count,
                    min_dn_bits,
                    max_dn_bits,
                    max_abs_dn_bits,
                    attachment_digest,
                }
            }
            "feature" => {
                let feature = req_str(o, "feature")?.to_owned();
                let nominal = req_str(o, "nominal")?.to_owned();
                if !is_id(&feature) || !is_id(&nominal) {
                    return Err(bad());
                }
                let location_bits = parse_bits3(o.get("location").ok_or_else(bad)?)?;
                let direction_rad_bits = opt_hex_bits(o.get("direction_rad"))?;
                let size_bits = opt_hex_bits(o.get("size"))?;
                PtDevEventPayload::Feature { feature, nominal, location_bits, direction_rad_bits, size_bits }
            }
            _ => return Err(bad()),
        };

        Ok(PtDevEvent {
            principal,
            branch,
            run,
            char,
            frame,
            variant,
            basis,
            value_si_bits,
            payload,
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

/// Result of exact replay over a sequence of `check.eval_deviation@1` journal entries (`AC-5`).
#[derive(Debug, Clone)]
pub struct PtDevReplayed {
    pub outcome: ReplayOutcome,
    pub steps: usize,
    pub store: ResultStore,
}

/// Replays `(entry_hash, principal, canonical_event_bytes)` entries in order (`AC-5`).
pub fn replay(
    inp: &EvalInputs<'_>,
    regs: &ReplayRegisters<'_>,
    entries: &[(Digest, Principal, &[u8])],
) -> PtDevReplayed {
    let mut results = ResultStore::new();
    let mut steps = 0usize;
    for &(entry_hash, entry_principal, raw) in entries {
        let Ok(recorded) = PtDevEvent::from_canonical_bytes(raw) else {
            return PtDevReplayed { outcome: ReplayOutcome::NotReplayable, steps, store: results };
        };
        if recorded.principal != entry_principal {
            return PtDevReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        }
        let Ok(register) = RegisterView::for_replay(&recorded.pin, regs.store, regs.sources) else {
            return PtDevReplayed { outcome: ReplayOutcome::NotReplayable, steps, store: results };
        };
        let replay_inputs =
            EvalInputs { spec: inp.spec, dprof: inp.dprof, table: inp.table, store: inp.store, register: &register };
        let target = match &recorded.payload {
            PtDevEventPayload::Points { points, nominals, .. } => {
                DeviationTarget::Points { points: points.clone(), nominals: nominals.clone() }
            }
            PtDevEventPayload::Feature { feature, nominal, .. } => {
                DeviationTarget::Feature { feature: feature.clone(), nominal: nominal.clone() }
            }
        };
        let req = DeviationRequest {
            run: recorded.run.as_str(),
            char: recorded.char.as_str(),
            target,
            frame: recorded.frame.as_str(),
            variant: recorded.variant.as_deref(),
        };
        let Ok(ev) = eval_deviation_for(&results, &replay_inputs, entry_principal, &req, false, Purpose::Replay) else {
            return PtDevReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        };
        let Some(rebuilt_event) = ev.event() else {
            return PtDevReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        };
        let Ok(rebuilt_bytes) = rebuilt_event.to_canonical_bytes() else {
            return PtDevReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        };
        if rebuilt_bytes != raw {
            return PtDevReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        }
        if commit(&mut results, entry_principal, &ev, entry_hash).is_err() {
            return PtDevReplayed { outcome: ReplayOutcome::Divergent, steps, store: results };
        }
        steps += 1;
    }
    PtDevReplayed { outcome: ReplayOutcome::Identical, steps, store: results }
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

fn opt_hex_bits(v: Option<&CValue>) -> Result<Option<u64>, EvalError> {
    match v {
        Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(parse_hex_bits(s)?)),
        _ => Err(EvalError::BadParams { reason: "malformed_event" }),
    }
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

fn parse_u64_num(v: &CValue) -> Result<u64, EvalError> {
    let bad = || EvalError::BadParams { reason: "malformed_event" };
    let CValue::Num(n) = v else { return Err(bad()) };
    if !n.is_finite() || *n < 0.0 || n.fract() != 0.0 || *n > (1u64 << 53) as f64 {
        return Err(bad());
    }
    Ok(*n as u64)
}
