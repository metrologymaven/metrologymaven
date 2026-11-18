//! The agent adapter contract (spec node AGENT.HARNESS.ADAPT).
//!
//! An adapter file (`adapter.json`) tells the harness how to start one agent:
//!
//! ```json
//! { "name": "exec-example", "version": "1",
//!   "argv": ["example-agent", "--mcp-config", "{mcp_config}", "--prompt-file", "{prompt_file}"],
//!   "prompt": "file", "token_report": "tokens.json",
//!   "model": "example-model", "settings": {"temperature": "0"}, "shell": false }
//! ```
//!
//! [`Adapter::parse`] is strict and reports every problem at once: unknown or missing fields,
//! wrong types, an empty argv, any `{` or `}` that is not one of the three placeholders
//! `{prompt_file}`, `{mcp_config}` and `{workdir}` (`invalid_adapter`), and a placeholder the
//! agent cannot work without (`missing_placeholder`). It also refuses anything that would hand
//! the agent the answers: an argument naming an oracle file or folder (`oracle*`), a task file
//! (`task.json`, which carries red-team predicates and markers), a `..` segment, or an absolute
//! path into the suite folder.
//!
//! The agent is never started through a shell: [`LaunchPlan::command`] runs `argv[0]` directly
//! with the remaining arguments, and an adapter whose program is itself a shell (or a Windows
//! batch file, which the standard library runs through `cmd.exe`) is refused unless it
//! declares `"shell": true`. The child environment is cleared and refilled from an allowlist
//! ([`ENV_ALLOWLIST`]); [`LaunchPlan::run`] enforces a wall-time limit and caps captured
//! output. Process-tree termination, the MCP tap and tool-call budgets belong to
//! AGENT.HARNESS.RUN.

use crate::path::{check_rel, is_same_or_ancestor};
use crate::{read_bounded, ErrorCode, SandboxError};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The only placeholders an argv element may contain.
pub const PLACEHOLDERS: [&str; 3] = ["prompt_file", "mcp_config", "workdir"];
/// Largest accepted adapter file, in bytes.
pub const MAX_ADAPTER_BYTES: usize = 1024 * 1024;
/// Most argv elements.
pub const MAX_ARGV: usize = 256;
/// Longest argv element, in bytes.
pub const MAX_ARG_BYTES: usize = 4096;
/// Longest name, version, model, setting key or setting value, in bytes.
pub const MAX_TEXT_BYTES: usize = 256;
/// Most entries in `settings`.
pub const MAX_SETTINGS: usize = 64;
/// Largest token report read, in bytes.
pub const MAX_TOKEN_REPORT_BYTES: u64 = 64 * 1024;
/// Default cap on each of stdout and stderr captured by [`LaunchPlan::run`].
pub const DEFAULT_OUTPUT_CAP: usize = 1024 * 1024;
/// The name of the single MCP server in the generated client configuration.
pub const MCP_SERVER_NAME: &str = "fid";

/// Environment variables a launched agent may receive (the variables the sandbox sets, see
/// `Sandbox::env`). Everything else, API keys included, is dropped.
pub const ENV_ALLOWLIST: [&str; 19] = [
    "APPDATA",
    "ComSpec",
    "HOME",
    "LOCALAPPDATA",
    "PATH",
    "PATHEXT",
    "SystemDrive",
    "SystemRoot",
    "TEMP",
    "TMP",
    "TMPDIR",
    "USERPROFILE",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_DIRS",
    "XDG_CONFIG_HOME",
    "XDG_DATA_DIRS",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "windir",
];

/// Programs that are command interpreters: launching one is shell access.
const SHELLS: [&str; 15] = [
    "ash", "bash", "busybox", "cmd", "csh", "dash", "env", "fish", "ksh", "powershell", "pwsh", "sh", "tcsh", "wsl",
    "zsh",
];

const FIELDS: [&str; 8] = ["argv", "model", "name", "prompt", "settings", "shell", "token_report", "version"];

