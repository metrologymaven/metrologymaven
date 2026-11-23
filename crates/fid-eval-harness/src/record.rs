//! Run records, exact integer repetition statistics and canonical suite report
//! (spec node `AGENT.HARNESS.RECORD`).
//!
//! Public aggregation entry point ([`aggregate_runs`], [`report_dir`]) reused by
//! `AGENT.BOARD.RECOMP` so scoreboard recomputation uses the exact same integer arithmetic.
//!
//! Pure `D0` integer arithmetic (`u64`/`u128`, zero floating-point rounding), RFC 8785
//! canonical JSON via `fid-canon`, `BTreeMap` only, bounded parsing, zero panics.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_types::Digest;

/// Wire format tag for `run-record.json` v1.
pub const RUN_RECORD_FORMAT: &str = "run-record/v1";
/// Wire format tag for `suite-report.json` v1.
pub const SUITE_REPORT_FORMAT: &str = "suite-report/v1";
/// Minimum runs per `(task, model, adapter)` group required for `publishable: true` (REQ-004).
pub const MIN_PUBLISHABLE_RUNS: u64 = 3;
/// Largest accepted `run-record.json` file (4 MiB).
pub const MAX_RECORD_BYTES: usize = 4 * 1024 * 1024;
/// Maximum JSON nesting depth for a run record.
pub const MAX_RECORD_DEPTH: usize = 32;
/// Maximum number of run records loaded from one directory scan.
pub const MAX_RUN_RECORDS: usize = 100_000;

/// Safety categories tracked in every run record (`AGENT.HARNESS.SAFETY`).
pub const SAFETY_CATEGORIES: [&str; 5] = ["forbidden_class", "out_of_root", "raw_path", "redteam", "self_adoption"];

/// Agent-supplied text wrapped with mandatory untrusted provenance marking (REQ-002, C6).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UntrustedText {
    pub text: String,
}

impl UntrustedText {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("text".to_string(), CValue::Str(self.text.clone()));
        m.insert("untrusted".to_string(), CValue::Bool(true));
        CValue::Obj(m)
    }

    pub fn from_cvalue(v: &CValue, path: &str, field: &str) -> Result<Self, RecordError> {
        let CValue::Obj(m) = v else {
            return Err(bad(path, format!("{field}: must be an untrusted text object")));
        };
        if m.len() != 2 {
            return Err(bad(path, format!("{field}: untrusted object must have only keys `text` and `untrusted`")));
        }
        match m.get("untrusted") {
            Some(CValue::Bool(true)) => {}
            _ => return Err(bad(path, format!("{field}.untrusted: must be true"))),
        }
        let Some(CValue::Str(text)) = m.get("text") else {
            return Err(bad(path, format!("{field}.text: must be a string")));
        };
        Ok(Self { text: text.clone() })
    }
}

/// Reason a harness run stopped (REQ-AGENT.HARNESS.RUN-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StopReason {
    AgentExit,
    WallTime,
    ToolCalls,
    HarnessError,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::AgentExit => "agent_exit",
            StopReason::WallTime => "wall_time",
            StopReason::ToolCalls => "tool_calls",
            StopReason::HarnessError => "harness_error",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "agent_exit" => Some(StopReason::AgentExit),
            "wall_time" => Some(StopReason::WallTime),
            "tool_calls" => Some(StopReason::ToolCalls),
            "harness_error" => Some(StopReason::HarnessError),
            _ => None,
        }
    }
}

/// Token count reported by an adapter, or `not_reported` (REQ-001, REQ-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TokenCount {
    Reported(u64),
    NotReported,
}

impl TokenCount {
    pub fn to_cvalue(self) -> CValue {
        match self {
            TokenCount::Reported(n) => CValue::U64(n),
            TokenCount::NotReported => CValue::Str("not_reported".to_string()),
        }
    }

    pub fn from_cvalue(v: &CValue, path: &str, field: &str) -> Result<Self, RecordError> {
        match v {
            CValue::Str(s) if s == "not_reported" => Ok(TokenCount::NotReported),
            _ => Ok(TokenCount::Reported(read_u64(v, path, field)?)),
        }
    }
}

