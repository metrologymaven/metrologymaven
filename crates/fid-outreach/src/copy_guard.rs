//! Copied-text policy guard (spec node `OUTR.BRAND.COPY`, decision D-013 option C).
//!
//! D-013 chose to keep no reference set of incumbents' text and to run no automatic copy
//! matcher: the clean-room rule and the human `copied_text` item of the trade-dress review
//! (`OUTR.BRAND.REVIEW`) are the control. This guard keeps that arrangement from eroding. It
//! fails when
//!
//! - the policy file leaves `mode = "review_only"` without a `counsel_review` reference
//!   (`counsel_review_missing`);
//! - the trade-dress checklist has no `copied_text` item (`copied_text_item_missing`);
//! - a review record answers `copied_text` with `fail` and the policy has no `[[revisit]]`
//!   entry for that release (`d013_revisit_due <release>`);
//! - in `review_only` mode, a file under the scanned root declares itself a reference set of
//!   third-party text: a `.refset` name part, or a TOML or JSON file whose top-level
//!   `kind` is `"incumbent_text"` (`reference_set_present <path>`).
//!
//! The guard never reads, stores or compares third-party text. Determinism D0: a pure function
//! of the input files; the walk and the findings are sorted.

use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest policy, checklist or record file, in bytes.
pub const MAX_FILE_BYTES: usize = 64 * 1024;
/// Largest TOML or JSON file whose content the reference-set scan reads, in bytes.
pub const MAX_SCAN_BYTES: usize = 4 * 1024 * 1024;
/// Most review records read from the records directory.
pub const MAX_RECORDS: usize = 4096;
/// Most directory entries visited by the reference-set scan.
pub const MAX_WALK_ENTRIES: usize = 200_000;
/// Deepest directory nesting visited by the reference-set scan.
pub const MAX_WALK_DEPTH: usize = 64;
/// Directory names the scan never enters, at any depth: version-control data, not content.
pub const SKIPPED_DIRS: [&str; 1] = [".git"];
/// Directory names the scan does not enter directly under the scan root only: build output.
/// Deeper directories with these names (for example `brand/target/`) are scanned.
pub const SKIPPED_ROOT_DIRS: [&str; 2] = ["target", "node_modules"];
/// The only policy mode under which D-013 option C holds.
pub const REVIEW_ONLY: &str = "review_only";
/// The decision the policy file must name.
pub const DECISION: &str = "D-013";
/// The checklist item and answer key the guard relies on.
pub const COPIED_TEXT: &str = "copied_text";
/// Top-level `kind` value that declares a reference set of incumbents' text.
pub const INCUMBENT_KIND: &str = "incumbent_text";

/// Finding codes (Design: Errors). Order is the report order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    CounselReviewMissing,
    CopiedTextItemMissing,
    D013RevisitDue,
    ReferenceSetPresent,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::CounselReviewMissing => "counsel_review_missing",
            Code::CopiedTextItemMissing => "copied_text_item_missing",
            Code::D013RevisitDue => "d013_revisit_due",
            Code::ReferenceSetPresent => "reference_set_present",
        }
    }
}

/// One finding. `subject` is the release (`d013_revisit_due`), the root-relative path with `/`
/// separators (`reference_set_present`), or empty.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: Code,
    pub subject: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.subject.is_empty() {
            f.write_str(self.code.as_str())
        } else {
            write!(f, "{} {}", self.code.as_str(), self.subject)
        }
    }
}

/// Inputs that stop the guard (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    /// A file or directory could not be read, or exceeds a bound.
    Unreadable { path: String, reason: String },
    /// The policy file does not parse or lacks a required field.
    PolicyParse { path: String, reason: String },
    /// The checklist, a record or a scanned declaration does not parse.
    InputParse { path: String, reason: String },
}

impl GuardError {
    pub fn code(&self) -> &'static str {
        match self {
            GuardError::Unreadable { .. } => "input_unreadable",
            GuardError::PolicyParse { .. } => "policy_parse",
            GuardError::InputParse { .. } => "input_parse",
        }
    }

    fn parts(&self) -> (&str, &str) {
        match self {
            GuardError::Unreadable { path, reason }
            | GuardError::PolicyParse { path, reason }
            | GuardError::InputParse { path, reason } => (path, reason),
        }
    }

    /// Canonical JSON for `--json` on exit 2.
    pub fn to_json(&self) -> Vec<u8> {
        let (path, reason) = self.parts();
        let mut e = BTreeMap::new();
        e.insert("code".to_string(), CValue::Str(self.code().to_string()));
        e.insert("path".to_string(), CValue::Str(path.to_string()));
        e.insert("detail".to_string(), CValue::Str(reason.to_string()));
        let mut o = BTreeMap::new();
        o.insert("error".to_string(), CValue::Obj(e));
        o.insert("ok".to_string(), CValue::Bool(false));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (path, reason) = self.parts();
        write!(f, "{} {path}: {reason}", self.code())
    }
}

