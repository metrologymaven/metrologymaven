//! Acceptance tests for spec node `AUTO.SURF.DOCS` (`DocsEmitter`, `docs-ref-v1`).
//! Expected goldens under `fixtures/docs/` come from `fixtures/docs/make_fixtures.py`
//! (an independent stdlib-Python reference implementation).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use fid_canon::{canonicalize, parse, CValue};
use fid_surface::{project, DocText};
use fid_surface_gen::{docs_ref_v1_schema_bytes, generate, render, DocsEmitter, GenError};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    fs::read(repo_root().join(rel)).unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("surface-gen-docs").join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.parent().unwrap()).unwrap();
    d
}

fn collect_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                let rel = p.strip_prefix(root).unwrap();
                let key: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                out.insert(key.join("/"), fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    if root.exists() {
        walk(root, root, &mut out);
    }
    out
}

fn parse_obj(bytes: &[u8]) -> BTreeMap<String, CValue> {
    match parse(bytes).expect("valid JSON") {
        CValue::Obj(o) => o,
        other => panic!("expected JSON object, got {other:?}"),
    }
}

fn get_str<'a>(obj: &'a BTreeMap<String, CValue>, key: &str) -> &'a str {
    match obj.get(key) {
        Some(CValue::Str(s)) => s.as_str(),
        other => panic!("expected string at {key}, got {other:?}"),
    }
}

fn get_obj<'a>(obj: &'a BTreeMap<String, CValue>, key: &str) -> &'a BTreeMap<String, CValue> {
    match obj.get(key) {
        Some(CValue::Obj(m)) => m,
        other => panic!("expected object at {key}, got {other:?}"),
    }
}

/// AC-1: `docs/ref/` has one record per command major in `min-classes.json`,
/// `docs/surface-index.json` lists every one of them, and every emitted file matches its
/// independent Python golden under `fixtures/docs/` byte-for-byte.
#[test]
fn ac1_min_classes_records_indexes_and_legend_match_goldens() {
    let out = tmp("ac1");
    let manifest = read("fixtures/manifest/min-classes.json");
    let proj = project(&manifest).unwrap();

    generate(&manifest, &[&DocsEmitter], &out).unwrap();
    let got_tree = collect_tree(&out.join("docs"));

    // Expected tree from fixtures/docs/ (excluding make_fixtures.py and LICENSE.txt).
    let mut want_tree = collect_tree(&repo_root().join("fixtures/docs"));
    want_tree.remove("make_fixtures.py");
    want_tree.remove("LICENSE.txt");

    assert_eq!(got_tree.len(), want_tree.len(), "file count mismatch");
    for (rel, want_bytes) in &want_tree {
        let got_bytes = got_tree.get(rel).unwrap_or_else(|| panic!("missing emitted file docs/{rel}"));
        assert_eq!(
            String::from_utf8_lossy(got_bytes),
            String::from_utf8_lossy(want_bytes),
            "byte mismatch in docs/{rel}"
        );
    }

    // Exactly one record per command major in docs/ref/.
    let ref_files: Vec<&String> = got_tree.keys().filter(|k| k.starts_with("ref/")).collect();
    assert_eq!(ref_files.len(), proj.commands.len());

    // surface-index.json lists every command major in byte order.
    let idx = parse_obj(&got_tree["surface-index.json"]);
    let CValue::Arr(cmds) = &idx["commands"] else { panic!("commands array") };
    assert_eq!(cmds.len(), proj.commands.len());
    let indexed: Vec<String> = cmds
        .iter()
        .map(|entry| {
            let CValue::Obj(m) = entry else { panic!("entry object") };
            get_str(m, "command_id").to_string()
        })
        .collect();
    let mut expected_ids: Vec<String> = proj.commands.iter().map(|c| c.command_id.as_str().to_string()).collect();
    expected_ids.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(indexed, expected_ids);

    // Every emitted JSON file is RFC 8785 canonical JSON (stripping optional trailing LF).
    for (rel, bytes) in &got_tree {
        let trimmed = bytes.strip_suffix(b"\n").unwrap_or(bytes);
        assert_eq!(canonicalize(trimmed).unwrap(), trimmed, "docs/{rel} is not RFC 8785 canonical JSON");
    }
}

