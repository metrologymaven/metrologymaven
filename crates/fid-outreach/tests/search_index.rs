//! Acceptance criteria of spec node OUTR.DOCS.SEARCH.INDEX (AC-1 .. AC-5) and edge cases.
//! The fixture edition under `fixtures/outr/docs/search/edition` is hand-written; the expected
//! index `fixtures/outr/docs/search/tiny/expected-search-index.js` and every expected value below
//! are hand-labelled from the spec's rules. Nothing expected here was produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_outreach::search_index::{cli_path, js_safe, mcp_name, names_table, read_html, tokenize, url_of};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ALIGN: [&str; 3] =
    ["ref/cli/align.run_bestfit@1.html", "ref/mcp/align.run_bestfit@1.html", "ref/python/align.run_bestfit@1.html"];
const REGISTRY: [&str; 3] = [
    "ref/cli/registry.show_manifest@1.html",
    "ref/mcp/registry.show_manifest@1.html",
    "ref/python/registry.show_manifest@1.html",
];
const PREFIX: &str = "window.FID_SEARCH_INDEX = ";
const SUFFIX: &str = ";\n";

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/docs/search").join(rel)
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
    let d = std::env::temp_dir().join(format!("fid-search-index-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), &target).unwrap();
        }
    }
}

/// `<tmp>/site/v0.2.0` holding a copy of the fixture edition, plus a sibling edition
/// `<tmp>/site/v0.1.0` whose pages must never appear in the v0.2.0 index.
fn site(tag: &str) -> PathBuf {
    let root = tmp(tag);
    let edition = root.join("site/v0.2.0");
    copy_dir(&fix("edition/v0.2.0"), &edition);
    let old = root.join("site/v0.1.0/book");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("old.html"), "<title>Old edition</title><p>siblingeditionword</p>").unwrap();
    edition
}

fn run(edition: &Path, budget: &Path, json: bool) -> Output {
    let mut args = vec!["search-index".to_string(), "--edition".into(), s(edition), "--budget".into(), s(budget)];
    if json {
        args.push("--json".into());
    }
    bin(&args)
}

/// Checks the single-global-assignment shape and returns the parsed object.
fn load(edition: &Path) -> BTreeMap<String, CValue> {
    let bytes = std::fs::read(edition.join("search-index.js")).unwrap();
    assert!(bytes.is_ascii(), "index must be pure ASCII");
    let text = String::from_utf8(bytes).unwrap();
    let json = text.strip_prefix(PREFIX).unwrap().strip_suffix(SUFFIX).unwrap();
    for bad in ['<', '>', '&', '\n'] {
        assert!(!json.contains(bad), "unescaped {bad:?}");
    }
    match fid_canon::parse(json.as_bytes()).unwrap() {
        CValue::Obj(o) => o,
        other => panic!("not an object: {other:?}"),
    }
}

fn strs(v: &CValue) -> Vec<String> {
    match v {
        CValue::Arr(a) => a
            .iter()
            .map(|x| match x {
                CValue::Str(s) => s.clone(),
                other => panic!("not a string: {other:?}"),
            })
            .collect(),
        other => panic!("not an array: {other:?}"),
    }
}

fn obj<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> &'a BTreeMap<String, CValue> {
    match o.get(key) {
        Some(CValue::Obj(m)) => m,
        other => panic!("{key} is not an object: {other:?}"),
    }
}

fn doc_urls(o: &BTreeMap<String, CValue>) -> Vec<String> {
    let Some(CValue::Arr(docs)) = o.get("docs") else { panic!("docs") };
    docs.iter()
        .map(|d| match d {
            CValue::Obj(m) => match m.get("u") {
                Some(CValue::Str(u)) => u.clone(),
                other => panic!("u: {other:?}"),
            },
            other => panic!("doc: {other:?}"),
        })
        .collect()
}

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        other => panic!("not a number: {other:?}"),
    }
}

/// Weight of `token` in the doc with URL `url`, if indexed there.
fn weight(o: &BTreeMap<String, CValue>, token: &str, url: &str) -> Option<f64> {
    let doc = doc_urls(o).iter().position(|u| u == url).unwrap() as f64;
    let Some(CValue::Arr(posts)) = obj(o, "terms").get(token) else { return None };
    posts.iter().find_map(|p| match p {
        CValue::Arr(pair) if num(&pair[0]) == doc => Some(num(&pair[1])),
        _ => None,
    })
}

