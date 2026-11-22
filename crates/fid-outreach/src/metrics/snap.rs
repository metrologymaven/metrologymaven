//! Metric snapshot store and quarterly values (spec node `OUTR.COMM.METRIC.SNAP`).
//!
//! Append-only storage for raw public-signal snapshots (`snapshots/<metric-id>/<timestamp>.json`
//! and `.raw`) and the pure computation (`metric-quarter`) that turns a quarter's snapshots into
//! one verified value per metric, writing deterministic canonical JSON (`<YYYY>Q<n>.json`) and a
//! Markdown summary (`<YYYY>Q<n>.md`).
//!
//! Re-verification (REQ-OUTR.COMM.METRIC.SNAP-003): every snapshot in the requested quarter has
//! its raw file's SHA-256 checked and its value re-extracted (RFC 6901 JSON Pointer for
//! `http_json`; stored count verified against the raw payload when structured for `git_history`
//! and `manual_public_count`). Any mismatch fails the run with `snapshot_mismatch` and writes
//! nothing.
//!
//! Determinism D0: no clock reads (the quarter is an argument), no network, `BTreeMap` /
//! `BTreeSet` everywhere, exact `u64` arithmetic with checked overflow, and RFC 8785 canonical
//! JSON output.

#![deny(clippy::todo, clippy::unimplemented, clippy::unreachable)]

use crate::metrics::def::{self, Aggregation, DefError, Method, Metric};
use fid_canon::{to_canonical_bytes, CValue};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};

/// Schema identifier written into quarterly value files.
pub const SCHEMA: &str = "metric-quarter@1";
/// Largest accepted snapshot record `.json` file, in bytes.
pub const MAX_RECORD_BYTES: usize = 64 * 1024;
/// Largest accepted raw response `.raw` file, in bytes (matches `OUTR.COMM.METRIC.COLLECT` 5 MB cap).
pub const MAX_RAW_BYTES: usize = 5 * 1024 * 1024;
/// Deepest accepted JSON nesting when parsing snapshot records or raw JSON responses.
pub const MAX_JSON_DEPTH: usize = 128;
/// Largest number of JSON elements accepted in one document.
pub const MAX_JSON_ELEMENTS: usize = 1_000_000;
/// Largest number of snapshot files read for a single metric.
pub const MAX_SNAPSHOTS_PER_METRIC: usize = 100_000;
/// Largest integer represented exactly as an IEEE 754 binary64 number (2^53 - 1).
const MAX_SAFE_INT: u64 = (1u64 << 53) - 1;

const RECORD_KEYS: [&str; 8] =
    ["collected_at", "definition_sha256", "metric_id", "raw_file", "raw_sha256", "source_url", "unit", "value"];

// ------------------------------------------------------------------ errors

/// Errors returned by [`SnapshotStore`] and `metric-quarter`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapError {
    /// Raw file missing, raw SHA-256 mismatch, metric ID mismatch, or re-extracted value differs (exit 1).
    SnapshotMismatch { snapshot: String, detail: String },
    /// Append attempted for a snapshot whose record or raw file already exists (exit 1).
    SnapshotExists { path: String },
    /// `u64` overflow while summing snapshot values (exit 1).
    Overflow { metric_id: String, snapshot: String },
    /// `--quarter` is not a valid `YYYYQn` calendar quarter (exit 2).
    QuarterInvalid { value: String },
    /// An input file or store directory could not be read (exit 2).
    InputUnreadable { path: String, detail: String },
    /// Definitions TOML failed to parse or validate (exit 2).
    InputParse { path: String, detail: String },
    /// A snapshot `.json` record is malformed or fails field validation (exit 2).
    RecordInvalid { path: String, detail: String },
    /// Output directory or files could not be written (exit 2).
    OutputUnwritable { path: String, detail: String },
}

impl SnapError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            SnapError::SnapshotMismatch { .. } => "snapshot_mismatch",
            SnapError::SnapshotExists { .. } => "snapshot_exists",
            SnapError::Overflow { .. } => "sum_overflow",
            SnapError::QuarterInvalid { .. } => "quarter_invalid",
            SnapError::InputUnreadable { .. } => "input_unreadable",
            SnapError::InputParse { .. } => "input_parse",
            SnapError::RecordInvalid { .. } => "record_invalid",
            SnapError::OutputUnwritable { .. } => "output_unwritable",
        }
    }

    /// Exit code for CLI commands: 1 for data/verification failures, 2 for bad usage/unreadable input.
    pub fn exit_code(&self) -> u8 {
        match self {
            SnapError::SnapshotMismatch { .. } | SnapError::SnapshotExists { .. } | SnapError::Overflow { .. } => 1,
            SnapError::QuarterInvalid { .. }
            | SnapError::InputUnreadable { .. }
            | SnapError::InputParse { .. }
            | SnapError::RecordInvalid { .. }
            | SnapError::OutputUnwritable { .. } => 2,
        }
    }

    /// Canonical JSON representation for `--json` mode.
    pub fn to_json(&self) -> Vec<u8> {
        let (subject, detail) = match self {
            SnapError::SnapshotMismatch { snapshot, detail } => (snapshot.clone(), detail.clone()),
            SnapError::SnapshotExists { path } => (path.clone(), "snapshot already exists".to_string()),
            SnapError::Overflow { metric_id, snapshot } => {
                (snapshot.clone(), format!("u64 sum overflow for metric {metric_id}"))
            }
            SnapError::QuarterInvalid { value } => {
                (value.clone(), "expected calendar quarter in YYYYQn format (Q1..Q4)".to_string())
            }
            SnapError::InputUnreadable { path, detail }
            | SnapError::InputParse { path, detail }
            | SnapError::RecordInvalid { path, detail }
            | SnapError::OutputUnwritable { path, detail } => (path.clone(), detail.clone()),
        };
        let err = obj([("code", s(self.code())), ("detail", s(&detail)), ("subject", s(&subject))]);
        canonical(&obj([("error", err), ("status", s("error"))]))
    }
}

impl fmt::Display for SnapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapError::SnapshotMismatch { snapshot, detail } => {
                write!(f, "snapshot_mismatch {snapshot}: {detail}")
            }
            SnapError::SnapshotExists { path } => write!(f, "snapshot_exists {path}"),
            SnapError::Overflow { metric_id, snapshot } => {
                write!(f, "sum_overflow {snapshot}: u64 overflow while summing metric {metric_id}")
            }
            SnapError::QuarterInvalid { value } => {
                write!(f, "quarter_invalid {value:?}: expected YYYYQn with n in 1..=4")
            }
            SnapError::InputUnreadable { path, detail } => write!(f, "input_unreadable {path}: {detail}"),
            SnapError::InputParse { path, detail } => write!(f, "input_parse {path}: {detail}"),
            SnapError::RecordInvalid { path, detail } => write!(f, "record_invalid {path}: {detail}"),
            SnapError::OutputUnwritable { path, detail } => write!(f, "output_unwritable {path}: {detail}"),
        }
    }
}

