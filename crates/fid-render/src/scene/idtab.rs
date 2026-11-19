//! Render-ID allocation and entity lookup tables (spec leaf `VIZ.RENDER.SCENE.IDTAB`).
//!
//! Every drawable gets a dense, session-local `u32` render ID bound to one [`Identity`]: an
//! entity ID, or a [`DerivedId`] (source entity ID plus a [`ViewOp`]). The ID buffer and every
//! pick resolve through an [`IdTable`], so the rules here are what keeps a stale ID from ever
//! resolving to the wrong entity:
//!
//! - IDs are allocated from a monotonic counter: 1, 2, 3, ... in registration order. 0 means
//!   "no drawable" in the ID buffer and is never allocated (REQ-001).
//! - An ID is never reused within a session. A retired ID stays in the table as a tombstone,
//!   so a late ID-buffer read resolves to [`Slot::Retired`], never to another identity
//!   (REQ-005). Tombstones plus no reuse are the generation guard: an ID names exactly one
//!   identity for the whole session.
//! - A registration without identity fails with [`IdError::MissingIdentity`] and consumes no
//!   ID (REQ-003). A second registration of an identity that is still live fails with
//!   [`IdError::DuplicateIdentity`] (one live render ID per identity).
//! - At most [`RenderId::MAX`] = 2^32 - 2 IDs are allocated per table; then registration fails
//!   with [`IdError::IdSpaceExhausted`] instead of wrapping (REQ-006). `u32::MAX` is never
//!   allocated either, so it stays free as an ID-buffer clear value.
//!
//! Forward lookup is a `Vec` indexed by render ID (O(1)). Reverse lookup is a `BTreeMap` from
//! entity ID to its live render IDs (O(log n) worst case, chosen over `HashMap` for
//! deterministic iteration); each list is ascending and holds at most [`MAX_IDS_PER_ENTITY`]
//! entries, because an entity has at most one live entity drawable and one live drawable per
//! view operation.
//!
//! Render IDs are display identities only, never values of record (constitution C2).
//! Determinism D0: the same sequence of calls gives the same IDs and the same answers.

use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU32;

/// The ID of a model entity: a UUID (CONSTITUTION §5), the workspace's [`fid_uuid::Uuid`].
pub type EntityId = fid_uuid::Uuid;

/// Largest number of live render IDs one entity can have: its own drawable plus one per
/// [`ViewOp`].
pub const MAX_IDS_PER_ENTITY: usize = 1 + ViewOp::ALL.len();

/// A render ID: a nonzero `u32` no greater than [`RenderId::MAX`]. 0 in the ID buffer means
/// "no drawable" and is not a render ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RenderId(NonZeroU32);

impl RenderId {
    /// The largest render ID, 2^32 - 2 (REQ-006). `u32::MAX` is never allocated.
    pub const MAX: u32 = u32::MAX - 1;

    /// The render ID with this value, or `None` for 0 and for `u32::MAX`.
    pub const fn new(raw: u32) -> Option<RenderId> {
        if raw > RenderId::MAX {
            return None;
        }
        match NonZeroU32::new(raw) {
            Some(n) => Some(RenderId(n)),
            None => None,
        }
    }

    /// The `u32` value written to the ID buffer.
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl fmt::Display for RenderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The view operation that turns a source entity into a derived drawable. The codes are
/// pinned (NODE.md Design) and never reordered: new operations get new codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewOp {
    /// Deviation whisker (code 1).
    Whisker,
    /// Label (code 2).
    Label,
    /// Leader line (code 3).
    Leader,
    /// Ghost copy (code 4).
    Ghost,
    /// Clip-plane cap (code 5).
    ClipCap,
    /// Simplified proxy (code 6).
    Proxy,
}

impl ViewOp {
    /// Every operation in code order.
    pub const ALL: [ViewOp; 6] =
        [ViewOp::Whisker, ViewOp::Label, ViewOp::Leader, ViewOp::Ghost, ViewOp::ClipCap, ViewOp::Proxy];

