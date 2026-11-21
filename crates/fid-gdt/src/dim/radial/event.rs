//! The `check.eval_diameter@1` / `check.eval_radius@1` journal event and exact replay
//! (determinism class D0).
//!
//! ```json
//! { "command": "check.eval_diameter@1", "principal": "human", "branch": "working",
//!   "run": "run-1", "char": "dia-1", "feature": "f-1", "variant": null, "gauge": null,
//!   "measurement": {"digest": "sha256:...", "points": 12},
//!   "fit": {"algorithm": "fit.circle3.ls@1.0.0", "output_digest": "sha256:..."},
//!   "chain": {"row": "dmt:asme/diameter/none@1", "variant": "ls_circle3",
//!             "chain_digest": "sha256:...", "dmt_digest": "sha256:..."},
//!   "value": "0x<bits>", "interpretation": "provisional",
//!   "interpretation_pin": { ..`fid.gdt-interp-pin/1`.. } }
//! ```
//!
//! The event holds the request, the digest of the measured points it read, the fit and chain
//! it used, the value and the interpretation pin (`GDT.CHAR.INTERP`) so replay consults the
//! same register. Serialised as RFC 8785 canonical JSON. It does not hold the ResultRecord:
//! the record names the journal entry that holds the event, so it is rebuilt from the event's
//! inputs and that entry's hash.
//!
//! **Not authority-bearing.** A ResultRecord is evidence; no later command's permission
//! depends on it (it carries no approval, branch, promotion, sign-off or conformance decision:
//! the decision is always `NOT_DECIDABLE` here), so REQ-CORE.RPL.RECON.FOLD-008 does not apply
//! and no effect name is reserved. Replay still binds each event to its journal entry: the
//! principal and the branch come from the entry (an event naming another principal is
//! `DIVERGENT`), and the record's `journal_entry` is the entry's hash, never a value from the
//! event body.
//!
//! **Replay.** Each recorded event is read strictly and re-executed with the principal of the
//! journal entry that holds it. The step is `IDENTICAL` when the re-derived event's canonical
//! bytes equal the recorded bytes and its record commits; `NOT_REPLAYABLE` when the bytes are not
//! a readable event, the measured points it names are no longer available with that digest,
//! or the pinned register cannot be loaded and re-verified for replay; and `DIVERGENT`
//! otherwise (including a command that now fails). Replay stops at the first step that is not
//! `IDENTICAL`.

use super::{
    command_of, eval_for, gauge_cvalue, hex, is_id, obj, EvalError, EvalInputs, EvalRequest, Evaluated,
    EVAL_DIAMETER_COMMAND, EVAL_RADIUS_COMMAND,
};
use crate::chars::dprof::store::parse_principal;
use crate::chars::dprof::{Branch, CharId};
use crate::chars::interp::{InterpretationPin, RegisterView};
use crate::chars::record::{ResultStore, RunId};
use crate::dmt_flow::status::Purpose;
use crate::drf::build::ReplayRegisters;
use fid_canon::CValue;
use fid_gps::dim::radial::{Gauge, Measure};
use fid_io_reg::Principal;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Largest event accepted by [`RadialEvent::from_canonical_bytes`], in bytes.
pub const MAX_EVENT_BYTES: usize = 16 * 1024;
/// Deepest event nesting: event, an object field.
const MAX_EVENT_DEPTH: usize = 3;
const FIELDS: [&str; 14] = [
    "branch",
    "chain",
    "char",
    "command",
    "feature",
    "fit",
    "gauge",
    "interpretation",
    "interpretation_pin",
    "measurement",
    "principal",
    "run",
    "value",
    "variant",
];

