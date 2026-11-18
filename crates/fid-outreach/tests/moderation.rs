//! Acceptance criteria of spec node OUTR.COMM.MOD (AC-1 .. AC-7) and further error paths.
//! Expected finding lines are hand-labelled from the spec's rules and the expected counts are
//! computed by the Python reference in `fixtures/outr/comm/mod/make_fixtures.py`; nothing here
//! was produced by this crate. Every log is copied to a temporary folder outside the repository.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_outreach::moderation::{
    check, parse_log, parse_roster, stats, Date, Quarter, Stamp, RETENTION_MONTHS,
};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/comm/mod").join(rel)
}

/// A fresh folder under the system temporary directory (outside the repository).
fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-outreach-mod-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Copies fixture `name`'s log to a temporary folder and returns the copy's path.
fn tmp_log(name: &str, tag: &str) -> PathBuf {
    let d = tmp(tag);
    let p = d.join("log.toml");
    std::fs::copy(fix(&format!("{name}/log.toml")), &p).unwrap();
    p
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-outreach")).current_dir(fix("")).args(args).output().unwrap()
}

fn show(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn mod_check(name: &str, log: &Path, extra: &[&str]) -> Output {
    let roster = format!("{name}/roster.toml");
    let log = log.to_str().unwrap();
    run(&[&["mod-check", "--log", log, "--roster", &roster, "--today", "2027-04-02"], extra].concat())
}

fn mod_stats(log: &Path, quarter: &str, out: &Path, extra: &[&str]) -> Output {
    let (log, out) = (log.to_str().unwrap(), out.to_str().unwrap());
    run(&[&["mod-stats", "--log", log, "--quarter", quarter, "--target-hours", "72", "--out", out], extra].concat())
}

fn finding_lines(o: &Output) -> Vec<String> {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("mod-")).map(str::to_string).collect()
}

/// The EXPECTED table of make_fixtures.py.
const EXPECTED: [(&str, u8, &[&str]); 6] = [
    ("ok", 0, &[]),
    ("conflict", 1, &["conflict_not_handed_over MOD-2027-004"]),
    ("old", 1, &["retention_overdue MOD-2024-031"]),
    ("bad-category", 1, &["field_invalid outcome MOD-2027-003"]),
    ("unknown-moderator", 1, &["moderator_unknown MOD-2027-005"]),
    ("personal-data", 1, &["field_invalid moderator MOD-2027-001", "field_invalid reporter_name MOD-2027-001"]),
];

#[test]
fn ac1_ok_log_exits_0() {
    let log = tmp_log("ok", "ac1");
    let o = mod_check("ok", &log, &["--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let bytes = o.stdout.strip_suffix(b"\n").unwrap_or(&o.stdout);
    let v = fid_canon::parse(bytes).unwrap();
    assert_eq!(fid_canon::to_canonical_bytes(&v).unwrap(), bytes, "not canonical JSON");
    let CValue::Obj(m) = v else { panic!("not an object") };
    assert_eq!(m.get("ok"), Some(&CValue::Bool(true)));
    assert_eq!(m.get("findings"), Some(&CValue::Arr(vec![])));
    assert_eq!(m.get("cases"), Some(&CValue::Num(5.0)));
}

#[test]
fn expected_table_for_every_fixture() {
    for (name, code, lines) in EXPECTED {
        let log = tmp_log(name, &format!("table-{name}"));
        let o = mod_check(name, &log, &[]);
        assert_eq!(o.status.code(), Some(i32::from(code)), "{name}: {}", show(&o));
        assert_eq!(finding_lines(&o), lines, "{name}: {}", show(&o));
    }
}

#[test]
fn ac2_stats_counts_and_privacy() {
    let log = tmp_log("ok", "ac2");
    let out = log.with_file_name("counts.json");
    let o = mod_stats(&log, "2027Q1", &out, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let got = std::fs::read(&out).unwrap();
    assert_eq!(got, std::fs::read(fix("ok/expected-2027Q1.json")).unwrap());
    let text = String::from_utf8(got).unwrap();
    // No case ID, date, channel or handle from the log.
    for needle in ["MOD-", "2027-0", "T0", "forum", "chat", "issues", "private", "mod-ash", "mod-birch", "mod-cedar"] {
        assert!(!text.contains(needle), "counts.json contains {needle}: {text}");
    }
}

#[test]
fn stats_include_anonymised_entries() {
    let log = tmp_log("old", "anon");
    for quarter in ["2024Q3", "2024Q4"] {
        let out = log.with_file_name(format!("{quarter}.json"));
        let o = mod_stats(&log, quarter, &out, &[]);
        assert_eq!(o.status.code(), Some(0), "{}", show(&o));
        let want = std::fs::read(fix(&format!("old/expected-{quarter}.json"))).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), want, "{quarter}");
    }
}

#[test]
fn ac3_conflict() {
    let log = tmp_log("conflict", "ac3");
    let o = mod_check("conflict", &log, &[]);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert!(show(&o).contains("conflict_not_handed_over MOD-2027-004"), "{}", show(&o));
}

#[test]
fn ac4_retention_overdue() {
    let log = tmp_log("old", "ac4");
    let o = mod_check("old", &log, &[]);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert!(show(&o).contains("retention_overdue MOD-2024-031"), "{}", show(&o));
}

#[test]
fn ac5_log_in_repo_both_commands() {
    let inside = fix("ok/log.toml");
    let o = mod_check("ok", &inside, &[]);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    assert!(show(&o).contains("log_in_repo"), "{}", show(&o));
    let o = mod_check("ok", &inside, &["--json"]);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("\"log_in_repo\""), "{}", show(&o));

    let out = tmp("ac5").join("counts.json");
    let o = mod_stats(&inside, "2027Q1", &out, &[]);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    assert!(show(&o).contains("log_in_repo"), "{}", show(&o));
    assert!(!out.exists(), "mod-stats wrote output for a log inside the repository");
}

