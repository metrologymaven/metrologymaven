//! Acceptance criteria of spec node OUTR.DOCS.FRAME.NAV (AC-1 .. AC-5) and edge cases.
//! All fixtures under `fixtures/outr/docs/frame/` are synthetic and written by `make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_outreach::docs_build::frame::{EDITION_NOTICE_PLACEHOLDER, FRAME_CSS_PATH};
use fid_outreach::docs_build::{is_static_path, MAX_SOURCE_BYTES};
use fid_outreach::qa::a11y;
use fid_outreach::qa::a11y::html::{parse as parse_html, Node};
use fid_outreach::search_index;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const EXPECTED_HTML_PAGES: [&str; 6] = [
    "book/index.html",
    "book/replay.html",
    "book/units.html",
    "contributor/guide.html",
    "ref/cli/demo.read_item@1.html",
    "ref/mcp/demo.read_item@1.html",
];

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/docs/frame").join(rel)
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn bin(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(args).output().unwrap()
}

fn show(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-docs-build-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run_build(src: &Path, ref_dir: &Path, release: &str, out: &Path, guides: Option<&Path>) -> Output {
    let mut args = vec![
        "docs-build".to_string(),
        "--src".into(),
        s(src),
        "--ref".into(),
        s(ref_dir),
        "--release".into(),
        release.to_string(),
        "--out".into(),
        s(out),
    ];
    if let Some(g) = guides {
        args.push("--guides".into());
        args.push(s(g));
    }
    bin(&args)
}

fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, d: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut m = BTreeMap::new();
    walk(dir, dir, &mut m);
    m
}

fn copy_dir(from: &Path, to: &Path) {
    for (rel, bytes) in tree(from) {
        let p = to.join(&rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
}

/// Resolves a relative link `href` (without `#fragment`) against a page path relative to the
/// edition root, returning the resolved edition-relative path or `None` if it escapes the root.
fn resolve_edition_path(page_rel: &str, href_path: &str) -> Option<String> {
    let mut stack: Vec<&str> = match page_rel.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => dir.split('/').collect(),
        _ => Vec::new(),
    };
    for seg in href_path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop()?;
            }
            other => stack.push(other),
        }
    }
    Some(stack.join("/"))
}

fn file_url(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap();
    let raw = canonical.to_string_lossy().replace('\\', "/");
    if raw.starts_with('/') {
        format!("file://{raw}")
    } else {
        format!("file:///{raw}")
    }
}

#[test]
fn ac1_builds_six_framed_pages_with_banner_and_notice_placeholder() {
    let root = tmp("ac1");
    let site = root.join("site");
    let o = run_build(&fix("src"), &fix("ref"), "v0.2.0", &site, None);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));

    let edition = site.join("v0.2.0");
    let files = tree(&edition);

    // Exactly six HTML pages, plus frame.css and assets/mark.svg.
    let html_pages: Vec<String> = files.keys().filter(|k| k.ends_with(".html")).cloned().collect();
    assert_eq!(html_pages, EXPECTED_HTML_PAGES);
    assert!(files.contains_key(FRAME_CSS_PATH));
    assert!(files.contains_key("assets/mark.svg"));

    // REQ-006: every written file is in the static allowlist and there are zero <script> elements.
    for rel in files.keys() {
        assert!(is_static_path(rel), "non-static output file: {rel}");
    }

    // REQ-001, REQ-002, REQ-003: every HTML page shares the frame, shows `v0.2.0` in the banner,
    // and places `<!-- edition-notice -->` directly below the banner.
    for rel in &html_pages {
        let html = String::from_utf8(files[rel].clone()).unwrap();
        assert!(html.contains("<header class=\"site-header\">"), "{rel} missing header");
        assert!(html.contains("<nav class=\"site-nav\""), "{rel} missing nav");
        assert!(html.contains("<main id=\"main-content\">"), "{rel} missing main");
        assert!(html.contains("<footer class=\"site-footer\">"), "{rel} missing footer");
        assert!(!html.contains("<script"), "{rel} must not contain raw <script>");

        let banner_needle = "<div class=\"release-banner\" role=\"region\" aria-label=\"Release\">Documentation for release <strong class=\"release-tag\">v0.2.0</strong></div>\n<!-- edition-notice -->\n";
        assert!(
            html.contains(banner_needle),
            "{rel} missing release banner immediately followed by {EDITION_NOTICE_PLACEHOLDER}"
        );
    }

    // Verify Markdown rendering details on book/index.html and book/units.html.
    let book_index = String::from_utf8(files["book/index.html"].clone()).unwrap();
    assert!(book_index.contains("<!-- cap: DEMO.CAP.01 -->"));
    assert!(book_index.contains("<!-- cap-planned: DEMO.CAP.02 -->"));
    assert!(book_index.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(book_index.contains("&lt;!-- not-a-cap --&gt;"));
    assert!(book_index.contains("href=\"units.html\""));
    assert!(book_index.contains("href=\"replay.html#exact-mode\""));

    let book_units = String::from_utf8(files["book/units.html"].clone()).unwrap();
    assert!(book_units.contains("<table>"));
    assert!(book_units.contains("<th>Quantity</th>"));
    assert!(book_units.contains("<pre><code class=\"language-sh\">fid demo read-item --unit mm\n</code></pre>"));
    assert!(book_units.contains("demo.read_item_two"));

    // Reference fragments are embedded raw (not mangled by Markdown).
    let ref_cli = String::from_utf8(files["ref/cli/demo.read_item@1.html"].clone()).unwrap();
    assert!(ref_cli.contains("<article class=\"ref-page ref-cli\">"));
    assert!(ref_cli.contains("<dl class=\"ref-facts\">"));
}

