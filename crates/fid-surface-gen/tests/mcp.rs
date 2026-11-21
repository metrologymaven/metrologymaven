//! The registered MCP emitter (spec node AUTO.MCP.PROJ) inside the generator: AC-1 and AC-2
//! end to end, and the `JsonMember` header syntax its JSON file uses. Expected bytes come from
//! `fixtures/mcp/make_fixtures.py`, an independent Python reference.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_surface::Projection;
use fid_surface_gen::{
    generate, header, registered, render, CommentSyntax, EmitError, GenError, GeneratedFile, McpEmitter, SurfaceEmitter,
};
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    fs::read(repo_root().join(rel)).unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("surface-gen-mcp").join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.parent().unwrap()).unwrap();
    d
}

#[test]
fn mcp_emitter_is_registered() {
    let surfaces: Vec<&str> = registered().iter().map(|e| e.surface()).collect();
    assert_eq!(surfaces, ["mcp"]);
}

/// AC-1: `mcp/mcp-tools.json` for min-classes equals the golden, byte for byte.
#[test]
fn ac1_generated_tool_list_equals_the_golden() {
    let out = tmp("ac1");
    generate(&read("fixtures/manifest/min-classes.json"), &[&McpEmitter], &out).unwrap();
    assert_eq!(fs::read(out.join("mcp").join("mcp-tools.json")).unwrap(), read("fixtures/mcp/min-classes.tools.json"));
    let index = fs::read_to_string(out.join("mcp").join("surface-index.json")).unwrap();
    for absent in ["motion.move_axis@1", "trust.sign_record@1", "plugin.install_pack@1"] {
        assert!(!index.contains(absent), "{absent} is in the MCP index");
    }
    assert!(index.contains("rpt.release_report@1"));
}

/// AC-2: the motion request fails generation, naming the command, and writes nothing.
#[test]
fn ac2_motion_request_fails_generation_and_writes_nothing() {
    let out = tmp("ac2");
    let e = generate(&read("fixtures/manifest/motion-request.json"), &[&McpEmitter], &out).unwrap_err();
    match &e {
        GenError::EmitFailed { surface, detail } => {
            assert_eq!(surface, "mcp");
            assert!(detail.starts_with("motion_tool_forbidden: cmm.request_run@1"), "{detail}");
        }
        other => panic!("{other}"),
    }
    assert!(!out.exists());
}

struct Json(&'static [u8]);

impl SurfaceEmitter for Json {
    fn surface(&self) -> &'static str {
        "json"
    }
    fn emit(&self, _p: &Projection) -> Result<Vec<GeneratedFile>, EmitError> {
        Ok(vec![GeneratedFile { rel_path: "x.json".into(), bytes: self.0.to_vec(), covers: Vec::new() }])
    }
    fn comment_syntax(&self, _path: &str) -> CommentSyntax {
        CommentSyntax::JsonMember
    }
}

#[test]
fn json_member_header() {
    let p = fid_surface::project(&read("fixtures/manifest/min-classes.json")).unwrap();
    let h = header(&p, CommentSyntax::JsonMember);
    let want = format!(
        "{{\"_header\":\"generated, do not edit; manifest {}; generator fid-surface-gen 1\",",
        p.manifest_digest.as_str()
    );
    assert_eq!(h, want);
    let file = |b: &'static [u8]| render(&p, &[&Json(b)]).map(|o| o[0].files["x.json"].clone());
    let one = file(b"{\"a\":1}").unwrap();
    assert_eq!(one, format!("{}\"a\":1}}\n", want).into_bytes());
    let empty = file(b"{}").unwrap();
    assert_eq!(empty, format!("{}}}\n", want.trim_end_matches(',')).into_bytes());
    for (n, s) in [&one, &empty].iter().enumerate() {
        assert!(fid_canon::parse(s).is_ok(), "case {n} is not JSON");
    }
    for bad in [&b"[1]"[..], b"not json", b"{\"_header\":\"x\"}", b"{} {}"] {
        let e = render(&p, &[&Json(bad)]).unwrap_err();
        assert_eq!(e.code(), "emit_failed", "{bad:?}");
    }
}
