//! Nightly acceptance report `acceptance-report@1`, append-only trend history, and
//! `accept.show_report@1` (spec node `FLOW.RUNNER.VERDICT`).
//!
//! - Every report is validated against a closed schema (`VERDICT-SCHEMA`) and scanned for
//!   host paths, user names, and address-like strings (`VERDICT-PII`, REQ-003).
//! - Assisted or tracked packages must declare `non_gating: true` separately from `verdict`
//!   (REQ-002).
//! - Report digest is the SHA-256 of the RFC 8785 canonical JSON form (REQ-003).
//! - Trend history is an append-only JSON Lines file (`trend.jsonl`, one [`TrendRecord`] per
//!   line), queried newest first with `last` in `1..=365` (REQ-004).
//! - Reports and trends are read strictly from the configured acceptance store directory, never
//!   from caller-supplied paths (`accept.show_report@1`, R0, D0).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_registry::{CommandDef, DefError};
use fid_types::Digest;

use crate::package::{is_ident, is_job_id, Assistance, Gating, CHECK_KINDS, MAX_SECONDS};

/// Format tag of a nightly acceptance report.
pub const REPORT_FORMAT: &str = "acceptance-report@1";
/// Command ID of the show-report command.
pub const COMMAND_ID: &str = "accept.show_report";
/// Command major version.
pub const COMMAND_MAJOR: u32 = 1;
/// Maximum number of trend records returned by one query (REQ-004).
pub const MAX_TREND_LAST: u16 = 365;
/// Default number of trend records when `--last` is omitted.
pub const DEFAULT_TREND_LAST: u16 = 30;
/// Largest accepted report JSON file, in bytes (8 MiB).
pub const MAX_REPORT_BYTES: usize = 8 << 20;
/// Largest accepted trend JSONL file, in bytes (32 MiB).
pub const MAX_TREND_BYTES: usize = 32 << 20;
/// Deepest JSON nesting accepted for an acceptance report.
pub const MAX_REPORT_DEPTH: usize = 12;
/// Maximum machines listed in a report.
pub const MAX_MACHINES: usize = 64;
/// Maximum packages listed in a report.
pub const MAX_PACKAGES: usize = 2048;
/// Maximum checks per package.
pub const MAX_CHECKS: usize = 256;
/// Maximum failure reasons per package.
pub const MAX_REASONS: usize = 128;
/// Maximum string length for short fields / reasons.
pub const MAX_REASON_BYTES: usize = 1024;
/// Subdirectory under the acceptance store holding `<hex>.json` reports.
pub const REPORTS_DIR: &str = "reports";
/// File name of the append-only trend history under the acceptance store.
pub const TREND_FILE: &str = "trend.jsonl";

// ------------------------------------------------------------------ errors

/// Typed error codes of `FLOW.RUNNER.VERDICT` (Design: Errors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VerdictErrorCode {
    /// Requested report digest was not found in the store.
    NotFound,
    /// Report, trend record, or query parameters failed schema validation.
    Schema,
    /// Report or trend record contains a host path, user name, or address-like string.
    Pii,
}

impl VerdictErrorCode {
    /// Stable wire error code string.
    pub fn as_str(self) -> &'static str {
        match self {
            VerdictErrorCode::NotFound => "not_found",
            VerdictErrorCode::Schema => "VERDICT-SCHEMA",
            VerdictErrorCode::Pii => "VERDICT-PII",
        }
    }
}

/// Typed error returned by report validation, store reads/writes, and `show_report`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictError {
    pub code: VerdictErrorCode,
    pub subject: String,
    pub detail: String,
}

impl VerdictError {
    pub fn new(code: VerdictErrorCode, subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { code, subject: subject.into(), detail: detail.into() }
    }

    pub fn not_found(subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(VerdictErrorCode::NotFound, subject, detail)
    }

    pub fn schema(subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(VerdictErrorCode::Schema, subject, detail)
    }

    pub fn pii(subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(VerdictErrorCode::Pii, subject, detail)
    }
}

impl fmt::Display for VerdictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]: {}", self.code.as_str(), self.subject, self.detail)
    }
}

impl std::error::Error for VerdictError {}

// ------------------------------------------------------------------ data structures

/// Reference machine entry in an acceptance report (REQ-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceMachine {
    pub id: String,
    pub host: String,
}

/// Replay outcome of a package run (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReplayOutcome {
    Identical,
    Equivalent,
    Diverged,
    NotReplayable,
}

impl ReplayOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            ReplayOutcome::Identical => "identical",
            ReplayOutcome::Equivalent => "equivalent",
            ReplayOutcome::Diverged => "diverged",
            ReplayOutcome::NotReplayable => "not_replayable",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "identical" => Some(ReplayOutcome::Identical),
            "equivalent" => Some(ReplayOutcome::Equivalent),
            "diverged" => Some(ReplayOutcome::Diverged),
            "not_replayable" => Some(ReplayOutcome::NotReplayable),
            _ => None,
        }
    }
}

/// Outcome of one quality check inside a package (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CheckStatus {
    Pass,
    Fail,
    Skipped,
}

impl CheckStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CheckStatus::Pass => "pass",
            CheckStatus::Fail => "fail",
            CheckStatus::Skipped => "skipped",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pass" => Some(CheckStatus::Pass),
            "fail" => Some(CheckStatus::Fail),
            "skipped" => Some(CheckStatus::Skipped),
            _ => None,
        }
    }
}

/// One quality-check result inside a package (REQ-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    pub id: String,
    pub kind: String,
    pub outcome: CheckStatus,
    pub reason: Option<String>,
}

/// Overall package verdict (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    Pass,
    Fail,
    NotReplayable,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
            Verdict::NotReplayable => "not_replayable",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pass" => Some(Verdict::Pass),
            "fail" => Some(Verdict::Fail),
            "not_replayable" => Some(Verdict::NotReplayable),
            _ => None,
        }
    }
}

/// One package's result in an acceptance report (REQ-001, REQ-002).
#[derive(Debug, Clone, PartialEq)]
pub struct PackageResult {
    pub job: String,
    pub variant: String,
    pub gating: Gating,
    pub assistance: Assistance,
    /// Separate from `verdict`: `true` whenever `gating == Tracked` or `assistance == Assisted`
    /// (REQ-002).
    pub non_gating: bool,
    pub replay_outcome: ReplayOutcome,
    pub checks: Vec<CheckOutcome>,
    /// Machine time in seconds keyed by reference machine ID.
    pub machine_times: BTreeMap<String, f64>,
    pub verdict: Verdict,
    pub reasons: Vec<String>,
}

