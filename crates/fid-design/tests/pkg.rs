//! Acceptance criteria of spec node UX.SYSTEM.PKG (AC-1 .. AC-6) and further error paths.
//! Expected results are hand-labelled in `fixtures/ux/system/pkg/expected.json`
//! (written by `make_fixtures.py` from the leaf's rules), not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_design_pkg::{Code, Package, Warning};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ux/system/pkg").join(rel)
}

fn p(rel: &str) -> String {
    fix(rel).to_string_lossy().into_owned()
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).args(args).output().expect("run fid-design")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn expected() -> CValue {
    fid_canon::parse(&std::fs::read(fix("expected.json")).unwrap()).unwrap()
}

fn get<'a>(v: &'a CValue, path: &[&str]) -> &'a CValue {
    let mut cur = v;
    for k in path {
        match cur {
            CValue::Obj(m) => cur = m.get(*k).unwrap_or_else(|| panic!("missing key {k}")),
            _ => panic!("not an object at {k}"),
        }
    }
    cur
}

fn num(v: &CValue) -> i32 {
    match v {
        CValue::Num(n) => *n as i32,
        other => panic!("not a number: {other:?}"),
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

/// `CODE subject` of each finding in a JSON report.
fn finding_keys(report: &CValue) -> Vec<String> {
    match get(report, &["findings"]) {
        CValue::Arr(a) => a
            .iter()
            .map(|f| match (get(f, &["code"]), get(f, &["subject"])) {
                (CValue::Str(c), CValue::Str(s)) => format!("{c} {s}"),
                _ => panic!("bad finding"),
            })
            .collect(),
        _ => panic!("findings not an array"),
    }
}

fn json_out(o: &Output) -> CValue {
    fid_canon::parse(stdout(o).trim_end().as_bytes()).unwrap()
}

fn check_case(name: &str) {
    let exp = expected();
    let e = get(&exp, &["check", name]);
    let o = run(&["pkg-check", "--root", &p(name), "--json"]);
    assert_eq!(o.status.code(), Some(num(get(e, &["exit"]))), "{name}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(finding_keys(&r), strs(get(e, &["findings"])), "{name}");
}

fn diff_case(key: &str) {
    let exp = expected();
    let e = get(&exp, &["diff", key]);
    let (from, to) = key.split_once("..").unwrap();
    let o = run(&["pkg-diff", "--from", &p(from), "--to", &p(to), "--json"]);
    assert_eq!(o.status.code(), Some(num(get(e, &["exit"]))), "{key}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(finding_keys(&r), strs(get(e, &["findings"])), "{key}");
    // Every name not listed as changed must be unchanged.
    let CValue::Obj(want) = get(e, &["changed"]) else { panic!() };
    let CValue::Obj(names) = get(&r, &["names"]) else { panic!() };
    for (n, c) in names {
        let got = get(c, &["change"]);
        match want.get(n) {
            Some(w) => assert_eq!(got, w, "{key}: {n}"),
            None => assert_eq!(got, &CValue::Str("unchanged".into()), "{key}: {n}"),
        }
    }
    for n in want.keys() {
        assert!(names.contains_key(n), "{key}: {n} not classified");
    }
}

#[test]
fn ac1_check_v1_reports_counts_per_kind() {
    check_case("v1.0.0");
    let o = run(&["pkg-check", "--root", &p("v1.0.0"), "--json"]);
    let r = json_out(&o);
    assert_eq!(get(&r, &["counts"]), get(&expected(), &["check", "v1.0.0", "counts"]));
    assert_eq!(get(&r, &["version"]), &CValue::Str("1.0.0".into()));
    // Text mode lists the counts too.
    let t = stdout(&run(&["pkg-check", "--root", &p("v1.0.0")]));
    assert!(t.contains("token 8"), "{t}");
}

#[test]
fn ac1_other_valid_packages_pass() {
    check_case("v1.1.0");
    check_case("v2.0.0");
}

#[test]
fn ac2_minor_release_with_addition_and_deprecation_passes() {
    diff_case("v1.0.0..v1.1.0");
}

#[test]
fn ac3_removal_in_minor_release_fails() {
    diff_case("v1.0.0..v1.1.0-removed");
    let o = run(&["pkg-diff", "--from", &p("v1.0.0"), "--to", &p("v1.1.0-removed")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("PKG-BREAKING-MINOR space.2"), "{}", stdout(&o));
}

#[test]
fn ac4_value_change_without_changelog_entry_fails() {
    diff_case("v1.1.0..v1.2.0-nolog");
    let o = run(&["pkg-diff", "--from", &p("v1.1.0"), "--to", &p("v1.2.0-nolog")]);
    assert!(stdout(&o).contains("PKG-CHANGELOG-MISSING motion.fast"), "{}", stdout(&o));
}

#[test]
fn ac5_reader_resolves_deprecated_token_to_replacement() {
    let pkg = Package::load(&fix("v1.1.0")).unwrap();
    assert!(pkg.findings.is_empty(), "{:?}", pkg.findings);
    let exp = expected();
    let CValue::Obj(cases) = get(&exp, &["resolve", "v1.1.0"]) else { panic!() };
    for (name, e) in cases {
        let r = pkg.resolve(name).unwrap();
        assert_eq!(&r.value, get(e, &["value"]), "{name}");
        let want: Vec<Vec<String>> = match get(e, &["warnings"]) {
            CValue::Arr(a) => a.iter().map(strs).collect(),
            _ => panic!(),
        };
        let got: Vec<Vec<String>> = r
            .warnings
            .iter()
            .map(|w| match w {
                Warning::DeprecatedName { name, replacement } => {
                    vec![w.code().to_string(), name.clone(), replacement.clone()]
                }
            })
            .collect();
        assert_eq!(got, want, "{name}");
    }
}

#[test]
fn ac6_duplicate_name_across_kinds_fails() {
    check_case("dup-name");
    let o = run(&["pkg-check", "--root", &p("dup-name")]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("PKG-NAME-DUPLICATE status.ok"), "{}", stdout(&o));
}

#[test]
fn backdated_deprecation_in_new_package_does_not_justify_removal() {
    check_case("v2.0.0-backdated");
    diff_case("v1.0.0..v2.0.0-backdated");
}

#[test]
fn breaking_changelog_entry_in_minor_release_fails() {
    diff_case("v1.0.0..v1.1.0-breaking-log");
}

#[test]
fn type_change_is_breaking_only() {
    diff_case("v1.0.0..v1.1.0-type");
}

#[test]
fn kind_change_is_breaking_only() {
    diff_case("v1.0.0..v1.1.0-kind");
}

#[test]
fn reports_are_byte_identical_across_runs() {
    let a = run(&["pkg-diff", "--from", &p("v1.0.0"), "--to", &p("v1.1.0-removed"), "--json"]);
    let b = run(&["pkg-diff", "--from", &p("v1.0.0"), "--to", &p("v1.1.0-removed"), "--json"]);
    assert_eq!(a.stdout, b.stdout);
}

#[test]
fn unreadable_root_and_bad_usage_exit_2() {
    assert_eq!(run(&["pkg-check", "--root", &p("does-not-exist")]).status.code(), Some(2));
    assert_eq!(run(&["pkg-check"]).status.code(), Some(2));
    assert_eq!(run(&["pkg-diff", "--from", &p("v1.0.0")]).status.code(), Some(2));
    assert_eq!(run(&["nope"]).status.code(), Some(2));
}

// ------------------------------------------------------------------ mutated copies

/// A scratch copy of a fixture package that removes itself when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn copy_of(fixture: &str, tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("fid-design-pkg-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fix(fixture), &dir);
        Scratch(dir)
    }
    fn write(&self, rel: &str, text: &str) {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn edit(&self, rel: &str, from: &str, to: &str) {
        let text = std::fs::read_to_string(self.0.join(rel)).unwrap().replace("\r\n", "\n");
        assert!(text.contains(from), "{rel} lacks {from:?}");
        self.write(rel, &text.replacen(from, to, 1));
    }
    fn codes(&self) -> Vec<(Code, String)> {
        Package::load(&self.0).unwrap().findings.into_iter().map(|f| (f.code, f.subject)).collect()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

fn has(codes: &[(Code, String)], code: Code, subject: &str) -> bool {
    codes.iter().any(|(c, s)| *c == code && s == subject)
}

#[test]
fn bad_name_syntax_is_reported() {
    let s = Scratch::copy_of("v1.0.0", "syntax");
    s.edit("names.json", "\"comp.button\"", "\"Comp.button\"");
    assert!(has(&s.codes(), Code::NameSyntax, "Comp.button"), "{:?}", s.codes());
}

#[test]
fn alias_cycle_and_missing_target_are_unresolved() {
    let s = Scratch::copy_of("v1.0.0", "alias");
    s.edit("tokens/primitive/color.tokens.json", "\"#4a4f55\"", "\"{sem.text.muted}\"");
    s.edit("tokens/semantic/color.tokens.json", "{prim.color.green.500}", "{prim.color.green.600}");
    let c = s.codes();
    assert!(has(&c, Code::AliasUnresolved, "sem.text.muted"), "{c:?}");
    assert!(has(&c, Code::AliasUnresolved, "prim.color.grey.700"), "{c:?}");
    assert!(has(&c, Code::AliasUnresolved, "sem.status.ok"), "{c:?}");
}

#[test]
fn deprecation_without_replacement_is_reported() {
    let s = Scratch::copy_of("v1.1.0", "norepl");
    s.edit("deprecations.json", "\"replacement\": \"sem.focus.outline\"", "\"replacement\": null");
    let c = s.codes();
    assert!(has(&c, Code::DeprecationNorepl, "sem.focus.ring"), "{c:?}");
    let pkg = Package::load(&s.0).unwrap();
    assert!(pkg.resolve("sem.focus.ring").is_err());
}

#[test]
fn changelog_class_contradiction_is_reported() {
    let s = Scratch::copy_of("v1.1.0", "class");
    s.edit("CHANGELOG.md", "- additive: sem.focus.outline", "- fix: sem.focus.outline");
    let r = fid_design::pkg::diff(&fix("v1.0.0"), &s.0).unwrap();
    let keys: Vec<(Code, &str)> = r.findings.iter().map(|f| (f.code, f.subject.as_str())).collect();
    assert_eq!(keys, vec![(Code::ChangelogClass, "sem.focus.outline")]);
}

#[test]
fn removal_in_the_release_after_deprecation_line_is_early() {
    // Deprecated in 1.1.0, removed in 1.1.1 (same minor line) with a major-less bump.
    let s = Scratch::copy_of("v2.0.0", "early");
    s.edit("package.toml", "version = \"2.0.0\"", "version = \"1.1.1\"");
    s.edit("CHANGELOG.md", "## 2.0.0", "## 1.1.1");
    let r = fid_design::pkg::diff(&fix("v1.1.0"), &s.0).unwrap();
    let keys: Vec<(Code, &str)> = r.findings.iter().map(|f| (f.code, f.subject.as_str())).collect();
    assert!(keys.contains(&(Code::BreakingMinor, "sem.focus.ring")), "{keys:?}");
    assert!(keys.contains(&(Code::DeprecationEarly, "sem.focus.ring")), "{keys:?}");
}

#[test]
fn removal_without_released_deprecation_is_early() {
    // 1.0.0 -> 2.0.0 skips the release that listed the deprecation.
    let r = fid_design::pkg::diff(&fix("v1.0.0"), &fix("v2.0.0")).unwrap();
    let keys: Vec<(Code, &str)> = r.findings.iter().map(|f| (f.code, f.subject.as_str())).collect();
    assert!(keys.contains(&(Code::DeprecationEarly, "sem.focus.ring")), "{keys:?}");
}

#[test]
fn foreign_files_and_bad_manifest_are_schema_findings() {
    let s = Scratch::copy_of("v1.0.0", "schema");
    s.write("notes.txt", "x");
    s.edit("package.toml", "{{FAMILY_NAME}}", "Acme");
    let c = s.codes();
    assert!(has(&c, Code::Schema, "notes.txt"), "{c:?}");
    assert!(has(&c, Code::Schema, "package.toml"), "{c:?}");
}

#[test]
fn stale_index_is_reported() {
    let s = Scratch::copy_of("v1.0.0", "stale");
    s.edit("tokens/space.tokens.json", "\"2\"", "\"3\"");
    let c = s.codes();
    assert!(has(&c, Code::Schema, "space.2"), "{c:?}");
    assert!(has(&c, Code::Schema, "space.3"), "{c:?}");
}

#[test]
fn counts_cover_every_kind() {
    let pkg = Package::load(&fix("dup-name")).unwrap();
    let c: BTreeMap<&str, usize> = pkg.counts().into_iter().map(|(k, n)| (k.as_str(), n)).collect();
    assert_eq!(c.len(), 7);
    assert_eq!(c["icon"], 1);
    assert_eq!(c["token"], 1);
}