/// How the task prompt reaches the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PromptMode {
    /// Written to the agent's standard input, which is then closed.
    Stdin,
    /// Written to the file named by `{prompt_file}`.
    File,
    /// Appended to argv as one last argument.
    Arg,
}

impl PromptMode {
    /// The wire string, e.g. `stdin`.
    pub fn as_str(&self) -> &'static str {
        match self {
            PromptMode::Stdin => "stdin",
            PromptMode::File => "file",
            PromptMode::Arg => "arg",
        }
    }
}

/// A validated adapter file. `model` and `settings` are declared by the agent's author and
/// are kept as untrusted text: never interpreted, only recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    pub name: String,
    pub version: String,
    /// Launch argv; element 0 is the program and holds no placeholder.
    pub argv: Vec<String>,
    pub prompt: PromptMode,
    /// File name (one segment) of the token report, below the agent's working folder.
    pub token_report: Option<String>,
    pub model: String,
    pub settings: BTreeMap<String, String>,
    /// Whether the agent gets shell access.
    pub shell: bool,
}

/// Every problem found in an adapter file, each with its error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterError {
    pub problems: Vec<SandboxError>,
}

impl AdapterError {
    fn one(e: SandboxError) -> AdapterError {
        AdapterError { problems: vec![e] }
    }

