//! `fid-xart compare` against the reports the Python oracle wrote
//! (`fixtures/man/xart-calls-v1/make_fixtures.py`): AC-1, AC-2, AC-3, AC-4, REQ-001, REQ-005.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use fid_xart::matrix::parse_matrix;
use fid_xart::report::{compare, Release};
use fid_xart::table::DigestTable;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/xart-calls-v1")
}

fn scenario_dir(name: &str) -> PathBuf {
    fixtures().join("scenarios").join(name)
}

struct Scenario {
    name: String,
    matrix: fid_xart::matrix::Matrix,
    tables: Vec<DigestTable>,
    previous: Option<DigestTable>,
    release: Release,
    exit: u8,
    report: Vec<u8>,
}

fn load(name: &str) -> Scenario {
    let d = scenario_dir(name);
    let text = |f: &str| std::fs::read_to_string(d.join(f)).unwrap();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(d.join("tables")).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    let tables = paths.iter().map(|p| DigestTable::parse(&std::fs::read(p).unwrap()).unwrap()).collect();
    let previous = d.join("previous.json");
    Scenario {
        name: name.to_string(),
        matrix: parse_matrix(&std::fs::read(d.join("matrix.toml")).unwrap()).unwrap(),
        tables,
        previous: previous.exists().then(|| DigestTable::parse(&std::fs::read(&previous).unwrap()).unwrap()),
        release: Release::parse(text("release.txt").trim_end()).unwrap(),
        exit: text("expected-exit.txt").trim_end().parse().unwrap(),
        report: std::fs::read(d.join("expected-report.json")).unwrap(),
    }
}

