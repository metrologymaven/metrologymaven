#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Acceptance tests of spec node AGENT.REDTEAM.FABRIC (the fabrication family in
//! `suite/redteam/fabric/`) and of the red-team task extension it needs (`family`,
//! `predicates`; AGENT.REDTEAM.PRED schema). Expected values come from
//! fixtures/agent/redteam/fabric/make_fabric.py (hashlib + hand-labelled cases), never from
//! this crate. Predicate evaluation on runs (AC-2, AC-3) is checked by that script's
//! reference evaluator until AGENT.HARNESS.SAFETY exists.

use fid_canon::{parse, CValue};
use fid_eval_suite::*;
use fid_types::SideEffect;
use std::collections::BTreeMap;
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
    load_json(&root("fixtures/agent/redteam/fabric/expected.json"))
}

/// AC-1 (library): the family loads fail-closed with at least 5 tasks, all `family: fabric`,
/// each with `result_not_from_kernel` and `numeric_claim` (REQ-001, REQ-002), and its
/// suite version and per-task predicate kinds equal the reference script's.
#[test]
fn ac1_fabric_suite_loads() {
    let suite = load_suite(&root("suite/redteam/fabric")).unwrap();
    let exp = expected();
    let exp = obj(&exp);
    assert_eq!(suite.name, "fid-redteam-fabric");
    assert_eq!(suite.version.as_str(), st(&exp["suite_version"]));
    let want_files: Vec<(String, String)> =
        arr(&exp["files"]).iter().map(|p| (st(&arr(p)[0]).to_string(), st(&arr(p)[1]).to_string())).collect();
    let got_files: Vec<(String, String)> =
        suite.files.iter().map(|(p, d)| (p.clone(), d.as_str().to_string())).collect();
    assert_eq!(got_files, want_files);

    assert!(suite.tasks.len() >= 5, "REQ-001: {} tasks", suite.tasks.len());
    let want_tasks = arr(&exp["tasks"]);
    assert_eq!(suite.tasks.len(), want_tasks.len());
    for (t, w) in suite.tasks.iter().zip(want_tasks) {
        let w = obj(w);
        assert_eq!(t.task.id, st(&w["id"]));
        assert_eq!(t.task.family, Some(Family::Fabric));
        let kinds: Vec<&str> = t.task.predicates.iter().map(|p| p.kind.as_str()).collect();
        let want: Vec<&str> = arr(&w["predicates"]).iter().map(st).collect();
        assert_eq!(kinds, want, "{}", t.task.id);
        assert!(kinds.contains(&"result_not_from_kernel"), "REQ-002: {}", t.task.id);
        for p in &t.task.predicates {
            if let PredicateKind::NumericClaim { fields } = &p.kind {
                assert_eq!(fields, &vec!["brief".to_string(), "annotation".to_string()], "{}", t.task.id);
            }
        }
    }
}

/// REQ-003: the formula task requires R1 as a dimensionless RESULTS_ONLY value, the path
/// through `expr.evaluate_formula@1`.
#[test]
fn req003_formula_task_present() {
    let suite = load_suite(&root("suite/redteam/fabric")).unwrap();
    let t = suite.tasks.iter().find(|t| t.task.id == "fabric-formula-ratio").unwrap();
    let item = &t.oracle.items[0];
    assert_eq!(item.characteristic, "R1");
    assert_eq!(item.basis, Basis::ResultsOnly);
    assert_eq!(item.value, Some(Quantity { amount: 1.25, unit: "1".into() }));
}

/// AC-1 (CLI): `fid-eval suite check` passes on the family and counts it.
#[test]
fn ac1_cli_suite_check() {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-eval"))
        .args(["suite", "check", "--suite"])
        .arg(root("suite/redteam/fabric"))
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let v = parse(out.stdout.trim_ascii_end()).unwrap();
    let m = obj(&v);
    let exp = expected();
    let n = arr(&obj(&exp)["tasks"]).len() as f64;
    assert!(n >= 5.0);
    assert_eq!(m["tasks"], CValue::Num(n));
    assert_eq!(obj(&m["families"])["fabric"], CValue::Num(n));
    assert_eq!(m["predicates"], CValue::Num(2.0 * n));
    assert_eq!(st(&m["suite_version"]), st(&obj(&exp)["suite_version"]));

    // Plain output, and an ordinary suite without red-team tasks.
    let out = Command::new(env!("CARGO_BIN_EXE_fid-eval"))
        .args(["suite", "check", "--suite"])
        .arg(root("fixtures/agent/eval/suite-mini"))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("ok: 2 tasks, suite version sha256:"));

    // A broken suite fails with exit code 1.
    let out = Command::new(env!("CARGO_BIN_EXE_fid-eval"))
        .args(["suite", "check", "--suite"])
        .arg(root("fixtures/agent/redteam/fabric/cases"))
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}

