//! Runs a call set through one kernel artifact and tabulates the answers (REQ-001, REQ-002).
//!
//! An artifact is anything that speaks the `fid-kernel-wasm` protocol: one canonical call on
//! stdin, and on stdout either the canonical `KernelResult` with exit 0 or `{"error":"<code>"}`
//! with exit 1, nothing on stderr. The WASM artifact is run through the pinned runner
//! (`fid-wasm-check run <wasm-runtime.toml> <wasm-imports.allow> <module>`), which passes those
//! answers through unchanged; a native artifact is the native build of the same shim.
//!
//! **Fail closed.** Every other answer stops the run with [`XartError::Artifact`]: the runner's
//! `NOT_REPLAYABLE(resource)` (exit 3) or `NOT_REPLAYABLE(trap)` (exit 4), a runner that could
//! not start (exit 5), a malformed or oversized call (exit 2), a process that could not be
//! spawned, was killed or missed its wall-clock deadline, anything on stderr, and a result envelope that is not canonical,
//! names another algorithm or whose `output_digest` is not the SHA-256 of its `output`. No
//! call is ever skipped, so a table always has one row per call.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fid_kci::kci::{sha256_digest, Sha256};
use fid_kci::Digest;

use crate::calls::{Call, CallSet};
use crate::table::{is_algorithm, DigestTable, Kind, Row};
use crate::{is_label, json, XartError};

/// Largest stdout accepted for one call (the result envelope is hex, so twice the output).
pub const MAX_ANSWER_BYTES: usize = 64 * 1024 * 1024;

/// What an artifact process did with one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// Exit code, `None` if the process was killed by a signal.
    pub exit: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// A kernel artifact that answers one call at a time.
pub trait Artifact {
    /// Sends `call` and collects the answer; `Err` if the artifact could not be run at all.
    fn answer(&mut self, call: &[u8]) -> Result<Answer, String>;
}

/// An artifact run as a child process, one process per call, with a wall-clock deadline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandArtifact {
    pub program: String,
    pub args: Vec<String>,
    /// Wall-clock deadline per call. A process still running at the deadline is killed and the
    /// call is an `artifact_failed` error (fail closed).
    pub timeout: Duration,
}

/// Reads at most `MAX_ANSWER_BYTES + 1` bytes of a pipe on its own thread.
fn reader<R: Read + Send + 'static>(pipe: R) -> JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        pipe.take(MAX_ANSWER_BYTES as u64 + 1).read_to_end(&mut buf).map(|_| buf)
    })
}

impl Artifact for CommandArtifact {
    fn answer(&mut self, call: &[u8]) -> Result<Answer, String> {
        let deadline = Instant::now() + self.timeout;
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", self.program))?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let input = call.to_vec();
        // Feed stdin and drain stdout and stderr on their own threads, so a child that writes
        // before reading cannot deadlock us; a child that exits early closes the pipe, which is
        // its outcome, not ours.
        let feeder = std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
        let out_reader = reader(child.stdout.take().ok_or("no stdout")?);
        let err_reader = reader(child.stderr.take().ok_or("no stderr")?);
        let mut pause = Duration::from_millis(1);
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| format!("wait: {e}"))? {
                break status;
            }
            let now = Instant::now();
            if now >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                // The pipe threads are not joined: a grandchild may still hold the pipes.
                return Err(format!("timeout: no answer within {} ms, killed", self.timeout.as_millis()));
            }
            std::thread::sleep(pause.min(deadline - now));
            pause = (pause * 2).min(Duration::from_millis(20));
        };
        if feeder.join().is_err() {
            return Err("stdin writer failed".to_string());
        }
        let stdout = match out_reader.join() {
            Ok(Ok(b)) => b,
            Ok(Err(e)) => return Err(format!("stdout: {e}")),
            Err(_) => return Err("stdout reader failed".to_string()),
        };
        let stderr = match err_reader.join() {
            Ok(Ok(b)) => b,
            _ => b"<stderr unreadable>".to_vec(),
        };
        if stdout.len() > MAX_ANSWER_BYTES {
            return Err("stdout_too_large".to_string());
        }
        Ok(Answer { exit: status.code(), stdout, stderr })
    }
}

const RESULT_KEYS: [&str; 5] = ["algorithm", "input_digests", "output", "output_digest", "params_digest"];

fn lower_hex(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.len() % 2 != 0 {
        return None;
    }
    let nib = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    b.chunks_exact(2).map(|p| Some((nib(p[0])? << 4) | nib(p[1])?)).collect()
}

