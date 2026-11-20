//! Stages: the stage entity, its events and the logic of the four stage commands. Spec node
//! `CORE.PRJ.STAGE.MODEL` (CONSTITUTION C6 untrusted text, §4 ordered collections only, §5
//! stable IDs, §8 no panics).
//!
//! - [`Stage`] `{ id, run, label, order, serial, metadata, removed }`: one measured instance
//!   (part, serial, repeated measurement) of a Run against the project's shared plan. Labels
//!   and serials are untrusted text with the container label rules (`label` otherwise).
//!   `order` is not stored: it is the stage's position in the project's stage sequence
//!   ([`crate::Tree::stage_order`]), so a reorder is one small event.
//! - The element tree ([`crate::Tree`]) holds the stage records and applies the three stage
//!   events ([`STAGE_ADDED`], [`STAGE_REMOVED`], [`STAGES_REORDERED`]); the per-stage slots
//!   are the element slots of `CORE.PRJ.TREE.MODEL`. Applying `StageAdded` gives every
//!   element a `not_computed` slot for the new stage in the same commit (REQ-002), with no
//!   per-element events. A project without explicit stages has the implicit default stage
//!   of `CORE.PRJ.TREE.MODEL`. The first `StageAdded` adopts it only while none of its
//!   slots holds data (every slot is `not_computed` with no digest): the new stage takes its
//!   place in the sequence and as the active stage, and the event says so (`"adopts":
//!   <default id>`). If the default stage holds data, it becomes an explicit stage instead
//!   (label [`DEFAULT_STAGE_LABEL`], the new stage's run) that keeps its reserved ID, its
//!   data and the active-stage role; the new stage is appended with empty slots
//!   (`"adopts": null`). Nothing moves silently. The reserved default-stage ID is never
//!   importable, even once adopted.
//! - Removal is soft (REQ-004): the record is marked `removed`, every element keeps its slot
//!   for it (the data stays in the state and the journal), a removed stage takes no new
//!   slots, no slot changes and no new elements' slots, and `project.list_stages@1` hides it
//!   unless `include_removed` is set. Undo is a head move: the state at the previous head
//!   has the stage live again with its data. Removing the active stage is `stage_active`
//!   (REQ-005).
//! - [`add_stage`], [`remove_stage`], [`reorder_stages`] and [`list_stages`] are the whole
//!   logic of the four commands: they read the state through a [`ContainerView`] and return
//!   the one event (or, for the R0 list, the result); they write nothing. The bus handlers,
//!   registry definitions and bus error mapping live in `fid-project-cmd`. The view has no
//!   listing, so the tree publishes the stage sequence as one entity, the stage list
//!   ([`STAGE_LIST_KIND`], ID [`stage_list_id`]): `{"active_stage", "adoptable": <default
//!   id> | null, "stages": [<id>, ...]}` in project order, removed stages included; each
//!   stage is an entity of kind
//!   [`STAGE_KIND`] whose body is [`Stage::body`].
//!
//! Check order of a command, first failure wins: parameter shape (`schema`), text
//! (`label`), references (`bad_reference`: the run must be a Run), the state rules
//! (`stage_not_found`, `stage_active`, `bad_order`; for add, whether the default stage is
//! adopted), then the ID. Only an add that passes
//! every check draws an ID (as the container commands do).
//!
//! Wire forms (RFC 8785; IDs are canonical lowercase UUID text):
//! - stage body: `{"id", "label", "metadata": {}, "removed": bool, "run", "serial": string |
//!   null}`;
//! - [`STAGE_ADDED`] `{"adopts": <id> | null, "id", "label", "metadata", "run", "serial"}`;
//! - [`STAGE_REMOVED`] `{"stage"}`;
//! - [`STAGES_REORDERED`] `{"order": [<id>, ...], "run"}`: the run's live stages in their new
//!   order; they take, in that order, the positions in the project sequence that the run's
//!   live stages held. Removed stages and other runs' stages keep their positions.
//! - list result: `{"stages": [<stage body> + "order": n, ...]}` in project order.

use crate::container::{ContainerError, ContainerKind, ContainerView, Hierarchy, Label};
use crate::id::{IdService, ObservationSource, Uuid};
use crate::tree::{reserved_id, TreeError, TreeEvent};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};

