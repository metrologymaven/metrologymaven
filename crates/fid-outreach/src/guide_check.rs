//! Brand-guideline token check and publish guard (spec node `OUTR.BRAND.GUIDE.NAME`).
//!
//! Every Markdown file under the guideline root writes the family name only as the token
//! [`FAMILY_TOKEN`]. The check reports
//!
//! - `literal_name <file>:<line> <name>` for a case-sensitive, whole-word occurrence of a
//!   working name or of the chosen name from the name state file, outside code spans and fenced
//!   code blocks (REQ-OUTR.BRAND.GUIDE.NAME-003);
//!
//! and, with `--publish`,
//!
//! - `name_not_accepted <name file>` unless the name file records `accepted = true` with a
//!   non-empty `name` and `decision` (REQ-OUTR.BRAND.GUIDE.NAME-004);
//! - `token_left <file>:<line> <token>` for any `{{...}}` left after the family token is
//!   replaced by the chosen name (REQ-OUTR.BRAND.GUIDE.NAME-005).
//!
//! On any finding nothing is written. Determinism D0: a pure function of the input files; the
//! walk and the findings are sorted.

use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The one placeholder for the family name.
pub const FAMILY_TOKEN: &str = "{{FAMILY_NAME}}";
/// Largest name file or guideline file, in bytes.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
/// Most directory entries visited under the root.
pub const MAX_WALK_ENTRIES: usize = 10_000;
/// Deepest directory nesting visited under the root.
pub const MAX_WALK_DEPTH: usize = 32;

/// Finding codes (Design: Errors). Order is the report order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    LiteralName,
    NameNotAccepted,
    TokenLeft,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::LiteralName => "literal_name",
            Code::NameNotAccepted => "name_not_accepted",
            Code::TokenLeft => "token_left",
        }
    }
}

/// One finding. `file` is root-relative with `/` separators (or the name file path for
/// `name_not_accepted`); `line` is 1-based (0 when it does not apply); `detail` is the name or
/// token found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: Code,
    pub file: String,
    pub line: usize,
    pub detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{} {}", self.code.as_str(), self.file)
        } else {
            write!(f, "{} {}:{} {}", self.code.as_str(), self.file, self.line, self.detail)
        }
    }
}

/// Inputs that stop the check (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuideError {
    /// A file or directory could not be read or written, or exceeds a bound.
    Unreadable { path: String, reason: String },
    /// The name state file does not parse or has a field of the wrong shape.
    NameFileParse { path: String, reason: String },
    /// `--out` exists and is not an empty directory; publishing refuses to mix with old files.
    OutNotEmpty { path: String },
}

impl GuideError {
    pub fn code(&self) -> &'static str {
        match self {
            GuideError::Unreadable { .. } => "input_unreadable",
            GuideError::NameFileParse { .. } => "name_file_parse",
            GuideError::OutNotEmpty { .. } => "out_not_empty",
        }
    }

    fn parts(&self) -> (&str, &str) {
        match self {
            GuideError::Unreadable { path, reason } | GuideError::NameFileParse { path, reason } => {
                (path.as_str(), reason.as_str())
            }
            GuideError::OutNotEmpty { path } => (path.as_str(), "exists and is not an empty directory"),
        }
    }

    /// Canonical JSON for `--json` on exit 2; `Err` carries a serialisation failure.
    pub fn to_json(&self) -> Result<Vec<u8>, String> {
        let (path, reason) = self.parts();
        let mut e = BTreeMap::new();
        e.insert("code".to_string(), CValue::Str(self.code().to_string()));
        e.insert("path".to_string(), CValue::Str(path.to_string()));
        e.insert("detail".to_string(), CValue::Str(reason.to_string()));
        let mut o = BTreeMap::new();
        o.insert("error".to_string(), CValue::Obj(e));
        o.insert("ok".to_string(), CValue::Bool(false));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).map_err(|e| format!("{e:?}"))
    }
}

impl fmt::Display for GuideError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (path, reason) = self.parts();
        write!(f, "{} {path}: {reason}", self.code())
    }
}

impl std::error::Error for GuideError {}

fn unreadable(path: &Path, reason: impl Into<String>) -> GuideError {
    GuideError::Unreadable { path: path.display().to_string(), reason: reason.into() }
}

/// The name state file (`brand/name.toml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameState {
    pub accepted: bool,
    pub working_names: Vec<String>,
    pub name: Option<String>,
    pub decision: Option<String>,
    pub date: Option<String>,
}

