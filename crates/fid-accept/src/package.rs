//! Acceptance-job package `acceptance-job@1`, its validator and the suite index
//! (spec node FLOW.RUNNER.PKG).
//!
//! A package is a folder holding `package.json` (the manifest), the golden journal and any
//! files its quality checks name (truth tables, expected outputs). The manifest says which
//! job and variant it is, which app and milestones it gates, whether it is gating or tracked
//! and unassisted or assisted, the journal by digest, the datasets by catalogue ID and
//! digest, the quality checks, the time targets and an optional forced-kill point
//! (REQ-FLOW.RUNNER.PKG-001). The schema is closed: an unknown field is `PKG-SCHEMA`, so no
//! field can override `gating`, `milestones` or `time_targets` (REQ-006), and the package
//! digest is the SHA-256 of the RFC 8785 form of the whole manifest, so it covers them.
//!
//! The validator treats a package as hostile input. Parsing is bounded (manifest size and
//! nesting, list lengths, journal size, directory listings); duplicate JSON keys, duplicate
//! IDs and file references that differ only by letter case are refused; every file
//! reference is a plain relative path (no `..`, no absolute or drive paths, no backslashes,
//! no reserved Windows device names) that must name, with exactly that spelling, a regular
//! file inside the package that is reached without following a symbolic link or junction.
//! Journals are digested by streaming and never parsed: entries are counted as the
//! LF-terminated lines of the file (JSON Lines), and a blank line is refused.
//!
//! Output is a pure function of the package folder and the catalogue: findings are sorted by
//! code, then subject, then detail, and their texts never include operating-system error
//! messages (determinism class D0).

use fid_canon::CValue;
use fid_types::{CommandId, Digest};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

/// Format tag of a package manifest.
pub const FORMAT: &str = "acceptance-job@1";
/// Format tag of the suite index `acceptance/suite.json`.
pub const SUITE_FORMAT: &str = "acceptance-suite@1";
/// File name of the manifest inside a package folder.
pub const MANIFEST: &str = "package.json";
/// Largest accepted manifest or suite index, in bytes (Design: 1 MiB).
pub const MAX_MANIFEST_BYTES: usize = 1 << 20;
/// Deepest accepted JSON nesting; the format needs 6 (object, checks, check, params,
/// steps, range).
pub const MAX_DEPTH: usize = 8;
/// Largest accepted golden journal, in bytes (streamed, never held in memory).
pub const MAX_JOURNAL_BYTES: u64 = 1 << 30;
/// Largest accepted dataset catalogue, in bytes (the ECO.DATA.CAT bound).
pub const MAX_CATALOGUE_BYTES: usize = 1 << 20;
/// Most entries read from one directory listing while resolving a file reference.
pub const MAX_DIR_ENTRIES: usize = 4096;
/// Most items in `datasets`, `checks`, a check's list parameter or step ranges.
pub const MAX_LIST: usize = 256;
/// Most milestones in one package.
pub const MAX_MILESTONES: usize = 32;
/// Most packages in the suite index.
pub const MAX_SUITE_PACKAGES: usize = 1024;
/// Longest free-text parameter, in bytes.
pub const MAX_TEXT_BYTES: usize = 256;
/// Longest file reference, in bytes.
pub const MAX_PATH_BYTES: usize = 255;
/// Most components (`/`-separated) in a file reference.
pub const MAX_PATH_COMPONENTS: usize = 8;
/// Longest component of a file reference, in bytes.
pub const MAX_COMPONENT_BYTES: usize = 100;
/// Largest time target, in seconds (one week; targets are minutes to hours).
pub const MAX_SECONDS: f64 = 604_800.0;

/// Required manifest fields (REQ-FLOW.RUNNER.PKG-001), in the order the spec lists them.
pub const REQUIRED_FIELDS: [&str; 12] = [
    "format",
    "job_id",
    "variant_id",
    "app",
    "milestones",
    "gating",
    "assistance",
    "input_mode",
    "journal",
    "datasets",
    "checks",
    "time_targets",
];
/// Optional manifest fields.
pub const OPTIONAL_FIELDS: [&str; 2] = ["kill_after", "journey_map"];

/// The closed set of quality-check kinds (REQ-FLOW.RUNNER.PKG-003), in the spec's order.
pub const CHECK_KINDS: [&str; 22] = [
    "replay_outcome",
    "all_characteristics_resolved",
    "value_within_u",
    "decision_equals",
    "result_count",
    "operator_decision_equals",
    "ors_case",
    "thickness_extrema",
    "lexical_identical",
    "basis_present",
    "lint_findings_equal",
    "no_manual_entry",
    "requirements_equal",
    "sheet_cells_equal",
    "index_equals",
    "verify_verdict",
    "records_equal",
    "file_digest",
    "no_edit_reachable",
    "no_pointer_events",
    "basis_label",
    "committed_survive_kill",
];

/// The check kind that a `kill_after` package must declare (REQ-005).
pub const KILL_CHECK: &str = "committed_survive_kill";

// ------------------------------------------------------------------ check parameters

/// Type of one check parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    /// A file in the package with its digest, `{file, sha256}`, like `journal`: the file must
    /// exist (same path rules as `journal.file`) and its SHA-256 must match.
    Path,
    /// The name of an output artefact of the run (an export file name, not a package file):
    /// the same path syntax as a file reference, not looked up.
    Artefact,
    /// Free text: 1 to [`MAX_TEXT_BYTES`] bytes, no control characters.
    Text,
    /// A variant ID of another package (lowercase letters, digits, inner hyphens).
    Variant,
    /// A `sha256:` digest.
    Digest,
    /// A whole number from 0 to 2^53 - 1.
    Count,
    /// A non-empty list of distinct free-text items.
    TextList,
    /// A list (possibly empty) of distinct command IDs `name@major`.
    CommandList,
    /// A non-empty list of journal entry ranges `{from, to}`, 1 <= from <= to <= entry count.
    Ranges,
}

/// Schema of one check parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamSpec {
    pub name: &'static str,
    pub ty: ParamType,
    pub required: bool,
}

const fn req(name: &'static str, ty: ParamType) -> ParamSpec {
    ParamSpec { name, ty, required: true }
}

const fn opt(name: &'static str, ty: ParamType) -> ParamSpec {
    ParamSpec { name, ty, required: false }
}

const NO_PARAMS: &[ParamSpec] = &[];
const TRUTH: &[ParamSpec] = &[req("truth", ParamType::Path)];
const EXPECTED: &[ParamSpec] = &[req("expected", ParamType::Path)];
const VALUE_WITHIN_U: &[ParamSpec] = &[req("truth", ParamType::Path), opt("items", ParamType::TextList)];
const RESULT_COUNT: &[ParamSpec] = &[req("count", ParamType::Count)];
const ORS_CASE: &[ParamSpec] = &[req("cases", ParamType::TextList)];
const LEXICAL: &[ParamSpec] = &[req("artefact", ParamType::Artefact), req("expected", ParamType::Path)];
const ARTEFACT: &[ParamSpec] = &[req("artefact", ParamType::Artefact)];
const VERIFY: &[ParamSpec] = &[req("expected", ParamType::Text)];
const RECORDS: &[ParamSpec] = &[req("against", ParamType::Variant)];
const FILE_DIGEST: &[ParamSpec] = &[req("artefact", ParamType::Artefact), opt("sha256", ParamType::Digest)];
const NO_EDIT: &[ParamSpec] = &[req("allowed_commands", ParamType::CommandList), opt("steps", ParamType::Ranges)];
const NO_POINTER: &[ParamSpec] = &[req("steps", ParamType::Ranges)];

