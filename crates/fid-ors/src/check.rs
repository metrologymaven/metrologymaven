//! The traceability checker behind `rdl.check_suite@1` (REQ-001 to REQ-007).
//!
//! Findings are data. The checker visits the manifest, then every case in manifest order,
//! then (with a previous release) the version rule, and runs the case checks in the pinned
//! order of `spec/unc/ors/trace/NODE.md`. A field path is built only from schema names and
//! indexes; no finding ever holds text taken from a case.
//!
//! The review check reads the *declared* review fields only. It never says that anything
//! is signed off: expert sign-off is the `GDT.RES.ROWFLOW` workflow and the status register
//! of `GDT.RES.STATUS` (D-009, D-045), and this checker neither reads nor claims it.

use crate::suite::{Suite, CASE_SCHEMA, MAX_TEXT_CHARS};
use fid_canon::{to_canonical_bytes, CValue};
use fid_gps::dmt::ident::is_label;
use fid_gps::dmt::row::is_reviewer_id;
use fid_gps::dmt::{DmtKey, DmtTable};
use fid_rdl::{is_control_or_format, licence_redistributable, RdlDataset, Tier};
use std::collections::{BTreeMap, BTreeSet};

/// Finding codes, in the order the leaf lists them.
pub const CODES: [&str; 8] = [
    "citation_missing",
    "dmt_row_unknown",
    "case_incomplete",
    "text_too_long",
    "review_missing",
    "licence_not_open",
    "version_not_bumped",
    "case_invalid",
];

/// Most citations in one case.
pub const MAX_CITATIONS: usize = 16;
/// Accepted units of `expected` and `acceptance_tolerance`.
pub const UNITS: [&str; 6] = ["m", "mm", "\u{00b5}m", "rad", "deg", "1"];
/// Accepted dialects.
pub const DIALECTS: [&str; 3] = ["ASME", "ISO_GPS", "none"];
/// Accepted review statuses.
pub const REVIEW_STATUSES: [&str; 3] = ["draft", "in_review", "reviewed"];

const CASE_KEYS: [&str; 11] = [
    "acceptance_tolerance",
    "basis",
    "case",
    "citations",
    "dataset",
    "dialect",
    "dmt_row",
    "expected",
    "note",
    "review",
    "schema",
];

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The case, or `None` for a manifest finding.
    pub case_id: Option<String>,
    /// One of [`CODES`].
    pub code: &'static str,
    /// The field path.
    pub field: String,
}

impl Finding {
    /// `{case, code, field}`.
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("case".to_string(), self.case_id.clone().map_or(CValue::Null, CValue::Str));
        m.insert("code".to_string(), CValue::Str(self.code.to_string()));
        m.insert("field".to_string(), CValue::Str(self.field.clone()));
        CValue::Obj(m)
    }
}

/// How each case is traced (REQ-001, AC-H2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Coverage {
    /// Cases with `citations: none` and `basis: closed_form_math`.
    pub closed_form_math: u64,
    /// Cases with valid citations and a DMT row present in the pinned DMT.
    pub dmt_row: u64,
    /// Every other case.
    pub neither: u64,
}

/// The result of one check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// The checked suite's handle.
    pub suite: String,
    /// The previous release's handle, if one was given.
    pub previous: Option<String>,
    /// Number of cases.
    pub cases: u64,
    /// Coverage counts.
    pub coverage: Coverage,
    /// Every finding, in the pinned order.
    pub findings: Vec<Finding>,
}

impl CheckReport {
    /// True when there is no finding.
    pub fn passed(&self) -> bool {
        self.findings.is_empty()
    }

