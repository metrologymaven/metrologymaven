//! The element tree: element records, element states, the element type registry and event
//! application. Spec node `CORE.PRJ.TREE.MODEL` (CONSTITUTION C6 untrusted text, §4 no
//! HashMap or HashSet, §5 stable IDs).
//!
//! - [`Tree`] holds every [`Element`] (ID, registered type, parent [`Group`], untrusted
//!   [`ElementName`], per-stage [`Slot`]s, visibility, metadata slot) and every group, keyed
//!   by ID in `BTreeMap`s, so every iteration is in ascending ID order (REQ-001).
//! - [`ElementState`] and [`ElementState::can_become`]: the only transitions are
//!   `not_computed` to `current` or `failed`, `current` to `outdated`, `failed` to
//!   `outdated`, and `outdated` to `current` or `failed` (REQ-003). An element's displayed
//!   state is its slot's in the active stage ([`Tree::displayed_state`], REQ-002).
//! - [`ElementTypeRegistry`]: domains register element types at start-up
//!   (`duplicate_type` on a second registration, REQ-005) and may register domain event
//!   types, each with a translation into core tree events (the registry dispatch of
//!   `apply`). Creating an element of an unregistered type fails with
//!   `unregistered_type` (REQ-004). Both registrations happen at start-up only, so
//!   `duplicate_type` and `duplicate_event` can never come from applying a commit.
//! - Every domain event type that can appear in a journal the tree is built from must be
//!   registered, with an empty translation (one returning no events) for events that do not
//!   touch the tree. An event type that is neither core nor registered is refused with
//!   `unknown_event`, which aborts its commit (fail closed). A translation that yields a
//!   non-core event is a bug in the registering domain and fails with `invariant_violated`.
//! - [`Tree::apply`] and [`Tree::apply_commit`]: event application, total (every input
//!   gives `Ok` or a typed error, nothing panics) and deterministic. A commit applies all
//!   its events or none: on the first refusal every change of the commit is rolled back.
//! - [`Tree::state_digest`]: the `fid-canon` (RFC 8785) SHA-256 of [`Tree::to_value`], the
//!   ordered tree (REQ-007).
//!
//! IDs (CONSTITUTION §5). The domain command that creates an entity draws or imports its ID
//! through [`IdService`]; applying the event imports it again through
//! [`IdService::import_uuid`] with the tree as the [`IdIndex`], so the nil and max UUID
//! (`bad_id`) and an ID the tree already holds (`id_collision`) are refused. The root group
//! and the implicit default stage get reserved UUIDv5s of the project namespace
//! ([`IdService::derive_v5`] over `{"kind": "project.tree_root"}` and
//! `{"kind": "project.default_stage"}`).
//!
//! Invariants, kept by every event and checked again by [`Tree::verify`]: every element's
//! parent is a group of the tree, every group but the root has a parent group (no orphans),
//! group parent chains end at the root within [`MAX_GROUP_DEPTH`] steps (no cycles), every
//! element type is registered, every element has exactly one slot per stage, element, group
//! and stage IDs are pairwise distinct, and every `current` slot carries a digest.
//!
//! Core events (type, then the exact keys of the body; IDs are canonical lowercase UUID text):
//! - [`GROUP_CREATED`] `{"id", "name", "parent"}`: a group under an existing group.
//! - [`ELEMENT_CREATED`] `{"id", "meta", "name", "parent", "type", "visible"}`: an element
//!   of a registered type in an existing group; `meta` is a JSON object (the metadata slot,
//!   whose contents `CORE.PRJ.META` owns), `visible` a boolean. Every slot starts
//!   `not_computed` with no digest.
//! - [`ELEMENT_SLOT_SET`] `{"digest", "element", "stage", "state"}`: moves one slot to
//!   `state` (an allowed transition) and sets its value digest (`sha256:` text, or `null`;
//!   a `current` slot must carry a digest).
//!
//! Check order of an event, first failure wins: body shape (`schema`), name (`name`), type
//! (`unregistered_type`), references (`bad_reference`, `bad_stage`), depth (`hierarchy`),
//! ID (`bad_id`, `id_collision`), then the transition (`invalid_transition`).
//!
//! Stages (spec node `CORE.PRJ.STAGE.MODEL`, see [`crate::stage`]): the tree also holds the
//! explicit stage records and applies `StageAdded`, `StageRemoved` and `StagesReordered`.
//! [`Tree::stages`] is the project's stage sequence, removed stages included;
//! [`Tree::live_stages`] leaves the removed ones out. Every element has a slot for every
//! live stage and may keep slots for removed ones (their data stays); a new element gets
//! one slot per live stage, and slots of removed stages cannot change. A state with
//! explicit stages adds the key `"stage_records": {<id>: <stage body>}` to
//! [`Tree::to_value`]; a project without explicit stages keeps the exact form above, so its
//! digest is unchanged. [`Tree::stage_digest`] is the digest of one stage's data.

use crate::container::{is_bidi_control, is_nfc, Label};
use crate::id::{IdError, IdIndex, IdService, ReplaySource, Uuid};
use crate::stage::{
    check_permutation, removed_stage, reordered, stage_from_added, stage_list_id, Stage, DEFAULT_STAGE_LABEL,
    STAGES_REORDERED, STAGE_ADDED, STAGE_EVENTS, STAGE_KIND, STAGE_LIST_KIND, STAGE_REMOVED,
};
use crate::stage_switch::{active_set, ACTIVE_STAGE_SET};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, OnceLock};

/// Longest element or group name, in Unicode scalar values (REQ-006).
pub const MAX_NAME_CHARS: usize = 256;
/// Longest element type ID, in bytes.
pub const MAX_TYPE_ID_BYTES: usize = 128;
/// Longest display kind, in bytes.
pub const MAX_DISPLAY_KIND_BYTES: usize = 64;
/// Longest event type, in bytes (the bus limit).
pub const MAX_EVENT_TYPE_BYTES: usize = 128;
/// Most groups on the path from the root to any group, the root excluded.
pub const MAX_GROUP_DEPTH: usize = 64;
/// Entity kind of a group in the state view.
pub const GROUP_KIND: &str = "project.group";
/// Entity kind of the group child-index entity in the state view.
pub const GROUP_LIST_KIND: &str = "project.group_list";
/// Entity kind of the pre-order tree index entity in the state view (`CORE.PRJ.TREE.QUERY`).
pub const TREE_INDEX_KIND: &str = "project.tree_index";
/// Core event: a group was created.
pub const GROUP_CREATED: &str = "GroupCreated";
/// Core event: an element was created.
pub const ELEMENT_CREATED: &str = "ElementCreated";
/// Core event: one slot of an element changed state.
pub const ELEMENT_SLOT_SET: &str = "ElementSlotSet";
/// Core event: elements and/or groups moved under a target group (`CORE.PRJ.TREE.GROUP`).
pub const ELEMENTS_MOVED: &str = "ElementsMoved";
/// Core event: an element or group was renamed (`CORE.PRJ.TREE.GROUP`).
pub const RENAMED: &str = "Renamed";
/// Core event: saved visibility set on elements (`CORE.PRJ.TREE.GROUP`).
pub const VISIBILITY_SET: &str = "VisibilitySet";
/// Maximum number of entity IDs in one batch move or visibility command (`CORE.PRJ.TREE.GROUP`).
pub const MAX_BATCH_IDS: usize = 10_000;
/// The core event types, sorted.
pub const CORE_EVENTS: [&str; 6] =
    [ELEMENT_CREATED, ELEMENT_SLOT_SET, ELEMENTS_MOVED, GROUP_CREATED, RENAMED, VISIBILITY_SET];
/// Type IDs under this prefix are reserved for core entities.
pub const RESERVED_TYPE_PREFIX: &str = "project.";

// ------------------------------------------------------------------ errors

/// Why an event, a registration or a check was refused. Codes are stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// An event body, type definition or event type does not have the required shape (`schema`).
    Schema(String),
    /// A name is longer than [`MAX_NAME_CHARS`], holds a control or bidi control, or is not NFC (`name`).
    Name,
    /// The element's type is not registered (`unregistered_type`).
    UnregisteredType(String),
    /// A type ID was registered twice (`duplicate_type`).
    DuplicateType(String),
    /// A domain event type was registered twice or names a core event (`duplicate_event`).
    DuplicateEvent(String),
    /// The event type is neither core nor registered (`unknown_event`).
    UnknownEvent(String),
    /// A named parent group or element does not exist or has the wrong entity kind (`bad_reference`).
    BadReference(String),
    /// A named entity does not exist (`not_found`).
    NotFound(String),
    /// A named stage is not a stage of the project (`bad_stage`).
    BadStage(Uuid),
    /// A group would be deeper than [`MAX_GROUP_DEPTH`] or become its own ancestor (`hierarchy`).
    Hierarchy(String),
    /// The requested state change is not an allowed transition (`invalid_transition`).
    InvalidTransition { from: ElementState, to: ElementState },
    /// The ID is already in the project (`id_collision`).
    IdCollision(Uuid),
    /// The ID service refused the ID or failed (`bad_id`, ...).
    Id(IdError),
    /// The tree breaks one of its invariants (`invariant_violated`).
    Invariant(String),
    /// A stage label or serial breaks the text rules (`label`).
    Label,
    /// A named stage (by ID or label) is not a live stage of the project (`stage_not_found`).
    StageNotFound(String),
    /// More than one live stage has the label (`ambiguous_stage`).
    AmbiguousStage(String),
    /// The stage to remove is the active stage (`stage_active`).
    StageActive(Uuid),
    /// A reorder list is not a permutation of the run's live stages (`bad_order`).
    BadOrder(String),
    /// A query continuation token is malformed (`bad_token`, `CORE.PRJ.TREE.QUERY`).
    BadToken(String),
    /// A query continuation token was issued for an older branch head (`stale_token`, `CORE.PRJ.TREE.QUERY`).
    StaleToken(String),
}

