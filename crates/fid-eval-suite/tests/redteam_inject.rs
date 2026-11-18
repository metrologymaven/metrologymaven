#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Acceptance tests of spec node AGENT.REDTEAM.INJECT (the injection family in
//! `suite/redteam/inject/`) and of the `payloads` task field it adds. Expected values come
//! from fixtures/agent/redteam/inject/make_inject.py (hashlib + hand-labelled cases), never
//! from this crate. Predicate evaluation on runs (AC-2, AC-3) is checked by that script's
//! reference evaluator until AGENT.HARNESS.SAFETY exists; the marker search of AC-3 is also
//! repeated here on the archived run.

use fid_canon::{parse, CValue};
use fid_eval_suite::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const SUITE: &str = "suite/redteam/inject";
const FIX: &str = "fixtures/agent/redteam/inject";

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
    load_json(&root(&format!("{FIX}/expected.json")))
}

/// The regular files of a start state: (path, bytes).
type Files = Vec<(String, Vec<u8>)>;

/// Minimal ustar reader for the start-state archives: (path, bytes) of every regular file.
fn untar(bytes: &[u8]) -> Files {
    let field = |h: &[u8]| String::from_utf8(h.iter().take_while(|b| **b != 0).copied().collect()).unwrap();
    let mut out = Vec::new();
    let mut pos = 0;
    while pos + 512 <= bytes.len() {
        let h = &bytes[pos..pos + 512];
        if h.iter().all(|b| *b == 0) {
            break;
        }
        assert_eq!(&h[257..262], b"ustar", "not a ustar header");
        let name = field(&h[0..100]);
        let prefix = field(&h[345..500]);
        let path = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let size = usize::from_str_radix(field(&h[124..136]).trim(), 8).unwrap();
        let data = bytes[pos + 512..pos + 512 + size].to_vec();
        if h[156] == b'0' || h[156] == 0 {
            out.push((path, data));
        }
        pos += 512 + size.div_ceil(512) * 512;
    }
    out
}

fn count(hay: &[u8], needle: &str) -> usize {
    let n = needle.as_bytes();
    if n.is_empty() || hay.len() < n.len() {
        return 0;
    }
    hay.windows(n.len()).filter(|w| *w == n).count()
}

/// Occurrences of `needle` in each start-state file, counting its path and its bytes.
fn hits(files: &[(String, Vec<u8>)], needle: &str) -> BTreeMap<String, usize> {
    files.iter().map(|(p, b)| (p.clone(), count(p.as_bytes(), needle) + count(b, needle))).collect()
}

fn start_files(t: &SuiteTask) -> Files {
    let p = root(SUITE).join("tasks").join(&t.task.id).join(t.task.start_state.path.as_str());
    untar(&std::fs::read(p).unwrap())
}

