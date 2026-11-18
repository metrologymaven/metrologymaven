//! Human trade-dress review gate (spec node `OUTR.BRAND.REVIEW`).
//!
//! Automation cannot judge look and feel, so a person other than a release's author answers the
//! trade-dress checklist (`brand/trade-dress-checklist.toml`) and commits a review record for the
//! release. This module only **reads** those human decisions: it has no code path that writes,
//! fills in or amends a record, and it never decides an item itself. The gate refuses
//! publication (exit 1) when
//!
//! - the reviewer is not on the roster (`unknown_reviewer`);
//! - the reviewer is the release author, compared case-insensitively (`self_review`);
//! - a checklist item has no answer (`missing_answer:<item>`);
//! - a checklist item is answered with anything other than exactly `"pass"`
//!   (`failed_item:<item>`).
//!
//! A missing record, or an input that cannot be read or parsed, stops the gate (exit 2).
//! Determinism D0: a pure function of the three input files; findings are sorted.
//!
//! The checklist and record parsers here are shared with [`crate::copy_guard`].

use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{ErrorKind, Read};
use std::path::Path;

/// Largest record, roster or checklist file, in bytes.
pub const MAX_FILE_BYTES: usize = 64 * 1024;
/// Most handles in a roster.
pub const MAX_ROSTER_HANDLES: usize = 10_000;
/// The only answer that passes an item.
pub const PASS: &str = "pass";
/// Suffix of automation accounts (GitHub app and bot handles). A roster may not list one: only
/// people may pass a review.
pub const BOT_SUFFIX: &str = "[bot]";

// ------------------------------------------------------------------ errors

/// Inputs that stop the gate (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateError {
    /// The review record file does not exist.
    RecordMissing { path: String },
    /// Another input file does not exist, cannot be read, or exceeds [`MAX_FILE_BYTES`].
    Unreadable { path: String, reason: String },
    /// A file is not UTF-8, not valid TOML, or lacks or mistypes a required field.
    Parse { path: String, reason: String },
}

impl GateError {
    pub fn code(&self) -> &'static str {
        match self {
            GateError::RecordMissing { .. } => "record_missing",
            GateError::Unreadable { .. } => "input_unreadable",
            GateError::Parse { .. } => "parse_error",
        }
    }

    pub fn path(&self) -> &str {
        match self {
            GateError::RecordMissing { path } | GateError::Unreadable { path, .. } | GateError::Parse { path, .. } => {
                path
            }
        }
    }

    pub fn reason(&self) -> &str {
        match self {
            GateError::RecordMissing { .. } => "no such file",
            GateError::Unreadable { reason, .. } | GateError::Parse { reason, .. } => reason,
        }
    }

    /// Canonical JSON for `--json` on exit 2.
    pub fn to_json(&self) -> Vec<u8> {
        let mut e = BTreeMap::new();
        e.insert("code".to_string(), CValue::Str(self.code().to_string()));
        e.insert("path".to_string(), CValue::Str(self.path().to_string()));
        e.insert("detail".to_string(), CValue::Str(self.reason().to_string()));
        let mut o = BTreeMap::new();
        o.insert("error".to_string(), CValue::Obj(e));
        o.insert("ok".to_string(), CValue::Bool(false));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }
}

impl fmt::Display for GateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code(), self.path(), self.reason())
    }
}

impl std::error::Error for GateError {}

fn parse_err(path: &str, reason: impl Into<String>) -> GateError {
    GateError::Parse { path: path.to_string(), reason: reason.into() }
}

// ------------------------------------------------------------------ reading

/// Reads at most `limit` bytes of `path` as UTF-8; more is an error. `NotFound` maps to
/// `missing(path)`.
fn read_text(path: &Path, limit: usize, missing: fn(String) -> GateError) -> Result<String, GateError> {
    let shown = path.display().to_string();
    let mut bytes = Vec::new();
    let res = std::fs::File::open(path).and_then(|f| f.take(limit as u64 + 1).read_to_end(&mut bytes));
    match res {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(missing(shown)),
        Err(e) => return Err(GateError::Unreadable { path: shown, reason: e.to_string() }),
    }
    if bytes.len() > limit {
        return Err(GateError::Unreadable { path: shown, reason: format!("larger than {limit} bytes") });
    }
    String::from_utf8(bytes).map_err(|_| parse_err(&shown, "not UTF-8"))
}