fn scenarios() -> Vec<Scenario> {
    let mut names: Vec<String> = std::fs::read_dir(fixtures().join("scenarios"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert!(names.len() >= 9, "scenarios missing: {names:?}");
    names.iter().map(|n| load(n)).collect()
}

#[test]
fn every_scenario_matches_the_oracle() {
    for s in scenarios() {
        let c = compare(&s.matrix, &s.tables, s.release, s.previous.as_ref()).unwrap();
        assert_eq!(u8::from(!c.passed()), s.exit, "{}: exit; failures {:?}", s.name, c.failures);
        assert_eq!(String::from_utf8_lossy(&c.report), String::from_utf8_lossy(&s.report), "{}: report bytes", s.name);
    }
}

#[test]
fn ac1_identical_wasm_digests_pass() {
    let s = load("ac1-ac2-identical");
    let c = compare(&s.matrix, &s.tables, s.release, None).unwrap();
    assert!(c.passed(), "{:?}", c.failures);
}

#[test]
fn ac2_divergent_native_is_counted_not_failed() {
    let s = load("ac1-ac2-identical");
    let c = compare(&s.matrix, &s.tables, s.release, None).unwrap();
    assert!(c.passed());
    let report = String::from_utf8(c.report).unwrap();
    let reversed = report.split("\"artifact\":\"native-reversed\"").next().unwrap();
    let echo = reversed.rsplit("{\"algorithms\":[").next().unwrap();
    assert!(
        echo.starts_with("{\"algorithm\":\"test.echo.identity@1.0.0\",\"calls\":\"8\",\"matches\":\"5\"}"),
        "{echo}"
    );
}

#[test]
fn ac3_patch_release_differing_entry_fails_naming_call_and_algorithm() {
    let s = load("ac3-patch-differs");
    let c = compare(&s.matrix, &s.tables, Release::Patch, s.previous.as_ref()).unwrap();
    assert!(!c.passed());
    assert_eq!(c.failures, ["patch_digest_mismatch: call 9 (recurse-100) algorithm test.stack.recurse@1.0.0"]);
}

#[test]
fn ac4_same_inputs_same_bytes_in_any_order() {
    for s in scenarios() {
        let a = compare(&s.matrix, &s.tables, s.release, s.previous.as_ref()).unwrap();
        let b = compare(&s.matrix, &s.tables, s.release, s.previous.as_ref()).unwrap();
        let mut reversed = s.tables.clone();
        reversed.reverse();
        let c = compare(&s.matrix, &reversed, s.release, s.previous.as_ref()).unwrap();
        assert_eq!(a, b, "{}", s.name);
        assert_eq!(a, c, "{}", s.name);
    }
}

#[test]
fn inputs_that_do_not_fit_are_errors() {
    let s = load("ac1-ac2-identical");
    let err = |tables: &[DigestTable], release, previous: Option<&DigestTable>| {
        compare(&s.matrix, tables, release, previous).unwrap_err().code()
    };
    assert_eq!(err(&[], Release::None, None), "bad_inputs");
    // A patch release without the previous table, and a previous table without a patch release.
    assert_eq!(err(&s.tables, Release::Patch, None), "bad_inputs");
    assert_eq!(err(&s.tables, Release::Minor, Some(&s.tables[0])), "bad_inputs");
    // Two WASM tables for one host.
    let mut dup = s.tables.clone();
    dup.push(s.tables.iter().find(|t| t.kind == fid_xart::table::Kind::Wasm).unwrap().clone());
    assert_eq!(err(&dup, Release::None, None), "bad_inputs");
    // A host outside the matrix.
    let mut stray = s.tables.clone();
    stray[0].host = "solaris-sparc".to_string();
    assert_eq!(err(&stray, Release::None, None), "bad_inputs");
    // Another call set, or other calls under the same call-set digest.
    let mut other = s.tables.clone();
    other[1].call_set_digest = fid_kci::kci::sha256_digest(b"other");
    assert_eq!(err(&other, Release::None, None), "bad_inputs");
    let mut renamed = s.tables.clone();
    renamed[1].rows[0].name = "renamed".to_string();
    assert_eq!(err(&renamed, Release::None, None), "bad_inputs");
}

fn cli(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xart")).args(args).output().unwrap();
    (out.status.code().unwrap(), String::from_utf8_lossy(&out.stdout).to_string())
}

#[test]
fn cli_compare_exit_codes_and_report_bytes() {
    let tmp = std::env::temp_dir().join(format!("fid-xart-scenarios-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    for (name, release, previous) in [
        ("ac1-ac2-identical", "none", false),
        ("ac3-patch-differs", "patch", true),
        ("required-not-run", "none", false),
    ] {
        let d = scenario_dir(name);
        let mut args = vec![
            "compare".to_string(),
            "--matrix".to_string(),
            d.join("matrix.toml").display().to_string(),
            "--tables".to_string(),
            d.join("tables").display().to_string(),
            "--release".to_string(),
            release.to_string(),
        ];
        if previous {
            args.extend(["--previous".to_string(), d.join("previous.json").display().to_string()]);
        }
        let mut reports = Vec::new();
        for run in 0..2 {
            let out = tmp.join(format!("{name}-{run}.json"));
            let mut a = args.clone();
            a.extend(["--out".to_string(), out.display().to_string()]);
            let a: Vec<&str> = a.iter().map(String::as_str).collect();
            let (code, stdout) = cli(&a);
            let expected: i32 =
                std::fs::read_to_string(d.join("expected-exit.txt")).unwrap().trim_end().parse().unwrap();
            assert_eq!(code, expected, "{name}: {stdout}");
            if name == "ac3-patch-differs" {
                assert!(stdout
                    .contains("FAIL patch_digest_mismatch: call 9 (recurse-100) algorithm test.stack.recurse@1.0.0"));
            }
            reports.push(std::fs::read(&out).unwrap());
        }
        assert_eq!(reports[0], reports[1], "{name}: AC-4");
        assert_eq!(reports[0], std::fs::read(d.join("expected-report.json")).unwrap(), "{name}");
    }
    // Usage errors exit 2: no previous table for a patch release, a stray file among the tables.
    let d = scenario_dir("ac1-ac2-identical");
    let m = d.join("matrix.toml").display().to_string();
    let t = d.join("tables").display().to_string();
    let o = tmp.join("x.json").display().to_string();
    assert_eq!(cli(&["compare", "--matrix", &m, "--tables", &t, "--release", "patch", "--out", &o]).0, 2);
    assert_eq!(
        cli(&["compare", "--matrix", &m, "--tables", &d.display().to_string(), "--release", "none", "--out", &o]).0,
        2
    );
    assert_eq!(cli(&["frobnicate"]).0, 2);
    std::fs::remove_dir_all(&tmp).unwrap();
}
