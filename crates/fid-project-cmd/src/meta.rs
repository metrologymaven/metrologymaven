//! Tags and metadata commands on the bus (`CORE.PRJ.META`, bus and registry half; the pure
//! model, untrusted wrappers, validation and inverted index live in `fid_project::meta`).
//!
//! Declares and registers:
//! - `project.set_tags@1` (W2, D0, undo `head_move`, effect `TagsSet`)
//! - `project.set_metadata@1` (W2, D0, undo `head_move`, effect `MetadataSet`)
//! - `project.find_elements_by_tag@1` (R0, D0, undo `not_undoable`)

use fid_bus::error::{BusError, Detail, InternalReason, NotFoundReason, ValidationReason};
use fid_bus::handler::{
    EntityId, Event, EventType, Handler, HandlerCtx, HandlerError, HandlerSet, StateView, ValidParams,
};
use fid_canon::CValue;
use fid_project::container::ID_PATTERN;
use fid_project::meta::{
    plan_set_metadata, plan_set_tags, run_find_elements_by_tag, MetaError, MetaStore, FIND_ELEMENTS_BY_TAG,
    MAX_TAGS_PER_TARGET, MAX_TAG_CHARS, MAX_VALUE_BYTES, METADATA_SET, META_KEY_PATTERN, SET_METADATA, SET_TAGS,
    TAGS_SET,
};
use fid_project::Uuid;
use fid_registry::{CommandDef, DefError};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

const LIFECYCLE_SINCE: &str = "2026-11-23";

/// Read source supplying the projected [`MetaStore`] to the bus handlers.
pub trait MetaSource: Send + Sync {
    fn store(&self) -> Result<MetaStore, String>;
    /// Returns `true` if `id` exists as an element in the project.
    fn element_exists(&self, view: &dyn StateView, id: &Uuid) -> bool {
        EntityId::new(&id.to_string()).ok().and_then(|k| view.entity(&k)).is_some()
    }
}

/// Maps every [`MetaError`] to the typed bus error (catalogue v3).
pub fn meta_bus_error(e: &MetaError) -> BusError {
    let d = Detail::new(e.detail());
    match e {
        MetaError::Validation(_) => BusError::Validation(ValidationReason::Schema, d),
        MetaError::NotFound(_) => BusError::NotFound(NotFoundReason::UnknownEntity, d),
        MetaError::Invariant(_) => BusError::Internal(InternalReason::InvariantViolated, d),
    }
}

/// The three `CORE.PRJ.META` commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaOp {
    SetTags,
    SetMetadata,
    FindElementsByTag,
}

impl MetaOp {
    pub const ALL: [MetaOp; 3] = [MetaOp::SetTags, MetaOp::SetMetadata, MetaOp::FindElementsByTag];

    pub const fn command(self) -> &'static str {
        match self {
            MetaOp::SetTags => SET_TAGS,
            MetaOp::SetMetadata => SET_METADATA,
            MetaOp::FindElementsByTag => FIND_ELEMENTS_BY_TAG,
        }
    }
}

fn invariant(msg: &str) -> BusError {
    BusError::Internal(InternalReason::InvariantViolated, Detail::new(msg))
}

/// Runs one `CORE.PRJ.META` command against `view` and `source`, returning the events to commit
/// (one event for W2 commands, empty for R0 `find_elements_by_tag`) and the command result value.
pub fn run_meta(
    op: MetaOp,
    view: &dyn StateView,
    source: &dyn MetaSource,
    params: &CValue,
) -> Result<(Vec<Event>, CValue), BusError> {
    let store = source.store().map_err(|e| invariant(&format!("meta store: {e}")))?;
    match op {
        MetaOp::SetTags => {
            let (ev, res) =
                plan_set_tags(|id| source.element_exists(view, id), params).map_err(|e| meta_bus_error(&e))?;
            let bus_ev = Event::new(EventType::new(ev.type_id())?, ev.body());
            Ok((vec![bus_ev], res))
        }
        MetaOp::SetMetadata => {
            let (ev, res) = plan_set_metadata(|id| source.element_exists(view, id), &store, params)
                .map_err(|e| meta_bus_error(&e))?;
            let bus_ev = Event::new(EventType::new(ev.type_id())?, ev.body());
            Ok((vec![bus_ev], res))
        }
        MetaOp::FindElementsByTag => {
            let res = run_find_elements_by_tag(&store, params).map_err(|e| meta_bus_error(&e))?;
            Ok((Vec::new(), res))
        }
    }
}

