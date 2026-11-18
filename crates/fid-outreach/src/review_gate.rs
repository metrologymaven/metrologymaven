//! Human trade-dress review gate (spec node `OUTR.BRAND.REVIEW`).
//!
//! Automation cannot judge look and feel, so a person other than a release's author answers the
//! trade-dress checklist (`brand/trade-dress-checklist.toml`) and commits a review record for the
//! release. This module only **reads** those human decisions: it has no code path that writes,
//! fills in or amends a record, and it never decides an item itself. The gate refuses
//! publication (exit 1) when
//!
//! - the reviewer is not on the roster (`unknown_reviewer`), or is an automation account
//!   (`bot_reviewer`);
//! - the reviewer is one of the release's authors, compared case-insensitively (`self_review`);
//! - the record is for another release (`release_mismatch`) or another checklist version
//!   (`checklist_version_mismatch`);
//! - a checklist item has no answer (`missing_answer:<item>`);
//! - a checklist item is answered with anything other than exactly `"pass"`
//!   (`failed_item:<item>`);
//! - the record answers an item that is not on the checklist (`unknown_item:<item>`).
//!
//! Every handle (roster, reviewer, author) must follow the GitHub login grammar, and the roster
//! may not list a denied automation account ([`DENIED_HANDLES`] and the optional bots register).
//! This does not authenticate the record: whoever can commit a record can name any reviewer; the
//! repository's pull-request review is what ties a record to a person.
//!
//! A missing record, or an input that cannot be read or parsed, stops the gate (exit 2).
//! Determinism D0: a pure function of the input files and arguments; findings are sorted.
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
    let items =
        table.get("items").and_then(toml::Value::as_array).ok_or_else(|| parse_err(path, "no `items` array"))?;
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

// ------------------------------------------------------------------ handles

/// True when `s` follows the GitHub login grammar: ASCII letters and digits, single hyphens
/// between them, 1 to 39 letters or digits (`^[A-Za-z0-9](-?[A-Za-z0-9]){0,38}$`). This rejects
/// `[bot]` app accounts, `agent:*` style identities and Unicode lookalikes.
pub fn is_valid_handle(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 77 {
        return false;
    }
    let mut groups = 0usize;
    let mut prev_hyphen = true; // a leading hyphen is invalid
    for &c in b {
        if c == b'-' {
            if prev_hyphen {
                return false;
            }
            prev_hyphen = true;
        } else if c.is_ascii_alphanumeric() {
            groups += 1;
            prev_hyphen = false;
        } else {
            return false;
        }
    }
    // `(-?[A-Za-z0-9]){0,38}` after the first character: at most 39 alphanumerics in all.
    !prev_hyphen && groups <= 39
}

/// Case-insensitive form of a handle (GitHub logins are ASCII and case-insensitive).
pub fn fold(handle: &str) -> String {
    handle.trim().to_ascii_lowercase()
}

/// Automation accounts that may never review, whatever a roster says.
pub const DENIED_HANDLES: [&str; 3] = ["github-actions", "dependabot", "renovate"];

/// The folded denylist: [`DENIED_HANDLES`] plus the `account` of every `[[bot]]` entry of a
/// bots register (`bots.toml`, spec node ECO.CI.BOTS.REG) when one is given. A register account
/// such as `renovate[bot]` or `app/renovate` is denied under its bare login.
pub fn denylist(bots: Option<&toml::Table>, path: &str) -> Result<BTreeSet<String>, GateError> {
    let mut out: BTreeSet<String> = DENIED_HANDLES.iter().map(|h| (*h).to_string()).collect();
    let Some(t) = bots else { return Ok(out) };
    let Some(v) = t.get("bot") else { return Ok(out) };
    let arr = v.as_array().ok_or_else(|| parse_err(path, "`bot` is not an array of tables"))?;
    for entry in arr {
        let e = entry.as_table().ok_or_else(|| parse_err(path, "`[[bot]]` entry is not a table"))?;
        let account = req_str(e, "account", path)?;
        let bare = account.strip_prefix("app/").unwrap_or(account);
        let bare = bare.strip_suffix("[bot]").unwrap_or(bare);
        out.insert(fold(bare));
    }
    Ok(out)
}

