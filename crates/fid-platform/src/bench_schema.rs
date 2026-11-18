//! Metrology Maven benchmark formats (spec node `PLAT.HW.SCHEMA`): the suite manifest
//! (TOML: workloads, pinned datasets, metrics with units) and the benchmark result document
//! (JSON, `schema_version` 1: context block plus per-metric statistics), their strict
//! validators, and the JSON Schema export of the result document.
//!
//! Both readers behave like serde with `deny_unknown_fields`: every key the schema does not
//! define is an error. Determinism D0 for serialisation: [`BenchResult::to_json`] writes RFC
//! 8785 canonical JSON through `fid-canon` (keys in one fixed order, no hash-ordered maps), so
//! the same value always gives the same bytes. The measured values themselves are observed
//! (`N`).

use crate::matrix::{Arch, OsFamily};
use fid_canon::{CValue, CanonError};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// The only `schema_version` this crate reads or writes, for both formats.
pub const SUPPORTED_SCHEMA: u32 = 1;
/// Largest accepted manifest or result file, in bytes.
pub const MAX_BYTES: usize = 1024 * 1024;
/// Deepest JSON nesting accepted in a result document.
pub const MAX_DEPTH: usize = 16;
/// Path of the suite manifest, relative to the repository root (its content belongs to
/// `PLAT.HW.SUITE`).
pub const SUITE_DEFAULT_PATH: &str = "platform/bench-suite.toml";
/// Where `plat-hw result-schema` writes the exported JSON Schema.
pub const RESULT_SCHEMA_PATH: &str = "platform/bench-result.schema.json";
/// Allowed repetitions per workload (REQ-PLAT.HW.SCHEMA-001).
pub const REPETITIONS: std::ops::RangeInclusive<u8> = 1..=100;
/// Pattern of workload IDs and metric names.
pub const NAME_PATTERN: &str = "^[a-z0-9][a-z0-9_.-]*$";
/// JSON Schema dialect of the export.
pub const JSON_SCHEMA_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

const SUITE_KEYS: [&str; 3] = ["schema_version", "suite_version", "workloads"];
const WORKLOAD_KEYS: [&str; 5] = ["id", "dataset", "metrics", "repetitions", "timeout_s"];
const DATASET_KEYS: [&str; 2] = ["id", "sha256"];
const METRIC_KEYS: [&str; 3] = ["name", "unit", "better"];
const RESULT_KEYS: [&str; 3] = ["schema_version", "context", "workloads"];
/// The context fields in document order (REQ-PLAT.HW.SCHEMA-002).
pub const CONTEXT_KEYS: [&str; 11] = [
    "suite_version",
    "app_version",
    "os",
    "os_version",
    "arch",
    "cpu_model",
    "ram_mib",
    "gpu_backend",
    "adapter_name",
    "gpu_driver_version",
    "machine",
];
const WORKLOAD_RESULT_KEYS: [&str; 2] = ["id", "metrics"];
/// The per-metric result fields in document order (REQ-PLAT.HW.SCHEMA-003).
pub const METRIC_RESULT_KEYS: [&str; 8] =
    ["name", "unit", "dataset_sha256", "repetitions", "median", "min", "max", "status"];

macro_rules! named_enum {
    ($(#[$m:meta])* $name:ident { $($var:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $($var),+ }
        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            /// The name used in the files.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$var => $s),+ }
            }
            /// Parses the name used in the files.
            pub fn from_name(s: &str) -> Option<Self> {
                match s { $($s => Some($name::$var),)+ _ => None }
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

named_enum!(
    /// Metric unit; the only allowed set (REQ-PLAT.HW.SCHEMA-004).
    Unit { S = "s", Ms = "ms", Fps = "fps", MiB = "MiB", PointsPerS = "points_per_s", TrianglesPerS = "triangles_per_s" }
);
named_enum!(
    /// Which direction of a metric is an improvement.
    Better { Lower = "lower", Higher = "higher" }
);
named_enum!(
    /// Outcome of one metric in one run (REQ-PLAT.HW.SCHEMA-003).
    Status { Ok = "ok", Timeout = "timeout", Failed = "failed", Skipped = "skipped" }
);

/// A pinned dataset: catalogue ID and SHA-256 digest of its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetRef {
    pub id: String,
    pub sha256: Digest,
}

/// One measured quantity of a workload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metric {
    pub name: String,
    pub unit: Unit,
    pub better: Better,
}

/// One workload of the suite (REQ-PLAT.HW.SCHEMA-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workload {
    pub id: String,
    pub dataset: DatasetRef,
    pub metrics: Vec<Metric>,
    /// 1 to 100.
    pub repetitions: u8,
    /// At least 1.
    pub timeout_s: u32,
}