/// Per-category safety attempt and violation counts recorded on one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CategoryCounts {
    pub attempts: u64,
    pub violations: u64,
}

/// Canonical `run-record.json` (v1) for one harness run (REQ-001, REQ-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedRunRecord {
    pub run_id: String,
    pub task_id: String,
    pub harness_version: String,
    pub suite_version: String,
    pub adapter_name: String,
    pub adapter_version: String,
    pub declared_model: UntrustedText,
    pub declared_settings: BTreeMap<String, UntrustedText>,
    pub stop_reason: StopReason,
    pub success: bool,
    pub adoptable: bool,
    pub safety: BTreeMap<String, CategoryCounts>,
    pub tool_calls: u64,
    pub wall_ms: u64,
    pub tokens_in: TokenCount,
    pub tokens_out: TokenCount,
    pub artifact_digests: BTreeMap<String, Digest>,
}

/// Reduced exact rational fraction `num / den` (`den >= 1`, `gcd(num, den) == 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fraction {
    pub num: u64,
    pub den: u64,
}

impl Fraction {
    /// Builds a reduced fraction from `u128` numerator and non-zero `u128` denominator.
    pub fn reduced_u128(num: u128, den: u128) -> Result<Self, RecordError> {
        if den == 0 {
            return Err(bad("<aggregate>", "division by zero in fraction"));
        }
        if num == 0 {
            return Ok(Self { num: 0, den: 1 });
        }
        let g = gcd_u128(num, den);
        let rn = num / g;
        let rd = den / g;
        let num_u64 = u64::try_from(rn).map_err(|_| bad("<aggregate>", "fraction numerator overflow"))?;
        let den_u64 = u64::try_from(rd).map_err(|_| bad("<aggregate>", "fraction denominator overflow"))?;
        Ok(Self { num: num_u64, den: den_u64 })
    }

    /// Builds a reduced fraction from `u64` numerator and non-zero `u64` denominator.
    pub fn reduced(num: u64, den: u64) -> Result<Self, RecordError> {
        Self::reduced_u128(u128::from(num), u128::from(den))
    }

    /// Renders `num / den` rounded half-to-even to exactly 4 decimal places using pure `u128`
    /// integer arithmetic (REQ-003, REQ-006; D0 deterministic).
    pub fn to_decimal_4(self) -> String {
        let den = u128::from(self.den.max(1));
        let scaled = u128::from(self.num).saturating_mul(10_000);
        let mut q = scaled / den;
        let r = scaled % den;
        let twice_r = r.saturating_mul(2);
        if twice_r > den || (twice_r == den && q % 2 == 1) {
            q = q.saturating_add(1);
        }
        let whole = q / 10_000;
        let frac = q % 10_000;
        format!("{whole}.{frac:04}")
    }

    pub fn to_cvalue(self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("decimal".to_string(), CValue::Str(self.to_decimal_4()));
        m.insert("den".to_string(), CValue::U64(self.den));
        m.insert("num".to_string(), CValue::U64(self.num));
        CValue::Obj(m)
    }
}

fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Exact integer aggregation statistics: exact rational mean, minimum, and maximum (REQ-003, -005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub mean: Fraction,
    pub min: u64,
    pub max: u64,
}

impl Stats {
    pub fn from_values(values: &[u64]) -> Result<Self, RecordError> {
        let Some(&first) = values.first() else {
            return Err(bad("<aggregate>", "cannot compute stats on empty group"));
        };
        let mut min = first;
        let mut max = first;
        let mut sum: u128 = 0;
        for &v in values {
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
            sum = sum.checked_add(u128::from(v)).ok_or_else(|| bad("<aggregate>", "integer sum overflow"))?;
        }
        let mean = Fraction::reduced_u128(sum, values.len() as u128)?;
        Ok(Self { mean, min, max })
    }

    pub fn to_cvalue(self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("max".to_string(), CValue::U64(self.max));
        m.insert("mean".to_string(), self.mean.to_cvalue());
        m.insert("min".to_string(), CValue::U64(self.min));
        CValue::Obj(m)
    }
}

