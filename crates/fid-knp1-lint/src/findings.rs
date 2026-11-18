//! Findings model, report and waivers of the KNP-1 lint. Spec node `KRN.NUM.LINT.FIND`.
//!
//! Every check of the lint emits [`Finding`]s. [`apply_waivers`] checks every waiver of
//! `knp1-waivers.toml` against the current source (a waiver whose recorded line digest no
//! longer matches is fatal, `KNP1-WAIVER-STALE`), moves matching findings to the waived list
//! and warns about unused waivers (`KNP1-WAIVER-UNUSED`). [`Report`] then writes
//! `knp1-findings.json`: RFC 8785 canonical JSON, schema 1, sorted, with no clocks or host
//! paths, so equal inputs give byte-identical reports. JSON Schema: `schema/knp1-findings-v1.json`.
//!
//! Line digests: the waived file is read as bytes, every CRLF becomes LF, and the digest is
//! SHA-256 over lines `start..=end` (1-based), each followed by one LF, written
//! `sha256:` + 64 lowercase hex digits.

use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Report schema version written as `schema`.
pub const REPORT_SCHEMA: u32 = 1;
/// Tool name written as `tool`.
pub const TOOL_NAME: &str = "fid-knp1-lint";
/// Largest accepted waiver file, in bytes.
pub const MAX_WAIVER_FILE_BYTES: usize = 1024 * 1024;
/// Most waivers accepted in one file.
pub const MAX_WAIVERS: usize = 10_000;
/// Largest source file read for a line digest, in bytes.
pub const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;

const MAX_NAME: usize = 128;
const MAX_PATH: usize = 4096;
const MAX_TEXT: usize = 4096;
const MAX_RULE_SUFFIX: usize = 48;

// ------------------------------------------------------------------ rules

/// Fixed rule IDs and their one-line descriptions, in catalogue order.
const FIXED: [(&str, &str); 12] = [
    ("KNP1-FMA", "fused multiply-add: `mul_add` or an FMA intrinsic"),
    ("KNP1-POW", "`powi`/`powf` and other std float powers outside the fid-num facade"),
    ("KNP1-LIBM", "std or libm transcendental function outside the fid-num facade"),
    ("KNP1-MINMAX", "`f64::min`/`f64::max` (not total-order)"),
    ("KNP1-HASH", "std `HashMap` or `HashSet` (iteration order not fixed)"),
    ("KNP1-SIMD", "runtime CPU-feature dispatch such as `is_x86_feature_detected!`"),
    ("KNP1-UNSAFE", "`unsafe` code"),
    ("KNP1-IO-FS", "file-system access through `std::fs`"),
    ("KNP1-IO-NET", "network access through `std::net`"),
    ("KNP1-CLOCK", "clock access through `std::time`"),
    ("KNP1-ENV", "environment access through `std::env`"),
    ("KNP1-THREAD", "threads (`std::thread` and similar)"),
];

/// Rule families with a free suffix, their prefixes and descriptions.
const FAMILIES: [(&str, &str); 3] = [
    ("KNP1-FLAG-", "kernel build configuration (target CPU, FMA feature, codegen units, toolchain pin)"),
    ("KNP1-ALLOW-", "dependency allowlist or SPDX licence check"),
    ("KNP1-DATA-", "reference-dataset denylist match"),
];

/// A KNP-1 rule with a stable string ID (`KNP1-…`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Rule {
    Fma,
    Pow,
    Libm,
    MinMax,
    Hash,
    Simd,
    Unsafe,
    IoFs,
    IoNet,
    Clock,
    Env,
    Thread,
    /// `KNP1-FLAG-<suffix>`.
    Flag(String),
    /// `KNP1-ALLOW-<suffix>`.
    Allow(String),
    /// `KNP1-DATA-<suffix>`.
    Data(String),
}

impl Rule {
    const FIXED_RULES: [Rule; 12] = [
        Rule::Fma,
        Rule::Pow,
        Rule::Libm,
        Rule::MinMax,
        Rule::Hash,
        Rule::Simd,
        Rule::Unsafe,
        Rule::IoFs,
        Rule::IoNet,
        Rule::Clock,
        Rule::Env,
        Rule::Thread,
    ];

    fn fixed_index(&self) -> Option<usize> {
        Rule::FIXED_RULES.iter().position(|r| r == self)
    }

