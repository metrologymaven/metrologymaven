//! `cargo xtask plat-gpu invariance`: the cross-backend value-of-record invariance harness
//! (spec node `PLAT.GPU.FALLBK.INVAR`, CONSTITUTION C2 and §4).
//!
//! The same example project and command sequence (`platform/invariance-sequence.toml`) are
//! run once per backend override, each in a fresh copy of the project; for every journal
//! entry the command ID and result digest are collected into a [`DigestList`], and the lists
//! are compared. Any difference fails the run.
//!
//! # Fail closed
//!
//! A requested backend that cannot be initialised is [`InvarError::BackendUnavailable`] and is
//! never replaced by another backend: a backend the host OS cannot offer (for example `metal`
//! on Windows) is refused before anything runs, and a driver that reports a different backend
//! than the one requested is refused too. A run needs at least two backends and every
//! sequence step must appear in every list, so the harness cannot pass vacuously.
//!
//! # Driver contract
//!
//! The harness never links product crates; it drives the `fid` binary ([`FidDriver`]):
//!
//! ```text
//! <fid> --gpu-backend <backend> invariance-run --project <copy> --sequence <file> --json
//! ```
//!
//! Exit 0 with a digest list on stdout (`{"backend": "...", "entries": [{"index": n,
//! "command": "name@major", "result": "sha256:..."}]}`, one entry per journal entry in journal
//! order); exit 3 when the backend cannot be initialised. Exit 2 (usage error, which is what a
//! `fid` without `--gpu-backend` returns) is also treated as BackendUnavailable; any other
//! failure is [`InvarError::SequenceFailed`].
//!
//! # Report rules (D0)
//!
//! [`compare`] is a pure function of the lists, in the order given (recorded lists are read in
//! file-name order). The first position where the lists' entries (index, command, result) are
//! not all equal is the first difference. Entries there are grouped by value; the reference
//! group is the largest one (on a tie, the one appearing first), and the differing backends are
//! all others, in list order. The report text, LF line endings:
//!
//! ```text
//! plat-gpu invariance: EQUAL | DIVERGED
//! backends: <b1> <b2> ...
//! entries: <b1>=<n1> <b2>=<n2> ...
//! first difference: entry <index> command <command>     (DIVERGED only, from the reference)
//! differing backends: <b> ...                            (DIVERGED only)
//!   <b>: <index> <command> <result> | <b>: missing       (DIVERGED only, one per backend)
//! ```

use fid_canon::CValue;
use fid_types::{CommandId, Digest};
use std::collections::BTreeSet;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use toml::{Table, Value};

/// The shipped sequence, relative to the workspace root.
pub const DEFAULT_SEQUENCE: &str = "platform/invariance-sequence.toml";
/// Where example projects live (ECO.DATA.EX), relative to the workspace root.
pub const DEFAULT_EXAMPLES: &str = "examples";
/// The largest sequence file or digest list read; bigger inputs are refused.
pub const MAX_BYTES: u64 = 4 << 20;
/// The most entries one digest list may hold.
pub const MAX_ENTRIES: usize = 100_000;
/// The most steps a sequence may hold.
pub const MAX_STEPS: usize = 1_000;
/// Bounds of a project copy.
pub const MAX_COPY_FILES: usize = 100_000;
pub const MAX_COPY_BYTES: u64 = 2 << 30;
/// The `fid` exit code for "backend cannot be initialised".
pub const EXIT_BACKEND_UNAVAILABLE: i32 = 3;

/// A rendering backend override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Backend {
    Dx12,
    Vulkan,
    Metal,
    Reduced,
    Software,
}

impl Backend {
    pub const ALL: [Backend; 5] = [Backend::Dx12, Backend::Vulkan, Backend::Metal, Backend::Reduced, Backend::Software];

