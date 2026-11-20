//! Exact replay of lifecycle journal entries (determinism class D0).
//!
//! Replay needs only the journal: from a start store, the provenance and the checker as they
//! were, each recorded entry is re-run with the same checks as the live command and the
//! recorded principal, branch, artifact, digest and note. A `register` entry is re-run against
//! the content it carries ([`LifeEvent::content`]), never against the project's current
//! drafts, so the frozen bytes of every version are rebuilt from the journal alone.
//!
//! A step is `IDENTICAL` when the re-derived event's canonical bytes equal the recorded bytes;
//! `DIVERGENT` when they differ, the command now fails, the carried content is not a valid
//! artifact, or the entry's envelope principal may not run it (a W3 command by anything but a
//! `human`, or a principal on the wrong side of the branch rule); `NOT_REPLAYABLE` when the
//! bytes are not a readable event. Replay stops at the first step that is not `IDENTICAL`.
//!
//! **Replay cannot create live lifecycle state.** Its result is a [`ReplayedLifecycle`], a
//! comparison-only copy with read-only queries and [`ReplayedLifecycle::matches`]; it cannot
//! be turned back into a [`LifeStore`], so a forged journal never reaches the store runs and
//! releases read. Replay records no authorship: the provenance is an input as it was.

use super::command::{branch_allowed, prepare, Actor, Drafts, View};
use super::event::LifeEvent;
use super::{ArtifactId, LifeStore, Lifecycle, ProgramChecker, RunGate, ScriptBundle};
use crate::adopt::{is_person, ProvenanceStore, RecordedEntry, ScriptId};
use fid_bus::principal::PrincipalKind;
use fid_program::{Program, Uuid7};
use fid_replay::ReplayOutcome;
use fid_types::Digest;

/// The lifecycle a replay rebuilt: for comparison and inspection only.
#[derive(Debug, PartialEq, Eq)]
pub struct ReplayedLifecycle(LifeStore);

impl ReplayedLifecycle {
    /// Whether the rebuilt state equals `live`, version for version, bytes included.
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

    /// [`LifeStore::frozen_bytes`] of the rebuilt state.
    pub fn frozen_bytes(&self, id: &ArtifactId, version: u32) -> Option<&[u8]> {
        self.0.frozen_bytes(id, version)
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

/// The one draft a recorded `register` event carries, decoded strictly.
#[derive(Default)]
struct Recorded {
    program: Option<Program>,
    script: Option<(ScriptId, ScriptBundle)>,
}

impl Drafts for Recorded {
    fn program(&self, id: &Uuid7) -> Option<&Program> {
        self.program.as_ref().filter(|p| p.program_id == *id)
    }

    fn script(&self, id: &ScriptId) -> Option<&ScriptBundle> {
        self.script.as_ref().filter(|(s, _)| s == id).map(|(_, b)| b)
    }
}

/// The draft of a `register` event from its content; `None` if the content is not a valid
/// artifact of the event's kind. Other events get no draft.
fn recorded_draft(e: &LifeEvent) -> Option<Recorded> {
    let Some(content) = &e.content else { return Some(Recorded::default()) };
    match &e.artifact {
        ArtifactId::Program(_) => {
            Some(Recorded { program: Some(Program::parse(content.as_bytes()).ok()?), script: None })
        }
        ArtifactId::Script(s) => {
            Some(Recorded { program: None, script: Some((s.clone(), ScriptBundle::parse(content.as_bytes()).ok()?)) })
        }
    }
}

/// Whether a principal of `kind` may have committed `e` (class rule, then branch rule).
fn may_commit(kind: PrincipalKind, e: &LifeEvent) -> bool {
    let person = is_person(kind);
    (person || !e.verb.is_w3()) && branch_allowed(person, &e.branch).is_ok()
}

fn step(
    store: &mut LifeStore,
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
    let Some(drafts) = recorded_draft(&e) else {
        return ReplayOutcome::Divergent;
    };
    let derived = {
        let view = View { store: &*store, drafts: &drafts, provenance, checker };
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

/// Replays `entries` (in journal order) from `start`, using nothing but the entries, the
/// provenance and the checker.
pub fn replay(
    start: &LifeStore,
    provenance: &ProvenanceStore,
    checker: &dyn ProgramChecker,
    entries: &[RecordedEntry],
) -> LifeReplayed {
    let mut store = start.clone();
    let mut steps = Vec::with_capacity(entries.len());
    for recorded in entries {
        let o = step(&mut store, provenance, checker, recorded);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    LifeReplayed { outcome: fid_replay::aggregate(&steps), steps, lifecycle: ReplayedLifecycle(store) }
}
