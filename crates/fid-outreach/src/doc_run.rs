//! `fid-outreach doc-run --guide <file> --repo <dir> [--json]`: the contributor guide checker
//! (spec node `OUTR.DOCS.CONTRIB`).
//!
//! The guide is Markdown with a front matter block that states `revision: <git commit id>`.
//! Every fenced code block whose info string is `sh run` (or `sh run expect-fail`) is
//! runnable. The checker:
//!
//! 1. reads the guide (at most [`MAX_GUIDE_BYTES`]) and parses its runnable blocks;
//! 2. compares `revision:` with `git rev-parse HEAD` of `--repo` and stops with
//!    `revision_mismatch` (exit 1) before running anything when they differ;
//! 3. exports that commit into a new temporary folder through a private index file
//!    (`GIT_INDEX_FILE`, `read-tree` + `checkout-index`), so the starting repository, its
//!    index and its working tree are only read;
//! 4. runs each block's lines in document order with the temporary tree as working folder
//!    and a per-block time limit (60 s by default);
//! 5. deletes the temporary folder (also on every early return) and reports.
//!
//! Lines are run **without a shell**: each non-empty line that does not start with `#` is
//! split into words at whitespace (single and double quotes group words), and the first word
//! is the program. Shell syntax (pipes, redirection, variables, globs, `cd`, environment
//! assignments, backslashes) is rejected as `guide_invalid`, so a line means the same on
//! every platform. A block passes when every line exits 0; an `expect-fail` block passes when
//! a line exits non-zero (the remaining lines are skipped). A program that cannot be started
//! fails its block, also in an `expect-fail` block.
//!
//! Exit codes: 0 all blocks pass; 1 revision mismatch or any failing block; 2 unreadable or
//! invalid guide, unreadable repository, or no temporary folder.

use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Largest guide accepted, in bytes.
pub const MAX_GUIDE_BYTES: u64 = 1024 * 1024;
/// Default time limit for one block.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
/// How much of the end of a failing command's stdout and stderr the report keeps, in bytes.
pub const TAIL_BYTES: usize = 2048;
/// Characters that only a shell would interpret; rejected outside quotes.
const SHELL_CHARS: [char; 14] = ['|', '&', ';', '<', '>', '$', '`', '(', ')', '*', '?', '\\', '~', '#'];
const POLL: Duration = Duration::from_millis(20);
/// How long to wait for a killed process's output pipes to close.
const DRAIN_WAIT: Duration = Duration::from_secs(2);

/// Errors that stop the checker with exit 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocRunError {
    /// The guide cannot be read, is not UTF-8, or is larger than [`MAX_GUIDE_BYTES`].
    GuideUnreadable { path: String, reason: String },
    /// The guide has no usable `revision:`, a malformed runnable block, or shell syntax.
    GuideInvalid { path: String, line: u32, reason: String },
    /// `git` cannot read the repository, or the commit cannot be exported.
    RepoUnreadable { path: String, reason: String },
    /// No temporary folder could be created.
    TempDir { reason: String },
}

impl DocRunError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            DocRunError::GuideUnreadable { .. } => "guide_unreadable",
            DocRunError::GuideInvalid { .. } => "guide_invalid",
            DocRunError::RepoUnreadable { .. } => "repo_unreadable",
            DocRunError::TempDir { .. } => "temp_dir",
        }
    }
}

impl fmt::Display for DocRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocRunError::GuideUnreadable { path, reason } => write!(f, "{path}: guide_unreadable: {reason}"),
            DocRunError::GuideInvalid { path, line, reason } => write!(f, "{path}:{line}: guide_invalid: {reason}"),
            DocRunError::RepoUnreadable { path, reason } => write!(f, "{path}: repo_unreadable: {reason}"),
            DocRunError::TempDir { reason } => write!(f, "temp_dir: {reason}"),
        }
    }
}

/// The JSON object printed for an error (`--json`).
pub fn error_json(e: &DocRunError) -> Vec<u8> {
    let mut o = BTreeMap::new();
    o.insert("error".to_string(), CValue::Str(e.code().to_string()));
    o.insert("message".to_string(), CValue::Str(e.to_string()));
    o.insert("ok".to_string(), CValue::Bool(false));
    fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
}

/// One command line of a runnable block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLine {
    /// 1-based line in the guide.
    pub line: u32,
    /// The line as written (trimmed).
    pub text: String,
    /// Program and arguments.
    pub argv: Vec<String>,
}

/// One runnable block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// 1-based line of the opening fence.
    pub line: u32,
    /// `sh run expect-fail`: the block documents an error and must exit non-zero.
    pub expect_fail: bool,
    pub lines: Vec<RunLine>,
}