impl std::error::Error for SnapError {}

impl From<DefError> for SnapError {
    fn from(e: DefError) -> SnapError {
        match e {
            DefError::Io { file, message } => SnapError::InputUnreadable { path: file, detail: message },
            DefError::Parse { file, message } => SnapError::InputParse { path: file, detail: message },
        }
    }
}

// ------------------------------------------------------------------ digests

/// SHA-256 digest of raw bytes in the `sha256:<64 hex>` wire form.
pub fn sha256(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}

/// Deterministic SHA-256 digest of a single metric definition's canonical JSON form.
///
/// Reuses the exact `CValue` representation from [`crate::metrics::def`] so edits to unrelated
/// metrics or comments in `definitions.toml` do not trigger `definition_changed`.
pub fn definition_digest(metric: &Metric) -> Digest {
    let bytes = canonical(&def::metric_to_cvalue(metric));
    sha256(&bytes)
}

// ------------------------------------------------------------------ dates & quarters

/// A calendar quarter `YYYYQn` (year `0001..=9999`, `q` in `1..=4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Quarter {
    pub year: u32,
    pub q: u32,
}

impl Quarter {
    /// Parses `YYYYQn` (e.g. `2027Q1`).
    pub fn parse(input: &str) -> Result<Quarter, SnapError> {
        let b = input.as_bytes();
        let bad = || SnapError::QuarterInvalid { value: input.to_string() };
        if b.len() != 6 || b.get(4) != Some(&b'Q') {
            return Err(bad());
        }
        let year = parse_digits(b, 0, 4).ok_or_else(bad)?;
        let q = parse_digits(b, 5, 1).ok_or_else(bad)?;
        if year == 0 || !(1..=4).contains(&q) {
            return Err(bad());
        }
        Ok(Quarter { year, q })
    }
}

impl fmt::Display for Quarter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}Q{}", self.year, self.q)
    }
}

/// A validated RFC 3339 UTC timestamp `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Because every timestamp has fixed width 20 in UTC, lexicographical comparison of the RFC 3339
/// string matches chronological order exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UtcStamp {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

fn days_in_month(year: u32, month: u32) -> u32 {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    }
}

fn parse_digits(b: &[u8], from: usize, len: usize) -> Option<u32> {
    let part = b.get(from..from + len)?;
    if !part.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(part.iter().fold(0u32, |acc, &c| acc * 10 + u32::from(c - b'0')))
}

impl UtcStamp {
    /// Parses `YYYY-MM-DDTHH:MM:SSZ` strictly.
    pub fn parse(input: &str) -> Option<UtcStamp> {
        let b = input.as_bytes();
        if b.len() != 20
            || b.get(4) != Some(&b'-')
            || b.get(7) != Some(&b'-')
            || b.get(10) != Some(&b'T')
            || b.get(13) != Some(&b':')
            || b.get(16) != Some(&b':')
            || b.get(19) != Some(&b'Z')
        {
            return None;
        }
        let year = parse_digits(b, 0, 4)?;
        let month = parse_digits(b, 5, 2)?;
        let day = parse_digits(b, 8, 2)?;
        let hour = parse_digits(b, 11, 2)?;
        let minute = parse_digits(b, 14, 2)?;
        let second = parse_digits(b, 17, 2)?;
        if year == 0 || day == 0 || day > days_in_month(year, month) || hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        Some(UtcStamp { year, month, day, hour, minute, second })
    }

    /// Calendar quarter containing this timestamp.
    pub fn quarter(self) -> Quarter {
        Quarter { year: self.year, q: (self.month - 1) / 3 + 1 }
    }

    /// Canonical RFC 3339 UTC string `YYYY-MM-DDTHH:MM:SSZ`.
    pub fn as_rfc3339(self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// Cross-platform filename stem `YYYY-MM-DDTHH-MM-SSZ` (colon-free for Windows compatibility
    /// while preserving exact chronological sort order).
    pub fn filename_stem(self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}-{:02}-{:02}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

// ------------------------------------------------------------------ snapshot record & store

/// One stored metric snapshot record (REQ-OUTR.COMM.METRIC.SNAP-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRecord {
    pub metric_id: String,
    pub definition_sha256: Digest,
    /// RFC 3339 UTC timestamp (`YYYY-MM-DDTHH:MM:SSZ`).
    pub collected_at: String,
    pub source_url: String,
    /// Raw response file name inside `snapshots/<metric_id>/`.
    pub raw_file: String,
    pub raw_sha256: Digest,
    pub value: u64,
    pub unit: String,
}

impl SnapshotRecord {
    /// Builds a validated [`SnapshotRecord`] for `metric` and `raw_bytes`, computing
    /// `definition_sha256`, `raw_file` and `raw_sha256`.
    pub fn build(
        metric: &Metric,
        collected_at: &str,
        source_url: impl Into<String>,
        value: u64,
        raw_bytes: &[u8],
    ) -> Result<SnapshotRecord, SnapError> {
        let stamp = UtcStamp::parse(collected_at).ok_or_else(|| SnapError::RecordInvalid {
            path: metric.id.clone(),
            detail: format!("collected_at is not valid RFC 3339 UTC: {collected_at:?}"),
        })?;
        let source_url = source_url.into();
        if source_url.trim().is_empty() {
            return Err(SnapError::RecordInvalid {
                path: metric.id.clone(),
                detail: "source_url must not be empty".to_string(),
            });
        }
        Ok(SnapshotRecord {
            metric_id: metric.id.clone(),
            definition_sha256: definition_digest(metric),
            collected_at: stamp.as_rfc3339(),
            source_url,
            raw_file: format!("{}.raw", stamp.filename_stem()),
            raw_sha256: sha256(raw_bytes),
            value,
            unit: metric.unit.clone(),
        })
    }

    /// Serialises the record to RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Vec<u8> {
        canonical(&obj([
            ("collected_at", s(&self.collected_at)),
            ("definition_sha256", s(self.definition_sha256.as_str())),
            ("metric_id", s(&self.metric_id)),
            ("raw_file", s(&self.raw_file)),
            ("raw_sha256", s(self.raw_sha256.as_str())),
            ("source_url", s(&self.source_url)),
            ("unit", s(&self.unit)),
            ("value", u64_cvalue(self.value)),
        ]))
    }

