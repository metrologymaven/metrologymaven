#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Acceptance tests of spec node AGENT.EVAL.FORMAT. Expected values come from
//! fixtures/agent/eval/make_fixtures.py (hashlib + hand-labelled cases), never from this crate.

use fid_canon::{parse, CValue};
use fid_eval_suite::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/agent/eval");
const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/canon/vectors.json");

fn fix(rel: &str) -> PathBuf {
    Path::new(FIX).join(rel)
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

fn expected_version() -> String {
    st(&obj(&load_json(&fix("expected.json")))["suite_version"]).to_string()
}

/// A fresh copy of suite-mini in the temp directory.
fn copy_suite(tag: &str) -> PathBuf {
    let dst = std::env::temp_dir().join(format!("fid-eval-suite-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dst);
    let mut stack = vec![(fix("suite-mini"), dst.clone())];
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

fn codes(errs: &[FormatError]) -> Vec<(&'static str, String, String)> {
    errs.iter().map(|e| (e.code.as_str(), e.file.clone(), e.field.clone())).collect()
}

/// AC-1: the CLI prints the golden suite version of suite-mini.
#[test]
fn ac1_cli_prints_golden_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-eval"))
        .args(["suite", "digest", "--suite"])
        .arg(fix("suite-mini"))
        .arg("--json")
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let v = parse(out.stdout.trim_ascii_end()).unwrap();
    let m = obj(&v);
    assert_eq!(st(&m["suite_version"]), expected_version());
    assert_eq!(m["tasks"], CValue::Num(2.0));
    assert_eq!(m["files"], CValue::Num(6.0));

    // Plain output is the version alone.
    let out = Command::new(env!("CARGO_BIN_EXE_fid-eval"))
        .args(["suite", "digest", "--suite"])
        .arg(fix("suite-mini"))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim_end(), expected_version());
}

/// AC-1 (library): the hashed file list and version match the reference script.
#[test]
fn ac1_library_files_and_version() {
    let suite = load_suite(&fix("suite-mini")).unwrap();
    assert_eq!(suite.name, "suite-mini");
    assert_eq!(suite.version.as_str(), expected_version());
    let exp = load_json(&fix("expected.json"));
    let want: Vec<(String, String)> = arr(&obj(&exp)["files"])
        .iter()
        .map(|p| (st(&arr(p)[0]).to_string(), st(&arr(p)[1]).to_string()))
        .collect();
    let got: Vec<(String, String)> = suite.files.iter().map(|(p, d)| (p.clone(), d.as_str().to_string())).collect();
    assert_eq!(got, want);
    assert_eq!(suite_version(&suite.files).unwrap(), suite.version);
    // Order of the input list does not matter.
    let mut rev = suite.files.clone();
    rev.reverse();
    assert_eq!(suite_version(&rev).unwrap(), suite.version);

    let t = &suite.tasks[0].task;
    assert_eq!(t.id, "mini-inspect-01");
    assert_eq!(t.tier, Tier::SingleCommand);
    assert_eq!(t.apps, vec![App::Inspect]);
    assert_eq!(t.budget, Budget { wall_s: 600, tool_calls: 40 });
    let t2 = &suite.tasks[1];
    assert_eq!(t2.task.apps, vec![App::Probe, App::Report]);
    assert!(t2.task.workflows.is_empty());
    // H1-DIA has a value and no tolerance: defaulted (AC-4 inside a suite).
    assert_eq!(t2.oracle.items[0].tol, Some(Quantity { amount: 1e-9, unit: "m".into() }));
    assert_eq!(t2.oracle.items[1].tol, None);
}

/// AC-2: one changed oracle byte changes the version; a copy elsewhere keeps it.
#[test]
fn ac2_version_tracks_content_not_location() {
    let same = copy_suite("ac2-copy");
    assert_eq!(load_suite(&same).unwrap().version.as_str(), expected_version());

    let changed = copy_suite("ac2-change");
    let p = changed.join("oracles").join("mini-inspect-01").join("oracle.json");
    let mut b = std::fs::read(&p).unwrap();
    let at = b.windows(5).position(|w| w == b"1e-07").unwrap();
    b[at] = b'2';
    std::fs::write(&p, &b).unwrap();
    let v = load_suite(&changed).unwrap().version;
    assert_ne!(v.as_str(), expected_version());

    let _ = std::fs::remove_dir_all(same);
    let _ = std::fs::remove_dir_all(changed);
}

/// AC-3: tier `easy` fails with invalid_task naming `tier`.
#[test]
fn ac3_bad_tier() {
    let errs = read_task(&fix("cases/task-bad-tier.json")).unwrap_err();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, ErrorCode::InvalidTask);
    assert_eq!(errs[0].field, "tier");
    assert!(errs[0].file.ends_with("task-bad-tier.json"));
}

/// AC-4: an item with a value and no tolerance gets 1e-9 in its unit.
#[test]
fn ac4_default_tolerance() {
    let o = read_oracle(&fix("cases/oracle-no-tol.json")).unwrap();
    assert_eq!(o.items[0].tol, Some(Quantity { amount: 1e-9, unit: "m".into() }));
    assert_eq!(o.items[1].tol, Some(Quantity { amount: 1e-9, unit: "rad".into() }));
    assert_eq!(o.items[1].value.as_ref().map(|q| q.amount.to_bits()), Some(0x3f50624dd2f1a9fc));
    assert_eq!(o.items[2].value, None);
    assert_eq!(o.items[2].tol, None);
    assert_eq!(DEFAULT_TOL.to_bits(), 1e-9f64.to_bits());
}

/// Every hand-labelled case: error code and the full set of fields named.
#[test]
fn labelled_cases() {
    let cases = load_json(&fix("cases.json"));
    let cases = arr(&cases);
    assert!(cases.len() >= 30);
    let mut failures = Vec::new();
    for c in cases {
        let c = obj(c);
        let file = st(&c["file"]);
        let path = fix("cases").join(file);
        let (code, mut fields, tols): (Option<ErrorCode>, Vec<String>, Option<Vec<Option<Quantity>>>) =
            match st(&c["kind"]) {
                "task" => match read_task(&path) {
                    Ok(_) => (None, vec![], None),
                    Err(e) => (Some(e[0].code), e.iter().map(|x| x.field.clone()).collect(), None),
                },
                _ => match read_oracle(&path) {
                    Ok(o) => (None, vec![], Some(o.items.iter().map(|i| i.tol.clone()).collect())),
                    Err(e) => (Some(e[0].code), e.iter().map(|x| x.field.clone()).collect(), None),
                },
            };
        fields.sort();
        let want_code = match &c["code"] {
            CValue::Null => None,
            v => Some(st(v).to_string()),
        };
        let mut want_fields: Vec<String> = arr(&c["fields"]).iter().map(|f| st(f).to_string()).collect();
        want_fields.sort();
        let got_code = code.map(|x| x.as_str().to_string());
        if got_code != want_code || fields != want_fields {
            failures.push(format!("{file}: got {got_code:?} {fields:?}, want {want_code:?} {want_fields:?}"));
        }
        if let Some(CValue::Arr(want)) = c.get("tols") {
            let want: Vec<Option<Quantity>> = want
                .iter()
                .map(|t| match t {
                    CValue::Null => None,
                    t => {
                        let m = obj(t);
                        let CValue::Num(a) = m["amount"] else { panic!("amount") };
                        Some(Quantity { amount: a, unit: st(&m["unit"]).to_string() })
                    }
                })
                .collect();
            if tols.as_ref() != Some(&want) {
                failures.push(format!("{file}: tolerances {tols:?}, want {want:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// REQ-006: a file over 64 MiB fails closed with input_limit naming the file.
#[test]
fn req006_file_size_limit() {
    let dir = std::env::temp_dir().join(format!("fid-eval-suite-{}-big", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("task.json");
    let f = std::fs::File::create(&p).unwrap();
    f.set_len(MAX_FILE_BYTES as u64 + 1).unwrap();
    drop(f);
    let errs = read_task(&p).unwrap_err();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, ErrorCode::InputLimit);
    assert!(errs[0].file.ends_with("task.json"));
    // The in-memory parser applies the same limit.
    let big = vec![b' '; MAX_FILE_BYTES + 1];
    assert_eq!(parse_task(&big, "mem.json").unwrap_err()[0].code, ErrorCode::InputLimit);
    let _ = std::fs::remove_dir_all(dir);
}

/// Suite-level fail-closed checks: missing oracle, tampered archive, stray file, ID mismatch.
#[test]
fn suite_layout_errors() {
    let d = copy_suite("no-oracle");
    std::fs::remove_dir_all(d.join("oracles").join("mini-probe-01")).unwrap();
    let errs = load_suite(&d).unwrap_err();
    assert_eq!(
        codes(&errs),
        vec![("missing_file", "oracles/mini-probe-01/oracle.json".to_string(), String::new())]
    );
    let _ = std::fs::remove_dir_all(d);

    let d = copy_suite("tamper");
    let p = d.join("tasks").join("mini-inspect-01").join("start.tar");
    let mut b = std::fs::read(&p).unwrap();
    b[600] ^= 1;
    std::fs::write(&p, b).unwrap();
    let errs = load_suite(&d).unwrap_err();
    assert_eq!(
        codes(&errs),
        vec![("invalid_task", "tasks/mini-inspect-01/task.json".to_string(), "start_state.digest".to_string())]
    );
    let _ = std::fs::remove_dir_all(d);

    let d = copy_suite("stray");
    std::fs::write(d.join("tasks").join("mini-probe-01").join("answer.txt"), b"C1 conforms").unwrap();
    let errs = load_suite(&d).unwrap_err();
    assert_eq!(codes(&errs), vec![("invalid_suite", "tasks/mini-probe-01/answer.txt".to_string(), String::new())]);
    let _ = std::fs::remove_dir_all(d);

    let d = copy_suite("rename");
    std::fs::rename(d.join("tasks").join("mini-probe-01"), d.join("tasks").join("mini-probe-02")).unwrap();
    let errs = load_suite(&d).unwrap_err();
    let c = codes(&errs);
    assert!(c.contains(&("invalid_task", "tasks/mini-probe-02/task.json".to_string(), "id".to_string())), "{c:?}");
    assert!(c.contains(&("invalid_suite", "oracles/mini-probe-01".to_string(), String::new())), "{c:?}");
    assert!(c.contains(&("missing_file", "oracles/mini-probe-02/oracle.json".to_string(), String::new())), "{c:?}");
    let _ = std::fs::remove_dir_all(d);

    let d = copy_suite("header");
    std::fs::write(d.join("suite.json"), b"{\"format\":\"fid-eval-suite@2\",\"name\":\"x\"}").unwrap();
    let errs = load_suite(&d).unwrap_err();
    assert_eq!(codes(&errs), vec![("invalid_suite", "suite.json".to_string(), "format".to_string())]);
    let _ = std::fs::remove_dir_all(d);
}

/// AC-5: the tooling canonical encoder reproduces every CORE.JRN.CANON vector.
#[test]
fn ac5_canon_vectors() {
    let vectors = load_json(Path::new(VECTORS));
    let vectors = arr(&vectors);
    assert!(vectors.len() >= 200);
    let mut failures = Vec::new();
    for v in vectors {
        let m = obj(v);
        let name = st(&m["name"]);
        match canonical_json(st(&m["input"]).as_bytes()) {
            Ok((bytes, d)) => {
                if bytes != st(&m["canonical"]).as_bytes() || d.as_str() != st(&m["digest"]) {
                    failures.push(name.to_string());
                }
            }
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }
    assert!(failures.is_empty(), "{} failed: {failures:?}", failures.len());
}

/// CLI usage and error paths.
#[test]
fn cli_errors() {
    let bin = env!("CARGO_BIN_EXE_fid-eval");
    let out = Command::new(bin).args(["suite", "digest"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = Command::new(bin).args(["suite", "digest", "--suite"]).arg(fix("cases")).arg("--json").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v = parse(out.stdout.trim_ascii_end()).unwrap();
    let errs = arr(&obj(&v)["errors"]);
    assert!(!errs.is_empty());
    assert_eq!(st(&obj(&errs[0])["code"]), "missing_file");
}
