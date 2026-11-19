//! Acceptance criteria of spec node AUTO.SURF.GEN (AC-1 .. AC-5) and the pinned path,
//! normalisation and stale-file policies. Expected bytes come from
//! `fixtures/auto/surf/gen/make_fixtures.py`, an independent Python reference.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_surface::Projection;
use fid_surface_gen::{
    check_rel_path, generate, normalise, render, CommentSyntax, EchoEmitter, EmitError, GenError, GeneratedFile,
    SurfaceEmitter, INDEX_FILE,
};
use fid_types::CommandId;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn manifest() -> Vec<u8> {
    fs::read(repo_root().join("fixtures/manifest/min-classes.json")).unwrap()
}

fn projection() -> Projection {
    fid_surface::project(&manifest()).unwrap()
}

/// A fresh, empty directory under the test target directory.
fn tmp(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("surface-gen").join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.parent().unwrap()).unwrap();
    d
}

/// Every file under `root`, by `/`-joined relative path.
fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
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

fn expected_tree() -> BTreeMap<String, Vec<u8>> {
    let raw = fs::read(repo_root().join("fixtures/auto/surf/gen/expected-min-classes.json")).unwrap();
    let CValue::Obj(o) = fid_canon::parse(&raw).unwrap() else { panic!("expected an object") };
    o.into_iter()
        .map(|(k, v)| match v {
            CValue::Str(s) => (k, s.into_bytes()),
            other => panic!("{k}: {other:?}"),
        })
        .collect()
}

/// An emitter returning fixed files.
struct Fixed {
    surface: &'static str,
    files: Vec<(&'static str, &'static [u8])>,
}

impl SurfaceEmitter for Fixed {
    fn surface(&self) -> &'static str {
        self.surface
    }
    fn emit(&self, _p: &Projection) -> Result<Vec<GeneratedFile>, EmitError> {
        Ok(self
            .files
            .iter()
            .map(|(p, b)| GeneratedFile { rel_path: (*p).to_string(), bytes: b.to_vec(), covers: Vec::new() })
            .collect())
    }
    fn comment_syntax(&self, _path: &str) -> CommentSyntax {
        CommentSyntax::SLASHES
    }
}

fn fixed(files: Vec<(&'static str, &'static [u8])>) -> Fixed {
    Fixed { surface: "fixed", files }
}

#[test]
fn ac1_two_runs_byte_identical_and_match_reference() {
    let (a, b) = (tmp("ac1-a"), tmp("ac1-b"));
    generate(&manifest(), &[&EchoEmitter], &a).unwrap();
    generate(&manifest(), &[&EchoEmitter], &b).unwrap();
    let (ta, tb) = (tree(&a), tree(&b));
    assert_eq!(ta, tb, "two generations differ");
    assert_eq!(ta, expected_tree(), "output differs from the Python reference");
    let digest = projection().manifest_digest;
    for (path, bytes) in &ta {
        let text = std::str::from_utf8(bytes).unwrap();
        let first = text.split('\n').take(3).collect::<Vec<_>>().join("\n");
        assert!(first.contains("generated, do not edit"), "{path}");
        assert!(first.contains(digest.as_str()), "{path}");
        assert!(
            text.starts_with("# generated, do not edit\n")
                || text.starts_with("{\"_header\":\"generated, do not edit; manifest sha256:"),
            "{path}"
        );
    }
    // Generating again over an existing tree gives the same bytes.
    generate(&manifest(), &[&EchoEmitter], &a).unwrap();
    assert_eq!(tree(&a), tb);
}