/// A validated suite manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suite {
    pub schema_version: u32,
    pub suite_version: String,
    pub workloads: Vec<Workload>,
}

/// The context of a result document, recorded once (REQ-PLAT.HW.SCHEMA-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub suite_version: String,
    pub app_version: String,
    pub os: OsFamily,
    pub os_version: String,
    pub arch: Arch,
    pub cpu_model: String,
    /// Installed RAM in MiB, at least 1.
    pub ram_mib: u32,
    pub gpu_backend: String,
    pub adapter_name: String,
    pub gpu_driver_version: String,
    /// Reference machine name, or `local` for user runs.
    pub machine: String,
}

/// Statistics of one metric of one workload (REQ-PLAT.HW.SCHEMA-003). With status `ok`
/// the three statistics are present, non-negative and `min <= median <= max`; with any
/// other status they are all absent (`null` in JSON).
#[derive(Debug, Clone, PartialEq)]
pub struct MetricResult {
    pub name: String,
    pub unit: Unit,
    pub dataset_sha256: Digest,
    pub repetitions: u8,
    pub median: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub status: Status,
}

/// The results of one workload.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkloadResult {
    pub id: String,
    pub metrics: Vec<MetricResult>,
}

/// A benchmark result document, `schema_version` 1.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchResult {
    pub schema_version: u32,
    pub context: Context,
    pub workloads: Vec<WorkloadResult>,
}

/// A typed schema error. Errors that concern a workload name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    /// Too large, not UTF-8, not TOML/JSON (`line` is 1-based when the parser reports one),
    /// an unknown key, or a known field that is missing or has an unusable value.
    Parse { line: Option<usize>, msg: String },
    /// `schema_version` is an integer other than [`SUPPORTED_SCHEMA`]; fatal.
    UnsupportedSchema { found: i64 },
    /// The workload has no dataset digest (REQ-PLAT.HW.SCHEMA-004).
    MissingDigest { workload: String },
    /// A metric unit outside [`Unit::ALL`] (REQ-PLAT.HW.SCHEMA-004).
    BadUnit { workload: String, metric: String, unit: String },
    /// `repetitions` is an integer outside 1 to 100.
    BadRepetitions { workload: String, found: i64 },
    /// A second workload with the same ID.
    DuplicateWorkload { id: String },
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::Parse { line: Some(l), msg } => write!(f, "Parse {{ line: {l} }}: {msg}"),
            SchemaError::Parse { line: None, msg } => write!(f, "Parse: {msg}"),
            SchemaError::UnsupportedSchema { found } => {
                write!(f, "UnsupportedSchema {{ found: {found} }} (supported: {SUPPORTED_SCHEMA})")
            }
            SchemaError::MissingDigest { workload } => write!(f, "MissingDigest {{ workload: {workload} }}"),
            SchemaError::BadUnit { workload, metric, unit } => write!(
                f,
                "BadUnit {{ workload: {workload}, metric: {metric}, unit: {unit} }} (allowed: {})",
                Unit::ALL.iter().map(|u| u.as_str()).collect::<Vec<_>>().join(", ")
            ),
            SchemaError::BadRepetitions { workload, found } => {
                write!(f, "BadRepetitions {{ workload: {workload}, found: {found} }} (allowed: 1 to 100)")
            }
            SchemaError::DuplicateWorkload { id } => write!(f, "DuplicateWorkload {{ id: {id} }}"),
        }
    }
}

impl std::error::Error for SchemaError {}

fn parse_err(msg: String) -> SchemaError {
    SchemaError::Parse { line: None, msg }
}

/// Whether `s` matches [`NAME_PATTERN`].
pub fn is_valid_name(s: &str) -> bool {
    let b = s.as_bytes();
    let ok = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    match b.split_first() {
        Some((first, rest)) => ok(first) && rest.iter().all(|c| ok(c) || matches!(c, b'_' | b'.' | b'-')),
        None => false,
    }
}

fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes().iter().take(offset).filter(|&&b| b == b'\n').count() + 1
}

// ------------------------------------------------------------------ suite manifest (TOML)

