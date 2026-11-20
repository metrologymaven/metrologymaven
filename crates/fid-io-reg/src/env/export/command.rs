//! Registry definitions of `export.file@1` and of the `export.<format>@1` commands built from
//! the skeleton (CORE.REG.DEF), built through [`CommandDef::from_value`] so every field rule
//! of the registry applies.
//!
//! Both are W1 with `fs_effect: write_root` (CONSTITUTION §3.1), `exposed` on MCP (the
//! loosest projection W1 allows), D0, `idempotency: none` (a second identical run fails
//! `target_exists`) and `not_undoable` (the file is outside the project). `target` is
//! `path_like`, so the bus's root guard (CORE.BUS.POL.FS) applies; its `x-extensions` list is
//! every extension the registry's export descriptors declare, so a human's
//! `force_extension` to another format's extension can still pass the guard. The result is
//! the [`crate::ExportReport`] JSON (`IO.REG.ENV.REPORT`).

use super::{format_command_id, ExportError, ENTITY_ID_PATTERN, EXPORT_FILE, MAX_SOURCE_IDS, MAX_TARGET_BYTES};
use crate::detect::{OriginFilter, Registry};
use crate::report::{FormatId, MAX_LISTED, MAX_LOSSY};
use crate::roots::Direction;
use fid_canon::CValue;
use fid_registry::CommandDef;
use std::collections::{BTreeMap, BTreeSet};

/// The day these definitions were first published.
const LIFECYCLE_SINCE: &str = "2026-11-20";
/// The pattern of a format ID (`IO.REG.ENV.REPORT`).
pub const FORMAT_ID_PATTERN: &str = "^[a-z0-9][a-z0-9._-]{0,63}$";
/// The pattern of a 64-bit integer written as a decimal string.
pub const U64_PATTERN: &str = "^(0|[1-9][0-9]{0,19})$";
/// The pattern of a `sha256:` digest.
pub const DIGEST_PATTERN: &str = "^sha256:[0-9a-f]{64}$";
/// The pattern of a warning code.
pub const CODE_PATTERN: &str = "^[a-z][a-z0-9_]{0,63}$";
/// The pattern of a lossy name (snake_case segments joined by `.`).
pub const TYPE_NAME_PATTERN: &str = "^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)*$";

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

fn flag(description: &str) -> CValue {
    obj([("type", text("boolean")), ("default", CValue::Bool(false)), ("description", text(description))])
}

/// Every extension declared by an export descriptor, sorted and without repeats.
fn export_extensions(reg: &Registry) -> Vec<String> {
    let set: BTreeSet<&String> =
        reg.list(Direction::Export, OriginFilter::Any).into_iter().flat_map(|d| d.extensions().iter()).collect();
    set.into_iter().cloned().collect()
}

/// The params schema; `format` is present (and required) for `export.file@1` only.
pub fn params_schema(reg: &Registry, with_format: bool) -> Result<CValue, ExportError> {
    let exts = export_extensions(reg);
    if exts.is_empty() {
        return Err(ExportError::BadParams("no export format declares an extension"));
    }
    let entity = |selector: &str| {
        obj([
            ("type", text("string")),
            ("x-ref", text("element")),
            ("x-selector", text(selector)),
            ("pattern", text(ENTITY_ID_PATTERN)),
        ])
    };
    let mut props: BTreeMap<String, CValue> = BTreeMap::new();
    props.insert(
        "source".into(),
        obj([
            (
                "anyOf",
                CValue::Arr(vec![
                    entity("single"),
                    obj([
                        ("type", text("array")),
                        ("x-ref", text("element")),
                        ("x-selector", text("list")),
                        ("items", obj([("type", text("string")), ("pattern", text(ENTITY_ID_PATTERN))])),
                        ("minItems", num(1)),
                        ("maxItems", num(MAX_SOURCE_IDS)),
                    ]),
                ]),
            ),
            ("description", text("The entity to export, or a non-empty selection of entities.")),
        ]),
    );
    if with_format {
        props.insert(
            "format".into(),
            obj([
                ("type", text("string")),
                ("pattern", text(FORMAT_ID_PATTERN)),
                ("description", text("The export format ID, as listed by export.list_formats@1.")),
            ]),
        );
    }
    props.insert(
        "target".into(),
        obj([
            ("type", text("string")),
            ("path_like", CValue::Bool(true)),
            ("x-extensions", strs(&exts)),
            ("minLength", num(1)),
            ("maxLength", num(MAX_TARGET_BYTES)),
            (
                "description",
                text("The file to write: a root:// handle in an export root (a human may give a raw path)."),
            ),
        ]),
    );
    props.insert(
        "options".into(),
        obj([
            ("type", text("object")),
            ("default", CValue::Obj(BTreeMap::new())),
            ("description", text("Writer options, checked against the format's options schema.")),
        ]),
    );
    props.insert("overwrite".into(), flag("Replace an existing regular file at the target."));
    props.insert(
        "force_extension".into(),
        flag("Write even if the target extension names another format. Human principals only."),
    );
    props.insert("dry_run".into(), flag("Serialise and report only; create no file."));
    let required: &[&str] = if with_format { &["format", "source", "target"] } else { &["source", "target"] };
    Ok(obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", CValue::Obj(props)),
    ]))
}

