//! Acceptance criteria of spec node OUTR.BRAND.REVIEW (AC-1 .. AC-6) and error paths.
//! Expected results are hand-labelled from the spec's rules (the EXPECTED table in
//! `fixtures/outr/review/make_fixtures.py`), not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_outreach::review_gate::{
    check, parse_checklist, parse_record, parse_roster, read_checklist, run, Code, GateError, Inputs,
};
use std::path::PathBuf;
use std::process::{Command, Output};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/review").join(rel)
}

/// Runs the AC-1 command line from inside fixture directory `name`.
fn gate(name: &str, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-outreach"))
        .current_dir(fix(name))
        .args([
            "review-gate",
            "--record",
            "records/site-2027.01.toml",
            "--author",
            "alice",
            "--roster",
            "roster.txt",
            "--checklist",
            "checklist.toml",
        ])
        .args(extra)
        .output()
        .unwrap()
}

fn show(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// Finding lines of a text-mode run (the summary line is dropped).
fn lines(o: &Output) -> Vec<String> {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("review-gate:")).map(str::to_string).collect()
}

fn table(text: &str) -> toml::Table {
    text.parse().unwrap()
}

fn json(o: &Output) -> CValue {
    let bytes = o.stdout.strip_suffix(b"\n").unwrap_or(&o.stdout);
    let v = fid_canon::parse(bytes).unwrap();
    assert_eq!(fid_canon::to_canonical_bytes(&v).unwrap(), bytes, "not canonical JSON");
    v
}

fn obj(v: &CValue) -> &std::collections::BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("not an object: {other:?}"),
    }
}