#[test]
fn ac2_every_href_and_src_is_relative_and_ends_in_filename() {
    let root = tmp("ac2");
    let site = root.join("site");
    let o = run_build(&fix("src"), &fix("ref"), "v0.2.0", &site, None);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));

    let edition = site.join("v0.2.0");
    let files = tree(&edition);

    // Pre-collect all element IDs per HTML page to validate fragment targets too.
    let mut ids_by_page: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for rel in EXPECTED_HTML_PAGES {
        let html = String::from_utf8(files[rel].clone()).unwrap();
        let doc = parse_html(&html).unwrap();
        let mut ids = BTreeSet::new();
        for node in &doc.nodes {
            if let Node::Element(el) = node {
                if let Some(id) = el.attr("id") {
                    ids.insert(id.to_string());
                }
            }
        }
        ids_by_page.insert(rel.to_string(), ids);
    }

    let mut total_links = 0usize;
    for rel in EXPECTED_HTML_PAGES {
        let html = String::from_utf8(files[rel].clone()).unwrap();
        let doc = parse_html(&html).unwrap();
        for node in &doc.nodes {
            let Node::Element(el) = node else { continue };
            for attr in ["href", "src"] {
                let Some(val) = el.attr(attr) else { continue };
                total_links += 1;
                assert!(!val.starts_with('/'), "{rel}: {attr}=\"{val}\" is absolute or protocol-relative");
                assert!(!val.starts_with("//"), "{rel}: {attr}=\"{val}\" is protocol-relative");
                assert!(!val.contains("://"), "{rel}: {attr}=\"{val}\" has a URL scheme");
                assert!(!val.ends_with('/'), "{rel}: {attr}=\"{val}\" ends in '/'");

                if let Some(frag) = val.strip_prefix('#') {
                    assert!(ids_by_page[rel].contains(frag), "{rel}: in-page fragment #{frag} has no matching id");
                    continue;
                }
                let (path_part, frag) = match val.split_once('#') {
                    Some((p, f)) => (p, Some(f)),
                    None => (val, None),
                };
                assert!(!path_part.ends_with('/'), "{rel}: {attr}=\"{val}\" path ends in '/'");
                let resolved = resolve_edition_path(rel, path_part)
                    .unwrap_or_else(|| panic!("{rel}: {attr}=\"{val}\" escapes edition root"));
                assert!(files.contains_key(&resolved), "{rel}: {attr}=\"{val}\" resolved to missing file {resolved}");
                if let Some(f) = frag {
                    assert!(
                        ids_by_page.get(&resolved).is_some_and(|set| set.contains(f)),
                        "{rel}: {attr}=\"{val}\" fragment #{f} missing in {resolved}"
                    );
                }
            }
        }
    }
    assert!(total_links >= 48, "expected many frame + nav + content links, saw {total_links}");
}

fn find_headless_chrome() -> Option<&'static str> {
    ["google-chrome", "chromium", "chromium-browser"]
        .into_iter()
        .find(|&candidate| Command::new(candidate).arg("--version").output().is_ok_and(|o| o.status.success()))
}