    /// The stable rule ID, e.g. `KNP1-POW` or `KNP1-FLAG-TARGET-CPU`.
    pub fn id(&self) -> String {
        match self {
            Rule::Flag(s) => format!("{}{s}", FAMILIES[0].0),
            Rule::Allow(s) => format!("{}{s}", FAMILIES[1].0),
            Rule::Data(s) => format!("{}{s}", FAMILIES[2].0),
            fixed => {
                fixed.fixed_index().and_then(|i| FIXED.get(i)).map(|(id, _)| (*id).to_string()).unwrap_or_default()
            }
        }
    }

    /// The one-line description from the catalogue.
    pub fn description(&self) -> &'static str {
        match self {
            Rule::Flag(_) => FAMILIES[0].1,
            Rule::Allow(_) => FAMILIES[1].1,
            Rule::Data(_) => FAMILIES[2].1,
            fixed => fixed.fixed_index().and_then(|i| FIXED.get(i)).map(|(_, d)| *d).unwrap_or_default(),
        }
    }

    /// Parses a rule ID. Family suffixes are `A-Z0-9` segments joined by single `-`, at most 48 bytes.
    pub fn parse(id: &str) -> Option<Rule> {
        if let Some(i) = FIXED.iter().position(|(f, _)| *f == id) {
            return Rule::FIXED_RULES.get(i).cloned();
        }
        for (k, (prefix, _)) in FAMILIES.iter().enumerate() {
            if let Some(suffix) = id.strip_prefix(prefix) {
                if !valid_suffix(suffix) {
                    return None;
                }
                let s = suffix.to_string();
                return Some(match k {
                    0 => Rule::Flag(s),
                    1 => Rule::Allow(s),
                    _ => Rule::Data(s),
                });
            }
        }
        None
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id())
    }
}

fn valid_suffix(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_RULE_SUFFIX
        && s.split('-').all(|seg| !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()))
}

/// The rule catalogue: `(id or family pattern, description)`, fixed rules first, then
/// the families `KNP1-FLAG-*`, `KNP1-ALLOW-*`, `KNP1-DATA-*`.
pub fn catalogue() -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = FIXED.iter().map(|(id, d)| ((*id).to_string(), *d)).collect();
    out.extend(FAMILIES.iter().map(|(p, d)| (format!("{p}*"), *d)));
    out
}

// ------------------------------------------------------------------ errors

/// Stable error codes of this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// A waiver is missing a field or has a malformed one (fatal).
    WaiverSchema,
    /// A waiver's recorded line digest no longer matches the source (fatal).
    WaiverStale,
    /// A finding or input digest breaks the report schema (fatal; a bug in a check).
    InvalidFinding,
    /// The report could not be serialised or written.
    ReportWriteFailed,
}

impl ErrorCode {
    /// The stable code string.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::WaiverSchema => "KNP1-WAIVER-SCHEMA",
            ErrorCode::WaiverStale => "KNP1-WAIVER-STALE",
            ErrorCode::InvalidFinding => "invalid_finding",
            ErrorCode::ReportWriteFailed => "report_write_failed",
        }
    }
}

/// A fatal error with its stable code and a message naming the waiver or finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindError {
    pub code: ErrorCode,
    pub message: String,
}

impl FindError {
    fn new(code: ErrorCode, message: impl Into<String>) -> FindError {
        FindError { code, message: message.into() }
    }
}

impl fmt::Display for FindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for FindError {}

// ------------------------------------------------------------------ findings

/// One finding emitted by a check (REQ-KRN.NUM.LINT.FIND-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule: Rule,
    pub crate_name: String,
    pub crate_version: String,
    /// Path relative to the crate root, `/` separated.
    pub file: String,
    /// 1-based line.
    pub line: u32,
    /// 1-based column.
    pub column: u32,
    /// `sha256:` digest of the offending excerpt.
    pub excerpt_digest: String,
    pub message: String,
}

type SortKey = (String, String, String, u32, u32, String, String, String);

