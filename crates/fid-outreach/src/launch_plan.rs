//! Milestone criteria file format and plan check (spec node `OUTR.LAUNCH.PLAN.FMT`).
//!
//! `launch/criteria.toml` (`version = 1`) holds every milestone of every focused app with its
//! checkable criteria. [`parse_criteria`] reads it into public types that the gate and review
//! checks reuse; [`check`] applies the plan rules: the D-017 milestone rule, the date rule for
//! the plan document and the announced-criteria rule against a base revision.
//!
//! Determinism D0: a pure function of the input files, findings sorted by (code, id, line).

use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::Path;

/// Largest accepted input file, in bytes (bounded parsing).
pub const MAX_FILE_BYTES: usize = 64 * 1024;
/// Most review entries read from the reviews directory.
pub const MAX_REVIEW_FILES: usize = 4096;
/// The only criteria file schema version this check accepts.
pub const SCHEMA_VERSION: i64 = 1;

/// Spec-tree phases in order (CONSTITUTION §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    P0,
    P1a,
    P1b,
    P2,
    P3,
    P4,
    Later,
}

impl Phase {
    pub fn parse(s: &str) -> Option<Phase> {
        Some(match s {
            "P0" => Phase::P0,
            "P1a" => Phase::P1a,
            "P1b" => Phase::P1b,
            "P2" => Phase::P2,
            "P3" => Phase::P3,
            "P4" => Phase::P4,
            "later" => Phase::Later,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Phase::P0 => "P0",
            Phase::P1a => "P1a",
            Phase::P1b => "P1b",
            Phase::P2 => "P2",
            Phase::P3 => "P3",
            Phase::P4 => "P4",
            Phase::Later => "later",
        }
    }
}

/// The D-017 rule table: the phase that holds each app's core workflow. An app reaches 1.0 only
/// when every phase holding its core workflow is finished (CONSTITUTION §7 "App milestones", D-017).
/// A new app needs a code change here and a decision.
pub const CORE_PHASES: [(&str, Phase); 4] =
    [("report", Phase::P1b), ("inspect", Phase::P1b), ("probe", Phase::P2), ("network", Phase::P3)];

/// The core phase of `app` in [`CORE_PHASES`], if the table has it.
pub fn core_phase(app: &str) -> Option<Phase> {
    CORE_PHASES.iter().find(|(a, _)| *a == app).map(|(_, p)| *p)
}

/// Node statuses a `node_status` criterion may name (the spec tree's status set).
pub const NODE_STATUSES: [&str; 5] = ["open", "split", "leaf", "decision", "frozen"];

/// Milestone level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Alpha,
    Beta,
    /// `1.0`.
    One,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        match s {
            "alpha" => Some(Level::Alpha),
            "beta" => Some(Level::Beta),
            "1.0" => Some(Level::One),
            _ => None,
        }
    }
}

/// Announcement record of a milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announced {
    pub date: String,
    pub release: String,
}

/// A criterion's kind and its fields (REQ-OUTR.LAUNCH.PLAN.FMT-002).
#[derive(Debug, Clone, PartialEq)]
pub enum CriterionKind {
    NodeStatus { node: String, statuses: Vec<String> },
    PhaseDone { phase: Phase, scope: Vec<String> },
    ParityApp { app: String, min_pct: f64 },
    ParitySeeds { seeds: Vec<String> },
    Signoff { role: String },
}

/// One milestone criterion.
#[derive(Debug, Clone, PartialEq)]
pub struct Criterion {
    pub id: String,
    pub kind: CriterionKind,
    /// Free text, not evaluated.
    pub note: Option<String>,
}

/// One milestone. Criteria that failed validation are left out (and reported).
#[derive(Debug, Clone, PartialEq)]
pub struct Milestone {
    pub id: String,
    pub app: String,
    pub level: Level,
    pub scope_label: Option<String>,
    pub target_date: Option<String>,
    pub announced: Option<Announced>,
    pub criteria: Vec<Criterion>,
}

/// The parsed criteria file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Criteria {
    pub milestones: Vec<Milestone>,
}

impl Criteria {
    /// Every `target_date` and `announced.date` in the file.
    pub fn dates(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for m in &self.milestones {
            out.extend(m.target_date.iter().cloned());
            out.extend(m.announced.iter().map(|a| a.date.clone()));
        }
        out
    }
}

