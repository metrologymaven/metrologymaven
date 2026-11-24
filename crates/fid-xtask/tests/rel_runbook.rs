//! Acceptance criteria of spec node ECO.REL.PROC.RUNBOOK (AC-1 .. AC-6). The fixtures under
//! `fixtures/rel/runbook/` are hand written; their `RUNBOOK.md` files (and the real
//! `release/RUNBOOK.md`) are rendered by `fixtures/rel/runbook/make_fixtures.py`, an
//! independent Python reference, so these tests also check the Rust renderer against it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::rel::runbook::{check, parse_runbook, render_dir, Code, Performer, Status, SIGN_APPROVAL};
use fid_xtask::rel::SUBCOMMANDS;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/rel/runbook").join(name)
}

/// Runs `fid-xtask rel <args>` from the workspace root (what `cargo xtask rel ...` runs);
/// returns (exit code, stdout).
fn cli(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask")).current_dir(root()).arg("rel").args(args).output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

/// The findings of a failing case as (code, subject).
fn findings(case: &str) -> Vec<(Code, String)> {
    check(&fixture(case), None).unwrap_err().into_iter().map(|f| (f.code, f.subject)).collect()
}

/// AC-1: the consistent fixture passes with the exact JSON line.
#[test]
fn ac1_ok_fixture_passes() {
    let (code, out) = cli(&["runbook-check", "--dir", "fixtures/rel/runbook/ok", "--json"]);
    assert_eq!((code, out.as_str()), (0, "{\"ok\":true,\"steps\":6}\n"));
    let (code, out) = cli(&["runbook-check", "--dir", "fixtures/rel/runbook/ok"]);
    assert_eq!((code, out.as_str()), (0, "runbook-check: ok 6 steps\n"));
}

/// AC-2: `publish` performed by `role:maintainer` is a second human step.
#[test]
fn ac2_two_humans() {
    let (code, out) = cli(&["runbook-check", "--dir", "fixtures/rel/runbook/two-humans"]);
    assert_eq!(code, 1);
    assert!(out.starts_with("RUNBOOK-HUMAN-STEP publish: "), "{out}");
    assert_eq!(findings("two-humans"), vec![(Code::HumanStep, "publish".to_string())]);
}

/// AC-3: workflow job `upload-extra` is named by no step.
#[test]
fn ac3_orphan_job() {
    let (code, out) = cli(&["runbook-check", "--dir", "fixtures/rel/runbook/orphan-job"]);
    assert_eq!(code, 1);
    assert!(out.starts_with("RUNBOOK-ORPHAN-JOB upload-extra: "), "{out}");
    assert_eq!(findings("orphan-job"), vec![(Code::OrphanJob, "upload-extra".to_string())]);
}

/// AC-4: step `redteam-verdict` lists gate `agent-redteam`, which gates.toml lacks.
#[test]
fn ac4_missing_gate() {
    let (code, out) = cli(&["runbook-check", "--dir", "fixtures/rel/runbook/missing-gate"]);
    assert_eq!(code, 1);
    assert!(out.starts_with("RUNBOOK-UNKNOWN-GATE agent-redteam: "), "{out}");
    assert_eq!(findings("missing-gate"), vec![(Code::UnknownGate, "agent-redteam".to_string())]);
}

/// AC-5: the ok fixture with one hand edit in RUNBOOK.md drifts; the JSON report says so.
#[test]
fn ac5_drift() {
    let (code, out) = cli(&["runbook-check", "--dir", "fixtures/rel/runbook/drift"]);
    assert_eq!(code, 1);
    assert!(out.starts_with("RUNBOOK-DRIFT RUNBOOK.md: "), "{out}");
    assert_eq!(findings("drift"), vec![(Code::Drift, "RUNBOOK.md".to_string())]);
    let (code, out) = cli(&["runbook-check", "--dir", "fixtures/rel/runbook/drift", "--json"]);
    assert_eq!(code, 1);
    assert!(out.starts_with("{\"findings\":[{\"code\":\"RUNBOOK-DRIFT\",\"message\":"), "{out}");
    assert!(out.ends_with(",\"subject\":\"RUNBOOK.md\"}],\"ok\":false}\n"), "{out}");
}

/// AC-6: the real release directory passes, and the four QUAL/red-team steps precede
/// sign-approval for beta, stable and lts with their gates.
#[test]
fn ac6_real_release_dir() {
    let (code, out) = cli(&["runbook-check", "--dir", "release"]);
    assert_eq!(code, 0, "{out}");
    let text = std::fs::read_to_string(root().join("release/runbook.toml")).unwrap();
    let steps = parse_runbook(&text).unwrap();
    let pos = |id: &str| steps.iter().position(|s| s.id == id).unwrap();
    let approval = pos(SIGN_APPROVAL);
    assert_eq!(steps[approval].performer, Performer::Role("release-approver".into()));
    for (id, gate) in [
        ("evidence-complete", "qual-evid"),
        ("threat-review-minor", "qual-threat"),
        ("eu-venue-gate", "qual-cra"),
        ("redteam-verdict", "agent-redteam"),
    ] {
        let s = &steps[pos(id)];
        assert!(pos(id) < approval, "{id} must precede {SIGN_APPROVAL}");
        assert_eq!(s.gates, vec![gate.to_string()], "{id}");
        for c in ["beta", "stable", "lts"] {
            assert!(s.channels.iter().any(|x| x == c), "{id} lacks {c}");
        }
    }
    // Only steps whose tool exists today are `available`; flipping a planned step fails here.
    let available: Vec<&str> = steps.iter().filter(|s| s.status == Status::Available).map(|s| s.id.as_str()).collect();
    assert_eq!(available, ["tag-validate", "sbom", "notes", "sums"]);
    // The spec's initial order (pinned here, not by runbook-check, so fixtures may differ).
    let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "tag-validate",
            "build-packages",
            "build-web",
            "sbom",
            "vuln-gate",
            "platform-gate",
            "evidence-complete",
            "threat-review-minor",
            "eu-venue-gate",
            "redteam-verdict",
            "ga-gate",
            "i18n-gate",
            "kernel-patch-gate",
            "lts-gate",
            "gates-summary",
            "notes",
            "sign-approval",
            "os-sign",
            "sums",
            "sign-statements",
            "publish",
            "feeds",
            "rebuild-compare",
        ]
    );
}

