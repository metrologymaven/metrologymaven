//! Rule-level tests for the manifest schema, the WGSL binding check, the Rust readback scan
//! and the toolkit walk. All inputs are inline and every expectation is hand-labelled.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_shlint::deps::{self, is_toolkit};
use fid_shlint::manifest::ReadbackClass;
use fid_shlint::rust_scan::{lex, resolve_relative, scan, scan_shader_sources, strip_cfg_test, Tok};
use std::collections::BTreeMap;
use fid_shlint::{check_shaders, wgsl, BindingClass, Finding, Rule, ShaderManifest, ShlintError};

const MANIFEST: &str = r#"
schema = 1

[[module]]
path = "shaders/a.wgsl"

[[module.binding]]
group = 0
binding = 0
name = "data"
class = "input"

[[module.binding]]
group = 1
binding = 2
name = "ids"
class = "id"

[[target]]
name = "colour_readback"
class = "colour"
"#;

fn manifest() -> ShaderManifest {
    ShaderManifest::parse(MANIFEST).unwrap()
}

fn rules(f: &[Finding]) -> Vec<(String, Rule)> {
    f.iter().map(|f| (f.location.clone(), f.rule)).collect()
}

// ------------------------------------------------------------------ manifest

#[test]
fn manifest_parses() {
    let m = manifest();
    assert_eq!(m.modules.len(), 1);
    assert_eq!(m.modules[0].bindings[1].class, BindingClass::Id);
    assert_eq!(m.targets[0].name, "colour_readback");
    assert_eq!(m.readback_class("ids"), ReadbackClass::Target);
    assert_eq!(m.readback_class("colour_readback"), ReadbackClass::Target);
    assert_eq!(m.readback_class("data"), ReadbackClass::Input);
    assert_eq!(m.readback_class("other"), ReadbackClass::Unknown);
}

#[test]
fn manifest_malformed_cases_fail_closed() {
    let cases: &[(&str, &str)] = &[
        ("", "`schema = 1` is missing"),
        ("schema = 2", "`schema` must be the integer 1"),
        ("schema = 1\nextra = true", "the top level: unknown key `extra`"),
        ("schema = 1\nschema = 1", "duplicate key"),
        ("schema = 1\nmodule = 3", "`module` must be an array of tables"),
        ("schema = 1\n[[module]]\npath = \"../x.wgsl\"", "must not contain empty, `.` or `..` segments"),
        ("schema = 1\n[[module]]\npath = \"/x.wgsl\"", "must be relative"),
        ("schema = 1\n[[module]]\npath = \"shaders\\\\x.wgsl\"", "must be relative"),
        ("schema = 1\n[[module]]\npath = \"x.glsl\"", "must end in .wgsl"),
        ("schema = 1\n[[module]]\npath = \"x.wgsl\"\n[[module]]\npath = \"x.wgsl\"", "listed twice"),
        (
            "schema = 1\n[[module]]\npath = \"x.wgsl\"\n[[module.binding]]\ngroup = -1\nbinding = 0\nname = \"a\"\nclass = \"input\"",
            "`group` must be an integer from 0 to 4294967295",
        ),
        (
            "schema = 1\n[[module]]\npath = \"x.wgsl\"\n[[module.binding]]\ngroup = 0\nbinding = 0\nname = \"a\"\nclass = \"color\"",
            "class `color` is not one of input, colour, depth, id",
        ),
        (
            "schema = 1\n[[module]]\npath = \"x.wgsl\"\n[[module.binding]]\ngroup = 0\nbinding = 0\nname = \"a\"",
            "`class` is missing",
        ),
        (
            "schema = 1\n[[module]]\npath = \"x.wgsl\"\n[[module.binding]]\ngroup = 0\nbinding = 0\nname = \"a b\"\nclass = \"input\"",
            "must contain only ASCII letters",
        ),
        (
            "schema = 1\n[[module]]\npath = \"x.wgsl\"\n[[module.binding]]\ngroup = 0\nbinding = 0\nname = \"a\"\nclass = \"input\"\n\
             [[module.binding]]\ngroup = 0\nbinding = 0\nname = \"b\"\nclass = \"input\"",
            "@group(0) @binding(0) is listed twice",
        ),
        (
            "schema = 1\n[[module]]\npath = \"x.wgsl\"\n[[module.binding]]\ngroup = 0\nbinding = 0\nname = \"a\"\nclass = \"input\"\n\
             [[module.binding]]\ngroup = 0\nbinding = 1\nname = \"a\"\nclass = \"input\"",
            "binding name `a` is listed twice",
        ),
        ("schema = 1\n[[target]]\nname = \"t\"\nclass = \"input\"", "must be colour, depth or id"),
        ("schema = 1\n[[target]]\nname = \"t\"\nclass = \"id\"\n[[target]]\nname = \"t\"\nclass = \"depth\"", "listed twice"),
        ("schema = 1\n[[target]]\nname = \"t\"\nclass = \"id\"\nsize = 4", "unknown key `size`"),
    ];
    for (text, want) in cases {
        match ShaderManifest::parse(text) {
            Ok(m) => panic!("accepted {text:?}: {m:?}"),
            Err(e) => assert!(e.contains(want), "{text:?}: got {e:?}, want {want:?}"),
        }
    }
}