    /// `invalid_adapter` if any problem has that code, otherwise the first problem's code.
    pub fn code(&self) -> ErrorCode {
        if self.problems.iter().any(|p| p.code == ErrorCode::InvalidAdapter) {
            return ErrorCode::InvalidAdapter;
        }
        self.problems.first().map_or(ErrorCode::InvalidAdapter, |p| p.code)
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, p) in self.problems.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for AdapterError {}

fn invalid(detail: impl Into<String>) -> SandboxError {
    SandboxError::new(ErrorCode::InvalidAdapter, detail)
}

/// One piece of an argv element.
enum Piece<'a> {
    Lit(&'a str),
    Ph(&'static str),
}

/// Splits an argv element into literal text and placeholders. Any `{` or `}` that is not
/// part of one of the three placeholders is an error (there is no escape syntax).
fn scan(s: &str) -> (Vec<Piece<'_>>, Vec<String>) {
    let mut pieces = Vec::new();
    let mut errors = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find(['{', '}']) {
        let (lit, tail) = rest.split_at(i);
        if !lit.is_empty() {
            pieces.push(Piece::Lit(lit));
        }
        if tail.starts_with('}') {
            errors.push("stray `}`".to_string());
            rest = tail.get(1..).unwrap_or("");
            continue;
        }
        let body = tail.get(1..).unwrap_or("");
        let end = body.find(['{', '}']).filter(|j| body.as_bytes().get(*j) == Some(&b'}'));
        match end {
            Some(j) => {
                let name = body.get(..j).unwrap_or("");
                match PLACEHOLDERS.iter().find(|p| **p == name) {
                    Some(p) => pieces.push(Piece::Ph(p)),
                    None => errors.push(format!("unknown placeholder `{{{name}}}`")),
                }
                rest = body.get(j + 1..).unwrap_or("");
            }
            None => {
                errors.push("unclosed `{`".to_string());
                rest = body;
            }
        }
    }
    if !rest.is_empty() {
        pieces.push(Piece::Lit(rest));
    }
    (pieces, errors)
}

/// Why an argument must never reach the agent, if it names oracle data or leaves the sandbox.
/// Path segments are split on `/`, `\`, `=` and `:` and compared case-insensitively.
fn oracle_reason(s: &str) -> Option<&'static str> {
    for seg in s.split(['/', '\\', '=', ':']) {
        let seg = seg.to_ascii_lowercase();
        if seg == ".." {
            return Some("has a `..` segment, which can leave the sandbox");
        }
        if seg.starts_with("oracle") {
            return Some("names an oracle file or folder; an agent never gets oracle paths");
        }
        if seg == "task.json" {
            return Some(
                "names a task file, which carries red-team predicates and markers; an agent never gets oracle data",
            );
        }
    }
    None
}

/// A lexically normalised, `/`-separated, ASCII-case-folded form of an absolute path, without
/// a Windows verbatim prefix.
fn norm(p: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut prefix = String::new();
    for c in p.components() {
        match c {
            Component::Prefix(x) => {
                let s = x.as_os_str().to_string_lossy().to_string();
                prefix = if let Some(u) = s.strip_prefix(r"\\?\UNC\") {
                    format!(r"\\{u}")
                } else if let Some(v) = s.strip_prefix(r"\\?\") {
                    v.to_string()
                } else {
                    s
                };
            }
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(x) => parts.push(x.to_string_lossy().to_string()),
        }
    }
    format!("{}/{}", prefix.replace('\\', "/"), parts.join("/")).to_ascii_lowercase()
}

/// The suite folder in the forms it may be written: as given (made absolute) and canonical.
fn suite_forms(suite: &Path) -> Vec<String> {
    let mut forms = Vec::new();
    if let Ok(abs) = std::path::absolute(suite) {
        forms.push(norm(&abs));
    }
    if let Ok(c) = std::fs::canonicalize(suite) {
        forms.push(norm(&c));
    }
    forms.sort();
    forms.dedup();
    forms
}

/// True when `s`, or the text after any `=` in it, is an absolute path at or below one of
/// `forms`.
fn points_into(s: &str, forms: &[String]) -> bool {
    std::iter::once(s).chain(s.split('=').skip(1)).any(|c| {
        let p = Path::new(c);
        let abs = p.is_absolute() || c.starts_with('/') || c.starts_with('\\');
        abs && forms.iter().any(|f| is_same_or_ancestor(f.trim_end_matches('/'), norm(p).trim_end_matches('/')))
    })
}

/// The program name without folders and a Windows executable extension, folded.
fn program_stem(program: &str) -> (String, String) {
    let base = program.rsplit(['/', '\\']).next().unwrap_or(program).to_ascii_lowercase();
    match base.rsplit_once('.') {
        Some((stem, ext)) => (stem.to_string(), ext.to_string()),
        None => (base, String::new()),
    }
}

fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

fn text(key: &str, s: &str, allow_empty: bool, p: &mut Vec<SandboxError>) {
    if s.is_empty() && !allow_empty {
        p.push(invalid(format!("`{key}` is empty")));
    } else if s.len() > MAX_TEXT_BYTES {
        p.push(invalid(format!("`{key}` is longer than {MAX_TEXT_BYTES} bytes")));
    } else if has_control(s) {
        p.push(invalid(format!("`{key}` contains a control character")));
    }
}

fn req_str<'a>(top: &'a BTreeMap<String, CValue>, key: &str, p: &mut Vec<SandboxError>) -> Option<&'a str> {
    match top.get(key) {
        None => {
            p.push(invalid(format!("missing field `{key}`")));
            None
        }
        Some(CValue::Str(s)) => Some(s.as_str()),
        Some(_) => {
            p.push(invalid(format!("`{key}` is not a string")));
            None
        }
    }
}

impl Adapter {
    /// Reads and checks an adapter file. With `suite_dir`, absolute paths into that folder are
    /// refused too.
    pub fn read(path: &Path, suite_dir: Option<&Path>) -> Result<Adapter, AdapterError> {
        let bytes = read_bounded(path, ErrorCode::InvalidAdapter).map_err(AdapterError::one)?;
        Adapter::parse(&bytes, suite_dir)
    }

