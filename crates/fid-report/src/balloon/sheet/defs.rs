//! Registry definitions for `report.add_drawing_balloon@1`, `report.list_drawing_balloon@1`
//! and `report.set_size_drawing_balloon@1` (`RPT.BALLOON.SHEET.IMPORT`), validated through
//! [`CommandDef::from_value`].

use super::command::MAX_FILE_PARAM_BYTES;
use super::model::MAX_DRAWING_TEXT_BYTES;
use crate::doc::defs::ID_PATTERN;
use crate::doc::model::obj;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;

const LIFECYCLE_SINCE: &str = "2026-11-22";
const UINT_STR_PATTERN: &str = "^(0|[1-9][0-9]{0,18})$";
const POS_INT_STR_PATTERN: &str = "^[1-9][0-9]{0,18}$";

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn num(n: usize) -> CValue {
    crate::json_num::num_usize(n)
}

fn id(description: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text(ID_PATTERN)), ("description", text(description))])
}

fn object(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

fn digest() -> CValue {
    obj([("type", text("string")), ("pattern", text(Digest::PATTERN))])
}

fn untrusted_text_schema(description: &str) -> CValue {
    let mut v = object(
        &["text", "untrusted"],
        obj([
            ("text", obj([("type", text("string")), ("x-untrusted", CValue::Bool(true))])),
            ("untrusted", obj([("const", CValue::Bool(true))])),
        ]),
    );
    if let CValue::Obj(o) = &mut v {
        o.insert("description".to_string(), text(description));
    }
    v
}

fn warning_schema() -> CValue {
    object(
        &["byte_offset", "code", "detail"],
        obj([
            ("byte_offset", obj([("type", strs(&["string", "null"])), ("pattern", text(UINT_STR_PATTERN))])),
            ("code", obj([("type", text("string"))])),
            ("detail", obj([("type", text("string"))])),
        ]),
    )
}

fn sheet_schema() -> CValue {
    let blob_ref = object(
        &["digest", "size"],
        obj([("digest", digest()), ("size", obj([("type", text("string")), ("pattern", text(UINT_STR_PATTERN))]))]),
    );
    let px = object(
        &["height", "width"],
        obj([
            ("height", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
            ("width", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
        ]),
    );
    object(
        &["drawing_rev", "height_cmm", "id", "image", "index", "px", "status", "width_cmm"],
        obj([
            ("drawing_rev", id("Owning drawing revision ID (UUIDv7).")),
            ("height_cmm", obj([("type", text("string")), ("pattern", text(UINT_STR_PATTERN))])),
            ("id", id("Drawing sheet ID (UUIDv7).")),
            ("image", obj([("anyOf", CValue::Arr(vec![blob_ref, obj([("type", text("null"))])]))])),
            ("index", obj([("type", text("string")), ("pattern", text(UINT_STR_PATTERN))])),
            ("px", obj([("anyOf", CValue::Arr(vec![px, obj([("type", text("null"))])]))])),
            ("status", obj([("type", text("string")), ("enum", strs(&["ready", "needs_size"]))])),
            ("width_cmm", obj([("type", text("string")), ("pattern", text(UINT_STR_PATTERN))])),
        ]),
    )
}

fn revision_schema() -> CValue {
    object(
        &[
            "carried_from",
            "drawing_number",
            "id",
            "part_rev",
            "revision_label",
            "sheets",
            "source_digest",
            "title",
            "warnings",
        ],
        obj([
            (
                "carried_from",
                obj([
                    ("type", text("object")),
                    ("additionalProperties", obj([("type", text("string")), ("pattern", text(ID_PATTERN))])),
                ]),
            ),
            ("drawing_number", untrusted_text_schema("Drawing number (untrusted text, <= 256 bytes).")),
            ("id", id("Drawing revision ID (UUIDv7).")),
            ("part_rev", id("Part revision ID (UUIDv7).")),
            ("revision_label", untrusted_text_schema("Revision label (untrusted text, <= 256 bytes).")),
            ("sheets", obj([("type", text("array")), ("items", id("Drawing sheet ID (UUIDv7)."))])),
            ("source_digest", digest()),
            (
                "title",
                obj([(
                    "anyOf",
                    CValue::Arr(vec![untrusted_text_schema("Optional title."), obj([("type", text("null"))])]),
                )]),
            ),
            ("warnings", obj([("type", text("array")), ("items", warning_schema())])),
        ]),
    )
}

struct Spec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    params: CValue,
    result: CValue,
    write: bool,
    reads_file: bool,
}

fn def(s: Spec) -> Result<CommandDef, DefError> {
    let event = format!("{}@1", s.name);
    CommandDef::from_value(obj([
        ("id", text(s.name)),
        ("major", num(1)),
        ("title", text(s.title)),
        ("description", text(s.description)),
        ("params_schema", s.params),
        ("result_schema", s.result),
        ("side_effect", text(if s.write { "W2" } else { "R0" })),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text(if s.write { "none" } else { "natural" })),
        ("fs_effect", text(if s.reads_file { "read_root" } else { "none" })),
        ("undo", text(if s.write { "head_move" } else { "not_undoable" })),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", if s.write { strs(&[event.as_str()]) } else { CValue::Arr(Vec::new()) }),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}

/// Returns the three command definitions of `RPT.BALLOON.SHEET.IMPORT` in order:
/// `report.add_drawing_balloon@1`, `report.list_drawing_balloon@1`,
/// `report.set_size_drawing_balloon@1`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let add = def(Spec {
        name: "report.add_drawing_balloon",
        title: "Import a 2D drawing revision for ballooning",
        description: "Imports a PDF, PNG or TIFF drawing from an approved import root into a new DrawingRevision and ordered DrawingSheets.",
        params: object(
            &["drawing_number", "file", "part_rev", "revision_label"],
            obj([
                (
                    "drawing_number",
                    obj([
                        ("type", text("string")),
                        ("minLength", num(1)),
                        ("maxLength", num(MAX_DRAWING_TEXT_BYTES)),
                        ("x-untrusted", CValue::Bool(true)),
                    ]),
                ),
                ("dry_run", obj([("type", text("boolean")), ("default", CValue::Bool(false))])),
                (
                    "file",
                    obj([
                        ("type", text("string")),
                        ("minLength", num(1)),
                        ("maxLength", num(MAX_FILE_PARAM_BYTES)),
                        ("description", text("Approved import-root handle (root://<id>/<rel>).")),
                    ]),
                ),
                ("part_rev", id("Part revision ID (UUIDv7).")),
                (
                    "revision_label",
                    obj([
                        ("type", text("string")),
                        ("minLength", num(1)),
                        ("maxLength", num(MAX_DRAWING_TEXT_BYTES)),
                        ("x-untrusted", CValue::Bool(true)),
                    ]),
                ),
                (
                    "title",
                    obj([
                        ("type", strs(&["string", "null"])),
                        ("maxLength", num(MAX_DRAWING_TEXT_BYTES)),
                        ("default", CValue::Null),
                        ("x-untrusted", CValue::Bool(true)),
                    ]),
                ),
            ]),
        ),
        result: object(
            &["committed", "drawing_rev", "sheets", "warnings"],
            obj([
                ("committed", obj([("type", text("boolean"))])),
                ("drawing_rev", revision_schema()),
                ("sheets", obj([("type", text("array")), ("items", sheet_schema())])),
                ("warnings", obj([("type", text("array")), ("items", warning_schema())])),
            ]),
        ),
        write: true,
        reads_file: true,
    })?;

    let list = def(Spec {
        name: "report.list_drawing_balloon",
        title: "List imported 2D drawing revisions and sheets",
        description: "Lists drawing revisions and their sheets ordered by drawing number, revision label, then ID.",
        params: object(
            &[],
            obj([(
                "part_rev",
                obj([("type", strs(&["string", "null"])), ("pattern", text(ID_PATTERN)), ("default", CValue::Null)]),
            )]),
        ),
        result: object(
            &["revisions"],
            obj([(
                "revisions",
                obj([
                    ("type", text("array")),
                    (
                        "items",
                        object(
                            &["drawing_rev", "sheets"],
                            obj([
                                ("drawing_rev", revision_schema()),
                                ("sheets", obj([("type", text("array")), ("items", sheet_schema())])),
                            ]),
                        ),
                    ),
                ]),
            )]),
        ),
        write: false,
        reads_file: false,
    })?;

    let size_width = object(
        &["cmm", "kind"],
        obj([
            ("cmm", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
            ("kind", obj([("const", text("width"))])),
        ]),
    );
    let size_dpi = object(
        &["kind", "x", "y"],
        obj([
            ("kind", obj([("const", text("dpi"))])),
            ("x", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
            ("y", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
        ]),
    );

    let set_size = def(Spec {
        name: "report.set_size_drawing_balloon",
        title: "Set physical size or resolution on a raster drawing sheet",
        description: "States physical width in 0.01 mm or DPI on a raster sheet and transitions it to ready.",
        params: object(
            &["sheet", "size"],
            obj([
                ("sheet", id("Target raster sheet ID (UUIDv7).")),
                ("size", obj([("anyOf", CValue::Arr(vec![size_width, size_dpi]))])),
            ]),
        ),
        result: sheet_schema(),
        write: true,
        reads_file: false,
    })?;

    Ok(vec![add, list, set_size])
}