#[test]
fn ac2_index_lists_every_command_in_byte_order() {
    let out = tmp("ac2");
    generate(&manifest(), &[&EchoEmitter], &out).unwrap();
    let index = fs::read(out.join("echo").join(INDEX_FILE)).unwrap();
    assert_eq!(index, expected_tree()["echo/surface-index.json"]);
    let CValue::Obj(o) = fid_canon::parse(&index).unwrap() else { panic!() };
    let CValue::Arr(list) = &o["commands"] else { panic!() };
    let ids: Vec<String> = list
        .iter()
        .map(|e| match e {
            CValue::Obj(m) => match &m["command_id"] {
                CValue::Str(s) => s.clone(),
                _ => panic!(),
            },
            _ => panic!(),
        })
        .collect();
    let mut want: Vec<String> = projection().commands.iter().map(|c| c.command_id.as_str().to_string()).collect();
    want.sort_by(|x, y| x.as_bytes().cmp(y.as_bytes()));
    assert_eq!(ids, want);
    assert_eq!(fid_canon::canonicalize(&index).unwrap(), index, "index is not RFC 8785");
}

#[test]
fn ac3_escape_fails_and_leaves_output_unchanged() {
    let out = tmp("ac3");
    generate(&manifest(), &[&EchoEmitter], &out).unwrap();
    fs::write(out.join("keep.txt"), b"not ours").unwrap();
    let before = tree(&out);
    let bad = fixed(vec![("ok.txt", b"x"), ("../escape.txt", b"y")]);
    let err = generate(&manifest(), &[&EchoEmitter, &bad], &out).unwrap_err();
    assert_eq!(err.code(), "path_outside_root");
    assert_eq!(tree(&out), before);
    assert!(!out.join("..").join("escape.txt").exists());
}

#[test]
fn path_rules_refuse_every_escape() {
    let bad = [
        ("", "empty path"),
        ("/etc/passwd", "absolute path"),
        ("C:/x.txt", "drive letter or stream separator"),
        ("c:x.txt", "drive letter or stream separator"),
        ("a.txt:stream", "drive letter or stream separator"),
        ("\\\\server\\share\\x", "backslash separator"),
        ("a\\b.txt", "backslash separator"),
        ("//server/share", "absolute path"),
        ("a//b", "empty segment"),
        ("a/", "empty segment"),
        ("a/./b", "dot segment"),
        ("a/../b", "dot segment"),
        ("..", "dot segment"),
        ("sp ace.txt", "non-portable character"),
        ("caf\u{e9}.txt", "non-portable character"),
        ("nul\0.txt", "non-portable character"),
        ("name.", "trailing dot"),
        ("CON", "reserved device name"),
        ("x/nul.txt", "reserved device name"),
        ("com1.md", "reserved device name"),
        ("Lpt9", "reserved device name"),
    ];
    for (p, why) in bad {
        assert_eq!(check_rel_path(p), Err(why), "{p:?}");
    }
    let long = "a".repeat(201);
    assert_eq!(check_rel_path(&long), Err("path longer than 200 bytes"));
    for ok in ["a.txt", "dir/sub/file.md", ".gitignore", "con-fig.txt", "com10.txt", "x_y-z.v1.txt"] {
        assert_eq!(check_rel_path(ok), Ok(()), "{ok:?}");
    }
}

#[test]
fn ac4_duplicates_fail_and_write_nothing() {
    let cases: Vec<Fixed> = vec![
        fixed(vec![("a.txt", b"1"), ("a.txt", b"2")]),
        fixed(vec![("Read.md", b"1"), ("read.MD", b"2")]),
        fixed(vec![("a", b"1"), ("a/b.txt", b"2")]),
        fixed(vec![("A/b.txt", b"1"), ("a", b"2")]),
        fixed(vec![("surface-index.json", b"{}")]),
    ];
    for (i, e) in cases.iter().enumerate() {
        let out = tmp(&format!("ac4-{i}"));
        let err = generate(&manifest(), &[e], &out).unwrap_err();
        assert_eq!(err.code(), "duplicate_output", "case {i}: {err}");
        assert!(!out.exists(), "case {i} wrote output");
    }
    // Two emitters for one surface.
    let out = tmp("ac4-surface");
    let (x, y) = (fixed(vec![("a.txt", b"1")]), fixed(vec![("b.txt", b"2")]));
    assert_eq!(generate(&manifest(), &[&x, &y], &out).unwrap_err().code(), "duplicate_output");
    assert!(!out.exists());
}

