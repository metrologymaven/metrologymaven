//! CORE.REG.DEF.MACRO: AC-1 (demo_ok schemas equal the golden files), AC-3 (byte-identical
//! across builds and calls), REQ-003/-006 (declarations from the handler's own types, in
//! source order), and the hand-written `registry.show_manifest@1` reproduced byte for byte.
//! AC-2 is `decl_compile_fail.rs`; AC-4 is in `fid-registry-macros`' unit tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(dead_code)]
#[path = "../../../fixtures/core/reg/def/macro/demo_ok/lib.rs"]
mod demo_ok;

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_registry::decl::{CommandDecl, FidSchema};
use fid_registry::manifest::{ManifestError, MAX_MANIFEST_BYTES};
use fid_registry::{
    check_schema, fid_command, validate, Catalogue, CommandDef, Determinism, FsEffect, Idempotency, Network,
    Projection, SideEffect, Surface, Undo, Web,
};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn golden(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/reg/def/macro/golden").join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let text = String::from_utf8(bytes).expect("utf-8");
    // The files are canonical JSON plus one newline (make_golden.py).
    text.strip_suffix('\n').map(|t| t.strip_suffix('\r').unwrap_or(t)).expect("trailing newline").as_bytes().to_vec()
}

fn canonical(v: &CValue) -> Vec<u8> {
    to_canonical_bytes(v).expect("canonical")
}

#[test]
fn ac1_schemas_equal_golden_files() {
    let decls = demo_ok::declarations();
    let ids: Vec<String> = decls.iter().map(|d| format!("{}@{}", d.def.id, d.def.major)).collect();
    // REQ-006: source order of the fid_commands! list.
    assert_eq!(ids, ["demo.status@1", "demo.offset_plane@1", "demo.set_telemetry@1"]);
    for d in &decls {
        let id = format!("{}@{}", d.def.id, d.def.major);
        for (kind, schema) in [("params", &d.def.params_schema), ("result", &d.def.result_schema)] {
            let want = golden(&format!("{id}.{kind}.json"));
            assert_eq!(String::from_utf8(canonical(schema)).unwrap(), String::from_utf8(want.clone()).unwrap());
            assert_eq!(&parse(&want).expect("golden parses"), schema, "{id} {kind}");
        }
    }
}

#[test]
fn req003_schemas_are_the_handler_types() {
    let decls = demo_ok::declarations();
    let offset = &decls[1].def;
    assert_eq!(offset.params_schema, demo_ok::OffsetParams::schema());
    assert_eq!(offset.result_schema, demo_ok::OffsetResult::schema());
    assert_eq!(decls[0].def.params_schema, demo_ok::NoParams::schema());
    assert_eq!(decls[2].def.params_schema, demo_ok::TelemetryParams::schema());
}

#[test]
fn ac1_policy_fields_come_from_the_attribute() {
    let decls = demo_ok::declarations();
    let [status, offset, telemetry] = &decls[..] else { panic!("three commands") };
    let s = &status.def;
    assert_eq!(
        (s.side_effect, s.mcp_projection, s.determinism),
        (SideEffect::R0, Projection::Exposed, Determinism::D0)
    );
    assert_eq!(
        (s.idempotency, s.fs_effect, s.undo, s.web),
        (Idempotency::Natural, FsEffect::None, Undo::NotUndoable, Web::Available)
    );
    assert!(s.cli_bare && !s.human_only && s.network == Network::None && s.web_note.is_none());
    assert_eq!(s.exposure, BTreeSet::from([Surface::Cli, Surface::Mcp]));
    assert_eq!(s.lifecycle.get("since"), Some(&CValue::Str("2026-11-19".into())));

    let o = &offset.def;
    assert_eq!(
        (o.side_effect, o.mcp_projection, o.idempotency, o.undo),
        (SideEffect::W2, Projection::RequestOnly, Idempotency::Keyed, Undo::HeadMove)
    );
    assert_eq!(o.preconditions.len(), 1);
    assert_eq!((o.preconditions[0].bound, o.preconditions[0].message_key.as_str()), (100, "demo.too_many_tags"));
    assert_eq!(o.effects, ["demo.plane_offset@1"]);
    assert_eq!(o.algorithms.iter().map(ToString::to_string).collect::<Vec<_>>(), ["geo.plane.offset@1.0.0"]);
    assert_eq!((o.web, o.web_note.as_deref()), (Web::Partial, Some("Preview only in the web build.")));

    let t = &telemetry.def;
    assert_eq!((t.side_effect, t.mcp_projection, t.human_only), (SideEffect::A0, Projection::Never, true));
    assert_eq!(t.web, Web::Unavailable);
}

#[test]
fn generated_definitions_pass_def_and_val() {
    let decls = demo_ok::declarations();
    let catalogue = Catalogue::geo_frame().expect("GEO.FRAME.UNITS catalogue");
    for d in &decls {
        // The direct construction agrees with CORE.REG.DEF.FIELD's parser.
        assert_eq!(CommandDef::from_value(d.def.to_value()).as_ref(), Ok(&d.def));
        for schema in [&d.def.params_schema, &d.def.result_schema] {
            assert_eq!(check_schema(schema, &catalogue), Vec::new(), "{}", d.def.id);
        }
    }
    let raw: Vec<_> = decls.iter().map(CommandDecl::to_validation).collect();
    assert_eq!(validate(&raw), Vec::new());
}

