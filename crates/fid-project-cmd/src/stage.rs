//! Stage commands on the bus. Spec node `CORE.PRJ.STAGE.MODEL` (the command half; the stage
//! entity, its events and the command logic are in `fid_project::stage`, layer model).
//!
//! - [`command_defs`]: `project.add_stage@1`, `project.remove_stage@1`,
//!   `project.reorder_stages@1` (W2, D0, undo `head_move`, effects `StageAdded`,
//!   `StageRemoved`, `StagesReordered`), `project.list_stages@1` (R0, D0) and, for spec node
//!   `CORE.PRJ.STAGE.SWITCH`, `project.set_active_stage@1` (W2, D0, undo `head_move`, effect
//!   `ActiveStageSet`), on the CLI (`fid project add-stage`, ...) and MCP
//!   (`project_add_stage`, ...) (REQ-007).
//! - [`StageHandler`] and [`register`]: the five bus handlers. Each reads the immutable
//!   [`StateView`] through [`BusView`] and runs the `fid_project` command function; the W2
//!   handlers return their one event, the R0 list handler returns none (its result is
//!   [`list_result`]).
//! - Refusals map to the bus error catalogue v3 through [`crate::tree::to_bus_error`]:
//!   `schema` and `bad_order` are `validation/schema` and `validation/bad_stage`, `label` is
//!   `validation/label`, a run that is not a Run is `validation/bad_reference`, a stage that
//!   is not a live stage is `not_found/unknown_entity`, removing the active stage is
//!   `precondition/stage_active`, an ID already in the project is `conflict/id_collision`.

use crate::tree::to_bus_error;
use crate::{BusView, SharedSource};
use fid_bus::error::{BusError, Detail, InternalReason};
use fid_bus::handler::{Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet, StateView, ValidParams};
use fid_canon::CValue;
use fid_project::container::ID_PATTERN;
use fid_project::{
    add_stage, list_stages, remove_stage, reorder_stages, set_active_stage, ObservationSource, TreeEvent, Uuid,
    ACTIVE_STAGE_SET, ADD_STAGE, LIST_STAGES, REMOVE_STAGE, REORDER_STAGES, SET_ACTIVE_STAGE, STAGES_REORDERED,
    STAGE_ADDED, STAGE_REMOVED,
};
use fid_registry::{CommandDef, DefError};
use std::fmt;
use std::sync::Arc;

const LIFECYCLE_SINCE: &str = "2026-11-20";

/// The five stage commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StageOp {
    Add,
    Remove,
    Reorder,
    List,
    /// `project.set_active_stage@1` (spec node `CORE.PRJ.STAGE.SWITCH`).
    SetActive,
}

impl StageOp {
    /// Every command, in [`command_defs`] order.
    pub const ALL: [StageOp; 5] = [StageOp::Add, StageOp::Remove, StageOp::Reorder, StageOp::List, StageOp::SetActive];

    /// The command name, without major.
    pub fn command(self) -> &'static str {
        match self {
            StageOp::Add => ADD_STAGE,
            StageOp::Remove => REMOVE_STAGE,
            StageOp::Reorder => REORDER_STAGES,
            StageOp::List => LIST_STAGES,
            StageOp::SetActive => SET_ACTIVE_STAGE,
        }
    }
}

fn event(e: TreeEvent) -> Result<Event, BusError> {
    Ok(Event::new(EventType::new(&e.event_type)?, e.body))
}

/// Runs one stage command as its handler does: the `fid_project` logic on the bus view,
/// refusals as bus errors, success as the events (one for a W2 command, none for the list).
pub fn run_stage(
    op: StageOp,
    view: &dyn StateView,
    params: &CValue,
    namespace: Uuid,
    source: &mut dyn ObservationSource,
) -> Result<Vec<Event>, BusError> {
    let v = BusView(view);
    let e = match op {
        StageOp::Add => add_stage(&v, params, namespace, source),
        StageOp::Remove => remove_stage(&v, params, namespace),
        StageOp::Reorder => reorder_stages(&v, params, namespace),
        StageOp::SetActive => set_active_stage(&v, params, namespace),
        StageOp::List => return list_stages(&v, params, namespace).map(|_| Vec::new()).map_err(|e| to_bus_error(&e)),
    }
    .map_err(|e| to_bus_error(&e))?;
    Ok(vec![event(e)?])
}

