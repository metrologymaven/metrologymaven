//! Spec leaf UX.PATTERN.SCHEMA: `fid-design pattern-check` on the fixtures of
//! `fixtures/ux/pattern/schema/` (AC-1 .. AC-4 and one case per further rule; expectations
//! hand-labelled in `make_fixtures.py`, cross-checked there by an independent stdlib reference
//! checker, and written to `expected.json`), the glob table `glob-cases.json` (matches computed in
//! Python with regular expressions), the shipped `design/patterns/` against the shipped package
//! `design/` and the shipped journey maps, plus D0, fail-closed, usage and exit-2 paths.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_design::pattern::{
    self, check, check_text, glob_match, parse_manifest, CommandPattern, Options, CANCEL_MISSING, COMMAND_UNMATCHED,
    COMPONENT_UNKNOWN, KEYBOARD_MISSING, SCHEMA,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FX: &str = "fixtures/ux/pattern/schema";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fx_dir() -> PathBuf {
    root().join(FX)
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).current_dir(dir).args(args).output().expect("run fid-design")
}

fn out(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn json(path: &Path) -> CValue {
    fid_canon::parse(&std::fs::read(path).unwrap()).unwrap()
}

fn s(v: &CValue) -> String {
    match v {
        CValue::Str(s) => s.clone(),
        other => panic!("not a string: {other:?}"),
    }
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(k).unwrap_or_else(|| panic!("no key {k}")),
        other => panic!("not an object: {other:?}"),
    }
}

fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        other => panic!("not an array: {other:?}"),
    }
}

fn opt_path(v: &CValue) -> Option<PathBuf> {
    match v {
        CValue::Null => None,
        other => Some(fx_dir().join(s(other))),
    }
}

fn pairs(v: &CValue) -> BTreeSet<(String, String)> {
    arr(v).iter().map(|p| (s(&arr(p)[0]), s(&arr(p)[1]))).collect()
}

fn got(f: &[fid_design::finding::Finding]) -> BTreeSet<(String, String)> {
    f.iter().map(|x| (x.code.to_string(), x.subject.clone())).collect()
}

/// AC-1: `pattern-check --root ok --package pkg --manifest manifest.json --json`, run in the
/// fixture folder, exits 0 and is byte-identical on a second run (D0). Without `--journeys` the
/// report says so (`unchecked`, `ok: false`); with it, `ok` is true.
#[test]
fn ac1_ok_passes() {
    let args = ["pattern-check", "--root", "ok", "--package", "pkg", "--manifest", "manifest.json", "--json"];
    let o1 = run_in(&fx_dir(), &args);
    let o2 = run_in(&fx_dir(), &args);
    assert_eq!(o1.status.code(), Some(0), "{}", out(&o1));
    assert_eq!(o1.stdout, o2.stdout, "pattern-check --json must be byte-identical (D0)");
    let j = String::from_utf8(o1.stdout).unwrap();
    assert!(j.contains("\"id\":\"pat.command-run\""), "{j}");
    assert!(j.contains("\"findings\":[]"), "{j}");
    assert!(j.contains("\"warnings\":[]"), "{j}");
    assert!(j.contains("\"unchecked\":[\"journeys\"]"), "{j}");
    assert!(j.contains("\"ok\":false"), "nothing unchecked may read as ok: {j}");
    assert!(j.contains("\"format\":\"fid-design-pattern-check@1\""), "{j}");
    assert!(j.contains("\"commands\":6"), "{j}");

    let full = [
        "pattern-check",
        "--root",
        "ok",
        "--package",
        "pkg",
        "--manifest",
        "manifest.json",
        "--journeys",
        "journeys",
        "--json",
    ];
    let o = run_in(&fx_dir(), &full);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    let j = String::from_utf8(o.stdout).unwrap();
    assert!(j.contains("\"ok\":true") && j.contains("\"unchecked\":[]") && j.contains("\"journeys\":2"), "{j}");

    let t = run_in(&fx_dir(), &full[..full.len() - 1]);
    assert_eq!(t.status.code(), Some(0), "{}", out(&t));
    assert!(out(&t).contains("ok: no findings"), "{}", out(&t));
    let t = run_in(&fx_dir(), &["pattern-check", "--root", "ok", "--package", "pkg"]);
    assert_eq!(t.status.code(), Some(0), "{}", out(&t));
    assert!(out(&t).contains("not checked: commands (no --manifest)"), "{}", out(&t));
    assert!(out(&t).contains("not checked: journeys (no --journeys)"), "{}", out(&t));
}

