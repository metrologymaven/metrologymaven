//! `fid-brand-lint`: the outreach claim and vocabulary lint (spec node `OUTR.BRAND.LINT`).
//! Project tooling, not a registry command; reads only (nearest class R0) and is
//! deterministic (D0: sorted inputs, BTreeMap only, no clock).
//!
//! This crate is the Markdown and HTML adapter of the shared engine `fid-vocab`
//! (`ECO.RULES.VOCAB.ENGINE`): [`extract`] turns each file into sentences with their links,
//! each sentence becomes one engine [`Unit`] (so "same sentence" and "same unit" coincide),
//! suppression markers become unit suppressions, and [`fid_vocab::lint`] applies the rule
//! file (`rules/claim-rules.toml`, format `fid-vocab-rules/1`, rules CL001 and CL002).
//!
//! Command: `fid-brand-lint claim-lint --rules <file> [--bench-prefix <url>]... [--json] <paths>...`.
//! Exit 0 with no error findings, 1 with any, 2 when an input or the rule file cannot be read
//! or parsed (or a file is over 5 MB, or the usage is wrong).

pub mod extract;

pub use extract::{extract, Comment, Format, SChar, Sentence};

use fid_canon::CValue;
use fid_vocab::{lint, Kind, Report, RuleSet, Suppression, Unit};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Largest accepted input file: 5 MB (REQ-OUTR.BRAND.LINT design, `InputTooLarge`).
pub const MAX_INPUT_BYTES: u64 = 5 * 1024 * 1024;
/// Most files one run accepts.
pub const MAX_FILES: usize = 100_000;
/// Deepest folder nesting followed when a path argument is a folder.
pub const MAX_DEPTH: usize = 64;
/// Text that opens a suppression marker inside an HTML comment.
pub const MARKER_PREFIX: &str = "claim-lint:";
/// File extensions collected when a path argument is a folder.
pub const EXTENSIONS: [&str; 4] = ["md", "markdown", "html", "htm"];

const USAGE: &str = "usage: fid-brand-lint claim-lint --rules <file> [--bench-prefix <url>]... [--json] <paths>...";

/// Errors; each makes the lint exit 2 and report no findings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintError {
    /// The rule file (or the incumbent list it names) cannot be read or parsed.
    RulesParse { path: String, reason: String },
    /// An input cannot be read or is not UTF-8.
    InputUnreadable { path: String, reason: String },
    /// An input is larger than [`MAX_INPUT_BYTES`].
    InputTooLarge { path: String },
    /// A `claim-lint:` comment is not a well-formed marker, or names an unknown rule.
    BadMarker { path: String, line: u32, reason: String },
    /// Wrong command-line usage.
    Usage(String),
}

impl LintError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            LintError::RulesParse { .. } => "rules_parse",
            LintError::InputUnreadable { .. } => "input_unreadable",
            LintError::InputTooLarge { .. } => "input_too_large",
            LintError::BadMarker { .. } => "bad_marker",
            LintError::Usage(_) => "usage",
        }
    }

    /// The file the error is about, if any.
    pub fn path(&self) -> Option<&str> {
        match self {
            LintError::RulesParse { path, .. }
            | LintError::InputUnreadable { path, .. }
            | LintError::InputTooLarge { path }
            | LintError::BadMarker { path, .. } => Some(path),
            LintError::Usage(_) => None,
        }
    }
}

impl fmt::Display for LintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LintError::RulesParse { path, reason } => write!(f, "cannot use rule file {path}: {reason}"),
            LintError::InputUnreadable { path, reason } => write!(f, "cannot read {path}: {reason}"),
            LintError::InputTooLarge { path } => write!(f, "{path} is larger than {MAX_INPUT_BYTES} bytes"),
            LintError::BadMarker { path, line, reason } => write!(f, "{path}:{line}: bad claim-lint marker: {reason}"),
            LintError::Usage(m) => write!(f, "{m}\n{USAGE}"),
        }
    }
}

impl std::error::Error for LintError {}

/// A path as written in reports: forward slashes on every platform.
pub fn display_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Source format from the file extension: `.html` and `.htm` are HTML, everything else Markdown.
pub fn format_of(path: &Path) -> Format {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("html" | "htm") => Format::Html,
        _ => Format::Markdown,
    }
}

// ------------------------------------------------------------------ markers

/// A suppression marker: `<!-- claim-lint: allow <RULE> reason="..." -->`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub line: u32,
    pub rule: String,
    pub reason: String,
}

/// Parse the inside of an HTML comment. `None` when it is not a `claim-lint:` comment;
/// `Some(Err(reason))` when it is one but malformed.
pub fn parse_marker(comment: &str) -> Option<Result<(String, String), String>> {
    let rest = comment.trim().strip_prefix(MARKER_PREFIX)?.trim_start();
    Some(parse_marker_body(rest))
}