/// The result of `project.list_stages@1` as its `result_schema` declares it.
pub fn list_result(view: &dyn StateView, params: &CValue, namespace: Uuid) -> Result<CValue, BusError> {
    list_stages(&BusView(view), params, namespace).map_err(|e| to_bus_error(&e))
}

/// The result of a W2 stage command from its event: `{"stage": <id>}` for add and remove,
/// `{"order": [<id>, ...]}` for reorder. `internal/invariant_violated` for any other event.
pub fn result_of(e: &Event) -> Result<CValue, BusError> {
    let bad = || BusError::Internal(InternalReason::InvariantViolated, Detail::new("not a stage command event"));
    let CValue::Obj(b) = e.body() else {
        return Err(bad());
    };
    let (key, field) = match e.type_id().as_str() {
        STAGE_ADDED => ("stage", "id"),
        STAGE_REMOVED => ("stage", "stage"),
        STAGES_REORDERED => ("order", "order"),
        ACTIVE_STAGE_SET => ("stage", "stage"),
        _ => return Err(bad()),
    };
    let v = b.get(field).cloned().ok_or_else(bad)?;
    Ok(obj([(key, v)]))
}

// ------------------------------------------------------------------ handlers

/// The bus handler of one stage command.
pub struct StageHandler {
    op: StageOp,
    namespace: Uuid,
    source: SharedSource,
}

impl StageHandler {
    /// `namespace` is the project namespace (stage list ID, ID service).
    pub fn new(op: StageOp, namespace: Uuid, source: SharedSource) -> StageHandler {
        StageHandler { op, namespace, source }
    }

    pub fn op(&self) -> StageOp {
        self.op
    }
}

impl fmt::Debug for StageHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StageHandler").field("op", &self.op).finish_non_exhaustive()
    }
}

impl Handler for StageHandler {
    fn run(&self, view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        if ctx.cancel().is_cancelled() {
            return Err(HandlerError::Cancelled);
        }
        let mut source =
            self.source.lock().map_err(|_| HandlerError::Failed(Detail::new("observation source lock poisoned")))?;
        Ok(run_stage(self.op, view, params.value(), self.namespace, &mut *source)?)
    }
}

