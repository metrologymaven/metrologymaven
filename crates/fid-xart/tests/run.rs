//! `fid-xart run`: the call set through native test artifacts gives the rows the Python oracle
//! derived (AC-1, AC-2 inputs), and every way an artifact can fail to answer is an error, never
//! a skipped row (fail closed).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use fid_kci::kci::sha256_digest;
use fid_xart::artifact::{classify, run_call_set, Answer, Artifact, CommandArtifact, TableMeta};
use fid_xart::calls::{parse_calls, CallSet};
use fid_xart::table::{DigestTable, Kind};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/xart-calls-v1")
}

fn call_set() -> CallSet {
    parse_calls(&std::fs::read(fixtures().join("calls.txt")).unwrap()).unwrap()
}

fn reference(file: &str) -> DigestTable {
    DigestTable::parse(&std::fs::read(fixtures().join(file)).unwrap()).unwrap()
}

fn meta(kind: Kind) -> TableMeta {
    TableMeta {
        artifact: "test-artifact".to_string(),
        artifact_sha256: sha256_digest(b"test-artifact"),
        host: "linux-x86_64".to_string(),
        kind,
    }
}

fn command(bin: &str) -> CommandArtifact {
    CommandArtifact { program: bin.to_string(), args: Vec::new(), timeout: Duration::from_secs(60) }
}

/// The faithful test artifact's answer to one call.
fn faithful(call: &[u8]) -> Answer {
    command(env!("CARGO_BIN_EXE_xart-test-artifact")).answer(call).unwrap()
}

#[test]
fn faithful_native_artifact_gives_the_oracle_rows() {
    let set = call_set();
    let t = run_call_set(&set, &mut command(env!("CARGO_BIN_EXE_xart-test-artifact")), meta(Kind::Native)).unwrap();
    let want = reference("reference-table.json");
    assert_eq!(t.call_set_digest, want.call_set_digest);
    assert_eq!(t.rows, want.rows);
}

#[test]
fn reversed_native_artifact_gives_the_oracle_rows() {
    let set = call_set();
    let bin = env!("CARGO_BIN_EXE_xart-test-artifact-reversed");
    let t = run_call_set(&set, &mut command(bin), meta(Kind::Native)).unwrap();
    let want = reference("reference-reversed-table.json");
    assert_eq!(t.rows, want.rows);
    let faithful = reference("reference-table.json");
    let differ: Vec<&str> = t
        .rows
        .iter()
        .zip(&faithful.rows)
        .filter(|(a, b)| a.result_digest != b.result_digest)
        .map(|(a, _)| a.name.as_str())
        .collect();
    assert_eq!(differ, ["echo-two-inputs", "echo-three-inputs", "echo-empty-and-z"]);
}

