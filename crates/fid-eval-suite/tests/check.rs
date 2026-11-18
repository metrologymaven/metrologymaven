#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Acceptance tests of spec node AGENT.EVAL.CHECK (`fid-eval suite check --catalog --jobs`).
//! Expected findings come from fixtures/agent/eval/make_check_fixtures.py (a stdlib
//! reference of the rules, guarded there by hand-labelled assertions), never from this crate.
//! Cases built here from temporary copies are hand-labelled from the spec's rules.

use fid_canon::{parse, CValue};
use fid_eval_suite::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/agent/eval");

fn fix(rel: &str) -> PathBuf {
    Path::new(FIX).join(rel)
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("expected object, got {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("expected array, got {other:?}"),
    }
}

fn opt_str(v: &CValue) -> String {
    match v {
        CValue::Str(s) => s.clone(),
        CValue::Null => String::new(),
        other => panic!("expected string or null, got {other:?}"),
    }
}

fn cli(args: &[&str], suite: &Path, catalog: &Path, jobs: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-eval"))
        .args(["suite", "check", "--suite"])
        .arg(suite)
        .arg("--catalog")
        .arg(catalog)
        .arg("--jobs")
        .arg(jobs)
        .args(args)
        .output()
        .unwrap()
}

/// (rule, task, file) triples, sorted; missing task or file is "".
type Triple = (String, String, String);

fn triples_of(findings: &[CheckFinding]) -> Vec<Triple> {
    let mut v: Vec<Triple> = findings
        .iter()
        .map(|f| (f.rule.as_str().to_string(), f.task.clone().unwrap_or_default(), f.file.clone().unwrap_or_default()))
        .collect();
    v.sort();
    v
}

struct Case {
    name: String,
    suite: PathBuf,
    catalog: PathBuf,
    jobs: PathBuf,
    exit: i32,
    triples: Vec<Triple>,
    /// (rule, subject) for findings whose detail must name a subject (workflow, job, ...).
    subjects: Vec<(String, String)>,
}

fn cases() -> Vec<Case> {
    let root = parse(&std::fs::read(fix("check-expected.json")).unwrap()).unwrap();
    arr(&obj(&root)["cases"])
        .iter()
        .map(|c| {
            let c = obj(c);
            let mut triples = Vec::new();
            let mut subjects = Vec::new();
            for f in arr(&c["findings"]) {
                let f = obj(f);
                let rule = opt_str(&f["rule"]);
                triples.push((rule.clone(), opt_str(&f["task"]), opt_str(&f["file"])));
                let subject = opt_str(&f["subject"]);
                if !subject.is_empty() {
                    subjects.push((rule, subject));
                }
            }
            triples.sort();
            let exit = match &c["exit"] {
                CValue::Num(n) => *n as i32,
                other => panic!("exit: {other:?}"),
            };
            Case {
                name: opt_str(&c["name"]),
                suite: fix(&opt_str(&c["suite"])),
                catalog: fix(&opt_str(&c["catalog"])),
                jobs: fix(&opt_str(&c["jobs"])),
                exit,
                triples,
                subjects,
            }
        })
        .collect()
}

fn case(name: &str) -> Case {
    cases().into_iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no case {name}"))
}

/// Library and CLI agree with the reference findings of one case.
fn assert_case(c: &Case) {
    let report = check_suite(&c.suite, &c.catalog, &c.jobs);
    assert_eq!(triples_of(&report.findings), c.triples, "case {}: {:#?}", c.name, report.findings);
    for (rule, subject) in &c.subjects {
        assert!(
            report.findings.iter().any(|f| f.rule.as_str() == rule && f.detail.contains(subject.as_str())),
            "case {}: no {rule} finding names {subject}: {:#?}",
            c.name,
            report.findings
        );
    }

    let out = cli(&["--json"], &c.suite, &c.catalog, &c.jobs);
    assert_eq!(out.status.code(), Some(c.exit), "case {}: stderr {}", c.name, String::from_utf8_lossy(&out.stderr));
    let v = parse(out.stdout.trim_ascii_end()).unwrap();
    let mut got: Vec<Triple> = arr(&obj(&v)["findings"])
        .iter()
        .map(|f| {
            let f = obj(f);
            (opt_str(&f["rule"]), opt_str(&f["task"]), opt_str(&f["file"]))
        })
        .collect();
    got.sort();
    assert_eq!(got, c.triples, "case {} (CLI)", c.name);
}

/// AC-1: suite-mini with its catalog index and job packages passes.
#[test]
fn ac1_suite_mini_passes() {
    let c = case("ac1-mini");
    assert_eq!(c.exit, 0);
    assert_case(&c);
    let out = cli(&[], &c.suite, &c.catalog, &c.jobs);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("ok: 2 tasks"));
}

/// AC-2: a read root holding a copy of the task's oracle fails with oracle_leak; the copy
/// outside every granted root is not reported.
#[test]
fn ac2_oracle_copy_in_read_root() {
    let c = case("ac2-leak");
    assert!(c.triples.iter().any(|t| t.0 == "oracle_leak"));
    assert_case(&c);
}

/// AC-3: a catalog workflow no task names fails with uncovered_workflow.
#[test]
fn ac3_uncovered_workflow() {
    let c = case("ac3-uncovered");
    assert!(c.triples.iter().any(|t| t.0 == "uncovered_workflow"));
    assert_case(&c);
}

/// AC-4: "expected: CONFORMS" next to the task's own characteristic fails with oracle_in_prompt.
#[test]
fn ac4_outcome_in_prompt() {
    let c = case("ac4-prompt");
    assert!(c.triples.iter().any(|t| t.0 == "oracle_in_prompt"));
    assert_case(&c);
}

