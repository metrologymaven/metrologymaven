//! The active stage and the per-stage value cache. Spec node `CORE.PRJ.STAGE.SWITCH`
//! (CONSTITUTION C1 no value recomputed outside the kernel path, §4 ordered collections only,
//! §8 no panics).
//!
//! - [`set_active_stage`] is the whole logic of `project.set_active_stage@1` (W2): params
//!   `{stage}`; a stage that is not a live stage of the project (unknown, removed, or any
//!   other entity) is `bad_stage` (REQ-003). It returns the one [`ACTIVE_STAGE_SET`] event
//!   `{"stage"}`. Applying it ([`crate::Tree`]) swaps the active-stage pointer only: no slot,
//!   value or stage record moves or merges, so every stage's per-stage digest is unchanged
//!   (REQ-001, REQ-002).
//! - [`StageContext`] is the read API over a borrowed tree: the active stage, any element's
//!   slot in a stage, and its value. The tree is the only source of truth. A value is read by
//!   the slot's digest, which is the SHA-256 of the value bytes (the content address): first
//!   from a [`ValueCache`], then from a [`ValueStore`] (the content-addressed store). Every
//!   byte served is hashed and compared with the slot digest the tree holds now: a cache
//!   entry that does not match is stale or damaged, is removed and counted
//!   ([`ReadStats::stale`]), and is never served; a store read that does not match is
//!   `blob_corrupt`. A slot without a digest (`not_computed`, or `failed`) has no value
//!   (`no_value`). Nothing here computes a value: there is no recompute path (REQ-005).
//! - [`MemoryValueCache`]: a bounded in-memory LRU [`ValueCache`] (logical clock, no wall
//!   clock). `fid-project-cmd` adapts the `fid-store` cache and blob store to the two traits;
//!   this crate stays free of the store (layer model, wasm32).

use crate::container::ContainerView;
use crate::id::Uuid;
use crate::stage::{param_uuid, params_obj, read_stage, required, uuid_value, ListHead};
use crate::tree::{ElementState, Slot, Tree, TreeError, TreeEvent};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Event: the active stage was set.
pub const ACTIVE_STAGE_SET: &str = "ActiveStageSet";
/// Command name, without major.
pub const SET_ACTIVE_STAGE: &str = "project.set_active_stage";

// ------------------------------------------------------------------ the command

/// `project.set_active_stage@1`: params `{stage}`. `bad_stage` unless `stage` is a live stage
/// of the project (the active stage itself included). Returns the one `ActiveStageSet` event.
pub fn set_active_stage(view: &dyn ContainerView, params: &CValue, namespace: Uuid) -> Result<TreeEvent, TreeError> {
    let p = params_obj(params, &["stage"])?;
    let stage = param_uuid(required(p, "stage")?, "stage")?;
    let head = ListHead::read(view, namespace)?;
    let live = stage == head.active_stage || (head.stages.contains(&stage) && !read_stage(view, &stage)?.removed());
    if !live {
        return Err(TreeError::BadStage(stage));
    }
    let mut body = BTreeMap::new();
    body.insert("stage".to_string(), uuid_value(&stage));
    Ok(TreeEvent::new(ACTIVE_STAGE_SET, CValue::Obj(body)))
}

/// The stage of an `ActiveStageSet` body; `schema` for a wrong shape.
pub(crate) fn active_set(body: &CValue) -> Result<Uuid, TreeError> {
    let CValue::Obj(o) = body else {
        return Err(TreeError::Schema("event body must be an object".to_string()));
    };
    if o.len() != 1 {
        return Err(TreeError::Schema("event body must have exactly the keys [\"stage\"]".to_string()));
    }
    param_uuid(required(o, "stage")?, "stage")
}

// ------------------------------------------------------------------ value reads

