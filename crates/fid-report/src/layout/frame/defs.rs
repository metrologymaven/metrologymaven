//! Registry definition of `report.move_block_layout@1` (`RPT.DESIGN.PLACE.FRAME`), validated
//! through [`CommandDef::from_value`]. The CLI (`fid report move-block-layout`) and MCP name
//! (`report_move_block_layout`) are derived from it by the registry.
//!
//! Lengths are plain strings with a unit suffix, as in `RPT.DESIGN.PAGE`: the handler turns
//! them into exact `0.01 mm` integers and refuses inexact text with `report.layout_invalid`
//! naming the field, so the schema only bounds their syntax. That absolute (`x`, `y`, `w`,
//! `h`) and delta (`dx`, `dy`, `dw`, `dh`) coordinates are not mixed, and that at least one is
//! given, is checked by the handler (`report.bad_params`).

use crate::doc::defs::ID_PATTERN;
use crate::doc::model::obj;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};

const LIFECYCLE_SINCE: &str = "2026-11-25";
/// A length as typed: decimal digits, an optional fraction, unit `mm` or `in`.
const LENGTH: &str = "[0-9]{1,16}([.][0-9]{1,32})?(mm|in)";
/// A signed integer count of `0.01 mm` as a decimal string.
const UNITS_PATTERN: &str = "^-?(0|[1-9][0-9]{0,18})$";

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn num(n: usize) -> CValue {
    crate::json_num::num_usize(n)
}

fn pattern(p: &str, description: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text(p)), ("description", text(description))])
}

fn nullable(p: &str, description: &str) -> CValue {
    obj([
        ("type", strs(&["string", "null"])),
        ("pattern", text(p)),
        ("default", CValue::Null),
        ("description", text(description)),
    ])
}

fn object(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

/// The one command definition of `RPT.DESIGN.PLACE.FRAME`: `report.move_block_layout@1`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let signed = format!("^-?{LENGTH}$");
    let unsigned = format!("^{LENGTH}$");
    let params = object(
        &["block", "report"],
        obj([
            ("block", pattern(ID_PATTERN, "Block ID (UUIDv7).")),
            ("dh", nullable(&signed, "Height change, e.g. -2mm.")),
            ("dw", nullable(&signed, "Width change, e.g. 5mm.")),
            ("dx", nullable(&signed, "Move right by this length (negative moves left), e.g. 0.35mm.")),
            ("dy", nullable(&signed, "Move down by this length (negative moves up), e.g. -1mm.")),
            ("h", nullable(&unsigned, "Frame height, at least 1 mm, e.g. 40mm.")),
            ("page", nullable(ID_PATTERN, "Page ID (UUIDv7) to move the block to; omit to keep its page.")),
            ("report", pattern(ID_PATTERN, "Report ID (UUIDv7).")),
            (
                "snap",
                obj([
                    ("type", text("boolean")),
                    ("default", CValue::Bool(false)),
                    ("description", text("Snap the frame edges to the report.grid_pitch setting.")),
                ]),
            ),
            ("w", nullable(&unsigned, "Frame width, at least 1 mm, e.g. 80mm.")),
            ("x", nullable(&signed, "Left edge from the page body origin, e.g. 10mm.")),
            ("y", nullable(&signed, "Top edge from the page body origin, e.g. 20mm.")),
        ]),
    );
    let units = |d: &str| pattern(UNITS_PATTERN, d);
    let result = object(
        &["block", "h", "page", "report", "w", "x", "y"],
        obj([
            ("block", pattern(ID_PATTERN, "Block ID (UUIDv7).")),
            ("h", units("Frame height, in 0.01 mm.")),
            ("page", pattern(ID_PATTERN, "The page that holds the block after the command.")),
            ("report", pattern(ID_PATTERN, "Report ID (UUIDv7).")),
            ("w", units("Frame width, in 0.01 mm.")),
            ("x", units("Left edge from the page body origin, in 0.01 mm.")),
            ("y", units("Top edge from the page body origin, in 0.01 mm.")),
        ]),
    );
    let def = CommandDef::from_value(obj([
        ("id", text("report.move_block_layout")),
        ("major", num(1)),
        ("title", text("Move or resize a block on the report page")),
        (
            "description",
            text(
                "Sets a block's frame (x, y, w, h relative to the page body origin) from absolute coordinates or a \
                 delta, optionally snapped to the report.grid_pitch setting and optionally moving the block to \
                 another page. Lengths take mm or in and must be exact multiples of 0.01 mm; coordinates left out \
                 keep their value. The frame must stay at least 1 mm wide and high and may lie outside the body. \
                 Changes no block content.",
            ),
        ),
        ("params_schema", params),
        ("result_schema", result),
        ("side_effect", text("W2")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&["report.move_block_layout@1"])),
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
