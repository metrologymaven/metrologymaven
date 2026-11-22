//! Read-only element tree list and describe commands on the bus.
//! Spec node `CORE.PRJ.TREE.QUERY` (bus and registry half; pure query logic lives in
//! `fid_project::query`, layer model).
//!
//! - [`command_defs`]: `project.list_elements@1` and `project.describe_element@1`, both R0,
//!   D0, undo `none`, zero effects, on CLI (`fid project list-elements | describe-element`)
//!   and MCP (`project_list_elements | project_describe_element`).
//! - [`QueryHandler`] and [`register`]: bus handlers validating inputs against the read-only
//!   [`StateView`] and emitting zero events (never touching the Record Journal).

use std::fmt;

use fid_bus::error::{BusError, Detail, InternalReason};
use fid_bus::handler::{Event, Handler, HandlerCtx, HandlerError, HandlerSet, StateView, ValidParams};
use fid_canon::CValue;
use fid_project::container::ID_PATTERN;
use fid_project::{
    describe_element, describe_element_tree, list_elements, list_elements_tree, Tree, Uuid, DESCRIBE_ELEMENT,
    LIST_ELEMENTS, MAX_PAGE_SIZE,
};
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;

use crate::tree::{to_bus_error, TreeState};
use crate::BusView;

const LIFECYCLE_SINCE: &str = "2026-11-22";
const DIGEST_PATTERN: &str = "^sha256:[0-9a-f]{64}$";

/// The two element-tree read commands (`CORE.PRJ.TREE.QUERY`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QueryOp {
    ListElements,
    DescribeElement,
}

impl QueryOp {
    /// Every command in [`command_defs`] order.
    pub const ALL: [QueryOp; 2] = [QueryOp::ListElements, QueryOp::DescribeElement];

    /// Command name without major.
    pub fn command(self) -> &'static str {
        match self {
            QueryOp::ListElements => LIST_ELEMENTS,
            QueryOp::DescribeElement => DESCRIBE_ELEMENT,
        }
    }
}

fn head_digest(view: &dyn StateView) -> Digest {
    Digest::from_bytes(view.head().0)
}

/// Evaluates `project.list_elements@1` through the bus [`StateView`] and returns the result JSON.
pub fn list_result(view: &dyn StateView, params: &CValue, namespace: Uuid) -> Result<CValue, BusError> {
    list_elements(&BusView(view), head_digest(view), namespace, params).map_err(|e| to_bus_error(&e))
}

/// Evaluates `project.describe_element@1` through the bus [`StateView`] and returns the result JSON.
pub fn describe_result(view: &dyn StateView, params: &CValue) -> Result<CValue, BusError> {
    describe_element(&BusView(view), params).map_err(|e| to_bus_error(&e))
}

/// Fast path evaluating `project.list_elements@1` directly on a [`TreeState`]'s [`Tree`].
pub fn list_result_tree(state: &TreeState, params: &CValue) -> Result<CValue, BusError> {
    list_elements_tree(state.tree(), head_digest(state), params).map_err(|e| to_bus_error(&e))
}

/// Fast path evaluating `project.describe_element@1` directly on a [`Tree`].
pub fn describe_result_tree(tree: &Tree, params: &CValue) -> Result<CValue, BusError> {
    describe_element_tree(tree, params).map_err(|e| to_bus_error(&e))
}

/// Runs one query command as its handler does, returning zero events on success (R0).
pub fn run_query(op: QueryOp, view: &dyn StateView, params: &CValue, namespace: Uuid) -> Result<Vec<Event>, BusError> {
    let _ = match op {
        QueryOp::ListElements => list_result(view, params, namespace)?,
        QueryOp::DescribeElement => describe_result(view, params)?,
    };
    Ok(Vec::new())
}

// ------------------------------------------------------------------ handlers

/// Bus handler for one `CORE.PRJ.TREE.QUERY` read command (`R0`).
pub struct QueryHandler {
    op: QueryOp,
    namespace: Uuid,
}

impl QueryHandler {
    pub fn new(op: QueryOp, namespace: Uuid) -> Self {
        QueryHandler { op, namespace }
    }

    pub fn op(&self) -> QueryOp {
        self.op
    }
}

impl fmt::Debug for QueryHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueryHandler").field("op", &self.op).field("namespace", &self.namespace).finish()
    }
}

impl Handler for QueryHandler {
    fn run(&self, view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        if ctx.cancel().is_cancelled() {
            return Err(HandlerError::Cancelled);
        }
        Ok(run_query(self.op, view, params.value(), self.namespace)?)
    }
}

/// Registers both query handlers (`project.list_elements@1`, `project.describe_element@1`).
pub fn register(set: &mut HandlerSet, namespace: Uuid) -> Result<(), BusError> {
    let defs = command_defs().map_err(|e| {
        BusError::Internal(InternalReason::InvariantViolated, Detail::new(&format!("query definitions: {e}")))
    })?;
    for (op, def) in QueryOp::ALL.into_iter().zip(defs) {
        set.register(def, Box::new(QueryHandler::new(op, namespace)))?;
    }
    Ok(())
}

// ------------------------------------------------------------------ definitions

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn object(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

fn id_schema(description: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text(ID_PATTERN)), ("description", text(description))])
}

fn untrusted_text_schema(description: &str) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["text", "untrusted"])),
        ("description", text(description)),
        (
            "properties",
            obj([
                ("text", obj([("type", text("string")), ("x-untrusted", CValue::Bool(true))])),
                ("untrusted", obj([("type", text("boolean")), ("const", CValue::Bool(true))])),
            ]),
        ),
    ])
}