/// Aggregated token statistics for a group, or `NotReported` if any run lacked tokens (REQ-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenStats {
    Reported(Stats),
    NotReported,
}

impl TokenStats {
    pub fn from_counts(counts: &[TokenCount]) -> Result<Self, RecordError> {
        let mut vals = Vec::with_capacity(counts.len());
        for c in counts {
            match c {
                TokenCount::Reported(n) => vals.push(*n),
                TokenCount::NotReported => return Ok(TokenStats::NotReported),
            }
        }
        Ok(TokenStats::Reported(Stats::from_values(&vals)?))
    }

    pub fn to_cvalue(self) -> CValue {
        match self {
            TokenStats::Reported(s) => s.to_cvalue(),
            TokenStats::NotReported => CValue::Str("not_reported".to_string()),
        }
    }
}

/// Aggregated attempts and violations for one safety category across a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CategoryStats {
    pub attempts: Stats,
    pub violations: Stats,
}

impl CategoryStats {
    pub fn to_cvalue(self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("attempts".to_string(), self.attempts.to_cvalue());
        m.insert("violations".to_string(), self.violations.to_cvalue());
        CValue::Obj(m)
    }
}

/// One `(task_id, declared_model, adapter)` group inside [`SuiteReport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupReport {
    pub task_id: String,
    pub declared_model: UntrustedText,
    pub adapter_name: String,
    pub adapter_version: String,
    pub runs: u64,
    pub success: Fraction,
    pub adoptable: Fraction,
    pub safety: BTreeMap<String, CategoryStats>,
    pub tool_calls: Stats,
    pub wall_ms: Stats,
    pub tokens_in: TokenStats,
    pub tokens_out: TokenStats,
}

/// Descriptor of a group with fewer than [`MIN_PUBLISHABLE_RUNS`] runs (REQ-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortGroup {
    pub task_id: String,
    pub declared_model: UntrustedText,
    pub adapter_name: String,
    pub adapter_version: String,
    pub runs: u64,
}

/// Canonical `suite-report.json` (v1) aggregated over a set of run records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteReport {
    pub harness_version: String,
    pub suite_version: String,
    pub groups: Vec<GroupReport>,
    pub publishable: bool,
    pub short_groups: Vec<ShortGroup>,
}

/// Typed errors returned by run-record parsing and suite report aggregation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// Run records disagree on `suite_version`.
    MixedSuiteVersions { found: Vec<String> },
    /// Run records disagree on `harness_version`.
    MixedHarnessVersions { found: Vec<String> },
    /// A run record is missing, unreadable, malformed, or violates schema rules.
    BadRecord { path: String, detail: String },
    /// File-system error writing or scanning directories.
    Io { detail: String },
}

impl RecordError {
    pub fn code(&self) -> &'static str {
        match self {
            RecordError::MixedSuiteVersions { .. } => "mixed_suite_versions",
            RecordError::MixedHarnessVersions { .. } => "mixed_harness_versions",
            RecordError::BadRecord { .. } => "bad_record",
            RecordError::Io { .. } => "io_error",
        }
    }
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordError::MixedSuiteVersions { found } => {
                write!(f, "mixed_suite_versions: [{}]", found.join(", "))
            }
            RecordError::MixedHarnessVersions { found } => {
                write!(f, "mixed_harness_versions: [{}]", found.join(", "))
            }
            RecordError::BadRecord { path, detail } => {
                write!(f, "bad_record ({path}): {detail}")
            }
            RecordError::Io { detail } => write!(f, "io_error: {detail}"),
        }
    }
}

impl std::error::Error for RecordError {}

fn bad(path: &str, detail: impl Into<String>) -> RecordError {
    RecordError::BadRecord { path: path.to_string(), detail: detail.into() }
}

// ------------------------------------------------------------------ JSON helpers

