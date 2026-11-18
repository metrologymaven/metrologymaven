//! Acceptance criteria of spec node OUTR.BRAND.LINT (AC-1 .. AC-8, AC-10) and REQ-006.
//! Fixtures under `fixtures/outr/lint/` are hand-written; the expected results below are
//! hand-labelled from the spec's rules, never produced by running this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_brand_lint::{extract, run, Format, Output};
use fid_canon::{parse, CValue};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn fix(name: &str) -> String {
    root().join("fixtures/outr/lint").join(name).to_string_lossy().into_owned()
}
fn rules() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("rules/claim-rules.toml").to_string_lossy().into_owned()
}
fn lint(extra: &[&str], files: &[&str]) -> Output {
    let mut args: Vec<String> = vec!["claim-lint".into(), "--rules".into(), rules(), "--json".into()];
    args.extend(extra.iter().map(|s| s.to_string()));
    args.extend(files.iter().map(|f| fix(f)));
    run(&args)
}
fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}
fn arr<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a Vec<CValue> {
    match o.get(k) {
        Some(CValue::Arr(a)) => a,
        other => panic!("{k}: expected array, got {other:?}"),
    }
}
fn st(o: &BTreeMap<String, CValue>, k: &str) -> String {
    match o.get(k) {
        Some(CValue::Str(s)) => s.clone(),
        other => panic!("{k}: expected string, got {other:?}"),
    }
}
fn num(o: &BTreeMap<String, CValue>, k: &str) -> u32 {
    match o.get(k) {
        Some(CValue::Num(n)) => *n as u32,
        other => panic!("{k}: expected number, got {other:?}"),
    }
}
fn strs(o: &BTreeMap<String, CValue>, k: &str) -> Vec<String> {
    arr(o, k)
        .iter()
        .map(|v| match v {
            CValue::Str(s) => s.clone(),
            other => panic!("expected string, got {other:?}"),
        })
        .collect()
}
fn report(out: &Output) -> BTreeMap<String, CValue> {
    obj(&parse(&out.stdout).unwrap_or_else(|e| panic!("report JSON: {e:?}; stderr: {}", out.stderr))).clone()
}
/// (rule, line, col, matched) of every finding.
fn findings(out: &Output) -> Vec<(String, u32, u32, Vec<String>)> {
    let r = report(out);
    arr(&r, "findings")
        .iter()
        .map(|f| {
            let f = obj(f);
            (st(f, "rule"), num(f, "line"), num(f, "col"), strs(f, "matched"))
        })
        .collect()
}

#[test]
fn ac1_cl001_fail() {
    let out = lint(&[], &["cl001-fail.md"]);
    assert_eq!(out.code, 1, "{}", out.stderr);
    assert_eq!(findings(&out), vec![("CL001".to_string(), 1, 1, vec!["certified".to_string()])]);
}

#[test]
fn ac2_cl001_pass() {
    let out = lint(&[], &["cl001-pass.md"]);
    assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stdout));
    assert!(findings(&out).is_empty());
}

#[test]
fn ac3_cl002_fail() {
    let out = lint(&[], &["cl002-fail.md"]);
    assert_eq!(out.code, 1);
    let f = findings(&out);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].0, "CL002");
    assert_eq!(f[0].1, 1);
    assert!(f[0].3.contains(&"PolyWorks".to_string()), "{:?}", f[0].3);
    assert!(f[0].3.contains(&"Faster".to_string()), "{:?}", f[0].3);
}

#[test]
fn ac4_cl002_pass_with_benchmark_link() {
    let out = lint(&[], &["cl002-pass.md"]);
    assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stdout));
    assert!(findings(&out).is_empty());
}

#[test]
fn ac4_bench_prefix_from_command_line() {
    let without = lint(&[], &["cl002-cli-prefix.md"]);
    assert_eq!(without.code, 1);
    assert_eq!(findings(&without).len(), 1);
    let with = lint(&["--bench-prefix", "https://bench.example.test/run/"], &["cl002-cli-prefix.md"]);
    assert_eq!(with.code, 0);
    assert!(findings(&with).is_empty());
}

#[test]
fn ac5_code_ignored() {
    let out = lint(&[], &["code-ignored.md"]);
    assert_eq!(out.code, 0, "{}", String::from_utf8_lossy(&out.stdout));
    assert!(findings(&out).is_empty());
}

#[test]
fn ac6_html_fail_line_7() {
    let out = lint(&[], &["html-fail.html"]);
    assert_eq!(out.code, 1);
    assert_eq!(findings(&out), vec![("CL001".to_string(), 7, 4, vec!["traceable".to_string()])]);
}

#[test]
fn ac7_broken_rule_file() {
    let args: Vec<String> =
        vec!["claim-lint".into(), "--rules".into(), fix("broken-rules.toml"), "--json".into(), fix("cl001-fail.md")];
    let out = run(&args);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("broken-rules.toml"), "{}", out.stderr);
    let r = report(&out);
    assert!(!r.contains_key("findings"));
    let e = obj(r.get("error").unwrap());
    assert_eq!(st(e, "code"), "rules_parse");
    assert!(st(e, "path").ends_with("broken-rules.toml"));
}

#[test]
fn ac8_suppressed() {
    let out = lint(&[], &["suppressed.md"]);
    assert_eq!(out.code, 0);
    let r = report(&out);
    assert!(arr(&r, "findings").is_empty());
    let s = arr(&r, "suppressions");
    assert_eq!(s.len(), 1);
    let s = obj(&s[0]);
    assert_eq!(st(s, "rule"), "CL001");
    assert_eq!(st(s, "reason"), "quoting a customer requirement");
    assert_eq!(num(s, "suppressed"), 1);
    assert_eq!(num(s, "line"), 2);
}