/// Bus handler for `project.set_tags@1`, `project.set_metadata@1`, or `project.find_elements_by_tag@1`.
pub struct MetaHandler {
    op: MetaOp,
    source: Arc<dyn MetaSource>,
}

impl MetaHandler {
    pub fn new(op: MetaOp, source: Arc<dyn MetaSource>) -> Self {
        Self { op, source }
    }
}

impl fmt::Debug for MetaHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MetaHandler").field("op", &self.op).finish_non_exhaustive()
    }
}

impl Handler for MetaHandler {
    fn run(&self, view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        if ctx.cancel().is_cancelled() {
            return Err(HandlerError::Cancelled);
        }
        Ok(run_meta(self.op, view, self.source.as_ref(), params.value())?.0)
    }
}

/// Registers `project.set_tags@1`, `project.set_metadata@1` and `project.find_elements_by_tag@1`.
pub fn register_meta(set: &mut HandlerSet, source: &Arc<dyn MetaSource>) -> Result<(), BusError> {
    let defs = command_defs().map_err(|e| invariant(&format!("meta definitions: {e}")))?;
    if defs.len() != MetaOp::ALL.len() {
        return Err(invariant("unexpected number of meta definitions"));
    }
    for (def, op) in defs.into_iter().zip(MetaOp::ALL) {
        if def.id != op.command() {
            return Err(invariant("meta definitions out of order"));
        }
        set.register(def, Box::new(MetaHandler::new(op, Arc::clone(source))))?;
    }
    Ok(())
}

