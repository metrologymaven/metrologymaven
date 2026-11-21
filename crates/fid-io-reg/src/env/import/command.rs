//! Registry definitions of `import.file@1` and of the `import.<format>@1` commands built from
//! the skeleton (CORE.REG.DEF), built through [`CommandDef::from_value`] so every field rule
//! of the registry applies.
//!
//! Both are W2 (they change the document: CONSTITUTION §3) with `fs_effect: read_root`,
//! `exposed` on MCP (the loosest projection W2 allows), D0, `idempotency: none` (a second
//! identical import collides on the entity IDs) and `undo: head_move`. Their effects are the
//! two tree events of every staged entity and the command's own record. `source` is
//! `path_like`, so the bus's root guard (CORE.BUS.POL.FS) applies, with every extension the
//! registry's import descriptors declare. The result is the [`crate::ImportReport`] JSON
//! (`IO.REG.ENV.REPORT`).

use super::{format_command_id, ImportError, IMPORT_FILE, MAX_SOURCE_BYTES, MAX_STAGE_TEXT_BYTES};
use crate::detect::{OriginFilter, Registry};
use crate::env::export::command::{CODE_PATTERN, FORMAT_ID_PATTERN, TYPE_NAME_PATTERN, U64_PATTERN};
use crate::prov::LengthUnit;
use crate::report::{FormatId, MAX_LISTED};
use crate::roots::Direction;
use fid_canon::CValue;
use fid_registry::CommandDef;
use std::collections::{BTreeMap, BTreeSet};

const LIFECYCLE_SINCE: &str = "2026-11-20";

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs<I: IntoIterator<Item = S>, S: AsRef<str>>(items: I) -> CValue {
    CValue::Arr(items.into_iter().map(|s| CValue::Str(s.as_ref().to_string())).collect())
}

fn num(n: usize) -> CValue {
    CValue::Num(n as f64)
}

/// The extensions the bus's root guard accepts for `source`: every import format's for
/// `import.file@1` (`format` = `None`), only that format's for `import.<format>@1`.
fn import_extensions(reg: &Registry, format: Option<&FormatId>) -> Vec<String> {
    let set: BTreeSet<&String> = reg
        .list(Direction::Import, OriginFilter::Any)
        .into_iter()
        .filter(|d| format.map_or(true, |f| d.id() == f))
        .flat_map(|d| d.extensions().iter())
        .collect();
    set.into_iter().cloned().collect()
}

/// The params schema of `import.file@1` (`format` = `None`: the optional `format` property and
/// every import extension) or of `import.<format>@1` (no `format` property, that format's
/// extensions only).
pub fn params_schema(reg: &Registry, format: Option<&FormatId>) -> Result<CValue, ImportError> {
    let with_format = format.is_none();
    let exts = import_extensions(reg, format);
    if exts.is_empty() {
        return Err(ImportError::BadParams("no import format declares an extension"));
    }
    let mut props: BTreeMap<String, CValue> = BTreeMap::new();
    props.insert(
        "source".into(),
        obj([
            ("type", text("string")),
            ("path_like", CValue::Bool(true)),
            ("x-extensions", strs(&exts)),
            ("minLength", num(1)),
            ("maxLength", num(MAX_SOURCE_BYTES)),
            (
                "description",
                text("The file to read: a root:// handle in an import root (a human may give a raw path)."),
            ),
        ]),
    );
    if with_format {
        props.insert(
            "format".into(),
            obj([
                ("type", text("string")),
                ("pattern", text(FORMAT_ID_PATTERN)),
                ("description", text("The import format; detected from the content when absent.")),
            ]),
        );
    }
    props.insert(
        "unit".into(),
        obj([
            ("type", text("string")),
            ("enum", strs(LengthUnit::ALL.iter().map(|u| u.symbol()))),
            ("description", text("The length unit, when the file declares none (IO.REG.PROV).")),
        ]),
    );
    props.insert(
        "stage".into(),
        obj([
            ("type", text("string")),
            ("x-untrusted", CValue::Bool(true)),
            ("minLength", num(1)),
            ("maxLength", num(MAX_STAGE_TEXT_BYTES)),
            ("description", text("The target stage: a stage ID or label (default: the active stage).")),
        ]),
    );
    props.insert(
        "options".into(),
        obj([
            ("type", text("object")),
            ("default", CValue::Obj(BTreeMap::new())),
            ("description", text("Reader options, checked against the format's options schema.")),
        ]),
    );
    props.insert(
        "dry_run".into(),
        obj([
            ("type", text("boolean")),
            ("default", CValue::Bool(false)),
            ("description", text("Read and report only; commit nothing.")),
        ]),
    );
    Ok(obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(["source"])),
        ("properties", CValue::Obj(props)),
    ]))
}

