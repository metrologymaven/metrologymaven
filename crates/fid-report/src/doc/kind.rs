//! The block-kind registry (REQ-007). Kinds are registered by the crates that own their
//! payload schemas (`RPT.MODEL.BLOCKS`, `RPT.BALLOON`, `RPT.FORMS`); this leaf ships only
//! the trait, the registry and, behind the `test-kinds` feature, two stub kinds.

use super::model::{BlockKindId, CanonicalJson};
use super::value::ValueLabel;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Most kinds one registry holds.
pub const MAX_KINDS: usize = 256;
/// Longest [`PayloadError::field`], in bytes; longer paths are cut at a character boundary.
pub const MAX_FIELD_BYTES: usize = 128;

/// Why a payload failed its kind's check: a JSON-pointer-like field path and a fixed
/// reason word. Both come from kind code, never from the payload's own text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadError {
    field: String,
    reason: &'static str,
}

impl PayloadError {
    /// `field` is cut to [`MAX_FIELD_BYTES`] and every character outside printable ASCII
    /// is replaced by `?`, so a kind that echoes payload keys cannot smuggle text out.
    pub fn new(field: &str, reason: &'static str) -> PayloadError {
        let mut f = String::new();
        for c in field.chars() {
            if f.len() >= MAX_FIELD_BYTES {
                break;
            }
            f.push(if c.is_ascii_graphic() { c } else { '?' });
        }
        PayloadError { field: f, reason }
    }

    pub fn field(&self) -> &str {
        &self.field
    }

    pub fn reason(&self) -> &'static str {
        self.reason
    }
}

impl fmt::Display for PayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.reason)
    }
}

/// A block kind: its ID, its payload schema version and its payload check.
pub trait BlockKind {
    fn id(&self) -> BlockKindId;
    /// Payload schema version, at least 1.
    fn version(&self) -> u32;
    /// Checks a payload. Must be bounded and must not panic.
    fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError>;
    /// The provenance labels this payload's values require. A kind whose values rest on
    /// uncertainty statements computes them with [`ValueLabel::required_by`] (fid-kunc
    /// `is_claimed()`, `rests_on_declared_input()`, `rests_on_uncharacterized_input()`).
    /// No default: every kind must answer, a kind without values returns the empty set.
    fn required_labels(&self, payload: &CanonicalJson) -> Result<BTreeSet<ValueLabel>, PayloadError>;
    /// RFC 6901 JSON pointers to every value field of this payload. Each must hold a
    /// `value_text@1` produced by `RPT.MODEL.FMT` (see [`super::value`]). No default: a
    /// kind without values returns an empty list.
    fn value_fields(&self, payload: &CanonicalJson) -> Result<Vec<String>, PayloadError>;
}

/// Registry errors (registration is code, not user input, but still fails closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindRegistryError {
    /// `duplicate_kind`: a kind with this ID is already registered.
    Duplicate(BlockKindId),
    /// `bad_version`: the kind reports version 0.
    BadVersion(BlockKindId),
    /// `too_many_kinds`: more than [`MAX_KINDS`].
    TooMany,
}

impl KindRegistryError {
    pub fn code(&self) -> &'static str {
        match self {
            KindRegistryError::Duplicate(_) => "duplicate_kind",
            KindRegistryError::BadVersion(_) => "bad_version",
            KindRegistryError::TooMany => "too_many_kinds",
        }
    }
}

impl fmt::Display for KindRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KindRegistryError::Duplicate(k) | KindRegistryError::BadVersion(k) => {
                write!(f, "{}: {}", self.code(), k.as_str())
            }
            KindRegistryError::TooMany => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for KindRegistryError {}

/// Registered kinds, keyed and iterated by kind ID.
#[derive(Default)]
pub struct KindRegistry {
    kinds: BTreeMap<BlockKindId, (u32, Box<dyn BlockKind>)>,
}

impl KindRegistry {
    pub fn new() -> KindRegistry {
        KindRegistry::default()
    }

    /// Registers a kind. The ID and version are read once, here; a kind whose `id()` or
    /// `version()` later changes is still found under the registered values.
    pub fn register(&mut self, kind: Box<dyn BlockKind>) -> Result<(), KindRegistryError> {
        let id = kind.id();
        let version = kind.version();
        if version == 0 {
            return Err(KindRegistryError::BadVersion(id));
        }
        if self.kinds.contains_key(&id) {
            return Err(KindRegistryError::Duplicate(id));
        }
        if self.kinds.len() >= MAX_KINDS {
            return Err(KindRegistryError::TooMany);
        }
        self.kinds.insert(id, (version, kind));
        Ok(())
    }

    /// The registered version and the kind.
    pub fn get(&self, id: &BlockKindId) -> Option<(u32, &dyn BlockKind)> {
        self.kinds.get(id).map(|(v, k)| (*v, k.as_ref()))
    }

    /// Registered kind IDs, ascending.
    pub fn ids(&self) -> impl Iterator<Item = &BlockKindId> {
        self.kinds.keys()
    }
}

impl fmt::Debug for KindRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.kinds.keys().map(BlockKindId::as_str)).finish()
    }
}
