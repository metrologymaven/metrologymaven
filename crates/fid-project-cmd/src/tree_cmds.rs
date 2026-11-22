//! Tree group, move, rename and visibility commands on the bus.
//! Spec node `CORE.PRJ.TREE.GROUP` (the bus and registry half; the pure command logic lives in
//! `fid_project::tree_cmds`, layer model).
//!
//! - [`command_defs`]: `project.create_group@1`, `project.move_element@1`,
//!   `project.rename_element@1` and `project.set_visibility@1`, all W2, D0, undo `head_move`,
//!   on CLI (`fid project create-group | move-element | rename-element | set-visibility`) and
//!   MCP (`project_create_group | project_move_element | project_rename_element |
//!   project_set_visibility`).
//! - [`TreeHandler`] and [`register`]: bus handlers reading the immutable [`StateView`] through
//!   [`BusView`] and returning one event per commit.

use std::fmt;
use std::sync::Arc;

use fid_bus::error::{BusError, Detail, InternalReason};
use fid_bus::handler::{Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet, StateView, ValidParams};
use fid_canon::CValue;
use fid_project::container::ID_PATTERN;
use fid_project::{
    create_group, move_element, rename_element, set_visibility, ObservationSource, TreeEvent, Uuid, CREATE_GROUP,
    ELEMENTS_MOVED, GROUP_CREATED, MAX_BATCH_IDS, MOVE_ELEMENT, RENAMED, RENAME_ELEMENT, SET_VISIBILITY,
    VISIBILITY_SET,
};
use fid_registry::{CommandDef, DefError};

use crate::tree::to_bus_error;
use crate::{BusView, SharedSource};

const LIFECYCLE_SINCE: &str = "2026-11-22";

/// The four tree-organisation commands (`CORE.PRJ.TREE.GROUP`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TreeOp {
    CreateGroup,
    MoveElement,
    RenameElement,
    SetVisibility,
}

impl TreeOp {
    /// Every command, in [`command_defs`] order.
    pub const ALL: [TreeOp; 4] =
        [TreeOp::CreateGroup, TreeOp::MoveElement, TreeOp::RenameElement, TreeOp::SetVisibility];

    /// The command name, without major.
    pub fn command(self) -> &'static str {
        match self {
            TreeOp::CreateGroup => CREATE_GROUP,
            TreeOp::MoveElement => MOVE_ELEMENT,
            TreeOp::RenameElement => RENAME_ELEMENT,
            TreeOp::SetVisibility => SET_VISIBILITY,
        }
    }
}

fn event(e: TreeEvent) -> Result<Event, BusError> {
    Ok(Event::new(EventType::new(&e.event_type)?, e.body))
}

/// Runs one tree-organisation command against `view` and returns its single W2 event.
pub fn run_tree(
    op: TreeOp,
    view: &dyn StateView,
    params: &CValue,
    namespace: Uuid,
    source: &mut dyn ObservationSource,
) -> Result<Vec<Event>, BusError> {
    let v = BusView(view);
    let e = match op {
        TreeOp::CreateGroup => create_group(&v, params, namespace, source),
        TreeOp::MoveElement => move_element(&v, params),
        TreeOp::RenameElement => rename_element(&v, params),
        TreeOp::SetVisibility => set_visibility(&v, params),
    }
    .map_err(|e| to_bus_error(&e))?;
    Ok(vec![event(e)?])
}

/// Command result derived from its committed event.
pub fn result_of(e: &Event) -> Result<CValue, BusError> {
    let bad = || BusError::Internal(InternalReason::InvariantViolated, Detail::new("not a tree group event"));
    let CValue::Obj(b) = e.body() else {
        return Err(bad());
    };
    match e.type_id().as_str() {
        GROUP_CREATED => {
            let id = b.get("id").cloned().ok_or_else(bad)?;
            Ok(obj([("group", id)]))
        }
        ELEMENTS_MOVED => {
            let ids = b.get("ids").cloned().ok_or_else(bad)?;
            let to = b.get("to").cloned().ok_or_else(bad)?;
            Ok(obj([("ids", ids), ("to", to)]))
        }
        RENAMED => {
            let id = b.get("id").cloned().ok_or_else(bad)?;
            let name = b.get("name").cloned().ok_or_else(bad)?;
            Ok(obj([("id", id), ("name", name)]))
        }
        VISIBILITY_SET => {
            let ids = b.get("ids").cloned().ok_or_else(bad)?;
            let visible = b.get("visible").cloned().ok_or_else(bad)?;
            Ok(obj([("ids", ids), ("visible", visible)]))
        }
        _ => Err(bad()),
    }
}

// ------------------------------------------------------------------ handlers

/// Bus handler for one `CORE.PRJ.TREE.GROUP` command.
pub struct TreeHandler {
    op: TreeOp,
    namespace: Uuid,
    source: SharedSource,
}

impl TreeHandler {
    pub fn new(op: TreeOp, namespace: Uuid, source: SharedSource) -> Self {
        Self { op, namespace, source }
    }

    pub fn op(&self) -> TreeOp {
        self.op
    }
}

impl fmt::Debug for TreeHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TreeHandler").field("op", &self.op).finish_non_exhaustive()
    }
}

impl Handler for TreeHandler {
    fn run(&self, view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        if ctx.cancel().is_cancelled() {
            return Err(HandlerError::Cancelled);
        }
        let mut source =
            self.source.lock().map_err(|_| HandlerError::Failed(Detail::new("observation source lock poisoned")))?;
        Ok(run_tree(self.op, view, params.value(), self.namespace, &mut *source)?)
    }
}

