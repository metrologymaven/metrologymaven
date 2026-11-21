//! The diff over a project database and the registry definition of `journal.diff_branch@1`
//! (feature `store`).
//!
//! # Pinned rules
//!
//! - **Read only (REQ-005).** One [`fid_store::Store::read`] of `entries` in `seq` order (the
//!   same bounded, type-blind row read as `journal.verify_chain@1`, one header in memory at a
//!   time); no transaction commits and nothing is written. The read holds the store's
//!   connection mutex for the whole walk, so appends through the same open store wait.
//! - **Rows.** Every row is read, from the lowest stored sequence, so a row stored below
//!   sequence 1 fails as `sequence/reorder`. `entry_blobs`, blob files, `checkpoints` and
//!   `heads` are not read.

use super::{diff_rows, BranchDiff, DiffError, DiffOptions, DIFF_BRANCH};
use crate::append::Journal;
use crate::branch::{Branches, NAME_PATTERN};
use crate::entry::{DIGEST_PATTERN, SEQ_PATTERN};
use crate::verify::store::{check_stream, storage, stored, to_i64, ROWS_SQL};
use crate::verify::{ReadError, MAX_HEADER_BYTES};
use crate::PrincipalKind;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};
use fid_store::Store;

const LIFECYCLE_SINCE: &str = "2026-11-21";
/// Most deltas in one list, and most command references in one delta.
pub const MAX_LIST_ITEMS: f64 = 10_000_000.0;
/// JSON Schema pattern of a command reference `id@major` (major a u32).
pub const COMMAND_REF_PATTERN: &str = "^[a-z0-9]([a-z0-9_.]*[a-z0-9])?@(0|[1-9][0-9]{0,9})$";