impl std::error::Error for GuardError {}

/// `counsel_review = { date, reviewer_ref }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounselReview {
    pub date: String,
    pub reviewer_ref: String,
}

/// One `[[revisit]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revisit {
    pub release: String,
    pub date: Option<String>,
    pub note: String,
}

/// The policy file `brand/copy-policy.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub mode: String,
    /// `None` when the entry is absent or incomplete (no date or no reviewer reference).
    pub counsel_review: Option<CounselReview>,
    pub revisits: Vec<Revisit>,
}

/// One review record, reduced to what the guard needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub release: String,
    /// `answers.copied_text.result` when the record answers `copied_text`; an answer without a
    /// string `result` reads as `""` (not a pass). `None` when there is no answer at all (that is
    /// the review gate's `missing_answer`).
    pub copied_text: Option<String>,
}

/// Paths given to [`run`].
#[derive(Debug, Clone)]
pub struct Inputs<'a> {
    pub policy: &'a Path,
    pub checklist: &'a Path,
    pub records: &'a Path,
    pub root: &'a Path,
}

/// The guard's result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// Sorted by (code, subject), without duplicates.
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    /// Canonical JSON (RFC 8785 via `fid-canon`).
    pub fn to_json(&self) -> Vec<u8> {
        let items = self
            .findings
            .iter()
            .map(|f| {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(f.code.as_str().to_string()));
                o.insert("subject".to_string(), CValue::Str(f.subject.clone()));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("findings".to_string(), CValue::Arr(items));
        o.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }
}

// ------------------------------------------------------------------ reading

fn unreadable(path: &Path, reason: impl Into<String>) -> GuardError {
    GuardError::Unreadable { path: path.display().to_string(), reason: reason.into() }
}

/// Reads at most `limit` bytes of `path`; more is an error.
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, GuardError> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(limit as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| unreadable(path, e.to_string()))?;
    if bytes.len() > limit {
        return Err(unreadable(path, format!("larger than {limit} bytes")));
    }
    Ok(bytes)
}

fn read_toml(path: &Path, parse_err: fn(String, String) -> GuardError) -> Result<toml::Table, GuardError> {
    let shown = path.display().to_string();
    let bytes = read_bounded(path, MAX_FILE_BYTES)?;
    let text = String::from_utf8(bytes).map_err(|_| parse_err(shown.clone(), "not UTF-8".into()))?;
    text.parse::<toml::Table>().map_err(|e| parse_err(shown, e.message().to_string()))
}

fn policy_err(path: String, reason: String) -> GuardError {
    GuardError::PolicyParse { path, reason }
}

fn input_err(path: String, reason: String) -> GuardError {
    GuardError::InputParse { path, reason }
}

