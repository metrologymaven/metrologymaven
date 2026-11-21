//! Registry definitions of the four branch commands (CORE.REG.DEF).

use super::{
    BRANCH_CREATED, BRANCH_PROTECTED, COEDITING_SET, CREATE_BRANCH, LIST_BRANCHES, NAME_PATTERN, PROTECT_BRANCH,
    SESSION_PATTERN, SET_BRANCH_COEDITING,
};
use crate::entry::DIGEST_PATTERN;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};

const LIFECYCLE_SINCE: &str = "2026-11-21";

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

fn name(description: &str) -> CValue {
    string(NAME_PATTERN, 100.0, description)
}

fn digest(description: &str) -> CValue {
    string(DIGEST_PATTERN, 71.0, description)
}

fn branch_info() -> CValue {
    object(
        &["coediting", "created_from", "head", "name", "owner", "protected"],
        obj([
            ("name", name("Branch name.")),
            ("head", digest("Current head entry hash.")),
            (
                "created_from",
                obj([
                    (
                        "anyOf",
                        CValue::Arr(vec![digest("Head the branch was created at."), obj([("type", text("null"))])]),
                    ),
                    ("description", text("Head the branch was created at; null for main.")),
                ]),
            ),
            ("protected", obj([("type", text("boolean")), ("description", text("Protected for good."))])),
            (
                "owner",
                obj([
                    ("type", text("string")),
                    ("enum", strs(&["agent", "human"])),
                    ("description", text("agent for the agent/ namespace, else human.")),
                ]),
            ),
            (
                "coediting",
                obj([
                    ("type", text("array")),
                    ("maxItems", CValue::Num(10_000.0)),
                    ("items", string(SESSION_PATTERN, 128.0, "Agent session allowed to commit directly.")),
                    ("description", text("Co-editing opt-ins, sorted.")),
                ]),
            ),
        ]),
    )
}

fn one_branch() -> CValue {
    object(&["branch"], obj([("branch", branch_info())]))
}

/// One command's varying fields.
struct Spec {
    id: &'static str,
    title: &'static str,
    description: &'static str,
    params: CValue,
    result: CValue,
    class: &'static str,
    projection: &'static str,
    effects: &'static [&'static str],
    exposure: &'static [&'static str],
    human_only: bool,
    undo: &'static str,
}

/// The definitions of `journal.create_branch@1`, `journal.list_branches@1`,
/// `journal.protect_branch@1` and `journal.set_branch_coediting@1`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let list_result = object(
        &["branches"],
        obj([(
            "branches",
            obj([
                ("type", text("array")),
                ("maxItems", CValue::Num(1_000_000.0)),
                ("items", branch_info()),
                ("description", text("Every branch, sorted by name.")),
            ]),
        )]),
    );
    let specs = [
        Spec {
            id: CREATE_BRANCH,
            title: "Create branch",
            description: "Creates a human branch at an existing head and records created_from. The name matches \
                          [a-z0-9][a-z0-9/_.-]{0,99} with no empty, . or .. segment, is new, and is not in the \
                          agent/ namespace (reserved for the bus routing module); else validation/bad_branch_name.",
            params: object(
                &["from", "name"],
                obj([("name", name("New branch name.")), ("from", digest("Head the branch starts at."))]),
            ),
            result: one_branch(),
            class: "W2",
            projection: "exposed",
            effects: &[BRANCH_CREATED],
            exposure: &["cli", "mcp"],
            human_only: false,
            undo: "head_move",
        },
        Spec {
            id: LIST_BRANCHES,
            title: "List branches",
            description: "Lists every branch with head, created_from, protected flag, owner and co-editing \
                          opt-ins, sorted by name.",
            params: object(&[], obj([])),
            result: list_result,
            class: "R0",
            projection: "exposed",
            effects: &[],
            exposure: &["cli", "mcp"],
            human_only: false,
            undo: "not_undoable",
        },
        Spec {
            id: PROTECT_BRANCH,
            title: "Protect branch",
            description: "Marks a branch protected for good (a controlled-record change); no command clears \
                          it. Human principal only.",
            params: object(&["branch"], obj([("branch", name("Branch to protect."))])),
            result: one_branch(),
            class: "W3",
            projection: "request_only",
            effects: &[BRANCH_PROTECTED],
            exposure: &["cli", "mcp"],
            human_only: false,
            undo: "not_undoable",
        },
        Spec {
            id: SET_BRANCH_COEDITING,
            title: "Set branch co-editing",
            description: "Records or revokes a human's opt-in that lets one agent session commit directly to \
                          one human branch.",
            params: object(
                &["agent_session", "allow", "branch"],
                obj([
                    ("branch", name("Human branch.")),
                    ("agent_session", string(SESSION_PATTERN, 128.0, "Agent session.")),
                    ("allow", obj([("type", text("boolean")), ("description", text("true records, false revokes."))])),
                ]),
            ),
            result: one_branch(),
            class: "W2",
            projection: "request_only",
            effects: &[COEDITING_SET],
            exposure: &["cli"],
            human_only: true,
            undo: "head_move",
        },
    ];
    specs
        .into_iter()
        .map(|s| {
            CommandDef::from_value(obj([
                ("id", text(s.id)),
                ("major", CValue::Num(1.0)),
                ("title", text(s.title)),
                ("description", text(s.description)),
                ("params_schema", s.params),
                ("result_schema", s.result),
                ("side_effect", text(s.class)),
                ("mcp_projection", text(s.projection)),
                ("determinism", text("D0")),
                ("idempotency", text("none")),
                ("fs_effect", text("none")),
                ("undo", text(s.undo)),
                ("preconditions", CValue::Arr(Vec::new())),
                ("effects", strs(s.effects)),
                ("algorithms", CValue::Arr(Vec::new())),
                ("exposure", strs(s.exposure)),
                ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
                ("human_only", CValue::Bool(s.human_only)),
                ("network", text("none")),
                ("web", text("available")),
                ("cli_bare", CValue::Bool(false)),
            ]))
        })
        .collect()
}