fn check_result(call: &Call, stdout: &[u8]) -> Result<(), String> {
    let v = json::parse_canonical(stdout, MAX_ANSWER_BYTES)?;
    let m = json::object(&v, &RESULT_KEYS, &["seed"])?;
    if json::string(m, "algorithm")? != call.algorithm {
        return Err("result names another algorithm".to_string());
    }
    for d in json::array(m, "input_digests")? {
        match d {
            fid_canon::CValue::Str(s) if Digest::is_valid(s) => {}
            _ => return Err("bad input digest".to_string()),
        }
    }
    json::digest(m, "params_digest")?;
    let output = lower_hex(json::string(m, "output")?).ok_or("output is not lowercase hex")?;
    if sha256_digest(&output) != json::digest(m, "output_digest")? {
        return Err("output_digest is not the SHA-256 of output".to_string());
    }
    if let Ok(seed) = json::string(m, "seed") {
        let digits = !seed.is_empty() && seed.bytes().all(|c| c.is_ascii_digit());
        let ok = digits && (seed == "0" || !seed.starts_with('0')) && seed.parse::<u64>().is_ok();
        if !ok {
            return Err("bad seed".to_string());
        }
    }
    Ok(())
}

fn check_error(stdout: &[u8]) -> Result<(), String> {
    let v = json::parse_canonical(stdout, 1024)?;
    let code = json::string(json::object(&v, &["error"], &[])?, "error")?;
    if (1..=64).contains(&code.len()) && code.bytes().all(|c| c.is_ascii_lowercase() || c == b'_') {
        Ok(())
    } else {
        Err("bad error code".to_string())
    }
}

/// Checks one answer; `Ok(exit)` (0 result, 1 kernel error) or why the artifact failed.
pub fn classify(call: &Call, answer: &Answer) -> Result<u8, String> {
    if !answer.stderr.is_empty() {
        let text = String::from_utf8_lossy(&answer.stderr);
        let last = text.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
        return Err(format!("wrote to stderr (exit {:?}): {last}", answer.exit));
    }
    let shown = || String::from_utf8_lossy(&answer.stdout[..answer.stdout.len().min(80)]).to_string();
    match answer.exit {
        Some(0) => check_result(call, &answer.stdout).map(|()| 0).map_err(|e| format!("bad result ({e}): {}", shown())),
        Some(1) => check_error(&answer.stdout).map(|()| 1).map_err(|e| format!("bad kernel error ({e}): {}", shown())),
        Some(c) => Err(format!("exit {c}: {}", shown())),
        None => Err("killed by a signal".to_string()),
    }
}

/// Identity of the artifact a table describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableMeta {
    pub artifact: String,
    pub artifact_sha256: Digest,
    pub host: String,
    pub kind: Kind,
}

/// Runs every call through `artifact`; any failure stops the run (see the module docs).
pub fn run_call_set(set: &CallSet, artifact: &mut dyn Artifact, meta: TableMeta) -> Result<DigestTable, XartError> {
    if !is_label(&meta.artifact) || !is_label(&meta.host) {
        return Err(XartError::Inputs("artifact and host must be labels ([a-z0-9._-], 1 to 64 bytes)".to_string()));
    }
    let mut rows = Vec::with_capacity(set.calls.len());
    for (i, call) in set.calls.iter().enumerate() {
        let failed = |reason: String| XartError::Artifact {
            call: i,
            name: call.name.clone(),
            algorithm: call.algorithm.clone(),
            reason,
        };
        if !is_algorithm(&call.algorithm) {
            return Err(failed("bad algorithm id".to_string()));
        }
        let answer = artifact.answer(&call.bytes).map_err(failed)?;
        let exit = classify(call, &answer).map_err(failed)?;
        rows.push(Row {
            call: i,
            name: call.name.clone(),
            algorithm: call.algorithm.clone(),
            exit,
            result_digest: sha256_digest(&answer.stdout),
        });
    }
    Ok(DigestTable {
        artifact: meta.artifact,
        artifact_sha256: meta.artifact_sha256,
        call_set_digest: set.digest,
        host: meta.host,
        kind: meta.kind,
        rows,
    })
}

/// SHA-256 of an artifact file, read in chunks.
pub fn hash_file(path: &Path) -> Result<Digest, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(Digest::from_bytes(h.finish()))
}
