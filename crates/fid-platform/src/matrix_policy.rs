//! Tier 1 OS window and promotion checks (spec node `PLAT.MATRIX.POLICY.ROWS`, decision D-010).
//!
//! Pure deterministic checks (`D0`, `R0`, `BTreeMap`/`BTreeSet` only, no panics, no `unsafe`):
//! - [`parse_window`]: strict bounded parser for `platform/tier1-window.toml`.
//! - [`window_check`]: verifies that the effective Tier 1 rows at a given [`Phase`] are
//!   exactly the window set (Windows 10 & 11 x86_64, the newest `ubuntu_lts` Ubuntu LTS rows
//!   on x86_64, and the newest `macos_major` macOS major versions on aarch64), excluding rows
//!   demoted via `demoted:` notes (REQ-001, REQ-003).
//! - [`promote_check`]: compares an old and new [`Matrix`]; any row newly raised to `Tier1`
//!   requires its Tier 1 jobs (`build` and `full_test`) to have passed in the two most recent
//!   release-candidate gate verdict JSON files from `PLAT.MATRIX.CI` (REQ-004).

use crate::matrix::{Arch, Matrix, OsFamily, Phase, Row, Tier, Version};
use fid_canon::{parse_with_limits, CValue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Default path of the Tier 1 rolling window file.
pub const DEFAULT_WINDOW_PATH: &str = "platform/tier1-window.toml";
/// Default path of the support matrix file.
pub const DEFAULT_MATRIX_PATH: &str = "platform/support-matrix.toml";
/// Supported `schema_version` for `tier1-window.toml`.
pub const SUPPORTED_WINDOW_SCHEMA: u32 = 1;
/// Largest accepted `tier1-window.toml`, in bytes (64 KiB).
pub const MAX_WINDOW_BYTES: usize = 64 * 1024;
/// Largest accepted gate verdict JSON file, in bytes (4 MiB).
pub const MAX_VERDICT_BYTES: usize = 4 * 1024 * 1024;
/// Maximum JSON nesting depth accepted in a gate verdict file.
pub const MAX_VERDICT_DEPTH: usize = 16;
/// Number of consecutive passing release-candidate verdicts required to promote a row to Tier 1.
pub const REQUIRED_RC_VERDICTS: usize = 2;

const WINDOW_KEYS: [&str; 4] = ["macos_major", "schema_version", "ubuntu_lts", "windows"];

/// Parsed `platform/tier1-window.toml` rule (D-010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tier1Window {
    pub schema_version: u32,
    pub ubuntu_lts: u8,
    pub macos_major: u8,
    pub windows: Vec<u32>,
}

/// Parse error for `tier1-window.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowError {
    TooLarge { limit: usize },
    NotUtf8,
    MalformedToml(String),
    UnknownKey(String),
    MissingKey(&'static str),
    BadType { key: &'static str, expected: &'static str },
    UnsupportedSchema { found: i64 },
    InvalidValue { key: &'static str, detail: String },
}

impl fmt::Display for WindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WindowError::TooLarge { limit } => write!(f, "TooLarge {{ limit: {limit} }}"),
            WindowError::NotUtf8 => write!(f, "NotUtf8"),
            WindowError::MalformedToml(m) => write!(f, "MalformedToml {{ detail: {m} }}"),
            WindowError::UnknownKey(k) => write!(f, "UnknownKey {{ key: {k} }}"),
            WindowError::MissingKey(k) => write!(f, "MissingKey {{ key: {k} }}"),
            WindowError::BadType { key, expected } => {
                write!(f, "BadType {{ key: {key}, expected: {expected} }}")
            }
            WindowError::UnsupportedSchema { found } => {
                write!(f, "UnsupportedSchema {{ found: {found}, supported: {SUPPORTED_WINDOW_SCHEMA} }}")
            }
            WindowError::InvalidValue { key, detail } => {
                write!(f, "InvalidValue {{ key: {key}, detail: {detail} }}")
            }
        }
    }
}

/// Findings emitted by [`window_check`] and [`promote_check`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PolicyFinding {
    /// An effective Tier 1 row falls outside the D-010 window (REQ-001).
    OutsideWindow { os: OsFamily, version: Version, arch: Arch },
    /// An expected window slot has no effective Tier 1 row (REQ-001).
    MissingFromWindow { os: OsFamily, version: String, arch: Arch },
    /// A row raised to Tier 1 lacks passing Tier 1 jobs in both latest RC verdicts (REQ-004).
    PromotionNotEarned { os: OsFamily, version: Version, arch: Arch, missing_jobs: Vec<String> },
    /// Fewer than [`REQUIRED_RC_VERDICTS`] gate verdict files were found when promoting a row.
    NoVerdicts { needed: usize, found: usize },
    /// A gate verdict JSON file could not be parsed.
    MalformedVerdict { file: String, detail: String },
}