/// Reliability summary section of an acceptance report (REQ-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReliabilitySection {
    pub total_packages: u64,
    pub passed: u64,
    pub failed: u64,
    pub not_replayable: u64,
    pub gating_passed: u64,
    pub gating_failed: u64,
    pub non_gating_count: u64,
    pub crash_recovery_passed: Option<bool>,
}

/// Nightly acceptance report `acceptance-report@1` (REQ-001..REQ-003).
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptanceReport {
    pub format: String,
    pub build_id: String,
    pub manifest_digest: Digest,
    pub suite_digest: Digest,
    pub started_at: String,
    pub ended_at: String,
    pub machines: Vec<ReferenceMachine>,
    pub packages: Vec<PackageResult>,
    pub reliability: ReliabilitySection,
}

/// One record in the append-only trend history (`trend.jsonl`, REQ-004).
#[derive(Debug, Clone, PartialEq)]
pub struct TrendRecord {
    /// ISO 8601 calendar date (`YYYY-MM-DD`).
    pub night: String,
    pub job: String,
    pub variant: String,
    pub verdict: Verdict,
    pub machine_s: f64,
}

// ------------------------------------------------------------------ PII lint (REQ-003, AC-4)

/// Scans a parsed JSON value recursively for host paths, user-directory patterns, or address-like
/// strings (`VERDICT-PII`). The top-level `"format"` field is exempt from the `@` check so
/// `"acceptance-report@1"` is allowed.
pub fn lint_pii(value: &CValue) -> Result<(), VerdictError> {
    lint_pii_at(value, "$", 0)
}

fn lint_pii_at(value: &CValue, path: &str, depth: usize) -> Result<(), VerdictError> {
    if depth > MAX_REPORT_DEPTH + 4 {
        return Err(VerdictError::schema(path, "nesting exceeds maximum depth"));
    }
    match value {
        CValue::Str(s) => check_string_pii(s, path, path == "$.format"),
        CValue::Arr(items) => {
            for (idx, item) in items.iter().enumerate() {
                lint_pii_at(item, &format!("{path}[{idx}]"), depth + 1)?;
            }
            Ok(())
        }
        CValue::Obj(map) => {
            for (k, v) in map {
                let child_path = format!("{path}.{k}");
                check_string_pii(k, &child_path, false)?;
                lint_pii_at(v, &child_path, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn check_string_pii(s: &str, path: &str, allow_format_at: bool) -> Result<(), VerdictError> {
    // 1. Address-like `@` (emails, user@host). Only the top-level format tag may hold `@`.
    if s.contains('@') && !(allow_format_at && s == REPORT_FORMAT) {
        return Err(VerdictError::pii(path, "contains '@' (address-like or user@host text is forbidden)"));
    }

    // 2. File URI scheme or home tilde path.
    let lower = s.to_ascii_lowercase();
    if lower.contains("file://") || s.starts_with("~/") || s.contains(" ~/") {
        return Err(VerdictError::pii(path, "contains host path reference"));
    }

    // 3. Windows drive-letter paths (`C:\` or `C:/`) and UNC paths (`\\server\share`).
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if i + 2 < bytes.len()
            && bytes[i].is_ascii_alphabetic()
            && bytes[i + 1] == b':'
            && (bytes[i + 2] == b'\\' || bytes[i + 2] == b'/')
        {
            let prev_is_alnum = i > 0 && bytes[i - 1].is_ascii_alphanumeric();
            if !prev_is_alnum {
                return Err(VerdictError::pii(path, "contains Windows drive path"));
            }
        }
        if i + 1 < bytes.len() && bytes[i] == b'\\' && bytes[i + 1] == b'\\' {
            return Err(VerdictError::pii(path, "contains UNC host path"));
        }
    }

    // 4. Backslash path separators anywhere in strings.
    if s.contains('\\') {
        return Err(VerdictError::pii(path, "contains backslash path separator"));
    }

    // 5. Unix absolute paths (`/home/`, `/Users/`, `/tmp/`, `/var/`, `/etc/`, `/root/`, `/opt/`,
    //    `/usr/`, `/mnt/`, `/srv/`, `/proc/`, `/private/`, `/Volumes/`, or any token starting
    //    with `/` followed by two path segments).
    const UNIX_ROOTS: [&str; 13] = [
        "/home/",
        "/users/",
        "/tmp/",
        "/var/",
        "/etc/",
        "/root/",
        "/opt/",
        "/usr/",
        "/mnt/",
        "/srv/",
        "/proc/",
        "/private/",
        "/volumes/",
    ];
    for prefix in UNIX_ROOTS {
        if lower.contains(prefix) {
            return Err(VerdictError::pii(path, "contains Unix host path"));
        }
    }
    if lower.contains("appdata") || lower.contains("documents and settings") {
        return Err(VerdictError::pii(path, "contains user profile directory marker"));
    }
    // Check whitespace-delimited tokens for generic absolute Unix path `/seg1/seg2`.
    for tok in s.split(|c: char| c.is_ascii_whitespace() || matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | ',')) {
        if let Some(rest) = tok.strip_prefix('/') {
            let mut segs = rest.split('/').filter(|seg| !seg.is_empty());
            if segs.next().is_some() && segs.next().is_some() {
                return Err(VerdictError::pii(path, "contains absolute host path"));
            }
        }
    }

    Ok(())
}

// ------------------------------------------------------------------ validation helpers

fn parse_json(bytes: &[u8], max_bytes: usize, subject: &str) -> Result<CValue, VerdictError> {
    if bytes.len() > max_bytes {
        return Err(VerdictError::schema(subject, format!("input exceeds {max_bytes} bytes")));
    }
    parse_with_limits(bytes, max_bytes, MAX_REPORT_DEPTH)
        .map_err(|e| VerdictError::schema(subject, format!("invalid JSON: {e}")))
}

fn require_obj<'a>(v: &'a CValue, subject: &str) -> Result<&'a BTreeMap<String, CValue>, VerdictError> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(VerdictError::schema(subject, "expected JSON object")),
    }
}

fn check_closed_keys(
    map: &BTreeMap<String, CValue>,
    required: &[&str],
    optional: &[&str],
    subject: &str,
) -> Result<(), VerdictError> {
    for &k in required {
        if !map.contains_key(k) {
            return Err(VerdictError::schema(subject, format!("missing required field `{k}`")));
        }
    }
    for k in map.keys() {
        if !required.contains(&k.as_str()) && !optional.contains(&k.as_str()) {
            return Err(VerdictError::schema(subject, format!("unknown field `{k}`")));
        }
    }
    Ok(())
}

