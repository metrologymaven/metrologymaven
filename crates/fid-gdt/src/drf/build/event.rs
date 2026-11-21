//! The `gdt.build_drf@1` journal event and exact replay (determinism class D0).
//!
//! ```json
//! { "command": "gdt.build_drf@1", "principal": "human", "branch": "working",
//!   "char": "drf-1", "datums": ["A", "B", "C"],
//!   "measurements": [{"feature": "f1", "digest": "sha256:..."}, ..],
//!   "drf_id": "sha256:...", "drf": { ..the DRF document.. },
//!   "interpretation_pin": { ..`fid.gdt-interp-pin/1`.. } }
//! ```
//!
//! The event holds the request, the digest of every measured point set it read, the full DRF
//! document with its ID, and the interpretation pin (`GDT.CHAR.INTERP`) so replay consults
//! the same register. Serialised as RFC 8785 canonical JSON.
//!
//! **Replay.** Each recorded event is read strictly and re-executed with the principal of the
//! journal entry that holds it (passed in by the caller, never taken from the event alone): an
//! event whose recorded principal differs from its entry's is `DIVERGENT`. The step is
//! `IDENTICAL` when the re-derived event's canonical bytes equal the recorded bytes,
//! `NOT_REPLAYABLE` when the bytes are not a readable event or a measured point set it names is
//! no longer available with that digest or the pinned register cannot be loaded and
//! re-verified for replay, and `DIVERGENT` otherwise (including a command that
//! now fails). Replay stops at the first step that is not `IDENTICAL`.

use super::{build_for, BuildError, BuildInputs, BuildRequest, DrfStore, BUILD_COMMAND};
use crate::chars::dprof::store::parse_principal;
use crate::chars::dprof::{Branch, CharId};
use crate::chars::interp::{InterpretationPin, RegisterSources, RegisterStore, RegisterView};
use crate::dmt_flow::status::Purpose;
use fid_canon::CValue;
use fid_io_reg::Principal;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Largest event accepted by [`DrfEvent::from_canonical_bytes`], in bytes.
pub const MAX_EVENT_BYTES: usize = 1024 * 1024;
/// Deepest event nesting: event, DRF, datums, one datum, its fit, its contacts.
const MAX_EVENT_DEPTH: usize = 6;
const FIELDS: [&str; 9] =
    ["branch", "char", "command", "datums", "drf", "drf_id", "interpretation_pin", "measurements", "principal"];

/// One `gdt.build_drf@1` event. Fields are private; events are made by [`build_drf`] or read
/// back by [`DrfEvent::from_canonical_bytes`]. The branch is always derived from the principal.
#[derive(Debug, Clone, PartialEq)]
pub struct DrfEvent {
    principal: Principal,
    branch: Branch,
    char: CharId,
    datums: Vec<String>,
    measurements: Vec<(String, Digest)>,
    drf: CValue,
    drf_id: Digest,
    pin: InterpretationPin,
}

