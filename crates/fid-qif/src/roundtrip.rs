//! The QIF-XML → QIF-JSON → QIF-XML round-trip harness (REQ-IO.QIF.NORM-005, AC-6).
//!
//! The harness reads a corpus manifest in the `IO.CORP.MANIF` format (`[[entry]]` tables
//! with `path` or `url`, and `tags`), takes every entry tagged `p0-subset`, runs each
//! file through a [`Projection`] to JSON and back, and compares the result with the
//! original under qif-norm. The report is deterministic JSON with one row per file; the
//! gate's exit code is 0 only when every row is `equal` and there is at least one row.
//!
//! Until the QIF-JSON projection (`IO.QIF.JSON`) exists, [`StubProjection`] stands in: it
//! passes the bytes through unchanged, which exercises the harness and the comparator.
//! Fail-closed choices: an entry given only by `url` is reported as error `not_local`
//! (the harness never fetches); a `path` that is absolute or contains `..` is error
//! `path_outside_root`; an empty selection fails the gate.

use crate::norm::{compare_with, default_budget, Diff, Equality, NormError, NormRules};
use std::io::Read;
use std::path::{Component, Path};

/// The tag that selects P0-subset entries.
pub const P0_TAG: &str = "p0-subset";
/// Largest manifest the harness reads (1 MiB).
pub const MAX_MANIFEST_BYTES: u64 = 1 << 20;
/// Largest corpus file the harness reads (256 MiB) [unverified: tune on corpus].
pub const MAX_FILE_BYTES: u64 = 256 << 20;

/// Why a projection failed; the code is stable snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionError {
    /// The stable code.
    pub code: &'static str,
}

/// A QIF-XML ↔ QIF-JSON projection.
pub trait Projection {
    /// QIF-XML bytes to QIF-JSON bytes.
    fn xml_to_json(&self, xml: &[u8]) -> Result<Vec<u8>, ProjectionError>;
    /// QIF-JSON bytes to QIF-XML bytes.
    fn json_to_xml(&self, json: &[u8]) -> Result<Vec<u8>, ProjectionError>;
}

/// The stand-in projection: both directions return the input unchanged.
#[derive(Debug, Clone, Copy, Default)]
pub struct StubProjection;

impl Projection for StubProjection {
    fn xml_to_json(&self, xml: &[u8]) -> Result<Vec<u8>, ProjectionError> {
        Ok(xml.to_vec())
    }

    fn json_to_xml(&self, json: &[u8]) -> Result<Vec<u8>, ProjectionError> {
        Ok(json.to_vec())
    }
}

/// The outcome for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowStatus {
    /// Equal under qif-norm after the round trip.
    Equal,
    /// The first difference between the original (`a`) and the round-tripped file (`b`).
    Different(Diff),
    /// The file could not be read, projected or normalised; the stable code.
    Error(&'static str),
}

/// One report row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The entry's `path` (or `url`) as written in the manifest.
    pub file: String,
    /// The outcome.
    pub status: RowStatus,
}

/// The harness report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The qif-norm version used.
    pub norm: String,
    /// One row per selected entry, in manifest order.
    pub rows: Vec<Row>,
}

impl Report {
    /// Number of rows that are equal.
    pub fn equal_count(&self) -> usize {
        self.rows.iter().filter(|r| r.status == RowStatus::Equal).count()
    }

    /// True when there is at least one row and every row is equal.
    pub fn passed(&self) -> bool {
        !self.rows.is_empty() && self.equal_count() == self.rows.len()
    }

    /// The gate's exit code: 0 when [`Report::passed`], else 1.
    pub fn exit_code(&self) -> i32 {
        if self.passed() {
            0
        } else {
            1
        }
    }

    /// The report as deterministic JSON (keys in fixed order, one line, trailing LF).
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        s.push_str("{\"norm\":");
        json_str(&mut s, &self.norm);
        s.push_str(&format!(",\"total\":{},\"equal\":{},\"passed\":{}", self.rows.len(), self.equal_count(), self.passed()));
        s.push_str(",\"rows\":[");
        for (i, row) in self.rows.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"file\":");
            json_str(&mut s, &row.file);
            match &row.status {
                RowStatus::Equal => s.push_str(",\"status\":\"equal\""),
                RowStatus::Error(code) => {
                    s.push_str(",\"status\":\"error\",\"error\":");
                    json_str(&mut s, code);
                }
                RowStatus::Different(d) => {
                    s.push_str(",\"status\":\"different\",\"diff\":{\"path\":");
                    json_str(&mut s, &d.path.as_display().to_string());
                    s.push_str(",\"line_a\":");
                    json_opt_num(&mut s, d.line_a);
                    s.push_str(",\"line_b\":");
                    json_opt_num(&mut s, d.line_b);
                    s.push_str(",\"a\":");
                    json_opt_str(&mut s, d.a.as_ref().map(|t| t.as_display().to_string()));
                    s.push_str(",\"b\":");
                    json_opt_str(&mut s, d.b.as_ref().map(|t| t.as_display().to_string()));
                    s.push('}');
                }
            }
            s.push('}');
        }
        s.push_str("]}\n");
        s
    }
}

