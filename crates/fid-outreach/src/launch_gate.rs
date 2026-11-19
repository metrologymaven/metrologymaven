//! Milestone gate check and report (spec node `OUTR.LAUNCH.GATE`).
//!
//! `gate-check` evaluates every criterion of one milestone of the criteria file (parsed with
//! [`crate::launch_plan`]) for one release candidate, from local files only: the spec tree's
//! `NODE.md` frontmatter, the parity trace and status report, the CI test report, the committed
//! sign-off records and the roster. It writes `gate-<milestone>-<release>.json` (RFC 8785) and
//! `.md` with each criterion's state (`met`, `not_met`, `needs_signoff`), its evidence and an
//! overall `ready` or `not_ready`.
//!
//! Fail closed: an input that cannot be read or parsed, or a criteria file with any schema
//! finding, is fatal (exit 2, nothing written); a node, seed, app, test result or sign-off that
//! is missing makes its criterion not met. A milestone without criteria is `not_ready`.
//!
//! Determinism D0: no clock, no network, no environment; inputs are read into sorted maps,
//! percentages are compared in exact integer arithmetic, and the report bytes depend only on the
//! inputs.

use crate::launch_plan::{self, CriterionKind, Milestone, Phase, PlanError, NODE_STATUSES};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest accepted JSON input (trace, status and test report), in bytes.
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
/// Most JSON nesting accepted in those reports.
pub const MAX_JSON_DEPTH: usize = 16;
/// Largest accepted `NODE.md`, in bytes.
pub const MAX_NODE_BYTES: usize = 256 * 1024;
/// Most `NODE.md` files read from the tree.
pub const MAX_NODES: usize = 20_000;
/// Deepest directory level walked under the tree root.
pub const MAX_TREE_DEPTH: usize = 32;
/// Most characters in a `--release` tag.
pub const MAX_RELEASE_LEN: usize = 64;

// ------------------------------------------------------------------ errors

/// Fatal errors (exit 2, nothing written).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateError {
    InputUnreadable { path: String, reason: String },
    InputParse { path: String, reason: String },
    MilestoneUnknown { milestone: String },
    /// The report could not be written to `--out`.
    OutputUnwritable { path: String, reason: String },
}

impl GateError {
    pub fn code(&self) -> &'static str {
        match self {
            GateError::InputUnreadable { .. } => "InputUnreadable",
            GateError::InputParse { .. } => "InputParse",
            GateError::MilestoneUnknown { .. } => "MilestoneUnknown",
            GateError::OutputUnwritable { .. } => "OutputUnwritable",
        }
    }

    /// RFC 8785 canonical JSON: `{"error":{"code":..,"detail":..,"path":..}}`.
    pub fn to_json(&self) -> Vec<u8> {
        let (path, detail) = match self {
            GateError::InputUnreadable { path, reason }
            | GateError::InputParse { path, reason }
            | GateError::OutputUnwritable { path, reason } => (path.clone(), reason.clone()),
            GateError::MilestoneUnknown { milestone } => {
                (String::new(), format!("milestone `{milestone}` is not in the criteria file"))
            }
        };
        let mut e = BTreeMap::new();
        e.insert("code".to_string(), CValue::Str(self.code().to_string()));
        e.insert("path".to_string(), CValue::Str(path));
        e.insert("detail".to_string(), CValue::Str(detail));
        let mut o = BTreeMap::new();
        o.insert("error".to_string(), CValue::Obj(e));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }
}

impl fmt::Display for GateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GateError::InputUnreadable { path, reason } => write!(f, "InputUnreadable: {path}: {reason}"),
            GateError::InputParse { path, reason } => write!(f, "InputParse: {path}: {reason}"),
            GateError::MilestoneUnknown { milestone } => {
                write!(f, "MilestoneUnknown: `{}` is not in the criteria file", sanitize(milestone))
            }
            GateError::OutputUnwritable { path, reason } => write!(f, "OutputUnwritable: {path}: {reason}"),
        }
    }
}

impl std::error::Error for GateError {}

impl From<PlanError> for GateError {
    fn from(e: PlanError) -> GateError {
        match e {
            PlanError::InputUnreadable { path, reason } => GateError::InputUnreadable { path, reason },
            PlanError::InputParse { path, reason } => GateError::InputParse { path, reason },
        }
    }
}

fn parse_err(path: &Path, reason: impl Into<String>) -> GateError {
    GateError::InputParse { path: path.display().to_string(), reason: reason.into() }
}

fn unreadable(path: &Path, reason: impl Into<String>) -> GateError {
    GateError::InputUnreadable { path: path.display().to_string(), reason: reason.into() }
}

// ------------------------------------------------------------------ result types

