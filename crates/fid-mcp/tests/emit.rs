//! AUTO.MCP.PROJ AC-1, AC-2 and AC-5, and the pinned schema, text and motion rules, against
//! fixtures written by the independent reference `fixtures/mcp/make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_mcp::{emit, tools, McpError, ToolKind, REQUEST_ONLY_SENTENCE};
use fid_surface::{project, Projection};
use fid_types::SideEffect;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn projection(rel: &str) -> Projection {
    project(&read(rel)).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("no {k}")),
        other => panic!("not an object: {other:?}"),
    }
}

fn st<'a>(v: &'a CValue, k: &str) -> &'a str {
    match get(v, k) {
        CValue::Str(s) => s,
        other => panic!("{k}: {other:?}"),
    }
}

/// AC-1: the tool list for min-classes equals the golden file (its `_header` member is the
/// generator's) and lists every exposable class and none of M1, S1, A0.
#[test]
fn ac1_min_classes_matches_golden() {
    let p = projection("fixtures/manifest/min-classes.json");
    let out = emit(&p).unwrap();
    let golden = read("fixtures/mcp/min-classes.tools.json");
    let CValue::Obj(mut g) = parse(&golden).unwrap() else { panic!("golden is not an object") };
    let header = g.remove("_header").unwrap();
    assert_eq!(
        header,
        CValue::Str(format!(
            "generated, do not edit; manifest {}; generator fid-surface-gen 1",
            p.manifest_digest.as_str()
        ))
    );
    assert_eq!(out.bytes, to_canonical_bytes(&CValue::Obj(g)).unwrap(), "mcp-tools.json differs from the golden");
    assert_eq!(emit(&p).unwrap(), out, "two runs differ");

    let classes: BTreeSet<SideEffect> =
        out.tools.iter().map(|t| p.commands.iter().find(|c| c.command_id == t.command).unwrap().side_effect).collect();
    use SideEffect as C;
    let want: BTreeSet<SideEffect> = [C::R0, C::R1, C::I1, C::W1, C::W2, C::W3, C::X1, C::D1].into_iter().collect();
    assert_eq!(classes, want);
    assert_eq!(out.tools.len(), 10);
    for t in &out.tools {
        let c = p.commands.iter().find(|c| c.command_id == t.command).unwrap();
        assert_eq!(t.name, c.mcp_name, "tool name is the derived name");
        assert!(!matches!(c.side_effect, C::M1 | C::S1 | C::A0));
    }
    assert_eq!(out.commands, out.tools.iter().map(|t| t.command.clone()).collect::<Vec<_>>());
}

/// AC-2: a request-only command with a run-ticket effect fails generation, naming it.
#[test]
fn ac2_motion_request_is_forbidden() {
    let p = projection("fixtures/manifest/motion-request.json");
    let e = emit(&p).unwrap_err();
    assert_eq!(e.code(), "motion_tool_forbidden");
    assert_eq!(e.command(), Some("cmm.request_run@1"));
    assert!(e.to_string().contains("cmm.request_run@1"), "{e}");
    assert!(matches!(e, McpError::MotionToolForbidden { ref effect, .. } if effect == "run_ticket.create"));
}

/// AC-5 and REQ-004: every request-only tool ends with the fixed sentence; no direct tool has it.
#[test]
fn ac5_request_only_sentence() {
    let p = projection("fixtures/manifest/min-classes.json");
    let ts = tools(&p).unwrap();
    let mut request_only = 0;
    for t in &ts {
        let c = p.commands.iter().find(|c| c.command_id == t.command).unwrap();
        match t.kind {
            ToolKind::RequestOnly => {
                request_only += 1;
                assert!(t.annotations.request_only);
                assert_eq!(t.description, format!("{} {REQUEST_ONLY_SENTENCE}", c.help.description));
            }
            ToolKind::Direct => {
                assert!(!t.annotations.request_only);
                assert_eq!(t.description, c.help.description);
                assert!(!t.description.contains(REQUEST_ONLY_SENTENCE));
            }
        }
        // REQ-006: title and description come from the registry help text only.
        assert_eq!(t.title, c.help.title);
    }
    // W3, X1, D1 and the human_only W2 adoption command.
    assert_eq!(request_only, 4);
}

