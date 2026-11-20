//! Exact replay of `gdt.define_datum@1` events (determinism class D0).
//!
//! From a given start model, each recorded event is re-executed: the define command re-runs
//! with the recorded principal, label pattern, `replace` flag and new definition. The step is
//! `IDENTICAL` when the re-derived event's canonical bytes equal the recorded bytes,
//! `DIVERGENT` when they differ, the event names another plan, or the command now fails, and
//! `NOT_REPLAYABLE` when the recorded bytes are not a readable event. Replay stops at the
//! first step that is not `IDENTICAL`.

use super::event::DatumEvent;
use super::model::{DatumModel, DefineRequest, TargetRequest};
use fid_replay::ReplayOutcome;

/// The result of a replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatumReplayed {
    /// The aggregate outcome.
    pub outcome: ReplayOutcome,
    /// One outcome per step run, in order.
    pub steps: Vec<ReplayOutcome>,
    /// The model after the steps run.
    pub model: DatumModel,
}

fn step(model: &mut DatumModel, recorded: &[u8]) -> ReplayOutcome {
    let Ok(e) = DatumEvent::from_canonical_bytes(recorded) else {
        return ReplayOutcome::NotReplayable;
    };
    if e.plan() != model.plan() {
        return ReplayOutcome::Divergent;
    }
    let def = e.new_definition();
    let faces: Vec<&str> = def.nominal_faces().iter().map(|f| f.as_str()).collect();
    let targets: Vec<TargetRequest<'_>> = def
        .targets()
        .iter()
        .map(|t| TargetRequest { id: t.id(), kind: t.kind().as_str(), geometry: t.geometry().as_str() })
        .collect();
    let req = DefineRequest {
        label: def.label().as_str(),
        feature: def.feature_ref().as_str(),
        faces: &faces,
        feature_type: def.feature_type().as_str(),
        targets: &targets,
        replace: e.replace(),
    };
    let derived = model.define_datum(e.principal(), e.label_pattern(), &req, false).ok();
    match derived.as_ref().and_then(|d| d.event()).and_then(|d| d.canonical_bytes().ok()) {
        Some(bytes) if bytes == recorded => ReplayOutcome::Identical,
        _ => ReplayOutcome::Divergent,
    }
}

/// Replays `events` (recorded canonical bytes, in journal order) from `start`.
pub fn replay(start: &DatumModel, events: &[Vec<u8>]) -> DatumReplayed {
    let mut model = start.clone();
    let mut steps = Vec::with_capacity(events.len());
    for recorded in events {
        let o = step(&mut model, recorded);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    DatumReplayed { outcome: fid_replay::aggregate(&steps), steps, model }
}
