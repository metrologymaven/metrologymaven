//! Acceptance criteria of spec node `KRN.NUM.LINT.CLI` (AC-1 to AC-5, REQ-001 to REQ-005).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use fid_canon::{parse, CValue};
use fid_knp1_lint::find::lines_digest;

const BIN: &str = env!("CARGO_BIN_EXE_fid-knp1-lint");

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> PathBuf {
    repo_root().join("fixtures/knp1").join(rel)
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("fid-knp1-cli-{}-{}-{n}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

fn parse_report(path: &Path) -> std::collections::BTreeMap<String, CValue> {
    let bytes = std::fs::read(path).unwrap();
    let val = parse(&bytes).unwrap();
    let CValue::Obj(map) = val else { panic!("report must be a JSON object") };
    map
}

#[test]
fn ac1_clean_workspace_exits_0_with_zero_findings() {
    let tmp = temp_dir("ac1");
    let out = tmp.join("knp1-findings.json");
    let status = Command::new(BIN)
        .args([
            "check",
            "--config",
            fix("cli/clean-ws/knp1-lint.toml").to_str().unwrap(),
            "--skip-typed",
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(
        status.status.code(),
        Some(0),
        "clean-ws must exit 0; stderr={}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert!(out.is_file(), "report must be written");

    let rep = parse_report(&out);
    assert_eq!(rep.get("schema"), Some(&CValue::Num(1.0)));
    assert_eq!(rep.get("tool"), Some(&CValue::Str("fid-knp1-lint".to_string())));
    // REQ-KRN.NUM.LINT.CLI-003: --skip-typed recorded in report as typed: skipped.
    assert_eq!(rep.get("typed"), Some(&CValue::Str("skipped".to_string())));
    assert_eq!(rep.get("findings"), Some(&CValue::Arr(Vec::new())));
    let Some(CValue::Obj(summary)) = rep.get("summary") else { panic!("missing summary") };
    assert_eq!(summary.get("findings"), Some(&CValue::Num(0.0)));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac2_dirty_workspace_exits_1_with_powi_and_hashmap_findings() {
    let tmp = temp_dir("ac2");
    let out = tmp.join("knp1-findings.json");
    let output = Command::new(BIN)
        .args([
            "check",
            "--config",
            fix("cli/dirty-ws/knp1-lint.toml").to_str().unwrap(),
            "--skip-typed",
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "dirty-ws must exit 1; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.is_file(), "report must still be written when findings exist (REQ-001)");

    let rep = parse_report(&out);
    let Some(CValue::Arr(findings)) = rep.get("findings") else { panic!("missing findings") };
    let rules: Vec<String> = findings
        .iter()
        .filter_map(|f| match f {
            CValue::Obj(o) => match o.get("rule") {
                Some(CValue::Str(r)) => Some(r.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert!(rules.iter().any(|r| r == "KNP1-POW"), "missing KNP1-POW in {rules:?}");
    assert!(rules.iter().any(|r| r == "KNP1-HASH"), "missing KNP1-HASH in {rules:?}");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac3_missing_config_exits_2_and_writes_no_report() {
    let tmp = temp_dir("ac3");
    let out = tmp.join("should-not-exist.json");
    let missing = tmp.join("no-such-config.toml");

    let output = Command::new(BIN)
        .args(["check", "--config", missing.to_str().unwrap(), "--skip-typed", "--out", out.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2), "missing config must exit 2");
    assert!(!out.exists(), "exit 2 must write no success report");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("input_missing"), "expected input_missing in stderr: {stderr}");

    // Bad CLI arguments also exit 2 and write no report.
    let bad_args = Command::new(BIN).args(["check", "--bogus"]).output().unwrap();
    assert_eq!(bad_args.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad_args.stderr).contains("config_invalid"));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac4_selftest_passes_complete_and_fails_naming_missing_rule() {
    // Complete fixtures/knp1/seeded/ -> exit 0.
    let ok = Command::new(BIN).current_dir(repo_root()).args(["selftest"]).output().unwrap();
    assert_eq!(
        ok.status.code(),
        Some(0),
        "selftest must pass on complete fixtures; stdout={} stderr={}",
        String::from_utf8_lossy(&ok.stdout),
        String::from_utf8_lossy(&ok.stderr)
    );

    // Delete KNP1-POW from a temp copy of seeded/ -> fails naming KNP1-POW.
    let tmp = temp_dir("ac4");
    let seeded_copy = tmp.join("seeded");
    copy_dir_recursive(&fix("seeded"), &seeded_copy);
    std::fs::remove_dir_all(seeded_copy.join("KNP1-POW")).unwrap();

    let fail = Command::new(BIN)
        .args([
            "selftest",
            "--seeded",
            seeded_copy.to_str().unwrap(),
            "--data-ws",
            fix("data/ws/knp1-lint.toml").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_ne!(fail.status.code(), Some(0), "selftest must fail when KNP1-POW is deleted");
    let stderr = String::from_utf8_lossy(&fail.stderr);
    assert!(stderr.contains("KNP1-POW"), "failure must name KNP1-POW; stderr={stderr}");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn ac5_identical_inputs_produce_byte_identical_reports() {
    let tmp = temp_dir("ac5");
    let out1 = tmp.join("r1.json");
    let out2 = tmp.join("r2.json");

    for out in [&out1, &out2] {
        let output = Command::new(BIN)
            .args([
                "check",
                "--config",
                fix("cli/dirty-ws/knp1-lint.toml").to_str().unwrap(),
                "--skip-typed",
                "--out",
                out.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
    }

    let b1 = std::fs::read(&out1).unwrap();
    let b2 = std::fs::read(&out2).unwrap();
    assert_eq!(b1, b2, "two runs on identical inputs must produce byte-identical reports");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn waivers_and_denylist_cli_flags_work_end_to_end() {
    let tmp = temp_dir("waivers");
    let src_bytes = std::fs::read(fix("cli/dirty-ws/crates/dirty-kernel/src/lib.rs")).unwrap();
    let pow_digest = lines_digest(&src_bytes, 4, 4).unwrap();
    let hash_digest = lines_digest(&src_bytes, 7, 8).unwrap();

    let waivers_path = tmp.join("knp1-waivers.toml");
    let waivers_toml = format!(
        r#"[[waiver]]
crate = "dirty-kernel"
file = "src/lib.rs"
lines = [4, 4]
rule = "KNP1-POW"
justification = "fixture waiver for powi"
reviewer = "reviewer-a"
date = "2026-11-21"
lines_sha256 = "{pow_digest}"

[[waiver]]
crate = "dirty-kernel"
file = "src/lib.rs"
lines = [7, 8]
rule = "KNP1-HASH"
justification = "fixture waiver for HashMap"
reviewer = "reviewer-a"
date = "2026-11-21"
lines_sha256 = "{hash_digest}"
"#
    );
    std::fs::write(&waivers_path, waivers_toml).unwrap();

    let out = tmp.join("waived-report.json");
    let output = Command::new(BIN)
        .args([
            "check",
            "--config",
            fix("cli/dirty-ws/knp1-lint.toml").to_str().unwrap(),
            "--waivers",
            waivers_path.to_str().unwrap(),
            "--denylist",
            fix("data/denylist.json").to_str().unwrap(),
            "--skip-typed",
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "all findings waived must exit 0; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rep = parse_report(&out);
    assert_eq!(rep.get("findings"), Some(&CValue::Arr(Vec::new())));
    let Some(CValue::Arr(waived)) = rep.get("waived") else { panic!("missing waived") };
    assert!(!waived.is_empty(), "expected waived findings in report");

    let _ = std::fs::remove_dir_all(&tmp);
}
