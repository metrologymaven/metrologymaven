//! Registry definitions of the four template commands, built through
//! [`CommandDef::from_value`] so every field rule of the registry applies. CLI names
//! (`fid report save-template`, ...) and MCP names (`report_save_template`, ...) are derived
//! from these IDs by the registry.

use super::command::MAX_FILE_PARAM_BYTES;
use crate::doc::defs::ID_PATTERN;
use crate::doc::model::obj;
use crate::doc::MAX_TEXT_BYTES;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;

const LIFECYCLE_SINCE: &str = "2026-11-21";
/// Pattern of a template version (a decimal string of at least 1).
pub const VERSION_PATTERN: &str = "^[1-9][0-9]{0,9}$";

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

fn version_param() -> CValue {
    obj([
        ("type", strs(&["string", "null"])),
        ("pattern", text(VERSION_PATTERN)),
        ("default", CValue::Null),
        ("description", text("Template version; null or absent for the latest.")),
    ])
}

fn builtin_param() -> CValue {
    obj([
        ("type", text("boolean")),
        ("default", CValue::Bool(false)),
        (
            "description",
            text("true: the built-in version builtin:<id>@<version>; false: the project's <id>@<version>."),
        ),
    ])
}

fn template_ref() -> CValue {
    object(
        &["builtin", "digest", "id", "version"],
        obj([
            ("builtin", obj([("type", text("boolean"))])),
            ("digest", digest()),
            ("id", id("Template ID (UUIDv7).")),
            ("version", obj([("type", text("string")), ("pattern", text(VERSION_PATTERN))])),
        ]),
    )
}

fn untrusted_text(description: &str) -> CValue {
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

/// The four command definitions, in the order save, apply, show, list.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let template = || id("The template (its template_id, a UUIDv7).");
    let summary = object(
        &["pages", "report_digest", "report_id", "template", "title"],
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
                                (
                                    "block_count",
                                    obj([("type", text("string")), ("pattern", text("^(0|[1-9][0-9]{0,9})$"))]),
                                ),
                                ("page_id", id("Page ID (UUIDv7).")),
                            ]),
                        ),
                    ),
                ]),
            ),
            ("report_digest", digest()),
            ("report_id", id("Report ID (UUIDv7).")),
            ("template", template_ref()),
            ("title", untrusted_text("Report title.")),
        ]),
    );
    [
        Spec {
            name: "report.save_template",
            title: "Save template",
            description: "Saves a report, or a template file named by an import-root handle, as the next version of a \
                          template ID in the content-addressed store. Identical bytes return the latest version and \
                          journal nothing; locked built-ins fail with report.template_locked and a concurrent save \
                          with conflict/head_moved. Give exactly one of from_report and file.",
            params: object(
                &["template"],
                obj([
                    ("template", template()),
                    ("from_report", id("The report to save as a template.")),
                    (
                        "file",
                        obj([
                            ("type", text("string")),
                            ("path_like", CValue::Bool(true)),
                            ("x-extensions", strs(&["json"])),
                            ("minLength", num(1)),
                            ("maxLength", num(MAX_FILE_PARAM_BYTES)),
                            (
                                "description",
                                text("A template file: a root:// handle in an import root (a human may give a raw path)."),
                            ),
                        ]),
                    ),
                ]),
            ),
            result: object(
                &["created", "template"],
                obj([
                    ("created", obj([("type", text("boolean"))])),
                    ("template", template_ref()),
                ]),
            ),
            write: true,
            reads_file: true,
        },
        Spec {
            name: "report.apply_template",
            title: "Apply template",
            description: "Creates a report from a template version; the report records the template ID, version and \
                          digest, and its blocks keep the template's block IDs and so its slots.",
            params: object(
                &["template", "title"],
                obj([
                    ("template", template()),
                    ("version", version_param()),
                    ("builtin", builtin_param()),
                    (
                        "title",
                        obj([
                            ("type", text("string")),
                            ("maxLength", num(MAX_TEXT_BYTES)),
                            ("x-untrusted", CValue::Bool(true)),
                            ("description", text("Report title; at most 4096 bytes of UTF-8, no NUL.")),
                        ]),
                    ),
                ]),
            ),
            result: summary,
            write: true,
            reads_file: false,
        },
        Spec {
            name: "report.show_template",
            title: "Show template",
            description: "Returns one template version's canonical document and its reference.",
            params: object(
                &["template"],
                obj([("template", template()), ("version", version_param()), ("builtin", builtin_param())]),
            ),
            result: object(
                &["document", "template"],
                obj([
                    (
                        "document",
                        obj([
                            ("type", text("object")),
                            ("description", text("The canonical template document; its strings are untrusted.")),
                        ]),
                    ),
                    ("template", template_ref()),
                ]),
            ),
            write: false,
            reads_file: false,
        },
        Spec {
            name: "report.list_template",
            title: "List templates",
            description: "Lists built-in and project template versions by ID, built-ins first, then version.",
            params: object(&[], obj([])),
            result: object(
                &["templates"],
                obj([(
                    "templates",
                    obj([
                        ("type", text("array")),
                        (
                            "items",
                            object(
                                &["builtin", "digest", "id", "locked", "version"],
                                obj([
                                    ("builtin", obj([("type", text("boolean"))])),
                                    ("digest", digest()),
                                    ("id", id("Template ID (UUIDv7).")),
                                    ("locked", obj([("type", text("boolean"))])),
                                    ("version", obj([("type", text("string")), ("pattern", text(VERSION_PATTERN))])),
                                ]),
                            ),
                        ),
                    ]),
                )]),
            ),
            write: false,
            reads_file: false,
        },
    ]
    .into_iter()
    .map(def)
    .collect()
}
