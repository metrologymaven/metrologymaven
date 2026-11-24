//! Acceptance criteria of spec node ECO.REL.PROC.FLOW (AC-1 .. AC-7).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::rel::flow::{check_workflow, simulate, FlowState, SimOptions};
use fid_xtask::rel::runbook;
use fid_xtask::rel::version::Channel;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(rel: &str) -> PathBuf {
    root().join("fixtures/rel/flow").join(rel)
}

fn tmp_dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("rel-flow").join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn cli(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .current_dir(root())
        .args(["rel", "flow-sim"])
        .args(args)
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}

/// AC-1: passing gates stop at `awaiting_approval` and record `presign_digest`.
#[test]
fn ac1_pass_stops_at_awaiting_approval_with_presign_digest() {
    let out_dir = tmp_dir("ac1");
    let out_s = out_dir.to_string_lossy().into_owned();
    let (code, stdout, stderr) =
        cli(&["--tag", "v1.4.0-beta.1", "--fixtures", "fixtures/rel/flow/pass", "--out", &out_s, "--json"]);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("\"state\":\"awaiting_approval\""), "{stdout}");
    assert!(stdout.contains("\"presign_digest\":\"sha256:"), "{stdout}");
    // No post-approval steps ran.
    for post in ["os-sign", "sums", "sign-statements", "publish", "feeds", "rebuild-compare"] {
        assert!(!stdout.contains(&format!("\"id\":\"{post}\"")), "{post} should not start before approval");
    }
    let written = std::fs::read_to_string(out_dir.join("release-state.json")).unwrap();
    assert_eq!(written.trim(), stdout.trim());
}

/// AC-2: resuming with `approval-ok.json` reaches `feeds_updated` and publishes only to enabled venues.
#[test]
fn ac2_approval_ok_reaches_feeds_updated_and_respects_disabled_venues() {
    let out_dir = tmp_dir("ac2");
    let out_s = out_dir.to_string_lossy().into_owned();
    let (code, stdout, stderr) = cli(&[
        "--tag",
        "v1.4.0-beta.1",
        "--fixtures",
        "fixtures/rel/flow/pass",
        "--approval",
        "fixtures/rel/flow/approval-ok.json",
        "--out",
        &out_s,
        "--json",
    ]);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("\"state\":\"feeds_updated\""), "{stdout}");
    assert!(stdout.contains("\"approval_ref\":\"approval-beta-1.4.0-1\""), "{stdout}");
    // Enabled venues received files; disabled venue `eu-mirror` did not.
    assert!(out_dir.join("venues/github-releases/fid-1.4.0-beta.1-x86_64.msi").is_file());
    assert!(out_dir.join("venues/s3-cdn/fid-verify-1.4.0-beta.1-linux.tar.gz").is_file());
    assert!(!out_dir.join("venues/eu-mirror").exists(), "disabled eu-mirror should receive no uploads");
}

/// AC-3: gate verdict `block` stops at `blocked` with zero approval request.
#[test]
fn ac3_block_gate_stops_blocked() {
    let out_dir = tmp_dir("ac3");
    let out_s = out_dir.to_string_lossy().into_owned();
    let (code, stdout, _) =
        cli(&["--tag", "v1.4.0-beta.1", "--fixtures", "fixtures/rel/flow/block", "--out", &out_s, "--json"]);
    assert_eq!(code, 1);
    assert!(stdout.contains("\"state\":\"blocked\""), "{stdout}");
    assert!(stdout.contains("\"presign_digest\":null"), "{stdout}");
    assert!(stdout.contains("gate_blocked:qual-evid"), "{stdout}");
}

/// AC-4: approval naming a mismatched digest stays `awaiting_approval` with `approval_digest_mismatch`.
#[test]
fn ac4_approval_wrong_digest_stays_awaiting_approval() {
    let (code, stdout, _) = cli(&[
        "--tag",
        "v1.4.0-beta.1",
        "--fixtures",
        "fixtures/rel/flow/pass",
        "--approval",
        "fixtures/rel/flow/approval-wrong.json",
        "--json",
    ]);
    assert_eq!(code, 1);
    assert!(stdout.contains("\"state\":\"awaiting_approval\""), "{stdout}");
    assert!(stdout.contains("\"approval_digest_mismatch\""), "{stdout}");
    assert!(!stdout.contains("\"id\":\"os-sign\""));
}