impl Finding {
    /// Checks the fields against the report schema.
    pub fn validate(&self) -> Result<(), FindError> {
        let bad = |what: &str| Err(FindError::new(ErrorCode::InvalidFinding, format!("{what} in {}", self.label())));
        if !valid_name(&self.crate_name) {
            return bad("bad crate name");
        }
        if !valid_name(&self.crate_version) {
            return bad("bad crate version");
        }
        if !valid_relpath(&self.file) {
            return bad("file path is not relative to the crate root");
        }
        if self.line == 0 || self.column == 0 {
            return bad("line and column are 1-based");
        }
        if !valid_digest(&self.excerpt_digest) {
            return bad("bad excerpt digest");
        }
        if self.message.len() > MAX_TEXT {
            return bad("message too long");
        }
        Ok(())
    }

    fn label(&self) -> String {
        format!("{} {} {}:{}:{} {}", self.crate_name, self.crate_version, self.file, self.line, self.column, self.rule)
    }

    /// Report order: (crate, version, file, line, column, rule); message and excerpt break exact ties.
    fn sort_key(&self) -> SortKey {
        (
            self.crate_name.clone(),
            self.crate_version.clone(),
            self.file.clone(),
            self.line,
            self.column,
            self.rule.id(),
            self.message.clone(),
            self.excerpt_digest.clone(),
        )
    }

    fn to_cvalue(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("rule".to_string(), CValue::Str(self.rule.id()));
        o.insert("crate_name".to_string(), CValue::Str(self.crate_name.clone()));
        o.insert("crate_version".to_string(), CValue::Str(self.crate_version.clone()));
        o.insert("file".to_string(), CValue::Str(self.file.clone()));
        o.insert("line".to_string(), CValue::Num(f64::from(self.line)));
        o.insert("column".to_string(), CValue::Num(f64::from(self.column)));
        o.insert("excerpt_digest".to_string(), CValue::Str(self.excerpt_digest.clone()));
        o.insert("message".to_string(), CValue::Str(self.message.clone()));
        CValue::Obj(o)
    }
}

/// Sorts findings into report order (REQ-KRN.NUM.LINT.FIND-001).
pub fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by_cached_key(Finding::sort_key);
}

fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_NAME && !s.chars().any(char::is_control)
}

/// A path relative to a crate root: `/` separated, not absolute, no drive, no backslash, no `..`.
fn valid_relpath(p: &str) -> bool {
    let b = p.as_bytes();
    !p.is_empty()
        && p.len() <= MAX_PATH
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.chars().any(char::is_control)
        && !(b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
        && p.split('/').all(|seg| seg != "..")
}

fn valid_digest(s: &str) -> bool {
    match s.strip_prefix("sha256:") {
        Some(hex) => hex.len() == 64 && hex.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        None => false,
    }
}

/// `sha256:` + hex digest of raw input bytes (lint config, lockfile, waiver file, denylist).
pub fn input_digest(bytes: &[u8]) -> String {
    digest_bytes(bytes).to_string()
}

// ------------------------------------------------------------------ waivers

/// One reviewed waiver from `knp1-waivers.toml` (REQ-KRN.NUM.LINT.FIND-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiver {
    pub crate_name: String,
    pub file: String,
    /// Inclusive 1-based line range `(start, end)`.
    pub lines: (u32, u32),
    pub rule: Rule,
    pub justification: String,
    pub reviewer: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// `sha256:` digest of the waived lines' LF-normalised text.
    pub lines_sha256: String,
}

impl Waiver {
    fn label(&self, index: usize) -> String {
        format!(
            "waiver #{} ({} {} lines {}-{} {})",
            index + 1,
            self.crate_name,
            self.file,
            self.lines.0,
            self.lines.1,
            self.rule
        )
    }

    fn matches(&self, f: &Finding) -> bool {
        self.crate_name == f.crate_name
            && self.file == f.file
            && self.rule == f.rule
            && self.lines.0 <= f.line
            && f.line <= self.lines.1
    }

    fn to_cvalue(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("crate".to_string(), CValue::Str(self.crate_name.clone()));
        o.insert("file".to_string(), CValue::Str(self.file.clone()));
        o.insert(
            "lines".to_string(),
            CValue::Arr(vec![CValue::Num(f64::from(self.lines.0)), CValue::Num(f64::from(self.lines.1))]),
        );
        o.insert("rule".to_string(), CValue::Str(self.rule.id()));
        o.insert("justification".to_string(), CValue::Str(self.justification.clone()));
        o.insert("reviewer".to_string(), CValue::Str(self.reviewer.clone()));
        o.insert("date".to_string(), CValue::Str(self.date.clone()));
        o.insert("lines_sha256".to_string(), CValue::Str(self.lines_sha256.clone()));
        CValue::Obj(o)
    }
}