// ------------------------------------------------------------------ WGSL

const WGSL_A: &str = "
@group(0) @binding(0) var<storage, read> data: array<f32>;
@group(1) @binding(2) var ids: texture_storage_2d<r32uint, write>;
@compute @workgroup_size(1)
fn main() {}
";

#[test]
fn wgsl_bindings_are_found() {
    let b = wgsl::bindings(WGSL_A).unwrap();
    let got: Vec<(u32, u32, Option<&str>)> = b.iter().map(|b| (b.group, b.binding, b.name.as_deref())).collect();
    assert_eq!(got, vec![(0, 0, Some("data")), (1, 2, Some("ids"))]);
}

#[test]
fn wgsl_parse_error_fails_closed() {
    let files = ["shaders/a.wgsl".to_string()];
    let sources = [("shaders/a.wgsl".to_string(), "fn main( {\n".to_string())];
    match check_shaders(&manifest(), &files, &sources) {
        Err(ShlintError::WgslParse { path, .. }) => assert_eq!(path, "shaders/a.wgsl"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn listed_module_matching_manifest_is_clean() {
    let files = ["shaders/a.wgsl".to_string()];
    let sources = [("shaders/a.wgsl".to_string(), WGSL_A.to_string())];
    assert!(check_shaders(&manifest(), &files, &sources).unwrap().is_empty());
}

#[test]
fn binding_mismatches_are_findings() {
    let wgsl = "
@group(0) @binding(0) var<storage, read> payload: array<f32>;
@group(2) @binding(0) var<uniform> extra: vec4<f32>;
@compute @workgroup_size(1)
fn main() {}
";
    let files = ["shaders/a.wgsl".to_string(), "shaders/b.wgsl".to_string()];
    let sources = [("shaders/a.wgsl".to_string(), wgsl.to_string())];
    let f = check_shaders(&manifest(), &files, &sources).unwrap();
    assert_eq!(
        rules(&f),
        vec![
            ("shaders/b.wgsl".to_string(), Rule::UnlistedModule),
            ("shaders/a.wgsl".to_string(), Rule::BindingName),
            ("shaders/a.wgsl".to_string(), Rule::UnlistedBinding),
            ("shaders/manifest.toml".to_string(), Rule::StaleBinding),
        ]
    );
    assert_eq!(
        f[1].to_string(),
        "shaders/a.wgsl: [SHLINT-001/binding-name] binding @group(0) @binding(0) is `payload` in WGSL but `data` in shaders/manifest.toml"
    );
    assert_eq!(
        f[2].to_string(),
        "shaders/a.wgsl: [SHLINT-001/unlisted-binding] binding @group(2) @binding(0) `extra` is not listed in shaders/manifest.toml"
    );
    assert_eq!(
        f[3].to_string(),
        "shaders/manifest.toml: [SHLINT-001/stale-binding] module shaders/a.wgsl lists @group(1) @binding(2) `ids`, which the WGSL does not declare"
    );
}

#[test]
fn missing_module_is_a_finding() {
    let f = check_shaders(&manifest(), &[], &[]).unwrap();
    assert_eq!(f.len(), 1);
    assert_eq!(
        f[0].to_string(),
        "shaders/manifest.toml: [SHLINT-001/missing-module] lists module shaders/a.wgsl, which does not exist"
    );
}

// ------------------------------------------------------------------ Rust scan

fn idents(src: &str) -> Vec<String> {
    let toks = strip_cfg_test(&lex(src).unwrap()).unwrap();
    toks.into_iter()
        .filter_map(|t| match t.tok {
            Tok::Ident(s) => Some(s),
            _ => None,
        })
        .collect()
}

#[test]
fn lexer_drops_comments_and_literals() {
    let src = r####"
// map_async
/* a /* nested */ map_async */
let a = "map_async \" still";
let b = r#"map_async "quoted" "#;
let c = br##"map_async"##;
let d = b"map_async";
let e = c"map_async";
let f = 'm';
let g = '\'';
let h = b'\\';
fn k<'x>(v: &'x u8) {}
"####;
    assert_eq!(
        idents(src),
        [
            "let", "a", "let", "b", "let", "c", "let", "d", "let", "e", "let", "f", "let", "g", "let", "h", "fn", "k",
            "x", "v", "x", "u8"
        ]
    );
}

#[test]
fn lexer_counts_lines_through_literals_and_comments() {
    let toks = lex("/* a\nb */ \"x\ny\" r#\"\n\"#\nlast").unwrap();
    let last = toks.last().unwrap();
    assert_eq!(last.tok, Tok::Ident("last".to_string()));
    assert_eq!(last.line, 5);
}

#[test]
fn unterminated_or_unbalanced_source_fails_closed() {
    for src in ["/* open", "\"open", "r#\"open\"", "fn f() {", "fn f() }", "let x = (];", "'"] {
        match scan("src/x.rs", src, &manifest()) {
            Err(ShlintError::RustParse { path, .. }) => assert_eq!(path, "src/x.rs"),
            other => panic!("{src:?}: {other:?}"),
        }
    }
}

#[test]
fn cfg_test_items_are_removed() {
    let src = "
#[cfg(test)]
mod tests { fn t() { data.map_async(MapMode::Read, cb); } }
#[cfg(test)]
#[allow(dead_code)]
fn helper<A, B>() { data.map_async(MapMode::Read, cb); }
#[cfg(test)]
use std::{a, b};
struct S {
    #[cfg(test)]
    probe: u8,
    keep: u8,
}
fn real() {}
";
    assert_eq!(idents(src), ["struct", "S", "keep", "u8", "fn", "real"]);
}

#[test]
fn inner_cfg_test_removes_the_scope() {
    assert_eq!(idents("#![cfg(test)]\nfn a() { data.map_async(MapMode::Read, cb); }"), Vec::<String>::new());
    assert_eq!(idents("mod t { #![cfg(test)] fn a() {} } fn b() {}"), ["mod", "t", "fn", "b"]);
}

#[test]
fn cfg_any_test_is_still_scanned() {
    let f = scan(
        "src/x.rs",
        "#[cfg(any(test, feature = \"x\"))]\nfn a() { data.map_async(MapMode::Read, cb); }",
        &manifest(),
    )
    .unwrap();
    assert_eq!(rules(&f), vec![("src/x.rs:2".to_string(), Rule::Readback)]);
}

#[test]
fn readback_targets_and_inputs() {
    let src = "fn a(f: &F) {
    ids.slice(..).map_async(wgpu::MapMode::Read, cb);
    f.colour_readback.map_async(MapMode::Read, 0..4, cb);
    data.slice(..).map_async(wgpu::MapMode::Read, cb);
    data.slice(0..16).map_async(wgpu::MapMode::Write, cb);
    staging.slice(..).map_async(mode, cb);
    self.data.map_async(MapMode::Read, cb);
}";
    let f = scan("src/r.rs", src, &manifest()).unwrap();
    assert_eq!(
        rules(&f),
        vec![
            ("src/r.rs:4".to_string(), Rule::Readback),
            ("src/r.rs:6".to_string(), Rule::Readback),
            ("src/r.rs:7".to_string(), Rule::Readback),
        ]
    );
    assert_eq!(
        f[1].to_string(),
        "src/r.rs:6: [SHLINT-002/readback] maps `staging` for reading, which shaders/manifest.toml does not declare as a colour, depth or ID target"
    );
}

#[test]
fn unresolvable_map_async_fails_closed() {
    let src = "fn a() {
    make().map_async(MapMode::Read, cb);
    bufs[0].map_async(MapMode::Read, cb);
    wgpu::BufferSlice::map_async(&s, MapMode::Read, cb);
    let f = Buffer::map_async;
    self.map_async(MapMode::Read, cb);
}
fn map_async() {}";
    let f = scan("src/u.rs", src, &manifest()).unwrap();
    let lines: Vec<String> = f.iter().map(|f| f.location.clone()).collect();
    assert_eq!(lines, ["src/u.rs:2", "src/u.rs:3", "src/u.rs:4", "src/u.rs:5", "src/u.rs:6", "src/u.rs:8"]);
    assert!(f.iter().all(|f| f.rule == Rule::UnresolvedReadback));
}