/// One `check.eval_diameter@1` / `check.eval_radius@1` event. Fields are private; events are
/// made by [`super::eval_radial`] or read back by [`RadialEvent::from_canonical_bytes`]. The
/// branch is always derived from the principal.
#[derive(Debug, Clone, PartialEq)]
pub struct RadialEvent {
    measure: Measure,
    principal: Principal,
    branch: Branch,
    run: RunId,
    char: CharId,
    feature: String,
    variant: Option<String>,
    gauge: Option<Gauge>,
    points_digest: Digest,
    points: u64,
    fit_algorithm: String,
    fit_digest: Digest,
    row: String,
    chain_variant: String,
    chain_digest: Digest,
    dmt_digest: Digest,
    value: f64,
    interpretation: String,
    pin: InterpretationPin,
}

fn bad(reason: &'static str) -> EvalError {
    EvalError::Event { reason }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn text<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, EvalError> {
    match o.get(k) {
        Some(CValue::Str(v)) => Ok(v),
        _ => Err(bad("wrong_type")),
    }
}

fn object<'a>(
    o: &'a BTreeMap<String, CValue>,
    k: &str,
    keys: &[&str],
) -> Result<&'a BTreeMap<String, CValue>, EvalError> {
    match o.get(k) {
        Some(CValue::Obj(m)) if m.len() == keys.len() && keys.iter().all(|x| m.contains_key(*x)) => Ok(m),
        Some(CValue::Obj(_)) => Err(bad("bad_keys")),
        _ => Err(bad("wrong_type")),
    }
}

fn digest(o: &BTreeMap<String, CValue>, k: &str) -> Result<Digest, EvalError> {
    Digest::parse(text(o, k)?).map_err(|_| bad("bad_digest"))
}

/// `0x` + 16 lowercase hex digits to the `f64` with those bits.
fn unhex(v: &str) -> Result<f64, EvalError> {
    let h = v.strip_prefix("0x").filter(|h| h.len() == 16).ok_or_else(|| bad("bad_float"))?;
    if !h.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
        return Err(bad("bad_float"));
    }
    u64::from_str_radix(h, 16).map(f64::from_bits).map_err(|_| bad("bad_float"))
}

impl RadialEvent {
    pub(super) fn of(ev: &Evaluated) -> RadialEvent {
        RadialEvent {
            measure: ev.value.measure(),
            principal: ev.principal,
            branch: ev.branch,
            run: ev.run.clone(),
            char: ev.char.clone(),
            feature: ev.feature.clone(),
            variant: ev.variant.clone(),
            gauge: ev.gauge,
            points_digest: ev.points_digest,
            points: ev.value.points() as u64,
            fit_algorithm: ev.value.algorithm().to_owned(),
            fit_digest: ev.value.fit_digest(),
            row: ev.row.clone(),
            chain_variant: ev.chain_variant.clone(),
            chain_digest: ev.chain_digest,
            dmt_digest: ev.dmt_digest,
            value: ev.value.value(),
            interpretation: ev.interpretation.to_owned(),
            pin: ev.pin,
        }
    }