#[test]
fn bad_surface_names_are_refused() {
    for name in ["", "Echo", "../x", "a/b", "con", "1st", ".hidden"] {
        struct Named(&'static str);
        impl SurfaceEmitter for Named {
            fn surface(&self) -> &'static str {
                self.0
            }
            fn emit(&self, _p: &Projection) -> Result<Vec<GeneratedFile>, EmitError> {
                Ok(Vec::new())
            }
            fn comment_syntax(&self, _path: &str) -> CommentSyntax {
                CommentSyntax::HASH
            }
        }
        let err = render(&projection(), &[&Named(name)]).unwrap_err();
        assert_eq!(err.code(), "path_outside_root", "{name:?}");
    }
}

#[test]
fn ac5_no_cr_and_no_bom_in_any_file() {
    let out = tmp("ac5");
    let crlf = fixed(vec![("crlf.txt", b"\xef\xbb\xbfone\r\ntwo\rthree"), ("empty.txt", b"")]);
    generate(&manifest(), &[&EchoEmitter, &crlf], &out).unwrap();
    let t = tree(&out);
    for (path, bytes) in &t {
        assert!(!bytes.contains(&b'\r'), "{path} has a CR byte");
        assert!(!bytes.starts_with(b"\xef\xbb\xbf"), "{path} has a BOM");
    }
    let body = String::from_utf8(t["fixed/crlf.txt"].clone()).unwrap();
    assert!(body.ends_with("// generator fid-surface-gen 1\none\ntwo\nthree\n"), "{body:?}");
    let empty = String::from_utf8(t["fixed/empty.txt"].clone()).unwrap();
    assert!(empty.ends_with("// generator fid-surface-gen 1\n") && empty.lines().count() == 3, "{empty:?}");
}

#[test]
fn normalise_rules() {
    assert_eq!(normalise(b"a\r\nb\rc\n").unwrap(), "a\nb\nc\n");
    assert_eq!(normalise(b"\r\r\n").unwrap(), "\n\n");
    assert_eq!(normalise(b"").unwrap(), "");
    assert_eq!(normalise(b"x").unwrap(), "x\n");
    assert_eq!(normalise("\u{feff}\u{b5}m".as_bytes()).unwrap(), "\u{b5}m\n");
    assert!(normalise(b"\xff").is_err());
}

#[test]
fn emit_failures_are_typed_and_write_nothing() {
    struct Failing;
    impl SurfaceEmitter for Failing {
        fn surface(&self) -> &'static str {
            "failing"
        }
        fn emit(&self, _p: &Projection) -> Result<Vec<GeneratedFile>, EmitError> {
            Err(EmitError { detail: "boom".into() })
        }
        fn comment_syntax(&self, _path: &str) -> CommentSyntax {
            CommentSyntax::HASH
        }
    }
    struct Covers(&'static str);
    impl SurfaceEmitter for Covers {
        fn surface(&self) -> &'static str {
            "covers"
        }
        fn emit(&self, _p: &Projection) -> Result<Vec<GeneratedFile>, EmitError> {
            Ok(vec![GeneratedFile {
                rel_path: "x.txt".into(),
                bytes: b"x".to_vec(),
                covers: vec![CommandId::parse(self.0).unwrap()],
            }])
        }
        fn comment_syntax(&self, _path: &str) -> CommentSyntax {
            CommentSyntax::Block("<!--", "-->")
        }
    }
    struct BadSyntax;
    impl SurfaceEmitter for BadSyntax {
        fn surface(&self) -> &'static str {
            "badsyntax"
        }
        fn emit(&self, _p: &Projection) -> Result<Vec<GeneratedFile>, EmitError> {
            Ok(vec![GeneratedFile { rel_path: "x.txt".into(), bytes: Vec::new(), covers: Vec::new() }])
        }
        fn comment_syntax(&self, _path: &str) -> CommentSyntax {
            CommentSyntax::Line("# x")
        }
    }
    let not_utf8 = fixed(vec![("bin.txt", b"\xff\xfe")]);
    let unknown = Covers("no.such_command@1");
    let cases: [&dyn SurfaceEmitter; 4] = [&Failing, &not_utf8, &unknown, &BadSyntax];
    for (i, e) in cases.iter().enumerate() {
        let out = tmp(&format!("emit-{i}"));
        let err = generate(&manifest(), &[&EchoEmitter, *e], &out).unwrap_err();
        assert_eq!(err.code(), "emit_failed", "case {i}: {err}");
        assert!(!out.exists(), "case {i} wrote output");
    }
    // A known command is accepted, and HTML syntax wraps every header line.
    let r = render(&projection(), &[&Covers("align.set_datum@1")]).unwrap();
    let text = String::from_utf8(r[0].files["x.txt"].clone()).unwrap();
    assert!(text.starts_with("<!-- generated, do not edit -->\n<!-- manifest sha256:"), "{text}");
    assert_eq!(r[0].commands, vec![CommandId::parse("align.set_datum@1").unwrap()]);
}

