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

    let nightly_out = tmp_dir("nightly-out");
    let nightly = simulate(&SimOptions {
        tag: Some("v1.4.0-nightly.20261124"),
        channel_override: Some(Channel::Nightly),
        fixtures: &fixture("pass"),
        approval: None,
        prior_state: None,
        out: Some(&nightly_out),
        runbook_dir: &root().join("release"),
    })
    .unwrap();
    assert_eq!(nightly.state, FlowState::FeedsUpdated);
    assert_eq!(nightly.uploaded_count.get("github-releases").copied(), Some(3));
    assert_eq!(nightly.uploaded_count.get("s3-cdn").copied(), Some(3));
    assert_eq!(nightly.uploaded_count.get("eu-mirror").copied(), Some(0));
    assert!(nightly_out.join("venues/github-releases/fid-1.4.0-beta.1-x86_64.msi").is_file());
    assert!(nightly_out.join("venues/s3-cdn/fid-verify-1.4.0-beta.1-linux.tar.gz").is_file());
    assert!(!nightly_out.join("venues/eu-mirror").exists());

    // Nightly venue conflict also fails closed.
    let nightly_conflict = simulate(&SimOptions {
        tag: Some("v1.4.0-nightly.20261124"),
        channel_override: Some(Channel::Nightly),
        fixtures: &fixture("venue-conflict"),
        approval: None,
        prior_state: None,
        out: None,
        runbook_dir: &root().join("release"),
    })
    .unwrap();
    assert_eq!(nightly_conflict.state, FlowState::Failed);
    assert!(
        nightly_conflict.reasons.iter().any(|r| r.contains("venue_conflict(fid-1.4.0-x86_64.msi, github-releases)")),
        "{:?}",
        nightly_conflict.reasons
    );

    assert_eq!(cli(&[]).0, 2);
    assert_eq!(cli(&["--bogus"]).0, 2);
    assert_eq!(cli(&["--fixtures", "fixtures/rel/flow/no-such-dir"]).0, 2);
    assert_eq!(cli(&["--check-workflow", "--fixtures", "fixtures/rel/flow/pass"]).0, 2);
}

fn copy_dir_all(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let p = entry.path();
        let target = dst.join(entry.file_name());
        if p.is_dir() {
            copy_dir_all(&p, &target);
        } else {
            std::fs::copy(&p, &target).unwrap();
        }
    }
}

#[test]
fn resume_hardening_rejects_mismatch_digest_change_and_blocked_gates() {
    let runbook = root().join("release");

    // 1. Mismatched tag/channel in prior state -> Err (CLI exit 2).
    let mismatch = simulate(&SimOptions {
        tag: Some("v9.9.9-beta.1"),
        channel_override: None,
        fixtures: &fixture("resume-partial"),
        approval: None,
        prior_state: None,
        out: None,
        runbook_dir: &runbook,
    });
    assert!(mismatch.is_err(), "expected mismatched tag error");
    assert_eq!(
        cli(&[
            "--tag",
            "v9.9.9-beta.1",
            "--fixtures",
            "fixtures/rel/flow/pass",
            "--state",
            "fixtures/rel/flow/resume-partial/release-state.json",
        ])
        .0,
        2
    );

    // 2. Changed artefact presign_digest invalidates prior approval and returns to AwaitingApproval.
    let changed_dir = tmp_dir("resume-changed-digest");
    copy_dir_all(&fixture("resume-partial"), &changed_dir);
    std::fs::write(changed_dir.join("artefacts/fid-1.4.0-beta.1-x86_64.msi"), b"tampered-msi-bytes").unwrap();
    let changed = simulate(&SimOptions {
        tag: Some("v1.4.0-beta.1"),
        channel_override: None,
        fixtures: &changed_dir,
        approval: None,
        prior_state: None,
        out: None,
        runbook_dir: &runbook,
    })
    .unwrap();
    assert_eq!(changed.state, FlowState::AwaitingApproval);
    assert!(changed.approval_ref.is_none());

    // 3. Blocked gate verdict always stops Blocked even if prior completed_steps has gates-summary.
    let blocked_dir = tmp_dir("resume-blocked-gate");
    copy_dir_all(&fixture("block"), &blocked_dir);
    let prior_signed = std::fs::read_to_string(fixture("resume-partial/release-state.json")).unwrap();
    let prior_path = blocked_dir.join("prior.json");
    std::fs::write(&prior_path, prior_signed).unwrap();
    let blocked = simulate(&SimOptions {
        tag: Some("v1.4.0-beta.1"),
        channel_override: None,
        fixtures: &blocked_dir,
        approval: None,
        prior_state: Some(&prior_path),
        out: None,
        runbook_dir: &runbook,
    })
    .unwrap();
    assert_eq!(blocked.state, FlowState::Blocked);
    assert!(blocked.presign_digest.is_none());

    // 4. Failed prior state preserves approval when presign_digest matches and signing step already completed.
    let failed_dir = tmp_dir("resume-failed-ok");
    copy_dir_all(&fixture("pass"), &failed_dir);
    let awaiting = simulate(&SimOptions {
        tag: Some("v1.4.0-beta.1"),
        channel_override: None,
        fixtures: &failed_dir,
        approval: None,
        prior_state: None,
        out: None,
        runbook_dir: &runbook,
    })
    .unwrap();
    let matching_presign = awaiting.presign_digest.unwrap();
    let failed_state = format!(
        "{{\"approval_ref\":\"approval-prior-failed\",\"channel\":\"beta\",\"presign_digest\":\"{matching_presign}\",\"published_venues\":{{}},\"reasons\":[\"transient_error\"],\"state\":\"failed\",\"steps\":[{{\"id\":\"os-sign\",\"outputs_sha256\":\"sha256:0000000000000000000000000000000000000000000000000000000000000001\",\"status\":\"completed\"}},{{\"id\":\"sign-statements\",\"outputs_sha256\":\"sha256:0000000000000000000000000000000000000000000000000000000000000002\",\"status\":\"completed\"}}],\"tag\":\"v1.4.0-beta.1\"}}"
    );
    let failed_state_path = failed_dir.join("failed-state.json");
    std::fs::write(&failed_state_path, failed_state).unwrap();
    let resumed = simulate(&SimOptions {
        tag: Some("v1.4.0-beta.1"),
        channel_override: None,
        fixtures: &failed_dir,
        approval: None,
        prior_state: Some(&failed_state_path),
        out: None,
        runbook_dir: &runbook,
    })
    .unwrap();
    assert_eq!(resumed.state, FlowState::FeedsUpdated);
    assert_eq!(resumed.approval_ref.as_deref(), Some("approval-prior-failed"));
}