/// Renders an OS version for human-readable findings and CI job IDs.
/// Ubuntu releases use two-digit month padding (`22.04`, `24.04`, `26.04`).
pub fn format_os_version(os: OsFamily, version: &Version) -> String {
    let c = version.components();
    if os == OsFamily::Ubuntu && c.len() >= 2 {
        let mut s = format!("{}.{:02}", c[0], c[1]);
        for part in &c[2..] {
            s.push('.');
            s.push_str(&part.to_string());
        }
        s
    } else {
        version.to_string()
    }
}

impl fmt::Display for PolicyFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyFinding::OutsideWindow { os, version, arch } => {
                let ver = format_os_version(*os, version);
                write!(f, "OutsideWindow {{ os: {os}, version: {ver}, arch: {arch} }}")
            }
            PolicyFinding::MissingFromWindow { os, version, arch } => {
                write!(f, "MissingFromWindow {{ os: {os}, version: {version}, arch: {arch} }}")
            }
            PolicyFinding::PromotionNotEarned { os, version, arch, missing_jobs } => {
                let ver = format_os_version(*os, version);
                write!(
                    f,
                    "PromotionNotEarned {{ os: {os}, version: {ver}, arch: {arch}, missing_jobs: [{}] }}",
                    missing_jobs.join(", ")
                )
            }
            PolicyFinding::NoVerdicts { needed, found } => {
                write!(f, "NoVerdicts {{ needed: {needed}, found: {found} }}")
            }
            PolicyFinding::MalformedVerdict { file, detail } => {
                write!(f, "MalformedVerdict {{ file: {file}, detail: {detail} }}")
            }
        }
    }
}

/// Strictly parses `platform/tier1-window.toml`.
pub fn parse_window(bytes: &[u8]) -> Result<Tier1Window, Vec<WindowError>> {
    if bytes.len() > MAX_WINDOW_BYTES {
        return Err(vec![WindowError::TooLarge { limit: MAX_WINDOW_BYTES }]);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| vec![WindowError::NotUtf8])?;
    let table: toml::Table =
        text.parse().map_err(|e: toml::de::Error| vec![WindowError::MalformedToml(e.to_string())])?;

    let mut errors = Vec::new();
    for k in table.keys() {
        if !WINDOW_KEYS.contains(&k.as_str()) {
            errors.push(WindowError::UnknownKey(k.clone()));
        }
    }

    let schema_version = match table.get("schema_version") {
        None => {
            errors.push(WindowError::MissingKey("schema_version"));
            0
        }
        Some(toml::Value::Integer(n)) => {
            if *n != i64::from(SUPPORTED_WINDOW_SCHEMA) {
                errors.push(WindowError::UnsupportedSchema { found: *n });
                0
            } else {
                SUPPORTED_WINDOW_SCHEMA
            }
        }
        Some(_) => {
            errors.push(WindowError::BadType { key: "schema_version", expected: "integer" });
            0
        }
    };

    let ubuntu_lts = parse_u8_field(&table, "ubuntu_lts", &mut errors);
    let macos_major = parse_u8_field(&table, "macos_major", &mut errors);

    let mut windows = Vec::new();
    match table.get("windows") {
        None => errors.push(WindowError::MissingKey("windows")),
        Some(toml::Value::Array(items)) => {
            let mut seen = BTreeSet::new();
            for item in items {
                match item {
                    toml::Value::Integer(n) if *n >= 1 && *n <= i64::from(u32::MAX) => {
                        let v = *n as u32;
                        if seen.insert(v) {
                            windows.push(v);
                        } else {
                            errors.push(WindowError::InvalidValue {
                                key: "windows",
                                detail: format!("duplicate Windows major {v}"),
                            });
                        }
                    }
                    _ => errors.push(WindowError::BadType { key: "windows", expected: "array of positive integers" }),
                }
            }
            windows.sort_unstable();
        }
        Some(_) => errors.push(WindowError::BadType { key: "windows", expected: "array of positive integers" }),
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Tier1Window { schema_version, ubuntu_lts, macos_major, windows })
}

