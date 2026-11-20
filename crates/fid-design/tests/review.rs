//! Acceptance criteria of spec node UX.SYSTEM.REVIEW (AC-1 .. AC-6) and further error paths.
//! Expected results are hand-labelled in `fixtures/ux/system/review/expected.json` (written by
//! `make_fixtures.py` from the leaf's rules and cross-checked there by an independent Python
//! reading), not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_design::review::{self, CHECKLIST_ITEMS};
use std::path::PathBuf;
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> String {
    root().join("fixtures/ux/system/review").join(rel).to_string_lossy().into_owned()
}

fn shipped(rel: &str) -> String {
    root().join("design").join(rel).to_string_lossy().into_owned()
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).current_dir(root()).args(args).output().expect("run fid-design")
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

/// `CODE subject` of every finding, in report order, repeated keys merged.
fn finding_keys(report: &CValue) -> Vec<String> {
    let CValue::Arr(a) = get(report, "findings") else { panic!("findings not an array") };
    let mut out: Vec<String> = Vec::new();
    for f in a {
        let (CValue::Str(c), CValue::Str(s)) = (get(f, "code"), get(f, "subject")) else { panic!("bad finding") };
        let k = format!("{c} {s}");
        if out.last() != Some(&k) {
            out.push(k);
        }
    }
    out
}

fn args_for(e: &CValue) -> Vec<String> {
    let CValue::Str(root) = get(e, "root") else { panic!("root") };
    let mut a = vec!["review-check".to_string(), "--root".to_string(), fix(root)];
    if let CValue::Str(c) = get(e, "changed") {
        a.push("--changed".to_string());
        a.push(fix(c));
    }
    if let CValue::Str(b) = get(e, "base") {
        a.push("--base".to_string());
        a.push(fix(b));
    }
    if let CValue::Num(n) = get(e, "pr") {
        a.push("--pr".to_string());
        a.push(format!("{n}"));
    }
    if get(e, "all") == &CValue::Bool(true) {
        a.push("--all".to_string());
    }
    if let CValue::Str(d) = get(e, "deny") {
        a.push("--deny-names".to_string());
        a.push(fix(d));
    }
    a
}