    /// `{cases, coverage, findings, passed, previous, suite}`.
    pub fn to_value(&self) -> CValue {
        let mut cov = BTreeMap::new();
        cov.insert("closed_form_math".to_string(), CValue::U64(self.coverage.closed_form_math));
        cov.insert("dmt_row".to_string(), CValue::U64(self.coverage.dmt_row));
        cov.insert("neither".to_string(), CValue::U64(self.coverage.neither));
        let mut m = BTreeMap::new();
        m.insert("cases".to_string(), CValue::U64(self.cases));
        m.insert("coverage".to_string(), CValue::Obj(cov));
        m.insert("findings".to_string(), CValue::Arr(self.findings.iter().map(Finding::to_value).collect()));
        m.insert("passed".to_string(), CValue::Bool(self.passed()));
        m.insert("previous".to_string(), self.previous.clone().map_or(CValue::Null, CValue::Str));
        m.insert("suite".to_string(), CValue::Str(self.suite.clone()));
        CValue::Obj(m)
    }
}

type Map = BTreeMap<String, CValue>;

struct Out {
    list: Vec<(&'static str, String)>,
}

impl Out {
    fn push(&mut self, code: &'static str, field: impl Into<String>) {
        self.list.push((code, field.into()));
    }
}

fn too_long(s: &str) -> bool {
    s.chars().count() > MAX_TEXT_CHARS
}

/// `0x` and 16 lower-case hex digits of a finite binary64; with `nonneg`, the sign bit clear.
pub fn hex_ok(s: &str, nonneg: bool) -> bool {
    let Some(h) = s.strip_prefix("0x") else { return false };
    if h.len() != 16 || !h.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
        return false;
    }
    let Ok(bits) = u64::from_str_radix(h, 16) else { return false };
    (bits >> 52) & 0x7ff != 0x7ff && !(nonneg && bits >> 63 == 1)
}

fn clause_ok(s: &str) -> bool {
    s.len() <= 32
        && !s.is_empty()
        && s.split('.').all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
}

fn citation_ok(v: &CValue) -> bool {
    let CValue::Obj(o) = v else { return false };
    if o.len() != 3 {
        return false;
    }
    let s = |k: &str| match o.get(k) {
        Some(CValue::Str(s)) => Some(s.as_str()),
        _ => None,
    };
    matches!((s("designator"), s("edition"), s("clause")),
        (Some(d), Some(e), Some(c)) if is_label(d, 64) && is_label(e, 32) && clause_ok(c))
}

/// Reviewer IDs, no repeats (the grammar is lower case, so IDs never differ only in case).
fn ids(v: &CValue) -> Option<Vec<&str>> {
    let CValue::Arr(a) = v else { return None };
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(a.len());
    for x in a {
        let CValue::Str(s) = x else { return None };
        if !is_reviewer_id(s) || !seen.insert(s.as_str()) {
            return None;
        }
        out.push(s.as_str());
    }
    Some(out)
}

