//! Exact replay of `gdt.set_governing_spec@1` events (determinism class D0).
//!
//! From given start states, each recorded event is re-executed: an explicit event re-runs
//! the set command with the recorded principal, characteristic and new spec; an inheritance
//! event re-runs creation in the recorded plan. The step is `IDENTICAL` when the re-derived
//! event's canonical bytes equal the recorded bytes, `DIVERGENT` when they differ or the
//! command now fails, and `NOT_REPLAYABLE` when the recorded event cannot be read. Replay
//! stops at the first step that is not `IDENTICAL`.

use super::event::SpecEvent;
use super::state::{SetRequest, SpecState};
use super::types::{dialect_wire, InheritedFrom};
use crate::chars::dprof::DprofState;
use fid_replay::ReplayOutcome;

/// The result of a replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecReplayed {
    /// The aggregate outcome.
    pub outcome: ReplayOutcome,
    /// One outcome per step run, in order.
    pub steps: Vec<ReplayOutcome>,
    /// The governing specs after the steps run.
    pub spec: SpecState,
    /// The profile store after the steps run.
    pub dprof: DprofState,
}

fn step(spec: &mut SpecState, dprof: &mut DprofState, recorded: &[u8]) -> ReplayOutcome {
    let e = match SpecEvent::from_canonical_bytes(recorded) {
        Ok(e) => e,
        Err(_) => return ReplayOutcome::NotReplayable,
    };
    let derived = match (e.inherited_from(), e.plan()) {
        (InheritedFrom::Explicit, _) => {
            let new = e.new_spec();
            let standards: Vec<String> = new.standards().iter().map(ToString::to_string).collect();
            let standards: Vec<&str> = standards.iter().map(String::as_str).collect();
            let profile = new.defaults_profile().to_string();
            let req = SetRequest {
                char: e.char().as_str(),
                dialect: dialect_wire(new.dialect()),
                standards: &standards,
                defaults_profile: &profile,
            };
            spec.set_governing_spec(dprof, e.principal(), &req, false).ok().and_then(|r| r.event().cloned())
        }
        (_, Some(plan)) => spec
            .create_characteristic(dprof, e.principal(), plan.as_str(), e.char().as_str(), false)
            .ok()
            .and_then(|c| c.event().cloned()),
        (_, None) => None,
    };
    match derived.map(|d| d.canonical_bytes()) {
        Some(Ok(bytes)) if bytes == recorded => ReplayOutcome::Identical,
        _ => ReplayOutcome::Divergent,
    }
}

/// Replays `events` (recorded canonical bytes, in journal order) from the start states.
pub fn replay(start_spec: &SpecState, start_dprof: &DprofState, events: &[Vec<u8>]) -> SpecReplayed {
    let mut spec = start_spec.clone();
    let mut dprof = start_dprof.clone();
    let mut steps = Vec::with_capacity(events.len());
    for recorded in events {
        let o = step(&mut spec, &mut dprof, recorded);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    SpecReplayed { outcome: fid_replay::aggregate(&steps), spec, steps, dprof }
}
