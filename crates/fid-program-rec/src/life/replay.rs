//! Exact replay of lifecycle journal entries (determinism class D0).
//!
//! From a start store, the drafts, the provenance and the checker as they were, each recorded
//! entry is re-run with the same checks as the live command and the recorded principal,
//! branch, artifact, digest and note. A step is `IDENTICAL` when the re-derived event's
//! canonical bytes equal the recorded bytes; `DIVERGENT` when they differ, the command now
//! fails, or the entry's envelope principal may not run it (a W3 command by anything but a
//! `human`, or a principal on the wrong side of the branch rule); `NOT_REPLAYABLE` when the
//! bytes are not a readable event. Replay stops at the first step that is not `IDENTICAL`.
//!
//! **Replay cannot create live lifecycle state.** Its result is a [`ReplayedLifecycle`], a
//! comparison-only copy with read-only queries and [`ReplayedLifecycle::matches`]; it cannot
//! be turned back into a [`LifeStore`], so a forged journal never reaches the store runs and
//! releases read. Replay records no authorship: the provenance is an input as it was.

use super::command::{branch_allowed, prepare, Actor, Drafts, Verb, View};
use super::event::LifeEvent;
use super::{ArtifactId, LifeStore, Lifecycle, ProgramChecker, RunGate};
use crate::adopt::{is_person, ProvenanceStore, RecordedEntry};
use fid_bus::principal::PrincipalKind;
use fid_replay::ReplayOutcome;
use fid_types::Digest;

/// The lifecycle a replay rebuilt: for comparison and inspection only.
#[derive(Debug, PartialEq, Eq)]
pub struct ReplayedLifecycle(LifeStore);

impl ReplayedLifecycle {
    /// Whether the rebuilt state equals `live`, version for version.
    pub fn matches(&self, live: &LifeStore) -> bool {
        self.0 == *live
    }

    /// [`LifeStore::lifecycle`] of the rebuilt state.
    pub fn lifecycle(&self, id: &ArtifactId, version: u32) -> Option<Lifecycle> {
        self.0.lifecycle(id, version)
    }

    /// [`LifeStore::run_gate`] of the rebuilt state.
    pub fn run_gate(&self, id: &ArtifactId, digest: &Digest) -> RunGate {
        self.0.run_gate(id, digest)
    }
}

/// The result of a replay.
#[derive(Debug, PartialEq, Eq)]
pub struct LifeReplayed {
    /// The aggregate outcome.
    pub outcome: ReplayOutcome,
    /// One outcome per step run, in order.
    pub steps: Vec<ReplayOutcome>,
    /// The lifecycle after the steps run (comparison only).
    pub lifecycle: ReplayedLifecycle,
}

/// Whether a principal of `kind` may have committed `e` (class rule, then branch rule).
fn may_commit(kind: PrincipalKind, e: &LifeEvent) -> bool {
    let person = is_person(kind);
    let w3 = matches!(e.verb, Verb::Release | Verb::Deprecate);
    !(w3 && !person) && branch_allowed(person, &e.branch).is_ok()
}

fn step(
    store: &mut LifeStore,
    drafts: &dyn Drafts,
    provenance: &ProvenanceStore,
    checker: &dyn ProgramChecker,
    recorded: &RecordedEntry,
) -> ReplayOutcome {
    let Ok(e) = LifeEvent::from_canonical_bytes(&recorded.event) else {
        return ReplayOutcome::NotReplayable;
    };
    if !may_commit(recorded.principal_kind, &e) {
        return ReplayOutcome::Divergent;
    }
    let derived = {
        let view = View { store: &*store, drafts, provenance, checker };
        prepare(&view, e.verb, &e.artifact, &e.digest, &Actor { by: &e.by, branch: &e.branch }, e.note.clone())
    };
    match derived {
        Ok(d) if d.event.to_canonical_bytes() == recorded.event => {
            store.apply(&d.change, recorded.journal_entry);
            ReplayOutcome::Identical
        }
        _ => ReplayOutcome::Divergent,
    }
}

/// Replays `entries` (in journal order) from `start`.
pub fn replay(
    start: &LifeStore,
    drafts: &dyn Drafts,
    provenance: &ProvenanceStore,
    checker: &dyn ProgramChecker,
    entries: &[RecordedEntry],
) -> LifeReplayed {
    let mut store = start.clone();
    let mut steps = Vec::with_capacity(entries.len());
    for recorded in entries {
        let o = step(&mut store, drafts, provenance, checker, recorded);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    LifeReplayed { outcome: fid_replay::aggregate(&steps), steps, lifecycle: ReplayedLifecycle(store) }
}