/// A parsed guide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guide {
    /// The `revision:` of the front matter (a full lowercase commit ID).
    pub revision: String,
    /// Runnable blocks in document order.
    pub blocks: Vec<Block>,
}

type ParseResult<T> = Result<T, (u32, String)>;

fn line_no(i: usize) -> u32 {
    u32::try_from(i + 1).unwrap_or(u32::MAX)
}

/// Parses a guide. Errors carry the 1-based line and the reason.
pub fn parse_guide(text: &str) -> Result<Guide, (u32, String)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let (revision, body_start) = front_matter_revision(&lines)?;
    let mut blocks = Vec::new();
    let mut i = body_start;
    while i < lines.len() {
        let Some((ch, n, info)) = fence_open(lines[i]) else {
            i += 1;
            continue;
        };
        let open = line_no(i);
        let mut j = i + 1;
        while j < lines.len() && !fence_close(lines[j], ch, n) {
            j += 1;
        }
        if j >= lines.len() {
            return Err((open, "code fence is never closed".to_string()));
        }
        if let Some(expect_fail) = runnable(info).map_err(|m| (open, m))? {
            let mut run_lines = Vec::new();
            for (k, raw) in lines.iter().enumerate().take(j).skip(i + 1) {
                let t = raw.trim();
                if t.is_empty() || t.starts_with('#') {
                    continue;
                }
                let argv = split_command(t).map_err(|m| (line_no(k), m))?;
                run_lines.push(RunLine { line: line_no(k), text: t.to_string(), argv });
            }
            if run_lines.is_empty() {
                return Err((open, "runnable block has no command".to_string()));
            }
            blocks.push(Block { line: open, expect_fail, lines: run_lines });
        }
        i = j + 1;
    }
    Ok(Guide { revision, blocks })
}

/// Returns the front matter's revision and the index of the first line after the front matter.
fn front_matter_revision(lines: &[&str]) -> ParseResult<(String, usize)> {
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return Err((1, "the guide must start with a front matter block (`---`) stating `revision:`".to_string()));
    }
    let Some(end) = lines.iter().skip(1).position(|l| l.trim_end() == "---").map(|p| p + 1) else {
        return Err((1, "front matter block is never closed (`---`)".to_string()));
    };
    let mut revision: Option<(usize, String)> = None;
    for (i, l) in lines.iter().enumerate().take(end).skip(1) {
        if let Some(v) = l.strip_prefix("revision:") {
            if revision.is_some() {
                return Err((line_no(i), "duplicate `revision:`".to_string()));
            }
            let v = v.trim();
            let v = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(v);
            revision = Some((i, v.to_string()));
        }
    }
    let Some((i, rev)) = revision else {
        return Err((1, "front matter has no `revision:`".to_string()));
    };
    let hex = rev.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !(hex && (rev.len() == 40 || rev.len() == 64)) {
        return Err((line_no(i), format!("`revision: {rev}` is not a full lowercase git commit ID")));
    }
    Ok((rev, end + 1))
}

/// An opening fence: up to 3 spaces, then 3 or more backticks or tildes, then the info string.
fn fence_open(line: &str) -> Option<(char, usize, &str)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let ch = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|c| *c == ch).count();
    if n < 3 {
        return None;
    }
    let info = t[n..].trim();
    if ch == '`' && info.contains('`') {
        return None;
    }
    Some((ch, n, info))
}

fn fence_close(line: &str, ch: char, n: usize) -> bool {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return false;
    }
    let run = t.chars().take_while(|c| *c == ch).count();
    run >= n && t[run..].trim().is_empty()
}

/// `Some(expect_fail)` for a runnable info string, `None` for any other block.
fn runnable(info: &str) -> Result<Option<bool>, String> {
    let words: Vec<&str> = info.split_whitespace().collect();
    match words.as_slice() {
        ["sh", "run"] => Ok(Some(false)),
        ["sh", "run", "expect-fail"] => Ok(Some(true)),
        ["sh", "run", ..] => Err(format!("unknown runnable block `{info}`; expected `sh run` or `sh run expect-fail`")),
        _ => Ok(None),
    }
}

/// Splits one command line into words; quotes group words and are removed.
pub fn split_command(s: &str) -> Result<Vec<String>, String> {
    let mut argv = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                in_word = true;
            }
            None if c.is_whitespace() => {
                if in_word {
                    argv.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            None if SHELL_CHARS.contains(&c) => {
                return Err(format!(
                    "`{c}` needs a shell, but runnable lines run without one (quote it to pass it as text)"
                ));
            }
            None => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return Err("unclosed quote".to_string());
    }
    if in_word {
        argv.push(cur);
    }
    match argv.first() {
        None => Err("empty command".to_string()),
        Some(p) if p.contains('=') => Err("environment assignments need a shell".to_string()),
        Some(p) if p == "cd" => Err("`cd` needs a shell; every line runs in the repository root".to_string()),
        Some(_) => Ok(argv),
    }
}