impl TreeError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            TreeError::Schema(_) => "schema",
            TreeError::Name => "name",
            TreeError::UnregisteredType(_) => "unregistered_type",
            TreeError::DuplicateType(_) => "duplicate_type",
            TreeError::DuplicateEvent(_) => "duplicate_event",
            TreeError::UnknownEvent(_) => "unknown_event",
            TreeError::BadReference(_) => "bad_reference",
            TreeError::NotFound(_) => "not_found",
            TreeError::BadStage(_) => "bad_stage",
            TreeError::Hierarchy(_) => "hierarchy",
            TreeError::InvalidTransition { .. } => "invalid_transition",
            TreeError::IdCollision(_) => "id_collision",
            TreeError::Id(e) => e.code(),
            TreeError::Invariant(_) => "invariant_violated",
            TreeError::Label => "label",
            TreeError::StageNotFound(_) => "stage_not_found",
            TreeError::AmbiguousStage(_) => "ambiguous_stage",
            TreeError::StageActive(_) => "stage_active",
            TreeError::BadOrder(_) => "bad_order",
            TreeError::BadToken(_) => "bad_token",
            TreeError::StaleToken(_) => "stale_token",
        }
    }
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreeError::Schema(m)
            | TreeError::UnregisteredType(m)
            | TreeError::DuplicateType(m)
            | TreeError::DuplicateEvent(m)
            | TreeError::UnknownEvent(m)
            | TreeError::BadReference(m)
            | TreeError::NotFound(m)
            | TreeError::Hierarchy(m)
            | TreeError::Invariant(m)
            | TreeError::BadOrder(m)
            | TreeError::StageNotFound(m)
            | TreeError::AmbiguousStage(m)
            | TreeError::BadToken(m)
            | TreeError::StaleToken(m) => write!(f, "{}: {m}", self.code()),
            TreeError::Label => write!(f, "label: must be 1..=256 characters, NFC, no control"),
            TreeError::StageActive(s) => write!(f, "stage_active: {s} is the active stage"),
            TreeError::Name => write!(f, "name: must be 0..={MAX_NAME_CHARS} characters, NFC, no control"),
            TreeError::BadStage(s) => write!(f, "bad_stage: {s}"),
            TreeError::InvalidTransition { from, to } => {
                write!(f, "invalid_transition: {} -> {}", from.as_str(), to.as_str())
            }
            TreeError::IdCollision(id) => write!(f, "id_collision: {id}"),
            TreeError::Id(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TreeError {}

impl From<IdError> for TreeError {
    fn from(e: IdError) -> Self {
        match e {
            IdError::IdCollision(id) => TreeError::IdCollision(id),
            other => TreeError::Id(other),
        }
    }
}

fn schema(m: &str) -> TreeError {
    TreeError::Schema(m.to_string())
}

fn invariant(m: String) -> TreeError {
    TreeError::Invariant(m)
}

// ------------------------------------------------------------------ states

/// The state of one slot of an element (REQ-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ElementState {
    Current,
    Outdated,
    Failed,
    NotComputed,
}

impl ElementState {
    /// Every state.
    pub const ALL: [ElementState; 4] =
        [ElementState::Current, ElementState::Outdated, ElementState::Failed, ElementState::NotComputed];

    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            ElementState::Current => "current",
            ElementState::Outdated => "outdated",
            ElementState::Failed => "failed",
            ElementState::NotComputed => "not_computed",
        }
    }

    /// Parses the exact wire name; anything else is `None`.
    pub fn parse(text: &str) -> Option<ElementState> {
        ElementState::ALL.into_iter().find(|s| s.as_str() == text)
    }

    /// Whether a slot in this state may move to `to` (REQ-003). Staying in the same state
    /// is not a transition and is refused too.
    pub fn can_become(self, to: ElementState) -> bool {
        use ElementState::{Current, Failed, NotComputed, Outdated};
        matches!(
            (self, to),
            (NotComputed, Current)
                | (NotComputed, Failed)
                | (Current, Outdated)
                | (Failed, Outdated)
                | (Outdated, Current)
                | (Outdated, Failed)
        )
    }
}

impl fmt::Display for ElementState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ------------------------------------------------------------------ names

/// An element or group name: untrusted text (CONSTITUTION C6) of 0 to [`MAX_NAME_CHARS`]
/// characters with no Cc control, no bidi control and in NFC (the label rules of
/// `CORE.PRJ.CONT.HIER`, except that a name may be empty). Stored and shown verbatim, never
/// interpreted, not required to be unique: identity is the ID only (REQ-006).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ElementName(String);

impl ElementName {
    /// Checks the text; `name` if it is too long, holds a control or bidi control, or is not NFC.
    pub fn new(text: &str) -> Result<ElementName, TreeError> {
        if text.chars().nth(MAX_NAME_CHARS).is_some()
            || text.chars().any(|c| c.is_control() || is_bidi_control(c))
            || !is_nfc(text)
        {
            return Err(TreeError::Name);
        }
        Ok(ElementName(text.to_string()))
    }

    /// The untrusted text, for display or storage only.
    pub fn as_untrusted_str(&self) -> &str {
        &self.0
    }
}

// ------------------------------------------------------------------ type registry

/// An element type ID: dotted lowercase segments `[a-z][a-z0-9_]*`, at least two, at most
/// [`MAX_TYPE_ID_BYTES`] bytes, e.g. `geo.mesh`.
pub fn is_type_id(s: &str) -> bool {
    let segment_ok = |seg: &str| {
        let b = seg.as_bytes();
        b.first().is_some_and(u8::is_ascii_lowercase)
            && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
    };
    s.len() <= MAX_TYPE_ID_BYTES && s.split('.').count() >= 2 && s.split('.').all(segment_ok)
}

/// An event type: the bus grammar `[A-Za-z][A-Za-z0-9_.@]*`, 1 to [`MAX_EVENT_TYPE_BYTES`] bytes.
pub fn is_event_type(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() <= MAX_EVENT_TYPE_BYTES
        && b.first().is_some_and(u8::is_ascii_alphabetic)
        && b.iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'@'))
}

/// A registered element type.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementTypeDef {
    type_id: String,
    payload_schema: CValue,
    display_kind: String,
}

impl ElementTypeDef {
    /// Checks a definition: a type ID ([`is_type_id`], not under [`RESERVED_TYPE_PREFIX`]),
    /// a payload schema that is a JSON object, and a display kind of 1 to
    /// [`MAX_DISPLAY_KIND_BYTES`] bytes of `[a-z0-9_]`. `schema` otherwise.
    pub fn new(type_id: &str, payload_schema: CValue, display_kind: &str) -> Result<ElementTypeDef, TreeError> {
        if !is_type_id(type_id) {
            return Err(schema("type id must be dotted lowercase segments, at most 128 bytes"));
        }
        if type_id.starts_with(RESERVED_TYPE_PREFIX) {
            return Err(schema("type ids under project. are reserved for core entities"));
        }
        if !matches!(payload_schema, CValue::Obj(_)) {
            return Err(schema("payload schema must be an object"));
        }
        let dk = display_kind.as_bytes();
        if dk.is_empty()
            || dk.len() > MAX_DISPLAY_KIND_BYTES
            || !dk.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        {
            return Err(schema("display kind must be 1..=64 bytes of [a-z0-9_]"));
        }
        Ok(ElementTypeDef { type_id: type_id.to_string(), payload_schema, display_kind: display_kind.to_string() })
    }

    pub fn type_id(&self) -> &str {
        &self.type_id
    }

    pub fn payload_schema(&self) -> &CValue {
        &self.payload_schema
    }

    pub fn display_kind(&self) -> &str {
        &self.display_kind
    }
}

/// One event for the tree: its type and canonical body.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeEvent {
    pub event_type: String,
    pub body: CValue,
}

impl TreeEvent {
    pub fn new(event_type: &str, body: CValue) -> TreeEvent {
        TreeEvent { event_type: event_type.to_string(), body }
    }
}

/// Translates a domain event body into the core tree events it implies (often none). A
/// plain function, so the translation holds no state and is deterministic.
pub type DomainTranslate = fn(&CValue) -> Result<Vec<TreeEvent>, TreeError>;

/// The element types and domain event types of a running application, fixed at start-up.
#[derive(Clone, Default)]
pub struct ElementTypeRegistry {
    types: BTreeMap<String, ElementTypeDef>,
    events: BTreeMap<String, DomainTranslate>,
}

