#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Acceptance tests of spec node AGENT.REDTEAM.PRED: the violation predicate schema and the
//! suite-wide payload marker rules of `fid-eval suite check`. Expected verdicts come from
//! fixtures/agent/redteam/pred/make_pred.py (hand-labelled, cross-checked there by an
//! independent marker scan and by the shared reference evaluator), never from this crate.
//! Predicate evaluation on runs (AC-4) is checked by that reference evaluator until
//! AGENT.HARNESS.SAFETY exists; here the runs are checked for coverage and their predicates
//! for validity against the schema.

use fid_canon::{parse, CValue};
use fid_eval_suite::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const FIX: &str = "fixtures/agent/redteam/pred";

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

/// (suite dir relative to FIX, expected ok, expected (code, file, field) errors).
fn expected() -> Vec<(String, bool, Vec<(String, String, String)>)> {
    let v = load_json(&root(&format!("{FIX}/expected.json")));
    arr(&obj(&v)["suites"])
        .iter()
        .map(|s| {
            let s = obj(s);
            let errs = arr(&s["errors"])
                .iter()
                .map(|e| {
                    let e = arr(e);
                    (st(&e[0]).to_string(), st(&e[1]).to_string(), st(&e[2]).to_string())
                })
                .collect();
            (st(&s["suite"]).to_string(), s["ok"] == CValue::Bool(true), errs)
        })
        .collect()
}

fn fid_eval(args: &[&str]) -> (i32, CValue) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-eval")).args(args).output().unwrap();
    let v = parse(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
    (out.status.code().unwrap(), v)
}

/// Every labelled suite gets exactly its labelled verdict and errors from `load_suite`, the
/// library behind `fid-eval suite check`.
#[test]
fn suites_get_their_labelled_verdicts() {
    let cases = expected();
    assert!(cases.len() >= 8, "{} suites", cases.len());
    for (dir, ok, want) in cases {
        let got = load_suite(&root(&format!("{FIX}/{dir}")));
        match (ok, got) {
            (true, Ok(_)) => assert!(want.is_empty()),
            (false, Err(errs)) => {
                let mut got: Vec<(String, String, String)> =
                    errs.iter().map(|e| (e.code.as_str().to_string(), e.file.clone(), e.field.clone())).collect();
                got.sort();
                assert_eq!(got, want, "{dir}: {errs:?}");
            }
            (true, Err(errs)) => panic!("{dir}: expected to pass, got {errs:?}"),
            (false, Ok(_)) => panic!("{dir}: expected to fail with {want:?}"),
        }
    }
}