    /// The pinned view-operation code, 1..=6.
    pub const fn code(self) -> u8 {
        match self {
            ViewOp::Whisker => 1,
            ViewOp::Label => 2,
            ViewOp::Leader => 3,
            ViewOp::Ghost => 4,
            ViewOp::ClipCap => 5,
            ViewOp::Proxy => 6,
        }
    }

    /// The operation with this code, or `None`.
    pub const fn from_code(code: u8) -> Option<ViewOp> {
        match code {
            1 => Some(ViewOp::Whisker),
            2 => Some(ViewOp::Label),
            3 => Some(ViewOp::Leader),
            4 => Some(ViewOp::Ghost),
            5 => Some(ViewOp::ClipCap),
            6 => Some(ViewOp::Proxy),
            _ => None,
        }
    }

    /// The pinned lowercase name (`whisker`, `label`, `leader`, `ghost`, `clip_cap`, `proxy`).
    pub const fn name(self) -> &'static str {
        match self {
            ViewOp::Whisker => "whisker",
            ViewOp::Label => "label",
            ViewOp::Leader => "leader",
            ViewOp::Ghost => "ghost",
            ViewOp::ClipCap => "clip_cap",
            ViewOp::Proxy => "proxy",
        }
    }

    /// The operation with this exact name, or `None` (case-sensitive).
    pub fn from_name(name: &str) -> Option<ViewOp> {
        ViewOp::ALL.into_iter().find(|op| op.name() == name)
    }
}

/// A derived identity: a drawable generated from a source entity by a view operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DerivedId {
    /// The entity the drawable is derived from.
    pub source: EntityId,
    /// How it is derived.
    pub op: ViewOp,
}

/// What a render ID is bound to (REQ-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Identity {
    /// A model entity drawn as itself.
    Entity(EntityId),
    /// A drawable derived from a source entity.
    Derived(DerivedId),
}

impl Identity {
    /// The entity this identity belongs to: the entity itself, or the derived ID's source. The
    /// entity-to-render lookup is keyed by it.
    pub const fn entity(&self) -> EntityId {
        match self {
            Identity::Entity(e) => *e,
            Identity::Derived(d) => d.source,
        }
    }
}

/// The state of an allocated render ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    /// The drawable is live and bound to this identity.
    Live(Identity),
    /// The ID was retired (tombstone). The identity it was bound to is kept for diagnostics;
    /// a pick that reads this ID must treat it as a miss.
    Retired(Identity),
}

impl Slot {
    /// The identity the ID is (or was) bound to.
    pub const fn identity(&self) -> Identity {
        match self {
            Slot::Live(i) | Slot::Retired(i) => *i,
        }
    }
}

/// Every error of the table. Errors are values; nothing here panics (CONSTITUTION §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdError {
    /// A drawable was registered with neither an entity ID nor a derived ID (REQ-003).
    MissingIdentity,
    /// The table has allocated its last render ID (REQ-006).
    IdSpaceExhausted,
    /// The value is 0, `u32::MAX` or an ID this table has not allocated.
    UnknownRenderId(u32),
    /// The identity already has this live render ID; it is not bound twice.
    DuplicateIdentity(RenderId),
    /// The render ID was already retired.
    AlreadyRetired(RenderId),
    /// Memory for the new entry could not be reserved.
    AllocationFailed,
}

impl IdError {
    /// The stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            IdError::MissingIdentity => "MissingIdentity",
            IdError::IdSpaceExhausted => "IdSpaceExhausted",
            IdError::UnknownRenderId(_) => "UnknownRenderId",
            IdError::DuplicateIdentity(_) => "DuplicateIdentity",
            IdError::AlreadyRetired(_) => "AlreadyRetired",
            IdError::AllocationFailed => "AllocationFailed",
        }
    }
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdError::MissingIdentity => write!(f, "MissingIdentity: drawable has neither an entity ID nor a derived ID"),
            IdError::IdSpaceExhausted => write!(f, "IdSpaceExhausted: no render ID left in this session"),
            IdError::UnknownRenderId(raw) => write!(f, "UnknownRenderId: {raw} is not an allocated render ID"),
            IdError::DuplicateIdentity(id) => write!(f, "DuplicateIdentity: identity is already live as render ID {id}"),
            IdError::AlreadyRetired(id) => write!(f, "AlreadyRetired: render ID {id} is already retired"),
            IdError::AllocationFailed => write!(f, "AllocationFailed: could not reserve memory for the entry"),
        }
    }
}