/// Entity kind of a stage in the state view.
pub const STAGE_KIND: &str = "project.stage";
/// Entity kind of the stage list in the state view.
pub const STAGE_LIST_KIND: &str = "project.stage_list";
/// Event: a stage was added.
pub const STAGE_ADDED: &str = "StageAdded";
/// Event: a stage was removed (soft).
pub const STAGE_REMOVED: &str = "StageRemoved";
/// Event: a run's live stages were reordered.
pub const STAGES_REORDERED: &str = "StagesReordered";
/// The stage events, sorted.
pub const STAGE_EVENTS: [&str; 4] =
    [crate::stage_switch::ACTIVE_STAGE_SET, STAGE_ADDED, STAGE_REMOVED, STAGES_REORDERED];
/// Command names, without major.
pub const ADD_STAGE: &str = "project.add_stage";
pub const REMOVE_STAGE: &str = "project.remove_stage";
pub const REORDER_STAGES: &str = "project.reorder_stages";
pub const LIST_STAGES: &str = "project.list_stages";
/// Label of the explicit stage the implicit default stage becomes when it holds data.
pub const DEFAULT_STAGE_LABEL: &str = "Default";

// ------------------------------------------------------------------ the entity

/// One stage (see the module docs). Read-only outside this crate; events change it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    id: Uuid,
    run: Uuid,
    label: Label,
    serial: Option<Label>,
    metadata: CValue,
    removed: bool,
    body: CValue,
}

impl Stage {
    pub(crate) fn new(
        id: Uuid,
        run: Uuid,
        label: Label,
        serial: Option<Label>,
        metadata: CValue,
        removed: bool,
    ) -> Stage {
        let mut s = Stage { id, run, label, serial, metadata, removed, body: CValue::Null };
        s.rebuild_body();
        s
    }

    fn rebuild_body(&mut self) {
        self.body = obj([
            ("id", uuid_value(&self.id)),
            ("label", text(self.label.as_untrusted_str())),
            ("metadata", self.metadata.clone()),
            ("removed", CValue::Bool(self.removed)),
            ("run", uuid_value(&self.run)),
            ("serial", self.serial.as_ref().map_or(CValue::Null, |s| text(s.as_untrusted_str()))),
        ]);
    }