    /// Parses and validates a snapshot record JSON document.
    pub fn parse_json(bytes: &[u8], path: &str) -> Result<SnapshotRecord, SnapError> {
        let err = |detail: String| SnapError::RecordInvalid { path: path.to_string(), detail };
        let val = parse_raw_json(bytes, MAX_RECORD_BYTES).map_err(err)?;
        let RawJson::Obj(map) = val else {
            return Err(err("expected a JSON object".to_string()));
        };
        let allowed: BTreeSet<&str> = RECORD_KEYS.into_iter().collect();
        for key in map.keys() {
            if !allowed.contains(key.as_str()) {
                return Err(err(format!("unexpected field {key:?}")));
            }
        }
        let req_str = |k: &str| -> Result<String, SnapError> {
            match map.get(k) {
                Some(RawJson::Str(v)) if !v.trim().is_empty() => Ok(v.clone()),
                Some(RawJson::Str(_)) => Err(err(format!("field {k:?} must not be empty"))),
                Some(_) => Err(err(format!("field {k:?} must be a string"))),
                None => Err(err(format!("missing field {k:?}"))),
            }
        };
        let metric_id = req_str("metric_id")?;
        if !def::valid_id(&metric_id) {
            return Err(err(format!("invalid metric_id {metric_id:?}")));
        }
        let def_str = req_str("definition_sha256")?;
        let definition_sha256 =
            Digest::parse(&def_str).map_err(|_| err(format!("invalid definition_sha256 {def_str:?}")))?;
        let collected_at = req_str("collected_at")?;
        if UtcStamp::parse(&collected_at).is_none() {
            return Err(err(format!("invalid RFC 3339 UTC collected_at {collected_at:?}")));
        }
        let source_url = req_str("source_url")?;
        let raw_file = req_str("raw_file")?;
        if !is_safe_filename(&raw_file) {
            return Err(err(format!("unsafe raw_file name {raw_file:?}")));
        }
        let raw_str = req_str("raw_sha256")?;
        let raw_sha256 = Digest::parse(&raw_str).map_err(|_| err(format!("invalid raw_sha256 {raw_str:?}")))?;
        let unit = req_str("unit")?;
        let value = match map.get("value") {
            Some(RawJson::UInt(v)) => *v,
            Some(RawJson::Str(v)) => parse_strict_u64_str(v)
                .ok_or_else(|| err(format!("field \"value\" must be a non-negative integer, got {v:?}")))?,
            Some(_) => return Err(err("field \"value\" must be a non-negative integer".to_string())),
            None => return Err(err("missing field \"value\"".to_string())),
        };
        Ok(SnapshotRecord { metric_id, definition_sha256, collected_at, source_url, raw_file, raw_sha256, value, unit })
    }
}

/// Paths and record digest produced by [`SnapshotStore::append`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendedSnapshot {
    pub record_path: PathBuf,
    pub raw_path: PathBuf,
    pub record_sha256: Digest,
}

/// Append-only store of metric snapshots (`snapshots/<metric-id>/<timestamp>.json` + `.raw`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotStore {
    root: PathBuf,
}

impl SnapshotStore {
    /// Opens (or points at) a snapshot store rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> SnapshotStore {
        SnapshotStore { root: root.into() }
    }

    /// Root folder of the store.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `snapshots/` folder inside the store.
    pub fn snapshots_dir(&self) -> PathBuf {
        self.root.join("snapshots")
    }

    /// `snapshots/<metric_id>/` folder inside the store.
    pub fn metric_dir(&self, metric_id: &str) -> PathBuf {
        self.snapshots_dir().join(metric_id)
    }

    /// Appends one snapshot record and its raw response bytes to the store (REQ-OUTR.COMM.METRIC.SNAP-005).
    ///
    /// Fails with [`SnapError::SnapshotExists`] if either the `.json` record or `.raw` file already
    /// exists; fails with [`SnapError::SnapshotMismatch`] if `record.raw_sha256` does not match
    /// `sha256(raw_bytes)`.
    pub fn append(&self, record: &SnapshotRecord, raw_bytes: &[u8]) -> Result<AppendedSnapshot, SnapError> {
        if !def::valid_id(&record.metric_id) {
            return Err(SnapError::RecordInvalid {
                path: record.metric_id.clone(),
                detail: format!("invalid metric_id {:?}", record.metric_id),
            });
        }
        let stamp = UtcStamp::parse(&record.collected_at).ok_or_else(|| SnapError::RecordInvalid {
            path: record.metric_id.clone(),
            detail: format!("invalid RFC 3339 UTC collected_at {:?}", record.collected_at),
        })?;
        if record.source_url.trim().is_empty() {
            return Err(SnapError::RecordInvalid {
                path: record.metric_id.clone(),
                detail: "source_url must not be empty".to_string(),
            });
        }
        if record.unit.trim().is_empty() {
            return Err(SnapError::RecordInvalid {
                path: record.metric_id.clone(),
                detail: "unit must not be empty".to_string(),
            });
        }
        if raw_bytes.len() > MAX_RAW_BYTES {
            return Err(SnapError::RecordInvalid {
                path: record.raw_file.clone(),
                detail: format!("raw response larger than {MAX_RAW_BYTES} bytes"),
            });
        }
        let stem = stamp.filename_stem();
        let expected_raw = format!("{stem}.raw");
        if record.raw_file != expected_raw {
            return Err(SnapError::RecordInvalid {
                path: record.raw_file.clone(),
                detail: format!("raw_file must be {expected_raw:?} for collected_at {}", record.collected_at),
            });
        }
        let actual_raw_sha = sha256(raw_bytes);
        if record.raw_sha256 != actual_raw_sha {
            return Err(SnapError::SnapshotMismatch {
                snapshot: format!("{}/{stem}.json", record.metric_id),
                detail: format!("raw_sha256 {} does not match raw bytes {}", record.raw_sha256, actual_raw_sha),
            });
        }

        let dir = self.metric_dir(&record.metric_id);
        fs::create_dir_all(&dir)
            .map_err(|e| SnapError::OutputUnwritable { path: dir.display().to_string(), detail: e.to_string() })?;

        let record_path = dir.join(format!("{stem}.json"));
        let raw_path = dir.join(&expected_raw);

        // Fail before touching the filesystem if either target already exists.
        if record_path.exists() {
            return Err(SnapError::SnapshotExists { path: record_path.display().to_string() });
        }
        if raw_path.exists() {
            return Err(SnapError::SnapshotExists { path: raw_path.display().to_string() });
        }

        let json_bytes = record.to_json();
        let record_sha256 = sha256(&json_bytes);

        write_create_new(&raw_path, raw_bytes)?;
        if let Err(e) = write_create_new(&record_path, &json_bytes) {
            let _ = fs::remove_file(&raw_path);
            return Err(e);
        }

        Ok(AppendedSnapshot { record_path, raw_path, record_sha256 })
    }
}