// ------------------------------------------------------------------ toolkit

#[test]
fn toolkit_denylist() {
    for n in [
        "egui",
        "eframe",
        "winit",
        "iced",
        "slint",
        "egui-wgpu",
        "egui_extras",
        "iced_wgpu",
        "slint-build",
        "i-slint-core",
        "epaint",
        "Winit",
    ] {
        assert!(is_toolkit(n), "{n}");
    }
    for n in ["wgpu", "raw-window-handle", "eguix", "winitx", "slinter", "fid-render", "naga"] {
        assert!(!is_toolkit(n), "{n}");
    }
}

fn meta(packages: &[(&str, Vec<&str>)]) -> String {
    let pkgs: Vec<String> =
        packages.iter().map(|(n, _)| format!("{{\"name\":\"{n}\",\"version\":\"1.0.0\",\"id\":\"{n}-id\",\"manifest_path\":\"/w/{n}/Cargo.toml\"}}")).collect();
    let nodes: Vec<String> = packages
        .iter()
        .map(|(n, deps)| {
            let d: Vec<String> = deps.iter().map(|d| format!("{{\"name\":\"{d}\",\"pkg\":\"{d}-id\"}}")).collect();
            format!("{{\"id\":\"{n}-id\",\"deps\":[{}]}}", d.join(","))
        })
        .collect();
    format!(
        "{{\"packages\":[{}],\"workspace_members\":[\"fid-render-id\"],\"resolve\":{{\"nodes\":[{}]}},\"version\":1}}",
        pkgs.join(","),
        nodes.join(",")
    )
}

