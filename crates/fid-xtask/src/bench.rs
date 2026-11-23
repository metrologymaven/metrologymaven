//! Provenance-capturing benchmark runner and history store (spec node `ECO.DATA.BENCH.RUN`).
//!
//! Subcommands:
//! - `cargo xtask bench run --suite <toml> [--workload <id>...] --machine-profile <id>
//!   --profiles <toml> --fid <path> --reps <n> --history <dir>
//!   [--catalogue <toml>] [--cache <dir>] [--offline] [--revision <40-hex>]
//!   [--build-profile <name>]`
//! - `cargo xtask bench compare <recordA> <recordB>`
//!
//! Links no product crate: invokes the built `fid` binary (`fid doctor run-benchmark`)
//! and `fid-data` (`data path`) as subprocesses, validates suites/profiles/results with
//! bounded parsers, writes `bench-record.json` (schema 1) atomically without ever
//! overwriting an existing history file.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_types::Digest;

/// Largest TOML suite / profiles file accepted (1 MiB).
pub const MAX_TOML_BYTES: usize = 1024 * 1024;
/// Largest JSON result / record document accepted (16 MiB).
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
/// Maximum JSON nesting depth accepted.
pub const MAX_JSON_DEPTH: usize = 32;
/// Minimum allowed repetition count (REQ-ECO.DATA.BENCH.RUN-003).
pub const MIN_REPS: u32 = 3;
/// Maximum allowed repetition count.
pub const MAX_REPS: u32 = 100;
/// `bench-record.json` schema version.
pub const RECORD_SCHEMA_VERSION: u32 = 1;
/// Harness version stamped on every record.
pub const HARNESS_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Error codes required by `ECO.DATA.BENCH.RUN`.
pub const ERR_REPS: &str = "BENCH-REPS";
pub const ERR_PROFILE: &str = "BENCH-PROFILE";
pub const ERR_SUITE: &str = "BENCH-SUITE";
pub const ERR_RESULT: &str = "BENCH-RESULT";
pub const ERR_IO: &str = "BENCH-IO";
pub const REASON_NOT_IN_CATALOGUE: &str = "NOT-IN-CATALOGUE";

const BENCH_USAGE: &str = "usage: cargo xtask bench run --suite <toml> [--workload <id>...] \
                           --machine-profile <id> --profiles <toml> --fid <path> --reps <n>=3 \
                           --history <dir> [--catalogue <toml>] [--cache <dir>] [--offline] \
                           [--revision <40-hex>] [--build-profile <name>]\n       \
                           cargo xtask bench compare <recordA> <recordB>";

/// Typed error carrying a stable `BENCH-*` code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchError {
    pub code: &'static str,
    pub detail: String,
}

impl BenchError {
    pub fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self { code, detail: detail.into() }
    }
}

impl fmt::Display for BenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for BenchError {}

/// One suite workload parsed from a `PLAT.HW.SCHEMA` suite TOML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteWorkload {
    pub id: String,
    pub dataset_id: String,
    pub dataset_sha256: Digest,
    pub metrics: Vec<(String, String)>,
    pub repetitions: u32,
    pub timeout_s: u32,
}

/// Validated suite manifest subset needed by `bench run`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSuite {
    pub suite_version: String,
    pub workloads: Vec<SuiteWorkload>,
}

/// Aggregated statistics for one metric over all repetitions (REQ-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricSummary {
    pub name: String,
    pub unit: String,
    pub median: String,
    pub min: String,
    pub max: String,
    pub rel_spread: String,
}

/// Full `bench-record.json` (schema 1) record written to the history store.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchRecord {
    pub schema: u32,
    pub benchmark_id: String,
    pub dataset_id: String,
    pub dataset_sha256: String,
    pub revision: String,
    pub build_profile: String,
    pub machine_profile_id: String,
    pub harness_version: String,
    pub repetitions: u32,
    pub status: String,
    pub reason: Option<String>,
    pub metrics: Vec<MetricSummary>,
    pub results: Vec<CValue>,
}

/// Options for `bench run`.
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub suite: PathBuf,
    pub workloads: Vec<String>,
    pub machine_profile: String,
    pub profiles: PathBuf,
    pub fid: PathBuf,
    pub reps: u32,
    pub history: PathBuf,
    pub catalogue: Option<PathBuf>,
    pub cache: Option<PathBuf>,
    pub offline: bool,
    pub revision: Option<String>,
    pub build_profile: String,
    /// Optional explicit `fid-data` binary path (tests can set this to avoid nested `cargo run`).
    pub fid_data_bin: Option<PathBuf>,
}