#[test]
fn manifest_errors_keep_the_projection_code() {
    let raw = fs::read(repo_root().join("fixtures/manifest/bad-projection.json")).unwrap();
    let out = tmp("manifest-err");
    let err = generate(&raw, &[&EchoEmitter], &out).unwrap_err();
    assert!(matches!(err, GenError::Manifest(_)));
    assert_eq!(err.code(), "projection_mismatch");
    assert!(!out.exists());
}

#[test]
fn stale_files_go_only_inside_owned_directories() {
    let out = tmp("stale");
    let first = fixed(vec![("old.txt", b"1"), ("dir/keep.txt", b"2")]);
    generate(&manifest(), &[&first], &out).unwrap();
    fs::create_dir_all(out.join("other")).unwrap();
    fs::write(out.join("other/user.txt"), b"mine").unwrap();
    fs::write(out.join("top.txt"), b"mine").unwrap();
    let second = fixed(vec![("dir/keep.txt", b"3")]);
    generate(&manifest(), &[&second], &out).unwrap();
    let t = tree(&out);
    let keys: Vec<&str> = t.keys().map(String::as_str).collect();
    assert_eq!(keys, ["fixed/dir/keep.txt", "fixed/surface-index.json", "other/user.txt", "top.txt"]);
    assert_eq!(t["other/user.txt"], b"mine");
}

#[test]
fn interrupted_swap_is_repaired() {
    let out = tmp("recover");
    generate(&manifest(), &[&EchoEmitter], &out).unwrap();
    let good = tree(&out);
    // Simulate a crash between the two renames: the tree sits at the old name, the target is
    // gone, and a half-written staging directory is left behind.
    fs::rename(out.join("echo"), out.join(".echo.gen-old")).unwrap();
    fs::create_dir_all(out.join(".echo.gen-staging/commands")).unwrap();
    fs::write(out.join(".echo.gen-staging/commands/partial.txt"), b"half").unwrap();
    generate(&manifest(), &[&EchoEmitter], &out).unwrap();
    assert_eq!(tree(&out), good);
}

#[test]
fn emitter_file_order_does_not_change_the_tree() {
    let fwd = fixed(vec![("b.txt", b"b"), ("a/c.txt", b"c"), ("a.md", b"a")]);
    let rev = fixed(vec![("a.md", b"a"), ("a/c.txt", b"c"), ("b.txt", b"b")]);
    let (x, y) = (tmp("order-a"), tmp("order-b"));
    generate(&manifest(), &[&fwd], &x).unwrap();
    generate(&manifest(), &[&rev], &y).unwrap();
    assert_eq!(tree(&x), tree(&y));
}

#[test]
fn a_file_in_place_of_the_surface_directory_is_not_replaced() {
    let out = tmp("not-dir");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("echo"), b"a file").unwrap();
    let err = generate(&manifest(), &[&EchoEmitter], &out).unwrap_err();
    assert_eq!(err.code(), "io_error");
    assert_eq!(fs::read(out.join("echo")).unwrap(), b"a file");
    assert!(!out.join(".echo.gen-staging").exists());
}

