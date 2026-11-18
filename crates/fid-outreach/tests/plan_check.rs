//! Acceptance criteria of spec node OUTR.LAUNCH.PLAN.FMT (AC-1 .. AC-7) and error paths.
//! Expected findings are hand-labelled from the spec's rules (see the comments in each fixture
//! under `fixtures/outr/launch/plan/`), not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_outreach::launch_plan::{
    check_rules, dates_in, is_iso_date, parse_criteria, parse_review, Code, CriterionKind, Level, Phase,
};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::{Command, Output};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/launch/plan").join(rel)
}

/// `name/file` if the fixture has it, else the shared `ok/file`.
fn pick(name: &str, file: &str) -> String {
    let p = fix(name).join(file);
    let p = if p.exists() { p } else { fix("ok").join(file) };
    p.to_string_lossy().into_owned()
}

fn run(name: &str, extra: &[&str]) -> Output {
    let mut args = vec![
        "plan-check".to_string(),
        "--criteria".into(),
        pick(name, "criteria.toml"),
        "--apps".into(),
        pick(name, "apps.toml"),
        "--plan".into(),
        pick(name, "PLAN.md"),
        "--json".into(),
    ];
    if !extra.contains(&"--reviews") {
        args.push("--reviews".into());
        args.push(pick(name, "reviews"));
    }
    args.extend(extra.iter().map(|s| s.to_string()));
    Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(&args).output().unwrap()
}

/// (code, id, line) of every finding in a JSON report, in report order.
fn findings(o: &Output) -> Vec<(String, String, Option<u32>)> {
    let v = fid_canon::parse(&o.stdout).unwrap();
    let CValue::Obj(top) = v else { panic!("not an object") };
    let CValue::Arr(items) = &top["findings"] else { panic!("no findings array") };
    items
        .iter()
        .map(|f| {
            let CValue::Obj(f) = f else { panic!("finding not an object") };
            let s = |k: &str| match &f[k] {
                CValue::Str(s) => s.clone(),
                other => panic!("{k} is {other:?}"),
            };
            let line = f.get("line").map(|l| match l {
                CValue::Num(n) => *n as u32,
                other => panic!("line is {other:?}"),
            });
            (s("code"), s("id"), line)
        })
        .collect()
}

fn one(code: &str, id: &str) -> Vec<(String, String, Option<u32>)> {
    vec![(code.to_string(), id.to_string(), None)]
}

#[test]
fn ac1_ok_fixture_exits_0_with_canonical_json() {
    let o = run("ok", &[]);
    assert_eq!(o.status.code(), Some(0), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8(o.stdout).unwrap(), "{\"findings\":[],\"ok\":true}\n");
}

#[test]
fn ac1_ok_fixture_parses_every_kind() {
    let text = std::fs::read_to_string(fix("ok/criteria.toml")).unwrap();
    let (c, f) = parse_criteria(&text.parse().unwrap());
    assert!(f.is_empty(), "{f:?}");
    assert_eq!(c.milestones.len(), 6);
    let levels: Vec<Level> = c.milestones.iter().map(|m| m.level).collect();
    assert_eq!(levels, [Level::Alpha, Level::Beta, Level::One, Level::Alpha, Level::Beta, Level::One]);
    // A TOML date literal is accepted as a target date.
    assert_eq!(c.milestones[1].target_date.as_deref(), Some("2099-04-01"));
    assert_eq!(c.milestones[3].announced.as_ref().unwrap().release, "v0.1.0-alpha");
    let kinds: Vec<&CriterionKind> = c.milestones.iter().flat_map(|m| m.criteria.iter().map(|c| &c.kind)).collect();
    assert_eq!(kinds.len(), 10);
    assert!(kinds.contains(&&CriterionKind::ParityApp { app: "inspect".into(), min_pct: 50.5 }));
    assert!(kinds.contains(&&CriterionKind::PhaseDone { phase: Phase::P2, scope: vec!["SCAN".into(), "GDT".into()] }));
    assert_eq!(c.milestones[0].criteria[1].note.as_deref(), Some("free text, not evaluated"));
    let dates: Vec<String> = c.dates().into_iter().collect();
    assert_eq!(dates, ["2099-01-15", "2099-02-01", "2099-04-01"]);
}