/// Result of comparing two `bench-record.json` files (REQ-006).
#[derive(Debug, Clone, PartialEq)]
pub struct CompareReport {
    pub benchmark_id: String,
    pub warnings: Vec<String>,
    pub metric_diffs: Vec<MetricDiff>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricDiff {
    pub name: String,
    pub unit: String,
    pub median_a: f64,
    pub median_b: f64,
    pub delta: f64,
    pub percent: Option<f64>,
}

// ------------------------------------------------------------------ bounded reading & parsing

fn read_bounded(path: &Path, limit: usize, err_code: &'static str) -> Result<Vec<u8>, BenchError> {
    let meta =
        fs::metadata(path).map_err(|e| BenchError::new(err_code, format!("cannot stat {}: {e}", path.display())))?;
    if meta.len() > limit as u64 {
        return Err(BenchError::new(err_code, format!("{} exceeds {} bytes", path.display(), limit)));
    }
    fs::read(path).map_err(|e| BenchError::new(err_code, format!("cannot read {}: {e}", path.display())))
}

/// Validates a `PLAT.HW.PROFILE` TOML file and returns its defined profile IDs.
pub fn parse_profile_ids(bytes: &[u8]) -> Result<BTreeSet<String>, BenchError> {
    if bytes.len() > MAX_TOML_BYTES {
        return Err(BenchError::new(ERR_PROFILE, format!("profiles file exceeds {MAX_TOML_BYTES} bytes")));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| BenchError::new(ERR_PROFILE, "profiles file is not UTF-8"))?;
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| BenchError::new(ERR_PROFILE, format!("malformed profiles TOML: {e}")))?;
    match table.get("schema_version") {
        Some(toml::Value::Integer(1)) => {}
        _ => return Err(BenchError::new(ERR_PROFILE, "profiles schema_version must be 1")),
    }
    let Some(toml::Value::Table(profiles)) = table.get("profiles") else {
        return Err(BenchError::new(ERR_PROFILE, "missing [profiles] table"));
    };
    Ok(profiles.keys().cloned().collect())
}

/// Validates a `PLAT.HW.SCHEMA` suite TOML file.
pub fn parse_suite(bytes: &[u8]) -> Result<ParsedSuite, BenchError> {
    if bytes.len() > MAX_TOML_BYTES {
        return Err(BenchError::new(ERR_SUITE, format!("suite file exceeds {MAX_TOML_BYTES} bytes")));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| BenchError::new(ERR_SUITE, "suite file is not UTF-8"))?;
    let table: toml::Table =
        text.parse().map_err(|e: toml::de::Error| BenchError::new(ERR_SUITE, format!("malformed suite TOML: {e}")))?;
    match table.get("schema_version") {
        Some(toml::Value::Integer(1)) => {}
        _ => return Err(BenchError::new(ERR_SUITE, "suite schema_version must be 1")),
    }
    let suite_version = match table.get("suite_version") {
        Some(toml::Value::String(s)) if !s.trim().is_empty() => s.clone(),
        _ => return Err(BenchError::new(ERR_SUITE, "missing non-empty suite_version")),
    };
    let Some(toml::Value::Array(raw_workloads)) = table.get("workloads") else {
        return Err(BenchError::new(ERR_SUITE, "missing [[workloads]] array"));
    };
    if raw_workloads.is_empty() {
        return Err(BenchError::new(ERR_SUITE, "suite has zero workloads"));
    }

    let mut seen_ids = BTreeSet::new();
    let mut workloads = Vec::with_capacity(raw_workloads.len());
    for (idx, item) in raw_workloads.iter().enumerate() {
        let Some(wt) = item.as_table() else {
            return Err(BenchError::new(ERR_SUITE, format!("workloads[{idx}] must be a table")));
        };
        let id = match wt.get("id") {
            Some(toml::Value::String(s)) if !s.trim().is_empty() && !s.contains('/') && !s.contains('\\') => s.clone(),
            _ => return Err(BenchError::new(ERR_SUITE, format!("workloads[{idx}] missing valid id"))),
        };
        if !seen_ids.insert(id.clone()) {
            return Err(BenchError::new(ERR_SUITE, format!("duplicate workload id: {id}")));
        }
        let Some(toml::Value::Table(ds)) = wt.get("dataset") else {
            return Err(BenchError::new(ERR_SUITE, format!("workload `{id}` missing dataset table")));
        };
        let dataset_id = match ds.get("id") {
            Some(toml::Value::String(s)) if !s.trim().is_empty() => s.clone(),
            _ => return Err(BenchError::new(ERR_SUITE, format!("workload `{id}` missing dataset.id"))),
        };
        let dataset_sha256 = match ds.get("sha256") {
            Some(toml::Value::String(s)) => Digest::parse(s)
                .map_err(|_| BenchError::new(ERR_SUITE, format!("workload `{id}` invalid dataset.sha256")))?,
            _ => return Err(BenchError::new(ERR_SUITE, format!("workload `{id}` missing dataset.sha256"))),
        };
        let repetitions = match wt.get("repetitions") {
            Some(toml::Value::Integer(n)) if (1..=i64::from(MAX_REPS)).contains(n) => *n as u32,
            _ => return Err(BenchError::new(ERR_SUITE, format!("workload `{id}` invalid repetitions"))),
        };
        let timeout_s = match wt.get("timeout_s") {
            Some(toml::Value::Integer(n)) if *n >= 1 && *n <= i64::from(u32::MAX) => *n as u32,
            _ => return Err(BenchError::new(ERR_SUITE, format!("workload `{id}` invalid timeout_s"))),
        };
        let Some(toml::Value::Array(raw_metrics)) = wt.get("metrics") else {
            return Err(BenchError::new(ERR_SUITE, format!("workload `{id}` missing metrics array")));
        };
        if raw_metrics.is_empty() {
            return Err(BenchError::new(ERR_SUITE, format!("workload `{id}` has zero metrics")));
        }
        let mut metrics = Vec::with_capacity(raw_metrics.len());
        for m in raw_metrics {
            let Some(mt) = m.as_table() else {
                return Err(BenchError::new(ERR_SUITE, format!("workload `{id}` metric must be a table")));
            };
            let name = match mt.get("name") {
                Some(toml::Value::String(s)) if !s.is_empty() => s.clone(),
                _ => return Err(BenchError::new(ERR_SUITE, format!("workload `{id}` metric missing name"))),
            };
            let unit = match mt.get("unit") {
                Some(toml::Value::String(s)) if !s.is_empty() => s.clone(),
                _ => return Err(BenchError::new(ERR_SUITE, format!("workload `{id}` metric `{name}` missing unit"))),
            };
            metrics.push((name, unit));
        }
        workloads.push(SuiteWorkload { id, dataset_id, dataset_sha256, metrics, repetitions, timeout_s });
    }
    Ok(ParsedSuite { suite_version, workloads })
}