#[test]
fn toolkit_paths_are_shortest_and_sorted() {
    let m = meta(&[
        ("fid-render", vec!["zeta", "alpha", "egui"]),
        ("alpha", vec!["egui", "winit"]),
        ("zeta", vec!["winit"]),
        ("egui", vec!["epaint"]),
        ("epaint", vec![]),
        ("winit", vec![]),
    ]);
    let f = deps::check(m.as_bytes(), "fid-render", &BTreeMap::new()).unwrap().findings;
    let got: Vec<String> = f.iter().map(|f| f.location.clone()).collect();
    assert_eq!(got, ["fid-render -> alpha -> winit", "fid-render -> egui", "fid-render -> egui -> epaint"]);
}

#[test]
fn clean_graph_and_cycles() {
    let m = meta(&[("fid-render", vec!["a"]), ("a", vec!["b"]), ("b", vec!["a", "fid-render"])]);
    let r = deps::check(m.as_bytes(), "fid-render", &BTreeMap::new()).unwrap();
    assert!(r.findings.is_empty());
    assert_eq!(r.manifest_path, "/w/fid-render/Cargo.toml");
}

#[test]
fn malformed_metadata_fails_closed() {
    let good = meta(&[("fid-render", vec!["a"]), ("a", vec![])]);
    let cases = [
        "not json".to_string(),
        "{\"a\":1,\"a\":2}".to_string(),
        good.replace("\"version\":1", "\"version\":2"),
        good.replace(",\"resolve\":{\"nodes\":[", ",\"resolve\":null,\"x\":{\"nodes\":["),
        good.replace("\"workspace_members\":[\"fid-render-id\"]", "\"workspace_members\":[]"),
        good.replace("\"pkg\":\"a-id\"", "\"pkg\":\"ghost-id\""),
        meta(&[("fid-render", vec!["a"])]),
        good.replace("\"deps\":[]", "\"other\":[]"),
        good.replace(",\"manifest_path\":\"/w/fid-render/Cargo.toml\"", ""),
        good.replace("\"manifest_path\":\"/w/fid-render/Cargo.toml\"", "\"manifest_path\":7"),
    ];
    for c in &cases {
        match deps::check(c.as_bytes(), "fid-render", &BTreeMap::new()) {
            Err(ShlintError::MetadataMalformed { .. }) => {}
            other => panic!("{c}: {other:?}"),
        }
    }
}