#[test]
fn ac1_ok_exits_0() {
    let o = gate("ok", &[]);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    assert!(lines(&o).is_empty(), "{}", show(&o));
    assert!(show(&o).contains("release site-2027.01 reviewed by bob: passed"), "{}", show(&o));
    let o = gate("ok", &["--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let v = json(&o);
    let m = obj(&v);
    assert_eq!(m.get("ok"), Some(&CValue::Bool(true)));
    assert_eq!(m.get("release"), Some(&CValue::Str("site-2027.01".into())));
    assert_eq!(m.get("reviewer"), Some(&CValue::Str("bob".into())));
    assert_eq!(m.get("findings"), Some(&CValue::Arr(vec![])));
}

#[test]
fn ac2_missing_item_names_screenshots() {
    let o = gate("missing-item", &[]);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(lines(&o), ["site-2027.01 missing_answer:screenshots"]);
}

#[test]
fn ac3_fail_item_names_layout() {
    let o = gate("fail-item", &[]);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(lines(&o), ["site-2027.01 failed_item:layout"]);
    let o = gate("fail-item", &["--json"]);
    assert_eq!(o.status.code(), Some(1));
    let v = json(&o);
    let m = obj(&v);
    assert_eq!(m.get("ok"), Some(&CValue::Bool(false)));
    let CValue::Arr(f) = m.get("findings").unwrap() else { panic!() };
    assert_eq!(f.len(), 1);
    assert_eq!(obj(&f[0]).get("code"), Some(&CValue::Str("failed_item".into())));
    assert_eq!(obj(&f[0]).get("item"), Some(&CValue::Str("layout".into())));
}

#[test]
fn ac4_self_review() {
    let o = gate("self", &[]);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(lines(&o), ["site-2027.01 self_review"]);
}

#[test]
fn ac5_reviewer_not_on_roster() {
    let o = gate("not-on-roster", &[]);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(lines(&o), ["site-2027.01 unknown_reviewer"]);
}

#[test]
fn ac6_missing_record_exits_2_naming_path() {
    let o = Command::new(env!("CARGO_BIN_EXE_fid-outreach"))
        .current_dir(fix("ok"))
        .args(["review-gate", "--record", "records/site-2099.01.toml", "--author", "alice"])
        .args(["--roster", "roster.txt", "--checklist", "checklist.toml"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("record_missing") && err.contains("site-2099.01.toml"), "{err}");
    let o = Command::new(env!("CARGO_BIN_EXE_fid-outreach"))
        .current_dir(fix("ok"))
        .args(["review-gate", "--record", "records/site-2099.01.toml", "--author", "alice"])
        .args(["--roster", "roster.txt", "--checklist", "checklist.toml", "--json"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    let v = json(&o);
    let e = obj(obj(&v).get("error").unwrap());
    assert_eq!(e.get("code"), Some(&CValue::Str("record_missing".into())));
    assert!(matches!(e.get("path"), Some(CValue::Str(p)) if p.contains("site-2099.01.toml")));
}

#[test]
fn missing_roster_or_checklist_exits_2() {
    for (roster, checklist) in [("nope.txt", "checklist.toml"), ("roster.txt", "nope.toml")] {
        let o = Command::new(env!("CARGO_BIN_EXE_fid-outreach"))
            .current_dir(fix("ok"))
            .args(["review-gate", "--record", "records/site-2027.01.toml", "--author", "alice"])
            .args(["--roster", roster, "--checklist", checklist])
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(2), "{}", show(&o));
        assert!(String::from_utf8_lossy(&o.stderr).contains("nope."), "{}", show(&o));
    }
}

#[test]
fn bad_usage_exits_2() {
    let bin = env!("CARGO_BIN_EXE_fid-outreach");
    let o = Command::new(bin).current_dir(fix("ok")).args(["review-gate", "--record", "x.toml"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    let o = Command::new(bin)
        .current_dir(fix("ok"))
        .args(["review-gate", "--record", "records/site-2027.01.toml", "--author", "alice", "--author", "bob"])
        .args(["--roster", "roster.txt", "--checklist", "checklist.toml"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
}

// ------------------------------------------------------------------ rules on parsed inputs

const CHECKLIST: &str = r#"version = 1
items = [
  { id = "colours", question = "q1" },
  { id = "layout", question = "q2" },
  { id = "screenshots", question = "q3" },
]
"#;

fn record_with(reviewer: &str, answers: &str) -> fid_outreach::review_gate::Record {
    let text = format!("release = \"r-1\"\nreviewer = \"{reviewer}\"\ndate = \"2027-01-20\"\n[answers]\n{answers}\n");
    parse_record(&table(&text), "rec").unwrap()
}

fn roster() -> std::collections::BTreeSet<String> {
    parse_roster("alice\nbob\n", "roster").unwrap()
}

fn findings(reviewer: &str, author: &str, answers: &str) -> Vec<String> {
    let c = parse_checklist(&table(CHECKLIST), "c").unwrap();
    check(&c, &record_with(reviewer, answers), author, &roster()).findings.iter().map(|f| f.to_string()).collect()
}

const ALL_PASS: &str =
    "colours = { result = \"pass\" }\nlayout = { result = \"pass\" }\nscreenshots = { result = \"pass\" }";

#[test]
fn all_findings_are_reported_sorted_by_item() {
    // Reviewer findings first, then item findings in item-ID order.
    let got = findings("Mallory", "mallory", "screenshots = { result = \"fail\" }\nextra = { result = \"fail\" }");
    assert_eq!(
        got,
        [
            "unknown_reviewer",
            "self_review",
            "missing_answer:colours",
            "missing_answer:layout",
            "failed_item:screenshots"
        ]
    );
}

#[test]
fn self_review_is_case_insensitive() {
    assert_eq!(findings("Alice", "alice", ALL_PASS), ["self_review"]);
    assert_eq!(findings("bob", " BOB ", ALL_PASS), ["self_review"]);
    assert!(findings("bob", "alice", ALL_PASS).is_empty());
    // Non-ASCII handles fold too.
    let c = parse_checklist(&table(CHECKLIST), "c").unwrap();
    let r = parse_roster("ÄNNE\n", "roster").unwrap();
    let got = check(&c, &record_with("änne", ALL_PASS), "Änne", &r);
    assert_eq!(got.findings.iter().map(|f| f.to_string()).collect::<Vec<_>>(), ["self_review"]);
}

#[test]
fn roster_match_is_case_insensitive() {
    assert!(findings("BOB", "alice", ALL_PASS).is_empty());
}

#[test]
fn only_exact_pass_passes() {
    let a = |r: &str| {
        format!("colours = {{ result = {r} }}\nlayout = {{ result = \"pass\" }}\nscreenshots = {{ result = \"pass\" }}")
    };
    for r in ["\"Pass\"", "\"PASS\"", "\"fail\"", "\"\"", "\"skipped\"", "\" pass\""] {
        assert_eq!(findings("bob", "alice", &a(r)), ["failed_item:colours"], "{r}");
    }
    // An answer with no string result has no answer.
    for body in ["colours = { note = \"looked\" }", "colours = { result = true }"] {
        let answers = format!("{body}\nlayout = {{ result = \"pass\" }}\nscreenshots = {{ result = \"pass\" }}");
        assert_eq!(findings("bob", "alice", &answers), ["missing_answer:colours"], "{body}");
    }
}

#[test]
fn record_without_answers_misses_every_item() {
    let rec = parse_record(&table("release = \"r-1\"\nreviewer = \"bob\"\ndate = \"2027-01-20\""), "rec").unwrap();
    let c = parse_checklist(&table(CHECKLIST), "c").unwrap();
    let got: Vec<String> = check(&c, &rec, "alice", &roster()).findings.iter().map(|f| f.to_string()).collect();
    assert_eq!(got, ["missing_answer:colours", "missing_answer:layout", "missing_answer:screenshots"]);
}

// ------------------------------------------------------------------ parsers

#[test]
fn checklist_parse_errors() {
    let bad = [
        "items = [{ id = \"a\", question = \"q\" }]",
        "version = 0\nitems = [{ id = \"a\", question = \"q\" }]",
        "version = \"1\"\nitems = [{ id = \"a\", question = \"q\" }]",
        "version = 1\nitems = []",
        "version = 1",
        "version = 1\nitems = [{ id = \"a\" }]",
        "version = 1\nitems = [{ id = \"\", question = \"q\" }]",
        "version = 1\nitems = [{ id = \"a\", question = \"q\" }, { id = \"a\", question = \"r\" }]",
        "version = 1\nitems = [\"a\"]",
    ];
    for t in bad {
        let e = parse_checklist(&table(t), "c").unwrap_err();
        assert_eq!(e.code(), "parse_error", "{t}");
    }
    let c = parse_checklist(&table(CHECKLIST), "c").unwrap();
    assert_eq!(c.version, 1);
    assert_eq!(c.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["colours", "layout", "screenshots"]);
}

#[test]
fn record_parse_rules() {
    for t in [
        "reviewer = \"bob\"",
        "release = \"\"",
        "release = \"r\"\nreviewer = \"\"",
        "release = \"r\"\nreviewer = 3",
        "release = \"r\"\ndate = \"20 January 2027\"",
        "release = \"r\"\ndate = 2027-01-20T10:00:00",
        "release = \"r\"\nanswers = \"all pass\"",
        "release = \"r\"\n[answers]\ncolours = \"pass\"",
        "release = \"r\"\n[answers]\ncolours = { result = \"pass\", note = 1 }",
    ] {
        assert_eq!(parse_record(&table(t), "rec").unwrap_err().code(), "parse_error", "{t}");
    }
    // A TOML local date is accepted and kept as YYYY-MM-DD.
    let r = parse_record(&table("release = \"r\"\nreviewer = \"bob\"\ndate = 2027-01-20"), "rec").unwrap();
    assert_eq!(r.date.as_deref(), Some("2027-01-20"));
}

#[test]
fn roster_rules() {
    let r = parse_roster("# reviewers\n\n  alice  \nBob\n", "roster").unwrap();
    assert_eq!(r.into_iter().collect::<Vec<_>>(), ["alice", "bob"]);
    for t in ["", "# only a comment\n", "alice smith\n", "alice\nrelease-helper[bot]\n", "Some-App[BOT]\n"] {
        assert_eq!(parse_roster(t, "roster").unwrap_err().code(), "parse_error", "{t:?}");
    }
}

fn tmp_case(name: &str, record: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-review-gate-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("record.toml"), record).unwrap();
    std::fs::write(d.join("roster.txt"), "alice\nbob\n").unwrap();
    std::fs::write(d.join("checklist.toml"), CHECKLIST).unwrap();
    d
}

fn run_case(d: &std::path::Path, author: &str) -> Result<fid_outreach::review_gate::Report, GateError> {
    run(&Inputs {
        record: &d.join("record.toml"),
        author,
        roster: &d.join("roster.txt"),
        checklist: &d.join("checklist.toml"),
    })
}

#[test]
fn gate_requires_reviewer_and_date() {
    let d = tmp_case("no-reviewer", "release = \"r\"\ndate = \"2027-01-20\"\n");
    let e = run_case(&d, "alice").unwrap_err();
    assert_eq!(e.code(), "parse_error");
    assert!(e.to_string().contains("reviewer"));
    let _ = std::fs::remove_dir_all(&d);
    let d = tmp_case("no-date", "release = \"r\"\nreviewer = \"bob\"\n");
    let e = run_case(&d, "alice").unwrap_err();
    assert!(e.to_string().contains("date"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn unparsable_record_and_empty_author_exit_2() {
    let d = tmp_case("garbled", "release = \"r\n");
    assert_eq!(run_case(&d, "alice").unwrap_err().code(), "parse_error");
    let _ = std::fs::remove_dir_all(&d);
    let d = tmp_case(
        "empty-author",
        &format!("release = \"r\"\nreviewer = \"bob\"\ndate = \"2027-01-20\"\n[answers]\n{ALL_PASS}\n"),
    );
    assert_eq!(run_case(&d, "  ").unwrap_err().code(), "parse_error");
    let rep = run_case(&d, "alice").unwrap();
    assert!(rep.findings.is_empty());
    assert_eq!(rep.exit_code(), 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn finding_codes() {
    assert_eq!(Code::UnknownReviewer.as_str(), "unknown_reviewer");
    assert_eq!(Code::SelfReview.as_str(), "self_review");
    assert_eq!(Code::MissingAnswer.as_str(), "missing_answer");
    assert_eq!(Code::FailedItem.as_str(), "failed_item");
}

/// REQ-OUTR.BRAND.REVIEW-001: the repository's checklist has the six required items.
#[test]
fn repository_checklist_has_required_items() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../brand/trade-dress-checklist.toml");
    let ids = read_checklist(&path).unwrap().ids();
    for id in ["colours", "icon_style", "layout", "screenshots", "typography", "copied_text"] {
        assert!(ids.contains(id), "{id}");
    }
}