/// Finding codes (Design: Errors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    UnknownKind,
    BadField,
    DuplicateId,
    BadId,
    UnknownApp,
    CorePhaseMissing,
    ScopeLabelMissing,
    StrayDate,
    AnnouncedCriteriaChanged,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::UnknownKind => "unknown_kind",
            Code::BadField => "bad_field",
            Code::DuplicateId => "duplicate_id",
            Code::BadId => "bad_id",
            Code::UnknownApp => "unknown_app",
            Code::CorePhaseMissing => "core_phase_missing",
            Code::ScopeLabelMissing => "scope_label_missing",
            Code::StrayDate => "stray_date",
            Code::AnnouncedCriteriaChanged => "announced_criteria_changed",
        }
    }
}

/// One finding. `id` names the milestone, criterion or (for `stray_date`) the date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub code: Code,
    pub id: String,
    pub detail: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

impl Finding {
    fn new(code: Code, id: impl Into<String>, detail: impl Into<String>) -> Finding {
        Finding { code, id: id.into(), detail: detail.into(), file: None, line: None }
    }

    fn to_cvalue(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code.as_str().to_string()));
        o.insert("id".to_string(), CValue::Str(self.id.clone()));
        o.insert("detail".to_string(), CValue::Str(self.detail.clone()));
        if let Some(f) = &self.file {
            o.insert("file".to_string(), CValue::Str(f.clone()));
        }
        if let Some(l) = self.line {
            o.insert("line".to_string(), CValue::Num(f64::from(l)));
        }
        CValue::Obj(o)
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code.as_str(), self.id)?;
        match (&self.file, self.line) {
            (Some(file), Some(line)) => write!(f, " ({file}:{line})")?,
            (Some(file), None) => write!(f, " ({file})")?,
            _ => {}
        }
        write!(f, ": {}", self.detail)
    }
}

/// Fatal errors (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    InputUnreadable { path: String, reason: String },
    InputParse { path: String, reason: String },
}

impl PlanError {
    pub fn code(&self) -> &'static str {
        match self {
            PlanError::InputUnreadable { .. } => "InputUnreadable",
            PlanError::InputParse { .. } => "InputParse",
        }
    }

    pub fn to_cvalue(&self) -> CValue {
        let (path, reason) = match self {
            PlanError::InputUnreadable { path, reason } | PlanError::InputParse { path, reason } => (path, reason),
        };
        let mut e = BTreeMap::new();
        e.insert("code".to_string(), CValue::Str(self.code().to_string()));
        e.insert("path".to_string(), CValue::Str(path.clone()));
        e.insert("detail".to_string(), CValue::Str(reason.clone()));
        let mut o = BTreeMap::new();
        o.insert("error".to_string(), CValue::Obj(e));
        CValue::Obj(o)
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::InputUnreadable { path, reason } => write!(f, "InputUnreadable: {path}: {reason}"),
            PlanError::InputParse { path, reason } => write!(f, "InputParse: {path}: {reason}"),
        }
    }
}

impl std::error::Error for PlanError {}

// ------------------------------------------------------------------ inputs

/// Reads a UTF-8 text file of at most [`MAX_FILE_BYTES`].
pub fn read_text(path: &Path) -> Result<String, PlanError> {
    let shown = path.display().to_string();
    let unreadable = |reason: String| PlanError::InputUnreadable { path: shown.clone(), reason };
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| unreadable(e.to_string()))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(unreadable(format!("larger than {MAX_FILE_BYTES} bytes")));
    }
    String::from_utf8(bytes).map_err(|_| PlanError::InputParse { path: shown.clone(), reason: "not UTF-8".into() })
}

/// Parses TOML text into a table.
pub fn parse_toml(text: &str, path: &str) -> Result<toml::Table, PlanError> {
    text.parse::<toml::Table>().map_err(|e| PlanError::InputParse { path: path.into(), reason: e.message().into() })
}