#[test]
fn log_in_explicit_repo_root_reads_nothing() {
    // The log path does not even exist: the guard decides on the path alone.
    let d = tmp("root");
    let missing = d.join("sub").join("log.toml");
    std::fs::create_dir_all(d.join("sub")).unwrap();
    let root = d.to_str().unwrap();
    let o = mod_check("ok", &missing, &["--repo-root", root]);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    assert!(show(&o).contains("log_in_repo"), "{}", show(&o));
    // Outside that root, the same missing log is a parse error instead.
    let other = tmp("root-other");
    let o = mod_check("ok", &missing, &["--repo-root", other.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2), "{}", show(&o));
    assert!(show(&o).contains("log_parse"), "{}", show(&o));
}

#[test]
fn ac6_bad_category() {
    let log = tmp_log("bad-category", "ac6");
    let o = mod_check("bad-category", &log, &[]);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert!(show(&o).contains("field_invalid outcome"), "{}", show(&o));
    // mod-stats refuses an invalid log and writes nothing.
    let out = log.with_file_name("counts.json");
    let o = mod_stats(&log, "2027Q1", &out, &[]);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert!(!out.exists());
}

#[test]
fn ac7_stats_byte_identical() {
    let log = tmp_log("ok", "ac7");
    let (a, b) = (log.with_file_name("a.json"), log.with_file_name("b.json"));
    assert_eq!(mod_stats(&log, "2027Q1", &a, &[]).status.code(), Some(0));
    assert_eq!(mod_stats(&log, "2027Q1", &b, &[]).status.code(), Some(0));
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}