impl NameState {
    /// The chosen name, when acceptance is fully recorded (`accepted`, `name`, `decision`).
    pub fn accepted_name(&self) -> Option<&str> {
        match (&self.name, &self.decision) {
            (Some(n), Some(_)) if self.accepted => Some(n.as_str()),
            _ => None,
        }
    }

    /// Every name the literal scan looks for: working names, then the chosen name.
    pub fn scanned_names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.working_names.iter().map(String::as_str).collect();
        if let Some(n) = &self.name {
            v.push(n.as_str());
        }
        v
    }
}

const NAME_KEYS: [&str; 5] = ["accepted", "working_names", "name", "decision", "date"];

/// Parses the name state file text. `accepted` is required; unknown keys, empty strings and
/// names containing `{{` or `}}` are rejected.
pub fn parse_name_state(text: &str, path: &str) -> Result<NameState, GuideError> {
    let err = |reason: String| GuideError::NameFileParse { path: path.to_string(), reason };
    let table = text.parse::<toml::Table>().map_err(|e| err(e.message().to_string()))?;
    for key in table.keys() {
        if !NAME_KEYS.contains(&key.as_str()) {
            return Err(err(format!("unknown key `{key}`")));
        }
    }
    let accepted = match table.get("accepted") {
        Some(toml::Value::Boolean(b)) => *b,
        Some(_) => return Err(err("`accepted` must be a boolean".into())),
        None => return Err(err("missing `accepted`".into())),
    };
    let text_field = |key: &str| -> Result<Option<String>, GuideError> {
        match table.get(key) {
            None => Ok(None),
            Some(toml::Value::String(s)) => {
                let s = s.trim();
                if s.is_empty() {
                    Err(err(format!("`{key}` is empty")))
                } else if s.contains("{{") || s.contains("}}") {
                    Err(err(format!("`{key}` contains a token delimiter")))
                } else {
                    Ok(Some(s.to_string()))
                }
            }
            Some(_) => Err(err(format!("`{key}` must be a string"))),
        }
    };
    let mut working_names = Vec::new();
    match table.get("working_names") {
        None => {}
        Some(toml::Value::Array(items)) => {
            for item in items {
                let Some(s) = item.as_str().map(str::trim) else {
                    return Err(err("`working_names` must hold strings".into()));
                };
                if s.is_empty() {
                    return Err(err("`working_names` holds an empty name".into()));
                }
                working_names.push(s.to_string());
            }
        }
        Some(_) => return Err(err("`working_names` must be an array".into())),
    }
    Ok(NameState {
        accepted,
        working_names,
        name: text_field("name")?,
        decision: text_field("decision")?,
        date: text_field("date")?,
    })
}

/// Byte ranges of `line` that are inside inline code spans: a run of N backticks up to the next
/// run of exactly N backticks on the same line. An unclosed run is literal text.
pub fn code_spans(line: &str) -> Vec<(usize, usize)> {
    let b = line.as_bytes();
    let run_at = |i: usize| b.get(i..).map_or(0, |s| s.iter().take_while(|&&c| c == b'`').count());
    let mut spans = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b.get(i) != Some(&b'`') {
            i += 1;
            continue;
        }
        let n = run_at(i);
        let mut j = i + n;
        let mut close = None;
        while j < b.len() {
            if b.get(j) == Some(&b'`') {
                let m = run_at(j);
                if m == n {
                    close = Some(j + m);
                    break;
                }
                j += m;
            } else {
                j += 1;
            }
        }
        match close {
            Some(end) => {
                spans.push((i, end));
                i = end;
            }
            None => i += n,
        }
    }
    spans
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// 1-based line numbers of `text` paired with the line, `\r` stripped, and whether the line is
/// prose (not inside or delimiting a fenced code block opened with three or more backticks or
/// tildes).
fn prose_lines(text: &str) -> Vec<(usize, &str, bool)> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (idx, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let indent = line.len() - line.trim_start_matches(' ').len();
        let body = line.trim_start_matches(' ');
        let marker = body.chars().next().filter(|c| *c == '`' || *c == '~');
        let run = marker.map_or(0, |m| body.chars().take_while(|c| *c == m).count());
        let is_fence = indent <= 3 && run >= 3;
        match (fence, marker) {
            (None, Some(m)) if is_fence => {
                fence = Some((m, run));
                out.push((idx + 1, line, false));
            }
            (Some((fm, fr)), Some(m))
                if is_fence && m == fm && run >= fr && body.get(run..).is_some_and(|t| t.trim().is_empty()) =>
            {
                fence = None;
                out.push((idx + 1, line, false));
            }
            (Some(_), _) => out.push((idx + 1, line, false)),
            (None, _) => out.push((idx + 1, line, true)),
        }
    }
    out
}

