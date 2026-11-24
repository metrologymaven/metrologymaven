//! Read-only profile query API and GUI gating flags (spec node `TRUST.PROFILE.GATE.QUERY`).
//!
//! Provides the single read path ([`ProfileQuery`]) to the active governance profile as an
//! immutable, `Arc`-shared [`ProfileSnapshot`], plus the deterministic [`GuiGates`] table
//! derived from any profile document. Nothing in this module exposes `&mut self` on
//! [`ProfileQuery`] or [`ProfileSnapshot`], links CORE, panics, or uses `unsafe`.
//!
//! # Read-only guarantee (AC-4)
//!
//! A compiling control block confirms the read-only API:
//!
//! ```
//! use fid_profile::query::{GateSwitch, ProfileQuery, ProfileSnapshot, SelectionSlot, StoredSelection};
//! use fid_profile::ProfileName;
//!
//! let slot = SelectionSlot::unset();
//! let query = slot.query();
//! let snap: ProfileSnapshot = query.active().unwrap();
//! assert_eq!(snap.name(), ProfileName::Lean);
//! assert_eq!(snap.gui_gates().software_key_signing, GateSwitch::Enabled);
//! ```
//!
//! Neither [`ProfileQuery`] nor [`ProfileSnapshot`] exposes any mutating `&mut` method:
//!
//! ```compile_fail,E0599
//! use fid_profile::query::{ProfileQuery, SelectionSlot};
//! use fid_profile::ProfileName;
//!
//! let slot = SelectionSlot::unset();
//! let mut query = slot.query();
//! query.set_active(ProfileName::Regulated);
//! ```
//!
//! ```compile_fail,E0599
//! use fid_profile::query::{ProfileQuery, SelectionSlot};
//! use fid_profile::ProfileName;
//!
//! let slot = SelectionSlot::unset();
//! let query = slot.query();
//! let mut snap = query.active().unwrap();
//! snap.set_name(ProfileName::Regulated);
//! ```
//!
//! ```compile_fail,E0599
//! use fid_profile::query::{ProfileQuery, SelectionSlot};
//!
//! let slot = SelectionSlot::unset();
//! let query = slot.query();
//! let mut snap = query.active().unwrap();
//! let _ = snap.document_mut();
//! ```

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use fid_trust_canon::{serialize, Value};
use fid_types::Digest;

use crate::defs::{
    instantiate, AgentWrites, CanonicalProfile, MeaningCode, ProfileDoc, ProfileError, ProfileName,
    ProgramReleaseByOther, TenantMeaning,
};

/// Tenant configuration key written by `TRUST.PROFILE.SELECT` (`sign.set_profile@1`).
pub const CONFIG_KEY: &str = "trust.profile";

/// Binary enable/disable flag for a GUI capability gate (REQ-TRUST.PROFILE.GATE.QUERY-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GateSwitch {
    Enabled,
    Disabled,
}

impl GateSwitch {
    /// Wire form: `"enabled"` or `"disabled"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            GateSwitch::Enabled => "enabled",
            GateSwitch::Disabled => "disabled",
        }
    }
}

/// GUI gating flags derived deterministically from a [`ProfileDoc`]
/// (REQ-TRUST.PROFILE.GATE.QUERY-002, -004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiGates {
    /// True under Regulated (and any profile with `release_requires_signatures`).
    pub require_signature_before_release: bool,
    /// `Disabled` when `allow_software_keys` is false (Regulated); `Enabled` otherwise.
    pub software_key_signing: GateSwitch,
    /// `Disabled` when `agent_writes` is `ProposalsOnly` (Assistive-only); `Enabled` otherwise.
    pub agent_direct_write: GateSwitch,
    /// Ordered signature meaning codes required before release.
    pub required_meanings: Vec<MeaningCode>,
}

impl GuiGates {
    /// Derives GUI gating flags from `doc` using the fixed table of
    /// `REQ-TRUST.PROFILE.GATE.QUERY-004`.
    pub fn from_doc(doc: &ProfileDoc) -> Self {
        let p = doc.params();
        Self {
            require_signature_before_release: p.release_requires_signatures(),
            software_key_signing: if p.allow_software_keys() { GateSwitch::Enabled } else { GateSwitch::Disabled },
            agent_direct_write: match p.agent_writes() {
                AgentWrites::Branch => GateSwitch::Enabled,
                AgentWrites::ProposalsOnly => GateSwitch::Disabled,
            },
            required_meanings: p.required_meanings().to_vec(),
        }
    }