/// Every generation case of the reference: the tool list (name and kind), or the error code
/// and the command it names.
#[test]
fn reference_cases() {
    let cases = parse(&read("fixtures/mcp/cases.json")).unwrap();
    let CValue::Arr(cases) = cases else { panic!("cases must be an array") };
    assert!(cases.len() >= 16);
    for case in &cases {
        let name = st(case, "name");
        let p = project(st(case, "manifest").as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let expect = get(case, "expect");
        match emit(&p) {
            Ok(out) => {
                let CValue::Arr(want) = get(expect, "ok") else { panic!("{name}: expected an error") };
                let got: Vec<CValue> = out
                    .tools
                    .iter()
                    .map(|t| {
                        let kind = match t.kind {
                            ToolKind::Direct => "direct",
                            ToolKind::RequestOnly => "request_only",
                        };
                        CValue::Arr(vec![CValue::Str(t.name.clone()), CValue::Str(kind.to_string())])
                    })
                    .collect();
                assert_eq!(&got, want, "{name}");
            }
            Err(e) => {
                assert_eq!(e.code(), st(expect, "error"), "{name}: {e}");
                assert_eq!(e.command(), Some(st(expect, "command")), "{name}");
            }
        }
    }
}

/// The text check names the code point; the bidi override is refused, not stripped.
#[test]
fn forbidden_character_names_the_code_point() {
    let cases = parse(&read("fixtures/mcp/cases.json")).unwrap();
    let CValue::Arr(cases) = cases else { panic!() };
    let bidi = cases.iter().find(|c| st(c, "name") == "text-bidi").unwrap();
    let e = emit(&project(st(bidi, "manifest").as_bytes()).unwrap()).unwrap_err();
    assert_eq!(e, McpError::ForbiddenCharacter { command: "demo.show_a@1".into(), code_point: 0x202E });
}

/// Input and output schemas of the W2 command with quantities, a union, a reference and
/// defaults (min-classes `align.set_datum@1`).
#[test]
fn schema_shapes() {
    let p = projection("fixtures/manifest/min-classes.json");
    let ts = tools(&p).unwrap();
    let t = ts.iter().find(|t| t.name == "align_set_datum").unwrap();
    let props = get(&t.input_schema, "properties");
    let q = get(props, "offset");
    assert_eq!(st(q, "type"), "object");
    assert_eq!(st(q, "x-quantity"), "length");
    assert_eq!(st(q, "x-unit"), "mm");
    assert_eq!(get(q, "additionalProperties"), &CValue::Bool(false));
    assert!(matches!(q, CValue::Obj(o) if !o.contains_key("default")), "a quantity carries no default");
    assert_eq!(st(get(get(q, "properties"), "value"), "type"), "string");
    assert_eq!(get(get(props, "mode"), "default"), &CValue::Str("exact".into()));
    assert_eq!(get(props, "frame"), &CValue::Obj(Default::default()));
    assert_eq!(get(&t.input_schema, "additionalProperties"), &CValue::Bool(false));
    assert_eq!(
        get(&t.input_schema, "required"),
        &CValue::Arr(vec![CValue::Str("offset".into()), CValue::Str("source".into())])
    );
    // The output is the result envelope with the result typed.
    let out = &t.output_schema;
    assert!(matches!(out, CValue::Obj(o) if !o.contains_key("$id") && !o.contains_key("$schema")));
    assert_eq!(st(get(get(out, "properties"), "result"), "type"), "object");
    assert!(matches!(get(get(out, "properties"), "outcome"), CValue::Obj(_)));
    // Annotations: W2 is direct and destructive, R1 read-only.
    assert!(t.annotations.destructive_hint && !t.annotations.read_only_hint);
    let r1 = ts.iter().find(|t| t.name == "align_run_bestfit").unwrap();
    assert!(r1.annotations.read_only_hint && !r1.annotations.destructive_hint);
}

/// An optional quantity (`number` or `null`) is `{value, unit}` or null.
#[test]
fn optional_quantity() {
    let cases = parse(&read("fixtures/mcp/cases.json")).unwrap();
    let CValue::Arr(cases) = cases else { panic!() };
    let case = cases.iter().find(|c| st(c, "name") == "schema-optional-quantity").unwrap();
    let out = emit(&project(st(case, "manifest").as_bytes()).unwrap()).unwrap();
    let o = get(get(&out.tools[0].input_schema, "properties"), "o");
    assert_eq!(get(o, "type"), &CValue::Arr(vec![CValue::Str("null".into()), CValue::Str("object".into())]));
    assert_eq!(st(o, "x-unit"), "s");
}