/// The result schema: the import report JSON.
pub fn result_schema() -> CValue {
    let u64s = |d: &str| obj([("type", text("string")), ("pattern", text(U64_PATTERN)), ("description", text(d))]);
    let line_col = obj([("type", strs(["integer", "null"])), ("minimum", num(1))]);
    let pos = |nullable: bool| {
        obj([
            ("type", if nullable { strs(["object", "null"]) } else { text("object") }),
            ("additionalProperties", CValue::Bool(false)),
            ("required", strs(["byte", "col", "line"])),
            (
                "properties",
                obj([
                    ("byte", obj([("type", text("string")), ("pattern", text(U64_PATTERN))])),
                    ("col", line_col.clone()),
                    ("line", line_col.clone()),
                ]),
            ),
        ])
    };
    let detail = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(["text", "truncated"])),
        (
            "properties",
            obj([
                ("text", obj([("type", text("string")), ("x-untrusted", CValue::Bool(true))])),
                ("truncated", obj([("type", text("boolean"))])),
            ]),
        ),
    ]);
    let code = obj([("type", text("string")), ("pattern", text(CODE_PATTERN))]);
    let warning = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(["code", "detail", "pos"])),
        ("properties", obj([("code", code.clone()), ("detail", detail.clone()), ("pos", pos(true))])),
    ]);
    let dropped = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(["detail", "kind", "pos", "reason"])),
        (
            "properties",
            obj([
                ("detail", detail),
                ("kind", obj([("type", text("string")), ("pattern", text(TYPE_NAME_PATTERN))])),
                ("pos", pos(false)),
                ("reason", code),
            ]),
        ),
    ]);
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        (
            "required",
            strs([
                "dropped",
                "dropped_total",
                "dry_run",
                "entity_counts",
                "format",
                "source",
                "warnings",
                "warnings_total",
            ]),
        ),
        (
            "properties",
            obj([
                ("dropped", obj([("type", text("array")), ("items", dropped), ("maxItems", num(MAX_LISTED))])),
                ("dropped_total", u64s("Dropped items, including those not listed.")),
                ("dry_run", obj([("type", text("boolean"))])),
                (
                    "entity_counts",
                    obj([
                        ("type", text("object")),
                        ("additionalProperties", obj([("type", text("string")), ("pattern", text(U64_PATTERN))])),
                        ("description", text("Imported entities by type name.")),
                    ]),
                ),
                ("format", obj([("type", text("string")), ("pattern", text(FORMAT_ID_PATTERN))])),
                ("source", obj([("type", text("string")), ("minLength", num(1)), ("x-untrusted", CValue::Bool(true))])),
                ("warnings", obj([("type", text("array")), ("items", warning), ("maxItems", num(MAX_LISTED))])),
                ("warnings_total", u64s("Warnings raised, including those not listed.")),
            ]),
        ),
    ])
}

fn def_value(id: &str, title: &str, description: &str, params: CValue) -> CValue {
    let name = id.split('@').next().unwrap_or(id);
    obj([
        ("id", text(name)),
        ("major", CValue::Num(1.0)),
        ("title", text(title)),
        ("description", text(description)),
        ("params_schema", params),
        ("result_schema", result_schema()),
        ("side_effect", text("W2")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("read_root")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs([fid_project::ELEMENT_CREATED, fid_project::ELEMENT_SLOT_SET, id])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("unavailable")),
        ("web_note", text("Reads from a local import root; the web build has no file-system roots.")),
        ("cli_bare", CValue::Bool(false)),
    ])
}

/// The definition value of `import.file@1` for the import formats in `reg`.
pub fn import_file_def_value(reg: &Registry) -> Result<CValue, ImportError> {
    Ok(def_value(
        IMPORT_FILE,
        "Import a file",
        "Reads an untrusted file from an approved import root with the detected (or named) format's reader, \
         under the read limits, and adds its entities to one stage in one commit, or adds nothing.",
        params_schema(reg, None)?,
    ))
}

/// The registry definition of `import.file@1`.
pub fn import_file_def(reg: &Registry) -> Result<CommandDef, ImportError> {
    CommandDef::from_value(import_file_def_value(reg)?).map_err(|_| ImportError::Internal("invalid command definition"))
}

/// The definition value of `import.<format>@1`.
pub fn format_def_value(reg: &Registry, format: &FormatId, taken: &[&str]) -> Result<CValue, ImportError> {
    let id = format_command_id(format, taken)?;
    Ok(def_value(
        id.as_str(),
        &format!("Import {}", format.as_str()),
        &format!(
            "Reads an untrusted {} file from an approved import root under the read limits and adds its \
             entities to one stage in one commit, or adds nothing.",
            format.as_str()
        ),
        params_schema(reg, Some(format))?,
    ))
}

/// The registry definition of `import.<format>@1`.
pub fn format_def(reg: &Registry, format: &FormatId, taken: &[&str]) -> Result<CommandDef, ImportError> {
    CommandDef::from_value(format_def_value(reg, format, taken)?)
        .map_err(|_| ImportError::Internal("invalid command definition"))
}