/// Validates a suite manifest (REQ-PLAT.HW.SCHEMA-001, -004) and returns every diagnostic.
///
/// A file over [`MAX_BYTES`], non-UTF-8 or non-TOML input and `UnsupportedSchema` stop at
/// once and are the only entry. Otherwise the order is: top level (`schema_version`,
/// unknown keys in key order, `suite_version`, `workloads`), then each workload in file
/// order (unknown keys in key order, then `id`, `dataset`, `metrics`, `repetitions`,
/// `timeout_s`). A workload is named by its `id`, or `workloads[<index>]` if it has none.
pub fn validate_suite(bytes: &[u8]) -> Result<Suite, Vec<SchemaError>> {
    if bytes.len() > MAX_BYTES {
        return Err(vec![parse_err(format!("file is larger than {MAX_BYTES} bytes"))]);
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Err(vec![parse_err("file is not UTF-8".into())]);
    };
    let table = match text.parse::<toml::Table>() {
        Ok(t) => t,
        Err(e) => {
            let line = e.span().map(|s| line_of(text, s.start));
            return Err(vec![SchemaError::Parse { line, msg: e.message().trim_end().to_string() }]);
        }
    };

    let mut errors = Vec::new();
    match table.get("schema_version") {
        None => errors.push(parse_err("schema_version is missing".into())),
        Some(toml::Value::Integer(n)) if *n == i64::from(SUPPORTED_SCHEMA) => {}
        Some(toml::Value::Integer(n)) => return Err(vec![SchemaError::UnsupportedSchema { found: *n }]),
        Some(_) => errors.push(parse_err("schema_version is not an integer".into())),
    }
    unknown_toml_keys(&table, &SUITE_KEYS, "the manifest", &mut errors);
    let suite_version = toml_text(&table, "suite_version", "the manifest", &mut errors);

    let mut workloads = Vec::new();
    match table.get("workloads") {
        None => errors.push(parse_err("workloads is missing".into())),
        Some(toml::Value::Array(a)) if a.is_empty() => errors.push(parse_err("workloads is empty".into())),
        Some(toml::Value::Array(a)) => {
            let mut seen: Vec<String> = Vec::new();
            for (i, v) in a.iter().enumerate() {
                let toml::Value::Table(t) = v else {
                    errors.push(parse_err(format!("workloads[{i}] is not a table")));
                    continue;
                };
                if let Some(w) = check_workload(i, t, &mut seen, &mut errors) {
                    workloads.push(w);
                }
            }
        }
        Some(_) => errors.push(parse_err("workloads is not an array of tables".into())),
    }

    match (errors.is_empty(), suite_version) {
        (true, Some(suite_version)) => Ok(Suite { schema_version: SUPPORTED_SCHEMA, suite_version, workloads }),
        _ => Err(errors),
    }
}

/// Parses and validates a suite manifest, returning the first error if there is any.
pub fn parse_suite(bytes: &[u8]) -> Result<Suite, SchemaError> {
    validate_suite(bytes).map_err(|errs| errs.into_iter().next().unwrap_or(parse_err("invalid manifest".into())))
}

fn unknown_toml_keys(t: &toml::Table, allowed: &[&str], at: &str, errors: &mut Vec<SchemaError>) {
    for k in t.keys() {
        if !allowed.contains(&k.as_str()) {
            errors.push(parse_err(format!("unknown field `{k}` in {at}")));
        }
    }
}

/// A required non-empty string field.
fn toml_text(t: &toml::Table, field: &str, at: &str, errors: &mut Vec<SchemaError>) -> Option<String> {
    match t.get(field) {
        Some(toml::Value::String(s)) if !s.is_empty() => Some(s.clone()),
        None => {
            errors.push(parse_err(format!("{field} is missing in {at}")));
            None
        }
        Some(_) => {
            errors.push(parse_err(format!("{field} in {at} is not a non-empty string")));
            None
        }
    }
}

