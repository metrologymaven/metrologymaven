//! Exact replay of `unc.create_model@1` journal entries (determinism class D0, AC-5).
//!
//! Replay needs the entries, the content-addressed blob store the events point into, and the
//! release manifest bound to the running kernel. From a start store, each entry is re-run with
//! the same checks as the live command: the event is read strictly, its principal kind must
//! match the recorded author kind and may write on the recorded branch, the blob it names is
//! read back and must be the canonical bytes of a model with the recorded name, semver and
//! digest, the model must validate against the release manifest, the release manifest's
//! digest must be the recorded one, and the version rule must still give a new version.
//!
//! A step is `IDENTICAL` when the re-derived event's canonical bytes equal the recorded bytes;
//! `DIVERGENT` when any check fails or the bytes differ (a replay under another kernel
//! manifest diverges); `NOT_REPLAYABLE` when the bytes are not a readable event or the blob
//! cannot be read. Replay stops at the first step that is not `IDENTICAL`.
//!
//! **Replay cannot create live state.** Its result is a [`ReplayedModels`], a comparison-only
//! copy with read-only queries; it cannot be turned back into a [`ModelStore`].

use super::command::{branch_allowed, check_create, stored_from, Plan};
use super::event::CreateModelEvent;
use super::manifest::ReleaseManifest;
use super::{AuthorKind, ModelBlobs, ModelStore, StoredModel};
use fid_bus::principal::PrincipalKind;
use fid_kci::kci::sha256_digest;
use fid_kunc::model::UncertaintyModel;
use fid_replay::ReplayOutcome;

/// One recorded journal entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedEntry {
    /// The entry's sequence number.
    pub journal_entry: u64,
    /// The principal kind in the entry's envelope.
    pub principal_kind: PrincipalKind,
    /// The recorded event bytes.
    pub event: Vec<u8>,
}

/// The store a replay rebuilt: for comparison and inspection only.
#[derive(Debug, PartialEq, Eq)]
pub struct ReplayedModels(ModelStore);

impl ReplayedModels {
    /// Whether the rebuilt store equals `live`, entry for entry.
    pub fn matches(&self, live: &ModelStore) -> bool {
        self.0 == *live
    }

    /// [`ModelStore::get`] of the rebuilt store.
    pub fn get(&self, name: &str, semver: &str) -> Option<&StoredModel> {
        self.0.get(name, semver)
    }

    /// Number of rebuilt versions.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether nothing was rebuilt.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The result of a replay.
#[derive(Debug, PartialEq, Eq)]
pub struct ModelReplayed {
    /// The aggregate outcome.
    pub outcome: ReplayOutcome,
    /// One outcome per step run, in order.
    pub steps: Vec<ReplayOutcome>,
    /// The store after the steps run (comparison only).
    pub models: ReplayedModels,
}

fn step(
    store: &mut ModelStore,
    blobs: &dyn ModelBlobs,
    release: &ReleaseManifest,
    recorded: &RecordedEntry,
) -> ReplayOutcome {
    let Ok(e) = CreateModelEvent::from_canonical_bytes(&recorded.event) else {
        return ReplayOutcome::NotReplayable;
    };
    if AuthorKind::of(recorded.principal_kind) != Some(e.author_kind)
        || branch_allowed(e.author_kind, &e.branch).is_err()
    {
        return ReplayOutcome::Divergent;
    }
    let Ok(bytes) = blobs.get(&e.blob) else {
        return ReplayOutcome::NotReplayable;
    };
    if sha256_digest(&bytes) != e.digest {
        return ReplayOutcome::Divergent;
    }
    let Ok(model) = UncertaintyModel::parse(&bytes) else {
        return ReplayOutcome::Divergent;
    };
    if model.canonical_json() != bytes.as_slice() {
        return ReplayOutcome::Divergent;
    }
    match check_create(store, &model, release) {
        Ok(Plan::New) => {}
        Ok(Plan::AlreadyStored(_)) | Err(_) => return ReplayOutcome::Divergent,
    }
    let derived = CreateModelEvent {
        name: model.name().to_owned(),
        semver: model.semver().to_owned(),
        digest: model.digest(),
        blob: e.blob,
        author_kind: e.author_kind,
        by: e.by.clone(),
        branch: e.branch.clone(),
        kernel_manifest_digest: release.manifest_digest(),
    };
    if derived.to_canonical_bytes().ok().as_deref() != Some(recorded.event.as_slice()) {
        return ReplayOutcome::Divergent;
    }
    match store.insert(stored_from(&derived, recorded.journal_entry)) {
        Ok(()) => ReplayOutcome::Identical,
        Err(_) => ReplayOutcome::Divergent,
    }
}

/// Replays `entries` (in journal order) from `start`, using nothing but the entries, the blob
/// store and the release manifest.
pub fn replay(
    start: &ModelStore,
    blobs: &dyn ModelBlobs,
    release: &ReleaseManifest,
    entries: &[RecordedEntry],
) -> ModelReplayed {
    let mut store = start.clone();
    let mut steps = Vec::with_capacity(entries.len());
    for recorded in entries {
        let o = step(&mut store, blobs, release, recorded);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    ModelReplayed { outcome: fid_replay::aggregate(&steps), steps, models: ReplayedModels(store) }
}