#[test]
fn ac1_writes_single_global_assignment() {
    let edition = site("ac1");
    let o = run(&edition, &fix("search-budget.toml"), false);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let idx = load(&edition);
    assert_eq!(num(idx.get("v").unwrap()), 1.0);
    let keys: Vec<&str> = idx.keys().map(String::as_str).collect();
    assert_eq!(keys, ["docs", "names", "terms", "v"]);
    // Ten book pages and three pages for each of the two commands.
    let urls = doc_urls(&idx);
    assert_eq!(urls.len(), 16);
    assert_eq!(urls.iter().filter(|u| u.starts_with("book/")).count(), 10);
    let mut sorted = urls.clone();
    sorted.sort();
    assert_eq!(urls, sorted, "docs are in URL order");
}

#[test]
fn ac2_exact_names_map_to_the_three_pages() {
    let edition = site("ac2");
    assert_eq!(run(&edition, &fix("search-budget.toml"), false).status.code(), Some(0));
    let idx = load(&edition);
    let names = obj(&idx, "names");
    for key in ["align.run_bestfit", "align.run_bestfit@1", "fid align run-bestfit", "align_run_bestfit"] {
        assert_eq!(strs(names.get(key).unwrap()), ALIGN, "{key}");
    }
    for key in
        ["registry.show_manifest", "registry.show_manifest@1", "fid registry show-manifest", "registry_show_manifest"]
    {
        assert_eq!(strs(names.get(key).unwrap()), REGISTRY, "{key}");
    }
    assert_eq!(names.len(), 8, "{:?}", names.keys().collect::<Vec<_>>());
}

#[test]
fn ac3_urls_are_relative_and_inside_the_edition() {
    let edition = site("ac3");
    assert_eq!(run(&edition, &fix("search-budget.toml"), false).status.code(), Some(0));
    let idx = load(&edition);
    let mut all = doc_urls(&idx);
    for v in obj(&idx, "names").values() {
        all.extend(strs(v));
    }
    for u in &all {
        assert!(!u.starts_with('/') && !u.contains(':') && !u.contains('\\'), "{u}");
        assert!(u.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != ".."), "{u}");
        assert!(edition.join(u).is_file(), "{u} is not a page of v0.2.0");
    }
    assert!(!obj(&idx, "terms").contains_key("siblingeditionword"), "another edition leaked in");
}

#[test]
fn ac4_over_budget_exits_1_with_size_and_budget() {
    let edition = site("ac4");
    let o = run(&edition, &fix("budget-1k.toml"), false);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(out.starts_with("over_budget "), "{out}");
    assert!(out.contains("1024 bytes"), "{out}");
    assert!(!edition.join("search-index.js").exists(), "nothing is written when over budget");

    // The size stated is the size the file has when written under a large budget.
    assert_eq!(run(&edition, &fix("search-budget.toml"), false).status.code(), Some(0));
    let size = std::fs::metadata(edition.join("search-index.js")).unwrap().len();
    assert!(out.contains(&format!("{size} bytes")), "{out}");

    let o = run(&edition, &fix("budget-1k.toml"), true);
    assert_eq!(o.status.code(), Some(1));
    let CValue::Obj(rep) = fid_canon::parse(String::from_utf8_lossy(&o.stdout).trim().as_bytes()).unwrap() else {
        panic!()
    };
    assert_eq!(rep.get("ok"), Some(&CValue::Bool(false)));
    assert_eq!(num(rep.get("bytes").unwrap()), size as f64);
    assert_eq!(num(rep.get("max_bytes").unwrap()), 1024.0);
    let Some(CValue::Arr(findings)) = rep.get("findings") else { panic!() };
    let CValue::Obj(f) = &findings[0] else { panic!() };
    assert_eq!(f.get("code"), Some(&CValue::Str("over_budget".into())));
}