/// Reads a bots register and returns the folded denylist ([`denylist`]).
pub fn read_denylist(path: Option<&Path>) -> Result<BTreeSet<String>, GateError> {
    match path {
        None => denylist(None, ""),
        Some(p) => denylist(Some(&read_toml(p)?), &p.display().to_string()),
    }
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

/// A review record (`release`, `reviewer`, `date`, `checklist_version`,
/// `answers.<item> = { result, note }`).
///
/// [`parse_record`] requires only `release`, so that other checks (the copy guard) can read
/// records in part; the gate additionally requires `reviewer`, `date` and `checklist_version`
/// ([`require_complete`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub release: String,
    /// A GitHub login ([`is_valid_handle`]).
    pub reviewer: Option<String>,
    /// A real calendar date `YYYY-MM-DD`, written as a string or a TOML local date.
    pub date: Option<String>,
    /// The `version` of the checklist the reviewer answered.
    pub checklist_version: Option<i64>,
    /// Keyed by item ID.
    pub answers: BTreeMap<String, Answer>,
}

/// True when `s` is `YYYY-MM-DD` naming a real day of the proleptic Gregorian calendar.
pub fn is_calendar_date(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = b.get(r)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some(part.iter().fold(0u32, |n, c| n * 10 + u32::from(c - b'0')))
    };
    if b.len() != 10 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return false;
    }
    let (Some(y), Some(m), Some(d)) = (digits(0..4), digits(5..7), digits(8..10)) else { return false };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}

fn parse_date(t: &toml::Table, path: &str) -> Result<Option<String>, GateError> {
    let text = match t.get("date") {
        None => return Ok(None),
        Some(toml::Value::String(s)) => s.trim().to_string(),
        Some(toml::Value::Datetime(d)) if d.time.is_none() && d.offset.is_none() => d.to_string(),
        Some(_) => return Err(parse_err(path, "`date` is not a YYYY-MM-DD date")),
    };
    if is_calendar_date(&text) {
        Ok(Some(text))
    } else {
        Err(parse_err(path, format!("`date` {text:?} is not a real YYYY-MM-DD date")))
    }
}

/// Parses a record table (REQ-OUTR.BRAND.REVIEW-002). `release` is required; `reviewer`, `date`
/// and `checklist_version` are read when present and must then be well-formed (`reviewer` a
/// GitHub login). `answers`, when present, must be a table of tables; within an answer, `result`
/// is kept only when it is a string and `note` must be a string when present.
pub fn parse_record(table: &toml::Table, path: &str) -> Result<Record, GateError> {
    let release = req_str(table, "release", path)?.to_string();
    let reviewer = match opt_str(table, "reviewer", path)?.map(str::trim) {
        None => None,
        Some(r) if is_valid_handle(r) => Some(r.to_string()),
        Some(r) => return Err(parse_err(path, format!("`reviewer` {r:?} is not a GitHub login"))),
    };
    let date = parse_date(table, path)?;
    let checklist_version = match table.get("checklist_version") {
        None => None,
        Some(v) => Some(v.as_integer().ok_or_else(|| parse_err(path, "`checklist_version` is not an integer"))?),
    };
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
    Ok(Record { release, reviewer, date, checklist_version, answers })
}