impl fmt::Debug for ElementTypeRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ElementTypeRegistry")
            .field("types", &self.types.keys().collect::<Vec<_>>())
            .field("events", &self.events.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ElementTypeRegistry {
    pub fn new() -> ElementTypeRegistry {
        ElementTypeRegistry::default()
    }

    /// Registers an element type; `duplicate_type` if its ID is already registered (REQ-005).
    pub fn register(&mut self, def: ElementTypeDef) -> Result<(), TreeError> {
        if self.types.contains_key(&def.type_id) {
            return Err(TreeError::DuplicateType(def.type_id));
        }
        self.types.insert(def.type_id.clone(), def);
        Ok(())
    }

    /// Registers a domain event type with its translation into core events. `schema` if the
    /// type breaks the event type grammar; `duplicate_event` if it is a core event type or
    /// already registered.
    pub fn register_event(&mut self, event_type: &str, translate: DomainTranslate) -> Result<(), TreeError> {
        if !is_event_type(event_type) {
            return Err(schema("event type must be 1..=128 bytes of [A-Za-z][A-Za-z0-9_.@]*"));
        }
        if CORE_EVENTS.contains(&event_type)
            || STAGE_EVENTS.contains(&event_type)
            || self.events.contains_key(event_type)
        {
            return Err(TreeError::DuplicateEvent(event_type.to_string()));
        }
        self.events.insert(event_type.to_string(), translate);
        Ok(())
    }

    /// The registered type with this ID.
    pub fn get(&self, type_id: &str) -> Option<&ElementTypeDef> {
        self.types.get(type_id)
    }

    /// Every registered type, in ascending type ID order.
    pub fn types(&self) -> impl Iterator<Item = &ElementTypeDef> {
        self.types.values()
    }

    /// Whether a domain event type is registered.
    pub fn has_event(&self, event_type: &str) -> bool {
        self.events.contains_key(event_type)
    }
}

// ------------------------------------------------------------------ records

/// One per-stage slot of an element: the value digest and the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub digest: Option<Digest>,
    pub state: ElementState,
}

impl Slot {
    /// The slot of a new element: `not_computed`, no digest.
    pub const NEW: Slot = Slot { digest: None, state: ElementState::NotComputed };

    pub(crate) fn to_value(self) -> CValue {
        obj([
            ("digest", self.digest.map_or(CValue::Null, |d| CValue::Str(d.as_str().to_string()))),
            ("state", text(self.state.as_str())),
        ])
    }
}

/// An element record (REQ-001). Read-only outside this module; events change it.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    id: Uuid,
    type_id: String,
    parent: Uuid,
    name: ElementName,
    slots: BTreeMap<Uuid, Slot>,
    visible: bool,
    meta: CValue,
    body: CValue,
}

impl Element {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn type_id(&self) -> &str {
        &self.type_id
    }

    /// The parent group.
    pub fn parent(&self) -> Uuid {
        self.parent
    }

    pub fn name(&self) -> &ElementName {
        &self.name
    }

    /// The slots, in ascending stage ID order.
    pub fn slots(&self) -> &BTreeMap<Uuid, Slot> {
        &self.slots
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// The metadata slot (a JSON object).
    pub fn meta(&self) -> &CValue {
        &self.meta
    }

    /// The canonical wire form: `{"id", "meta", "name", "parent", "slots": {<stage>:
    /// {"digest", "state"}}, "type", "visible"}`.
    pub fn body(&self) -> &CValue {
        &self.body
    }

    fn rebuild_body(&mut self) {
        let slots = self.slots.iter().map(|(s, slot)| (s.to_string(), slot.to_value())).collect();
        self.body = obj([
            ("id", uuid_value(&self.id)),
            ("meta", self.meta.clone()),
            ("name", text(self.name.as_untrusted_str())),
            ("parent", uuid_value(&self.parent)),
            ("slots", CValue::Obj(slots)),
            ("type", text(&self.type_id)),
            ("visible", CValue::Bool(self.visible)),
        ]);
    }

    /// Sets one slot, updating the cached body in place (a stage event touches every
    /// element, so a full rebuild per element would cost O(stages) each).
    fn put_slot(&mut self, stage: Uuid, slot: Slot) {
        self.slots.insert(stage, slot);
        if let CValue::Obj(b) = &mut self.body {
            if let Some(CValue::Obj(s)) = b.get_mut("slots") {
                s.insert(stage.to_string(), slot.to_value());
                return;
            }
        }
        self.rebuild_body();
    }

    /// Removes one slot, updating the cached body in place; returns the slot.
    fn take_slot(&mut self, stage: &Uuid) -> Option<Slot> {
        let old = self.slots.remove(stage);
        if let CValue::Obj(b) = &mut self.body {
            if let Some(CValue::Obj(s)) = b.get_mut("slots") {
                s.remove(&stage.to_string());
                return old;
            }
        }
        self.rebuild_body();
        old
    }
}

/// A group of elements and groups. The root group has no parent.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    id: Uuid,
    parent: Option<Uuid>,
    name: ElementName,
    body: CValue,
}

impl Group {
    fn new(id: Uuid, parent: Option<Uuid>, name: ElementName) -> Group {
        let mut g = Group { id, parent, name, body: CValue::Null };
        g.rebuild_body();
        g
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    /// The parent group; `None` for the root.
    pub fn parent(&self) -> Option<Uuid> {
        self.parent
    }

    pub fn name(&self) -> &ElementName {
        &self.name
    }

    /// The canonical wire form: `{"id", "name", "parent"}` (`parent` null for the root).
    pub fn body(&self) -> &CValue {
        &self.body
    }

    fn rebuild_body(&mut self) {
        self.body = obj([
            ("id", uuid_value(&self.id)),
            ("name", text(self.name.as_untrusted_str())),
            ("parent", self.parent.as_ref().map_or(CValue::Null, uuid_value)),
        ]);
    }
}

// ------------------------------------------------------------------ tree

/// The stage fields of the tree, saved before a stage event so a refused commit can be
/// rolled back.
struct StageSnapshot {
    stages: Vec<Uuid>,
    active_stage: Uuid,
    records: BTreeMap<Uuid, Stage>,
    list_body: CValue,
}

/// What one change replaced, so a refused commit can be rolled back.
enum Undo {
    Element(Uuid, Option<Element>),
    Group(Uuid, Option<Group>),
    Stages(Box<StageSnapshot>),
    /// A `StageAdded` gave every element a slot for `stage` (moved from `adopted_from`, the
    /// implicit default stage, when it was the first explicit stage). Undone by the inverse
    /// move, so a stage event does not copy every element.
    StageSlots {
        stage: Uuid,
        adopted_from: Option<Uuid>,
    },
    /// The count of default-stage slots holding data before a slot change.
    DefaultData(usize),
    /// The active stage before an `ActiveStageSet`.
    Active(Uuid),
    /// One group's child-order vector before a move, plus the previous `has_moved` flag.
    ChildOrder {
        group: Uuid,
        prev: Option<Vec<Uuid>>,
        has_moved: bool,
    },
    /// O(1) undo for pushing one child onto `group`'s `child_order` during create.
    ChildOrderPush(Uuid),
    /// O(1) undo for inserting a new empty `child_order` entry for a newly created group.
    ChildOrderNew(Uuid),
}

/// The in-memory element tree of one project.
#[derive(Debug, Clone)]
pub struct Tree {
    registry: Arc<ElementTypeRegistry>,
    root: Uuid,
    stages: Vec<Uuid>,
    active_stage: Uuid,
    elements: BTreeMap<Uuid, Element>,
    groups: BTreeMap<Uuid, Group>,
    /// Ordered child IDs (groups and elements) per parent group (`CORE.PRJ.TREE.GROUP`).
    child_order: BTreeMap<Uuid, Vec<Uuid>>,
    /// True once an `ElementsMoved` event has been applied; controls whether `child_order`
    /// is emitted in [`Tree::to_value`] so pre-move trees keep their exact legacy digest.
    has_moved: bool,
    group_list: Uuid,
    group_list_body: CValue,
    tree_index: Uuid,
    tree_order_cache: OnceLock<Vec<Uuid>>,
    tree_index_body_cache: OnceLock<CValue>,
    stage_list: Uuid,
    records: BTreeMap<Uuid, Stage>,
    list_body: CValue,
    /// The reserved ID of the implicit default stage; never importable, even once replaced.
    default_stage: Uuid,
    /// While there are no explicit stages: how many elements hold data (a slot other than
    /// [`Slot::NEW`]) in the implicit default stage.
    default_data: usize,
}

/// The reserved UUIDv5 of the project namespace for `{"kind": <kind>}`.
pub(crate) fn reserved_id(namespace: Uuid, kind: &str) -> Result<Uuid, TreeError> {
    let mut no_source = ReplaySource::new(Vec::new());
    let none: Vec<Uuid> = Vec::new();
    let ids = IdService::new(namespace, &mut no_source, &none);
    Ok(ids.derive_v5(&obj([("kind", text(kind))]))?)
}

/// The reserved ID of the group child-index entity in `namespace`.
pub fn group_list_id(namespace: Uuid) -> Result<Uuid, TreeError> {
    reserved_id(namespace, GROUP_LIST_KIND)
}

/// The reserved ID of the pre-order tree index entity in `namespace` (`CORE.PRJ.TREE.QUERY`).
pub fn tree_index_id(namespace: Uuid) -> Result<Uuid, TreeError> {
    reserved_id(namespace, TREE_INDEX_KIND)
}

impl Tree {
    /// An empty tree: the root group and the implicit default stage, which is also the
    /// active stage. Both IDs are reserved UUIDv5s of `namespace` (see the module docs).
    pub fn new(namespace: Uuid, registry: Arc<ElementTypeRegistry>) -> Result<Tree, TreeError> {
        let root = reserved_id(namespace, "project.tree_root")?;
        let stage = reserved_id(namespace, "project.default_stage")?;
        let stage_list = stage_list_id(namespace)?;
        let group_list = group_list_id(namespace)?;
        let tree_index = tree_index_id(namespace)?;
        let mut groups = BTreeMap::new();
        groups.insert(root, Group::new(root, None, ElementName::default()));
        let mut child_order = BTreeMap::new();
        child_order.insert(root, Vec::new());
        let mut tree = Tree {
            registry,
            root,
            stages: vec![stage],
            active_stage: stage,
            elements: BTreeMap::new(),
            groups,
            child_order,
            has_moved: false,
            group_list,
            group_list_body: CValue::Null,
            tree_index,
            tree_order_cache: OnceLock::new(),
            tree_index_body_cache: OnceLock::new(),
            stage_list,
            records: BTreeMap::new(),
            list_body: CValue::Null,
            default_stage: stage,
            default_data: 0,
        };
        tree.rebuild_list();
        tree.rebuild_group_list();
        Ok(tree)
    }