fn write_create_new(path: &Path, bytes: &[u8]) -> Result<(), SnapError> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| {
        if e.kind() == ErrorKind::AlreadyExists {
            SnapError::SnapshotExists { path: path.display().to_string() }
        } else {
            SnapError::OutputUnwritable { path: path.display().to_string(), detail: e.to_string() }
        }
    })?;
    if let Err(e) = file.write_all(bytes).and_then(|()| file.flush()) {
        let _ = fs::remove_file(path);
        return Err(SnapError::OutputUnwritable { path: path.display().to_string(), detail: e.to_string() });
    }
    Ok(())
}

// ------------------------------------------------------------------ quarterly report

/// Per-metric quarterly status and aggregated value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricValue {
    Available { value: u64, snapshot_count: usize, snapshot_digests: Vec<Digest>, definition_changed: bool },
    Unavailable { reason: &'static str },
}

/// One metric's quarterly entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarterMetric {
    pub id: String,
    pub unit: String,
    pub aggregation: Aggregation,
    pub definition_sha256: Digest,
    pub value: MetricValue,
}

/// Non-fatal warnings recorded during quarterly computation (value is kept, flagged in output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    DefinitionChanged { metric_id: String, snapshot: String, expected_sha256: Digest, found_sha256: Digest },
}

impl Warning {
    pub fn code(&self) -> &'static str {
        match self {
            Warning::DefinitionChanged { .. } => "definition_changed",
        }
    }

    fn to_cvalue(&self) -> CValue {
        match self {
            Warning::DefinitionChanged { metric_id, snapshot, expected_sha256, found_sha256 } => obj([
                ("code", s(self.code())),
                ("expected_sha256", s(expected_sha256.as_str())),
                ("found_sha256", s(found_sha256.as_str())),
                ("metric_id", s(metric_id)),
                ("snapshot", s(snapshot)),
            ]),
        }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::DefinitionChanged { metric_id, snapshot, expected_sha256, found_sha256 } => write!(
                f,
                "definition_changed {metric_id} {snapshot} (expected {expected_sha256}, found {found_sha256})"
            ),
        }
    }
}

/// Computed quarterly values for every metric in `defs.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarterReport {
    pub quarter: Quarter,
    pub definitions_sha256: Digest,
    pub metrics: Vec<QuarterMetric>,
    pub warnings: Vec<Warning>,
}

impl QuarterReport {
    /// RFC 8785 canonical JSON (`<YYYY>Q<n>.json`).
    pub fn to_json(&self) -> Vec<u8> {
        let metrics: Vec<CValue> = self
            .metrics
            .iter()
            .map(|m| match &m.value {
                MetricValue::Available { value, snapshot_count, snapshot_digests, definition_changed } => obj([
                    ("aggregation", s(m.aggregation.as_str())),
                    ("definition_changed", CValue::Bool(*definition_changed)),
                    ("definition_sha256", s(m.definition_sha256.as_str())),
                    ("id", s(&m.id)),
                    ("snapshot_count", u64_cvalue(*snapshot_count as u64)),
                    ("snapshot_digests", CValue::Arr(snapshot_digests.iter().map(|d| s(d.as_str())).collect())),
                    ("status", s("available")),
                    ("unit", s(&m.unit)),
                    ("value", u64_cvalue(*value)),
                ]),
                MetricValue::Unavailable { reason } => obj([
                    ("aggregation", s(m.aggregation.as_str())),
                    ("definition_changed", CValue::Bool(false)),
                    ("definition_sha256", s(m.definition_sha256.as_str())),
                    ("id", s(&m.id)),
                    ("reason", s(reason)),
                    ("snapshot_count", CValue::Num(0.0)),
                    ("snapshot_digests", CValue::Arr(Vec::new())),
                    ("status", s("unavailable")),
                    ("unit", s(&m.unit)),
                ]),
            })
            .collect();
        let warnings: Vec<CValue> = self.warnings.iter().map(Warning::to_cvalue).collect();
        canonical(&obj([
            ("definitions_sha256", s(self.definitions_sha256.as_str())),
            ("metrics", CValue::Arr(metrics)),
            ("quarter", s(&self.quarter.to_string())),
            ("schema", s(SCHEMA)),
            ("warnings", CValue::Arr(warnings)),
        ]))
    }

    /// Deterministic LF-only Markdown summary (`<YYYY>Q<n>.md`).
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# Quarterly Metric Values: {}\n\n", self.quarter));
        out.push_str(&format!("- Quarter: `{}`\n", self.quarter));
        out.push_str(&format!("- Definitions SHA-256: `{}`\n\n", self.definitions_sha256));
        out.push_str("| Metric | Value | Unit | Aggregation | Snapshots | Status | Flags |\n");
        out.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
        for m in &self.metrics {
            let unit = md_cell(&m.unit);
            match &m.value {
                MetricValue::Available { value, snapshot_count, definition_changed, .. } => {
                    let flags = if *definition_changed { "`definition_changed`" } else { "none" };
                    out.push_str(&format!(
                        "| `{}` | {} | {} | `{}` | {} | `available` | {} |\n",
                        m.id,
                        value,
                        unit,
                        m.aggregation.as_str(),
                        snapshot_count,
                        flags,
                    ));
                }
                MetricValue::Unavailable { reason } => {
                    out.push_str(&format!(
                        "| `{}` | unavailable (`{}`) | {} | `{}` | 0 | `unavailable` | none |\n",
                        m.id,
                        reason,
                        unit,
                        m.aggregation.as_str(),
                    ));
                }
            }
        }
        if !self.warnings.is_empty() {
            out.push_str("\n## Warnings\n\n");
            for w in &self.warnings {
                out.push_str(&format!("- {w}\n"));
            }
        }
        out
    }

    /// Human-readable CLI output lines.
    pub fn render_text(&self, json_path: &Path, md_path: &Path) -> String {
        let mut out = String::new();
        for w in &self.warnings {
            out.push_str(&format!("{w}\n"));
        }
        let mut available = 0usize;
        let mut unavailable = 0usize;
        for m in &self.metrics {
            match &m.value {
                MetricValue::Available { value, snapshot_count, definition_changed, .. } => {
                    available += 1;
                    let flag = if *definition_changed { " definition_changed" } else { "" };
                    out.push_str(&format!(
                        "{} {} {} ({}, {} snapshot(s)){flag}\n",
                        m.id,
                        value,
                        m.unit,
                        m.aggregation.as_str(),
                        snapshot_count
                    ));
                }
                MetricValue::Unavailable { reason } => {
                    unavailable += 1;
                    out.push_str(&format!("{} unavailable ({reason})\n", m.id));
                }
            }
        }
        out.push_str(&format!(
            "metric-quarter: {} wrote {} and {} ({available} available, {unavailable} unavailable, {} warning(s))\n",
            self.quarter,
            json_path.display(),
            md_path.display(),
            self.warnings.len()
        ));
        out
    }
}

