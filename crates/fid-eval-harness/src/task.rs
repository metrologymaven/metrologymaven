//! The fields of `task.json` (AGENT.EVAL.FORMAT) that the sandbox needs.

use crate::path::check_rel;
use crate::{read_bounded, ErrorCode, SandboxError, MAX_FILE_BYTES};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What the sandbox reads from a task: the start-state archive and digest, and the granted
/// roots. Other fields are ignored here; `fid-eval-suite` validates the whole file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub id: String,
    /// The start-state archive, the task's `start_state.path` joined to the task folder.
    pub start_archive: PathBuf,
    /// The task's `start_state.digest`: SHA-256 of the archive bytes.
    pub start_digest: Digest,
    /// Granted read roots, as written (checked when the sandbox is built).
    pub read_roots: Vec<String>,
    /// Granted write roots, as written (checked when the sandbox is built).
    pub write_roots: Vec<String>,
}

fn bad(file: &str, detail: impl std::fmt::Display) -> SandboxError {
    SandboxError::new(ErrorCode::InvalidTask, format!("{file}: {detail}"))
}

fn get<'a>(o: &'a BTreeMap<String, CValue>, key: &str, file: &str) -> Result<&'a CValue, SandboxError> {
    o.get(key).ok_or_else(|| bad(file, format!("missing `{key}`")))
}

fn obj<'a>(v: &'a CValue, what: &str, file: &str) -> Result<&'a BTreeMap<String, CValue>, SandboxError> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(bad(file, format!("`{what}` is not an object"))),
    }
}

fn string<'a>(v: &'a CValue, what: &str, file: &str) -> Result<&'a str, SandboxError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(bad(file, format!("`{what}` is not a string"))),
    }
}

fn strings(v: &CValue, what: &str, file: &str) -> Result<Vec<String>, SandboxError> {
    match v {
        CValue::Arr(items) => {
            items.iter().map(|i| string(i, what, file).map(str::to_string)).collect::<Result<Vec<_>, _>>()
        }
        _ => Err(bad(file, format!("`{what}` is not an array"))),
    }
}

impl TaskSpec {
    /// Reads `<task_dir>/task.json` (a regular file of at most 64 MiB).
    pub fn read(task_dir: &Path) -> Result<TaskSpec, SandboxError> {
        let bytes = read_bounded(&task_dir.join("task.json"), ErrorCode::InvalidTask)?;
        TaskSpec::parse(&bytes, task_dir)
    }

    /// Parses task JSON; `task_dir` is the folder the start-state path is relative to.
    pub fn parse(bytes: &[u8], task_dir: &Path) -> Result<TaskSpec, SandboxError> {
        let file = "task.json";
        let limit = usize::try_from(MAX_FILE_BYTES).unwrap_or(usize::MAX);
        let v = fid_canon::parse_with_limits(bytes, limit, 64).map_err(|e| bad(file, e))?;
        let top = obj(&v, "task", file)?;
        let id = string(get(top, "id", file)?, "id", file)?.to_string();
        let start = obj(get(top, "start_state", file)?, "start_state", file)?;
        let path = string(get(start, "path", file)?, "start_state.path", file)?;
        check_rel(path).map_err(|why| bad(file, format!("`start_state.path`: {why}")))?;
        let digest = string(get(start, "digest", file)?, "start_state.digest", file)?;
        let start_digest =
            Digest::parse(digest).map_err(|_| bad(file, "`start_state.digest` is not a sha256: digest"))?;
        let roots = obj(get(top, "roots", file)?, "roots", file)?;
        let read_roots = strings(get(roots, "read", file)?, "roots.read", file)?;
        let write_roots = strings(get(roots, "write", file)?, "roots.write", file)?;
        Ok(TaskSpec { id, start_archive: task_dir.join(path), start_digest, read_roots, write_roots })
    }
}

/// What the adapter stage reads from a task (AGENT.HARNESS.ADAPT): the prompt, the MCP server
/// profile and the budgets. Like [`TaskSpec`], other fields are ignored here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTask {
    pub id: String,
    pub prompt: String,
    /// The `fid mcp --server` profile; checked by [`crate::check_profile`] before use.
    pub profile: String,
    pub wall_s: u64,
    pub tool_calls: u64,
}

fn count(v: &CValue, what: &str, file: &str) -> Result<u64, SandboxError> {
    match v {
        CValue::Num(f) if *f >= 0.0 && f.fract() == 0.0 && *f <= 9_007_199_254_740_992.0 => Ok(*f as u64),
        _ => Err(bad(file, format!("`{what}` is not a non-negative integer"))),
    }
}

impl AgentTask {
    /// Reads `<task_dir>/task.json` (a regular file of at most 64 MiB).
    pub fn read(task_dir: &Path) -> Result<AgentTask, SandboxError> {
        let bytes = read_bounded(&task_dir.join("task.json"), ErrorCode::InvalidTask)?;
        AgentTask::parse(&bytes)
    }

    /// Parses task JSON.
    pub fn parse(bytes: &[u8]) -> Result<AgentTask, SandboxError> {
        let file = "task.json";
        let limit = usize::try_from(MAX_FILE_BYTES).unwrap_or(usize::MAX);
        let v = fid_canon::parse_with_limits(bytes, limit, 64).map_err(|e| bad(file, e))?;
        let top = obj(&v, "task", file)?;
        let id = string(get(top, "id", file)?, "id", file)?.to_string();
        let prompt = string(get(top, "prompt", file)?, "prompt", file)?.to_string();
        let profile = string(get(top, "profile", file)?, "profile", file)?.to_string();
        let budget = obj(get(top, "budget", file)?, "budget", file)?;
        let wall_s = count(get(budget, "wall_s", file)?, "budget.wall_s", file)?;
        let tool_calls = count(get(budget, "tool_calls", file)?, "budget.tool_calls", file)?;
        Ok(AgentTask { id, prompt, profile, wall_s, tool_calls })
    }
}
