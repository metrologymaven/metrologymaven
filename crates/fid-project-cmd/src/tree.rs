//! The element tree on the bus. Spec node `CORE.PRJ.TREE.MODEL` (the bus half; the tree,
//! its states, type registry and event application are in `fid_project::tree`, layer model).
//!
//! - [`TreeState`]: a [`Tree`] with the branch head and project settings, implementing the
//!   bus's read-only [`StateView`]. The leaf asks for `impl StateView for Tree`; the trait
//!   lives in `fid-bus` (engine, bundled SQLite) and `Tree` in `fid-project` (model, wasm32),
//!   so the orphan rule and the layer rule put the implementation on this wrapper.
//!   [`EntityRef`] lends its entity ID by reference, so the wrapper keeps one bus
//!   [`EntityId`] per entity, updated as commits create entities.
//! - [`TreeState::apply_commit`]: the events of one commit, all or nothing
//!   ([`Tree::apply_commit`]); a refused commit leaves the state unchanged.
//! - [`to_bus_error`]: refusals a caller can cause become `validation/<reason>` (`schema`,
//!   `name`, `unregistered_type`, `bad_reference`, `bad_stage`, `hierarchy`; the nil or max
//!   UUID is `validation/schema`) or `conflict/id_collision`. The bus catalogue has no
//!   `invalid_transition`, `duplicate_type`, `duplicate_event` or `unknown_event` reason: an
//!   event that breaks them at commit time, or a start-up registration clash, is a bug in
//!   the emitting domain, so they become `internal/invariant_violated` with the tree's code
//!   first in the detail. ID service faults become `internal/handler_failed`.

use fid_bus::error::{BusError, ConflictReason, Detail, InternalReason, ValidationReason};
use fid_bus::handler::{EntityId, EntityRef, Event, SettingKey, StateView};
use fid_canon::{CValue, Digest};
use fid_project::{IdError, Tree, TreeError, TreeEvent, Uuid};
use std::collections::BTreeMap;

/// The bus error of a tree refusal (see the module docs).
pub fn to_bus_error(e: &TreeError) -> BusError {
    let d = Detail::new(&e.to_string());
    match e {
        TreeError::Schema(_) | TreeError::Id(IdError::BadId) => BusError::Validation(ValidationReason::Schema, d),
        TreeError::Name => BusError::Validation(ValidationReason::Name, d),
        TreeError::UnregisteredType(_) => BusError::Validation(ValidationReason::UnregisteredType, d),
        TreeError::BadReference(_) => BusError::Validation(ValidationReason::BadReference, d),
        TreeError::BadStage(_) => BusError::Validation(ValidationReason::BadStage, d),
        TreeError::Hierarchy(_) => BusError::Validation(ValidationReason::Hierarchy, d),
        TreeError::IdCollision(_) => BusError::Conflict(ConflictReason::IdCollision, d),
        TreeError::Id(_) => BusError::Internal(InternalReason::HandlerFailed, d),
        TreeError::InvalidTransition { .. }
        | TreeError::DuplicateType(_)
        | TreeError::DuplicateEvent(_)
        | TreeError::UnknownEvent(_)
        | TreeError::Invariant(_) => BusError::Internal(InternalReason::InvariantViolated, d),
    }
}

/// A bus event as a tree event.
pub fn tree_event(e: &Event) -> TreeEvent {
    TreeEvent::new(e.type_id().as_str(), e.body().clone())
}

fn entity_id(id: &Uuid) -> Result<EntityId, BusError> {
    EntityId::new(&id.to_string())
}

/// The element tree as the bus state view (see the module docs).
#[derive(Debug, Clone)]
pub struct TreeState {
    tree: Tree,
    ids: BTreeMap<Uuid, EntityId>,
    head: Digest,
    settings: BTreeMap<String, CValue>,
}

impl TreeState {
    /// Wraps `tree` at branch head `head`, with no settings.
    pub fn new(tree: Tree, head: Digest) -> Result<TreeState, BusError> {
        let mut ids = BTreeMap::new();
        for id in tree.groups().map(|g| g.id()).chain(tree.elements().map(|e| e.id())) {
            ids.insert(id, entity_id(&id)?);
        }
        Ok(TreeState { tree, ids, head, settings: BTreeMap::new() })
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Moves the view to a new branch head (the journal computes it).
    pub fn set_head(&mut self, head: Digest) {
        self.head = head;
    }

    /// Sets a project setting the view reports.
    pub fn set_setting(&mut self, key: &SettingKey, value: CValue) {
        self.settings.insert(key.as_str().to_string(), value);
    }

    /// Applies the events of one commit, all or nothing; a refusal is returned as a bus
    /// error ([`to_bus_error`]) and changes nothing.
    pub fn apply_commit(&mut self, events: &[Event]) -> Result<(), BusError> {
        let evs: Vec<TreeEvent> = events.iter().map(tree_event).collect();
        let created = self.tree.apply_commit(&evs).map_err(|e| to_bus_error(&e))?;
        // Canonical UUID text (36 ASCII bytes) is always a valid entity ID. Should that ever
        // fail, the entity stays unreachable through the view (fails closed) and the error
        // is reported as internal.
        for id in created {
            let key = entity_id(&id).map_err(|e| {
                BusError::Internal(InternalReason::InvariantViolated, Detail::new(&format!("entity id {id}: {e}")))
            })?;
            self.ids.insert(id, key);
        }
        Ok(())
    }
}

impl StateView for TreeState {
    /// The element or group whose ID is the canonical text of a UUID; any other text is
    /// `None`.
    fn entity(&self, id: &EntityId) -> Option<EntityRef<'_>> {
        let uuid = Uuid::parse(id.as_str()).ok()?;
        let (stored, (kind, body)) = (self.ids.get(&uuid)?, self.tree.entity(&uuid)?);
        (stored.as_str() == id.as_str()).then_some(EntityRef { id: stored, kind, body })
    }

    fn head(&self) -> Digest {
        self.head
    }

    fn setting(&self, key: &SettingKey) -> Option<CValue> {
        self.settings.get(key.as_str()).cloned()
    }
}
