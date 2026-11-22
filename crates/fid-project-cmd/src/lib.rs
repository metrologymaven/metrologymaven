//! Project commands on the bus. Spec node `CORE.PRJ.CONT.HIER` (the command half; the
//! container types and hierarchy rules are in `fid-project`, layer model).
//!
//! - [`command_defs`]: `project.create_part@1`, `project.create_revision@1`,
//!   `project.create_plan@1` and `project.create_run@1` as registry definitions: W2, D0,
//!   undo `head_move`, effect `ContainerCreated`, on the CLI (`fid project create-part`, ...)
//!   and MCP (`project_create_part`, ...) (REQ-005).
//! - [`CreateHandler`] and [`register_containers`]: the four container handlers. Each reads the immutable
//!   [`StateView`] through [`BusView`], runs [`fid_project::create`] with the call's principal
//!   and the shared [`ObservationSource`], and returns one `ContainerCreated` event
//!   ([`created_event`]); the bus commits it.
//! - [`to_bus_error`]: `schema`, `label`, `bad_reference` and `hierarchy` become
//!   `validation/<reason>`; `id_collision` becomes `conflict/id_collision` (bus catalogue v2);
//!   ID service faults become `internal/handler_failed`; a malformed container entity in the
//!   state becomes `internal/invariant_violated`.
//! - [`tree`]: the element tree on the bus (spec node `CORE.PRJ.TREE.MODEL`, bus half): the
//!   [`StateView`] over `fid_project::Tree` and the mapping of tree refusals to bus errors.
//! - [`stage`]: the five stage commands (spec nodes `CORE.PRJ.STAGE.MODEL` and
//!   `CORE.PRJ.STAGE.SWITCH`, bus half); [`stage_switch`]: the store adapters of the value reads.
//! - [`command_defs`] lists all nine commands of this crate (container, then stage) and
//!   [`register`] registers all nine handlers.
//!
//! [`container_defs`] and [`register_containers`] cover the four container commands only.

use fid_bus::error::{BusError, ConflictReason, Detail, InternalReason, ValidationReason};
use fid_bus::handler::{
    EntityId, Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet, StateView, ValidParams,
};
use fid_bus::principal::Principal;
use fid_canon::CValue;
use fid_project::container::ID_PATTERN;
use fid_project::{
    create, Container, ContainerError, ContainerKind, ContainerView, CreatedBy, ObservationSource, Uuid,
    CONTAINER_CREATED,
};
use fid_registry::{CommandDef, DefError};
use std::fmt;
use std::sync::{Arc, Mutex};

pub mod stage;
pub mod stage_switch;
pub mod tree;
pub mod tree_cmds;

const LIFECYCLE_SINCE: &str = "2026-11-19";

// ------------------------------------------------------------------ adapters

/// The bus state view as a [`ContainerView`]: entities keyed by the canonical text of their
/// UUID.
#[derive(Clone, Copy)]
pub struct BusView<'a>(pub &'a dyn StateView);

impl fmt::Debug for BusView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BusView")
    }
}

impl ContainerView for BusView<'_> {
    fn entity(&self, id: &Uuid) -> Option<(&str, &CValue)> {
        let key = EntityId::new(&id.to_string()).ok()?;
        self.0.entity(&key).map(|e| (e.kind, e.body))
    }
}

/// The `created_by` of a call: its principal's kind and identity.
pub fn created_by(principal: &Principal) -> Result<CreatedBy, ContainerError> {
    CreatedBy::new(principal.kind().as_str(), principal.id().as_str())
}

/// The bus error of a container refusal (see the crate docs).
pub fn to_bus_error(e: &ContainerError) -> BusError {
    let d = Detail::new(&e.to_string());
    match e {
        ContainerError::Schema(_) => BusError::Validation(ValidationReason::Schema, d),
        ContainerError::Label => BusError::Validation(ValidationReason::Label, d),
        ContainerError::BadReference(_) => BusError::Validation(ValidationReason::BadReference, d),
        ContainerError::Hierarchy(_) => BusError::Validation(ValidationReason::Hierarchy, d),
        ContainerError::IdCollision(_) => BusError::Conflict(ConflictReason::IdCollision, d),
        ContainerError::Id(_) => BusError::Internal(InternalReason::HandlerFailed, d),
        ContainerError::BadState(_) => BusError::Internal(InternalReason::InvariantViolated, d),
    }
}

/// The `ContainerCreated` event of a new container; its body is [`Container::to_value`].
pub fn created_event(c: &Container) -> Result<Event, BusError> {
    Ok(Event::new(EventType::new(CONTAINER_CREATED)?, c.to_value()))
}

/// The command result of a new container, as its `result_schema` declares it:
/// `{"part": <id>}`, `{"revision": <id>}`, `{"plan": <id>}` or `{"run": <id>}`.
pub fn result_of(c: &Container) -> CValue {
    obj([(c.kind().short(), text(&c.id().to_string()))])
}

/// Runs one create command as its handler does, with the principal already reduced to a
/// [`CreatedBy`]: the container rules on the bus view, refusals as bus errors, success as
/// the one event.
pub fn run_create(
    kind: ContainerKind,
    view: &dyn StateView,
    params: &CValue,
    by: &CreatedBy,
    namespace: Uuid,
    source: &mut dyn ObservationSource,
) -> Result<Event, BusError> {
    let c = create(kind, &BusView(view), params, by, namespace, source).map_err(|e| to_bus_error(&e))?;
    created_event(&c)
}

// ------------------------------------------------------------------ handlers