/// Per-criterion reasons (Design: Errors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    NodeMissing,
    StatusMismatch,
    NodeOpen,
    LeafUntested,
    AcFailed,
    BelowThreshold,
    SeedNotLive,
    SignoffMissing,
    RoleMissing,
    SelfSignoff,
    ReleaseMismatch,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::NodeMissing => "node_missing",
            Reason::StatusMismatch => "status_mismatch",
            Reason::NodeOpen => "node_open",
            Reason::LeafUntested => "leaf_untested",
            Reason::AcFailed => "ac_failed",
            Reason::BelowThreshold => "below_threshold",
            Reason::SeedNotLive => "seed_not_live",
            Reason::SignoffMissing => "signoff_missing",
            Reason::RoleMissing => "role_missing",
            Reason::SelfSignoff => "self_signoff",
            Reason::ReleaseMismatch => "release_mismatch",
        }
    }
}

/// State of one criterion (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Met,
    NotMet,
    NeedsSignoff,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Met => "met",
            State::NotMet => "not_met",
            State::NeedsSignoff => "needs_signoff",
        }
    }
}

/// Why a criterion is not met. `subject` names the node, leaf and AC, seed, app or record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Problem {
    pub reason: Reason,
    pub subject: String,
    pub detail: String,
}

impl Problem {
    fn new(reason: Reason, subject: impl Into<String>, detail: impl Into<String>) -> Problem {
        Problem { reason, subject: subject.into(), detail: detail.into() }
    }
}

/// One evidence item: field name to value, all strings.
pub type EvidenceItem = BTreeMap<String, String>;

fn item(pairs: &[(&str, &str)]) -> EvidenceItem {
    pairs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
}

/// The evaluated state of one criterion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriterionResult {
    pub id: String,
    pub kind: &'static str,
    pub state: State,
    /// Sorted, without duplicates; empty exactly when `state` is `met`.
    pub problems: Vec<Problem>,
    pub evidence: Vec<EvidenceItem>,
}

/// The gate report of one milestone and release candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub milestone: String,
    pub release: String,
    pub author: String,
    /// In criteria-file order.
    pub criteria: Vec<CriterionResult>,
}

impl Report {
    /// `ready` only when there is at least one criterion and every criterion is met (REQ-006).
    pub fn ready(&self) -> bool {
        !self.criteria.is_empty() && self.criteria.iter().all(|c| c.state == State::Met)
    }

    pub fn result(&self) -> &'static str {
        if self.ready() {
            "ready"
        } else {
            "not_ready"
        }
    }

    /// `gate-<milestone>-<release>`.
    pub fn file_stem(&self) -> String {
        format!("gate-{}-{}", self.milestone, self.release)
    }

    /// RFC 8785 canonical JSON of the report.
    pub fn to_json(&self) -> Vec<u8> {
        let s = |v: &str| CValue::Str(v.to_string());
        let criteria = self
            .criteria
            .iter()
            .map(|c| {
                let problems = c
                    .problems
                    .iter()
                    .map(|p| {
                        let mut o = BTreeMap::new();
                        o.insert("reason".to_string(), s(p.reason.as_str()));
                        o.insert("subject".to_string(), s(&p.subject));
                        o.insert("detail".to_string(), s(&p.detail));
                        CValue::Obj(o)
                    })
                    .collect();
                let evidence = c
                    .evidence
                    .iter()
                    .map(|e| CValue::Obj(e.iter().map(|(k, v)| (k.clone(), s(v))).collect()))
                    .collect();
                let mut o = BTreeMap::new();
                o.insert("id".to_string(), s(&c.id));
                o.insert("kind".to_string(), s(c.kind));
                o.insert("state".to_string(), s(c.state.as_str()));
                o.insert("problems".to_string(), CValue::Arr(problems));
                o.insert("evidence".to_string(), CValue::Arr(evidence));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("milestone".to_string(), s(&self.milestone));
        o.insert("release".to_string(), s(&self.release));
        o.insert("author".to_string(), s(&self.author));
        o.insert("result".to_string(), s(self.result()));
        o.insert("criteria".to_string(), CValue::Arr(criteria));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }

    /// The Markdown report. Every value taken from an input is escaped with [`md_escape`].
    pub fn to_markdown(&self) -> String {
        let e = md_escape;
        let mut s = String::new();
        s.push_str(&format!("# Gate report: {} at {}\n\n", e(&self.milestone), e(&self.release)));
        s.push_str("| Field | Value |\n|---|---|\n");
        s.push_str(&format!("| Milestone | {} |\n", e(&self.milestone)));
        s.push_str(&format!("| Release | {} |\n", e(&self.release)));
        s.push_str(&format!("| Author | {} |\n", e(&self.author)));
        s.push_str(&format!("| Result | **{}** |\n\n", e(self.result())));
        s.push_str("## Criteria\n\n");
        if self.criteria.is_empty() {
            s.push_str("The milestone has no criteria, so it is not ready.\n");
            return s;
        }
        s.push_str("| Criterion | Kind | State | Reasons |\n|---|---|---|---|\n");
        for c in &self.criteria {
            let reasons: BTreeSet<&str> = c.problems.iter().map(|p| p.reason.as_str()).collect();
            let reasons: Vec<String> = reasons.into_iter().map(e).collect();
            s.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                e(&c.id),
                e(c.kind),
                e(c.state.as_str()),
                reasons.join(", ")
            ));
        }
        for c in &self.criteria {
            s.push_str(&format!("\n### {} ({}): {}\n\n", e(&c.id), e(c.kind), e(c.state.as_str())));
            for p in &c.problems {
                s.push_str(&format!("- {} {}: {}\n", e(p.reason.as_str()), e(&p.subject), e(&p.detail)));
            }
            if !c.problems.is_empty() && !c.evidence.is_empty() {
                s.push('\n');
            }
            for ev in &c.evidence {
                let fields: Vec<String> = ev.iter().map(|(k, v)| format!("{} {}", e(k), e(v))).collect();
                s.push_str(&format!("- evidence: {}\n", fields.join("; ")));
            }
        }
        s
    }

    /// Terminal summary: one line per criterion and problem, then the result. Control
    /// characters from inputs are replaced.
    pub fn render(&self) -> String {
        let mut s = String::new();
        for c in &self.criteria {
            s.push_str(&format!("{} {} {}\n", sanitize(&c.id), c.kind, c.state.as_str()));
            for p in &c.problems {
                s.push_str(&format!("  {} {}: {}\n", p.reason.as_str(), sanitize(&p.subject), sanitize(&p.detail)));
            }
        }
        s.push_str(&format!("result: {}\n", self.result()));
        s
    }
}

