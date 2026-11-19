//! Acceptance criteria of spec node PLAT.DEPLOY.OFFLINE.BUNDLE (AC-1 .. AC-6) and the
//! requirement edge cases. The bundles, install tree, package plan and exported schemas under
//! `fixtures/plat/deploy/` and `platform/` are written by `fixtures/plat/deploy/make_bundles.py`
//! (hashlib digests of hand-written bytes; expected outcomes stated in its docstring). Inline
//! cases below are hand-labelled from the spec's rules; none is produced by running this crate.
//! Tests that change a bundle work on a copy in the system temporary folder.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_platform::install_manifest::{
    check_bundle, check_bundles, check_relpath, check_roles, is_remote, json_schema_files, make_install_manifest,
    parse_bundle, parse_install_manifest, parse_plan, scan_docs_file, tier1_combinations, BundleError, FileEntry,
    InstallKind, InstallManifest, PackagePlan, Role, SetFinding, BUNDLE_FILE, MAX_BYTES, MAX_ENTRIES, MAX_SIZE,
};
use fid_platform::matrix::{Arch, OsFamily, Tier};
use std::path::{Path, PathBuf};
use std::process::Command;

const DIR: &str = "fixtures/plat/deploy";
const PLAN: &str = "fixtures/plat/deploy/pkg-plan.json";
const ZERO: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap()
}

fn plan() -> PackagePlan {
    parse_plan(&read(PLAN), PLAN).unwrap()
}

fn fixture(name: &str) -> PathBuf {
    root().join(DIR).join(name)
}

/// Runs `fid-platform plat-deploy <args>` from the repository root.
fn cli(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-platform"))
        .current_dir(root())
        .arg("plat-deploy")
        .args(args)
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for item in std::fs::read_dir(from).unwrap() {
        let item = item.unwrap();
        let target = to.join(item.file_name());
        if item.file_type().unwrap().is_dir() {
            copy_dir(&item.path(), &target);
        } else {
            std::fs::copy(item.path(), &target).unwrap();
        }
    }
}

/// A private copy of a fixture bundle, removed on drop.
struct TempCopy(PathBuf);