/// `literal_name` findings for one file's text (REQ-OUTR.BRAND.GUIDE.NAME-003): whole-word,
/// case-sensitive matches of `names` outside code spans and fenced code blocks. One finding per
/// line and name.
pub fn scan_literal_names(file: &str, text: &str, names: &[&str]) -> Vec<Finding> {
    let mut found = Vec::new();
    for (lineno, line, prose) in prose_lines(text) {
        if !prose {
            continue;
        }
        let spans = code_spans(line);
        for name in names {
            let hit = line.match_indices(name).any(|(start, m)| {
                let end = start + m.len();
                let in_code = spans.iter().any(|&(s, e)| start < e && end > s);
                let before_ok =
                    line.get(..start).and_then(|p| p.chars().next_back()).map_or(true, |c| !is_word_char(c));
                let after_ok = line.get(end..).and_then(|p| p.chars().next()).map_or(true, |c| !is_word_char(c));
                !in_code && before_ok && after_ok
            });
            if hit {
                found.push(Finding {
                    code: Code::LiteralName,
                    file: file.to_string(),
                    line: lineno,
                    detail: name.to_string(),
                });
            }
        }
    }
    found
}

/// `token_left` findings: every `{{...}}` (no braces or line break inside) in `text`.
pub fn scan_tokens(file: &str, text: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    for (idx, raw) in text.split('\n').enumerate() {
        let mut rest = raw;
        while let Some(open) = rest.find("{{") {
            let after = rest.get(open + 2..).unwrap_or("");
            let inner_len = after.find(['{', '}']).unwrap_or(after.len());
            if after.get(inner_len..).is_some_and(|t| t.starts_with("}}")) {
                let token = format!("{{{{{}}}}}", after.get(..inner_len).unwrap_or(""));
                found.push(Finding { code: Code::TokenLeft, file: file.to_string(), line: idx + 1, detail: token });
                rest = after.get(inner_len + 2..).unwrap_or("");
            } else {
                rest = after;
            }
        }
    }
    found
}

/// Command options.
#[derive(Debug, Clone)]
pub struct Options {
    pub root: PathBuf,
    pub name_file: PathBuf,
    /// `Some(out)` with `--publish --out <dir>`.
    pub publish_to: Option<PathBuf>,
}

/// The result of a run that read all its inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub publish: bool,
    pub files: Vec<String>,
    pub findings: Vec<Finding>,
    /// Root-relative paths written under `--out` (empty unless the publish succeeded).
    pub written: Vec<String>,
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        for f in &self.findings {
            s.push_str(&f.to_string());
            s.push('\n');
        }
        s.push_str(&format!(
            "guide-check: {} file(s), {} finding(s){}\n",
            self.files.len(),
            self.findings.len(),
            if self.publish { format!(", {} written", self.written.len()) } else { String::new() }
        ));
        s
    }

    /// Canonical JSON report; `Err` carries a serialisation failure (exit 2).
    pub fn to_json(&self) -> Result<Vec<u8>, String> {
        let strs = |v: &[String]| CValue::Arr(v.iter().map(|s| CValue::Str(s.clone())).collect());
        let findings = self
            .findings
            .iter()
            .map(|f| {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(f.code.as_str().to_string()));
                o.insert("file".to_string(), CValue::Str(f.file.clone()));
                o.insert("line".to_string(), CValue::U64(f.line as u64));
                o.insert("detail".to_string(), CValue::Str(f.detail.clone()));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        o.insert("mode".to_string(), CValue::Str(if self.publish { "publish" } else { "check" }.to_string()));
        o.insert("files".to_string(), strs(&self.files));
        o.insert("findings".to_string(), CValue::Arr(findings));
        o.insert("written".to_string(), strs(&self.written));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).map_err(|e| format!("{e:?}"))
    }
}

fn read_bounded(path: &Path) -> Result<String, GuideError> {
    let file = std::fs::File::open(path).map_err(|e| unreadable(path, e.to_string()))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| unreadable(path, e.to_string()))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(unreadable(path, format!("larger than {MAX_FILE_BYTES} bytes")));
    }
    String::from_utf8(bytes).map_err(|_| unreadable(path, "not UTF-8"))
}