fn dump_dom_offline(browser: &str, url: &str) -> String {
    let out = Command::new(browser)
        .args([
            "--headless",
            "--no-sandbox",
            "--disable-gpu",
            "--disable-background-networking",
            "--proxy-server=http://127.0.0.1:1",
            "--host-resolver-rules=MAP * ~NOTFOUND",
            "--dump-dom",
            url,
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "headless browser failed on {url}: {}", show(&out));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn ac3_navigation_links_load_via_file_url_offline() {
    let root = tmp("ac3");
    let site = root.join("site");
    let o = run_build(&fix("src"), &fix("ref"), "v0.2.0", &site, None);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));

    let edition = site.join("v0.2.0");

    // 1. Always verify every navigation link from every page resolves cleanly on disk.
    for page_rel in EXPECTED_HTML_PAGES {
        let html = std::fs::read_to_string(edition.join(page_rel)).unwrap();
        let doc = parse_html(&html).unwrap();
        let nav_hrefs = extract_nav_hrefs(&doc);
        assert_eq!(nav_hrefs.len(), 6, "{page_rel}: expected 6 nav links, got {nav_hrefs:?}");
        for href in &nav_hrefs {
            let target_rel = resolve_edition_path(page_rel, href).unwrap();
            let target_path = edition.join(&target_rel);
            assert!(target_path.is_file(), "{page_rel}: nav link {href} -> {target_rel} missing");
        }
    }

    // 2. Open the edition through `file://` in a headless browser with networking disabled and
    // follow every navigation link.
    if let Some(browser) = find_headless_chrome() {
        let start_rel = "book/index.html";
        let start_dom = dump_dom_offline(browser, &file_url(&edition.join(start_rel)));
        assert!(start_dom.contains("User guide introduction"));
        assert!(start_dom.contains("v0.2.0"));

        let start_doc = parse_html(&start_dom).unwrap();
        let nav_hrefs = extract_nav_hrefs(&start_doc);
        assert_eq!(nav_hrefs.len(), 6, "headless DOM should show all 6 navigation links");

        let mut loaded_targets = BTreeSet::new();
        for href in nav_hrefs {
            let target_rel = resolve_edition_path(start_rel, &href).unwrap();
            let target_dom = dump_dom_offline(browser, &file_url(&edition.join(&target_rel)));
            assert!(
                target_dom.contains("<main id=\"main-content\">") && target_dom.contains("v0.2.0"),
                "target {target_rel} failed to load cleanly in headless browser"
            );
            loaded_targets.insert(target_rel);
        }
        let expected: BTreeSet<String> = EXPECTED_HTML_PAGES.iter().map(|p| (*p).to_string()).collect();
        assert_eq!(loaded_targets, expected);
    }
}

fn extract_nav_hrefs(doc: &fid_outreach::qa::a11y::html::Doc) -> Vec<String> {
    let mut hrefs = Vec::new();
    for (idx, node) in doc.nodes.iter().enumerate() {
        let Node::Element(el) = node else { continue };
        if el.name == "a" {
            let in_nav = doc.ancestors(idx).into_iter().any(|anc| doc.element(anc).is_some_and(|e| e.name == "nav"));
            if in_nav {
                if let Some(h) = el.attr("href") {
                    hrefs.push(h.to_string());
                }
            }
        }
    }
    hrefs
}

#[test]
fn ac4_nav_missing_exits_1_names_source_and_writes_nothing() {
    let root = tmp("ac4");
    let site = root.join("site");
    let o = run_build(&fix("nav-missing"), &fix("ref"), "v0.2.0", &site, None);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    let out_text = show(&o);
    assert!(out_text.contains("book/ghost.md"), "error must name book/ghost.md: {out_text}");
    assert!(!site.join("v0.2.0").exists(), "must write nothing on NavSourceMissing");
    assert!(!site.exists(), "must not create output directory on NavSourceMissing");
}

#[test]
fn ac5_two_builds_are_byte_identical() {
    let root = tmp("ac5");
    let site_a = root.join("site-a");
    let site_b = root.join("site-b");
    let oa = run_build(&fix("src"), &fix("ref"), "v0.2.0", &site_a, None);
    let ob = run_build(&fix("src"), &fix("ref"), "v0.2.0", &site_b, None);
    assert_eq!(oa.status.code(), Some(0), "{}", show(&oa));
    assert_eq!(ob.status.code(), Some(0), "{}", show(&ob));
    assert_eq!(tree(&site_a), tree(&site_b));
}