/// `runbook-render` prints exactly the committed RUNBOOK.md (rendered by the Python
/// reference) and is deterministic.
#[test]
fn render_matches_python_reference() {
    for case in ["ok", "two-humans", "orphan-job", "missing-gate"] {
        let expected = std::fs::read(fixture(case).join("RUNBOOK.md")).unwrap();
        let (_, text) = render_dir(&fixture(case)).unwrap();
        assert_eq!(text.as_bytes(), expected.as_slice(), "{case}");
    }
    let (code, out) = cli(&["runbook-render", "--dir", "release"]);
    assert_eq!(code, 0);
    let expected = std::fs::read_to_string(root().join("release/RUNBOOK.md")).unwrap().replace("\r\n", "\n");
    assert_eq!(out, expected);
    assert_eq!(cli(&["runbook-render", "--dir", "release"]).1, out);
}

/// Every subcommand runbook-check accepts as a tool is one the binary really dispatches.
#[test]
fn subcommands_match_usage() {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask")).output().unwrap();
    let usage = String::from_utf8(out.stderr).unwrap();
    for sub in SUBCOMMANDS {
        assert!(usage.contains(&format!("cargo xtask rel {sub}")), "{sub} missing from usage");
    }
}

/// The check reads files only: running it leaves the release directory unchanged.
#[test]
fn check_is_read_only() {
    let listing = |d: &Path| {
        let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap())
            })
            .collect();
        v.sort();
        v
    };
    let before = listing(&fixture("drift"));
    let _ = cli(&["runbook-check", "--dir", "fixtures/rel/runbook/drift", "--json"]);
    assert_eq!(listing(&fixture("drift")), before);
}

/// An explicit `--workflow` is used instead of the default, and a missing or broken input
/// is RUNBOOK-PARSE with exit 2.
#[test]
fn workflow_option_and_parse_errors() {
    let (code, out) = cli(&[
        "runbook-check",
        "--dir",
        "fixtures/rel/runbook/ok",
        "--workflow",
        "fixtures/rel/runbook/orphan-job/release-workflow.yml",
    ]);
    assert_eq!(code, 1);
    assert!(out.starts_with("RUNBOOK-ORPHAN-JOB upload-extra: "), "{out}");
    let (code, out) =
        cli(&["runbook-check", "--dir", "fixtures/rel/runbook/ok", "--workflow", "fixtures/rel/runbook/ok/gates.toml"]);
    assert_eq!(code, 2);
    assert!(out.starts_with("RUNBOOK-PARSE gates.toml: "), "{out}");
    let (code, out) = cli(&["runbook-check", "--dir", "fixtures/rel/runbook/no-such-dir"]);
    assert_eq!(code, 2);
    assert!(out.starts_with("RUNBOOK-PARSE runbook.toml: "), "{out}");
    let (code, _) = cli(&["runbook-check", "--bogus"]);
    assert_eq!(code, 2);
}