/// Escapes text for Markdown: `& < > " '` become HTML character references, every other ASCII
/// punctuation character is backslash-escaped (CommonMark), and control characters become spaces,
/// so input text cannot open markup, links, code spans or table cells.
pub fn md_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c.is_ascii_punctuation() => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Replaces control characters (terminal escapes, line breaks) with `?`.
fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { '?' } else { c }).collect()
}

// ------------------------------------------------------------------ inputs

/// Paths and arguments of a `gate-check` run.
#[derive(Debug, Clone)]
pub struct Inputs<'a> {
    pub milestone: &'a str,
    pub release: &'a str,
    pub author: &'a str,
    pub criteria: &'a Path,
    pub tree: &'a Path,
    pub trace: &'a Path,
    pub status: &'a Path,
    pub tests: &'a Path,
    pub signoffs: &'a Path,
    pub roster: &'a Path,
}

/// One spec node's frontmatter fields used by the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub status: String,
    pub phase: Phase,
}

/// A committed sign-off record (`signoffs/<milestone>/<criterion>.toml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignoffRecord {
    pub milestone: String,
    pub criterion: String,
    pub release: String,
    pub signer: String,
    pub role: String,
    pub date: String,
}

/// Leaf ID to AC ID to every outcome reported for that pair.
pub type TestResults = BTreeMap<String, BTreeMap<String, Vec<String>>>;

/// Everything the evaluation reads, already parsed.
#[derive(Debug, Clone, Default)]
pub struct Data {
    /// Node ID to node.
    pub nodes: BTreeMap<String, Node>,
    /// Seed ID to state, from the trace.
    pub seeds: BTreeMap<String, String>,
    /// App ID to (live, in_scope), from the status report.
    pub apps: BTreeMap<String, (u32, u32)>,
    /// Leaf ID to AC ID to every outcome reported for it, from the CI test report.
    pub results: TestResults,
    /// Handle to roles, from the roster.
    pub roster: BTreeMap<String, BTreeSet<String>>,
    /// Criterion ID to its sign-off record, for each `signoff` criterion of the milestone
    /// (`None` when the record file does not exist).
    pub signoffs: BTreeMap<String, Option<SignoffRecord>>,
}

fn read_capped(path: &Path, max: usize) -> Result<Vec<u8>, GateError> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(max as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| unreadable(path, e.to_string()))?;
    if bytes.len() > max {
        return Err(unreadable(path, format!("larger than {max} bytes")));
    }
    Ok(bytes)
}

/// Frontmatter of a `NODE.md`, with the rules of `tools/scope.py` (`parse_front`): the first line
/// is `---`; until the next `---` line, blank and `#` lines are skipped, every other line is
/// `key: value`; a `[a, b]` value is a list (`None` here), other values lose surrounding quotes.
pub fn parse_frontmatter(text: &str) -> Result<BTreeMap<String, Option<String>>, String> {
    let mut lines = text.lines().map(|l| l.strip_suffix('\r').unwrap_or(l));
    if lines.next().map(str::trim) != Some("---") {
        return Err("missing frontmatter".into());
    }
    let mut meta = BTreeMap::new();
    for line in lines {
        let t = line.trim();
        if t == "---" {
            return Ok(meta);
        }
        if t.is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            return Err(format!("bad frontmatter line: {}", sanitize(line)));
        };
        let v = v.trim();
        let val = if v.starts_with('[') && v.ends_with(']') {
            None
        } else {
            Some(v.trim_matches(|c| c == '\'' || c == '"').to_string())
        };
        meta.insert(k.trim().to_string(), val);
    }
    Err("unterminated frontmatter".into())
}

