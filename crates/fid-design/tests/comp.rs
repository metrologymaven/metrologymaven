//! Spec leaf UX.COMP.SCHEMA: `fid-design comp-check` on the fixtures of
//! `fixtures/ux/comp/schema/` (AC-1 .. AC-5 and one case per further rule, expectations
//! hand-labelled in `make_fixtures.py` and written to `expected.json`), on the shipped
//! `design/components/` against the shipped package `design/`, plus D0, usage and exit-2 paths.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_design::comp::{self, check, parse_pairs, POINTER_ONLY, ROLES, SCHEMA};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FX: &str = "fixtures/ux/comp/schema";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).current_dir(dir).args(args).output().expect("run fid-design")
}

fn run(args: &[&str]) -> Output {
    run_in(&root(), args)
}

fn fx_dir() -> PathBuf {
    root().join(FX)
}

fn out(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// AC-1: `comp-check --root ok --package v1.0.0 --json`, run in the fixture folder, exits 0
/// and is byte-identical on a second run (D0).
#[test]
fn ac1_ok_button_passes() {
    let args = ["comp-check", "--root", "ok", "--package", "v1.0.0", "--json"];
    let o1 = run_in(&fx_dir(), &args);
    let o2 = run_in(&fx_dir(), &args);
    assert_eq!(o1.status.code(), Some(0), "{}", out(&o1));
    assert_eq!(o1.stdout, o2.stdout, "comp-check --json must be byte-identical (D0)");
    let s = String::from_utf8(o1.stdout).unwrap();
    assert!(s.contains("\"ok\":true"), "{s}");
    assert!(s.contains("\"id\":\"comp.button\""), "{s}");
    assert!(s.contains("\"format\":\"fid-design-comp-check@1\""), "{s}");
    let t = run_in(&fx_dir(), &["comp-check", "--root", "ok", "--package", "v1.0.0"]);
    assert_eq!(t.status.code(), Some(0), "{}", out(&t));
    assert!(out(&t).contains("ok: no findings"), "{}", out(&t));
}

fn fails_with(file: &str, needle: &str) {
    let o = run_in(&fx_dir(), &["comp-check", "--root", file, "--package", "v1.0.0"]);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert!(out(&o).contains(needle), "{file}: {}", out(&o));
    let j = run_in(&fx_dir(), &["comp-check", "--root", file, "--package", "v1.0.0", "--json"]);
    assert_eq!(j.status.code(), Some(1));
    assert!(out(&j).contains("\"ok\":false"), "{}", out(&j));
}

/// AC-2: drag to reorder with no keyboard entry.
#[test]
fn ac2_pointer_only() {
    fails_with("pointer-only.md", "COMP-POINTER-ONLY reorder");
}

/// AC-3: a token the package does not define.
#[test]
fn ac3_bad_token() {
    fails_with("bad-token.md", "COMP-TOKEN-UNKNOWN sem.text.nonexistent");
}

/// AC-4: the disabled state applies and no reason text is defined.
#[test]
fn ac4_no_reason() {
    fails_with("no-reason.md", "COMP-DISABLED-REASON");
}

/// AC-5: `role: widgetish`.
#[test]
fn ac5_bad_role() {
    fails_with("bad-role.md", "COMP-ROLE");
    assert!(ROLES.binary_search(&"widgetish").is_err());
}

fn json(path: &Path) -> serde_like::Value {
    serde_like::parse(&std::fs::read(path).unwrap())
}

/// Minimal access to `expected.json` through the workspace's canonical JSON parser.
mod serde_like {
    pub use fid_canon::CValue as Value;
    pub fn parse(b: &[u8]) -> Value {
        fid_canon::parse(b).unwrap()
    }
}

fn s(v: &fid_canon::CValue) -> String {
    match v {
        fid_canon::CValue::Str(s) => s.clone(),
        other => panic!("not a string: {other:?}"),
    }
}

fn get<'a>(v: &'a fid_canon::CValue, k: &str) -> &'a fid_canon::CValue {
    match v {
        fid_canon::CValue::Obj(m) => m.get(k).unwrap_or_else(|| panic!("no key {k}")),
        other => panic!("not an object: {other:?}"),
    }
}

fn arr(v: &fid_canon::CValue) -> &Vec<fid_canon::CValue> {
    match v {
        fid_canon::CValue::Arr(a) => a,
        other => panic!("not an array: {other:?}"),
    }
}