const WAIVER_KEYS: [&str; 8] = ["crate", "file", "lines", "rule", "justification", "reviewer", "date", "lines_sha256"];

/// Loads `knp1-waivers.toml` from raw bytes (bounded, UTF-8 only). An empty file has no waivers.
pub fn load_waivers(bytes: &[u8]) -> Result<Vec<Waiver>, FindError> {
    if bytes.len() > MAX_WAIVER_FILE_BYTES {
        return Err(schema_err(None, format!("waiver file larger than {MAX_WAIVER_FILE_BYTES} bytes")));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| schema_err(None, "waiver file is not UTF-8"))?;
    parse_waivers(text)
}

fn schema_err(index: Option<usize>, detail: impl fmt::Display) -> FindError {
    let msg = match index {
        Some(i) => format!("waiver #{}: {detail}", i + 1),
        None => detail.to_string(),
    };
    FindError::new(ErrorCode::WaiverSchema, msg)
}

/// Parses the waiver file text. Any missing, unknown or malformed field fails with
/// `KNP1-WAIVER-SCHEMA` naming the waiver (1-based, in file order).
pub fn parse_waivers(text: &str) -> Result<Vec<Waiver>, FindError> {
    if text.len() > MAX_WAIVER_FILE_BYTES {
        return Err(schema_err(None, format!("waiver file larger than {MAX_WAIVER_FILE_BYTES} bytes")));
    }
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| schema_err(None, e.to_string().trim_end()))?;
    let mut out = Vec::new();
    for (key, value) in &table {
        if key != "waiver" {
            return Err(schema_err(None, format!("unknown top-level key {key:?}; expected [[waiver]] tables")));
        }
        let toml::Value::Array(items) = value else {
            return Err(schema_err(None, "`waiver` must be an array of tables ([[waiver]])"));
        };
        if items.len() > MAX_WAIVERS {
            return Err(schema_err(None, format!("more than {MAX_WAIVERS} waivers")));
        }
        for (i, item) in items.iter().enumerate() {
            let toml::Value::Table(t) = item else {
                return Err(schema_err(Some(i), "not a table"));
            };
            out.push(parse_waiver(i, t)?);
        }
    }
    Ok(out)
}

fn parse_waiver(i: usize, t: &toml::Table) -> Result<Waiver, FindError> {
    for k in t.keys() {
        if !WAIVER_KEYS.contains(&k.as_str()) {
            return Err(schema_err(Some(i), format!("unknown field {k:?}")));
        }
    }
    for k in WAIVER_KEYS {
        if !t.contains_key(k) {
            return Err(schema_err(Some(i), format!("missing field `{k}`")));
        }
    }
    let crate_name = string_field(i, t, "crate")?;
    if !valid_name(&crate_name) {
        return Err(schema_err(Some(i), "bad `crate`"));
    }
    let file = string_field(i, t, "file")?;
    if !valid_relpath(&file) {
        return Err(schema_err(Some(i), "`file` must be a '/'-separated path relative to the crate root"));
    }
    let lines = lines_field(i, t)?;
    let rule_id = string_field(i, t, "rule")?;
    let rule = Rule::parse(&rule_id).ok_or_else(|| schema_err(Some(i), format!("unknown rule {rule_id:?}")))?;
    let justification = text_field(i, t, "justification")?;
    let reviewer = text_field(i, t, "reviewer")?;
    let date = date_field(i, t)?;
    let lines_sha256 = string_field(i, t, "lines_sha256")?;
    if !valid_digest(&lines_sha256) {
        return Err(schema_err(Some(i), "`lines_sha256` must be sha256: plus 64 lowercase hex digits"));
    }
    Ok(Waiver { crate_name, file, lines, rule, justification, reviewer, date, lines_sha256 })
}