    /// The override name passed to `--gpu-backend`.
    pub fn name(self) -> &'static str {
        match self {
            Backend::Dx12 => "dx12",
            Backend::Vulkan => "vulkan",
            Backend::Metal => "metal",
            Backend::Reduced => "reduced",
            Backend::Software => "software",
        }
    }

    pub fn parse(s: &str) -> Option<Backend> {
        Backend::ALL.into_iter().find(|b| b.name() == s)
    }

    /// The host operating systems (`std::env::consts::OS` values) that can offer this backend,
    /// following the backend policy proposal of PLAT.GPU.POLICY; `None` means every OS.
    pub fn host_os(self) -> Option<&'static [&'static str]> {
        match self {
            Backend::Dx12 => Some(&["windows"]),
            Backend::Vulkan => Some(&["windows", "linux"]),
            Backend::Metal => Some(&["macos"]),
            Backend::Reduced | Backend::Software => None,
        }
    }

    /// Whether the host OS can offer this backend at all.
    pub fn available_on(self, os: &str) -> bool {
        match self.host_os() {
            Some(list) => list.contains(&os),
            None => true,
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Harness errors. `Input` is a usage or input-format error (exit 2); the rest exit 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvarError {
    /// The backend cannot be initialised; no other backend was substituted.
    BackendUnavailable { backend: String, reason: String },
    /// The sequence could not be run, or its journal lacks a step.
    SequenceFailed { backend: Option<String>, reason: String },
    /// The digest lists differ.
    Diverged { index: u64, command: String, backends: Vec<String> },
    /// Bad usage, an unreadable or malformed sequence or digest list.
    Input(String),
}

impl InvarError {
    pub fn exit_code(&self) -> u8 {
        match self {
            InvarError::Input(_) => 2,
            _ => 1,
        }
    }
}

impl fmt::Display for InvarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvarError::BackendUnavailable { backend, reason } => {
                write!(f, "BackendUnavailable {{ backend: {backend} }}: {reason}")
            }
            InvarError::SequenceFailed { backend: Some(b), reason } => {
                write!(f, "SequenceFailed {{ backend: {b} }}: {reason}")
            }
            InvarError::SequenceFailed { backend: None, reason } => write!(f, "SequenceFailed: {reason}"),
            InvarError::Diverged { index, command, backends } => {
                write!(f, "Diverged {{ index: {index}, command: {command}, backends: [{}] }}", backends.join(", "))
            }
            InvarError::Input(m) => write!(f, "INVAR-INPUT: {m}"),
        }
    }
}

fn input(m: impl Into<String>) -> InvarError {
    InvarError::Input(m.into())
}

/// Parses a comma-separated backend list: known names, no duplicates, at least two.
pub fn parse_backends(list: &str) -> Result<Vec<Backend>, InvarError> {
    let mut out: Vec<Backend> = Vec::new();
    for part in list.split(',').map(str::trim) {
        let b = Backend::parse(part).ok_or_else(|| {
            let known: Vec<&str> = Backend::ALL.iter().map(|b| b.name()).collect();
            input(format!("unknown backend {part:?} (known: {})", known.join(", ")))
        })?;
        if out.contains(&b) {
            return Err(input(format!("backend {b} given twice")));
        }
        out.push(b);
    }
    if out.len() < 2 {
        return Err(input("at least two backends are needed to compare"));
    }
    Ok(out)
}

/// The kind of a sequence step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StepKind {
    Pick,
    Lasso,
    Fit,
    Evaluation,
    Report,
}

impl StepKind {
    pub const ALL: [StepKind; 5] =
        [StepKind::Pick, StepKind::Lasso, StepKind::Fit, StepKind::Evaluation, StepKind::Report];

    pub fn name(self) -> &'static str {
        match self {
            StepKind::Pick => "pick",
            StepKind::Lasso => "lasso",
            StepKind::Fit => "fit",
            StepKind::Evaluation => "evaluation",
            StepKind::Report => "report",
        }
    }

    /// Command-name prefixes a step of this kind may use.
    fn prefixes(self) -> &'static [&'static str] {
        match self {
            StepKind::Pick => &["view.pick_point"],
            StepKind::Lasso => &["view.resolve_lasso", "geo.resolve_selection"],
            StepKind::Fit => &["fit."],
            StepKind::Evaluation => &["check."],
            StepKind::Report => &["report."],
        }
    }
}

/// One step of the sequence (its `args` are passed to `fid` unread).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub kind: StepKind,
    pub command: CommandId,
}