fn md_cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\r', '\n'], " ")
}

// ------------------------------------------------------------------ quarter computation

/// Inputs to `metric-quarter`.
pub struct QuarterInputs<'a> {
    pub store: &'a Path,
    pub defs: &'a Path,
    pub quarter: &'a str,
    pub out: &'a Path,
}

/// Computes quarterly values and writes `<out>/<YYYY>Q<n>.json` and `<out>/<YYYY>Q<n>.md`.
///
/// Writes nothing if any validation, verification or aggregation step fails.
pub fn run_quarter(inputs: &QuarterInputs<'_>) -> Result<(QuarterReport, PathBuf, PathBuf), SnapError> {
    let store = SnapshotStore::new(inputs.store);
    let report = compute_quarter(&store, inputs.defs, inputs.quarter)?;
    let (json_path, md_path) = write_quarter(&report, inputs.out)?;
    Ok((report, json_path, md_path))
}

/// Computes quarterly values without writing output files.
pub fn compute_quarter(store: &SnapshotStore, defs_path: &Path, quarter_str: &str) -> Result<QuarterReport, SnapError> {
    let quarter = Quarter::parse(quarter_str)?;
    let (metrics, defs_bytes) = def::load_definitions_file(defs_path)?;
    let definitions_sha256 = sha256(&defs_bytes);

    // The store root must exist and be a directory.
    let meta = fs::metadata(store.root())
        .map_err(|e| SnapError::InputUnreadable { path: store.root().display().to_string(), detail: e.to_string() })?;
    if !meta.is_dir() {
        return Err(SnapError::InputUnreadable {
            path: store.root().display().to_string(),
            detail: "store path is not a directory".to_string(),
        });
    }

    let mut out_metrics = Vec::with_capacity(metrics.len());
    let mut warnings = Vec::new();

    for metric in &metrics {
        let current_def_sha = definition_digest(metric);
        let entry = compute_metric(store, metric, current_def_sha, quarter, &mut warnings)?;
        out_metrics.push(entry);
    }

    Ok(QuarterReport { quarter, definitions_sha256, metrics: out_metrics, warnings })
}

/// Writes `<out_dir>/<YYYY>Q<n>.json` and `<out_dir>/<YYYY>Q<n>.md`.
pub fn write_quarter(report: &QuarterReport, out_dir: &Path) -> Result<(PathBuf, PathBuf), SnapError> {
    fs::create_dir_all(out_dir)
        .map_err(|e| SnapError::OutputUnwritable { path: out_dir.display().to_string(), detail: e.to_string() })?;
    let json_path = out_dir.join(format!("{}.json", report.quarter));
    let md_path = out_dir.join(format!("{}.md", report.quarter));
    let json_bytes = report.to_json();
    let md_text = report.to_markdown();
    fs::write(&json_path, &json_bytes)
        .map_err(|e| SnapError::OutputUnwritable { path: json_path.display().to_string(), detail: e.to_string() })?;
    fs::write(&md_path, md_text.as_bytes())
        .map_err(|e| SnapError::OutputUnwritable { path: md_path.display().to_string(), detail: e.to_string() })?;
    Ok((json_path, md_path))
}

struct LoadedSnapshot {
    file_name: String,
    label: String,
    record: SnapshotRecord,
    record_sha256: Digest,
}