fn marker_of(t: &Task) -> &str {
    let ms: Vec<&str> = t
        .predicates
        .iter()
        .filter_map(|p| match &p.kind {
            PredicateKind::EntryWithMarker { marker } => Some(marker.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ms.len(), 1, "{}: one entry_with_marker", t.id);
    ms[0]
}

/// AC-1 (library), REQ-001, REQ-003: the family loads fail-closed with at least 5 tasks of
/// family inject, distinct markers, the P0 vectors covered, and predicates, payloads and
/// suite version equal to the reference script's.
#[test]
fn ac1_inject_suite_loads() {
    let suite = load_suite(&root(SUITE)).unwrap();
    let exp = expected();
    let exp = obj(&exp);
    assert_eq!(suite.name, "fid-redteam-inject");
    assert_eq!(suite.version.as_str(), st(&exp["suite_version"]));
    let want_files: Vec<(String, String)> =
        arr(&exp["files"]).iter().map(|p| (st(&arr(p)[0]).to_string(), st(&arr(p)[1]).to_string())).collect();
    let got_files: Vec<(String, String)> =
        suite.files.iter().map(|(p, d)| (p.clone(), d.as_str().to_string())).collect();
    assert_eq!(got_files, want_files);

    assert!(suite.tasks.len() >= 5, "REQ-001: {} tasks", suite.tasks.len());
    let want_tasks = arr(&exp["tasks"]);
    assert_eq!(suite.tasks.len(), want_tasks.len());
    let mut markers = BTreeSet::new();
    let mut vectors = BTreeSet::new();
    for (t, w) in suite.tasks.iter().zip(want_tasks) {
        let w = obj(w);
        let t = &t.task;
        assert_eq!(t.id, st(&w["id"]));
        assert_eq!(t.family, Some(Family::Inject));
        let m = marker_of(t);
        assert_eq!(m, st(&w["marker"]));
        assert!(markers.insert(m.to_string()), "REQ-AGENT.REDTEAM-007: marker of {} repeats", t.id);
        let preds: Vec<(String, String)> =
            t.predicates.iter().map(|p| (p.id.clone(), p.kind.as_str().to_string())).collect();
        let want: Vec<(String, String)> =
            arr(&w["predicates"]).iter().map(|p| (st(&arr(p)[0]).to_string(), st(&arr(p)[1]).to_string())).collect();
        assert_eq!(preds, want, "{}", t.id);
        // REQ-003: a goal predicate besides the marker predicate.
        let goals: Vec<&str> = arr(&w["goal_predicates"]).iter().map(st).collect();
        assert!(!goals.is_empty() && !goals.contains(&"marker"), "{}", t.id);
        for g in &goals {
            assert!(t.predicates.iter().any(|p| p.id == *g), "{}: goal predicate {g}", t.id);
        }
        let payloads: Vec<(String, String)> =
            t.payloads.iter().map(|p| (p.file.as_str().to_string(), p.vector.as_str().to_string())).collect();
        let want: Vec<(String, String)> =
            arr(&w["payloads"]).iter().map(|p| (st(&arr(p)[0]).to_string(), st(&arr(p)[1]).to_string())).collect();
        assert_eq!(payloads, want, "{}", t.id);
        vectors.extend(t.payloads.iter().map(|p| p.vector));
    }
    for v in [Vector::ElementName, Vector::FeatureName, Vector::FileMetadata, Vector::FileName, Vector::OperatorText] {
        assert!(vectors.contains(&v), "REQ-001: vector {v} not covered");
    }
}

/// AC-1 (CLI): `fid-eval suite check` passes and reports at least 5 tasks and as many
/// distinct markers.
#[test]
fn ac1_cli_suite_check() {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-eval"))
        .args(["suite", "check", "--suite"])
        .arg(root(SUITE))
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let v = parse(out.stdout.trim_ascii_end()).unwrap();
    let m = obj(&v);
    let exp = expected();
    let tasks = arr(&obj(&exp)["tasks"]);
    let n = tasks.len() as f64;
    assert!(n >= 5.0);
    assert_eq!(m["tasks"], CValue::Num(n));
    assert_eq!(m["markers"], CValue::Num(n));
    assert_eq!(obj(&m["families"])["inject"], CValue::Num(n));
    let payloads: usize = tasks.iter().map(|t| arr(&obj(t)["payloads"]).len()).sum();
    assert_eq!(m["payloads"], CValue::Num(payloads as f64));
    let preds: usize = tasks.iter().map(|t| arr(&obj(t)["predicates"]).len()).sum();
    assert_eq!(m["predicates"], CValue::Num(preds as f64));
    assert_eq!(st(&m["suite_version"]), st(&obj(&exp)["suite_version"]));

    // The fabrication family has no entry_with_marker predicates and no payloads.
    let out = Command::new(env!("CARGO_BIN_EXE_fid-eval"))
        .args(["suite", "check", "--suite"])
        .arg(root("suite/redteam/fabric"))
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success());
    let v = parse(out.stdout.trim_ascii_end()).unwrap();
    assert_eq!(obj(&v)["markers"], CValue::Num(0.0));
    assert_eq!(obj(&v)["payloads"], CValue::Num(0.0));
}

/// REQ-002, REQ-003 and REQ-AGENT.REDTEAM.PRED-004/-005 on the archives: each marker sits in
/// a payload file of its task (path or bytes) and nowhere else in the suite, never in a
/// prompt; every start-state file has a licence entry; the payload files exist.
#[test]
fn markers_only_in_payloads() {
    let suite = load_suite(&root(SUITE)).unwrap();
    let all: Vec<(String, Files)> = suite.tasks.iter().map(|t| (t.task.id.clone(), start_files(t))).collect();
    for (t, (_, files)) in suite.tasks.iter().zip(&all) {
        let t = &t.task;
        let m = marker_of(t);
        assert!(!t.prompt.contains(m), "{}: marker in prompt", t.id);
        let payload_files: BTreeSet<&str> = t.payloads.iter().map(|p| p.file.as_str()).collect();
        let paths: BTreeSet<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        let licensed: BTreeSet<&str> = t.licences.iter().map(|l| l.file.as_str()).collect();
        assert_eq!(paths, licensed, "{}: licences must list every start-state file", t.id);
        for p in &payload_files {
            assert!(paths.contains(p), "{}: payload {p} not in the start state", t.id);
        }
        let h = hits(files, m);
        assert!(payload_files.iter().any(|p| h[*p] > 0), "{}: marker in no payload file", t.id);
        for (p, n) in &h {
            assert!(*n == 0 || payload_files.contains(p.as_str()), "{}: marker in non-payload file {p}", t.id);
        }
        for (other, ofiles) in &all {
            if other != &t.id {
                assert!(hits(ofiles, m).values().all(|n| *n == 0), "marker of {} found in {other}", t.id);
            }
        }
    }
}

/// AC-4, REQ-004: exactly one task carries the closing untrusted delimiter, exactly once, in
/// its payload file, and no task carries the opening delimiter.
#[test]
fn ac4_delimiter_task() {
    let delims = load_json(&root(&format!("{FIX}/taint-delimiters.json")));
    let open = st(&obj(&delims)["open"]).to_string();
    let close = st(&obj(&delims)["close"]).to_string();
    let suite = load_suite(&root(SUITE)).unwrap();
    let exp = expected();
    let mut with_close = Vec::new();
    for (t, w) in suite.tasks.iter().zip(arr(&obj(&exp)["tasks"])) {
        let files = start_files(t);
        assert!(hits(&files, &open).values().all(|n| *n == 0), "{}: opening delimiter", t.task.id);
        let h = hits(&files, &close);
        let total: usize = h.values().sum();
        if total > 0 {
            with_close.push(t.task.id.clone());
            assert_eq!(total, 1, "AC-4: {} has the delimiter {total} times", t.task.id);
            let in_payload: usize = t.task.payloads.iter().map(|p| h[p.file.as_str()]).sum();
            assert_eq!(in_payload, 1, "AC-4: delimiter outside the payload of {}", t.task.id);
        }
        assert_eq!(obj(w)["delimiter"], CValue::Bool(total > 0), "{}", t.task.id);
    }
    assert_eq!(with_close, vec!["inject-delimiter-close".to_string()]);
}

/// AC-2 and AC-3 on the archived runs, by a direct search for the marker in the committed
/// entries' arguments (the full predicate set is checked by the reference evaluator).
#[test]
fn ac2_ac3_marker_in_runs() {
    let suite = load_suite(&root(SUITE)).unwrap();
    let labels = load_json(&root(&format!("{FIX}/runs/labels.json")));
    let mut nulls = BTreeSet::new();
    let mut obey = false;
    for l in arr(&labels) {
        let l = obj(l);
        let run = load_json(&root(&format!("{FIX}/runs/{}/run.json", st(&l["run"]))));
        let task = suite.tasks.iter().find(|t| t.task.id == st(&l["task"])).unwrap();
        let m = marker_of(&task.task);
        let found = arr(&obj(&run)["entries"]).iter().any(|e| {
            let args = fid_canon::to_canonical_bytes(&obj(e)["args"]).unwrap();
            count(&args, m) > 0
        });
        assert_eq!(CValue::Bool(found), obj(&l["expected"])["marker"], "{}", st(&l["run"]));
        if st(&l["agent"]) == "null" {
            assert!(arr(&obj(&run)["entries"]).is_empty());
            assert!(obj(&l["expected"]).values().all(|v| *v == CValue::Bool(false)), "AC-2: {}", st(&l["run"]));
            nulls.insert(st(&l["task"]).to_string());
        }
        if st(&l["agent"]) == "obey-names" && st(&l["task"]) == "inject-element-name" {
            assert!(found, "AC-3");
            obey = true;
        }
    }
    let ids: BTreeSet<String> = suite.tasks.iter().map(|t| t.task.id.clone()).collect();
    assert_eq!(nulls, ids, "AC-2: a null run per task");
    assert!(obey, "AC-3: obey-names run");
    assert!(root("fixtures/agent/redteam/agents/obey-names/agent.json").is_file());
}

/// Every hand-labelled `payloads` case: error code and the set of fields named.
#[test]
fn payload_labelled_cases() {
    let dir = root(FIX);
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

    let t = read_task(&dir.join("cases/inj-valid-pmi-two.json")).unwrap();
    assert_eq!(
        t.payloads.iter().map(|p| (p.file.as_str(), p.vector)).collect::<Vec<_>>(),
        vec![
            ("in/part.json", Vector::ElementName),
            ("in/part.json", Vector::PmiNote),
            ("README.txt", Vector::FileName)
        ]
    );
    // An ordinary task has no payloads.
    let plain = read_task(&root("fixtures/agent/redteam/fabric/cases/rt-plain-task.json")).unwrap();
    assert!(plain.payloads.is_empty());
}

/// Drift guard: the vector list and the `payloads` rules in the published task schema
/// equal the Rust side.
#[test]
fn payload_schema_matches_rust() {
    let schema = load_json(&root("schema/eval-task-v1.schema.json"));
    let s = obj(&schema);
    let strs = |v: &CValue| arr(v).iter().map(|x| st(x).to_string()).collect::<Vec<_>>();
    let defs = obj(&s["$defs"]);
    assert_eq!(
        strs(&obj(&defs["vector"])["enum"]),
        Vector::ALL.iter().map(|v| v.as_str().to_string()).collect::<Vec<_>>()
    );
    let payloads = obj(&obj(&s["properties"])["payloads"]);
    let item = obj(&payloads["items"]);
    assert_eq!(strs(&item["required"]), vec!["file", "vector"]);
    assert_eq!(obj(&item["properties"]).keys().collect::<Vec<_>>(), vec!["file", "vector"]);
    assert_eq!(strs(&obj(&s["dependentRequired"])["payloads"]), vec!["family"]);
    assert!(!strs(&s["required"]).contains(&"payloads".to_string()));
}
