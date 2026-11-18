//! Acceptance criteria of spec node OUTR.BRAND.REVIEW (AC-1 .. AC-6) and error paths.
//! Expected results are hand-labelled from the spec's rules (the EXPECTED table in
//! `fixtures/outr/review/make_fixtures.py`), not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_outreach::review_gate::{
    check, denylist, is_calendar_date, is_valid_handle, parse_checklist, parse_record, parse_roster, read_checklist,
    run, Code, Expect, GateError, Inputs, Record, Report,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/review").join(rel)
}

/// The AC-1 command line, plus `--release` (added after review of PR #106).
const AC_ARGS: [&str; 11] = [
    "review-gate",
    "--record",
    "records/site-2027.01.toml",
    "--release",
    "site-2027.01",
    "--author",
    "alice",
    "--roster",
    "roster.txt",
    "--checklist",
    "checklist.toml",
];

/// Runs `args` from inside fixture directory `name`.
fn gate_args(name: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-outreach")).current_dir(fix(name)).args(args).output().unwrap()
}

/// Runs the AC command line plus `extra` from inside fixture directory `name`.
fn gate(name: &str, extra: &[&str]) -> Output {
    gate_args(name, &[&AC_ARGS[..], extra].concat())
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
    let mut args = AC_ARGS;
    args[2] = "records/site-2099.01.toml";
    let o = gate_args("ok", &args);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("record_missing") && err.contains("site-2099.01.toml"), "{err}");
    let o = gate_args("ok", &[&args[..], &["--json"][..]].concat());
    assert_eq!(o.status.code(), Some(2));
    let v = json(&o);
    let e = obj(obj(&v).get("error").unwrap());
    assert_eq!(e.get("code"), Some(&CValue::Str("record_missing".into())));
    assert!(matches!(e.get("path"), Some(CValue::Str(p)) if p.contains("site-2099.01.toml")));
}

#[test]
fn co_authors_cannot_review() {
    // bob reviewed `ok`; naming bob as a co-author makes it a self-review.
    let o = gate("ok", &["--author", "BOB"]);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(lines(&o), ["site-2027.01 self_review"]);
    let o = gate("ok", &["--author", "carol"]);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
}

#[test]
fn release_must_match_record() {
    let mut args = AC_ARGS;
    args[4] = "site-2027.02";
    let o = gate_args("ok", &args);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert_eq!(lines(&o), ["site-2027.02 release_mismatch"]);
}

#[test]
fn missing_roster_or_checklist_exits_2() {
    for (i, name) in [(8, "nope.txt"), (10, "nope.toml")] {
        let mut args = AC_ARGS;
        args[i] = name;
        let o = gate_args("ok", &args);
        assert_eq!(o.status.code(), Some(2), "{}", show(&o));
        assert!(String::from_utf8_lossy(&o.stderr).contains("nope."), "{}", show(&o));
    }
}