/// AC-1: the fixture injection task with one `entry_with_marker` predicate passes
/// `fid-eval suite check`, and its marker counts once.
#[test]
fn ac1_inject_name_passes_suite_check() {
    let dir = root(&format!("{FIX}/inject-name"));
    let suite = load_suite(&dir).unwrap();
    assert_eq!(suite.tasks.len(), 1);
    let t = &suite.tasks[0].task;
    assert_eq!(t.id, "inject-name");
    let markers: Vec<&Predicate> =
        t.predicates.iter().filter(|p| matches!(p.kind, PredicateKind::EntryWithMarker { .. })).collect();
    assert_eq!(markers.len(), 1);
    let (code, v) = fid_eval(&["suite", "check", "--suite", dir.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0, "{v:?}");
    let m = obj(&v);
    assert_eq!(m["markers"], CValue::Num(1.0));
    assert_eq!(m["suite_version"], CValue::Str(suite.version.as_str().to_string()));
}

/// The CLI error of a failing suite: exit 1 and the labelled codes in `--json`, each on the
/// task file that names the task.
fn cli_errors(dir: &str) -> Vec<(String, String)> {
    let path = root(&format!("{FIX}/{dir}"));
    let (code, v) = fid_eval(&["suite", "check", "--suite", path.to_str().unwrap(), "--json"]);
    assert_eq!(code, 1, "{dir}: {v:?}");
    let mut out: Vec<(String, String)> = arr(&obj(&v)["errors"])
        .iter()
        .map(|e| (st(&obj(e)["code"]).to_string(), st(&obj(e)["file"]).to_string()))
        .collect();
    out.sort();
    out
}

/// AC-2 (error path): two tasks reusing marker `FIDRT-000...` fail with `duplicate_marker`.
#[test]
fn ac2_duplicate_marker() {
    let marker = format!("FIDRT-{}", "0".repeat(26));
    let suite_dir = root(&format!("{FIX}/suites/dup-marker"));
    for id in ["dup-a", "dup-b"] {
        let t = read_task(&suite_dir.join("tasks").join(id).join("task.json")).unwrap();
        assert!(t.predicates.iter().any(|p| p.kind == PredicateKind::EntryWithMarker { marker: marker.clone() }));
    }
    assert_eq!(
        cli_errors("suites/dup-marker"),
        vec![
            ("duplicate_marker".to_string(), "tasks/dup-a/task.json".to_string()),
            ("duplicate_marker".to_string(), "tasks/dup-b/task.json".to_string()),
        ]
    );
    // A marker of one task inside another task's payload is a shared marker too.
    assert_eq!(
        cli_errors("suites/shared-payload"),
        vec![("duplicate_marker".to_string(), "tasks/share-a/task.json".to_string())]
    );
}

/// AC-3 (error path): a marker absent from the task's payload files fails naming the task,
/// also when the marker sits in a non-payload file or the task has no payloads.
#[test]
fn ac3_marker_not_in_payload_names_the_task() {
    assert_eq!(
        cli_errors("suites/marker-absent"),
        vec![("marker_not_in_payload".to_string(), "tasks/absent/task.json".to_string())]
    );
    assert_eq!(
        cli_errors("suites/no-payloads"),
        vec![("marker_not_in_payload".to_string(), "tasks/no-payloads/task.json".to_string())]
    );
    let errs = load_suite(&root(&format!("{FIX}/suites/marker-absent"))).unwrap_err();
    assert!(errs[0].detail.contains("task absent"), "{}", errs[0]);
}

/// REQ-004 and REQ-005: no predicate, an unknown kind, or a marker in the prompt (in any
/// letter case) fail naming the task.
#[test]
fn req4_req5_schema_and_prompt_errors() {
    for (dir, code, file) in [
        ("suites/missing-predicate", "missing_predicate", "tasks/no-predicate/task.json"),
        ("suites/unknown-predicate", "unknown_predicate", "tasks/unknown-kind/task.json"),
        ("suites/marker-in-prompt", "marker_in_prompt", "tasks/in-prompt/task.json"),
        ("suites/marker-in-prompt-lower", "marker_in_prompt", "tasks/in-prompt-lower/task.json"),
    ] {
        assert_eq!(cli_errors(dir), vec![(code.to_string(), file.to_string())], "{dir}");
    }
}

/// The published red-team suites still pass the marker rules.
#[test]
fn published_redteam_suites_pass_marker_rules() {
    for s in ["suite/redteam/inject", "suite/redteam/escal", "suite/redteam/fabric", "suite/redteam/shell"] {
        if let Err(errs) = load_suite(&root(s)) {
            panic!("{s}: {errs:?}");
        }
    }
}

/// AC-4: one positive and one negative run per predicate kind, every run file present, and
/// every run's predicate valid against the schema (wrapped in a fabrication task, the only
/// family where all six kinds are allowed).
#[test]
fn ac4_runs_cover_every_kind() {
    let labels = load_json(&root(&format!("{FIX}/runs/labels.json")));
    let mut seen: BTreeMap<String, BTreeSet<bool>> = BTreeMap::new();
    let base = std::fs::read_to_string(root(&format!("{FIX}/inject-name/tasks/inject-name/task.json"))).unwrap();
    let base = parse(base.as_bytes()).unwrap();
    for l in arr(&labels) {
        let l = obj(l);
        let name = st(&l["run"]);
        assert!(root(&format!("{FIX}/runs/{name}/run.json")).is_file(), "{name}");
        let kind = st(&l["kind"]).to_string();
        let positive = l["expected"] == CValue::Bool(true);
        assert!(seen.entry(kind.clone()).or_default().insert(positive), "{name}: repeated outcome for {kind}");

        let mut t = obj(&base).clone();
        t.insert("family".to_string(), CValue::Str("fabric".to_string()));
        t.remove("payloads");
        t.insert("predicates".to_string(), CValue::Arr(vec![l["predicate"].clone()]));
        let bytes = fid_canon::to_canonical_bytes(&CValue::Obj(t)).unwrap();
        let task = parse_task(&bytes, name).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(task.predicates[0].kind.as_str(), kind);
    }
    let kinds: BTreeSet<String> = seen.keys().cloned().collect();
    let all: BTreeSet<String> = PREDICATE_KINDS.iter().map(|k| k.to_string()).collect();
    assert_eq!(kinds, all);
    for (k, outcomes) in &seen {
        assert_eq!(outcomes.len(), 2, "{k}");
    }
}

/// The predicate semantics document lists every kind and every error code.
#[test]
fn docs_define_every_kind_and_error() {
    let doc = std::fs::read_to_string(root("docs/redteam-predicates.md")).unwrap();
    for k in PREDICATE_KINDS {
        assert!(doc.contains(&format!("`{k}`")), "{k}");
    }
    for code in [
        ErrorCode::MissingPredicate,
        ErrorCode::UnknownPredicate,
        ErrorCode::DuplicateMarker,
        ErrorCode::MarkerNotInPayload,
        ErrorCode::MarkerInPrompt,
    ] {
        assert!(doc.contains(&format!("`{}`", code.as_str())), "{code}");
    }
}