fn not_found(path: String) -> GateError {
    GateError::Unreadable { path, reason: "no such file".to_string() }
}

fn record_not_found(path: String) -> GateError {
    GateError::RecordMissing { path }
}

/// Reads and parses a bounded TOML file. A missing file is [`GateError::Unreadable`].
pub fn read_toml(path: &Path) -> Result<toml::Table, GateError> {
    let text = read_text(path, MAX_FILE_BYTES, not_found)?;
    text.parse::<toml::Table>().map_err(|e| parse_err(&path.display().to_string(), e.message().to_string()))
}

fn opt_str<'t>(t: &'t toml::Table, key: &str, path: &str) -> Result<Option<&'t str>, GateError> {
    match t.get(key) {
        None => Ok(None),
        Some(v) => match v.as_str() {
            Some(s) => Ok(Some(s)),
            None => Err(parse_err(path, format!("`{key}` is not a string"))),
        },
    }
}

fn req_str<'t>(t: &'t toml::Table, key: &str, path: &str) -> Result<&'t str, GateError> {
    match opt_str(t, key, path)?.map(str::trim) {
        Some(s) if !s.is_empty() => Ok(s),
        _ => Err(parse_err(path, format!("no non-empty `{key}`"))),
    }
}

// ------------------------------------------------------------------ checklist

/// One checklist item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub question: String,
}

/// The trade-dress checklist (`version`, `items = [{ id, question }]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checklist {
    pub version: i64,
    /// In file order. IDs are unique.
    pub items: Vec<Item>,
}

impl Checklist {
    /// The item IDs, sorted.
    pub fn ids(&self) -> BTreeSet<String> {
        self.items.iter().map(|i| i.id.clone()).collect()
    }
}

/// Parses a checklist table (REQ-OUTR.BRAND.REVIEW-001). Requires an integer `version` of at
/// least 1 and a non-empty `items` array of tables with a non-empty `id` and `question`; IDs must
/// be unique.
pub fn parse_checklist(table: &toml::Table, path: &str) -> Result<Checklist, GateError> {
    let version = match table.get("version").and_then(toml::Value::as_integer) {
        Some(v) if v >= 1 => v,
        _ => return Err(parse_err(path, "no integer `version` of at least 1")),
    };
    let items = table.get("items").and_then(toml::Value::as_array).ok_or_else(|| parse_err(path, "no `items` array"))?;
    if items.is_empty() {
        return Err(parse_err(path, "`items` is empty"));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let t = item.as_table().ok_or_else(|| parse_err(path, "`items` entry is not a table"))?;
        let id = req_str(t, "id", path)?;
        let question = req_str(t, "question", path)?;
        if !seen.insert(id.to_string()) {
            return Err(parse_err(path, format!("duplicate item `{id}`")));
        }
        out.push(Item { id: id.to_string(), question: question.to_string() });
    }
    Ok(Checklist { version, items: out })
}

/// Reads and parses a checklist file.
pub fn read_checklist(path: &Path) -> Result<Checklist, GateError> {
    parse_checklist(&read_toml(path)?, &path.display().to_string())
}

// ------------------------------------------------------------------ record

/// One answer, as the reviewer wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The `result` string; `None` when the answer has no string `result`.
    pub result: Option<String>,
    pub note: Option<String>,
}

impl Answer {
    /// Only exactly `"pass"` passes; any other result fails closed.
    pub fn passed(&self) -> bool {
        self.result.as_deref() == Some(PASS)
    }
}