#[test]
fn bad_usage_exits_2() {
    // Missing options, a repeated single-valued option, invalid author handles.
    let cases: Vec<Vec<&str>> = vec![
        vec!["review-gate", "--record", "x.toml"],
        AC_ARGS.iter().copied().filter(|a| *a != "--release" && *a != "site-2027.01").collect(),
        AC_ARGS.iter().copied().filter(|a| *a != "--author" && *a != "alice").collect(),
        [&AC_ARGS[..], &["--release", "site-2027.01"][..]].concat(),
        [&AC_ARGS[..], &["--author", "release-bot[bot]"][..]].concat(),
        [&AC_ARGS[..], &["--author", "agent:claude"][..]].concat(),
    ];
    for args in cases {
        let o = gate_args("ok", &args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", show(&o));
    }
}

fn tmp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-review-gate-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn bots_register_extends_the_denylist() {
    let d = tmp_dir("bots");
    let reg = d.join("bots.toml");
    std::fs::write(&reg, "[[bot]]\nid = \"lint\"\naccount = \"house-linter[bot]\"\n").unwrap();
    let roster = d.join("roster.txt");
    std::fs::write(&roster, "alice\nbob\nhouse-linter\n").unwrap();
    let roster_s = roster.to_string_lossy().into_owned();
    let reg_s = reg.to_string_lossy().into_owned();
    let mut args: Vec<&str> = AC_ARGS.to_vec();
    args[8] = &roster_s;
    // Without the register the roster parses and bob passes.
    assert_eq!(gate_args("ok", &args).status.code(), Some(0));
    // With it, the roster lists a bot: exit 2 naming it.
    args.extend(["--bots", reg_s.as_str()]);
    let o = gate_args("ok", &args);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    assert!(show(&o).contains("house-linter"), "{}", show(&o));
    // A missing register is exit 2.
    let o = gate("ok", &["--bots", "no-such-bots.toml"]);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    let _ = std::fs::remove_dir_all(&d);
}

// ------------------------------------------------------------------ rules on parsed inputs

const CHECKLIST: &str = r#"version = 2
items = [
  { id = "colours", question = "q1" },
  { id = "layout", question = "q2" },
  { id = "screenshots", question = "q3" },
]
"#;

const ALL_PASS: &str =
    "colours = { result = \"pass\" }\nlayout = { result = \"pass\" }\nscreenshots = { result = \"pass\" }";

fn record_full(release: &str, reviewer: &str, version: i64, answers: &str) -> Record {
    let head = format!("release = \"{release}\"\nreviewer = \"{reviewer}\"\ndate = \"2027-01-20\"\n");
    let text = format!("{head}checklist_version = {version}\n[answers]\n{answers}\n");
    parse_record(&table(&text), "rec").unwrap()
}

fn record_with(reviewer: &str, answers: &str) -> Record {
    record_full("r-1", reviewer, 2, answers)
}

fn no_bots() -> BTreeSet<String> {
    denylist(None, "").unwrap()
}

fn roster() -> BTreeSet<String> {
    parse_roster("alice\nbob\n", "roster", &no_bots()).unwrap()
}

fn check_with(record: &Record, authors: &[&str], roster: &BTreeSet<String>, denied: &BTreeSet<String>) -> Report {
    let c = parse_checklist(&table(CHECKLIST), "c").unwrap();
    let authors: Vec<String> = authors.iter().map(|a| (*a).to_string()).collect();
    check(&c, record, &Expect { release: "r-1", authors: &authors, roster, denied })
}

fn strings(r: &Report) -> Vec<String> {
    r.findings.iter().map(|f| f.to_string()).collect()
}

fn findings(reviewer: &str, author: &str, answers: &str) -> Vec<String> {
    strings(&check_with(&record_with(reviewer, answers), &[author], &roster(), &no_bots()))
}

#[test]
fn all_findings_are_reported_sorted_by_item() {
    // Findings without an item first (in code order), then item findings in item-ID order.
    let rec = record_full("r-0", "Mallory", 1, "screenshots = { result = \"fail\" }\nbanner = { result = \"pass\" }");
    let got = strings(&check_with(&rec, &["mallory"], &roster(), &no_bots()));
    assert_eq!(
        got,
        [
            "unknown_reviewer",
            "self_review",
            "release_mismatch",
            "checklist_version_mismatch",
            "unknown_item:banner",
            "missing_answer:colours",
            "missing_answer:layout",
            "failed_item:screenshots",
        ]
    );
}

#[test]
fn self_review_is_case_insensitive_and_covers_co_authors() {
    assert_eq!(findings("Alice", "alice", ALL_PASS), ["self_review"]);
    assert_eq!(findings("bob", " BOB ", ALL_PASS), ["self_review"]);
    assert!(findings("bob", "alice", ALL_PASS).is_empty());
    let r = check_with(&record_with("bob", ALL_PASS), &["alice", "carol", "Bob"], &roster(), &no_bots());
    assert_eq!(strings(&r), ["self_review"]);
}

#[test]
fn roster_match_is_case_insensitive() {
    assert!(findings("BOB", "alice", ALL_PASS).is_empty());
}

#[test]
fn denied_reviewer_is_a_bot_reviewer() {
    // Even if a roster somehow held a denied account, the reviewer is refused.
    let mut roster = roster();
    roster.insert("dependabot".into());
    let r = check_with(&record_with("Dependabot", ALL_PASS), &["alice"], &roster, &no_bots());
    assert_eq!(strings(&r), ["bot_reviewer"]);
}

#[test]
fn checklist_version_must_match() {
    let r = check_with(&record_full("r-1", "bob", 1, ALL_PASS), &["alice"], &roster(), &no_bots());
    assert_eq!(strings(&r), ["checklist_version_mismatch"]);
}

#[test]
fn answers_to_unknown_items_are_findings() {
    let answers = format!("{ALL_PASS}\nlogo = {{ result = \"pass\" }}");
    assert_eq!(findings("bob", "alice", &answers), ["unknown_item:logo"]);
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
    let r = check_with(&record_with("bob", ""), &["alice"], &roster(), &no_bots());
    assert_eq!(strings(&r), ["missing_answer:colours", "missing_answer:layout", "missing_answer:screenshots"]);
}

// ------------------------------------------------------------------ handles and dates

#[test]
fn github_login_grammar() {
    let max = "a".repeat(39);
    let too_long = "a".repeat(40);
    let max_hyphens = format!("a{}", "-a".repeat(38));
    for ok in ["a", "Bob", "alice-smith", "a1-b2-c3", "0", max.as_str(), max_hyphens.as_str()] {
        assert!(is_valid_handle(ok), "{ok}");
    }
    let bad = [
        "",
        "-alice",
        "alice-",
        "al--ice",
        "alice_smith",
        "alice.smith",
        "renovate[bot]",
        "agent:claude",
        "app/renovate",
        "al ice",
        "\u{430}lice",
        "ali\u{441}e",
        "\u{ff41}lice",
        too_long.as_str(),
    ];
    for b in bad {
        assert!(!is_valid_handle(b), "{b:?}");
    }
}

#[test]
fn calendar_dates() {
    for ok in ["2027-01-20", "2028-02-29", "2000-02-29", "2027-12-31", "2027-04-30"] {
        assert!(is_calendar_date(ok), "{ok}");
    }
    let bad = ["2027-02-29", "1900-02-29", "2027-04-31", "2027-13-01", "2027-00-10", "2027-01-00", "2027-1-20"];
    for b in bad {
        assert!(!is_calendar_date(b), "{b}");
    }
}

#[test]
fn denylist_defaults_and_register() {
    let d = no_bots();
    for h in ["github-actions", "dependabot", "renovate"] {
        assert!(d.contains(h), "{h}");
    }
    let reg = table(
        "[[bot]]\nid = \"a\"\naccount = \"Review-Helper[bot]\"\n\n[[bot]]\nid = \"b\"\naccount = \"app/scanner\"\n",
    );
    let d = denylist(Some(&reg), "bots.toml").unwrap();
    assert!(d.contains("review-helper") && d.contains("scanner"));
    for bad in ["bot = 1", "[[bot]]\nid = \"a\"", "[[bot]]\naccount = 3"] {
        assert_eq!(denylist(Some(&table(bad)), "bots.toml").unwrap_err().code(), "parse_error", "{bad}");
    }
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
    assert_eq!(c.version, 2);
    assert_eq!(c.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["colours", "layout", "screenshots"]);
}

#[test]
fn record_parse_rules() {
    for t in [
        "reviewer = \"bob\"",
        "release = \"\"",
        "release = \"r\"\nreviewer = \"\"",
        "release = \"r\"\nreviewer = 3",
        "release = \"r\"\nreviewer = \"helper[bot]\"",
        "release = \"r\"\nreviewer = \"agent:claude\"",
        "release = \"r\"\nreviewer = \"\u{430}lice\"",
        "release = \"r\"\ndate = \"20 January 2027\"",
        "release = \"r\"\ndate = \"2027-02-30\"",
        "release = \"r\"\ndate = 2027-01-20T10:00:00",
        "release = \"r\"\nchecklist_version = \"1\"",
        "release = \"r\"\nanswers = \"all pass\"",
        "release = \"r\"\n[answers]\ncolours = \"pass\"",
        "release = \"r\"\n[answers]\ncolours = { result = \"pass\", note = 1 }",
    ] {
        assert_eq!(parse_record(&table(t), "rec").unwrap_err().code(), "parse_error", "{t}");
    }
    // A TOML local date is accepted and kept as YYYY-MM-DD.
    let t = "release = \"r\"\nreviewer = \"bob\"\ndate = 2027-01-20\nchecklist_version = 1";
    let r = parse_record(&table(t), "rec").unwrap();
    assert_eq!(r.date.as_deref(), Some("2027-01-20"));
    assert_eq!(r.checklist_version, Some(1));
}

#[test]
fn roster_rules() {
    let r = parse_roster("# reviewers\n\n  alice  \nBob\n", "roster", &no_bots()).unwrap();
    assert_eq!(r.into_iter().collect::<Vec<_>>(), ["alice", "bob"]);
    for t in [
        "",
        "# only a comment\n",
        "alice smith\n",
        "alice\nrelease-helper[bot]\n",
        "Some-App[BOT]\n",
        "agent:claude\n",
        "\u{430}lice\n",
        "alice\nGitHub-Actions\n",
        "dependabot\n",
        "renovate\n",
    ] {
        assert_eq!(parse_roster(t, "roster", &no_bots()).unwrap_err().code(), "parse_error", "{t:?}");
    }
}

fn tmp_case(name: &str, record: &str) -> PathBuf {
    let d = tmp_dir(name);
    std::fs::write(d.join("record.toml"), record).unwrap();
    std::fs::write(d.join("roster.txt"), "alice\nbob\n").unwrap();
    std::fs::write(d.join("checklist.toml"), CHECKLIST).unwrap();
    d
}

fn run_case(d: &Path, release: &str, authors: &[&str]) -> Result<Report, GateError> {
    let authors: Vec<String> = authors.iter().map(|a| (*a).to_string()).collect();
    run(&Inputs {
        record: &d.join("record.toml"),
        release,
        authors: &authors,
        roster: &d.join("roster.txt"),
        checklist: &d.join("checklist.toml"),
        bots: None,
    })
}

#[test]
fn gate_requires_reviewer_date_and_checklist_version() {
    for (name, text, field) in [
        ("no-reviewer", "release = \"r\"\ndate = \"2027-01-20\"\nchecklist_version = 2\n", "reviewer"),
        ("no-date", "release = \"r\"\nreviewer = \"bob\"\nchecklist_version = 2\n", "date"),
        ("no-version", "release = \"r\"\nreviewer = \"bob\"\ndate = \"2027-01-20\"\n", "checklist_version"),
    ] {
        let d = tmp_case(name, text);
        let e = run_case(&d, "r", &["alice"]).unwrap_err();
        assert_eq!(e.code(), "parse_error", "{name}");
        assert!(e.to_string().contains(field), "{name}: {e}");
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[test]
fn unparsable_record_and_bad_arguments_exit_2() {
    let d = tmp_case("garbled", "release = \"r\n");
    assert_eq!(run_case(&d, "r", &["alice"]).unwrap_err().code(), "parse_error");
    let _ = std::fs::remove_dir_all(&d);
    let head = "release = \"r\"\nreviewer = \"bob\"\ndate = \"2027-01-20\"\nchecklist_version = 2\n";
    let d = tmp_case("args", &format!("{head}[answers]\n{ALL_PASS}\n"));
    assert_eq!(run_case(&d, "r", &[]).unwrap_err().code(), "parse_error");
    assert_eq!(run_case(&d, "r", &["  "]).unwrap_err().code(), "parse_error");
    assert_eq!(run_case(&d, "r", &["alice", "ci[bot]"]).unwrap_err().code(), "parse_error");
    assert_eq!(run_case(&d, " ", &["alice"]).unwrap_err().code(), "parse_error");
    let rep = run_case(&d, "r", &["alice"]).unwrap();
    assert!(rep.findings.is_empty());
    assert_eq!(rep.exit_code(), 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn finding_codes() {
    let all = [
        (Code::UnknownReviewer, "unknown_reviewer"),
        (Code::BotReviewer, "bot_reviewer"),
        (Code::SelfReview, "self_review"),
        (Code::ReleaseMismatch, "release_mismatch"),
        (Code::ChecklistVersionMismatch, "checklist_version_mismatch"),
        (Code::MissingAnswer, "missing_answer"),
        (Code::FailedItem, "failed_item"),
        (Code::UnknownItem, "unknown_item"),
    ];
    for (c, s) in all {
        assert_eq!(c.as_str(), s);
    }
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