fn parse_marker_body(rest: &str) -> Result<(String, String), String> {
    let expected = "expected: claim-lint: allow <RULE> reason=\"<why>\"";
    let rest = rest.strip_prefix("allow").ok_or_else(|| expected.to_string())?;
    if !rest.starts_with(char::is_whitespace) {
        return Err(expected.to_string());
    }
    let rest = rest.trim_start();
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let rule = &rest[..end];
    if rule.is_empty() || !rule.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("bad rule ID; {expected}"));
    }
    let rest = rest[end..].trim();
    let reason = rest
        .strip_prefix("reason=\"")
        .and_then(|r| r.strip_suffix('"'))
        .ok_or_else(|| format!("missing reason; {expected}"))?;
    if reason.trim().is_empty() || reason.contains('"') {
        return Err(format!("the reason must be non-empty and contain no '\"'; {expected}"));
    }
    Ok((rule.to_string(), reason.to_string()))
}

// ------------------------------------------------------------------ units

/// Turn one file's text into engine units: one unit per sentence, with its links and the
/// suppressions of every marker on the line before the sentence or on a line it spans.
/// `rules` is the set of rule IDs a marker may name.
pub fn units_for_text(
    file: &str,
    text: &str,
    format: Format,
    rules: &BTreeSet<String>,
) -> Result<Vec<Unit>, LintError> {
    let ex = extract(text, format);
    let mut markers = Vec::new();
    for c in &ex.comments {
        match parse_marker(&c.text) {
            None => {}
            Some(Err(reason)) => return Err(LintError::BadMarker { path: file.to_string(), line: c.line, reason }),
            Some(Ok((rule, reason))) => {
                if !rules.contains(&rule) {
                    return Err(LintError::BadMarker {
                        path: file.to_string(),
                        line: c.line,
                        reason: format!("unknown rule {rule:?}"),
                    });
                }
                markers.push(Marker { line: c.line, rule, reason });
            }
        }
    }
    let mut units = Vec::new();
    for s in &ex.sentences {
        let start = s.line;
        let end = s.chars.last().map_or(start, |c| c.line);
        let suppress = markers
            .iter()
            .filter(|m| m.line.saturating_add(1) >= start && m.line <= end)
            .map(|m| Suppression { rule: m.rule.clone(), reason: m.reason.clone() })
            .collect();
        units.push(Unit {
            id: format!("{file}:{}:{}", s.line, s.col),
            file: file.to_string(),
            line: s.line,
            col: s.col,
            text: s.text(),
            links: s.links.clone(),
            suppress,
        });
    }
    Ok(units)
}

// ------------------------------------------------------------------ inputs

fn walk(dir: &Path, depth: usize, out: &mut BTreeMap<String, PathBuf>) -> Result<(), LintError> {
    let unreadable = |e: std::io::Error| LintError::InputUnreadable { path: display_path(dir), reason: e.to_string() };
    if depth > MAX_DEPTH {
        return Err(LintError::InputUnreadable {
            path: display_path(dir),
            reason: format!("folders nested deeper than {MAX_DEPTH}"),
        });
    }
    let mut entries = Vec::new();
    for e in std::fs::read_dir(dir).map_err(unreadable)? {
        let e = e.map_err(unreadable)?;
        let ft = e.file_type().map_err(unreadable)?;
        entries.push((e.file_name().to_string_lossy().into_owned(), e.path(), ft));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, path, ft) in entries {
        if name.starts_with('.') {
            continue;
        }
        if ft.is_dir() {
            walk(&path, depth + 1, out)?;
        } else if ft.is_file() {
            let ext = path.extension().and_then(|x| x.to_str()).map(str::to_ascii_lowercase);
            if ext.as_deref().is_some_and(|x| EXTENSIONS.contains(&x)) {
                out.insert(display_path(&path), path);
            }
        }
        if out.len() > MAX_FILES {
            return Err(LintError::InputUnreadable {
                path: display_path(dir),
                reason: format!("more than {MAX_FILES} files"),
            });
        }
    }
    Ok(())
}

/// Expand the path arguments (files as given; folders searched for `.md`, `.markdown`,
/// `.html` and `.htm`, skipping names that start with `.`) into a sorted, de-duplicated map
/// from report path to file path. Argument order does not matter.
pub fn collect_inputs(paths: &[PathBuf]) -> Result<BTreeMap<String, PathBuf>, LintError> {
    let mut out = BTreeMap::new();
    for p in paths {
        let meta = std::fs::metadata(p)
            .map_err(|e| LintError::InputUnreadable { path: display_path(p), reason: e.to_string() })?;
        if meta.is_dir() {
            walk(p, 0, &mut out)?;
        } else {
            out.insert(display_path(p), p.clone());
        }
    }
    Ok(out)
}

/// Read one input: at most [`MAX_INPUT_BYTES`], UTF-8.
pub fn read_input(path: &Path) -> Result<String, LintError> {
    let shown = display_path(path);
    let io = |e: std::io::Error| LintError::InputUnreadable { path: shown.clone(), reason: e.to_string() };
    let file = std::fs::File::open(path).map_err(io)?;
    let mut buf = Vec::new();
    file.take(MAX_INPUT_BYTES + 1).read_to_end(&mut buf).map_err(io)?;
    if buf.len() as u64 > MAX_INPUT_BYTES {
        return Err(LintError::InputTooLarge { path: shown });
    }
    String::from_utf8(buf).map_err(|_| LintError::InputUnreadable { path: shown, reason: "not UTF-8".into() })
}

