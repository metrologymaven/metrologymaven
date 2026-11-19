//! Exact replay of this leaf's journal events (determinism class D0).
//!
//! Starting from a given store state, each recorded event is re-executed: a publish
//! re-parses the profile content fetched by its digest and publishes it with the recorded
//! principal and `file`; a re-point re-runs with the recorded characteristics and target.
//! The step is `IDENTICAL` when the re-derived event's canonical bytes equal the recorded
//! bytes, `DIVERGENT` when they differ or the command now fails, and `NOT_REPLAYABLE` when
//! the recorded event cannot be read or the profile content is missing. Replay stops at the
//! first step that is not `IDENTICAL`; the outcome aggregates the steps run.

use super::event::DprofEvent;
use super::profile::{parse_profile, KeyVocabulary};
use super::store::DprofState;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Where replay fetches profile content: canonical (or any equivalent) JSON bytes by
/// digest, typically the content-addressed store.
pub trait ProfileContent {
    /// The bytes stored under `digest`, if any.
    fn content(&self, digest: &Digest) -> Option<&[u8]>;
}

impl ProfileContent for BTreeMap<Digest, Vec<u8>> {
    fn content(&self, digest: &Digest) -> Option<&[u8]> {
        self.get(digest).map(Vec::as_slice)
    }
}

impl ProfileContent for DprofState {
    fn content(&self, digest: &Digest) -> Option<&[u8]> {
        // Linear in the profile count, which is bounded by `MAX_PROFILES`.
        self.profiles().find(|p| p.doc().digest() == digest).map(|p| p.doc().canonical())
    }
}

/// The result of a replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replayed {
    /// The aggregate outcome.
    pub outcome: ReplayOutcome,
    /// One outcome per step run, in order.
    pub steps: Vec<ReplayOutcome>,
    /// The state after the steps run.
    pub state: DprofState,
}

fn step(state: &mut DprofState, recorded: &[u8], content: &dyn ProfileContent, vocab: &KeyVocabulary) -> ReplayOutcome {
    let event = match DprofEvent::from_canonical_bytes(recorded) {
        Ok(e) => e,
        Err(_) => return ReplayOutcome::NotReplayable,
    };
    let derived = match &event {
        DprofEvent::Publish(e) => {
            let bytes = match content.content(e.reference().digest()) {
                Some(b) => b,
                None => return ReplayOutcome::NotReplayable,
            };
            match parse_profile(bytes, vocab) {
                Ok(doc) if &doc.reference() == e.reference() => {
                    state.publish(doc, e.principal(), e.file(), false).ok().and_then(|p| p.into_event())
                }
                _ => None,
            }
        }
        DprofEvent::Repoint(e) => {
            let chars: Vec<&str> = e.changes().iter().map(|c| c.char().as_str()).collect();
            state.repoint(e.principal(), &chars, &e.to().to_string(), false).ok().and_then(|r| r.into_event())
        }
    };
    match derived.map(|d| d.canonical_bytes()) {
        Some(Ok(bytes)) if bytes == recorded => ReplayOutcome::Identical,
        _ => ReplayOutcome::Divergent,
    }
}

/// Replays `events` (recorded canonical bytes, in journal order) from `start`.
pub fn replay(start: &DprofState, events: &[Vec<u8>], content: &dyn ProfileContent, vocab: &KeyVocabulary) -> Replayed {
    let mut state = start.clone();
    let mut steps = Vec::with_capacity(events.len());
    for recorded in events {
        let o = step(&mut state, recorded, content, vocab);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    Replayed { outcome: fid_replay::aggregate(&steps), steps, state }
}