/// The diff of `source` against `target` over `stream` in `store` (module documentation;
/// [`diff_rows`]).
pub fn diff_store(
    store: &Store,
    stream: &str,
    branches: &Branches,
    source: &str,
    target: &str,
    opts: &DiffOptions,
) -> Result<BranchDiff, DiffError> {
    check_stream(stream, None)?;
    let max_entries = to_i64(opts.max_entries.saturating_add(1));
    let header_cap = to_i64(u64::try_from(MAX_HEADER_BYTES).unwrap_or(1 << 20));
    store
        .read(|conn| {
            let mut st = conn.prepare(ROWS_SQL)?;
            let rows = st.query_map([i64::MIN, header_cap, max_entries], |r| {
                Ok(stored((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))
            })?;
            let rows = rows.map(|r| r.map_err(|e| storage("read entries", e)));
            Ok(diff_rows(stream, rows, branches, source, target, opts))
        })
        .map_err(|e| DiffError::from(storage("diff", e)))?
}

impl Journal {
    /// `journal.diff_branch@1` over this journal's stream ([`diff_store`]).
    pub fn diff_branch(
        &self,
        branches: &Branches,
        source: &str,
        target: &str,
        opts: &DiffOptions,
    ) -> Result<BranchDiff, DiffError> {
        self.check_poison().map_err(|e| DiffError::Read(ReadError::Storage(e.to_string())))?;
        diff_store(&self.store, &self.stream, branches, source, target, opts)
    }
}

// ------------------------------------------------------------------ registry definition

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

fn string(pattern: &str, max: f64, description: &str) -> CValue {
    obj([
        ("type", text("string")),
        ("pattern", text(pattern)),
        ("maxLength", CValue::Num(max)),
        ("description", text(description)),
    ])
}

fn digest(description: &str) -> CValue {
    string(DIGEST_PATTERN, 71.0, description)
}

fn digest_or_null(description: &str) -> CValue {
    obj([
        ("anyOf", CValue::Arr(vec![digest(description), obj([("type", text("null"))])])),
        ("description", text(description)),
    ])
}

fn name(description: &str) -> CValue {
    string(NAME_PATTERN, 100.0, description)
}

fn array(items: CValue, description: &str) -> CValue {
    obj([
        ("type", text("array")),
        ("maxItems", CValue::Num(MAX_LIST_ITEMS)),
        ("items", items),
        ("description", text(description)),
    ])
}

fn command_ref() -> CValue {
    let kinds: Vec<&str> = PrincipalKind::ALL.iter().map(|k| k.as_str()).collect();
    object(
        &["command", "entry", "principal", "seq"],
        obj([
            ("entry", digest("Hash of the entry that recorded the command.")),
            (
                "seq",
                obj([
                    ("type", text("string")),
                    ("pattern", text(SEQ_PATTERN)),
                    ("maxLength", CValue::Num(20.0)),
                    ("description", text("Its sequence (decimal).")),
                ]),
            ),
            ("command", string(COMMAND_REF_PATTERN, 200.0, "Command id@major.")),
            (
                "principal",
                obj([
                    ("type", text("string")),
                    ("enum", strs(&kinds)),
                    ("description", text("Principal kind of the verified entry header.")),
                ]),
            ),
        ]),
    )
}

fn entity_delta() -> CValue {
    object(
        &["conflict", "id", "source_commands", "source_digest", "target_commands", "target_digest"],
        obj([
            (
                "id",
                obj([
                    ("type", text("string")),
                    ("maxLength", CValue::Num(1_048_576.0)),
                    ("x-untrusted", CValue::Bool(true)),
                    ("description", text("Stable entity ID, as the journal headers name it.")),
                ]),
            ),
            ("target_digest", digest_or_null("Last output digest on the target; null when absent or deleted.")),
            ("source_digest", digest_or_null("Last output digest on the source; null when absent or deleted.")),
            ("target_commands", array(command_ref(), "Commands that output it on the target since the ancestor.")),
            ("source_commands", array(command_ref(), "Commands that output it on the source since the ancestor.")),
            (
                "conflict",
                obj([("type", text("boolean")), ("description", text("Both sides changed it, to different digests."))]),
            ),
        ]),
    )
}

fn side(description: &str) -> CValue {
    let mut v = object(
        &["branch", "head", "owner"],
        obj([
            ("branch", name("Branch name.")),
            ("head", digest_or_null("Head the diff used; null for the empty state.")),
            (
                "owner",
                obj([
                    ("type", text("string")),
                    ("enum", strs(&["agent", "human"])),
                    ("description", text("agent for the agent/ namespace, else human (branch table).")),
                ]),
            ),
        ]),
    );
    if let CValue::Obj(o) = &mut v {
        o.insert("description".to_string(), text(description));
    }
    v
}

/// The params schema of `journal.diff_branch@1`.
pub fn params_schema() -> CValue {
    object(
        &["source", "target"],
        obj([("source", name("Branch whose changes would be promoted.")), ("target", name("Branch promoted into."))]),
    )
}

/// The result schema of `journal.diff_branch@1`; `schema/branch-diff.schema.json` is this
/// value plus `$schema`, `$id` and `title`.
pub fn result_schema() -> CValue {
    object(
        &["added", "ancestor", "changed", "removed", "source", "target"],
        obj([
            ("ancestor", digest_or_null("Nearest common ancestor on the logical DAG; null when there is none.")),
            ("source", side("The source branch.")),
            ("target", side("The target branch.")),
            ("added", array(entity_delta(), "Entities added on the source, sorted by ID.")),
            ("removed", array(entity_delta(), "Entities removed on the source, sorted by ID.")),
            ("changed", array(entity_delta(), "Entities changed on either side, sorted by ID.")),
        ]),
    )
}

/// The definition of `journal.diff_branch@1`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let def = CommandDef::from_value(obj([
        ("id", text(DIFF_BRANCH)),
        ("major", CValue::Num(1.0)),
        ("title", text("Diff branches")),
        (
            "description",
            text(
                "Lists entities added, removed and changed between a source and a target branch since their \
                 nearest common ancestor on the logical DAG, with old and new output digests, the commands that \
                 changed each one and a conflict flag, from verified entry headers only.",
            ),
        ),
        ("params_schema", params_schema()),
        ("result_schema", result_schema()),
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
    ]))?;
    Ok(vec![def])
}
