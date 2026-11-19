//! Exact replay of a characteristic journal (determinism class D0): the
//! `gdt.set_governing_spec@1` events of this leaf and the `gdt.publish_defaults_profile@1`
//! and `gdt.repoint_defaults_profile@1` events of `GDT.CHAR.DPROF`, in journal order.
//!
//! From given start states, each recorded event is re-executed: an explicit set event re-runs
//! the set command with the recorded principal, characteristic and new spec; an inheritance
//! event re-runs creation in the recorded plan; a publish re-parses the profile content
//! fetched by digest and publishes it; a re-point re-runs through
//! [`SpecState::repoint_defaults_profile`], so governing specs move with their bindings. The
//! step is `IDENTICAL` when the re-derived event's canonical bytes equal the recorded bytes,
//! `DIVERGENT` when they differ or the command now fails, and `NOT_REPLAYABLE` when the
//! recorded event cannot be read as either kind or a published profile's content is missing.
//! Replay stops at the first step that is not `IDENTICAL`.

use super::event::SpecEvent;
use super::state::{SetRequest, SpecState};
use super::types::{dialect_wire, InheritedFrom};
use crate::chars::dprof::{parse_profile, DprofEvent, DprofState, KeyVocabulary, ProfileContent};
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

fn same(derived: Option<Vec<u8>>, recorded: &[u8]) -> ReplayOutcome {
    match derived {
        Some(bytes) if bytes == recorded => ReplayOutcome::Identical,
        _ => ReplayOutcome::Divergent,
    }
}

fn spec_step(spec: &mut SpecState, dprof: &mut DprofState, e: &SpecEvent, recorded: &[u8]) -> ReplayOutcome {
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
    same(derived.and_then(|d| d.canonical_bytes().ok()), recorded)
}

fn dprof_step(
    spec: &mut SpecState,
    dprof: &mut DprofState,
    e: &DprofEvent,
    recorded: &[u8],
    content: &dyn ProfileContent,
    vocab: &KeyVocabulary,
) -> ReplayOutcome {
    let derived = match e {
        DprofEvent::Publish(p) => {
            let bytes = match content.content(p.reference().digest()) {
                Some(b) => b,
                None => return ReplayOutcome::NotReplayable,
            };
            match parse_profile(bytes, vocab) {
                Ok(doc) if &doc.reference() == p.reference() => {
                    dprof.publish(doc, p.principal(), p.file(), false).ok().and_then(|r| r.into_event())
                }
                _ => None,
            }
        }
        DprofEvent::Repoint(r) => {
            let chars: Vec<&str> = r.changes().iter().map(|c| c.char().as_str()).collect();
            spec.repoint_defaults_profile(dprof, r.principal(), &chars, &r.to().to_string(), false)
                .ok()
                .and_then(|x| x.into_event())
        }
    };
    same(derived.and_then(|d| d.canonical_bytes().ok()), recorded)
}

fn step(
    spec: &mut SpecState,
    dprof: &mut DprofState,
    recorded: &[u8],
    content: &dyn ProfileContent,
    vocab: &KeyVocabulary,
) -> ReplayOutcome {
    if let Ok(e) = SpecEvent::from_canonical_bytes(recorded) {
        return spec_step(spec, dprof, &e, recorded);
    }
    match DprofEvent::from_canonical_bytes(recorded) {
        Ok(e) => dprof_step(spec, dprof, &e, recorded, content, vocab),
        Err(_) => ReplayOutcome::NotReplayable,
    }
}

/// Replays `events` (recorded canonical bytes, in journal order) from the start states.
/// `content` supplies published profiles by digest and `vocab` checks them, as in
/// `GDT.CHAR.DPROF`'s replay.
pub fn replay(
    start_spec: &SpecState,
    start_dprof: &DprofState,
    events: &[Vec<u8>],
    content: &dyn ProfileContent,
    vocab: &KeyVocabulary,
) -> SpecReplayed {
    let mut spec = start_spec.clone();
    let mut dprof = start_dprof.clone();
    let mut steps = Vec::with_capacity(events.len());
    for recorded in events {
        let o = step(&mut spec, &mut dprof, recorded, content, vocab);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    SpecReplayed { outcome: fid_replay::aggregate(&steps), spec, steps, dprof }
}