    /// The command ID.
    pub fn command(&self) -> &'static str {
        command_of(self.measure)
    }
    /// Who acted (as recorded).
    pub fn principal(&self) -> Principal {
        self.principal
    }
    /// The branch written.
    pub fn branch(&self) -> Branch {
        self.branch
    }
    /// The run.
    pub fn run(&self) -> &RunId {
        &self.run
    }
    /// The characteristic.
    pub fn char(&self) -> &CharId {
        &self.char
    }
    /// The feature.
    pub fn feature(&self) -> &str {
        &self.feature
    }
    /// The digest of the measured points read.
    pub fn points_digest(&self) -> Digest {
        self.points_digest
    }
    /// The value in metres.
    pub fn value(&self) -> f64 {
        self.value
    }
    /// The algorithm ID of the fit.
    pub fn fit_algorithm(&self) -> &str {
        &self.fit_algorithm
    }
    /// The interpretation pin.
    pub fn pin(&self) -> &InterpretationPin {
        &self.pin
    }

    /// The event as a canonical-JSON value.
    pub fn to_cvalue(&self) -> CValue {
        obj([
            ("branch", s(self.branch.as_str())),
            (
                "chain",
                obj([
                    ("chain_digest", s(self.chain_digest.as_str())),
                    ("dmt_digest", s(self.dmt_digest.as_str())),
                    ("row", s(&self.row)),
                    ("variant", s(&self.chain_variant)),
                ]),
            ),
            ("char", s(self.char.as_str())),
            ("command", s(self.command())),
            ("feature", s(&self.feature)),
            ("fit", obj([("algorithm", s(&self.fit_algorithm)), ("output_digest", s(self.fit_digest.as_str()))])),
            ("gauge", gauge_cvalue(self.gauge)),
            ("interpretation", s(&self.interpretation)),
            ("interpretation_pin", self.pin.to_cvalue()),
            (
                "measurement",
                obj([("digest", s(self.points_digest.as_str())), ("points", CValue::Num(self.points as f64))]),
            ),
            ("principal", s(self.principal.as_str())),
            ("run", s(self.run.as_str())),
            ("value", hex(self.value)),
            ("variant", self.variant.as_deref().map_or(CValue::Null, s)),
        ])
    }

    /// RFC 8785 canonical JSON bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, EvalError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| bad("encode"))
    }

    /// Reads an event back strictly: at most [`MAX_EVENT_BYTES`], no duplicate key, the
    /// fourteen fields and no other, a known command, the branch of the principal
    /// (`branch_principal_mismatch`), valid run, characteristic and feature IDs, a variant
    /// string or null, a gauge `{from_apex}` or null, digests, `0x` floats, a canonical
    /// interpretation pin (`bad_pin`), and bytes that are the event's own canonical form
    /// (`not_canonical`). Fails `event_invalid{reason}`.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<RadialEvent, EvalError> {
        if bytes.len() > MAX_EVENT_BYTES {
            return Err(bad("too_large"));
        }
        let v = fid_canon::parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad("json"))?;
        let o = match &v {
            CValue::Obj(o) if o.len() == FIELDS.len() && FIELDS.iter().all(|k| o.contains_key(*k)) => o,
            CValue::Obj(_) => return Err(bad("bad_keys")),
            _ => return Err(bad("wrong_type")),
        };
        let measure = match text(o, "command")? {
            EVAL_DIAMETER_COMMAND => Measure::Diameter,
            EVAL_RADIUS_COMMAND => Measure::Radius,
            _ => return Err(bad("unknown_command")),
        };
        let principal = parse_principal(text(o, "principal")?).ok_or_else(|| bad("bad_principal"))?;
        let branch = Branch::parse(text(o, "branch")?).ok_or_else(|| bad("bad_branch"))?;
        if Branch::for_principal(principal) != branch {
            return Err(bad("branch_principal_mismatch"));
        }
        let run = RunId::parse(text(o, "run")?).map_err(|_| bad("bad_run"))?;
        let char = CharId::parse(text(o, "char")?).ok_or_else(|| bad("bad_char_id"))?;
        let feature = text(o, "feature")?;
        if !is_id(feature) {
            return Err(bad("bad_feature"));
        }
        let variant = match o.get("variant") {
            Some(CValue::Null) => None,
            Some(CValue::Str(v)) if is_id(v) => Some(v.clone()),
            _ => return Err(bad("bad_variant")),
        };
        let gauge = match o.get("gauge") {
            Some(CValue::Null) => None,
            Some(CValue::Obj(_)) => {
                Some(Gauge::FromApex(unhex(text(object(o, "gauge", &["from_apex"])?, "from_apex")?)?))
            }
            _ => return Err(bad("bad_gauge")),
        };
        let m = object(o, "measurement", &["digest", "points"])?;
        let points = match m.get("points") {
            Some(CValue::Num(n))
                if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= 9.007_199_254_740_992e15 =>
            {
                *n as u64
            }
            _ => return Err(bad("bad_points")),
        };
        let fit = object(o, "fit", &["algorithm", "output_digest"])?;
        let chain = object(o, "chain", &["chain_digest", "dmt_digest", "row", "variant"])?;
        let interpretation = text(o, "interpretation")?;
        if interpretation != "provisional" && interpretation != "signed_off" {
            return Err(bad("bad_interpretation"));
        }
        let pin_value = o.get("interpretation_pin").ok_or_else(|| bad("bad_keys"))?;
        let pin_bytes = fid_canon::to_canonical_bytes(pin_value).map_err(|_| bad("bad_pin"))?;
        let pin = InterpretationPin::from_canonical_bytes(&pin_bytes).ok_or_else(|| bad("bad_pin"))?;
        let e = RadialEvent {
            measure,
            principal,
            branch,
            run,
            char,
            feature: feature.to_owned(),
            variant,
            gauge,
            points_digest: digest(m, "digest")?,
            points,
            fit_algorithm: text(fit, "algorithm")?.to_owned(),
            fit_digest: digest(fit, "output_digest")?,
            row: text(chain, "row")?.to_owned(),
            chain_variant: text(chain, "variant")?.to_owned(),
            chain_digest: digest(chain, "chain_digest")?,
            dmt_digest: digest(chain, "dmt_digest")?,
            value: unhex(text(o, "value")?)?,
            interpretation: interpretation.to_owned(),
            pin,
        };
        if e.canonical_bytes()?.as_slice() != bytes {
            return Err(bad("not_canonical"));
        }
        Ok(e)
    }
}

