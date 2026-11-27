//! Path-scoped check planning and CI-change isolation (spec node `ECO.CI.GATES`).
//!
//! `cargo xtask ci-plan --config <ci-gates.toml> --changed <file> [--json]`
//!
//! Reads the gate configuration (`[[gate]]` with `id`, `run`, `globs` and optional `always`;
//! optional `[isolation] ci_globs` and `[shared] tool_globs`) and a list of changed
//! repo-relative paths (one per line), and prints the governed checks the change must run.
//! The list must come from `git diff --name-only --no-renames <base> <head>`: with rename
//! detection on, git lists a renamed file only under its new path, so a gate that governs the
//! old path would silently not be selected.
//!
//!
//! - REQ-ECO.CI.GATES-001: every gate whose globs match at least one changed path, plus every
//!   gate with `always = true`, sorted by gate ID.
//! - REQ-ECO.CI.GATES-002: if a file named `ci-gates.toml` changed (wherever it lives), or a
//!   path matching `[shared] tool_globs` (default `xtask/**`), every gate is selected.
//! - REQ-ECO.CI.GATES-003: if the changed paths include a CI path (`[isolation] ci_globs`,
//!   default `.github/workflows/**`, `.github/actions/**`, `ci/**`) and any other path, there
//!   is no plan: one `CI-MIXED <path>` finding per non-CI path, sorted, and exit 1.
//!
//! Globs are matched by the fixed matcher in [`Glob`]: `*` and `?` stay inside one path
//! segment, `**` must be a whole segment and matches any number of segments. Every other
//! glob feature (`[...]`, `{...}`, a leading `!`, backslashes) is refused as `GATES-CONFIG`,
//! so a pattern never silently means something other than what it says.
//!
//! Exit codes: 0 plan printed; 1 `CI-MIXED`; 2 usage error, `GATES-CONFIG` (bad, unknown or
//! duplicate keys, bad globs), `GATES-IO` (unreadable file) or `GATES-INPUT` (a changed-path
//! line that is not a clean repo-relative path). Inputs are bounded (config 64 KiB, changed
//! list 16 MiB and 100 000 paths of at most 4 096 bytes each). D0: the output is sorted and
//! depends only on the two inputs; no clock or environment is read.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use toml::{Table, Value};

/// CLI usage string for `cargo xtask ci-plan`.
pub const USAGE: &str = "usage: cargo xtask ci-plan --config <ci-gates.toml> --changed <file> [--json]";

/// Maximum size of the gate configuration (Design: 64 KiB).
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;
/// Maximum size of the changed-path list.
pub const MAX_CHANGED_BYTES: usize = 16 * 1024 * 1024;
/// Maximum number of non-blank lines in the changed-path list (Design: 100 000).
pub const MAX_CHANGED_PATHS: usize = 100_000;
/// Maximum length of one changed path, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
/// Maximum length of one glob, in bytes.
pub const MAX_GLOB_BYTES: usize = 256;
/// Maximum number of `[[gate]]` entries.
pub const MAX_GATES: usize = 256;
/// Maximum number of globs in one list (`globs`, `ci_globs`, `tool_globs`).
pub const MAX_GLOBS: usize = 256;
/// Maximum length of a gate ID, in bytes.
pub const MAX_ID_BYTES: usize = 64;
/// Maximum length of a gate's `run` command, in bytes.
pub const MAX_RUN_BYTES: usize = 4096;
/// A changed file with this name selects every gate (REQ-ECO.CI.GATES-002).
pub const CONFIG_FILE_NAME: &str = "ci-gates.toml";
/// CI paths when `[isolation] ci_globs` is not given (REQ-ECO.CI.GATES-003).
pub const DEFAULT_CI_GLOBS: [&str; 3] = [".github/workflows/**", ".github/actions/**", "ci/**"];
/// Shared check tools when `[shared] tool_globs` is not given (REQ-ECO.CI.GATES-002).
pub const DEFAULT_TOOL_GLOBS: [&str; 1] = ["xtask/**"];