/// The parameter schema of a check kind, or `None` for a kind outside the closed set.
/// Parameter objects are closed: a name not listed here is `PKG-CHECK`.
pub fn param_specs(kind: &str) -> Option<&'static [ParamSpec]> {
    Some(match kind {
        "replay_outcome"
        | "all_characteristics_resolved"
        | "basis_present"
        | "no_manual_entry"
        | "basis_label"
        | "committed_survive_kill" => NO_PARAMS,
        "value_within_u" => VALUE_WITHIN_U,
        "decision_equals" | "operator_decision_equals" | "thickness_extrema" => TRUTH,
        "result_count" => RESULT_COUNT,
        "ors_case" => ORS_CASE,
        "lexical_identical" => LEXICAL,
        "lint_findings_equal" | "requirements_equal" | "index_equals" => EXPECTED,
        "sheet_cells_equal" => ARTEFACT,
        "verify_verdict" => VERIFY,
        "records_equal" => RECORDS,
        "file_digest" => FILE_DIGEST,
        "no_edit_reachable" => NO_EDIT,
        "no_pointer_events" => NO_POINTER,
        _ => return None,
    })
}

// ------------------------------------------------------------------ findings

/// Finding codes (spec Design: Errors). Declared in alphabetical order of their text so the
/// derived order sorts findings by code text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Code {
    /// A check names a kind outside the closed set or has parameters invalid for its kind
    /// (REQ-003), or a check's file or step range does not exist.
    Check,
    /// A dataset ID is not in the catalogue, or its digest differs from the catalogue's.
    Dataset,
    /// The journal cannot be found or digested, or its digest differs; a check file's digest
    /// differs from its declared `sha256`; or a suite entry's digest differs from the package
    /// digest.
    Digest,
    /// `kill_after` without a `committed_survive_kill` check, or not inside the journal (REQ-005).
    Kill,
    /// A field is missing or malformed (REQ-002), or the manifest cannot be read.
    Schema,
    /// A time target names an unknown metric, a non-positive or oversized value, a metric
    /// twice, or a gating package lacks `task_s` (REQ-004).
    Target,
}

impl Code {
    /// Stable text of the code.
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Check => "PKG-CHECK",
            Code::Dataset => "PKG-DATASET",
            Code::Digest => "PKG-DIGEST",
            Code::Kill => "PKG-KILL",
            Code::Schema => "PKG-SCHEMA",
            Code::Target => "PKG-TARGET",
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One finding: `<CODE> <subject>: <detail>`. The subject is the field path, such as
/// `gating`, `journal.sha256` or `checks[1].params.truth`, `package.json` for the manifest
/// as a whole, or `-` for the package folder.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: Code,
    pub subject: String,
    pub detail: String,
}

impl Finding {
    fn new(code: Code, subject: impl Into<String>, detail: impl Into<String>) -> Finding {
        Finding { code, subject: subject.into(), detail: detail.into() }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code, self.subject, self.detail)
    }
}

// ------------------------------------------------------------------ data structures

/// Whether a package gates a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gating {
    Gating,
    Tracked,
}

/// Whether an agent principal may act in the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assistance {
    Unassisted,
    Assisted,
}

/// Input devices the run may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Any,
    KeyboardOnly,
}

/// What a time target measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Metric {
    MachineS,
    TaskS,
    OperatorS,
}

impl Metric {
    /// Wire name of the metric.
    pub fn as_str(self) -> &'static str {
        match self {
            Metric::MachineS => "machine_s",
            Metric::TaskS => "task_s",
            Metric::OperatorS => "operator_s",
        }
    }
}

/// The golden journal: a file in the package and its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalRef {
    pub file: String,
    pub sha256: Digest,
}

/// A dataset by catalogue ID and digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetRef {
    pub id: String,
    pub sha256: Digest,
}

/// One quality check (spec Design: `CheckSpec`).
#[derive(Debug, Clone, PartialEq)]
pub struct CheckSpec {
    pub id: String,
    pub kind: String,
    pub params: BTreeMap<String, CValue>,
}

/// One time target (spec Design: `TimeTarget`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeTarget {
    pub metric: Metric,
    pub seconds: f64,
    pub verified: bool,
}

/// A validated package manifest (spec Design: `JobPackage`).
#[derive(Debug, Clone, PartialEq)]
pub struct JobPackage {
    pub job_id: String,
    pub variant_id: String,
    pub app: String,
    pub milestones: Vec<String>,
    pub gating: Gating,
    pub assistance: Assistance,
    pub input_mode: InputMode,
    pub journal: JournalRef,
    pub datasets: Vec<DatasetRef>,
    pub checks: Vec<CheckSpec>,
    pub time_targets: Vec<TimeTarget>,
    /// 1-based index of the journal entry after which the run is killed.
    pub kill_after: Option<u64>,
    /// `map_id` of the job's journey map.
    pub journey_map: Option<String>,
}

/// Result of validating one package.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// The package, present only when there are no findings.
    pub package: Option<JobPackage>,
    /// The package digest, present whenever the manifest is readable JSON within the bounds.
    pub digest: Option<Digest>,
    /// Number of journal entries, when the journal was found and well formed.
    pub journal_entries: Option<u64>,
    /// Sorted findings; empty means the package is accepted.
    pub findings: Vec<Finding>,
}

impl Report {
    fn failed(finding: Finding) -> Report {
        Report { package: None, digest: None, journal_entries: None, findings: vec![finding] }
    }

    /// True when there are no findings.
    pub fn is_ok(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Bounds applied while validating a folder. [`Limits::default`] gives the published bounds;
/// tests lower them to exercise the refusals cheaply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub manifest_bytes: usize,
    /// Largest journal or check file, each streamed through SHA-256.
    pub journal_bytes: u64,
    pub dir_entries: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { manifest_bytes: MAX_MANIFEST_BYTES, journal_bytes: MAX_JOURNAL_BYTES, dir_entries: MAX_DIR_ENTRIES }
    }
}

// ------------------------------------------------------------------ catalogue

/// Dataset IDs and digests from the dataset catalogue (ECO.DATA.CAT, `datasets.toml`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalogue {
    entries: BTreeMap<String, Digest>,
}

impl Catalogue {
    /// An empty catalogue.
    pub fn new() -> Catalogue {
        Catalogue::default()
    }

    /// Adds or replaces an entry.
    pub fn insert(&mut self, id: &str, sha256: Digest) {
        self.entries.insert(id.to_string(), sha256);
    }