impl TempCopy {
    fn of(name: &str, tag: &str) -> TempCopy {
        let dst = std::env::temp_dir().join(format!("fid-bundle-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dst);
        copy_dir(&fixture(name), &dst);
        TempCopy(dst)
    }
}

impl Drop for TempCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A canonical install manifest (keys sorted by hand) with the given `files` array text.
fn manifest_json(files: &str) -> String {
    format!(
        r#"{{"arch":"x86_64","files":[{files}],"kind":"per_user","os":"windows","release":"0.1.0","schema_version":1}}"#
    )
}

fn file_entry(relpath: &str) -> String {
    format!(r#"{{"relpath":"{relpath}","sha256":"{ZERO}","size":1}}"#)
}

fn parse_files(relpaths: &[&str]) -> Result<(), BundleError> {
    let files: Vec<String> = relpaths.iter().map(|p| file_entry(p)).collect();
    parse_install_manifest(manifest_json(&files.join(",")).as_bytes(), "m.json").map(|_| ())
}

// ------------------------------------------------------------------------------ AC-1 .. AC-4

#[test]
fn ac1_bundle_ok_passes() {
    assert_eq!(check_bundle(&fixture("bundle-ok"), &plan()), vec![]);
    let (code, out) = cli(&["check-bundle", "--bundle", "fixtures/plat/deploy/bundle-ok", "--plan", PLAN]);
    assert_eq!((code, out.as_str()), (0, "check-bundle: ok\n"));
}

#[test]
fn ac2_missing_cli() {
    assert_eq!(
        check_bundle(&fixture("bundle-missing-cli"), &plan()),
        vec![BundleError::MissingRole { role: Role::Cli, name: None }]
    );
    let (code, out) = cli(&["check-bundle", "--bundle", "fixtures/plat/deploy/bundle-missing-cli", "--plan", PLAN]);
    assert_eq!((code, out.as_str()), (1, "MissingRole { role: cli }\n"));
}

#[test]
fn ac3_tampered_installer() {
    let path = "installers/fid-report-0.1.0-windows-x86_64.msi".to_string();
    assert_eq!(check_bundle(&fixture("bundle-tampered"), &plan()), vec![BundleError::DigestMismatch { path }]);
    let (code, out) = cli(&["check-bundle", "--bundle", "fixtures/plat/deploy/bundle-tampered", "--plan", PLAN]);
    assert_eq!(code, 1);
    assert!(out.contains("DigestMismatch"), "{out}");
}

#[test]
fn ac4_remote_font() {
    assert_eq!(
        check_bundle(&fixture("bundle-remote-font"), &plan()),
        vec![BundleError::RemoteResource {
            file: "docs/style.css".into(),
            url: "https://fonts.example.com/mm.woff2".into()
        }]
    );
}

// ------------------------------------------------------------------------------ AC-5, AC-6

#[test]
fn ac5_dotdot_is_bad_path() {
    let bytes = read("fixtures/plat/deploy/install-manifest-dotdot.json");
    assert_eq!(
        parse_install_manifest(&bytes, "install-manifest-dotdot.json"),
        Err(BundleError::BadPath { path: "../x".into(), reason: "dot_dot" })
    );
}

#[test]
fn ac6_regenerated_manifest_is_byte_identical() {
    let tree = fixture("install-tree");
    let expected = read("fixtures/plat/deploy/bundle-ok/install/install-manifest-per_user.json");
    let make = || {
        make_install_manifest(&tree, InstallKind::PerUser, "0.1.0", OsFamily::Windows, Arch::X86_64)
            .unwrap()
            .to_json()
            .unwrap()
    };
    let first = make();
    assert_eq!(first, expected);
    assert_eq!(make(), first);
    // Reading and writing again gives the same bytes, for both kinds.
    for kind in ["per_user", "machine"] {
        let rel = format!("fixtures/plat/deploy/bundle-ok/install/install-manifest-{kind}.json");
        let bytes = read(&rel);
        let m = parse_install_manifest(&bytes, &rel).unwrap();
        assert_eq!(m.kind.as_str(), kind);
        assert_eq!(m.files.len(), 4);
        assert_eq!(m.to_json().unwrap(), bytes);
    }
    // The command writes the same bytes.
    let out = std::env::temp_dir().join(format!("fid-bundle-test-{}-ac6.json", std::process::id()));
    let out_s = out.to_str().unwrap();
    let args = [
        "make-install-manifest",
        "--root",
        "fixtures/plat/deploy/install-tree",
        "--kind",
        "per_user",
        "--release",
        "0.1.0",
        "--os",
        "windows",
        "--arch",
        "x86_64",
        "--out",
        out_s,
    ];
    assert_eq!(cli(&args).0, 0);
    assert_eq!(std::fs::read(&out).unwrap(), expected);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn bundle_listing_round_trips() {
    let rel = "fixtures/plat/deploy/bundle-ok/offline-bundle.json";
    let bytes = read(rel);
    let b = parse_bundle(&bytes, rel).unwrap();
    assert_eq!(
        (b.bundle_id.as_str(), b.release.as_str(), b.os, b.arch),
        ("bundle-ok", "0.1.0", OsFamily::Windows, Arch::X86_64)
    );
    assert_eq!(b.entries.len(), 12);
    assert_eq!(b.to_json().unwrap(), bytes);
}

#[test]
fn exported_schemas_match_committed_files() {
    for (path, bytes) in json_schema_files().unwrap() {
        assert_eq!(String::from_utf8(read(path)).unwrap(), String::from_utf8(bytes).unwrap(), "{path} is stale");
    }
}

// ------------------------------------------------------------------------------ path rules

#[test]
fn relpath_rules() {
    for ok in ["a", "a/b", "bin/fid.exe", "docs/index.html", ".hidden", "a..b", "x y/z"] {
        assert_eq!(check_relpath(ok), Ok(()), "{ok}");
    }
    let bad = [
        ("", "empty"),
        ("/a", "absolute"),
        ("//server/share/a", "absolute"),
        ("C:/x", "drive"),
        ("c:x", "drive"),
        ("a\\b", "backslash"),
        ("..", "dot_dot"),
        ("a/../b", "dot_dot"),
        ("./a", "dot_segment"),
        ("a//b", "empty_segment"),
        ("a/", "empty_segment"),
        ("CON", "bad_name"),
        ("a/nul.txt", "bad_name"),
        ("a.", "bad_name"),
        ("a /b", "bad_name"),
        ("ab:c", "bad_name"),
        ("a\u{1}b", "bad_name"),
        ("...", "bad_name"),
    ];
    for (p, reason) in bad {
        assert_eq!(check_relpath(p), Err(reason), "{p:?}");
    }
    assert_eq!(check_relpath(&"a".repeat(1025)), Err("too_long"));
}

#[test]
fn path_list_rules() {
    assert_eq!(parse_files(&["a", "b/c"]), Ok(()));
    let bad = |paths: &[&str], path: &str, reason: &'static str| {
        assert_eq!(parse_files(paths), Err(BundleError::BadPath { path: path.into(), reason }), "{paths:?}");
    };
    bad(&["b", "a"], "a", "not_sorted");
    bad(&["a", "a"], "a", "duplicate");
    bad(&["B", "b"], "b", "case_duplicate");
    bad(&["a", "a/b"], "a/b", "file_and_dir");
    bad(&["a", "a-b", "a/b"], "a/b", "file_and_dir");
    bad(&["/etc/passwd"], "/etc/passwd", "absolute");
}

// ------------------------------------------------------------------------------ format rules

#[test]
fn strict_reader() {
    let parse = |s: &str| parse_install_manifest(s.as_bytes(), "m.json");
    let ok = manifest_json(&file_entry("a"));
    assert!(parse(&ok).is_ok());
    // Not canonical: a space, a trailing newline, keys out of order.
    for text in [ok.replacen(',', ", ", 1), format!("{ok}\n")] {
        assert!(matches!(parse(&text), Err(BundleError::Parse { .. })), "{text}");
    }
    let reordered =
        r#"{"schema_version":1,"arch":"x86_64","files":[],"kind":"per_user","os":"windows","release":"0.1.0"}"#;
    assert!(matches!(parse(reordered), Err(BundleError::Parse { .. })));
    // Unknown keys, at the top and in an entry; a missing key; a bad kind, digest and size.
    for text in [
        ok.replacen(r#""arch""#, r#""a":1,"arch""#, 1),
        manifest_json(&format!(r#"{{"extra":1,"relpath":"a","sha256":"{ZERO}","size":1}}"#)),
        ok.replacen(r#""kind":"per_user","#, "", 1),
        ok.replacen("per_user", "system", 1),
        manifest_json(r#"{"relpath":"a","sha256":"sha256:00","size":1}"#),
        manifest_json(&format!(r#"{{"relpath":"a","sha256":"{ZERO}","size":-1}}"#)),
        manifest_json(&format!(r#"{{"relpath":"a","sha256":"{ZERO}","size":1.5}}"#)),
        manifest_json(&format!(r#"{{"relpath":"a","sha256":"{ZERO}","size":9007199254740992}}"#)),
        ok.replacen(r#""os":"windows""#, r#""os":"plan9""#, 1),
        ok.replacen(r#""release":"0.1.0""#, r#""release":"0 1""#, 1),
        "[]".to_string(),
        "{".to_string(),
    ] {
        assert!(matches!(parse(&text), Err(BundleError::Parse { .. })), "{text}");
    }
    assert_eq!(
        parse(&ok.replacen(r#""schema_version":1"#, r#""schema_version":2"#, 1)),
        Err(BundleError::UnsupportedSchema { file: "m.json".into(), found: 2 })
    );
}

#[test]
fn size_and_entry_caps_fail_closed() {
    let big = vec![b' '; MAX_BYTES + 1];
    assert_eq!(parse_install_manifest(&big, "m.json"), Err(BundleError::TooLarge { file: "m.json".into() }));
    assert_eq!(parse_bundle(&big, BUNDLE_FILE), Err(BundleError::TooLarge { file: BUNDLE_FILE.into() }));
    let one = file_entry("a");
    let many = vec![one.as_str(); MAX_ENTRIES + 1].join(",");
    let text = manifest_json(&many);
    assert!(text.len() < MAX_BYTES);
    assert_eq!(parse_install_manifest(text.as_bytes(), "m.json"), Err(BundleError::TooLarge { file: "m.json".into() }));
}

// ------------------------------------------------------------------------------ bundle check

#[test]
fn unlisted_missing_and_resized_files_are_named() {
    let copy = TempCopy::of("bundle-ok", "files");
    std::fs::write(copy.0.join("docs/extra.txt"), b"x").unwrap();
    std::fs::remove_file(copy.0.join("licences/LICENSE-MIT.txt")).unwrap();
    let mut f = std::fs::OpenOptions::new().append(true).open(copy.0.join("runtime/c-runtime-x86_64.exe")).unwrap();
    std::io::Write::write_all(&mut f, b"!").unwrap();
    drop(f);
    assert_eq!(
        check_bundle(&copy.0, &plan()),
        vec![
            BundleError::Unlisted { path: "docs/extra.txt".into() },
            BundleError::MissingFile { path: "licences/LICENSE-MIT.txt".into() },
            BundleError::DigestMismatch { path: "runtime/c-runtime-x86_64.exe".into() },
        ]
    );
}

#[test]
fn plan_rules_for_the_bundle_combination() {
    // Without the inspect app and with an extra runtime dependency in the plan.
    let mut p = plan();
    p.packages.retain(|x| x.package != "inspect");
    p.runtime_deps[0].file_name = "other-runtime.exe".into();
    assert_eq!(
        check_bundle(&fixture("bundle-ok"), &p),
        vec![
            BundleError::UnexpectedEntry {
                path: "installers/fid-inspect-0.1.0-windows-x86_64.msi".into(),
                role: Role::Installer
            },
            BundleError::UnexpectedEntry { path: "runtime/c-runtime-x86_64.exe".into(), role: Role::RuntimeDep },
            BundleError::MissingRole { role: Role::RuntimeDep, name: Some("other-runtime.exe".into()) },
        ]
    );
    // A plan with nothing for windows x86_64.
    let mut p = plan();
    p.packages.retain(|x| x.os != OsFamily::Windows);
    assert_eq!(
        check_bundle(&fixture("bundle-ok"), &p),
        vec![BundleError::NotInPlan { os: OsFamily::Windows, arch: Arch::X86_64 }]
    );
    // A different release.
    let mut p = plan();
    p.version = "0.2.0".into();
    assert_eq!(
        check_bundle(&fixture("bundle-ok"), &p),
        vec![BundleError::Mismatch { file: BUNDLE_FILE.into(), field: "release" }]
    );
}

#[test]
fn bad_plan_is_a_usage_error() {
    assert!(parse_plan(b"{}", "p.json").is_err());
    assert!(parse_plan(
        br#"{"version":"1","packages":[{"package":"a","kind":"app","os":"windows","arch":"x86_64"}]}"#,
        "p.json"
    )
    .is_err());
    let (code, _) = cli(&[
        "check-bundle",
        "--bundle",
        "fixtures/plat/deploy/bundle-ok",
        "--plan",
        "fixtures/plat/deploy/install-manifest-dotdot.json",
    ]);
    assert_eq!(code, 2);
    assert_eq!(cli(&["check-bundle", "--bundle", "x"]).0, 2);
    assert_eq!(cli(&["check-bundle", "--bundle", "x", "--bundle", "y", "--plan", PLAN]).0, 2);
}

#[cfg(unix)]
#[test]
fn symlinks_are_reported_not_followed() {
    let copy = TempCopy::of("bundle-ok", "link");
    std::os::unix::fs::symlink("/etc/hostname", copy.0.join("docs/link.html")).unwrap();
    assert_eq!(
        check_bundle(&copy.0, &plan()),
        vec![BundleError::BadPath { path: "docs/link.html".into(), reason: "symlink" }]
    );
    let err =
        make_install_manifest(&copy.0, InstallKind::Machine, "0.1.0", OsFamily::Ubuntu, Arch::X86_64).unwrap_err();
    assert_eq!(err, vec![BundleError::BadPath { path: "docs/link.html".into(), reason: "symlink" }]);
}

// ------------------------------------------------------------------------------ docs scan

#[test]
fn docs_scan_finds_auto_loaded_remote_resources() {
    let html = |s: &str| scan_docs_file("docs/x.html", s.as_bytes());
    let css = |s: &str| scan_docs_file("docs/x.css", s.as_bytes());
    let js = |s: &str| scan_docs_file("docs/x.js", s.as_bytes());
    assert_eq!(
        html(r#"<link rel="stylesheet" href="https://cdn.example.com/a.css">"#),
        ["https://cdn.example.com/a.css"]
    );
    assert_eq!(html("<script src=//cdn.example.com/x.js></script>"), ["//cdn.example.com/x.js"]);
    assert_eq!(html(r#"<IMG SRCSET="a.png 1x, https://e.example/b.png 2x">"#), ["https://e.example/b.png"]);
    assert_eq!(html(r#"<iframe src=" HTTPS://x.example/">"#), ["HTTPS://x.example/"]);
    assert_eq!(html(r#"<style>@import url("http://x.example/s.css");</style>"#), ["http://x.example/s.css"]);
    assert_eq!(html(r#"<div style="background:url(https://x.example/b.png)">"#), ["https://x.example/b.png"]);
    assert_eq!(html(r#"<script>fetch("https://api.example.com/q")</script>"#), ["https://api.example.com/q"]);
    assert_eq!(
        html(r#"<img src="https://a.example/i.png"><img src="https://a.example/i.png">"#),
        ["https://a.example/i.png"]
    );
    // Not loaded on their own: links, comments, relative URLs, text.
    for quiet in [
        r#"<a href="https://x.example/">x</a>"#,
        r#"<!-- <img src="https://x.example/i.png"> -->"#,
        r#"<img src="img/a.png"><link href="style.css">"#,
        "<p>https://x.example/ is the site</p>",
        "<img src=",
        "<",
        "<img src='unterminated",
    ] {
        assert!(html(quiet).is_empty(), "{quiet}");
    }
    assert_eq!(css("@import 'https://x.example/t.css';"), ["https://x.example/t.css"]);
    assert_eq!(css("a { background: URL( http://x.example/b.png ) }"), ["http://x.example/b.png"]);
    assert!(css("/* url(https://x.example/a) */ a { background: url(b.png) }").is_empty());
    assert!(css("url(").is_empty());
    assert_eq!(js("const u = 'https://api.example.com/v';"), ["https://api.example.com/v"]);
    assert!(js("// see https://example.org for details\nconst a = 'it''s';").is_empty());
    assert!(scan_docs_file("docs/logo.png", b"<image href=\"https://x.example/i.png\"/>").is_empty());
}

// Review of PR #166, item 1: one test per added case (hand-labelled).

#[test]
fn docs_scan_base_href() {
    let html = r#"<head><base href="https://cdn.example.com/docs/"></head>"#;
    assert_eq!(scan_docs_file("docs/a.html", html.as_bytes()), ["https://cdn.example.com/docs/"]);
}

#[test]
fn docs_scan_svg_image_href() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="https://x.example/i.png"/></svg>"#;
    assert_eq!(scan_docs_file("docs/logo.svg", svg.as_bytes()), ["https://x.example/i.png"]);
    // The SVG namespace declaration alone loads nothing.
    assert!(scan_docs_file("docs/logo.svg", br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#).is_empty());
}

#[test]
fn docs_scan_svg_use_xlink_href() {
    let svg = r##"<svg><use xlink:href="https://x.example/sprite.svg#icon"/><use href="#local"/></svg>"##;
    assert_eq!(scan_docs_file("docs/icons.svg", svg.as_bytes()), ["https://x.example/sprite.svg#icon"]);
    let inline = r#"<p><svg><use href="//x.example/s.svg#i"></use></svg></p>"#;
    assert_eq!(scan_docs_file("docs/a.html", inline.as_bytes()), ["//x.example/s.svg#i"]);
}

#[test]
fn docs_scan_css_image_set_strings() {
    let css = r#"a { background-image: image-set("https://x.example/a.png" 1x, "b.png" 2x); }"#;
    assert_eq!(scan_docs_file("docs/a.css", css.as_bytes()), ["https://x.example/a.png"]);
    let prefixed = r#"a { background: -webkit-image-set(url(https://x.example/c.png) 1x, 'd.png' 2x) }"#;
    assert_eq!(scan_docs_file("docs/a.css", prefixed.as_bytes()), ["https://x.example/c.png"]);
}

#[test]
fn docs_scan_link_imagesrcset() {
    let html = r#"<link rel="preload" as="image" imagesrcset="a.png 1x, https://x.example/b.png 2x">"#;
    assert_eq!(scan_docs_file("docs/a.html", html.as_bytes()), ["https://x.example/b.png"]);
}

#[test]
fn docs_scan_meta_refresh() {
    let html = r#"<meta http-equiv="Refresh" content="0; URL='https://x.example/new'">"#;
    assert_eq!(scan_docs_file("docs/a.html", html.as_bytes()), ["https://x.example/new"]);
    let local = r#"<meta http-equiv="refresh" content="0; url=other.html">"#;
    assert!(scan_docs_file("docs/a.html", local.as_bytes()).is_empty());
    // Not a refresh: a description that mentions a URL.
    let desc = r#"<meta name="description" content="0; url=https://x.example/">"#;
    assert!(scan_docs_file("docs/a.html", desc.as_bytes()).is_empty());
}

// Review item 2: Tier 1 coverage over a set of bundles.

#[test]
fn tier1_coverage_over_a_set_of_bundles() {
    let ok = fixture("bundle-ok");
    assert_eq!(tier1_combinations(&plan()).into_iter().collect::<Vec<_>>(), [(OsFamily::Windows, Arch::X86_64)]);
    assert_eq!(check_bundles(std::slice::from_ref(&ok), &plan()), vec![]);
    assert_eq!(
        check_bundles(&[], &plan()),
        vec![SetFinding {
            bundle: None,
            error: BundleError::MissingBundle { os: OsFamily::Windows, arch: Arch::X86_64 }
        }]
    );
    assert_eq!(
        check_bundles(&[ok.clone(), ok.clone()], &plan()),
        vec![SetFinding {
            bundle: None,
            error: BundleError::DuplicateBundle { os: OsFamily::Windows, arch: Arch::X86_64 }
        }]
    );
    // Ubuntu aarch64 promoted to Tier 1: its bundle is now required.
    let mut p = plan();
    for x in p.packages.iter_mut().filter(|x| x.os == OsFamily::Ubuntu) {
        x.tier = Tier::Tier1;
    }
    assert_eq!(
        check_bundles(std::slice::from_ref(&ok), &p),
        vec![SetFinding {
            bundle: None,
            error: BundleError::MissingBundle { os: OsFamily::Ubuntu, arch: Arch::Aarch64 }
        }]
    );
    // Findings of one bundle name it.
    let missing = fixture("bundle-missing-cli");
    let found = check_bundles(std::slice::from_ref(&missing), &plan());
    assert_eq!(
        found,
        vec![SetFinding {
            bundle: Some(missing.clone()),
            error: BundleError::MissingRole { role: Role::Cli, name: None }
        }]
    );
    let (code, out) = cli(&["check-bundles", "--plan", PLAN, "--bundle", "fixtures/plat/deploy/bundle-ok"]);
    assert_eq!((code, out.as_str()), (0, "check-bundles: ok\n"));
    let (code, out) = cli(&["check-bundles", "--plan", PLAN]);
    assert_eq!((code, out.as_str()), (1, "MissingBundle { os: windows, arch: x86_64 }\n"));
}

#[test]
fn plan_tier_is_required() {
    let text = r#"{"version":"1","packages":[{"package":"fid","kind":"cli","os":"windows","arch":"x86_64"}]}"#;
    assert!(matches!(parse_plan(text.as_bytes(), "p.json"), Err(BundleError::Parse { .. })));
    let text = text.replacen(r#""arch":"x86_64""#, r#""arch":"x86_64","tier":"tier9""#, 1);
    assert!(matches!(parse_plan(text.as_bytes(), "p.json"), Err(BundleError::Parse { .. })));
}

// Review item 3: non-ASCII names are rejected, in documents and on disk.

#[test]
fn non_ascii_names_are_bad_paths() {
    assert_eq!(check_relpath("docs/caf\u{e9}.html"), Err("non_ascii"));
    assert_eq!(
        parse_files(&["caf\u{e9}"]),
        Err(BundleError::BadPath { path: "caf\u{e9}".into(), reason: "non_ascii" })
    );
    let copy = TempCopy::of("bundle-ok", "nonascii");
    std::fs::write(copy.0.join("docs/caf\u{e9}.txt"), b"x").unwrap();
    assert_eq!(
        check_bundle(&copy.0, &plan()),
        vec![BundleError::BadPath { path: "docs/caf\u{e9}.txt".into(), reason: "non_ascii" }]
    );
}

// Review item 6: sizes above 2^53 - 1 are an error, never clamped.

#[test]
fn oversized_size_is_an_error() {
    let mut m = parse_install_manifest(
        &read("fixtures/plat/deploy/bundle-ok/install/install-manifest-per_user.json"),
        "m.json",
    )
    .unwrap();
    m.files[0].size = MAX_SIZE;
    assert!(m.to_json().is_ok());
    m.files[0].size = MAX_SIZE + 1;
    assert!(matches!(m.to_json(), Err(BundleError::Parse { .. })));
    let digest = m.files[0].sha256;
    let big =
        InstallManifest { files: vec![FileEntry { relpath: "a".into(), sha256: digest, size: u64::MAX }], ..m.clone() };
    assert!(matches!(big.to_json(), Err(BundleError::Parse { .. })));
}

// Review item 7: the docs tree needs an HTML entry point.

#[test]
fn docs_need_an_html_entry_point() {
    let rel = "fixtures/plat/deploy/bundle-ok/offline-bundle.json";
    let mut b = parse_bundle(&read(rel), rel).unwrap();
    assert_eq!(check_roles(&b, &plan()), vec![]);
    b.entries.retain(|e| e.path != "docs/index.html");
    assert_eq!(
        check_roles(&b, &plan()),
        vec![BundleError::MissingRole { role: Role::Docs, name: Some("index.html".into()) }]
    );
    b.entries.retain(|e| e.role != Role::Docs);
    assert_eq!(check_roles(&b, &plan()), vec![BundleError::MissingRole { role: Role::Docs, name: None }]);
}

// Review item 4: a Windows junction is reported, never followed.

#[cfg(windows)]
#[test]
fn windows_junction_is_reported_not_followed() {
    let copy = TempCopy::of("bundle-ok", "junction");
    let link = copy.0.join("docs-junction");
    let target = copy.0.join("docs");
    let status = Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(&target).output().unwrap();
    assert!(status.status.success(), "mklink /J failed: {}", String::from_utf8_lossy(&status.stderr));
    let expected = vec![BundleError::BadPath { path: "docs-junction".into(), reason: "symlink" }];
    assert_eq!(check_bundle(&copy.0, &plan()), expected);
    let err =
        make_install_manifest(&copy.0, InstallKind::Machine, "0.1.0", OsFamily::Windows, Arch::X86_64).unwrap_err();
    assert_eq!(err, expected);
    // Remove the junction itself (not its target) before the copy is deleted.
    std::fs::remove_dir(&link).unwrap();
}

// Review item 8: make-install-manifest --out replaces the file whole and leaves no temporary.

#[test]
fn make_manifest_out_is_atomic() {
    let dir = std::env::temp_dir().join(format!("fid-bundle-test-{}-atomic", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("install-manifest.json");
    std::fs::write(&out, b"old contents").unwrap();
    let out_s = out.to_str().unwrap();
    let args = [
        "make-install-manifest",
        "--root",
        "fixtures/plat/deploy/install-tree",
        "--kind",
        "machine",
        "--release",
        "0.1.0",
        "--os",
        "windows",
        "--arch",
        "x86_64",
        "--out",
        out_s,
    ];
    assert_eq!(cli(&args).0, 0);
    assert_eq!(
        std::fs::read(&out).unwrap(),
        read("fixtures/plat/deploy/bundle-ok/install/install-manifest-machine.json")
    );
    let names: Vec<String> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["install-manifest.json"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn remote_url_detection() {
    for remote in ["http://a", "https://a", "//a", " HTTPS://a", "\\\\a", "/\\a", "ht\ttps://a"] {
        assert!(is_remote(remote), "{remote:?}");
    }
    for local in ["a.css", "/a.css", "./a", "data:font/woff2;base64,AA", "file:///a", "#x", ""] {
        assert!(!is_remote(local), "{local:?}");
    }
}