fn get_str<'a>(map: &'a BTreeMap<String, CValue>, key: &str, subject: &str) -> Result<&'a str, VerdictError> {
    match map.get(key) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(VerdictError::schema(format!("{subject}.{key}"), "expected string")),
    }
}

fn get_bool(map: &BTreeMap<String, CValue>, key: &str, subject: &str) -> Result<bool, VerdictError> {
    match map.get(key) {
        Some(CValue::Bool(b)) => Ok(*b),
        _ => Err(VerdictError::schema(format!("{subject}.{key}"), "expected boolean")),
    }
}

fn get_u64(map: &BTreeMap<String, CValue>, key: &str, subject: &str) -> Result<u64, VerdictError> {
    match map.get(key) {
        Some(CValue::U64(n)) => Ok(*n),
        Some(CValue::I64(n)) if *n >= 0 => Ok(*n as u64),
        Some(CValue::Num(n)) if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= 9_007_199_254_740_991.0 => {
            Ok(*n as u64)
        }
        _ => Err(VerdictError::schema(format!("{subject}.{key}"), "expected non-negative integer")),
    }
}

fn get_nonneg_seconds(v: &CValue, subject: &str) -> Result<f64, VerdictError> {
    let x = match v {
        CValue::Num(n) => *n,
        CValue::U64(n) => *n as f64,
        CValue::I64(n) => *n as f64,
        _ => return Err(VerdictError::schema(subject, "expected number")),
    };
    if !x.is_finite() || x < 0.0 || x > MAX_SECONDS {
        return Err(VerdictError::schema(subject, format!("machine time must be finite in 0..={MAX_SECONDS}")));
    }
    // Normalise -0.0 to +0.0.
    Ok(if x == 0.0 { 0.0 } else { x })
}

