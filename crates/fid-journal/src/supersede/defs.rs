//! Registry definition of `journal.supersede_record@1` (CORE.REG.DEF).

use super::{MAX_REASON_CHARS, RECORD_SUPERSEDED, SUPERSEDE_RECORD};
use crate::entry::DIGEST_PATTERN;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};

const LIFECYCLE_SINCE: &str = "2026-11-22";

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

fn digest(description: &str) -> CValue {
    obj([
        ("type", text("string")),
        ("pattern", text(DIGEST_PATTERN)),
        ("maxLength", CValue::Num(71.0)),
        ("description", text(description)),
    ])
}

/// The params schema: `{old, new, reason}`.
pub fn params_schema() -> CValue {
    object(
        &["new", "old", "reason"],
        vec![
            ("old", digest("Digest of the released record that is superseded.")),
            ("new", digest("Digest of the released record that replaces it.")),
            (
                "reason",
                obj([
                    ("type", text("string")),
                    ("minLength", CValue::Num(1.0)),
                    ("maxLength", CValue::Num(MAX_REASON_CHARS as f64)),
                    ("x-untrusted", CValue::Bool(true)),
                    (
                        "description",
                        text("Why the record is superseded; untrusted text, kept as the entry's salted reason."),
                    ),
                ]),
            ),
        ],
    )
}

/// The result schema: `{supersession}`, the committed event's body.
pub fn result_schema() -> CValue {
    object(
        &["supersession"],
        vec![(
            "supersession",
            object(
                &["head", "new", "old"],
                vec![
                    ("head", digest("Logical head of the branch the supersession was decided against.")),
                    ("new", digest("The replacing record.")),
                    ("old", digest("The superseded record; its entry stays unchanged and visible.")),
                ],
            ),
        )],
    )
}

/// The definition of `journal.supersede_record@1`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let def = CommandDef::from_value(obj([
        ("id", text(SUPERSEDE_RECORD)),
        ("major", CValue::Num(1.0)),
        ("title", text("Supersede released record")),
        (
            "description",
            text(
                "Records that a new released record replaces an old one on the branch, with a reason; the old \
                 record's entry stays unchanged and visible. W3: a human only; an agent files a request.",
            ),
        ),
        ("params_schema", params_schema()),
        ("result_schema", result_schema()),
        ("side_effect", text("W3")),
        ("mcp_projection", text("request_only")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[RECORD_SUPERSEDED])),
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