/// AC-5 / REQ-005: a suite with many different faults reports every one of them.
#[test]
fn ac5_all_faults_reported() {
    let c = case("ac5-faults");
    let rules: std::collections::BTreeSet<&str> = c.triples.iter().map(|t| t.0.as_str()).collect();
    assert!(rules.len() >= 3, "{rules:?}");
    assert_case(&c);
    // Text mode: every finding on stderr, exit 1.
    let out = cli(&[], &c.suite, &c.catalog, &c.jobs);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(err.lines().filter(|l| l.starts_with("error: ")).count(), c.triples.len(), "{err}");
}

/// Every case of the reference file, so new cases are not forgotten.
#[test]
fn all_reference_cases() {
    for c in cases() {
        assert_case(&c);
    }
}

/// A fresh copy of a fixture directory in the temp directory.
fn copy_dir(src: &Path, tag: &str) -> PathBuf {
    let dst = std::env::temp_dir().join(format!("fid-eval-check-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dst);
    let mut stack = vec![(src.to_path_buf(), dst.clone())];
    while let Some((s, d)) = stack.pop() {
        std::fs::create_dir_all(&d).unwrap();
        for e in std::fs::read_dir(&s).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                stack.push((e.path(), d.join(e.file_name())));
            } else {
                std::fs::copy(e.path(), d.join(e.file_name())).unwrap();
            }
        }
    }
    dst
}

/// REQ-001 (hand-labelled): a start-state archive that no longer matches its recorded
/// digest fails with digest_mismatch naming the task.
#[test]
fn req1_start_state_digest_mismatch() {
    let suite = copy_dir(&fix("suite-mini"), "start");
    let tar = suite.join("tasks/mini-probe-01/start.tar");
    let mut b = std::fs::read(&tar).unwrap();
    let last = b.len() - 1;
    b[last] ^= 1;
    std::fs::write(&tar, b).unwrap();
    let report = check_suite(&suite, &fix("catalog-index-mini.json"), &fix("jobs"));
    let t = triples_of(&report.findings);
    assert_eq!(
        t,
        vec![("digest_mismatch".to_string(), "mini-probe-01".to_string(), "tasks/mini-probe-01/task.json".to_string())]
    );
    let _ = std::fs::remove_dir_all(&suite);
}

/// REQ-001 (hand-labelled): a package whose golden journal changed is reported, and the
/// task citing the old digest still matches the package's declared digest.
#[test]
fn req1_package_journal_mismatch() {
    let jobs = copy_dir(&fix("jobs"), "jobs");
    std::fs::write(jobs.join("acc-01/golden-journal.txt"), b"edited\n").unwrap();
    let report = check_suite(&fix("suite-mini"), &fix("catalog-index-mini.json"), &jobs);
    assert_eq!(
        triples_of(&report.findings),
        vec![("digest_mismatch".to_string(), String::new(), "acc-01/package.json".to_string())]
    );
    let _ = std::fs::remove_dir_all(&jobs);
}

/// Unreadable inputs pass through as parse_error, and the CLI usage rules hold.
#[test]
fn parse_errors_and_usage() {
    let report = check_suite(&fix("suite-mini"), &fix("no-such-catalog.json"), &fix("jobs"));
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].rule, Rule::ParseError);

    let bin = env!("CARGO_BIN_EXE_fid-eval");
    let mini = fix("suite-mini");
    let only_catalog = Command::new(bin)
        .args(["suite", "check", "--suite"])
        .arg(&mini)
        .arg("--catalog")
        .arg(fix("catalog-index-mini.json"))
        .output()
        .unwrap();
    assert_eq!(only_catalog.status.code(), Some(2));
    let on_digest = Command::new(bin)
        .args(["suite", "digest", "--suite"])
        .arg(&mini)
        .arg("--jobs")
        .arg(fix("jobs"))
        .output()
        .unwrap();
    assert_eq!(on_digest.status.code(), Some(2));
}

/// Hand-labelled prompt scanning rules.
#[test]
fn prompt_scanning_rules() {
    assert!(has_token("check PL1-FLAT: expected CONFORMS.", "CONFORMS"));
    assert!(!has_token("check PL1-FLAT: NONCONFORMS", "CONFORMS"));
    assert!(!has_token("plane PL1-FLATNESS", "PL1-FLAT"));
    assert!(has_token("(PL1-FLAT)", "PL1-FLAT"));

    // 1.25e-5 m with tolerance 1e-7 m
    let (a, tol) = (1.25e-5, 1e-7);
    assert!(value_in_prompt("measured 12.5 um", a, "m", tol));
    assert!(value_in_prompt("measured 0.0125mm.", a, "m", tol));
    assert!(value_in_prompt("measured 1.25e-5 m", a, "m", tol));
    assert!(value_in_prompt("measured 12.5 \u{b5}m", a, "m", tol));
    assert!(!value_in_prompt("measured 12.5 min", a, "m", tol));
    assert!(!value_in_prompt("measured 12.5", a, "m", tol));
    assert!(!value_in_prompt("measured 13 um", a, "m", tol));
    assert!(!value_in_prompt("part A12.5 um", a, "m", tol));
    // degrees for an angle in radians
    assert!(value_in_prompt("rotate by 90 deg", std::f64::consts::FRAC_PI_2, "rad", 1e-9));
    // dimensionless values are never matched
    assert!(!value_in_prompt("count 3", 3.0, "1", 1e-9));
}
