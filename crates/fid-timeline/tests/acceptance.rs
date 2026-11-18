//! Acceptance criteria of spec node ECO.COMM.TIMELINE (AC-1 .. AC-5) and its requirements.
//! Expected clocks and actions come from `fixtures/timeline/expected.tsv` and
//! `fixtures/timeline/timestamps.tsv`, written by the independent Python reference
//! `fixtures/timeline/make_cases.py` (error cases are hand-labelled there). Offline: the
//! tests read only fixture files.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_timeline::{
    evaluate, evaluate_inputs, parse_config, parse_events, parse_timestamp, Action, Config, ErrorCode, Kind,
    MAX_INPUT_BYTES, SECONDS_PER_DAY,
};
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/timeline")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

struct Row {
    case: String,
    events: String,
    config: Option<String>,
    now: String,
    /// `Ok((clock_seconds, clock_days, action))` or `Err(code)`.
    expect: Result<(i64, i64, String), String>,
}

fn rows() -> Vec<Row> {
    let text = String::from_utf8(read("expected.tsv")).unwrap();
    let rows: Vec<Row> = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            assert_eq!(f.len(), 7, "bad row {l:?}");
            let expect = match f[6].strip_prefix("error:") {
                Some(code) => Err(code.to_string()),
                None => Ok((f[4].parse().unwrap(), f[5].parse().unwrap(), f[6].to_string())),
            };
            Row {
                case: f[0].into(),
                events: f[1].into(),
                config: (f[2] != "-").then(|| f[2].to_string()),
                now: f[3].into(),
                expect,
            }
        })
        .collect();
    assert!(rows.len() >= 40, "expected.tsv has only {} rows", rows.len());
    rows
}

fn row(case: &str) -> Row {
    rows().into_iter().find(|r| r.case == case).unwrap_or_else(|| panic!("no case {case}"))
}

fn run_lib(r: &Row) -> Result<fid_timeline::Evaluation, fid_timeline::TimelineError> {
    let cfg = r.config.as_ref().map(|c| String::from_utf8(read(c)).unwrap());
    evaluate_inputs(&read(&r.events), cfg.as_deref(), &r.now)
}

/// Runs the binary with `--json`; returns (exit code, stdout).
fn run_cli(events: &str, config: Option<&str>, now: &str) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-timeline"));
    cmd.arg("--events").arg(fixtures().join(events)).arg("--now").arg(now).arg("--json");
    if let Some(c) = config {
        cmd.arg("--config").arg(fixtures().join(c));
    }
    let out = cmd.output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap().trim_end().to_string())
}

#[test]
fn every_reference_case_matches_library_and_cli() {
    for r in rows() {
        let got = run_lib(&r);
        let (code, stdout) = run_cli(&r.events, r.config.as_deref(), &r.now);
        match (&r.expect, &got) {
            (Ok((secs, days, action)), Ok(ev)) => {
                assert_eq!(ev.clock_seconds, *secs, "{}", r.case);
                assert_eq!(ev.clock_days, *days, "{}", r.case);
                assert_eq!(ev.action.as_str(), action, "{}", r.case);
                assert_eq!(code, 0, "{}", r.case);
                assert_eq!(stdout, format!("{{\"clock_days\":{days},\"action\":\"{action}\"}}"), "{}", r.case);
            }
            (Err(want), Err(e)) => {
                assert_eq!(e.code.as_str(), want, "{}: {e}", r.case);
                assert_eq!(code, 1, "{}", r.case);
                assert_eq!(stdout, format!("{{\"error\":\"{want}\"}}"), "{}", r.case);
                assert!(!stdout.contains("action"), "{}", r.case);
            }
            _ => panic!("{}: expected {:?}, got {:?}", r.case, r.expect, got),
        }
    }
}