/// The app IDs of an app list (`[[app]]` tables with a string `id`).
pub fn parse_apps(text: &str, path: &str) -> Result<BTreeSet<String>, PlanError> {
    let t = parse_toml(text, path)?;
    let bad = |reason: &str| PlanError::InputParse { path: path.into(), reason: reason.into() };
    let apps = t.get("app").and_then(|v| v.as_array()).ok_or_else(|| bad("no [[app]] array"))?;
    let mut ids = BTreeSet::new();
    for a in apps {
        let id =
            a.as_table().and_then(|t| t.get("id")).and_then(|v| v.as_str()).ok_or_else(|| bad("app without id"))?;
        ids.insert(id.to_string());
    }
    Ok(ids)
}

/// A post-launch review entry: front matter only (`milestone`, `announced`, `changes`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewEntry {
    pub file: String,
    pub milestone: String,
    pub announced: String,
    pub changes: Vec<String>,
}

/// Parses the front matter of a review entry. Fields are `key: value` lines; `changes` is a
/// single-line list `[A, B]` like the spec tree's frontmatter.
pub fn parse_review(text: &str, file: &str) -> Result<ReviewEntry, PlanError> {
    let bad = |reason: &str| PlanError::InputParse { path: file.into(), reason: reason.into() };
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    if lines.next() != Some("---") {
        return Err(bad("no front matter"));
    }
    let mut fields = BTreeMap::new();
    let mut closed = false;
    for line in lines {
        if line == "---" {
            closed = true;
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let (k, v) = line.split_once(':').ok_or_else(|| bad("front matter line without ':'"))?;
        fields.insert(k.trim().to_string(), v.trim().to_string());
    }
    if !closed {
        return Err(bad("front matter not closed"));
    }
    let get = |k: &str| fields.get(k).cloned().ok_or_else(|| bad(&format!("front matter lacks {k}")));
    let raw = get("changes")?;
    let inner = raw.strip_prefix('[').and_then(|r| r.strip_suffix(']')).ok_or_else(|| bad("changes is not [..]"))?;
    let changes = inner.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    Ok(ReviewEntry { file: file.into(), milestone: get("milestone")?, announced: get("announced")?, changes })
}

/// Reads every `*.md` review entry in `dir`, in file-name order.
pub fn read_reviews(dir: &Path) -> Result<Vec<ReviewEntry>, PlanError> {
    let shown = dir.display().to_string();
    let unreadable = |reason: String| PlanError::InputUnreadable { path: shown.clone(), reason };
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| unreadable(e.to_string()))? {
        let p = entry.map_err(|e| unreadable(e.to_string()))?.path();
        if p.extension().and_then(|e| e.to_str()) == Some("md") && p.is_file() {
            paths.push(p);
            if paths.len() > MAX_REVIEW_FILES {
                return Err(unreadable(format!("more than {MAX_REVIEW_FILES} review entries")));
            }
        }
    }
    paths.sort();
    paths.iter().map(|p| parse_review(&read_text(p)?, &p.display().to_string())).collect()
}

// ------------------------------------------------------------------ criteria parser

fn is_milestone_id(s: &str) -> bool {
    (3..=32).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_criterion_id(s: &str) -> bool {
    let b = s.as_bytes();
    (3..=32).contains(&b.len())
        && b.first().is_some_and(|c| c.is_ascii_uppercase())
        && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'-')
}

/// `YYYY-MM-DD` with a real month and day (proleptic Gregorian).
pub fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return false;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = b.get(r)?;
        part.iter().try_fold(0u32, |acc, c| c.is_ascii_digit().then(|| acc * 10 + u32::from(c - b'0')))
    };
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else {
        return false;
    };
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

/// A date field: a `YYYY-MM-DD` string or a TOML local date literal.
fn date_value(v: &toml::Value) -> Option<String> {
    let s = match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Datetime(dt) if dt.time.is_none() && dt.offset.is_none() => dt.to_string(),
        _ => return None,
    };
    is_iso_date(&s).then_some(s)
}

fn str_list(v: Option<&toml::Value>) -> Option<Vec<String>> {
    let a = v?.as_array()?;
    let out: Option<Vec<String>> = a.iter().map(|x| x.as_str().map(str::to_string)).collect();
    out.filter(|l| !l.is_empty())
}

fn nonempty_str(v: Option<&toml::Value>) -> Option<String> {
    v.and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()).map(str::to_string)
}