fn bad(reason: &'static str) -> BuildError {
    BuildError::Event { reason }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn text<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, BuildError> {
    match o.get(k) {
        Some(CValue::Str(v)) => Ok(v),
        _ => Err(bad("wrong_type")),
    }
}

impl DrfEvent {
    pub(super) fn new(
        principal: Principal,
        char: CharId,
        datums: Vec<String>,
        measurements: Vec<(String, Digest)>,
        drf: CValue,
        drf_id: Digest,
        pin: InterpretationPin,
    ) -> DrfEvent {
        DrfEvent { principal, branch: Branch::for_principal(principal), char, datums, measurements, drf, drf_id, pin }
    }

    /// Who acted.
    pub fn principal(&self) -> Principal {
        self.principal
    }
    /// The branch written.
    pub fn branch(&self) -> Branch {
        self.branch
    }
    /// The characteristic.
    pub fn char(&self) -> &CharId {
        &self.char
    }
    /// The datum references as given, in precedence order.
    pub fn datums(&self) -> &[String] {
        &self.datums
    }
    /// The measured point sets read: feature and digest, in precedence order.
    pub fn measurements(&self) -> &[(String, Digest)] {
        &self.measurements
    }
    /// The DRF ID.
    pub fn drf_id(&self) -> Digest {
        self.drf_id
    }
    /// The interpretation pin.
    pub fn pin(&self) -> &InterpretationPin {
        &self.pin
    }

    /// The event as a canonical-JSON value.
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("branch".to_owned(), s(self.branch.as_str()));
        m.insert("char".to_owned(), s(self.char.as_str()));
        m.insert("command".to_owned(), s(BUILD_COMMAND));
        m.insert("datums".to_owned(), CValue::Arr(self.datums.iter().map(|d| s(d)).collect()));
        m.insert("drf".to_owned(), self.drf.clone());
        m.insert("drf_id".to_owned(), s(self.drf_id.as_str()));
        m.insert("interpretation_pin".to_owned(), self.pin.to_cvalue());
        let ms = self
            .measurements
            .iter()
            .map(|(f, d)| {
                let mut o = BTreeMap::new();
                o.insert("digest".to_owned(), s(d.as_str()));
                o.insert("feature".to_owned(), s(f));
                CValue::Obj(o)
            })
            .collect();
        m.insert("measurements".to_owned(), CValue::Arr(ms));
        m.insert("principal".to_owned(), s(self.principal.as_str()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, BuildError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| bad("encode"))
    }

    /// Reads an event back strictly: at most [`MAX_EVENT_BYTES`], no duplicate key, the nine
    /// fields and no other, the known command, the branch of the principal
    /// (`branch_principal_mismatch`), a valid characteristic ID, one to three datum strings, one
    /// `{digest, feature}` per datum, a DRF object whose canonical digest is `drf_id`
    /// (`drf_id_mismatch`), a canonical interpretation pin (`bad_pin`), and bytes that are the
    /// event's own canonical form (`not_canonical`). Fails `event_invalid{reason}`.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<DrfEvent, BuildError> {
        if bytes.len() > MAX_EVENT_BYTES {
            return Err(bad("too_large"));
        }
        let v = fid_canon::parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad("json"))?;
        let o = match &v {
            CValue::Obj(o) if o.len() == FIELDS.len() && FIELDS.iter().all(|k| o.contains_key(*k)) => o,
            CValue::Obj(_) => return Err(bad("bad_keys")),
            _ => return Err(bad("wrong_type")),
        };
        if text(o, "command")? != BUILD_COMMAND {
            return Err(bad("unknown_command"));
        }
        let principal = parse_principal(text(o, "principal")?).ok_or_else(|| bad("bad_principal"))?;
        let branch = Branch::parse(text(o, "branch")?).ok_or_else(|| bad("bad_branch"))?;
        if Branch::for_principal(principal) != branch {
            return Err(bad("branch_principal_mismatch"));
        }
        let char = CharId::parse(text(o, "char")?).ok_or_else(|| bad("bad_char_id"))?;
        let datums: Vec<String> = match o.get("datums") {
            Some(CValue::Arr(a)) if !a.is_empty() && a.len() <= fid_gps::drf::build::MAX_DATUMS => a
                .iter()
                .map(|d| match d {
                    CValue::Str(t) => Ok(t.clone()),
                    _ => Err(bad("wrong_type")),
                })
                .collect::<Result<_, _>>()?,
            _ => return Err(bad("bad_datums")),
        };
        let measurements: Vec<(String, Digest)> = match o.get("measurements") {
            Some(CValue::Arr(a)) if a.len() == datums.len() => a
                .iter()
                .map(|m| match m {
                    CValue::Obj(m) if m.len() == 2 => {
                        let d = Digest::parse(text(m, "digest")?).map_err(|_| bad("bad_digest"))?;
                        Ok((text(m, "feature")?.to_owned(), d))
                    }
                    _ => Err(bad("bad_measurements")),
                })
                .collect::<Result<_, _>>()?,
            _ => return Err(bad("bad_measurements")),
        };
        let drf = match o.get("drf") {
            Some(d @ CValue::Obj(_)) => d.clone(),
            _ => return Err(bad("wrong_type")),
        };
        let drf_id = Digest::parse(text(o, "drf_id")?).map_err(|_| bad("bad_digest"))?;
        let drf_bytes = fid_canon::to_canonical_bytes(&drf).map_err(|_| bad("json"))?;
        if Digest::from_bytes(fid_canon::digest_bytes(&drf_bytes).0) != drf_id {
            return Err(bad("drf_id_mismatch"));
        }
        let pin_value = o.get("interpretation_pin").ok_or_else(|| bad("bad_keys"))?;
        let pin_bytes = fid_canon::to_canonical_bytes(pin_value).map_err(|_| bad("bad_pin"))?;
        let pin = InterpretationPin::from_canonical_bytes(&pin_bytes).ok_or_else(|| bad("bad_pin"))?;
        let e = DrfEvent { principal, branch, char, datums, measurements, drf, drf_id, pin };
        if e.canonical_bytes()?.as_slice() != bytes {
            return Err(bad("not_canonical"));
        }
        Ok(e)
    }
}