impl std::error::Error for IdError {}

/// The allocation step: the render ID that follows `allocated` IDs under `limit`, or
/// [`IdError::IdSpaceExhausted`]. `limit` is clamped to [`RenderId::MAX`]. Pure, so the
/// boundary at 2^32 - 2 is testable without allocating four billion entries.
pub const fn next_render_id(allocated: u32, limit: u32) -> Result<RenderId, IdError> {
    let limit = if limit > RenderId::MAX { RenderId::MAX } else { limit };
    if allocated >= limit {
        return Err(IdError::IdSpaceExhausted);
    }
    match RenderId::new(allocated + 1) {
        Some(id) => Ok(id),
        None => Err(IdError::IdSpaceExhausted),
    }
}

/// The render-ID tables of one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdTable {
    /// Slot of render ID `n` at index `n - 1`; its length is the number of IDs allocated.
    by_render: Vec<Slot>,
    /// Live render IDs per entity, ascending; an entity with none has no entry.
    by_entity: BTreeMap<EntityId, EntityIds>,
    /// Most IDs this table allocates, at most [`RenderId::MAX`].
    limit: u32,
    /// Number of live IDs.
    live: usize,
}

impl Default for IdTable {
    fn default() -> IdTable {
        IdTable::new()
    }
}

impl IdTable {
    /// An empty table that may allocate up to [`RenderId::MAX`] IDs.
    pub const fn new() -> IdTable {
        IdTable::with_limit(RenderId::MAX)
    }

    /// An empty table that allocates at most `limit` IDs (clamped to [`RenderId::MAX`]), for
    /// callers that bound a session's memory: each allocated ID keeps a slot for the rest of
    /// the session.
    pub const fn with_limit(limit: u32) -> IdTable {
        let limit = if limit > RenderId::MAX { RenderId::MAX } else { limit };
        IdTable { by_render: Vec::new(), by_entity: BTreeMap::new(), limit, live: 0 }
    }

    /// The most IDs this table allocates.
    pub const fn limit(&self) -> u32 {
        self.limit
    }

    /// The number of IDs allocated so far (live and retired); the last allocated ID equals it.
    pub fn allocated(&self) -> u32 {
        u32::try_from(self.by_render.len()).unwrap_or(u32::MAX)
    }

    /// The number of live IDs.
    pub const fn live_count(&self) -> usize {
        self.live
    }

    /// Binds a new render ID to `identity`. Fails, allocating nothing and leaving the table
    /// unchanged, with [`IdError::MissingIdentity`] for `None`, [`IdError::DuplicateIdentity`]
    /// if the identity is live already, [`IdError::IdSpaceExhausted`] at the limit, and
    /// [`IdError::AllocationFailed`] if memory cannot be reserved.
    pub fn register(&mut self, identity: Option<Identity>) -> Result<RenderId, IdError> {
        let identity = identity.ok_or(IdError::MissingIdentity)?;
        let entity = identity.entity();
        if let Some(id) = self.render_id_of(identity) {
            return Err(IdError::DuplicateIdentity(id));
        }
        let id = next_render_id(self.allocated(), self.limit)?;
        if self.render_ids(entity).len() >= MAX_IDS_PER_ENTITY {
            // Unreachable while identities are unique (one entity drawable plus one per view
            // operation), kept so a broken invariant fails closed instead of overflowing.
            return Err(IdError::AllocationFailed);
        }
        self.by_render.try_reserve(1).map_err(|_| IdError::AllocationFailed)?;
        // IDs grow monotonically, so appending keeps every list ascending.
        if !self.by_entity.entry(entity).or_default().push(id) {
            return Err(IdError::AllocationFailed);
        }
        self.by_render.push(Slot::Live(identity));
        self.live += 1;
        Ok(id)
    }

