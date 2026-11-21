//! Registry definitions of the three promotion commands (CORE.REG.DEF).

use super::{
    BRANCH_PROMOTED, MAX_ASSERTION_BYTES, MAX_NOTE_CHARS, MAX_STEPS, PROMOTE_BRANCH, PROMOTE_PROTECTED_BRANCH,
    PROMOTION_REQUESTED, PROTECTED_BRANCH_PROMOTED, REQUEST_PROMOTE_BRANCH,
};
use crate::branch::NAME_PATTERN;
use crate::diff::COMMAND_REF_PATTERN;
use crate::entry::DIGEST_PATTERN;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};

const LIFECYCLE_SINCE: &str = "2026-11-22";
/// JSON Schema pattern of an assertion reference.
pub const ASSERTION_PATTERN: &str = "^[A-Za-z0-9._:/+=-]{1,4096}$";

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn object(required: &[&str], properties: Vec<(&str, CValue)>) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", CValue::Obj(properties.into_iter().map(|(k, v)| (k.to_string(), v)).collect())),
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

fn note() -> CValue {
    obj([
        ("type", text("string")),
        ("maxLength", CValue::Num(MAX_NOTE_CHARS as f64)),
        ("x-untrusted", CValue::Bool(true)),
        ("description", text("Free text from the requester, kept as untrusted text.")),
    ])
}

/// The fields every call and event binds.
fn binding() -> Vec<(&'static str, CValue)> {
    vec![
        ("source", name("Branch whose work is promoted.")),
        ("source_head", digest("Source head the diff was approved at.")),
        ("target", name("Human branch promoted into.")),
        ("target_head", digest_or_null("Target head the diff was approved at; null for the empty state.")),
        ("diff", digest("SHA-256 of the canonical journal.diff_branch@1 result the promotion was approved against.")),
    ]
}

const BINDING: [&str; 5] = ["diff", "source", "source_head", "target", "target_head"];

fn step() -> CValue {
    object(
        &["command", "digest", "outcome", "step"],
        vec![
            ("command", string(COMMAND_REF_PATTERN, 200.0, "Re-executed command id@major.")),
            ("digest", digest("Digest of the re-executed entry.")),
            (
                "outcome",
                obj([
                    ("type", text("string")),
                    ("enum", strs(&["EQUIVALENT", "IDENTICAL"])),
                    ("description", text("Replay outcome of the step.")),
                ]),
            ),
            (
                "step",
                obj([
                    ("type", text("integer")),
                    ("minimum", CValue::Num(1.0)),
                    ("maximum", CValue::Num(MAX_STEPS as f64)),
                    ("description", text("1-based step number.")),
                ]),
            ),
        ],
    )
}

fn promotion(protected: bool) -> CValue {
    let mut props = binding();
    props.push((
        "mode",
        obj([
            ("type", text("string")),
            ("enum", strs(&["fast_forward", "rebased"])),
            ("description", text("fast_forward moves the target head; rebased re-executed the source's commands.")),
        ]),
    ));
    props.push((
        "entries",
        obj([
            ("type", text("array")),
            ("maxItems", CValue::Num(MAX_STEPS as f64)),
            ("items", step()),
            ("description", text("Re-executed steps in order, promoted from source_head; empty for fast_forward.")),
        ]),
    ));
    let mut required: Vec<&str> = BINDING.to_vec();
    required.extend(["entries", "mode"]);
    if protected {
        props.push(("pre_image", digest("Digest the human assertion confirms.")));
        required.push("pre_image");
    }
    required.sort_unstable();
    object(&required, props)
}

fn request() -> CValue {
    let mut props = binding();
    props.push(("note", note()));
    object(&BINDING, props)
}

/// The params schema of a command.
pub fn params_schema(command: &str) -> CValue {
    let mut props = binding();
    let mut required: Vec<&str> = BINDING.to_vec();
    if command == PROMOTE_PROTECTED_BRANCH {
        props.push((
            "assertion",
            obj([
                ("type", text("string")),
                ("pattern", text(ASSERTION_PATTERN)),
                ("maxLength", CValue::Num(MAX_ASSERTION_BYTES as f64)),
                ("x-untrusted", CValue::Bool(true)),
                ("description", text("Reference to the human assertion over the promotion's pre-image.")),
            ]),
        ));
        required.push("assertion");
        required.sort_unstable();
    }
    if command == REQUEST_PROMOTE_BRANCH {
        props.push(("note", note()));
    }
    object(&required, props)
}

/// The result schema of a command: `{promotion}` or `{request}`, the committed event's body.
pub fn result_schema(command: &str) -> CValue {
    match command {
        REQUEST_PROMOTE_BRANCH => object(&["request"], vec![("request", request())]),
        PROMOTE_PROTECTED_BRANCH => object(&["promotion"], vec![("promotion", promotion(true))]),
        _ => object(&["promotion"], vec![("promotion", promotion(false))]),
    }
}

struct Spec {
    id: &'static str,
    title: &'static str,
    description: &'static str,
    class: &'static str,
    projection: &'static str,
    effect: &'static str,
    exposure: &'static [&'static str],
    human_only: bool,
    undo: &'static str,
}

/// The definitions of `journal.promote_branch@1`, `journal.promote_protected_branch@1` and
/// `journal.request_promote_branch@1`, in that order.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let specs = [
        Spec {
            id: PROMOTE_BRANCH,
            title: "Promote branch",
            description: "Adopts a branch's work onto an unprotected human branch, bound to the approved diff by \
                          digest and to both heads (compare-and-swap): fast-forward, or the Rebaser's re-executed \
                          steps, all or nothing. Human only; an agent files a request.",
            class: "W2",
            projection: "request_only",
            effect: BRANCH_PROMOTED,
            exposure: &["cli", "mcp"],
            human_only: true,
            undo: "head_move",
        },
        Spec {
            id: PROMOTE_PROTECTED_BRANCH,
            title: "Promote into protected branch",
            description: "As journal.promote_branch@1, into a protected branch: a controlled-record change that \
                          needs a confirmed human assertion over the promotion's pre-image.",
            class: "W3",
            projection: "request_only",
            effect: PROTECTED_BRANCH_PROMOTED,
            exposure: &["cli"],
            human_only: false,
            undo: "not_undoable",
        },
        Spec {
            id: REQUEST_PROMOTE_BRANCH,
            title: "Request branch promotion",
            description: "Records a request that a human promote a branch, bound to the diff and both heads; \
                          moves no head. The requester is the caller.",
            class: "W1",
            projection: "exposed",
            effect: PROMOTION_REQUESTED,
            exposure: &["cli", "mcp"],
            human_only: false,
            undo: "not_undoable",
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
                ("params_schema", params_schema(s.id)),
                ("result_schema", result_schema(s.id)),
                ("side_effect", text(s.class)),
                ("mcp_projection", text(s.projection)),
                ("determinism", text("D0")),
                ("idempotency", text("none")),
                ("fs_effect", text("none")),
                ("undo", text(s.undo)),
                ("preconditions", CValue::Arr(Vec::new())),
                ("effects", strs(&[s.effect])),
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