#[test]
fn ac2_bad_kind_reports_unknown_kind() {
    let o = run("bad-kind", &[]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(findings(&o), one("unknown_kind", "RPT-A-09"));
}

#[test]
fn ac3_early_probe_1_0_reports_core_phase_missing() {
    let o = run("early-1.0", &[]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(findings(&o), one("core_phase_missing", "probe-1-0"));
}

#[test]
fn ac4_beta_unlabelled_reports_scope_label_missing() {
    let o = run("beta-unlabelled", &[]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(findings(&o), one("scope_label_missing", "report-beta"));
}

#[test]
fn ac5_stray_date_reports_file_and_line() {
    let o = run("stray-date", &[]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(findings(&o), vec![("stray_date".to_string(), "2027-03-01".to_string(), Some(6))]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("stray-date"), "finding names the plan file");
}

#[test]
fn ac6_changed_announced_criterion_needs_a_review_entry() {
    let base = pick("changed-announced", "base.toml");
    let none = pick("changed-announced", "reviews-none");
    let o = run("changed-announced", &["--base", &base, "--reviews", &none]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(findings(&o), one("announced_criteria_changed", "RPT-A-02"));

    let reviews = pick("changed-announced", "reviews");
    let o = run("changed-announced", &["--base", &base, "--reviews", &reviews]);
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stdout));
    assert!(findings(&o).is_empty());

    // Without --base the announced rule does not run.
    let o = run("changed-announced", &["--reviews", &none]);
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn ac6_added_and_removed_criteria_count_as_changes() {
    let base: toml::Table = "version = 1\n[[milestone]]\nid = \"report-alpha\"\nannounced = { date = \"2099-01-20\", release = \"r\" }\n[[milestone.criterion]]\nid = \"OLD-01\"\nkind = \"signoff\"\nrole = \"x\"\n".parse().unwrap();
    let new: toml::Table = "version = 1\n[[milestone]]\nid = \"report-alpha\"\nannounced = { date = \"2099-01-20\", release = \"r\" }\n[[milestone.criterion]]\nid = \"NEW-01\"\nkind = \"signoff\"\nrole = \"x\"\n".parse().unwrap();
    let f = fid_outreach::launch_plan::check_announced(&base, &new, &[]);
    let ids: Vec<&str> = f.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids, ["NEW-01", "OLD-01"]);
    assert!(f.iter().all(|f| f.code == Code::AnnouncedCriteriaChanged));
    // A review entry for another milestone does not count.
    let other = parse_review("---\nmilestone: other-ms\nannounced: 2099-01-20\nchanges: [NEW-01, OLD-01]\n---\n", "r.md")
        .unwrap();
    assert_eq!(fid_outreach::launch_plan::check_announced(&base, &new, &[other]).len(), 2);
}

#[test]
fn ac7_dup_id_reports_duplicate_id() {
    let o = run("dup-id", &[]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(findings(&o), one("duplicate_id", "RPT-A-01"));
}

#[test]
fn exit_2_on_parse_error_and_missing_input() {
    let o = run("parse-error", &[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stdout).contains("\"code\":\"InputParse\""));

    let missing = fix("ok/no-such-dir").to_string_lossy().into_owned();
    let o = run("ok", &["--reviews", &missing]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stdout).contains("\"code\":\"InputUnreadable\""));

    let o = Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(["plan-check", "--json"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
}

fn check_str(text: &str) -> BTreeSet<(Code, String)> {
    let (c, mut f) = parse_criteria(&text.parse().unwrap());
    let apps: BTreeSet<String> = ["report", "inspect", "probe", "network", "extra"].iter().map(|s| s.to_string()).collect();
    f.extend(check_rules(&c, &apps));
    f.into_iter().map(|f| (f.code, f.id)).collect()
}

fn set(items: &[(Code, &str)]) -> BTreeSet<(Code, String)> {
    items.iter().map(|(c, s)| (*c, s.to_string())).collect()
}

#[test]
fn bad_ids_fields_and_apps_are_reported() {
    let text = r#"
version = 2
extra_top = 1

[[milestone]]
id = "Bad_Milestone"
app = "nosuch"
level = "gamma"

[[milestone]]
id = "net-1-0"
app = "network"
level = "1.0"
target_date = "2099-02-30"

[[milestone.criterion]]
id = "x1"
kind = "phase_done"
phase = "P2"
scope = ["NET"]

[[milestone.criterion]]
id = "NET-02"
kind = "parity"
app = "ghost"
min_pct = 50

[[milestone.criterion]]
id = "NET-03"
kind = "parity"
app = "network"
min_pct = 101

[[milestone.criterion]]
id = "NET-04"
kind = "node_status"
node = "NET"
in = ["done"]
colour = "red"

[[milestone]]
id = "extra-1-0"
app = "extra"
level = "1.0"
"#;
    let got = check_str(text);
    let want = set(&[
        (Code::BadField, "file"),          // version 2 and unknown top-level key
        (Code::BadField, "Bad_Milestone"), // level gamma
        (Code::BadId, "Bad_Milestone"),
        (Code::BadField, "net-1-0"), // 2099-02-30 is not a date
        (Code::BadId, "x1"),
        (Code::CorePhaseMissing, "net-1-0"), // network needs P3; x1 is P2
        (Code::UnknownApp, "NET-02"),
        (Code::BadField, "NET-03"), // min_pct out of range
        (Code::BadField, "NET-04"), // status `done` and field `colour`
        (Code::CorePhaseMissing, "extra-1-0"), // no rule-table entry
    ]);
    assert_eq!(got, want);
}

#[test]
fn node_status_and_parity_forms() {
    let ok = "version = 1\n[[milestone]]\nid = \"inspect-beta\"\napp = \"inspect\"\nlevel = \"beta\"\nscope_label = \"s\"\n[[milestone.criterion]]\nid = \"INS-01\"\nkind = \"parity\"\nseeds = [\"S1\"]\n";
    assert!(check_str(ok).is_empty());
    let both = ok.replace("seeds = [\"S1\"]", "seeds = [\"S1\"]\napp = \"inspect\"\nmin_pct = 5");
    assert_eq!(check_str(&both), set(&[(Code::BadField, "INS-01")]));
}

#[test]
fn date_scanning() {
    assert!(is_iso_date("2096-02-29"));
    assert!(!is_iso_date("2100-02-29"));
    assert!(!is_iso_date("2099-00-10"));
    let found = dates_in("a 2099-01-02b\n\nx12099-01-02 2099-01-023 (2099-12-31)\n");
    assert_eq!(found, [(1, "2099-01-02".to_string()), (3, "2099-12-31".to_string())]);
}