/// Error codes of the planner (`ECO.CI.GATES` Design, plus `GATES-INPUT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorCode {
    /// Bad, unknown or duplicate configuration keys, or a bad glob.
    Config,
    /// A file could not be read.
    Io,
    /// The changed-path list is too large or holds a line that is not a clean path.
    Input,
}

impl ErrorCode {
    /// The machine-readable code.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Config => "GATES-CONFIG",
            ErrorCode::Io => "GATES-IO",
            ErrorCode::Input => "GATES-INPUT",
        }
    }
}

/// A planner error; it always means no plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanError {
    pub code: ErrorCode,
    pub detail: String,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.detail)
    }
}

fn config_err(detail: impl Into<String>) -> PlanError {
    PlanError { code: ErrorCode::Config, detail: detail.into() }
}

fn input_err(detail: impl Into<String>) -> PlanError {
    PlanError { code: ErrorCode::Input, detail: detail.into() }
}

// ============================================================================
// Glob matcher
// ============================================================================

/// One compiled glob element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    /// One literal character.
    Lit(char),
    /// `?`: one character other than `/`.
    Any,
    /// `*`: zero or more characters other than `/`.
    Star,
    /// A `**/` segment: zero or more whole segments, each followed by `/`.
    Segs,
    /// A final `**` segment: any remaining characters, `/` included.
    Rest,
}

/// A glob in the planner's fixed syntax (`*`, `?`, whole-segment `**`), compiled for a
/// linear-time match: [`Glob::matches`] simulates the pattern's automaton over the path's
/// characters, so one match costs O(path length x glob length) with the glob bounded by
/// [`MAX_GLOB_BYTES`], and never backtracks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    src: String,
    toks: Vec<Tok>,
}

impl Glob {
    /// Compiles `src`, refusing anything outside the fixed syntax.
    pub fn parse(src: &str) -> Result<Glob, String> {
        if src.is_empty() {
            return Err("empty glob".into());
        }
        if src.len() > MAX_GLOB_BYTES {
            return Err(format!("glob longer than {MAX_GLOB_BYTES} bytes"));
        }
        if let Some(c) = src.chars().find(|c| c.is_control() || matches!(c, '\\' | '[' | ']' | '{' | '}')) {
            return Err(format!("glob {src:?} uses unsupported character {c:?}"));
        }
        if src.starts_with('!') {
            return Err(format!("glob {src:?}: negation (leading !) is not supported"));
        }
        if src.starts_with('/') || src.ends_with('/') {
            return Err(format!("glob {src:?} must be repo-relative, without a leading or trailing /"));
        }
        let segs: Vec<&str> = src.split('/').collect();
        let last = segs.len().saturating_sub(1);
        let mut toks = Vec::new();
        for (i, seg) in segs.iter().enumerate() {
            if seg.is_empty() {
                return Err(format!("glob {src:?} has an empty path segment"));
            }
            if *seg == "." || *seg == ".." {
                return Err(format!("glob {src:?} has a {seg} segment"));
            }
            if *seg == "**" {
                toks.push(if i == last { Tok::Rest } else { Tok::Segs });
                continue;
            }
            if seg.contains("**") {
                return Err(format!("glob {src:?}: ** must be a whole path segment"));
            }
            toks.extend(seg.chars().map(|c| match c {
                '*' => Tok::Star,
                '?' => Tok::Any,
                c => Tok::Lit(c),
            }));
            if i != last {
                toks.push(Tok::Lit('/'));
            }
        }
        Ok(Glob { src: src.to_string(), toks })
    }

    /// The glob as written.
    pub fn as_str(&self) -> &str {
        &self.src
    }