    /// Retires a live render ID: it stays allocated as a tombstone and is never handed out
    /// again. Returns the identity it was bound to.
    pub fn retire(&mut self, id: RenderId) -> Result<Identity, IdError> {
        let index = slot_index(id);
        let slot = self.by_render.get_mut(index).ok_or(IdError::UnknownRenderId(id.get()))?;
        let identity = match *slot {
            Slot::Live(identity) => identity,
            Slot::Retired(_) => return Err(IdError::AlreadyRetired(id)),
        };
        *slot = Slot::Retired(identity);
        self.live = self.live.saturating_sub(1);
        let entity = identity.entity();
        if let Some(ids) = self.by_entity.get_mut(&entity) {
            ids.remove(id);
            if ids.as_slice().is_empty() {
                self.by_entity.remove(&entity);
            }
        }
        Ok(identity)
    }

    /// The slot of an allocated render ID: [`Slot::Live`] with its identity, or
    /// [`Slot::Retired`] for a tombstone. Fails with [`IdError::UnknownRenderId`] for an ID
    /// this table has not allocated.
    pub fn identity(&self, id: RenderId) -> Result<Slot, IdError> {
        self.slot(id).ok_or(IdError::UnknownRenderId(id.get()))
    }

    /// Resolves a raw ID-buffer value: `Ok(None)` for 0 ("no drawable"), the slot for an
    /// allocated ID, and [`IdError::UnknownRenderId`] for anything else (fail closed: a value
    /// the table never handed out is a corrupt read, not a miss).
    pub fn resolve(&self, raw: u32) -> Result<Option<Slot>, IdError> {
        if raw == 0 {
            return Ok(None);
        }
        let id = RenderId::new(raw).ok_or(IdError::UnknownRenderId(raw))?;
        self.identity(id).map(Some)
    }

    /// The live render IDs of an entity, ascending: its own drawable's ID and the IDs of the
    /// drawables derived from it. Empty if it has none. At most [`MAX_IDS_PER_ENTITY`] long.
    pub fn render_ids(&self, entity: EntityId) -> &[RenderId] {
        match self.by_entity.get(&entity) {
            Some(ids) => ids.as_slice(),
            None => &[],
        }
    }

    /// The live render ID bound to exactly this identity, if any.
    pub fn render_id_of(&self, identity: Identity) -> Option<RenderId> {
        self.render_ids(identity.entity()).iter().copied().find(|&id| self.slot(id) == Some(Slot::Live(identity)))
    }

    fn slot(&self, id: RenderId) -> Option<Slot> {
        self.by_render.get(slot_index(id)).copied()
    }
}

/// The live render IDs of one entity, ascending, stored inline (no heap allocation per
/// entity): at most [`MAX_IDS_PER_ENTITY`] of them exist at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EntityIds {
    ids: [RenderId; MAX_IDS_PER_ENTITY],
    len: usize,
}

impl Default for EntityIds {
    fn default() -> EntityIds {
        EntityIds { ids: [RenderId(NonZeroU32::MIN); MAX_IDS_PER_ENTITY], len: 0 }
    }
}

impl EntityIds {
    fn as_slice(&self) -> &[RenderId] {
        self.ids.get(..self.len).unwrap_or(&[])
    }

    /// Appends `id`; false (and unchanged) when full.
    fn push(&mut self, id: RenderId) -> bool {
        match self.ids.get_mut(self.len) {
            Some(slot) => {
                *slot = id;
                self.len += 1;
                true
            }
            None => false,
        }
    }

    /// Removes `id` if present, keeping the order of the rest.
    fn remove(&mut self, id: RenderId) {
        if let Some(pos) = self.as_slice().iter().position(|&x| x == id) {
            if let Some(tail) = self.ids.get_mut(pos..self.len) {
                tail.rotate_left(1);
            }
            self.len -= 1;
        }
    }
}

/// Index of a render ID's slot. `RenderId` is at least 1 and at most 2^32 - 2, which fits
/// `usize` on every supported target (32 and 64 bit); on a narrower one the index is out of
/// range and the lookup misses.
fn slot_index(id: RenderId) -> usize {
    usize::try_from(id.get() - 1).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_is_small() {
        // One slot per allocated ID for the whole session: keep it compact.
        assert!(std::mem::size_of::<Slot>() <= 24);
        assert!(std::mem::size_of::<EntityIds>() <= 40);
    }
}