/// Checker inputs.
#[derive(Debug, Clone)]
pub struct Options {
    pub guide: PathBuf,
    pub repo: PathBuf,
    /// Time limit per block.
    pub timeout: Duration,
}

/// How a block ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Passed,
    /// A line exited non-zero (or, for `expect-fail`, every line exited 0).
    Failed,
    /// The block ran longer than the time limit; the running process was killed.
    TimedOut,
    /// A program could not be started.
    SpawnFailed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Passed => "passed",
            Status::Failed => "failed",
            Status::TimedOut => "timeout",
            Status::SpawnFailed => "spawn_failed",
        }
    }
}

/// The result of one block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockResult {
    /// Line of the opening fence.
    pub line: u32,
    pub expect_fail: bool,
    pub status: Status,
    /// Line and text of the last command started.
    pub command_line: u32,
    pub command: String,
    /// Exit code of the last command (None when it was killed, stopped by a signal, or never started).
    pub exit_code: Option<i32>,
    /// End of stdout and stderr of the last command (empty for passing blocks) or the start error.
    pub stdout_tail: String,
    pub stderr_tail: String,
}

/// The checker report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub guide: String,
    /// The revision the guide states.
    pub revision: String,
    /// `git rev-parse HEAD` of the repository.
    pub head: String,
    pub blocks: Vec<BlockResult>,
    /// The temporary folder used (None when nothing ran).
    pub workdir: Option<String>,
    /// Whether the temporary folder was deleted.
    pub workdir_removed: bool,
}

impl Report {
    pub fn revision_mismatch(&self) -> bool {
        self.revision != self.head
    }

    pub fn failed(&self) -> usize {
        self.blocks.iter().filter(|b| b.status != Status::Passed).count()
    }

    /// 0 when every block passed, 1 on a revision mismatch or any failing block.
    pub fn exit_code(&self) -> u8 {
        u8::from(self.revision_mismatch() || self.failed() > 0)
    }