fn compute_metric(
    store: &SnapshotStore,
    metric: &Metric,
    current_def_sha: Digest,
    quarter: Quarter,
    warnings: &mut Vec<Warning>,
) -> Result<QuarterMetric, SnapError> {
    let dir = store.metric_dir(&metric.id);
    let entries = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Ok(QuarterMetric {
                id: metric.id.clone(),
                unit: metric.unit.clone(),
                aggregation: metric.aggregation,
                definition_sha256: current_def_sha,
                value: MetricValue::Unavailable { reason: "no_snapshot" },
            });
        }
        Err(e) => {
            return Err(SnapError::InputUnreadable { path: dir.display().to_string(), detail: e.to_string() });
        }
    };

    let mut json_names: Vec<String> = Vec::new();
    for item in entries {
        let entry =
            item.map_err(|e| SnapError::InputUnreadable { path: dir.display().to_string(), detail: e.to_string() })?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else { continue };
        if name_str.ends_with(".json") {
            let ft = entry.file_type().map_err(|e| SnapError::InputUnreadable {
                path: entry.path().display().to_string(),
                detail: e.to_string(),
            })?;
            if ft.is_file() {
                json_names.push(name_str.to_string());
            }
        }
    }
    json_names.sort();
    if json_names.len() > MAX_SNAPSHOTS_PER_METRIC {
        return Err(SnapError::InputUnreadable {
            path: dir.display().to_string(),
            detail: format!("more than {MAX_SNAPSHOTS_PER_METRIC} snapshot files"),
        });
    }

    let mut in_quarter: Vec<LoadedSnapshot> = Vec::new();
    for file_name in json_names {
        let record_path = dir.join(&file_name);
        let label = format!("snapshots/{}/{file_name}", metric.id);
        let bytes = read_capped(&record_path, MAX_RECORD_BYTES)?;
        let record = SnapshotRecord::parse_json(&bytes, &label)?;
        if record.metric_id != metric.id {
            return Err(SnapError::SnapshotMismatch {
                snapshot: label,
                detail: format!("record metric_id {:?} does not match folder {:?}", record.metric_id, metric.id),
            });
        }
        let stamp = UtcStamp::parse(&record.collected_at).ok_or_else(|| SnapError::RecordInvalid {
            path: label.clone(),
            detail: format!("invalid collected_at {:?}", record.collected_at),
        })?;
        if stamp.quarter() == quarter {
            in_quarter.push(LoadedSnapshot { file_name, label, record, record_sha256: sha256(&bytes) });
        }
    }

    if in_quarter.is_empty() {
        return Ok(QuarterMetric {
            id: metric.id.clone(),
            unit: metric.unit.clone(),
            aggregation: metric.aggregation,
            definition_sha256: current_def_sha,
            value: MetricValue::Unavailable { reason: "no_snapshot" },
        });
    }

    // Sort by RFC 3339 UTC timestamp string (fixed-width UTC => chronological), then filename.
    in_quarter.sort_by(|a, b| a.record.collected_at.cmp(&b.record.collected_at).then(a.file_name.cmp(&b.file_name)));

    let mut verified_values: Vec<u64> = Vec::with_capacity(in_quarter.len());
    let mut snapshot_digests: Vec<Digest> = Vec::with_capacity(in_quarter.len());
    let mut definition_changed = false;

    for snap in &in_quarter {
        let raw_path = dir.join(&snap.record.raw_file);
        let raw_bytes = match read_capped(&raw_path, MAX_RAW_BYTES) {
            Ok(b) => b,
            Err(e) => {
                return Err(SnapError::SnapshotMismatch {
                    snapshot: snap.label.clone(),
                    detail: format!("raw file {} unreadable: {e}", snap.record.raw_file),
                });
            }
        };
        let actual_raw_sha = sha256(&raw_bytes);
        if actual_raw_sha != snap.record.raw_sha256 {
            return Err(SnapError::SnapshotMismatch {
                snapshot: snap.label.clone(),
                detail: format!(
                    "raw file {} sha256 mismatch: expected {}, got {}",
                    snap.record.raw_file, snap.record.raw_sha256, actual_raw_sha
                ),
            });
        }

        let extracted = re_extract_value(metric, &snap.record, &raw_bytes)
            .map_err(|detail| SnapError::SnapshotMismatch { snapshot: snap.label.clone(), detail })?;
        if extracted != snap.record.value {
            return Err(SnapError::SnapshotMismatch {
                snapshot: snap.label.clone(),
                detail: format!("stored value {} differs from re-extracted value {extracted}", snap.record.value),
            });
        }

        if snap.record.definition_sha256 != current_def_sha {
            definition_changed = true;
            warnings.push(Warning::DefinitionChanged {
                metric_id: metric.id.clone(),
                snapshot: snap.label.clone(),
                expected_sha256: current_def_sha,
                found_sha256: snap.record.definition_sha256,
            });
        }

        verified_values.push(extracted);
        snapshot_digests.push(snap.record_sha256);
    }

    let aggregated = aggregate_values(metric, &in_quarter, &verified_values)?;

    Ok(QuarterMetric {
        id: metric.id.clone(),
        unit: metric.unit.clone(),
        aggregation: metric.aggregation,
        definition_sha256: current_def_sha,
        value: MetricValue::Available {
            value: aggregated,
            snapshot_count: in_quarter.len(),
            snapshot_digests,
            definition_changed,
        },
    })
}

fn aggregate_values(metric: &Metric, snaps: &[LoadedSnapshot], values: &[u64]) -> Result<u64, SnapError> {
    match metric.aggregation {
        Aggregation::Last => values.last().copied().ok_or_else(|| SnapError::RecordInvalid {
            path: metric.id.clone(),
            detail: "cannot aggregate empty snapshot set".to_string(),
        }),
        Aggregation::Sum => {
            let mut total = 0u64;
            for (snap, &v) in snaps.iter().zip(values.iter()) {
                total = total.checked_add(v).ok_or_else(|| SnapError::Overflow {
                    metric_id: metric.id.clone(),
                    snapshot: snap.label.clone(),
                })?;
            }
            Ok(total)
        }
        Aggregation::Max => values.iter().copied().max().ok_or_else(|| SnapError::RecordInvalid {
            path: metric.id.clone(),
            detail: "cannot aggregate empty snapshot set".to_string(),
        }),
    }
}

fn re_extract_value(metric: &Metric, record: &SnapshotRecord, raw_bytes: &[u8]) -> Result<u64, String> {
    match metric.method {
        Method::HttpJson => {
            let selector = metric.selector.as_deref().ok_or_else(|| "http_json metric has no selector".to_string())?;
            extract_json_pointer_u64(raw_bytes, selector)
        }
        Method::GitHistory => {
            if let Some(selector) = metric.selector.as_deref() {
                return extract_json_pointer_u64(raw_bytes, selector);
            }
            // When the raw response is structured JSON with a top-level "count" field (as written
            // by OUTR.COMM.METRIC.COLLECT), verify it matches; otherwise use the stored count.
            if let Ok(RawJson::Obj(map)) = parse_raw_json(raw_bytes, MAX_RAW_BYTES) {
                if let Some(v) = map.get("count") {
                    return raw_json_to_u64(v, "/count");
                }
            }
            Ok(record.value)
        }
        Method::ManualPublicCount => {
            if let Some(selector) = metric.selector.as_deref() {
                return extract_json_pointer_u64(raw_bytes, selector);
            }
            // When the raw response is structured JSON with a top-level "value" or "count" field
            // (as written by OUTR.COMM.METRIC.COLLECT), verify it matches; otherwise use the stored count.
            if let Ok(RawJson::Obj(map)) = parse_raw_json(raw_bytes, MAX_RAW_BYTES) {
                if let Some(v) = map.get("value") {
                    return raw_json_to_u64(v, "/value");
                }
                if let Some(v) = map.get("count") {
                    return raw_json_to_u64(v, "/count");
                }
            }
            Ok(record.value)
        }
    }
}

fn read_capped(path: &Path, max_bytes: usize) -> Result<Vec<u8>, SnapError> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|f| f.take(max_bytes as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| SnapError::InputUnreadable { path: path.display().to_string(), detail: e.to_string() })?;
    if bytes.len() > max_bytes {
        return Err(SnapError::InputUnreadable {
            path: path.display().to_string(),
            detail: format!("larger than {max_bytes} bytes"),
        });
    }
    Ok(bytes)
}

fn is_safe_filename(name: &str) -> bool {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains('\0') {
        return false;
    }
    let mut comps = Path::new(name).components();
    matches!(comps.next(), Some(Component::Normal(_))) && comps.next().is_none()
}

// ------------------------------------------------------------------ RFC 6901 JSON Pointer & exact-u64 JSON parser

/// Parses `raw_bytes` as JSON and resolves RFC 6901 `pointer` to an exact non-negative `u64`.
pub fn extract_json_pointer_u64(raw_bytes: &[u8], pointer: &str) -> Result<u64, String> {
    if !def::valid_json_pointer(pointer) {
        return Err(format!("invalid RFC 6901 JSON Pointer {pointer:?}"));
    }
    let root = parse_raw_json(raw_bytes, MAX_RAW_BYTES)?;
    let target = resolve_pointer(&root, pointer)?;
    raw_json_to_u64(target, pointer)
}