fn is_node_id(s: &str) -> bool {
    !s.is_empty()
        && s.split('.').all(|seg| {
            let b = seg.as_bytes();
            b.first().is_some_and(|c| c.is_ascii_uppercase())
                && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), GateError> {
    if depth > MAX_TREE_DEPTH {
        return Err(parse_err(dir, format!("tree deeper than {MAX_TREE_DEPTH} levels")));
    }
    let entries = std::fs::read_dir(dir).map_err(|e| unreadable(dir, e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| unreadable(dir, e.to_string()))?;
        // `DirEntry::file_type` does not follow symbolic links: links are skipped.
        let ft = entry.file_type().map_err(|e| unreadable(&entry.path(), e.to_string()))?;
        if ft.is_dir() {
            walk(&entry.path(), depth + 1, out)?;
        } else if ft.is_file() && entry.file_name() == "NODE.md" {
            out.push(entry.path());
            if out.len() > MAX_NODES {
                return Err(parse_err(dir, format!("more than {MAX_NODES} NODE.md files")));
            }
        }
    }
    Ok(())
}

/// Reads `id`, `status` and `phase` of every `NODE.md` under `root` (REQ-002). A node without a
/// valid ID, status or phase, or a duplicate ID, is a parse error (fail closed).
pub fn read_tree(root: &Path) -> Result<BTreeMap<String, Node>, GateError> {
    let mut files = Vec::new();
    walk(root, 0, &mut files)?;
    files.sort();
    let mut nodes = BTreeMap::new();
    for f in files {
        let bytes = read_capped(&f, MAX_NODE_BYTES)?;
        let text = String::from_utf8(bytes).map_err(|_| parse_err(&f, "not UTF-8"))?;
        let meta = parse_frontmatter(&text).map_err(|r| parse_err(&f, r))?;
        let field = |k: &str| meta.get(k).and_then(|v| v.clone());
        let id = field("id").filter(|i| is_node_id(i)).ok_or_else(|| parse_err(&f, "no valid `id`"))?;
        let status = field("status")
            .filter(|s| NODE_STATUSES.contains(&s.as_str()))
            .ok_or_else(|| parse_err(&f, format!("`status` must be one of {NODE_STATUSES:?}")))?;
        let phase = field("phase")
            .and_then(|p| Phase::parse(&p))
            .ok_or_else(|| parse_err(&f, "`phase` must be one of P0 P1a P1b P2 P3 P4 later"))?;
        if nodes.contains_key(&id) {
            return Err(parse_err(&f, format!("duplicate node id {id}")));
        }
        nodes.insert(id.clone(), Node { id, status, phase });
    }
    Ok(nodes)
}

fn read_json(path: &Path) -> Result<BTreeMap<String, CValue>, GateError> {
    let bytes = read_capped(path, MAX_JSON_BYTES)?;
    match fid_canon::parse_with_limits(&bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH) {
        Ok(CValue::Obj(o)) => Ok(o),
        Ok(_) => Err(parse_err(path, "top level is not an object")),
        Err(e) => Err(parse_err(path, e.to_string())),
    }
}

/// The objects of the array `key` of a JSON object.
fn objects<'v>(
    top: &'v BTreeMap<String, CValue>,
    key: &str,
    path: &Path,
) -> Result<Vec<&'v BTreeMap<String, CValue>>, GateError> {
    let Some(CValue::Arr(items)) = top.get(key) else {
        return Err(parse_err(path, format!("no `{key}` array")));
    };
    items
        .iter()
        .map(|v| match v {
            CValue::Obj(o) => Ok(o),
            _ => Err(parse_err(path, format!("an entry of `{key}` is not an object"))),
        })
        .collect()
}

fn json_str<'v>(o: &'v BTreeMap<String, CValue>, key: &str, path: &Path) -> Result<&'v str, GateError> {
    match o.get(key) {
        Some(CValue::Str(s)) if !s.is_empty() => Ok(s),
        _ => Err(parse_err(path, format!("an entry lacks a non-empty string `{key}`"))),
    }
}

