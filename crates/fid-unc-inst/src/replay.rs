//! Exact replay of `unc.import_instrument_model@1` and `unc.bind_instrument_model@1` journal
//! entries (determinism class D0, AC-6).
//!
//! Replay needs only the entries and the content-addressed blob store the events point into.
//! From a start store, each entry is re-run with the live command's checks:
//!
//! - every event is read strictly; its principal kind must match the recorded author kind,
//!   which may write on the recorded branch;
//! - **import**: the blob is read back and must be the canonical bytes of a model with the
//!   recorded id and digest, and the version rule must give a new version;
//! - **bind**: the model must be in the replayed store with the recorded digest and its bytes
//!   must re-verify; for a device, the evidence blob is read back, its descriptor is
//!   validated again by `fid-hal-idl`, and the descriptor and device-identity digests and the
//!   evidence bytes are derived again from it, not from a live device.
//!
//! A step is `IDENTICAL` when the re-derived event's canonical bytes equal the recorded ones;
//! `DIVERGENT` when a check fails or the bytes differ; `NOT_REPLAYABLE` when the bytes are not
//! a readable event or a blob cannot be read. Replay stops at the first step that is not
//! `IDENTICAL`. Its result is a comparison-only copy of the store.

use crate::command::{binding_from, branch_allowed, check_import, evidence_of, stored_from, Plan};
use crate::event::{BindEvent, DeviceFacts, ImportEvent, InstEvent};
use crate::model::InstrumentModel;
use crate::store::{read_back, BindTarget, InstrumentBinding, InstrumentStore, StoredInstrumentModel};
use fid_canon::{parse_with_limits, CValue};
use fid_hal_idl::DeviceInstance;
use fid_kci::kci::sha256_digest;
use fid_replay::ReplayOutcome;
use fid_unc::model_store::{AuthorKind, ModelBlobs, RecordedEntry};

/// Deepest nesting of an evidence document: the descriptor's own limit plus one level.
const MAX_EVIDENCE_DEPTH: usize = fid_hal_idl::profile::MAX_DEPTH + 1;
/// Largest evidence document: a descriptor at its limit plus the device identity.
const MAX_EVIDENCE_BYTES: usize = fid_hal_idl::profile::MAX_INPUT_BYTES + 64 * 1024;

/// The store a replay rebuilt: for comparison and inspection only.
#[derive(Debug, PartialEq, Eq)]
pub struct ReplayedInstruments(InstrumentStore);

impl ReplayedInstruments {
    /// Whether the rebuilt store equals `live`, entry for entry.
    pub fn matches(&self, live: &InstrumentStore) -> bool {
        self.0 == *live
    }

    /// The rebuilt model `id`.
    pub fn model(&self, id: &str) -> Option<&StoredInstrumentModel> {
        self.0.model(id)
    }

    /// The rebuilt binding of `target`.
    pub fn binding(&self, target: &BindTarget) -> Option<&InstrumentBinding> {
        self.0.binding(target)
    }
}

/// The result of a replay.
#[derive(Debug, PartialEq, Eq)]
pub struct InstReplayed {
    /// The aggregate outcome.
    pub outcome: ReplayOutcome,
    /// One outcome per step run, in order.
    pub steps: Vec<ReplayOutcome>,
    /// The store after the steps run (comparison only).
    pub store: ReplayedInstruments,
}

fn import_step(store: &mut InstrumentStore, blobs: &dyn ModelBlobs, e: &ImportEvent, entry: u64) -> ReplayOutcome {
    let Ok(bytes) = blobs.get(&e.blob) else {
        return ReplayOutcome::NotReplayable;
    };
    if sha256_digest(&bytes) != e.digest {
        return ReplayOutcome::Divergent;
    }
    let Ok(model) = InstrumentModel::parse(&bytes) else {
        return ReplayOutcome::Divergent;
    };
    if model.canonical_json() != bytes.as_slice() || model.id() != e.model {
        return ReplayOutcome::Divergent;
    }
    match check_import(store, &model) {
        Ok(Plan::New) => {}
        Ok(Plan::AlreadyStored(_)) | Err(_) => return ReplayOutcome::Divergent,
    }
    let derived = ImportEvent {
        model: model.id(),
        digest: model.digest(),
        blob: e.blob,
        author_kind: e.author_kind,
        by: e.by.clone(),
        branch: e.branch.clone(),
    };
    if derived != *e {
        return ReplayOutcome::Divergent;
    }
    match store.insert_model(stored_from(&derived, entry)) {
        Ok(()) => ReplayOutcome::Identical,
        Err(_) => ReplayOutcome::Divergent,
    }
}