/// A review record (`release`, `reviewer`, `date`, `answers.<item> = { result, note }`).
///
/// [`parse_record`] requires only `release`, so that other checks (the copy guard) can read
/// records in part; the gate additionally requires `reviewer` and `date` ([`require_complete`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub release: String,
    pub reviewer: Option<String>,
    /// `YYYY-MM-DD`, written as a string or a TOML local date.
    pub date: Option<String>,
    /// Keyed by item ID.
    pub answers: BTreeMap<String, Answer>,
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn parse_date(t: &toml::Table, path: &str) -> Result<Option<String>, GateError> {
    let text = match t.get("date") {
        None => return Ok(None),
        Some(toml::Value::String(s)) => s.trim().to_string(),
        Some(toml::Value::Datetime(d)) if d.time.is_none() && d.offset.is_none() => d.to_string(),
        Some(_) => return Err(parse_err(path, "`date` is not a YYYY-MM-DD date")),
    };
    if is_iso_date(&text) {
        Ok(Some(text))
    } else {
        Err(parse_err(path, "`date` is not a YYYY-MM-DD date"))
    }
}

/// Parses a record table (REQ-OUTR.BRAND.REVIEW-002). `release` is required; `reviewer` and
/// `date` are read when present and must then be well-formed. `answers`, when present, must be a
/// table of tables; within an answer, `result` is kept only when it is a string and `note` must
/// be a string when present.
pub fn parse_record(table: &toml::Table, path: &str) -> Result<Record, GateError> {
    let release = req_str(table, "release", path)?.to_string();
    let reviewer = match opt_str(table, "reviewer", path)?.map(str::trim) {
        None => None,
        Some("") => return Err(parse_err(path, "`reviewer` is empty")),
        Some(r) => Some(r.to_string()),
    };
    let date = parse_date(table, path)?;
    let mut answers = BTreeMap::new();
    if let Some(v) = table.get("answers") {
        let a = v.as_table().ok_or_else(|| parse_err(path, "`answers` is not a table"))?;
        for (item, v) in a {
            let t = v.as_table().ok_or_else(|| parse_err(path, format!("answer `{item}` is not a table")))?;
            let result = t.get("result").and_then(toml::Value::as_str).map(str::to_string);
            let note = opt_str(t, "note", path)?.map(str::to_string);
            answers.insert(item.clone(), Answer { result, note });
        }
    }
    Ok(Record { release, reviewer, date, answers })
}

/// The gate needs a reviewer and a date in every record (REQ-OUTR.BRAND.REVIEW-002).
pub fn require_complete(record: &Record, path: &str) -> Result<(), GateError> {
    if record.reviewer.is_none() {
        return Err(parse_err(path, "no `reviewer`"));
    }
    if record.date.is_none() {
        return Err(parse_err(path, "no `date`"));
    }
    Ok(())
}

/// Reads and parses a record file. A missing file is [`GateError::RecordMissing`].
pub fn read_record(path: &Path) -> Result<Record, GateError> {
    let shown = path.display().to_string();
    let text = read_text(path, MAX_FILE_BYTES, record_not_found)?;
    let table = text.parse::<toml::Table>().map_err(|e| parse_err(&shown, e.message().to_string()))?;
    parse_record(&table, &shown)
}

// ------------------------------------------------------------------ roster

/// Case-insensitive form of a handle, used for every comparison.
pub fn fold(handle: &str) -> String {
    handle.trim().to_lowercase()
}

/// Parses a roster: one handle per line. Blank lines and lines starting with `#` are skipped.
/// A handle may not contain whitespace and may not be an automation account (ends with
/// [`BOT_SUFFIX`]). Returns the folded handles.
pub fn parse_roster(text: &str, path: &str) -> Result<BTreeSet<String>, GateError> {
    let mut out = BTreeSet::new();
    for (n, line) in text.lines().enumerate() {
        let h = line.trim();
        if h.is_empty() || h.starts_with('#') {
            continue;
        }
        if h.chars().any(char::is_whitespace) {
            return Err(parse_err(path, format!("line {}: a handle may not contain whitespace", n + 1)));
        }
        let folded = fold(h);
        if folded.ends_with(BOT_SUFFIX) {
            return Err(parse_err(path, format!("line {}: `{h}` is an automation account, not a person", n + 1)));
        }
        out.insert(folded);
        if out.len() > MAX_ROSTER_HANDLES {
            return Err(parse_err(path, format!("more than {MAX_ROSTER_HANDLES} handles")));
        }
    }
    if out.is_empty() {
        return Err(parse_err(path, "no handles"));
    }
    Ok(out)
}