/// A non-negative integer count that fits in `u32`.
fn json_count(o: &BTreeMap<String, CValue>, key: &str, path: &Path) -> Result<u32, GateError> {
    let bad = || parse_err(path, format!("`{key}` must be an integer in 0..={}", u32::MAX));
    match o.get(key) {
        // The shortest decimal form of an f64 parses as u32 exactly when the value is an
        // integer in range (`41` yes; `41.5`, `-1`, `-0` and `1e20` no): no lossy cast.
        Some(CValue::Num(n)) => format!("{n}").parse::<u32>().map_err(|_| bad()),
        Some(CValue::I64(i)) => u32::try_from(*i).map_err(|_| bad()),
        Some(CValue::U64(u)) => u32::try_from(*u).map_err(|_| bad()),
        _ => Err(bad()),
    }
}

/// Trace `seeds[] {id, state}`; other fields are ignored.
pub fn read_trace(path: &Path) -> Result<BTreeMap<String, String>, GateError> {
    let top = read_json(path)?;
    let mut out = BTreeMap::new();
    for s in objects(&top, "seeds", path)? {
        let id = json_str(s, "id", path)?;
        let state = json_str(s, "state", path)?;
        if out.insert(id.to_string(), state.to_string()).is_some() {
            return Err(parse_err(path, format!("seed {} listed twice", sanitize(id))));
        }
    }
    Ok(out)
}

/// Status report `apps[] {id, live, in_scope}`; `live` may not exceed `in_scope`.
pub fn read_status(path: &Path) -> Result<BTreeMap<String, (u32, u32)>, GateError> {
    let top = read_json(path)?;
    let mut out = BTreeMap::new();
    for a in objects(&top, "apps", path)? {
        let id = json_str(a, "id", path)?;
        let live = json_count(a, "live", path)?;
        let in_scope = json_count(a, "in_scope", path)?;
        if live > in_scope {
            return Err(parse_err(path, format!("app {}: live {live} exceeds in_scope {in_scope}", sanitize(id))));
        }
        if out.insert(id.to_string(), (live, in_scope)).is_some() {
            return Err(parse_err(path, format!("app {} listed twice", sanitize(id))));
        }
    }
    Ok(out)
}

/// CI test report `results[] {leaf, ac, outcome}`. Repeated (leaf, ac) pairs keep every outcome.
pub fn read_tests(path: &Path) -> Result<TestResults, GateError> {
    let top = read_json(path)?;
    let mut out = TestResults::new();
    for r in objects(&top, "results", path)? {
        let leaf = json_str(r, "leaf", path)?;
        let ac = json_str(r, "ac", path)?;
        let outcome = json_str(r, "outcome", path)?;
        out.entry(leaf.to_string()).or_default().entry(ac.to_string()).or_default().push(outcome.to_string());
    }
    for acs in out.values_mut() {
        for outcomes in acs.values_mut() {
            outcomes.sort();
        }
    }
    Ok(out)
}

fn toml_table(path: &Path) -> Result<toml::Table, GateError> {
    let text = launch_plan::read_text(path)?;
    Ok(launch_plan::parse_toml(&text, &path.display().to_string())?)
}

fn only_keys(t: &toml::Table, allowed: &[&str], path: &Path) -> Result<(), GateError> {
    match t.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(parse_err(path, format!("unknown field `{}`", sanitize(k)))),
        None => Ok(()),
    }
}

/// Roster `[[person]] handle, roles[]`.
pub fn read_roster(path: &Path) -> Result<BTreeMap<String, BTreeSet<String>>, GateError> {
    let t = toml_table(path)?;
    only_keys(&t, &["person"], path)?;
    let people = t.get("person").and_then(|v| v.as_array()).ok_or_else(|| parse_err(path, "no [[person]] array"))?;
    let mut out = BTreeMap::new();
    for p in people {
        let p = p.as_table().ok_or_else(|| parse_err(path, "a person is not a table"))?;
        only_keys(p, &["handle", "roles"], path)?;
        let handle = p
            .get("handle")
            .and_then(|v| v.as_str())
            .filter(|h| !h.trim().is_empty())
            .ok_or_else(|| parse_err(path, "a person lacks a non-empty `handle`"))?;
        let roles = p
            .get("roles")
            .and_then(|v| v.as_array())
            .ok_or_else(|| parse_err(path, "a person lacks a `roles` list"))?
            .iter()
            .map(|r| r.as_str().map(str::to_string))
            .collect::<Option<BTreeSet<String>>>()
            .ok_or_else(|| parse_err(path, "`roles` must be a list of strings"))?;
        if out.insert(handle.to_string(), roles).is_some() {
            return Err(parse_err(path, format!("handle {} listed twice", sanitize(handle))));
        }
    }
    Ok(out)
}