    /// Whether the whole of `path` matches the glob.
    pub fn matches(&self, path: &str) -> bool {
        let n = self.toks.len();
        // at[i]: positioned before token i (at[n]: accepted); mid[i]: inside a segment of `Segs` i.
        let mut at = vec![false; n + 1];
        let mut mid = vec![false; n];
        let mut next_at = vec![false; n + 1];
        let mut next_mid = vec![false; n];
        set(&mut at, 0);
        close(&self.toks, &mut at);
        for c in path.chars() {
            next_at.fill(false);
            next_mid.fill(false);
            for (i, tok) in self.toks.iter().enumerate() {
                if flag(&at, i) {
                    match *tok {
                        Tok::Lit(l) if c == l => set(&mut next_at, i + 1),
                        Tok::Any if c != '/' => set(&mut next_at, i + 1),
                        Tok::Star if c != '/' => set(&mut next_at, i),
                        Tok::Segs if c == '/' => set(&mut next_at, i),
                        Tok::Segs => set(&mut next_mid, i),
                        Tok::Rest => set(&mut next_at, i),
                        Tok::Lit(_) | Tok::Any | Tok::Star => {}
                    }
                }
                if flag(&mid, i) {
                    if c == '/' {
                        set(&mut next_at, i);
                    } else {
                        set(&mut next_mid, i);
                    }
                }
            }
            close(&self.toks, &mut next_at);
            std::mem::swap(&mut at, &mut next_at);
            std::mem::swap(&mut mid, &mut next_mid);
            if !at.iter().any(|&b| b) && !mid.iter().any(|&b| b) {
                return false;
            }
        }
        flag(&at, n)
    }
}

fn flag(v: &[bool], i: usize) -> bool {
    v.get(i).copied().unwrap_or(false)
}

fn set(v: &mut [bool], i: usize) {
    if let Some(s) = v.get_mut(i) {
        *s = true;
    }
}

/// Epsilon closure: `*`, `**/` and a final `**` may each match nothing. Epsilon moves only
/// go from token i to i + 1, so one forward pass is enough.
fn close(toks: &[Tok], at: &mut [bool]) {
    for (i, tok) in toks.iter().enumerate() {
        if flag(at, i) && matches!(tok, Tok::Star | Tok::Segs | Tok::Rest) {
            set(at, i + 1);
        }
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// One `[[gate]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    pub id: String,
    pub run: String,
    pub globs: Vec<Glob>,
    pub always: bool,
}

/// A validated `ci-gates.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Gates keyed (and so sorted) by ID.
    pub gates: BTreeMap<String, Gate>,
    /// CI paths for the isolation rule.
    pub ci_globs: Vec<Glob>,
    /// Shared check tools whose change selects every gate.
    pub tool_globs: Vec<Glob>,
}

fn check_keys(t: &Table, allowed: &[&str], what: &str) -> Result<(), PlanError> {
    match t.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(config_err(format!("unknown key {k:?} in {what}"))),
        None => Ok(()),
    }
}