fn is_node_id(s: &str) -> bool {
    !s.is_empty()
        && s.split('.').all(|seg| {
            let b = seg.as_bytes();
            b.first().is_some_and(|c| c.is_ascii_uppercase())
                && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
}

struct Ctx {
    findings: Vec<Finding>,
}

impl Ctx {
    fn bad(&mut self, id: &str, detail: impl Into<String>) {
        self.findings.push(Finding::new(Code::BadField, id, detail));
    }

    fn unknown_keys(&mut self, id: &str, t: &toml::Table, allowed: &[&str]) {
        for k in t.keys().filter(|k| !allowed.contains(&k.as_str())) {
            self.bad(id, format!("unknown field `{k}`"));
        }
    }
}

fn parse_criterion(cx: &mut Ctx, ms: &str, idx: usize, v: &toml::Value) -> Option<Criterion> {
    let fallback = format!("{ms}/criterion[{idx}]");
    let Some(t) = v.as_table() else {
        cx.bad(&fallback, "criterion is not a table");
        return None;
    };
    let Some(id) = t.get("id").and_then(|v| v.as_str()).map(str::to_string) else {
        cx.bad(&fallback, "criterion lacks a string `id`");
        return None;
    };
    let note = match t.get("note") {
        None => None,
        Some(toml::Value::String(s)) => Some(s.clone()),
        Some(_) => {
            cx.bad(&id, "`note` is not a string");
            None
        }
    };
    let Some(kind) = t.get("kind").and_then(|v| v.as_str()) else {
        cx.bad(&id, "criterion lacks a string `kind`");
        return None;
    };
    let (allowed, parsed): (&[&str], Option<CriterionKind>) = match kind {
        "node_status" => {
            let node = nonempty_str(t.get("node")).filter(|n| is_node_id(n));
            let statuses = str_list(t.get("in")).filter(|l| l.iter().all(|s| NODE_STATUSES.contains(&s.as_str())));
            if node.is_none() {
                cx.bad(&id, "node_status needs `node`: a spec node ID");
            }
            if statuses.is_none() {
                cx.bad(&id, format!("node_status needs `in`: a non-empty list of {NODE_STATUSES:?}"));
            }
            (
                &["node", "in"][..],
                node.zip(statuses).map(|(node, statuses)| CriterionKind::NodeStatus { node, statuses }),
            )
        }
        "phase_done" => {
            let phase = t.get("phase").and_then(|v| v.as_str()).and_then(Phase::parse);
            let scope = str_list(t.get("scope")).filter(|l| l.iter().all(|s| is_node_id(s)));
            if phase.is_none() {
                cx.bad(&id, "phase_done needs `phase`: one of P0 P1a P1b P2 P3 P4 later");
            }
            if scope.is_none() {
                cx.bad(&id, "phase_done needs `scope`: a non-empty list of spec node IDs");
            }
            (&["phase", "scope"][..], phase.zip(scope).map(|(phase, scope)| CriterionKind::PhaseDone { phase, scope }))
        }
        "parity" => {
            let parsed = match (t.get("app"), t.get("min_pct"), t.get("seeds")) {
                (Some(app), Some(pct), None) => {
                    let pct = pct.as_float().or_else(|| pct.as_integer().map(|i| i as f64));
                    match (nonempty_str(Some(app)), pct.filter(|p| (0.0..=100.0).contains(p))) {
                        (Some(app), Some(min_pct)) => Some(CriterionKind::ParityApp { app, min_pct }),
                        _ => None,
                    }
                }
                (None, None, Some(_)) => str_list(t.get("seeds")).map(|seeds| CriterionKind::ParitySeeds { seeds }),
                _ => None,
            };
            if parsed.is_none() {
                cx.bad(&id, "parity needs either `app` with `min_pct` in 0..=100, or a non-empty `seeds` list");
            }
            (&["app", "min_pct", "seeds"][..], parsed)
        }
        "signoff" => {
            let role = nonempty_str(t.get("role"));
            if role.is_none() {
                cx.bad(&id, "signoff needs a non-empty `role`");
            }
            (&["role"][..], role.map(|role| CriterionKind::Signoff { role }))
        }
        other => {
            cx.findings.push(Finding::new(Code::UnknownKind, &id, format!("criterion kind `{other}` is not allowed")));
            return None;
        }
    };
    let mut all = vec!["id", "kind", "note"];
    all.extend_from_slice(allowed);
    cx.unknown_keys(&id, t, &all);
    parsed.map(|kind| Criterion { id, kind, note })
}

fn parse_milestone(cx: &mut Ctx, idx: usize, v: &toml::Value) -> Option<Milestone> {
    let fallback = format!("milestone[{idx}]");
    let Some(t) = v.as_table() else {
        cx.bad(&fallback, "milestone is not a table");
        return None;
    };
    let Some(id) = t.get("id").and_then(|v| v.as_str()).map(str::to_string) else {
        cx.bad(&fallback, "milestone lacks a string `id`");
        return None;
    };
    cx.unknown_keys(&id, t, &["id", "app", "level", "scope_label", "target_date", "announced", "criterion"]);
    let app = nonempty_str(t.get("app"));
    if app.is_none() {
        cx.bad(&id, "milestone lacks a string `app`");
    }
    let level = t.get("level").and_then(|v| v.as_str()).and_then(Level::parse);
    if level.is_none() {
        cx.bad(&id, "`level` must be alpha, beta or 1.0");
    }
    let scope_label = match t.get("scope_label") {
        None => None,
        Some(v) => {
            let s = nonempty_str(Some(v));
            if s.is_none() {
                cx.bad(&id, "`scope_label` must be a non-empty string");
            }
            s
        }
    };
    let target_date = match t.get("target_date") {
        None => None,
        Some(v) => {
            let d = date_value(v);
            if d.is_none() {
                cx.bad(&id, "`target_date` must be a YYYY-MM-DD date");
            }
            d
        }
    };
    let announced = match t.get("announced") {
        None => None,
        Some(v) => {
            let a = v.as_table();
            if let Some(a) = a {
                cx.unknown_keys(&id, a, &["date", "release"]);
            }
            let date = a.and_then(|a| a.get("date")).and_then(date_value);
            let release = nonempty_str(a.and_then(|a| a.get("release")));
            if date.is_none() || release.is_none() {
                cx.bad(&id, "`announced` needs `date` (YYYY-MM-DD) and a non-empty `release`");
            }
            date.zip(release).map(|(date, release)| Announced { date, release })
        }
    };
    let mut criteria = Vec::new();
    match t.get("criterion") {
        None => {}
        Some(toml::Value::Array(a)) => {
            for (i, c) in a.iter().enumerate() {
                criteria.extend(parse_criterion(cx, &id, i, c));
            }
        }
        Some(_) => cx.bad(&id, "`criterion` must be an array of tables"),
    }
    Some(Milestone { id, app: app?, level: level?, scope_label, target_date, announced, criteria })
}

/// Parses and validates the criteria file's schema (REQ-001, REQ-002, REQ-003). Milestones and
/// criteria that fail validation are left out of the result and reported as findings.
pub fn parse_criteria(t: &toml::Table) -> (Criteria, Vec<Finding>) {
    let mut cx = Ctx { findings: Vec::new() };
    cx.unknown_keys("file", t, &["version", "milestone"]);
    if t.get("version").and_then(|v| v.as_integer()) != Some(SCHEMA_VERSION) {
        cx.bad("file", format!("`version` must be {SCHEMA_VERSION}"));
    }
    let mut criteria = Criteria::default();
    match t.get("milestone") {
        Some(toml::Value::Array(a)) => {
            for (i, m) in a.iter().enumerate() {
                criteria.milestones.extend(parse_milestone(&mut cx, i, m));
            }
        }
        _ => cx.bad("file", "`milestone` must be an array of tables"),
    }
    // IDs: pattern and uniqueness, over every entry that carries a string id (REQ-003).
    let mut ms_seen = BTreeMap::<String, usize>::new();
    let mut cr_seen = BTreeMap::<String, usize>::new();
    for m in t.get("milestone").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|m| m.as_table()) {
        if let Some(id) = m.get("id").and_then(|v| v.as_str()) {
            *ms_seen.entry(id.to_string()).or_default() += 1;
        }
        let crits = m.get("criterion").and_then(|v| v.as_array()).into_iter().flatten();
        for id in crits.filter_map(|c| c.as_table()?.get("id")?.as_str()) {
            *cr_seen.entry(id.to_string()).or_default() += 1;
        }
    }
    for (id, n) in &ms_seen {
        if !is_milestone_id(id) {
            cx.findings.push(Finding::new(Code::BadId, id, "milestone ID must match [a-z0-9-]{3,32}"));
        }
        if *n > 1 {
            cx.findings.push(Finding::new(Code::DuplicateId, id, format!("milestone ID used {n} times")));
        }
    }
    for (id, n) in &cr_seen {
        if !is_criterion_id(id) {
            cx.findings.push(Finding::new(Code::BadId, id, "criterion ID must match [A-Z][A-Z0-9-]{2,31}"));
        }
        if *n > 1 {
            cx.findings.push(Finding::new(Code::DuplicateId, id, format!("criterion ID used {n} times")));
        }
    }
    (criteria, cx.findings)
}

// ------------------------------------------------------------------ rules

/// App and D-017 rules (REQ-004, REQ-005) plus app references.
pub fn check_rules(c: &Criteria, apps: &BTreeSet<String>) -> Vec<Finding> {
    let mut out = Vec::new();
    for m in &c.milestones {
        if !apps.contains(&m.app) {
            out.push(Finding::new(Code::UnknownApp, &m.id, format!("app `{}` is not in the app list", m.app)));
        }
        for cr in &m.criteria {
            if let CriterionKind::ParityApp { app, .. } = &cr.kind {
                if !apps.contains(app) {
                    out.push(Finding::new(Code::UnknownApp, &cr.id, format!("app `{app}` is not in the app list")));
                }
            }
        }
        match m.level {
            Level::One => {
                let detail = match core_phase(&m.app) {
                    None => Some(format!("app `{}` has no core phase in the D-017 rule table", m.app)),
                    Some(core) => {
                        let ok = m
                            .criteria
                            .iter()
                            .any(|cr| matches!(cr.kind, CriterionKind::PhaseDone { phase, .. } if phase >= core));
                        (!ok).then(|| format!("1.0 needs a phase_done criterion at {} or later", core.as_str()))
                    }
                };
                out.extend(detail.map(|d| Finding::new(Code::CorePhaseMissing, &m.id, d)));
            }
            Level::Beta if m.scope_label.is_none() => {
                out.push(Finding::new(Code::ScopeLabelMissing, &m.id, "a beta milestone needs a scope_label"));
            }
            _ => {}
        }
    }
    out
}

/// Every ISO 8601 calendar date (`YYYY-MM-DD`, not part of a longer digit run) in `text`, with
/// its 1-based line number.
pub fn dates_in(text: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let b = line.as_bytes();
        let mut i = 0;
        while i + 10 <= b.len() {
            let before_digit = i > 0 && b.get(i - 1).is_some_and(u8::is_ascii_digit);
            let after_digit = b.get(i + 10).is_some_and(u8::is_ascii_digit);
            let cand = line.get(i..i + 10).filter(|s| is_iso_date(s));
            match cand {
                Some(s) if !before_digit && !after_digit => {
                    out.push((u32::try_from(n + 1).unwrap_or(u32::MAX), s.to_string()));
                    i += 10;
                }
                _ => i += 1,
            }
        }
    }
    out
}