fn state_enum_schema(description: &str) -> CValue {
    obj([
        ("type", text("string")),
        ("enum", strs(&["not_computed", "current", "outdated", "failed"])),
        ("description", text(description)),
    ])
}

/// The `result_schema` of `project.list_elements@1` (matches `schema/element-list.schema.json`).
pub fn element_list_schema() -> CValue {
    let summary = object(
        &["id", "name", "parent", "stage", "state", "type", "visible"],
        obj([
            ("id", id_schema("Element ID.")),
            ("name", untrusted_text_schema("Element display name (untrusted text).")),
            ("parent", id_schema("Parent group ID.")),
            ("stage", id_schema("Stage ID in which `state` was evaluated.")),
            ("state", state_enum_schema("Element slot state in `stage`.")),
            ("type", obj([("type", text("string")), ("description", text("Registered element type ID."))])),
            ("visible", obj([("type", text("boolean")), ("description", text("Saved element visibility."))])),
        ]),
    );
    object(
        &["elements", "next"],
        obj([
            (
                "elements",
                obj([
                    ("type", text("array")),
                    ("maxItems", CValue::Num(MAX_PAGE_SIZE as f64)),
                    ("items", summary),
                    ("description", text("Matching elements in tree order.")),
                ]),
            ),
            (
                "next",
                obj([
                    ("type", strs(&["string", "null"])),
                    ("description", text("Head-bound continuation token, or null on the last page.")),
                ]),
            ),
        ]),
    )
}

/// The `result_schema` of `project.describe_element@1`.
pub fn element_detail_schema() -> CValue {
    let slot = object(
        &["digest", "state"],
        obj([
            (
                "digest",
                obj([
                    ("type", strs(&["string", "null"])),
                    ("pattern", text(DIGEST_PATTERN)),
                    ("description", text("Canonical value digest, or null.")),
                ]),
            ),
            ("state", state_enum_schema("Slot state in this stage.")),
        ]),
    );
    object(
        &["group_path", "id", "metadata", "name", "parent", "slots", "tags", "type", "visible"],
        obj([
            (
                "group_path",
                obj([
                    ("type", text("array")),
                    ("items", untrusted_text_schema("Ancestor group name (untrusted text).")),
                    ("description", text("Ancestor group names from root's child down to parent.")),
                ]),
            ),
            ("id", id_schema("Element ID.")),
            (
                "metadata",
                object(
                    &["untrusted", "value"],
                    obj([
                        ("untrusted", obj([("type", text("boolean")), ("const", CValue::Bool(true))])),
                        (
                            "value",
                            obj([
                                ("type", text("object")),
                                ("description", text("Element metadata object (untrusted).")),
                            ]),
                        ),
                    ]),
                ),
            ),
            ("name", untrusted_text_schema("Element display name (untrusted text).")),
            ("parent", id_schema("Parent group ID.")),
            (
                "slots",
                obj([
                    ("type", text("object")),
                    ("additionalProperties", slot),
                    ("description", text("Per-stage slots keyed by stage UUID.")),
                ]),
            ),
            (
                "tags",
                obj([
                    ("type", text("array")),
                    ("items", untrusted_text_schema("Element tag (untrusted text).")),
                    ("description", text("Element tags (untrusted text).")),
                ]),
            ),
            ("type", obj([("type", text("string")), ("description", text("Registered element type ID."))])),
            ("visible", obj([("type", text("boolean")), ("description", text("Saved element visibility."))])),
        ]),
    )
}

/// Registry definitions for `project.list_elements@1` and `project.describe_element@1` (both R0, D0).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let list_params = object(
        &[],
        obj([
            ("group", id_schema("Optional group ID to filter by.")),
            (
                "page_size",
                obj([
                    ("type", text("integer")),
                    ("minimum", CValue::Num(1.0)),
                    ("maximum", CValue::Num(MAX_PAGE_SIZE as f64)),
                    ("description", text("Maximum elements to return (1..=5000, default 5000).")),
                ]),
            ),
            (
                "page_token",
                obj([
                    ("type", text("string")),
                    ("description", text("Continuation token returned by a previous page at the same head.")),
                ]),
            ),
            (
                "recursive",
                obj([
                    ("type", text("boolean")),
                    ("description", text("When true with `group`, include descendant groups recursively.")),
                ]),
            ),
            ("stage", id_schema("Optional stage ID; defaults to the active stage.")),
            ("state", state_enum_schema("Optional element slot state filter.")),
            (
                "type",
                obj([("type", text("string")), ("description", text("Optional registered element type filter."))]),
            ),
        ]),
    );

    let describe_params = object(&["id"], obj([("id", id_schema("Element ID to describe."))]));

    let specs: [(QueryOp, &str, &str, CValue, CValue); 2] = [
        (
            QueryOp::ListElements,
            "List elements",
            "Lists elements in tree order filtered by type, state, group (direct or recursive) and stage, paged up to 5,000 per call.",
            list_params,
            element_list_schema(),
        ),
        (
            QueryOp::DescribeElement,
            "Describe element",
            "Describes one element in full (type, parent, group path, name, per-stage slots, visibility, tags and metadata) with untrusted user-text markers.",
            describe_params,
            element_detail_schema(),
        ),
    ];

    specs
        .into_iter()
        .map(|(op, title, description, params, result)| {
            CommandDef::from_value(obj([
                ("id", text(op.command())),
                ("major", CValue::Num(1.0)),
                ("title", text(title)),
                ("description", text(description)),
                ("params_schema", params),
                ("result_schema", result),
                ("side_effect", text("R0")),
                ("mcp_projection", text("exposed")),
                ("determinism", text("D0")),
                ("idempotency", text("none")),
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
            ]))
        })
        .collect()
}