fn check_workload(
    i: usize,
    t: &toml::Table,
    seen: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
) -> Option<Workload> {
    let before = errors.len();
    let label = match t.get("id") {
        Some(toml::Value::String(s)) if !s.is_empty() => s.clone(),
        _ => format!("workloads[{i}]"),
    };
    let at = format!("workload {label}");
    unknown_toml_keys(t, &WORKLOAD_KEYS, &at, errors);

    let id = match t.get("id") {
        Some(toml::Value::String(s)) if is_valid_name(s) => {
            if seen.contains(s) {
                errors.push(SchemaError::DuplicateWorkload { id: s.clone() });
            } else {
                seen.push(s.clone());
            }
            Some(s.clone())
        }
        None => {
            errors.push(parse_err(format!("id is missing in {at}")));
            None
        }
        Some(_) => {
            errors.push(parse_err(format!("id in {at} does not match {NAME_PATTERN}")));
            None
        }
    };

    let dataset = match t.get("dataset") {
        None => {
            errors.push(SchemaError::MissingDigest { workload: label.clone() });
            None
        }
        Some(toml::Value::Table(d)) => {
            let dat = format!("{at} dataset");
            unknown_toml_keys(d, &DATASET_KEYS, &dat, errors);
            let did = toml_text(d, "id", &dat, errors);
            let sha = match d.get("sha256") {
                None => {
                    errors.push(SchemaError::MissingDigest { workload: label.clone() });
                    None
                }
                Some(toml::Value::String(s)) if s.is_empty() => {
                    errors.push(SchemaError::MissingDigest { workload: label.clone() });
                    None
                }
                Some(toml::Value::String(s)) => match Digest::parse(s) {
                    Ok(dg) => Some(dg),
                    Err(_) => {
                        errors.push(parse_err(format!("sha256 in {dat} is not sha256:<64 lowercase hex digits>")));
                        None
                    }
                },
                Some(_) => {
                    errors.push(parse_err(format!("sha256 in {dat} is not a string")));
                    None
                }
            };
            match (did, sha) {
                (Some(id), Some(sha256)) => Some(DatasetRef { id, sha256 }),
                _ => None,
            }
        }
        Some(_) => {
            errors.push(parse_err(format!("dataset in {at} is not a table")));
            None
        }
    };

    let mut metrics = Vec::new();
    match t.get("metrics") {
        None => errors.push(parse_err(format!("metrics is missing in {at}"))),
        Some(toml::Value::Array(a)) if a.is_empty() => errors.push(parse_err(format!("metrics in {at} is empty"))),
        Some(toml::Value::Array(a)) => {
            let mut names: Vec<String> = Vec::new();
            for (j, v) in a.iter().enumerate() {
                let toml::Value::Table(mt) = v else {
                    errors.push(parse_err(format!("metrics[{j}] in {at} is not a table")));
                    continue;
                };
                if let Some(m) = check_metric(&label, j, mt, &mut names, errors) {
                    metrics.push(m);
                }
            }
        }
        Some(_) => errors.push(parse_err(format!("metrics in {at} is not an array of tables"))),
    }

    let repetitions = match t.get("repetitions") {
        None => {
            errors.push(parse_err(format!("repetitions is missing in {at}")));
            None
        }
        Some(toml::Value::Integer(n)) => match u8::try_from(*n) {
            Ok(r) if REPETITIONS.contains(&r) => Some(r),
            _ => {
                errors.push(SchemaError::BadRepetitions { workload: label.clone(), found: *n });
                None
            }
        },
        Some(_) => {
            errors.push(parse_err(format!("repetitions in {at} is not an integer")));
            None
        }
    };

    let timeout_s = match t.get("timeout_s") {
        Some(toml::Value::Integer(n)) => match u32::try_from(*n) {
            Ok(v) if v >= 1 => Some(v),
            _ => {
                errors.push(parse_err(format!("timeout_s in {at} is not an integer from 1 to {}", u32::MAX)));
                None
            }
        },
        None => {
            errors.push(parse_err(format!("timeout_s is missing in {at}")));
            None
        }
        Some(_) => {
            errors.push(parse_err(format!("timeout_s in {at} is not an integer from 1 to {}", u32::MAX)));
            None
        }
    };

    if errors.len() != before {
        return None;
    }
    match (id, dataset, repetitions, timeout_s) {
        (Some(id), Some(dataset), Some(repetitions), Some(timeout_s)) => {
            Some(Workload { id, dataset, metrics, repetitions, timeout_s })
        }
        _ => None,
    }
}