/// The result schema: the export report JSON.
pub fn result_schema() -> CValue {
    let u64s = |d: &str| obj([("type", text("string")), ("pattern", text(U64_PATTERN)), ("description", text(d))]);
    let line_col = obj([("type", strs(["integer", "null"])), ("minimum", num(1))]);
    let pos = obj([
        ("type", strs(["object", "null"])),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(["byte", "col", "line"])),
        (
            "properties",
            obj([
                ("byte", obj([("type", text("string")), ("pattern", text(U64_PATTERN))])),
                ("col", line_col.clone()),
                ("line", line_col),
            ]),
        ),
    ]);
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
    let warning = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(["code", "detail", "pos"])),
        (
            "properties",
            obj([
                ("code", obj([("type", text("string")), ("pattern", text(CODE_PATTERN))])),
                ("detail", detail),
                ("pos", pos),
            ]),
        ),
    ]);
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(["bytes", "dry_run", "format", "lossy", "sha256", "target", "warnings", "warnings_total"])),
        (
            "properties",
            obj([
                ("bytes", u64s("Size of the written (or, for a dry run, would-be) file in bytes.")),
                ("dry_run", obj([("type", text("boolean"))])),
                ("format", obj([("type", text("string")), ("pattern", text(FORMAT_ID_PATTERN))])),
                (
                    "lossy",
                    obj([
                        ("type", text("array")),
                        ("items", obj([("type", text("string")), ("pattern", text(TYPE_NAME_PATTERN))])),
                        ("maxItems", num(MAX_LOSSY)),
                        ("description", text("Fields and entity kinds the format could not represent, sorted.")),
                    ]),
                ),
                (
                    "sha256",
                    obj([
                        ("type", text("string")),
                        ("pattern", text(DIGEST_PATTERN)),
                        ("description", text("SHA-256 of the file's bytes (would-be bytes for a dry run).")),
                    ]),
                ),
                ("target", obj([("type", text("string")), ("minLength", num(1)), ("x-untrusted", CValue::Bool(true))])),
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
        ("side_effect", text("W1")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("write_root")),
        ("undo", text("not_undoable")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs([id])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("unavailable")),
        ("web_note", text("Writes into a local export root; the web build has no file-system roots.")),
        ("cli_bare", CValue::Bool(false)),
    ])
}

/// The definition value of `export.file@1` for the export formats in `reg`.
pub fn export_file_def_value(reg: &Registry) -> Result<CValue, ExportError> {
    Ok(def_value(
        EXPORT_FILE,
        "Export to a file",
        "Serialises entities with the chosen format's writer into a file in an approved export root, \
         atomically and without overwriting unless asked, and journals the file's SHA-256.",
        params_schema(reg, true)?,
    ))
}

/// The registry definition of `export.file@1`.
pub fn export_file_def(reg: &Registry) -> Result<CommandDef, ExportError> {
    CommandDef::from_value(export_file_def_value(reg)?).map_err(|_| ExportError::Internal("invalid command definition"))
}

/// The definition value of `export.<format>@1`.
pub fn format_def_value(reg: &Registry, format: &FormatId) -> Result<CValue, ExportError> {
    let id = format_command_id(format)?;
    Ok(def_value(
        id.as_str(),
        &format!("Export to {}", format.as_str()),
        &format!(
            "Serialises entities as {} into a file in an approved export root, atomically and without \
             overwriting unless asked, and journals the file's SHA-256.",
            format.as_str()
        ),
        params_schema(reg, false)?,
    ))
}

/// The registry definition of `export.<format>@1`.
pub fn format_def(reg: &Registry, format: &FormatId) -> Result<CommandDef, ExportError> {
    CommandDef::from_value(format_def_value(reg, format)?)
        .map_err(|_| ExportError::Internal("invalid command definition"))
}