    /// The digest the catalogue records for `id`.
    pub fn get(&self, id: &str) -> Option<&Digest> {
        self.entries.get(id)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the catalogue has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Reads `[[dataset]] id, sha256` from catalogue TOML text. The other fields and the
    /// admission rules are the business of `datasets-check` (ECO.DATA.CAT); this only needs
    /// each ID once with a 64-hex digest.
    pub fn parse_toml(text: &str) -> Result<Catalogue, String> {
        if text.len() > MAX_CATALOGUE_BYTES {
            return Err(format!("catalogue exceeds {MAX_CATALOGUE_BYTES} bytes"));
        }
        let table: toml::Table =
            text.parse().map_err(|e: toml::de::Error| format!("catalogue is not TOML: {}", e.message()))?;
        let mut out = Catalogue::new();
        match table.get("dataset") {
            None => {}
            Some(toml::Value::Array(rows)) => {
                for (i, row) in rows.iter().enumerate() {
                    let id = match row.get("id") {
                        Some(toml::Value::String(s)) => s,
                        _ => return Err(format!("catalogue entry dataset[{i}] has no string `id`")),
                    };
                    let digest = match row.get("sha256") {
                        Some(toml::Value::String(hex)) => Digest::parse(&format!("sha256:{hex}")).ok(),
                        _ => None,
                    };
                    let Some(digest) = digest else {
                        return Err(format!("catalogue entry {id} has no 64-hex `sha256`"));
                    };
                    if out.entries.insert(id.clone(), digest).is_some() {
                        return Err(format!("catalogue lists {id} twice"));
                    }
                }
            }
            Some(_) => return Err("catalogue `dataset` is not an array of tables".to_string()),
        }
        Ok(out)
    }

    /// Reads and parses a catalogue file of at most [`MAX_CATALOGUE_BYTES`].
    pub fn read(path: &Path) -> Result<Catalogue, String> {
        let bytes = read_bounded(path, MAX_CATALOGUE_BYTES).map_err(|e| format!("catalogue {e}"))?;
        let text = String::from_utf8(bytes).map_err(|_| "catalogue is not UTF-8".to_string())?;
        Catalogue::parse_toml(&text)
    }
}

// ------------------------------------------------------------------ syntax rules

/// Whether `s` is a lowercase identifier: 1 to 64 bytes of `a-z`, `0-9` and inner hyphens.
/// Used for `variant_id`, `app`, milestones, check IDs and `journey_map`.
pub fn is_ident(s: &str) -> bool {
    let b = s.as_bytes();
    let edge = |c: Option<&u8>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    b.len() <= 64
        && edge(b.first())
        && edge(b.last())
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// Whether `s` is a `JOB.*` seed ID: `JOB.<UPPER>.<two digits>`, such as `JOB.ACC.01`.
pub fn is_job_id(s: &str) -> bool {
    let mut parts = s.split('.');
    let (Some("JOB"), Some(group), Some(n), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    (1..=8).contains(&group.len())
        && group.bytes().all(|c| c.is_ascii_uppercase())
        && n.len() == 2
        && n.bytes().all(|c| c.is_ascii_digit())
}

/// Whether `s` is a catalogue dataset ID (`^[a-z0-9][a-z0-9._-]{2,63}$`, ECO.DATA.CAT REQ-001).
pub fn is_dataset_id(s: &str) -> bool {
    let b = s.as_bytes();
    (3..=64).contains(&b.len())
        && b.first().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
}

fn is_reserved_device(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
    let numbered = (stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.len() == 4
        && stem.as_bytes().get(3).is_some_and(|c| (b'1'..=b'9').contains(c));
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered
}

/// Checks a file reference: a relative path of `/`-separated components, each 1 to
/// [`MAX_COMPONENT_BYTES`] bytes of ASCII letters, digits, `.`, `_` and `-`, not `.` or `..`,
/// not ending in `.`, and not a reserved Windows device name; at most [`MAX_PATH_BYTES`]
/// bytes and [`MAX_PATH_COMPONENTS`] components. Returns why it is refused.
pub fn check_path(p: &str) -> Result<(), &'static str> {
    if p.is_empty() {
        return Err("must not be empty");
    }
    if p.len() > MAX_PATH_BYTES {
        return Err("must be at most 255 bytes");
    }
    let parts: Vec<&str> = p.split('/').collect();
    if parts.len() > MAX_PATH_COMPONENTS {
        return Err("must have at most 8 path components");
    }
    for part in parts {
        if part.is_empty() {
            return Err("must be a relative path without empty components");
        }
        if part == "." || part == ".." {
            return Err("must not contain `.` or `..` components");
        }
        if part.len() > MAX_COMPONENT_BYTES {
            return Err("has a path component longer than 100 bytes");
        }
        if !part.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
            return Err("may use only ASCII letters, digits, `.`, `_`, `-` and `/` separators");
        }
        if part.ends_with('.') {
            return Err("has a path component ending in `.`");
        }
        if is_reserved_device(part) {
            return Err("names a reserved Windows device");
        }
    }
    Ok(())
}

fn as_count(v: &CValue) -> Option<u64> {
    match v {
        CValue::Num(f) if f.is_finite() && *f >= 0.0 && f.fract() == 0.0 && *f <= 9_007_199_254_740_991.0 => {
            Some(*f as u64)
        }
        _ => None,
    }
}

fn is_text(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_TEXT_BYTES && !s.chars().any(char::is_control)
}

/// A field name as shown in a subject: printable ASCII, at most 64 characters, others `?`.
fn shown(s: &str) -> String {
    s.chars().take(64).map(|c| if c.is_ascii_graphic() { c } else { '?' }).collect()
}

// ------------------------------------------------------------------ manifest checks

type Obj = BTreeMap<String, CValue>;

struct FileRef {
    subject: String,
    path: String,
    sha256: Digest,
}

#[derive(Default)]
struct Draft {
    findings: Vec<Finding>,
    job_id: Option<String>,
    variant_id: Option<String>,
    app: Option<String>,
    milestones: Option<Vec<String>>,
    gating: Option<Gating>,
    assistance: Option<Assistance>,
    input_mode: Option<InputMode>,
    journal_file: Option<String>,
    journal_sha: Option<Digest>,
    datasets: Option<Vec<DatasetRef>>,
    checks: Option<Vec<CheckSpec>>,
    time_targets: Option<Vec<TimeTarget>>,
    kill_after: Option<u64>,
    journey_map: Option<String>,
    has_kill_check: bool,
    /// Check parameter files still to be located.
    files: Vec<FileRef>,
    /// Highest entry named by each step-range parameter.
    steps: Vec<(String, u64)>,
}

impl Draft {
    fn push(&mut self, code: Code, subject: impl Into<String>, detail: impl Into<String>) {
        self.findings.push(Finding::new(code, subject, detail));
    }

    fn missing(&mut self, subject: impl Into<String>) {
        self.push(Code::Schema, subject, "missing");
    }

    /// Reports keys of `o` outside `allowed` as `<prefix>.<key>`. Returns whether all were allowed.
    fn closed(&mut self, o: &Obj, allowed: &[&str], prefix: &str, code: Code) -> bool {
        let mut ok = true;
        for key in o.keys() {
            if !allowed.contains(&key.as_str()) {
                self.push(code, format!("{prefix}.{}", shown(key)), "unknown field");
                ok = false;
            }
        }
        ok
    }

    fn text_field(&mut self, m: &Obj, key: &str, ok: fn(&str) -> bool, what: &str) -> Option<String> {
        match m.get(key) {
            None => {
                self.missing(key);
                None
            }
            Some(CValue::Str(s)) if ok(s) => Some(s.clone()),
            Some(_) => {
                self.push(Code::Schema, key, format!("must be {what}"));
                None
            }
        }
    }

    fn enum_field<T: Copy>(&mut self, m: &Obj, key: &str, options: &[(&str, T)]) -> Option<T> {
        let found = match m.get(key) {
            None => {
                self.missing(key);
                return None;
            }
            Some(CValue::Str(s)) => options.iter().find(|(n, _)| *n == s.as_str()).map(|(_, t)| *t),
            Some(_) => None,
        };
        if found.is_none() {
            let names: Vec<String> = options.iter().map(|(n, _)| format!("`{n}`")).collect();
            self.push(Code::Schema, key, format!("must be one of {}", names.join(", ")));
        }
        found
    }

    fn digest_field(&mut self, o: &Obj, key: &str, subject: &str) -> Option<Digest> {
        match o.get(key) {
            None => {
                self.missing(subject);
                None
            }
            Some(CValue::Str(s)) => match Digest::parse(s) {
                Ok(d) => Some(d),
                Err(_) => {
                    self.push(Code::Schema, subject, "must be `sha256:` and 64 lowercase hex digits");
                    None
                }
            },
            Some(_) => {
                self.push(Code::Schema, subject, "must be `sha256:` and 64 lowercase hex digits");
                None
            }
        }
    }

    fn list<'a>(&mut self, m: &'a Obj, key: &str, max: usize) -> Option<&'a [CValue]> {
        match m.get(key) {
            None => {
                self.missing(key);
                None
            }
            Some(CValue::Arr(a)) if a.len() <= max => Some(a.as_slice()),
            Some(CValue::Arr(_)) => {
                self.push(Code::Schema, key, format!("must have at most {max} items"));
                None
            }
            Some(_) => {
                self.push(Code::Schema, key, "must be a list");
                None
            }
        }
    }