/// AC-2: M1, S1, A0 records have `mcp: {available: false, projection: "none", reason: "class_never_on_mcp"}`;
/// W3, X1, D1 have `mcp.reason = "request_only"` with tool name present;
/// R0 has `mcp.available = true`, derived tool name, and no reason.
#[test]
fn ac2_mcp_reachability_and_reason_codes_by_class() {
    let out = tmp("ac2");
    generate(&read("fixtures/manifest/min-classes.json"), &[&DocsEmitter], &out).unwrap();
    let docs = out.join("docs");

    for never_cmd in ["motion.move_axis@1", "trust.sign_record@1", "plugin.install_pack@1"] {
        let rec = parse_obj(&fs::read(docs.join(format!("ref/{never_cmd}.json"))).unwrap());
        let mcp = get_obj(get_obj(&rec, "surfaces"), "mcp");
        assert_eq!(mcp.get("available"), Some(&CValue::Bool(false)), "{never_cmd}");
        assert_eq!(mcp.get("projection"), Some(&CValue::Str("none".into())), "{never_cmd}");
        assert_eq!(mcp.get("reason"), Some(&CValue::Str("class_never_on_mcp".into())), "{never_cmd}");
        assert!(!mcp.contains_key("name"), "{never_cmd} must not have an MCP tool name");
    }

    for (req_cmd, tool_name) in [
        ("rpt.release_report@1", "rpt_release_report"),
        ("egress.send@1", "egress_send"),
        ("probe.read_temperature@1", "probe_read_temperature"),
    ] {
        let rec = parse_obj(&fs::read(docs.join(format!("ref/{req_cmd}.json"))).unwrap());
        let mcp = get_obj(get_obj(&rec, "surfaces"), "mcp");
        assert_eq!(mcp.get("available"), Some(&CValue::Bool(true)), "{req_cmd}");
        assert_eq!(mcp.get("name"), Some(&CValue::Str(tool_name.into())), "{req_cmd}");
        assert_eq!(mcp.get("projection"), Some(&CValue::Str("request_only".into())), "{req_cmd}");
        assert_eq!(mcp.get("reason"), Some(&CValue::Str("request_only".into())), "{req_cmd}");
    }

    // R0 record: available = true, derived tool name, no reason.
    let r0 = parse_obj(&fs::read(docs.join("ref/project.list_features@1.json")).unwrap());
    let mcp = get_obj(get_obj(&r0, "surfaces"), "mcp");
    assert_eq!(mcp.get("available"), Some(&CValue::Bool(true)));
    assert_eq!(mcp.get("name"), Some(&CValue::Str("project_list_features".into())));
    assert_eq!(mcp.get("projection"), Some(&CValue::Str("exposed".into())));
    assert!(!mcp.contains_key("reason"));
}

/// AC-3: `fixtures/manifest/human-only.json` W2 `human_only: true` command records
/// `human_only: true` and `mcp.reason = "human_only"` with the tool name present.
#[test]
fn ac3_human_only_w2_command_marks_human_only_reason() {
    let out = tmp("ac3");
    generate(&read("fixtures/manifest/human-only.json"), &[&DocsEmitter], &out).unwrap();
    let rec = parse_obj(&fs::read(out.join("docs/ref/align.adopt_datum_frame@1.json")).unwrap());
    assert_eq!(rec.get("human_only"), Some(&CValue::Bool(true)));
    assert_eq!(rec.get("side_effect"), Some(&CValue::Str("W2".into())));

    let mcp = get_obj(get_obj(&rec, "surfaces"), "mcp");
    assert_eq!(mcp.get("available"), Some(&CValue::Bool(true)));
    assert_eq!(mcp.get("name"), Some(&CValue::Str("align_adopt_datum_frame".into())));
    assert_eq!(mcp.get("projection"), Some(&CValue::Str("request_only".into())));
    assert_eq!(mcp.get("reason"), Some(&CValue::Str("human_only".into())));
}

/// AC-4: `help` and `summary` are byte-for-byte verbatim copies of the projection's `DocText`,
/// including special characters and multi-line content, with zero rewording or added prose.
#[test]
fn ac4_help_is_verbatim_copy_of_projection_doctext() {
    let manifest = read("fixtures/manifest/min-classes.json");
    let mut proj = project(&manifest).unwrap();
    // Inject special characters & newlines into align.run_bestfit@1 to prove verbatim copy.
    let special_help = "Best-fit alignment body.\n\nCompare a < b & c > d, \"quoted\" and 'single'.";
    let target = proj.commands.iter_mut().find(|c| c.command_id.as_str() == "align.run_bestfit@1").unwrap();
    target.help = DocText { title: target.help.title.clone(), description: special_help.to_string() };
    let expected_title = target.help.title.clone();

    let rendered = render(&proj, &[&DocsEmitter]).unwrap();
    let rec_bytes = &rendered[0].files["ref/align.run_bestfit@1.json"];
    let rec = parse_obj(rec_bytes);
    assert_eq!(get_str(&rec, "help").as_bytes(), special_help.as_bytes());
    assert_eq!(get_str(&rec, "summary").as_bytes(), expected_title.as_bytes());
}