    pub(crate) fn set_removed(&mut self, removed: bool) {
        self.removed = removed;
        self.rebuild_body();
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    /// The Run the stage belongs to.
    pub fn run(&self) -> Uuid {
        self.run
    }

    /// The untrusted label.
    pub fn label(&self) -> &Label {
        &self.label
    }

    /// The untrusted serial, if any.
    pub fn serial(&self) -> Option<&Label> {
        self.serial.as_ref()
    }

    /// The metadata object (always `{}` for now: no command sets it yet).
    pub fn metadata(&self) -> &CValue {
        &self.metadata
    }

    /// Whether the stage was removed (soft).
    pub fn removed(&self) -> bool {
        self.removed
    }

    /// The canonical wire form (see the module docs).
    pub fn body(&self) -> &CValue {
        &self.body
    }

    /// Reads a stage body from the state. Exactly the keys of the wire form, canonical IDs,
    /// valid texts and an object `metadata`; anything else is `invariant_violated` (the state
    /// is corrupt, not the caller's input).
    pub fn from_value(v: &CValue) -> Result<Stage, TreeError> {
        let bad = |m: &str| TreeError::Invariant(format!("stage body: {m}"));
        let CValue::Obj(o) = v else {
            return Err(bad("not an object"));
        };
        let keys = ["id", "label", "metadata", "removed", "run", "serial"];
        if o.len() != keys.len() || !keys.iter().all(|k| o.contains_key(*k)) {
            return Err(bad("missing or extra fields"));
        }
        let id = state_uuid(o.get("id")).ok_or_else(|| bad("id"))?;
        let run = state_uuid(o.get("run")).ok_or_else(|| bad("run"))?;
        let label = match o.get("label") {
            Some(CValue::Str(s)) => Label::new(s).map_err(|_| bad("label"))?,
            _ => return Err(bad("label")),
        };
        let serial = match o.get("serial") {
            Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(Label::new(s).map_err(|_| bad("serial"))?),
            _ => return Err(bad("serial")),
        };
        let metadata = match o.get("metadata") {
            Some(m @ CValue::Obj(_)) => m.clone(),
            _ => return Err(bad("metadata")),
        };
        let removed = match o.get("removed") {
            Some(CValue::Bool(b)) => *b,
            _ => return Err(bad("removed")),
        };
        Ok(Stage::new(id, run, label, serial, metadata, removed))
    }
}

/// The checked body of a `StageAdded` event: the new, live stage and the default stage it
/// adopts (`adopts`, or `None`). `schema` for a wrong shape, `label` for a label or serial
/// breaking the text rules.
pub(crate) fn stage_from_added(body: &CValue) -> Result<(Stage, Option<Uuid>), TreeError> {
    let o = exact_keys(body, &["adopts", "id", "label", "metadata", "run", "serial"])?;
    let adopts = match o.get("adopts") {
        Some(CValue::Null) => None,
        _ => Some(uuid_field(o, "adopts")?),
    };
    let id = uuid_field(o, "id")?;
    let run = uuid_field(o, "run")?;
    let label = str_field(o, "label")?;
    let serial = match o.get("serial") {
        Some(CValue::Null) => None,
        Some(CValue::Str(s)) => Some(s.as_str()),
        _ => return Err(schema("serial must be a string or null")),
    };
    let metadata = match o.get("metadata") {
        Some(m @ CValue::Obj(_)) => m.clone(),
        _ => return Err(schema("metadata must be an object")),
    };
    let label = stage_text(label)?;
    let serial = serial.map(stage_text).transpose()?;
    Ok((Stage::new(id, run, label, serial, metadata, false), adopts))
}

/// A label or serial: the container label rules (1 to 256 characters, no Cc or bidi
/// control, NFC); `label` otherwise.
fn stage_text(s: &str) -> Result<Label, TreeError> {
    Label::new(s).map_err(|_| TreeError::Label)
}

/// The stage removed by a `StageRemoved` body; `schema` for a wrong shape.
pub(crate) fn removed_stage(body: &CValue) -> Result<Uuid, TreeError> {
    let o = exact_keys(body, &["stage"])?;
    uuid_field(o, "stage")
}

/// The run and new order of a `StagesReordered` body; `schema` for a wrong shape.
pub(crate) fn reordered(body: &CValue) -> Result<(Uuid, Vec<Uuid>), TreeError> {
    let o = exact_keys(body, &["order", "run"])?;
    Ok((uuid_field(o, "run")?, uuid_list(o, "order")?))
}

/// `Ok` when `order` is exactly a permutation of `live` (no missing, extra or repeated ID);
/// `bad_order` otherwise.
pub(crate) fn check_permutation(order: &[Uuid], live: &[Uuid]) -> Result<(), TreeError> {
    let mut seen = BTreeSet::new();
    for id in order {
        if !seen.insert(*id) {
            return Err(TreeError::BadOrder(format!("stage {id} is listed twice")));
        }
        if !live.contains(id) {
            return Err(TreeError::BadOrder(format!("{id} is not a live stage of the run")));
        }
    }
    if let Some(missing) = live.iter().find(|id| !seen.contains(*id)) {
        return Err(TreeError::BadOrder(format!("live stage {missing} of the run is missing")));
    }
    Ok(())
}

// ------------------------------------------------------------------ reading the view

/// The ID of the stage list entity: the reserved UUIDv5 of the project namespace over
/// `{"kind": "project.stage_list"}`.
pub fn stage_list_id(namespace: Uuid) -> Result<Uuid, TreeError> {
    reserved_id(namespace, "project.stage_list")
}

/// The project's stages as a command sees them through the view.
#[derive(Debug, Clone, PartialEq)]
pub struct StageListing {
    /// The active stage (the implicit default stage while there are no explicit stages).
    pub active_stage: Uuid,
    /// The implicit default stage the next added stage adopts, if it holds no data.
    pub adoptable: Option<Uuid>,
    /// Every explicit stage, removed ones included, in project order.
    pub stages: Vec<Stage>,
}

impl StageListing {
    /// Reads the stage list entity and every stage it names. A missing or malformed list or
    /// stage is `invariant_violated`.
    pub fn read(view: &dyn ContainerView, namespace: Uuid) -> Result<StageListing, TreeError> {
        let head = ListHead::read(view, namespace)?;
        let mut stages = Vec::with_capacity(head.stages.len());
        for id in &head.stages {
            stages.push(read_stage(view, id)?);
        }
        Ok(StageListing { active_stage: head.active_stage, adoptable: head.adoptable, stages })
    }