/// Registers the four tree-organisation handlers, each with its definition from [`command_defs`].
pub fn register(set: &mut HandlerSet, namespace: Uuid, source: &SharedSource) -> Result<(), BusError> {
    let defs = command_defs().map_err(|e| {
        BusError::Internal(InternalReason::InvariantViolated, Detail::new(&format!("tree group definitions: {e}")))
    })?;
    for (op, def) in TreeOp::ALL.into_iter().zip(defs) {
        set.register(def, Box::new(TreeHandler::new(op, namespace, Arc::clone(source))))?;
    }
    Ok(())
}

// ------------------------------------------------------------------ definitions

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn id_schema(description: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text(ID_PATTERN)), ("description", text(description))])
}

fn id_array_schema(description: &str) -> CValue {
    obj([
        ("type", text("array")),
        ("minItems", CValue::Num(1.0)),
        ("maxItems", CValue::Num(MAX_BATCH_IDS as f64)),
        ("items", id_schema("Entity UUID.")),
        ("description", text(description)),
    ])
}

fn name_schema(description: &str) -> CValue {
    obj([("type", text("string")), ("x-untrusted", CValue::Bool(true)), ("description", text(description))])
}

fn object(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

struct Spec {
    op: TreeOp,
    title: &'static str,
    description: &'static str,
    params: CValue,
    result: CValue,
    effects: &'static [&'static str],
}

/// The four tree-organisation command definitions, in [`TreeOp::ALL`] order.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    const NAME_RULE: &str = "Untrusted name, 0 to 256 characters, no control or bidi control, NFC (else validation/name). Need not be unique.";
    let specs: [Spec; 4] = [
        Spec {
            op: TreeOp::CreateGroup,
            title: "Create group",
            description: "Creates an element-tree group under a parent group (the tree root if parent is omitted).",
            params: object(
                &["name"],
                obj([
                    ("id", id_schema("Imported ID, kept unchanged; omit it to get a new UUIDv7.")),
                    ("name", name_schema(NAME_RULE)),
                    ("parent", id_schema("Parent group UUID; defaults to the root group if omitted.")),
                ]),
            ),
            result: object(&["group"], obj([("group", id_schema("The new group's ID."))])),
            effects: &[GROUP_CREATED],
        },
        Spec {
            op: TreeOp::MoveElement,
            title: "Move elements",
            description: "Moves 1 to 10,000 elements or groups under a target group in one commit, preserving their relative order; fails with validation/hierarchy if a group would become its own ancestor.",
            params: object(
                &["ids", "to"],
                obj([
                    ("ids", id_array_schema("Distinct element or group IDs to move (1..=10,000), order preserved.")),
                    ("to", id_schema("Target parent group UUID.")),
                ]),
            ),
            result: object(
                &["ids", "to"],
                obj([
                    ("ids", id_array_schema("Moved IDs in order.")),
                    ("to", id_schema("Target parent group UUID.")),
                ]),
            ),
            effects: &[ELEMENTS_MOVED],
        },
        Spec {
            op: TreeOp::RenameElement,
            title: "Rename element",
            description: "Sets an element's or group's untrusted name (0..=256 characters) without checking uniqueness.",
            params: object(
                &["id", "name"],
                obj([("id", id_schema("Element or group UUID.")), ("name", name_schema(NAME_RULE))]),
            ),
            result: object(
                &["id", "name"],
                obj([
                    ("id", id_schema("Renamed element or group UUID.")),
                    ("name", name_schema("The saved untrusted name.")),
                ]),
            ),
            effects: &[RENAMED],
        },
        Spec {
            op: TreeOp::SetVisibility,
            title: "Set visibility",
            description: "Sets the saved visibility flag of 1 to 10,000 elements in one commit.",
            params: object(
                &["ids", "visible"],
                obj([
                    ("ids", id_array_schema("Distinct element IDs (1..=10,000).")),
                    ("visible", obj([("type", text("boolean")), ("description", text("Saved visibility flag."))])),
                ]),
            ),
            result: object(
                &["ids", "visible"],
                obj([
                    ("ids", id_array_schema("Updated element IDs.")),
                    ("visible", obj([("type", text("boolean")), ("description", text("Saved visibility flag."))])),
                ]),
            ),
            effects: &[VISIBILITY_SET],
        },
    ];
    specs
        .into_iter()
        .map(|Spec { op, title, description, params, result, effects }| {
            CommandDef::from_value(obj([
                ("id", text(op.command())),
                ("major", CValue::Num(1.0)),
                ("title", text(title)),
                ("description", text(description)),
                ("params_schema", params),
                ("result_schema", result),
                ("side_effect", text("W2")),
                ("mcp_projection", text("exposed")),
                ("determinism", text("D0")),
                ("idempotency", text("none")),
                ("fs_effect", text("none")),
                ("undo", text("head_move")),
                ("preconditions", CValue::Arr(Vec::new())),
                ("effects", strs(effects)),
                ("algorithms", CValue::Arr(Vec::new())),
                ("exposure", strs(&["cli", "mcp"])),
                ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
                ("human_only", CValue::Bool(false)),
                ("network", text("none")),
                ("web", text("available")),
                ("cli_bare", CValue::Bool(false)),
            ]))
        })
        .collect()
}
