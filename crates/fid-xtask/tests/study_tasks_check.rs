//! Acceptance criteria of spec node FLOW.STUDY.PROTO (AC-1 .. AC-3), run through the
//! `fid-xtask study-tasks-check` binary. Expected findings are hand-labelled in
//! `fixtures/flow/study/tasks/expected.txt`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `fid-xtask study-tasks-check <args>` from the repository root; returns (exit code, stdout).
fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .current_dir(repo_root())
        .arg("study-tasks-check")
        .args(args)
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap().replace("\r\n", "\n"))
}

/// AC-1: the shipped task list passes: every job ID has a map and every command ID has the
/// form `domain.verb_object@N` (and the shipped protocol passes the document check).
#[test]
fn ac1_shipped_task_list_passes() {
    let got = run(&[]);
    assert_eq!(got.0, 0, "{}", got.1);
    assert_eq!(got.1, "study-tasks-check: 3 tasks\n");
    // D0: the output is identical across runs.
    assert_eq!(run(&[]), got);
}

/// AC-2: a task with job `JOB.ACC.99` exits 1 with `TASK-NO-MAP`.
#[test]
fn ac2_unmapped_job_exits_1() {
    let (code, out) = run(&["--tasks", "fixtures/flow/study/tasks/no-map.json"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.lines().any(|l| l.starts_with("TASK-NO-MAP JOB.ACC.99")), "{out}");
}

/// AC-3: the protocol must cite the record format of FLOW.STUDY.RECORD (`study-record@1`).
#[test]
fn ac3_protocol_without_record_format_exits_1() {
    let (code, out) = run(&["--protocol", "fixtures/flow/study/tasks/protocol-no-record.md"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.lines().any(|l| l.starts_with("TASK-DOC protocol") && l.contains("study-record@1")), "{out}");
}

/// A malformed command ID exits 1 with `TASK-SCHEMA`.
#[test]
fn bad_command_exits_1() {
    let (code, out) = run(&["--tasks", "fixtures/flow/study/tasks/bad-command.json"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.lines().any(|l| l.starts_with("TASK-SCHEMA JOB.ACC.02")), "{out}");
}

#[test]
fn bad_usage_exits_2() {
    assert_eq!(run(&["--nope"]).0, 2);
    assert_eq!(run(&["--tasks"]).0, 2);
}