/// The gate needs a reviewer, a date and a checklist version in every record
/// (REQ-OUTR.BRAND.REVIEW-002).
pub fn require_complete(record: &Record, path: &str) -> Result<(), GateError> {
    if record.reviewer.is_none() {
        return Err(parse_err(path, "no `reviewer`"));
    }
    if record.date.is_none() {
        return Err(parse_err(path, "no `date`"));
    }
    if record.checklist_version.is_none() {
        return Err(parse_err(path, "no `checklist_version`"));
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

/// Parses a roster: one handle per line. Blank lines and lines starting with `#` are skipped.
/// Every handle must be a GitHub login ([`is_valid_handle`]) and not on `denied`. Returns the
/// folded handles.
pub fn parse_roster(text: &str, path: &str, denied: &BTreeSet<String>) -> Result<BTreeSet<String>, GateError> {
    let mut out = BTreeSet::new();
    for (n, line) in text.lines().enumerate() {
        let h = line.trim();
        if h.is_empty() || h.starts_with('#') {
            continue;
        }
        if !is_valid_handle(h) {
            return Err(parse_err(path, format!("line {}: {h:?} is not a GitHub login", n + 1)));
        }
        let folded = fold(h);
        if denied.contains(&folded) {
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
pub fn read_roster(path: &Path, denied: &BTreeSet<String>) -> Result<BTreeSet<String>, GateError> {
    parse_roster(&read_text(path, MAX_FILE_BYTES, not_found)?, &path.display().to_string(), denied)
}

// ------------------------------------------------------------------ gate

/// Finding kinds (Design: Errors, plus the record/release consistency checks). Order is the
/// report order among findings with the same item (reviewer and record findings have none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    UnknownReviewer,
    BotReviewer,
    SelfReview,
    ReleaseMismatch,
    ChecklistVersionMismatch,
    MissingAnswer,
    FailedItem,
    UnknownItem,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::UnknownReviewer => "unknown_reviewer",
            Code::BotReviewer => "bot_reviewer",
            Code::SelfReview => "self_review",
            Code::ReleaseMismatch => "release_mismatch",
            Code::ChecklistVersionMismatch => "checklist_version_mismatch",
            Code::MissingAnswer => "missing_answer",
            Code::FailedItem => "failed_item",
            Code::UnknownItem => "unknown_item",
        }
    }
}

/// One finding. `item` is the item ID for `missing_answer`, `failed_item` and `unknown_item`,
/// else empty. Field order gives the report order: findings without an item first, then item
/// findings sorted by item ID.
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
    /// The release given to the gate (`--release`).
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

/// What the record is checked against, besides the checklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expect<'a> {
    /// The release being published (`--release`).
    pub release: &'a str,
    /// The release's authors and co-authors (`--author`, repeatable); none may review it.
    pub authors: &'a [String],
    /// Folded roster handles ([`parse_roster`]).
    pub roster: &'a BTreeSet<String>,
    /// Folded automation accounts ([`denylist`]).
    pub denied: &'a BTreeSet<String>,
}

/// Applies the gate rules (REQ-OUTR.BRAND.REVIEW-003..005) to parsed inputs.
pub fn check(checklist: &Checklist, record: &Record, expect: &Expect<'_>) -> Report {
    let reviewer = record.reviewer.clone().unwrap_or_default();
    let folded = fold(&reviewer);
    let mut findings = BTreeSet::new();
    let mut add = |item: &str, code: Code| {
        findings.insert(Finding { item: item.to_string(), code });
    };
    if folded.is_empty() || !expect.roster.contains(&folded) {
        add("", Code::UnknownReviewer);
    }
    if expect.denied.contains(&folded) {
        add("", Code::BotReviewer);
    }
    if !folded.is_empty() && expect.authors.iter().any(|a| fold(a) == folded) {
        add("", Code::SelfReview);
    }
    if record.release != expect.release {
        add("", Code::ReleaseMismatch);
    }
    if record.checklist_version != Some(checklist.version) {
        add("", Code::ChecklistVersionMismatch);
    }
    for item in &checklist.items {
        match record.answers.get(&item.id) {
            None | Some(Answer { result: None, .. }) => add(&item.id, Code::MissingAnswer),
            Some(a) if a.passed() => {}
            Some(_) => add(&item.id, Code::FailedItem),
        }
    }
    let known = checklist.ids();
    for id in record.answers.keys().filter(|id| !known.contains(*id)) {
        add(id, Code::UnknownItem);
    }
    Report { release: expect.release.to_string(), reviewer, findings: findings.into_iter().collect() }
}

/// Paths and identities given to [`run`].
#[derive(Debug, Clone)]
pub struct Inputs<'a> {
    pub record: &'a Path,
    pub release: &'a str,
    pub authors: &'a [String],
    pub roster: &'a Path,
    pub checklist: &'a Path,
    /// Optional bots register (`bots.toml`) whose accounts join the denylist.
    pub bots: Option<&'a Path>,
}

/// Reads the inputs and runs the gate. The record is read first, so a missing record is reported
/// before anything else (AC-6). An empty `--release`, no `--author`, or an author that is not a
/// GitHub login is reported as a parse error of that option.
pub fn run(inputs: &Inputs<'_>) -> Result<Report, GateError> {
    let record = read_record(inputs.record)?;
    require_complete(&record, &inputs.record.display().to_string())?;
    let denied = read_denylist(inputs.bots)?;
    let roster = read_roster(inputs.roster, &denied)?;
    let checklist = read_checklist(inputs.checklist)?;
    if inputs.release.trim().is_empty() {
        return Err(parse_err("--release", "empty release"));
    }
    if inputs.authors.is_empty() {
        return Err(parse_err("--author", "no author given"));
    }
    if let Some(a) = inputs.authors.iter().find(|a| !is_valid_handle(a.trim())) {
        return Err(parse_err("--author", format!("{a:?} is not a GitHub login")));
    }
    let expect = Expect { release: inputs.release.trim(), authors: inputs.authors, roster: &roster, denied: &denied };
    Ok(check(&checklist, &record, &expect))
}
