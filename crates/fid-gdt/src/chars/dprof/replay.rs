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
use super::profile::{parse_profile, ProfileDoc, VocabularySource};
use super::store::DprofState;
use super::ProfileRef;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Where replay fetches profile content: canonical (or any equivalent) JSON bytes by
/// digest, together with the digest of the key vocabulary the profile was pinned to at
/// publish; typically the store snapshot or project the journal belongs to.
pub trait ProfileContent {
    /// The bytes stored under `digest` and their pinned vocabulary digest, if any.
    fn content(&self, digest: &Digest) -> Option<(&[u8], &Digest)>;
}

/// Profile digest to (content bytes, pinned vocabulary digest).
impl ProfileContent for BTreeMap<Digest, (Vec<u8>, Digest)> {
    fn content(&self, digest: &Digest) -> Option<(&[u8], &Digest)> {
        self.get(digest).map(|(b, v)| (b.as_slice(), v))
    }
}

impl ProfileContent for DprofState {
    fn content(&self, digest: &Digest) -> Option<(&[u8], &Digest)> {
        // Linear in the profile count, which is bounded by `MAX_PROFILES`.
        self.profiles().find(|p| p.doc().digest() == digest).map(|p| (p.doc().canonical(), p.doc().vocabulary()))
    }
}

/// The profile a publish event names, re-parsed against its pinned vocabulary. `Err(())` is
/// `NOT_REPLAYABLE` (content or vocabulary missing); `Ok(None)` is a mismatch (`DIVERGENT`).
pub(crate) fn pinned_doc(
    content: &dyn ProfileContent,
    vocabs: &dyn VocabularySource,
    reference: &ProfileRef,
) -> Result<Option<ProfileDoc>, ()> {
    let (bytes, pin) = content.content(reference.digest()).ok_or(())?;
    let vocab = vocabs.vocabulary(pin).ok_or(())?;
    Ok(parse_profile(bytes, vocab).ok().filter(|d| &d.reference() == reference && d.vocabulary() == pin))
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

fn step(
    state: &mut DprofState,
    recorded: &[u8],
    content: &dyn ProfileContent,
    vocabs: &dyn VocabularySource,
) -> ReplayOutcome {
    let event = match DprofEvent::from_canonical_bytes(recorded) {
        Ok(e) => e,
        Err(_) => return ReplayOutcome::NotReplayable,
    };
    let derived = match &event {
        DprofEvent::Publish(e) => match pinned_doc(content, vocabs, e.reference()) {
            Err(()) => return ReplayOutcome::NotReplayable,
            Ok(Some(doc)) => state.publish(doc, e.principal(), e.file(), false).ok().and_then(|p| p.into_event()),
            Ok(None) => None,
        },
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

/// Replays `events` (recorded canonical bytes, in journal order) from `start`. Each published
/// profile is checked against the vocabulary `content` says it was pinned to, looked up in
/// `vocabs`.
pub fn replay(
    start: &DprofState,
    events: &[Vec<u8>],
    content: &dyn ProfileContent,
    vocabs: &dyn VocabularySource,
) -> Replayed {
    let mut state = start.clone();
    let mut steps = Vec::with_capacity(events.len());
    for recorded in events {
        let o = step(&mut state, recorded, content, vocabs);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    Replayed { outcome: fid_replay::aggregate(&steps), steps, state }
}