/// The result of a replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrfReplayed {
    /// The aggregate outcome.
    pub outcome: ReplayOutcome,
    /// One outcome per step run, in order.
    pub steps: Vec<ReplayOutcome>,
    /// The store after the steps run.
    pub store: DrfStore,
}

/// What replay reads besides the build inputs: the registers by digest and the sources a
/// `verified` pin's register is re-checked against (`GDT.CHAR.INTERP`).
#[derive(Debug, Clone, Copy)]
pub struct ReplayRegisters<'a> {
    /// The registers the pins may name.
    pub store: &'a RegisterStore,
    /// The journal, rosters, conflict inputs and published lists of the check.
    pub sources: &'a RegisterSources<'a>,
}

fn step(
    store: &mut DrfStore,
    inp: &BuildInputs<'_>,
    regs: &ReplayRegisters<'_>,
    entry: Principal,
    recorded: &[u8],
) -> ReplayOutcome {
    let Ok(e) = DrfEvent::from_canonical_bytes(recorded) else {
        return ReplayOutcome::NotReplayable;
    };
    if e.principal() != entry {
        return ReplayOutcome::Divergent;
    }
    if e.measurements().iter().any(|(f, d)| inp.points.digest(f) != Some(*d)) {
        return ReplayOutcome::NotReplayable;
    }
    // The pinned register, checked for replay, never the caller's current one.
    let Ok(view) = RegisterView::for_replay(e.pin(), regs.store, regs.sources) else {
        return ReplayOutcome::NotReplayable;
    };
    let pinned = BuildInputs { register: &view, ..*inp };
    let datums: Vec<&str> = e.datums().iter().map(String::as_str).collect();
    let req = BuildRequest { char: e.char().as_str(), datums: &datums };
    let derived = build_for(store, &pinned, entry, &req, false, Purpose::Replay).ok();
    match derived.as_ref().and_then(|b| b.event()).and_then(|d| d.canonical_bytes().ok()) {
        Some(bytes) if bytes == recorded => ReplayOutcome::Identical,
        _ => ReplayOutcome::Divergent,
    }
}

/// Replays `events` from `start`: each item is the principal of the journal entry that holds
/// the event and the event's recorded canonical bytes, in journal order. `inp` holds the
/// inputs the events read; its `register` is not used: each event is re-run against the
/// register its pin names, loaded from `regs` and re-checked for [`Purpose::Replay`] (so a
/// register that was the latest when the DRF was built still replays exactly after a newer
/// one is published). A pinned register that is missing or no longer verifies is
/// `NOT_REPLAYABLE`.
pub fn replay(
    start: &DrfStore,
    inp: &BuildInputs<'_>,
    regs: &ReplayRegisters<'_>,
    events: &[(Principal, Vec<u8>)],
) -> DrfReplayed {
    let mut store = start.clone();
    let mut steps = Vec::with_capacity(events.len());
    for (entry, recorded) in events {
        let o = step(&mut store, inp, regs, *entry, recorded);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    DrfReplayed { outcome: fid_replay::aggregate(&steps), steps, store }
}
