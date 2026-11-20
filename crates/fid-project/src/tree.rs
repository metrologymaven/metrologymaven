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
//!   `unregistered_type` (REQ-004).
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
//! element type is registered, and every element has exactly one slot per stage.
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

use crate::container::{is_bidi_control, is_nfc};
use crate::id::{IdError, IdIndex, IdService, ReplaySource, Uuid};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

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
/// Core event: a group was created.
pub const GROUP_CREATED: &str = "GroupCreated";
/// Core event: an element was created.
pub const ELEMENT_CREATED: &str = "ElementCreated";
/// Core event: one slot of an element changed state.
pub const ELEMENT_SLOT_SET: &str = "ElementSlotSet";
/// The core event types, sorted.
pub const CORE_EVENTS: [&str; 3] = [ELEMENT_CREATED, ELEMENT_SLOT_SET, GROUP_CREATED];
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
    /// A named parent group or element does not exist (`bad_reference`).
    BadReference(String),
    /// A named stage is not a stage of the project (`bad_stage`).
    BadStage(Uuid),
    /// A group would be deeper than [`MAX_GROUP_DEPTH`] (`hierarchy`).
    Hierarchy(String),
    /// The requested state change is not an allowed transition (`invalid_transition`).
    InvalidTransition { from: ElementState, to: ElementState },
    /// The ID is already in the project (`id_collision`).
    IdCollision(Uuid),
    /// The ID service refused the ID or failed (`bad_id`, ...).
    Id(IdError),
    /// The tree breaks one of its invariants (`invariant_violated`).
    Invariant(String),
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
            TreeError::BadStage(_) => "bad_stage",
            TreeError::Hierarchy(_) => "hierarchy",
            TreeError::InvalidTransition { .. } => "invalid_transition",
            TreeError::IdCollision(_) => "id_collision",
            TreeError::Id(e) => e.code(),
            TreeError::Invariant(_) => "invariant_violated",
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
            | TreeError::Hierarchy(m)
            | TreeError::Invariant(m) => write!(f, "{}: {m}", self.code()),
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
        if CORE_EVENTS.contains(&event_type) || self.events.contains_key(event_type) {
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

    fn to_value(self) -> CValue {
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
        let body = obj([
            ("id", uuid_value(&id)),
            ("name", text(name.as_untrusted_str())),
            ("parent", parent.as_ref().map_or(CValue::Null, uuid_value)),
        ]);
        Group { id, parent, name, body }
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
}

// ------------------------------------------------------------------ tree

/// What one change replaced, so a refused commit can be rolled back.
enum Undo {
    Element(Uuid, Option<Element>),
    Group(Uuid, Option<Group>),
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
}

/// The reserved UUIDv5 of the project namespace for `{"kind": <kind>}`.
fn reserved_id(namespace: Uuid, kind: &str) -> Result<Uuid, TreeError> {
    let mut no_source = ReplaySource::new(Vec::new());
    let none: Vec<Uuid> = Vec::new();
    let ids = IdService::new(namespace, &mut no_source, &none);
    Ok(ids.derive_v5(&obj([("kind", text(kind))]))?)
}

impl Tree {
    /// An empty tree: the root group and the implicit default stage, which is also the
    /// active stage. Both IDs are reserved UUIDv5s of `namespace` (see the module docs).
    pub fn new(namespace: Uuid, registry: Arc<ElementTypeRegistry>) -> Result<Tree, TreeError> {
        let root = reserved_id(namespace, "project.tree_root")?;
        let stage = reserved_id(namespace, "project.default_stage")?;
        let mut groups = BTreeMap::new();
        groups.insert(root, Group::new(root, None, ElementName::default()));
        Ok(Tree { registry, root, stages: vec![stage], active_stage: stage, elements: BTreeMap::new(), groups })
    }

    pub fn registry(&self) -> &ElementTypeRegistry {
        &self.registry
    }

    /// The root group.
    pub fn root(&self) -> Uuid {
        self.root
    }

    /// The stages: the implicit default stage while the project has no explicit stages.
    pub fn stages(&self) -> &[Uuid] {
        &self.stages
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
        self.groups.get(id).map(|g| (GROUP_KIND, &g.body))
    }

    /// The ordered tree: `{"active_stage", "elements": {<id>: <element body>}, "groups":
    /// {<id>: <group body>}, "root", "stages": [<id>, ...]}`.
    pub fn to_value(&self) -> CValue {
        let elements = self.elements.iter().map(|(id, e)| (id.to_string(), e.body.clone())).collect();
        let groups = self.groups.iter().map(|(id, g)| (id.to_string(), g.body.clone())).collect();
        obj([
            ("active_stage", uuid_value(&self.active_stage)),
            ("elements", CValue::Obj(elements)),
            ("groups", CValue::Obj(groups)),
            ("root", uuid_value(&self.root)),
            ("stages", CValue::Arr(self.stages.iter().map(uuid_value).collect())),
        ])
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
        match r {
            Ok(()) => Ok(created),
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
                    }
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
        let translate = *self.registry.events.get(event_type).ok_or_else(|| {
            TreeError::UnknownEvent(if is_event_type(event_type) {
                event_type.to_string()
            } else {
                "malformed event type".to_string()
            })
        })?;
        for ev in translate(body)? {
            if !CORE_EVENTS.contains(&ev.event_type.as_str()) {
                return Err(TreeError::Schema(format!("{event_type} translated into a non-core event")));
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
                undo.push(Undo::Group(id, None));
                self.groups.insert(id, g);
                created.push(id);
            }
            ELEMENT_CREATED => {
                let e = self.check_element_created(body)?;
                let id = e.id;
                undo.push(Undo::Element(id, None));
                self.elements.insert(id, e);
                created.push(id);
            }
            ELEMENT_SLOT_SET => {
                let (id, stage, slot) = self.check_slot_set(body)?;
                let Some(e) = self.elements.get_mut(&id) else {
                    return Err(invariant(format!("element {id} vanished")));
                };
                undo.push(Undo::Element(id, Some(e.clone())));
                e.slots.insert(stage, slot);
                e.rebuild_body();
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
        let slots = self.stages.iter().map(|s| (*s, Slot::NEW)).collect();
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
        if !self.stages.contains(&stage) {
            return Err(TreeError::BadStage(stage));
        }
        let from = e.slots.get(&stage).map_or(ElementState::NotComputed, |s| s.state);
        if !from.can_become(state) {
            return Err(TreeError::InvalidTransition { from, to: state });
        }
        Ok((id, stage, Slot { digest, state }))
    }

    /// Checks every invariant of the module docs; `invariant_violated` on the first breach.
    /// Fails closed: a tree that passes has no cycle, no orphan and no unregistered type.
    pub fn verify(&self) -> Result<(), TreeError> {
        match self.groups.get(&self.root) {
            Some(g) if g.parent.is_none() => {}
            _ => return Err(invariant("root group missing or has a parent".to_string())),
        }
        if !self.stages.contains(&self.active_stage) {
            return Err(invariant("active stage is not a stage".to_string()));
        }
        for g in self.groups.values() {
            self.depth(&g.id).map_err(|e| invariant(e.to_string()))?;
            if *g != Group::new(g.id, g.parent, g.name.clone()) {
                return Err(invariant(format!("group {} body is stale", g.id)));
            }
        }
        for e in self.elements.values() {
            if !self.groups.contains_key(&e.parent) {
                return Err(invariant(format!("element {} is an orphan", e.id)));
            }
            if self.groups.contains_key(&e.id) || self.stages.contains(&e.id) {
                return Err(invariant(format!("element {} shares its ID", e.id)));
            }
            if self.registry.get(&e.type_id).is_none() {
                return Err(invariant(format!("element {} has an unregistered type", e.id)));
            }
            if !e.slots.keys().eq(self.stages_sorted().iter()) {
                return Err(invariant(format!("element {} does not have one slot per stage", e.id)));
            }
            let mut fresh = e.clone();
            fresh.rebuild_body();
            if fresh.body != e.body {
                return Err(invariant(format!("element {} body is stale", e.id)));
            }
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
    /// Elements, groups (the root included) and stages hold IDs.
    fn contains_id(&self, id: &Uuid) -> bool {
        self.elements.contains_key(id) || self.groups.contains_key(id) || self.stages.contains(id)
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