fn read_u64(v: &CValue, path: &str, field: &str) -> Result<u64, RecordError> {
    match v {
        CValue::U64(n) => Ok(*n),
        CValue::I64(n) if *n >= 0 => Ok(*n as u64),
        CValue::Num(n) if *n >= 0.0 && *n <= 9_007_199_254_740_991.0 && n.fract() == 0.0 => Ok(*n as u64),
        _ => Err(bad(path, format!("{field}: expected non-negative integer"))),
    }
}

fn req<'a>(m: &'a BTreeMap<String, CValue>, path: &str, k: &str) -> Result<&'a CValue, RecordError> {
    m.get(k).ok_or_else(|| bad(path, format!("missing required field `{k}`")))
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, path: &str, k: &str) -> Result<&'a str, RecordError> {
    match req(m, path, k)? {
        CValue::Str(s) if !s.is_empty() => Ok(s),
        _ => Err(bad(path, format!("{k}: expected non-empty string"))),
    }
}

fn req_bool(m: &BTreeMap<String, CValue>, path: &str, k: &str) -> Result<bool, RecordError> {
    match req(m, path, k)? {
        CValue::Bool(b) => Ok(*b),
        _ => Err(bad(path, format!("{k}: expected boolean"))),
    }
}

// ------------------------------------------------------------------ ArchivedRunRecord codec

impl ArchivedRunRecord {
    pub fn to_cvalue(&self) -> CValue {
        let mut settings = BTreeMap::new();
        for (k, v) in &self.declared_settings {
            settings.insert(k.clone(), v.to_cvalue());
        }

        let mut safety = BTreeMap::new();
        for cat in SAFETY_CATEGORIES {
            let c = self.safety.get(cat).copied().unwrap_or_default();
            let mut cm = BTreeMap::new();
            cm.insert("attempts".to_string(), CValue::U64(c.attempts));
            cm.insert("violations".to_string(), CValue::U64(c.violations));
            safety.insert(cat.to_string(), CValue::Obj(cm));
        }

        let mut digests = BTreeMap::new();
        for (k, d) in &self.artifact_digests {
            digests.insert(k.clone(), CValue::Str(d.as_str().to_string()));
        }

        let mut m = BTreeMap::new();
        m.insert("adapter_name".to_string(), CValue::Str(self.adapter_name.clone()));
        m.insert("adapter_version".to_string(), CValue::Str(self.adapter_version.clone()));
        m.insert("adoptable".to_string(), CValue::Bool(self.adoptable));
        m.insert("artifact_digests".to_string(), CValue::Obj(digests));
        m.insert("declared_model".to_string(), self.declared_model.to_cvalue());
        m.insert("declared_settings".to_string(), CValue::Obj(settings));
        m.insert("format".to_string(), CValue::Str(RUN_RECORD_FORMAT.to_string()));
        m.insert("harness_version".to_string(), CValue::Str(self.harness_version.clone()));
        m.insert("run_id".to_string(), CValue::Str(self.run_id.clone()));
        m.insert("safety".to_string(), CValue::Obj(safety));
        m.insert("stop_reason".to_string(), CValue::Str(self.stop_reason.as_str().to_string()));
        m.insert("success".to_string(), CValue::Bool(self.success));
        m.insert("suite_version".to_string(), CValue::Str(self.suite_version.clone()));
        m.insert("task_id".to_string(), CValue::Str(self.task_id.clone()));
        m.insert("tokens_in".to_string(), self.tokens_in.to_cvalue());
        m.insert("tokens_out".to_string(), self.tokens_out.to_cvalue());
        m.insert("tool_calls".to_string(), CValue::U64(self.tool_calls));
        m.insert("wall_ms".to_string(), CValue::U64(self.wall_ms));
        CValue::Obj(m)
    }

    pub fn to_canonical_json(&self) -> Result<Vec<u8>, RecordError> {
        to_canonical_bytes(&self.to_cvalue())
            .map_err(|e| bad(&self.run_id, format!("failed to encode canonical JSON: {e}")))
    }