    /// Parses and checks adapter JSON, collecting every problem.
    pub fn parse(bytes: &[u8], suite_dir: Option<&Path>) -> Result<Adapter, AdapterError> {
        let v = fid_canon::parse_with_limits(bytes, MAX_ADAPTER_BYTES, 16)
            .map_err(|e| AdapterError::one(invalid(format!("not valid JSON: {e}"))))?;
        let top = match v {
            CValue::Obj(m) => m,
            _ => return Err(AdapterError::one(invalid("the adapter is not a JSON object"))),
        };
        let mut p = Vec::new();
        for k in top.keys() {
            if !FIELDS.contains(&k.as_str()) {
                p.push(invalid(format!("unknown field `{k}`")));
            }
        }

        let name = req_str(&top, "name", &mut p).unwrap_or_default().to_string();
        let name_ok = !name.is_empty()
            && name.len() <= 64
            && name.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'));
        if matches!(top.get("name"), Some(CValue::Str(_))) && !name_ok {
            p.push(invalid("`name` is not 1 to 64 characters of [A-Za-z0-9._-]"));
        }
        let version = req_str(&top, "version", &mut p).unwrap_or_default().to_string();
        if matches!(top.get("version"), Some(CValue::Str(_))) {
            text("version", &version, false, &mut p);
        }
        let model = req_str(&top, "model", &mut p).unwrap_or_default().to_string();
        if matches!(top.get("model"), Some(CValue::Str(_))) {
            text("model", &model, false, &mut p);
        }

        let prompt = match req_str(&top, "prompt", &mut p) {
            Some("stdin") => Some(PromptMode::Stdin),
            Some("file") => Some(PromptMode::File),
            Some("arg") => Some(PromptMode::Arg),
            Some(other) => {
                p.push(invalid(format!("`prompt` is `{other}`, not one of stdin, file, arg")));
                None
            }
            None => None,
        };

        let token_report = match top.get("token_report") {
            None => None,
            Some(CValue::Str(s)) => {
                if check_rel(s).is_err() || s.contains('/') {
                    p.push(invalid("`token_report` is not a plain file name"));
                } else if let Some(why) = oracle_reason(s) {
                    p.push(invalid(format!("`token_report` {why}")));
                }
                Some(s.clone())
            }
            Some(_) => {
                p.push(invalid("`token_report` is not a string"));
                None
            }
        };

        let mut settings = BTreeMap::new();
        match top.get("settings") {
            None => p.push(invalid("missing field `settings`")),
            Some(CValue::Obj(m)) => {
                if m.len() > MAX_SETTINGS {
                    p.push(invalid(format!("`settings` has more than {MAX_SETTINGS} entries")));
                }
                for (k, v) in m {
                    text("settings key", k, false, &mut p);
                    match v {
                        CValue::Str(s) => {
                            text(&format!("settings.{k}"), s, true, &mut p);
                            settings.insert(k.clone(), s.clone());
                        }
                        _ => p.push(invalid(format!("`settings.{k}` is not a string"))),
                    }
                }
            }
            Some(_) => p.push(invalid("`settings` is not an object")),
        }

        let shell = match top.get("shell") {
            None => {
                p.push(invalid("missing field `shell`"));
                false
            }
            Some(CValue::Bool(b)) => *b,
            Some(_) => {
                p.push(invalid("`shell` is not true or false"));
                false
            }
        };

        let mut argv = Vec::new();
        match top.get("argv") {
            None => p.push(invalid("missing field `argv`")),
            Some(CValue::Arr(items)) => {
                if items.is_empty() {
                    p.push(invalid("`argv` is empty"));
                }
                if items.len() > MAX_ARGV {
                    p.push(invalid(format!("`argv` has more than {MAX_ARGV} elements")));
                }
                for (i, item) in items.iter().enumerate() {
                    match item {
                        CValue::Str(s) => argv.push(s.clone()),
                        _ => p.push(invalid(format!("argv[{i}] is not a string"))),
                    }
                }
            }
            Some(_) => p.push(invalid("`argv` is not an array")),
        }
        let suite_forms = suite_dir.map(suite_forms).unwrap_or_default();
        let mut used = BTreeSet::new();
        for (i, a) in argv.iter().enumerate() {
            if a.len() > MAX_ARG_BYTES {
                p.push(invalid(format!("argv[{i}] is longer than {MAX_ARG_BYTES} bytes")));
            }
            if has_control(a) {
                p.push(invalid(format!("argv[{i}] contains a control character")));
            }
            let (pieces, errors) = scan(a);
            for e in errors {
                p.push(invalid(format!("argv[{i}]: {e}")));
            }
            for piece in &pieces {
                if let Piece::Ph(name) = piece {
                    used.insert(*name);
                    if i == 0 {
                        p.push(invalid(format!("argv[0] (the program) contains the placeholder `{{{name}}}`")));
                    }
                }
            }
            if i == 0 && a.is_empty() {
                p.push(invalid("argv[0] (the program) is empty"));
            }
            if let Some(why) = oracle_reason(a) {
                p.push(invalid(format!("argv[{i}] {why}")));
            }
            if points_into(a, &suite_forms) {
                p.push(invalid(format!("argv[{i}] is an absolute path into the suite folder")));
            }
        }
        if let Some(program) = argv.first() {
            let (stem, ext) = program_stem(program);
            if !shell && (SHELLS.contains(&stem.as_str()) || ext == "bat" || ext == "cmd") {
                p.push(invalid(format!(
                    "argv[0] `{program}` runs a command interpreter, but the adapter does not declare shell access"
                )));
            }
            if !used.contains("mcp_config") {
                p.push(SandboxError::new(
                    ErrorCode::MissingPlaceholder,
                    "argv never uses `{mcp_config}`, so the agent would get no MCP server",
                ));
            }
            if prompt == Some(PromptMode::File) && !used.contains("prompt_file") {
                p.push(SandboxError::new(
                    ErrorCode::MissingPlaceholder,
                    "prompt delivery is `file` but argv never uses `{prompt_file}`",
                ));
            }
        }

        if !p.is_empty() {
            return Err(AdapterError { problems: p });
        }
        Ok(Adapter {
            name,
            version,
            argv,
            prompt: prompt.unwrap_or(PromptMode::Stdin),
            token_report,
            model,
            settings,
            shell,
        })
    }