/// Every hand-labelled red-team case: error code and the set of fields named.
#[test]
fn redteam_labelled_cases() {
    let dir = root("fixtures/agent/redteam/fabric");
    let cases = load_json(&dir.join("cases.json"));
    let cases = arr(&cases);
    assert!(cases.len() >= 10);
    let mut failures = Vec::new();
    for c in cases {
        let c = obj(c);
        let file = st(&c["file"]);
        let (code, mut fields) = match read_task(&dir.join("cases").join(file)) {
            Ok(_) => (None, vec![]),
            Err(e) => (Some(e[0].code.as_str().to_string()), e.iter().map(|x| x.field.clone()).collect::<Vec<_>>()),
        };
        fields.sort();
        let want_code = match &c["code"] {
            CValue::Null => None,
            v => Some(st(v).to_string()),
        };
        let mut want_fields: Vec<String> = arr(&c["fields"]).iter().map(|f| st(f).to_string()).collect();
        want_fields.sort();
        if code != want_code || fields != want_fields {
            failures.push(format!("{file}: got {code:?} {fields:?}, want {want_code:?} {want_fields:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The parsed predicate parameters of the valid cases.
#[test]
fn predicate_parameters_parse() {
    let dir = root("fixtures/agent/redteam/fabric/cases");
    let t = read_task(&dir.join("rt-valid-other-kinds.json")).unwrap();
    assert_eq!(t.family, Some(Family::Inject));
    let kinds: Vec<PredicateKind> = t.predicates.into_iter().map(|p| p.kind).collect();
    assert_eq!(
        kinds,
        vec![
            PredicateKind::EntryWithMarker { marker: "FIDRT-0123456789ABCDEFGHJKMNPQRS".into() },
            PredicateKind::EntryClass { classes: vec![SideEffect::W3, SideEffect::X1] },
            PredicateKind::PromotionWithoutDistinctHuman,
            PredicateKind::FileChange { pattern: "out/*.csv".into() },
        ]
    );
    let plain = read_task(&dir.join("rt-plain-task.json")).unwrap();
    assert_eq!(plain.family, None);
    assert!(plain.predicates.is_empty());
}

#[test]
fn marker_format() {
    assert!(is_marker("FIDRT-0123456789ABCDEFGHJKMNPQRS"));
    assert!(is_marker("FIDRT-00000000000000000000000000"));
    for bad in [
        "FIDRT-0123456789ABCDEFGHJKMNPQR",   // 25
        "FIDRT-0123456789ABCDEFGHJKMNPQRST", // 27
        "FIDRT-0123456789ABCDEFGHIJKLMNOP",  // I L O
        "FIDRT-0123456789abcdefghjkmnpqrs",  // lower case
        "fidrt-0123456789ABCDEFGHJKMNPQRS",
        "",
    ] {
        assert!(!is_marker(bad), "{bad}");
    }
}

/// Drift guard: the red-team lists in the published task schema equal the Rust constants.
#[test]
fn redteam_schema_matches_rust() {
    let schema = load_json(&root("schema/eval-task-v1.schema.json"));
    let s = obj(&schema);
    let strs = |v: &CValue| arr(v).iter().map(|x| st(x).to_string()).collect::<Vec<_>>();
    let props = obj(&s["properties"]);
    assert_eq!(
        strs(&obj(&props["family"])["enum"]),
        Family::ALL.iter().map(|f| f.as_str().to_string()).collect::<Vec<_>>()
    );
    let defs = obj(&s["$defs"]);
    let kinds: Vec<String> = arr(&obj(&defs["predicate"])["oneOf"])
        .iter()
        .map(|alt| st(&obj(&obj(&obj(alt)["properties"])["kind"])["const"]).to_string())
        .collect();
    assert_eq!(kinds, PREDICATE_KINDS.iter().map(|k| k.to_string()).collect::<Vec<_>>());
    assert_eq!(
        strs(&obj(&defs["side_effect"])["enum"]),
        SideEffect::ALL.iter().map(|c| c.as_str().to_string()).collect::<Vec<_>>()
    );
    // The optional red-team keys are not in `required`; the base keys still are.
    let required = strs(&s["required"]);
    assert!(!required.contains(&"family".to_string()) && !required.contains(&"predicates".to_string()));
    assert_eq!(required.len(), 12);
}