fn exits(file: &str, code: i32, needle: &str) {
    let o = run_in(&fx_dir(), &["pattern-check", "--root", file, "--package", "pkg", "--manifest", "manifest.json"]);
    assert_eq!(o.status.code(), Some(code), "{}", out(&o));
    assert!(out(&o).contains(needle), "{file}: {}", out(&o));
}

/// AC-2: an empty cancel path.
#[test]
fn ac2_no_cancel() {
    exits("no-cancel.md", 1, "PAT-CANCEL-MISSING no-cancel.md");
}

/// AC-3: `comp.nope` is no component spec of the package.
#[test]
fn ac3_bad_comp() {
    exits("bad-comp.md", 1, "PAT-COMPONENT-UNKNOWN comp.nope");
}

/// AC-4: `foo.bar_*@1` matches no command: a warning, exit 0.
#[test]
fn ac4_stale_cmd() {
    exits("stale-cmd.md", 0, "warning: PAT-COMMAND-UNMATCHED foo.bar_*@1");
    let o = run_in(
        &fx_dir(),
        &["pattern-check", "--root", "stale-cmd.md", "--package", "pkg", "--manifest", "manifest.json", "--json"],
    );
    let j = String::from_utf8(o.stdout).unwrap();
    assert!(j.contains("\"code\":\"PAT-COMMAND-UNMATCHED\"") && j.contains("\"findings\":[]"), "{j}");
}

/// Every hand-labelled case of `expected.json`: the (code, subject) sets of errors and
/// warnings and the unchecked list are exact.
#[test]
fn expected_cases() {
    let e = json(&fx_dir().join("expected.json"));
    assert_eq!(s(get(&e, "format")), "pattern-check-cases@1");
    let cases = arr(get(&e, "cases"));
    assert!(cases.len() >= 24, "{}", cases.len());
    for c in cases {
        let rootp = fx_dir().join(s(get(c, "root")));
        let pkg = fx_dir().join(s(get(c, "package")));
        let manifest = opt_path(get(c, "manifest"));
        let journeys = opt_path(get(c, "journeys"));
        let o = Options { root: &rootp, package: &pkg, manifest: manifest.as_deref(), journeys: journeys.as_deref() };
        let r = check(&o).unwrap();
        let label = format!("{} (manifest {:?}, journeys {:?})", s(get(c, "root")), manifest, journeys);
        assert_eq!(got(&r.findings), pairs(get(c, "errors")), "{label}: {:#?}", r.findings);
        assert_eq!(got(&r.warnings), pairs(get(c, "warnings")), "{label}: {:#?}", r.warnings);
        let unchecked: Vec<String> = arr(get(c, "unchecked")).iter().map(s).collect();
        assert_eq!(r.unchecked, unchecked, "{label}");
        let want_exit = u8::from(!arr(get(c, "errors")).is_empty());
        assert_eq!(r.exit_code(), want_exit, "{label}");
        assert_eq!(r.ok(), want_exit == 0 && unchecked.is_empty(), "{label}");
    }
}

/// The glob table: every match computed in Python by regular expression, and every malformed
/// pattern refused.
#[test]
fn glob_table() {
    let g = json(&fx_dir().join("glob-cases.json"));
    assert_eq!(s(get(&g, "format")), "pattern-glob-cases@1");
    let rows = arr(get(&g, "matches"));
    assert!(rows.len() >= 100);
    let mut hits = 0;
    for r in rows {
        let p = CommandPattern::parse(&s(get(r, "pattern"))).unwrap();
        let cmd = s(get(r, "command"));
        let (name, major) = cmd.rsplit_once('@').unwrap();
        let want = matches!(get(r, "match"), CValue::Bool(true));
        assert_eq!(p.matches(name, major.parse().unwrap()), want, "{:?} vs {cmd}", s(get(r, "pattern")));
        // The manifest index gives the same answer as the direct match (for command names a
        // manifest can hold; `view.select_` cannot end a name).
        if fid_types::CommandName::is_valid(name) {
            let m = format!("{{\"commands\":[{{\"id\":\"{name}\",\"major\":{major}}}]}}");
            assert_eq!(parse_manifest(&m).unwrap().any_match(&p), want, "{cmd}");
        }
        hits += usize::from(want);
    }
    assert!(hits >= 15 && hits < rows.len(), "{hits}");
    for b in arr(get(&g, "malformed")) {
        assert!(CommandPattern::parse(&s(b)).is_err(), "{}", s(b));
    }
    assert!(glob_match("a*", "a") && !glob_match("a", ""));
}