    /// The live stages of `run`, in project order.
    pub fn live_of_run(&self, run: &Uuid) -> Vec<Uuid> {
        self.stages.iter().filter(|s| !s.removed && s.run == *run).map(|s| s.id).collect()
    }
}

/// One stage entity read from the view; `invariant_violated` if it is missing or malformed.
pub(crate) fn read_stage(view: &dyn ContainerView, id: &Uuid) -> Result<Stage, TreeError> {
    let bad = |m: String| TreeError::Invariant(format!("stage list: {m}"));
    let Some((kind, body)) = view.entity(id) else {
        return Err(bad(format!("stage {id} is missing")));
    };
    if kind != STAGE_KIND {
        return Err(bad(format!("entity {id} is not a stage")));
    }
    let s = Stage::from_value(body)?;
    if s.id != *id {
        return Err(bad(format!("stage {id} body names another ID")));
    }
    Ok(s)
}

/// The stage list entity alone (no stage bodies): what add and remove need, read in
/// O(stages) without parsing every stage.
pub(crate) struct ListHead {
    pub(crate) active_stage: Uuid,
    pub(crate) adoptable: Option<Uuid>,
    pub(crate) stages: Vec<Uuid>,
}

impl ListHead {
    pub(crate) fn read(view: &dyn ContainerView, namespace: Uuid) -> Result<ListHead, TreeError> {
        let bad = |m: String| TreeError::Invariant(format!("stage list: {m}"));
        let lid = stage_list_id(namespace)?;
        let Some((kind, body)) = view.entity(&lid) else {
            return Err(bad("the state has no stage list".to_string()));
        };
        if kind != STAGE_LIST_KIND {
            return Err(bad(format!("entity {lid} is not the stage list")));
        }
        let CValue::Obj(o) = body else {
            return Err(bad("not an object".to_string()));
        };
        if o.len() != 3 {
            return Err(bad("missing or extra fields".to_string()));
        }
        let active_stage = state_uuid(o.get("active_stage")).ok_or_else(|| bad("active_stage".to_string()))?;
        let adoptable = match o.get("adoptable") {
            Some(CValue::Null) => None,
            v => Some(state_uuid(v).ok_or_else(|| bad("adoptable".to_string()))?),
        };
        let Some(CValue::Arr(ids)) = o.get("stages") else {
            return Err(bad("stages must be an array".to_string()));
        };
        let stages = ids
            .iter()
            .map(|v| match v {
                CValue::Str(s) => canonical_uuid(s),
                _ => None,
            })
            .map(|id| id.ok_or_else(|| bad("a stage ID is not a canonical UUID".to_string())))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ListHead { active_stage, adoptable, stages })
    }
}

fn from_container(e: ContainerError) -> TreeError {
    match e {
        ContainerError::Schema(m) => TreeError::Schema(m),
        ContainerError::Label => TreeError::Label,
        ContainerError::BadReference(m) => TreeError::BadReference(m),
        ContainerError::Hierarchy(m) => TreeError::Hierarchy(m),
        ContainerError::IdCollision(id) => TreeError::IdCollision(id),
        ContainerError::Id(e) => TreeError::Id(e),
        ContainerError::BadState(m) => TreeError::Invariant(m),
    }
}

fn expect_run(view: &dyn ContainerView, run: &Uuid) -> Result<(), TreeError> {
    Hierarchy::new(view).expect(run, ContainerKind::Run, "run").map(|_| ()).map_err(from_container)
}

// ------------------------------------------------------------------ commands

pub(crate) type Params<'a> = &'a BTreeMap<String, CValue>;