/// The date rule (REQ-006): every date in the plan document is a milestone date of the file.
pub fn check_plan_dates(c: &Criteria, plan_text: &str, plan_file: &str) -> Vec<Finding> {
    let allowed = c.dates();
    dates_in(plan_text)
        .into_iter()
        .filter(|(_, d)| !allowed.contains(d))
        .map(|(line, d)| Finding {
            code: Code::StrayDate,
            detail: format!("date {d} is not a target_date or announced.date in the criteria file"),
            id: d,
            file: Some(plan_file.to_string()),
            line: Some(line),
        })
        .collect()
}

/// Criteria of each milestone, keyed by criterion ID, without the free-text `note`. Read from
/// the raw table so a criterion is compared even if it does not validate.
fn raw_criteria(t: &toml::Table) -> BTreeMap<String, (bool, BTreeMap<String, toml::Table>)> {
    let mut out = BTreeMap::new();
    for m in t.get("milestone").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|m| m.as_table()) {
        let Some(id) = m.get("id").and_then(|v| v.as_str()) else { continue };
        if out.contains_key(id) {
            continue; // duplicate milestone: the first one counts (duplicate_id is reported separately)
        }
        let mut crits = BTreeMap::new();
        for c in m.get("criterion").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|c| c.as_table()) {
            if let Some(cid) = c.get("id").and_then(|v| v.as_str()) {
                let mut c = c.clone();
                c.remove("note");
                crits.entry(cid.to_string()).or_insert(c);
            }
        }
        out.insert(id.to_string(), (m.contains_key("announced"), crits));
    }
    out
}