#[test]
fn header_goes_after_a_shebang_or_coding_line_only_when_asked() {
    let h = "H\n";
    let keep = CommentSyntax::HASH_AFTER_PROLOGUE;
    let cases = [
        (
            "#!/usr/bin/env python3\n# -*- coding: utf-8 -*-\nx\n",
            "#!/usr/bin/env python3\n# -*- coding: utf-8 -*-\nH\nx\n",
        ),
        ("#!/bin/sh\necho\n", "#!/bin/sh\nH\necho\n"),
        ("# vim: set fileencoding=utf-8 :\nx\n", "# vim: set fileencoding=utf-8 :\nH\nx\n"),
        ("# a plain comment\nx\n", "H\n# a plain comment\nx\n"),
        ("# coding=latin-1\nx\n", "# coding=latin-1\nH\nx\n"),
        ("x\n# coding: utf-8\n", "H\nx\n# coding: utf-8\n"),
        ("#!/bin/sh\nx\n# coding: utf-8\n", "#!/bin/sh\nH\nx\n# coding: utf-8\n"),
        ("", "H\n"),
    ];
    for (body, want) in cases {
        assert_eq!(keep.stamp(h, body), want, "{body:?}");
    }
    assert_eq!(CommentSyntax::HASH.stamp(h, "#!/bin/sh\nx\n"), "H\n#!/bin/sh\nx\n");
    assert_eq!(CommentSyntax::HTML.stamp(h, "#!/bin/sh\n"), "H\n#!/bin/sh\n");
}

#[test]
fn an_owned_name_in_another_case_is_refused() {
    let out = tmp("case");
    fs::create_dir_all(out.join("Echo")).unwrap();
    fs::write(out.join("Echo").join("mine.txt"), b"mine").unwrap();
    let before = tree(&out);
    let err = generate(&manifest(), &[&EchoEmitter], &out).unwrap_err();
    assert_eq!(err.code(), "case_mismatch", "{err}");
    assert_eq!(tree(&out), before);
    let names: Vec<String> =
        fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["Echo"]);
}

#[cfg(unix)]
#[test]
fn links_in_owned_names_are_never_followed() {
    use std::os::unix::fs::symlink;
    let base = tmp("links");
    let outside = base.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("precious.txt"), b"keep me").unwrap();
    let out = base.join("out");
    fs::create_dir_all(&out).unwrap();
    symlink(&outside, out.join("echo")).unwrap();
    symlink(&outside, out.join(".echo.gen-old")).unwrap();
    let err = generate(&manifest(), &[&EchoEmitter], &out).unwrap_err();
    assert_eq!(err.code(), "io_error", "{err}");
    assert_eq!(tree(&outside).len(), 1);
    assert_eq!(fs::read(outside.join("precious.txt")).unwrap(), b"keep me");
    // The link at the owned name stays a link; the work-directory link is removed as a link.
    assert!(fs::symlink_metadata(out.join("echo")).unwrap().file_type().is_symlink());
    assert!(fs::symlink_metadata(out.join(".echo.gen-old")).is_err());
    assert!(fs::symlink_metadata(out.join(".echo.gen-staging")).is_err());

    // A lone work-directory link (no owned directory) is dropped as a link, never restored.
    fs::remove_file(out.join("echo")).unwrap();
    symlink(&outside, out.join(".echo.gen-old")).unwrap();
    symlink(&outside, out.join(".echo.gen-staging")).unwrap();
    generate(&manifest(), &[&EchoEmitter], &out).unwrap();
    assert_eq!(fs::read(outside.join("precious.txt")).unwrap(), b"keep me");
    assert_eq!(tree(&outside).len(), 1);
    assert!(!fs::symlink_metadata(out.join("echo")).unwrap().file_type().is_symlink());
    assert_eq!(tree(&out.join("echo")).len(), 14);
}
