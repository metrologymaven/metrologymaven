//! Registry definitions of the six balloon commands (`RPT.BALLOON.REG`), validated through
//! [`CommandDef::from_value`].

use super::model::MAX_ZONE_BYTES;
use crate::doc::defs::ID_PATTERN;
use crate::doc::model::obj;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};

const LIFECYCLE_SINCE: &str = "2026-11-24";
/// A balloon number: decimal `u32`, at least 1.
const NUMBER_PATTERN: &str = "^[1-9][0-9]{0,9}$";
/// A sheet coordinate in `0.01 mm`: decimal `i64` (range checked by the handler).
const COORD_PATTERN: &str = "^-?(0|[1-9][0-9]{0,18})$";
/// A characteristic ID (`fid_gdt::chars::dprof::CharId`).
const CHAR_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";

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

fn id(description: &str) -> CValue {
    pattern(ID_PATTERN, description)
}

fn object(required: &[&str], properties: CValue) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(required)),
        ("properties", properties),
    ])
}

fn point(description: &str) -> CValue {
    let mut v = object(
        &["x", "y"],
        obj([
            ("x", pattern(COORD_PATTERN, "x in 0.01 mm from the sheet origin.")),
            ("y", pattern(COORD_PATTERN, "y in 0.01 mm from the sheet origin.")),
        ]),
    );
    if let CValue::Obj(o) = &mut v {
        o.insert("description".to_string(), text(description));
    }
    v
}

fn nullable_point(description: &str) -> CValue {
    obj([("anyOf", CValue::Arr(vec![point(description), obj([("type", text("null"))])])), ("default", CValue::Null)])
}

fn untrusted_zone() -> CValue {
    let zone = object(
        &["text", "untrusted"],
        obj([
            ("text", obj([("type", text("string")), ("x-untrusted", CValue::Bool(true))])),
            ("untrusted", obj([("const", CValue::Bool(true))])),
        ]),
    );
    obj([("anyOf", CValue::Arr(vec![zone, obj([("type", text("null"))])]))])
}