struct ObservedMetric {
    name: String,
    unit: String,
    value: f64,
}

/// Validates a `PLAT.HW.SCHEMA` `BenchResult` JSON document and extracts observed metrics
/// for `workload_id`.
fn validate_and_extract_result(bytes: &[u8], workload_id: &str) -> Result<(CValue, Vec<ObservedMetric>), BenchError> {
    if bytes.len() > MAX_JSON_BYTES {
        return Err(BenchError::new(ERR_RESULT, format!("result JSON exceeds {MAX_JSON_BYTES} bytes")));
    }
    let val = parse_with_limits(bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH)
        .map_err(|e| BenchError::new(ERR_RESULT, format!("malformed result JSON: {e}")))?;
    let CValue::Obj(root) = &val else {
        return Err(BenchError::new(ERR_RESULT, "result root must be a JSON object"));
    };
    match root.get("schema_version") {
        Some(CValue::Num(n)) if (*n - 1.0).abs() == 0.0 => {}
        _ => return Err(BenchError::new(ERR_RESULT, "result schema_version must be 1")),
    }
    if !matches!(root.get("context"), Some(CValue::Obj(_))) {
        return Err(BenchError::new(ERR_RESULT, "result missing context object"));
    }
    let Some(CValue::Arr(workloads)) = root.get("workloads") else {
        return Err(BenchError::new(ERR_RESULT, "result missing workloads array"));
    };

    let mut target_metrics: Option<&[CValue]> = None;
    for w in workloads {
        let CValue::Obj(wo) = w else {
            return Err(BenchError::new(ERR_RESULT, "workload entry must be an object"));
        };
        if matches!(wo.get("id"), Some(CValue::Str(s)) if s == workload_id) {
            let Some(CValue::Arr(ms)) = wo.get("metrics") else {
                return Err(BenchError::new(ERR_RESULT, format!("workload `{workload_id}` missing metrics array")));
            };
            target_metrics = Some(ms.as_slice());
            break;
        }
    }
    let Some(ms) = target_metrics else {
        return Err(BenchError::new(ERR_RESULT, format!("result missing workload `{workload_id}`")));
    };

    let mut extracted = Vec::with_capacity(ms.len());
    for m in ms {
        let CValue::Obj(mo) = m else {
            return Err(BenchError::new(ERR_RESULT, "metric entry must be an object"));
        };
        let Some(CValue::Str(name)) = mo.get("name") else {
            return Err(BenchError::new(ERR_RESULT, "metric missing string name"));
        };
        let Some(CValue::Str(unit)) = mo.get("unit") else {
            return Err(BenchError::new(ERR_RESULT, format!("metric `{name}` missing string unit")));
        };
        let Some(CValue::Str(status)) = mo.get("status") else {
            return Err(BenchError::new(ERR_RESULT, format!("metric `{name}` missing status")));
        };
        if status != "ok" {
            return Err(BenchError::new(ERR_RESULT, format!("metric `{name}` status is `{status}`, expected `ok`")));
        }
        let Some(CValue::Num(med)) = mo.get("median") else {
            return Err(BenchError::new(ERR_RESULT, format!("metric `{name}` missing numeric median")));
        };
        if !med.is_finite() || *med < 0.0 {
            return Err(BenchError::new(ERR_RESULT, format!("metric `{name}` median must be finite and >= 0")));
        }
        extracted.push(ObservedMetric { name: name.clone(), unit: unit.clone(), value: *med });
    }
    Ok((val, extracted))
}

// ------------------------------------------------------------------ numeric formatting & aggregation

/// Formats a finite `f64` with up to 6 significant digits without trailing fractional zeros
/// (e.g., `0.04`, `100`, `1.23457`).
pub fn format_sig6(x: f64) -> String {
    if !x.is_finite() || x == 0.0 {
        return "0".to_string();
    }
    let abs = x.abs();
    let exp10 = abs.log10().floor() as i32;
    if (-4..=8).contains(&exp10) {
        let dec_places = (5 - exp10).max(0) as usize;
        let raw = format!("{x:.dec_places$}");
        trim_decimal_zeros(&raw)
    } else {
        let raw = format!("{x:.5e}");
        if let Some((mant, exp)) = raw.split_once('e') {
            format!("{}e{exp}", trim_decimal_zeros(mant))
        } else {
            raw
        }
    }
}

