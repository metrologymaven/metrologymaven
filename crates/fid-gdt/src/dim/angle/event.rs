//! Journal event `check.eval_angle@1` and exact D0 replay (`GDT.DIM.ANGLE`, `AC-5`).

use super::{
    commit, eval_for, hex, is_id, obj, EvalError, EvalInputs, EvalRequest, Evaluated, TargetB, EVAL_ANGLE_COMMAND, UNIT,
};
use crate::chars::dprof::store::parse_principal;
use crate::chars::dprof::{Branch, CharId};
use crate::chars::interp::{InterpretationPin, RegisterView};
use crate::chars::record::{Quantity, RecordError, ResultStore, RunId};
use crate::dmt_flow::status::Purpose;
use crate::drf::build::ReplayRegisters;
use fid_canon::CValue;
use fid_gps::dim::angle::{AngleMode, AuthoredUnit, FrameAxis};
use fid_io_reg::Principal;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Largest accepted `check.eval_angle@1` event payload in bytes.
pub const MAX_EVENT_BYTES: usize = 16 * 1024;
const MAX_EVENT_DEPTH: usize = 3;

const FIELDS: [&str; 17] = [
    "a",
    "authored_unit",
    "authored_value",
    "b",
    "branch",
    "chain",
    "char",
    "command",
    "input_digests",
    "interpretation",
    "interpretation_pin",
    "mode",
    "plane",
    "principal",
    "projected",
    "run",
    "value_rad",
];

/// One `check.eval_angle@1` journal event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AngleEvent {
    principal: Principal,
    branch: Branch,
    run: RunId,
    char: CharId,
    a: String,
    b: TargetB,
    mode: AngleMode,
    plane: Option<String>,
    variant: Option<String>,
    projected: bool,
    authored_unit: AuthoredUnit,
    value_rad_bits: u64,
    authored_value_bits: u64,
    input_digests: Vec<Digest>,
    row: String,
    chain_variant: String,
    chain_digest: Digest,
    dmt_digest: Digest,
    interpretation: String,
    pin: InterpretationPin,
}

impl AngleEvent {
    pub(crate) fn of(ev: &Evaluated) -> AngleEvent {
        AngleEvent {
            principal: ev.principal,
            branch: ev.branch,
            run: ev.run.clone(),
            char: ev.char.clone(),
            a: ev.a.clone(),
            b: ev.b.clone(),
            mode: ev.mode,
            plane: ev.plane.clone(),
            variant: ev.variant.clone(),
            projected: ev.value.projected,
            authored_unit: ev.value.authored_unit,
            value_rad_bits: ev.value.value_rad.to_bits(),
            authored_value_bits: ev.value.authored_value.to_bits(),
            input_digests: ev.input_digests.clone(),
            row: ev.row.clone(),
            chain_variant: ev.chain_variant.clone(),
            chain_digest: ev.chain_digest,
            dmt_digest: ev.dmt_digest,
            interpretation: ev.interpretation.to_owned(),
            pin: ev.pin,
        }
    }

    /// IEEE 754 binary64 bits of the radian angle (D0, avoids exposing raw `f64` to report crates).
    pub fn value_rad_bits(&self) -> u64 {
        self.value_rad_bits
    }

    /// Typed radian [`Quantity`] of the recorded angle.
    pub fn quantity(&self) -> Result<Quantity, RecordError> {
        Quantity::new(f64::from_bits(self.value_rad_bits), UNIT)
    }

    /// Canonical [`CValue`] representation of this event.
    pub fn to_value(&self) -> CValue {
        let b_val = match &self.b {
            TargetB::Element(id) => obj([("element", CValue::Str(id.clone()))]),
            TargetB::Axis(ax) => obj([("axis", CValue::Str(ax.as_str().to_owned()))]),
        };
        let mut m = BTreeMap::new();
        m.insert("a".to_owned(), CValue::Str(self.a.clone()));
        m.insert("authored_unit".to_owned(), CValue::Str(self.authored_unit.as_str().to_owned()));
        m.insert("authored_value".to_owned(), CValue::Str(hex(f64::from_bits(self.authored_value_bits))));
        m.insert("b".to_owned(), b_val);
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
        m.insert("command".to_owned(), CValue::Str(EVAL_ANGLE_COMMAND.to_owned()));
        m.insert(
            "input_digests".to_owned(),
            CValue::Arr(self.input_digests.iter().map(|d| CValue::Str(d.as_str().to_owned())).collect()),
        );
        m.insert("interpretation".to_owned(), CValue::Str(self.interpretation.clone()));
        m.insert("interpretation_pin".to_owned(), self.pin.to_cvalue());
        m.insert("mode".to_owned(), CValue::Str(self.mode.as_str().to_owned()));
        m.insert("plane".to_owned(), self.plane.as_ref().map_or(CValue::Null, |p| CValue::Str(p.clone())));
        m.insert("principal".to_owned(), CValue::Str(principal_str(self.principal).to_owned()));
        m.insert("projected".to_owned(), CValue::Bool(self.projected));
        m.insert("run".to_owned(), CValue::Str(self.run.as_str().to_owned()));
        m.insert("value_rad".to_owned(), CValue::Str(hex(f64::from_bits(self.value_rad_bits))));
        if let Some(v) = &self.variant {
            m.insert("variant".to_owned(), CValue::Str(v.clone()));
        } else {
            m.insert("variant".to_owned(), CValue::Null);
        }
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes of this event.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, EvalError> {
        fid_canon::to_canonical_bytes(&self.to_value())
            .map_err(|_| EvalError::BadParams { reason: "canonical_encode_failed" })
    }

    /// Strictly parses an [`AngleEvent`] from RFC 8785 canonical JSON bytes.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<AngleEvent, EvalError> {
        let bad = || EvalError::BadParams { reason: "malformed_event" };
        let v = fid_canon::parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad())?;
        let CValue::Obj(o) = &v else { return Err(bad()) };
        for k in FIELDS {
            if !o.contains_key(k) {
                return Err(bad());
            }
        }
        if !o.contains_key("variant") || o.len() != FIELDS.len() + 1 {
            return Err(bad());
        }
        if req_str(o, "command")? != EVAL_ANGLE_COMMAND {
            return Err(bad());
        }
        let principal = parse_principal(req_str(o, "principal")?).ok_or_else(bad)?;
        let branch = Branch::for_principal(principal);
        if req_str(o, "branch")? != branch.as_str() {
            return Err(bad());
        }
        let run = RunId::parse(req_str(o, "run")?).map_err(|_| bad())?;
        let char = CharId::parse(req_str(o, "char")?).ok_or_else(bad)?;
        let a = req_str(o, "a")?.to_owned();
        if !is_id(&a) {
            return Err(bad());
        }
        let b = parse_target_b(o.get("b").ok_or_else(bad)?)?;
        let mode = AngleMode::parse(req_str(o, "mode")?).ok_or_else(bad)?;
        let plane = opt_str(o, "plane")?;
        if let Some(p) = &plane {
            if !is_id(p) {
                return Err(bad());
            }
        }
        let variant = opt_str(o, "variant")?;
        if let Some(vr) = &variant {
            if !is_id(vr) {
                return Err(bad());
            }
        }
        let Some(CValue::Bool(projected)) = o.get("projected") else { return Err(bad()) };
        let authored_unit = AuthoredUnit::parse(req_str(o, "authored_unit")?).ok_or_else(bad)?;
        let value_rad_bits = parse_hex_bits(req_str(o, "value_rad")?)?;
        let authored_value_bits = parse_hex_bits(req_str(o, "authored_value")?)?;
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