/// Load the rule file and add the extra benchmark prefixes to every comparison (`cooccur`) rule.
pub fn load_rules(path: &Path, bench_prefixes: &[String]) -> Result<RuleSet, LintError> {
    let mut rules =
        RuleSet::load(path).map_err(|e| LintError::RulesParse { path: display_path(path), reason: e.to_string() })?;
    for r in &mut rules.rules {
        if r.kind == Kind::Cooccur {
            for p in bench_prefixes {
                if !r.link_prefixes.contains(p) {
                    r.link_prefixes.push(p.clone());
                }
            }
        }
    }
    Ok(rules)
}

/// Lint files with a loaded rule set.
pub fn lint_paths(rules: &RuleSet, paths: &[PathBuf]) -> Result<Report, LintError> {
    let ids: BTreeSet<String> = rules.rules.iter().map(|r| r.id.clone()).collect();
    let mut units = Vec::new();
    for (shown, path) in collect_inputs(paths)? {
        let text = read_input(&path)?;
        units.extend(units_for_text(&shown, &text, format_of(&path), &ids)?);
    }
    Ok(lint(rules, &units))
}

// ------------------------------------------------------------------ command line

/// Result of one command-line run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub code: u8,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

struct Args {
    rules: PathBuf,
    bench: Vec<String>,
    json: bool,
    paths: Vec<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<Option<Args>, LintError> {
    let usage = |m: &str| LintError::Usage(m.to_string());
    match args.first().map(String::as_str) {
        Some("claim-lint") => {}
        Some("--help" | "-h" | "help") => return Ok(None),
        Some(other) => return Err(usage(&format!("unknown command `{other}`"))),
        None => return Err(usage("missing command")),
    }
    let (mut rules, mut bench, mut json, mut paths) = (None, Vec::new(), false, Vec::new());
    let mut it = args[1..].iter();
    let mut only_paths = false;
    while let Some(a) = it.next() {
        if only_paths {
            paths.push(PathBuf::from(a));
            continue;
        }
        match a.as_str() {
            "--" => only_paths = true,
            "--json" => json = true,
            "--rules" => rules = Some(PathBuf::from(it.next().ok_or_else(|| usage("--rules needs a file"))?)),
            "--bench-prefix" => {
                let p = it.next().ok_or_else(|| usage("--bench-prefix needs a URL"))?;
                if p.trim().is_empty() {
                    return Err(usage("--bench-prefix must not be empty"));
                }
                bench.push(p.clone());
            }
            "--help" | "-h" => return Ok(None),
            s if s.starts_with("--") => return Err(usage(&format!("unknown option `{s}`"))),
            s => paths.push(PathBuf::from(s)),
        }
    }
    let rules = rules.ok_or_else(|| usage("missing --rules <file>"))?;
    if paths.is_empty() {
        return Err(usage("no input paths"));
    }
    Ok(Some(Args { rules, bench, json, paths }))
}

fn error_output(e: &LintError, json: bool) -> Output {
    let stderr = format!("claim-lint: error: {e}\n");
    let mut stdout = Vec::new();
    if json {
        let mut inner = BTreeMap::new();
        inner.insert("code".to_string(), CValue::Str(e.code().to_string()));
        inner.insert("message".to_string(), CValue::Str(e.to_string()));
        if let Some(p) = e.path() {
            inner.insert("path".to_string(), CValue::Str(p.to_string()));
        }
        let mut o = BTreeMap::new();
        o.insert("error".to_string(), CValue::Obj(inner));
        if let Ok(mut b) = fid_canon::to_canonical_bytes(&CValue::Obj(o)) {
            b.push(b'\n');
            stdout = b;
        }
    }
    Output { code: 2, stdout, stderr }
}

/// Run the command line (`args` without the program name). Never panics; never exits.
pub fn run(args: &[String]) -> Output {
    let json_flag = args.iter().any(|a| a == "--json");
    let a = match parse_args(args) {
        Ok(Some(a)) => a,
        Ok(None) => return Output { code: 0, stdout: format!("{USAGE}\n").into_bytes(), stderr: String::new() },
        Err(e) => return error_output(&e, json_flag),
    };
    let report = match load_rules(&a.rules, &a.bench).and_then(|r| lint_paths(&r, &a.paths)) {
        Ok(r) => r,
        Err(e) => return error_output(&e, a.json),
    };
    let stdout = if a.json {
        match report.to_json() {
            Ok(mut b) => {
                b.push(b'\n');
                b
            }
            Err(e) => {
                let err = LintError::InputUnreadable { path: String::new(), reason: format!("report: {}", e.code()) };
                return error_output(&err, true);
            }
        }
    } else {
        report.to_text().replacen("fid-vocab:", "claim-lint:", 1).into_bytes()
    };
    Output { code: report.exit_code(), stdout, stderr: String::new() }
}