fn trim_decimal_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" || trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Formats an observed metric value cleanly as a decimal string.
pub fn format_metric_value(x: f64) -> String {
    if x == 0.0 {
        "0".to_string()
    } else if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        trim_decimal_zeros(&format!("{x:.12}"))
    }
}

/// Computes `(min, median, max, rel_spread)` deterministically over non-empty finite values.
pub fn aggregate_samples(samples: &[f64]) -> (f64, f64, f64, f64) {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    let min = sorted[0];
    let max = sorted[n - 1];
    let median = if n % 2 == 1 { sorted[n / 2] } else { 0.5 * (sorted[n / 2 - 1] + sorted[n / 2]) };
    let rel_spread = if median == 0.0 { 0.0 } else { (max - min) / median };
    (min, median, max, rel_spread)
}

// ------------------------------------------------------------------ dataset lookup via fid-data

fn resolve_dataset_via_fid_data(opts: &RunOptions, dataset_id: &str) -> Result<PathBuf, String> {
    let mut cmd = if let Some(bin) = &opts.fid_data_bin {
        Command::new(bin)
    } else {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
        let mut c = Command::new(cargo);
        c.args(["run", "-q", "--manifest-path", manifest, "-p", "fid-data", "--bin", "fid-data", "--"]);
        c
    };

    cmd.args(["data", "path", dataset_id]);
    if let Some(cat) = &opts.catalogue {
        cmd.arg("--catalogue").arg(cat);
    }
    if let Some(cache) = &opts.cache {
        cmd.arg("--cache").arg(cache);
    }
    if opts.offline {
        cmd.arg("--offline");
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return Err("FETCH-IO".to_string()),
    };

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(line) = stdout.lines().find(|l| !l.trim().is_empty()) {
            return Ok(PathBuf::from(line.trim()));
        }
        return Err("FETCH-IO".to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = stderr
        .lines()
        .find_map(|line| {
            let tok = line.split_whitespace().next()?;
            if tok.starts_with("FETCH-") || tok.starts_with("DATA-") {
                Some(tok.to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "FETCH-IO".to_string());

    if code == "FETCH-UNKNOWN-ID" {
        Err(REASON_NOT_IN_CATALOGUE.to_string())
    } else {
        Err(code)
    }
}

fn command_for_fid(fid: &Path) -> Command {
    #[cfg(windows)]
    {
        if let Ok(bytes) = fs::read(fid) {
            if bytes.starts_with(b"#!")
                && bytes.iter().take(64).copied().collect::<Vec<u8>>().windows(6).any(|w| w == b"python")
            {
                let mut cmd = Command::new("python");
                cmd.arg(fid);
                return cmd;
            }
        }
    }
    Command::new(fid)
}

fn detect_revision(explicit: Option<&str>) -> String {
    if let Some(rev) = explicit {
        if rev.len() == 40 && rev.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return rev.to_string();
        }
    }
    if let Ok(out) = Command::new("git").args(["rev-parse", "HEAD"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.len() == 40 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                return s;
            }
        }
    }
    "0000000000000000000000000000000000000000".to_string()
}

// ------------------------------------------------------------------ JSON codec for bench-record.json

impl BenchRecord {
    pub fn to_value(&self) -> CValue {
        let mut ds = BTreeMap::new();
        ds.insert("id".to_string(), CValue::Str(self.dataset_id.clone()));
        ds.insert("sha256".to_string(), CValue::Str(self.dataset_sha256.clone()));

        let metrics_arr: Vec<CValue> = self
            .metrics
            .iter()
            .map(|m| {
                let mut o = BTreeMap::new();
                o.insert("max".to_string(), CValue::Str(m.max.clone()));
                o.insert("median".to_string(), CValue::Str(m.median.clone()));
                o.insert("min".to_string(), CValue::Str(m.min.clone()));
                o.insert("name".to_string(), CValue::Str(m.name.clone()));
                o.insert("rel_spread".to_string(), CValue::Str(m.rel_spread.clone()));
                o.insert("unit".to_string(), CValue::Str(m.unit.clone()));
                CValue::Obj(o)
            })
            .collect();

        let mut root = BTreeMap::new();
        root.insert("benchmark_id".to_string(), CValue::Str(self.benchmark_id.clone()));
        root.insert("build_profile".to_string(), CValue::Str(self.build_profile.clone()));
        root.insert("dataset".to_string(), CValue::Obj(ds));
        root.insert("harness_version".to_string(), CValue::Str(self.harness_version.clone()));
        root.insert("machine_profile_id".to_string(), CValue::Str(self.machine_profile_id.clone()));
        root.insert("metrics".to_string(), CValue::Arr(metrics_arr));
        if let Some(reason) = &self.reason {
            root.insert("reason".to_string(), CValue::Str(reason.clone()));
        }
        root.insert("repetitions".to_string(), CValue::Num(f64::from(self.repetitions)));
        root.insert("results".to_string(), CValue::Arr(self.results.clone()));
        root.insert("revision".to_string(), CValue::Str(self.revision.clone()));
        root.insert("schema".to_string(), CValue::Num(f64::from(self.schema)));
        root.insert("status".to_string(), CValue::Str(self.status.clone()));
        CValue::Obj(root)
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, BenchError> {
        let mut bytes = to_canonical_bytes(&self.to_value())
            .map_err(|_| BenchError::new(ERR_IO, "failed to encode bench-record.json"))?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, BenchError> {
        if bytes.len() > MAX_JSON_BYTES {
            return Err(BenchError::new(ERR_RESULT, format!("record exceeds {MAX_JSON_BYTES} bytes")));
        }
        let val = parse_with_limits(bytes, MAX_JSON_BYTES, MAX_JSON_DEPTH)
            .map_err(|e| BenchError::new(ERR_RESULT, format!("malformed record JSON: {e}")))?;
        let CValue::Obj(root) = val else {
            return Err(BenchError::new(ERR_RESULT, "record root must be an object"));
        };
        let schema = match root.get("schema") {
            Some(CValue::Num(n)) if (*n - 1.0).abs() == 0.0 => 1,
            _ => return Err(BenchError::new(ERR_RESULT, "record schema must be 1")),
        };
        let req_str = |k: &str| -> Result<String, BenchError> {
            match root.get(k) {
                Some(CValue::Str(s)) => Ok(s.clone()),
                _ => Err(BenchError::new(ERR_RESULT, format!("record missing string field `{k}`"))),
            }
        };
        let benchmark_id = req_str("benchmark_id")?;
        let build_profile = req_str("build_profile")?;
        let harness_version = req_str("harness_version")?;
        let machine_profile_id = req_str("machine_profile_id")?;
        let revision = req_str("revision")?;
        let status = req_str("status")?;
        let reason = match root.get("reason") {
            None | Some(CValue::Null) => None,
            Some(CValue::Str(s)) => Some(s.clone()),
            _ => return Err(BenchError::new(ERR_RESULT, "record reason must be string")),
        };
        let repetitions = match root.get("repetitions") {
            Some(CValue::Num(n)) if *n >= 0.0 && n.fract() == 0.0 => *n as u32,
            _ => return Err(BenchError::new(ERR_RESULT, "record missing repetitions")),
        };
        let Some(CValue::Obj(ds)) = root.get("dataset") else {
            return Err(BenchError::new(ERR_RESULT, "record missing dataset object"));
        };
        let dataset_id = match ds.get("id") {
            Some(CValue::Str(s)) => s.clone(),
            _ => return Err(BenchError::new(ERR_RESULT, "record missing dataset.id")),
        };
        let dataset_sha256 = match ds.get("sha256") {
            Some(CValue::Str(s)) => s.clone(),
            _ => return Err(BenchError::new(ERR_RESULT, "record missing dataset.sha256")),
        };
        let Some(CValue::Arr(raw_metrics)) = root.get("metrics") else {
            return Err(BenchError::new(ERR_RESULT, "record missing metrics array"));
        };
        let mut metrics = Vec::with_capacity(raw_metrics.len());
        for m in raw_metrics {
            let CValue::Obj(mo) = m else {
                return Err(BenchError::new(ERR_RESULT, "metric must be an object"));
            };
            let mstr = |k: &str| -> Result<String, BenchError> {
                match mo.get(k) {
                    Some(CValue::Str(s)) => Ok(s.clone()),
                    _ => Err(BenchError::new(ERR_RESULT, format!("metric missing string `{k}`"))),
                }
            };
            metrics.push(MetricSummary {
                name: mstr("name")?,
                unit: mstr("unit")?,
                median: mstr("median")?,
                min: mstr("min")?,
                max: mstr("max")?,
                rel_spread: mstr("rel_spread")?,
            });
        }
        let results = match root.get("results") {
            Some(CValue::Arr(a)) => a.clone(),
            _ => return Err(BenchError::new(ERR_RESULT, "record missing results array")),
        };
        Ok(BenchRecord {
            schema,
            benchmark_id,
            dataset_id,
            dataset_sha256,
            revision,
            build_profile,
            machine_profile_id,
            harness_version,
            repetitions,
            status,
            reason,
            metrics,
            results,
        })
    }
}

// ------------------------------------------------------------------ atomic non-overwriting history write

/// Writes `bytes` atomically to `<dir>/<stem>.json` (or `<stem>-1.json`, `<stem>-2.json`, ...)
/// without ever overwriting an existing record (REQ-ECO.DATA.BENCH.RUN-005).
fn write_history_record(dir: &Path, workload_id: &str, bytes: &[u8]) -> Result<PathBuf, BenchError> {
    fs::create_dir_all(dir)
        .map_err(|e| BenchError::new(ERR_IO, format!("cannot create history dir {}: {e}", dir.display())))?;

    for suffix in 0u32..10_000 {
        let filename = if suffix == 0 { format!("{workload_id}.json") } else { format!("{workload_id}-{suffix}.json") };
        let final_path = dir.join(&filename);
        if final_path.exists() {
            continue;
        }
        let tmp_name = format!(".{filename}.{}.{suffix}.tmp", std::process::id());
        let tmp_path = dir.join(tmp_name);
        let write_res = (|| -> std::io::Result<()> {
            let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp_path)?;
            f.write_all(bytes)?;
            f.sync_all()?;
            Ok(())
        })();
        if let Err(e) = write_res {
            let _ = fs::remove_file(&tmp_path);
            return Err(BenchError::new(ERR_IO, format!("cannot write temp file {}: {e}", tmp_path.display())));
        }
        // Hard-link into final_path fails with AlreadyExists if raced, preserving no-overwrite invariant.
        match fs::hard_link(&tmp_path, &final_path) {
            Ok(()) => {
                let _ = fs::remove_file(&tmp_path);
                return Ok(final_path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&tmp_path);
                continue;
            }
            Err(_) => {
                // Fallback on filesystems without hard links: re-check existence and rename.
                if final_path.exists() {
                    let _ = fs::remove_file(&tmp_path);
                    continue;
                }
                fs::rename(&tmp_path, &final_path).map_err(|e| {
                    let _ = fs::remove_file(&tmp_path);
                    BenchError::new(
                        ERR_IO,
                        format!("cannot rename {} -> {}: {e}", tmp_path.display(), final_path.display()),
                    )
                })?;
                return Ok(final_path);
            }
        }
    }
    Err(BenchError::new(ERR_IO, format!("exhausted numeric suffixes for workload `{workload_id}`")))
}

// ------------------------------------------------------------------ core operations

/// Runs selected suite workloads and writes provenance records to `opts.history`.
pub fn run_suite(opts: &RunOptions) -> Result<Vec<PathBuf>, BenchError> {
    // 1. Validate reps >= 3 before running anything (REQ-003).
    if opts.reps < MIN_REPS || opts.reps > MAX_REPS {
        return Err(BenchError::new(
            ERR_REPS,
            format!("--reps must be between {MIN_REPS} and {MAX_REPS}, got {}", opts.reps),
        ));
    }

    // 2. Validate machine_profile_id exists in profiles TOML before running anything (REQ-004).
    let prof_bytes = read_bounded(&opts.profiles, MAX_TOML_BYTES, ERR_PROFILE)?;
    let profile_ids = parse_profile_ids(&prof_bytes)?;
    if !profile_ids.contains(&opts.machine_profile) {
        return Err(BenchError::new(
            ERR_PROFILE,
            format!("machine profile `{}` not found in {}", opts.machine_profile, opts.profiles.display()),
        ));
    }

    // 3. Validate suite TOML.
    let suite_bytes = read_bounded(&opts.suite, MAX_TOML_BYTES, ERR_SUITE)?;
    let suite = parse_suite(&suite_bytes)?;

    let selected: Vec<&SuiteWorkload> = if opts.workloads.is_empty() {
        suite.workloads.iter().collect()
    } else {
        let mut out = Vec::new();
        for wid in &opts.workloads {
            let Some(w) = suite.workloads.iter().find(|w| &w.id == wid) else {
                return Err(BenchError::new(ERR_SUITE, format!("unknown --workload `{wid}`")));
            };
            out.push(w);
        }
        out
    };

    let revision = detect_revision(opts.revision.as_deref());
    let target_dir = opts.history.join(&revision).join(&opts.machine_profile);
    let mut written = Vec::with_capacity(selected.len());

    for workload in selected {
        // 4. Verify dataset through fid-data; on failure emit `status = "not-run"` without calling fid (REQ-002).
        let dataset_path = match resolve_dataset_via_fid_data(opts, &workload.dataset_id) {
            Ok(p) => p,
            Err(reason) => {
                let record = BenchRecord {
                    schema: RECORD_SCHEMA_VERSION,
                    benchmark_id: workload.id.clone(),
                    dataset_id: workload.dataset_id.clone(),
                    dataset_sha256: workload.dataset_sha256.as_str().to_string(),
                    revision: revision.clone(),
                    build_profile: opts.build_profile.clone(),
                    machine_profile_id: opts.machine_profile.clone(),
                    harness_version: HARNESS_VERSION.to_string(),
                    repetitions: opts.reps,
                    status: "not-run".to_string(),
                    reason: Some(reason),
                    metrics: Vec::new(),
                    results: Vec::new(),
                };
                let path = write_history_record(&target_dir, &workload.id, &record.to_canonical_bytes()?)?;
                written.push(path);
                continue;
            }
        };

        // 5. Run `<fid> doctor run-benchmark --workload <id> --dataset <path> --json` `reps` times.
        let mut raw_results = Vec::with_capacity(opts.reps as usize);
        let mut metric_order: Vec<(String, String)> = Vec::new();
        let mut samples_by_metric: BTreeMap<String, Vec<f64>> = BTreeMap::new();

        for rep in 0..opts.reps {
            let out = command_for_fid(&opts.fid)
                .args(["doctor", "run-benchmark", "--workload", &workload.id, "--dataset"])
                .arg(&dataset_path)
                .arg("--json")
                .output()
                .map_err(|e| {
                    BenchError::new(
                        ERR_IO,
                        format!(
                            "cannot execute `{}` for workload `{}` rep {}: {e}",
                            opts.fid.display(),
                            workload.id,
                            rep + 1
                        ),
                    )
                })?;
            if !out.status.success() {
                return Err(BenchError::new(
                    ERR_RESULT,
                    format!(
                        "fid exited {} for workload `{}` rep {}: {}",
                        out.status.code().unwrap_or(-1),
                        workload.id,
                        rep + 1,
                        String::from_utf8_lossy(&out.stderr).trim()
                    ),
                ));
            }
            let (doc, extracted) = validate_and_extract_result(&out.stdout, &workload.id)?;
            if rep == 0 {
                for m in &extracted {
                    metric_order.push((m.name.clone(), m.unit.clone()));
                }
            }
            for m in extracted {
                samples_by_metric.entry(m.name).or_default().push(m.value);
            }
            raw_results.push(doc);
        }

        let mut summaries = Vec::with_capacity(metric_order.len());
        for (name, unit) in metric_order {
            let Some(vals) = samples_by_metric.get(&name) else { continue };
            if vals.len() != opts.reps as usize {
                return Err(BenchError::new(
                    ERR_RESULT,
                    format!("workload `{}` metric `{name}` missing in one or more repetitions", workload.id),
                ));
            }
            let (min, med, max, spread) = aggregate_samples(vals);
            summaries.push(MetricSummary {
                name,
                unit,
                median: format_metric_value(med),
                min: format_metric_value(min),
                max: format_metric_value(max),
                rel_spread: format_sig6(spread),
            });
        }

        let record = BenchRecord {
            schema: RECORD_SCHEMA_VERSION,
            benchmark_id: workload.id.clone(),
            dataset_id: workload.dataset_id.clone(),
            dataset_sha256: workload.dataset_sha256.as_str().to_string(),
            revision: revision.clone(),
            build_profile: opts.build_profile.clone(),
            machine_profile_id: opts.machine_profile.clone(),
            harness_version: HARNESS_VERSION.to_string(),
            repetitions: opts.reps,
            status: "ok".to_string(),
            reason: None,
            metrics: summaries,
            results: raw_results,
        };
        let path = write_history_record(&target_dir, &workload.id, &record.to_canonical_bytes()?)?;
        written.push(path);
    }

    Ok(written)
}

/// Compares two `bench-record.json` records of the same workload (REQ-ECO.DATA.BENCH.RUN-006).
pub fn compare_records(a: &BenchRecord, b: &BenchRecord) -> Result<CompareReport, BenchError> {
    if a.benchmark_id != b.benchmark_id {
        return Err(BenchError::new(
            ERR_RESULT,
            format!("cannot compare different workloads: `{}` vs `{}`", a.benchmark_id, b.benchmark_id),
        ));
    }

    let mut warnings = Vec::new();
    if a.dataset_sha256 != b.dataset_sha256 {
        warnings.push("BENCH-INCOMPARABLE dataset.sha256".to_string());
    }
    if a.machine_profile_id != b.machine_profile_id {
        warnings.push("BENCH-INCOMPARABLE machine_profile_id".to_string());
    }
    if a.build_profile != b.build_profile {
        warnings.push("BENCH-INCOMPARABLE build_profile".to_string());
    }

    let b_map: BTreeMap<&str, &MetricSummary> = b.metrics.iter().map(|m| (m.name.as_str(), m)).collect();
    let mut metric_diffs = Vec::new();
    for ma in &a.metrics {
        let Some(mb) = b_map.get(ma.name.as_str()) else { continue };
        let va: f64 = ma
            .median
            .parse()
            .map_err(|_| BenchError::new(ERR_RESULT, format!("non-numeric median `{}` in record A", ma.median)))?;
        let vb: f64 = mb
            .median
            .parse()
            .map_err(|_| BenchError::new(ERR_RESULT, format!("non-numeric median `{}` in record B", mb.median)))?;
        let delta = vb - va;
        let percent = if va == 0.0 { None } else { Some((delta / va) * 100.0) };
        metric_diffs.push(MetricDiff {
            name: ma.name.clone(),
            unit: ma.unit.clone(),
            median_a: va,
            median_b: vb,
            delta,
            percent,
        });
    }

    Ok(CompareReport { benchmark_id: a.benchmark_id.clone(), warnings, metric_diffs })
}

/// Formats a [`CompareReport`] deterministically for stdout.
pub fn format_compare_report(report: &CompareReport) -> String {
    let mut out = String::new();
    for w in &report.warnings {
        out.push_str(w);
        out.push('\n');
    }
    out.push_str(&format!("workload: {}\n", report.benchmark_id));
    for d in &report.metric_diffs {
        let sign = if d.delta >= 0.0 { "+" } else { "" };
        let pct_str = match d.percent {
            Some(p) => format!("{}{}%", if p >= 0.0 { "+" } else { "" }, format_sig6(p)),
            None => "n/a".to_string(),
        };
        out.push_str(&format!(
            "  {}: {} -> {} {} ({}{} {}, {})\n",
            d.name,
            format_metric_value(d.median_a),
            format_metric_value(d.median_b),
            d.unit,
            sign,
            format_sig6(d.delta),
            d.unit,
            pct_str
        ));
    }
    out
}

// ------------------------------------------------------------------ CLI entry point

pub fn cli(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("run") => cli_run(&args[1..]),
        Some("compare") => cli_compare(&args[1..]),
        _ => {
            eprintln!("{BENCH_USAGE}");
            ExitCode::from(2)
        }
    }
}

fn cli_run(args: &[String]) -> ExitCode {
    let mut suite: Option<PathBuf> = None;
    let mut workloads: Vec<String> = Vec::new();
    let mut machine_profile: Option<String> = None;
    let mut profiles: Option<PathBuf> = None;
    let mut fid: Option<PathBuf> = None;
    let mut reps: u32 = MIN_REPS;
    let mut history: Option<PathBuf> = None;
    let mut catalogue: Option<PathBuf> = None;
    let mut cache: Option<PathBuf> = None;
    let mut offline = false;
    let mut revision: Option<String> = None;
    let mut build_profile = "release".to_string();

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--suite" => {
                let Some(v) = it.next() else {
                    eprintln!("bench run: missing value for --suite\n{BENCH_USAGE}");
                    return ExitCode::from(2);
                };
                suite = Some(PathBuf::from(v));
            }
            "--workload" => {
                let Some(v) = it.next() else {
                    eprintln!("bench run: missing value for --workload\n{BENCH_USAGE}");
                    return ExitCode::from(2);
                };
                workloads.push(v.clone());
            }
            "--machine-profile" => {
                let Some(v) = it.next() else {
                    eprintln!("bench run: missing value for --machine-profile\n{BENCH_USAGE}");
                    return ExitCode::from(2);
                };
                machine_profile = Some(v.clone());
            }
            "--profiles" => {
                let Some(v) = it.next() else {
                    eprintln!("bench run: missing value for --profiles\n{BENCH_USAGE}");
                    return ExitCode::from(2);
                };
                profiles = Some(PathBuf::from(v));
            }
            "--fid" => {
                let Some(v) = it.next() else {
                    eprintln!("bench run: missing value for --fid\n{BENCH_USAGE}");
                    return ExitCode::from(2);
                };
                fid = Some(PathBuf::from(v));
            }
            "--reps" => {
                let Some(v) = it.next() else {
                    eprintln!("bench run: missing value for --reps\n{BENCH_USAGE}");
                    return ExitCode::from(2);
                };
                let Ok(n) = v.parse::<u32>() else {
                    eprintln!("{ERR_REPS}: invalid integer for --reps: {v}");
                    return ExitCode::from(1);
                };
                reps = n;
            }
            "--history" => {
                let Some(v) = it.next() else {
                    eprintln!("bench run: missing value for --history\n{BENCH_USAGE}");
                    return ExitCode::from(2);
                };
                history = Some(PathBuf::from(v));
            }
            "--catalogue" => {
                let Some(v) = it.next() else {
                    eprintln!("bench run: missing value for --catalogue\n{BENCH_USAGE}");
                    return ExitCode::from(2);
                };
                catalogue = Some(PathBuf::from(v));
            }
            "--cache" => {
                let Some(v) = it.next() else {
                    eprintln!("bench run: missing value for --cache\n{BENCH_USAGE}");
                    return ExitCode::from(2);
                };
                cache = Some(PathBuf::from(v));
            }
            "--offline" => offline = true,
            "--revision" => {
                let Some(v) = it.next() else {
                    eprintln!("bench run: missing value for --revision\n{BENCH_USAGE}");
                    return ExitCode::from(2);
                };
                revision = Some(v.clone());
            }
            "--build-profile" => {
                let Some(v) = it.next() else {
                    eprintln!("bench run: missing value for --build-profile\n{BENCH_USAGE}");
                    return ExitCode::from(2);
                };
                build_profile = v.clone();
            }
            other => {
                eprintln!("bench run: unknown argument `{other}`\n{BENCH_USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    let (Some(suite), Some(machine_profile), Some(profiles), Some(fid), Some(history)) =
        (suite, machine_profile, profiles, fid, history)
    else {
        eprintln!(
            "bench run: --suite, --machine-profile, --profiles, --fid, and --history are required\n{BENCH_USAGE}"
        );
        return ExitCode::from(2);
    };

    let opts = RunOptions {
        suite,
        workloads,
        machine_profile,
        profiles,
        fid,
        reps,
        history,
        catalogue,
        cache,
        offline,
        revision,
        build_profile,
        fid_data_bin: std::env::var_os("FID_DATA_BIN").map(PathBuf::from),
    };

    match run_suite(&opts) {
        Ok(paths) => {
            for p in paths {
                println!("{}", p.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

fn cli_compare(args: &[String]) -> ExitCode {
    if args.len() != 2 {
        eprintln!("bench compare: expected two record paths\n{BENCH_USAGE}");
        return ExitCode::from(2);
    }
    let a_bytes = match read_bounded(Path::new(&args[0]), MAX_JSON_BYTES, ERR_IO) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let b_bytes = match read_bounded(Path::new(&args[1]), MAX_JSON_BYTES, ERR_IO) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let a = match BenchRecord::from_json(&a_bytes) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let b = match BenchRecord::from_json(&b_bytes) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    match compare_records(&a, &b) {
        Ok(report) => {
            print!("{}", format_compare_report(&report));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