fn balloon_schema() -> CValue {
    object(
        &["anchor", "characteristic", "drawing_rev", "id", "label", "number", "sheet", "zone"],
        obj([
            ("anchor", point("Anchor on the callout.")),
            ("characteristic", pattern(CHAR_PATTERN, "Linked characteristic.")),
            ("drawing_rev", id("Drawing revision ID (UUIDv7).")),
            ("id", id("Balloon ID (UUIDv7, never reused).")),
            ("label", point("Label position.")),
            ("number", pattern(NUMBER_PATTERN, "Balloon number, unique per drawing revision.")),
            ("sheet", id("Sheet ID (UUIDv7).")),
            ("zone", untrusted_zone()),
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
        ("fs_effect", text("none")),
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

/// The six command definitions of `RPT.BALLOON.REG`, in order: `report.add_balloon@1`,
/// `report.move_balloon@1`, `report.renumber_balloon@1`, `report.relink_balloon@1`,
/// `report.remove_balloon@1`, `report.list_balloon@1`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let add = def(Spec {
        name: "report.add_balloon",
        title: "Place a balloon on a drawing sheet",
        description: "Places a numbered balloon linked to one characteristic on a drawing revision's sheet; \
                      without a number it takes the highest number in use plus one; mode insert moves numbers \
                      at or above the given one up by one.",
        params: object(
            &["anchor", "characteristic", "drawing_rev", "label", "sheet"],
            obj([
                ("anchor", point("Anchor on the callout, in 0.01 mm sheet coordinates.")),
                ("characteristic", pattern(CHAR_PATTERN, "Characteristic to link.")),
                ("drawing_rev", id("Drawing revision ID (UUIDv7).")),
                ("label", point("Label position, in 0.01 mm sheet coordinates.")),
                ("mode", obj([("type", text("string")), ("enum", strs(&["set", "insert"])), ("default", text("set"))])),
                ("number", nullable(NUMBER_PATTERN, "Explicit balloon number; mode insert needs one.")),
                ("sheet", id("Sheet ID (UUIDv7) on the drawing revision.")),
                (
                    "zone",
                    obj([
                        ("type", strs(&["string", "null"])),
                        ("maxLength", num(MAX_ZONE_BYTES)),
                        (
                            "description",
                            text(
                                "Drawing zone, at most 16 bytes of UTF-8 (maxLength counts characters; the handler checks bytes).",
                            ),
                        ),
                        ("default", CValue::Null),
                        ("x-untrusted", CValue::Bool(true)),
                    ]),
                ),
            ]),
        ),
        result: balloon_schema(),
        write: true,
    })?;

    let mv = def(Spec {
        name: "report.move_balloon",
        title: "Move a balloon",
        description: "Moves a balloon's anchor or label, or puts it on another sheet of the same drawing revision.",
        params: object(
            &["balloon"],
            obj([
                ("anchor", nullable_point("New anchor.")),
                ("balloon", id("Balloon ID (UUIDv7).")),
                ("label", nullable_point("New label position.")),
                ("sheet", nullable(ID_PATTERN, "New sheet (UUIDv7) on the same drawing revision.")),
            ]),
        ),
        result: balloon_schema(),
        write: true,
    })?;

    let change = object(
        &["balloon", "new", "old"],
        obj([
            ("balloon", id("Balloon ID (UUIDv7).")),
            ("new", pattern(NUMBER_PATTERN, "New number.")),
            ("old", pattern(NUMBER_PATTERN, "Old number.")),
        ]),
    );
    let renumber = def(Spec {
        name: "report.renumber_balloon",
        title: "Renumber balloons",
        description: "Renumbers balloons of a drawing revision: set gives a balloon a free number, insert gives it \
                      a number and moves the others at or above it up by one, compact makes 1..n in current order. \
                      IDs and links never change.",
        params: object(
            &["drawing_rev", "mode"],
            obj([
                ("balloon", nullable(ID_PATTERN, "Balloon to renumber (set and insert).")),
                ("drawing_rev", id("Drawing revision ID (UUIDv7).")),
                ("mode", obj([("type", text("string")), ("enum", strs(&["set", "insert", "compact"]))])),
                ("number", nullable(NUMBER_PATTERN, "Target number (set and insert).")),
            ]),
        ),
        result: object(&["changes"], obj([("changes", obj([("type", text("array")), ("items", change)]))])),
        write: true,
    })?;

    let relink = def(Spec {
        name: "report.relink_balloon",
        title: "Link a balloon to another characteristic",
        description: "Links a balloon to another characteristic that has no balloon on the drawing revision.",
        params: object(
            &["balloon", "characteristic"],
            obj([
                ("balloon", id("Balloon ID (UUIDv7).")),
                ("characteristic", pattern(CHAR_PATTERN, "Characteristic to link.")),
            ]),
        ),
        result: balloon_schema(),
        write: true,
    })?;

    let remove = def(Spec {
        name: "report.remove_balloon",
        title: "Remove a balloon",
        description: "Removes a balloon; its number becomes free and its ID is never reused.",
        params: object(&["balloon"], obj([("balloon", id("Balloon ID (UUIDv7)."))])),
        result: object(&[], obj([])),
        write: true,
    })?;

    let list = def(Spec {
        name: "report.list_balloon",
        title: "List the balloons of a drawing revision",
        description: "Lists a drawing revision's balloons by ascending number.",
        params: object(&["drawing_rev"], obj([("drawing_rev", id("Drawing revision ID (UUIDv7)."))])),
        result: object(&["balloons"], obj([("balloons", obj([("type", text("array")), ("items", balloon_schema())]))])),
        write: false,
    })?;

    Ok(vec![add, mv, renumber, relink, remove, list])
}