#[test]
fn allowed_toolkit_is_informational_with_reason() {
    let m = meta(&[("fid-render", vec!["winit", "egui"]), ("winit", vec![]), ("egui", vec![])]);
    let manifest =
        ShaderManifest::parse("schema = 1\n[[allow]]\ncrate = \"Winit\"\nreason = \"window host, D-xyz\"\n").unwrap();
    let f = deps::check(m.as_bytes(), "fid-render", &manifest.allows).unwrap().findings;
    assert_eq!(
        f.iter().map(|f| (f.location.as_str(), f.rule)).collect::<Vec<_>>(),
        [("fid-render -> egui", Rule::Toolkit), ("fid-render -> winit", Rule::ToolkitAllowed)]
    );
    assert_eq!(
        f[1].to_string(),
        "fid-render -> winit: [SHLINT-003/toolkit-allowed] fid-render depends on UI-toolkit crate winit, \
         allowed in shaders/manifest.toml: window host, D-xyz"
    );
    assert!(Rule::Toolkit.is_error());
    assert!(!Rule::ToolkitAllowed.is_error());
}

#[test]
fn allow_entries_are_validated() {
    let cases: &[(&str, &str)] = &[
        ("schema = 1\n[[allow]]\ncrate = \"wgpu\"\nreason = \"x\"", "is not on the UI-toolkit denylist"),
        ("schema = 1\n[[allow]]\ncrate = \"winit\"\nreason = \" \"", "must be one non-empty line"),
        ("schema = 1\n[[allow]]\ncrate = \"winit\"", "`reason` is missing"),
        (
            "schema = 1\n[[allow]]\ncrate = \"egui_extras\"\nreason = \"a\"\n[[allow]]\ncrate = \"EGUI-extras\"\nreason = \"b\"",
            "is allowed twice",
        ),
        ("schema = 1\n[[allow]]\ncrate = \"winit\"\nreason = \"a\"\nscope = \"all\"", "unknown key `scope`"),
    ];
    for (text, want) in cases {
        match ShaderManifest::parse(text) {
            Ok(m) => panic!("accepted {text:?}: {m:?}"),
            Err(e) => assert!(e.contains(want), "{text:?}: got {e:?}, want {want:?}"),
        }
    }
}