    fn milestones(&mut self, m: &Obj) {
        let Some(arr) = self.list(m, "milestones", MAX_MILESTONES) else { return };
        let mut out = Vec::new();
        let mut ok = true;
        for (i, v) in arr.iter().enumerate() {
            match v {
                CValue::Str(s) if is_ident(s) => {
                    if out.contains(s) {
                        self.push(Code::Schema, format!("milestones[{i}]"), "duplicate milestone");
                        ok = false;
                    } else {
                        out.push(s.clone());
                    }
                }
                _ => {
                    self.push(Code::Schema, format!("milestones[{i}]"), "must be a milestone ID such as `launch-1`");
                    ok = false;
                }
            }
        }
        if ok {
            self.milestones = Some(out);
        }
    }

    fn journal(&mut self, m: &Obj) {
        let o = match m.get("journal") {
            None => return self.missing("journal"),
            Some(CValue::Obj(o)) => o,
            Some(_) => return self.push(Code::Schema, "journal", "must be an object {file, sha256}"),
        };
        self.closed(o, &["file", "sha256"], "journal", Code::Schema);
        match o.get("file") {
            None => self.missing("journal.file"),
            Some(CValue::Str(s)) => match check_path(s) {
                Ok(()) => self.journal_file = Some(s.clone()),
                Err(why) => self.push(Code::Schema, "journal.file", why),
            },
            Some(_) => self.push(Code::Schema, "journal.file", "must be a relative file path"),
        }
        self.journal_sha = self.digest_field(o, "sha256", "journal.sha256");
    }

    fn datasets(&mut self, m: &Obj, catalogue: &Catalogue) {
        let Some(arr) = self.list(m, "datasets", MAX_LIST) else { return };
        let mut out: Vec<DatasetRef> = Vec::new();
        let mut ok = true;
        for (i, v) in arr.iter().enumerate() {
            let at = format!("datasets[{i}]");
            let CValue::Obj(o) = v else {
                self.push(Code::Schema, at, "must be an object {id, sha256}");
                ok = false;
                continue;
            };
            ok &= self.closed(o, &["id", "sha256"], &at, Code::Schema);
            let id = match o.get("id") {
                Some(CValue::Str(s)) if is_dataset_id(s) => Some(s.clone()),
                None => {
                    self.missing(format!("{at}.id"));
                    None
                }
                Some(_) => {
                    self.push(Code::Schema, format!("{at}.id"), "must be a catalogue dataset ID");
                    None
                }
            };
            let sha = self.digest_field(o, "sha256", &format!("{at}.sha256"));
            let (Some(id), Some(sha)) = (id, sha) else {
                ok = false;
                continue;
            };
            if out.iter().any(|d| d.id == id) {
                self.push(Code::Schema, format!("{at}.id"), "duplicate dataset ID");
                ok = false;
                continue;
            }
            match catalogue.get(&id) {
                None => {
                    self.push(Code::Dataset, format!("{at}.id"), "not in the dataset catalogue");
                    ok = false;
                }
                Some(want) if *want != sha => {
                    self.push(
                        Code::Dataset,
                        format!("{at}.sha256"),
                        format!("differs from the catalogue digest {want}"),
                    );
                    ok = false;
                }
                Some(_) => {}
            }
            out.push(DatasetRef { id, sha256: sha });
        }
        if ok {
            self.datasets = Some(out);
        }
    }

    fn checks(&mut self, m: &Obj) {
        let Some(arr) = self.list(m, "checks", MAX_LIST) else { return };
        if arr.is_empty() {
            return self.push(Code::Schema, "checks", "must hold at least one check");
        }
        let mut out: Vec<CheckSpec> = Vec::new();
        let mut ids: BTreeSet<String> = BTreeSet::new();
        let mut ok = true;
        for (i, v) in arr.iter().enumerate() {
            let at = format!("checks[{i}]");
            let CValue::Obj(o) = v else {
                self.push(Code::Schema, at, "must be an object {id, kind, params}");
                ok = false;
                continue;
            };
            ok &= self.closed(o, &["id", "kind", "params"], &at, Code::Schema);
            let id = match o.get("id") {
                None => {
                    self.missing(format!("{at}.id"));
                    None
                }
                Some(CValue::Str(s)) if is_ident(s) => {
                    if ids.insert(s.clone()) {
                        Some(s.clone())
                    } else {
                        self.push(Code::Schema, format!("{at}.id"), "duplicate check ID");
                        None
                    }
                }
                Some(_) => {
                    self.push(
                        Code::Schema,
                        format!("{at}.id"),
                        "must be a check ID (lowercase letters, digits, hyphens)",
                    );
                    None
                }
            };
            let kind = match o.get("kind") {
                None => {
                    self.missing(format!("{at}.kind"));
                    None
                }
                Some(CValue::Str(s)) if CHECK_KINDS.contains(&s.as_str()) => Some(s.clone()),
                Some(_) => {
                    self.push(Code::Check, format!("{at}.kind"), "not a check kind of acceptance-job@1");
                    None
                }
            };
            if kind.as_deref() == Some(KILL_CHECK) {
                self.has_kill_check = true;
            }
            let params = match o.get("params") {
                None => {
                    self.missing(format!("{at}.params"));
                    None
                }
                Some(CValue::Obj(p)) => Some(p),
                Some(_) => {
                    self.push(Code::Check, format!("{at}.params"), "must be an object");
                    None
                }
            };
            if let (Some(kind), Some(params)) = (&kind, params) {
                ok &= self.params(kind, params, &at);
            }
            match (id, kind, params) {
                (Some(id), Some(kind), Some(params)) => out.push(CheckSpec { id, kind, params: params.clone() }),
                _ => ok = false,
            }
        }
        if ok {
            self.checks = Some(out);
        }
    }