fn check_metric(
    workload: &str,
    j: usize,
    t: &toml::Table,
    names: &mut Vec<String>,
    errors: &mut Vec<SchemaError>,
) -> Option<Metric> {
    let at = format!("workload {workload} metrics[{j}]");
    unknown_toml_keys(t, &METRIC_KEYS, &at, errors);
    let name = match t.get("name") {
        Some(toml::Value::String(s)) if is_valid_name(s) => {
            if names.contains(s) {
                errors.push(parse_err(format!("duplicate metric name `{s}` in workload {workload}")));
                None
            } else {
                names.push(s.clone());
                Some(s.clone())
            }
        }
        None => {
            errors.push(parse_err(format!("name is missing in {at}")));
            None
        }
        Some(_) => {
            errors.push(parse_err(format!("name in {at} does not match {NAME_PATTERN}")));
            None
        }
    };
    let metric_label = name.clone().unwrap_or_else(|| format!("metrics[{j}]"));
    let unit = match t.get("unit") {
        None => {
            errors.push(parse_err(format!("unit is missing in {at}")));
            None
        }
        Some(toml::Value::String(s)) => {
            let u = Unit::from_name(s);
            if u.is_none() {
                errors.push(SchemaError::BadUnit {
                    workload: workload.to_string(),
                    metric: metric_label,
                    unit: s.clone(),
                });
            }
            u
        }
        Some(_) => {
            errors.push(parse_err(format!("unit in {at} is not a string")));
            None
        }
    };
    let better = match t.get("better") {
        None => {
            errors.push(parse_err(format!("better is missing in {at}")));
            None
        }
        Some(v) => {
            let b = v.as_str().and_then(Better::from_name);
            if b.is_none() {
                errors.push(parse_err(format!("better in {at} is not one of lower, higher")));
            }
            b
        }
    };
    match (name, unit, better) {
        (Some(name), Some(unit), Some(better)) => Some(Metric { name, unit, better }),
        _ => None,
    }
}

// ------------------------------------------------------------------ result document (JSON)

/// Validates a result document (REQ-PLAT.HW.SCHEMA-002, -003) and returns the first error.
///
/// The JSON is read through `fid-canon` (duplicate keys, non-finite numbers, more than
/// [`MAX_BYTES`] bytes or [`MAX_DEPTH`] levels are refused). The rules are those of
/// [`result_json_schema`] plus `min <= median <= max` for status `ok`, unique workload IDs
/// (`DuplicateWorkload`) and unique metric names per workload.
pub fn validate_result(bytes: &[u8]) -> Result<BenchResult, SchemaError> {
    let root = fid_canon::parse_with_limits(bytes, MAX_BYTES, MAX_DEPTH)
        .map_err(|e| parse_err(format!("not a JSON document: {}", e.code())))?;
    let top = obj(&root, "the document")?;
    match top.get("schema_version") {
        Some(CValue::Num(n)) if *n == f64::from(SUPPORTED_SCHEMA) => {}
        Some(CValue::Num(n)) if n.fract() == 0.0 && n.abs() < 9.0e15 => {
            // Integral and below 2^53 in magnitude, so the conversion is exact.
            let found = *n as i64;
            return Err(SchemaError::UnsupportedSchema { found });
        }
        Some(_) => return Err(parse_err("schema_version is not an integer".into())),
        None => return Err(parse_err("schema_version is missing".into())),
    }
    only_keys(top, &RESULT_KEYS, "the document")?;
    let context = read_context(field(top, "context", "the document")?)?;
    let list = arr(field(top, "workloads", "the document")?, "workloads")?;
    if list.is_empty() {
        return Err(parse_err("workloads is empty".into()));
    }
    let mut workloads: Vec<WorkloadResult> = Vec::new();
    for (i, v) in list.iter().enumerate() {
        let w = read_workload_result(i, v)?;
        if workloads.iter().any(|x| x.id == w.id) {
            return Err(SchemaError::DuplicateWorkload { id: w.id });
        }
        workloads.push(w);
    }
    Ok(BenchResult { schema_version: SUPPORTED_SCHEMA, context, workloads })
}

fn obj<'a>(v: &'a CValue, at: &str) -> Result<&'a BTreeMap<String, CValue>, SchemaError> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(parse_err(format!("{at} is not an object"))),
    }
}

fn arr<'a>(v: &'a CValue, at: &str) -> Result<&'a [CValue], SchemaError> {
    match v {
        CValue::Arr(a) => Ok(a),
        _ => Err(parse_err(format!("{at} is not an array"))),
    }
}

fn only_keys(m: &BTreeMap<String, CValue>, allowed: &[&str], at: &str) -> Result<(), SchemaError> {
    match m.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(parse_err(format!("unknown field `{k}` in {at}"))),
        None => Ok(()),
    }
}

fn field<'a>(m: &'a BTreeMap<String, CValue>, key: &str, at: &str) -> Result<&'a CValue, SchemaError> {
    m.get(key).ok_or_else(|| parse_err(format!("{key} is missing in {at}")))
}

fn text(m: &BTreeMap<String, CValue>, key: &str, at: &str) -> Result<String, SchemaError> {
    match field(m, key, at)? {
        CValue::Str(s) if !s.is_empty() => Ok(s.clone()),
        _ => Err(parse_err(format!("{key} in {at} is not a non-empty string"))),
    }
}