fn value_field(o: &Map, name: &str, out: &mut Out, nonneg: bool) {
    let Some(v) = o.get(name) else {
        out.push("case_incomplete", name);
        return;
    };
    let CValue::Obj(vo) = v else {
        out.push("case_invalid", name);
        return;
    };
    match vo.get("value_hex") {
        None => out.push("case_incomplete", format!("{name}.value_hex")),
        Some(CValue::Str(s)) if hex_ok(s, nonneg) => {}
        Some(_) => out.push("case_invalid", format!("{name}.value_hex")),
    }
    match vo.get("unit") {
        None => out.push("case_incomplete", format!("{name}.unit")),
        Some(CValue::Str(s)) if UNITS.contains(&s.as_str()) => {}
        Some(_) => out.push("case_invalid", format!("{name}.unit")),
    }
    if vo.keys().any(|k| k != "value_hex" && k != "unit") {
        out.push("case_invalid", name);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cov {
    Closed,
    Row,
    Neither,
}

fn dataset_checks(o: &Map, out: &mut Out) {
    let Some(d) = o.get("dataset") else {
        out.push("case_incomplete", "dataset");
        return;
    };
    let rec = match RdlDataset::from_value(d) {
        Ok(r) => r,
        Err(e) => {
            out.push("case_invalid", format!("dataset.{}", e.field()));
            return;
        }
    };
    if rec.tier() != Tier::O {
        out.push("case_invalid", "dataset.tier");
        return;
    }
    if !licence_redistributable(rec.licence()) {
        out.push("licence_not_open", "dataset.licence");
    }
    let p = rec.provenance();
    if too_long(&p.source) {
        out.push("text_too_long", "dataset.provenance.source");
    }
    if p.method.as_deref().is_some_and(too_long) {
        out.push("text_too_long", "dataset.provenance.method");
    }
}

/// The findings and coverage class of one case document.
fn check_case(c: &CValue, rows: Option<&DmtTable>) -> (Vec<(&'static str, String)>, Cov) {
    let mut out = Out { list: Vec::new() };
    let CValue::Obj(o) = c else {
        return (vec![("case_invalid", "case".to_string())], Cov::Neither);
    };
    if !matches!(o.get("schema"), Some(CValue::Str(s)) if s == CASE_SCHEMA) {
        return (vec![("case_invalid", "schema".to_string())], Cov::Neither);
    }
    // a. dataset
    dataset_checks(o, &mut out);
    // b. citations and basis
    let (mut cited, mut closed) = (false, false);
    match o.get("citations") {
        None => out.push("citation_missing", "citations"),
        Some(CValue::Str(s)) if s == "none" => {
            if matches!(o.get("basis"), Some(CValue::Str(b)) if b == "closed_form_math") {
                closed = true;
            } else {
                out.push("citation_missing", "basis");
            }
        }
        Some(CValue::Arr(cs)) => {
            if cs.is_empty() {
                out.push("citation_missing", "citations");
            } else if cs.len() > MAX_CITATIONS {
                out.push("case_invalid", "citations");
            } else if let Some(i) = cs.iter().position(|x| !citation_ok(x)) {
                out.push("case_invalid", format!("citations.{i}"));
            } else {
                cited = true;
            }
            if o.contains_key("basis") {
                out.push("case_invalid", "basis");
            }
        }
        Some(_) => out.push("case_invalid", "citations"),
    }
    // c. DMT row
    let has_row = o.contains_key("dmt_row");
    let mut resolved = false;
    if let Some(r) = o.get("dmt_row") {
        match r {
            CValue::Str(s) if s.len() <= 128 && DmtKey::parse(s).is_ok() => {
                if rows.is_some_and(|t| t.get(s).is_some()) {
                    resolved = true;
                } else {
                    out.push("dmt_row_unknown", "dmt_row");
                }
            }
            _ => out.push("case_invalid", "dmt_row"),
        }
    }
    // d. dialect
    match o.get("dialect") {
        None => out.push("case_incomplete", "dialect"),
        Some(CValue::Str(s)) if DIALECTS.contains(&s.as_str()) => {}
        Some(_) => out.push("case_invalid", "dialect"),
    }
    // e, f. expected result and acceptance tolerance
    value_field(o, "expected", &mut out, false);
    value_field(o, "acceptance_tolerance", &mut out, true);
    // g. review
    match o.get("review") {
        None => out.push("case_incomplete", "review"),
        Some(CValue::Obj(r)) => {
            let before = out.list.len();
            let authors = match r.get("authors") {
                None => {
                    out.push("case_incomplete", "review.authors");
                    None
                }
                Some(CValue::Arr(a)) if a.is_empty() => {
                    out.push("case_incomplete", "review.authors");
                    None
                }
                Some(v) => {
                    let ids = ids(v);
                    if ids.is_none() {
                        out.push("case_invalid", "review.authors");
                    }
                    ids
                }
            };
            let reviewers = match r.get("reviewers") {
                None => {
                    out.push("case_incomplete", "review.reviewers");
                    None
                }
                Some(v) => {
                    let ids = ids(v);
                    if ids.is_none() {
                        out.push("case_invalid", "review.reviewers");
                    }
                    ids
                }
            };
            let status = match r.get("status") {
                None => {
                    out.push("case_incomplete", "review.status");
                    None
                }
                Some(CValue::Str(s)) if REVIEW_STATUSES.contains(&s.as_str()) => Some(s.as_str()),
                Some(_) => {
                    out.push("case_invalid", "review.status");
                    None
                }
            };
            if r.keys().any(|k| !matches!(k.as_str(), "authors" | "reviewers" | "status")) {
                out.push("case_invalid", "review");
            }
            if let (true, true, Some(a), Some(rs), Some(st)) =
                (has_row, out.list.len() == before, authors, reviewers, status)
            {
                if st != "reviewed" {
                    out.push("review_missing", "review.status");
                } else if rs.iter().any(|x| a.contains(x)) || rs.len() < 2 {
                    out.push("review_missing", "review.reviewers");
                }
            }
        }
        Some(_) => out.push("case_invalid", "review"),
    }
    // h. note
    if let Some(n) = o.get("note") {
        match n {
            CValue::Str(s) if too_long(s) => out.push("text_too_long", "note"),
            CValue::Str(s) if s.chars().any(is_control_or_format) => out.push("case_invalid", "note"),
            CValue::Str(_) => {}
            _ => out.push("case_invalid", "note"),
        }
    }
    // i. unknown fields
    if o.keys().any(|k| !CASE_KEYS.contains(&k.as_str())) {
        out.push("case_invalid", "case");
    }
    let cov = if closed {
        Cov::Closed
    } else if cited && resolved {
        Cov::Row
    } else {
        Cov::Neither
    };
    (out.list, cov)
}

fn canon(v: Option<&CValue>) -> Option<Vec<u8>> {
    v.and_then(|x| to_canonical_bytes(x).ok())
}

fn field_of<'a>(c: Option<&'a CValue>, key: &str) -> Option<&'a CValue> {
    match c {
        Some(CValue::Obj(o)) => o.get(key),
        _ => None,
    }
}

/// Checks `suite` against the DMT table `dmt` (used only if its digest is the one the
/// manifest pins) and, if given, the previous release `previous` (same suite name, else
/// `suite_invalid{previous}`). See the module documentation for the order.
pub fn check_suite(
    suite: &Suite,
    dmt: Option<&DmtTable>,
    previous: Option<&Suite>,
) -> Result<CheckReport, crate::OrsError> {
    let m = suite.manifest();
    if let Some(p) = previous {
        if p.manifest().name != m.name {
            return Err(crate::suite::invalid("previous", "previous release of another suite"));
        }
    }
    let rows = dmt.filter(|t| t.digest() == m.dmt_digest);
    let mut findings = Vec::new();
    let suite_level = |code: &'static str, field: String| Finding { case_id: None, code, field };
    if !licence_redistributable(&m.licence) {
        findings.push(suite_level("licence_not_open", "licence".to_string()));
    }
    for (i, e) in m.changelog.iter().enumerate() {
        if too_long(&e.reason) {
            findings.push(suite_level("text_too_long", format!("changelog.{i}.reason")));
        }
    }
    let mut coverage = Coverage::default();
    for id in &m.cases {
        let Some(c) = suite.case(id) else { return Err(crate::suite::invalid("cases", "case document missing")) };
        let (fs, cov) = check_case(c, rows);
        match cov {
            Cov::Closed => coverage.closed_form_math += 1,
            Cov::Row => coverage.dmt_row += 1,
            Cov::Neither => coverage.neither += 1,
        }
        findings.extend(fs.into_iter().map(|(code, field)| Finding { case_id: Some(id.clone()), code, field }));
    }
    if let Some(p) = previous {
        let pm = p.manifest();
        for id in &m.cases {
            let Some(prev) = p.case(id) else { continue };
            let cur = suite.case(id);
            let same = ["expected", "acceptance_tolerance"]
                .iter()
                .all(|k| canon(field_of(cur, k)) == canon(field_of(Some(prev), k)));
            if same {
                continue;
            }
            let field = if m.version <= pm.version {
                Some("version")
            } else if !m
                .changelog
                .iter()
                .any(|e| e.version == m.version && e.cases.contains(id) && !e.reason.is_empty())
            {
                Some("changelog")
            } else {
                None
            };
            if let Some(f) = field {
                findings.push(Finding { case_id: Some(id.clone()), code: "version_not_bumped", field: f.to_string() });
            }
        }
    }
    Ok(CheckReport {
        suite: m.handle(),
        previous: previous.map(Suite::handle),
        cases: m.cases.len() as u64,
        coverage,
        findings,
    })
}