fn string_field(i: usize, t: &toml::Table, k: &str) -> Result<String, FindError> {
    match t.get(k) {
        Some(toml::Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(schema_err(Some(i), format!("`{k}` must be a string"))),
        None => Err(schema_err(Some(i), format!("missing field `{k}`"))),
    }
}

fn text_field(i: usize, t: &toml::Table, k: &str) -> Result<String, FindError> {
    let s = string_field(i, t, k)?;
    if s.trim().is_empty() || s.len() > MAX_TEXT {
        return Err(schema_err(Some(i), format!("`{k}` must be non-empty and at most {MAX_TEXT} bytes")));
    }
    Ok(s)
}

fn lines_field(i: usize, t: &toml::Table) -> Result<(u32, u32), FindError> {
    let bad = || schema_err(Some(i), "`lines` must be [start, end] with 1 <= start <= end");
    let Some(toml::Value::Array(a)) = t.get("lines") else {
        return Err(bad());
    };
    let nums: Vec<u32> = a
        .iter()
        .map(|v| match v {
            toml::Value::Integer(n) => u32::try_from(*n).ok(),
            _ => None,
        })
        .collect::<Option<Vec<u32>>>()
        .ok_or_else(bad)?;
    match nums.as_slice() {
        [s, e] if *s >= 1 && s <= e => Ok((*s, *e)),
        _ => Err(bad()),
    }
}

fn date_field(i: usize, t: &toml::Table) -> Result<String, FindError> {
    let s = match t.get("date") {
        Some(toml::Value::String(s)) => s.clone(),
        Some(toml::Value::Datetime(d)) => d.to_string(),
        _ => return Err(schema_err(Some(i), "`date` must be a YYYY-MM-DD date")),
    };
    if !valid_date(&s) {
        return Err(schema_err(Some(i), format!("`date` {s:?} is not YYYY-MM-DD")));
    }
    Ok(s)
}

fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = b.get(r)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some(part.iter().fold(0u32, |acc, d| acc * 10 + u32::from(d - b'0')))
    };
    match (digits(0..4), digits(5..7), digits(8..10)) {
        (Some(_), Some(m), Some(d)) => (1..=12).contains(&m) && (1..=31).contains(&d),
        _ => false,
    }
}

// ------------------------------------------------------------------ source access and line digests

/// Read access to the current source of the crates the lint scanned.
pub trait SourceTree {
    /// Raw bytes of `file` (relative to the root of `crate_name`), or `None` if unavailable.
    fn read(&self, crate_name: &str, file: &str) -> Option<Vec<u8>>;
}

/// In-memory sources keyed by `(crate, file)`.
impl SourceTree for BTreeMap<(String, String), Vec<u8>> {
    fn read(&self, crate_name: &str, file: &str) -> Option<Vec<u8>> {
        self.get(&(crate_name.to_string(), file.to_string())).cloned()
    }
}

/// Sources on disk: one root directory per crate name. Reads are bounded by [`MAX_SOURCE_BYTES`].
#[derive(Debug, Clone, Default)]
pub struct CrateRoots {
    roots: BTreeMap<String, PathBuf>,
}

impl CrateRoots {
    pub fn new() -> CrateRoots {
        CrateRoots::default()
    }

    /// Registers the root directory of `crate_name`.
    pub fn insert(&mut self, crate_name: impl Into<String>, root: impl Into<PathBuf>) {
        self.roots.insert(crate_name.into(), root.into());
    }
}

impl SourceTree for CrateRoots {
    fn read(&self, crate_name: &str, file: &str) -> Option<Vec<u8>> {
        if !valid_relpath(file) {
            return None;
        }
        let path = self.roots.get(crate_name)?.join(file);
        let f = std::fs::File::open(path).ok()?;
        let mut buf = Vec::new();
        f.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut buf).ok()?;
        if buf.len() as u64 > MAX_SOURCE_BYTES {
            return None;
        }
        Some(buf)
    }
}

/// Digest of lines `start..=end` (1-based) of `source` after CRLF→LF, each followed by LF.
/// `None` if the range is empty or runs past the end of the file.
pub fn lines_digest(source: &[u8], start: u32, end: u32) -> Option<String> {
    if start == 0 || start > end {
        return None;
    }
    let mut text = Vec::with_capacity(source.len());
    let mut k = 0;
    while k < source.len() {
        if source[k] == b'\r' && source.get(k + 1) == Some(&b'\n') {
            k += 1;
            continue;
        }
        text.push(source[k]);
        k += 1;
    }
    let body = text.strip_suffix(b"\n").unwrap_or(&text);
    let lines: Vec<&[u8]> = if text.is_empty() { Vec::new() } else { body.split(|b| *b == b'\n').collect() };
    let s = usize::try_from(start).ok()?;
    let e = usize::try_from(end).ok()?;
    let chosen = lines.get(s - 1..e)?;
    let mut buf = Vec::new();
    for l in chosen {
        buf.extend_from_slice(l);
        buf.push(b'\n');
    }
    Some(input_digest(&buf))
}