/// A parsed `invariance-sequence.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    pub project: String,
    pub steps: Vec<Step>,
}

fn valid_project_id(s: &str) -> bool {
    // ECO.DATA.EX: ^ex-[a-z0-9-]{3,40}$
    s.strip_prefix("ex-").is_some_and(|rest| {
        (3..=40).contains(&rest.len())
            && rest.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

/// Parses and checks a sequence file (REQ-001): schema version 1, an example project ID, and at
/// least one step of every [`StepKind`], each with a command ID of a matching family.
pub fn parse_sequence(text: &str) -> Result<Sequence, InvarError> {
    let table = text.parse::<Table>().map_err(|e| input(format!("sequence: {}", e.message())))?;
    for key in table.keys() {
        if !matches!(key.as_str(), "schema_version" | "project" | "step") {
            return Err(input(format!("sequence: unknown key {key:?}")));
        }
    }
    if table.get("schema_version").and_then(Value::as_integer) != Some(1) {
        return Err(input("sequence: schema_version must be 1"));
    }
    let project = match table.get("project").and_then(Value::as_str) {
        Some(p) if valid_project_id(p) => p.to_string(),
        _ => return Err(input("sequence: project must be an example ID (ex-...)")),
    };
    let raw = table.get("step").and_then(Value::as_array).ok_or_else(|| input("sequence: no [[step]]"))?;
    if raw.len() > MAX_STEPS {
        return Err(input(format!("sequence: more than {MAX_STEPS} steps")));
    }
    let mut steps = Vec::new();
    for (i, s) in raw.iter().enumerate() {
        let n = i + 1;
        let t = s.as_table().ok_or_else(|| input(format!("sequence: step {n} is not a table")))?;
        for key in t.keys() {
            if !matches!(key.as_str(), "kind" | "command" | "args") {
                return Err(input(format!("sequence: step {n}: unknown key {key:?}")));
            }
        }
        let kind_s = t.get("kind").and_then(Value::as_str).unwrap_or("");
        let kind = StepKind::ALL
            .into_iter()
            .find(|k| k.name() == kind_s)
            .ok_or_else(|| input(format!("sequence: step {n}: unknown kind {kind_s:?}")))?;
        let cmd_s = t.get("command").and_then(Value::as_str).unwrap_or("");
        let command =
            CommandId::parse(cmd_s).map_err(|_| input(format!("sequence: step {n}: bad command ID {cmd_s:?}")))?;
        if !kind.prefixes().iter().any(|p| command.name().starts_with(p)) {
            return Err(input(format!("sequence: step {n}: {command} is not a {} command", kind.name())));
        }
        if t.get("args").is_some_and(|a| !a.is_table()) {
            return Err(input(format!("sequence: step {n}: args must be a table")));
        }
        steps.push(Step { kind, command });
    }
    let missing: Vec<&str> =
        StepKind::ALL.into_iter().filter(|k| !steps.iter().any(|s| s.kind == *k)).map(StepKind::name).collect();
    if !missing.is_empty() {
        return Err(input(format!("sequence: no step of kind {}", missing.join(", "))));
    }
    Ok(Sequence { project, steps })
}

/// One journal entry's command ID and result digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestEntry {
    pub index: u64,
    pub command: CommandId,
    pub result: Digest,
}

/// The digests recorded for one backend, in journal order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestList {
    pub backend: Backend,
    pub entries: Vec<DigestEntry>,
}

fn obj_keys(o: &std::collections::BTreeMap<String, CValue>, allowed: &[&str], what: &str) -> Result<(), InvarError> {
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(input(format!("{what}: unknown key {k:?}")));
        }
    }
    Ok(())
}