#[test]
fn ac1_active_timeline_clock_is_eleven_days() {
    let (code, out) = run_cli("active.json", None, "2026-03-14T00:00:00Z");
    assert_eq!(code, 0);
    assert_eq!(out, r#"{"clock_days":11,"action":"none"}"#);
}

#[test]
fn ac2_stalled_remind_then_idempotent_then_propose_close() {
    assert_eq!(run_lib(&row("ac2-day15")).unwrap().action, Action::Remind);
    assert_eq!(run_lib(&row("ac2-reminded-day20")).unwrap().action, Action::None);
    assert_eq!(run_lib(&row("ac2-reminded-day31")).unwrap().action, Action::ProposeClose);
}

#[test]
fn ac3_bot_comments_do_not_move_the_clock() {
    let ev = run_lib(&row("ac3")).unwrap();
    assert_eq!((ev.clock_days, ev.action), (15, Action::Remind));
    // Same verdict as the bot-free timeline.
    assert_eq!(ev, run_lib(&row("ac2-day15")).unwrap());
}

#[test]
fn req002_bot_list_is_what_keeps_bot_events_out() {
    let events = parse_events(&read("bots-noise.json")).unwrap();
    let now = parse_timestamp("2026-04-01T00:00:00Z").unwrap();
    let cfg = parse_config(&String::from_utf8(read("bots-only.toml")).unwrap()).unwrap();
    let with_bots = evaluate(&events, &cfg, now).unwrap();
    assert_eq!((with_bots.clock_seconds, with_bots.action), (7 * SECONDS_PER_DAY, Action::None));
    // Without the bot list, the bot's label removal and change request would count.
    let without = evaluate(&events, &Config { bots: vec![], ..cfg }, now).unwrap();
    assert_ne!(without.clock_seconds, with_bots.clock_seconds);
}

#[test]
fn ac4_maintainer_blocked_clock_is_zero() {
    let ev = run_lib(&row("ac4")).unwrap();
    assert_eq!((ev.clock_seconds, ev.action, ev.author_is_blocker), (0, Action::None, false));
}

#[test]
fn ac5_malformed_json_exits_1_with_timeline_input_and_no_action() {
    let (code, out) = run_cli("malformed.json", None, "2026-03-03T00:00:00Z");
    assert_eq!(code, 1);
    assert_eq!(out, r#"{"error":"TIMELINE-INPUT"}"#);
    let err = Command::new(env!("CARGO_BIN_EXE_fid-timeline"))
        .arg("--events")
        .arg(fixtures().join("malformed.json"))
        .arg("--now")
        .arg("2026-03-03T00:00:00Z")
        .output()
        .unwrap();
    assert_eq!(err.status.code(), Some(1));
    assert!(String::from_utf8(err.stderr).unwrap().starts_with("TIMELINE-INPUT"));
    assert!(err.stdout.is_empty());
}

#[test]
fn cli_usage_errors_exit_2() {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-timeline")).arg("--events").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = Command::new(env!("CARGO_BIN_EXE_fid-timeline"))
        .args(["--events", "x.json", "--bogus"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn missing_files_are_typed_errors() {
    let (code, out) = run_cli("does-not-exist.json", None, "2026-03-03T00:00:00Z");
    assert_eq!((code, out.as_str()), (1, r#"{"error":"TIMELINE-INPUT"}"#));
    let (code, out) = run_cli("stalled.json", Some("does-not-exist.toml"), "2026-03-03T00:00:00Z");
    assert_eq!((code, out.as_str()), (1, r#"{"error":"TIMELINE-CONFIG"}"#));
}

#[test]
fn timestamps_match_the_python_datetime_reference() {
    let text = String::from_utf8(read("timestamps.tsv")).unwrap();
    let mut n = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let (s, want) = line.split_once('\t').unwrap();
        let got = parse_timestamp(s);
        if want == "invalid" {
            let e = got.expect_err(s);
            assert_eq!(e.code, ErrorCode::Input, "{s:?}");
        } else {
            assert_eq!(got.unwrap(), want.parse::<i64>().unwrap(), "{s:?}");
        }
        n += 1;
    }
    assert!(n >= 30);
}

#[test]
fn d0_same_result_every_run_and_independent_of_file_order_for_distinct_times() {
    let cfg = parse_config(&String::from_utf8(read("config.toml")).unwrap()).unwrap();
    let now = parse_timestamp("2026-03-14T00:00:00Z").unwrap();
    let events = parse_events(&read("active.json")).unwrap();
    let a = evaluate(&events, &cfg, now).unwrap();
    assert_eq!(a, evaluate(&events, &cfg, now).unwrap());
    let mut rev = events.clone();
    rev.reverse();
    // active.json's only tie is `opened` with the first review, whose order does not matter.
    assert_eq!(a, evaluate(&rev, &cfg, now).unwrap());
}

#[test]
fn config_toml_subset_and_json_agree() {
    let toml = parse_config(
        "# c\r\nauthor = 'alice' # x\r\nmaintainers = [\r\n \"bob\", # one\r\n \"carol\",\r\n]\r\nbots=[]\r\nremind_days = +7\r\nclose_days = 9\r\n",
    )
    .unwrap();
    let json = parse_config(
        r#" {"author": "alice", "maintainers": ["bob", "carol"], "bots": [], "remind_days": 7, "close_days": 9}"#,
    )
    .unwrap();
    assert_eq!(toml, json);
    assert_eq!(toml.maintainers, Some(vec!["bob".to_string(), "carol".to_string()]));
    assert_eq!(parse_config("").unwrap(), Config::default());
    assert_eq!(parse_config("author = \"a\\\"b\"").unwrap().author.as_deref(), Some("a\"b"));
}

#[test]
fn config_rejects_what_it_does_not_understand() {
    for bad in [
        "author = \"alice\"\nauthor = \"bob\"",
        "remind_days = 14.5",
        "remind_days = 36501",
        "close_days = -3",
        "remind_days = 1234567890123456789",
        "author = \"\"",
        "bots = [\"\"]",
        "bots = [1, 2]",
        "bots = \"x\"",
        "author = \"alice",
        "author = \"a\\qb\"",
        "author = \"alice\" extra",
        "author: \"alice\"",
        "[[x]]",
        "a.b = 1",
        "flag = true",
        "{\"remind_days\": 1.5}",
        "{\"remind_days\": 14, \"remind_days\": 15}",
        "{\"bots\": [1]}",
        "{\"author\": null}",
    ] {
        let e = parse_config(bad).expect_err(bad);
        assert_eq!(e.code, ErrorCode::Config, "{bad:?}");
    }
    assert!(parse_config(&" ".repeat(fid_timeline::MAX_CONFIG_BYTES + 1)).is_err());
}

#[test]
fn events_are_bounded_and_strict() {
    assert_eq!(parse_events(&vec![b' '; MAX_INPUT_BYTES + 1]).unwrap_err().code, ErrorCode::Input);
    for bad in [
        &b"[1]"[..],
        &b"[{\"at\": 5, \"actor\": \"a\", \"kind\": \"opened\"}]"[..],
        &b"[{\"at\": \"2026-03-02T00:00:00Z\", \"actor\": \" \", \"kind\": \"opened\"}]"[..],
        &b"[{\"at\": \"2026-03-02T00:00:00Z\", \"actor\": \"a\", \"kind\": \"opened\", \"label\": \"x\"}]"[..],
        &b"[{\"at\": \"2026-03-02T00:00:00Z\", \"actor\": \"a\"}]"[..],
        &b"\xff"[..],
    ] {
        assert_eq!(parse_events(bad).unwrap_err().code, ErrorCode::Input);
    }
    assert!(parse_events(b"[]").unwrap().is_empty());
    for k in Kind::ALL {
        assert_eq!(Kind::parse(k.as_str()), Some(k));
    }
}

#[test]
fn empty_timeline_with_configured_author_is_none() {
    let cfg = Config { author: Some("alice".into()), ..Config::default() };
    let ev = evaluate(&[], &cfg, 0).unwrap();
    assert_eq!((ev.clock_seconds, ev.action), (0, Action::None));
}
