//! The instrument models and bindings of one journal branch: a derived view of the journal.
//! An entry is added only after its event is committed.
//!
//! - Models are keyed by `im:<name>@<semver>` and never removed or replaced: the same id
//!   with another digest is `version_conflict` (REQ-UNC.INST.SCHEMA-003).
//! - A binding is keyed by its target, a device instance or the project default. Binding a
//!   target again replaces its current binding (a device may move to a new model version);
//!   the journal keeps every binding ever made.

use crate::model::{split_id, InstrumentModel};
use crate::InstError;
use fid_kci::kci::sha256_digest;
use fid_types::Digest;
use fid_unc::model_store::{AuthorKind, BlobKey, ModelBlobs};
use std::collections::BTreeMap;

/// One imported model version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredInstrumentModel {
    /// `im:<name>@<semver>`.
    pub model: String,
    /// SHA-256 of its canonical bytes.
    pub digest: Digest,
    /// Where the canonical bytes are.
    pub blob: BlobKey,
    /// Who imported it.
    pub author_kind: AuthorKind,
    /// The journal entry that imported it.
    pub imported_entry: u64,
}

/// What a model is bound to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindTarget {
    /// A device instance, by its device ID.
    Device(String),
    /// The project default.
    Project,
}

impl BindTarget {
    /// `device` or `project`.
    pub fn kind(&self) -> &'static str {
        match self {
            BindTarget::Device(_) => "device",
            BindTarget::Project => "project",
        }
    }
}

/// What a device binding recorded about the device (REQ-UNC.INST.SCHEMA-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceEvidence {
    /// SHA-256 of the canonical `DeviceInstance` (the HAL device identity).
    pub device_instance_digest: Digest,
    /// The `instrument-profile/v1` descriptor digest (HAL.IDL.PROFILE, without signature).
    pub descriptor_digest: Digest,
    /// The content-addressed evidence blob: the canonical
    /// `{"descriptor", "device", "instance"}` document the digests were taken from.
    pub evidence: BlobKey,
}

/// One binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentBinding {
    /// `im:<name>@<semver>`.
    pub model: String,
    /// The bound model's digest.
    pub model_digest: Digest,
    /// The target.
    pub target: BindTarget,
    /// For a device target, what was recorded about the device.
    pub device: Option<DeviceEvidence>,
    /// Who bound it.
    pub author_kind: AuthorKind,
    /// The journal entry that bound it.
    pub bound_entry: u64,
}

impl InstrumentBinding {
    /// Whether `other` binds the same model to the same device state (entry and author
    /// aside).
    pub fn same_as(&self, other: &InstrumentBinding) -> bool {
        self.model == other.model
            && self.model_digest == other.model_digest
            && self.target == other.target
            && self.device == other.device
    }
}

/// The models and bindings of one branch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstrumentStore {
    models: BTreeMap<String, StoredInstrumentModel>,
    bindings: BTreeMap<BindTarget, InstrumentBinding>,
}

impl InstrumentStore {
    /// An empty store.
    pub fn new() -> InstrumentStore {
        InstrumentStore::default()
    }

    /// The model `im:<name>@<semver>`.
    pub fn model(&self, id: &str) -> Option<&StoredInstrumentModel> {
        self.models.get(id)
    }

    /// Every model, by id.
    pub fn models(&self) -> impl Iterator<Item = &StoredInstrumentModel> {
        self.models.values()
    }

    /// The current binding of `target`.
    pub fn binding(&self, target: &BindTarget) -> Option<&InstrumentBinding> {
        self.bindings.get(target)
    }

    /// Every current binding, by target.
    pub fn bindings(&self) -> impl Iterator<Item = &InstrumentBinding> {
        self.bindings.values()
    }

    /// The model a selector names: unknown id `model_not_found`, another digest than the
    /// selector's `digest_mismatch`.
    pub fn lookup(&self, selector: &ModelSelector) -> Result<&StoredInstrumentModel, InstError> {
        let stored =
            self.models.get(&selector.id).ok_or_else(|| InstError::ModelNotFound { model: selector.id.clone() })?;
        match selector.digest {
            Some(d) if d != stored.digest => Err(InstError::mismatch(d, stored.digest)),
            _ => Ok(stored),
        }
    }

    /// Adds a model after its event was committed; an id is never overwritten.
    pub(crate) fn insert_model(&mut self, stored: StoredInstrumentModel) -> Result<(), InstError> {
        if self.models.contains_key(&stored.model) {
            return Err(InstError::Internal { reason: "an imported model version is never overwritten" });
        }
        self.models.insert(stored.model.clone(), stored);
        Ok(())
    }

    /// Sets a target's binding after its event was committed. The model must be imported
    /// with the binding's digest.
    pub(crate) fn set_binding(&mut self, binding: InstrumentBinding) -> Result<(), InstError> {
        match self.models.get(&binding.model) {
            Some(m) if m.digest == binding.model_digest => {}
            _ => return Err(InstError::Internal { reason: "a binding names an imported model" }),
        }
        self.bindings.insert(binding.target.clone(), binding);
        Ok(())
    }
}

/// `im:<name>@<semver>`, optionally followed by `#sha256:<hex>` (the digest the caller
/// expects).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelector {
    /// `im:<name>@<semver>`.
    pub id: String,
    /// The expected digest, if given.
    pub digest: Option<Digest>,
}

impl ModelSelector {
    /// Reads the text form; anything else is `bad_params`.
    pub fn parse(s: &str) -> Result<ModelSelector, InstError> {
        let (id, digest) = match s.split_once('#') {
            Some((id, d)) => (id, Some(Digest::parse(d).map_err(|_| InstError::bad("model digest is malformed"))?)),
            None => (s, None),
        };
        if split_id(id).is_none() {
            return Err(InstError::bad("model is not im:<name>@<semver>"));
        }
        Ok(ModelSelector { id: id.to_owned(), digest })
    }
}

/// Reads a stored model's bytes back and re-verifies them: they must hash to the stored
/// digest and be the canonical form of a model with the stored id (`digest_mismatch`).
pub fn read_back(stored: &StoredInstrumentModel, blobs: &dyn ModelBlobs) -> Result<InstrumentModel, InstError> {
    let bytes = blobs.get(&stored.blob).map_err(|d| InstError::BlobStore { detail: crate::echo(&d) })?;
    let found = sha256_digest(&bytes);
    if found != stored.digest {
        return Err(InstError::mismatch(stored.digest, found));
    }
    let model = InstrumentModel::parse(&bytes).map_err(|_| InstError::mismatch(stored.digest, found))?;
    if model.canonical_json() != bytes.as_slice() || model.id() != stored.model {
        return Err(InstError::mismatch(stored.digest, model.digest()));
    }
    Ok(model)
}