#[test]
fn refuses_non_empty_edition_directory_and_leaves_existing_files_untouched() {
    let root = tmp("exists");
    let site = root.join("site");
    let edition = site.join("v0.2.0");
    std::fs::create_dir_all(&edition).unwrap();
    std::fs::write(edition.join("keep.txt"), b"original").unwrap();

    let o = run_build(&fix("src"), &fix("ref"), "v0.2.0", &site, None);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert!(show(&o).contains("output_exists"));
    assert_eq!(tree(&edition), BTreeMap::from([("keep.txt".to_string(), b"original".to_vec())]));
}

#[test]
fn allows_existing_empty_edition_directory() {
    let root = tmp("empty-dir");
    let site = root.join("site");
    std::fs::create_dir_all(site.join("v0.2.0")).unwrap();

    let o = run_build(&fix("src"), &fix("ref"), "v0.2.0", &site, None);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    assert!(site.join("v0.2.0/book/index.html").is_file());
}

#[test]
fn refuses_non_static_asset_and_writes_nothing() {
    let root = tmp("non-static");
    let src = root.join("src");
    copy_dir(&fix("src"), &src);
    std::fs::write(src.join("assets/handler.php"), b"<?php echo 1; ?>").unwrap();
    let site = root.join("site");

    let o = run_build(&src, &fix("ref"), "v0.2.0", &site, None);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    let msg = show(&o);
    assert!(msg.contains("handler.php"), "error should name offending asset: {msg}");
    assert!(!site.exists(), "must write nothing when a non-static asset is present");
}

#[test]
fn guides_option_accepted_and_unreadable_guides_fails() {
    let root = tmp("guides");
    let guides = root.join("guides.toml");
    std::fs::write(&guides, "version = 1\n").unwrap();
    let site = root.join("site");

    let ok = run_build(&fix("src"), &fix("ref"), "v0.2.0", &site, Some(&guides));
    assert_eq!(ok.status.code(), Some(0), "{}", show(&ok));

    let site2 = root.join("site2");
    let bad = run_build(&fix("src"), &fix("ref"), "v0.2.0", &site2, Some(&root.join("missing.toml")));
    assert_eq!(bad.status.code(), Some(2), "{}", show(&bad));
    assert!(!site2.exists());
}

#[test]
fn rejects_oversized_source_and_invalid_release_tag() {
    let root = tmp("bounds");
    let src = root.join("src");
    copy_dir(&fix("src"), &src);
    std::fs::write(src.join("book/units.md"), vec![b'a'; MAX_SOURCE_BYTES + 1]).unwrap();
    let site = root.join("site");

    let big = run_build(&src, &fix("ref"), "v0.2.0", &site, None);
    assert_eq!(big.status.code(), Some(2), "{}", show(&big));
    assert!(!site.exists());

    let bad_rel = run_build(&fix("src"), &fix("ref"), "../escape", &site, None);
    assert_eq!(bad_rel.status.code(), Some(2), "{}", show(&bad_rel));
    assert!(!site.exists());

    let bad_usage = bin(&["docs-build".into(), "--src".into(), s(&fix("src"))]);
    assert_eq!(bad_usage.status.code(), Some(2));
}

#[test]
fn built_edition_passes_a11y_gate_and_search_index() {
    let root = tmp("interop");
    let site = root.join("site");
    let o = run_build(&fix("src"), &fix("ref"), "v0.2.0", &site, None);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));

    let edition = site.join("v0.2.0");

    // Passes `qa a11y` with zero violations using the project's clean token set.
    let tokens = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/site/qa/a11y/clean/tokens.json");
    let report = a11y::run(&a11y::Options { root: edition.clone(), tokens, config: None }).unwrap();
    assert!(report.violations.is_empty(), "a11y violations: {:?}", report.violations);
    assert_eq!(report.pages, 6);

    // Indexes cleanly with `search-index` and populates exact command names.
    let budget = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/docs/search/search-budget.toml");
    let outcome = search_index::run(&edition, &budget).unwrap();
    assert_eq!(outcome.exit_code(), 0, "{}", outcome.render());
    assert!(edition.join("search-index.js").is_file());
}
