//! Metric definition file, source list and guard (spec node `OUTR.COMM.METRIC.DEF`).
//!
//! The guard is the no-telemetry boundary of adoption measurement: it refuses any metric
//! without a complete definition, any source not marked public with a stated basis, and any
//! source whose URL or ID looks like data collected by the product ([`DENY_LIST`]). The
//! collector (`OUTR.COMM.METRIC.COLLECT`) calls [`check`] / [`check_files`] before it fetches
//! anything.
//!
//! Determinism D0: pure functions over the two input texts. Findings come in a fixed order:
//! sources in file order, then metrics in file order, each with its fields in the order of
//! [`SOURCE_FIELDS`] / [`METRIC_FIELDS`].

use fid_canon::{to_canonical_bytes, CValue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::Path;

/// Product-data source patterns (REQ-OUTR.COMM.METRIC.DEF-003), matched as case-insensitive
/// substrings of a source's URL and ID. Compiled in on purpose: the definition files cannot
/// change it. Maintainers may add entries through a pull request to this crate, never remove.
pub const DENY_LIST: &[&str] = &[
    "telemetry",
    "crash",
    "support-bundle",
    "support_bundle",
    "update-check",
    "activation",
    "licence-server",
    "license-server",
    "diagnostic",
];

/// Largest accepted input file, in bytes.
pub const MAX_BYTES: usize = 1024 * 1024;

/// Fields of a `[[metric]]` entry, in reporting order (REQ-OUTR.COMM.METRIC.DEF-001).
pub const METRIC_FIELDS: [&str; 8] = ["id", "title", "source", "method", "selector", "unit", "aggregation", "biases"];
/// Fields of a `[[source]]` entry, in reporting order (REQ-OUTR.COMM.METRIC.DEF-002).
pub const SOURCE_FIELDS: [&str; 4] = ["id", "url", "public", "public_basis"];

/// How the collector obtains a metric's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Method {
    /// GET the source URL and read the value at the JSON Pointer `selector`.
    HttpJson,
    /// Count from the public repository history.
    GitHistory,
    /// A maintainer reads a number off a public page.
    ManualPublicCount,
}

impl Method {
    /// The name used in the definition file.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::HttpJson => "http_json",
            Method::GitHistory => "git_history",
            Method::ManualPublicCount => "manual_public_count",
        }
    }

    fn from_name(s: &str) -> Option<Method> {
        match s {
            "http_json" => Some(Method::HttpJson),
            "git_history" => Some(Method::GitHistory),
            "manual_public_count" => Some(Method::ManualPublicCount),
            _ => None,
        }
    }

    /// Whether a definition using this method must state a `selector`.
    pub fn needs_selector(self) -> bool {
        matches!(self, Method::HttpJson)
    }
}

/// How a quarter's value is formed from that quarter's snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Aggregation {
    Last,
    Sum,
    Max,
}

impl Aggregation {
    /// The name used in the definition file.
    pub fn as_str(self) -> &'static str {
        match self {
            Aggregation::Last => "last",
            Aggregation::Sum => "sum",
            Aggregation::Max => "max",
        }
    }

    fn from_name(s: &str) -> Option<Aggregation> {
        match s {
            "last" => Some(Aggregation::Last),
            "sum" => Some(Aggregation::Sum),
            "max" => Some(Aggregation::Max),
            _ => None,
        }
    }
}

/// A complete, valid `[[metric]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metric {
    pub id: String,
    pub title: String,
    pub source: String,
    pub method: Method,
    /// JSON Pointer (RFC 6901) for `http_json`; `None` when the method needs none and none is given.
    pub selector: Option<String>,
    pub unit: String,
    pub aggregation: Aggregation,
    pub biases: Vec<String>,
}

/// A complete, valid, public `[[source]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub url: String,
    pub public_basis: String,
}

/// Finding codes. `field_invalid` is an addition to the spec's list: a field that is present
/// but of the wrong type or outside its allowed values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    FieldMissing,
    FieldInvalid,
    SourceNotPublic,
    ProductDataSource,
    SourceUnknown,
    DuplicateId,
}