/// Every hand-labelled case of `expected.json`: the set of (code, subject) pairs is exact.
#[test]
fn expected_cases() {
    let e = json(&fx_dir().join("expected.json"));
    assert_eq!(s(get(&e, "format")), "comp-check-cases@1");
    let cases = arr(get(&e, "cases"));
    assert!(cases.len() >= 14, "{}", cases.len());
    for c in cases {
        let rootp = s(get(c, "root"));
        let pkg = s(get(c, "package"));
        let want: BTreeSet<(String, String)> =
            arr(get(c, "findings")).iter().map(|p| (s(&arr(p)[0]), s(&arr(p)[1]))).collect();
        let r = check(&fx_dir().join(&rootp), &fx_dir().join(&pkg)).unwrap();
        let got: BTreeSet<(String, String)> =
            r.findings.iter().map(|f| (f.code.to_string(), f.subject.clone())).collect();
        assert_eq!(got, want, "{rootp} against {pkg}: {:#?}", r.findings);
        assert_eq!(r.exit_code(), u8::from(!want.is_empty()));
        assert_eq!(r.specs.len(), 1);
    }
}

/// A directory root checks every `*.md` directly under it; the findings are the union of the
/// single-file runs, and each finding names its file.
#[test]
fn directory_root_collects_all_files() {
    let r = check(&fx_dir().join("extra"), &fx_dir().join("v1.0.0")).unwrap();
    assert_eq!(r.specs.len(), 8, "{:?}", r.specs);
    let files: Vec<&str> = r.specs.iter().map(|s| s.file.as_str()).collect();
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(files, sorted, "sorted walk");
    for code in [comp::STATE_MISSING, comp::FOCUS, comp::DENSITY, comp::DISABLED_REASON, comp::SCHEMA] {
        assert!(r.findings.iter().any(|f| f.code == code), "{code}: {:#?}", r.findings);
    }
    // The schema-level root holds AC-2 .. AC-5 (subdirectories are not walked).
    let r = check(&fx_dir(), &fx_dir().join("v1.0.0")).unwrap();
    assert_eq!(r.specs.len(), 4, "{:?}", r.specs);
    assert!(r.findings.iter().any(|f| f.code == POINTER_ONLY && f.detail.starts_with("pointer-only.md:")));
}

/// Real inputs: the shipped template passes against the shipped package, and every spec the
/// sibling UX.COMP leaves add to `design/components/` is held to the same check.
#[test]
fn shipped_components_pass() {
    let o = run(&["comp-check", "--root", "design/components", "--package", "design", "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert!(out(&o).contains("\"id\":\"comp.template\""), "{}", out(&o));
    let r = check(&root().join("design/components"), &root().join("design")).unwrap();
    assert!(r.findings.is_empty(), "{:#?}", r.findings);
    // The template's focus pairs are declared non-text in the shipped pairs file.
    let pairs = comp::load_pairs(&root().join("design")).unwrap().unwrap();
    for bg in ["sem.surface.base", "sem.surface.raised"] {
        assert_eq!(pairs.get(&("sem.focus.ring".to_string(), bg.to_string())).map(String::as_str), Some("non-text"));
    }
}

