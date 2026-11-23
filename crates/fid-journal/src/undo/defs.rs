//! Store reader and registry definitions of `journal.undo@1`, `journal.redo@1` and
//! `journal.show_undo_tree@1` (feature `store`, `CORE.JRN.UNDO.MOVE`).

use super::{
    undo_rows, UndoError, UndoTree, BRANCH_NAME_PATTERN, HEAD_MOVED, MAX_STEPS, MAX_TREE_DEPTH, MAX_TREE_NODES,
    MIN_STEPS, REDO, SHOW_UNDO_TREE, UNDO,
};
use crate::append::Journal;
use crate::entry::DIGEST_PATTERN;
use crate::verify::store::{check_stream, storage, stored, to_i64, ROWS_SQL};
use crate::verify::{MAX_HEADER_BYTES, MAX_SCAN};
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};
use fid_store::Store;

const LIFECYCLE_SINCE: &str = "2026-11-23";

/// Reads and verifies every entry of `stream` from `store` and builds its [`UndoTree`].
/// Read-only: opens no write transaction and writes nothing to the store.
pub fn undo_store<F>(store: &Store, stream: &str, is_undoable: F) -> Result<UndoTree, UndoError>
where
    F: Fn(&str, u32) -> bool,
{
    check_stream(stream, None)?;
    let max_entries = to_i64(u64::try_from(MAX_SCAN).unwrap_or(u64::MAX).saturating_add(1));
    let header_cap = to_i64(u64::try_from(MAX_HEADER_BYTES).unwrap_or(1 << 20));
    store
        .read(|conn| {
            let mut st = conn.prepare(ROWS_SQL)?;
            let rows = st.query_map([i64::MIN, header_cap, max_entries], |r| {
                Ok(stored((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))
            })?;
            let rows = rows.map(|r| r.map_err(|e| storage("read entries", e)));
            Ok(undo_rows(stream, rows, is_undoable))
        })
        .map_err(|e| UndoError::from(storage("undo_store", e)))?
}

impl Journal {
    /// Projects this journal's verified entries into an [`UndoTree`] (`CORE.JRN.UNDO.MOVE`).
    pub fn undo_tree<F>(&self, is_undoable: F) -> Result<UndoTree, UndoError>
    where
        F: Fn(&str, u32) -> bool,
    {
        self.check_poison().map_err(|e| UndoError::Storage(e.to_string()))?;
        undo_store(&self.store, &self.stream, is_undoable)
    }
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

fn digest(description: &str) -> CValue {
    obj([
        ("type", text("string")),
        ("pattern", text(DIGEST_PATTERN)),
        ("maxLength", CValue::Num(71.0)),
        ("description", text(description)),
    ])
}

fn branch_prop() -> CValue {
    obj([
        ("type", text("string")),
        ("pattern", text(BRANCH_NAME_PATTERN)),
        ("maxLength", CValue::Num(100.0)),
        ("description", text("Target branch name (defaults to the session's active branch or main).")),
    ])
}

fn steps_prop() -> CValue {
    obj([
        ("type", text("integer")),
        ("minimum", CValue::Num(f64::from(MIN_STEPS))),
        ("maximum", CValue::Num(f64::from(MAX_STEPS))),
        ("description", text("Number of logical steps to move (1..100, default 1).")),
    ])
}

fn move_result() -> CValue {
    object(
        &["from", "to"],
        obj([
            ("from", digest("Logical head before the head move.")),
            ("to", digest("Logical head after the head move.")),
        ]),
    )
}

fn tree_node_schema() -> CValue {
    object(
        &["children", "command", "parent", "state", "visited_at"],
        obj([
            ("state", digest("Verified command entry hash.")),
            (
                "parent",
                obj([
                    ("anyOf", CValue::Arr(vec![digest("Logical parent state."), obj([("type", text("null"))])])),
                    ("description", text("Logical parent state hash, or null for the root state.")),
                ]),
            ),
            (
                "children",
                obj([
                    ("type", text("array")),
                    ("maxItems", CValue::Num(MAX_TREE_NODES as f64)),
                    ("items", digest("Child state hash.")),
                    ("description", text("Direct child states in commit order.")),
                ]),
            ),
            (
                "visited_at",
                obj([
                    ("type", text("string")),
                    ("pattern", text("^(0|[1-9][0-9]{0,19})$")),
                    ("maxLength", CValue::Num(20.0)),
                    ("description", text("Journal sequence when this state was most recently visited.")),
                ]),
            ),
            (
                "command",
                obj([
                    ("type", text("string")),
                    ("maxLength", CValue::Num(128.0)),
                    ("description", text("Command reference id@major that produced this state.")),
                ]),
            ),
        ]),
    )
}

/// Registry definitions of `journal.undo@1` (W2), `journal.redo@1` (W2) and
/// `journal.show_undo_tree@1` (R0) (`CORE.JRN.UNDO.MOVE`, `REQ-007`).
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let undo_def = CommandDef::from_value(obj([
        ("id", text(UNDO)),
        ("major", CValue::Num(1.0)),
        ("title", text("Undo")),
        (
            "description",
            text(
                "Moves the branch head backwards by 1..100 logical steps by appending a head_move entry \
                 without deleting history; refuses all-or-nothing if any W3 or not_undoable barrier is crossed.",
            ),
        ),
        ("params_schema", object(&[], obj([("branch", branch_prop()), ("steps", steps_prop())]))),
        ("result_schema", move_result()),
        ("side_effect", text("W2")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[HEAD_MOVED])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))?;

    let redo_def = CommandDef::from_value(obj([
        ("id", text(REDO)),
        ("major", CValue::Num(1.0)),
        ("title", text("Redo")),
        (
            "description",
            text(
                "Moves the branch head forwards by 1..100 steps to a named child or the most recently \
                 visited child by appending a head_move entry without deleting history.",
            ),
        ),
        (
            "params_schema",
            object(
                &[],
                obj([
                    ("branch", branch_prop()),
                    ("child", digest("Optional direct child state hash to move to.")),
                    ("steps", steps_prop()),
                ]),
            ),
        ),
        ("result_schema", move_result()),
        ("side_effect", text("W2")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[HEAD_MOVED])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))?;

    let tree_result = object(
        &["nodes"],
        obj([(
            "nodes",
            obj([
                ("type", text("array")),
                ("maxItems", CValue::Num(MAX_TREE_NODES as f64)),
                ("items", tree_node_schema()),
                ("description", text("Undo tree nodes reachable from the requested root, up to 1,000 nodes.")),
            ]),
        )]),
    );

    let show_def = CommandDef::from_value(obj([
        ("id", text(SHOW_UNDO_TREE)),
        ("major", CValue::Num(1.0)),
        ("title", text("Show undo tree")),
        (
            "description",
            text(
                "Returns the undo tree from a given state (or branch root) up to depth 1,000 and at most 1,000 nodes.",
            ),
        ),
        (
            "params_schema",
            object(
                &[],
                obj([
                    ("branch", branch_prop()),
                    ("from", digest("Optional starting state hash.")),
                    (
                        "depth",
                        obj([
                            ("type", text("integer")),
                            ("minimum", CValue::Num(1.0)),
                            ("maximum", CValue::Num(f64::from(MAX_TREE_DEPTH))),
                            ("description", text("Maximum tree depth to return (1..1,000, default 1,000).")),
                        ]),
                    ),
                ]),
            ),
        ),
        ("result_schema", tree_result),
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

    Ok(vec![undo_def, redo_def, show_def])
}