        Ok(AngleEvent {
            principal,
            branch,
            run,
            char,
            a,
            b,
            mode,
            plane,
            variant,
            projected: *projected,
            authored_unit,
            value_rad_bits,
            authored_value_bits,
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

/// Result of exact replay over a sequence of `check.eval_angle@1` journal entries (`AC-5`).
#[derive(Debug, Clone)]
pub struct AngleReplayed {
    pub outcome: ReplayOutcome,
    pub steps: usize,
    pub store: ResultStore,
}

/// Replays `(entry_hash, principal, canonical_event_bytes)` entries in order (`AC-5`).
pub fn replay(
    inp: &EvalInputs<'_>,
    regs: &ReplayRegisters<'_>,
    entries: &[(Digest, Principal, &[u8])],
) -> AngleReplayed {
    let mut store = ResultStore::new();
    let mut steps = 0usize;
    for &(entry_hash, entry_principal, raw) in entries {
        let Ok(recorded) = AngleEvent::from_canonical_bytes(raw) else {
            return AngleReplayed { outcome: ReplayOutcome::NotReplayable, steps, store };
        };
        if recorded.principal != entry_principal {
            return AngleReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        }
        let Ok(register) = RegisterView::for_replay(&recorded.pin, regs.store, regs.sources) else {
            return AngleReplayed { outcome: ReplayOutcome::NotReplayable, steps, store };
        };
        let replay_inputs = EvalInputs {
            spec: inp.spec,
            dprof: inp.dprof,
            table: inp.table,
            elements: inp.elements,
            register: &register,
        };
        let req = EvalRequest {
            run: recorded.run.as_str(),
            char: recorded.char.as_str(),
            a: recorded.a.as_str(),
            b: recorded.b.clone(),
            mode: recorded.mode,
            plane: recorded.plane.as_deref(),
            authored_unit: recorded.authored_unit,
            variant: recorded.variant.as_deref(),
        };
        let Ok(ev) = eval_for(&store, &replay_inputs, entry_principal, &req, false, Purpose::Replay) else {
            return AngleReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        };
        let Some(rebuilt_event) = ev.event() else {
            return AngleReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        };
        let Ok(rebuilt_bytes) = rebuilt_event.to_canonical_bytes() else {
            return AngleReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        };
        if rebuilt_bytes != raw {
            return AngleReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        }
        if commit(&mut store, entry_principal, &ev, entry_hash).is_err() {
            return AngleReplayed { outcome: ReplayOutcome::Divergent, steps, store };
        }
        steps += 1;
    }
    AngleReplayed { outcome: ReplayOutcome::Identical, steps, store }
}

fn principal_str(p: Principal) -> &'static str {
    match p {
        Principal::Human => "human",
        Principal::Agent => "agent",
        Principal::Script => "script",
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

fn parse_target_b(v: &CValue) -> Result<TargetB, EvalError> {
    let bad = || EvalError::BadParams { reason: "malformed_event" };
    let CValue::Obj(o) = v else { return Err(bad()) };
    if o.len() != 1 {
        return Err(bad());
    }
    if let Some(CValue::Str(el)) = o.get("element") {
        if !is_id(el) {
            return Err(bad());
        }
        return Ok(TargetB::Element(el.clone()));
    }
    if let Some(CValue::Str(ax)) = o.get("axis") {
        let parsed = FrameAxis::parse(ax).ok_or_else(bad)?;
        return Ok(TargetB::Axis(parsed));
    }
    Err(bad())
}

fn parse_hex_bits(s: &str) -> Result<u64, EvalError> {
    let bad = || EvalError::BadParams { reason: "malformed_event" };
    let rest = s.strip_prefix("0x").ok_or_else(bad)?;
    if rest.len() != 16 {
        return Err(bad());
    }
    u64::from_str_radix(rest, 16).map_err(|_| bad())
}