#[test]
fn bad_usage_exits_2() {
    let log = tmp_log("ok", "usage");
    let l = log.to_str().unwrap();
    for args in [
        vec!["mod-check", "--log", l, "--roster", "ok/roster.toml"],
        vec!["mod-check", "--log", l, "--roster", "ok/roster.toml", "--today", "2027-02-30"],
        vec!["mod-stats", "--log", l, "--quarter", "2027Q5", "--target-hours", "72", "--out", "x.json"],
        vec!["mod-stats", "--log", l, "--quarter", "2027Q1", "--target-hours", "0", "--out", "x.json"],
        vec!["mod-stats", "--log", l, "--quarter", "2027Q1", "--target-hours", "72", "--out", "x", "--json"],
    ] {
        let o = run(&args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", show(&o));
    }
}

// ------------------------------------------------------------------ library level

fn table(text: &str) -> toml::Table {
    text.parse().unwrap()
}

const ONE_CASE: &str = r#"
[[case]]
id = "C1"
filed_at = 2027-01-04T09:00:00Z
acknowledged_at = "2027-01-04T09:00:00Z"
channel = "forum"
outcome = "warning"
moderator = "mod-ash"
parties_conflicted = []
"#;

#[test]
fn toml_datetimes_and_strings_both_parse() {
    let log = parse_log(&table(ONE_CASE));
    assert!(log.findings.is_empty(), "{:?}", log.findings);
    assert_eq!(log.cases.len(), 1);
}

#[test]
fn field_rules() {
    let roster = parse_roster(&table(r#"moderators = ["mod-ash"]"#), "r").unwrap();
    let today = Date::parse("2027-04-02").unwrap();
    let cases: [(&str, &str, &str); 7] = [
        ("moderator = \"mod-ash\"\n", "", "field_missing moderator C1"),
        ("acknowledged_at = \"2027-01-04T09:00:00Z\"\n", "acknowledged_at = \"2027-01-04T08:59:59Z\"\n", "field_invalid acknowledged_at C1"),
        ("channel = \"forum\"\n", "channel = \"Forum\"\n", "field_invalid channel C1"),
        ("parties_conflicted = []\n", "parties_conflicted = [\"mod-ash\", \"mod-ash\"]\n", "field_invalid parties_conflicted C1"),
        ("parties_conflicted = []\n", "parties_conflicted = []\nhandover = { from = \"mod-x\", to = \"mod-y\" }\n", "field_invalid handover C1"),
        ("parties_conflicted = []\n", "parties_conflicted = []\nnote = \"text of a removed post\"\n", "field_invalid note C1"),
        ("id = \"C1\"\n", "id = \"a b\"\n", "field_invalid id case#1"),
    ];
    for (from, to, want) in cases {
        let text = ONE_CASE.replace(from, to);
        let got: Vec<String> = check(&parse_log(&table(&text)), &roster, today).iter().map(ToString::to_string).collect();
        assert_eq!(got, vec![want.to_string()], "{text}");
    }
    // A duplicate case ID.
    let text = format!("{ONE_CASE}{ONE_CASE}");
    let got: Vec<String> = check(&parse_log(&table(&text)), &roster, today).iter().map(ToString::to_string).collect();
    assert_eq!(got, vec!["field_invalid id C1".to_string()]);
}

#[test]
fn roster_is_handles_only() {
    assert!(parse_roster(&table(r#"moderators = ["a@example.invalid"]"#), "r").is_err());
    assert!(parse_roster(&table("moderators = [\"x\"]\nemails = []"), "r").is_err());
    assert!(parse_roster(&table(r#"moderators = ["x", "x"]"#), "r").is_err());
    assert_eq!(parse_roster(&table("moderators = []"), "r").unwrap().len(), 0);
}

#[test]
fn retention_boundary_and_month_ends() {
    // Hand-labelled: overdue only when --today is after the filing date plus 24 months.
    assert_eq!(RETENTION_MONTHS, 24);
    let d = |s| Date::parse(s).unwrap();
    assert_eq!(d("2024-12-01").add_months(24), d("2026-12-01"));
    assert_eq!(d("2024-02-29").add_months(24), d("2026-02-28"));
    assert_eq!(d("2025-11-30").add_months(3), d("2026-02-28"));
    let roster = parse_roster(&table(r#"moderators = ["mod-ash"]"#), "r").unwrap();
    let log = parse_log(&table(&ONE_CASE.replace("2027-01-04", "2025-01-04")));
    assert!(check(&log, &roster, d("2027-01-04")).is_empty());
    let got: Vec<String> = check(&log, &roster, d("2027-01-05")).iter().map(ToString::to_string).collect();
    assert_eq!(got, vec!["retention_overdue C1".to_string()]);
}

#[test]
fn dates_and_quarters() {
    assert!(Stamp::parse("2027-01-04T09:00:00Z").is_some());
    for bad in ["2027-01-04T09:00:00", "2027-01-04T09:00:00+00:00", "2027-01-04 09:00:00Z", "2027-02-29T00:00:00Z"] {
        assert!(Stamp::parse(bad).is_none(), "{bad}");
    }
    let a = Stamp::parse("2027-01-20T22:15:00Z").unwrap();
    let b = Stamp::parse("2027-01-23T22:15:00Z").unwrap();
    assert_eq!(b.secs - a.secs, 72 * 3600);
    // 1970-01-01 is day zero; 2000-03-01 is day 11017 (hand-computed).
    assert_eq!(Stamp::parse("1970-01-01T00:00:00Z").unwrap().secs, 0);
    assert_eq!(Stamp::parse("2000-03-01T00:00:00Z").unwrap().secs, 11_017 * 86_400);
    assert_eq!(Quarter::parse("2027Q1"), Some(Quarter { year: 2027, q: 1 }));
    assert!(Quarter::parse("2027Q0").is_none() && Quarter::parse("27Q1").is_none());
    assert_eq!(Date::parse("2027-12-31").unwrap().quarter().to_string(), "2027Q4");
}

#[test]
fn empty_quarter_has_zero_counts() {
    let log = parse_log(&table(ONE_CASE));
    let v = stats(&log, Quarter::parse("2026Q4").unwrap(), 72);
    let bytes = fid_canon::to_canonical_bytes(&v).unwrap();
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        r#"{"ack_within_target":{"count":0,"target_hours":72,"total":0},"by_outcome":{"ban":0,"no_action":0,"removal":0,"warning":0},"quarter":"2026Q4","total":0}"#
    );
}

#[test]
fn repository_roster_parses() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../community/moderation/roster.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(parse_roster(&table(&text), "roster.toml").is_ok());
}