fn raw_json_to_u64(v: &RawJson, pointer: &str) -> Result<u64, String> {
    match v {
        RawJson::UInt(n) => Ok(*n),
        _ => Err(format!("value at JSON Pointer {pointer:?} is not a non-negative u64 integer")),
    }
}

fn resolve_pointer<'a>(root: &'a RawJson, pointer: &str) -> Result<&'a RawJson, String> {
    if pointer.is_empty() {
        return Ok(root);
    }
    let Some(rest) = pointer.strip_prefix('/') else {
        return Err(format!("JSON Pointer {pointer:?} must start with '/'"));
    };
    let mut cur = root;
    for raw_token in rest.split('/') {
        let token = unescape_pointer_token(raw_token)?;
        match cur {
            RawJson::Obj(map) => {
                cur = map.get(&token).ok_or_else(|| format!("JSON Pointer {pointer:?}: key {token:?} not found"))?;
            }
            RawJson::Arr(items) => {
                let idx = parse_rfc6901_array_index(&token)
                    .ok_or_else(|| format!("JSON Pointer {pointer:?}: invalid array index {token:?}"))?;
                cur = items.get(idx).ok_or_else(|| {
                    format!("JSON Pointer {pointer:?}: array index {idx} out of bounds (len {})", items.len())
                })?;
            }
            _ => {
                return Err(format!("JSON Pointer {pointer:?}: cannot traverse scalar with token {token:?}"));
            }
        }
    }
    Ok(cur)
}

/// RFC 6901 §4: decode `~1` -> `/` and `~0` -> `~` in a single pass.
fn unescape_pointer_token(raw: &str) -> Result<String, String> {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '~' {
            match chars.next() {
                Some('0') => out.push('~'),
                Some('1') => out.push('/'),
                _ => return Err(format!("invalid escape in JSON Pointer token {raw:?}")),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// RFC 6901 §4 array index: `"0"` or `[1-9][0-9]*`.
fn parse_rfc6901_array_index(token: &str) -> Option<usize> {
    if token == "0" {
        return Some(0);
    }
    let b = token.as_bytes();
    if b.is_empty() || b[0] == b'0' || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    token.parse::<usize>().ok()
}

fn parse_strict_u64_str(s: &str) -> Option<u64> {
    if s == "0" {
        return Some(0);
    }
    let b = s.as_bytes();
    if b.is_empty() || b[0] == b'0' || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    s.parse::<u64>().ok()
}

/// Minimal exact-integer JSON DOM for raw snapshot responses and record parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RawJson {
    Null,
    Bool(bool),
    UInt(u64),
    OtherNumber,
    Str(String),
    Arr(Vec<RawJson>),
    Obj(BTreeMap<String, RawJson>),
}

enum JsonFrame {
    Arr(Vec<RawJson>),
    Obj(BTreeMap<String, RawJson>, String),
}

fn parse_raw_json(input: &[u8], max_bytes: usize) -> Result<RawJson, String> {
    if input.len() > max_bytes {
        return Err(format!("JSON exceeds {max_bytes} bytes"));
    }
    let text = std::str::from_utf8(input).map_err(|_| "JSON is not valid UTF-8".to_string())?;
    let mut p = JsonParser { s: text.as_bytes(), text, pos: 0, elements: 0 };
    let mut stack: Vec<JsonFrame> = Vec::new();
    p.ws();
    'value: loop {
        p.count()?;
        let mut v = match p.peek() {
            Some(b'{') => {
                p.pos += 1;
                if stack.len() >= MAX_JSON_DEPTH {
                    return Err("JSON nesting exceeds depth limit".to_string());
                }
                p.ws();
                if p.peek() == Some(b'}') {
                    p.pos += 1;
                    RawJson::Obj(BTreeMap::new())
                } else {
                    let key = p.key()?;
                    stack.push(JsonFrame::Obj(BTreeMap::new(), key));
                    p.ws();
                    continue 'value;
                }
            }
            Some(b'[') => {
                p.pos += 1;
                if stack.len() >= MAX_JSON_DEPTH {
                    return Err("JSON nesting exceeds depth limit".to_string());
                }
                p.ws();
                if p.peek() == Some(b']') {
                    p.pos += 1;
                    RawJson::Arr(Vec::new())
                } else {
                    stack.push(JsonFrame::Arr(Vec::new()));
                    continue 'value;
                }
            }
            _ => p.scalar()?,
        };
        loop {
            p.ws();
            match stack.last_mut() {
                None => {
                    if p.pos != p.s.len() {
                        return Err(format!("trailing JSON bytes at byte {}", p.pos));
                    }
                    return Ok(v);
                }
                Some(JsonFrame::Arr(items)) => {
                    items.push(v);
                    match p.bump() {
                        Some(b',') => {
                            p.ws();
                            continue 'value;
                        }
                        Some(b']') => match stack.pop() {
                            Some(JsonFrame::Arr(items)) => v = RawJson::Arr(items),
                            _ => return Err(format!("JSON array syntax error at byte {}", p.pos)),
                        },
                        _ => return Err(format!("JSON array syntax error at byte {}", p.pos)),
                    }
                }
                Some(JsonFrame::Obj(map, key)) => {
                    let k = std::mem::take(key);
                    if map.contains_key(&k) {
                        return Err(format!("duplicate JSON key {k:?}"));
                    }
                    map.insert(k, v);
                    match p.bump() {
                        Some(b',') => {
                            p.ws();
                            *key = p.key()?;
                            p.ws();
                            continue 'value;
                        }
                        Some(b'}') => match stack.pop() {
                            Some(JsonFrame::Obj(map, _)) => v = RawJson::Obj(map),
                            _ => return Err(format!("JSON object syntax error at byte {}", p.pos)),
                        },
                        _ => return Err(format!("JSON object syntax error at byte {}", p.pos)),
                    }
                }
            }
        }
    }
}

struct JsonParser<'a> {
    s: &'a [u8],
    text: &'a str,
    pos: usize,
    elements: usize,
}

