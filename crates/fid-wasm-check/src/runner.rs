//! Runs the kernel module under the pinned wasmtime configuration and classifies the outcome
//! (REQ-KRN.MAN.WASM-004, -005).
//!
//! The runner refuses to start unless `wasmtime --version` names exactly the pinned version
//! and the module passes the import check. It then runs `wasmtime` with
//! [`RuntimeConfig::wasmtime_args`], the call on stdin, and maps the process outcome:
//!
//! | wasmtime outcome | runner stdout | runner exit |
//! |---|---|---|
//! | exit 0, stdout starts `{"algorithm":"`, stderr empty | the shim's stdout | 0 |
//! | exit 1 or 2, stdout starts `{"error":"`, stderr empty | the shim's stdout | 1 or 2 |
//! | anything else, stderr has a resource marker | `NOT_REPLAYABLE(resource)` | 3 |
//! | anything else | `NOT_REPLAYABLE(trap)` | 4 |
//!
//! Resource markers ([`RESOURCE_MARKERS`]): wasmtime's stack-overflow trap text (`call stack
//! exhausted`); an out-of-bounds memory access, which in a module built from code without
//! `unsafe` only arises when the shadow stack in linear memory runs past its start; and Rust's
//! allocation-failure message (`memory allocation of`), printed when `memory.grow` fails at
//! the configured memory limit. A wasmtime usage or start-up error never looks like a result:
//! it has no shim-shaped stdout or has stderr output, so it is a trap, not a value.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::runtime::RuntimeConfig;

/// Runner stdout for an exhausted stack or memory.
pub const NOT_REPLAYABLE_RESOURCE: &str = "NOT_REPLAYABLE(resource)";
/// Runner stdout for any other abnormal end.
pub const NOT_REPLAYABLE_TRAP: &str = "NOT_REPLAYABLE(trap)";
/// Stderr substrings that mark stack or memory exhaustion.
pub const RESOURCE_MARKERS: [&str; 3] =
    ["call stack exhausted", "out of bounds memory access", "memory allocation of "];

/// What one run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The shim answered: exit code 0, 1 or 2 and its stdout.
    Shim { exit: u8, stdout: Vec<u8> },
    /// Stack or memory exhausted: `NOT_REPLAYABLE(resource)`.
    Resource,
    /// Any other abnormal end: `NOT_REPLAYABLE(trap)`.
    Trap,
}

impl Outcome {
    /// The runner's exit code.
    pub fn exit_code(&self) -> u8 {
        match self {
            Outcome::Shim { exit, .. } => *exit,
            Outcome::Resource => 3,
            Outcome::Trap => 4,
        }
    }

    /// The runner's stdout.
    pub fn stdout(&self) -> &[u8] {
        match self {
            Outcome::Shim { stdout, .. } => stdout,
            Outcome::Resource => NOT_REPLAYABLE_RESOURCE.as_bytes(),
            Outcome::Trap => NOT_REPLAYABLE_TRAP.as_bytes(),
        }
    }
}

/// Classifies a finished wasmtime process (see the module docs).
pub fn classify(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> Outcome {
    let shaped = match code {
        Some(0) => stdout.starts_with(b"{\"algorithm\":\""),
        Some(1) | Some(2) => stdout.starts_with(b"{\"error\":\""),
        _ => false,
    };
    if shaped && stderr.is_empty() {
        if let Some(c) = code.and_then(|c| u8::try_from(c).ok()) {
            return Outcome::Shim { exit: c, stdout: stdout.to_vec() };
        }
    }
    let err = String::from_utf8_lossy(stderr);
    if RESOURCE_MARKERS.iter().any(|m| err.contains(m)) {
        Outcome::Resource
    } else {
        Outcome::Trap
    }
}

/// True when `wasmtime --version` output names exactly `pinned`: its first line's first word
/// is `wasmtime` or `wasmtime-cli` and its second word is the version.
pub fn version_matches(version_stdout: &str, pinned: &str) -> bool {
    let mut words = version_stdout.lines().next().unwrap_or("").split_whitespace();
    matches!(words.next(), Some("wasmtime") | Some("wasmtime-cli")) && words.next() == Some(pinned)
}

/// Why the runner could not run at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerError {
    /// `wasmtime` could not be started or did not report its version.
    NoWasmtime,
    /// `wasmtime --version` names another version (the reported first line).
    VersionMismatch(String),
    /// The module path cannot be passed safely.
    BadModulePath,
    /// Spawning or talking to the child failed.
    Io(String),
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunnerError::NoWasmtime => f.write_str("no_wasmtime"),
            RunnerError::VersionMismatch(v) => write!(f, "version_mismatch: {v}"),
            RunnerError::BadModulePath => f.write_str("bad_module_path"),
            RunnerError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

/// Checks that the installed `wasmtime` is the pinned version.
pub fn check_wasmtime(cfg: &RuntimeConfig) -> Result<(), RunnerError> {
    let out = Command::new("wasmtime").arg("--version").output().map_err(|_| RunnerError::NoWasmtime)?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        return Err(RunnerError::NoWasmtime);
    }
    if !version_matches(&text, &cfg.wasmtime_version) {
        return Err(RunnerError::VersionMismatch(text.lines().next().unwrap_or("").to_string()));
    }
    Ok(())
}

/// Runs one call through `module` under `cfg`. Call [`check_wasmtime`] first.
pub fn run(cfg: &RuntimeConfig, module: &str, call: &[u8]) -> Result<Outcome, RunnerError> {
    let args = cfg.wasmtime_args(module).ok_or(RunnerError::BadModulePath)?;
    let mut child = Command::new("wasmtime")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| RunnerError::Io(e.to_string()))?;
    let stdin = child.stdin.take().ok_or_else(|| RunnerError::Io("no stdin".to_string()))?;
    let input = call.to_vec();
    // Feed stdin from a thread so a child that writes before reading cannot deadlock us.
    let feeder = std::thread::spawn(move || {
        let mut stdin = stdin;
        // A child that exits early closes the pipe; that is its outcome, not ours.
        let _ = stdin.write_all(&input);
    });
    let out = child.wait_with_output().map_err(|e| RunnerError::Io(e.to_string()))?;
    if feeder.join().is_err() {
        return Err(RunnerError::Io("stdin writer failed".to_string()));
    }
    Ok(classify(out.status.code(), &out.stdout, &out.stderr))
}

/// One row of a calls file (`fixtures/man/wasm/calls-v1.txt`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub name: String,
    /// `any` (native and WASM) or `wasm` (needs the WASM runtime limits).
    pub wasm_only: bool,
    pub call: Vec<u8>,
    pub exit: u8,
    pub stdout: Vec<u8>,
}

/// Parses a calls file: `#` comments, blank lines, and rows of five TAB-separated fields
/// `<name> <any|wasm> <call> <runner exit> <expected stdout>`. The error is the bad line number.
pub fn parse_cases(text: &str) -> Result<Vec<Case>, usize> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let [name, mode, call, exit, stdout] = f.as_slice() else {
            return Err(i + 1);
        };
        let wasm_only = match *mode {
            "any" => false,
            "wasm" => true,
            _ => return Err(i + 1),
        };
        let exit = match *exit {
            "0" => 0,
            "1" => 1,
            "2" => 2,
            "3" => 3,
            "4" => 4,
            _ => return Err(i + 1),
        };
        out.push(Case {
            name: name.to_string(),
            wasm_only,
            call: call.as_bytes().to_vec(),
            exit,
            stdout: stdout.as_bytes().to_vec(),
        });
    }
    Ok(out)
}