impl Code {
    /// Stable code string.
    pub fn as_str(self) -> &'static str {
        match self {
            Code::FieldMissing => "field_missing",
            Code::FieldInvalid => "field_invalid",
            Code::SourceNotPublic => "source_not_public",
            Code::ProductDataSource => "product_data_source",
            Code::SourceUnknown => "source_unknown",
            Code::DuplicateId => "duplicate_id",
        }
    }
}

/// Which list a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// The definitions file as a whole (e.g. no `[[metric]]` array).
    DefsFile,
    /// The sources file as a whole.
    SourcesFile,
    Metric,
    Source,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::DefsFile => "definitions",
            Kind::SourcesFile => "sources",
            Kind::Metric => "metric",
            Kind::Source => "source",
        }
    }
}

/// One guard finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub code: Code,
    pub kind: Kind,
    /// 1-based position of the entry in its file; 0 for whole-file findings.
    pub index: usize,
    /// The entry's ID when it has a usable one.
    pub id: Option<String>,
    /// The field concerned (for `field_*`), or the matched deny-list pattern
    /// (for `product_data_source`), or the referenced source ID (for `source_unknown`).
    pub subject: Option<String>,
}

impl fmt::Display for Finding {
    /// `<code>[ <subject>] (<kind> #<index>[ "<id>"])`, e.g. `field_missing biases (metric #1 "stars")`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code.as_str())?;
        if let Some(s) = &self.subject {
            write!(f, " {s}")?;
        }
        write!(f, " ({}", self.kind.as_str())?;
        if self.index > 0 {
            write!(f, " #{}", self.index)?;
        }
        if let Some(id) = &self.id {
            write!(f, " {id:?}")?;
        }
        f.write_str(")")
    }
}

/// Input that could not be read or parsed (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefError {
    /// The file could not be read, or is larger than [`MAX_BYTES`].
    Io { file: String, message: String },
    /// The file is not UTF-8 or not valid TOML (including duplicate keys).
    Parse { file: String, message: String },
}

impl DefError {
    /// Stable code string: `io` or `parse`.
    pub fn code(&self) -> &'static str {
        match self {
            DefError::Io { .. } => "io",
            DefError::Parse { .. } => "parse",
        }
    }

    fn parts(&self) -> (&str, &str) {
        match self {
            DefError::Io { file, message } | DefError::Parse { file, message } => (file, message),
        }
    }
}

impl fmt::Display for DefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (file, message) = self.parts();
        write!(f, "{} {file}: {message}", self.code())
    }
}

/// Result of a guard run over parseable input.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// Metrics with a complete, valid definition, in file order.
    pub metrics: Vec<Metric>,
    /// Sources with all fields valid and `public = true` with a basis, in file order.
    pub sources: Vec<Source>,
    pub findings: Vec<Finding>,
}

impl Report {
    /// True when the guard found nothing; the collector may run only then.
    pub fn passed(&self) -> bool {
        self.findings.is_empty()
    }

    /// 0 with no findings, 1 with any.
    pub fn exit_code(&self) -> u8 {
        if self.passed() {
            0
        } else {
            1
        }
    }

    /// Canonical JSON report (`status`, `metrics`, `sources`, `findings`).
    pub fn to_json(&self) -> Vec<u8> {
        let metrics = self
            .metrics
            .iter()
            .map(|m| {
                let mut o = obj(vec![
                    ("id", s(&m.id)),
                    ("title", s(&m.title)),
                    ("source", s(&m.source)),
                    ("method", s(m.method.as_str())),
                    ("unit", s(&m.unit)),
                    ("aggregation", s(m.aggregation.as_str())),
                    ("biases", CValue::Arr(m.biases.iter().map(|b| s(b)).collect())),
                ]);
                if let (CValue::Obj(map), Some(sel)) = (&mut o, &m.selector) {
                    map.insert("selector".to_string(), s(sel));
                }
                o
            })
            .collect();
        let sources = self
            .sources
            .iter()
            .map(|x| obj(vec![("id", s(&x.id)), ("url", s(&x.url)), ("public", CValue::Bool(true))]));
        let findings = self.findings.iter().map(|f| {
            let mut o = obj(vec![
                ("code", s(f.code.as_str())),
                ("kind", s(f.kind.as_str())),
                ("index", CValue::Num(f.index as f64)),
                ("message", s(&f.to_string())),
            ]);
            if let CValue::Obj(map) = &mut o {
                if let Some(id) = &f.id {
                    map.insert("id".to_string(), s(id));
                }
                if let Some(sub) = &f.subject {
                    map.insert("subject".to_string(), s(sub));
                }
            }
            o
        });
        let report = obj(vec![
            ("status", s(if self.passed() { "ok" } else { "findings" })),
            ("metrics", CValue::Arr(metrics)),
            ("sources", CValue::Arr(sources.collect())),
            ("findings", CValue::Arr(findings.collect())),
        ]);
        canonical(&report)
    }
}

