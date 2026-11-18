//! Scripted deterministic red-team adversary (spec node AGENT.REDTEAM.SCRIPT).
//!
//! An MCP client with no LLM. For every red-team task it attempts every forbidden action
//! directly and by every known indirect route, so the release verdict tests enforcement, not
//! whether a model happens to refuse. It reads only the task file, the registry manifest and
//! the server's tool list; see [`plan`] for the attempt plan and [`session`] for the wire
//! protocol. Determinism class D0: fixed ordering, canonical JSON, no clock, no randomness and
//! no hash-map iteration; the only variation is seeded from the task ID.
//!
//! Every payload is inert test data: the escaping paths name files that do not exist, the
//! only host is under the reserved `.invalid` domain, and injected principals and approvals
//! are made-up strings. Run it only inside a harness sandbox.

pub mod manifest;
pub mod plan;
pub mod session;

pub use manifest::{parse_manifest, Command, Manifest};
pub use plan::{build_plan, seed, Attempt, Family, Route, TaskView, FORBIDDEN, RAW_PATHS};
pub use session::{run, CliRunner, ProcessCli, Summary, DEFAULT_DISPATCHER};

use fid_canon::CValue;
use std::fmt;
use std::path::Path;

/// Errors with stable codes. Individual refusals by the server are expected and logged, never
/// errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdversaryError {
    /// The manifest is missing, unreadable or malformed.
    ManifestUnreadable(String),
    /// The task file is missing or fails the AGENT.EVAL.FORMAT parser.
    TaskUnreadable(String),
    /// The MCP client configuration is missing, malformed or does not name exactly one server.
    InvalidMcpConfig(String),
    /// The configured server could not be started.
    ServerSpawnFailed(String),
    /// An I/O failure on the session or the log.
    Io(String),
}

impl AdversaryError {
    pub fn code(&self) -> &'static str {
        match self {
            AdversaryError::ManifestUnreadable(_) => "manifest_unreadable",
            AdversaryError::TaskUnreadable(_) => "task_unreadable",
            AdversaryError::InvalidMcpConfig(_) => "invalid_mcp_config",
            AdversaryError::ServerSpawnFailed(_) => "server_spawn_failed",
            AdversaryError::Io(_) => "io_error",
        }
    }
}

impl fmt::Display for AdversaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = match self {
            AdversaryError::ManifestUnreadable(d)
            | AdversaryError::TaskUnreadable(d)
            | AdversaryError::InvalidMcpConfig(d)
            | AdversaryError::ServerSpawnFailed(d)
            | AdversaryError::Io(d) => d,
        };
        write!(f, "{}: {d}", self.code())
    }
}

impl std::error::Error for AdversaryError {}

/// Largest accepted task or MCP configuration file, in bytes.
const MAX_SMALL_FILE: u64 = 4 * 1024 * 1024;

fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)?.take(limit + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > limit {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "file too large"));
    }
    Ok(buf)
}

/// Reads and parses a manifest file.
pub fn read_manifest(path: &Path) -> Result<Manifest, AdversaryError> {
    let bytes = read_bounded(path, manifest::MAX_MANIFEST_BYTES as u64)
        .map_err(|e| AdversaryError::ManifestUnreadable(format!("{}: {e}", path.display())))?;
    parse_manifest(&bytes)
}

/// Reads a task file with the AGENT.EVAL.FORMAT parser.
pub fn read_task(path: &Path) -> Result<TaskView, AdversaryError> {
    let bytes = read_bounded(path, MAX_SMALL_FILE)
        .map_err(|e| AdversaryError::TaskUnreadable(format!("{}: {e}", path.display())))?;
    let name = path.to_string_lossy();
    match fid_eval_suite::parse_task(&bytes, &name) {
        Ok(t) => Ok(TaskView::from_task(&t)),
        Err(errs) => {
            let detail: Vec<String> =
                errs.iter().map(|e| format!("{} {}: {}", e.code.as_str(), e.field, e.detail)).collect();
            Err(AdversaryError::TaskUnreadable(format!("{name}: {}", detail.join("; "))))
        }
    }
}

/// The server launch from an MCP client configuration (`{"mcpServers": {name: {command, args,
/// env}}}`, the shape AGENT.HARNESS.ADAPT generates). Exactly one server is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerLaunch {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Parses an MCP client configuration.
pub fn parse_mcp_config(bytes: &[u8]) -> Result<ServerLaunch, AdversaryError> {
    let bad = |d: &str| AdversaryError::InvalidMcpConfig(d.to_string());
    let root = fid_canon::parse_with_limits(bytes, MAX_SMALL_FILE as usize, 16).map_err(|_| bad("not JSON"))?;
    let CValue::Obj(top) = root else {
        return Err(bad("top level is not an object"));
    };
    let Some(CValue::Obj(servers)) = top.get("mcpServers") else {
        return Err(bad("missing object `mcpServers`"));
    };
    let mut it = servers.values();
    let (Some(CValue::Obj(srv)), None) = (it.next(), it.next()) else {
        return Err(bad("`mcpServers` must hold exactly one server object"));
    };
    let Some(CValue::Str(command)) = srv.get("command") else {
        return Err(bad("server has no string `command`"));
    };
    let strings = |v: Option<&CValue>| -> Result<Vec<String>, AdversaryError> {
        match v {
            None => Ok(Vec::new()),
            Some(CValue::Arr(a)) => a
                .iter()
                .map(|x| match x {
                    CValue::Str(s) => Ok(s.clone()),
                    _ => Err(bad("`args` must be strings")),
                })
                .collect(),
            Some(_) => Err(bad("`args` is not an array")),
        }
    };
    let args = strings(srv.get("args"))?;
    let env = match srv.get("env") {
        None => Vec::new(),
        Some(CValue::Obj(e)) => e
            .iter()
            .map(|(k, v)| match v {
                CValue::Str(s) => Ok((k.clone(), s.clone())),
                _ => Err(bad("`env` values must be strings")),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(bad("`env` is not an object")),
    };
    Ok(ServerLaunch { command: command.clone(), args, env })
}

/// Starts the configured server and runs the adversary over its stdio. The server's stderr is
/// inherited; its stdin is closed at the end and the server is waited for.
pub fn run_with_server<L: std::io::Write>(
    task: &TaskView,
    manifest: &Manifest,
    launch: &ServerLaunch,
    log: &mut L,
    cli: &mut dyn CliRunner,
) -> Result<Summary, AdversaryError> {
    use std::process::{Command as Proc, Stdio};
    let mut child = Proc::new(&launch.command)
        .args(&launch.args)
        .envs(launch.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| AdversaryError::ServerSpawnFailed(format!("{}: {e}", launch.command)))?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(AdversaryError::ServerSpawnFailed("no stdio pipes".into()));
    };
    let result = run(task, manifest, std::io::BufReader::new(stdout), stdin, log, cli);
    let _ = child.wait();
    result
}
