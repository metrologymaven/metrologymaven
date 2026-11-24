//! Acceptance criteria of spec node UX.TOKENS.SPACE (AC-1 .. AC-4) and further error paths.
//! Expected results are in `fixtures/ux/tokens/space/expected.json`, written by
//! `make_fixtures.py`: hand-labelled findings, each confirmed by an independent Python reading
//! of the rules, never by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> PathBuf {
    repo().join("fixtures/ux/tokens/space").join(rel)
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).args(args).output().expect("run fid-design")
}

fn check(root: &Path, json: bool) -> Output {
    let r = root.to_string_lossy().into_owned();
    let mut args = vec!["space-check", "--root", r.as_str()];
    if json {
        args.push("--json");
    }
    run(&args)
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn json_out(o: &Output) -> CValue {
    fid_canon::parse(stdout(o).trim_end().as_bytes()).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(m) => m.get(k).unwrap_or_else(|| panic!("missing key {k}")),
        _ => panic!("not an object at {k}"),
    }
}

fn labels(v: &CValue) -> Vec<String> {
    let CValue::Arr(a) = v else { panic!("findings must be an array") };
    let mut out: Vec<String> = a
        .iter()
        .map(|f| match (get(f, "code"), get(f, "subject")) {
            (CValue::Str(c), CValue::Str(s)) => format!("{c} {s}"),
            _ => panic!("bad finding"),
        })
        .collect();
    out.sort();
    out
}

fn strings(v: &CValue) -> Vec<String> {
    let CValue::Arr(a) = v else { panic!() };
    let mut out: Vec<String> = a
        .iter()
        .map(|x| match x {
            CValue::Str(s) => s.clone(),
            _ => panic!(),
        })
        .collect();
    out.sort();
    out
}

#[test]
fn ac1_shipped_tokens_pass() {
    let o = check(&repo().join("design"), true);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    let v = json_out(&o);
    assert_eq!(get(&v, "ok"), &CValue::Bool(true));
    assert_eq!(get(&v, "format"), &CValue::Str("fid-design-space-check@1".into()));
    let counts = get(&v, "counts");
    for k in ["space_tokens", "motion_tokens", "elevation_levels"] {
        assert!(matches!(get(counts, k), CValue::Num(n) if *n >= 1.0), "{k}");
    }
    let exp = fid_canon::parse(&std::fs::read(fix("expected.json")).unwrap()).unwrap();
    assert_eq!(labels(get(&v, "findings")), strings(get(get(&exp, "shipped"), "findings")));
}

#[test]
fn every_fixture_matches_its_hand_labels() {
    let exp = fid_canon::parse(&std::fs::read(fix("expected.json")).unwrap()).unwrap();
    let CValue::Obj(cases) = get(&exp, "fixtures") else { panic!() };
    assert!(cases.len() >= 20);
    for (name, want) in cases {
        let o = check(&fix(name), true);
        let CValue::Num(code) = get(want, "exit") else { panic!() };
        assert_eq!(o.status.code(), Some(*code as i32), "{name}: {}", stdout(&o));
        assert_eq!(labels(get(&json_out(&o), "findings")), strings(get(want, "findings")), "{name}");
    }
}

#[test]
fn ac2_to_ac4_text_reports_name_the_rule() {
    for (name, text) in [
        ("odd-space", "SPACE-GRID"),
        ("small-target", "SIZE-TARGET floor"),
        ("motion-no-reduced", "MOTION-REDUCED motion.panel.open"),
    ] {
        let o = check(&fix(name), false);
        assert_eq!(o.status.code(), Some(1), "{name}");
        assert!(stdout(&o).contains(text), "{name}: {}", stdout(&o));
    }
}

#[test]
fn output_is_byte_stable() {
    for root in [repo().join("design"), fix("elev-six")] {
        let a = check(&root, true);
        let b = check(&root, true);
        assert_eq!(a.stdout, b.stdout);
        assert!(stdout(&a).ends_with('\n'));
    }
}

#[test]
fn unreadable_root_and_bad_usage_exit_2() {
    let missing = fix("no-such-tree");
    assert_eq!(check(&missing, true).status.code(), Some(2));
    assert_eq!(run(&["space-check"]).status.code(), Some(2));
    let r = repo().join("design").to_string_lossy().into_owned();
    assert_eq!(run(&["space-check", "--root", r.as_str(), "--file", "x"]).status.code(), Some(2));
}