/// Parses one sign-off record. Every field is required and no other field is allowed.
pub fn read_signoff(path: &Path) -> Result<SignoffRecord, GateError> {
    let t = toml_table(path)?;
    only_keys(&t, &["milestone", "criterion", "release", "signer", "role", "date"], path)?;
    let get = |k: &str| {
        t.get(k)
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| parse_err(path, format!("record lacks a non-empty string `{k}`")))
    };
    let date = match t.get("date") {
        Some(toml::Value::Datetime(dt)) if dt.time.is_none() && dt.offset.is_none() => dt.to_string(),
        _ => get("date")?,
    };
    if !launch_plan::is_iso_date(&date) {
        return Err(parse_err(path, "`date` must be a YYYY-MM-DD date"));
    }
    Ok(SignoffRecord {
        milestone: get("milestone")?,
        criterion: get("criterion")?,
        release: get("release")?,
        signer: get("signer")?,
        role: get("role")?,
        date,
    })
}

/// A release tag usable in a file name: 1..=64 characters of `[A-Za-z0-9._+-]`, starting with an
/// ASCII letter or digit.
pub fn is_release_tag(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=MAX_RELEASE_LEN).contains(&b.len())
        && b.first().is_some_and(u8::is_ascii_alphanumeric)
        && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'+' | b'-'))
}

/// Reads and parses the criteria file. Any schema finding is fatal: a criterion that failed
/// validation would otherwise be left out of the evaluation.
pub fn read_criteria(path: &Path) -> Result<launch_plan::Criteria, GateError> {
    let t = toml_table(path)?;
    let (criteria, findings) = launch_plan::parse_criteria(&t);
    match findings.first() {
        Some(f) => Err(parse_err(path, format!("criteria file has {} finding(s), first: {f}", findings.len()))),
        None => Ok(criteria),
    }
}

/// Reads every input (REQ-002), finds the milestone and evaluates it. Writes nothing.
pub fn run(inp: &Inputs<'_>) -> Result<Report, GateError> {
    if !is_release_tag(inp.release) {
        return Err(GateError::InputParse {
            path: "--release".into(),
            reason: "release must be 1..=64 characters of [A-Za-z0-9._+-], starting with a letter or digit".into(),
        });
    }
    if inp.author.trim().is_empty() {
        return Err(GateError::InputParse { path: "--author".into(), reason: "author must not be empty".into() });
    }
    let criteria = read_criteria(inp.criteria)?;
    let mut data = Data {
        nodes: read_tree(inp.tree)?,
        seeds: read_trace(inp.trace)?,
        apps: read_status(inp.status)?,
        results: read_tests(inp.tests)?,
        roster: read_roster(inp.roster)?,
        signoffs: BTreeMap::new(),
    };
    if !inp.signoffs.is_dir() {
        return Err(unreadable(inp.signoffs, "not a directory"));
    }
    let milestone = criteria
        .milestones
        .iter()
        .find(|m| m.id == inp.milestone)
        .ok_or_else(|| GateError::MilestoneUnknown { milestone: inp.milestone.to_string() })?;
    for c in &milestone.criteria {
        if let CriterionKind::Signoff { .. } = c.kind {
            // Milestone and criterion IDs passed the parser's ID patterns: no path separators.
            let p = inp.signoffs.join(&milestone.id).join(format!("{}.toml", c.id));
            let rec = match std::fs::symlink_metadata(&p) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(unreadable(&p, e.to_string())),
                Ok(m) if m.is_file() => Some(read_signoff(&p)?),
                Ok(_) => return Err(unreadable(&p, "not a regular file")),
            };
            data.signoffs.insert(c.id.clone(), rec);
        }
    }
    Ok(evaluate(milestone, inp.release, inp.author, &data))
}

/// Writes `gate-<milestone>-<release>.json` and `.md` into `out` (created if missing).
pub fn write_report(report: &Report, out: &Path) -> Result<(), GateError> {
    let fail = |p: &Path, e: std::io::Error| GateError::OutputUnwritable {
        path: p.display().to_string(),
        reason: e.to_string(),
    };
    std::fs::create_dir_all(out).map_err(|e| fail(out, e))?;
    let stem = report.file_stem();
    let json = out.join(format!("{stem}.json"));
    std::fs::write(&json, report.to_json()).map_err(|e| fail(&json, e))?;
    let md = out.join(format!("{stem}.md"));
    std::fs::write(&md, report.to_markdown().as_bytes()).map_err(|e| fail(&md, e))?;
    Ok(())
}

// ------------------------------------------------------------------ evaluation