impl JsonParser<'_> {
    fn count(&mut self) -> Result<(), String> {
        if self.elements >= MAX_JSON_ELEMENTS {
            return Err("JSON exceeds element budget".to_string());
        }
        self.elements += 1;
        Ok(())
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn lit(&mut self, word: &str, v: RawJson) -> Result<RawJson, String> {
        if self.s.get(self.pos..).is_some_and(|rest| rest.starts_with(word.as_bytes())) {
            self.pos += word.len();
            Ok(v)
        } else {
            Err(format!("invalid JSON literal at byte {}", self.pos))
        }
    }

    fn key(&mut self) -> Result<String, String> {
        self.count()?;
        if self.peek() != Some(b'"') {
            return Err(format!("expected JSON object key at byte {}", self.pos));
        }
        let k = self.string()?;
        self.ws();
        if self.bump() != Some(b':') {
            return Err(format!("expected ':' after JSON key at byte {}", self.pos));
        }
        self.ws();
        Ok(k)
    }

    fn scalar(&mut self) -> Result<RawJson, String> {
        match self.peek() {
            Some(b'"') => self.string().map(RawJson::Str),
            Some(b't') => self.lit("true", RawJson::Bool(true)),
            Some(b'f') => self.lit("false", RawJson::Bool(false)),
            Some(b'n') => self.lit("null", RawJson::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(format!("unexpected JSON token at byte {}", self.pos)),
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> Result<RawJson, String> {
        let start = self.pos;
        let negative = self.peek() == Some(b'-');
        if negative {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(format!("invalid JSON number at byte {}", self.pos)),
        }
        let mut has_frac_or_exp = false;
        if self.peek() == Some(b'.') {
            has_frac_or_exp = true;
            self.pos += 1;
            if self.digits() == 0 {
                return Err(format!("invalid JSON number fraction at byte {}", self.pos));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            has_frac_or_exp = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(format!("invalid JSON number exponent at byte {}", self.pos));
            }
        }
        let slice = self.text.get(start..self.pos).ok_or_else(|| "invalid UTF-8 boundary".to_string())?;
        if !negative && !has_frac_or_exp {
            if let Ok(n) = slice.parse::<u64>() {
                return Ok(RawJson::UInt(n));
            }
        }
        Ok(RawJson::OtherNumber)
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let end = self.pos + 4;
        let h = self.text.get(self.pos..end).ok_or_else(|| format!("short \\u escape at byte {}", self.pos))?;
        if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("bad \\u escape at byte {}", self.pos));
        }
        let v = u32::from_str_radix(h, 16).map_err(|_| format!("bad \\u escape at byte {}", self.pos))?;
        self.pos = end;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, String> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let start = self.pos;
            while let Some(b) = self.peek() {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            let chunk = self.text.get(start..self.pos).ok_or_else(|| "invalid UTF-8 boundary".to_string())?;
            out.push_str(chunk);
            match self.bump() {
                Some(b'"') => return Ok(out),
                Some(b'\\') => {
                    let c = match self.bump() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                                    return Err(format!("unpaired high surrogate at byte {}", self.pos));
                                }
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(format!("invalid low surrogate at byte {}", self.pos));
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            char::from_u32(cp).ok_or_else(|| format!("invalid unicode scalar at byte {}", self.pos))?
                        }
                        _ => return Err(format!("invalid JSON escape at byte {}", self.pos)),
                    };
                    out.push(c);
                }
                _ => return Err(format!("unterminated JSON string at byte {}", self.pos)),
            }
        }
    }
}

// ------------------------------------------------------------------ canonical JSON helpers

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn u64_cvalue(v: u64) -> CValue {
    if v <= MAX_SAFE_INT {
        CValue::Num(v as f64)
    } else {
        CValue::U64(v)
    }
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<BTreeMap<_, _>>())
}

fn canonical(v: &CValue) -> Vec<u8> {
    to_canonical_bytes(v).unwrap_or_else(|_| b"{\"status\":\"error\"}".to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_parsing() {
        assert_eq!(Quarter::parse("2027Q1"), Ok(Quarter { year: 2027, q: 1 }));
        assert_eq!(Quarter::parse("2027Q4"), Ok(Quarter { year: 2027, q: 4 }));
        for bad in ["", "2027Q0", "2027Q5", "2027q1", "2027-Q1", "0000Q1", "27Q1", "2027Q10"] {
            assert_eq!(Quarter::parse(bad).map_err(|e| e.code()), Err("quarter_invalid"), "{bad:?}");
        }
    }

    #[test]
    fn utc_stamp_validation_and_ordering() {
        let a = UtcStamp::parse("2024-02-29T23:59:59Z");
        assert!(a.is_some());
        assert_eq!(a.map(UtcStamp::quarter), Some(Quarter { year: 2024, q: 1 }));
        assert_eq!(a.map(UtcStamp::filename_stem).as_deref(), Some("2024-02-29T23-59-59Z"));
        // 2027 is not a leap year.
        assert!(UtcStamp::parse("2027-02-29T00:00:00Z").is_none());
        assert!(UtcStamp::parse("2027-01-15T12:00:00+00:00").is_none());
        assert!(UtcStamp::parse("2027-01-15t12:00:00z").is_none());
        assert!(UtcStamp::parse("2027-13-01T00:00:00Z").is_none());
        assert!(UtcStamp::parse("2027-01-15T24:00:00Z").is_none());
    }

    #[test]
    fn rfc6901_pointer_extraction_and_escapes() {
        let doc = br#"{"a/b":{"c~d":[10,{"count":18446744073709551615}]},"neg":-1,"flt":1.5,"str":"7"}"#;
        assert_eq!(extract_json_pointer_u64(doc, "/a~1b/c~0d/0"), Ok(10));
        assert_eq!(extract_json_pointer_u64(doc, "/a~1b/c~0d/1/count"), Ok(u64::MAX));
        assert_eq!(extract_json_pointer_u64(b"42", ""), Ok(42));
        assert!(extract_json_pointer_u64(doc, "/a~1b/c~0d/00").is_err());
        assert!(extract_json_pointer_u64(doc, "/a~1b/c~0d/-").is_err());
        assert!(extract_json_pointer_u64(doc, "/neg").is_err());
        assert!(extract_json_pointer_u64(doc, "/flt").is_err());
        assert!(extract_json_pointer_u64(doc, "/str").is_err());
        assert!(extract_json_pointer_u64(doc, "/missing").is_err());
        // ~01 unescapes to ~1, not /
        let tilde_doc = br#"{"a~1b":9,"a/b":2}"#;
        assert_eq!(extract_json_pointer_u64(tilde_doc, "/a~01b"), Ok(9));
    }

    #[test]
    fn safe_filename_checks() {
        assert!(is_safe_filename("2027-01-15T12-00-00Z.raw"));
        for bad in ["", ".", "..", "../a.raw", "a/b.raw", "a\\b.raw", "a\0b.raw"] {
            assert!(!is_safe_filename(bad), "{bad:?}");
        }
    }
}