    fn params(&mut self, kind: &str, params: &Obj, at: &str) -> bool {
        let specs = param_specs(kind).unwrap_or(NO_PARAMS);
        let mut ok = true;
        for key in params.keys() {
            if !specs.iter().any(|s| s.name == key.as_str()) {
                self.push(Code::Check, format!("{at}.params.{}", shown(key)), format!("not a parameter of {kind}"));
                ok = false;
            }
        }
        for spec in specs {
            let sub = format!("{at}.params.{}", spec.name);
            match params.get(spec.name) {
                None if spec.required => {
                    self.push(Code::Check, sub, "missing");
                    ok = false;
                }
                None => {}
                Some(v) => {
                    if let Err(why) = self.param(spec.ty, v, &sub) {
                        self.push(Code::Check, sub, why);
                        ok = false;
                    }
                }
            }
        }
        ok
    }

    fn param(&mut self, ty: ParamType, v: &CValue, sub: &str) -> Result<(), String> {
        match (ty, v) {
            (ParamType::Path, CValue::Obj(o)) if o.len() == 2 => {
                let file = match o.get("file") {
                    Some(CValue::Str(s)) => s,
                    _ => return Err("must be an object {file, sha256} naming a file in the package".into()),
                };
                check_path(file).map_err(|why| format!("file {why}"))?;
                let sha256 = match o.get("sha256") {
                    Some(CValue::Str(s)) => Digest::parse(s).ok(),
                    _ => None,
                };
                let Some(sha256) = sha256 else {
                    return Err("sha256 must be `sha256:` and 64 lowercase hex digits".into());
                };
                self.files.push(FileRef { subject: sub.to_string(), path: file.clone(), sha256 });
                Ok(())
            }
            (ParamType::Path, _) => Err("must be an object {file, sha256} naming a file in the package".into()),
            (ParamType::Artefact, CValue::Str(s)) => check_path(s).map_err(|why| format!("artefact name {why}")),
            (ParamType::Artefact, _) => Err("must be an artefact file name".into()),
            (ParamType::Text, CValue::Str(s)) if is_text(s) => Ok(()),
            (ParamType::Text, _) => Err(format!("must be text of 1 to {MAX_TEXT_BYTES} bytes")),
            (ParamType::Variant, CValue::Str(s)) if is_ident(s) => Ok(()),
            (ParamType::Variant, _) => Err("must be a variant ID".into()),
            (ParamType::Digest, CValue::Str(s)) if Digest::is_valid(s) => Ok(()),
            (ParamType::Digest, _) => Err("must be `sha256:` and 64 lowercase hex digits".into()),
            (ParamType::Count, v) if as_count(v).is_some() => Ok(()),
            (ParamType::Count, _) => Err("must be a whole number".into()),
            (ParamType::TextList, CValue::Arr(a)) if !a.is_empty() && a.len() <= MAX_LIST => {
                let mut seen = BTreeSet::new();
                for (i, x) in a.iter().enumerate() {
                    match x {
                        CValue::Str(s) if is_text(s) => {
                            if !seen.insert(s.as_str()) {
                                return Err(format!("item {i} is a duplicate"));
                            }
                        }
                        _ => return Err(format!("item {i} must be text of 1 to {MAX_TEXT_BYTES} bytes")),
                    }
                }
                Ok(())
            }
            (ParamType::TextList, _) => Err(format!("must be a list of 1 to {MAX_LIST} text items")),
            (ParamType::CommandList, CValue::Arr(a)) if a.len() <= MAX_LIST => {
                let mut seen = BTreeSet::new();
                for (i, x) in a.iter().enumerate() {
                    match x {
                        CValue::Str(s) if CommandId::is_valid(s) => {
                            if !seen.insert(s.as_str()) {
                                return Err(format!("item {i} is a duplicate"));
                            }
                        }
                        _ => return Err(format!("item {i} must be a command ID `name@major`")),
                    }
                }
                Ok(())
            }
            (ParamType::CommandList, _) => Err(format!("must be a list of at most {MAX_LIST} command IDs")),
            (ParamType::Ranges, CValue::Arr(a)) if !a.is_empty() && a.len() <= MAX_LIST => {
                let mut top = 0;
                for (i, r) in a.iter().enumerate() {
                    let bounds = match r {
                        CValue::Obj(o) if o.len() == 2 => {
                            (o.get("from").and_then(as_count), o.get("to").and_then(as_count))
                        }
                        _ => (None, None),
                    };
                    match bounds {
                        (Some(from), Some(to)) if from >= 1 && to >= from => top = top.max(to),
                        _ => return Err(format!("range {i} must be {{from, to}} with whole numbers 1 <= from <= to")),
                    }
                }
                self.steps.push((sub.to_string(), top));
                Ok(())
            }
            (ParamType::Ranges, _) => Err(format!("must be a list of 1 to {MAX_LIST} entry ranges {{from, to}}")),
        }
    }

    fn time_targets(&mut self, m: &Obj) {
        let Some(arr) = self.list(m, "time_targets", MAX_LIST) else { return };
        let mut out: Vec<TimeTarget> = Vec::new();
        let mut metrics: BTreeSet<Metric> = BTreeSet::new();
        let mut ok = true;
        for (i, v) in arr.iter().enumerate() {
            let at = format!("time_targets[{i}]");
            let CValue::Obj(o) = v else {
                self.push(Code::Schema, at, "must be an object {metric, seconds, verified}");
                ok = false;
                continue;
            };
            ok &= self.closed(o, &["metric", "seconds", "verified"], &at, Code::Schema);
            let metric = match o.get("metric") {
                None => {
                    self.missing(format!("{at}.metric"));
                    None
                }
                Some(CValue::Str(s)) => {
                    let metric = match s.as_str() {
                        "machine_s" => Some(Metric::MachineS),
                        "task_s" => Some(Metric::TaskS),
                        "operator_s" => Some(Metric::OperatorS),
                        _ => None,
                    };
                    match metric {
                        None => self.push(
                            Code::Target,
                            format!("{at}.metric"),
                            "must be `machine_s`, `task_s` or `operator_s`",
                        ),
                        Some(mt) if !metrics.insert(mt) => {
                            self.push(Code::Target, format!("{at}.metric"), "duplicate metric");
                        }
                        Some(_) => {}
                    }
                    metric
                }
                Some(_) => {
                    self.push(Code::Schema, format!("{at}.metric"), "must be a string");
                    None
                }
            };
            let seconds = match o.get("seconds") {
                None => {
                    self.missing(format!("{at}.seconds"));
                    None
                }
                Some(CValue::Num(s)) if *s > 0.0 && *s <= MAX_SECONDS => Some(*s),
                Some(CValue::Num(_)) => {
                    self.push(Code::Target, format!("{at}.seconds"), "must be above 0 and at most 604800");
                    None
                }
                Some(_) => {
                    self.push(Code::Schema, format!("{at}.seconds"), "must be a number");
                    None
                }
            };
            let verified = match o.get("verified") {
                None => {
                    self.missing(format!("{at}.verified"));
                    None
                }
                Some(CValue::Bool(b)) => Some(*b),
                Some(_) => {
                    self.push(Code::Schema, format!("{at}.verified"), "must be true or false");
                    None
                }
            };
            match (metric, seconds, verified) {
                (Some(metric), Some(seconds), Some(verified)) => out.push(TimeTarget { metric, seconds, verified }),
                _ => ok = false,
            }
        }
        if self.gating == Some(Gating::Gating) && !metrics.contains(&Metric::TaskS) {
            self.push(Code::Target, "time_targets", "a gating package needs a `task_s` target");
            ok = false;
        }
        if ok {
            self.time_targets = Some(out);
        }
    }