/// Runs one labelled case and checks exit code, findings, paths needing review and records.
fn case(key: &str) -> Output {
    let exp = expected();
    let e = get(&exp, key);
    let mut args = args_for(e);
    args.push("--json".to_string());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let o = run(&refs);
    let CValue::Num(code) = get(e, "exit") else { panic!("exit") };
    assert_eq!(f64::from(o.status.code().unwrap()), *code, "{key}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(finding_keys(&r), strs(get(e, "findings")), "{key}");
    assert_eq!(strs(get(&r, "needs_review")), strs(get(e, "needs_review")), "{key}");
    assert_eq!(strs(get(&r, "records")), strs(get(e, "records")), "{key}");
    assert_eq!(get(&r, "format"), &CValue::Str("fid-design-review-check@1".into()));
    assert_eq!(get(&r, "ok"), &CValue::Bool(*code == 0.0), "{key}");
    o
}

#[test]
fn ac1_ok_fixture_passes() {
    let o = case("ok");
    assert_eq!(o.status.code(), Some(0));
    case("ok+deny");
    // Text mode.
    let o = run(&["review-check", "--changed", &fix("ok/changed.txt"), "--root", &fix("ok/design")]);
    assert_eq!(o.status.code(), Some(0));
    assert!(stdout(&o).contains("ok: no findings"), "{}", stdout(&o));
}

#[test]
fn ac2_missing_record() {
    let o = case("missing-record");
    assert!(stdout(&o).contains("\"REVIEW-RECORD-MISSING\""));
    let t = stdout(&run(&[
        "review-check",
        "--changed",
        &fix("missing-record/changed.txt"),
        "--root",
        &fix("missing-record/design"),
    ]));
    assert!(t.contains("REVIEW-RECORD-MISSING tokens/color.tokens.json"), "{t}");
}

#[test]
fn ac3_agent_approver() {
    case("agent-approver");
    case("bot-approver");
}

#[test]
fn ac4_dress_gap_names_the_rule() {
    case("dress-gap");
}

#[test]
fn ac5_approved_icon_without_declaration() {
    case("no-decl");
}

#[test]
fn ac6_name_leak_names_the_file_and_withholds_the_name() {
    let o = case("name-leak");
    let out = stdout(&o).to_lowercase();
    assert!(out.contains("components/button.md") && out.contains("copy/labels.txt"));
    assert!(!out.contains("examplar") && !out.contains("quarzolith"), "the deny list leaked into the report");
    // Without --deny-names the same package passes.
    case("name-leak-nodeny");
}

#[test]
fn every_labelled_case() {
    let CValue::Obj(m) = expected() else { panic!("expected.json") };
    assert!(m.len() >= 38);
    for key in m.keys() {
        case(key);
    }
}

#[test]
fn untrusted_text_fails_closed_and_empty_inputs_never_pass() {
    for key in [
        "bidi-record",
        "nfd-record",
        "obfuscated-changed",
        "empty-changed",
        "empty-deny",
        "deny-bidi",
        "empty-dress",
        "dup-key-record",
        "bad-map",
        "map-weakened",
        "stale-record",
        "result-fail",
        "checklist-gap",
        "principle-unknown",
        "bad-decl",
        "two-decl",
        "tag-record",
        "changed-decl",
    ] {
        let o = case(key);
        assert_eq!(o.status.code(), Some(1), "{key}");
    }
    // A refused deny-list entry is not echoed.
    let o = case("deny-bidi");
    assert!(!stdout(&o).to_lowercase().contains("exam"), "{}", stdout(&o));
}

#[test]
fn stale_records_never_approve() {
    // An old record (PR 41) touched in PR 42 is stale with --pr 42.
    let o = case("stale-pr");
    assert!(stdout(&o).contains("REVIEW-RECORD-STALE"));
    // An artefact edited after approval no longer matches the record's digest.
    let o = case("stale-digest");
    assert!(stdout(&o).contains("digest differs"), "{}", stdout(&o));
    case("ok+pr");
    case("stale-pr-nopr");
    // The approvals are reported for the CI wrapper.
    let r = json_out(&case("ok"));
    let CValue::Arr(a) = get(&r, "approvals") else { panic!("approvals") };
    assert_eq!(a.len(), 1);
    assert_eq!(get(&a[0], "approver"), &CValue::Str("A. Maintainer".into()));
}

#[test]
fn map_change_and_governance_files() {
    // Changing the map puts every changed path under review.
    case("map-weakened");
    // Governance files under review/ need no declaration; other artefacts still do.
    case("map-reviewed");
}

#[test]
fn whole_package_sweep_and_repo_root_paths() {
    case("all-ok");
    case("all-no-decl");
    case("base-ok");
    case("base-extra");
    case("base-all");
    // --root outside --base is a usage error.
    let o = run(&["review-check", "--all", "--root", &fix("ok/design"), "--base", &fix("base-repo/repo")]);
    assert_eq!(o.status.code(), Some(2));
    // The shipped package: every artefact under the map has a declaration.
    let o = run(&["review-check", "--all", "--root", &shipped(""), "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    let r = json_out(&o);
    let need = strs(get(&r, "needs_review"));
    for f in ["principles.md", "reserved-words.json", "writing-guide.md"] {
        assert!(need.contains(&f.to_string()), "{f} not swept: {need:?}");
    }
}

#[test]
fn unreadable_input_exits_2() {
    let missing = fix("no-such-dir/changed.txt");
    let o = run(&["review-check", "--changed", &missing, "--root", &fix("ok/design")]);
    assert_eq!(o.status.code(), Some(2));
    // A root without review/paths.toml or principles.md.
    let o = run(&["review-check", "--changed", &fix("ok/changed.txt"), "--root", &fix("ok")]);
    assert_eq!(o.status.code(), Some(2));
    let o = run(&[
        "review-check",
        "--changed",
        &fix("ok/changed.txt"),
        "--root",
        &fix("ok/design"),
        "--deny-names",
        &fix("no-such-deny.txt"),
    ]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn bad_usage_exits_2() {
    for args in [
        vec!["review-check"],
        vec!["review-check", "--changed", "x"],
        vec!["review-check", "--root", "x"],
        vec!["review-check", "--changed", "x", "--root", "y", "--from", "z"],
        vec!["pkg-check", "--root", "x", "--changed", "y"],
        vec!["princ-check", "--deny-names", "y"],
        vec!["review-check", "--changed", "x", "--root", "y", "--bogus"],
        vec!["review-check", "--changed", "x", "--root", "y", "--pr", "0"],
        vec!["review-check", "--changed", "x", "--root", "y", "--pr", "abc"],
        vec!["review-check", "--all", "--changed", "x", "--root", "y"],
        vec!["review-check", "--all", "--pr", "3", "--root", "y"],
        vec!["princ-check", "--all"],
        vec!["pkg-check", "--root", "x", "--base", "y"],
    ] {
        assert_eq!(run(&args).status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn shipped_review_files() {
    // REQ-006: every checklist item, the originality statement and the placeholder name.
    let checklist = std::fs::read_to_string(shipped("review/checklist.md")).unwrap();
    for item in CHECKLIST_ITEMS {
        assert!(checklist.contains(&format!("**{item}**")), "{item} missing from checklist.md");
    }
    assert!(checklist.contains(review::STATEMENT_VERSION));
    assert!(checklist.contains("{{FAMILY_NAME}}"));
    assert!(checklist.contains("cannot verify who approved"));
    for needle in ["colour alone", "keyboard path", "Contrast evidence", "trade-dress"] {
        assert!(checklist.to_lowercase().contains(&needle.to_lowercase()), "{needle}");
    }
    // The shipped map is valid; the shipped rule slot is empty until a reviewed PR fills it,
    // and the check then refuses to pass (fail closed).
    let mut f = Vec::new();
    let map = review::parse_paths(&std::fs::read_to_string(shipped("review/paths.toml")).unwrap(), "p", &mut f);
    assert_eq!(map, Some(vec!["**".to_string()]));
    assert!(f.is_empty());
    let dress = review::parse_dress(&std::fs::read_to_string(shipped("review/dress-rules.json")).unwrap(), "d", &mut f);
    assert_eq!(dress, None);
    assert_eq!(f.len(), 1);
    // The shipped package names no deny-listed name.
    let o = run(&[
        "review-check",
        "--changed",
        &fix("ok/changed.txt"),
        "--root",
        &shipped(""),
        "--deny-names",
        &fix("deny-names.txt"),
        "--json",
    ]);
    let r = json_out(&o);
    assert!(!finding_keys(&r).iter().any(|k| k.starts_with("REVIEW-NAME-LEAK")), "{}", stdout(&o));
}

#[test]
fn library_record_and_declaration_checks() {
    let known: std::collections::BTreeSet<String> = ["P-DATA".to_string()].into_iter().collect();
    let rules = vec!["TD-01".to_string()];
    let checklist: Vec<String> =
        CHECKLIST_ITEMS.iter().map(|c| format!("{{\"item\":\"{c}\",\"result\":\"pass\"}}")).collect();
    let rec = |pr: &str, date: &str| {
        format!(
            "{{\"pr\":{pr},\"artefacts\":[\"a.svg\"],\"digests\":{{\"a.svg\":\"deleted\"}},\"principles\":[\"P-DATA\"],\"dress\":[{{\"rule\":\"TD-01\",\"result\":\"pass\"}}],\"checklist\":[{}],\"approver\":{{\"name\":\"A\",\"principal\":\"human\"}},\"date\":\"{date}\"}}",
            checklist.join(",")
        )
    };
    let mut f = Vec::new();
    assert_eq!(
        review::check_record("r.json", &rec("7", "2026-11-19"), Some(rules.as_slice()), &known, Some(7), &mut f)
            .map(|a| (a.artefacts, a.approver)),
        Some((vec!["a.svg".to_string()], "A".to_string()))
    );
    assert!(f.is_empty(), "{f:?}");
    // --pr mismatch.
    let mut f = Vec::new();
    assert!(review::check_record("r.json", &rec("7", "2026-11-19"), Some(rules.as_slice()), &known, Some(8), &mut f)
        .is_none());
    assert_eq!(f[0].code, review::RECORD_STALE);
    // Digests: wrong form, or not one per artefact.
    for d in ["{\"a.svg\":\"sha256:ABC\"}", "{}", "{\"a.svg\":\"deleted\",\"b.svg\":\"deleted\"}", "[]"] {
        let text = rec("7", "2026-11-19").replace("{\"a.svg\":\"deleted\"}", d);
        let mut f = Vec::new();
        assert!(review::check_record("r.json", &text, Some(rules.as_slice()), &known, None, &mut f).is_none(), "{d}");
        assert!(f.iter().all(|x| x.code == review::SCHEMA), "{f:?}");
    }
    for (pr, date) in [("0", "2026-11-19"), ("1.5", "2026-11-19"), ("\"7\"", "2026-11-19"), ("7", "2026-02-30")] {
        let mut f = Vec::new();
        assert_eq!(
            review::check_record("r.json", &rec(pr, date), Some(rules.as_slice()), &known, None, &mut f),
            None,
            "{pr} {date}"
        );
        assert!(f.iter().all(|x| x.code == review::SCHEMA), "{f:?}");
    }
    let decl = |refs: &str| {
        format!("{{\"artefact\":\"a.svg\",\"author\":\"B\",\"date\":\"2026-11-19\",\"references\":[{refs}],\"statement_version\":\"originality-statement@1\"}}")
    };
    assert_eq!(review::check_decl(&decl("\"A sketch\""), "a.svg"), Ok(()));
    assert!(review::check_decl(&decl("\"A sketch\""), "b.svg").is_err());
    assert!(review::check_decl(&decl(""), "a.svg").is_err());
    assert!(review::check_decl(&decl("\"   \""), "a.svg").is_err());
    assert!(review::check_decl(&decl("\"see shot.PNG\""), "a.svg").is_err());
    assert!(review::check_decl(&decl("\"Ana \\u202eLopez\""), "a.svg").is_err());
}