fn is_valid_token(s: &str, max_len: usize) -> bool {
    !s.is_empty()
        && s.len() <= max_len
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn is_valid_reason(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_REASON_BYTES && !s.chars().any(char::is_control)
}

fn parse_u32_slice(b: &[u8]) -> Option<u32> {
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(b).ok()?.parse().ok()
}

fn is_leap_year(y: u32) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

fn days_in_month(y: u32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Validates `YYYY-MM-DD` as a real Gregorian calendar date.
pub fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let (Some(y), Some(m), Some(d)) =
        (parse_u32_slice(&b[0..4]), parse_u32_slice(&b[5..7]), parse_u32_slice(&b[8..10]))
    else {
        return false;
    };
    (1970..=9999).contains(&y) && (1..=12).contains(&m) && (1..=days_in_month(y, m)).contains(&d)
}

/// Validates RFC 3339 UTC timestamp `YYYY-MM-DDTHH:MM:SSZ` (with optional `.fff..` fractional seconds).
pub fn is_rfc3339_utc(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 20 || b[10] != b'T' || b[13] != b':' || b[16] != b':' || *b.last().unwrap_or(&0) != b'Z' {
        return false;
    }
    let Ok(date) = std::str::from_utf8(&b[0..10]) else { return false };
    if !is_iso_date(date) {
        return false;
    }
    let (Some(h), Some(min), Some(sec)) =
        (parse_u32_slice(&b[11..13]), parse_u32_slice(&b[14..16]), parse_u32_slice(&b[17..19]))
    else {
        return false;
    };
    if h > 23 || min > 59 || sec > 59 {
        return false;
    }
    if b.len() == 20 {
        return true;
    }
    b[19] == b'.' && b.len() > 21 && b.len() <= 30 && b[20..b.len() - 1].iter().all(u8::is_ascii_digit)
}

// ------------------------------------------------------------------ AcceptanceReport impl

impl AcceptanceReport {
    /// Parses, PII-lints (`VERDICT-PII`), and validates (`VERDICT-SCHEMA`) an
    /// `acceptance-report@1` JSON byte slice.
    pub fn parse_json(bytes: &[u8]) -> Result<Self, VerdictError> {
        let val = parse_json(bytes, MAX_REPORT_BYTES, "$")?;
        Self::from_value(&val)
    }

    /// PII-lints and validates an `acceptance-report@1` from a parsed [`CValue`].
    pub fn from_value(val: &CValue) -> Result<Self, VerdictError> {
        // Run PII lint first so host paths or addresses always surface as VERDICT-PII (AC-4).
        lint_pii(val)?;

        let root = require_obj(val, "$")?;
        check_closed_keys(
            root,
            &[
                "format",
                "build_id",
                "manifest_digest",
                "suite_digest",
                "started_at",
                "ended_at",
                "machines",
                "packages",
                "reliability",
            ],
            &[],
            "$",
        )?;

        let format = get_str(root, "format", "$")?.to_string();
        if format != REPORT_FORMAT {
            return Err(VerdictError::schema("$.format", format!("expected `{REPORT_FORMAT}`, got `{format}`")));
        }

        let build_id = get_str(root, "build_id", "$")?.to_string();
        if !is_valid_token(&build_id, 128) {
            return Err(VerdictError::schema("$.build_id", "invalid build_id token"));
        }

        let manifest_digest = Digest::parse(get_str(root, "manifest_digest", "$")?)
            .map_err(|e| VerdictError::schema("$.manifest_digest", format!("invalid digest: {e}")))?;
        let suite_digest = Digest::parse(get_str(root, "suite_digest", "$")?)
            .map_err(|e| VerdictError::schema("$.suite_digest", format!("invalid digest: {e}")))?;

        let started_at = get_str(root, "started_at", "$")?.to_string();
        if !is_rfc3339_utc(&started_at) {
            return Err(VerdictError::schema("$.started_at", "expected RFC 3339 UTC timestamp"));
        }
        let ended_at = get_str(root, "ended_at", "$")?.to_string();
        if !is_rfc3339_utc(&ended_at) {
            return Err(VerdictError::schema("$.ended_at", "expected RFC 3339 UTC timestamp"));
        }
        if ended_at < started_at {
            return Err(VerdictError::schema("$.ended_at", "ended_at precedes started_at"));
        }

        // Reference machines.
        let Some(CValue::Arr(machines_arr)) = root.get("machines") else {
            return Err(VerdictError::schema("$.machines", "expected array"));
        };
        if machines_arr.is_empty() || machines_arr.len() > MAX_MACHINES {
            return Err(VerdictError::schema("$.machines", format!("must contain 1..={MAX_MACHINES} machines")));
        }
        let mut machines = Vec::with_capacity(machines_arr.len());
        let mut machine_ids = BTreeSet::new();
        for (i, item) in machines_arr.iter().enumerate() {
            let sub = format!("$.machines[{i}]");
            let m = require_obj(item, &sub)?;
            check_closed_keys(m, &["id", "host"], &[], &sub)?;
            let id = get_str(m, "id", &sub)?.to_string();
            let host = get_str(m, "host", &sub)?.to_string();
            if !is_ident(&id) {
                return Err(VerdictError::schema(format!("{sub}.id"), "invalid machine id"));
            }
            if !is_valid_token(&host, 128) {
                return Err(VerdictError::schema(format!("{sub}.host"), "invalid host token"));
            }
            if !machine_ids.insert(id.clone()) {
                return Err(VerdictError::schema(format!("{sub}.id"), format!("duplicate machine id `{id}`")));
            }
            machines.push(ReferenceMachine { id, host });
        }

        // Packages.
        let Some(CValue::Arr(pkgs_arr)) = root.get("packages") else {
            return Err(VerdictError::schema("$.packages", "expected array"));
        };
        if pkgs_arr.is_empty() || pkgs_arr.len() > MAX_PACKAGES {
            return Err(VerdictError::schema("$.packages", format!("must contain 1..={MAX_PACKAGES} packages")));
        }
        let mut packages = Vec::with_capacity(pkgs_arr.len());
        let mut seen_pkg = BTreeSet::new();
        for (i, item) in pkgs_arr.iter().enumerate() {
            let sub = format!("$.packages[{i}]");
            let p = require_obj(item, &sub)?;
            check_closed_keys(
                p,
                &[
                    "job",
                    "variant",
                    "gating",
                    "assistance",
                    "non_gating",
                    "replay_outcome",
                    "checks",
                    "machine_times",
                    "verdict",
                    "reasons",
                ],
                &[],
                &sub,
            )?;

            let job = get_str(p, "job", &sub)?.to_string();
            if !is_job_id(&job) {
                return Err(VerdictError::schema(format!("{sub}.job"), format!("invalid job id `{job}`")));
            }
            let variant = get_str(p, "variant", &sub)?.to_string();
            if !is_ident(&variant) {
                return Err(VerdictError::schema(format!("{sub}.variant"), format!("invalid variant `{variant}`")));
            }
            if !seen_pkg.insert((job.clone(), variant.clone())) {
                return Err(VerdictError::schema(&sub, format!("duplicate package ({job}, {variant})")));
            }

            let gating = match get_str(p, "gating", &sub)? {
                "gating" => Gating::Gating,
                "tracked" => Gating::Tracked,
                other => {
                    return Err(VerdictError::schema(format!("{sub}.gating"), format!("invalid gating `{other}`")))
                }
            };
            let assistance = match get_str(p, "assistance", &sub)? {
                "unassisted" => Assistance::Unassisted,
                "assisted" => Assistance::Assisted,
                other => {
                    return Err(VerdictError::schema(
                        format!("{sub}.assistance"),
                        format!("invalid assistance `{other}`"),
                    ))
                }
            };
            let non_gating = get_bool(p, "non_gating", &sub)?;
            let expected_non_gating = gating == Gating::Tracked || assistance == Assistance::Assisted;
            if non_gating != expected_non_gating {
                return Err(VerdictError::schema(
                    format!("{sub}.non_gating"),
                    format!(
                        "non_gating must be {expected_non_gating} for gating={gating:?}, assistance={assistance:?}"
                    ),
                ));
            }

            let replay_outcome = ReplayOutcome::parse(get_str(p, "replay_outcome", &sub)?)
                .ok_or_else(|| VerdictError::schema(format!("{sub}.replay_outcome"), "invalid replay_outcome"))?;

            // Checks.
            let Some(CValue::Arr(checks_arr)) = p.get("checks") else {
                return Err(VerdictError::schema(format!("{sub}.checks"), "expected array"));
            };
            if checks_arr.len() > MAX_CHECKS {
                return Err(VerdictError::schema(format!("{sub}.checks"), format!("exceeds {MAX_CHECKS} checks")));
            }
            let mut checks = Vec::with_capacity(checks_arr.len());
            let mut seen_check_ids = BTreeSet::new();
            for (ci, c_val) in checks_arr.iter().enumerate() {
                let csub = format!("{sub}.checks[{ci}]");
                let cobj = require_obj(c_val, &csub)?;
                check_closed_keys(cobj, &["id", "kind", "outcome"], &["reason"], &csub)?;
                let cid = get_str(cobj, "id", &csub)?.to_string();
                if !is_ident(&cid) {
                    return Err(VerdictError::schema(format!("{csub}.id"), "invalid check id"));
                }
                if !seen_check_ids.insert(cid.clone()) {
                    return Err(VerdictError::schema(format!("{csub}.id"), format!("duplicate check id `{cid}`")));
                }
                let kind = get_str(cobj, "kind", &csub)?.to_string();
                if !CHECK_KINDS.contains(&kind.as_str()) {
                    return Err(VerdictError::schema(format!("{csub}.kind"), format!("unknown check kind `{kind}`")));
                }
                let outcome = CheckStatus::parse(get_str(cobj, "outcome", &csub)?)
                    .ok_or_else(|| VerdictError::schema(format!("{csub}.outcome"), "invalid check outcome"))?;
                let reason = match cobj.get("reason") {
                    None => None,
                    Some(CValue::Str(r)) if is_valid_reason(r) => Some(r.clone()),
                    Some(_) => return Err(VerdictError::schema(format!("{csub}.reason"), "invalid check reason")),
                };
                checks.push(CheckOutcome { id: cid, kind, outcome, reason });
            }

            // Machine times.
            let mt_obj = match p.get("machine_times") {
                Some(v) => require_obj(v, &format!("{sub}.machine_times"))?,
                None => return Err(VerdictError::schema(format!("{sub}.machine_times"), "missing machine_times")),
            };
            let mut machine_times = BTreeMap::new();
            for (mid, tv) in mt_obj {
                if !machine_ids.contains(mid) {
                    return Err(VerdictError::schema(
                        format!("{sub}.machine_times.{mid}"),
                        format!("machine `{mid}` not declared in top-level machines"),
                    ));
                }
                let secs = get_nonneg_seconds(tv, &format!("{sub}.machine_times.{mid}"))?;
                machine_times.insert(mid.clone(), secs);
            }

            let verdict = Verdict::parse(get_str(p, "verdict", &sub)?)
                .ok_or_else(|| VerdictError::schema(format!("{sub}.verdict"), "invalid verdict"))?;

            let Some(CValue::Arr(reasons_arr)) = p.get("reasons") else {
                return Err(VerdictError::schema(format!("{sub}.reasons"), "expected array"));
            };
            if reasons_arr.len() > MAX_REASONS {
                return Err(VerdictError::schema(format!("{sub}.reasons"), format!("exceeds {MAX_REASONS} reasons")));
            }
            let mut reasons = Vec::with_capacity(reasons_arr.len());
            for (ri, rv) in reasons_arr.iter().enumerate() {
                match rv {
                    CValue::Str(rs) if is_valid_reason(rs) => reasons.push(rs.clone()),
                    _ => return Err(VerdictError::schema(format!("{sub}.reasons[{ri}]"), "invalid reason string")),
                }
            }
            if verdict != Verdict::Pass && reasons.is_empty() {
                return Err(VerdictError::schema(
                    format!("{sub}.reasons"),
                    "non-pass verdict must provide at least one reason",
                ));
            }

            packages.push(PackageResult {
                job,
                variant,
                gating,
                assistance,
                non_gating,
                replay_outcome,
                checks,
                machine_times,
                verdict,
                reasons,
            });
        }

        // Reliability section.
        let rel_val = root.get("reliability").ok_or_else(|| VerdictError::schema("$.reliability", "missing"))?;
        let rel = require_obj(rel_val, "$.reliability")?;
        check_closed_keys(
            rel,
            &[
                "total_packages",
                "passed",
                "failed",
                "not_replayable",
                "gating_passed",
                "gating_failed",
                "non_gating_count",
            ],
            &["crash_recovery_passed"],
            "$.reliability",
        )?;
        let total_packages = get_u64(rel, "total_packages", "$.reliability")?;
        let passed = get_u64(rel, "passed", "$.reliability")?;
        let failed = get_u64(rel, "failed", "$.reliability")?;
        let not_replayable = get_u64(rel, "not_replayable", "$.reliability")?;
        let gating_passed = get_u64(rel, "gating_passed", "$.reliability")?;
        let gating_failed = get_u64(rel, "gating_failed", "$.reliability")?;
        let non_gating_count = get_u64(rel, "non_gating_count", "$.reliability")?;
        let crash_recovery_passed = match rel.get("crash_recovery_passed") {
            None => None,
            Some(CValue::Bool(b)) => Some(*b),
            Some(_) => return Err(VerdictError::schema("$.reliability.crash_recovery_passed", "expected boolean")),
        };

        // Cross-check reliability counts against packages.
        let exp_total = packages.len() as u64;
        let exp_passed = packages.iter().filter(|p| p.verdict == Verdict::Pass).count() as u64;
        let exp_failed = packages.iter().filter(|p| p.verdict == Verdict::Fail).count() as u64;
        let exp_nr = packages.iter().filter(|p| p.verdict == Verdict::NotReplayable).count() as u64;
        let exp_gp = packages.iter().filter(|p| !p.non_gating && p.verdict == Verdict::Pass).count() as u64;
        let exp_gf = packages.iter().filter(|p| !p.non_gating && p.verdict != Verdict::Pass).count() as u64;
        let exp_ng = packages.iter().filter(|p| p.non_gating).count() as u64;
        if (total_packages, passed, failed, not_replayable, gating_passed, gating_failed, non_gating_count)
            != (exp_total, exp_passed, exp_failed, exp_nr, exp_gp, exp_gf, exp_ng)
        {
            return Err(VerdictError::schema("$.reliability", "reliability counts do not match package results"));
        }

        let report = AcceptanceReport {
            format,
            build_id,
            manifest_digest,
            suite_digest,
            started_at,
            ended_at,
            machines,
            packages,
            reliability: ReliabilitySection {
                total_packages,
                passed,
                failed,
                not_replayable,
                gating_passed,
                gating_failed,
                non_gating_count,
                crash_recovery_passed,
            },
        };
        Ok(report)
    }

    /// Converts the report to its canonical [`CValue`] representation.
    pub fn to_value(&self) -> CValue {
        let machines =
            CValue::Arr(self.machines.iter().map(|m| obj([("host", s(&m.host)), ("id", s(&m.id))])).collect());
        let packages = CValue::Arr(
            self.packages
                .iter()
                .map(|p| {
                    let checks = CValue::Arr(
                        p.checks
                            .iter()
                            .map(|c| {
                                let mut m = BTreeMap::new();
                                m.insert("id".to_string(), s(&c.id));
                                m.insert("kind".to_string(), s(&c.kind));
                                m.insert("outcome".to_string(), s(c.outcome.as_str()));
                                if let Some(r) = &c.reason {
                                    m.insert("reason".to_string(), s(r));
                                }
                                CValue::Obj(m)
                            })
                            .collect(),
                    );
                    let mt = CValue::Obj(p.machine_times.iter().map(|(k, &v)| (k.clone(), CValue::Num(v))).collect());
                    let reasons = CValue::Arr(p.reasons.iter().map(|r| s(r)).collect());
                    let gating = match p.gating {
                        Gating::Gating => "gating",
                        Gating::Tracked => "tracked",
                    };
                    let assistance = match p.assistance {
                        Assistance::Unassisted => "unassisted",
                        Assistance::Assisted => "assisted",
                    };
                    obj([
                        ("assistance", s(assistance)),
                        ("checks", checks),
                        ("gating", s(gating)),
                        ("job", s(&p.job)),
                        ("machine_times", mt),
                        ("non_gating", CValue::Bool(p.non_gating)),
                        ("reasons", reasons),
                        ("replay_outcome", s(p.replay_outcome.as_str())),
                        ("variant", s(&p.variant)),
                        ("verdict", s(p.verdict.as_str())),
                    ])
                })
                .collect(),
        );
        let mut rel = BTreeMap::new();
        if let Some(cr) = self.reliability.crash_recovery_passed {
            rel.insert("crash_recovery_passed".to_string(), CValue::Bool(cr));
        }
        rel.insert("failed".to_string(), CValue::Num(self.reliability.failed as f64));
        rel.insert("gating_failed".to_string(), CValue::Num(self.reliability.gating_failed as f64));
        rel.insert("gating_passed".to_string(), CValue::Num(self.reliability.gating_passed as f64));
        rel.insert("non_gating_count".to_string(), CValue::Num(self.reliability.non_gating_count as f64));
        rel.insert("not_replayable".to_string(), CValue::Num(self.reliability.not_replayable as f64));
        rel.insert("passed".to_string(), CValue::Num(self.reliability.passed as f64));
        rel.insert("total_packages".to_string(), CValue::Num(self.reliability.total_packages as f64));

        obj([
            ("build_id", s(&self.build_id)),
            ("ended_at", s(&self.ended_at)),
            ("format", s(&self.format)),
            ("machines", machines),
            ("manifest_digest", s(self.manifest_digest.as_str())),
            ("packages", packages),
            ("reliability", CValue::Obj(rel)),
            ("started_at", s(&self.started_at)),
            ("suite_digest", s(self.suite_digest.as_str())),
        ])
    }

    /// Serialises the report to RFC 8785 canonical JSON bytes after verifying PII and schema rules.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, VerdictError> {
        let val = self.to_value();
        lint_pii(&val)?;
        to_canonical_bytes(&val).map_err(|e| VerdictError::schema("$", format!("canonical encode failed: {e}")))
    }

    /// Computes the SHA-256 digest of the report's RFC 8785 canonical JSON form (REQ-003).
    pub fn digest(&self) -> Result<Digest, VerdictError> {
        let bytes = self.to_canonical_bytes()?;
        let canon_d = digest_bytes(&bytes);
        Digest::parse(&canon_d.to_string())
            .map_err(|e| VerdictError::schema("$", format!("digest conversion failed: {e}")))
    }
}