pub(crate) fn params_obj<'a>(params: &'a CValue, allowed: &[&str]) -> Result<Params<'a>, TreeError> {
    let CValue::Obj(p) = params else {
        return Err(schema("parameters must be an object"));
    };
    if let Some(k) = p.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(TreeError::Schema(format!("unknown parameter {k:?}")));
    }
    Ok(p)
}

pub(crate) fn required<'a>(p: Params<'a>, key: &str) -> Result<&'a CValue, TreeError> {
    p.get(key).ok_or_else(|| TreeError::Schema(format!("{key} is required")))
}

pub(crate) fn param_uuid(v: &CValue, key: &str) -> Result<Uuid, TreeError> {
    match v {
        CValue::Str(s) => {
            canonical_uuid(s).ok_or_else(|| TreeError::Schema(format!("{key} must be a canonical lowercase UUID")))
        }
        _ => Err(TreeError::Schema(format!("{key} must be a string"))),
    }
}

/// `project.add_stage@1`: params `{run, label, serial?, id?}` (`id` imports an ID unchanged;
/// omitted, a UUIDv7 is drawn from `source`). Returns the one `StageAdded` event.
pub fn add_stage(
    view: &dyn ContainerView,
    params: &CValue,
    namespace: Uuid,
    source: &mut dyn ObservationSource,
) -> Result<TreeEvent, TreeError> {
    // 1. Shape.
    let p = params_obj(params, &["id", "label", "run", "serial"])?;
    let run = param_uuid(required(p, "run")?, "run")?;
    let label = match required(p, "label")? {
        CValue::Str(s) => s.as_str(),
        _ => return Err(schema("label must be a string")),
    };
    let serial = match p.get("serial") {
        None => None,
        Some(CValue::Str(s)) => Some(s.as_str()),
        Some(_) => return Err(schema("serial must be a string")),
    };
    let imported = p.get("id").map(|v| param_uuid(v, "id")).transpose()?;
    if imported.is_some_and(|id| id == Uuid::NIL || id == Uuid::MAX) {
        return Err(schema("id must not be the nil or max UUID"));
    }
    // 2. Text.
    let label = stage_text(label)?;
    let serial = serial.map(stage_text).transpose()?;
    // 3. References.
    expect_run(view, &run)?;
    // 4. State: whether the implicit default stage is adopted (it holds no data).
    let adopts = ListHead::read(view, namespace)?.adoptable;
    // 5. The ID: imported unchanged, or a fresh UUIDv7. Nothing is drawn before this point.
    // The reserved default-stage ID is never available, even once replaced.
    let reserved = reserved_id(namespace, "project.default_stage")?;
    let h = Hierarchy::new(view);
    let mut ids = IdService::new(namespace, source, &h);
    let id = match imported {
        Some(id) if id == reserved => return Err(TreeError::IdCollision(id)),
        Some(id) => ids.import_uuid(id)?,
        None => {
            let id = ids.new_v7()?;
            if id == reserved || crate::id::IdIndex::contains_id(&h, &id) {
                return Err(TreeError::IdCollision(id));
            }
            id
        }
    };
    let body = obj([
        ("adopts", adopts.as_ref().map_or(CValue::Null, uuid_value)),
        ("id", uuid_value(&id)),
        ("label", text(label.as_untrusted_str())),
        ("metadata", CValue::Obj(BTreeMap::new())),
        ("run", uuid_value(&run)),
        ("serial", serial.as_ref().map_or(CValue::Null, |s| text(s.as_untrusted_str()))),
    ]);
    Ok(TreeEvent::new(STAGE_ADDED, body))
}

/// `project.remove_stage@1`: params `{stage}`. `stage_not_found` if it is not a live stage
/// of the project, `stage_active` if it is the active stage. Returns the one `StageRemoved`
/// event.
pub fn remove_stage(view: &dyn ContainerView, params: &CValue, namespace: Uuid) -> Result<TreeEvent, TreeError> {
    let p = params_obj(params, &["stage"])?;
    let stage = param_uuid(required(p, "stage")?, "stage")?;
    let head = ListHead::read(view, namespace)?;
    if !head.stages.contains(&stage) || read_stage(view, &stage)?.removed {
        return Err(TreeError::StageNotFound(stage.to_string()));
    }
    if head.active_stage == stage {
        return Err(TreeError::StageActive(stage));
    }
    Ok(TreeEvent::new(STAGE_REMOVED, obj([("stage", uuid_value(&stage))])))
}