/// The manifest is read fail closed: anything malformed cannot be checked against (exit 2).
#[test]
fn manifest_rules() {
    let ok = r#"{"format":1,"commands":[{"id":"view.select_one","major":1}]}"#;
    assert_eq!(parse_manifest(ok).unwrap().len(), 1);
    for bad in [
        r#"{"commands":[{"id":"view.a","major":1},{"id":"view.a","major":1}]}"#,
        r#"{"commands":[{"id":"View.a","major":1}]}"#,
        r#"{"commands":[{"id":"view.a","major":0}]}"#,
        r#"{"commands":[{"id":"view.a","major":1.5}]}"#,
        r#"{"commands":[{"id":"view.a"}]}"#,
        r#"{"commands":[{"id":"view.a","major":1,"major":2}]}"#,
        r#"{"commands":{}}"#,
        r#"{"commandz":[]}"#,
        "[]",
        "not json",
    ] {
        assert!(parse_manifest(bad).is_err(), "{bad}");
    }
}

/// Real inputs: the shipped template passes against the shipped package and the shipped
/// persona journey maps (no manifest is built here, so commands are reported unchecked).
#[test]
fn shipped_patterns_pass() {
    let o = run_in(
        &root(),
        &[
            "pattern-check",
            "--root",
            "design/patterns",
            "--package",
            "design",
            "--journeys",
            "acceptance/journeys/persona",
            "--json",
        ],
    );
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    let j = out(&o);
    assert!(j.contains("\"id\":\"pat.template\"") && j.contains("\"findings\":[]"), "{j}");
    assert!(j.contains("\"comp.template\""), "{j}");
    assert!(j.contains("\"unchecked\":[\"commands\"]"), "{j}");
    // Against a manifest that has the template's one command, nothing is unchecked.
    let dir = std::env::temp_dir().join(format!("fid-design-pattern-man-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let man = dir.join("m.json");
    std::fs::write(&man, r#"{"commands":[{"id":"project.create_project","major":1}]}"#).unwrap();
    let r = check(&Options {
        root: &root().join("design/patterns"),
        package: &root().join("design"),
        manifest: Some(&man),
        journeys: Some(&root().join("acceptance/journeys/persona")),
    })
    .unwrap();
    assert!(r.ok() && r.warnings.is_empty(), "{:#?} {:#?}", r.findings, r.warnings);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Inline variants of the AC-1 spec for rules the fixture files do not isolate.
#[test]
fn inline_rules() {
    let base = std::fs::read_to_string(fx_dir().join("ok/ok.md")).unwrap();
    let codes = |text: &str| -> Vec<(String, String)> {
        let mut f = Vec::new();
        check_text("x.md", text, &mut f);
        let mut v: Vec<(String, String)> = f.into_iter().map(|x| (x.code.to_string(), x.subject)).collect();
        v.sort();
        v.dedup();
        v
    };
    assert!(codes(&base).is_empty());
    let one = |from: &str, to: &str| {
        assert_eq!(base.matches(from).count(), 1, "{from}");
        codes(&base.replacen(from, to, 1))
    };
    let pair = |c: &str, s: &str| (c.to_string(), s.to_string());
    let schema = vec![pair(SCHEMA, "x.md")];
    // Wrong format, bad id, missing trigger, a duplicate TOML key.
    assert_eq!(one("pattern-spec@1", "pattern-spec@2"), schema);
    assert_eq!(one("id = \"pat.command-run\"", "id = \"command-run\""), schema);
    assert_eq!(one("trigger = ", "triggers = "), schema);
    assert_eq!(one("title = \"Run", "title = \"x\"\ntitle = \"Run"), schema);
    // A duplicate state, a duplicate command pattern, a duplicate journey map.
    assert_eq!(one("\"running\", \"done\"]", "\"running\", \"running\"]"), schema);
    assert_eq!(one("\"palette.run_command@1\"]", "\"view.select_*@1\"]"), schema);
    assert_eq!(one("map = \"fx-review\"", "map = \"fx-run\""), schema);
    // A keyboard entry for a step that does not exist.
    assert_eq!(one("step = 1\n", "step = 7\n"), vec![pair(KEYBOARD_MISSING, "x.md"), pair(SCHEMA, "x.md")]);
    // A missing keyboard key, failure key or journeys key.
    let no_kb = base
        .replacen("[[keyboard]]\nstep = 1\nkeys = \"Ctrl+K, then type\"\n\n", "", 1)
        .replacen(
            "[[keyboard]]\nstep = 2\nkeys = \"Up and Down, then Enter\"\nnote = \"Enter on a command that needs parameters opens its form.\"\n\n",
            "",
            1,
        );
    assert_eq!(codes(&no_kb), vec![pair(KEYBOARD_MISSING, "x.md")]);
    assert_eq!(
        one("failure = [\"A refused command shows its error code and one suggested fix inside the palette.\"]\n", ""),
        vec![pair(pattern::FAILURE_MISSING, "x.md")]
    );
    // Cancel given as a string, not an array of lines.
    assert_eq!(
        one(
            "cancel = [\"Escape closes the palette; no command runs and the selection is unchanged.\"]",
            "cancel = \"Escape.\""
        ),
        vec![pair(CANCEL_MISSING, "x.md"), pair(SCHEMA, "x.md")]
    );
    // Steps given as one table, a step without input, an empty journeys step list.
    assert_eq!(one("components = []\n", "components = 3\n"), schema);
    assert_eq!(one("input = \"Text that names the command.\"\n", ""), schema);
    assert_eq!(one("steps = [1]", "steps = []"), schema);
    assert_eq!(one("steps = [1]", "steps = [0]"), schema);
    // Untrusted strings never become subjects: a component value that is not comp-shaped is
    // named by its field (`\\u0007` is a TOML escape, so the value holds BEL).
    assert_eq!(
        one("components = [\"comp.field\"]", "components = [\"comp.f\\u0007\"]"),
        vec![pair(COMPONENT_UNKNOWN, "steps[0].components[0]")]
    );
    // Control characters in free text are refused.
    // (The entry is then unusable, so step 1 has no keyboard entry either.)
    assert_eq!(
        one("keys = \"Ctrl+K, then type\"", "keys = \"Ctrl+K\\u001b[31m\""),
        vec![pair(KEYBOARD_MISSING, "x.md"), pair(SCHEMA, "x.md")]
    );
}

#[test]
fn a_command_warning_needs_the_manifest() {
    // Without a manifest the stale pattern is not judged either way.
    let r = check(&Options {
        root: &fx_dir().join("stale-cmd.md"),
        package: &fx_dir().join("pkg"),
        manifest: None,
        journeys: None,
    })
    .unwrap();
    assert!(r.warnings.iter().all(|w| w.code != COMMAND_UNMATCHED));
    assert!(!r.ok() && r.exit_code() == 0);
}

#[test]
fn empty_root_and_duplicate_ids_are_findings() {
    let dir = std::env::temp_dir().join(format!("fid-design-pattern-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let o = |d: &Path| check(&Options { root: d, package: &fx_dir().join("pkg"), manifest: None, journeys: None });
    let r = o(&dir).unwrap();
    assert_eq!(got(&r.findings), BTreeSet::from([(SCHEMA.to_string(), "--root".to_string())]));
    let ok = std::fs::read(fx_dir().join("ok/ok.md")).unwrap();
    std::fs::write(dir.join("a.md"), &ok).unwrap();
    std::fs::write(dir.join("b.md"), &ok).unwrap();
    let r = o(&dir).unwrap();
    assert_eq!(got(&r.findings), BTreeSet::from([(SCHEMA.to_string(), "b.md".to_string())]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn usage_and_unreadable_exit_2() {
    let fx = fx_dir();
    for args in [
        &["pattern-check", "--root", "ok"][..],
        &["pattern-check", "--package", "pkg"],
        &["pattern-check", "--root", "ok", "--package", "pkg", "--all"],
        &["pattern-check", "--root", "ok", "--package", "pkg", "--ga", "report"],
        &["pattern-check", "--root", "ok", "--package", "pkg", "--bogus"],
        &["pattern-check", "--root", "ok", "--package", "pkg", "--manifest"],
    ] {
        assert_eq!(run_in(&fx, args).status.code(), Some(2), "{args:?}");
    }
    for args in [
        &["pattern-check", "--root", "no-such", "--package", "pkg"][..],
        &["pattern-check", "--root", "ok", "--package", "no-such"],
        &["pattern-check", "--root", "ok", "--package", "pkg", "--manifest", "no-such.json"],
        &["pattern-check", "--root", "ok", "--package", "pkg", "--manifest", "expected.json"],
        &["pattern-check", "--root", "ok", "--package", "pkg", "--journeys", "no-such"],
        &["pattern-check", "--root", "ok", "--package", "pkg", "--journeys", "manifest.json"],
    ] {
        let o = run_in(&fx, args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", out(&o));
    }
}