fn parse_u8_field(table: &toml::Table, key: &'static str, errors: &mut Vec<WindowError>) -> u8 {
    match table.get(key) {
        None => {
            errors.push(WindowError::MissingKey(key));
            0
        }
        Some(toml::Value::Integer(n)) if *n >= 0 && *n <= i64::from(u8::MAX) => *n as u8,
        Some(toml::Value::Integer(n)) => {
            errors.push(WindowError::InvalidValue { key, detail: format!("out of range 0..=255: {n}") });
            0
        }
        Some(_) => {
            errors.push(WindowError::BadType { key, expected: "integer" });
            0
        }
    }
}

/// True if a matrix row has been demoted via a `demoted:` entry in its `notes` (REQ-003).
pub fn is_demoted_row(row: &Row) -> bool {
    row.notes.contains("demoted:")
}

/// True if `v` is an Ubuntu LTS version (even year, `.04` release, e.g. `22.04`, `24.04`, `26.04`).
pub fn is_ubuntu_lts(v: &Version) -> bool {
    let c = v.components();
    c.len() >= 2 && c[0] % 2 == 0 && c[1] == 4
}

/// Maps a Windows `version_min` to its desktop major generation (`10` for Windows 10 builds
/// `< 10.0.22000` or explicit `10`, `11` for builds `>= 10.0.22000` or explicit `11`).
pub fn windows_major(v: &Version) -> Option<u32> {
    let c = v.components();
    let major = *c.first()?;
    if major == 10 {
        if c.len() >= 3 && c[1] == 0 && c[2] >= 22_000 {
            Some(11)
        } else {
            Some(10)
        }
    } else {
        Some(major)
    }
}

/// Checks that the effective Tier 1 rows of `matrix` at `phase` match `window` exactly,
/// excluding rows marked with `demoted:` in `notes` (REQ-001, REQ-003, REQ-005).
pub fn window_check(matrix: &Matrix, window: &Tier1Window, phase: Phase) -> Vec<PolicyFinding> {
    let mut findings = Vec::new();

    // Active (effective at `phase`) non-demoted rows.
    let active_rows: Vec<&Row> = matrix.rows.iter().filter(|r| r.since_phase <= phase && !is_demoted_row(r)).collect();

    // 1. Ubuntu LTS x86_64: newest `window.ubuntu_lts` non-demoted LTS versions listed in the matrix.
    let mut ubuntu_lts_versions: BTreeSet<Version> = BTreeSet::new();
    for r in &active_rows {
        if r.os == OsFamily::Ubuntu && r.arch == Arch::X86_64 && is_ubuntu_lts(&r.version_min) {
            ubuntu_lts_versions.insert(r.version_min);
        }
    }
    let expected_ubuntu: BTreeSet<Version> =
        ubuntu_lts_versions.iter().rev().take(usize::from(window.ubuntu_lts)).cloned().collect();

    // 2. macOS aarch64: newest `window.macos_major` non-demoted majors listed in the matrix.
    let mut macos_majors: BTreeSet<u32> = BTreeSet::new();
    for r in &active_rows {
        if r.os == OsFamily::Macos && r.arch == Arch::Aarch64 {
            if let Some(&maj) = r.version_min.components().first() {
                macos_majors.insert(maj);
            }
        }
    }
    let expected_macos: BTreeSet<u32> =
        macos_majors.iter().rev().take(usize::from(window.macos_major)).copied().collect();

    // 3. Windows x86_64: majors listed in `window.windows`, minus any major that has a demoted row.
    let mut demoted_win_majors: BTreeSet<u32> = BTreeSet::new();
    for r in &matrix.rows {
        if r.since_phase <= phase && r.os == OsFamily::Windows && r.arch == Arch::X86_64 && is_demoted_row(r) {
            if let Some(m) = windows_major(&r.version_min) {
                demoted_win_majors.insert(m);
            }
        }
    }
    let expected_windows: BTreeSet<u32> = window.windows.iter().copied().collect();

    // Check every effective Tier 1 row against the allowed window sets.
    let mut found_ubuntu: BTreeSet<Version> = BTreeSet::new();
    let mut found_macos: BTreeSet<u32> = BTreeSet::new();
    let mut found_windows: BTreeSet<u32> = BTreeSet::new();

    for r in &active_rows {
        if r.tier != Tier::Tier1 {
            continue;
        }
        match (r.os, r.arch) {
            (OsFamily::Ubuntu, Arch::X86_64) => {
                if expected_ubuntu.contains(&r.version_min) {
                    found_ubuntu.insert(r.version_min);
                } else {
                    findings.push(PolicyFinding::OutsideWindow { os: r.os, version: r.version_min, arch: r.arch });
                }
            }
            (OsFamily::Macos, Arch::Aarch64) => {
                let maj = r.version_min.components().first().copied();
                if maj.is_some_and(|m| expected_macos.contains(&m)) {
                    if let Some(m) = maj {
                        found_macos.insert(m);
                    }
                } else {
                    findings.push(PolicyFinding::OutsideWindow { os: r.os, version: r.version_min, arch: r.arch });
                }
            }
            (OsFamily::Windows, Arch::X86_64) => {
                let maj = windows_major(&r.version_min);
                if maj.is_some_and(|m| expected_windows.contains(&m)) {
                    if let Some(m) = maj {
                        found_windows.insert(m);
                    }
                } else {
                    findings.push(PolicyFinding::OutsideWindow { os: r.os, version: r.version_min, arch: r.arch });
                }
            }
            _ => {
                findings.push(PolicyFinding::OutsideWindow { os: r.os, version: r.version_min, arch: r.arch });
            }
        }
    }

    // Check for missing expected window entries (only when phase >= P1a or when expected sets are populated).
    if phase >= Phase::P1a {
        for v in &expected_ubuntu {
            if !found_ubuntu.contains(v) {
                findings.push(PolicyFinding::MissingFromWindow {
                    os: OsFamily::Ubuntu,
                    version: format_os_version(OsFamily::Ubuntu, v),
                    arch: Arch::X86_64,
                });
            }
        }
        if expected_ubuntu.is_empty() && window.ubuntu_lts > 0 {
            findings.push(PolicyFinding::MissingFromWindow {
                os: OsFamily::Ubuntu,
                version: format!("lts(x{})", window.ubuntu_lts),
                arch: Arch::X86_64,
            });
        }

        for m in &expected_macos {
            if !found_macos.contains(m) {
                findings.push(PolicyFinding::MissingFromWindow {
                    os: OsFamily::Macos,
                    version: m.to_string(),
                    arch: Arch::Aarch64,
                });
            }
        }
        if expected_macos.is_empty() && window.macos_major > 0 {
            // Only flag missing if there is no demoted macOS row accounting for the slot.
            let has_demoted_macos = matrix.rows.iter().any(|r| {
                r.since_phase <= phase && r.os == OsFamily::Macos && r.arch == Arch::Aarch64 && is_demoted_row(r)
            });
            if !has_demoted_macos {
                findings.push(PolicyFinding::MissingFromWindow {
                    os: OsFamily::Macos,
                    version: format!("major(x{})", window.macos_major),
                    arch: Arch::Aarch64,
                });
            }
        }

        for m in &expected_windows {
            if !found_windows.contains(m) && !demoted_win_majors.contains(m) {
                findings.push(PolicyFinding::MissingFromWindow {
                    os: OsFamily::Windows,
                    version: m.to_string(),
                    arch: Arch::X86_64,
                });
            }
        }
    }

    findings.sort();
    findings
}