    /// Direct children (elements and child groups) of `group`, in tree order.
    pub fn children_of(&self, group: &Uuid) -> &[Uuid] {
        self.child_order.get(group).map_or(&[], Vec::as_slice)
    }

    pub fn registry(&self) -> &ElementTypeRegistry {
        &self.registry
    }

    /// The root group.
    pub fn root(&self) -> Uuid {
        self.root
    }

    /// The stage sequence in project order, removed stages included: the implicit default
    /// stage while the project has no explicit stages.
    pub fn stages(&self) -> &[Uuid] {
        &self.stages
    }

    /// The live (not removed) stages, in project order.
    pub fn live_stages(&self) -> Vec<Uuid> {
        self.stages.iter().filter(|s| self.is_live(s)).copied().collect()
    }

    /// Whether `stage` is a live stage: an explicit stage not removed, or the implicit default
    /// stage while there are no explicit stages. O(log n): records only.
    fn is_live(&self, stage: &Uuid) -> bool {
        match self.records.get(stage) {
            Some(r) => !r.removed(),
            None => self.records.is_empty() && *stage == self.default_stage,
        }
    }

    /// Whether `stage` is in the project sequence (removed stages included).
    fn is_known(&self, stage: &Uuid) -> bool {
        self.records.contains_key(stage) || (self.records.is_empty() && *stage == self.default_stage)
    }

    /// The reserved ID of the implicit default stage.
    pub fn default_stage_id(&self) -> Uuid {
        self.default_stage
    }

    /// The default stage the next `StageAdded` adopts: the implicit default stage while there
    /// are no explicit stages and none of its slots holds data; else `None` (the default
    /// stage then becomes an explicit stage that keeps its ID and data).
    pub fn adoptable_default(&self) -> Option<Uuid> {
        (self.records.is_empty() && self.default_data == 0).then_some(self.default_stage)
    }

    /// Whether the project has explicit stages (else it has only the implicit default stage).
    pub fn has_explicit_stages(&self) -> bool {
        !self.records.is_empty()
    }

    /// The explicit stage with this ID, removed or not.
    pub fn stage(&self, id: &Uuid) -> Option<&Stage> {
        self.records.get(id)
    }

    /// Every explicit stage, removed ones included, in project order.
    pub fn stage_records(&self) -> impl Iterator<Item = &Stage> {
        self.stages.iter().filter_map(|s| self.records.get(s))
    }

    /// The position of a stage in the project sequence (removed stages count).
    pub fn stage_order(&self, id: &Uuid) -> Option<usize> {
        self.stages.iter().position(|s| s == id)
    }

    /// The live explicit stages whose untrusted label is exactly `label`, in project order.
    /// Labels need not be unique, so this may return several.
    pub fn stages_labelled(&self, label: &str) -> Vec<Uuid> {
        self.stage_records().filter(|s| !s.removed() && s.label().as_untrusted_str() == label).map(Stage::id).collect()
    }

    /// The one live stage labelled exactly `label`: `stage_not_found` if none is,
    /// `ambiguous_stage` (bus `validation/bad_stage`) if several are.
    pub fn stage_by_label(&self, label: &str) -> Result<Uuid, TreeError> {
        match self.stages_labelled(label).as_slice() {
            [one] => Ok(*one),
            [] => Err(TreeError::StageNotFound("no live stage has this label".to_string())),
            many => Err(TreeError::AmbiguousStage(format!("{} live stages have this label", many.len()))),
        }
    }

    /// The stage an import or another stage-targeting command writes to: `stage` when it is a
    /// live stage of the project, else `stage_not_found`; the active stage when `None`.
    pub fn target_stage(&self, stage: Option<&Uuid>) -> Result<Uuid, TreeError> {
        match stage {
            None => Ok(self.active_stage),
            Some(s) if self.is_live(s) => Ok(*s),
            Some(s) => Err(TreeError::StageNotFound(s.to_string())),
        }
    }

    /// The ID of the stage list entity (see [`crate::stage`]).
    pub fn stage_list_id(&self) -> Uuid {
        self.stage_list
    }

    /// The ID of the group child-index entity (`GROUP_LIST_KIND`).
    pub fn group_list_id(&self) -> Uuid {
        self.group_list
    }

    /// The ID of the pre-order tree index entity (`TREE_INDEX_KIND`).
    pub fn tree_index_id(&self) -> Uuid {
        self.tree_index
    }

    /// Every element ID in pre-order tree order (groups walked iteratively in `child_order`
    /// starting at `root`, tie-broken by element position/ID). Lazily built and cached across
    /// queries; invalidated on any mutation in `finish`.
    pub fn tree_order(&self) -> &[Uuid] {
        self.tree_order_cache.get_or_init(|| self.compute_tree_order()).as_slice()
    }

    /// Names of ancestor groups from root's child down to `group` (empty when `group == root`).
    pub fn group_path(&self, group: Uuid) -> Vec<&ElementName> {
        let mut rev = Vec::new();
        let mut cur = group;
        for _ in 0..=MAX_GROUP_DEPTH {
            let Some(g) = self.groups.get(&cur) else {
                break;
            };
            let Some(parent) = g.parent else {
                break;
            };
            rev.push(&g.name);
            cur = parent;
        }
        rev.reverse();
        rev
    }

    /// True if `group` is `target` or an ancestor-chain descendant of `target`.
    pub fn is_in_group_subtree(&self, group: Uuid, target: Uuid) -> bool {
        let mut cur = group;
        for _ in 0..=MAX_GROUP_DEPTH {
            if cur == target {
                return true;
            }
            let Some(g) = self.groups.get(&cur) else {
                return false;
            };
            match g.parent {
                Some(p) => cur = p,
                None => return false,
            }
        }
        false
    }

    fn compute_tree_order(&self) -> Vec<Uuid> {
        let mut out = Vec::with_capacity(self.elements.len());
        // Explicit stack of (group_id, next_child_index) to avoid recursion (CONSTITUTION §8).
        let mut stack: Vec<(Uuid, usize)> = Vec::with_capacity(MAX_GROUP_DEPTH + 2);
        stack.push((self.root, 0));
        while let Some((gid, idx)) = stack.last_mut() {
            let children = self.child_order.get(gid).map_or(&[][..], Vec::as_slice);
            if *idx >= children.len() {
                stack.pop();
                continue;
            }
            let cid = children[*idx];
            *idx += 1;
            if self.elements.contains_key(&cid) {
                out.push(cid);
            } else if self.groups.contains_key(&cid) && stack.len() <= MAX_GROUP_DEPTH + 1 {
                stack.push((cid, 0));
            }
        }
        out
    }

    fn invalidate_order_cache(&mut self) {
        self.tree_order_cache = OnceLock::new();
        self.tree_index_body_cache = OnceLock::new();
    }

    /// One stage's data: `{"elements": {<id>: {"digest", "state"}}, "stage": <id>}` over
    /// every element whose slot for the stage holds data (empty `not_computed` slots with no
    /// digest are left out, so creating an element, or giving it data in another stage, does
    /// not change this stage's value). `bad_stage` if the stage is not in the project
    /// sequence (a removed stage keeps its data and has a value).
    pub fn stage_value(&self, stage: &Uuid) -> Result<CValue, TreeError> {
        if !self.is_known(stage) {
            return Err(TreeError::BadStage(*stage));
        }
        let elements = self.elements.iter().filter_map(|(id, e)| {
            e.slots.get(stage).filter(|s| **s != Slot::NEW).map(|s| (id.to_string(), s.to_value()))
        });
        Ok(obj([("elements", CValue::Obj(elements.collect())), ("stage", uuid_value(stage))]))
    }

    /// The per-stage data digest: SHA-256 of the RFC 8785 bytes of [`Tree::stage_value`].
    /// Changing one stage's slots leaves every other stage's digest unchanged.
    pub fn stage_digest(&self, stage: &Uuid) -> Result<Digest, TreeError> {
        let d = fid_canon::digest(&self.stage_value(stage)?)
            .map_err(|e| invariant(format!("stage data has no canonical form: {e}")))?;
        Ok(Digest::from_bytes(d.0))
    }

    pub fn active_stage(&self) -> Uuid {
        self.active_stage
    }

    pub fn element(&self, id: &Uuid) -> Option<&Element> {
        self.elements.get(id)
    }

    pub fn group(&self, id: &Uuid) -> Option<&Group> {
        self.groups.get(id)
    }

    /// Every element, in ascending ID order.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.elements.values()
    }