/// AC-5: Two emissions into two directories produce byte-identical `docs/` trees,
/// and every record names both the manifest digest and its verified command digest.
#[test]
fn ac5_double_emission_is_byte_identical_and_stamps_digests() {
    let manifest = read("fixtures/manifest/min-classes.json");
    let proj = project(&manifest).unwrap();
    let a = tmp("ac5-a");
    let b = tmp("ac5-b");

    generate(&manifest, &[&DocsEmitter], &a).unwrap();
    generate(&manifest, &[&DocsEmitter], &b).unwrap();

    let ta = collect_tree(&a.join("docs"));
    let tb = collect_tree(&b.join("docs"));
    assert_eq!(ta, tb);

    for cmd in &proj.commands {
        let rel = format!("ref/{}@{}.json", cmd.command_id.name(), cmd.major);
        let rec = parse_obj(&ta[&rel]);
        assert_eq!(get_str(&rec, "manifest_digest"), proj.manifest_digest.as_str());
        assert_eq!(get_str(&rec, "command_digest"), cmd.command_digest.as_str());
        assert_eq!(get_str(&rec, "schema"), "docs-ref-v1");
    }
}

/// AC-6 (error path): Empty `DocText` (empty description or empty title) fails with
/// `missing_help` naming the offending command, and writes nothing to disk.
#[test]
fn ac6_empty_doctext_fails_with_missing_help_and_writes_nothing() {
    let manifest = read("fixtures/manifest/min-classes.json");
    for clear_title in [false, true] {
        let mut proj = project(&manifest).unwrap();
        let target = proj.commands.iter_mut().find(|c| c.command_id.as_str() == "align.run_bestfit@1").unwrap();
        if clear_title {
            target.help.title.clear();
        } else {
            target.help.description = "   \n\t  ".to_string();
        }

        let err = render(&proj, &[&DocsEmitter]).unwrap_err();
        match &err {
            GenError::EmitFailed { surface, detail } => {
                assert_eq!(surface, "docs");
                assert!(detail.starts_with("missing_help: align.run_bestfit@1"), "unexpected detail: {detail}");
            }
            other => panic!("expected EmitFailed, got {other:?}"),
        }
    }

    // End-to-end via generate() on a manifest whose command has empty description: writes nothing.
    let mut doc: BTreeMap<String, CValue> = parse_obj(&manifest);
    let CValue::Arr(ref mut cmds) = doc.get_mut("commands").unwrap() else { panic!() };
    let CValue::Obj(ref mut first) = cmds[0] else { panic!() };
    first.insert("description".to_string(), CValue::Str(String::new()));
    first.remove("definition_digest");
    let new_digest = fid_canon::digest(&CValue::Obj(first.clone())).unwrap();
    first.insert("definition_digest".to_string(), CValue::Str(new_digest.to_string()));
    let bad_manifest = fid_canon::to_canonical_bytes(&CValue::Obj(doc)).unwrap();

    let out = tmp("ac6-disk");
    let err = generate(&bad_manifest, &[&DocsEmitter], &out).unwrap_err();
    match &err {
        GenError::EmitFailed { surface, detail } => {
            assert_eq!(surface, "docs");
            assert!(detail.starts_with("missing_help: "), "unexpected detail: {detail}");
        }
        other => panic!("expected EmitFailed, got {other:?}"),
    }
    assert!(!out.exists(), "output directory must not be created on missing_help");
}

/// Published `schema/docs-ref-v1.json` matches the Rust-generated schema bytes verbatim.
#[test]
fn published_schema_matches_rust_types() {
    let want = docs_ref_v1_schema_bytes().unwrap();
    let got = read("schema/docs-ref-v1.json");
    assert_eq!(
        String::from_utf8_lossy(&got),
        String::from_utf8_lossy(&want),
        "schema/docs-ref-v1.json is out of sync with fid_surface_gen::docs_ref_v1_schema_bytes()"
    );
}