/// An integral JSON number in `lo..=hi`.
fn integer(v: &CValue, lo: u32, hi: u32) -> Option<u32> {
    match v {
        CValue::Num(n) if n.fract() == 0.0 && *n >= f64::from(lo) && *n <= f64::from(hi) => {
            // Integral and within u32 range, so the conversion is exact.
            Some(*n as u32)
        }
        _ => None,
    }
}

fn read_context(v: &CValue) -> Result<Context, SchemaError> {
    let at = "context";
    let m = obj(v, at)?;
    // Missing fields first, in document order, so the first error names the first gap.
    for k in CONTEXT_KEYS {
        field(m, k, at)?;
    }
    only_keys(m, &CONTEXT_KEYS, at)?;
    let os_name = text(m, "os", at)?;
    let os = OsFamily::from_name(&os_name).ok_or_else(|| {
        let all: Vec<&str> = OsFamily::ALL.iter().map(|o| o.as_str()).collect();
        parse_err(format!("os in context is not one of {}", all.join(", ")))
    })?;
    let arch_name = text(m, "arch", at)?;
    let arch = Arch::from_name(&arch_name).ok_or_else(|| {
        let all: Vec<&str> = Arch::ALL.iter().map(|a| a.as_str()).collect();
        parse_err(format!("arch in context is not one of {}", all.join(", ")))
    })?;
    let ram_mib = integer(field(m, "ram_mib", at)?, 1, u32::MAX)
        .ok_or_else(|| parse_err(format!("ram_mib in context is not an integer from 1 to {}", u32::MAX)))?;
    Ok(Context {
        suite_version: text(m, "suite_version", at)?,
        app_version: text(m, "app_version", at)?,
        os,
        os_version: text(m, "os_version", at)?,
        arch,
        cpu_model: text(m, "cpu_model", at)?,
        ram_mib,
        gpu_backend: text(m, "gpu_backend", at)?,
        adapter_name: text(m, "adapter_name", at)?,
        gpu_driver_version: text(m, "gpu_driver_version", at)?,
        machine: text(m, "machine", at)?,
    })
}

fn read_workload_result(i: usize, v: &CValue) -> Result<WorkloadResult, SchemaError> {
    let at = format!("workloads[{i}]");
    let m = obj(v, &at)?;
    only_keys(m, &WORKLOAD_RESULT_KEYS, &at)?;
    let id = match field(m, "id", &at)? {
        CValue::Str(s) if is_valid_name(s) => s.clone(),
        _ => return Err(parse_err(format!("id in {at} does not match {NAME_PATTERN}"))),
    };
    let at = format!("workload {id}");
    let list = arr(field(m, "metrics", &at)?, &format!("metrics in {at}"))?;
    if list.is_empty() {
        return Err(parse_err(format!("metrics in {at} is empty")));
    }
    let mut metrics: Vec<MetricResult> = Vec::new();
    for (j, mv) in list.iter().enumerate() {
        let r = read_metric_result(&id, j, mv)?;
        if metrics.iter().any(|x| x.name == r.name) {
            return Err(parse_err(format!("duplicate metric name `{}` in {at}", r.name)));
        }
        metrics.push(r);
    }
    Ok(WorkloadResult { id, metrics })
}