/// The result of a replay.
#[derive(Debug, Clone)]
pub struct RadialReplayed {
    /// The aggregate outcome.
    pub outcome: ReplayOutcome,
    /// One outcome per step run, in order.
    pub steps: Vec<ReplayOutcome>,
    /// The record store after the steps run.
    pub store: ResultStore,
}

fn step(
    store: &mut ResultStore,
    inp: &EvalInputs<'_>,
    regs: &ReplayRegisters<'_>,
    entry: Principal,
    entry_hash: Digest,
    recorded: &[u8],
) -> ReplayOutcome {
    let Ok(e) = RadialEvent::from_canonical_bytes(recorded) else {
        return ReplayOutcome::NotReplayable;
    };
    if e.principal() != entry {
        return ReplayOutcome::Divergent;
    }
    if inp.features.digest(&e.feature) != Some(e.points_digest) {
        return ReplayOutcome::NotReplayable;
    }
    // The pinned register, checked for replay, never the caller's current one.
    let Ok(view) = RegisterView::for_replay(e.pin(), regs.store, regs.sources) else {
        return ReplayOutcome::NotReplayable;
    };
    let pinned = EvalInputs { register: &view, ..*inp };
    let req = EvalRequest {
        run: e.run.as_str(),
        char: e.char.as_str(),
        feature: &e.feature,
        variant: e.variant.as_deref(),
        gauge: e.gauge,
    };
    let Ok(derived) = eval_for(store, &pinned, entry, e.measure, &req, false, Purpose::Replay) else {
        return ReplayOutcome::Divergent;
    };
    match derived.event().and_then(|d| d.canonical_bytes().ok()) {
        Some(bytes) if bytes == recorded => {}
        _ => return ReplayOutcome::Divergent,
    }
    match super::commit(store, entry, &derived, entry_hash) {
        Ok(_) => ReplayOutcome::Identical,
        Err(_) => ReplayOutcome::Divergent,
    }
}

/// Replays `events` from `start`: each item is the principal and the hash of the journal entry
/// that holds the event, and the event's recorded canonical bytes, in journal order. `inp`
/// holds the inputs the events read; its `register` is not used: each event is re-run against
/// the register its pin names, loaded from `regs` and re-checked for [`Purpose::Replay`]. Each
/// identical step commits its rebuilt record, naming the entry's hash, into the returned store.
pub fn replay(
    start: &ResultStore,
    inp: &EvalInputs<'_>,
    regs: &ReplayRegisters<'_>,
    events: &[(Principal, Digest, Vec<u8>)],
) -> RadialReplayed {
    let mut store = start.clone();
    let mut steps = Vec::with_capacity(events.len());
    for (entry, hash, recorded) in events {
        let o = step(&mut store, inp, regs, *entry, *hash, recorded);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    RadialReplayed { outcome: fid_replay::aggregate(&steps), steps, store }
}