    /// Fail-closed GUI gates used when a stored profile selection fails to instantiate
    /// (Design: Regulated flags).
    pub fn fail_closed_regulated() -> Self {
        Self::from_doc(&ProfileDoc::builtin(ProfileName::Regulated))
    }

    /// Serialises the gates as RFC 8785 canonical JSON.
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, ProfileError> {
        let fields = vec![
            ("agent_direct_write".to_string(), Value::String(self.agent_direct_write.as_str().to_string())),
            ("require_signature_before_release".to_string(), Value::Bool(self.require_signature_before_release)),
            (
                "required_meanings".to_string(),
                Value::Array(self.required_meanings.iter().map(|m| Value::String(m.as_str().to_string())).collect()),
            ),
            ("software_key_signing".to_string(), Value::String(self.software_key_signing.as_str().to_string())),
        ];
        serialize(&Value::Object(fields))
            .map_err(|e| ProfileError::Malformed { reason: format!("cannot canonicalise gui gates: {e}") })
    }
}

/// Immutable, `Arc`-shared snapshot of an instantiated governance profile
/// (REQ-TRUST.PROFILE.GATE.QUERY-001, -005).
///
/// Once created, a [`ProfileSnapshot`] never changes even if the active profile is updated
/// concurrently on another thread. Exposes read-only accessors only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSnapshot {
    doc: Arc<ProfileDoc>,
    bytes: Arc<[u8]>,
    digest: Digest,
}

impl ProfileSnapshot {
    /// Wraps a verified [`CanonicalProfile`] as an immutable snapshot.
    pub fn from_canonical(profile: CanonicalProfile) -> Self {
        let digest = profile.digest();
        let bytes: Arc<[u8]> = Arc::from(profile.bytes());
        let doc = Arc::new(profile.document().clone());
        Self { doc, bytes, digest }
    }

    /// Instantiates a plain built-in profile as a snapshot.
    pub fn builtin(name: ProfileName) -> Result<Self, ProfileError> {
        CanonicalProfile::builtin(name).map(Self::from_canonical)
    }

    /// Built-in profile name.
    pub fn name(&self) -> ProfileName {
        self.doc.name()
    }

    /// Built-in definition version.
    pub fn version(&self) -> u32 {
        self.doc.version()
    }

    /// SHA-256 digest over the RFC 8785 canonical JSON document.
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// The instantiated [`ProfileDoc`].
    pub fn document(&self) -> &ProfileDoc {
        &self.doc
    }

    /// The RFC 8785 canonical JSON bytes whose SHA-256 equals [`Self::digest`].
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Signature meanings required before release, in required order.
    pub fn required_meanings(&self) -> &[MeaningCode] {
        self.doc.params().required_meanings()
    }

    /// Whether releasing a Result Package requires the required signatures.
    pub fn release_requires_signatures(&self) -> bool {
        self.doc.params().release_requires_signatures()
    }

    /// Whether software-held signing keys are accepted.
    pub fn allow_software_keys(&self) -> bool {
        self.doc.params().allow_software_keys()
    }

    /// Where agents may write.
    pub fn agent_writes(&self) -> AgentWrites {
        self.doc.params().agent_writes()
    }

    /// When program release requires a different human than the author.
    pub fn program_release_by_other(&self) -> ProgramReleaseByOther {
        self.doc.params().program_release_by_other()
    }

    /// Tenant-defined signature meanings, sorted by code.
    pub fn tenant_meanings(&self) -> &[TenantMeaning] {
        self.doc.params().tenant_meanings()
    }

    /// GUI gating flags derived from this snapshot's document.
    pub fn gui_gates(&self) -> GuiGates {
        GuiGates::from_doc(&self.doc)
    }
}

/// The stored profile selection under `CONFIG_KEY` (`trust.profile`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredSelection {
    /// No profile has been set; defaults to the Lean built-in (REQ-003).
    Unset,
    /// A canonical `profile/profile.json` document written by `sign.set_profile@1`.
    CanonicalDocument(Vec<u8>),
    /// A built-in profile name plus optional raw tenant parameter file bytes.
    Named { name: ProfileName, tenant_file: Option<Vec<u8>> },
}

/// Read-only storage backend supplying the current [`StoredSelection`].
pub trait SelectionReader: Send + Sync {
    /// Reads the stored selection under `trust.profile`.
    fn read_selection(&self) -> Result<StoredSelection, ProfileError>;
}

