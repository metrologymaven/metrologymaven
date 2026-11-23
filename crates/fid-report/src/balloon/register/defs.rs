//! Registry definitions (`CommandDef`) and JSON `execute` dispatcher for `RPT.BALLOON.REG`.

use super::error::RegisterError;
use super::event::{ADD_BALLOON, LIST_BALLOON, MOVE_BALLOON, RELINK_BALLOON, REMOVE_BALLOON, RENUMBER_BALLOON};
use super::model::{RenumberChange, RenumberMode, SheetPoint, MAX_ZONE_BYTES};
use super::store::{AddBalloonParams, BalloonRegister, MoveBalloonParams, RelinkBalloonParams, RenumberBalloonParams};
use crate::balloon::sheet::{BalloonId, DrawingRevId, SheetId, SheetInfo, UuidV7Gen};
use crate::doc::defs::ID_PATTERN;
use crate::doc::model::obj;
use fid_canon::CValue;
use fid_gdt::chars::dprof::CharId;
use fid_registry::{CommandDef, DefError};
use std::collections::BTreeMap;

const LIFECYCLE_SINCE: &str = "2026-11-22";
const INT_STR_PATTERN: &str = "^-?(0|[1-9][0-9]{0,18})$";
const POS_INT_STR_PATTERN: &str = "^[1-9][0-9]{0,9}$";

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

fn id(desc: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text(ID_PATTERN)), ("description", text(desc))])
}

fn point_schema() -> CValue {
    object(
        &["x", "y"],
        obj([
            ("x", obj([("type", text("string")), ("pattern", text(INT_STR_PATTERN))])),
            ("y", obj([("type", text("string")), ("pattern", text(INT_STR_PATTERN))])),
        ]),
    )
}

fn untrusted_zone_schema() -> CValue {
    let ut = object(
        &["text", "untrusted"],
        obj([
            ("text", obj([("type", text("string")), ("x-untrusted", CValue::Bool(true))])),
            ("untrusted", obj([("const", CValue::Bool(true))])),
        ]),
    );
    obj([("anyOf", CValue::Arr(vec![ut, obj([("type", text("null"))])]))])
}