/// The announced-criteria rule (REQ-007): criteria of a milestone announced in `base` that were
/// added, removed or changed need a review entry for that milestone listing their IDs.
pub fn check_announced(base: &toml::Table, new: &toml::Table, reviews: &[ReviewEntry]) -> Vec<Finding> {
    let base = raw_criteria(base);
    let new = raw_criteria(new);
    let empty = BTreeMap::new();
    let mut out = Vec::new();
    for (ms, (announced, old)) in &base {
        if !announced {
            continue;
        }
        let cur = new.get(ms).map_or(&empty, |(_, c)| c);
        let ids: BTreeSet<&String> = old.keys().chain(cur.keys()).collect();
        for id in ids {
            let what = match (old.get(id), cur.get(id)) {
                (Some(a), Some(b)) if a == b => continue,
                (Some(_), Some(_)) => "changed",
                (Some(_), None) => "removed",
                _ => "added",
            };
            let reviewed = reviews.iter().any(|r| r.milestone == *ms && r.changes.iter().any(|c| c == id));
            if !reviewed {
                out.push(Finding::new(
                    Code::AnnouncedCriteriaChanged,
                    id.as_str(),
                    format!("criterion {what} in announced milestone {ms} without a review entry listing it"),
                ));
            }
        }
    }
    out
}

