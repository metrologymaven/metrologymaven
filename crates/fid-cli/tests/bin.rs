//! The `fid` binary keeps REQ-AUTO.CLI.OUT-001 on its own error paths: under `--json`,
//! usage and input errors are still exactly one envelope on stdout, with the table's
//! exit code (2 usage, 4 failed).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_surface::{Envelope, ErrorCode, Outcome};
use std::io::Write;
use std::process::{Command, Stdio};

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

fn fid(args: &[&str], stdin: &[u8]) -> Ran {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fid"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The child may exit before reading stdin (usage errors), so a broken pipe is fine.
    let _ = child.stdin.take().unwrap().write_all(stdin);
    let out = child.wait_with_output().unwrap();
    Ran {
        code: out.status.code().unwrap(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

fn one_envelope(stdout: &str) -> Envelope {
    assert!(stdout.ends_with('\n') && stdout.matches('\n').count() == 1, "not one line: {stdout:?}");
    let env = Envelope::decode(stdout.trim_end_matches('\n').as_bytes()).unwrap();
    env.validate().unwrap();
    env
}

fn failed_with(r: &Ran, code: ErrorCode, exit: i32) {
    assert_eq!(r.code, exit, "stderr: {}", r.stderr);
    let env = one_envelope(&r.stdout);
    assert_eq!(env.outcome, Outcome::Failed);
    assert_eq!(env.error.unwrap().code, code);
}

#[test]
fn json_unknown_command_is_usage_envelope() {
    failed_with(&fid(&["nope", "--json"], b""), ErrorCode::UsageError, 2);
}

#[test]
fn json_canon_usage_errors_are_envelopes() {
    failed_with(&fid(&["canon", "--bogus", "--json"], b"{}"), ErrorCode::UsageError, 2);
    failed_with(&fid(&["canon", "--json", "a.json", "b.json"], b""), ErrorCode::UsageError, 2);
    // `--json` after the bad option still counts.
    failed_with(&fid(&["canon", "--json", "--bogus"], b"{}"), ErrorCode::UsageError, 2);
}

#[test]
fn json_canon_missing_file_is_failed_envelope() {
    let missing = std::env::temp_dir().join("fid-cli-out-test-does-not-exist.json");
    failed_with(&fid(&["canon", missing.to_str().unwrap(), "--json"], b""), ErrorCode::NotFound, 4);
}

#[test]
fn json_canon_ok_and_invalid() {
    let r = fid(&["canon", "--json"], br#"{"b":1}"#);
    assert_eq!(r.code, 0);
    assert_eq!(one_envelope(&r.stdout).outcome, Outcome::Computed);
    failed_with(&fid(&["canon", "--json"], br#"{"a":1,"a":2}"#), ErrorCode::ValidationError, 4);
}

#[test]
fn plain_errors_keep_stdout_empty() {
    let r = fid(&["canon", "--bogus"], b"");
    assert_eq!(r.code, 2);
    assert_eq!(r.stdout, "");
    let r = fid(&["nope"], b"");
    assert_eq!(r.code, 2);
    assert_eq!(r.stdout, "");
}