fn balloon_schema() -> CValue {
    object(
        &["anchor", "characteristic", "drawing_rev", "id", "label", "number", "sheet", "zone"],
        obj([
            ("anchor", point_schema()),
            ("characteristic", obj([("type", text("string")), ("minLength", num(1))])),
            ("drawing_rev", id("Drawing revision ID (UUIDv7).")),
            ("id", id("Balloon ID (UUIDv7).")),
            ("label", point_schema()),
            ("number", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
            ("sheet", id("Sheet ID (UUIDv7).")),
            ("zone", untrusted_zone_schema()),
        ]),
    )
}

fn def(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    params: CValue,
    result: CValue,
    write: bool,
) -> Result<CommandDef, DefError> {
    let event = format!("{name}@1");
    CommandDef::from_value(obj([
        ("id", text(name)),
        ("major", num(1)),
        ("title", text(title)),
        ("description", text(description)),
        ("params_schema", params),
        ("result_schema", result),
        ("side_effect", text(if write { "W2" } else { "R0" })),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text(if write { "none" } else { "natural" })),
        ("fs_effect", text("none")),
        ("undo", text(if write { "head_move" } else { "not_undoable" })),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", if write { strs(&[event.as_str()]) } else { CValue::Arr(Vec::new()) }),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}

/// Returns all 6 registry [`CommandDef`]s for `RPT.BALLOON.REG`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let add = def(
        "report.add_balloon",
        "Add a balloon to a drawing sheet",
        "Places a numbered balloon linked to one characteristic on a drawing revision.",
        object(
            &["anchor", "characteristic", "drawing_rev", "label", "sheet"],
            obj([
                ("anchor", point_schema()),
                ("characteristic", obj([("type", text("string")), ("minLength", num(1))])),
                ("drawing_rev", id("Drawing revision ID (UUIDv7).")),
                ("insert", obj([("type", text("boolean")), ("default", CValue::Bool(false))])),
                ("label", point_schema()),
                (
                    "number",
                    obj([
                        ("type", strs(&["string", "null"])),
                        ("pattern", text(POS_INT_STR_PATTERN)),
                        ("default", CValue::Null),
                    ]),
                ),
                ("sheet", id("Drawing sheet ID (UUIDv7).")),
                (
                    "zone",
                    obj([
                        ("type", strs(&["string", "null"])),
                        ("maxLength", num(MAX_ZONE_BYTES)),
                        ("default", CValue::Null),
                        ("x-untrusted", CValue::Bool(true)),
                    ]),
                ),
            ]),
        ),
        balloon_schema(),
        true,
    )?;

    let mv = def(
        "report.move_balloon",
        "Move a balloon anchor, label or sheet",
        "Updates anchor/label coordinates or sheet within the same drawing revision.",
        object(
            &["balloon"],
            obj([
                ("anchor", obj([("anyOf", CValue::Arr(vec![point_schema(), obj([("type", text("null"))])]))])),
                ("balloon", id("Balloon ID (UUIDv7).")),
                ("label", obj([("anyOf", CValue::Arr(vec![point_schema(), obj([("type", text("null"))])]))])),
                ("sheet", obj([("type", strs(&["string", "null"])), ("pattern", text(ID_PATTERN))])),
            ]),
        ),
        balloon_schema(),
        true,
    )?;

    let set_mode = object(
        &["balloon", "mode", "number"],
        obj([
            ("balloon", id("Balloon ID (UUIDv7).")),
            ("mode", obj([("const", text("set"))])),
            ("number", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
        ]),
    );
    let ins_mode = object(
        &["balloon", "mode", "number"],
        obj([
            ("balloon", id("Balloon ID (UUIDv7).")),
            ("mode", obj([("const", text("insert"))])),
            ("number", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
        ]),
    );
    let cmp_mode = object(&["mode"], obj([("mode", obj([("const", text("compact"))]))]));

    let renumber = def(
        "report.renumber_balloon",
        "Renumber balloons on a drawing revision",
        "Renumbers balloons via set, insert, or compact mode without changing IDs or characteristic links.",
        object(
            &["drawing_rev", "mode"],
            obj([
                ("drawing_rev", id("Drawing revision ID (UUIDv7).")),
                ("mode", obj([("anyOf", CValue::Arr(vec![set_mode, ins_mode, cmp_mode]))])),
            ]),
        ),
        object(
            &["changes"],
            obj([(
                "changes",
                obj([
                    ("type", text("array")),
                    (
                        "items",
                        object(
                            &["balloon", "new", "old"],
                            obj([
                                ("balloon", id("Balloon ID (UUIDv7).")),
                                ("new", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
                                ("old", obj([("type", text("string")), ("pattern", text(POS_INT_STR_PATTERN))])),
                            ]),
                        ),
                    ),
                ]),
            )]),
        ),
        true,
    )?;

    let relink = def(
        "report.relink_balloon",
        "Relink a balloon to another characteristic",
        "Reassigns a balloon to an unballooned characteristic on the same drawing revision.",
        object(
            &["balloon", "characteristic"],
            obj([
                ("balloon", id("Balloon ID (UUIDv7).")),
                ("characteristic", obj([("type", text("string")), ("minLength", num(1))])),
            ]),
        ),
        balloon_schema(),
        true,
    )?;

    let remove = def(
        "report.remove_balloon",
        "Remove a balloon from a drawing revision",
        "Removes a balloon, freeing its number and permanently retiring its ID.",
        object(&["balloon"], obj([("balloon", id("Balloon ID (UUIDv7)."))])),
        object(&["removed"], obj([("removed", CValue::Obj(BTreeMap::from([("type".into(), text("boolean"))])))])),
        true,
    )?;

    let list = def(
        "report.list_balloon",
        "List balloons on a drawing revision",
        "Returns balloons ordered by ascending balloon number.",
        object(&["drawing_rev"], obj([("drawing_rev", id("Drawing revision ID (UUIDv7)."))])),
        object(&["balloons"], obj([("balloons", obj([("type", text("array")), ("items", balloon_schema())]))])),
        false,
    )?;

    Ok(vec![add, mv, renumber, relink, remove, list])
}

/// Dispatches any of the six `RPT.BALLOON.REG` commands via canonical JSON `CValue`.
pub fn execute(
    command: &str,
    params: &CValue,
    reg: &mut BalloonRegister,
    sheets: &dyn SheetInfo,
    ids: &mut dyn UuidV7Gen,
) -> Result<CValue, RegisterError> {
    let CValue::Obj(m) = params else {
        return Err(RegisterError::BadParams("params must be an object"));
    };
    match command {
        ADD_BALLOON => {
            let drawing_rev = DrawingRevId::parse(req_str(m, "drawing_rev")?)
                .map_err(|_| RegisterError::BadParams("invalid drawing_rev"))?;
            let sheet = SheetId::parse(req_str(m, "sheet")?).map_err(|_| RegisterError::BadParams("invalid sheet"))?;
            let characteristic = CharId::parse(req_str(m, "characteristic")?)
                .ok_or(RegisterError::BadParams("invalid characteristic"))?;
            let anchor = SheetPoint::from_value(m.get("anchor").ok_or(RegisterError::BadParams("missing anchor"))?)?;
            let label = SheetPoint::from_value(m.get("label").ok_or(RegisterError::BadParams("missing label"))?)?;
            let number = match m.get("number") {
                Some(CValue::Null) | None => None,
                Some(CValue::Str(s)) => Some(s.parse::<u32>().map_err(|_| RegisterError::BadParams("bad number"))?),
                _ => return Err(RegisterError::BadParams("number must be string or null")),
            };
            let insert = match m.get("insert") {
                Some(CValue::Bool(b)) => *b,
                None => false,
                _ => return Err(RegisterError::BadParams("insert must be boolean")),
            };
            let zone = match m.get("zone") {
                Some(CValue::Null) | None => None,
                Some(CValue::Str(s)) => Some(s.clone()),
                _ => return Err(RegisterError::BadParams("zone must be string or null")),
            };
            let b = reg.add_balloon(
                sheets,
                ids,
                &AddBalloonParams { drawing_rev, sheet, characteristic, anchor, label, number, insert, zone },
            )?;
            Ok(b.to_value())
        }
        MOVE_BALLOON => {
            let balloon =
                BalloonId::parse(req_str(m, "balloon")?).map_err(|_| RegisterError::BadParams("invalid balloon"))?;
            let anchor = match m.get("anchor") {
                Some(CValue::Null) | None => None,
                Some(v) => Some(SheetPoint::from_value(v)?),
            };
            let label = match m.get("label") {
                Some(CValue::Null) | None => None,
                Some(v) => Some(SheetPoint::from_value(v)?),
            };
            let sheet = match m.get("sheet") {
                Some(CValue::Null) | None => None,
                Some(CValue::Str(s)) => Some(SheetId::parse(s).map_err(|_| RegisterError::BadParams("invalid sheet"))?),
                _ => return Err(RegisterError::BadParams("invalid sheet")),
            };
            let b = reg.move_balloon(sheets, &MoveBalloonParams { balloon, anchor, label, sheet })?;
            Ok(b.to_value())
        }
        RENUMBER_BALLOON => {
            let drawing_rev = DrawingRevId::parse(req_str(m, "drawing_rev")?)
                .map_err(|_| RegisterError::BadParams("invalid drawing_rev"))?;
            let mode = RenumberMode::from_value(m.get("mode").ok_or(RegisterError::BadParams("missing mode"))?)?;
            let ch = reg.renumber_balloon(&RenumberBalloonParams { drawing_rev, mode })?;
            Ok(obj([("changes", CValue::Arr(ch.into_iter().map(RenumberChange::to_value).collect()))]))
        }
        RELINK_BALLOON => {
            let balloon =
                BalloonId::parse(req_str(m, "balloon")?).map_err(|_| RegisterError::BadParams("invalid balloon"))?;
            let characteristic = CharId::parse(req_str(m, "characteristic")?)
                .ok_or(RegisterError::BadParams("invalid characteristic"))?;
            let b = reg.relink_balloon(&RelinkBalloonParams { balloon, characteristic })?;
            Ok(b.to_value())
        }
        REMOVE_BALLOON => {
            let balloon =
                BalloonId::parse(req_str(m, "balloon")?).map_err(|_| RegisterError::BadParams("invalid balloon"))?;
            reg.remove_balloon(balloon)?;
            Ok(obj([("removed", CValue::Bool(true))]))
        }
        LIST_BALLOON => {
            let drawing_rev = DrawingRevId::parse(req_str(m, "drawing_rev")?)
                .map_err(|_| RegisterError::BadParams("invalid drawing_rev"))?;
            let list = reg.list(drawing_rev);
            Ok(obj([("balloons", CValue::Arr(list.iter().map(|b| b.to_value()).collect()))]))
        }
        _ => Err(RegisterError::BadParams("unknown balloon register command")),
    }
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, RegisterError> {
    match m.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(RegisterError::BadParams("missing or non-string field")),
    }
}
