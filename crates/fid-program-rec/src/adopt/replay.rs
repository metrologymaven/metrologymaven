//! Exact replay of adoption journal entries (determinism class D0).
//!
//! From a given start store and the programs as they were, each recorded entry is re-run:
//! the same checks as the live command, against the replayed copy, with the recorded person,
//! steps (or script version) and digest. A step is `IDENTICAL` when the re-derived event's
//! canonical bytes equal the recorded bytes, `DIVERGENT` when they differ, the command now
//! fails, or the entry's envelope principal is not `human`, and `NOT_REPLAYABLE` when the
//! recorded bytes are not a readable event. Replay stops at the first step that is not
//! `IDENTICAL`.
//!
//! **Replay cannot create live adoptions.** Its result is a [`ReplayedProvenance`]: a
//! comparison-only copy with read-only predicates and [`ReplayedProvenance::matches`]. It
//! cannot be turned back into a [`ProvenanceStore`], so a forged journal fed to replay never
//! reaches the store the release gate reads. On top of that, an entry is accepted only when the
//! principal kind in its journal envelope (written by the bus, not by the event) is `human`.

use super::command::{apply, prepare_steps, prepare_version, Programs, Selection};
use super::event::AdoptEvent;
use super::{ProvenanceStore, ScriptId};
use fid_bus::principal::PrincipalKind;
use fid_program::{Program, Uuid7};
use fid_replay::ReplayOutcome;
use fid_types::Digest;

/// One recorded adoption entry as read from the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedEntry {
    /// The entry's sequence number.
    pub journal_entry: u64,
    /// The principal kind in the entry's envelope.
    pub principal_kind: PrincipalKind,
    /// The recorded event bytes.
    pub event: Vec<u8>,
}

/// The provenance a replay rebuilt: for comparison and inspection only. There is no way to
/// get a [`ProvenanceStore`] out of it.
#[derive(Debug, PartialEq, Eq)]
pub struct ReplayedProvenance(ProvenanceStore);

impl ReplayedProvenance {
    /// Whether the rebuilt provenance equals `live`, record for record.
    pub fn matches(&self, live: &ProvenanceStore) -> bool {
        self.0 == *live
    }

    /// [`ProvenanceStore::unadopted_steps`] of the rebuilt provenance.
    pub fn unadopted_steps(&self, program: &Program) -> Vec<Uuid7> {
        self.0.unadopted_steps(program)
    }

    /// [`ProvenanceStore::unadopted_script`] of the rebuilt provenance.
    pub fn unadopted_script(&self, script: &ScriptId, version: &Digest) -> bool {
        self.0.unadopted_script(script, version)
    }
}

/// The result of a replay.
#[derive(Debug, PartialEq, Eq)]
pub struct AdoptReplayed {
    /// The aggregate outcome.
    pub outcome: ReplayOutcome,
    /// One outcome per step run, in order.
    pub steps: Vec<ReplayOutcome>,
    /// The provenance after the steps run (comparison only).
    pub provenance: ReplayedProvenance,
}

fn step(store: &mut ProvenanceStore, programs: &dyn Programs, recorded: &RecordedEntry) -> ReplayOutcome {
    let Ok(e) = AdoptEvent::from_canonical_bytes(&recorded.event) else {
        return ReplayOutcome::NotReplayable;
    };
    if recorded.principal_kind != PrincipalKind::Human {
        return ReplayOutcome::Divergent;
    }
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
        Ok(d) if d.event.to_canonical_bytes() == recorded.event => {
            apply(store, &d, recorded.journal_entry);
            ReplayOutcome::Identical
        }
        _ => ReplayOutcome::Divergent,
    }
}

/// Replays `entries` (in journal order) from `start`.
pub fn replay(start: &ProvenanceStore, programs: &dyn Programs, entries: &[RecordedEntry]) -> AdoptReplayed {
    let mut store = start.clone();
    let mut steps = Vec::with_capacity(entries.len());
    for recorded in entries {
        let o = step(&mut store, programs, recorded);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    AdoptReplayed { outcome: fid_replay::aggregate(&steps), steps, provenance: ReplayedProvenance(store) }
}