/// `project.reorder_stages@1`: params `{run, order}`. `bad_reference` if `run` is not a Run,
/// `bad_order` unless `order` is exactly a permutation of the run's live stages. Returns the
/// one `StagesReordered` event.
pub fn reorder_stages(view: &dyn ContainerView, params: &CValue, namespace: Uuid) -> Result<TreeEvent, TreeError> {
    let p = params_obj(params, &["order", "run"])?;
    let run = param_uuid(required(p, "run")?, "run")?;
    let order = match required(p, "order")? {
        CValue::Arr(a) => a.iter().map(|v| param_uuid(v, "order")).collect::<Result<Vec<_>, _>>()?,
        _ => return Err(schema("order must be an array")),
    };
    expect_run(view, &run)?;
    let listing = StageListing::read(view, namespace)?;
    check_permutation(&order, &listing.live_of_run(&run))?;
    let body = obj([("order", CValue::Arr(order.iter().map(uuid_value).collect())), ("run", uuid_value(&run))]);
    Ok(TreeEvent::new(STAGES_REORDERED, body))
}

/// `project.list_stages@1` (R0): params `{run?, include_removed?}`. The explicit stages in
/// project order, each its body plus `order` (its position in the project sequence),
/// filtered to `run` and, unless `include_removed`, to live stages.
pub fn list_stages(view: &dyn ContainerView, params: &CValue, namespace: Uuid) -> Result<CValue, TreeError> {
    let p = params_obj(params, &["include_removed", "run"])?;
    let include_removed = match p.get("include_removed") {
        None => false,
        Some(CValue::Bool(b)) => *b,
        Some(_) => return Err(schema("include_removed must be a boolean")),
    };
    let run = p.get("run").map(|v| param_uuid(v, "run")).transpose()?;
    if let Some(r) = &run {
        expect_run(view, r)?;
    }
    let listing = StageListing::read(view, namespace)?;
    let mut out = Vec::new();
    for (i, s) in listing.stages.iter().enumerate() {
        if (s.removed && !include_removed) || run.is_some_and(|r| r != s.run) {
            continue;
        }
        let mut b = match s.body.clone() {
            CValue::Obj(b) => b,
            _ => return Err(TreeError::Invariant("stage body is not an object".to_string())),
        };
        let order = u32::try_from(i).map_err(|_| TreeError::Invariant("too many stages".to_string()))?;
        b.insert("order".to_string(), CValue::Num(f64::from(order)));
        out.push(CValue::Obj(b));
    }
    Ok(obj([("stages", CValue::Arr(out))]))
}

// ------------------------------------------------------------------ wire helpers

pub(crate) fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

pub(crate) fn uuid_value(id: &Uuid) -> CValue {
    CValue::Str(id.to_string())
}

pub(crate) fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn schema(m: &str) -> TreeError {
    TreeError::Schema(m.to_string())
}

fn canonical_uuid(s: &str) -> Option<Uuid> {
    let id = Uuid::parse(s).ok()?;
    (id.to_string() == s).then_some(id)
}

fn state_uuid(v: Option<&CValue>) -> Option<Uuid> {
    match v {
        Some(CValue::Str(s)) => canonical_uuid(s),
        _ => None,
    }
}

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

fn uuid_field(o: &BTreeMap<String, CValue>, key: &str) -> Result<Uuid, TreeError> {
    let s = str_field(o, key)?;
    canonical_uuid(s).ok_or_else(|| TreeError::Schema(format!("{key} must be a canonical lowercase UUID")))
}

fn uuid_list(o: &BTreeMap<String, CValue>, key: &str) -> Result<Vec<Uuid>, TreeError> {
    let Some(CValue::Arr(a)) = o.get(key) else {
        return Err(TreeError::Schema(format!("{key} must be an array")));
    };
    a.iter()
        .map(|v| match v {
            CValue::Str(s) => canonical_uuid(s),
            _ => None,
        })
        .map(|u| u.ok_or_else(|| TreeError::Schema(format!("{key} must hold canonical lowercase UUIDs"))))
        .collect()
}