fn non_empty_str<'t>(t: &'t toml::Table, key: &str) -> Option<&'t str> {
    t.get(key).and_then(toml::Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

/// Parses the policy file (REQ-OUTR.BRAND.COPY-001). `mode` and `decision = "D-013"` are
/// required; `counsel_review` counts only with both a date and a reviewer reference; each
/// `[[revisit]]` needs a `release` and a `note`.
pub fn parse_policy(table: &toml::Table, path: &str) -> Result<Policy, GuardError> {
    let bad = |reason: &str| policy_err(path.to_string(), reason.to_string());
    let mode = non_empty_str(table, "mode").ok_or_else(|| bad("no string `mode`"))?.to_string();
    match non_empty_str(table, "decision") {
        Some(DECISION) => {}
        _ => return Err(bad("`decision` must be \"D-013\"")),
    }
    let counsel_review = match table.get("counsel_review") {
        None => None,
        Some(v) => {
            let t = v.as_table().ok_or_else(|| bad("`counsel_review` is not a table"))?;
            match (non_empty_str(t, "date"), non_empty_str(t, "reviewer_ref")) {
                (Some(d), Some(r)) => Some(CounselReview { date: d.to_string(), reviewer_ref: r.to_string() }),
                _ => None,
            }
        }
    };
    let mut revisits = Vec::new();
    if let Some(v) = table.get("revisit") {
        let arr = v.as_array().ok_or_else(|| bad("`revisit` is not an array of tables"))?;
        for entry in arr {
            let t = entry.as_table().ok_or_else(|| bad("`revisit` entry is not a table"))?;
            let release = non_empty_str(t, "release").ok_or_else(|| bad("`[[revisit]]` without `release`"))?;
            let note = non_empty_str(t, "note").ok_or_else(|| bad("`[[revisit]]` without `note`"))?;
            revisits.push(Revisit {
                release: release.to_string(),
                date: non_empty_str(t, "date").map(str::to_string),
                note: note.to_string(),
            });
        }
    }
    Ok(Policy { mode, counsel_review, revisits })
}

/// The item IDs of a trade-dress checklist (`items = [{ id, question }]`).
pub fn parse_checklist(table: &toml::Table, path: &str) -> Result<BTreeSet<String>, GuardError> {
    let bad = |reason: &str| input_err(path.to_string(), reason.to_string());
    let items = table.get("items").and_then(toml::Value::as_array).ok_or_else(|| bad("no `items` array"))?;
    let mut ids = BTreeSet::new();
    for item in items {
        let id = item.as_table().and_then(|t| non_empty_str(t, "id")).ok_or_else(|| bad("item without `id`"))?;
        ids.insert(id.to_string());
    }
    Ok(ids)
}

/// A review record (`release`, `answers.<item> = { result, note }`).
pub fn parse_record(table: &toml::Table, path: &str) -> Result<Record, GuardError> {
    let bad = |reason: &str| input_err(path.to_string(), reason.to_string());
    let release = non_empty_str(table, "release").ok_or_else(|| bad("no `release`"))?.to_string();
    let copied_text =
        table.get("answers").and_then(toml::Value::as_table).and_then(|a| a.get(COPIED_TEXT)).map(|v| {
            v.as_table().and_then(|t| t.get("result")).and_then(toml::Value::as_str).unwrap_or("").to_string()
        });
    Ok(Record { release, copied_text })
}

/// Reads every `*.toml` record in `dir`, in file-name order.
pub fn read_records(dir: &Path) -> Result<Vec<Record>, GuardError> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| unreadable(dir, e.to_string()))? {
        let p = entry.map_err(|e| unreadable(dir, e.to_string()))?.path();
        if p.extension().and_then(|e| e.to_str()) == Some("toml") && p.is_file() {
            paths.push(p);
            if paths.len() > MAX_RECORDS {
                return Err(unreadable(dir, format!("more than {MAX_RECORDS} records")));
            }
        }
    }
    paths.sort();
    paths.iter().map(|p| parse_record(&read_toml(p, input_err)?, &p.display().to_string())).collect()
}

// ------------------------------------------------------------------ reference-set scan

/// True when a file name has a `refset` part after its first dot (`x.refset`, `x.refset.json`).
pub fn is_refset_name(name: &str) -> bool {
    name.split('.').skip(1).any(|part| part.eq_ignore_ascii_case("refset"))
}