#[test]
fn ac5_two_runs_are_byte_identical() {
    let a = site("ac5a");
    assert_eq!(run(&a, &fix("search-budget.toml"), false).status.code(), Some(0));
    let first = std::fs::read(a.join("search-index.js")).unwrap();
    // A second run on the same edition (which now also holds the index file) ...
    assert_eq!(run(&a, &fix("search-budget.toml"), false).status.code(), Some(0));
    assert_eq!(first, std::fs::read(a.join("search-index.js")).unwrap());
    // ... and a run on a copy at another path give the same bytes.
    let b = site("ac5b");
    assert_eq!(run(&b, &fix("search-budget.toml"), false).status.code(), Some(0));
    assert_eq!(first, std::fs::read(b.join("search-index.js")).unwrap());
}

#[test]
fn tiny_edition_matches_hand_labelled_index() {
    let root = tmp("tiny");
    copy_dir(&fix("tiny/edition"), &root);
    let o = run(&root, &fix("search-budget.toml"), false);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let got = std::fs::read(root.join("search-index.js")).unwrap();
    let want = std::fs::read(fix("tiny/expected-search-index.js")).unwrap();
    assert_eq!(String::from_utf8_lossy(&got), String::from_utf8_lossy(&want));
}

#[test]
fn weights_and_skipped_content() {
    let edition = site("weights");
    assert_eq!(run(&edition, &fix("search-budget.toml"), false).status.code(), Some(0));
    let idx = load(&edition);
    let terms = obj(&idx, "terms");
    for hidden in ["navonlyword", "scriptonlyword", "styleonlyword", "commentonlyword"] {
        assert!(!terms.contains_key(hidden), "{hidden} must not be indexed");
    }
    // Title 3, heading 2, body 1; the highest field wins per page.
    assert_eq!(weight(&idx, "datums", "book/datums.html"), Some(3.0));
    assert_eq!(weight(&idx, "frames", "book/datums.html"), Some(2.0));
    assert_eq!(weight(&idx, "datums", "book/index.html"), Some(1.0));
    assert_eq!(weight(&idx, "bestfit", "book/glossary.html"), Some(2.0));
    assert_eq!(weight(&idx, "tolerances", "book/units.html"), Some(3.0));
    assert_eq!(weight(&idx, "si", "book/units.html"), Some(1.0));
    assert_eq!(weight(&idx, "not_replayable", "book/replay.html"), Some(1.0));
    assert_eq!(weight(&idx, "least-squares", "book/glossary.html"), Some(1.0));
    assert_eq!(weight(&idx, "align.run_bestfit@1", "ref/mcp/align.run_bestfit@1.html"), Some(3.0));
    assert_eq!(weight(&idx, "run-bestfit", "book/alignment.html"), Some(1.0));
    // Ref pages have no <title>: the first heading is the title.
    let Some(CValue::Arr(docs)) = idx.get("docs") else { panic!() };
    let pos = doc_urls(&idx).iter().position(|u| u == ALIGN[0]).unwrap();
    let CValue::Obj(d) = &docs[pos] else { panic!() };
    assert_eq!(d.get("t"), Some(&CValue::Str("align.run_bestfit@1".into())));
    assert_eq!(strs(d.get("h").unwrap()), ["align.run_bestfit@1", "CLI"]);
    let pos = doc_urls(&idx).iter().position(|u| u == "book/units.html").unwrap();
    let CValue::Obj(d) = &docs[pos] else { panic!() };
    assert_eq!(d.get("t"), Some(&CValue::Str("Units & tolerances".into())));
}

#[test]
fn tokenizer_rules() {
    assert_eq!(tokenize("--json a.b. x@1 run_bestfit a -x- 1"), ["json", "a.b", "x@1", "run_bestfit"]);
    assert_eq!(tokenize("Fid ALIGN Run-BestFit"), ["fid", "align", "run-bestfit"]);
    // Only ASCII letters are lower-cased: no locale or Unicode case folding.
    assert_eq!(tokenize("STRASSE Straße İSTANBUL"), ["strasse", "straße", "İstanbul"]);
    // Non-ASCII punctuation separates words; non-ASCII letters join them.
    assert_eq!(tokenize("probe\u{2014}stylus «datum» naïve"), ["probe", "stylus", "datum", "naïve"]);
    let long = "a".repeat(65);
    assert!(tokenize(&long).is_empty());
    assert_eq!(tokenize(&"b".repeat(64)).len(), 1);
}