    /// Every group, the root included, in ascending ID order.
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.values()
    }

    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// The element's state in the active stage (REQ-002).
    pub fn displayed_state(&self, id: &Uuid) -> Option<ElementState> {
        self.elements.get(id)?.slots.get(&self.active_stage).map(|s| s.state)
    }

    /// Any entity of the tree as `(kind, body)`: an element by its type ID, a group as
    /// [`GROUP_KIND`]. The read side of the bus state view.
    pub fn entity(&self, id: &Uuid) -> Option<(&str, &CValue)> {
        if let Some(e) = self.elements.get(id) {
            return Some((&e.type_id, &e.body));
        }
        if let Some(s) = self.records.get(id) {
            return Some((STAGE_KIND, s.body()));
        }
        if *id == self.stage_list {
            return Some((STAGE_LIST_KIND, &self.list_body));
        }
        if *id == self.group_list {
            return Some((GROUP_LIST_KIND, &self.group_list_body));
        }
        if *id == self.tree_index {
            let body = self.tree_index_body_cache.get_or_init(|| {
                let order = self.tree_order().iter().map(uuid_value).collect();
                let stages = self.stages.iter().map(uuid_value).collect();
                obj([
                    ("active_stage", uuid_value(&self.active_stage)),
                    ("order", CValue::Arr(order)),
                    ("root", uuid_value(&self.root)),
                    ("stages", CValue::Arr(stages)),
                ])
            });
            return Some((TREE_INDEX_KIND, body));
        }
        self.groups.get(id).map(|g| (GROUP_KIND, &g.body))
    }

    fn group_list_value(&self) -> CValue {
        let mut children: BTreeMap<String, Vec<CValue>> = BTreeMap::new();
        for g in self.groups.values() {
            if let Some(p) = g.parent {
                children.entry(p.to_string()).or_default().push(uuid_value(&g.id));
            }
        }
        let map = children.into_iter().map(|(k, v)| (k, CValue::Arr(v))).collect();
        obj([("children", CValue::Obj(map))])
    }

    fn rebuild_group_list(&mut self) {
        self.group_list_body = self.group_list_value();
    }

    fn list_value(&self) -> CValue {
        let ids = self.stages.iter().filter(|s| self.records.contains_key(s)).map(uuid_value).collect();
        obj([
            ("active_stage", uuid_value(&self.active_stage)),
            ("adoptable", self.adoptable_default().as_ref().map_or(CValue::Null, uuid_value)),
            ("stages", CValue::Arr(ids)),
        ])
    }

    fn rebuild_list(&mut self) {
        self.list_body = self.list_value();
    }

    fn stage_snapshot(&self) -> Undo {
        Undo::Stages(Box::new(StageSnapshot {
            stages: self.stages.clone(),
            active_stage: self.active_stage,
            records: self.records.clone(),
            list_body: self.list_body.clone(),
        }))
    }

    /// The ordered tree: `{"active_stage", "elements": {<id>: <element body>}, "groups":
    /// {<id>: <group body>}, "root", "stages": [<id>, ...]}`, plus `"stage_records": {<id>:
    /// <stage body>}` when the project has explicit stages, and `"child_order": {<group>:
    /// [<id>, ...]}` once an `ElementsMoved` event has been applied.
    pub fn to_value(&self) -> CValue {
        let elements = self.elements.iter().map(|(id, e)| (id.to_string(), e.body.clone())).collect();
        let groups = self.groups.iter().map(|(id, g)| (id.to_string(), g.body.clone())).collect();
        let mut v = BTreeMap::new();
        v.insert("active_stage".to_string(), uuid_value(&self.active_stage));
        if self.has_moved {
            let order = self
                .child_order
                .iter()
                .map(|(gid, kids)| (gid.to_string(), CValue::Arr(kids.iter().map(uuid_value).collect())))
                .collect();
            v.insert("child_order".to_string(), CValue::Obj(order));
        }
        v.insert("elements".to_string(), CValue::Obj(elements));
        v.insert("groups".to_string(), CValue::Obj(groups));
        v.insert("root".to_string(), uuid_value(&self.root));
        v.insert("stages".to_string(), CValue::Arr(self.stages.iter().map(uuid_value).collect()));
        if !self.records.is_empty() {
            let records = self.records.iter().map(|(id, s)| (id.to_string(), s.body().clone())).collect();
            v.insert("stage_records".to_string(), CValue::Obj(records));
        }
        CValue::Obj(v)
    }

    /// The canonical state digest: SHA-256 of the RFC 8785 bytes of [`Tree::to_value`] (REQ-007).
    pub fn state_digest(&self) -> Result<Digest, TreeError> {
        let d =
            fid_canon::digest(&self.to_value()).map_err(|e| invariant(format!("state has no canonical form: {e}")))?;
        Ok(Digest::from_bytes(d.0))
    }

    /// Applies one event, all or nothing. Returns the IDs of the entities it created.
    pub fn apply(&mut self, event_type: &str, body: &CValue) -> Result<Vec<Uuid>, TreeError> {
        let mut undo = Vec::new();
        let mut created = Vec::new();
        let r = self.apply_one(event_type, body, &mut undo, &mut created);
        self.finish(r, undo, created)
    }

    /// Applies the events of one commit in order, all or nothing: on the first refusal the
    /// tree is restored to its state before the commit and the error is returned (REQ-003).
    /// Returns the IDs of the entities the commit created, in creation order.
    pub fn apply_commit(&mut self, events: &[TreeEvent]) -> Result<Vec<Uuid>, TreeError> {
        let mut undo = Vec::new();
        let mut created = Vec::new();
        let mut r = Ok(());
        for ev in events {
            r = self.apply_one(&ev.event_type, &ev.body, &mut undo, &mut created);
            if r.is_err() {
                break;
            }
        }
        self.finish(r, undo, created)
    }

    fn finish(
        &mut self,
        r: Result<(), TreeError>,
        undo: Vec<Undo>,
        created: Vec<Uuid>,
    ) -> Result<Vec<Uuid>, TreeError> {
        let changed = !undo.is_empty();
        match r {
            Ok(()) => {
                if changed {
                    self.invalidate_order_cache();
                }
                Ok(created)
            }
            Err(e) => {
                for u in undo.into_iter().rev() {
                    match u {
                        Undo::Element(id, Some(old)) => {
                            self.elements.insert(id, old);
                        }
                        Undo::Element(id, None) => {
                            self.elements.remove(&id);
                        }
                        Undo::Group(id, Some(old)) => {
                            self.groups.insert(id, old);
                        }
                        Undo::Group(id, None) => {
                            self.groups.remove(&id);
                        }
                        Undo::Stages(snap) => {
                            let snap = *snap;
                            self.stages = snap.stages;
                            self.active_stage = snap.active_stage;
                            self.records = snap.records;
                            self.list_body = snap.list_body;
                        }
                        Undo::StageSlots { stage, adopted_from } => {
                            for e in self.elements.values_mut() {
                                let slot = e.take_slot(&stage);
                                if let (Some(old), Some(slot)) = (adopted_from, slot) {
                                    e.put_slot(old, slot);
                                }
                            }
                        }
                        Undo::DefaultData(n) => self.default_data = n,
                        Undo::Active(old) => self.active_stage = old,
                        Undo::ChildOrder { group, prev, has_moved } => {
                            self.has_moved = has_moved;
                            match prev {
                                Some(kids) => {
                                    self.child_order.insert(group, kids);
                                }
                                None => {
                                    self.child_order.remove(&group);
                                }
                            }
                        }
                        Undo::ChildOrderPush(group) => {
                            if let Some(kids) = self.child_order.get_mut(&group) {
                                kids.pop();
                            }
                        }
                        Undo::ChildOrderNew(group) => {
                            self.child_order.remove(&group);
                        }
                    }
                }
                self.rebuild_list();
                self.rebuild_group_list();
                if changed {
                    self.invalidate_order_cache();
                }
                Err(e)
            }
        }
    }

    /// One event: a core event directly, a registered domain event through its translation
    /// (which must yield core events only).
    fn apply_one(
        &mut self,
        event_type: &str,
        body: &CValue,
        undo: &mut Vec<Undo>,
        created: &mut Vec<Uuid>,
    ) -> Result<(), TreeError> {
        if CORE_EVENTS.contains(&event_type) {
            return self.apply_core(event_type, body, undo, created);
        }
        if STAGE_EVENTS.contains(&event_type) {
            return self.apply_stage(event_type, body, undo, created);
        }
        let translate = *self.registry.events.get(event_type).ok_or_else(|| {
            TreeError::UnknownEvent(if is_event_type(event_type) {
                event_type.to_string()
            } else {
                "malformed event type".to_string()
            })
        })?;
        for ev in translate(body)? {
            if !CORE_EVENTS.contains(&ev.event_type.as_str()) {
                return Err(TreeError::Invariant(format!("{event_type} translated into a non-core event")));
            }
            self.apply_core(&ev.event_type, &ev.body, undo, created)?;
        }
        Ok(())
    }

    fn apply_core(
        &mut self,
        event_type: &str,
        body: &CValue,
        undo: &mut Vec<Undo>,
        created: &mut Vec<Uuid>,
    ) -> Result<(), TreeError> {
        match event_type {
            GROUP_CREATED => {
                let g = self.check_group_created(body)?;
                let id = g.id;
                let parent = g.parent;
                undo.push(Undo::Group(id, None));
                undo.push(Undo::ChildOrderNew(id));
                if let Some(p) = parent {
                    undo.push(Undo::ChildOrderPush(p));
                    self.child_order.entry(p).or_default().push(id);
                }
                self.child_order.insert(id, Vec::new());
                self.groups.insert(id, g);
                self.rebuild_group_list();
                created.push(id);
            }
            ELEMENT_CREATED => {
                let e = self.check_element_created(body)?;
                let id = e.id;
                let parent = e.parent;
                undo.push(Undo::ChildOrderPush(parent));
                undo.push(Undo::Element(id, None));
                self.child_order.entry(parent).or_default().push(id);
                self.elements.insert(id, e);
                created.push(id);
            }
            ELEMENT_SLOT_SET => {
                let (id, stage, slot) = self.check_slot_set(body)?;
                let was_new = self.elements.get(&id).and_then(|e| e.slots.get(&stage)).is_some_and(|s| *s == Slot::NEW);
                if self.records.is_empty() && was_new && slot != Slot::NEW {
                    undo.push(Undo::DefaultData(self.default_data));
                    self.default_data = self.default_data.saturating_add(1);
                    if self.default_data == 1 {
                        self.rebuild_list();
                    }
                }
                let Some(e) = self.elements.get_mut(&id) else {
                    return Err(invariant(format!("element {id} vanished")));
                };
                undo.push(Undo::Element(id, Some(e.clone())));
                e.slots.insert(stage, slot);
                e.rebuild_body();
            }
            ELEMENTS_MOVED => {
                let (ids, to) = self.check_elements_moved(body)?;
                let moving_set: BTreeSet<Uuid> = ids.iter().copied().collect();
                let mut touched_parents: BTreeSet<Uuid> = BTreeSet::new();
                touched_parents.insert(to);
                for id in &ids {
                    if let Some(e) = self.elements.get(id) {
                        touched_parents.insert(e.parent);
                    } else if let Some(g) = self.groups.get(id) {
                        if let Some(p) = g.parent {
                            touched_parents.insert(p);
                        }
                    }
                }
                for p in &touched_parents {
                    let prev = self.child_order.get(p).cloned();
                    undo.push(Undo::ChildOrder { group: *p, prev, has_moved: self.has_moved });
                }
                for p in &touched_parents {
                    if let Some(kids) = self.child_order.get_mut(p) {
                        kids.retain(|k| !moving_set.contains(k));
                    }
                }
                let mut moved_group = false;
                for id in &ids {
                    if let Some(e) = self.elements.get_mut(id) {
                        undo.push(Undo::Element(*id, Some(e.clone())));
                        e.parent = to;
                        e.rebuild_body();
                    } else if let Some(g) = self.groups.get_mut(id) {
                        undo.push(Undo::Group(*id, Some(g.clone())));
                        g.parent = Some(to);
                        g.rebuild_body();
                        moved_group = true;
                    }
                }
                self.child_order.entry(to).or_default().extend(ids);
                self.has_moved = true;
                if moved_group {
                    self.rebuild_group_list();
                }
            }
            RENAMED => {
                let (id, name) = self.check_renamed(body)?;
                if let Some(e) = self.elements.get_mut(&id) {
                    undo.push(Undo::Element(id, Some(e.clone())));
                    e.name = name;
                    e.rebuild_body();
                } else if let Some(g) = self.groups.get_mut(&id) {
                    undo.push(Undo::Group(id, Some(g.clone())));
                    g.name = name;
                    g.rebuild_body();
                } else {
                    return Err(invariant(format!("entity {id} vanished")));
                }
            }
            VISIBILITY_SET => {
                let (ids, visible) = self.check_visibility_set(body)?;
                for id in ids {
                    let Some(e) = self.elements.get_mut(&id) else {
                        return Err(invariant(format!("element {id} vanished")));
                    };
                    undo.push(Undo::Element(id, Some(e.clone())));
                    e.visible = visible;
                    e.rebuild_body();
                }
            }
            other => return Err(TreeError::UnknownEvent(other.to_string())),
        }
        Ok(())
    }

    /// A stage event (see [`crate::stage`]).
    fn apply_stage(
        &mut self,
        event_type: &str,
        body: &CValue,
        undo: &mut Vec<Undo>,
        created: &mut Vec<Uuid>,
    ) -> Result<(), TreeError> {
        match event_type {
            STAGE_ADDED => {
                let (stage, adopt) = stage_from_added(body)?;
                let id = self.import_id(stage.id())?;
                // `adopts` must say what the state allows: the implicit default stage while it
                // holds no data, else null.
                if adopt != self.adoptable_default() {
                    return Err(TreeError::BadStage(adopt.unwrap_or(self.default_stage)));
                }
                undo.push(self.stage_snapshot());
                if adopt.is_none() && self.records.is_empty() {
                    // The default stage holds data: it becomes an explicit stage that keeps its
                    // reserved ID, its data and the active-stage role.
                    let label = Label::new(DEFAULT_STAGE_LABEL)
                        .map_err(|_| invariant("default stage label breaks the text rules".to_string()))?;
                    let d = self.default_stage;
                    self.records.insert(d, Stage::new(d, stage.run(), label, None, obj([]), false));
                    created.push(d);
                }
                undo.push(Undo::StageSlots { stage: id, adopted_from: adopt });
                for e in self.elements.values_mut() {
                    let slot = match adopt {
                        Some(old) => e.take_slot(&old).unwrap_or(Slot::NEW),
                        None => Slot::NEW,
                    };
                    e.put_slot(id, slot);
                }
                if adopt.is_some() {
                    self.stages = vec![id];
                    self.active_stage = id;
                } else {
                    self.stages.push(id);
                }
                self.records.insert(id, stage);
                self.rebuild_list();
                created.push(id);
            }
            ACTIVE_STAGE_SET => {
                // Swaps the active-stage pointer only (CORE.PRJ.STAGE.SWITCH): no slot, value or
                // stage record moves, so every per-stage digest is unchanged.
                let id = active_set(body)?;
                if !self.is_live(&id) {
                    return Err(TreeError::BadStage(id));
                }
                undo.push(Undo::Active(self.active_stage));
                self.active_stage = id;
                if let CValue::Obj(b) = &mut self.list_body {
                    b.insert("active_stage".to_string(), uuid_value(&id));
                }
            }
            STAGE_REMOVED => {
                let id = removed_stage(body)?;
                if !self.records.contains_key(&id) || !self.is_live(&id) {
                    return Err(TreeError::StageNotFound(id.to_string()));
                }
                if id == self.active_stage {
                    return Err(TreeError::StageActive(id));
                }
                undo.push(self.stage_snapshot());
                if let Some(s) = self.records.get_mut(&id) {
                    s.set_removed(true);
                }
                self.rebuild_list();
            }
            STAGES_REORDERED => {
                let (run, order) = reordered(body)?;
                let live: Vec<Uuid> =
                    self.stage_records().filter(|s| !s.removed() && s.run() == run).map(Stage::id).collect();
                check_permutation(&order, &live)?;
                undo.push(self.stage_snapshot());
                let mut next = order.into_iter();
                for slot in self.stages.iter_mut() {
                    if live.contains(slot) {
                        if let Some(id) = next.next() {
                            *slot = id;
                        }
                    }
                }
                self.rebuild_list();
            }
            other => return Err(TreeError::UnknownEvent(other.to_string())),
        }
        Ok(())
    }

    /// The number of groups from the root (exclusive) to `group` (inclusive); `hierarchy`
    /// if the chain is longer than [`MAX_GROUP_DEPTH`] or does not reach the root.
    fn depth(&self, group: &Uuid) -> Result<usize, TreeError> {
        let mut at = *group;
        let mut depth = 0;
        loop {
            let Some(g) = self.groups.get(&at) else {
                return Err(TreeError::Invariant(format!("group {at} is an orphan")));
            };
            match g.parent {
                None if at == self.root => return Ok(depth),
                None => return Err(TreeError::Invariant(format!("group {at} has no parent"))),
                Some(p) => {
                    depth += 1;
                    if depth > MAX_GROUP_DEPTH {
                        return Err(TreeError::Hierarchy(format!("group {group} is deeper than {MAX_GROUP_DEPTH}")));
                    }
                    at = p;
                }
            }
        }
    }

    fn import_id(&self, id: Uuid) -> Result<Uuid, TreeError> {
        let mut no_source = ReplaySource::new(Vec::new());
        let ids = IdService::new(Uuid::NIL, &mut no_source, self);
        Ok(ids.import_uuid(id)?)
    }

    fn existing_group(&self, id: Uuid) -> Result<Uuid, TreeError> {
        if self.groups.contains_key(&id) {
            Ok(id)
        } else {
            Err(TreeError::BadReference(format!("parent {id} is not a group of this project")))
        }
    }

    fn check_group_created(&self, body: &CValue) -> Result<Group, TreeError> {
        let o = exact_keys(body, &["id", "name", "parent"])?;
        let id = uuid_field(o, "id")?;
        let parent = uuid_field(o, "parent")?;
        let name = str_field(o, "name")?;
        let name = ElementName::new(name)?;
        self.existing_group(parent)?;
        if self.depth(&parent)? + 1 > MAX_GROUP_DEPTH {
            return Err(TreeError::Hierarchy(format!("a group under {parent} would be deeper than {MAX_GROUP_DEPTH}")));
        }
        let id = self.import_id(id)?;
        Ok(Group::new(id, Some(parent), name))
    }

    fn check_element_created(&self, body: &CValue) -> Result<Element, TreeError> {
        let o = exact_keys(body, &["id", "meta", "name", "parent", "type", "visible"])?;
        let id = uuid_field(o, "id")?;
        let parent = uuid_field(o, "parent")?;
        let name = str_field(o, "name")?;
        let type_id = str_field(o, "type")?;
        let meta = match o.get("meta") {
            Some(m @ CValue::Obj(_)) => m.clone(),
            _ => return Err(schema("meta must be an object")),
        };
        let visible = match o.get("visible") {
            Some(CValue::Bool(b)) => *b,
            _ => return Err(schema("visible must be a boolean")),
        };
        let name = ElementName::new(name)?;
        if self.registry.get(type_id).is_none() {
            let shown = if is_type_id(type_id) { type_id.to_string() } else { "malformed type id".to_string() };
            return Err(TreeError::UnregisteredType(shown));
        }
        self.existing_group(parent)?;
        let id = self.import_id(id)?;
        let slots = self.live_stages().into_iter().map(|s| (s, Slot::NEW)).collect();
        let mut e =
            Element { id, type_id: type_id.to_string(), parent, name, slots, visible, meta, body: CValue::Null };
        e.rebuild_body();
        Ok(e)
    }

    fn check_slot_set(&self, body: &CValue) -> Result<(Uuid, Uuid, Slot), TreeError> {
        let o = exact_keys(body, &["digest", "element", "stage", "state"])?;
        let id = uuid_field(o, "element")?;
        let stage = uuid_field(o, "stage")?;
        let state = ElementState::parse(str_field(o, "state")?).ok_or_else(|| schema("state is not a state"))?;
        let digest = match o.get("digest") {
            Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(Digest::parse(s).map_err(|_| schema("digest must be sha256: text"))?),
            _ => return Err(schema("digest must be a string or null")),
        };
        if state == ElementState::Current && digest.is_none() {
            return Err(schema("a current slot must carry a digest"));
        }
        let e = self.elements.get(&id).ok_or_else(|| TreeError::BadReference(format!("element {id} not found")))?;
        if !self.is_live(&stage) {
            return Err(TreeError::BadStage(stage));
        }
        // Every element has a slot per stage (an invariant), so a missing one is corrupt state.
        let from = e
            .slots
            .get(&stage)
            .map(|s| s.state)
            .ok_or_else(|| invariant(format!("element {id} has no slot for stage {stage}")))?;
        if !from.can_become(state) {
            return Err(TreeError::InvalidTransition { from, to: state });
        }
        Ok((id, stage, Slot { digest, state }))
    }

    fn group_ancestors(&self, group: &Uuid) -> Result<BTreeSet<Uuid>, TreeError> {
        let mut ancestors = BTreeSet::new();
        let mut at = *group;
        let mut depth = 0usize;
        loop {
            if !ancestors.insert(at) {
                return Err(invariant(format!("cycle in group hierarchy at {at}")));
            }
            let Some(g) = self.groups.get(&at) else {
                return Err(invariant(format!("group {at} is an orphan")));
            };
            match g.parent {
                None if at == self.root => return Ok(ancestors),
                None => return Err(invariant(format!("group {at} has no parent"))),
                Some(p) => {
                    depth += 1;
                    if depth > MAX_GROUP_DEPTH {
                        return Err(TreeError::Hierarchy(format!("group {group} is deeper than {MAX_GROUP_DEPTH}")));
                    }
                    at = p;
                }
            }
        }
    }

    /// Maximum depth of descendant groups below `group` (0 if `group` has no child groups).
    fn group_subtree_height(&self, group: Uuid) -> usize {
        let mut max_h = 0usize;
        let mut stack: Vec<(Uuid, usize)> = vec![(group, 0)];
        while let Some((cur, h)) = stack.pop() {
            if h > max_h {
                max_h = h;
            }
            if let Some(kids) = self.child_order.get(&cur) {
                for kid in kids {
                    if self.groups.contains_key(kid) {
                        stack.push((*kid, h + 1));
                    }
                }
            }
        }
        max_h
    }

    fn check_elements_moved(&self, body: &CValue) -> Result<(Vec<Uuid>, Uuid), TreeError> {
        let o = exact_keys(body, &["ids", "to"])?;
        let ids = uuid_batch_field(o, "ids")?;
        let to = uuid_field(o, "to")?;
        if !self.groups.contains_key(&to) {
            return Err(TreeError::BadReference(format!("target {to} is not a group of this project")));
        }
        let ancestors = self.group_ancestors(&to)?;
        let to_depth = ancestors.len().saturating_sub(1);
        for id in &ids {
            if self.elements.contains_key(id) {
                continue;
            }
            let Some(g) = self.groups.get(id) else {
                return Err(TreeError::NotFound(format!("entity {id} not found")));
            };
            if g.parent.is_none() {
                return Err(TreeError::Hierarchy(format!("root group {id} cannot be moved")));
            }
            if ancestors.contains(id) {
                return Err(TreeError::Hierarchy(format!(
                    "moving group {id} into {to} would make it its own ancestor"
                )));
            }
            let h = self.group_subtree_height(*id);
            if to_depth + 1 + h > MAX_GROUP_DEPTH {
                return Err(TreeError::Hierarchy(format!(
                    "moving group {id} under {to} would exceed depth {MAX_GROUP_DEPTH}"
                )));
            }
        }
        Ok((ids, to))
    }

    fn check_renamed(&self, body: &CValue) -> Result<(Uuid, ElementName), TreeError> {
        let o = exact_keys(body, &["id", "name"])?;
        let id = uuid_field(o, "id")?;
        let name = ElementName::new(str_field(o, "name")?)?;
        if !self.elements.contains_key(&id) && !self.groups.contains_key(&id) {
            return Err(TreeError::NotFound(format!("entity {id} not found")));
        }
        Ok((id, name))
    }

    fn check_visibility_set(&self, body: &CValue) -> Result<(Vec<Uuid>, bool), TreeError> {
        let o = exact_keys(body, &["ids", "visible"])?;
        let ids = uuid_batch_field(o, "ids")?;
        let visible = match o.get("visible") {
            Some(CValue::Bool(b)) => *b,
            _ => return Err(schema("visible must be a boolean")),
        };
        for id in &ids {
            if self.elements.contains_key(id) {
                continue;
            }
            if self.groups.contains_key(id) {
                return Err(TreeError::BadReference(format!("entity {id} is a group, not an element")));
            }
            return Err(TreeError::NotFound(format!("element {id} not found")));
        }
        Ok((ids, visible))
    }

    /// Checks every invariant of the module docs; `invariant_violated` on the first breach.
    /// Fails closed: a tree that passes has no cycle, no orphan and no unregistered type.
    pub fn verify(&self) -> Result<(), TreeError> {
        match self.groups.get(&self.root) {
            Some(g) if g.parent.is_none() => {}
            _ => return Err(invariant("root group missing or has a parent".to_string())),
        }
        if !self.is_live(&self.active_stage) {
            return Err(invariant("active stage is not a live stage".to_string()));
        }
        let sorted = self.stages_sorted();
        if sorted.windows(2).any(|w| w[0] == w[1]) {
            return Err(invariant("stage IDs are not distinct".to_string()));
        }
        if self.records.is_empty() {
            if self.stages != [self.default_stage] {
                return Err(invariant("a project without explicit stages has only the default stage".to_string()));
            }
        } else if !self.records.keys().eq(sorted.iter()) {
            return Err(invariant("stage records and the stage sequence disagree".to_string()));
        }
        for (key, s) in &self.records {
            if *key != s.id() {
                return Err(invariant(format!("stage {} is filed under {key}", s.id())));
            }
            let fresh =
                Stage::new(s.id(), s.run(), s.label().clone(), s.serial().cloned(), s.metadata().clone(), s.removed());
            if fresh != *s {
                return Err(invariant(format!("stage {} body is stale", s.id())));
            }
        }
        if self.list_value() != self.list_body {
            return Err(invariant("stage list is stale".to_string()));
        }
        let live = self.live_stages();
        for (key, g) in &self.groups {
            if *key != g.id {
                return Err(invariant(format!("group {} is filed under {key}", g.id)));
            }
            if self.stages.contains(&g.id) {
                return Err(invariant(format!("group {} shares its ID with a stage", g.id)));
            }
            self.depth(&g.id).map_err(|e| invariant(e.to_string()))?;
            if *g != Group::new(g.id, g.parent, g.name.clone()) {
                return Err(invariant(format!("group {} body is stale", g.id)));
            }
        }
        for (key, e) in &self.elements {
            if *key != e.id {
                return Err(invariant(format!("element {} is filed under {key}", e.id)));
            }
            if !self.groups.contains_key(&e.parent) {
                return Err(invariant(format!("element {} is an orphan", e.id)));
            }
            if self.groups.contains_key(&e.id) || self.stages.contains(&e.id) {
                return Err(invariant(format!("element {} shares its ID", e.id)));
            }
            if self.registry.get(&e.type_id).is_none() {
                return Err(invariant(format!("element {} has an unregistered type", e.id)));
            }
            if !live.iter().all(|s| e.slots.contains_key(s)) || !e.slots.keys().all(|s| self.stages.contains(s)) {
                return Err(invariant(format!("element {} does not have one slot per stage", e.id)));
            }
            if e.slots.values().any(|s| s.state == ElementState::Current && s.digest.is_none()) {
                return Err(invariant(format!("element {} has a current slot without a digest", e.id)));
            }
            let mut fresh = e.clone();
            fresh.rebuild_body();
            if fresh.body != e.body {
                return Err(invariant(format!("element {} body is stale", e.id)));
            }
        }
        if self.records.is_empty() {
            let data = self
                .elements
                .values()
                .filter(|e| e.slots.get(&self.default_stage).is_some_and(|s| *s != Slot::NEW))
                .count();
            if data != self.default_data {
                return Err(invariant("default stage data count is stale".to_string()));
            }
        }
        let mut expected_children: BTreeMap<Uuid, BTreeSet<Uuid>> =
            self.groups.keys().map(|g| (*g, BTreeSet::new())).collect();
        for g in self.groups.values() {
            if let Some(p) = g.parent {
                expected_children.entry(p).or_default().insert(g.id);
            }
        }
        for e in self.elements.values() {
            expected_children.entry(e.parent).or_default().insert(e.id);
        }
        if !self.child_order.keys().eq(self.groups.keys()) {
            return Err(invariant("child_order keys do not match groups".to_string()));
        }
        for (gid, kids) in &self.child_order {
            let set: BTreeSet<Uuid> = kids.iter().copied().collect();
            if set.len() != kids.len() {
                return Err(invariant(format!("group {gid} has duplicate children in child_order")));
            }
            if expected_children.get(gid) != Some(&set) {
                return Err(invariant(format!("group {gid} child_order disagrees with parent links")));
            }
        }
        if self.group_list_value() != self.group_list_body {
            return Err(invariant("group list is stale".to_string()));
        }
        Ok(())
    }

    fn stages_sorted(&self) -> Vec<Uuid> {
        let mut s = self.stages.clone();
        s.sort();
        s
    }
}

