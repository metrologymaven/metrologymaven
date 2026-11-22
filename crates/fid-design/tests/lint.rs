//! Acceptance criteria of spec node UX.SYSTEM.LINT (AC-1 .. AC-5), multi-rule / multi-token
//! suggestions, allowlist file, custom `lint-rules@1` schema/regex errors, `design/consumption.md`
//! and exit-2 usage/I/O errors.
//! Expected results are hand-labelled in `fixtures/ux/system/lint/expected.json`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_design::lint::{self, MAX_SOURCE_BYTES, REPORT_FORMAT};
use std::path::PathBuf;
use std::process::{Command, Output};

const FX: &str = "fixtures/ux/system/lint";

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

fn run_case(key: &str) -> Output {
    let exp = expected();
    let e = get(&exp, key);
    let CValue::Str(root_rel) = get(e, "root") else { panic!("root") };
    let CValue::Str(pkg_rel) = get(e, "package") else { panic!("package") };
    let o = run(&["lint-literals", "--root", &fix(root_rel), "--package", &fix(pkg_rel), "--json"]);
    let CValue::Num(code) = get(e, "exit") else { panic!("exit") };
    assert_eq!(f64::from(o.status.code().unwrap()), *code, "{key}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(get(&r, "format"), &CValue::Str(REPORT_FORMAT.into()));
    assert_eq!(get(&r, "ok"), &CValue::Bool(*code == 0.0), "{key}");

    let CValue::Arr(got_f) = get(&r, "findings") else { panic!("got findings") };
    let CValue::Arr(want_f) = get(e, "findings") else { panic!("want findings") };
    assert_eq!(got_f.len(), want_f.len(), "{key}: got {got_f:?}");
    for (g, w) in got_f.iter().zip(want_f.iter()) {
        for field in ["code", "file", "line", "literal", "suggestions"] {
            assert_eq!(get(g, field), get(w, field), "{key}.{field}: got {g:?}, want {w:?}");
        }
    }
    o
}

/// AC-1: `clean/` (Rust & TOML with token names only) and `pkg/v1.0.0/` exits 0 with no findings.
#[test]
fn ac1_clean_fixture_passes() {
    run_case("clean");
    let t = run(&["lint-literals", "--root", &fix("clean"), "--package", &fix("../pkg/v1.0.0")]);
    assert_eq!(t.status.code(), Some(0));
    assert!(stdout(&t).contains("ok: no findings"), "{}", stdout(&t));
}

/// AC-2: `dirty/theme.rs` line 12 `Color32::from_rgb(0x1f, 0x6f, 0xeb)` exits 1 with
/// `LIT-COLOR-RUST dirty/theme.rs:12` and suggests the `#1f6feb` token.
#[test]
fn ac2_dirty_rust_color_and_suggestion() {
    run_case("dirty_ac2");

    // Also run against a directory containing `dirty/theme.rs` to verify exact `dirty/theme.rs:12` text output.
    let tmp = temp("ac2-parent");
    std::fs::create_dir_all(tmp.join("dirty")).unwrap();
    std::fs::copy(repo().join(FX).join("dirty/theme.rs"), tmp.join("dirty/theme.rs")).unwrap();
    let t = run(&["lint-literals", "--root", tmp.to_str().unwrap(), "--package", &fix("pkg-accent")]);
    assert_eq!(t.status.code(), Some(1));
    let out = stdout(&t);
    assert!(out.contains("LIT-COLOR-RUST dirty/theme.rs:12"), "{out}");
    assert!(out.contains("sem.brand.accent"), "{out}");
}

/// AC-3: `allowed/legacy.css` with `/* design-allow: third-party widget */` is not reported.
#[test]
fn ac3_allowed_line_with_reason_passes() {
    run_case("allowed_legacy");
}

/// AC-4: `allowed/empty-reason.css` with `design-allow:` and no reason reports `LIT-ALLOW-NOREASON`.
#[test]
fn ac4_empty_allow_reason_fails() {
    run_case("allowed_empty");
    let t = run(&["lint-literals", "--root", &fix("allowed/empty-reason.css")]);
    assert_eq!(t.status.code(), Some(1));
    assert!(stdout(&t).contains("LIT-ALLOW-NOREASON"), "{}", stdout(&t));
}

/// AC-5: repeated runs on the same inputs produce byte-identical canonical JSON.
#[test]
fn ac5_deterministic_json_output() {
    let a = run_case("dirty_multi");
    let b = run(&["lint-literals", "--root", &fix("dirty-multi"), "--package", &fix("../pkg/v1.0.0"), "--json"]);
    assert_eq!(a.stdout, b.stdout);
}

/// Allowlist file exempts listed paths when a non-empty reason is provided, and fails on empty reason.
#[test]
fn allowlist_file_rules() {
    let o = run(&[
        "lint-literals",
        "--root",
        &fix("dirty"),
        "--package",
        &fix("pkg-accent"),
        "--allowlist",
        &fix("allowlist.txt"),
        "--json",
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));

    // Allowlist entry with no reason fails with LIT-ALLOW-NOREASON.
    let tmp = temp("bad-allowlist");
    let al = tmp.join("allow.txt");
    std::fs::write(&al, "theme.rs:\n").unwrap();
    let o2 = run(&["lint-literals", "--root", &fix("clean"), "--allowlist", al.to_str().unwrap(), "--json"]);
    assert_eq!(o2.status.code(), Some(1), "{}", stdout(&o2));
    assert!(stdout(&o2).contains(lint::ALLOW_NOREASON), "{}", stdout(&o2));
}

/// Custom `--rules` TOML validation: schema errors (`LINT-RULES-SCHEMA`) and bad regex (`LINT-REGEX`).
#[test]
fn custom_rules_schema_and_regex_errors() {
    let tmp = temp("rules");
    let bad_schema = tmp.join("bad-schema.toml");
    std::fs::write(&bad_schema, "format = \"wrong@1\"\n").unwrap();
    let o = run(&["lint-literals", "--root", &fix("clean"), "--rules", bad_schema.to_str().unwrap(), "--json"]);
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert!(stdout(&o).contains(lint::RULES_SCHEMA), "{}", stdout(&o));

    let bad_re = tmp.join("bad-regex.toml");
    std::fs::write(
        &bad_re,
        "format = \"lint-rules@1\"\n\n[[rule]]\nid = \"LIT-COLOR-BAD\"\nkind = \"color\"\nglobs = [\"**/*.rs\"]\nregex = \"([unclosed\"\nvalue_group = 0\n",
    )
    .unwrap();
    let o2 = run(&["lint-literals", "--root", &fix("clean"), "--rules", bad_re.to_str().unwrap(), "--json"]);
    assert_eq!(o2.status.code(), Some(1), "{}", stdout(&o2));
    assert!(stdout(&o2).contains(lint::REGEX_ERR), "{}", stdout(&o2));
}

/// Shipped `design/consumption.md` explains the contract, rules, allow markers and placeholder name.
#[test]
fn shipped_consumption_contract() {
    let doc = std::fs::read_to_string(repo().join("design/consumption.md")).unwrap();
    for needle in [
        "{{FAMILY_NAME}}",
        "lint-literals",
        "lint-rules@1",
        "design-allow:",
        "LIT-ALLOW-NOREASON",
        "fid-design-pkg",
        "--allowlist",
        "--package",
        "Rust",
        "TOML",
        "JSON",
        "CSS",
        "SVG",
    ] {
        assert!(doc.contains(needle), "missing {needle:?} in design/consumption.md");
    }
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-design-lint-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Oversized source (> 4 MiB), unreadable inputs and bad CLI usage exit 2.
#[test]
fn oversized_unreadable_and_bad_usage_exit_2() {
    let d = temp("oversized");
    let big = d.join("huge.css");
    let pad = "a".repeat((MAX_SOURCE_BYTES as usize) + 64);
    std::fs::write(&big, format!("/* {pad} */\n")).unwrap();
    assert_eq!(run(&["lint-literals", "--root", big.to_str().unwrap()]).status.code(), Some(2));

    for args in [
        &["lint-literals"][..],
        &["lint-literals", "--root", &fix("clean"), "--all"],
        &["lint-literals", "--root", &fix("clean"), "--from", "x"],
        &["pkg-check", "--root", &fix("../pkg/v1.0.0"), "--rules", "x"],
        &["lint-literals", "--root", "no/such/dir"],
        &["lint-literals", "--root", &fix("clean"), "--rules", "no/such/rules.toml"],
        &["lint-literals", "--root", &fix("clean"), "--package", "no/such/pkg"],
        &["lint-literals", "--root", &fix("clean"), "--allowlist", "no/such/allow.txt"],
    ] {
        assert_eq!(run(args).status.code(), Some(2), "{args:?}");
    }
}