/// Reads and parses a roster file.
pub fn read_roster(path: &Path) -> Result<BTreeSet<String>, GateError> {
    parse_roster(&read_text(path, MAX_FILE_BYTES, not_found)?, &path.display().to_string())
}

// ------------------------------------------------------------------ gate

/// Finding kinds (Design: Errors). Order is the report order for findings without an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    UnknownReviewer,
    SelfReview,
    MissingAnswer,
    FailedItem,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::UnknownReviewer => "unknown_reviewer",
            Code::SelfReview => "self_review",
            Code::MissingAnswer => "missing_answer",
            Code::FailedItem => "failed_item",
        }
    }
}

/// One finding. `item` is the checklist item for `missing_answer` and `failed_item`, else empty.
/// Field order gives the report order: reviewer findings first, then item findings sorted by item
/// ID.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub item: String,
    pub code: Code,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.item.is_empty() {
            f.write_str(self.code.as_str())
        } else {
            write!(f, "{}:{}", self.code.as_str(), self.item)
        }
    }
}

/// The gate's result for one release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub release: String,
    pub reviewer: String,
    /// Sorted, without duplicates.
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    /// Text lines: one `<release> <finding>` line per finding, then a summary.
    pub fn render(&self) -> String {
        let mut s = String::new();
        for f in &self.findings {
            s.push_str(&format!("{} {f}\n", self.release));
        }
        let verdict = if self.findings.is_empty() { "passed" } else { "refused" };
        s.push_str(&format!(
            "review-gate: release {} reviewed by {}: {verdict}, {} finding(s)\n",
            self.release,
            self.reviewer,
            self.findings.len()
        ));
        s
    }

    /// Canonical JSON (RFC 8785 via `fid-canon`).
    pub fn to_json(&self) -> Vec<u8> {
        let items = self
            .findings
            .iter()
            .map(|f| {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(f.code.as_str().to_string()));
                o.insert("item".to_string(), CValue::Str(f.item.clone()));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("findings".to_string(), CValue::Arr(items));
        o.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        o.insert("release".to_string(), CValue::Str(self.release.clone()));
        o.insert("reviewer".to_string(), CValue::Str(self.reviewer.clone()));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }
}

/// Applies the gate rules (REQ-OUTR.BRAND.REVIEW-003..005) to parsed inputs. `roster` holds
/// folded handles ([`parse_roster`]). Answers to items not on the checklist are ignored.
pub fn check(checklist: &Checklist, record: &Record, author: &str, roster: &BTreeSet<String>) -> Report {
    let reviewer = record.reviewer.clone().unwrap_or_default();
    let folded = fold(&reviewer);
    let mut findings = BTreeSet::new();
    if folded.is_empty() || !roster.contains(&folded) {
        findings.insert(Finding { item: String::new(), code: Code::UnknownReviewer });
    }
    if !folded.is_empty() && folded == fold(author) {
        findings.insert(Finding { item: String::new(), code: Code::SelfReview });
    }
    for item in &checklist.items {
        let code = match record.answers.get(&item.id) {
            None | Some(Answer { result: None, .. }) => Code::MissingAnswer,
            Some(a) if a.passed() => continue,
            Some(_) => Code::FailedItem,
        };
        findings.insert(Finding { item: item.id.clone(), code });
    }
    Report { release: record.release.clone(), reviewer, findings: findings.into_iter().collect() }
}

/// Paths and author given to [`run`].
#[derive(Debug, Clone)]
pub struct Inputs<'a> {
    pub record: &'a Path,
    pub author: &'a str,
    pub roster: &'a Path,
    pub checklist: &'a Path,
}

/// Reads the inputs and runs the gate. The record is read first, so a missing record is reported
/// before anything else (AC-6). An empty author is a usage error reported as a parse error of
/// `--author`.
pub fn run(inputs: &Inputs<'_>) -> Result<Report, GateError> {
    let record = read_record(inputs.record)?;
    require_complete(&record, &inputs.record.display().to_string())?;
    let roster = read_roster(inputs.roster)?;
    let checklist = read_checklist(inputs.checklist)?;
    if inputs.author.trim().is_empty() {
        return Err(parse_err("--author", "empty author"));
    }
    Ok(check(&checklist, &record, inputs.author, &roster))
}