impl IdIndex for Tree {
    /// Elements, groups (the root included), stages, the stage list and the group list hold IDs.
    /// The reserved default-stage ID stays held after the first explicit stage replaces it.
    fn contains_id(&self, id: &Uuid) -> bool {
        self.elements.contains_key(id)
            || self.groups.contains_key(id)
            || self.records.contains_key(id)
            || *id == self.default_stage
            || *id == self.stage_list
            || *id == self.group_list
            || *id == self.tree_index
    }
}

// ------------------------------------------------------------------ wire helpers

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn uuid_value(id: &Uuid) -> CValue {
    CValue::Str(id.to_string())
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// The body as an object with exactly `keys`; `schema` otherwise.
fn exact_keys<'a>(body: &'a CValue, keys: &[&str]) -> Result<&'a BTreeMap<String, CValue>, TreeError> {
    let CValue::Obj(o) = body else {
        return Err(schema("event body must be an object"));
    };
    if o.len() != keys.len() || !keys.iter().all(|k| o.contains_key(*k)) {
        return Err(TreeError::Schema(format!("event body must have exactly the keys {keys:?}")));
    }
    Ok(o)
}

fn str_field<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, TreeError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(TreeError::Schema(format!("{key} must be a string"))),
    }
}

/// A canonical lowercase hyphenated UUID; `schema` for anything else.
fn uuid_field(o: &BTreeMap<String, CValue>, key: &str) -> Result<Uuid, TreeError> {
    let s = str_field(o, key)?;
    match Uuid::parse(s) {
        Ok(id) if id.to_string() == s => Ok(id),
        _ => Err(TreeError::Schema(format!("{key} must be a canonical lowercase UUID"))),
    }
}