/// Why a value could not be read. Codes are stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    /// The element is not an element of the project (`bad_reference`).
    BadReference(Uuid),
    /// The stage is not a live stage of the project (`stage_not_found`).
    StageNotFound(Uuid),
    /// The slot holds no digest, so there is no value to present (`no_value`).
    NoValue { element: Uuid, stage: Uuid, state: ElementState },
    /// The store holds nothing under the slot digest (`blob_missing`).
    BlobMissing(Digest),
    /// The store's bytes do not hash to the slot digest (`blob_corrupt`).
    BlobCorrupt(Digest),
    /// The store failed otherwise (`storage`).
    Storage(String),
}

impl ValueError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ValueError::BadReference(_) => "bad_reference",
            ValueError::StageNotFound(_) => "stage_not_found",
            ValueError::NoValue { .. } => "no_value",
            ValueError::BlobMissing(_) => "blob_missing",
            ValueError::BlobCorrupt(_) => "blob_corrupt",
            ValueError::Storage(_) => "storage",
        }
    }
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            ValueError::BadReference(id) => write!(f, "{code}: element {id} not found"),
            ValueError::StageNotFound(id) => write!(f, "{code}: {id} is not a live stage"),
            ValueError::NoValue { element, stage, state } => {
                write!(f, "{code}: element {element} has no value in stage {stage} ({})", state.as_str())
            }
            ValueError::BlobMissing(d) | ValueError::BlobCorrupt(d) => write!(f, "{code}: {d}"),
            ValueError::Storage(m) => write!(f, "{code}: {m}"),
        }
    }
}

impl std::error::Error for ValueError {}

/// The content-addressed store values are loaded from on a cache miss.
pub trait ValueStore {
    /// The bytes stored under `digest` (the caller verifies them).
    fn load(&self, digest: &Digest) -> Result<Vec<u8>, ValueError>;
}

/// An evictable cache of value bytes keyed by slot digest. Never a source of truth: whatever
/// it returns is verified against the slot digest before it is served.
pub trait ValueCache {
    fn get(&mut self, digest: &Digest) -> Option<Vec<u8>>;
    fn put(&mut self, digest: &Digest, bytes: &[u8]);
    fn remove(&mut self, digest: &Digest);
}

/// What the reads of one [`StageContext`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadStats {
    /// Values served from the cache (verified).
    pub hits: u64,
    /// Values loaded from the store (verified), after a miss or a stale entry.
    pub loads: u64,
    /// Cache entries whose bytes did not hash to the slot digest; removed, never served.
    pub stale: u64,
}

/// The SHA-256 of `bytes` as a `sha256:` digest.
pub fn value_digest(bytes: &[u8]) -> Digest {
    Digest::from_bytes(fid_canon::digest_bytes(bytes).0)
}

/// The read API over one tree state (see the module docs).
#[derive(Debug)]
pub struct StageContext<'t> {
    tree: &'t Tree,
    stats: ReadStats,
}

