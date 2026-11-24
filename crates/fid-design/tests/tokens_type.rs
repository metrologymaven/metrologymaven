//! Acceptance criteria of spec node UX.TOKENS.TYPE (AC-1 .. AC-4), shipped typography tokens
//! and `fonts.json`, schema/guard hardening, and exit-2 error paths.
//! Expected results are hand-labelled in `fixtures/ux/tokens/type/expected.json` (written by
//! `make_fixtures.py` and cross-checked there by an independent Python oracle).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_design::fonts::{self, check, REPORT_FORMAT};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FX: &str = "fixtures/ux/tokens/type";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> String {
    repo().join(FX).join(rel).to_string_lossy().into_owned()
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).current_dir(repo()).args(args).output().expect("run fid-design")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn json_out(o: &Output) -> CValue {
    fid_canon::parse(stdout(o).trim_end().as_bytes()).unwrap()
}

fn expected() -> CValue {
    fid_canon::parse(&std::fs::read(fix("expected.json")).unwrap()).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(k).unwrap_or_else(|| panic!("missing key {k}")),
        _ => panic!("not an object at {k}"),
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

fn finding_keys(report: &CValue) -> Vec<String> {
    let CValue::Arr(a) = get(report, "findings") else { panic!("findings not an array") };
    let mut out = Vec::new();
    for f in a {
        let (CValue::Str(c), CValue::Str(s)) = (get(f, "code"), get(f, "subject")) else { panic!("bad finding") };
        let k = format!("{c} {s}");
        if out.last() != Some(&k) {
            out.push(k);
        }
    }
    out
}

fn run_case(key: &str) -> Output {
    let exp = expected();
    let e = get(&exp, key);
    let CValue::Str(root_rel) = get(e, "root") else { panic!("root") };
    let o = run(&["fonts-check", "--root", &fix(root_rel), "--allowlist", &fix("allow.txt"), "--json"]);
    let CValue::Num(code) = get(e, "exit") else { panic!("exit") };
    assert_eq!(f64::from(o.status.code().unwrap()), *code, "{key}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(get(&r, "format"), &CValue::Str(REPORT_FORMAT.into()));
    assert_eq!(get(&r, "ok"), &CValue::Bool(*code == 0.0), "{key}");
    assert_eq!(finding_keys(&r), strs(get(e, "findings")), "{key}");
    o
}

/// AC-1: shipped `design/tokens/type.tokens.json`, `design/tokens/fonts.json` and `allow.txt` exit 0.
#[test]
fn ac1_shipped_tokens_and_fonts_pass() {
    let a = run(&["fonts-check", "--root", "design", "--allowlist", &fix("allow.txt"), "--json"]);
    assert_eq!(a.status.code(), Some(0), "{}{}", stdout(&a), String::from_utf8_lossy(&a.stderr));
    let b = run(&["fonts-check", "--root", "design", "--allowlist", &fix("allow.txt"), "--json"]);
    assert_eq!(a.stdout, b.stdout, "D0 canonical bytes must match across runs");

    let r = json_out(&a);
    assert_eq!(get(&r, "format"), &CValue::Str(REPORT_FORMAT.into()));
    assert_eq!(get(&r, "ok"), &CValue::Bool(true));
    assert_eq!(get(&r, "families"), &CValue::Num(3.0));
    assert_eq!(get(&r, "roles"), &CValue::Num(10.0));
    assert_eq!(get(&r, "allowlist"), &CValue::Num(3.0));

    let t = run(&["fonts-check", "--root", "design", "--allowlist", &fix("allow.txt")]);
    assert_eq!(t.status.code(), Some(0));
    assert!(stdout(&t).contains("ok: no findings"), "{}", stdout(&t));
}

/// AC-2: `proprietary/` exits 1 with `FONT-LICENCE <family>`.
#[test]
fn ac2_proprietary_licence_fails() {
    run_case("proprietary");
    let t = run(&["fonts-check", "--root", &fix("proprietary"), "--allowlist", &fix("allow.txt")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("FONT-LICENCE Synth Sans"), "{}", stdout(&t));
}

/// AC-3: `no-tnum/` and `no-tnum-feature/` exit 1 with `FONT-TNUM type.numeric`.
#[test]
fn ac3_numeric_without_tabular_numerals_fails() {
    run_case("no-tnum");
    let t = run(&["fonts-check", "--root", &fix("no-tnum"), "--allowlist", &fix("allow.txt")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("FONT-TNUM type.numeric"), "{}", stdout(&t));

    run_case("no-tnum-feature");
}

/// AC-4: `small-floor/` exits 1 with `TYPE-SIZE floor type.body`; `small-standard/` exits 1 with `TYPE-SIZE light type.label`.
#[test]
fn ac4_small_floor_and_standard_size_fail() {
    run_case("small-floor");
    let t = run(&["fonts-check", "--root", &fix("small-floor"), "--allowlist", &fix("allow.txt")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("TYPE-SIZE floor type.body"), "{}", stdout(&t));

    run_case("small-standard");
}

/// Additional error paths: unknown font family and missing required role.
#[test]
fn unknown_font_and_missing_role() {
    run_case("unknown-font");
    run_case("missing-role");
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-tokens-type-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn copy_shipped_type(to: &Path) {
    std::fs::create_dir_all(to.join("tokens")).unwrap();
    std::fs::copy(repo().join("design/tokens/fonts.json"), to.join("tokens/fonts.json")).unwrap();
    std::fs::copy(repo().join("design/tokens/type.tokens.json"), to.join("tokens/type.tokens.json")).unwrap();
}

/// Schema and security guard mutations on copies of shipped files.
#[test]
fn schema_and_guard_mutations() {
    let allow = repo().join(FX).join("allow.txt");
    let codes = |d: &Path, al: &Path| -> Vec<&'static str> {
        check(d, al).unwrap().findings.into_iter().map(|f| f.code).collect()
    };

    // Invalid JSON in fonts.json.
    let d = temp("bad-json");
    copy_shipped_type(&d);
    std::fs::write(d.join("tokens/fonts.json"), "{bad").unwrap();
    assert!(codes(&d, &allow).contains(&fonts::SCHEMA));

    // Duplicate JSON key refused.
    let d = temp("dup-key");
    copy_shipped_type(&d);
    std::fs::write(d.join("tokens/fonts.json"), r#"{"format":"font-list@1","format":"font-list@1","families":[]}"#)
        .unwrap();
    assert!(codes(&d, &allow).contains(&fonts::SCHEMA));

    // Bidirectional control char refused.
    let d = temp("bidi");
    copy_shipped_type(&d);
    let t = std::fs::read_to_string(d.join("tokens/fonts.json")).unwrap().replace("Inter", "In\u{202e}ter");
    std::fs::write(d.join("tokens/fonts.json"), t).unwrap();
    assert!(codes(&d, &allow).contains(&fonts::SCHEMA));

    // Non-HTTPS source URL refused.
    let d = temp("http-src");
    copy_shipped_type(&d);
    let t = std::fs::read_to_string(d.join("tokens/fonts.json"))
        .unwrap()
        .replace("https://github.com/rsms/inter", "http://github.com/rsms/inter");
    std::fs::write(d.join("tokens/fonts.json"), t).unwrap();
    assert!(codes(&d, &allow).contains(&fonts::SCHEMA));

    // Bad fontSize unit (pt instead of px) refused.
    let d = temp("pt-size");
    copy_shipped_type(&d);
    let t = std::fs::read_to_string(d.join("tokens/type.tokens.json")).unwrap().replace("\"14px\"", "\"14pt\"");
    std::fs::write(d.join("tokens/type.tokens.json"), t).unwrap();
    assert!(codes(&d, &allow).contains(&fonts::SCHEMA));

    // Empty allowlist fails closed with FONTS-SCHEMA.
    let d = temp("empty-allow");
    copy_shipped_type(&d);
    let empty_allow = d.join("empty.txt");
    std::fs::write(&empty_allow, "# comments only\n").unwrap();
    assert_eq!(codes(&d, &empty_allow), [fonts::SCHEMA]);
}

/// Bad CLI usage and unreadable inputs exit 2.
#[test]
fn bad_usage_and_unreadable_exit_2() {
    let al = fix("allow.txt");
    for args in [
        vec!["fonts-check"],
        vec!["fonts-check", "--root", "design"],
        vec!["fonts-check", "--allowlist", &al],
        vec!["fonts-check", "--root", "design", "--allowlist", &al, "--all"],
        vec!["fonts-check", "--root", "design", "--allowlist", &al, "--from", "x"],
        vec!["pkg-check", "--root", "design", "--allowlist", &al],
    ] {
        assert_eq!(run(&args).status.code(), Some(2), "{args:?}");
    }
    assert_eq!(run(&["fonts-check", "--root", "no/such/dir", "--allowlist", &al]).status.code(), Some(2));
    assert_eq!(run(&["fonts-check", "--root", "design", "--allowlist", "no/such/allow.txt"]).status.code(), Some(2));
    assert_eq!(run(&["fonts-check", "--root", "spec", "--allowlist", &al]).status.code(), Some(2));
}