pub(crate) fn uuid_batch_field(o: &BTreeMap<String, CValue>, key: &str) -> Result<Vec<Uuid>, TreeError> {
    let Some(CValue::Arr(arr)) = o.get(key) else {
        return Err(TreeError::Schema(format!("{key} must be an array of UUIDs")));
    };
    if arr.is_empty() || arr.len() > MAX_BATCH_IDS {
        return Err(TreeError::Schema(format!("{key} must contain 1..={MAX_BATCH_IDS} IDs")));
    }
    let mut out = Vec::with_capacity(arr.len());
    let mut seen = BTreeSet::new();
    for (i, item) in arr.iter().enumerate() {
        let CValue::Str(s) = item else {
            return Err(TreeError::Schema(format!("{key}[{i}] must be a string")));
        };
        let id = match Uuid::parse(s) {
            Ok(id) if id.to_string() == *s => id,
            _ => return Err(TreeError::Schema(format!("{key}[{i}] must be a canonical lowercase UUID"))),
        };
        if !seen.insert(id) {
            return Err(TreeError::Schema(format!("duplicate ID {id} in {key}")));
        }
        out.push(id);
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    //! Corrupt states that no event can produce, built by reaching into the private fields.
    use super::*;

    const D: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

    fn tree_with_element() -> (Tree, Uuid) {
        let mut reg = ElementTypeRegistry::new();
        reg.register(ElementTypeDef::new("test.alpha", obj([]), "point").unwrap()).unwrap();
        let mut tree = Tree::new(Uuid::from_u128(1), Arc::new(reg)).unwrap();
        let id = Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001);
        let body = obj([
            ("id", uuid_value(&id)),
            ("meta", obj([])),
            ("name", text("e")),
            ("parent", uuid_value(&tree.root())),
            ("type", text("test.alpha")),
            ("visible", CValue::Bool(true)),
        ]);
        tree.apply(ELEMENT_CREATED, &body).unwrap();
        tree.verify().unwrap();
        (tree, id)
    }

    fn set(tree: &Tree, id: Uuid, state: &str, digest: Option<&str>) -> CValue {
        obj([
            ("digest", digest.map_or(CValue::Null, text)),
            ("element", uuid_value(&id)),
            ("stage", uuid_value(&tree.active_stage())),
            ("state", text(state)),
        ])
    }

    #[test]
    fn missing_slot_is_an_invariant_breach() {
        let (mut tree, id) = tree_with_element();
        tree.elements.get_mut(&id).unwrap().slots.clear();
        let body = set(&tree, id, "current", Some(D));
        assert_eq!(tree.apply(ELEMENT_SLOT_SET, &body).unwrap_err().code(), "invariant_violated");
        assert_eq!(tree.verify().unwrap_err().code(), "invariant_violated");
    }

    #[test]
    fn verify_refuses_a_current_slot_without_digest() {
        let (mut tree, id) = tree_with_element();
        let stage = tree.active_stage();
        let e = tree.elements.get_mut(&id).unwrap();
        e.slots.insert(stage, Slot { digest: None, state: ElementState::Current });
        e.rebuild_body();
        assert!(tree.verify().unwrap_err().to_string().contains("without a digest"));
    }

    #[test]
    fn verify_refuses_a_group_sharing_a_stage_id() {
        let (mut tree, _) = tree_with_element();
        let stage = tree.active_stage();
        let root = tree.root();
        tree.groups.insert(stage, Group::new(stage, Some(root), ElementName::default()));
        assert!(tree.verify().unwrap_err().to_string().contains("shares its ID with a stage"));
    }

    #[test]
    fn verify_refuses_cycles_orphans_and_misfiled_entries() {
        let (tree, id) = tree_with_element();
        let (a, b) = (Uuid::from_u128(0xa), Uuid::from_u128(0xb));
        let mut cyc = tree.clone();
        cyc.groups.insert(a, Group::new(a, Some(b), ElementName::default()));
        cyc.groups.insert(b, Group::new(b, Some(a), ElementName::default()));
        assert_eq!(cyc.verify().unwrap_err().code(), "invariant_violated");
        let mut orphan = tree.clone();
        orphan.groups.insert(a, Group::new(a, Some(b), ElementName::default()));
        assert_eq!(orphan.verify().unwrap_err().code(), "invariant_violated");
        let mut misfiled = tree.clone();
        let e = misfiled.elements.remove(&id).unwrap();
        misfiled.elements.insert(a, e);
        assert!(misfiled.verify().unwrap_err().to_string().contains("filed under"));
    }

    #[test]
    fn failing_commit_rolls_back_move_rename_and_visibility() {
        let (mut tree, eid) = tree_with_element();
        let root = tree.root();
        let g1 = Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0010);
        let g2 = Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0020);
        let missing = Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00ff);
        tree.apply(GROUP_CREATED, &obj([("id", uuid_value(&g1)), ("name", text("G1")), ("parent", uuid_value(&root))]))
            .unwrap();
        let before_digest = tree.state_digest().unwrap();
        let before_root_children = tree.children_of(&root).to_vec();
        assert!(!tree.has_moved);

        // Commit creates g2, moves eid and g2 into g1, renames eid, hides eid, then fails on missing entity.
        let failing_commit = vec![
            TreeEvent::new(
                GROUP_CREATED,
                obj([("id", uuid_value(&g2)), ("name", text("G2")), ("parent", uuid_value(&root))]),
            ),
            TreeEvent::new(
                ELEMENTS_MOVED,
                obj([("ids", CValue::Arr(vec![uuid_value(&eid), uuid_value(&g2)])), ("to", uuid_value(&g1))]),
            ),
            TreeEvent::new(RENAMED, obj([("id", uuid_value(&eid)), ("name", text("Renamed"))])),
            TreeEvent::new(
                VISIBILITY_SET,
                obj([("ids", CValue::Arr(vec![uuid_value(&eid)])), ("visible", CValue::Bool(false))]),
            ),
            TreeEvent::new(RENAMED, obj([("id", uuid_value(&missing)), ("name", text("Boom"))])),
        ];
        assert_eq!(tree.apply_commit(&failing_commit).unwrap_err().code(), "not_found");
        tree.verify().unwrap();
        assert_eq!(tree.state_digest().unwrap(), before_digest);
        assert_eq!(tree.children_of(&root), before_root_children.as_slice());
        assert!(tree.children_of(&g1).is_empty());
        assert!(tree.group(&g2).is_none());
        assert!(!tree.has_moved);
        assert_eq!(tree.element(&eid).unwrap().parent(), root);
        assert_eq!(tree.element(&eid).unwrap().name().as_untrusted_str(), "e");
        assert!(tree.element(&eid).unwrap().visible());
    }
}