/// AC-5: partial publish resume skips venue already holding matching `(name, sha256)` and uploads second venue.
#[test]
fn ac5_resume_skips_matching_venue_and_uploads_remaining() {
    let out_dir = tmp_dir("ac5");
    let res = simulate(&SimOptions {
        tag: Some("v1.4.0-beta.1"),
        channel_override: None,
        fixtures: &fixture("resume-partial"),
        approval: None,
        prior_state: None,
        out: Some(&out_dir),
        runbook_dir: &root().join("release"),
    })
    .unwrap();
    assert_eq!(res.state, FlowState::FeedsUpdated);
    assert_eq!(res.uploaded_count.get("venue-a").copied(), Some(0));
    assert_eq!(res.uploaded_count.get("venue-b").copied(), Some(2));
    assert!(!out_dir.join("venues/venue-a").exists());
    assert!(out_dir.join("venues/venue-b/fid-1.4.0-beta.1-x86_64.msi").is_file());
    assert!(out_dir.join("venues/venue-b/fid-1.4.0-beta.1-x86_64.msi.spdx.json").is_file());
}

/// AC-6: same-name different-SHA-256 at venue stops `failed` naming file and venue.
#[test]
fn ac6_venue_conflict_fails_naming_file_and_venue() {
    let (code, stdout, _) = cli(&[
        "--tag",
        "v1.4.0",
        "--fixtures",
        "fixtures/rel/flow/venue-conflict",
        "--approval",
        "fixtures/rel/flow/approval-conflict.json",
        "--json",
    ]);
    assert_eq!(code, 1);
    assert!(stdout.contains("\"state\":\"failed\""), "{stdout}");
    assert!(stdout.contains("venue_conflict(fid-1.4.0-x86_64.msi, github-releases)"), "{stdout}");
}

/// AC-7: real `release/release-workflow.yml` passes runbook alignment and secrets-scope check,
/// while negative workflow fixtures fail cleanly.
#[test]
fn ac7_real_workflow_and_negative_fixtures() {
    let report = check_workflow(&root().join("release/release-workflow.yml"), &root().join("release")).unwrap();
    assert!(report.is_ok(), "findings={:?}", report.findings);
    assert_eq!(report.jobs.len(), 22);
    assert_eq!(report.pre_approval_jobs.len(), 16);
    assert_eq!(report.post_approval_jobs.len(), 6);

    // Runbook consistency check on real `release/` directory also passes.
    assert!(runbook::check(&root().join("release"), None).is_ok());

    let (code, stdout, stderr) =
        cli(&["--check-workflow", "--workflow", "release/release-workflow.yml", "--runbook", "release", "--json"]);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("\"ok\":true"), "{stdout}");

    for (file, expected_code) in [
        ("bad-workflows/pre-approval-secret.yml", "FLOW-PREAPPROVAL-SECRET"),
        ("bad-workflows/pre-approval-env.yml", "FLOW-PREAPPROVAL-ENV"),
        ("bad-workflows/fork-trigger.yml", "FLOW-FORK-TRIGGER"),
    ] {
        let rep = check_workflow(&fixture(file), &root().join("release")).unwrap();
        assert!(!rep.is_ok(), "{file} unexpectedly passed");
        assert!(
            rep.findings.iter().any(|f| f.code == expected_code),
            "{file}: expected {expected_code}, got {:?}",
            rep.findings
        );
    }
}

/// D0 determinism, nightly auto-sign path (no approval step), and bad usage exit 2.
#[test]
fn deterministic_nightly_and_usage_errors() {
    let a = simulate(&SimOptions {
        tag: Some("v1.4.0-beta.1"),
        channel_override: None,
        fixtures: &fixture("pass"),
        approval: Some(&fixture("approval-ok.json")),
        prior_state: None,
        out: None,
        runbook_dir: &root().join("release"),
    })
    .unwrap();
    let b = simulate(&SimOptions {
        tag: Some("v1.4.0-beta.1"),
        channel_override: None,
        fixtures: &fixture("pass"),
        approval: Some(&fixture("approval-ok.json")),
        prior_state: None,
        out: None,
        runbook_dir: &root().join("release"),
    })
    .unwrap();
    assert_eq!(a.to_canonical_json(), b.to_canonical_json());

    let nightly = simulate(&SimOptions {
        tag: Some("v1.4.0-nightly.20261124"),
        channel_override: Some(Channel::Nightly),
        fixtures: &fixture("pass"),
        approval: None,
        prior_state: None,
        out: None,
        runbook_dir: &root().join("release"),
    })
    .unwrap();
    assert_eq!(nightly.state, FlowState::FeedsUpdated);

    assert_eq!(cli(&[]).0, 2);
    assert_eq!(cli(&["--bogus"]).0, 2);
    assert_eq!(cli(&["--fixtures", "fixtures/rel/flow/no-such-dir"]).0, 2);
}