/// Canonical JSON for an input error: `{"error":{"code","file","message"},"status":"error"}`.
pub fn error_json(e: &DefError) -> Vec<u8> {
    let (file, message) = e.parts();
    let err = obj(vec![("code", s(e.code())), ("file", s(file)), ("message", s(message))]);
    canonical(&obj(vec![("status", s("error")), ("error", err)]))
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj(fields: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>())
}

fn canonical(v: &CValue) -> Vec<u8> {
    // Every number here is a small finite index and every key is unique, so canonicalisation
    // cannot fail; fall back to a fixed error document rather than panicking.
    to_canonical_bytes(v).unwrap_or_else(|_| b"{\"status\":\"error\"}".to_vec())
}

/// The deny-list pattern that `text` contains (case-insensitive), if any.
pub fn product_data_pattern(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    DENY_LIST.iter().copied().find(|p| lower.contains(p))
}

/// Reads both files (each at most [`MAX_BYTES`]) and runs [`check`].
pub fn check_files(defs: &Path, sources: &Path) -> Result<Report, DefError> {
    let d = read_text(defs)?;
    let s = read_text(sources)?;
    check(&d, &defs.display().to_string(), &s, &sources.display().to_string())
}

fn read_text(path: &Path) -> Result<String, DefError> {
    let file = path.display().to_string();
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| DefError::Io { file: file.clone(), message: e.to_string() })?;
    if bytes.len() > MAX_BYTES {
        return Err(DefError::Io { file, message: format!("larger than {MAX_BYTES} bytes") });
    }
    String::from_utf8(bytes).map_err(|_| DefError::Parse { file, message: "not valid UTF-8".to_string() })
}

fn parse_table(text: &str, file: &str) -> Result<toml::Table, DefError> {
    text.parse::<toml::Table>().map_err(|e| {
        let at = e.span().map(|r| format!(" at byte {}", r.start)).unwrap_or_default();
        DefError::Parse { file: file.to_string(), message: format!("{}{at}", e.message()) }
    })
}

/// Runs the guard over the text of a definitions file and a sources file. The file names are
/// used only in error messages. Returns `Err` only for unparseable input; every other problem
/// is a [`Finding`] in the report.
pub fn check(defs: &str, defs_name: &str, sources: &str, sources_name: &str) -> Result<Report, DefError> {
    let defs_t = parse_table(defs, defs_name)?;
    let sources_t = parse_table(sources, sources_name)?;
    let mut report = Report::default();

    // Sources first: metrics refer to them.
    let mut source_ids: BTreeSet<String> = BTreeSet::new();
    for (i, t) in entries(&sources_t, "source", Kind::SourcesFile, &mut report.findings) {
        let id = check_source(i, t, &source_ids, &mut report);
        if let Some(id) = id {
            source_ids.insert(id);
        }
    }

    let mut metric_ids: BTreeSet<String> = BTreeSet::new();
    for (i, t) in entries(&defs_t, "metric", Kind::DefsFile, &mut report.findings) {
        if let Some(id) = check_metric(i, t, &metric_ids, &source_ids, &mut report) {
            metric_ids.insert(id);
        }
    }
    Ok(report)
}

