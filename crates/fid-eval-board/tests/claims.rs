#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Acceptance tests of spec node AGENT.BOARD.CLAIMS. Fixtures and the expected findings come
//! from fixtures/agent/board/claims/make_cases.py (hand-labelled codes and claims, thresholds
//! confirmed with Python fractions), never from this crate.

use fid_canon::{parse, CValue};
use fid_eval_board::*;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/agent/board/claims");
const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

type Row = (String, usize, String, Option<String>);

fn case_dir(case: &str) -> PathBuf {
    Path::new(FIX).join(case)
}

fn run_case(case: &str) -> Report {
    let dir = case_dir(case);
    let register = dir.join("claims.json");
    let board = dir.join("board");
    let sources = vec![dir.join("docs")];
    check_claims(&Inputs { register: &register, board: &board, phrases: None, sources: &sources })
}

fn rows(case: &str, r: &Report) -> Vec<Row> {
    let prefix = format!("{}/", display_path(&case_dir(case)));
    let mut out: Vec<Row> = r
        .findings
        .iter()
        .map(|f| {
            let file = f.file.strip_prefix(&prefix).unwrap_or_else(|| panic!("{} outside {prefix}", f.file));
            (file.to_string(), f.line, f.code.as_str().to_string(), f.claim.clone())
        })
        .collect();
    out.sort();
    out
}

fn expected(case: &str) -> Vec<Row> {
    let v = parse(&std::fs::read(Path::new(FIX).join("expected.json")).unwrap()).unwrap();
    let CValue::Obj(cases) = v else { panic!("expected.json is not an object") };
    let Some(CValue::Arr(items)) = cases.get(case) else { panic!("no case {case}") };
    let mut out: Vec<Row> = items
        .iter()
        .map(|it| {
            let CValue::Obj(m) = it else { panic!("bad row") };
            let st = |k: &str| match m.get(k) {
                Some(CValue::Str(s)) => Some(s.clone()),
                Some(CValue::Null) => None,
                other => panic!("bad {k}: {other:?}"),
            };
            let line = match m.get("line") {
                Some(CValue::Num(n)) => *n as usize,
                other => panic!("bad line: {other:?}"),
            };
            (st("file").unwrap(), line, st("code").unwrap(), st("claim"))
        })
        .collect();
    out.sort();
    out
}

fn assert_case(case: &str) -> Report {
    let r = run_case(case);
    assert_eq!(rows(case, &r), expected(case), "case {case}: {:#?}", r.findings);
    r
}

#[test]
fn ac1_ok_passes() {
    let r = assert_case("ok");
    assert!(r.ok());
    assert_eq!((r.claims, r.tags, r.files), (4, 4, 2));
}

#[test]
fn ac2_unknown_tag_names_file_and_line() {
    let r = assert_case("unknown-tag");
    assert!(r.findings[0].file.ends_with("/docs/news.md"));
    assert_eq!((r.findings[0].code, r.findings[0].line), (Code::UnknownClaim, 3));
}

#[test]
fn ac3_threshold_not_met_names_claim() {
    let r = assert_case("threshold");
    let f = &r.findings[0];
    assert_eq!(f.claim.as_deref(), Some("c1"));
    assert!(f.detail.contains("17/20") && f.detail.contains("9/10"), "{}", f.detail);
}

#[test]
fn ac4_untagged_capability_phrase() {
    let r = assert_case("untagged");
    assert!(r.findings.iter().all(|f| f.code == Code::UntaggedClaim));
}

#[test]
fn req004_blocked_and_incomplete_verdicts() {
    for case in ["verdict-block", "verdict-incomplete"] {
        let r = assert_case(case);
        assert!(r.findings.iter().all(|f| f.code == Code::ClaimOnBlockedRelease));
    }
}

#[test]
fn entries_missing_from_board() {
    assert_case("entry-missing");
}

#[test]
fn invalid_inputs_fail_closed() {
    let dir = case_dir("ok");
    let sources = vec![dir.join("docs")];
    let missing = dir.join("no-such-register.json");
    let board = dir.join("board");
    let r = check_claims(&Inputs { register: &missing, board: &board, phrases: None, sources: &sources });
    assert!(r.findings.iter().any(|f| f.code == Code::InvalidRegister));
    // The phrase list defaults to the register's folder; there is none next to this one.
    let register = dir.join("claims.json");
    let no_phrases = dir.join("docs").join("none.txt");
    let r = check_claims(&Inputs { register: &register, board: &board, phrases: Some(no_phrases.as_path()), sources: &sources });
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].code, Code::InvalidPhrases);
    let bad_src = vec![dir.join("no-such-docs")];
    let r = check_claims(&Inputs { register: &register, board: &board, phrases: None, sources: &bad_src });
    assert_eq!(r.findings[0].code, Code::InvalidSource);
}

#[test]
fn repo_register_and_phrase_list_are_valid() {
    let reg = Path::new(REPO).join("agent/claims.json");
    parse_register(&std::fs::read(reg).unwrap(), "agent/claims.json").unwrap();
    let phrases = std::fs::read_to_string(Path::new(REPO).join("agent/claim-phrases.txt")).unwrap();
    let list = parse_phrases(&phrases);
    assert!(!list.is_empty());
    // AC-4's sentence must be caught by the project's own list too.
    let (_, f) = scan_text("our agent programs a part from PMI\n", "x.md", &list);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].code, Code::UntaggedClaim);
}

fn cli(case: &str) -> (i32, String) {
    let dir = case_dir(case);
    let out = Command::new(env!("CARGO_BIN_EXE_fid-eval-board"))
        .args(["board", "check-claims", "--json", "--register"])
        .arg(dir.join("claims.json"))
        .arg("--board")
        .arg(dir.join("board"))
        .arg("--sources")
        .arg(dir.join("docs"))
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

#[test]
fn cli_exit_codes_and_json() {
    let (code, out) = cli("ok");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("\"ok\":true"), "{out}");
    let (code, out) = cli("untagged");
    assert_eq!(code, 1);
    assert!(out.contains("\"code\":\"untagged_claim\"") && out.contains("\"ok\":false"), "{out}");
    let usage = Command::new(env!("CARGO_BIN_EXE_fid-eval-board")).args(["board", "check-claims"]).output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
}

#[test]
fn d0_same_inputs_same_output() {
    assert_eq!(cli("threshold"), cli("threshold"));
}