    /// The launch argv with the placeholders replaced by `paths` and, for prompt delivery
    /// `arg`, the prompt appended. Fails with `invalid_adapter` when a path names oracle data
    /// or lies in `suite_dir`, or when an `arg` prompt is empty, starts with `-` (it would be
    /// read as an option) or contains a NUL.
    pub fn resolve_argv(
        &self,
        paths: &AgentPaths,
        prompt: &str,
        suite_dir: Option<&Path>,
    ) -> Result<Vec<String>, AdapterError> {
        let forms = suite_dir.map(suite_forms).unwrap_or_default();
        let mut p = Vec::new();
        for (name, value) in paths.pairs() {
            if let Some(why) = oracle_reason(value) {
                p.push(invalid(format!("`{{{name}}}` {why}")));
            }
            if points_into(value, &forms) {
                p.push(invalid(format!("`{{{name}}}` lies in the suite folder")));
            }
        }
        if self.prompt == PromptMode::Arg && (prompt.is_empty() || prompt.starts_with('-') || prompt.contains('\0')) {
            p.push(invalid("an `arg` prompt must be non-empty, must not start with `-` and must not contain NUL"));
        }
        if !p.is_empty() {
            return Err(AdapterError { problems: p });
        }
        let mut out = Vec::with_capacity(self.argv.len() + 1);
        for a in &self.argv {
            let (pieces, _) = scan(a);
            let mut s = String::new();
            for piece in pieces {
                match piece {
                    Piece::Lit(l) => s.push_str(l),
                    Piece::Ph(name) => s.push_str(paths.get(name)),
                }
            }
            out.push(s);
        }
        if self.prompt == PromptMode::Arg {
            out.push(prompt.to_string());
        }
        Ok(out)
    }

    /// A launch plan: the resolved argv, no shell, the scrubbed environment and the limits.
    pub fn plan(&self, input: &LaunchInput<'_>) -> Result<LaunchPlan, AdapterError> {
        let mut argv = self.resolve_argv(&input.paths, input.prompt, input.suite_dir)?;
        let forms = input.suite_dir.map(suite_forms).unwrap_or_default();
        let env = scrub_env(input.env);
        let mut p = Vec::new();
        for (k, v) in &env {
            if std::env::split_paths(v).any(|x| x.to_str().is_some_and(|s| points_into(s, &forms))) {
                p.push(invalid(format!("environment variable {k} points into the suite folder")));
            }
        }
        if points_into(&input.cwd.to_string_lossy(), &forms) {
            p.push(invalid("the working folder lies in the suite folder"));
        }
        if !p.is_empty() {
            return Err(AdapterError { problems: p });
        }
        let program = if argv.is_empty() { String::new() } else { argv.remove(0) };
        let stdin = (self.prompt == PromptMode::Stdin).then(|| input.prompt.as_bytes().to_vec());
        Ok(LaunchPlan { program, args: argv, stdin, cwd: input.cwd.to_path_buf(), env, limits: input.limits })
    }
}