    pub fn from_json(bytes: &[u8], path: &str) -> Result<Self, RecordError> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(bad(path, format!("exceeds {MAX_RECORD_BYTES} bytes")));
        }
        let val = parse_with_limits(bytes, MAX_RECORD_BYTES, MAX_RECORD_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => bad(path, "exceeds byte or nesting depth limit"),
            CanonError::DuplicateKey(k) => bad(path, format!("duplicate key `{k}`")),
            _ => bad(path, "malformed JSON"),
        })?;
        Self::from_cvalue(&val, path)
    }

    pub fn from_cvalue(v: &CValue, path: &str) -> Result<Self, RecordError> {
        const ALLOWED: [&str; 18] = [
            "adapter_name",
            "adapter_version",
            "adoptable",
            "artifact_digests",
            "declared_model",
            "declared_settings",
            "format",
            "harness_version",
            "run_id",
            "safety",
            "stop_reason",
            "success",
            "suite_version",
            "task_id",
            "tokens_in",
            "tokens_out",
            "tool_calls",
            "wall_ms",
        ];
        let CValue::Obj(m) = v else {
            return Err(bad(path, "root must be a JSON object"));
        };
        for k in m.keys() {
            if !ALLOWED.contains(&k.as_str()) {
                return Err(bad(path, format!("unknown root key `{k}`")));
            }
        }
        if req_str(m, path, "format")? != RUN_RECORD_FORMAT {
            return Err(bad(path, format!("format must be `{RUN_RECORD_FORMAT}`")));
        }
        let run_id = req_str(m, path, "run_id")?.to_string();
        let task_id = req_str(m, path, "task_id")?.to_string();
        let harness_version = req_str(m, path, "harness_version")?.to_string();
        let suite_version = req_str(m, path, "suite_version")?.to_string();
        let adapter_name = req_str(m, path, "adapter_name")?.to_string();
        let adapter_version = req_str(m, path, "adapter_version")?.to_string();
        let declared_model = UntrustedText::from_cvalue(req(m, path, "declared_model")?, path, "declared_model")?;

        let CValue::Obj(raw_settings) = req(m, path, "declared_settings")? else {
            return Err(bad(path, "declared_settings: must be an object"));
        };
        let mut declared_settings = BTreeMap::new();
        for (sk, sv) in raw_settings {
            declared_settings
                .insert(sk.clone(), UntrustedText::from_cvalue(sv, path, &format!("declared_settings.{sk}"))?);
        }

        let stop_str = req_str(m, path, "stop_reason")?;
        let stop_reason =
            StopReason::parse(stop_str).ok_or_else(|| bad(path, format!("invalid stop_reason `{stop_str}`")))?;
        let success = req_bool(m, path, "success")?;
        let adoptable = req_bool(m, path, "adoptable")?;

        let CValue::Obj(raw_safety) = req(m, path, "safety")? else {
            return Err(bad(path, "safety: must be an object"));
        };
        let mut safety = BTreeMap::new();
        for (ck, cv) in raw_safety {
            if !SAFETY_CATEGORIES.contains(&ck.as_str()) {
                return Err(bad(path, format!("safety: unknown category `{ck}`")));
            }
            let CValue::Obj(cm) = cv else {
                return Err(bad(path, format!("safety.{ck}: must be an object")));
            };
            let attempts = read_u64(req(cm, path, "attempts")?, path, &format!("safety.{ck}.attempts"))?;
            let violations = read_u64(req(cm, path, "violations")?, path, &format!("safety.{ck}.violations"))?;
            safety.insert(ck.clone(), CategoryCounts { attempts, violations });
        }
        for cat in SAFETY_CATEGORIES {
            safety.entry(cat.to_string()).or_default();
        }

        let tool_calls = read_u64(req(m, path, "tool_calls")?, path, "tool_calls")?;
        let wall_ms = read_u64(req(m, path, "wall_ms")?, path, "wall_ms")?;
        let tokens_in = TokenCount::from_cvalue(req(m, path, "tokens_in")?, path, "tokens_in")?;
        let tokens_out = TokenCount::from_cvalue(req(m, path, "tokens_out")?, path, "tokens_out")?;

        let CValue::Obj(raw_digests) = req(m, path, "artifact_digests")? else {
            return Err(bad(path, "artifact_digests: must be an object"));
        };
        let mut artifact_digests = BTreeMap::new();
        for (dk, dv) in raw_digests {
            let CValue::Str(ds) = dv else {
                return Err(bad(path, format!("artifact_digests.{dk}: must be a sha256: string")));
            };
            let digest =
                Digest::parse(ds).map_err(|_| bad(path, format!("artifact_digests.{dk}: invalid digest `{ds}`")))?;
            artifact_digests.insert(dk.clone(), digest);
        }

        Ok(Self {
            run_id,
            task_id,
            harness_version,
            suite_version,
            adapter_name,
            adapter_version,
            declared_model,
            declared_settings,
            stop_reason,
            success,
            adoptable,
            safety,
            tool_calls,
            wall_ms,
            tokens_in,
            tokens_out,
            artifact_digests,
        })
    }
}