fn glob_list(v: &Value, what: &str) -> Result<Vec<Glob>, PlanError> {
    let Value::Array(items) = v else {
        return Err(config_err(format!("{what} must be an array of strings")));
    };
    if items.len() > MAX_GLOBS {
        return Err(config_err(format!("{what} has more than {MAX_GLOBS} globs")));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Value::String(s) = item else {
            return Err(config_err(format!("{what} must be an array of strings")));
        };
        if !seen.insert(s.as_str()) {
            return Err(config_err(format!("{what} lists {s:?} twice")));
        }
        out.push(Glob::parse(s).map_err(|e| config_err(format!("{what}: {e}")))?);
    }
    Ok(out)
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= MAX_ID_BYTES
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn parse_gate(v: &Value, index: usize) -> Result<Gate, PlanError> {
    let Value::Table(t) = v else {
        return Err(config_err(format!("gate {index} must be a table")));
    };
    let what = format!("gate {index}");
    check_keys(t, &["always", "globs", "id", "run"], &what)?;
    let id = match t.get("id") {
        Some(Value::String(s)) if valid_id(s) => s.clone(),
        Some(Value::String(s)) => {
            return Err(config_err(format!("{what}: id {s:?} must match [a-z][a-z0-9-]* (at most 64 bytes)")))
        }
        Some(_) => return Err(config_err(format!("{what}: id must be a string"))),
        None => return Err(config_err(format!("{what}: missing id"))),
    };
    let what = format!("gate {id:?}");
    let run = match t.get("run") {
        Some(Value::String(s))
            if !s.trim().is_empty() && s.len() <= MAX_RUN_BYTES && !s.chars().any(char::is_control) =>
        {
            s.clone()
        }
        Some(_) => {
            return Err(config_err(format!(
                "{what}: run must be a non-empty one-line string of at most {MAX_RUN_BYTES} bytes"
            )))
        }
        None => return Err(config_err(format!("{what}: missing run"))),
    };
    let always = match t.get("always") {
        Some(Value::Boolean(b)) => *b,
        Some(_) => return Err(config_err(format!("{what}: always must be a boolean"))),
        None => false,
    };
    let globs = match t.get("globs") {
        Some(v) => glob_list(v, &format!("{what} globs"))?,
        None => return Err(config_err(format!("{what}: missing globs"))),
    };
    if globs.is_empty() && !always {
        return Err(config_err(format!("{what} has no globs and is not always, so it could never be selected")));
    }
    Ok(Gate { id, run, globs, always })
}

fn defaults(globs: &[&str]) -> Result<Vec<Glob>, PlanError> {
    globs.iter().map(|g| Glob::parse(g).map_err(config_err)).collect()
}

fn optional_list(top: &Table, section: &str, key: &str, default: &[&str]) -> Result<Vec<Glob>, PlanError> {
    match top.get(section) {
        None => defaults(default),
        Some(Value::Table(t)) => {
            check_keys(t, &[key], &format!("[{section}]"))?;
            match t.get(key) {
                Some(v) => glob_list(v, &format!("[{section}] {key}")),
                None => defaults(default),
            }
        }
        Some(_) => Err(config_err(format!("{section} must be a table"))),
    }
}

impl Config {
    /// Parses and validates the configuration text. Unknown keys fail closed; TOML itself
    /// refuses duplicate keys.
    pub fn parse(text: &str) -> Result<Config, PlanError> {
        if text.len() > MAX_CONFIG_BYTES {
            return Err(config_err(format!("configuration is larger than {MAX_CONFIG_BYTES} bytes")));
        }
        let top: Table = text.parse().map_err(|e: toml::de::Error| {
            config_err(format!("malformed TOML: {}", e.to_string().trim_end().replace('\n', " ")))
        })?;
        check_keys(&top, &["gate", "isolation", "shared"], "the top level")?;
        let entries = match top.get("gate") {
            Some(Value::Array(a)) => a,
            Some(_) => return Err(config_err("gate must be an array of tables ([[gate]])")),
            None => return Err(config_err("no [[gate]] entries")),
        };
        if entries.is_empty() {
            return Err(config_err("no [[gate]] entries"));
        }
        if entries.len() > MAX_GATES {
            return Err(config_err(format!("more than {MAX_GATES} gates")));
        }
        let mut gates = BTreeMap::new();
        for (i, v) in entries.iter().enumerate() {
            let gate = parse_gate(v, i + 1)?;
            if gates.contains_key(&gate.id) {
                return Err(config_err(format!("gate id {:?} is used twice", gate.id)));
            }
            gates.insert(gate.id.clone(), gate);
        }
        let ci_globs = optional_list(&top, "isolation", "ci_globs", &DEFAULT_CI_GLOBS)?;
        if ci_globs.is_empty() {
            return Err(config_err("[isolation] ci_globs is empty, which would turn the CI-change isolation rule off"));
        }
        let tool_globs = optional_list(&top, "shared", "tool_globs", &DEFAULT_TOOL_GLOBS)?;
        Ok(Config { gates, ci_globs, tool_globs })
    }

    /// Like [`Config::parse`], from raw bytes (must be UTF-8).
    pub fn from_bytes(bytes: &[u8]) -> Result<Config, PlanError> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(config_err(format!("configuration is larger than {MAX_CONFIG_BYTES} bytes")));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| config_err("configuration is not UTF-8"))?;
        Config::parse(text)
    }
}

// ============================================================================
// Changed-path list
// ============================================================================

