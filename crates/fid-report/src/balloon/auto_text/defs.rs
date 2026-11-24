//! Registry definition for `report.extract_drawing_text@1` (`RPT.BALLOON.AUTO.TEXT`),
//! built through [`CommandDef::from_value`].

use crate::doc::defs::ID_PATTERN;
use crate::doc::model::obj;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};

const LIFECYCLE_SINCE: &str = "2026-11-22";
const INT_STR_PATTERN: &str = "^-?(0|[1-9][0-9]{0,18})$";
const UINT_STR_PATTERN: &str = "^(0|[1-9][0-9]{0,18})$";

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn num(n: usize) -> CValue {
    crate::json_num::num_usize(n)
}

fn object(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

fn rect_schema() -> CValue {
    object(
        &["x0", "x1", "y0", "y1"],
        obj([
            ("x0", obj([("type", text("string")), ("pattern", text(INT_STR_PATTERN))])),
            ("x1", obj([("type", text("string")), ("pattern", text(INT_STR_PATTERN))])),
            ("y0", obj([("type", text("string")), ("pattern", text(INT_STR_PATTERN))])),
            ("y1", obj([("type", text("string")), ("pattern", text(INT_STR_PATTERN))])),
        ]),
    )
}

fn untrusted_text_schema() -> CValue {
    object(
        &["text", "untrusted"],
        obj([
            ("text", obj([("type", text("string")), ("x-untrusted", CValue::Bool(true))])),
            ("untrusted", obj([("const", CValue::Bool(true))])),
        ]),
    )
}

fn text_run_schema() -> CValue {
    object(
        &["angle_mdeg", "bbox", "font_h_cmm", "id", "text"],
        obj([
            ("angle_mdeg", obj([("type", text("string")), ("pattern", text(INT_STR_PATTERN))])),
            ("bbox", rect_schema()),
            ("font_h_cmm", obj([("type", text("string")), ("pattern", text(INT_STR_PATTERN))])),
            ("id", obj([("type", text("string")), ("pattern", text(UINT_STR_PATTERN))])),
            ("text", untrusted_text_schema()),
        ]),
    )
}

/// Returns the registry [`CommandDef`] for `report.extract_drawing_text@1` (`R1`, MCP exposed).
pub fn command_def() -> Result<CommandDef, DefError> {
    let params = object(
        &["sheet"],
        obj([
            ("include_frame_boxes", obj([("type", text("boolean")), ("default", CValue::Bool(true))])),
            (
                "sheet",
                obj([
                    ("type", text("string")),
                    ("pattern", text(ID_PATTERN)),
                    ("description", text("Drawing sheet ID (UUIDv7).")),
                ]),
            ),
        ]),
    );
    let result = object(
        &["frame_boxes", "page_kind", "runs", "sheet", "warnings"],
        obj([
            ("frame_boxes", obj([("type", text("array")), ("items", rect_schema())])),
            ("page_kind", obj([("type", text("string")), ("enum", strs(&["vector", "mixed", "raster_only"]))])),
            ("runs", obj([("type", text("array")), ("items", text_run_schema())])),
            ("sheet", obj([("type", text("string")), ("pattern", text(ID_PATTERN))])),
            ("warnings", obj([("type", text("array")), ("items", obj([("type", text("string"))]))])),
        ]),
    );

    CommandDef::from_value(obj([
        ("id", text("report.extract_drawing_text")),
        ("major", num(1)),
        ("title", text("Extract bounded vector text runs and frame boxes from a drawing sheet")),
        (
            "description",
            text("Extracts vector text runs, integer 0.01 mm sheet bounding boxes and callout frame boxes from an imported PDF drawing sheet without executing PDF actions or JavaScript."),
        ),
        ("params_schema", params),
        ("result_schema", result),
        ("side_effect", text("R1")),
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
    ]))
}