/// Inline variants of the AC-1 spec for rules the fixture files do not isolate.
#[test]
fn inline_rules() {
    let base = std::fs::read_to_string(fx_dir().join("ok/button.md")).unwrap();
    let pkg = fid_design_pkg::Package::load(&fx_dir().join("v1.0.0")).unwrap();
    let pairs = comp::load_pairs(&fx_dir().join("v1.0.0")).unwrap();
    let codes = |text: &str| -> Vec<(String, String)> {
        let mut f = Vec::new();
        comp::check_text("x.md", text, &pkg, &pairs, &mut f);
        f.sort();
        f.into_iter().map(|x| (x.code.to_string(), x.subject)).collect()
    };
    assert!(codes(&base).is_empty());
    let one = |from: &str, to: &str| {
        assert_eq!(base.matches(from).count(), 1, "{from}");
        codes(&base.replacen(from, to, 1))
    };
    let pair = |c: &str, s: &str| (c.to_string(), s.to_string());
    // Unknown front-block key, wrong format, bad id.
    assert_eq!(one("title = \"Button\"", "title = \"Button\"\ncolour = \"red\""), vec![pair(SCHEMA, "x.md")]);
    assert_eq!(one("component-spec@1", "component-spec@2"), vec![pair(SCHEMA, "x.md")]);
    assert_eq!(one("id = \"comp.button\"", "id = \"button\""), vec![pair(SCHEMA, "x.md")]);
    // Invalid TOML (a duplicate key).
    assert_eq!(one("title = \"Button\"", "title = \"Button\"\ntitle = \"Again\""), vec![pair(SCHEMA, "x.md")]);
    // A state that does not apply without a reason.
    assert_eq!(one("loading = { applies = true }", "loading = { applies = false }"), {
        // Tokens are not given for loading, so only the missing reason is reported; a11y still
        // lists busy, which is allowed.
        vec![pair(SCHEMA, "x.md")]
    });
    // An interactive spec whose focus state does not apply.
    assert_eq!(
        one("focus = { applies = true }", "focus = { applies = false, reason = \"No.\" }"),
        vec![pair(comp::FOCUS, "x.md"), pair(SCHEMA, "x.md")]
    );
    // Pointer action on a part that is not actionable.
    assert_eq!(
        one("gesture = \"click\"\npart = \"container\"", "gesture = \"click\"\npart = \"label\""),
        vec![pair(SCHEMA, "x.md")]
    );
    // A disabled state that a11y does not expose.
    assert_eq!(one("states = [\"busy\", \"disabled\"]", "states = [\"busy\"]"), vec![pair(SCHEMA, "x.md")]);
    // An unknown ARIA state name.
    assert_eq!(
        one("states = [\"busy\", \"disabled\"]", "states = [\"busy\", \"disabled\", \"shiny\"]"),
        vec![pair(SCHEMA, "x.md")]
    );
    // Density with different properties in the two modes.
    assert_eq!(one("padding = \"space.2\"", "margin = \"space.2\""), vec![pair(comp::DENSITY, "x.md")]);
    // A density token that does not resolve.
    assert_eq!(one("padding = \"space.2\"", "padding = \"space.9\""), vec![pair(comp::TOKEN_UNKNOWN, "space.9")]);
    // A focus width given as a float.
    assert_eq!(one("width_px = 2", "width_px = 2.0"), vec![pair(comp::FOCUS, "x.md")]);
    // A part without default tokens.
    assert_eq!(one("[tokens.reason.default]\ntext = \"sem.text.muted\"\n", ""), vec![pair(SCHEMA, "x.md")]);
    // Tokens for a state that does not apply.
    assert_eq!(
        one("[tokens.label.default]", "[tokens.label.error]\ntext = \"sem.text.muted\"\n\n[tokens.label.default]"),
        vec![pair(SCHEMA, "x.md")]
    );
    // An interactive spec with no keyboard entries: the pointer action is then pointer-only.
    let no_keys = base
        .replacen("[[keyboard]]\nkey = \"Enter\"\ncontext = \"The button has focus.\"\naction = \"activate\"\n", "", 1)
        .replacen("[[keyboard]]\nkey = \"Space\"\ncontext = \"The button has focus.\"\naction = \"activate\"\n", "", 1)
        .replacen("title = \"Button\"\n", "title = \"Button\"\nkeyboard = []\n", 1);
    assert_eq!(codes(&no_keys), vec![pair(POINTER_ONLY, "activate"), pair(SCHEMA, "x.md")]);
    // A focus-reachable reason is accepted; a reason on an undeclared part is not.
    assert!(one("shown_as = \"visible\"", "shown_as = \"focus-reachable\"").is_empty());
    assert_eq!(one("part = \"reason\"\n", "part = \"tooltip\"\n"), vec![pair(comp::DISABLED_REASON, "x.md")]);
}

#[test]
fn pairs_file_rules() {
    let ok = r#"{"format":"fid-design-pairs@1","pairs":[{"fg":"a.b","bg":"c.d","tag":"non-text"}]}"#;
    assert_eq!(parse_pairs(ok).unwrap().len(), 1);
    for bad in [
        r#"{"format":"fid-design-pairs@1","pairs":[{"fg":"a.b","bg":"c.d","tag":"non-text"},{"fg":"a.b","bg":"c.d","tag":"text"}]}"#,
        r#"{"format":"fid-design-pairs@1","format":"fid-design-pairs@1","pairs":[]}"#,
        r#"{"format":"fid-design-pairs@2","pairs":[]}"#,
        r#"{"format":"fid-design-pairs@1","pairs":[{"fg":"a.b","bg":"c.d","tag":"bold"}]}"#,
        r#"{"format":"fid-design-pairs@1","pairs":[{"fg":"a.b","bg":"c.d"}]}"#,
        "[]",
        "not json",
    ] {
        assert!(parse_pairs(bad).is_err(), "{bad}");
    }
}

#[test]
fn empty_root_is_a_finding() {
    let dir = std::env::temp_dir().join(format!("fid-design-comp-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let r = check(&dir, &fx_dir().join("v1.0.0")).unwrap();
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].code, SCHEMA);
    assert_eq!(r.exit_code(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn usage_and_unreadable_exit_2() {
    let fx = fx_dir();
    for args in [
        &["comp-check", "--root", "ok"][..],
        &["comp-check", "--package", "v1.0.0"],
        &["comp-check", "--root", "ok", "--package", "v1.0.0", "--all"],
        &["comp-check", "--root", "ok", "--package", "v1.0.0", "--rules", "x"],
        &["comp-check", "--root", "ok", "--package", "v1.0.0", "--bogus"],
    ] {
        assert_eq!(run_in(&fx, args).status.code(), Some(2), "{args:?}");
    }
    assert_eq!(run_in(&fx, &["comp-check", "--root", "no-such", "--package", "v1.0.0"]).status.code(), Some(2));
    assert_eq!(run_in(&fx, &["comp-check", "--root", "ok", "--package", "no-such"]).status.code(), Some(2));
}