/// Normalises one changed-path line: backslashes become `/` and one leading `./` is dropped.
/// Anything that is not a clean repo-relative path is refused, so that a path the globs were
/// never meant to see (absolute, `..`, git-quoted, padded) cannot slip past every gate.
pub fn normalise_path(line: &str) -> Result<String, String> {
    if line.len() > MAX_PATH_BYTES {
        return Err(format!("path longer than {MAX_PATH_BYTES} bytes"));
    }
    if line.starts_with(char::is_whitespace) || line.ends_with(char::is_whitespace) {
        return Err("leading or trailing whitespace".into());
    }
    if line.chars().any(char::is_control) {
        return Err("control character".into());
    }
    if line.contains('\u{feff}') {
        return Err("byte-order mark (U+FEFF)".into());
    }
    if line.starts_with('"') {
        return Err("quoted path (run git with -c core.quotePath=false)".into());
    }
    let unified = line.replace('\\', "/");
    let p = unified.strip_prefix("./").unwrap_or(&unified);
    let b = p.as_bytes();
    if p.starts_with('/') || (b.len() >= 2 && b.first().is_some_and(u8::is_ascii_alphabetic) && b.get(1) == Some(&b':'))
    {
        return Err("absolute path".into());
    }
    for seg in p.split('/') {
        if seg.is_empty() {
            return Err("empty path segment".into());
        }
        if seg == "." || seg == ".." {
            return Err(format!("{seg} segment"));
        }
    }
    Ok(p.to_string())
}

/// Parses the changed-path list (one path per line, LF or CRLF; blank lines are skipped).
/// Returns the distinct normalised paths, sorted. A list with no paths is refused.
pub fn parse_changed(bytes: &[u8]) -> Result<BTreeSet<String>, PlanError> {
    if bytes.len() > MAX_CHANGED_BYTES {
        return Err(input_err(format!("changed list is larger than {MAX_CHANGED_BYTES} bytes")));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| input_err("changed list is not UTF-8"))?;
    let mut out = BTreeSet::new();
    let mut count = 0usize;
    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() {
            continue;
        }
        count += 1;
        if count > MAX_CHANGED_PATHS {
            return Err(input_err(format!("changed list has more than {MAX_CHANGED_PATHS} paths")));
        }
        let p = normalise_path(line).map_err(|e| input_err(format!("line {}: {e}", i + 1)))?;
        out.insert(p);
    }
    if out.is_empty() {
        // A pull request always changes at least one file, so an empty list means the step
        // that produced it failed; planning from it would silently skip every gate.
        return Err(input_err("changed list has no paths"));
    }
    Ok(out)
}

// ============================================================================
// Planning
// ============================================================================

/// Why a gate is in the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// The gate has `always = true`.
    Always,
    /// Every gate is selected because `path` is a `ci-gates.toml` or a shared check tool.
    AllSelected { path: String },
    /// `path` (the first in sorted order) matches the gate's `glob`.
    Matched { path: String, glob: String },
}

/// One planned gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub id: String,
    pub run: String,
    pub reason: Reason,
}

/// The planner's result for valid inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The gates to run, sorted by ID (possibly none).
    Plan(Vec<Selected>),
    /// CI paths changed together with other paths: no plan. `non_ci` is sorted.
    CiMixed { non_ci: Vec<String> },
}

fn any_match(globs: &[Glob], path: &str) -> Option<String> {
    globs.iter().find(|g| g.matches(path)).map(|g| g.as_str().to_string())
}

fn is_config_file(path: &str) -> bool {
    path.rsplit('/').next() == Some(CONFIG_FILE_NAME)
}

/// Applies REQ-ECO.CI.GATES-003, then -002, then -001 to the distinct, sorted changed paths.
pub fn plan(config: &Config, changed: &BTreeSet<String>) -> Outcome {
    let (ci, non_ci): (Vec<&String>, Vec<&String>) =
        changed.iter().partition(|p| any_match(&config.ci_globs, p).is_some());
    if !ci.is_empty() && !non_ci.is_empty() {
        return Outcome::CiMixed { non_ci: non_ci.into_iter().cloned().collect() };
    }
    let trigger = changed.iter().find(|p| is_config_file(p) || any_match(&config.tool_globs, p).is_some());
    let mut out = Vec::new();
    for gate in config.gates.values() {
        let reason = if gate.always {
            Reason::Always
        } else if let Some(p) = trigger {
            Reason::AllSelected { path: p.clone() }
        } else if let Some((path, glob)) =
            changed.iter().find_map(|p| any_match(&gate.globs, p).map(|g| (p.clone(), g)))
        {
            Reason::Matched { path, glob }
        } else {
            continue;
        };
        out.push(Selected { id: gate.id.clone(), run: gate.run.clone(), reason });
    }
    Outcome::Plan(out)
}