    fn error(&self) -> Option<&'static str> {
        if self.revision_mismatch() {
            Some("revision_mismatch")
        } else if self.failed() > 0 {
            Some("block_failed")
        } else {
            None
        }
    }

    /// Canonical JSON report.
    pub fn to_json(&self) -> Vec<u8> {
        let num = |n: u32| CValue::Num(f64::from(n));
        let blocks = self
            .blocks
            .iter()
            .map(|b| {
                let mut o = BTreeMap::new();
                o.insert("command".to_string(), CValue::Str(b.command.clone()));
                o.insert("command_line".to_string(), num(b.command_line));
                o.insert("exit_code".to_string(), b.exit_code.map_or(CValue::Null, |c| CValue::Num(f64::from(c))));
                o.insert("expect_fail".to_string(), CValue::Bool(b.expect_fail));
                o.insert("line".to_string(), num(b.line));
                o.insert("status".to_string(), CValue::Str(b.status.as_str().to_string()));
                o.insert("stderr_tail".to_string(), CValue::Str(b.stderr_tail.clone()));
                o.insert("stdout_tail".to_string(), CValue::Str(b.stdout_tail.clone()));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("blocks".to_string(), CValue::Arr(blocks));
        o.insert("error".to_string(), self.error().map_or(CValue::Null, |e| CValue::Str(e.to_string())));
        o.insert("guide".to_string(), CValue::Str(self.guide.clone()));
        o.insert("head".to_string(), CValue::Str(self.head.clone()));
        o.insert("ok".to_string(), CValue::Bool(self.exit_code() == 0));
        o.insert("revision".to_string(), CValue::Str(self.revision.clone()));
        o.insert("workdir".to_string(), self.workdir.clone().map_or(CValue::Null, CValue::Str));
        o.insert("workdir_removed".to_string(), CValue::Bool(self.workdir_removed));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }

    /// Text report: one line per block (`<guide>:<line>: <status>`), then a summary.
    pub fn render(&self) -> String {
        let mut s = String::new();
        if self.revision_mismatch() {
            s.push_str(&format!(
                "{}: revision_mismatch: the guide documents {}, but the repository is at {}; no block was run\n",
                self.guide, self.revision, self.head
            ));
            return s;
        }
        for b in &self.blocks {
            s.push_str(&format!("{}:{}: {}", self.guide, b.line, b.status.as_str()));
            if b.status != Status::Passed {
                let code = b.exit_code.map_or_else(|| "none".to_string(), |c| c.to_string());
                s.push_str(&format!(" (line {}: `{}`, exit code {code})", b.command_line, b.command));
                if b.status == Status::Failed && b.expect_fail {
                    s.push_str(" expected a non-zero exit");
                }
            }
            s.push('\n');
            if b.status != Status::Passed {
                for l in b.stdout_tail.lines().chain(b.stderr_tail.lines()) {
                    s.push_str(&format!("    {l}\n"));
                }
            }
        }
        if let (false, Some(w)) = (self.workdir_removed, &self.workdir) {
            s.push_str(&format!("warning: temporary folder {w} could not be deleted\n"));
        }
        s.push_str(&format!("doc-run: {} block(s), {} failed\n", self.blocks.len(), self.failed()));
        s
    }
}

/// Runs the checker. See the module documentation.
pub fn run(opts: &Options) -> Result<Report, DocRunError> {
    let guide_name = opts.guide.display().to_string();
    let text = read_guide(&opts.guide)?;
    let guide = parse_guide(&text).map_err(|(line, reason)| DocRunError::GuideInvalid {
        path: guide_name.clone(),
        line,
        reason,
    })?;
    let repo_name = opts.repo.display().to_string();
    let repo_err = |reason: String| DocRunError::RepoUnreadable { path: repo_name.clone(), reason };
    let head = git(&opts.repo, &["rev-parse", "HEAD"], None).map_err(repo_err)?;
    let mut report = Report {
        guide: guide_name,
        revision: guide.revision.clone(),
        head: head.clone(),
        blocks: Vec::new(),
        workdir: None,
        workdir_removed: false,
    };
    if report.revision_mismatch() {
        return Ok(report);
    }
    let git_dir = git(&opts.repo, &["rev-parse", "--absolute-git-dir"], None).map_err(repo_err)?;
    let tmp = TempDir::new()?;
    let tree = tmp.root.join("tree");
    std::fs::create_dir(&tree).map_err(|e| DocRunError::TempDir { reason: format!("{}: {e}", tree.display()) })?;
    export(Path::new(&git_dir), &tree, &tmp.root.join("index"), &head).map_err(repo_err)?;
    for b in &guide.blocks {
        report.blocks.push(run_block(b, &tree, opts.timeout));
    }
    report.workdir = Some(tmp.root.display().to_string());
    report.workdir_removed = tmp.remove();
    Ok(report)
}

fn read_guide(path: &Path) -> Result<String, DocRunError> {
    let err = |reason: String| DocRunError::GuideUnreadable { path: path.display().to_string(), reason };
    let f = std::fs::File::open(path).map_err(|e| err(e.to_string()))?;
    let mut bytes = Vec::new();
    f.take(MAX_GUIDE_BYTES + 1).read_to_end(&mut bytes).map_err(|e| err(e.to_string()))?;
    if bytes.len() as u64 > MAX_GUIDE_BYTES {
        return Err(err(format!("larger than {MAX_GUIDE_BYTES} bytes")));
    }
    String::from_utf8(bytes).map_err(|_| err("not UTF-8".to_string()))
}

/// Runs `git` with `args` in `cwd` (optionally with a private index file); returns trimmed stdout.
fn git(cwd: &Path, args: &[&str], index: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(cwd).stdin(Stdio::null());
    if let Some(i) = index {
        cmd.env("GIT_INDEX_FILE", i);
    }
    let out = cmd.output().map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("git {} failed: {msg}", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Writes the files of `commit` into `tree` without touching the repository's own index or
/// working tree: the tree is read into a private index file, which is then checked out.
fn export(git_dir: &Path, tree: &Path, index: &Path, commit: &str) -> Result<(), String> {
    let gd = format!("--git-dir={}", git_dir.display());
    let wt = format!("--work-tree={}", tree.display());
    git(tree, &[gd.as_str(), wt.as_str(), "read-tree", commit], Some(index))?;
    git(tree, &[gd.as_str(), wt.as_str(), "checkout-index", "--all", "--force"], Some(index))?;
    Ok(())
}

/// A temporary folder, deleted by [`TempDir::remove`] or, on early returns, on drop.
struct TempDir {
    root: PathBuf,
    removed: bool,
}

impl TempDir {
    fn new() -> Result<TempDir, DocRunError> {
        let base = std::env::temp_dir();
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let mut last = String::new();
        for attempt in 0..100u32 {
            let root = base.join(format!("fid-doc-run-{}-{nanos}-{attempt}", std::process::id()));
            match std::fs::create_dir(&root) {
                Ok(()) => return Ok(TempDir { root, removed: false }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = e.to_string(),
                Err(e) => return Err(DocRunError::TempDir { reason: format!("{}: {e}", root.display()) }),
            }
        }
        Err(DocRunError::TempDir { reason: format!("{}: {last}", base.display()) })
    }

    /// Deletes the folder; true when it is gone.
    fn remove(mut self) -> bool {
        self.removed = true;
        delete_tree(&self.root)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.removed {
            let _ = delete_tree(&self.root);
        }
    }
}

fn delete_tree(root: &Path) -> bool {
    // A process killed on timeout may release its files a moment later (Windows).
    for _ in 0..5 {
        if std::fs::remove_dir_all(root).is_ok() || !root.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    !root.exists()
}

enum LineOutcome {
    Exited(Option<i32>),
    TimedOut,
    SpawnFailed(String),
}

struct LineResult {
    outcome: LineOutcome,
    stdout: String,
    stderr: String,
}

fn run_block(block: &Block, dir: &Path, timeout: Duration) -> BlockResult {
    let deadline = Instant::now() + timeout;
    let mut result = BlockResult {
        line: block.line,
        expect_fail: block.expect_fail,
        status: Status::Passed,
        command_line: block.line,
        command: String::new(),
        exit_code: None,
        stdout_tail: String::new(),
        stderr_tail: String::new(),
    };
    for cmd in &block.lines {
        result.command_line = cmd.line;
        result.command = cmd.text.clone();
        let r = run_line(cmd, dir, deadline);
        let (status, code) = match r.outcome {
            LineOutcome::Exited(Some(0)) => {
                result.exit_code = Some(0);
                continue;
            }
            LineOutcome::Exited(code) if block.expect_fail => {
                result.exit_code = code;
                return result;
            }
            LineOutcome::Exited(code) => (Status::Failed, code),
            LineOutcome::TimedOut => (Status::TimedOut, None),
            LineOutcome::SpawnFailed(msg) => {
                result.status = Status::SpawnFailed;
                result.stderr_tail = format!("cannot start `{}`: {msg}", cmd.argv.first().map_or("", String::as_str));
                return result;
            }
        };
        result.status = status;
        result.exit_code = code;
        result.stdout_tail = r.stdout;
        result.stderr_tail = r.stderr;
        return result;
    }
    if block.expect_fail {
        result.status = Status::Failed;
    }
    result
}

fn run_line(cmd: &RunLine, dir: &Path, deadline: Instant) -> LineResult {
    let fail = |msg: String| LineResult {
        outcome: LineOutcome::SpawnFailed(msg),
        stdout: String::new(),
        stderr: String::new(),
    };
    let Some((prog, args)) = cmd.argv.split_first() else {
        return fail("empty command".to_string());
    };
    let spawned = Command::new(prog)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return fail(e.to_string()),
    };
    let out_rx = drain(child.stdout.take());
    let err_rx = drain(child.stderr.take());
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(st)) => break LineOutcome::Exited(st.code()),
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break LineOutcome::SpawnFailed(format!("wait: {e}"));
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break LineOutcome::TimedOut;
        }
        std::thread::sleep(POLL);
    };
    LineResult { outcome, stdout: collect(&out_rx), stderr: collect(&err_rx) }
}

/// Reads a pipe to its end on a helper thread, keeping only the last [`TAIL_BYTES`].
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    if let Some(mut r) = pipe {
        let spawned = std::thread::Builder::new().name("doc-run-pipe".to_string()).spawn(move || {
            let mut tail: Vec<u8> = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                match r.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        tail.extend_from_slice(buf.get(..n).unwrap_or(&[]));
                        if tail.len() > 2 * TAIL_BYTES {
                            tail.drain(..tail.len() - TAIL_BYTES);
                        }
                    }
                }
            }
            let _ = tx.send(tail);
        });
        // Without a reader thread the receiver reports a closed channel; the tail stays empty.
        let _ = spawned;
    }
    rx
}

/// The tail a reader thread collected; empty when the pipe stays open (for example held by a
/// leftover child of a killed process) longer than [`DRAIN_WAIT`].
fn collect(rx: &mpsc::Receiver<Vec<u8>>) -> String {
    let mut bytes = rx.recv_timeout(DRAIN_WAIT).unwrap_or_default();
    if bytes.len() > TAIL_BYTES {
        bytes.drain(..bytes.len() - TAIL_BYTES);
    }
    String::from_utf8_lossy(&bytes).trim().to_string()
}