/// Parses a digest list (JSON): known backend, 1 to [`MAX_ENTRIES`] entries with strictly
/// increasing integer indices, valid command IDs and `sha256:` digests.
pub fn parse_digest_list(bytes: &[u8]) -> Result<DigestList, InvarError> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(input(format!("digest list larger than {MAX_BYTES} bytes")));
    }
    let v = fid_canon::parse(bytes).map_err(|e| input(format!("digest list: {}", e.code())))?;
    let CValue::Obj(top) = v else {
        return Err(input("digest list: not an object"));
    };
    obj_keys(&top, &["backend", "entries"], "digest list")?;
    let backend = match top.get("backend") {
        Some(CValue::Str(s)) => {
            Backend::parse(s).ok_or_else(|| input(format!("digest list: unknown backend {s:?}")))?
        }
        _ => return Err(input("digest list: backend missing")),
    };
    let Some(CValue::Arr(raw)) = top.get("entries") else {
        return Err(input("digest list: entries missing"));
    };
    if raw.is_empty() || raw.len() > MAX_ENTRIES {
        return Err(input(format!("digest list {backend}: needs 1 to {MAX_ENTRIES} entries")));
    }
    let mut entries: Vec<DigestEntry> = Vec::with_capacity(raw.len());
    for (p, e) in raw.iter().enumerate() {
        let what = format!("digest list {backend}: entry {}", p + 1);
        let CValue::Obj(o) = e else {
            return Err(input(format!("{what}: not an object")));
        };
        obj_keys(o, &["index", "command", "result"], &what)?;
        let index = match o.get("index") {
            Some(CValue::Num(f)) if f.fract() == 0.0 && *f >= 0.0 && *f <= 9_007_199_254_740_991.0 => *f as u64,
            _ => return Err(input(format!("{what}: index must be a non-negative integer"))),
        };
        if entries.last().is_some_and(|prev| prev.index >= index) {
            return Err(input(format!("{what}: indices must increase")));
        }
        let command = match o.get("command") {
            Some(CValue::Str(s)) => CommandId::parse(s).map_err(|_| input(format!("{what}: bad command ID")))?,
            _ => return Err(input(format!("{what}: command missing"))),
        };
        let result = match o.get("result") {
            Some(CValue::Str(s)) => Digest::parse(s).map_err(|_| input(format!("{what}: bad result digest")))?,
            _ => return Err(input(format!("{what}: result missing"))),
        };
        entries.push(DigestEntry { index, command, result });
    }
    Ok(DigestList { backend, entries })
}

/// The first position where the lists differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstDiff {
    /// Journal index and command of the reference entry.
    pub index: u64,
    pub command: String,
    /// Backends whose entry differs from the reference, in list order.
    pub backends: Vec<String>,
    /// Every backend's entry at that position (`None`: its list is shorter).
    pub rows: Vec<(String, Option<DigestEntry>)>,
}

/// The comparison result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvarReport {
    pub equal: bool,
    /// Backend and entry count, in list order.
    pub lists: Vec<(String, usize)>,
    pub first_diff: Option<FirstDiff>,
}

/// Compares digest lists (REQ-003, REQ-005): a pure function of its input.
pub fn compare(lists: &[DigestList]) -> InvarReport {
    let summary: Vec<(String, usize)> = lists.iter().map(|l| (l.backend.name().to_string(), l.entries.len())).collect();
    let n = lists.iter().map(|l| l.entries.len()).max().unwrap_or(0);
    for p in 0..n {
        let row: Vec<Option<&DigestEntry>> = lists.iter().map(|l| l.entries.get(p)).collect();
        if row.windows(2).all(|w| w[0] == w[1]) {
            continue;
        }
        let mut groups: Vec<(Option<&DigestEntry>, Vec<usize>)> = Vec::new();
        for (i, e) in row.iter().enumerate() {
            match groups.iter_mut().find(|g| g.0 == *e) {
                Some(g) => g.1.push(i),
                None => groups.push((*e, vec![i])),
            }
        }
        let mut best = 0;
        for (gi, g) in groups.iter().enumerate() {
            if g.1.len() > groups.get(best).map_or(0, |b| b.1.len()) {
                best = gi;
            }
        }
        let (reference, members) = groups.get(best).map_or((None, Vec::new()), |g| (g.0, g.1.clone()));
        let reference = reference.or_else(|| row.iter().flatten().next().copied());
        let first = FirstDiff {
            index: reference.map_or(0, |e| e.index),
            command: reference.map_or_else(String::new, |e| e.command.to_string()),
            backends: (0..row.len())
                .filter(|i| !members.contains(i))
                .filter_map(|i| summary.get(i).map(|s| s.0.clone()))
                .collect(),
            rows: summary.iter().zip(&row).map(|(s, e)| (s.0.clone(), e.cloned())).collect(),
        };
        return InvarReport { equal: false, lists: summary, first_diff: Some(first) };
    }
    InvarReport { equal: true, lists: summary, first_diff: None }
}