// ------------------------------------------------------------------ applying waivers

/// A warning that does not fail the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Always `KNP1-WAIVER-UNUSED` in schema 1.
    pub code: &'static str,
    pub waiver: Waiver,
    /// Human-readable message naming the waiver (not written to the report).
    pub message: String,
}

/// Code of the unused-waiver warning.
pub const WAIVER_UNUSED: &str = "KNP1-WAIVER-UNUSED";

/// Findings split by the waivers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    pub findings: Vec<Finding>,
    pub waived: Vec<(Finding, Waiver)>,
    pub warnings: Vec<Warning>,
}

impl Applied {
    /// No waiver file: every finding stays unwaived.
    pub fn without_waivers(findings: Vec<Finding>) -> Applied {
        Applied { findings, waived: Vec::new(), warnings: Vec::new() }
    }
}

/// Checks every waiver against the current source, then waives matching findings.
///
/// Fails with every stale waiver (`KNP1-WAIVER-STALE`, REQ-005) whether or not a finding still
/// occurs there. Otherwise a finding is waived by the first waiver (file order) with the same
/// crate, file and rule whose range contains its line (REQ-004), and every waiver that waived
/// nothing yields a `KNP1-WAIVER-UNUSED` warning (REQ-006).
pub fn apply_waivers(
    findings: Vec<Finding>,
    waivers: &[Waiver],
    sources: &dyn SourceTree,
) -> Result<Applied, Vec<FindError>> {
    let mut stale = Vec::new();
    for (i, w) in waivers.iter().enumerate() {
        let current = sources.read(&w.crate_name, &w.file).and_then(|src| lines_digest(&src, w.lines.0, w.lines.1));
        if current.as_deref() != Some(w.lines_sha256.as_str()) {
            let now = current.unwrap_or_else(|| "unavailable (file missing or range past end of file)".to_string());
            stale.push(FindError::new(
                ErrorCode::WaiverStale,
                format!("{}: recorded {}, current {now}; renew the waiver", w.label(i), w.lines_sha256),
            ));
        }
    }
    if !stale.is_empty() {
        return Err(stale);
    }
    let mut used = vec![false; waivers.len()];
    let mut out = Applied::default();
    for f in findings {
        match waivers.iter().position(|w| w.matches(&f)) {
            Some(i) => {
                if let Some(u) = used.get_mut(i) {
                    *u = true;
                }
                if let Some(w) = waivers.get(i) {
                    out.waived.push((f, w.clone()));
                }
            }
            None => out.findings.push(f),
        }
    }
    for (i, w) in waivers.iter().enumerate() {
        if !used.get(i).copied().unwrap_or(false) {
            out.warnings.push(Warning {
                code: WAIVER_UNUSED,
                waiver: w.clone(),
                message: format!("{}: matches no finding; remove it", w.label(i)),
            });
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ report

/// SHA-256 digests of the inputs the run used (REQ-KRN.NUM.LINT.FIND-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDigests {
    pub config: String,
    pub lockfile: String,
    pub waivers: String,
    pub denylist: String,
}

impl InputDigests {
    /// Digests of the raw input files. Pass empty bytes for an absent waiver file.
    pub fn of(config: &[u8], lockfile: &[u8], waivers: &[u8], denylist: &[u8]) -> InputDigests {
        InputDigests {
            config: input_digest(config),
            lockfile: input_digest(lockfile),
            waivers: input_digest(waivers),
            denylist: input_digest(denylist),
        }
    }
}

/// The `knp1-findings.json` report, schema 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub tool_version: String,
    pub inputs: InputDigests,
    /// Unwaived findings, sorted.
    pub findings: Vec<Finding>,
    /// Waived findings with their waiver, sorted by finding.
    pub waived: Vec<(Finding, Waiver)>,
    pub warnings: Vec<Warning>,
}

impl Report {
    /// Validates and sorts. Fails with `invalid_finding` if a finding or digest breaks the schema.
    pub fn new(tool_version: &str, inputs: InputDigests, applied: Applied) -> Result<Report, FindError> {
        if !valid_name(tool_version) {
            return Err(FindError::new(ErrorCode::InvalidFinding, "bad tool version"));
        }
        for (name, d) in [
            ("config", &inputs.config),
            ("lockfile", &inputs.lockfile),
            ("waivers", &inputs.waivers),
            ("denylist", &inputs.denylist),
        ] {
            if !valid_digest(d) {
                return Err(FindError::new(ErrorCode::InvalidFinding, format!("bad {name} digest")));
            }
        }
        let Applied { mut findings, mut waived, warnings } = applied;
        for f in findings.iter().chain(waived.iter().map(|(f, _)| f)) {
            f.validate()?;
        }
        sort_findings(&mut findings);
        waived.sort_by_cached_key(|(f, _)| f.sort_key());
        Ok(Report { tool_version: tool_version.to_string(), inputs, findings, waived, warnings })
    }

    /// Number of unwaived findings.
    pub fn unwaived_count(&self) -> usize {
        self.findings.len()
    }

    /// True when no unwaived finding remains (warnings do not fail the run).
    pub fn passed(&self) -> bool {
        self.findings.is_empty()
    }

    /// The report as a canonical-JSON value.
    pub fn to_cvalue(&self) -> CValue {
        let count = |n: usize| CValue::Num(n as f64);
        let mut inputs = BTreeMap::new();
        inputs.insert("config".to_string(), CValue::Str(self.inputs.config.clone()));
        inputs.insert("lockfile".to_string(), CValue::Str(self.inputs.lockfile.clone()));
        inputs.insert("waivers".to_string(), CValue::Str(self.inputs.waivers.clone()));
        inputs.insert("denylist".to_string(), CValue::Str(self.inputs.denylist.clone()));

        let mut by_rule: BTreeMap<String, usize> = BTreeMap::new();
        for f in &self.findings {
            *by_rule.entry(f.rule.id()).or_insert(0) += 1;
        }
        let mut summary = BTreeMap::new();
        summary.insert("findings".to_string(), count(self.findings.len()));
        summary.insert("waived".to_string(), count(self.waived.len()));
        summary.insert("warnings".to_string(), count(self.warnings.len()));
        summary.insert("by_rule".to_string(), CValue::Obj(by_rule.into_iter().map(|(k, n)| (k, count(n))).collect()));

        let waived = self
            .waived
            .iter()
            .map(|(f, w)| {
                let mut o = BTreeMap::new();
                o.insert("finding".to_string(), f.to_cvalue());
                o.insert("waiver".to_string(), w.to_cvalue());
                CValue::Obj(o)
            })
            .collect();
        let warnings = self
            .warnings
            .iter()
            .map(|w| {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(w.code.to_string()));
                o.insert("waiver".to_string(), w.waiver.to_cvalue());
                CValue::Obj(o)
            })
            .collect();

        let mut r = BTreeMap::new();
        r.insert("schema".to_string(), CValue::Num(f64::from(REPORT_SCHEMA)));
        r.insert("tool".to_string(), CValue::Str(TOOL_NAME.to_string()));
        r.insert("tool_version".to_string(), CValue::Str(self.tool_version.clone()));
        r.insert("inputs".to_string(), CValue::Obj(inputs));
        r.insert("findings".to_string(), CValue::Arr(self.findings.iter().map(Finding::to_cvalue).collect()));
        r.insert("waived".to_string(), CValue::Arr(waived));
        r.insert("warnings".to_string(), CValue::Arr(warnings));
        r.insert("summary".to_string(), CValue::Obj(summary));
        CValue::Obj(r)
    }

    /// RFC 8785 canonical bytes of the report (no trailing newline).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, FindError> {
        to_canonical_bytes(&self.to_cvalue()).map_err(|e| FindError::new(ErrorCode::ReportWriteFailed, e.to_string()))
    }

    /// Writes the canonical report to `path`.
    pub fn write(&self, path: &Path) -> Result<(), FindError> {
        let bytes = self.to_canonical_bytes()?;
        std::fs::write(path, bytes)
            .map_err(|e| FindError::new(ErrorCode::ReportWriteFailed, format!("{}: {e}", path.display())))
    }
}