/// The observation source the container handlers share (the live recording source, or a
/// replay source). Locked for the length of one handler call.
pub type SharedSource = Arc<Mutex<dyn ObservationSource + Send>>;

/// The bus handler of one create command.
pub struct CreateHandler {
    kind: ContainerKind,
    namespace: Uuid,
    source: SharedSource,
}

impl CreateHandler {
    /// `namespace` is the project namespace UUID handed to the ID service.
    pub fn new(kind: ContainerKind, namespace: Uuid, source: SharedSource) -> CreateHandler {
        CreateHandler { kind, namespace, source }
    }

    pub fn kind(&self) -> ContainerKind {
        self.kind
    }
}

impl fmt::Debug for CreateHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateHandler").field("kind", &self.kind).finish_non_exhaustive()
    }
}

impl Handler for CreateHandler {
    fn run(&self, view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        if ctx.cancel().is_cancelled() {
            return Err(HandlerError::Cancelled);
        }
        let by = created_by(ctx.principal()).map_err(|e| HandlerError::Refused(to_bus_error(&e)))?;
        let mut source =
            self.source.lock().map_err(|_| HandlerError::Failed(Detail::new("observation source lock poisoned")))?;
        let event = run_create(self.kind, view, params.value(), &by, self.namespace, &mut *source)?;
        Ok(vec![event])
    }
}

/// Registers all thirteen handlers of this crate: the four container commands
/// ([`register_containers`]), the five stage commands ([`stage::register`]), and the four tree
/// group commands ([`tree_cmds::register`]), each with its definition from [`command_defs`]. A
/// handler already in `set` is `internal/invariant_violated`.
pub fn register(set: &mut HandlerSet, namespace: Uuid, source: &SharedSource) -> Result<(), BusError> {
    register_containers(set, namespace, source)?;
    stage::register(set, namespace, source)?;
    tree_cmds::register(set, namespace, source)
}

/// Registers the four container create handlers only, each with its definition from
/// [`container_defs`].
pub fn register_containers(set: &mut HandlerSet, namespace: Uuid, source: &SharedSource) -> Result<(), BusError> {
    let defs = container_defs().map_err(|e| {
        BusError::Internal(InternalReason::InvariantViolated, Detail::new(&format!("container definitions: {e}")))
    })?;
    for (kind, def) in ContainerKind::ALL.into_iter().zip(defs) {
        set.register(def, Box::new(CreateHandler::new(kind, namespace, Arc::clone(source))))?;
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

/// A label: a string only. The length and text rules (1 to [`fid_project::MAX_LABEL_CHARS`] characters,
/// no Cc or bidi control, NFC) are checked by the handler, so a caller gets the specific
/// `validation/label` reason instead of a generic `validation/schema`.
fn label_schema(description: &str) -> CValue {
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

const IMPORTED: &str = "Imported ID, kept unchanged; omit it to get a new UUIDv7.";

/// Every command definition of this crate: the four container commands in
/// [`ContainerKind::ALL`] order, the five stage commands ([`stage::command_defs`]), and the
/// four tree group commands ([`tree_cmds::command_defs`]).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let mut defs = container_defs()?;
    defs.extend(stage::command_defs()?);
    defs.extend(tree_cmds::command_defs()?);
    Ok(defs)
}

/// The four container command definitions, in [`ContainerKind::ALL`] order. Built through
/// [`CommandDef::from_value`], so every field rule of the registry applies.
pub fn container_defs() -> Result<Vec<CommandDef>, DefError> {
    let rule = "untrusted text, 1 to 256 characters, no control or bidi control, NFC (else validation/label).";
    let label_req = || label_schema(&format!("Label; {rule} Need not be unique."));
    let label_opt = || label_schema(&format!("Optional label; {rule} Need not be unique."));
    let specs: [(ContainerKind, &str, &str, CValue); 4] = [
        (
            ContainerKind::Part,
            "Create part",
            "Creates a Part, the top of the Part, Revision, Plan, Run hierarchy.",
            object(&["label"], obj([("id", id_schema(IMPORTED)), ("label", label_req())])),
        ),
        (
            ContainerKind::Revision,
            "Create revision",
            "Creates a Revision of a Part; fails with validation/bad_reference if the part is not a Part.",
            object(
                &["label", "part"],
                obj([("id", id_schema(IMPORTED)), ("label", label_req()), ("part", id_schema("The Part."))]),
            ),
        ),
        (
            ContainerKind::Plan,
            "Create plan",
            "Creates a Plan of a Revision; fails with validation/bad_reference if the revision is not a Revision.",
            object(
                &["revision"],
                obj([("id", id_schema(IMPORTED)), ("label", label_opt()), ("revision", id_schema("The Revision."))]),
            ),
        ),
        (
            ContainerKind::Run,
            "Create run",
            "Creates a Run of a Plan and that Plan's Revision; fails with validation/hierarchy if the Plan \
             belongs to another Revision.",
            object(
                &["plan", "revision"],
                obj([
                    ("id", id_schema(IMPORTED)),
                    ("label", label_opt()),
                    ("plan", id_schema("The Plan.")),
                    ("revision", id_schema("The Revision of the Plan.")),
                ]),
            ),
        ),
    ];
    specs
        .into_iter()
        .map(|(kind, title, description, params)| {
            let result = object(&[kind.short()], obj([(kind.short(), id_schema("The new entity's ID."))]));
            CommandDef::from_value(obj([
                ("id", text(kind.command())),
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
                ("effects", strs(&[CONTAINER_CREATED])),
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
