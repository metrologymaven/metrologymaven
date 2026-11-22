#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Acceptance criteria of spec node AGENT.HARNESS.SCORE (AC-1 .. AC-5) and REQ-001..REQ-007.
//!
//! Expected verdicts of the archived runs are hand-labelled in
//! `fixtures/agent/harness/runs/make_runs.py` (`score-expected.json`).

use fid_canon::{parse, CValue};
use fid_eval_harness::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn runs_dir() -> PathBuf {
    Path::new(ROOT).join("fixtures/agent/harness/runs")
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-eval-score-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dst = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dst);
        } else {
            std::fs::copy(e.path(), &dst).unwrap();
        }
    }
}

fn copy_run(name: &str, tag: &str) -> PathBuf {
    let dst = tmp(tag).join(name);
    copy_dir(&runs_dir().join(name), &dst);
    dst
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fid-eval-harness")).args(args).output().unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("expected object, got {other:?}"),
    }
}

#[test]
fn ac1_pass_complete_succeeds_and_is_adoptable() {
    let run = copy_run("pass-complete", "ac1");
    let out = run_cli(&["score", run.to_str().unwrap(), "--json"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let line = String::from_utf8(out.stdout).unwrap();
    assert_eq!(line.lines().count(), 1);
    let parsed = parse(line.trim_end().as_bytes()).unwrap();
    let m = obj(&parsed);

    assert_eq!(m.get("format"), Some(&CValue::Str(SCORE_FORMAT.into())));
    assert_eq!(m.get("run_id"), Some(&CValue::Str("pass-complete".into())));
    assert_eq!(m.get("branch"), Some(&CValue::Str("agent/run-1".into())));
    assert_eq!(m.get("success"), Some(&CValue::Bool(true)));
    assert_eq!(m.get("adoptable"), Some(&CValue::Bool(true)));
    assert_eq!(m.get("adoptable_reasons"), Some(&CValue::Arr(Vec::new())));
    assert_eq!(m.get("forbidden_hits"), Some(&CValue::Arr(Vec::new())));

    let score_file = std::fs::read(run.join("artifacts/score.json")).unwrap();
    assert_eq!(score_file, line.trim_end().as_bytes());
}

#[test]
fn ac2_value_off_by_2e_9_under_default_1e_9_tolerance_fails() {
    let run = copy_run("value-off", "ac2");
    let score = score_run(&run, &ScoreOptions::default()).unwrap();
    assert!(!score.success);
    assert!(!score.adoptable);
    assert_eq!(score.items.len(), 1);
    assert!(!score.items[0].pass);
    assert_eq!(score.items[0].reason.as_deref(), Some("value_out_of_tolerance"));
    assert_eq!(score.adoptable_reasons, vec![AdoptableReason::TaskFailed]);
}

#[test]
fn ac3_mismatch_succeeds_but_not_adoptable_due_to_claim_mismatch() {
    let run = copy_run("mismatch", "ac3");
    let score = score_run(&run, &ScoreOptions::default()).unwrap();
    assert!(score.success);
    assert!(!score.adoptable);
    assert_eq!(score.adoptable_reasons, vec![AdoptableReason::ClaimMismatch]);
}

#[test]
fn ac4_forbidden_outcome_fails_task_even_when_items_pass() {
    let run = copy_run("forbidden", "ac4");
    let score = score_run(&run, &ScoreOptions::default()).unwrap();
    assert!(score.items.iter().all(|it| it.pass));
    assert_eq!(score.forbidden_hits.len(), 1);
    assert_eq!(score.forbidden_hits[0].characteristic, "C2");
    assert_eq!(score.forbidden_hits[0].outcome, Outcome::Conforms);
    assert!(!score.success);
    assert!(!score.adoptable);
}

#[test]
fn ac5_rescoring_archived_run_is_byte_identical() {
    let run = copy_run("pass-complete", "ac5");
    let out1 = run_cli(&["score", run.to_str().unwrap(), "--json"]);
    assert!(out1.status.success(), "{}", String::from_utf8_lossy(&out1.stderr));
    let bytes1 = std::fs::read(run.join("artifacts/score.json")).unwrap();

    let out2 = run_cli(&["score", run.to_str().unwrap(), "--json"]);
    assert!(out2.status.success(), "{}", String::from_utf8_lossy(&out2.stderr));
    let bytes2 = std::fs::read(run.join("artifacts/score.json")).unwrap();

    assert_eq!(bytes1, bytes2);
    assert_eq!(out1.stdout, out2.stdout);
}

#[test]
fn every_score_fixture_matches_hand_labelled_score_expected() {
    let expected_raw = std::fs::read(runs_dir().join("score-expected.json")).unwrap();
    let expected = parse(&expected_raw).unwrap();
    let runs = obj(obj(&expected).get("runs").unwrap());
    assert!(runs.len() >= 6);

    for (name, want_val) in runs {
        let want = obj(want_val);
        let run = copy_run(name, &format!("hand-{name}"));
        let score = score_run(&run, &ScoreOptions::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let got_val = score.to_cvalue();
        let got = obj(&got_val);

        assert_eq!(got.get("branch"), want.get("branch"), "{name} branch");
        assert_eq!(got.get("success"), want.get("success"), "{name} success");
        assert_eq!(got.get("adoptable"), want.get("adoptable"), "{name} adoptable");
        assert_eq!(got.get("adoptable_reasons"), want.get("adoptable_reasons"), "{name} adoptable_reasons");
        assert_eq!(got.get("forbidden_hits"), want.get("forbidden_hits"), "{name} forbidden_hits");
    }
}

#[test]
fn req005_safety_violation_and_non_complete_brief_prevent_adoptability() {
    // Take `stray-file` (which has 1 committed out_of_root safety violation) and add passing score artifacts.
    let run = copy_run("stray-file", "safety-blocks");
    copy_score_artifacts(&runs_dir().join("pass-complete"), &run);
    let score = score_run(&run, &ScoreOptions::default()).unwrap();
    assert!(score.success);
    assert_eq!(score.safety_violations, 1);
    assert!(!score.adoptable);
    assert_eq!(score.adoptable_reasons, vec![AdoptableReason::SafetyViolation]);

    // Stale brief prevents adoptability.
    let stale_run = copy_run("pass-complete", "stale-brief");
    std::fs::remove_file(stale_run.join("artifacts/score-inputs.json")).unwrap();
    std::fs::write(stale_run.join("artifacts/show-brief.json"), b"{\"verdict\":\"stale\",\"mismatches\":[]}").unwrap();
    let stale_score = score_run(&stale_run, &ScoreOptions::default()).unwrap();
    assert!(stale_score.success);
    assert!(!stale_score.adoptable);
    assert_eq!(stale_score.adoptable_reasons, vec![AdoptableReason::BriefStale]);
}

fn copy_score_artifacts(from_run: &Path, to_run: &Path) {
    for name in ["oracle.json", "branch-diff.json", "show-brief.json", "score-inputs.json"] {
        std::fs::copy(from_run.join("artifacts").join(name), to_run.join("artifacts").join(name)).unwrap();
    }
}

#[test]
fn req007_first_scoring_calls_fid_and_caches_digests_rescore_never_calls_fid() {
    let run = copy_run("pass-complete", "req007");
    let diff_raw = std::fs::read(run.join("artifacts/branch-diff.json")).unwrap();
    let brief_raw = std::fs::read(run.join("artifacts/show-brief.json")).unwrap();
    // Remove stored outputs so first scoring must invoke the stub `fid` binary.
    std::fs::remove_file(run.join("artifacts/branch-diff.json")).unwrap();
    std::fs::remove_file(run.join("artifacts/show-brief.json")).unwrap();
    std::fs::remove_file(run.join("artifacts/score-inputs.json")).unwrap();
    std::fs::create_dir_all(run.join("project")).unwrap();

    // Missing artifacts without `--fid` returns ArtifactMissing.
    let err = score_run(&run, &ScoreOptions::default()).unwrap_err();
    assert_eq!(err.code, ErrorCode::ArtifactMissing);

    struct CountingFidProvider {
        diff_raw: Vec<u8>,
        brief_raw: Vec<u8>,
        calls: Vec<Vec<String>>,
        forbid_calls: bool,
    }
    impl fid_eval_harness::FidProvider for CountingFidProvider {
        fn diff_branch_json(
            &mut self,
            branch: &str,
            project_dir: &Path,
        ) -> Result<Vec<u8>, fid_eval_harness::SandboxError> {
            assert!(!self.forbid_calls, "re-scoring must not call diff_branch_json");
            let args = fid_diff_branch_args(branch, project_dir);
            assert_eq!(args[0], "journal");
            assert_eq!(args[1], "diff-branch");
            assert_eq!(args[2], "--branch");
            assert_eq!(args[3], "agent/run-1");
            assert_eq!(args[4], "--project");
            assert_eq!(args[6], "--json");
            self.calls.push(args);
            Ok(self.diff_raw.clone())
        }
        fn show_brief_json(
            &mut self,
            branch: &str,
            project_dir: &Path,
        ) -> Result<Vec<u8>, fid_eval_harness::SandboxError> {
            assert!(!self.forbid_calls, "re-scoring must not call show_brief_json");
            let args = fid_show_brief_args(branch, project_dir);
            assert_eq!(args[0], "agent");
            assert_eq!(args[1], "show-brief");
            assert_eq!(args[2], "--branch");
            assert_eq!(args[3], "agent/run-1");
            assert_eq!(args[4], "--project");
            assert_eq!(args[6], "--json");
            self.calls.push(args);
            Ok(self.brief_raw.clone())
        }
    }

    // First scoring calls the provider twice (diff-branch + show-brief) and caches artifacts + digests.
    let opts = ScoreOptions { write_artifacts: true };
    let mut provider = CountingFidProvider { diff_raw, brief_raw, calls: Vec::new(), forbid_calls: false };
    let first = score_run_with_provider(&run, &opts, Some(&mut provider)).unwrap();
    assert!(first.success && first.adoptable);
    assert!(run.join("artifacts/branch-diff.json").is_file());
    assert!(run.join("artifacts/show-brief.json").is_file());
    assert!(run.join("artifacts/score-inputs.json").is_file());
    assert_eq!(provider.calls.len(), 2);

    // Forbid further provider calls: re-scoring must use stored artifacts and be byte-identical.
    provider.forbid_calls = true;
    let second = score_run_with_provider(&run, &opts, Some(&mut provider)).unwrap();
    assert_eq!(first.to_json().unwrap(), second.to_json().unwrap());
    assert_eq!(provider.calls.len(), 2, "re-scoring must not call fid again");

    // Re-scoring with no provider at all also succeeds and matches byte-for-byte.
    let third = score_run(&run, &opts).unwrap();
    assert_eq!(first.to_json().unwrap(), third.to_json().unwrap());

    // Tampering with stored branch-diff.json fails closed against score-inputs.json digest.
    std::fs::write(run.join("artifacts/branch-diff.json"), b"{\"branch\":\"agent/run-1\",\"result_records\":[]}")
        .unwrap();
    let tamper_err = score_run(&run, &ScoreOptions::default()).unwrap_err();
    assert_eq!(tamper_err.code, ErrorCode::InvalidArtifact);
}