/// Sorted root-relative paths (with `/`) and full paths of the `.md` files under `root`.
/// Symbolic links are not followed.
pub fn markdown_files(root: &Path) -> Result<Vec<(String, PathBuf)>, GuideError> {
    let mut found = Vec::new();
    let mut visited = 0usize;
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((dir, prefix, depth)) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| unreadable(&dir, e.to_string()))? {
            let entry = entry.map_err(|e| unreadable(&dir, e.to_string()))?;
            visited += 1;
            if visited > MAX_WALK_ENTRIES {
                return Err(unreadable(root, format!("more than {MAX_WALK_ENTRIES} entries")));
            }
            let ft = entry.file_type().map_err(|e| unreadable(&entry.path(), e.to_string()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            if ft.is_dir() {
                if depth >= MAX_WALK_DEPTH {
                    return Err(unreadable(&entry.path(), format!("nested deeper than {MAX_WALK_DEPTH}")));
                }
                stack.push((entry.path(), rel, depth + 1));
            } else if ft.is_file() && name.to_ascii_lowercase().ends_with(".md") {
                found.push((rel, entry.path()));
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Runs the check, and with `publish_to` the publish (writing only when there is no finding).
pub fn run(opts: &Options) -> Result<Report, GuideError> {
    let name_path = opts.name_file.display().to_string();
    let state = parse_name_state(&read_bounded(&opts.name_file)?, &name_path)?;
    let names = state.scanned_names();
    let files = markdown_files(&opts.root)?;
    let mut texts = Vec::new();
    let mut findings = Vec::new();
    for (rel, path) in &files {
        let text = read_bounded(path)?;
        findings.extend(scan_literal_names(rel, &text, &names));
        texts.push((rel.clone(), text));
    }
    let mut report = Report {
        publish: opts.publish_to.is_some(),
        files: files.into_iter().map(|(rel, _)| rel).collect(),
        findings: Vec::new(),
        written: Vec::new(),
    };
    if let Some(out) = &opts.publish_to {
        match state.accepted_name() {
            None => {
                findings.push(Finding { code: Code::NameNotAccepted, file: name_path, line: 0, detail: String::new() })
            }
            Some(chosen) => {
                let copies: Vec<(String, String)> =
                    texts.iter().map(|(rel, t)| (rel.clone(), t.replace(FAMILY_TOKEN, chosen))).collect();
                for (rel, t) in &copies {
                    findings.extend(scan_tokens(rel, t));
                }
                if findings.is_empty() {
                    publish_files(out, &copies)?;
                    report.written = copies.into_iter().map(|(rel, _)| rel).collect();
                }
            }
        }
    }
    findings.sort();
    findings.dedup();
    report.findings = findings;
    Ok(report)
}

fn is_empty_dir(dir: &Path) -> Result<bool, GuideError> {
    let mut it = std::fs::read_dir(dir).map_err(|e| unreadable(dir, e.to_string()))?;
    Ok(it.next().is_none())
}

/// Writes `copies` (root-relative path with `/`, text) into `out` all at once: they go to a
/// temporary sibling folder first, which is renamed onto `out` only after every file is written.
/// `out` must not exist or be an empty directory; a non-empty `out` is refused. On any error the
/// temporary folder is removed and `out` is left as it was, so no partial output remains.
pub fn publish_files(out: &Path, copies: &[(String, String)]) -> Result<(), GuideError> {
    let out_exists = out.exists();
    if out_exists && !(out.is_dir() && is_empty_dir(out)?) {
        return Err(GuideError::OutNotEmpty { path: out.display().to_string() });
    }
    let Some(base) = out.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Err(unreadable(out, "output path has no final component"));
    };
    let tmp = out.with_file_name(format!(".{base}.guide-check-{}.tmp", std::process::id()));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(|e| unreadable(&tmp, e.to_string()))?;
    }
    let staged = stage(&tmp, copies).and_then(|()| {
        if out_exists {
            std::fs::remove_dir(out).map_err(|e| unreadable(out, e.to_string()))?;
        }
        std::fs::rename(&tmp, out).map_err(|e| unreadable(out, e.to_string()))
    });
    if staged.is_err() {
        // Best effort: the error being returned matters more than a failed cleanup.
        let _ = std::fs::remove_dir_all(&tmp);
    }
    staged
}

fn stage(tmp: &Path, copies: &[(String, String)]) -> Result<(), GuideError> {
    std::fs::create_dir_all(tmp).map_err(|e| unreadable(tmp, e.to_string()))?;
    for (rel, text) in copies {
        let dest = rel.split('/').fold(tmp.to_path_buf(), |p, part| p.join(part));
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| unreadable(parent, e.to_string()))?;
        }
        std::fs::write(&dest, text).map_err(|e| unreadable(&dest, e.to_string()))?;
    }
    Ok(())
}