    fn kill_after(&mut self, m: &Obj) {
        match m.get("kill_after") {
            None => {
                if self.has_kill_check {
                    self.push(Code::Kill, "kill_after", "a `committed_survive_kill` check needs `kill_after`");
                }
            }
            Some(v) => match as_count(v) {
                Some(0) => self.push(Code::Kill, "kill_after", "must name a journal entry (entries count from 1)"),
                Some(k) => {
                    self.kill_after = Some(k);
                    if !self.has_kill_check {
                        self.push(Code::Kill, "kill_after", "needs a `committed_survive_kill` check");
                    }
                }
                None => self.push(Code::Schema, "kill_after", "must be a whole number"),
            },
        }
    }

    /// File references that differ only by letter case, in the whole path or in any folder
    /// prefix (`a/j.jsonl` and `A/t.json`), would name one folder or file on Windows and macOS
    /// and two on Linux; they and references to the manifest are refused.
    fn collisions(&mut self) {
        let mut all: Vec<(String, String)> = Vec::new();
        if let Some(f) = &self.journal_file {
            all.push(("journal.file".to_string(), f.clone()));
        }
        all.extend(self.files.iter().map(|f| (f.subject.clone(), f.path.clone())));
        // Folded prefix -> the first exact spelling seen.
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        let mut bad: BTreeSet<String> = BTreeSet::new();
        for (subject, path) in all {
            if path.to_ascii_lowercase() == MANIFEST {
                self.push(Code::Schema, subject.clone(), "must not name the manifest");
                bad.insert(subject);
                continue;
            }
            let mut prefixes: Vec<String> = Vec::new();
            for part in path.split('/') {
                let next = match prefixes.last() {
                    Some(p) => format!("{p}/{part}"),
                    None => part.to_string(),
                };
                prefixes.push(next);
            }
            let clash = prefixes.iter().find_map(|p| match seen.get(&p.to_ascii_lowercase()) {
                Some(prev) if prev != p => Some((p.clone(), prev.clone())),
                _ => None,
            });
            if let Some((mine, prev)) = clash {
                self.push(Code::Schema, subject.clone(), format!("{mine:?} differs from {prev:?} only by letter case"));
                bad.insert(subject);
                continue;
            }
            for p in prefixes {
                seen.entry(p.to_ascii_lowercase()).or_insert(p);
            }
        }
        if bad.contains("journal.file") {
            self.journal_file = None;
        }
        self.files.retain(|f| !bad.contains(&f.subject));
    }

    fn build(&self) -> Option<JobPackage> {
        Some(JobPackage {
            job_id: self.job_id.clone()?,
            variant_id: self.variant_id.clone()?,
            app: self.app.clone()?,
            milestones: self.milestones.clone()?,
            gating: self.gating?,
            assistance: self.assistance?,
            input_mode: self.input_mode?,
            journal: JournalRef { file: self.journal_file.clone()?, sha256: self.journal_sha? },
            datasets: self.datasets.clone()?,
            checks: self.checks.clone()?,
            time_targets: self.time_targets.clone()?,
            kill_after: self.kill_after,
            journey_map: self.journey_map.clone(),
        })
    }
}

fn check_value(value: &CValue, catalogue: &Catalogue) -> Draft {
    let mut d = Draft::default();
    let CValue::Obj(m) = value else {
        d.push(Code::Schema, MANIFEST, "must be a JSON object");
        return d;
    };
    for key in m.keys() {
        if !REQUIRED_FIELDS.contains(&key.as_str()) && !OPTIONAL_FIELDS.contains(&key.as_str()) {
            d.push(Code::Schema, shown(key), "unknown field");
        }
    }
    match m.get("format") {
        None => d.missing("format"),
        Some(CValue::Str(s)) if s == FORMAT => {}
        Some(_) => d.push(Code::Schema, "format", format!("must be `{FORMAT}`")),
    }
    d.job_id = d.text_field(m, "job_id", is_job_id, "a `JOB.*` seed ID such as `JOB.ACC.01`");
    d.variant_id = d.text_field(m, "variant_id", is_ident, "a variant ID (lowercase letters, digits, hyphens)");
    d.app = d.text_field(m, "app", is_ident, "an app ID (lowercase letters, digits, hyphens)");
    d.milestones(m);
    d.gating = d.enum_field(m, "gating", &[("gating", Gating::Gating), ("tracked", Gating::Tracked)]);
    d.assistance =
        d.enum_field(m, "assistance", &[("unassisted", Assistance::Unassisted), ("assisted", Assistance::Assisted)]);
    d.input_mode =
        d.enum_field(m, "input_mode", &[("any", InputMode::Any), ("keyboard_only", InputMode::KeyboardOnly)]);
    d.journal(m);
    d.datasets(m, catalogue);
    d.checks(m);
    d.time_targets(m);
    d.kill_after(m);
    if m.contains_key("journey_map") {
        d.journey_map = d.text_field(m, "journey_map", is_ident, "a journey-map `map_id`");
    }
    d.collisions();
    d
}

fn parse_manifest(input: &[u8], max_bytes: usize) -> Result<(CValue, Digest), Finding> {
    let limit = max_bytes.min(MAX_MANIFEST_BYTES);
    if input.len() > limit {
        return Err(Finding::new(Code::Schema, MANIFEST, format!("exceeds {limit} bytes")));
    }
    let value = fid_canon::parse_with_limits(input, limit, MAX_DEPTH)
        .map_err(|e| Finding::new(Code::Schema, MANIFEST, format!("not valid JSON ({e})")))?;
    let digest = fid_canon::digest(&value)
        .map_err(|e| Finding::new(Code::Schema, MANIFEST, format!("has no canonical form ({e})")))?;
    Ok((value, Digest::from_bytes(digest.0)))
}

/// The package digest: SHA-256 of the RFC 8785 form of the manifest (REQ-006).
pub fn package_digest(manifest: &[u8]) -> Result<Digest, Finding> {
    parse_manifest(manifest, MAX_MANIFEST_BYTES).map(|(_, d)| d)
}

