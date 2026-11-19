//! Acceptance criteria of spec node CORE.REG.VER.GATE (AC-1 .. AC-5) and the hand-labelled
//! cases of `fixtures/core/reg/ver/gate/expect.json`, written by `make_fixtures.py` there
//! from the leaf's rules (never by running this gate).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_registry::Date;
use fid_xtask::registry_gate::{
    bytes_digest, digest_path, exit_code, gate, load_baseline, render, run, GateReport, BASELINE_INVALID,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const DIR: &str = "fixtures/core/reg/ver/gate";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join(DIR).join(name)
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    let CValue::Obj(o) = v else { panic!("object expected") };
    o
}

fn text(v: &CValue) -> &str {
    let CValue::Str(s) = v else { panic!("string expected") };
    s
}

fn cases() -> Vec<BTreeMap<String, CValue>> {
    let doc = parse(&std::fs::read(fixture("expect.json")).unwrap()).unwrap();
    let CValue::Arr(items) = &obj(&doc)["cases"] else { panic!("cases") };
    items.iter().map(|c| obj(c).clone()).collect()
}

fn canon(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

fn release(case: &BTreeMap<String, CValue>) -> Option<Date> {
    case.get("release_date").map(|d| Date::parse(text(d)).unwrap())
}

/// Every hand-labelled case: exit status, failures, exact report, and the same bytes on a
/// second run (REQ-005 determinism).
#[test]
fn expect_cases() {
    let all = cases();
    assert!(all.len() >= 25, "only {} cases", all.len());
    for c in &all {
        let name = text(&c["name"]);
        let (b, cur) = (fixture(text(&c["baseline"])), fixture(text(&c["current"])));
        let result = run(&b, &cur, release(c));
        let want_exit = match &c["exit"] {
            CValue::Num(n) => *n as u8,
            CValue::I64(n) => *n as u8,
            CValue::U64(n) => *n as u8,
            other => panic!("{name}: exit {other:?}"),
        };
        assert_eq!(exit_code(&result), want_exit, "{name}: {}", render(&result));
        assert_eq!(render(&run(&b, &cur, release(c))), render(&result), "{name}: not deterministic");
        match &result {
            Ok(report) => {
                let got = CValue::Arr(report.failures.iter().map(|f| f.to_value()).collect());
                assert_eq!(canon(&got), canon(&c["failures"]), "{name}");
                assert_eq!(report.render(), text(&c["report"]), "{name}");
            }
            Err(e) => assert_eq!(e.code, text(&c["code"]), "{name}: {e}"),
        }
    }
}

/// Runs the binary from the repository root; returns (exit code, stdout).
fn cli(baseline: &str, current: &str) -> (i32, String) {
    let go = || {
        let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
            .current_dir(root())
            .args(["registry-gate", "--baseline", &format!("{DIR}/{baseline}"), "--current"])
            .arg(format!("{DIR}/{current}"))
            .output()
            .unwrap();
        (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
    };
    let got = go();
    assert_eq!(go(), got, "D0: identical output across runs");
    got
}

/// AC-1: additive changes and new (ID, major)s pass.
#[test]
fn ac1_additive_exits_0() {
    let (code, out) = cli("baseline.json", "additive.json");
    assert_eq!((code, out.as_str()), (0, "registry-gate: ok\n"));
}

/// AC-2: a parameter type change under the same major exits 1 naming command and pointer.
#[test]
fn ac2_type_change_exits_1() {
    let (code, out) = cli("baseline.json", "type_change.json");
    assert_eq!(code, 1, "{out}");
    assert_eq!(
        out,
        "{\"code\":\"type_changed\",\"command\":\"demo.measure\",\"major\":1,\
         \"pointer\":\"/params_schema/properties/tol/type\"}\nregistry-gate: fail (1)\n"
    );
}

/// AC-3: W2 -> W1 and nothing else exits 1.
#[test]
fn ac3_class_change_exits_1() {
    let (code, out) = cli("baseline.json", "class_change.json");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("\"code\":\"side_effect_changed\",\"command\":\"demo.export\""), "{out}");
}

/// AC-4: a silent removal exits 1 with `silent_removal`.
#[test]
fn ac4_silent_removal_exits_1() {
    let (code, out) = cli("baseline.json", "silent_removal.json");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("\"code\":\"silent_removal\",\"command\":\"demo.export\""), "{out}");
}

/// AC-5: a baseline whose stated digest does not match its bytes exits 2.
#[test]
fn ac5_tampered_baseline_exits_2() {
    let (code, out) = cli("baseline_tampered.json", "additive.json");
    assert_eq!(code, 2, "{out}");
    assert!(out.starts_with("registry-gate: baseline_invalid: "), "{out}");
    // The tampered file still parses; only the digest catches it.
    assert!(parse(&std::fs::read(fixture("baseline_tampered.json")).unwrap()).is_ok());
}

/// The sidecar of the real baseline states the digest of its exact bytes.
#[test]
fn sidecar_matches_bytes() {
    let b = fixture("baseline.json");
    let stated = std::fs::read_to_string(digest_path(&b)).unwrap();
    assert_eq!(stated, format!("{}\n", bytes_digest(&std::fs::read(&b).unwrap()).as_str()));
    assert!(load_baseline(&b).is_ok());
}

/// A missing default baseline is `baseline_invalid`, exit 2, never a pass.
#[test]
fn missing_default_baseline_exits_2() {
    let dir = std::env::temp_dir().join(format!("fid-reg-gate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .current_dir(&dir)
        .args(["registry-gate", "--current"])
        .arg(fixture("additive.json"))
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out.status.code(), Some(2));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with(&format!("registry-gate: {BASELINE_INVALID}: ")), "{stdout}");
}

/// Usage errors exit 2.
#[test]
fn usage_errors_exit_2() {
    for args in [
        vec!["registry-gate"],
        vec!["registry-gate", "--current"],
        vec!["registry-gate", "--bogus", "x"],
        vec!["registry-gate", "--current", "x.json", "--release-date", "2026-02-30"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask")).current_dir(root()).args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

/// Identical manifests pass; the pure gate agrees with `run`.
#[test]
fn identical_manifests_pass() {
    let b = load_baseline(&fixture("baseline.json")).unwrap();
    assert_eq!(gate(&b, &b, None), GateReport::default());
    assert!(gate(&b, &b, None).passed());
}