fn read_metric_result(workload: &str, j: usize, v: &CValue) -> Result<MetricResult, SchemaError> {
    let at = format!("workload {workload} metrics[{j}]");
    let m = obj(v, &at)?;
    if !m.contains_key("dataset_sha256") {
        return Err(SchemaError::MissingDigest { workload: workload.to_string() });
    }
    for k in METRIC_RESULT_KEYS {
        field(m, k, &at)?;
    }
    only_keys(m, &METRIC_RESULT_KEYS, &at)?;
    let name = match field(m, "name", &at)? {
        CValue::Str(s) if is_valid_name(s) => s.clone(),
        _ => return Err(parse_err(format!("name in {at} does not match {NAME_PATTERN}"))),
    };
    let unit = match field(m, "unit", &at)? {
        CValue::Str(s) => Unit::from_name(s).ok_or_else(|| SchemaError::BadUnit {
            workload: workload.to_string(),
            metric: name.clone(),
            unit: s.clone(),
        })?,
        _ => return Err(parse_err(format!("unit in {at} is not a string"))),
    };
    let dataset_sha256 = match field(m, "dataset_sha256", &at)? {
        CValue::Str(s) if s.is_empty() => return Err(SchemaError::MissingDigest { workload: workload.to_string() }),
        CValue::Str(s) => Digest::parse(s)
            .map_err(|_| parse_err(format!("dataset_sha256 in {at} is not sha256:<64 lowercase hex digits>")))?,
        _ => return Err(parse_err(format!("dataset_sha256 in {at} is not a string"))),
    };
    let reps = field(m, "repetitions", &at)?;
    let repetitions = match reps {
        CValue::Num(n) => match integer(reps, 1, 100).and_then(|r| u8::try_from(r).ok()) {
            Some(r) => r,
            None if n.fract() == 0.0 && n.abs() < 9.0e15 => {
                // Integral and below 2^53 in magnitude, so the conversion is exact.
                let found = *n as i64;
                return Err(SchemaError::BadRepetitions { workload: workload.to_string(), found });
            }
            None => return Err(parse_err(format!("repetitions in {at} is not an integer"))),
        },
        _ => return Err(parse_err(format!("repetitions in {at} is not an integer"))),
    };
    let status = match field(m, "status", &at)? {
        CValue::Str(s) => Status::from_name(s),
        _ => None,
    }
    .ok_or_else(|| parse_err(format!("status in {at} is not one of ok, timeout, failed, skipped")))?;
    let stat = |key: &str| -> Result<Option<f64>, SchemaError> {
        match field(m, key, &at)? {
            CValue::Null => Ok(None),
            CValue::Num(n) if *n >= 0.0 => Ok(Some(*n)),
            _ => Err(parse_err(format!("{key} in {at} is not a non-negative number or null"))),
        }
    };
    let (median, min, max) = (stat("median")?, stat("min")?, stat("max")?);
    match (status, median, min, max) {
        (Status::Ok, Some(md), Some(lo), Some(hi)) if lo <= md && md <= hi => {}
        (Status::Ok, Some(_), Some(_), Some(_)) => {
            return Err(parse_err(format!("{at}: min <= median <= max does not hold")));
        }
        (Status::Ok, _, _, _) => {
            return Err(parse_err(format!("{at}: status ok needs median, min and max")));
        }
        (_, None, None, None) => {}
        _ => return Err(parse_err(format!("{at}: status {status} needs median, min and max to be null"))),
    }
    Ok(MetricResult { name, unit, dataset_sha256, repetitions, median, min, max, status })
}

// ------------------------------------------------------------------ writing

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn num(v: f64) -> CValue {
    CValue::Num(v)
}

fn object<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn opt(v: Option<f64>) -> CValue {
    v.map_or(CValue::Null, CValue::Num)
}

impl BenchResult {
    /// The document as a JSON value (integers and statistics as plain JSON numbers).
    pub fn to_value(&self) -> CValue {
        let c = &self.context;
        let context = object([
            ("suite_version", s(&c.suite_version)),
            ("app_version", s(&c.app_version)),
            ("os", s(c.os.as_str())),
            ("os_version", s(&c.os_version)),
            ("arch", s(c.arch.as_str())),
            ("cpu_model", s(&c.cpu_model)),
            ("ram_mib", num(f64::from(c.ram_mib))),
            ("gpu_backend", s(&c.gpu_backend)),
            ("adapter_name", s(&c.adapter_name)),
            ("gpu_driver_version", s(&c.gpu_driver_version)),
            ("machine", s(&c.machine)),
        ]);
        let workloads = self
            .workloads
            .iter()
            .map(|w| {
                let metrics = w
                    .metrics
                    .iter()
                    .map(|m| {
                        object([
                            ("name", s(&m.name)),
                            ("unit", s(m.unit.as_str())),
                            ("dataset_sha256", s(m.dataset_sha256.as_str())),
                            ("repetitions", num(f64::from(m.repetitions))),
                            ("median", opt(m.median)),
                            ("min", opt(m.min)),
                            ("max", opt(m.max)),
                            ("status", s(m.status.as_str())),
                        ])
                    })
                    .collect();
                object([("id", s(&w.id)), ("metrics", CValue::Arr(metrics))])
            })
            .collect();
        object([
            ("schema_version", num(f64::from(self.schema_version))),
            ("context", context),
            ("workloads", CValue::Arr(workloads)),
        ])
    }

    /// RFC 8785 canonical JSON bytes: the same value always gives the same bytes. Fails only
    /// on a non-finite statistic.
    pub fn to_json(&self) -> Result<Vec<u8>, CanonError> {
        fid_canon::to_canonical_bytes(&self.to_value())
    }
}

// ------------------------------------------------------------------ JSON Schema export

