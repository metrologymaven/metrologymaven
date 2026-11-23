//! The element tree on the bus. Spec node `CORE.PRJ.TREE.MODEL` (the bus half; the tree,
//! its states, type registry and event application are in `fid_project::tree`, layer model).
//!
//! - [`TreeState`]: a [`Tree`] with the branch head and project settings, implementing the
//!   bus's read-only [`StateView`]. The leaf asks for `impl StateView for Tree`; the trait
//!   lives in `fid-bus` (engine, bundled SQLite) and `Tree` in `fid-project` (model, wasm32),
//!   so the orphan rule and the layer rule put the implementation on this wrapper.
//!   [`EntityRef`] lends its entity ID by reference, so the wrapper keeps one bus
//!   [`EntityId`] per entity, updated as commits create entities.
//! - [`TreeState::apply_commit`] (a new commit) and [`TreeState::replay_commit`] (a
//!   journaled commit read back): the events of one commit, all or nothing. The wrapper
//!   keeps a copy of the tree and the entity ID map and restores both on any error, so a
//!   refused commit leaves the state unchanged.
//! - [`to_bus_error`] (commit path): refusals a caller can cause become
//!   `validation/<reason>` (`schema`, `name`, `unregistered_type`, `bad_reference`,
//!   `bad_stage`, `hierarchy`; the nil or max UUID is `validation/schema`),
//!   `conflict/id_collision`, or `precondition/invalid_transition` (bus catalogue v3; the
//!   commit is aborted, REQ-003). `duplicate_type` and `duplicate_event` come from start-up
//!   registration only and can never come from a commit; they, `unknown_event` (an event
//!   type no domain registered) and a broken invariant become `internal/invariant_violated`
//!   with the tree's code first in the detail. ID service faults become
//!   `internal/handler_failed`.
//! - [`to_replay_error`] (replay path): as [`to_bus_error`], except that
//!   `invalid_transition` is `internal/invariant_violated`. A journaled commit was accepted
//!   once, so a transition it can no longer make means the state is corrupt. The stage
//!   state rules (`stage_not_found`, `stage_active`, `bad_order`) are treated the same way.
//! - Stage refusals (spec node `CORE.PRJ.STAGE.MODEL`, catalogue v3): `label` is
//!   `validation/label`, `bad_order` and `ambiguous_stage` are `validation/bad_stage`, `stage_not_found` is
//!   `not_found/unknown_entity` and `stage_active` is `precondition/stage_active`.
//! - The state view also serves the stage records (kind `project.stage`) and the stage list
//!   (kind `project.stage_list`), which the stage commands read.

use fid_bus::error::{
    BusError, ConflictReason, Detail, InternalReason, NotFoundReason, PreconditionReason, ValidationReason,
};
use fid_bus::handler::{EntityId, EntityRef, Event, SettingKey, StateView};
use fid_canon::{CValue, Digest};
use fid_project::{IdError, Tree, TreeError, TreeEvent, Uuid};
use std::collections::BTreeMap;

/// The bus error of a tree refusal on the commit path (see the module docs).
pub fn to_bus_error(e: &TreeError) -> BusError {
    let d = Detail::new(&e.to_string());
    match e {
        TreeError::Schema(_) | TreeError::Id(IdError::BadId) => BusError::Validation(ValidationReason::Schema, d),
        TreeError::Name => BusError::Validation(ValidationReason::Name, d),
        TreeError::UnregisteredType(_) => BusError::Validation(ValidationReason::UnregisteredType, d),
        TreeError::BadReference(_) => BusError::Validation(ValidationReason::BadReference, d),
        TreeError::NotFound(_) => BusError::NotFound(NotFoundReason::UnknownEntity, d),
        TreeError::BadStage(_) => BusError::Validation(ValidationReason::BadStage, d),
        TreeError::Hierarchy(_) => BusError::Validation(ValidationReason::Hierarchy, d),
        TreeError::IdCollision(_) => BusError::Conflict(ConflictReason::IdCollision, d),
        TreeError::Id(_) => BusError::Internal(InternalReason::HandlerFailed, d),
        TreeError::InvalidTransition { .. } => BusError::Precondition(PreconditionReason::InvalidTransition, d),
        TreeError::Label => BusError::Validation(ValidationReason::Label, d),
        TreeError::BadOrder(_) | TreeError::AmbiguousStage(_) => BusError::Validation(ValidationReason::BadStage, d),
        TreeError::StageNotFound(_) => BusError::NotFound(NotFoundReason::UnknownEntity, d),
        TreeError::StageActive(_) => BusError::Precondition(PreconditionReason::StageActive, d),
        TreeError::BadToken(_) => BusError::Validation(ValidationReason::BadToken, d),
        TreeError::StaleToken(_) => BusError::Validation(ValidationReason::StaleToken, d),
        TreeError::DuplicateType(_)
        | TreeError::DuplicateEvent(_)
        | TreeError::UnknownEvent(_)
        | TreeError::Invariant(_) => BusError::Internal(InternalReason::InvariantViolated, d),
    }
}

/// The bus error of a tree refusal on the replay path: an invalid transition in a journaled
/// commit means corrupt state (`internal/invariant_violated`); everything else as
/// [`to_bus_error`].
pub fn to_replay_error(e: &TreeError) -> BusError {
    match e {
        TreeError::InvalidTransition { .. }
        | TreeError::NotFound(_)
        | TreeError::StageNotFound(_)
        | TreeError::StageActive(_)
        | TreeError::BadOrder(_) => BusError::Internal(InternalReason::InvariantViolated, Detail::new(&e.to_string())),
        other => to_bus_error(other),
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
        let stages: Vec<Uuid> = tree.stage_records().map(|s| s.id()).collect();
        let all = tree.groups().map(|g| g.id()).chain(tree.elements().map(|e| e.id())).chain(stages);
        for id in all.chain([tree.stage_list_id(), tree.group_list_id(), tree.tree_index_id()]) {
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

    /// Applies the events of a new commit, all or nothing; a refusal is returned as a bus
    /// error ([`to_bus_error`]) and changes nothing.
    pub fn apply_commit(&mut self, events: &[Event]) -> Result<(), BusError> {
        self.commit(events, to_bus_error)
    }

    /// Applies the events of a journaled commit during replay, all or nothing; a refusal is
    /// returned as a bus error ([`to_replay_error`]) and changes nothing.
    pub fn replay_commit(&mut self, events: &[Event]) -> Result<(), BusError> {
        self.commit(events, to_replay_error)
    }

    fn commit(&mut self, events: &[Event], map: fn(&TreeError) -> BusError) -> Result<(), BusError> {
        let saved = (self.tree.clone(), self.ids.clone());
        let r = self.commit_unguarded(events, map);
        if r.is_err() {
            (self.tree, self.ids) = saved;
        }
        r
    }

    fn commit_unguarded(&mut self, events: &[Event], map: fn(&TreeError) -> BusError) -> Result<(), BusError> {
        let evs: Vec<TreeEvent> = events.iter().map(tree_event).collect();
        let created = self.tree.apply_commit(&evs).map_err(|e| map(&e))?;
        for id in created {
            // Canonical UUID text (36 ASCII bytes) is always a valid entity ID; should that
            // ever fail, the caller restores the state from before the commit.
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