/// Evaluates every criterion of `milestone` (REQ-001, REQ-003 .. REQ-006). Pure.
pub fn evaluate(milestone: &Milestone, release: &str, author: &str, data: &Data) -> Report {
    let criteria = milestone
        .criteria
        .iter()
        .map(|c| {
            let (kind, problems, evidence, unmet) = match &c.kind {
                CriterionKind::NodeStatus { node, statuses } => {
                    let (p, e) = node_status(node, statuses, data);
                    ("node_status", p, e, State::NotMet)
                }
                CriterionKind::PhaseDone { phase, scope } => {
                    let (p, e) = phase_done(*phase, scope, data);
                    ("phase_done", p, e, State::NotMet)
                }
                CriterionKind::ParityApp { app, min_pct } => {
                    let (p, e) = parity_app(app, *min_pct, data);
                    ("parity", p, e, State::NotMet)
                }
                CriterionKind::ParitySeeds { seeds } => {
                    let (p, e) = parity_seeds(seeds, data);
                    ("parity", p, e, State::NotMet)
                }
                CriterionKind::Signoff { role } => {
                    let (p, e) = signoff(&milestone.id, &c.id, role, release, author, data);
                    ("signoff", p, e, State::NeedsSignoff)
                }
            };
            let mut problems = problems;
            problems.sort();
            problems.dedup();
            let state = if problems.is_empty() { State::Met } else { unmet };
            CriterionResult { id: c.id.clone(), kind, state, problems, evidence }
        })
        .collect();
    Report {
        milestone: milestone.id.clone(),
        release: release.to_string(),
        author: author.to_string(),
        criteria,
    }
}

type Outcome = (Vec<Problem>, Vec<EvidenceItem>);

fn node_status(node: &str, statuses: &[String], data: &Data) -> Outcome {
    let wanted = statuses.join(" ");
    match data.nodes.get(node) {
        None => (
            vec![Problem::new(Reason::NodeMissing, node, "node is not in the spec tree")],
            vec![item(&[("node", node), ("status", "absent"), ("required", wanted.as_str())])],
        ),
        Some(n) => {
            let ok = statuses.iter().any(|s| *s == n.status);
            let problems = if ok {
                Vec::new()
            } else {
                vec![Problem::new(Reason::StatusMismatch, node, format!("status {} is not one of {wanted}", n.status))]
            };
            (problems, vec![item(&[("node", node), ("status", n.status.as_str()), ("required", wanted.as_str())])])
        }
    }
}

fn in_subtree(id: &str, scope: &str) -> bool {
    id == scope || id.strip_prefix(scope).is_some_and(|rest| rest.starts_with('.'))
}

fn phase_done(phase: Phase, scope: &[String], data: &Data) -> Outcome {
    let mut problems = Vec::new();
    let mut evidence = Vec::new();
    for s in scope {
        if !data.nodes.contains_key(s) {
            problems.push(Problem::new(Reason::NodeMissing, s.as_str(), "scope node is not in the spec tree"));
        }
    }
    let considered: Vec<&Node> = data
        .nodes
        .values()
        .filter(|n| n.phase <= phase && scope.iter().any(|s| in_subtree(&n.id, s)))
        .collect();
    if considered.is_empty() && problems.is_empty() {
        problems.push(Problem::new(
            Reason::NodeMissing,
            scope.join(" "),
            format!("no node in the scope is at or before phase {}", phase.as_str()),
        ));
    }
    for n in considered {
        let mut ev = item(&[("node", n.id.as_str()), ("status", n.status.as_str()), ("phase", n.phase.as_str())]);
        if n.status == "open" || n.status == "decision" {
            problems.push(Problem::new(
                Reason::NodeOpen,
                n.id.as_str(),
                format!("status {} at phase {}", n.status, n.phase.as_str()),
            ));
        }
        // `frozen` nodes are finished leaves: they need passing results too (fail closed).
        if n.status == "leaf" || n.status == "frozen" {
            let acs = data.results.get(&n.id);
            let total = acs.map_or(0, BTreeMap::len);
            let mut passed = 0usize;
            for (ac, outcomes) in acs.into_iter().flatten() {
                let bad: BTreeSet<&str> = outcomes.iter().map(String::as_str).filter(|o| *o != "pass").collect();
                if bad.is_empty() {
                    passed += 1;
                } else {
                    let bad: Vec<&str> = bad.into_iter().collect();
                    problems.push(Problem::new(
                        Reason::AcFailed,
                        format!("{} {ac}", n.id),
                        format!("outcome {}", bad.join(", ")),
                    ));
                }
            }
            if total == 0 {
                problems.push(Problem::new(Reason::LeafUntested, n.id.as_str(), "no AC result in the CI test report"));
            }
            ev.insert("ac_passed".into(), passed.to_string());
            ev.insert("ac_total".into(), total.to_string());
        }
        evidence.push(ev);
    }
    (problems, evidence)
}