// ------------------------------------------------------------------ TrendRecord impl

impl TrendRecord {
    /// Parses, PII-lints, and validates one JSONL trend record.
    pub fn parse_line(line: &[u8]) -> Result<Self, VerdictError> {
        let val = parse_json(line, 4096, "trend")?;
        Self::from_value(&val)
    }

    /// Parses and validates a [`TrendRecord`] from [`CValue`].
    pub fn from_value(val: &CValue) -> Result<Self, VerdictError> {
        lint_pii(val)?;
        let m = require_obj(val, "trend")?;
        check_closed_keys(m, &["night", "job", "variant", "verdict", "machine_s"], &[], "trend")?;
        let night = get_str(m, "night", "trend")?.to_string();
        if !is_iso_date(&night) {
            return Err(VerdictError::schema("trend.night", "expected YYYY-MM-DD date"));
        }
        let job = get_str(m, "job", "trend")?.to_string();
        if !is_job_id(&job) {
            return Err(VerdictError::schema("trend.job", format!("invalid job id `{job}`")));
        }
        let variant = get_str(m, "variant", "trend")?.to_string();
        if !is_ident(&variant) {
            return Err(VerdictError::schema("trend.variant", format!("invalid variant `{variant}`")));
        }
        let verdict = Verdict::parse(get_str(m, "verdict", "trend")?)
            .ok_or_else(|| VerdictError::schema("trend.verdict", "invalid verdict"))?;
        let machine_s = match m.get("machine_s") {
            Some(v) => get_nonneg_seconds(v, "trend.machine_s")?,
            None => return Err(VerdictError::schema("trend.machine_s", "missing machine_s")),
        };
        Ok(TrendRecord { night, job, variant, verdict, machine_s })
    }