#[test]
fn html_reader_bounds_and_entities() {
    let p = read_html(
        "<TITLE>A &amp; B</TITLE><NAV><h2>hidden</h2></NAV><H2 class='x>y'>Head &#x41;&#66;</H2>\
         <p>x < y &bogus; &#0;</p><script>a</SCRIPT>tail<!-- c --",
    );
    assert_eq!(p.title, "A & B");
    assert_eq!(p.headings, ["Head AB"]);
    assert!(p.body.contains("x < y &bogus; \u{fffd}"), "{:?}", p.body);
    assert!(p.body.contains("tail") && !p.body.contains("hidden") && !p.body.contains('c'));
    // Unterminated tags and raw text end the page without a panic.
    let _ = read_html("<p class=\"never closed");
    let _ = read_html("<script>never closed");
    let _ = read_html("<");
    let _ = read_html("&#xFFFFFFFFFF;&#99999999999;");
}

#[test]
fn script_text_is_ascii_and_cannot_close_a_script_element() {
    assert_eq!(js_safe("\"</script>\u{2028}\u{1F600}&\""), "\"\\u003c/script\\u003e\\u2028\\ud83d\\ude00\\u0026\"");
}

#[test]
fn names_follow_constitution_section_3() {
    assert_eq!(cli_path("align.run_bestfit"), "fid align run-bestfit");
    assert_eq!(cli_path("doctor"), "fid doctor");
    assert_eq!(mcp_name("align.run_bestfit", 1), "align_run_bestfit");
    assert_eq!(mcp_name("align.run_bestfit", 2), "align_run_bestfit_v2");
    let rels: Vec<String> = [
        "book/x.html",
        "ref/cli/x.do_it@1.html",
        "ref/cli/x.do_it@2.html",
        "ref/mcp/x.do_it@2.html",
        "ref/cli/Bad@1.html",
        "ref/other/x.y@1.html",
        "ref/cli/sub/x.y@1.html",
        "ref/cli/x.y.html",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let t = names_table(&rels);
    let v = |k: &str| t.get(k).cloned().unwrap_or_default();
    let newest_first = ["ref/cli/x.do_it@2.html", "ref/mcp/x.do_it@2.html", "ref/cli/x.do_it@1.html"];
    assert_eq!(v("x.do_it"), newest_first);
    assert_eq!(v("fid x do-it"), newest_first);
    assert_eq!(v("x.do_it@2"), ["ref/cli/x.do_it@2.html", "ref/mcp/x.do_it@2.html"]);
    assert_eq!(v("x_do_it_v2"), ["ref/cli/x.do_it@2.html", "ref/mcp/x.do_it@2.html"]);
    assert_eq!(v("x.do_it@1"), ["ref/cli/x.do_it@1.html"]);
    assert_eq!(v("x_do_it"), ["ref/cli/x.do_it@1.html"]);
    assert_eq!(t.len(), 6, "{:?}", t.keys().collect::<Vec<_>>());
    assert_eq!(url_of("a b/\u{fc}.html"), "a%20b/%C3%BC.html");
}

#[test]
fn errors_exit_2() {
    let root = tmp("errors");
    let o = run(&root.join("missing"), &fix("search-budget.toml"), true);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stdout).contains("edition_unreadable"), "{}", show(&o));

    let edition = site("errors-budget");
    for (name, body) in [
        ("bad.toml", "max_bytes = \"5\"\n"),
        ("zero.toml", "max_bytes = 0\n"),
        ("none.toml", "x = 1\n"),
        ("broken.toml", "max_bytes = = 1\n"),
    ] {
        let p = root.join(name);
        std::fs::write(&p, body).unwrap();
        let o = run(&edition, &p, false);
        assert_eq!(o.status.code(), Some(2), "{name}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("budget_parse"), "{name}: {}", show(&o));
    }
    assert!(!edition.join("search-index.js").exists());

    for args in [
        vec!["search-index".to_string()],
        vec!["search-index".into(), "--edition".into(), s(&edition)],
        vec![
            "search-index".into(),
            "--edition".into(),
            s(&edition),
            "--edition".into(),
            s(&edition),
            "--budget".into(),
            s(&fix("search-budget.toml")),
        ],
        vec!["search-index".into(), "--bogus".into()],
    ] {
        assert_eq!(bin(&args).status.code(), Some(2), "{args:?}");
    }
}