#[test]
fn readback_helpers_are_unresolved() {
    let src = "use wgpu::util::DownloadBuffer;\nfn a() {\n    DownloadBuffer::read_buffer(&d, &q, &s, cb);\n}";
    let f = scan("src/h.rs", src, &manifest()).unwrap();
    assert_eq!(
        rules(&f),
        vec![
            ("src/h.rs:1".to_string(), Rule::UnresolvedReadback),
            ("src/h.rs:3".to_string(), Rule::UnresolvedReadback),
            ("src/h.rs:3".to_string(), Rule::UnresolvedReadback),
        ]
    );
    assert_eq!(
        f[2].to_string(),
        "src/h.rs:3: [SHLINT-002/unresolved-readback] uses wgpu readback helper `read_buffer`; cannot tell which target is read"
    );
}

#[test]
fn shader_sources_from_listed_modules_are_clean() {
    let src = "fn m(d: &wgpu::Device) {
    let a = wgpu::ShaderSource::Wgsl(include_str!(\"../shaders/a.wgsl\").into());
    let b = wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(\"../shaders/./a.wgsl\")));
    let c = wgpu::include_wgsl!(\"../shaders/a.wgsl\");
    let label = Some(\"not a shader\");
}
#[cfg(test)]
mod t { const S: &str = include_str!(\"fixture.txt\"); }";
    assert!(scan_shader_sources("src/p.rs", src, &manifest()).unwrap().is_empty());
}

#[test]
fn shader_text_outside_the_manifest_is_flagged() {
    let src = "fn m() {
    let a = ShaderSource::Wgsl(\"@fragment fn f() {}\".into());
    let b = ShaderSource::Wgsl(text.into());
    let c = ShaderSource::Glsl { shader: s, stage: v, defines: d };
    let d = include_str!(\"../shaders/a.txt\");
    let e = include_str!(\"../shaders/other.wgsl\");
    let f = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/a.wgsl\"));
    let g = wgpu::include_spirv!(\"a.spv\");
    let h = include_str!(\"../../outside.wgsl\");
    let i = ShaderSource::Wgsl(Cow::Owned(format!(\"{}\", include_str!(\"../shaders/a.wgsl\"))));
}";
    let f = scan_shader_sources("src/p.rs", src, &manifest()).unwrap();
    let lines: Vec<&str> = f.iter().map(|f| f.location.as_str()).collect();
    assert_eq!(
        lines,
        ["src/p.rs:2", "src/p.rs:3", "src/p.rs:4", "src/p.rs:5", "src/p.rs:6", "src/p.rs:7", "src/p.rs:8", "src/p.rs:9", "src/p.rs:10"]
    );
    assert!(f.iter().all(|f| f.rule == Rule::UnmanifestedShader));
    assert_eq!(
        f[4].to_string(),
        "src/p.rs:6: [SHLINT-001/unmanifested-shader] include_str! of `../shaders/other.wgsl` (shaders/other.wgsl), \
         which is not listed in shaders/manifest.toml"
    );
}

#[test]
fn relative_include_paths() {
    assert_eq!(resolve_relative("src/a/b.rs", "../../shaders/x.wgsl").as_deref(), Some("shaders/x.wgsl"));
    assert_eq!(resolve_relative("build.rs", "shaders/x.wgsl").as_deref(), Some("shaders/x.wgsl"));
    assert_eq!(resolve_relative("src/b.rs", "../../x.wgsl"), None);
    assert_eq!(resolve_relative("src/b.rs", "/abs/x.wgsl"), None);
}