/// Parsed summary of one release-candidate gate verdict file from `PLAT.MATRIX.CI`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedVerdict {
    file: String,
    verdict_pass: bool,
    blocking_jobs: BTreeSet<String>,
    job_statuses: BTreeMap<String, String>,
}

fn parse_verdict_json(file: &str, bytes: &[u8]) -> Result<ParsedVerdict, PolicyFinding> {
    if bytes.len() > MAX_VERDICT_BYTES {
        return Err(PolicyFinding::MalformedVerdict {
            file: file.to_owned(),
            detail: format!("exceeds {MAX_VERDICT_BYTES} bytes"),
        });
    }
    let val = parse_with_limits(bytes, MAX_VERDICT_BYTES, MAX_VERDICT_DEPTH)
        .map_err(|e| PolicyFinding::MalformedVerdict { file: file.to_owned(), detail: e.to_string() })?;
    let CValue::Obj(root) = val else {
        return Err(PolicyFinding::MalformedVerdict {
            file: file.to_owned(),
            detail: "verdict must be a JSON object".to_owned(),
        });
    };

    let verdict_pass = match root.get("verdict") {
        Some(CValue::Str(s)) if s == "PASS" => true,
        Some(CValue::Str(s)) if s == "BLOCK" => false,
        _ => {
            return Err(PolicyFinding::MalformedVerdict {
                file: file.to_owned(),
                detail: "missing or invalid verdict (expected PASS or BLOCK)".to_owned(),
            });
        }
    };

    let mut blocking_jobs = BTreeSet::new();
    if let Some(CValue::Arr(items)) = root.get("blocking") {
        for item in items {
            match item {
                CValue::Str(id) => {
                    blocking_jobs.insert(id.clone());
                }
                CValue::Obj(m) => {
                    if let Some(CValue::Str(id)) = m.get("id").or_else(|| m.get("job")) {
                        blocking_jobs.insert(id.clone());
                    }
                }
                CValue::Arr(pair) => {
                    if let Some(CValue::Str(id)) = pair.first() {
                        blocking_jobs.insert(id.clone());
                    }
                }
                _ => {}
            }
        }
    }

    let mut job_statuses = BTreeMap::new();
    if let Some(CValue::Arr(jobs)) = root.get("jobs").or_else(|| root.get("results")) {
        for j in jobs {
            if let CValue::Obj(m) = j {
                if let (Some(CValue::Str(id)), Some(CValue::Str(st))) = (m.get("id"), m.get("status")) {
                    job_statuses.insert(id.clone(), st.clone());
                }
            }
        }
    } else if let Some(CValue::Obj(jobs_map)) = root.get("jobs").or_else(|| root.get("results")) {
        for (id, v) in jobs_map {
            if let CValue::Str(st) = v {
                job_statuses.insert(id.clone(), st.clone());
            }
        }
    }

    Ok(ParsedVerdict { file: file.to_owned(), verdict_pass, blocking_jobs, job_statuses })
}

