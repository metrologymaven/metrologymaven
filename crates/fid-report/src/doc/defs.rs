//! Registry definitions of the six report commands, built through
//! [`CommandDef::from_value`] so every field rule of the registry applies. CLI names
//! (`fid report create-report`, ...) and MCP names (`report_create_report`, ...) are
//! derived from these IDs by the registry.

use super::model::{obj, MAX_PAYLOAD_DEPTH};
use super::text::MAX_TEXT_BYTES;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;

const LIFECYCLE_SINCE: &str = "2026-11-19";
/// Pattern of a canonical ID in the published schemas.
pub const ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";
/// Pattern of a block kind ID.
pub const KIND_PATTERN: &str = "^[a-z][a-z0-9_]{0,31}$";

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

fn nullable_id(description: &str) -> CValue {
    obj([
        ("type", strs(&["string", "null"])),
        ("pattern", text(ID_PATTERN)),
        ("default", CValue::Null),
        ("description", text(description)),
    ])
}

fn decimal(description: &str) -> CValue {
    obj([("type", text("string")), ("pattern", text("^(0|[1-9][0-9]{0,9})$")), ("description", text(description))])
}

/// `{"text": <untrusted string>, "untrusted": true}`.
fn untrusted_text(description: &str) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["text", "untrusted"])),
        ("description", text(description)),
        (
            "properties",
            obj([
                (
                    "text",
                    obj([
                        ("type", text("string")),
                        ("maxLength", num(MAX_TEXT_BYTES)),
                        ("x-untrusted", CValue::Bool(true)),
                    ]),
                ),
                ("untrusted", obj([("const", CValue::Bool(true))])),
            ]),
        ),
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

fn report_schema() -> CValue {
    let payload = obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["untrusted", "value"])),
        ("description", text("Opaque payload, validated by the block kind; all of it is content-derived data.")),
        ("properties", obj([("untrusted", obj([("const", CValue::Bool(true))])), ("value", obj([]))])),
    ]);
    let block = object(
        &["id", "kind", "kind_version", "payload"],
        obj([
            ("id", id("Block ID (UUIDv7).")),
            ("kind", obj([("type", text("string")), ("pattern", text(KIND_PATTERN))])),
            ("kind_version", decimal("Payload schema version of the kind.")),
            ("payload", payload),
        ]),
    );
    let page = object(
        &["blocks", "id"],
        obj([("blocks", obj([("type", text("array")), ("items", block)])), ("id", id("Page ID (UUIDv7)."))]),
    );
    let template = obj([
        ("type", strs(&["object", "null"])),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["id", "version"])),
        ("properties", obj([("id", id("Template ID.")), ("version", decimal("Template version."))])),
    ]);
    object(
        &["id", "pages", "retired_ids", "schema_version", "template_ref", "title"],
        obj([
            ("id", id("Report ID (UUIDv7).")),
            ("pages", obj([("type", text("array")), ("items", page)])),
            (
                "retired_ids",
                obj([
                    ("type", text("array")),
                    ("items", id("A removed page or block ID; never issued again.")),
                    ("description", text("Ascending.")),
                ]),
            ),
            ("schema_version", decimal("Stored schema version, \"1\".")),
            ("template_ref", template),
            ("title", untrusted_text("Report title.")),
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

/// The six command definitions, in the order create, add page, remove page, add block,
/// remove block, show.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let report = || id("The report.");
    let empty = || object(&[], obj([]));
    let summary = object(
        &["pages", "report_digest", "report_id", "title"],
        obj([
            (
                "pages",
                obj([
                    ("type", text("array")),
                    (
                        "items",
                        object(
                            &["block_count", "page_id"],
                            obj([
                                ("block_count", decimal("Blocks on the page.")),
                                ("page_id", id("Page ID (UUIDv7).")),
                            ]),
                        ),
                    ),
                ]),
            ),
            ("report_digest", obj([("type", text("string")), ("pattern", text(Digest::PATTERN))])),
            ("report_id", id("Report ID (UUIDv7).")),
            ("title", untrusted_text("Report title.")),
        ]),
    );
    [
        Spec {
            name: "report.create_report",
            title: "Create report",
            description: "Creates a report with a new UUIDv7 ID and one empty page.",
            params: object(
                &["title", "project"],
                obj([
                    (
                        "title",
                        obj([
                            ("type", text("string")),
                            ("maxLength", num(MAX_TEXT_BYTES)),
                            ("x-untrusted", CValue::Bool(true)),
                            ("description", text("Report title; at most 4096 bytes of UTF-8, no NUL.")),
                        ]),
                    ),
                    ("project", id("The open project.")),
                ]),
            ),
            result: summary,
            write: true,
        },
        Spec {
            name: "report.add_page",
            title: "Add page",
            description: "Inserts a new empty page after the named page, or at the end.",
            params: object(&["report"], obj([("report", report()), ("after", nullable_id("Page to insert after."))])),
            result: object(&["page_id"], obj([("page_id", id("The new page (UUIDv7)."))])),
            write: true,
        },
        Spec {
            name: "report.remove_page",
            title: "Remove page",
            description: "Removes a page; fails with report.page_not_empty if it holds blocks, unless force is set.",
            params: object(
                &["report", "page"],
                obj([
                    ("report", report()),
                    ("page", id("The page to remove.")),
                    ("force", obj([("type", text("boolean")), ("default", CValue::Bool(false))])),
                ]),
            ),
            result: empty(),
            write: true,
        },
        Spec {
            name: "report.add_block",
            title: "Add block",
            description: "Appends a block to a page, or inserts it after a sibling block.",
            params: object(
                &["report", "page", "kind", "payload"],
                obj([
                    ("report", report()),
                    ("page", id("The page.")),
                    ("after", nullable_id("Sibling block to insert after.")),
                    ("kind", obj([("type", text("string")), ("pattern", text(KIND_PATTERN))])),
                    (
                        "payload",
                        obj([(
                            "description",
                            text(&format!(
                                "Kind-specific payload, at most 65536 canonical bytes and {MAX_PAYLOAD_DEPTH} levels."
                            )),
                        )]),
                    ),
                ]),
            ),
            result: object(&["block_id"], obj([("block_id", id("The new block (UUIDv7)."))])),
            write: true,
        },
        Spec {
            name: "report.remove_block",
            title: "Remove block",
            description: "Removes a block; its ID is never issued again in the report.",
            params: object(&["report", "block"], obj([("report", report()), ("block", id("The block to remove."))])),
            result: empty(),
            write: true,
        },
        Spec {
            name: "report.show_report",
            title: "Show report",
            description: "Returns the stored report: pages, blocks, IDs and untrusted text.",
            params: object(&["report"], obj([("report", report())])),
            result: report_schema(),
            write: false,
        },
    ]
    .into_iter()
    .map(def)
    .collect()
}