impl InvarReport {
    /// The report text (see the module docs), byte-stable.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(if self.equal { "plat-gpu invariance: EQUAL\n" } else { "plat-gpu invariance: DIVERGED\n" });
        let names: Vec<&str> = self.lists.iter().map(|l| l.0.as_str()).collect();
        out.push_str(&format!("backends: {}\n", names.join(" ")));
        let counts: Vec<String> = self.lists.iter().map(|(b, n)| format!("{b}={n}")).collect();
        out.push_str(&format!("entries: {}\n", counts.join(" ")));
        if let Some(d) = &self.first_diff {
            out.push_str(&format!("first difference: entry {} command {}\n", d.index, d.command));
            out.push_str(&format!("differing backends: {}\n", d.backends.join(" ")));
            for (b, e) in &d.rows {
                match e {
                    Some(e) => out.push_str(&format!("  {b}: {} {} {}\n", e.index, e.command, e.result)),
                    None => out.push_str(&format!("  {b}: missing\n")),
                }
            }
        }
        out
    }

    /// `Diverged` when the lists differ.
    pub fn error(&self) -> Option<InvarError> {
        self.first_diff.as_ref().map(|d| InvarError::Diverged {
            index: d.index,
            command: d.command.clone(),
            backends: d.backends.clone(),
        })
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, InvarError> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| input(format!("cannot read {}: {e}", path.display())))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(input(format!("{} is larger than {MAX_BYTES} bytes", path.display())));
    }
    Ok(bytes)
}

/// Reads every `*.json` digest list in `dir`, in file-name order. Needs at least two lists
/// and no backend twice.
pub fn read_recorded(dir: &Path) -> Result<Vec<DigestList>, InvarError> {
    let rd = std::fs::read_dir(dir).map_err(|e| input(format!("cannot read {}: {e}", dir.display())))?;
    let mut files: Vec<PathBuf> = Vec::new();
    for e in rd {
        let p = e.map_err(|e| input(format!("cannot read {}: {e}", dir.display())))?.path();
        if p.extension().is_some_and(|x| x == "json") && p.is_file() {
            files.push(p);
        }
    }
    files.sort();
    let mut lists = Vec::new();
    let mut seen = BTreeSet::new();
    for f in &files {
        let l = parse_digest_list(&read_bounded(f)?)
            .map_err(|e| input(format!("{}: {}", f.display(), e.to_string().trim_start_matches("INVAR-INPUT: "))))?;
        if !seen.insert(l.backend) {
            return Err(input(format!("{}: backend {} recorded twice", dir.display(), l.backend)));
        }
        lists.push(l);
    }
    if lists.len() < 2 {
        return Err(input(format!("{}: at least two digest lists are needed", dir.display())));
    }
    Ok(lists)
}

/// Runs the sequence on one backend in a prepared project copy.
pub trait Driver {
    fn run(&mut self, backend: Backend, project: &Path, sequence: &Path) -> Result<DigestList, InvarError>;
}

/// Drives the `fid` binary (see the module docs for the contract).
#[derive(Debug, Clone)]
pub struct FidDriver {
    pub fid: PathBuf,
}

fn first_line(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(200).collect()
}