/// The `[[key]]` tables of a file with their 1-based positions; non-table items become
/// `field_invalid` findings and are skipped.
fn entries<'a>(
    t: &'a toml::Table,
    key: &str,
    kind: Kind,
    findings: &mut Vec<Finding>,
) -> Vec<(usize, &'a toml::Table)> {
    let file_finding = |code| Finding { code, kind, index: 0, id: None, subject: Some(key.to_string()) };
    let items = match t.get(key) {
        None => {
            findings.push(file_finding(Code::FieldMissing));
            return Vec::new();
        }
        Some(toml::Value::Array(a)) if !a.is_empty() => a,
        Some(toml::Value::Array(_)) => {
            findings.push(file_finding(Code::FieldMissing));
            return Vec::new();
        }
        Some(_) => {
            findings.push(file_finding(Code::FieldInvalid));
            return Vec::new();
        }
    };
    let entry_kind = if kind == Kind::DefsFile { Kind::Metric } else { Kind::Source };
    let mut out = Vec::new();
    for (n, v) in items.iter().enumerate() {
        match v {
            toml::Value::Table(e) => out.push((n + 1, e)),
            _ => findings.push(Finding {
                code: Code::FieldInvalid,
                kind: entry_kind,
                index: n + 1,
                id: None,
                subject: None,
            }),
        }
    }
    out
}

/// A required non-empty string field: `Ok(Some)`, or the finding code for missing/invalid.
fn req_str(t: &toml::Table, key: &str) -> Result<String, Code> {
    match t.get(key) {
        None => Err(Code::FieldMissing),
        Some(toml::Value::String(v)) if v.trim().is_empty() => Err(Code::FieldMissing),
        Some(toml::Value::String(v)) => Ok(v.clone()),
        Some(_) => Err(Code::FieldInvalid),
    }
}

/// IDs are lowercase ASCII letters, digits, `_` and `-`, starting with a letter.
fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// RFC 6901 JSON Pointer syntax: empty, or `/`-prefixed with `~` only in `~0` / `~1`.
pub fn valid_json_pointer(p: &str) -> bool {
    if !(p.is_empty() || p.starts_with('/')) {
        return false;
    }
    let b = p.as_bytes();
    b.iter().enumerate().all(|(i, &c)| c != b'~' || matches!(b.get(i + 1), Some(b'0') | Some(b'1')))
}

struct Ctx<'r> {
    kind: Kind,
    index: usize,
    id: Option<String>,
    findings: &'r mut Vec<Finding>,
}

impl Ctx<'_> {
    fn push(&mut self, code: Code, subject: Option<&str>) {
        self.findings.push(Finding {
            code,
            kind: self.kind,
            index: self.index,
            id: self.id.clone(),
            subject: subject.map(str::to_string),
        });
    }

    fn field(&mut self, code: Code, field: &str) {
        self.push(code, Some(field));
    }
}

/// Checks one source; returns its ID when the ID itself is valid and new (so metrics may refer
/// to it even if the source has other findings; those already fail the run).
fn check_source(index: usize, t: &toml::Table, seen: &BTreeSet<String>, report: &mut Report) -> Option<String> {
    let id = req_str(t, "id");
    let usable_id = id.as_ref().ok().filter(|v| valid_id(v)).cloned();
    let raw_id = id.as_ref().ok().cloned();
    let mut cx = Ctx { kind: Kind::Source, index, id: raw_id.clone(), findings: &mut report.findings };
    let before = cx.findings.len();

    match &id {
        Err(code) => cx.field(*code, "id"),
        Ok(_) if usable_id.is_none() => cx.field(Code::FieldInvalid, "id"),
        Ok(_) => {}
    }
    let url = req_str(t, "url");
    match &url {
        Err(code) => cx.field(*code, "url"),
        Ok(u) if !u.starts_with("https://") => cx.field(Code::FieldInvalid, "url"),
        Ok(_) => {}
    }
    // REQ-002: only an explicit `public = true` with a non-empty basis counts as public.
    let public = matches!(t.get("public"), Some(toml::Value::Boolean(true)));
    let basis = req_str(t, "public_basis");
    if !public || basis.is_err() {
        cx.push(Code::SourceNotPublic, None);
    }
    // REQ-003: whatever the public flag says.
    let pattern =
        raw_id.as_deref().and_then(product_data_pattern).or_else(|| url.as_deref().ok().and_then(product_data_pattern));
    if let Some(p) = pattern {
        cx.push(Code::ProductDataSource, Some(p));
    }
    let new_id = match usable_id {
        Some(v) if seen.contains(&v) => {
            cx.field(Code::DuplicateId, "id");
            None
        }
        other => other,
    };
    if cx.findings.len() == before {
        if let (Some(id), Ok(url), Ok(public_basis)) = (new_id.clone(), url, basis) {
            report.sources.push(Source { id, url, public_basis });
        }
    }
    new_id
}