    /// Converts the trend record to [`CValue`].
    pub fn to_value(&self) -> CValue {
        obj([
            ("job", s(&self.job)),
            ("machine_s", CValue::Num(self.machine_s)),
            ("night", s(&self.night)),
            ("variant", s(&self.variant)),
            ("verdict", s(self.verdict.as_str())),
        ])
    }

    /// Serialises the trend record to one RFC 8785 canonical JSON line (no trailing newline).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, VerdictError> {
        let val = self.to_value();
        lint_pii(&val)?;
        to_canonical_bytes(&val).map_err(|e| VerdictError::schema("trend", format!("canonical encode failed: {e}")))
    }
}

/// Parses an entire `trend.jsonl` byte buffer (strict LF lines, no blank interior lines).
pub fn parse_trend_jsonl(bytes: &[u8]) -> Result<Vec<TrendRecord>, VerdictError> {
    if bytes.len() > MAX_TREND_BYTES {
        return Err(VerdictError::schema("trend", format!("trend file exceeds {MAX_TREND_BYTES} bytes")));
    }
    let mut out = Vec::new();
    for (idx, raw_line) in bytes.split(|&b| b == b'\n').enumerate() {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if line.is_empty() {
            // Allow trailing newline at EOF only.
            let is_eof = idx == bytes.split(|&b| b == b'\n').count().saturating_sub(1);
            if is_eof {
                continue;
            }
            return Err(VerdictError::schema(format!("trend:line {}", idx + 1), "blank line in trend history"));
        }
        let rec = TrendRecord::parse_line(line)
            .map_err(|e| VerdictError::new(e.code, format!("trend:line {}", idx + 1), e.detail))?;
        out.push(rec);
    }
    Ok(out)
}

// ------------------------------------------------------------------ AcceptanceStore

/// Project-scoped store holding canonical acceptance reports (`<root>/reports/<hex>.json`) and
/// the append-only trend history (`<root>/trend.jsonl`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceStore {
    root: PathBuf,
}

impl AcceptanceStore {
    /// Opens a handle to an acceptance store directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Root directory of the store.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Path to the report file for `digest` (`<root>/reports/<hex>.json`).
    pub fn report_path(&self, digest: &Digest) -> PathBuf {
        self.root.join(REPORTS_DIR).join(format!("{}.json", digest.hex()))
    }

    /// Path to the append-only trend history file (`<root>/trend.jsonl`).
    pub fn trend_path(&self) -> PathBuf {
        self.root.join(TREND_FILE)
    }

    /// Stores a validated report under its canonical digest (refusing symlinks and verifying
    /// immutability if already stored). Returns the report's [`Digest`].
    pub fn put_report(&self, report: &AcceptanceReport) -> Result<Digest, VerdictError> {
        let bytes = report.to_canonical_bytes()?;
        // Re-validate through `from_value` to ensure struct fields satisfy all invariants.
        let _ = AcceptanceReport::parse_json(&bytes)?;
        let digest = Digest::parse(&digest_bytes(&bytes).to_string())
            .map_err(|e| VerdictError::schema("$", format!("digest conversion failed: {e}")))?;

        let dir = self.root.join(REPORTS_DIR);
        fs::create_dir_all(&dir).map_err(|_| VerdictError::schema("store", "failed to create reports directory"))?;
        let dest = self.report_path(&digest);
        if dest.exists() {
            let existing =
                fs::read(&dest).map_err(|_| VerdictError::schema("store", "failed to read existing report"))?;
            if existing != bytes {
                return Err(VerdictError::schema("store", "existing report bytes differ from canonical digest"));
            }
            return Ok(digest);
        }
        let tmp = dir.join(format!(".{}.{}.tmp", digest.hex(), std::process::id()));
        if fs::write(&tmp, &bytes).is_err() {
            let _ = fs::remove_file(&tmp);
            return Err(VerdictError::schema("store", "failed to write report temp file"));
        }
        if fs::rename(&tmp, &dest).is_err() {
            let _ = fs::remove_file(&tmp);
            return Err(VerdictError::schema("store", "failed to rename report into place"));
        }
        Ok(digest)
    }

