//! Registry definitions of the two page-setup commands (`RPT.DESIGN.PAGE`), validated
//! through [`CommandDef::from_value`]. The CLI (`fid report set-page-layout`,
//! `fid report show-page-layout`) and MCP names (`report_set_page_layout`,
//! `report_show_page_layout`) are derived from them by the registry.
//!
//! Lengths are plain strings with a unit suffix, not `x-quantity` literals: the handler
//! converts them to exact `0.01 mm` integers and refuses inexact text with
//! `report.layout_invalid` naming the field (REQ-003, REQ-004), so the schema only bounds
//! their syntax.

use crate::doc::defs::ID_PATTERN;
use crate::doc::model::obj;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};

const LIFECYCLE_SINCE: &str = "2026-11-25";
/// A length as typed: decimal digits, an optional fraction, unit `mm` or `in`.
const LENGTH: &str = "[0-9]{1,16}([.][0-9]{1,32})?(mm|in)";
/// An integer count of `0.01 mm` as a decimal string.
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

fn units(description: &str) -> CValue {
    pattern(UNITS_PATTERN, description)
}

fn length_pattern() -> String {
    format!("^{LENGTH}$")
}

fn margins_pattern() -> String {
    format!("^{LENGTH},{LENGTH},{LENGTH},{LENGTH}$")
}

/// The effective layout keys shared by both results ([`super::PageLayout::effective_map`]).
fn effective_properties(extra: [(&str, CValue); 2]) -> CValue {
    let rect = object(
        &["height", "width", "x", "y"],
        obj([
            ("height", units("Body height.")),
            ("width", units("Body width.")),
            ("x", units("Body left edge from the page's left edge.")),
            ("y", units("Body top edge from the page's top edge.")),
        ]),
    );
    let margins = object(
        &["bottom", "left", "right", "top"],
        obj([
            ("bottom", units("Bottom margin.")),
            ("left", units("Left margin.")),
            ("right", units("Right margin.")),
            ("top", units("Top margin.")),
        ]),
    );
    let mut props = match obj([
        ("body", rect),
        ("footer", units("Footer area height, in 0.01 mm.")),
        ("header", units("Header area height, in 0.01 mm.")),
        ("margins", margins),
        ("orientation", obj([("type", text("string")), ("enum", strs(&["portrait", "landscape"]))])),
        ("page_height", units("Page height after orientation, in 0.01 mm.")),
        ("page_width", units("Page width after orientation, in 0.01 mm.")),
        ("size", obj([("type", text("string")), ("enum", strs(&["A4", "A3", "Letter", "custom"]))])),
        ("size_height", units("Size height before orientation, in 0.01 mm.")),
        ("size_width", units("Size width before orientation, in 0.01 mm.")),
    ]) {
        CValue::Obj(m) => m,
        _ => Default::default(),
    };
    for (k, v) in extra {
        props.insert(k.to_string(), v);
    }
    CValue::Obj(props)
}

const EFFECTIVE_KEYS: [&str; 10] = [
    "body",
    "footer",
    "header",
    "margins",
    "orientation",
    "page_height",
    "page_width",
    "size",
    "size_height",
    "size_width",
];

fn effective_object(extra: [(&str, CValue); 2]) -> CValue {
    let mut required: Vec<&str> = EFFECTIVE_KEYS.to_vec();
    required.extend(extra.iter().map(|(k, _)| *k));
    required.sort_unstable();
    object(&required, effective_properties(extra))
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

/// The two command definitions of `RPT.DESIGN.PAGE`, in order: `report.set_page_layout@1`,
/// `report.show_page_layout@1`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let length = length_pattern();
    let set = def(Spec {
        name: "report.set_page_layout",
        title: "Set the page layout of a report",
        description: "Sets page size (A4, A3, Letter or custom), orientation, margins (top,right,bottom,left) and \
                      header and footer area heights, for the report default or, with page, one page's override. \
                      Lengths take mm or in and must be exact multiples of 0.01 mm; parameters left out keep the \
                      value in effect. The body must stay at least 20 mm wide and high.",
        params: object(
            &["report", "size"],
            obj([
                ("footer", nullable(&length, "Footer area height, e.g. 10mm.")),
                ("header", nullable(&length, "Header area height, e.g. 12mm.")),
                ("height", nullable(&length, "Custom page height (size custom only), 50 mm to 2000 mm.")),
                ("margins", nullable(&margins_pattern(), "Margins top,right,bottom,left, e.g. 15mm,15mm,15mm,15mm.")),
                (
                    "orientation",
                    nullable("^(portrait|landscape)$", "portrait or landscape (landscape swaps the size)."),
                ),
                ("page", nullable(ID_PATTERN, "Page ID (UUIDv7) to override; omit for the report default.")),
                ("report", pattern(ID_PATTERN, "Report ID (UUIDv7).")),
                ("size", obj([("type", text("string")), ("enum", strs(&["A4", "A3", "Letter", "custom"]))])),
                ("width", nullable(&length, "Custom page width (size custom only), 50 mm to 2000 mm.")),
            ]),
        ),
        result: effective_object([
            ("page", nullable(ID_PATTERN, "The overridden page, or null for the report default.")),
            ("report", pattern(ID_PATTERN, "Report ID (UUIDv7).")),
        ]),
        write: true,
    })?;

    let page = effective_object([
        ("page_id", pattern(ID_PATTERN, "Page ID (UUIDv7).")),
        ("source", obj([("type", text("string")), ("enum", strs(&["default", "override"]))])),
    ]);
    let show = def(Spec {
        name: "report.show_page_layout",
        title: "Show the page layout of every page",
        description: "Shows, for every page of a report in order, the effective page size, orientation, margins, \
                      header and footer heights and body rectangle, in 0.01 mm units.",
        params: object(&["report"], obj([("report", pattern(ID_PATTERN, "Report ID (UUIDv7)."))])),
        result: object(
            &["pages", "report"],
            obj([
                ("pages", obj([("type", text("array")), ("items", page)])),
                ("report", pattern(ID_PATTERN, "Report ID (UUIDv7).")),
            ]),
        ),
        write: false,
    })?;

    Ok(vec![set, show])
}