#[test]
fn ac3_generated_schemas_are_byte_identical() {
    let bytes = |decls: Vec<CommandDecl>| -> Vec<Vec<u8>> {
        decls.iter().flat_map(|d| [canonical(&d.def.to_value()), canonical(&d.def.params_schema)]).collect()
    };
    assert_eq!(bytes(demo_ok::declarations()), bytes(demo_ok::declarations()));
}

#[test]
fn handler_ref_recovers_the_typed_handler() {
    type Offset = fn(&demo_ok::Ctx, demo_ok::OffsetParams) -> Result<demo_ok::OffsetResult, demo_ok::DemoError>;
    let decls = demo_ok::declarations();
    let h = &decls[1].handler;
    assert_eq!(h.path(), "offset_plane");
    let f = h.get::<Offset>().expect("typed handler");
    let params =
        demo_ok::OffsetParams { feature: "ftr_0000000a".into(), distance: 2.5, tags: Vec::new(), dry_run: false };
    assert_eq!(f(&demo_ok::Ctx, params).map(|r| (r.revision, r.applied)), Ok((1, 2.5)));
    // The wrong type is refused, not reinterpreted.
    assert!(h.get::<fn()>().is_none());
    assert!(decls[0].handler.get::<Offset>().is_none());
}

// ------------------------------------------------------------ registry.show_manifest@1

/// `registry.show_manifest@1` parameters, as the macro would declare them.
#[derive(FidSchema)]
pub struct ShowManifestParams {
    #[fid(default = false, description = "Also return the manifest bytes.")]
    pub full: bool,
    #[fid(
        max_length = MAX_MANIFEST_BYTES,
        description = "The text of a manifest to verify instead of showing the one in force; at most 67108864 characters (maxLength), and refused as manifest_invalid above 67108864 bytes of UTF-8."
    )]
    pub check: Option<String>,
}

/// `registry.show_manifest@1` result.
#[derive(FidSchema)]
pub struct ShowManifestResult {
    #[fid(pattern = "^sha256:[0-9a-f]{64}$")]
    pub digest: String,
    pub bytes: Option<String>,
}

#[fid_command(
    id = "registry.show_manifest",
    major = 1,
    title = "Show manifest",
    description = "Returns the digest of the registry manifest in force and, with full, its canonical bytes; with check, verifies a supplied manifest instead.",
    side_effect = R0,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = Natural,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-19"),
    web = Available,
)]
fn show_manifest(_ctx: (), _p: ShowManifestParams) -> Result<ShowManifestResult, ManifestError> {
    Err(ManifestError::UnknownCommand)
}

#[test]
fn macro_reproduces_hand_written_show_manifest_byte_for_byte() {
    let hand = fid_registry::manifest::command_def().expect("hand-written definition");
    let generated = __fid_command_show_manifest().def;
    assert_eq!(canonical(&generated.params_schema), canonical(&hand.params_schema));
    assert_eq!(canonical(&generated.result_schema), canonical(&hand.result_schema));
    assert_eq!(canonical(&generated.to_value()), canonical(&hand.to_value()));
    assert_eq!(generated, hand);
}

// ------------------------------------------------------------ type mapping

#[allow(dead_code)]
#[derive(FidSchema)]
struct Mapping {
    small: u8,
    signed: i32,
    #[fid(quantity = "length", unit = "mm")]
    lengths: Vec<f64>,
    #[fid(dimensionless_unchecked)]
    ratio: f64,
    set: BTreeSet<String>,
    #[fid(min_items = 1, untrusted = true, rename = "text")]
    texts: Vec<String>,
    r#type: Option<u16>,
}

#[test]
fn rust_types_map_to_json_types() {
    let CValue::Obj(o) = Mapping::schema() else { panic!("object") };
    let CValue::Obj(p) = &o["properties"] else { panic!("properties") };
    let text = |v: &CValue| String::from_utf8(canonical(v)).unwrap();
    assert_eq!(text(&p["small"]), r#"{"maximum":255,"minimum":0,"type":"integer"}"#);
    assert_eq!(text(&p["signed"]), r#"{"maximum":2147483647,"minimum":-2147483648,"type":"integer"}"#);
    assert_eq!(
        text(&p["lengths"]),
        r#"{"items":{"type":"number","x-quantity":"length","x-unit":"mm"},"type":"array"}"#
    );
    assert_eq!(text(&p["ratio"]), r#"{"type":"number"}"#);
    assert_eq!(text(&p["set"]), r#"{"items":{"type":"string"},"type":"array","uniqueItems":true}"#);
    assert_eq!(text(&p["text"]), r#"{"items":{"type":"string","x-untrusted":true},"minItems":1,"type":"array"}"#);
    assert_eq!(text(&p["type"]), r#"{"maximum":65535,"minimum":0,"type":"integer"}"#);
    assert_eq!(text(&o["required"]), r#"["lengths","ratio","set","signed","small","text"]"#);
}