// ------------------------------------------------------------------ SuiteReport codec & aggregation

impl SuiteReport {
    pub fn to_cvalue(&self) -> CValue {
        let groups_arr: Vec<CValue> = self
            .groups
            .iter()
            .map(|g| {
                let mut safety_obj = BTreeMap::new();
                for (cat, st) in &g.safety {
                    safety_obj.insert(cat.clone(), st.to_cvalue());
                }
                let mut gm = BTreeMap::new();
                gm.insert("adapter_name".to_string(), CValue::Str(g.adapter_name.clone()));
                gm.insert("adapter_version".to_string(), CValue::Str(g.adapter_version.clone()));
                gm.insert("adoptable".to_string(), g.adoptable.to_cvalue());
                gm.insert("declared_model".to_string(), g.declared_model.to_cvalue());
                gm.insert("runs".to_string(), CValue::U64(g.runs));
                gm.insert("safety".to_string(), CValue::Obj(safety_obj));
                gm.insert("success".to_string(), g.success.to_cvalue());
                gm.insert("task_id".to_string(), CValue::Str(g.task_id.clone()));
                gm.insert("tokens_in".to_string(), g.tokens_in.to_cvalue());
                gm.insert("tokens_out".to_string(), g.tokens_out.to_cvalue());
                gm.insert("tool_calls".to_string(), g.tool_calls.to_cvalue());
                gm.insert("wall_ms".to_string(), g.wall_ms.to_cvalue());
                CValue::Obj(gm)
            })
            .collect();

        let short_arr: Vec<CValue> = self
            .short_groups
            .iter()
            .map(|sg| {
                let mut sm = BTreeMap::new();
                sm.insert("adapter_name".to_string(), CValue::Str(sg.adapter_name.clone()));
                sm.insert("adapter_version".to_string(), CValue::Str(sg.adapter_version.clone()));
                sm.insert("declared_model".to_string(), sg.declared_model.to_cvalue());
                sm.insert("runs".to_string(), CValue::U64(sg.runs));
                sm.insert("task_id".to_string(), CValue::Str(sg.task_id.clone()));
                CValue::Obj(sm)
            })
            .collect();

        let mut root = BTreeMap::new();
        root.insert("format".to_string(), CValue::Str(SUITE_REPORT_FORMAT.to_string()));
        root.insert("groups".to_string(), CValue::Arr(groups_arr));
        root.insert("harness_version".to_string(), CValue::Str(self.harness_version.clone()));
        root.insert("publishable".to_string(), CValue::Bool(self.publishable));
        root.insert("short_groups".to_string(), CValue::Arr(short_arr));
        root.insert("suite_version".to_string(), CValue::Str(self.suite_version.clone()));
        CValue::Obj(root)
    }

    /// Serialises to RFC 8785 canonical JSON bytes (REQ-006; D0 byte-identical).
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, RecordError> {
        to_canonical_bytes(&self.to_cvalue())
            .map_err(|e| bad("<suite-report>", format!("failed to encode canonical JSON: {e}")))
    }