// ------------------------------------------------------------------ the check

/// Paths of a `plan-check` run.
#[derive(Debug, Clone)]
pub struct Inputs<'a> {
    pub criteria: &'a Path,
    pub apps: &'a Path,
    pub plan: &'a Path,
    pub reviews: &'a Path,
    pub base: Option<&'a Path>,
}

/// The sorted findings of a `plan-check` run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    pub findings: Vec<Finding>,
}

impl Report {
    /// RFC 8785 canonical JSON: `{"findings":[...],"ok":bool}`.
    pub fn to_json(&self) -> Vec<u8> {
        let mut o = BTreeMap::new();
        o.insert("findings".to_string(), CValue::Arr(self.findings.iter().map(Finding::to_cvalue).collect()));
        o.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }

    /// One finding per line, then `<n> findings`.
    pub fn render(&self) -> String {
        let mut s: String = self.findings.iter().map(|f| format!("{f}\n")).collect();
        s.push_str(&format!("{} findings\n", self.findings.len()));
        s
    }
}

/// Runs every plan rule over the inputs (REQ-001 .. REQ-008).
pub fn check(inp: &Inputs<'_>) -> Result<Report, PlanError> {
    let crit_path = inp.criteria.display().to_string();
    let table = parse_toml(&read_text(inp.criteria)?, &crit_path)?;
    let apps = parse_apps(&read_text(inp.apps)?, &inp.apps.display().to_string())?;
    let plan_text = read_text(inp.plan)?;
    let reviews = read_reviews(inp.reviews)?;
    let base = match inp.base {
        Some(b) => Some(parse_toml(&read_text(b)?, &b.display().to_string())?),
        None => None,
    };

    let (criteria, mut findings) = parse_criteria(&table);
    findings.extend(check_rules(&criteria, &apps));
    findings.extend(check_plan_dates(&criteria, &plan_text, &inp.plan.display().to_string()));
    if let Some(base) = &base {
        findings.extend(check_announced(base, &table, &reviews));
    }
    findings
        .sort_by(|a, b| (a.code.as_str(), &a.id, a.line, &a.detail).cmp(&(b.code.as_str(), &b.id, b.line, &b.detail)));
    findings.dedup();
    Ok(Report { findings })
}