/// `{"gates":[...]}` plus a newline. Gate IDs are `[a-z][a-z0-9-]*`, so no escaping is needed.
pub fn render_json(plan: &[Selected]) -> String {
    let ids: Vec<String> = plan.iter().map(|s| format!("\"{}\"", s.id)).collect();
    format!("{{\"gates\":[{}]}}\n", ids.join(","))
}

/// One line per planned gate with the reason it was selected.
pub fn render_text(plan: &[Selected]) -> String {
    if plan.is_empty() {
        return "ci-plan: 0 gates: no changed path matches a gate glob and no gate is always\n".to_string();
    }
    let mut s = format!("ci-plan: {} gate(s)\n", plan.len());
    for g in plan {
        let why = match &g.reason {
            Reason::Always => "always".to_string(),
            Reason::AllSelected { path } => format!("every gate selected: {path} is a ci-gates.toml or a shared tool"),
            Reason::Matched { path, glob } => format!("{path} matches {glob}"),
        };
        s.push_str(&format!("{}: {why}\n", g.id));
    }
    s
}

/// One `CI-MIXED <path>` line per non-CI path.
pub fn render_mixed(non_ci: &[String]) -> String {
    non_ci.iter().map(|p| format!("CI-MIXED {p}\n")).collect()
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, PlanError> {
    let file = std::fs::File::open(path)
        .map_err(|e| PlanError { code: ErrorCode::Io, detail: format!("{}: {e}", path.display()) })?;
    let mut bytes = Vec::new();
    // One byte over the limit is enough for the parsers to report the input as too large.
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    file.take(cap)
        .read_to_end(&mut bytes)
        .map_err(|e| PlanError { code: ErrorCode::Io, detail: format!("{}: {e}", path.display()) })?;
    Ok(bytes)
}

/// Reads both files and plans.
pub fn run(config: &Path, changed: &Path) -> Result<Outcome, PlanError> {
    let cfg = Config::from_bytes(&read_bounded(config, MAX_CONFIG_BYTES)?)?;
    let paths = parse_changed(&read_bounded(changed, MAX_CHANGED_BYTES)?)?;
    Ok(plan(&cfg, &paths))
}

fn emit(text: &str) -> bool {
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes()).and_then(|()| out.flush()).is_ok()
}

/// `cargo xtask ci-plan` entry point.
pub fn cli(args: &[String]) -> ExitCode {
    let (mut config, mut changed, mut json) = (None::<PathBuf>, None::<PathBuf>, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" if !json => json = true,
            "--config" if config.is_none() => match it.next() {
                Some(v) => config = Some(PathBuf::from(v)),
                None => return usage(),
            },
            "--changed" if changed.is_none() => match it.next() {
                Some(v) => changed = Some(PathBuf::from(v)),
                None => return usage(),
            },
            _ => return usage(),
        }
    }
    let (Some(config), Some(changed)) = (config, changed) else {
        eprintln!("ci-plan: --config and --changed are required");
        return usage();
    };
    let (text, code) = match run(&config, &changed) {
        Ok(Outcome::Plan(p)) => (if json { render_json(&p) } else { render_text(&p) }, ExitCode::SUCCESS),
        Ok(Outcome::CiMixed { non_ci }) => (render_mixed(&non_ci), ExitCode::from(1)),
        Err(e) => {
            eprintln!("ci-plan: {e}");
            return ExitCode::from(2);
        }
    };
    if emit(&text) {
        code
    } else {
        eprintln!("ci-plan: GATES-IO: cannot write to standard output");
        ExitCode::from(2)
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