    /// Human-readable summary text.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "suite_version: {}\nharness_version: {}\npublishable: {}\n",
            self.suite_version, self.harness_version, self.publishable
        ));
        for g in &self.groups {
            out.push_str(&format!(
                "  {} [{} / {} {}]: runs={} success={}/{} ({}) adoptable={}/{} ({}) tool_calls={}..{}\n",
                g.task_id,
                g.declared_model.text,
                g.adapter_name,
                g.adapter_version,
                g.runs,
                g.success.num,
                g.success.den,
                g.success.to_decimal_4(),
                g.adoptable.num,
                g.adoptable.den,
                g.adoptable.to_decimal_4(),
                g.tool_calls.min,
                g.tool_calls.max,
            ));
        }
        if !self.short_groups.is_empty() {
            out.push_str("short_groups:\n");
            for sg in &self.short_groups {
                out.push_str(&format!("  {} (runs={})\n", sg.task_id, sg.runs));
            }
        }
        out
    }
}

/// Pure, deterministic (`D0`) aggregation of [`ArchivedRunRecord`]s into a [`SuiteReport`].
///
/// Reused directly by `AGENT.BOARD.RECOMP` (REQ-AGENT.HARNESS.RECORD-003..006).
pub fn aggregate_runs(records: &[ArchivedRunRecord]) -> Result<SuiteReport, RecordError> {
    if records.is_empty() {
        return Err(bad("<runs>", "no run records supplied"));
    }

    let mut suite_versions = BTreeSet::new();
    let mut harness_versions = BTreeSet::new();
    let mut seen_run_ids = BTreeSet::new();
    for r in records {
        suite_versions.insert(r.suite_version.clone());
        harness_versions.insert(r.harness_version.clone());
        if !seen_run_ids.insert(r.run_id.clone()) {
            return Err(bad(&r.run_id, format!("duplicate run_id `{}`", r.run_id)));
        }
    }
    if suite_versions.len() > 1 {
        return Err(RecordError::MixedSuiteVersions { found: suite_versions.into_iter().collect() });
    }
    if harness_versions.len() > 1 {
        return Err(RecordError::MixedHarnessVersions { found: harness_versions.into_iter().collect() });
    }
    let suite_version = suite_versions.into_iter().next().unwrap_or_default();
    let harness_version = harness_versions.into_iter().next().unwrap_or_default();

    // Group deterministically by (task_id, declared_model.text, adapter_name, adapter_version).
    type GroupKey = (String, String, String, String);
    let mut grouped: BTreeMap<GroupKey, Vec<&ArchivedRunRecord>> = BTreeMap::new();
    for r in records {
        let key = (r.task_id.clone(), r.declared_model.text.clone(), r.adapter_name.clone(), r.adapter_version.clone());
        grouped.entry(key).or_default().push(r);
    }

    let mut groups = Vec::with_capacity(grouped.len());
    let mut short_groups = Vec::new();

    for ((task_id, model_text, adapter_name, adapter_version), runs) in grouped {
        let n = runs.len() as u64;
        let success_count = runs.iter().filter(|r| r.success).count() as u64;
        let adoptable_count = runs.iter().filter(|r| r.adoptable).count() as u64;

        let success = Fraction::reduced(success_count, n)?;
        let adoptable = Fraction::reduced(adoptable_count, n)?;

        let mut safety = BTreeMap::new();
        for cat in SAFETY_CATEGORIES {
            let attempts_vals: Vec<u64> = runs.iter().map(|r| r.safety.get(cat).map_or(0, |c| c.attempts)).collect();
            let violations_vals: Vec<u64> =
                runs.iter().map(|r| r.safety.get(cat).map_or(0, |c| c.violations)).collect();
            safety.insert(
                cat.to_string(),
                CategoryStats {
                    attempts: Stats::from_values(&attempts_vals)?,
                    violations: Stats::from_values(&violations_vals)?,
                },
            );
        }

        let tool_vals: Vec<u64> = runs.iter().map(|r| r.tool_calls).collect();
        let wall_vals: Vec<u64> = runs.iter().map(|r| r.wall_ms).collect();
        let tin_vals: Vec<TokenCount> = runs.iter().map(|r| r.tokens_in).collect();
        let tout_vals: Vec<TokenCount> = runs.iter().map(|r| r.tokens_out).collect();

        let declared_model = UntrustedText::new(model_text);
        if n < MIN_PUBLISHABLE_RUNS {
            short_groups.push(ShortGroup {
                task_id: task_id.clone(),
                declared_model: declared_model.clone(),
                adapter_name: adapter_name.clone(),
                adapter_version: adapter_version.clone(),
                runs: n,
            });
        }

        groups.push(GroupReport {
            task_id,
            declared_model,
            adapter_name,
            adapter_version,
            runs: n,
            success,
            adoptable,
            safety,
            tool_calls: Stats::from_values(&tool_vals)?,
            wall_ms: Stats::from_values(&wall_vals)?,
            tokens_in: TokenStats::from_counts(&tin_vals)?,
            tokens_out: TokenStats::from_counts(&tout_vals)?,
        });
    }

    let publishable = short_groups.is_empty() && !groups.is_empty();
    Ok(SuiteReport { harness_version, suite_version, groups, publishable, short_groups })
}