fn ext_lower(name: &str) -> Option<String> {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// True when TOML or JSON `bytes` have a top-level `kind = "incumbent_text"`.
///
/// A file is parsed when it contains `incumbent_text` literally or any `\u` / `\U` escape (JSON
/// and TOML escapes can spell the key or the value). Content with neither cannot declare the kind
/// and is not parsed. A parse failure fails closed (error naming the file) when the file contains
/// `incumbent_text` literally, or `kind` together with an escape; otherwise the unparsable file
/// cannot be a declaration and is ignored.
pub fn declares_incumbent_text(bytes: &[u8], ext: &str, path: &Path) -> Result<bool, GuardError> {
    let literal = contains(bytes, INCUMBENT_KIND.as_bytes());
    let escaped = contains(bytes, b"\\u") || contains(bytes, b"\\U");
    if !literal && !escaped {
        return Ok(false);
    }
    let suspicious = literal || contains(bytes, b"kind");
    let shown = path.display().to_string();
    let fail = |reason: String| {
        if suspicious {
            Err(input_err(shown.clone(), format!("may declare {INCUMBENT_KIND} but {reason}")))
        } else {
            Ok(false)
        }
    };
    if ext == "json" {
        match fid_canon::parse_with_limits(bytes, MAX_SCAN_BYTES, fid_canon::MAX_DEPTH) {
            Ok(CValue::Obj(o)) => Ok(matches!(o.get("kind"), Some(CValue::Str(k)) if k == INCUMBENT_KIND)),
            Ok(_) => Ok(false),
            Err(e) => fail(format!("does not parse as JSON ({})", e.code())),
        }
    } else {
        let Ok(text) = std::str::from_utf8(bytes) else { return fail("is not UTF-8".into()) };
        match text.parse::<toml::Table>() {
            Ok(t) => Ok(t.get("kind").and_then(toml::Value::as_str) == Some(INCUMBENT_KIND)),
            Err(e) => fail(format!("does not parse as TOML ({})", e.message())),
        }
    }
}

/// Root-relative paths (with `/`) of files that declare themselves reference sets of
/// third-party text (REQ-OUTR.BRAND.COPY-004). Sorted walk; symbolic links are not followed, and
/// a symbolic link named `*.toml` or `*.json` (whose target cannot be checked) is an error rather
/// than a silent skip; [`SKIPPED_DIRS`] are not entered at any depth and [`SKIPPED_ROOT_DIRS`]
/// not directly under the root.
pub fn scan_reference_sets(root: &Path) -> Result<Vec<String>, GuardError> {
    let mut found = Vec::new();
    let mut visited = 0usize;
    // (directory, its root-relative prefix, depth)
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((dir, prefix, depth)) = stack.pop() {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| unreadable(&dir, e.to_string()))? {
            let entry = entry.map_err(|e| unreadable(&dir, e.to_string()))?;
            visited += 1;
            if visited > MAX_WALK_ENTRIES {
                return Err(unreadable(root, format!("more than {MAX_WALK_ENTRIES} entries")));
            }
            let ft = entry.file_type().map_err(|e| unreadable(&entry.path(), e.to_string()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            entries.push((name, entry.path(), ft));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut subdirs = Vec::new();
        for (name, path, ft) in entries {
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            if ft.is_dir() {
                let skipped =
                    SKIPPED_DIRS.contains(&name.as_str()) || (depth == 0 && SKIPPED_ROOT_DIRS.contains(&name.as_str()));
                if skipped {
                    continue;
                }
                if depth >= MAX_WALK_DEPTH {
                    return Err(unreadable(&path, format!("nested deeper than {MAX_WALK_DEPTH}")));
                }
                subdirs.push((path, rel, depth + 1));
                continue;
            }
            if is_refset_name(&name) {
                found.push(rel);
                continue;
            }
            let Some(ext) = ext_lower(&name).filter(|e| e == "toml" || e == "json") else { continue };
            if ft.is_symlink() {
                return Err(unreadable(&path, "symbolic link to a TOML or JSON file is not followed"));
            }
            if !ft.is_file() {
                continue;
            }
            let bytes = read_bounded(&path, MAX_SCAN_BYTES)?;
            if declares_incumbent_text(&bytes, &ext, &path)? {
                found.push(rel);
            }
        }
        // Pop order: visit subdirectories in name order.
        stack.extend(subdirs.into_iter().rev());
    }
    found.sort();
    Ok(found)
}

// ------------------------------------------------------------------ rules

/// Applies the rules to parsed inputs. `refsets` is consulted only in `review_only` mode.
pub fn check_rules(policy: &Policy, checklist: &BTreeSet<String>, records: &[Record], refsets: &[String]) -> Report {
    let mut findings = BTreeSet::new();
    if policy.mode != REVIEW_ONLY && policy.counsel_review.is_none() {
        findings.insert(Finding { code: Code::CounselReviewMissing, subject: String::new() });
    }
    if !checklist.contains(COPIED_TEXT) {
        findings.insert(Finding { code: Code::CopiedTextItemMissing, subject: String::new() });
    }
    let revisited: BTreeSet<&str> = policy.revisits.iter().map(|r| r.release.as_str()).collect();
    for r in records {
        // Any recorded result other than exactly "pass" counts as a fail (fail closed).
        let failed = r.copied_text.as_deref().is_some_and(|res| res != "pass");
        if failed && !revisited.contains(r.release.as_str()) {
            findings.insert(Finding { code: Code::D013RevisitDue, subject: r.release.clone() });
        }
    }
    if policy.mode == REVIEW_ONLY {
        for p in refsets {
            findings.insert(Finding { code: Code::ReferenceSetPresent, subject: p.clone() });
        }
    }
    Report { findings: findings.into_iter().collect() }
}

/// Reads the inputs and runs the guard. The policy is read first, so a missing policy is
/// reported before anything else.
pub fn run(inputs: &Inputs<'_>) -> Result<Report, GuardError> {
    let policy = parse_policy(&read_toml(inputs.policy, policy_err)?, &inputs.policy.display().to_string())?;
    let checklist = parse_checklist(&read_toml(inputs.checklist, input_err)?, &inputs.checklist.display().to_string())?;
    let records = read_records(inputs.records)?;
    let refsets = if policy.mode == REVIEW_ONLY { scan_reference_sets(inputs.root)? } else { Vec::new() };
    Ok(check_rules(&policy, &checklist, &records, &refsets))
}