/// The values of the three placeholders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPaths {
    pub prompt_file: String,
    pub mcp_config: String,
    pub workdir: String,
}

impl AgentPaths {
    /// From native paths; fails with `invalid_adapter` if one is not UTF-8.
    pub fn new(prompt_file: &Path, mcp_config: &Path, workdir: &Path) -> Result<AgentPaths, AdapterError> {
        let s = |name: &str, p: &Path| {
            p.to_str()
                .map(str::to_string)
                .ok_or_else(|| AdapterError::one(invalid(format!("`{{{name}}}` path is not UTF-8"))))
        };
        Ok(AgentPaths {
            prompt_file: s("prompt_file", prompt_file)?,
            mcp_config: s("mcp_config", mcp_config)?,
            workdir: s("workdir", workdir)?,
        })
    }

    fn pairs(&self) -> [(&'static str, &str); 3] {
        [("prompt_file", &self.prompt_file), ("mcp_config", &self.mcp_config), ("workdir", &self.workdir)]
    }

    fn get(&self, name: &str) -> &str {
        match name {
            "prompt_file" => &self.prompt_file,
            "mcp_config" => &self.mcp_config,
            _ => &self.workdir,
        }
    }
}

/// Keeps only the [`ENV_ALLOWLIST`] variables (names compared ignoring ASCII case), sorted.
pub fn scrub_env(env: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> =
        env.iter().filter(|(k, _)| ENV_ALLOWLIST.iter().any(|a| a.eq_ignore_ascii_case(k))).cloned().collect();
    out.sort();
    out
}

/// Wall-time limit and output cap of one launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub wall_time: Duration,
    /// Bytes kept of each of stdout and stderr; the rest is read and discarded.
    pub output_cap: usize,
}

/// What [`Adapter::plan`] needs besides the adapter.
#[derive(Debug, Clone)]
pub struct LaunchInput<'a> {
    pub paths: AgentPaths,
    pub prompt: &'a str,
    /// The agent's working folder (normally the sandbox project).
    pub cwd: &'a Path,
    /// The candidate environment (normally `Sandbox::env`); scrubbed to [`ENV_ALLOWLIST`].
    pub env: &'a [(String, String)],
    /// The suite folder, which nothing given to the agent may point into.
    pub suite_dir: Option<&'a Path>,
    pub limits: Limits,
}

/// A fully resolved launch: program and arguments (never a shell command line), standard
/// input, working folder, the complete environment and the limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub program: String,
    pub args: Vec<String>,
    /// The prompt bytes for delivery `stdin`; otherwise standard input is empty.
    pub stdin: Option<Vec<u8>>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub limits: Limits,
}

/// The result of [`LaunchPlan::run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// The exit code, if the process exited normally.
    pub exit_code: Option<i32>,
    /// The wall-time limit was reached and the process was killed.
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

fn spawn_reader<R: Read + Send + 'static>(which: u8, mut r: R, cap: usize, tx: mpsc::Sender<(u8, Vec<u8>, bool)>) {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut truncated = false;
        let mut chunk = [0u8; 8192];
        loop {
            match r.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let room = cap.saturating_sub(buf.len());
                    let data = chunk.get(..n).unwrap_or(&[]);
                    let keep = data.get(..room.min(n)).unwrap_or(&[]);
                    buf.extend_from_slice(keep);
                    truncated |= keep.len() < n;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = tx.send((which, buf, truncated));
    });
}