    /// Reads and validates a report by its canonical [`Digest`], failing with `not_found` if
    /// absent and verifying that the stored bytes hash to `digest`.
    pub fn get_report(&self, digest: &Digest) -> Result<(AcceptanceReport, Vec<u8>), VerdictError> {
        let path = self.report_path(digest);
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                return Err(VerdictError::not_found(digest.to_string(), "acceptance report not found in store"));
            }
        };
        if !meta.file_type().is_file() {
            return Err(VerdictError::schema(digest.to_string(), "report path is not a regular file"));
        }
        if meta.len() > MAX_REPORT_BYTES as u64 {
            return Err(VerdictError::schema(digest.to_string(), "report exceeds maximum size"));
        }
        let bytes = fs::read(&path)
            .map_err(|_| VerdictError::not_found(digest.to_string(), "failed to read report from store"))?;
        let report = AcceptanceReport::parse_json(&bytes)?;
        let actual = report.digest()?;
        if actual != *digest {
            return Err(VerdictError::schema(
                digest.to_string(),
                format!("stored report digest mismatch: expected {digest}, computed {actual}"),
            ));
        }
        let canonical = report.to_canonical_bytes()?;
        Ok((report, canonical))
    }

    /// Appends one validated [`TrendRecord`] to `<root>/trend.jsonl` (append-only, LF-terminated).
    pub fn append_trend(&self, record: &TrendRecord) -> Result<(), VerdictError> {
        let bytes = record.to_canonical_bytes()?;
        // Validate round-trip before appending.
        let _ = TrendRecord::parse_line(&bytes)?;
        fs::create_dir_all(&self.root)
            .map_err(|_| VerdictError::schema("store", "failed to create acceptance store directory"))?;
        let path = self.trend_path();
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if !meta.file_type().is_file() {
                return Err(VerdictError::schema("trend", "trend path is not a regular file"));
            }
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|_| VerdictError::schema("trend", "failed to open trend file for append"))?;
        f.write_all(&bytes)
            .and_then(|()| f.write_all(b"\n"))
            .map_err(|_| VerdictError::schema("trend", "failed to append trend record"))?;
        Ok(())
    }

    /// Loads all trend records from `<root>/trend.jsonl` (returns empty vector if file does not
    /// exist yet).
    pub fn load_trends(&self) -> Result<Vec<TrendRecord>, VerdictError> {
        let path = self.trend_path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => return Ok(Vec::new()),
        };
        if !meta.file_type().is_file() {
            return Err(VerdictError::schema("trend", "trend path is not a regular file"));
        }
        if meta.len() > MAX_TREND_BYTES as u64 {
            return Err(VerdictError::schema("trend", "trend file exceeds maximum size"));
        }
        let bytes = fs::read(&path).map_err(|_| VerdictError::schema("trend", "failed to read trend file"))?;
        parse_trend_jsonl(&bytes)
    }
}

// ------------------------------------------------------------------ accept.show_report@1

/// Parameters for `accept.show_report@1` (REQ-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowReportParams {
    pub digest: Option<Digest>,
    pub trend: Option<String>,
    pub last: Option<u16>,
}

/// Result of `accept.show_report@1`.
#[derive(Debug, Clone, PartialEq)]
pub enum ShowReportResult {
    Report { digest: Digest, report: Box<AcceptanceReport> },
    Trend { job: String, records: Vec<TrendRecord> },
}

impl ShowReportResult {
    /// Serialises the command result to canonical [`CValue`].
    pub fn to_value(&self) -> CValue {
        match self {
            ShowReportResult::Report { digest, report } => {
                obj([("digest", s(digest.as_str())), ("kind", s("report")), ("report", report.to_value())])
            }
            ShowReportResult::Trend { job, records } => obj([
                ("job", s(job)),
                ("kind", s("trend")),
                ("records", CValue::Arr(records.iter().map(TrendRecord::to_value).collect())),
            ]),
        }
    }
}

/// Executes `accept.show_report@1` against `store` (REQ-004, AC-1..AC-3).
pub fn show_report(store: &AcceptanceStore, params: &ShowReportParams) -> Result<ShowReportResult, VerdictError> {
    match (&params.digest, &params.trend) {
        (Some(digest), None) => {
            if params.last.is_some() {
                return Err(VerdictError::schema("params.last", "`last` may only be set with `trend`"));
            }
            let (report, _) = store.get_report(digest)?;
            Ok(ShowReportResult::Report { digest: *digest, report: Box::new(report) })
        }
        (None, Some(job)) => {
            if !is_job_id(job) {
                return Err(VerdictError::schema("params.trend", format!("invalid job id `{job}`")));
            }
            let last = params.last.unwrap_or(DEFAULT_TREND_LAST);
            if last == 0 || last > MAX_TREND_LAST {
                return Err(VerdictError::schema(
                    "params.last",
                    format!("`last` must be between 1 and {MAX_TREND_LAST}, got {last}"),
                ));
            }
            let all = store.load_trends()?;
            // Preserve append-order tie-breaking for equal nights, returning newest first (AC-2).
            let mut indexed: Vec<(usize, TrendRecord)> =
                all.into_iter().enumerate().filter(|(_, r)| r.job == *job).collect();
            indexed.sort_by(|(i1, r1), (i2, r2)| r2.night.cmp(&r1.night).then_with(|| i2.cmp(i1)));
            let records: Vec<TrendRecord> = indexed.into_iter().take(usize::from(last)).map(|(_, r)| r).collect();
            Ok(ShowReportResult::Trend { job: job.clone(), records })
        }
        (Some(_), Some(_)) => Err(VerdictError::schema("params", "specify either `digest` or `trend`, not both")),
        (None, None) => Err(VerdictError::schema("params", "must specify either `digest` or `trend`")),
    }
}