impl Driver for FidDriver {
    fn run(&mut self, backend: Backend, project: &Path, sequence: &Path) -> Result<DigestList, InvarError> {
        let out = Command::new(&self.fid)
            .arg("--gpu-backend")
            .arg(backend.name())
            .arg("invariance-run")
            .arg("--project")
            .arg(project)
            .arg("--sequence")
            .arg(sequence)
            .arg("--json")
            .output()
            .map_err(|e| InvarError::SequenceFailed {
                backend: Some(backend.name().into()),
                reason: format!("cannot start {}: {e}", self.fid.display()),
            })?;
        match out.status.code() {
            Some(0) => parse_digest_list(&out.stdout).map_err(|e| InvarError::SequenceFailed {
                backend: Some(backend.name().into()),
                reason: format!("unreadable digest list from fid: {e}"),
            }),
            Some(code @ (2 | EXIT_BACKEND_UNAVAILABLE)) => Err(InvarError::BackendUnavailable {
                backend: backend.name().into(),
                reason: format!("fid exited {code}: {}", first_line(&out.stderr)),
            }),
            code => Err(InvarError::SequenceFailed {
                backend: Some(backend.name().into()),
                reason: format!(
                    "fid exited {}: {}",
                    code.map_or_else(|| "without a code".into(), |c| c.to_string()),
                    first_line(&out.stderr)
                ),
            }),
        }
    }
}

/// Copies a project directory tree (no symbolic links; bounded). `dst` must not exist.
fn copy_tree(src: &Path, dst: &Path) -> Result<(), InvarError> {
    let io = |p: &Path, e: std::io::Error| InvarError::SequenceFailed {
        backend: None,
        reason: format!("cannot copy project {}: {e}", p.display()),
    };
    if dst.exists() {
        return Err(input(format!("work folder {} is not fresh", dst.display())));
    }
    let (mut files, mut bytes) = (0usize, 0u64);
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((s, d)) = stack.pop() {
        std::fs::create_dir_all(&d).map_err(|e| io(&d, e))?;
        let mut children: Vec<_> =
            std::fs::read_dir(&s).map_err(|e| io(&s, e))?.collect::<Result<Vec<_>, _>>().map_err(|e| io(&s, e))?;
        children.sort_by_key(std::fs::DirEntry::file_name);
        for c in children {
            let meta = std::fs::symlink_metadata(c.path()).map_err(|e| io(&c.path(), e))?;
            let target = d.join(c.file_name());
            if meta.is_dir() {
                stack.push((c.path(), target));
            } else if meta.is_file() {
                files += 1;
                bytes = bytes.saturating_add(meta.len());
                if files > MAX_COPY_FILES || bytes > MAX_COPY_BYTES {
                    return Err(InvarError::SequenceFailed {
                        backend: None,
                        reason: format!("project {} exceeds the copy bounds", src.display()),
                    });
                }
                std::fs::copy(c.path(), &target).map_err(|e| io(&c.path(), e))?;
            } else {
                return Err(InvarError::SequenceFailed {
                    backend: None,
                    reason: format!("project contains a link or special file: {}", c.path().display()),
                });
            }
        }
    }
    Ok(())
}

/// Inputs of one harness run.
#[derive(Debug, Clone)]
pub struct RunOptions<'a> {
    pub sequence: &'a Path,
    pub examples: &'a Path,
    pub work: &'a Path,
    pub backends: &'a [Backend],
    /// The host OS (`std::env::consts::OS`), a parameter so the check is testable.
    pub host_os: &'a str,
}