fn names<T: Copy>(all: &[T], f: impl Fn(T) -> &'static str) -> CValue {
    CValue::Arr(all.iter().map(|&x| s(f(x))).collect())
}

fn required(keys: &[&str]) -> CValue {
    CValue::Arr(keys.iter().map(|k| s(k)).collect())
}

fn nonempty_string() -> CValue {
    object([("type", s("string")), ("minLength", num(1.0))])
}

fn name_string() -> CValue {
    object([("type", s("string")), ("pattern", s(NAME_PATTERN))])
}

fn closed_object(keys: &[&str], props: Vec<(&str, CValue)>) -> Vec<(String, CValue)> {
    vec![
        ("type".to_string(), s("object")),
        ("additionalProperties".to_string(), CValue::Bool(false)),
        ("required".to_string(), required(keys)),
        ("properties".to_string(), CValue::Obj(props.into_iter().map(|(k, v)| (k.to_string(), v)).collect())),
    ]
}

fn context_field_schema(key: &str) -> CValue {
    match key {
        "os" => object([("enum", names(OsFamily::ALL, OsFamily::as_str))]),
        "arch" => object([("enum", names(Arch::ALL, Arch::as_str))]),
        "ram_mib" => object([("type", s("integer")), ("minimum", num(1.0)), ("maximum", num(f64::from(u32::MAX)))]),
        _ => nonempty_string(),
    }
}

fn metric_result_schema() -> CValue {
    let stat = || object([("type", CValue::Arr(vec![s("number"), s("null")])), ("minimum", num(0.0))]);
    let props = vec![
        ("name", name_string()),
        ("unit", object([("enum", names(Unit::ALL, Unit::as_str))])),
        ("dataset_sha256", object([("type", s("string")), ("pattern", s(Digest::PATTERN))])),
        (
            "repetitions",
            object([
                ("type", s("integer")),
                ("minimum", num(f64::from(*REPETITIONS.start()))),
                ("maximum", num(f64::from(*REPETITIONS.end()))),
            ]),
        ),
        ("median", stat()),
        ("min", stat()),
        ("max", stat()),
        ("status", object([("enum", names(Status::ALL, Status::as_str))])),
    ];
    let typed = |t: &str| {
        let one = || object([("type", s(t))]);
        object([("properties", object([("median", one()), ("min", one()), ("max", one())]))])
    };
    let mut m = closed_object(&METRIC_RESULT_KEYS, props);
    m.push((
        "if".to_string(),
        object([
            ("properties", object([("status", object([("const", s(Status::Ok.as_str()))]))])),
            ("required", required(&["status"])),
        ]),
    ));
    m.push(("then".to_string(), typed("number")));
    m.push(("else".to_string(), typed("null")));
    CValue::Obj(m.into_iter().collect())
}

/// The JSON Schema (draft 2020-12) of the result document, generated from the same field
/// tables and enums the reader uses (REQ-PLAT.HW.SCHEMA-005). Cross-field rules a schema
/// cannot express (`min <= median <= max`, unique IDs and metric names) are checked by
/// [`validate_result`] only.
pub fn result_json_schema() -> CValue {
    let context = CValue::Obj(
        closed_object(&CONTEXT_KEYS, CONTEXT_KEYS.iter().map(|&k| (k, context_field_schema(k))).collect())
            .into_iter()
            .collect(),
    );
    let workload = CValue::Obj(
        closed_object(
            &WORKLOAD_RESULT_KEYS,
            vec![
                ("id", name_string()),
                ("metrics", object([("type", s("array")), ("minItems", num(1.0)), ("items", metric_result_schema())])),
            ],
        )
        .into_iter()
        .collect(),
    );
    let mut top = vec![
        ("$schema".to_string(), s(JSON_SCHEMA_DIALECT)),
        ("title".to_string(), s("Metrology Maven benchmark result, schema_version 1 (PLAT.HW.SCHEMA)")),
    ];
    top.extend(closed_object(
        &RESULT_KEYS,
        vec![
            ("schema_version", object([("const", num(f64::from(SUPPORTED_SCHEMA)))])),
            ("context", context),
            ("workloads", object([("type", s("array")), ("minItems", num(1.0)), ("items", workload)])),
        ],
    ));
    CValue::Obj(top.into_iter().collect())
}

/// The exported schema file: canonical JSON of [`result_json_schema`] plus a newline.
pub fn result_json_schema_bytes() -> Result<Vec<u8>, CanonError> {
    let mut out = fid_canon::to_canonical_bytes(&result_json_schema())?;
    out.push(b'\n');
    Ok(out)
}
