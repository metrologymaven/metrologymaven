#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Acceptance tests of spec node AGENT.EVAL.CORPUS (the first evaluation task corpus in
//! `suite/`). Expected values come from fixtures/agent/eval/corpus/make_corpus.py (hashlib and a
//! stdlib reference geometry), never from this crate. The trace, licence-allowlist and tier
//! checks (AC-2, AC-3, AC-4) are tools/check_eval_corpus.py and its unit tests.

use fid_canon::{parse, CValue};
use fid_eval_suite::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn root(rel: &str) -> PathBuf {
    Path::new(ROOT).join(rel)
}

fn load_json(p: &Path) -> CValue {
    parse(&std::fs::read(p).unwrap()).unwrap()
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

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("expected string, got {other:?}"),
    }
}

fn expected() -> CValue {
    load_json(&root("fixtures/agent/eval/corpus/expected.json"))
}

/// AC-1 (library): the corpus loads fail-closed; its files, suite version, tiers, app tags
/// and oracle sizes equal the reference script's; no task is a red-team task.
#[test]
fn ac1_corpus_loads() {
    let suite = load_suite(&root("suite")).unwrap();
    let exp = expected();
    let exp = obj(&exp);
    assert_eq!(suite.name, "fid-eval-corpus");
    assert_eq!(suite.version.as_str(), st(&exp["suite_version"]));
    let want_files: Vec<(String, String)> =
        arr(&exp["files"]).iter().map(|p| (st(&arr(p)[0]).to_string(), st(&arr(p)[1]).to_string())).collect();
    let got_files: Vec<(String, String)> =
        suite.files.iter().map(|(p, d)| (p.clone(), d.as_str().to_string())).collect();
    assert_eq!(got_files, want_files);

    let want_tasks = arr(&exp["tasks"]);
    assert_eq!(suite.tasks.len(), want_tasks.len());
    for (t, w) in suite.tasks.iter().zip(want_tasks) {
        let w = obj(w);
        assert_eq!(t.task.id, st(&w["id"]));
        assert_eq!(t.task.tier.as_str(), st(&w["tier"]), "{}", t.task.id);
        let apps: Vec<&str> = t.task.apps.iter().map(App::as_str).collect();
        let want_apps: Vec<&str> = arr(&w["apps"]).iter().map(st).collect();
        assert_eq!(apps, want_apps, "{}", t.task.id);
        assert_eq!(CValue::Num(t.oracle.items.len() as f64), w["items"], "{}", t.task.id);
        assert_eq!(CValue::Num(t.oracle.forbidden.len() as f64), w["forbidden"], "{}", t.task.id);
        assert_eq!(t.task.family, None, "{}", t.task.id);
        assert!(t.task.predicates.is_empty());
        // REQ-AGENT.EVAL-002: every value is in SI units with a stated tolerance.
        for it in &t.oracle.items {
            if let Some(v) = &it.value {
                assert!(SI_UNITS.contains(&v.unit.as_str()), "{} {}", t.task.id, it.characteristic);
                assert!(it.tol.is_some(), "{} {}", t.task.id, it.characteristic);
            }
        }
        // REQ-AGENT.EVAL.CORPUS-003: every input is project-authored synthetic data.
        assert!(t.task.licences.iter().all(|l| l.spdx == "project-authored"), "{}", t.task.id);
    }
}

/// AC-4: each app of the corpus has a task in every tier.
#[test]
fn ac4_every_app_in_every_tier() {
    let suite = load_suite(&root("suite")).unwrap();
    let mut tiers: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for t in &suite.tasks {
        for a in &t.task.apps {
            tiers.entry(a.as_str()).or_default().insert(t.task.tier.as_str());
        }
    }
    assert_eq!(tiers.keys().copied().collect::<Vec<_>>(), vec!["inspect", "probe", "report"]);
    for (app, ts) in &tiers {
        assert_eq!(ts.len(), Tier::ALL.len(), "{app}: {ts:?}");
    }
}

/// REQ-AGENT.EVAL.CORPUS-004: the formula-derived wall thickness T1 is a length in metres on
/// the RESULTS_ONLY basis, as in the golden journal's `expr.evaluate_formula@1` entry.
#[test]
fn req004_formula_item() {
    let suite = load_suite(&root("suite")).unwrap();
    let t = suite.tasks.iter().find(|t| t.task.id == "report-derived-and-arm").unwrap();
    let item = t.oracle.items.iter().find(|i| i.characteristic == "T1").unwrap();
    assert_eq!(item.basis, Basis::ResultsOnly);
    assert_eq!(item.outcome, Outcome::Conforms);
    assert_eq!(item.value.as_ref().map(|v| v.unit.as_str()), Some("m"));
}

/// AC-1 (CLI): `fid-eval suite check` exits 0 on the corpus and counts its tasks.
#[test]
fn ac1_cli_suite_check() {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-eval"))
        .args(["suite", "check", "--suite"])
        .arg(root("suite"))
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let v = parse(out.stdout.trim_ascii_end()).unwrap();
    let m = obj(&v);
    let exp = expected();
    let n = arr(&obj(&exp)["tasks"]).len() as f64;
    assert!(n >= 9.0);
    assert_eq!(m["tasks"], CValue::Num(n));
    assert_eq!(m["families"], CValue::Obj(BTreeMap::new()));
    assert_eq!(m["predicates"], CValue::Num(0.0));
    assert_eq!(st(&m["suite_version"]), st(&obj(&exp)["suite_version"]));
}