/// `live * 100 >= min_pct * in_scope`, exactly. `min_pct` (a finite f64 in 0..=100) is split
/// into its integer mantissa `m` and binary exponent `e`, and `live * 100` is compared with
/// `ceil(m * in_scope * 2^e)` in `u128`.
pub fn meets_pct(live: u32, in_scope: u32, min_pct: f64) -> bool {
    if !(0.0..=100.0).contains(&min_pct) {
        return false;
    }
    let bits = min_pct.to_bits();
    let exp_bits = i64::from(u16::try_from((bits >> 52) & 0x7ff).unwrap_or(0));
    let frac = bits & ((1u64 << 52) - 1);
    let (mant, exp) = if exp_bits == 0 { (frac, -1074i64) } else { (frac | (1u64 << 52), exp_bits - 1075) };
    let lhs = u128::from(live) * 100;
    let rhs = u128::from(mant) * u128::from(in_scope);
    if exp >= 0 {
        // A normal f64 with exponent >= 0 is at least 2^52, outside 0..=100: fail closed.
        return false;
    }
    let k = u32::try_from(-exp).unwrap_or(u32::MAX);
    let need = if k >= 128 {
        u128::from(rhs != 0)
    } else {
        (rhs >> k) + u128::from(rhs & ((1u128 << k) - 1) != 0)
    };
    lhs >= need
}

fn pct_text(min_pct: f64) -> String {
    fid_canon::format_number(min_pct).unwrap_or_else(|_| "invalid".into())
}

fn parity_app(app: &str, min_pct: f64, data: &Data) -> Outcome {
    let pct = pct_text(min_pct);
    match data.apps.get(app) {
        None => (
            vec![Problem::new(Reason::BelowThreshold, app, "app is not in the status report")],
            vec![item(&[("app", app), ("live", "absent"), ("in_scope", "absent"), ("min_pct", pct.as_str())])],
        ),
        Some(&(live, in_scope)) => {
            let ev = vec![item(&[
                ("app", app),
                ("live", live.to_string().as_str()),
                ("in_scope", in_scope.to_string().as_str()),
                ("min_pct", pct.as_str()),
            ])];
            let problem = if in_scope == 0 {
                Some("in_scope is 0, so no percentage exists".to_string())
            } else if !meets_pct(live, in_scope, min_pct) {
                Some(format!("live {live} of {in_scope} is below {pct} percent"))
            } else {
                None
            };
            (problem.map(|d| Problem::new(Reason::BelowThreshold, app, d)).into_iter().collect(), ev)
        }
    }
}

fn parity_seeds(seeds: &[String], data: &Data) -> Outcome {
    let mut problems = Vec::new();
    let mut evidence = Vec::new();
    for s in seeds {
        match data.seeds.get(s) {
            None => {
                problems.push(Problem::new(Reason::SeedNotLive, s.as_str(), "seed is not in the trace"));
                evidence.push(item(&[("seed", s.as_str()), ("state", "absent")]));
            }
            Some(state) => {
                if state != "live" {
                    problems.push(Problem::new(Reason::SeedNotLive, s.as_str(), format!("state {state}")));
                }
                evidence.push(item(&[("seed", s.as_str()), ("state", state.as_str())]));
            }
        }
    }
    (problems, evidence)
}

fn signoff(milestone: &str, criterion: &str, role: &str, release: &str, author: &str, data: &Data) -> Outcome {
    let file = format!("{milestone}/{criterion}.toml");
    let Some(Some(rec)) = data.signoffs.get(criterion) else {
        return (
            vec![Problem::new(Reason::SignoffMissing, file.as_str(), "no committed sign-off record")],
            vec![item(&[("record", file.as_str()), ("role_required", role)])],
        );
    };
    let mut problems = Vec::new();
    if rec.milestone != milestone || rec.criterion != criterion {
        problems.push(Problem::new(
            Reason::SignoffMissing,
            file.as_str(),
            format!("record names milestone {} criterion {}", rec.milestone, rec.criterion),
        ));
    }
    if rec.release != release {
        problems.push(Problem::new(
            Reason::ReleaseMismatch,
            rec.signer.as_str(),
            format!("record signs release {}, the candidate is {release}", rec.release),
        ));
    }
    if rec.role != role {
        problems.push(Problem::new(
            Reason::RoleMissing,
            rec.signer.as_str(),
            format!("record signs as {}, the criterion needs {role}", rec.role),
        ));
    }
    match data.roster.get(&rec.signer) {
        None => problems.push(Problem::new(Reason::RoleMissing, rec.signer.as_str(), "signer is not in the roster")),
        Some(roles) if !roles.contains(role) => problems.push(Problem::new(
            Reason::RoleMissing,
            rec.signer.as_str(),
            format!("signer does not hold role {role} in the roster"),
        )),
        Some(_) => {}
    }
    // Handles are compared without ASCII case, as on the forge: `Bob` signing for `bob` is a
    // self sign-off.
    if rec.signer.eq_ignore_ascii_case(author) {
        problems.push(Problem::new(Reason::SelfSignoff, rec.signer.as_str(), "signer is the candidate's author"));
    }
    let ev = item(&[
        ("record", file.as_str()),
        ("signer", rec.signer.as_str()),
        ("role", rec.role.as_str()),
        ("role_required", role),
        ("release", rec.release.as_str()),
        ("date", rec.date.as_str()),
    ]);
    (problems, vec![ev])
}
