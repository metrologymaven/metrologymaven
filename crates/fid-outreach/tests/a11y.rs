//! Acceptance criteria AC-1 .. AC-7 of spec node OUTR.SITE.QA.A11Y, plus error paths.
//! Expected results are hand-labelled in `fixtures/outr/site/qa/a11y/make_fixtures.py`
//! (contrast ratios computed there from the WCAG definition), never produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_outreach::qa::a11y::{run, A11yError, Options, Report};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/site/qa/a11y").join(rel)
}

fn opts(case: &str, config: Option<&str>) -> Options {
    Options {
        root: fix(case),
        tokens: fix(&format!("{case}/tokens.json")),
        config: config.map(|c| fix(&format!("{case}/{c}"))),
    }
}

fn gate(case: &str, config: Option<&str>) -> Report {
    run(&opts(case, config)).unwrap_or_else(|e| panic!("{case}: {e}"))
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}

fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}

fn st(o: &BTreeMap<String, CValue>, k: &str) -> String {
    match o.get(k) {
        Some(CValue::Str(s)) => s.clone(),
        other => panic!("{k}: expected string, got {other:?}"),
    }
}

fn int(o: &BTreeMap<String, CValue>, k: &str) -> u32 {
    match o.get(k) {
        Some(CValue::Num(n)) => *n as u32,
        other => panic!("{k}: expected number, got {other:?}"),
    }
}

fn expected(case: &str) -> CValue {
    parse(&std::fs::read(fix(&format!("expected/{case}.json"))).unwrap()).unwrap()
}