fn finish(d: Draft, digest: Option<Digest>, journal_entries: Option<u64>) -> Report {
    let mut findings = d.findings.clone();
    let package = if findings.is_empty() { d.build() } else { None };
    if findings.is_empty() && package.is_none() {
        findings.push(Finding::new(Code::Schema, MANIFEST, "incomplete package"));
    }
    findings.sort();
    findings.dedup();
    Report { package, digest, journal_entries, findings }
}

/// Checks manifest bytes without touching the file system: every rule except the journal
/// digest and entry count, `kill_after` and step ranges against the journal, and the
/// existence of referenced files. Use [`validate_dir`] to validate a package folder.
pub fn check_manifest(input: &[u8], catalogue: &Catalogue) -> Report {
    match parse_manifest(input, MAX_MANIFEST_BYTES) {
        Ok((value, digest)) => finish(check_value(&value, catalogue), Some(digest), None),
        Err(f) => Report::failed(f),
    }
}

// ------------------------------------------------------------------ file system

/// Whether `dir` lists `name` with exactly that spelling. The whole listing is read (up to
/// `max_entries`) before answering, so a crowded folder fails the same way whatever order
/// the operating system lists it in.
fn has_entry(dir: &Path, name: &str, max_entries: usize) -> Result<bool, String> {
    let listing = fs::read_dir(dir).map_err(|_| "cannot list its folder".to_string())?;
    let mut found = false;
    for (n, entry) in listing.enumerate() {
        if n >= max_entries {
            return Err(format!("its folder has more than {max_entries} entries"));
        }
        let entry = entry.map_err(|_| "cannot list its folder".to_string())?;
        if entry.file_name().as_os_str() == OsStr::new(name) {
            found = true;
        }
    }
    Ok(found)
}

/// Resolves `rel` (already accepted by [`check_path`]) under `base`, component by component:
/// each must be listed under exactly that spelling, no component may be a symbolic link or
/// junction, the inner ones must be folders and the last a regular file (or a folder when
/// `want_dir`).
fn locate(base: &Path, rel: &str, want_dir: bool, max_entries: usize) -> Result<PathBuf, String> {
    let mut cur = base.to_path_buf();
    let parts: Vec<&str> = rel.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if !has_entry(&cur, part, max_entries)? {
            return Err("not found in the package (names must match letter case)".to_string());
        }
        cur.push(part);
        let md = fs::symlink_metadata(&cur).map_err(|_| "cannot be read".to_string())?;
        let last = i + 1 == parts.len();
        if md.file_type().is_symlink() {
            return Err("is or passes through a symbolic link".to_string());
        }
        if !last && !md.is_dir() {
            return Err("not found in the package (names must match letter case)".to_string());
        }
        if last && want_dir && !md.is_dir() {
            return Err("is not a folder".to_string());
        }
        if last && !want_dir && !md.is_file() {
            return Err("is not a regular file".to_string());
        }
    }
    Ok(cur)
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|_| "cannot be read".to_string())?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).map_err(|_| "cannot be read".to_string())?;
    if bytes.len() > limit {
        return Err(format!("exceeds {limit} bytes"));
    }
    Ok(bytes)
}

/// Streams a referenced file of at most `limit` bytes through SHA-256.
fn digest_file(path: &Path, limit: u64) -> Result<Digest, String> {
    let file = fs::File::open(path).map_err(|_| "cannot be read".to_string())?;
    let mut reader = file.take(limit.saturating_add(1));
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err("cannot be read".to_string()),
        };
        total += n as u64;
        if total > limit {
            return Err(format!("exceeds {limit} bytes"));
        }
        hasher.update(buf.get(..n).unwrap_or(&[]));
    }
    Ok(Digest::from_bytes(hasher.finalize().into()))
}

/// What streaming a journal found: its digest, entry count and any structural problem.
struct JournalScan {
    digest: Digest,
    entries: u64,
    problem: Option<&'static str>,
}

fn scan_journal(path: &Path, limit: u64) -> Result<JournalScan, String> {
    let file = fs::File::open(path).map_err(|_| "cannot be read".to_string())?;
    let mut reader = file.take(limit.saturating_add(1));
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let (mut total, mut entries, mut line_len) = (0u64, 0u64, 0u64);
    let mut blank = false;
    let mut last: Option<u8> = None;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err("cannot be read".to_string()),
        };
        total += n as u64;
        if total > limit {
            return Err(format!("exceeds {limit} bytes"));
        }
        let chunk = buf.get(..n).unwrap_or(&[]);
        hasher.update(chunk);
        for &b in chunk {
            if b == b'\n' {
                if line_len == 0 {
                    blank = true;
                } else {
                    entries += 1;
                }
                line_len = 0;
            } else {
                line_len += 1;
            }
        }
        last = chunk.last().copied();
    }
    let problem = if total == 0 {
        Some("holds no entries")
    } else if blank {
        Some("has a blank line (one entry per line)")
    } else if last != Some(b'\n') {
        Some("does not end with a line feed")
    } else {
        None
    };
    Ok(JournalScan { digest: Digest::from_bytes(hasher.finalize().into()), entries, problem })
}

/// Validates the package folder `dir` against the catalogue with the published bounds.
pub fn validate_dir(dir: &Path, catalogue: &Catalogue) -> Report {
    validate_dir_with(dir, catalogue, &Limits::default())
}

/// [`validate_dir`] with caller-chosen bounds (each clamped to the published bound).
pub fn validate_dir_with(dir: &Path, catalogue: &Catalogue, limits: &Limits) -> Report {
    let max_entries = limits.dir_entries.min(MAX_DIR_ENTRIES);
    match fs::symlink_metadata(dir) {
        Ok(md) if md.is_dir() => {}
        Ok(md) if md.file_type().is_symlink() => {
            return Report::failed(Finding::new(Code::Schema, "-", "package folder is a symbolic link"))
        }
        _ => return Report::failed(Finding::new(Code::Schema, "-", "package folder not found")),
    }
    let bytes = match locate(dir, MANIFEST, false, max_entries)
        .and_then(|p| read_bounded(&p, limits.manifest_bytes.min(MAX_MANIFEST_BYTES)))
    {
        Ok(b) => b,
        Err(why) => return Report::failed(Finding::new(Code::Schema, MANIFEST, why)),
    };
    let (value, digest) = match parse_manifest(&bytes, limits.manifest_bytes) {
        Ok(v) => v,
        Err(f) => return Report::failed(f),
    };
    let mut d = check_value(&value, catalogue);
    let mut entries = None;
    if let Some(file) = d.journal_file.clone() {
        match locate(dir, &file, false, max_entries)
            .and_then(|p| scan_journal(&p, limits.journal_bytes.min(MAX_JOURNAL_BYTES)))
        {
            Err(why) => d.push(Code::Digest, "journal.file", why),
            Ok(scan) => {
                if let Some(want) = d.journal_sha {
                    if scan.digest != want {
                        d.push(Code::Digest, "journal.sha256", format!("the file's digest is {}", scan.digest));
                    }
                }
                match scan.problem {
                    Some(why) => d.push(Code::Schema, "journal.file", why),
                    None => entries = Some(scan.entries),
                }
            }
        }
    }
    if let (Some(k), Some(n)) = (d.kill_after, entries) {
        if k > n {
            d.push(Code::Kill, "kill_after", format!("names entry {k}; the journal holds {n}"));
        }
    }
    if let Some(n) = entries {
        let over: Vec<(String, u64)> = d.steps.iter().filter(|(_, top)| *top > n).cloned().collect();
        for (subject, top) in over {
            d.push(Code::Check, subject, format!("reaches entry {top}; the journal holds {n}"));
        }
    }
    let refs: Vec<(String, String, Digest)> =
        d.files.iter().map(|f| (f.subject.clone(), f.path.clone(), f.sha256)).collect();
    for (subject, path, want) in refs {
        match locate(dir, &path, false, max_entries)
            .and_then(|p| digest_file(&p, limits.journal_bytes.min(MAX_JOURNAL_BYTES)))
        {
            Err(why) => d.push(Code::Check, subject, why),
            Ok(got) if got != want => d.push(Code::Digest, subject, format!("the file's digest is {got}")),
            Ok(_) => {}
        }
    }
    finish(d, Some(digest), entries)
}