/// The single read-only query interface for the active governance profile
/// (REQ-TRUST.PROFILE.GATE.QUERY-001..005).
///
/// Every method takes `&self`; no mutation is reachable through this trait.
pub trait ProfileQuery: Send + Sync {
    /// Returns an immutable snapshot of the currently active profile, defaulting to Lean
    /// when no selection has been set, or `Err(ProfileError)` if the stored selection is
    /// malformed or invalid.
    fn active(&self) -> Result<ProfileSnapshot, ProfileError>;

    /// Returns the GUI gating flags for the active profile, failing closed to Regulated
    /// flags if the stored selection fails to instantiate.
    fn gui_gates_fail_closed(&self) -> GuiGates {
        match self.active() {
            Ok(snap) => snap.gui_gates(),
            Err(_) => GuiGates::fail_closed_regulated(),
        }
    }
}

/// Read-only [`ProfileQuery`] implementation backed by a [`SelectionReader`] with an
/// internal digest-keyed cache of instantiated [`ProfileSnapshot`]s.
///
/// Exposes no `&mut self` methods; concurrent updates to the underlying selection never
/// mutate any already-returned [`ProfileSnapshot`].
pub struct StoredProfileQuery<R: SelectionReader> {
    reader: R,
    cache: RwLock<BTreeMap<Digest, ProfileSnapshot>>,
}

impl<R: SelectionReader> StoredProfileQuery<R> {
    /// Creates a read-only query handle over `reader`.
    pub fn new(reader: R) -> Self {
        Self { reader, cache: RwLock::new(BTreeMap::new()) }
    }

    fn cached_or_insert(&self, profile: CanonicalProfile) -> ProfileSnapshot {
        let digest = profile.digest();
        if let Ok(guard) = self.cache.read() {
            if let Some(existing) = guard.get(&digest) {
                return existing.clone();
            }
        }
        let snap = ProfileSnapshot::from_canonical(profile);
        if let Ok(mut guard) = self.cache.write() {
            guard.entry(digest).or_insert_with(|| snap.clone()).clone()
        } else {
            snap
        }
    }
}

impl<R: SelectionReader> ProfileQuery for StoredProfileQuery<R> {
    fn active(&self) -> Result<ProfileSnapshot, ProfileError> {
        let selection = self.reader.read_selection()?;
        let profile = match selection {
            StoredSelection::Unset => CanonicalProfile::builtin(ProfileName::Lean)?,
            StoredSelection::CanonicalDocument(bytes) => CanonicalProfile::read(&bytes)?,
            StoredSelection::Named { name, tenant_file } => instantiate(name, tenant_file.as_deref())?,
        };
        Ok(self.cached_or_insert(profile))
    }
}

/// Thread-safe in-memory selection holder whose read-only view [`SelectionSlot::query`]
/// implements [`ProfileQuery`] without exposing any mutation path.
#[derive(Debug, Clone)]
pub struct SelectionSlot {
    inner: Arc<RwLock<StoredSelection>>,
}

impl SelectionSlot {
    /// Creates an unset slot (resolves to Lean by default, REQ-003).
    pub fn unset() -> Self {
        Self { inner: Arc::new(RwLock::new(StoredSelection::Unset)) }
    }

    /// Creates a slot initialized to `selection`.
    pub fn new(selection: StoredSelection) -> Self {
        Self { inner: Arc::new(RwLock::new(selection)) }
    }

    /// Replaces the stored selection (simulates a committed `sign.set_profile@1` update).
    /// Existing [`ProfileSnapshot`]s taken earlier are unaffected (REQ-005).
    pub fn set(&self, selection: StoredSelection) -> Result<(), ProfileError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| ProfileError::Malformed { reason: "selection lock poisoned".to_string() })?;
        *guard = selection;
        Ok(())
    }

    /// Produces a read-only [`StoredProfileQuery`] handle backed by this slot.
    pub fn query(&self) -> StoredProfileQuery<SlotReader> {
        StoredProfileQuery::new(SlotReader { inner: Arc::clone(&self.inner) })
    }
}

/// Read-only [`SelectionReader`] projecting a [`SelectionSlot`].
#[derive(Debug, Clone)]
pub struct SlotReader {
    inner: Arc<RwLock<StoredSelection>>,
}

impl SelectionReader for SlotReader {
    fn read_selection(&self) -> Result<StoredSelection, ProfileError> {
        let guard =
            self.inner.read().map_err(|_| ProfileError::Malformed { reason: "selection lock poisoned".to_string() })?;
        Ok(guard.clone())
    }
}
