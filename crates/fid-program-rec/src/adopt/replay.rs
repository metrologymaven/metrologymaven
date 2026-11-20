//! Exact replay of adoption events (determinism class D0).
//!
//! From a given start store and the programs as they were, each recorded event is re-run:
//! the same checks as the live command run against the replayed store with the recorded
//! person, steps (or script version) and digest. A step is `IDENTICAL` when the re-derived
//! event's canonical bytes equal the recorded bytes, `DIVERGENT` when they differ or the
//! command now fails, and `NOT_REPLAYABLE` when the recorded bytes are not a readable event.
//! Replay stops at the first step that is not `IDENTICAL`.
//!
//! Replay verifies a journal; it does not authorise anyone. The recorded person was checked
//! by the bus and the handler when the entry was written, and the Record Journal's chain
//! (`CORE.JRN`) protects the entries. The store it returns is a copy for comparison.

use super::command::{prepare_steps, prepare_version, Programs, Selection};
use super::event::AdoptEvent;
use super::ProvenanceStore;
use fid_replay::ReplayOutcome;

/// The result of a replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptReplayed {
    /// The aggregate outcome.
    pub outcome: ReplayOutcome,
    /// One outcome per step run, in order.
    pub steps: Vec<ReplayOutcome>,
    /// The store after the steps run.
    pub store: ProvenanceStore,
}

fn step(store: &mut ProvenanceStore, programs: &dyn Programs, recorded: &[u8]) -> ReplayOutcome {
    let Ok(e) = AdoptEvent::from_canonical_bytes(recorded) else {
        return ReplayOutcome::NotReplayable;
    };
    let derived = match &e {
        AdoptEvent::Steps { human, program, steps, program_digest } => {
            let named: Vec<String> = steps.iter().map(ToString::to_string).collect();
            prepare_steps(store, programs, human, &program.to_string(), Selection::Ids(&named), program_digest.as_str())
        }
        AdoptEvent::Version { human, script, version_digest } => {
            prepare_version(store, human, script.as_str(), version_digest.as_str())
        }
    };
    match derived {
        Ok(d) if d.to_canonical_bytes().as_slice() == recorded => {
            super::command::apply(store, &d);
            ReplayOutcome::Identical
        }
        _ => ReplayOutcome::Divergent,
    }
}

/// Replays `events` (recorded canonical bytes, in journal order) from `start`.
pub fn replay(start: &ProvenanceStore, programs: &dyn Programs, events: &[Vec<u8>]) -> AdoptReplayed {
    let mut store = start.clone();
    let mut steps = Vec::with_capacity(events.len());
    for recorded in events {
        let o = step(&mut store, programs, recorded);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    AdoptReplayed { outcome: fid_replay::aggregate(&steps), steps, store }
}