#[test]
fn marker_without_reason_is_an_input_error() {
    let out = lint(&[], &["bad-marker.md"]);
    assert_eq!(out.code, 2);
    assert_eq!(st(obj(report(&out).get("error").unwrap()), "code"), "bad_marker");
}

#[test]
fn ac10_order_independent_and_byte_identical() {
    let files = ["cl001-fail.md", "cl002-fail.md", "html-fail.html", "suppressed.md", "mixed.md"];
    let a = lint(&[], &files);
    let mut rev = files;
    rev.reverse();
    let b = lint(&[], &rev);
    assert_eq!(a.code, 1);
    assert_eq!(a.stdout, b.stdout);
    let c = lint(&[], &files);
    assert_eq!(a.stdout, c.stdout);
}

#[test]
fn rules_version_in_every_report() {
    let out = lint(&[], &["cl001-pass.md"]);
    assert_eq!(st(&report(&out), "rules_version"), "outr-claim-rules-1");
    let text = run(&["claim-lint".into(), "--rules".into(), rules(), fix("cl001-pass.md")]);
    let s = String::from_utf8(text.stdout).unwrap();
    assert!(s.lines().next().unwrap().contains("outr-claim-rules-1"), "{s}");
}

#[test]
fn front_matter_headings_reference_links_and_sentences() {
    // Hand-labelled: front matter and heading are not part of the paragraph; "Every result is
    // traceable." is its own sentence (col 48) with no designator or link; the third sentence
    // has a designator with edition and a reference-style link.
    let out = lint(&[], &["mixed.md"]);
    assert_eq!(out.code, 1);
    assert_eq!(findings(&out), vec![("CL001".to_string(), 7, 48, vec!["traceable".to_string()])]);
}

#[test]
fn missing_input_and_usage_exit_2() {
    assert_eq!(lint(&[], &["does-not-exist.md"]).code, 2);
    assert_eq!(run(&["claim-lint".into(), fix("cl001-fail.md")]).code, 2);
    assert_eq!(run(&["frobnicate".into()]).code, 2);
    assert_eq!(run(&["--help".into()]).code, 0);
}

#[test]
fn oversized_input_exits_2() {
    let dir = std::env::temp_dir().join(format!("fid-brand-lint-big-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let big = dir.join("big.md");
    std::fs::write(&big, vec![b'a'; 5 * 1024 * 1024 + 1]).unwrap();
    let out = run(&["claim-lint".into(), "--rules".into(), rules(), "--json".into(), big.to_string_lossy().into()]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(out.code, 2);
    assert_eq!(st(obj(report(&out).get("error").unwrap()), "code"), "input_too_large");
}

/// REQ-OUTR.BRAND.LINT-006: every rule has a passing and a failing fixture, and its inline
/// examples behave.
#[test]
fn every_rule_has_pass_and_fail_fixture() {
    let set = fid_vocab::RuleSet::load(std::path::Path::new(&rules())).unwrap();
    assert!(!set.rules.is_empty());
    let problems = fid_vocab::test_rules(&set);
    assert!(problems.is_empty(), "{problems:?}");
    for r in &set.rules {
        let id = r.id.to_lowercase();
        let pass = format!("{id}-pass.md");
        let fail = format!("{id}-fail.md");
        assert!(PathBuf::from(fix(&pass)).is_file(), "rule {} lacks fixture {pass}", r.id);
        assert!(PathBuf::from(fix(&fail)).is_file(), "rule {} lacks fixture {fail}", r.id);
        let p = lint(&[], &[&pass]);
        assert_eq!(p.code, 0, "{pass}");
        let f = lint(&[], &[&fail]);
        assert_eq!(f.code, 1, "{fail}");
        assert!(findings(&f).iter().any(|x| x.0 == r.id), "{fail} has no {} finding", r.id);
    }
}

#[test]
fn extraction_details() {
    // Hand-labelled extraction cases.
    let md = "Intro text. [Second](https://a.example/x) sentence here.\n\n| a | b |\n|---|---|\n| one | two |\n";
    let ex = extract(md, Format::Markdown);
    let texts: Vec<String> = ex.sentences.iter().map(|s| s.text()).collect();
    assert_eq!(texts, vec!["Intro text.", "Second sentence here.", "a", "b", "one", "two"]);
    assert!(ex.sentences[0].links.is_empty());
    assert_eq!(ex.sentences[1].links, vec!["https://a.example/x".to_string()]);
    assert_eq!((ex.sentences[1].line, ex.sentences[1].col), (1, 14));

    let html = "<p>A &amp; B <a href=\"https://b.example/?a=1&amp;b=2\">link</a>.</p><style>p{}</style><p>Next</p>";
    let ex = extract(html, Format::Html);
    let texts: Vec<String> = ex.sentences.iter().map(|s| s.text()).collect();
    assert_eq!(texts, vec!["A & B link.", "Next"]);
    assert_eq!(ex.sentences[0].links, vec!["https://b.example/?a=1&b=2".to_string()]);

    let crlf = "First line\r\ncontinues here.\r\n";
    let ex = extract(crlf, Format::Markdown);
    assert_eq!(ex.sentences.len(), 1);
    assert_eq!(ex.sentences[0].text(), "First line continues here.");
}