/// Runs the binary; returns exit code, report bytes and stderr.
fn cli(case: &str, config: Option<&str>, tag: &str) -> (i32, Vec<u8>, String) {
    let o = opts(case, config);
    let out = std::env::temp_dir().join(format!("fid-outreach-a11y-{}-{tag}.json", std::process::id()));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-outreach"));
    cmd.args(["qa", "a11y", "--root"]).arg(&o.root).arg("--tokens").arg(&o.tokens).arg("--out").arg(&out);
    if let Some(c) = &o.config {
        cmd.arg("--config").arg(c);
    }
    let res = cmd.output().unwrap();
    let report = std::fs::read(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    (res.status.code().unwrap_or(-1), report, String::from_utf8_lossy(&res.stderr).into_owned())
}

#[test]
fn ac1_clean_site_passes() {
    let r = gate("clean", None);
    assert_eq!(r.pages, 4);
    assert_eq!(r.stylesheets, 1);
    assert_eq!(r.themes, vec!["dark".to_string(), "light".to_string()]);
    assert!(r.violations.is_empty(), "{:#?}", r.violations);
    assert!(r.suppressed.is_empty());
    assert_eq!(r.exit_code(), 0);
    let (code, report, _) = cli("clean", None, "ac1");
    assert_eq!(code, 0);
    let rep = parse(&report).unwrap();
    assert_eq!(st(obj(&rep), "result"), "pass");
    assert!(arr(obj(&rep).get("violations").unwrap()).is_empty());
}

#[test]
fn ac2_one_violation_per_rule() {
    let r = gate("rules", None);
    let exp = expected("rules");
    let mut want: Vec<(String, u32, String)> = arr(obj(&exp).get("violations").unwrap())
        .iter()
        .map(|v| (st(obj(v), "page"), int(obj(v), "line"), st(obj(v), "rule")))
        .collect();
    want.sort();
    let got: Vec<(String, u32, String)> =
        r.violations.iter().map(|v| (v.page.clone(), v.line, v.rule.clone())).collect();
    assert_eq!(got, want, "{:#?}", r.violations);
    assert_eq!(r.violations.len(), 11);
    let rules: Vec<&str> = r.violations.iter().map(|v| v.rule.as_str()).collect();
    for k in 1..=11 {
        assert!(rules.contains(&format!("A{k:02}").as_str()), "A{k:02} missing");
    }
    assert_eq!(r.exit_code(), 1);
    let (code, report, _) = cli("rules", None, "ac2");
    assert_eq!(code, 1);
    let rep = parse(&report).unwrap();
    let first = obj(&arr(obj(&rep).get("violations").unwrap())[0]);
    assert_eq!(st(first, "rule"), "A01");
    assert_eq!(st(first, "sc"), "1.1.1");
    assert!(st(first, "element").starts_with("<img"));
}

#[test]
fn ac3_low_contrast_dark_theme() {
    let r = gate("low-contrast", None);
    let exp = expected("low-contrast");
    let e = obj(&arr(obj(&exp).get("violations").unwrap())[0]).clone();
    assert_eq!(r.violations.len(), 1, "{:#?}", r.violations);
    let v = &r.violations[0];
    assert_eq!(v.page, st(&e, "page"));
    assert_eq!(v.line, int(&e, "line"));
    assert_eq!(v.rule, "A12");
    assert_eq!(v.element, st(&e, "element"));
    assert!(v.detail.contains(&format!("theme {}", st(&e, "theme"))), "{}", v.detail);
    assert!(v.detail.contains(&format!("contrast {}:1", st(&e, "ratio"))), "{}", v.detail);
    // The same pair at 26 px (rule .big) is large text and passes.
    assert!(r.violations.iter().all(|v| v.element != ".big"));
}

#[test]
fn ac4_inline_literal_colour() {
    let r = gate("inline-colour", None);
    let exp = expected("inline-colour");
    let e = obj(&arr(obj(&exp).get("violations").unwrap())[0]).clone();
    assert_eq!(r.violations.len(), 1, "{:#?}", r.violations);
    let v = &r.violations[0];
    assert_eq!((v.page.clone(), v.line, v.rule.clone()), (st(&e, "page"), int(&e, "line"), st(&e, "rule")));
    assert!(v.detail.contains("#777"), "{}", v.detail);
}

#[test]
fn ac5_suppression_used_and_unmatched() {
    let r = gate("suppressed", Some("config.json"));
    let exp = expected("suppressed");
    assert!(r.violations.is_empty(), "{:#?}", r.violations);
    assert_eq!(r.exit_code(), 0);
    let s0 = obj(&arr(obj(&exp).get("suppressed").unwrap())[0]).clone();
    assert_eq!(r.suppressed.len(), 1);
    assert_eq!((r.suppressed[0].page.clone(), r.suppressed[0].line), (st(&s0, "page"), int(&s0, "line")));
    let u0 = obj(&arr(obj(&exp).get("used").unwrap())[0]).clone();
    let n0 = obj(&arr(obj(&exp).get("unmatched").unwrap())[0]).clone();
    assert_eq!(r.used.len(), 1);
    assert_eq!(
        (r.used[0].0.page.clone(), r.used[0].0.rule.clone(), r.used[0].1),
        (st(&u0, "page"), st(&u0, "rule"), 1)
    );
    assert_eq!(r.unmatched.len(), 1);
    assert_eq!((r.unmatched[0].page.clone(), r.unmatched[0].rule.clone()), (st(&n0, "page"), st(&n0, "rule")));
    let (code, report, _) = cli("suppressed", Some("config.json"), "ac5");
    assert_eq!(code, 0);
    let rep = parse(&report).unwrap();
    let sup = obj(obj(&rep).get("suppressions").unwrap());
    assert_eq!(arr(sup.get("used").unwrap()).len(), 1);
    assert_eq!(arr(sup.get("unmatched").unwrap()).len(), 1);
    assert!(!st(obj(&arr(sup.get("used").unwrap())[0]), "reason").is_empty());
    // Without the configuration the A04 violation is reported.
    assert_eq!(gate("suppressed", None).violations.len(), 1);
}

#[test]
fn ac6_truncated_token_file() {
    let err = run(&opts("bad-tokens", None)).unwrap_err();
    let exp = expected("bad-tokens");
    assert_eq!(err.code(), st(obj(&exp), "code"));
    assert!(err.file().ends_with(&st(obj(&exp), "file_suffix")), "{err}");
    assert!(matches!(err, A11yError::Parse { .. }));
    let (code, report, stderr) = cli("bad-tokens", None, "ac6");
    assert_eq!(code, 2);
    assert!(stderr.contains("A11Y-PARSE(") && stderr.contains("tokens.json"), "{stderr}");
    let rep = parse(&report).unwrap();
    assert_eq!(st(obj(&rep), "result"), "error");
}

#[test]
fn ac7_reports_are_byte_identical() {
    let a = gate("rules", None).to_json().unwrap();
    let b = gate("rules", None).to_json().unwrap();
    assert_eq!(a, b);
    let (_, c, _) = cli("rules", None, "ac7a");
    let (_, d, _) = cli("rules", None, "ac7b");
    assert!(!c.is_empty());
    assert_eq!(c, d);
    assert_eq!(a, c);
}

#[test]
fn missing_root_is_io_error_and_bad_usage_exits_2() {
    let o = Options { root: fix("no-such-dir"), tokens: fix("clean/tokens.json"), config: None };
    let err = run(&o).unwrap_err();
    assert_eq!(err.code(), "A11Y-IO");
    let res = Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(["qa", "a11y", "--root"]).output().unwrap();
    assert_eq!(res.status.code(), Some(2));
}

#[test]
fn suppression_without_reason_is_rejected() {
    let bad = fid_outreach::qa::a11y::load_config(br#"{"suppressions":[{"page":"a.html","rule":"A04","reason":" "}]}"#);
    assert!(bad.is_err());
    let unknown =
        fid_outreach::qa::a11y::load_config(br#"{"suppressions":[{"page":"a.html","rule":"A99","reason":"x"}]}"#);
    assert!(unknown.is_err());
}

const LIGHT_TOKENS: &str =
    r##"{"themes":{"light":{"surface.base":"#ffffff","text.body":"#1b2027","accent":"#1b2027"}}}"##;

/// Writes a small case (token file plus root files) to a fresh temporary directory.
fn tmp_case(name: &str, tokens: &str, files: &[(&str, &str)]) -> Options {
    let dir = std::env::temp_dir().join(format!("fid-outreach-a11y-{}-{name}", std::process::id()));
    let root = dir.join("root");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(dir.join("tokens.json"), tokens).unwrap();
    for (rel, text) in files {
        std::fs::write(root.join(rel), text).unwrap();
    }
    Options { root, tokens: dir.join("tokens.json"), config: None }
}

fn rule_ids(r: &Report) -> Vec<(String, String)> {
    r.violations.iter().map(|v| (v.rule.clone(), v.element.clone())).collect()
}

#[test]
fn token_property_must_equal_token_value() {
    let bad = tmp_case("mirror-bad", LIGHT_TOKENS, &[("s.css", ":root{--text-body:#000000}")]);
    let r = run(&bad).unwrap();
    assert_eq!(rule_ids(&r), vec![("A13".to_string(), ":root".to_string())]);
    let good = tmp_case("mirror-good", LIGHT_TOKENS, &[("s.css", ":root{--text-body:#1B2027}")]);
    assert!(run(&good).unwrap().violations.is_empty());
}

#[test]
fn unresolved_var_is_a13() {
    let o = tmp_case("unresolved", LIGHT_TOKENS, &[("s.css", "p{color:var(--nope)}\nq{font-family:var(--sans)}")]);
    let r = run(&o).unwrap();
    assert_eq!(rule_ids(&r), vec![("A13".to_string(), "p".to_string())]);
    assert!(r.violations[0].detail.contains("var(--nope)"));
}

#[test]
fn background_only_rule_uses_inherited_text_colour() {
    let css = "body{background:var(--surface-base);color:var(--text-body)}\n.chip{background:var(--accent)}";
    let r = run(&tmp_case("bg-only", LIGHT_TOKENS, &[("s.css", css)])).unwrap();
    assert_eq!(rule_ids(&r), vec![("A12".to_string(), ".chip".to_string())]);
    assert!(r.violations[0].detail.contains("contrast 1.00:1"), "{}", r.violations[0].detail);
}

#[test]
fn fail_closed_inputs_exit_2() {
    let nested = r##"{"themes":{"light":{"text":{"body":{"$value":"#000000"}}}}}"##;
    let e = run(&tmp_case("nested-tokens", nested, &[])).unwrap_err();
    assert!(e.file().ends_with("tokens.json") && e.code() == "A11Y-PARSE", "{e}");
    let mixed = r##"{"themes":{"light":{"text.body":"#000000","text":{"muted":{"$value":"#555555"}}}}}"##;
    let e = run(&tmp_case("mixed-tokens", mixed, &[])).unwrap_err();
    assert!(e.file().ends_with("tokens.json") && e.code() == "A11Y-PARSE", "{e}");
    let dark = "@media (prefers-color-scheme: dark){p{color:var(--text-body)}}";
    let e = run(&tmp_case("no-dark", LIGHT_TOKENS, &[("s.css", dark)])).unwrap_err();
    assert!(e.to_string().contains("dark"), "{e}");
    let e = run(&tmp_case("css-nesting", LIGHT_TOKENS, &[("s.css", ".a{color:var(--text-body); .b{color:red}}")]))
        .unwrap_err();
    assert_eq!(e.code(), "A11Y-PARSE");
}

fn repo_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

fn tmp_site(name: &str, tokens: PathBuf, files: &[(&str, &str)]) -> Options {
    let dir = std::env::temp_dir().join(format!("fid-outreach-ux-a11y-{}-{name}", std::process::id()));
    let root = dir.join("root");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&root).unwrap();
    for (rel, text) in files {
        std::fs::write(root.join(rel), text).unwrap();
    }
    Options { root, tokens, config: None }
}

const CLEAN_HTML: &str = "<!doctype html><html lang=\"en\"><head><title>UX Tokens</title><link rel=\"stylesheet\" href=\"s.css\"></head><body><a href=\"#main\">Skip</a><main id=\"main\"><h1>UX</h1><p>Body</p><button>Go</button></main></body></html>";

#[test]
fn shipped_design_package_tokens_load_and_check_all_four_themes() {
    let css = "body{background:var(--sem-surface-base);color:var(--sem-text-primary)}\n.muted{color:var(--sem-text-muted)}\nbutton{background:var(--comp-button-bg-primary);color:var(--comp-button-fg-primary)}\n@media (prefers-color-scheme: dark){body{background:var(--sem-surface-base);color:var(--sem-text-primary)}}";
    let r = run(&tmp_site("design-ok", repo_path("design"), &[("index.html", CLEAN_HTML), ("s.css", css)])).unwrap();
    assert_eq!(r.themes, vec!["dark".to_string(), "floor".to_string(), "hc".to_string(), "light".to_string()]);
    assert!(r.violations.is_empty(), "{:#?}", r.violations);
}

#[test]
fn design_package_detects_a12_contrast_and_a13_token_mismatch() {
    let low_contrast = "body{background:var(--sem-surface-base);color:var(--sem-text-primary)}\n.bad{background:var(--sem-surface-base);color:var(--sem-border-subtle)}";
    let r = run(&tmp_site("design-a12", repo_path("design"), &[("index.html", CLEAN_HTML), ("s.css", low_contrast)]))
        .unwrap();
    assert!(r.violations.iter().any(|v| v.rule == "A12" && v.element == ".bad"), "{:#?}", r.violations);

    let wrong_root =
        ":root{--sem-text-primary:#000000}\nbody{background:var(--sem-surface-base);color:var(--sem-text-primary)}";
    let r_bad_root =
        run(&tmp_site("design-root-bad", repo_path("design"), &[("index.html", CLEAN_HTML), ("s.css", wrong_root)]))
            .unwrap();
    assert_eq!(rule_ids(&r_bad_root), vec![("A13".to_string(), ":root".to_string())]);

    let ok_root =
        ":root{--sem-text-primary:#2d2f32}\nbody{background:var(--sem-surface-base);color:var(--sem-text-primary)}";
    let r_ok_root =
        run(&tmp_site("design-root-ok", repo_path("design"), &[("index.html", CLEAN_HTML), ("s.css", ok_root)]))
            .unwrap();
    assert!(r_ok_root.violations.is_empty(), "{:#?}", r_ok_root.violations);

    let prim_ref =
        "body{background:var(--sem-surface-base);color:var(--prim-color-neutral-0)}\np{color:var(--nonexistent)}";
    let r_prim =
        run(&tmp_site("design-prim", repo_path("design"), &[("index.html", CLEAN_HTML), ("s.css", prim_ref)])).unwrap();
    assert!(r_prim.violations.iter().any(|v| v.rule == "A13" && v.detail.contains("--prim-color-neutral-0")));
    assert!(r_prim.violations.iter().any(|v| v.rule == "A13" && v.detail.contains("--nonexistent")));
}

#[test]
fn ux_color_fixtures_fail_closed() {
    let ok_css = "body{background:var(--sem-surface-base);color:var(--sem-text-primary)}";
    let r_ok = run(&tmp_site(
        "ux-ok",
        repo_path("fixtures/ux/tokens/color/ok"),
        &[("index.html", CLEAN_HTML), ("s.css", ok_css)],
    ))
    .unwrap();
    assert!(r_ok.violations.is_empty(), "{:#?}", r_ok.violations);

    for bad_dir in ["missing-dark", "alias-cycle", "unknown-target"] {
        let err = run(&tmp_site(
            &format!("ux-{bad_dir}"),
            repo_path(&format!("fixtures/ux/tokens/color/{bad_dir}")),
            &[("index.html", CLEAN_HTML), ("s.css", ok_css)],
        ))
        .unwrap_err();
        assert_eq!(err.code(), "A11Y-PARSE", "{bad_dir}: {err}");
    }
}

#[test]
fn ux_system_pkg_directory_without_themes_dir() {
    let css = "body{background:#ffffff;color:var(--sem-text-muted)}";
    let r = run(&tmp_site("ux-sys-pkg", repo_path("fixtures/ux/system/pkg/v1.0.0"), &[("s.css", css)])).unwrap();
    assert_eq!(r.themes, vec!["light".to_string()]);
    assert!(r.violations.iter().any(|v| v.rule == "A13" && v.detail.contains("#ffffff")));
    assert!(!r.violations.iter().any(|v| v.detail.contains("--sem-text-muted")));

    let t = fid_outreach::qa::a11y::load_tokens_path(&repo_path("fixtures/ux/system/pkg/v1.0.0")).unwrap();
    assert!(t.props.contains("--sem-focus-ring"));
    assert!(t.props.contains("--sem-status-ok"));
    assert!(t.props.contains("--sem-text-muted"));
    assert!(!t.props.contains("--prim-color-blue-500"));
}

#[test]
fn single_file_dtcg_and_nested_themes_json() {
    let single_dtcg = r##"{
        "surface": {"$type": "color", "base": {"$value": "#ffffff"}},
        "text": {"$type": "color", "primary": {"$value": "#111111"}}
    }"##;
    let r1 = run(&tmp_case(
        "single-dtcg",
        single_dtcg,
        &[("s.css", "body{background:var(--surface-base);color:var(--text-primary)}")],
    ))
    .unwrap();
    assert_eq!(r1.themes, vec!["light".to_string()]);
    assert!(r1.violations.is_empty(), "{:#?}", r1.violations);

    let nested_themes = r##"{
        "themes": {
            "light": {
                "prim": {"$type": "color", "bg": {"$value": "#ffffff"}, "fg": {"$value": "#111111"}},
                "sem": {"$type": "color", "bg": {"$value": "{prim.bg}"}, "fg": {"$value": "{prim.fg}"}}
            },
            "dark": {
                "prim": {"$type": "color", "bg": {"$value": "#111111"}, "fg": {"$value": "#eeeeee"}},
                "sem": {"$type": "color", "bg": {"$value": "{prim.bg}"}, "fg": {"$value": "{prim.fg}"}}
            }
        }
    }"##;
    let r2 = run(&tmp_case(
        "nested-dtcg",
        nested_themes,
        &[("s.css", "body{background:var(--sem-bg);color:var(--sem-fg)}")],
    ))
    .unwrap();
    assert_eq!(r2.themes, vec!["dark".to_string(), "light".to_string()]);
    assert!(r2.violations.is_empty(), "{:#?}", r2.violations);
}

#[test]
fn cli_accepts_design_package_directory() {
    let css = "body{background:var(--sem-surface-base);color:var(--sem-text-primary)}";
    let o = tmp_site("cli-design", repo_path("design"), &[("index.html", CLEAN_HTML), ("s.css", css)]);
    let out = std::env::temp_dir().join(format!("fid-outreach-a11y-cli-design-{}.json", std::process::id()));
    let res = Command::new(env!("CARGO_BIN_EXE_fid-outreach"))
        .args(["qa", "a11y", "--root"])
        .arg(&o.root)
        .arg("--tokens")
        .arg(&o.tokens)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    let report_bytes = std::fs::read(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    assert_eq!(res.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&res.stderr));
    let rep = parse(&report_bytes).unwrap();
    assert_eq!(st(obj(&rep), "result"), "pass");
    let themes: Vec<String> = arr(obj(&rep).get("themes").unwrap())
        .iter()
        .map(|v| match v {
            CValue::Str(s) => s.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(themes, vec!["dark", "floor", "hc", "light"]);
}