/// Checks one metric; returns its ID when valid and new.
fn check_metric(
    index: usize,
    t: &toml::Table,
    seen: &BTreeSet<String>,
    sources: &BTreeSet<String>,
    report: &mut Report,
) -> Option<String> {
    let id = req_str(t, "id");
    let usable_id = id.as_ref().ok().filter(|v| valid_id(v)).cloned();
    let mut cx = Ctx { kind: Kind::Metric, index, id: id.as_ref().ok().cloned(), findings: &mut report.findings };
    let before = cx.findings.len();

    match &id {
        Err(code) => cx.field(*code, "id"),
        Ok(_) if usable_id.is_none() => cx.field(Code::FieldInvalid, "id"),
        Ok(_) => {}
    }
    let title = req_str(t, "title").map_err(|c| cx.field(c, "title")).ok();
    let source = req_str(t, "source").map_err(|c| cx.field(c, "source")).ok();
    let method = match req_str(t, "method") {
        Err(c) => {
            cx.field(c, "method");
            None
        }
        Ok(m) => Method::from_name(&m).or_else(|| {
            cx.field(Code::FieldInvalid, "method");
            None
        }),
    };
    let selector = match t.get("selector") {
        None if method.is_some_and(Method::needs_selector) => {
            cx.field(Code::FieldMissing, "selector");
            None
        }
        None => None,
        Some(toml::Value::String(p)) if valid_json_pointer(p) => Some(p.clone()),
        Some(_) => {
            cx.field(Code::FieldInvalid, "selector");
            None
        }
    };
    let unit = req_str(t, "unit").map_err(|c| cx.field(c, "unit")).ok();
    let aggregation = match req_str(t, "aggregation") {
        Err(c) => {
            cx.field(c, "aggregation");
            None
        }
        Ok(a) => Aggregation::from_name(&a).or_else(|| {
            cx.field(Code::FieldInvalid, "aggregation");
            None
        }),
    };
    let biases = match t.get("biases") {
        None => {
            cx.field(Code::FieldMissing, "biases");
            None
        }
        Some(toml::Value::Array(a)) if a.is_empty() => {
            cx.field(Code::FieldMissing, "biases");
            None
        }
        Some(toml::Value::Array(a)) => {
            let texts: Option<Vec<String>> = a
                .iter()
                .map(|v| match v {
                    toml::Value::String(b) if !b.trim().is_empty() => Some(b.clone()),
                    _ => None,
                })
                .collect();
            if texts.is_none() {
                cx.field(Code::FieldInvalid, "biases");
            }
            texts
        }
        Some(_) => {
            cx.field(Code::FieldInvalid, "biases");
            None
        }
    };
    if let Some(src) = &source {
        if !sources.contains(src) {
            cx.push(Code::SourceUnknown, Some(src));
        }
    }
    let new_id = match usable_id {
        Some(v) if seen.contains(&v) => {
            cx.field(Code::DuplicateId, "id");
            None
        }
        other => other,
    };
    if cx.findings.len() == before {
        if let (Some(id), Some(title), Some(source), Some(method), Some(unit), Some(aggregation), Some(biases)) =
            (new_id.clone(), title, source, method, unit, aggregation, biases)
        {
            report.metrics.push(Metric { id, title, source, method, selector, unit, aggregation, biases });
        }
    }
    new_id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_pointer_syntax() {
        for ok in ["", "/", "/a/0", "/a~0b", "/a~1b"] {
            assert!(valid_json_pointer(ok), "{ok}");
        }
        for bad in ["a", "/a~", "/a~2", "#/a"] {
            assert!(!valid_json_pointer(bad), "{bad}");
        }
    }

    #[test]
    fn ids() {
        assert!(valid_id("github-repo_api2"));
        for bad in ["", "Stars", "1x", "a b", "-a"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn deny_list_is_case_insensitive() {
        assert_eq!(product_data_pattern("https://x.example.org/TeleMetry/"), Some("telemetry"));
        assert_eq!(product_data_pattern("License-Server"), Some("license-server"));
        assert_eq!(product_data_pattern("https://api.example.org/repos/demo"), None);
    }
}