#[test]
fn check_workflow_catches_inline_triggers_bracket_secrets_top_env_and_inherit() {
    let base_wf = std::fs::read_to_string(root().join("release/release-workflow.yml")).unwrap();
    let runbook = root().join("release");
    let dir = tmp_dir("wf-adv");
    std::fs::create_dir_all(&dir).unwrap();

    // 1. Single-line `on: pull_request`
    let wf1 = base_wf.replacen("\non:\n", "\non: pull_request\n", 1);
    let p1 = dir.join("wf1.yml");
    std::fs::write(&p1, wf1).unwrap();
    let r1 = check_workflow(&p1, &runbook).unwrap();
    assert!(r1.findings.iter().any(|f| f.code == "FLOW-FORK-TRIGGER"), "{:?}", r1.findings);

    // 2. Inline flow sequence `on: [push, pull_request_target]`
    let wf2 = base_wf.replacen("\non:\n", "\non: [push, pull_request_target]\n", 1);
    let p2 = dir.join("wf2.yml");
    std::fs::write(&p2, wf2).unwrap();
    let r2 = check_workflow(&p2, &runbook).unwrap();
    assert!(r2.findings.iter().any(|f| f.code == "FLOW-FORK-TRIGGER"), "{:?}", r2.findings);

    // 3. `secrets['KEY']` in pre-approval job
    let wf3 = base_wf.replacen(
        "echo \"gate=plat-matrix step=platform-gate\"",
        "echo \"${{ secrets['RELEASE_SIGNING_KEY'] }}\"",
        1,
    );
    let p3 = dir.join("wf3.yml");
    std::fs::write(&p3, wf3).unwrap();
    let r3 = check_workflow(&p3, &runbook).unwrap();
    assert!(
        r3.findings.iter().any(|f| f.code == "FLOW-PREAPPROVAL-SECRET" && f.subject == "platform-gate"),
        "{:?}",
        r3.findings
    );

    // 4. Top-level `env:` referencing `secrets.KEY`
    let wf4 = base_wf.replacen(
        "env:\n  CARGO_TERM_COLOR: never",
        "env:\n  LEAK: ${{ secrets.RELEASE_SIGNING_KEY }}\n  CARGO_TERM_COLOR: never",
        1,
    );
    let p4 = dir.join("wf4.yml");
    std::fs::write(&p4, wf4).unwrap();
    let r4 = check_workflow(&p4, &runbook).unwrap();
    assert!(r4.findings.iter().any(|f| f.code == "FLOW-PREAPPROVAL-SECRET" && f.subject == "env"), "{:?}", r4.findings);

    // 5. `secrets: inherit` in pre-approval job
    let wf5 = base_wf.replacen("echo \"gate=plat-matrix step=platform-gate\"", "secrets: inherit", 1);
    let p5 = dir.join("wf5.yml");
    std::fs::write(&p5, wf5).unwrap();
    let r5 = check_workflow(&p5, &runbook).unwrap();
    assert!(
        r5.findings.iter().any(|f| f.code == "FLOW-PREAPPROVAL-SECRET" && f.subject == "platform-gate"),
        "{:?}",
        r5.findings
    );

    // 6. Deduplicated FLOW-PREAPPROVAL-ENV for `environment: release-signing`
    let r_env = check_workflow(&fixture("bad-workflows/pre-approval-env.yml"), &runbook).unwrap();
    let env_count = r_env.findings.iter().filter(|f| f.code == "FLOW-PREAPPROVAL-ENV").count();
    assert_eq!(env_count, 1, "{:?}", r_env.findings);
}