/// Registers the five stage handlers, each with its definition from [`command_defs`].
pub fn register(set: &mut HandlerSet, namespace: Uuid, source: &SharedSource) -> Result<(), BusError> {
    let defs = command_defs().map_err(|e| {
        BusError::Internal(InternalReason::InvariantViolated, Detail::new(&format!("stage definitions: {e}")))
    })?;
    for (op, def) in StageOp::ALL.into_iter().zip(defs) {
        set.register(def, Box::new(StageHandler::new(op, namespace, Arc::clone(source))))?;
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

fn object(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

const TEXT_RULE: &str = "untrusted text, 1 to 256 characters, no control or bidi control, NFC (else validation/label)";

fn stage_item() -> CValue {
    object(
        &["id", "label", "metadata", "order", "removed", "run", "serial"],
        obj([
            ("id", id_schema("The stage.")),
            (
                "label",
                obj([("type", text("string")), ("x-untrusted", CValue::Bool(true)), ("description", text("Label."))]),
            ),
            ("metadata", obj([("type", text("object")), ("description", text("Metadata object."))])),
            (
                "order",
                obj([
                    ("type", text("integer")),
                    ("minimum", CValue::Num(0.0)),
                    ("description", text("Position in the project's stage sequence (removed stages count).")),
                ]),
            ),
            ("removed", obj([("type", text("boolean")), ("description", text("Whether the stage was removed."))])),
            ("run", id_schema("The Run of the stage.")),
            (
                "serial",
                obj([
                    ("type", strs(&["string", "null"])),
                    ("x-untrusted", CValue::Bool(true)),
                    ("description", text("Serial, or null.")),
                ]),
            ),
        ]),
    )
}

/// One command definition's varying fields.
struct Spec {
    op: StageOp,
    title: &'static str,
    description: &'static str,
    params: CValue,
    result: CValue,
    class: &'static str,
    effects: &'static [&'static str],
}

fn spec(
    op: StageOp,
    title: &'static str,
    description: &'static str,
    params: CValue,
    result: CValue,
    class: &'static str,
    effects: &'static [&'static str],
) -> Spec {
    Spec { op, title, description, params, result, class, effects }
}

/// The five command definitions, in [`StageOp::ALL`] order. Built through
/// [`CommandDef::from_value`], so every field rule of the registry applies.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let untrusted =
        |d: String| obj([("type", text("string")), ("x-untrusted", CValue::Bool(true)), ("description", text(&d))]);
    let order = obj([
        ("type", text("array")),
        ("items", id_schema("A live stage of the run.")),
        (
            "description",
            text("The run's live stages in their new order: exactly a permutation (else validation/bad_stage)."),
        ),
    ]);
    let specs: [Spec; 5] = [
        spec(
            StageOp::Add,
            "Add stage",
            "Adds a stage (a measured instance against the shared plan) to a Run; every element gets an empty \
             not_computed slot for it in the same commit.",
            object(
                &["label", "run"],
                obj([
                    ("id", id_schema("Imported ID, kept unchanged; omit it to get a new UUIDv7.")),
                    ("label", untrusted(format!("Label; {TEXT_RULE}. Need not be unique."))),
                    ("run", id_schema("The Run (else validation/bad_reference).")),
                    ("serial", untrusted(format!("Optional serial; {TEXT_RULE}."))),
                ]),
            ),
            object(&["stage"], obj([("stage", id_schema("The new stage's ID."))])),
            "W2",
            &[STAGE_ADDED],
        ),
        spec(
            StageOp::Remove,
            "Remove stage",
            "Marks a stage removed (soft): its data stays and undo restores it. Fails with \
             precondition/stage_active for the active stage and not_found/unknown_entity for a stage that is not live.",
            object(&["stage"], obj([("stage", id_schema("The stage to remove."))])),
            object(&["stage"], obj([("stage", id_schema("The removed stage."))])),
            "W2",
            &[STAGE_REMOVED],
        ),
        spec(
            StageOp::Reorder,
            "Reorder stages",
            "Sets the order of a Run's live stages; the list must be exactly a permutation of them.",
            object(&["order", "run"], obj([("order", order.clone()), ("run", id_schema("The Run."))])),
            object(&["order"], obj([("order", order)])),
            "W2",
            &[STAGES_REORDERED],
        ),
        spec(
            StageOp::List,
            "List stages",
            "Lists the project's stages in order, optionally of one Run; removed stages only with include_removed.",
            object(
                &[],
                obj([
                    (
                        "include_removed",
                        obj([("type", text("boolean")), ("description", text("Also list removed stages."))]),
                    ),
                    ("run", id_schema("Only the stages of this Run.")),
                ]),
            ),
            object(&["stages"], obj([("stages", obj([("type", text("array")), ("items", stage_item())]))])),
            "R0",
            &[],
        ),
        spec(
            StageOp::SetActive,
            "Set active stage",
            "Makes a live stage the active stage; only the active-stage pointer changes, no data moves. Fails with              validation/bad_stage for a removed or unknown stage.",
            object(&["stage"], obj([("stage", id_schema("The stage to activate."))])),
            object(&["stage"], obj([("stage", id_schema("The active stage."))])),
            "W2",
            &[ACTIVE_STAGE_SET],
        ),
    ];
    specs
        .into_iter()
        .map(|Spec { op, title, description, params, result, class, effects }| {
            let r0 = class == "R0";
            CommandDef::from_value(obj([
                ("id", text(op.command())),
                ("major", CValue::Num(1.0)),
                ("title", text(title)),
                ("description", text(description)),
                ("params_schema", params),
                ("result_schema", result),
                ("side_effect", text(class)),
                ("mcp_projection", text("exposed")),
                ("determinism", text("D0")),
                ("idempotency", text(if r0 { "natural" } else { "none" })),
                ("fs_effect", text("none")),
                ("undo", text(if r0 { "not_undoable" } else { "head_move" })),
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