fn json_opt_num(s: &mut String, v: Option<u32>) {
    match v {
        Some(n) => s.push_str(&n.to_string()),
        None => s.push_str("null"),
    }
}

fn json_opt_str(s: &mut String, v: Option<String>) {
    match v {
        Some(t) => json_str(s, &t),
        None => s.push_str("null"),
    }
}

/// A JSON string: `"` and `\` escaped, control characters as `\u00XX`.
fn json_str(s: &mut String, v: &str) {
    s.push('"');
    for c in v.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)),
            c => s.push(c),
        }
    }
    s.push('"');
}

/// Runs the harness over the manifest at `manifest` with `projection` and the shipped
/// rules. Errors only for a missing or malformed manifest; per-file failures are rows.
pub fn run_manifest(manifest: &Path, projection: &dyn Projection) -> Result<Report, NormError> {
    let rules = NormRules::builtin()?;
    let text = read_capped(manifest, MAX_MANIFEST_BYTES).map_err(|code| NormError::Manifest {
        code: if code == "io" { "manifest_missing" } else { "malformed" },
        reason: "the manifest cannot be read",
    })?;
    let bad = |reason| NormError::Manifest { code: "malformed", reason };
    let text = String::from_utf8(text).map_err(|_| bad("the manifest is not UTF-8"))?;
    let table: toml::Table = text.parse().map_err(|_| bad("the manifest is not valid TOML"))?;
    let entries = match table.get("entry") {
        None => Vec::new(),
        Some(v) => v.as_array().ok_or(bad("`entry` is not an array of tables"))?.clone(),
    };
    let base = manifest.parent().unwrap_or(Path::new(""));
    let mut rows = Vec::new();
    for entry in &entries {
        let entry = entry.as_table().ok_or(bad("an entry is not a table"))?;
        let tags = match entry.get("tags") {
            None => Vec::new(),
            Some(v) => v.as_array().ok_or(bad("tags is not an array"))?.iter().filter_map(|t| t.as_str()).collect(),
        };
        if !tags.contains(&P0_TAG) {
            continue;
        }
        let path = entry.get("path").and_then(toml::Value::as_str);
        let url = entry.get("url").and_then(toml::Value::as_str);
        let row = match (path, url) {
            (Some(p), _) => Row { file: p.to_string(), status: run_file(base, p, projection, &rules) },
            (None, Some(u)) => Row { file: u.to_string(), status: RowStatus::Error("not_local") },
            (None, None) => return Err(bad("an entry has neither path nor url")),
        };
        rows.push(row);
    }
    Ok(Report { norm: rules.version().to_string(), rows })
}

fn run_file(base: &Path, rel: &str, projection: &dyn Projection, rules: &NormRules) -> RowStatus {
    let rel_path = Path::new(rel);
    if rel.is_empty() || !rel_path.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)) {
        return RowStatus::Error("path_outside_root");
    }
    let original = match read_capped(&base.join(rel_path), MAX_FILE_BYTES) {
        Ok(b) => b,
        Err(code) => return RowStatus::Error(code),
    };
    let back = match projection.xml_to_json(&original).and_then(|j| projection.json_to_xml(&j)) {
        Ok(b) => b,
        Err(e) => return RowStatus::Error(e.code),
    };
    let budgets = default_budget().and_then(|a| default_budget().map(|b| (a, b)));
    let result = budgets.and_then(|(ba, bb)| compare_with(&original, &back, ba, bb, rules));
    match result {
        Ok(Equality::Equal) => RowStatus::Equal,
        Ok(Equality::Different(d)) => RowStatus::Different(d),
        Err(e) => RowStatus::Error(e.code()),
    }
}

/// Reads at most `cap` bytes; `limit_exceeded` if the file is longer, `io` if it cannot
/// be read.
fn read_capped(path: &Path, cap: u64) -> Result<Vec<u8>, &'static str> {
    let file = std::fs::File::open(path).map_err(|_| "io")?;
    let mut out = Vec::new();
    file.take(cap.saturating_add(1)).read_to_end(&mut out).map_err(|_| "io")?;
    if out.len() as u64 > cap {
        return Err("limit_exceeded");
    }
    Ok(out)
}