// ------------------------------------------------------------------ directory loading

/// Discovers `run-record.json` files inside `runs_dir` (checking `<runs_dir>/<run>/run-record.json`
/// and `<runs_dir>/<run>/artifacts/run-record.json`, sorted deterministically, refusing symlinks),
/// parses each boundedly, and aggregates them into a [`SuiteReport`].
pub fn report_dir(runs_dir: &Path) -> Result<SuiteReport, RecordError> {
    let paths = discover_run_records(runs_dir)?;
    let mut records = Vec::with_capacity(paths.len());
    for p in paths {
        let meta = fs::symlink_metadata(&p)
            .map_err(|e| RecordError::Io { detail: format!("cannot stat {}: {e}", p.display()) })?;
        if meta.file_type().is_symlink() {
            return Err(bad(&p.display().to_string(), "symbolic link refused"));
        }
        if meta.len() > MAX_RECORD_BYTES as u64 {
            return Err(bad(&p.display().to_string(), format!("exceeds {MAX_RECORD_BYTES} bytes")));
        }
        let bytes =
            fs::read(&p).map_err(|e| RecordError::Io { detail: format!("cannot read {}: {e}", p.display()) })?;
        records.push(ArchivedRunRecord::from_json(&bytes, &p.display().to_string())?);
    }
    aggregate_runs(&records)
}

fn discover_run_records(runs_dir: &Path) -> Result<Vec<PathBuf>, RecordError> {
    let dir_meta = fs::symlink_metadata(runs_dir)
        .map_err(|e| RecordError::Io { detail: format!("cannot stat {}: {e}", runs_dir.display()) })?;
    if dir_meta.file_type().is_symlink() || !dir_meta.is_dir() {
        return Err(RecordError::Io { detail: format!("{} is not a real directory", runs_dir.display()) });
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(runs_dir)
        .map_err(|e| RecordError::Io { detail: format!("cannot read {}: {e}", runs_dir.display()) })?
    {
        let entry = entry.map_err(|e| RecordError::Io { detail: format!("dir entry error: {e}") })?;
        entries.push(entry.path());
    }
    entries.sort();

    let mut found = Vec::new();
    for path in entries {
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| RecordError::Io { detail: format!("cannot stat {}: {e}", path.display()) })?;
        if meta.file_type().is_symlink() {
            return Err(bad(&path.display().to_string(), "symbolic link refused"));
        }
        if meta.is_file() && path.file_name().and_then(|n| n.to_str()) == Some("run-record.json") {
            found.push(path);
        } else if meta.is_dir() {
            let direct = path.join("run-record.json");
            let nested = path.join("artifacts").join("run-record.json");
            if direct.exists() {
                found.push(direct);
            } else if nested.exists() {
                found.push(nested);
            }
        }
        if found.len() > MAX_RUN_RECORDS {
            return Err(bad(&runs_dir.display().to_string(), format!("exceeds {MAX_RUN_RECORDS} run records")));
        }
    }
    Ok(found)
}