/// Checks every row whose tier was raised to `Tier1` between `old` and `new` against the two
/// latest release-candidate gate verdicts (`verdicts` as `(filename, json_bytes)` sorted by
/// filename ascending; REQ-004, REQ-005).
pub fn promote_check(old: &Matrix, new: &Matrix, verdicts: &[(String, Vec<u8>)]) -> Vec<PolicyFinding> {
    let mut old_tiers: BTreeMap<(OsFamily, Version, Arch), Tier> = BTreeMap::new();
    for r in &old.rows {
        old_tiers.insert((r.os, r.version_min, r.arch), r.tier);
    }

    let mut raised: Vec<&Row> = Vec::new();
    for r in &new.rows {
        if r.tier != Tier::Tier1 || is_demoted_row(r) {
            continue;
        }
        let key = (r.os, r.version_min, r.arch);
        let prev = old_tiers.get(&key).copied();
        if prev != Some(Tier::Tier1) {
            raised.push(r);
        }
    }

    if raised.is_empty() {
        return Vec::new();
    }

    if verdicts.len() < REQUIRED_RC_VERDICTS {
        return vec![PolicyFinding::NoVerdicts { needed: REQUIRED_RC_VERDICTS, found: verdicts.len() }];
    }

    // Sort verdict files deterministically by filename and take the two newest.
    let mut sorted_files: Vec<&(String, Vec<u8>)> = verdicts.iter().collect();
    sorted_files.sort_by(|a, b| a.0.cmp(&b.0));
    let latest_two = &sorted_files[sorted_files.len() - REQUIRED_RC_VERDICTS..];

    let mut parsed = Vec::with_capacity(REQUIRED_RC_VERDICTS);
    let mut findings = Vec::new();
    for (name, bytes) in latest_two {
        match parse_verdict_json(name, bytes) {
            Ok(v) => parsed.push(v),
            Err(f) => findings.push(f),
        }
    }
    if !findings.is_empty() {
        findings.sort();
        return findings;
    }

    for r in raised {
        let ver_disp = format_os_version(r.os, &r.version_min);
        let ver_raw = r.version_min.to_string();
        let req_jobs = [
            (format!("{}-{}-{}-build", r.os, ver_disp, r.arch), format!("{}-{}-{}-build", r.os, ver_raw, r.arch)),
            (
                format!("{}-{}-{}-full_test", r.os, ver_disp, r.arch),
                format!("{}-{}-{}-full_test", r.os, ver_raw, r.arch),
            ),
        ];
        let mut missing = Vec::new();
        for v in &parsed {
            for (primary_id, alt_id) in &req_jobs {
                let blocked = v.blocking_jobs.contains(primary_id) || v.blocking_jobs.contains(alt_id);
                let passed = if v.job_statuses.is_empty() {
                    // When a verdict omits per-job status list, require PASS and not in blocking.
                    v.verdict_pass && !blocked
                } else {
                    let st_ok = v
                        .job_statuses
                        .get(primary_id)
                        .or_else(|| v.job_statuses.get(alt_id))
                        .is_some_and(|s| s == "passed");
                    !blocked && st_ok
                };
                if !passed {
                    missing.push(format!("{}:{primary_id}", v.file));
                }
            }
        }
        if !missing.is_empty() {
            findings.push(PolicyFinding::PromotionNotEarned {
                os: r.os,
                version: r.version_min,
                arch: r.arch,
                missing_jobs: missing,
            });
        }
    }

    findings.sort();
    findings
}