impl LaunchPlan {
    /// The command: `program` run directly with `args` (no shell), the environment cleared and
    /// set to exactly [`LaunchPlan::env`], in [`LaunchPlan::cwd`], with piped stdout and stderr.
    pub fn command(&self) -> Command {
        let mut c = Command::new(&self.program);
        c.args(&self.args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .current_dir(&self.cwd)
            .stdin(if self.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        c
    }

    /// Runs the plan: writes the stdin prompt, captures at most `output_cap` bytes of each
    /// output stream, and kills the process when the wall time is reached. Only the direct
    /// child is killed; process-tree termination belongs to AGENT.HARNESS.RUN.
    pub fn run(&self) -> Result<RunOutcome, SandboxError> {
        let io = |e: std::io::Error| SandboxError::new(ErrorCode::Io, format!("{}: {e}", self.program));
        let mut child = self.command().spawn().map_err(io)?;
        let (tx, rx) = mpsc::channel();
        if let Some(out) = child.stdout.take() {
            spawn_reader(0, out, self.limits.output_cap, tx.clone());
        }
        if let Some(err) = child.stderr.take() {
            spawn_reader(1, err, self.limits.output_cap, tx.clone());
        }
        drop(tx);
        if let (Some(mut sin), Some(data)) = (child.stdin.take(), self.stdin.clone()) {
            std::thread::spawn(move || {
                let _ = sin.write_all(&data);
            });
        }
        let deadline = Instant::now() + self.limits.wall_time;
        let (status, timed_out) = loop {
            if let Some(s) = child.try_wait().map_err(io)? {
                break (Some(s), false);
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                break (child.wait().ok(), true);
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let mut out = RunOutcome {
            exit_code: if timed_out { None } else { status.and_then(|s| s.code()) },
            timed_out,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        };
        // A leftover grandchild can hold the pipes open: wait for the readers only briefly.
        let grace = Instant::now() + Duration::from_secs(2);
        for _ in 0..2 {
            match rx.recv_timeout(grace.saturating_duration_since(Instant::now())) {
                Ok((0, b, t)) => {
                    out.stdout = b;
                    out.stdout_truncated = t;
                }
                Ok((_, b, t)) => {
                    out.stderr = b;
                    out.stderr_truncated = t;
                }
                Err(_) => break,
            }
        }
        Ok(out)
    }
}

/// Checks an MCP server profile name: 1 to 64 characters of `[A-Za-z0-9._-]`, not starting
/// with `-` (it is passed as an argument).
pub fn check_profile(profile: &str) -> Result<(), SandboxError> {
    let ok = !profile.is_empty()
        && profile.len() <= 64
        && !profile.starts_with('-')
        && profile.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(SandboxError::new(ErrorCode::InvalidTask, format!("profile {profile:?} is not a safe profile name")))
    }
}

/// The programs in the generated MCP server command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCommand {
    /// The harness tap (`fid-eval`, whose `tap` subcommand relays stdio).
    pub tap: String,
    /// The product CLI (`fid`).
    pub fid: String,
}

impl Default for McpCommand {
    fn default() -> McpCommand {
        McpCommand { tap: "fid-eval".to_string(), fid: "fid".to_string() }
    }
}

/// The MCP client configuration given to the agent (REQ-002), as canonical JSON in the common
/// `mcpServers` shape: exactly one server, [`MCP_SERVER_NAME`], whose command is the tap in
/// front of `fid mcp --headless --server <profile> --project <project>`.
pub fn mcp_config(profile: &str, project: &Path, cmd: &McpCommand) -> Result<Vec<u8>, SandboxError> {
    check_profile(profile)?;
    let proj = project
        .to_str()
        .filter(|s| project.is_absolute() && !s.starts_with('-'))
        .ok_or_else(|| SandboxError::new(ErrorCode::InvalidWorkDir, "sandbox project path is not absolute UTF-8"))?;
    let s = |v: &str| CValue::Str(v.to_string());
    let args = ["tap", "--", cmd.fid.as_str(), "mcp", "--headless", "--server", profile, "--project", proj];
    let args: Vec<CValue> = args.iter().map(|a| CValue::Str((*a).to_string())).collect();
    let mut server = BTreeMap::new();
    server.insert("command".to_string(), s(cmd.tap.as_str()));
    server.insert("args".to_string(), CValue::Arr(args));
    let mut servers = BTreeMap::new();
    servers.insert(MCP_SERVER_NAME.to_string(), CValue::Obj(server));
    let mut top = BTreeMap::new();
    top.insert("mcpServers".to_string(), CValue::Obj(servers));
    fid_canon::to_canonical_bytes(&CValue::Obj(top)).map_err(|e| SandboxError::new(ErrorCode::Io, e.to_string()))
}

/// Tokens an agent reported, or `not_reported` (REQ-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tokens {
    Reported { tokens_in: u64, tokens_out: u64 },
    NotReported,
}

impl Tokens {
    /// `{"tokens_in": n, "tokens_out": n}` (exact: counts are at most 2^53), or both fields
    /// `"not_reported"`.
    pub fn to_value(&self) -> CValue {
        let (i, o) = match self {
            Tokens::Reported { tokens_in, tokens_out } => {
                (CValue::Num(*tokens_in as f64), CValue::Num(*tokens_out as f64))
            }
            Tokens::NotReported => {
                (CValue::Str("not_reported".to_string()), CValue::Str("not_reported".to_string()))
            }
        };
        let mut m = BTreeMap::new();
        m.insert("tokens_in".to_string(), i);
        m.insert("tokens_out".to_string(), o);
        CValue::Obj(m)
    }
}

fn token_count(v: Option<&CValue>) -> Option<u64> {
    match v {
        Some(CValue::Num(f)) if *f >= 0.0 && f.fract() == 0.0 && *f <= 9_007_199_254_740_992.0 => Some(*f as u64),
        _ => None,
    }
}

/// Parses a token report: exactly `{"tokens_in": n, "tokens_out": n}` with non-negative
/// integers up to 2^53. Anything else is [`Tokens::NotReported`].
pub fn parse_token_report(bytes: &[u8]) -> Tokens {
    let limit = usize::try_from(MAX_TOKEN_REPORT_BYTES).unwrap_or(usize::MAX);
    let Ok(CValue::Obj(m)) = fid_canon::parse_with_limits(bytes, limit, 4) else {
        return Tokens::NotReported;
    };
    if m.len() != 2 {
        return Tokens::NotReported;
    }
    match (token_count(m.get("tokens_in")), token_count(m.get("tokens_out"))) {
        (Some(tokens_in), Some(tokens_out)) => Tokens::Reported { tokens_in, tokens_out },
        _ => Tokens::NotReported,
    }
}

/// Reads the token report `name` in `workdir` after a run: a regular file (never through a
/// symbolic link) of at most [`MAX_TOKEN_REPORT_BYTES`]. Absent, unreadable or malformed
/// reports are [`Tokens::NotReported`].
pub fn read_token_report(workdir: &Path, name: &str) -> Tokens {
    if check_rel(name).is_err() || name.contains('/') {
        return Tokens::NotReported;
    }
    let p = workdir.join(name);
    match std::fs::symlink_metadata(&p) {
        Ok(m) if m.file_type().is_file() && m.len() <= MAX_TOKEN_REPORT_BYTES => {}
        _ => return Tokens::NotReported,
    }
    let mut buf = Vec::new();
    let read = std::fs::File::open(&p).and_then(|f| f.take(MAX_TOKEN_REPORT_BYTES + 1).read_to_end(&mut buf));
    match read {
        Ok(_) => parse_token_report(&buf),
        Err(_) => Tokens::NotReported,
    }
}

/// The suite folder of a task folder `<suite>/tasks/<id>`, if it has that shape.
pub fn suite_dir_of(task_dir: &Path) -> Option<PathBuf> {
    let parent = task_dir.parent()?;
    if parent.file_name()? == "tasks" {
        parent.parent().map(Path::to_path_buf)
    } else {
        None
    }
}