#[test]
fn cli_run_writes_the_table_and_hashes_the_artifact_file() {
    let tmp = std::env::temp_dir().join(format!("fid-xart-run-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let out = tmp.join("table.json");
    let bin = env!("CARGO_BIN_EXE_xart-test-artifact");
    let calls = fixtures().join("calls.txt");
    let args = [
        "run",
        "--calls",
        calls.to_str().unwrap(),
        "--kind",
        "native",
        "--host",
        "linux-x86_64",
        "--artifact",
        "xart-test-artifact",
        "--artifact-file",
        bin,
        "--out",
        out.to_str().unwrap(),
        "--",
        bin,
    ];
    let status = Command::new(env!("CARGO_BIN_EXE_fid-xart")).args(args).status().unwrap();
    assert_eq!(status.code(), Some(0));
    let t = DigestTable::parse(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(t.artifact_sha256, sha256_digest(&std::fs::read(bin).unwrap()));
    assert_eq!(t.rows, reference("reference-table.json").rows);
    assert_eq!(t.to_json().unwrap(), std::fs::read(&out).unwrap());

    // A program that cannot be started: exit 2 and no table.
    let missing = tmp.join("no-such-artifact");
    let gone = tmp.join("gone.json");
    std::fs::write(&gone, b"stale table from an earlier run").unwrap();
    let mut bad = args;
    bad[12] = gone.to_str().unwrap();
    bad[14] = missing.to_str().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_fid-xart")).args(bad).status().unwrap();
    assert_eq!(status.code(), Some(2));
    assert!(!gone.exists());
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// Answers every call with the faithful test table, except call `at`, which gets `answer`.
struct Faulty {
    at: usize,
    seen: usize,
    answer: Answer,
}

impl Artifact for Faulty {
    fn answer(&mut self, call: &[u8]) -> Result<Answer, String> {
        let i = self.seen;
        self.seen += 1;
        if i == self.at {
            return Ok(self.answer.clone());
        }
        Ok(faithful(call))
    }
}

fn fails_at(at: usize, exit: Option<i32>, stdout: &[u8], stderr: &[u8]) -> String {
    let set = call_set();
    let answer = Answer { exit, stdout: stdout.to_vec(), stderr: stderr.to_vec() };
    let mut a = Faulty { at, seen: 0, answer };
    let e = run_call_set(&set, &mut a, meta(Kind::Wasm)).unwrap_err();
    assert_eq!(e.code(), "artifact_failed");
    let text = e.to_string();
    assert!(text.contains(&format!("call {at} (")), "{text}");
    text
}

#[test]
fn every_non_answer_fails_the_run() {
    // The runner's NOT_REPLAYABLE verdicts and its could-not-run exit.
    fails_at(10, Some(3), b"NOT_REPLAYABLE(resource)", b"");
    fails_at(10, Some(4), b"NOT_REPLAYABLE(trap)", b"");
    fails_at(0, Some(5), b"", b"error: no_wasmtime\n");
    // A malformed call, a signal, stray stderr.
    fails_at(1, Some(2), b"{\"error\":\"malformed_call\"}", b"");
    fails_at(1, None, b"", b"");
    let ok = faithful(&call_set().calls[0].bytes).stdout;
    let text = fails_at(0, Some(0), &ok, b"warning\n");
    assert!(text.contains("stderr"), "{text}");
    // A result for another algorithm, a lying output digest, non-canonical bytes.
    let other = String::from_utf8(ok.clone()).unwrap().replace("test.echo.identity", "test.echo.other");
    fails_at(0, Some(0), other.as_bytes(), b"");
    let lie = String::from_utf8(ok.clone()).unwrap().replace("\"output\":\"7c2d\"", "\"output\":\"7c2e\"");
    assert_ne!(lie.as_bytes(), ok.as_slice());
    fails_at(0, Some(0), lie.as_bytes(), b"");
    let spaced = String::from_utf8(ok).unwrap().replace(',', ", ");
    fails_at(0, Some(0), spaced.as_bytes(), b"");
    // Kernel errors must be exactly {"error":"<code>"} with exit 1.
    fails_at(5, Some(1), b"{\"error\":\"Bad Code\"}", b"");
    fails_at(5, Some(1), b"{\"error\":\"algorithm_failed\",\"x\":\"y\"}", b"");
    fails_at(5, Some(0), b"{\"error\":\"algorithm_failed\"}", b"");
}

#[test]
fn classify_accepts_results_and_kernel_errors() {
    let set = call_set();
    for call in &set.calls {
        let a = faithful(&call.bytes);
        let code = u8::try_from(a.exit.unwrap()).unwrap();
        assert!(code <= 1, "{}", call.name);
        assert_eq!(classify(call, &a), Ok(code), "{}", call.name);
    }
}

#[test]
fn labels_are_checked() {
    let set = call_set();
    let mut m = meta(Kind::Native);
    m.host = "Linux".to_string();
    let e = run_call_set(&set, &mut command(env!("CARGO_BIN_EXE_xart-test-artifact")), m).unwrap_err();
    assert_eq!(e.code(), "bad_inputs");
}

#[test]
fn hanging_native_artifact_is_killed_at_the_deadline() {
    let set = call_set();
    let mut a = CommandArtifact {
        program: env!("CARGO_BIN_EXE_xart-test-artifact-hang").to_string(),
        args: Vec::new(),
        timeout: Duration::from_millis(500),
    };
    let start = Instant::now();
    let e = run_call_set(&set, &mut a, meta(Kind::Native)).unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(30), "deadline not enforced");
    assert_eq!(e.code(), "artifact_failed");
    let text = e.to_string();
    assert!(text.contains("call 0 (") && text.contains("timeout"), "{text}");

    // Through the CLI: exit 2, and no table, not even a stale one.
    let tmp = std::env::temp_dir().join(format!("fid-xart-hang-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let out = tmp.join("table.json");
    std::fs::write(&out, b"stale").unwrap();
    let calls = fixtures().join("calls.txt");
    let bin = env!("CARGO_BIN_EXE_xart-test-artifact-hang");
    let status = Command::new(env!("CARGO_BIN_EXE_fid-xart"))
        .args(["run", "--calls", calls.to_str().unwrap(), "--kind", "native", "--host", "linux-x86_64"])
        .args(["--artifact", "hang", "--artifact-file", bin, "--out", out.to_str().unwrap(), "--timeout-ms", "300"])
        .args(["--", bin])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
    assert!(!out.exists());
    std::fs::remove_dir_all(&tmp).unwrap();
}