// ------------------------------------------------------------------ suite index

/// One registered package: its folder (relative to the repository root) and digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteEntry {
    pub path: String,
    pub digest: Digest,
}

/// The suite index `acceptance/suite.json` (spec Design: `SuiteIndex`), in run order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SuiteIndex {
    pub packages: Vec<SuiteEntry>,
}

/// Result of validating a suite index and its packages.
#[derive(Debug, Clone, PartialEq)]
pub struct SuiteReport {
    /// The index, present when it has no findings of its own.
    pub index: Option<SuiteIndex>,
    /// Findings on the index itself (subjects `suite`, `packages[i].path`,
    /// `packages[i].digest`), sorted.
    pub findings: Vec<Finding>,
    /// Each registered package's own report, in index order.
    pub packages: Vec<(String, Report)>,
}

impl SuiteReport {
    /// True when neither the index nor any package has findings.
    pub fn is_ok(&self) -> bool {
        self.findings.is_empty() && self.packages.iter().all(|(_, r)| r.is_ok())
    }
}

/// Parses and checks a suite index: `{format: "acceptance-suite@1", packages: [{path,
/// digest}]}`. Paths follow the file-reference rules; a path listed twice, two paths that
/// differ only by letter case, and a package folder inside another are refused.
pub fn parse_suite(input: &[u8]) -> (Option<SuiteIndex>, Vec<Finding>) {
    let mut f: Vec<Finding> = Vec::new();
    let value = match fid_canon::parse_with_limits(input, MAX_MANIFEST_BYTES, MAX_DEPTH) {
        Ok(v) => v,
        Err(e) => return (None, vec![Finding::new(Code::Schema, "suite", format!("not valid JSON ({e})"))]),
    };
    let CValue::Obj(m) = &value else {
        return (None, vec![Finding::new(Code::Schema, "suite", "must be a JSON object")]);
    };
    for key in m.keys() {
        if key != "format" && key != "packages" {
            f.push(Finding::new(Code::Schema, format!("suite.{}", shown(key)), "unknown field"));
        }
    }
    if !matches!(m.get("format"), Some(CValue::Str(s)) if s == SUITE_FORMAT) {
        f.push(Finding::new(Code::Schema, "suite.format", format!("must be `{SUITE_FORMAT}`")));
    }
    let arr = match m.get("packages") {
        Some(CValue::Arr(a)) if a.len() <= MAX_SUITE_PACKAGES => a,
        _ => {
            f.push(Finding::new(
                Code::Schema,
                "suite.packages",
                format!("must be a list of at most {MAX_SUITE_PACKAGES} entries"),
            ));
            f.sort();
            return (None, f);
        }
    };
    let mut out: Vec<SuiteEntry> = Vec::new();
    let mut folded: Vec<(usize, String)> = Vec::new();
    for (i, v) in arr.iter().enumerate() {
        let at = format!("packages[{i}]");
        let CValue::Obj(o) = v else {
            f.push(Finding::new(Code::Schema, at, "must be an object {path, digest}"));
            continue;
        };
        for key in o.keys() {
            if key != "path" && key != "digest" {
                f.push(Finding::new(Code::Schema, format!("{at}.{}", shown(key)), "unknown field"));
            }
        }
        let path = match o.get("path") {
            Some(CValue::Str(s)) => match check_path(s) {
                Ok(()) => Some(s.clone()),
                Err(why) => {
                    f.push(Finding::new(Code::Schema, format!("{at}.path"), why));
                    None
                }
            },
            _ => {
                f.push(Finding::new(Code::Schema, format!("{at}.path"), "must be a relative folder path"));
                None
            }
        };
        let digest = match o.get("digest") {
            Some(CValue::Str(s)) => Digest::parse(s).ok(),
            _ => None,
        };
        if digest.is_none() {
            f.push(Finding::new(Code::Schema, format!("{at}.digest"), "must be `sha256:` and 64 lowercase hex digits"));
        }
        let Some(path) = path else { continue };
        let fold = path.to_ascii_lowercase();
        let mut clash = None;
        for (j, other) in &folded {
            let prev = out.get(*j).map(|e| e.path.as_str()).unwrap_or_default();
            if *other == fold {
                clash = Some(if prev == path {
                    "is listed twice".to_string()
                } else {
                    format!("differs from {prev:?} only by letter case")
                });
            } else if fold.starts_with(&format!("{other}/")) || other.starts_with(&format!("{fold}/")) {
                clash = Some(format!("is nested with package {prev:?}"));
            }
            if clash.is_some() {
                break;
            }
        }
        if let Some(why) = clash {
            f.push(Finding::new(Code::Schema, format!("{at}.path"), why));
            continue;
        }
        if let Some(digest) = digest {
            folded.push((out.len(), fold));
            out.push(SuiteEntry { path, digest });
        }
    }
    f.sort();
    if f.is_empty() {
        (Some(SuiteIndex { packages: out }), f)
    } else {
        (None, f)
    }
}

/// Validates a suite index and every package it lists. Package folders are resolved under
/// `root` (the repository root) with the same exact-case, no-link rules as file references;
/// each package is validated and its digest compared with the index (`PKG-DIGEST`).
pub fn validate_suite(root: &Path, index: &[u8], catalogue: &Catalogue) -> SuiteReport {
    let (parsed, mut findings) = parse_suite(index);
    let mut packages = Vec::new();
    if let Some(ix) = &parsed {
        for (i, entry) in ix.packages.iter().enumerate() {
            match locate(root, &entry.path, true, MAX_DIR_ENTRIES) {
                Err(why) => findings.push(Finding::new(Code::Schema, format!("packages[{i}].path"), why)),
                Ok(dir) => {
                    let report = validate_dir(&dir, catalogue);
                    match report.digest {
                        Some(d) if d == entry.digest => {}
                        Some(d) => findings.push(Finding::new(
                            Code::Digest,
                            format!("packages[{i}].digest"),
                            format!("the package digest is {d}"),
                        )),
                        None => findings.push(Finding::new(
                            Code::Digest,
                            format!("packages[{i}].digest"),
                            "the package has no digest",
                        )),
                    }
                    packages.push((entry.path.clone(), report));
                }
            }
        }
    }
    findings.sort();
    SuiteReport { index: if findings.is_empty() { parsed } else { None }, findings, packages }
}