// ------------------------------------------------------------------ registry definitions

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn object(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

fn target_schema() -> CValue {
    obj([
        ("type", text("string")),
        ("maxLength", CValue::Num(36.0)),
        ("description", text("Target element UUID or literal \"project\".")),
    ])
}

fn untrusted_string_schema(description: &str) -> CValue {
    object(
        &["text", "untrusted"],
        obj([
            ("untrusted", obj([("type", text("boolean")), ("description", text("Always true (C6 untrusted text)."))])),
            (
                "text",
                obj([
                    ("type", text("string")),
                    ("maxLength", CValue::Num(MAX_VALUE_BYTES as f64)),
                    ("x-untrusted", CValue::Bool(true)),
                    ("description", text(description)),
                ]),
            ),
        ]),
    )
}

/// Registry definitions for `project.set_tags@1`, `project.set_metadata@1`, and
/// `project.find_elements_by_tag@1` (`CORE.PRJ.META`).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let set_tags_def = CommandDef::from_value(obj([
        ("id", text(SET_TAGS)),
        ("major", CValue::Num(1.0)),
        ("title", text("Set tags")),
        (
            "description",
            text("Replaces the tag set of an element or the project; each tag is NFC-normalised, 1..64 chars, at most 64 tags, stored untrusted."),
        ),
        (
            "params_schema",
            object(
                &["tags", "target"],
                obj([
                    ("target", target_schema()),
                    (
                        "tags",
                        obj([
                            ("type", text("array")),
                            ("maxItems", CValue::Num(MAX_TAGS_PER_TARGET as f64)),
                            (
                                "items",
                                obj([
                                    ("type", text("string")),
                                    ("minLength", CValue::Num(1.0)),
                                    ("maxLength", CValue::Num(MAX_TAG_CHARS as f64)),
                                    ("x-untrusted", CValue::Bool(true)),
                                    ("description", text("Untrusted tag string (NFC, 1..64 chars).")),
                                ]),
                            ),
                            ("description", text("Replacement tag set.")),
                        ]),
                    ),
                ]),
            ),
        ),
        (
            "result_schema",
            object(
                &["tags", "target"],
                obj([
                    ("target", target_schema()),
                    (
                        "tags",
                        obj([
                            ("type", text("array")),
                            ("maxItems", CValue::Num(MAX_TAGS_PER_TARGET as f64)),
                            ("items", untrusted_string_schema("Untrusted NFC tag.")),
                            ("description", text("Sorted untrusted tags on target.")),
                        ]),
                    ),
                ]),
            ),
        ),
        ("side_effect", text("W2")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[TAGS_SET])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))?;

    let meta_map_schema = obj([
        ("type", text("object")),
        ("maxProperties", CValue::Num(1024.0)),
        (
            "additionalProperties",
            obj([
                (
                    "anyOf",
                    CValue::Arr(vec![
                        obj([
                            ("type", text("string")),
                            ("maxLength", CValue::Num(MAX_VALUE_BYTES as f64)),
                            ("x-untrusted", CValue::Bool(true)),
                        ]),
                        obj([("type", text("null"))]),
                    ]),
                ),
                ("description", text("Untrusted string value (<= 4096 UTF-8 bytes) or null to unset.")),
            ]),
        ),
        ("description", text("Keys matching [a-z0-9_.-]{1,64} mapped to untrusted values or null.")),
    ]);

    let result_meta_map = obj([
        ("type", text("object")),
        ("maxProperties", CValue::Num(1024.0)),
        ("additionalProperties", untrusted_string_schema("Untrusted metadata value.")),
        ("description", text("Complete metadata map for target.")),
    ]);

    let set_meta_def = CommandDef::from_value(obj([
        ("id", text(SET_METADATA)),
        ("major", CValue::Num(1.0)),
        ("title", text("Set metadata")),
        (
            "description",
            text("Sets key-value untrusted metadata and removes unset/null keys on an element or the project in one commit."),
        ),
        (
            "params_schema",
            object(
                &["target"],
                obj([
                    ("target", target_schema()),
                    ("set", meta_map_schema),
                    (
                        "unset",
                        obj([
                            ("type", text("array")),
                            ("maxItems", CValue::Num(1024.0)),
                            (
                                "items",
                                obj([
                                    ("type", text("string")),
                                    ("pattern", text(META_KEY_PATTERN)),
                                    ("maxLength", CValue::Num(64.0)),
                                ]),
                            ),
                            ("description", text("Metadata keys to remove.")),
                        ]),
                    ),
                ]),
            ),
        ),
        (
            "result_schema",
            object(&["metadata", "target"], obj([("target", target_schema()), ("metadata", result_meta_map)])),
        ),
        ("side_effect", text("W2")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[METADATA_SET])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))?;

    let find_def = CommandDef::from_value(obj([
        ("id", text(FIND_ELEMENTS_BY_TAG)),
        ("major", CValue::Num(1.0)),
        ("title", text("Find elements by tag")),
        ("description", text("Returns all element IDs carrying the given NFC tag, sorted ascending by element ID.")),
        (
            "params_schema",
            object(
                &["tag"],
                obj([(
                    "tag",
                    obj([
                        ("type", text("string")),
                        ("minLength", CValue::Num(1.0)),
                        ("maxLength", CValue::Num(MAX_TAG_CHARS as f64)),
                        ("x-untrusted", CValue::Bool(true)),
                        ("description", text("Tag to search for (NFC normalised).")),
                    ]),
                )]),
            ),
        ),
        (
            "result_schema",
            object(
                &["elements"],
                obj([(
                    "elements",
                    obj([
                        ("type", text("array")),
                        ("maxItems", CValue::Num(1_000_000.0)),
                        (
                            "items",
                            obj([
                                ("type", text("string")),
                                ("pattern", text(ID_PATTERN)),
                                ("maxLength", CValue::Num(36.0)),
                            ]),
                        ),
                        ("description", text("Matching element UUIDs sorted ascending.")),
                    ]),
                )]),
            ),
        ),
        ("side_effect", text("R0")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", CValue::Arr(Vec::new())),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))?;

    let _ = BTreeMap::<String, CValue>::new();
    Ok(vec![set_tags_def, set_meta_def, find_def])
}