/// Runs the sequence once per backend (REQ-002, REQ-004) and compares the lists. Order: read
/// and check the sequence; refuse any backend the host cannot offer; find the project; then per
/// backend copy the project fresh into `<work>/<backend>/<project>` and run the driver.
pub fn run(opts: &RunOptions<'_>, driver: &mut dyn Driver) -> Result<InvarReport, InvarError> {
    let text = String::from_utf8(read_bounded(opts.sequence)?)
        .map_err(|_| input(format!("{} is not UTF-8", opts.sequence.display())))?;
    let seq = parse_sequence(&text)?;
    if opts.backends.len() < 2 {
        return Err(input("at least two backends are needed to compare"));
    }
    for b in opts.backends {
        if !b.available_on(opts.host_os) {
            let offers = b.host_os().map_or_else(|| "every OS".to_string(), |l| l.join(", "));
            return Err(InvarError::BackendUnavailable {
                backend: b.name().into(),
                reason: format!("not offered on {} (only on {offers})", opts.host_os),
            });
        }
    }
    let project = opts.examples.join(&seq.project);
    if !project.is_dir() {
        return Err(InvarError::SequenceFailed {
            backend: None,
            reason: format!("example project {} not found at {}", seq.project, project.display()),
        });
    }
    let mut lists = Vec::new();
    for &b in opts.backends {
        let copy = opts.work.join(b.name()).join(&seq.project);
        copy_tree(&project, &copy)?;
        let list = driver.run(b, &copy, opts.sequence)?;
        if list.backend != b {
            return Err(InvarError::BackendUnavailable {
                backend: b.name().into(),
                reason: format!("the run reported backend {} instead; substitution is refused", list.backend),
            });
        }
        let mut entries = list.entries.iter();
        for (i, step) in seq.steps.iter().enumerate() {
            if !entries.any(|e| e.command == step.command) {
                return Err(InvarError::SequenceFailed {
                    backend: Some(b.name().into()),
                    reason: format!("journal has no entry for step {} ({})", i + 1, step.command),
                });
            }
        }
        lists.push(list);
    }
    Ok(compare(&lists))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn entry(index: u64, cmd: &str, byte: u8) -> DigestEntry {
        DigestEntry { index, command: CommandId::parse(cmd).unwrap(), result: Digest::from_bytes([byte; 32]) }
    }

    #[test]
    fn shorter_list_is_missing_and_named() {
        let a = DigestList { backend: Backend::Dx12, entries: vec![entry(1, "a.b@1", 1), entry(2, "a.c@1", 2)] };
        let b = DigestList { backend: Backend::Software, entries: vec![entry(1, "a.b@1", 1)] };
        let r = compare(&[a, b]);
        let d = r.first_diff.clone().unwrap();
        // Tie of one against one: the first group (dx12) is the reference.
        assert_eq!((d.index, d.command.as_str(), d.backends), (2, "a.c@1", vec!["software".to_string()]));
        assert!(r.render().contains("  software: missing\n"));
    }

    #[test]
    fn majority_is_reference_even_when_first_differs() {
        let l = |b, byte| DigestList { backend: b, entries: vec![entry(1, "a.b@1", byte)] };
        let r = compare(&[l(Backend::Dx12, 9), l(Backend::Vulkan, 1), l(Backend::Software, 1)]);
        assert_eq!(r.first_diff.unwrap().backends, vec!["dx12".to_string()]);
    }

    #[test]
    fn backend_list_rules() {
        assert_eq!(parse_backends("dx12,vulkan,software").unwrap().len(), 3);
        assert!(matches!(parse_backends("software"), Err(InvarError::Input(_))));
        assert!(matches!(parse_backends("software,software"), Err(InvarError::Input(_))));
        assert!(matches!(parse_backends("gl,software"), Err(InvarError::Input(_))));
    }

    #[test]
    fn digest_list_rejects_bad_input() {
        let bad = [
            r#"{"backend":"dx12","entries":[]}"#,
            r#"{"backend":"gl","entries":[{"index":1,"command":"a.b@1","result":"sha256:00"}]}"#,
            r#"{"backend":"dx12","entries":[{"index":1.5,"command":"a.b@1","result":"x"}]}"#,
            r#"{"backend":"dx12","entries":[{"index":1,"command":"A@1","result":"x"}]}"#,
            r#"{"backend":"dx12","entries":[],"extra":1}"#,
        ];
        for b in bad {
            assert!(matches!(parse_digest_list(b.as_bytes()), Err(InvarError::Input(_))), "{b}");
        }
        let d = format!("sha256:{}", "0".repeat(64));
        let dup = format!(
            r#"{{"backend":"dx12","entries":[{{"index":2,"command":"a.b@1","result":"{d}"}},{{"index":2,"command":"a.b@1","result":"{d}"}}]}}"#
        );
        assert!(matches!(parse_digest_list(dup.as_bytes()), Err(InvarError::Input(_))));
    }

    #[test]
    fn host_table() {
        assert!(!Backend::Metal.available_on("windows"));
        assert!(!Backend::Dx12.available_on("linux"));
        assert!(Backend::Software.available_on("windows") && Backend::Reduced.available_on("macos"));
    }
}