/// Registry command definition for `accept.show_report@1` (R0, D0, CLI + MCP).
pub fn command_def() -> Result<CommandDef, DefError> {
    let params_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("digest", obj([("pattern", s("^sha256:[0-9a-f]{64}$")), ("type", s("string"))])),
                (
                    "last",
                    obj([
                        ("maximum", CValue::Num(f64::from(MAX_TREND_LAST))),
                        ("minimum", CValue::Num(1.0)),
                        ("type", s("integer")),
                    ]),
                ),
                ("trend", obj([("pattern", s("^JOB\\.[A-Z]{1,8}\\.[0-9]{2}$")), ("type", s("string"))])),
            ]),
        ),
        ("type", s("object")),
    ]);
    let result_schema = obj([
        ("additionalProperties", CValue::Bool(false)),
        (
            "properties",
            obj([
                ("digest", obj([("pattern", s("^sha256:[0-9a-f]{64}$")), ("type", s("string"))])),
                ("job", obj([("type", s("string"))])),
                ("kind", obj([("enum", CValue::Arr(vec![s("report"), s("trend")])), ("type", s("string"))])),
                ("records", obj([("type", s("array"))])),
                ("report", obj([("type", s("object"))])),
            ]),
        ),
        ("required", CValue::Arr(vec![s("kind")])),
        ("type", s("object")),
    ]);

    CommandDef::from_value(obj([
        ("algorithms", CValue::Arr(Vec::new())),
        (
            "description",
            s("Shows a nightly acceptance report by canonical SHA-256 digest or returns the last N trend records for a job."),
        ),
        ("determinism", s("D0")),
        ("effects", CValue::Arr(Vec::new())),
        ("exposure", CValue::Arr(vec![s("cli"), s("mcp")])),
        ("fs_effect", s("none")),
        ("human_only", CValue::Bool(false)),
        ("id", s(COMMAND_ID)),
        ("idempotency", s("natural")),
        ("lifecycle", obj([("state", s("active")), ("since", s("0.1.0"))])),
        ("major", CValue::Num(f64::from(COMMAND_MAJOR))),
        ("mcp_projection", s("exposed")),
        ("network", s("none")),
        ("params_schema", params_schema),
        ("preconditions", CValue::Arr(Vec::new())),
        ("result_schema", result_schema),
        ("side_effect", s("R0")),
        ("title", s("Show acceptance report or trend history")),
        ("undo", s("not_undoable")),
        ("web", s("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}

// ------------------------------------------------------------------ CLI runner

/// Runs `fid-accept show-report --store <dir> [--digest <sha256:...>] [--trend <job> --last <N>] [--json]`.
/// Returns `(exit_code, stdout, stderr)`.
pub fn run_cli(args: &[String]) -> (i32, String, String) {
    let mut iter = args.iter();
    let Some(sub) = iter.next() else {
        return (
            2,
            String::new(),
            "usage: fid-accept show-report --store <dir> (--digest <sha256:...> | --trend <job> [--last N]) [--json]\n"
                .to_string(),
        );
    };
    if sub != "show-report" {
        return (2, String::new(), format!("unknown subcommand `{sub}`; expected `show-report`\n"));
    }

    let mut store_dir: Option<PathBuf> = None;
    let mut digest_raw: Option<String> = None;
    let mut trend: Option<String> = None;
    let mut last: Option<u16> = None;
    let mut as_json = false;

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--store" => {
                let Some(v) = iter.next() else {
                    return (2, String::new(), "VERDICT-SCHEMA: --store requires a directory\n".to_string());
                };
                store_dir = Some(PathBuf::from(v));
            }
            "--digest" => {
                let Some(v) = iter.next() else {
                    return (2, String::new(), "VERDICT-SCHEMA: --digest requires a sha256:... value\n".to_string());
                };
                digest_raw = Some(v.clone());
            }
            "--trend" => {
                let Some(v) = iter.next() else {
                    return (2, String::new(), "VERDICT-SCHEMA: --trend requires a job ID\n".to_string());
                };
                trend = Some(v.clone());
            }
            "--last" => {
                let Some(v) = iter.next() else {
                    return (2, String::new(), "VERDICT-SCHEMA: --last requires an integer 1..=365\n".to_string());
                };
                match v.parse::<u16>() {
                    Ok(n) => last = Some(n),
                    Err(_) => {
                        return (2, String::new(), format!("VERDICT-SCHEMA: invalid --last value `{v}`\n"));
                    }
                }
            }
            "--json" => as_json = true,
            other => {
                return (2, String::new(), format!("VERDICT-SCHEMA: unknown argument `{other}`\n"));
            }
        }
    }

    let Some(dir) = store_dir else {
        return (2, String::new(), "VERDICT-SCHEMA: --store <dir> is required\n".to_string());
    };
    let digest = match digest_raw {
        Some(raw) => match Digest::parse(&raw) {
            Ok(d) => Some(d),
            Err(e) => return (2, String::new(), format!("VERDICT-SCHEMA: invalid digest `{raw}`: {e}\n")),
        },
        None => None,
    };

    let store = AcceptanceStore::new(dir);
    let params = ShowReportParams { digest, trend, last };
    match show_report(&store, &params) {
        Ok(res) => {
            if as_json {
                match to_canonical_bytes(&res.to_value()).ok().and_then(|b| String::from_utf8(b).ok()) {
                    Some(text) => (0, format!("{text}\n"), String::new()),
                    None => (2, String::new(), "VERDICT-SCHEMA: failed to encode JSON output\n".to_string()),
                }
            } else {
                let text = match res {
                    ShowReportResult::Report { digest, report } => format!(
                        "report {} build={} total={} passed={} failed={} not_replayable={}\n",
                        digest,
                        report.build_id,
                        report.reliability.total_packages,
                        report.reliability.passed,
                        report.reliability.failed,
                        report.reliability.not_replayable
                    ),
                    ShowReportResult::Trend { job, records } => {
                        let mut out = format!("trend {job} ({} records, newest first)\n", records.len());
                        for r in records {
                            out.push_str(&format!(
                                "{} {} {} {:.3}s\n",
                                r.night,
                                r.variant,
                                r.verdict.as_str(),
                                r.machine_s
                            ));
                        }
                        out
                    }
                };
                (0, text, String::new())
            }
        }
        Err(e) => {
            let code = match e.code {
                VerdictErrorCode::NotFound => 1,
                VerdictErrorCode::Schema | VerdictErrorCode::Pii => 2,
            };
            (code, String::new(), format!("{e}\n"))
        }
    }
}

fn s(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