impl<'t> StageContext<'t> {
    pub fn new(tree: &'t Tree) -> StageContext<'t> {
        StageContext { tree, stats: ReadStats::default() }
    }

    /// The active stage.
    pub fn active(&self) -> Uuid {
        self.tree.active_stage()
    }

    /// What the reads so far did.
    pub fn stats(&self) -> ReadStats {
        self.stats
    }

    /// The element's slot in `stage` (a live stage).
    pub fn slot(&self, element: &Uuid, stage: &Uuid) -> Result<Slot, ValueError> {
        self.tree.target_stage(Some(stage)).map_err(|_| ValueError::StageNotFound(*stage))?;
        let e = self.tree.element(element).ok_or(ValueError::BadReference(*element))?;
        // Every element has a slot for every live stage (a tree invariant).
        e.slots().get(stage).copied().ok_or(ValueError::StageNotFound(*stage))
    }

    /// The element's value in the active stage.
    pub fn value(
        &mut self,
        element: &Uuid,
        cache: &mut dyn ValueCache,
        store: &dyn ValueStore,
    ) -> Result<Vec<u8>, ValueError> {
        let stage = self.active();
        self.value_in(element, &stage, cache, store)
    }

    /// The element's value in `stage`: by the slot digest the tree holds now, from the cache
    /// if its bytes hash to it, else from the store (and then cached).
    pub fn value_in(
        &mut self,
        element: &Uuid,
        stage: &Uuid,
        cache: &mut dyn ValueCache,
        store: &dyn ValueStore,
    ) -> Result<Vec<u8>, ValueError> {
        let slot = self.slot(element, stage)?;
        let Some(digest) = slot.digest else {
            return Err(ValueError::NoValue { element: *element, stage: *stage, state: slot.state });
        };
        if let Some(bytes) = cache.get(&digest) {
            if value_digest(&bytes) == digest {
                self.stats.hits += 1;
                return Ok(bytes);
            }
            cache.remove(&digest);
            self.stats.stale += 1;
        }
        let bytes = store.load(&digest)?;
        if value_digest(&bytes) != digest {
            return Err(ValueError::BlobCorrupt(digest));
        }
        self.stats.loads += 1;
        cache.put(&digest, &bytes);
        Ok(bytes)
    }
}

// ------------------------------------------------------------------ in-memory cache

/// A bounded in-memory LRU [`ValueCache`]: at most `limit` bytes of values; every put and hit
/// takes the next logical clock value, and the least recently used entries are evicted after
/// a put. A value larger than the limit is not stored.
#[derive(Debug, Clone, Default)]
pub struct MemoryValueCache {
    limit: u64,
    used: u64,
    clock: u64,
    items: BTreeMap<Digest, (u64, Vec<u8>)>,
    order: BTreeMap<u64, Digest>,
}

impl MemoryValueCache {
    pub fn new(limit: u64) -> MemoryValueCache {
        MemoryValueCache { limit, ..MemoryValueCache::default() }
    }

    /// Bytes held.
    pub fn used(&self) -> u64 {
        self.used
    }

    /// Entries held.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Drops every entry.
    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.used = 0;
    }

    /// Test hook: replaces the bytes held under `digest` (to show damage is never served).
    pub fn corrupt(&mut self, digest: &Digest, bytes: Vec<u8>) {
        if let Some((_, b)) = self.items.get_mut(digest) {
            self.used = self.used.saturating_sub(len64(b)).saturating_add(len64(&bytes));
            *b = bytes;
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock = self.clock.saturating_add(1);
        self.clock
    }
}

fn len64(b: &[u8]) -> u64 {
    u64::try_from(b.len()).unwrap_or(u64::MAX)
}

impl ValueCache for MemoryValueCache {
    fn get(&mut self, digest: &Digest) -> Option<Vec<u8>> {
        let t = self.tick();
        let (at, bytes) = self.items.get_mut(digest)?;
        self.order.remove(at);
        *at = t;
        self.order.insert(t, *digest);
        Some(bytes.clone())
    }

    fn put(&mut self, digest: &Digest, bytes: &[u8]) {
        let size = len64(bytes);
        if size > self.limit {
            return;
        }
        self.remove(digest);
        let t = self.tick();
        self.items.insert(*digest, (t, bytes.to_vec()));
        self.order.insert(t, *digest);
        self.used = self.used.saturating_add(size);
        while self.used > self.limit {
            let Some((&oldest, &d)) = self.order.iter().next() else { break };
            if d == *digest {
                break;
            }
            self.order.remove(&oldest);
            if let Some((_, b)) = self.items.remove(&d) {
                self.used = self.used.saturating_sub(len64(&b));
            }
        }
    }

    fn remove(&mut self, digest: &Digest) {
        if let Some((at, b)) = self.items.remove(digest) {
            self.order.remove(&at);
            self.used = self.used.saturating_sub(len64(&b));
        }
    }
}