/// Re-derives a device's facts from its evidence blob.
fn device_step(blobs: &dyn ModelBlobs, d: &DeviceFacts) -> Result<DeviceFacts, ReplayOutcome> {
    let bytes = blobs.get(&d.evidence).map_err(|_| ReplayOutcome::NotReplayable)?;
    if sha256_digest(&bytes) != d.evidence.address {
        return Err(ReplayOutcome::Divergent);
    }
    let v = parse_with_limits(&bytes, MAX_EVIDENCE_BYTES, MAX_EVIDENCE_DEPTH).map_err(|_| ReplayOutcome::Divergent)?;
    let CValue::Obj(mut o) = v else { return Err(ReplayOutcome::Divergent) };
    let (Some(descriptor), Some(CValue::Str(device)), Some(instance)) =
        (o.remove("descriptor"), o.remove("device"), o.remove("instance"))
    else {
        return Err(ReplayOutcome::Divergent);
    };
    if !o.is_empty() || device != d.device {
        return Err(ReplayOutcome::Divergent);
    }
    let profile = fid_hal_idl::profile::validate_value(descriptor).map_err(|_| ReplayOutcome::Divergent)?;
    let instance = DeviceInstance::from_value(instance).map_err(|_| ReplayOutcome::Divergent)?;
    let ev = evidence_of(&device, &instance, &profile).map_err(|_| ReplayOutcome::Divergent)?;
    if ev.bytes != bytes {
        return Err(ReplayOutcome::Divergent);
    }
    Ok(DeviceFacts {
        device,
        device_instance_digest: ev.instance_digest,
        descriptor_digest: ev.descriptor_digest,
        evidence: d.evidence,
    })
}

fn bind_step(store: &mut InstrumentStore, blobs: &dyn ModelBlobs, e: &BindEvent, entry: u64) -> ReplayOutcome {
    let Some(stored) = store.model(&e.model) else {
        return ReplayOutcome::Divergent;
    };
    if stored.digest != e.model_digest || read_back(stored, blobs).is_err() {
        return ReplayOutcome::Divergent;
    }
    let device = match &e.device {
        None => None,
        Some(d) => match device_step(blobs, d) {
            Ok(f) => Some(f),
            Err(o) => return o,
        },
    };
    let derived = BindEvent {
        model: stored.model.clone(),
        model_digest: stored.digest,
        device,
        author_kind: e.author_kind,
        by: e.by.clone(),
        branch: e.branch.clone(),
    };
    if derived != *e {
        return ReplayOutcome::Divergent;
    }
    // The live command journals nothing for an unchanged binding.
    let binding = binding_from(&derived, entry);
    if store.binding(&binding.target).is_some_and(|b| b.same_as(&binding)) {
        return ReplayOutcome::Divergent;
    }
    match store.set_binding(binding) {
        Ok(()) => ReplayOutcome::Identical,
        Err(_) => ReplayOutcome::Divergent,
    }
}

fn step(store: &mut InstrumentStore, blobs: &dyn ModelBlobs, recorded: &RecordedEntry) -> ReplayOutcome {
    let Ok(event) = InstEvent::from_canonical_bytes(&recorded.event) else {
        return ReplayOutcome::NotReplayable;
    };
    let (kind, branch) = match &event {
        InstEvent::Import(e) => (e.author_kind, &e.branch),
        InstEvent::Bind(e) => (e.author_kind, &e.branch),
    };
    if AuthorKind::of(recorded.principal_kind) != Some(kind) || branch_allowed(kind, branch).is_err() {
        return ReplayOutcome::Divergent;
    }
    let outcome = match &event {
        InstEvent::Import(e) => import_step(store, blobs, e, recorded.journal_entry),
        InstEvent::Bind(e) => bind_step(store, blobs, e, recorded.journal_entry),
    };
    // The re-derived event equals the recorded one; its bytes must be the recorded bytes.
    if outcome == ReplayOutcome::Identical && event.to_canonical_bytes().ok().as_deref() != Some(&recorded.event[..]) {
        return ReplayOutcome::Divergent;
    }
    outcome
}

/// Replays `entries` (in journal order) from `start`, using nothing but the entries and the
/// blob store.
pub fn replay(start: &InstrumentStore, blobs: &dyn ModelBlobs, entries: &[RecordedEntry]) -> InstReplayed {
    let mut store = start.clone();
    let mut steps = Vec::with_capacity(entries.len());
    for recorded in entries {
        let o = step(&mut store, blobs, recorded);
        steps.push(o);
        if o != ReplayOutcome::Identical {
            break;
        }
    }
    InstReplayed { outcome: fid_replay::aggregate(&steps), steps, store: ReplayedInstruments(store) }
}
