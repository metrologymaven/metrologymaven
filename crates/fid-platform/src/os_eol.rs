//! OS end-of-support tracking, warning and demotion check (spec node
//! `PLAT.MATRIX.POLICY.EOL`, REQ-PLAT.MATRIX.POLICY.EOL-001 .. -006).
//!
//! Reads `platform/os-eol.toml` (capped at [`MAX_BYTES`] = 64 KiB) and checks it against
//! `platform/support-matrix.toml` at an explicit `--today <YYYY-MM-DD>` date (no system clock,
//! D0 deterministic):
//! - Every Tier 1 or Tier 2 row in the matrix must have a matching EOL entry
//!   (`MissingEolEntry`).
//! - Effective end date is `extended_until` when present, else `eol` (REQ-002).
//! - When `today` is within [`WARN_LEAD_DAYS`] (90) calendar days before or on the effective
//!   end date, emit a warning and exit 0 (REQ-003).
//! - When `today` is strictly after the effective end date and a matching row is effective
//!   Tier 1 without a `demoted: <YYYY-MM-DD> eol` note, emit `PastEndOfSupport` and exit 1
//!   (REQ-004).
//! - Every entry must carry a `source` URL (`MissingSource`, REQ-005).

use crate::matrix::{self, Matrix, OsFamily, Phase, Row, Tier, Version};
use std::collections::BTreeSet;
use std::fmt;

/// Default path of the OS end-of-support list.
pub const DEFAULT_EOL_PATH: &str = "platform/os-eol.toml";
/// Default path of the support matrix.
pub const DEFAULT_MATRIX_PATH: &str = "platform/support-matrix.toml";
/// Supported `schema_version`.
pub const SUPPORTED_SCHEMA: u32 = 1;
/// Largest accepted `os-eol.toml`, in bytes (64 KiB, Build Prompt).
pub const MAX_BYTES: usize = 64 * 1024;
/// Most entries accepted in one file.
pub const MAX_ENTRIES: usize = 1000;
/// Warning lead time in calendar days before the effective end date (REQ-003).
pub const WARN_LEAD_DAYS: i64 = 90;

const TOP_KEYS: [&str; 2] = ["entry", "schema_version"];
const ENTRY_KEYS: [&str; 5] = ["eol", "extended_until", "os", "source", "version"];

/// Proleptic Gregorian calendar date (`YYYY-MM-DD`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Date {
    /// Parses strict `YYYY-MM-DD` (valid Gregorian calendar date, year 1900..=9999).
    pub fn parse(s: &str) -> Option<Self> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let year: u16 = s.get(0..4)?.parse().ok()?;
        let month: u8 = s.get(5..7)?.parse().ok()?;
        let day: u8 = s.get(8..10)?.parse().ok()?;
        if !(1900..=9999).contains(&year) || !(1..=12).contains(&month) || day == 0 {
            return None;
        }
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let max_day = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if leap {
                    29
                } else {
                    28
                }
            }
            _ => return None,
        };
        (day <= max_day).then_some(Self { year, month, day })
    }

    /// Days since 1970-01-01 (Howard Hinnant's exact civil-to-days algorithm).
    pub fn days_since_epoch(self) -> i64 {
        let mut y = i64::from(self.year);
        let m = i64::from(self.month);
        let d = i64::from(self.day);
        if m <= 2 {
            y -= 1;
        }
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = if m > 2 { m - 3 } else { m + 9 };
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// One OS end-of-support entry in `platform/os-eol.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EolEntry {
    pub os: OsFamily,
    pub version: Version,
    pub eol: Date,
    pub extended_until: Option<Date>,
    pub source: String,
}

impl EolEntry {
    /// Effective end date: `extended_until` when present, else `eol` (REQ-002).
    pub fn effective_end(&self) -> Date {
        self.extended_until.unwrap_or(self.eol)
    }
}

/// Parsed `platform/os-eol.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsEol {
    pub schema_version: u32,
    pub entries: Vec<EolEntry>,
}

/// Errors produced while parsing or evaluating `os-eol.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EolError {
    TooLarge { found: usize, max: usize },
    UnsupportedSchema { found: u64 },
    Parse { message: String },
    MissingSource { entry: usize, os: String, version: String },
    MissingEolEntry { row: usize, os: OsFamily, version: String, tier: Tier },
    PastEndOfSupport { row: usize, os: OsFamily, version: String, arch: String, effective_end: Date, today: Date },
}

impl EolError {
    /// Stable diagnostic code name matching `PLAT.MATRIX.POLICY.EOL`.
    pub fn code(&self) -> &'static str {
        match self {
            EolError::TooLarge { .. } => "TooLarge",
            EolError::UnsupportedSchema { .. } => "UnsupportedSchema",
            EolError::Parse { .. } => "Parse",
            EolError::MissingSource { .. } => "MissingSource",
            EolError::MissingEolEntry { .. } => "MissingEolEntry",
            EolError::PastEndOfSupport { .. } => "PastEndOfSupport",
        }
    }
}

impl fmt::Display for EolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EolError::TooLarge { found, max } => write!(f, "TooLarge: {found} bytes exceeds {max} byte cap"),
            EolError::UnsupportedSchema { found } => {
                write!(f, "UnsupportedSchema: schema_version {found} > {SUPPORTED_SCHEMA}")
            }
            EolError::Parse { message } => write!(f, "Parse: {message}"),
            EolError::MissingSource { entry, os, version } => {
                write!(f, "MissingSource: entry {entry} ({os} {version}) lacks a valid http(s) source URL")
            }
            EolError::MissingEolEntry { row, os, version, tier } => {
                write!(f, "MissingEolEntry: row {row} ({os} {version}, {tier}) has no entry in os-eol.toml")
            }
            EolError::PastEndOfSupport { row, os, version, arch, effective_end, today } => {
                write!(
                    f,
                    "PastEndOfSupport: row {row} ({os} {version} {arch}) passed effective end {effective_end} (today {today}) and is still tier1 without a `demoted: <date> eol` note"
                )
            }
        }
    }
}

/// Non-fatal warning when `today` is within 90 days before a row's effective end date (REQ-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EolWarning {
    pub row: usize,
    pub os: OsFamily,
    pub version: String,
    pub arch: String,
    pub effective_end: Date,
    pub days_remaining: i64,
}

impl fmt::Display for EolWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: row {} ({} {} {}) reaches effective end of support on {} ({} day(s) remaining)",
            self.row, self.os, self.version, self.arch, self.effective_end, self.days_remaining
        )
    }
}

/// Complete outcome of `eol-check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    pub rows_checked: usize,
    pub entries: usize,
    pub warnings: Vec<EolWarning>,
    pub errors: Vec<EolError>,
}

impl CheckOutcome {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.errors.is_empty())
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        for w in &self.warnings {
            s.push_str(&format!("{w}\n"));
        }
        for e in &self.errors {
            s.push_str(&format!("{e}\n"));
        }
        s.push_str(&format!(
            "eol-check: {} row(s) checked, {} entry/entries, {} warning(s), {} error(s)\n",
            self.rows_checked,
            self.entries,
            self.warnings.len(),
            self.errors.len()
        ));
        s
    }
}

fn valid_source_url(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")) else {
        return false;
    };
    if rest.is_empty() || s.len() > 1024 || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let host = rest.split('/').next().unwrap_or("");
    !host.is_empty() && host.contains('.') && !host.starts_with('.') && !host.ends_with('.')
}

fn parse_toml_date(v: &toml::Value) -> Option<Date> {
    match v {
        toml::Value::String(s) => Date::parse(s.trim()),
        toml::Value::Datetime(dt) => {
            if dt.time.is_some() || dt.offset.is_some() {
                return None;
            }
            let d = dt.date?;
            Date::parse(&format!("{:04}-{:02}-{:02}", d.year, d.month, d.day))
        }
        _ => None,
    }
}

/// Parses and validates `os-eol.toml` bytes (capped at [`MAX_BYTES`]).
pub fn parse(bytes: &[u8]) -> Result<OsEol, Vec<EolError>> {
    if bytes.len() > MAX_BYTES {
        return Err(vec![EolError::TooLarge { found: bytes.len(), max: MAX_BYTES }]);
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => return Err(vec![EolError::Parse { message: format!("not valid UTF-8: {e}") }]),
    };
    let table: toml::Table = match toml::from_str(text) {
        Ok(t) => t,
        Err(e) => return Err(vec![EolError::Parse { message: format!("invalid TOML: {e}") }]),
    };

    let mut errors = Vec::new();
    for k in table.keys() {
        if !TOP_KEYS.contains(&k.as_str()) {
            errors.push(EolError::Parse { message: format!("unknown top-level key `{k}`") });
        }
    }

    let schema_version = match table.get("schema_version") {
        Some(toml::Value::Integer(n)) if *n >= 1 => {
            let v = *n as u64;
            if v > u64::from(SUPPORTED_SCHEMA) {
                return Err(vec![EolError::UnsupportedSchema { found: v }]);
            }
            v as u32
        }
        _ => {
            errors.push(EolError::Parse { message: "`schema_version` must be integer 1".into() });
            SUPPORTED_SCHEMA
        }
    };

    let arr = match table.get("entry") {
        Some(toml::Value::Array(a)) => a,
        _ => {
            errors.push(EolError::Parse { message: "missing `[[entry]]` array".into() });
            return Err(errors);
        }
    };
    if arr.len() > MAX_ENTRIES {
        errors.push(EolError::Parse { message: format!("{} entries exceeds limit {MAX_ENTRIES}", arr.len()) });
        return Err(errors);
    }

    let mut entries = Vec::with_capacity(arr.len());
    let mut seen = BTreeSet::new();

    for (idx, item) in arr.iter().enumerate() {
        let n = idx + 1;
        let Some(tbl) = item.as_table() else {
            errors.push(EolError::Parse { message: format!("entry {n} must be a TOML table") });
            continue;
        };
        for k in tbl.keys() {
            if !ENTRY_KEYS.contains(&k.as_str()) {
                errors.push(EolError::Parse { message: format!("entry {n}: unknown key `{k}`") });
            }
        }
        let os_str = match tbl.get("os") {
            Some(toml::Value::String(s)) => s.as_str(),
            _ => "",
        };
        let os = match OsFamily::from_name(os_str) {
            Some(o) => Some(o),
            None => {
                errors.push(EolError::Parse { message: format!("entry {n}: invalid `os` {os_str:?}") });
                None
            }
        };
        let ver_str = match tbl.get("version") {
            Some(toml::Value::String(s)) => s.as_str(),
            _ => "",
        };
        let version = match Version::parse(ver_str) {
            Some(v) => Some(v),
            None => {
                errors.push(EolError::Parse { message: format!("entry {n}: invalid `version` {ver_str:?}") });
                None
            }
        };
        let eol = match tbl.get("eol").and_then(parse_toml_date) {
            Some(d) => Some(d),
            None => {
                errors.push(EolError::Parse { message: format!("entry {n}: `eol` must be YYYY-MM-DD") });
                None
            }
        };
        let extended_until = match tbl.get("extended_until") {
            None => None,
            Some(v) => match parse_toml_date(v) {
                Some(d) => {
                    if let Some(base) = eol {
                        if d < base {
                            errors.push(EolError::Parse {
                                message: format!("entry {n}: `extended_until` ({d}) precedes `eol` ({base})"),
                            });
                        }
                    }
                    Some(d)
                }
                None => {
                    errors.push(EolError::Parse { message: format!("entry {n}: `extended_until` must be YYYY-MM-DD") });
                    None
                }
            },
        };
        let source = match tbl.get("source") {
            Some(toml::Value::String(s)) if valid_source_url(s.trim()) => Some(s.trim().to_string()),
            _ => {
                errors.push(EolError::MissingSource { entry: n, os: os_str.to_string(), version: ver_str.to_string() });
                None
            }
        };

        if let (Some(os), Some(version), Some(eol), Some(source)) = (os, version, eol, source) {
            if !seen.insert((os, version)) {
                errors.push(EolError::Parse { message: format!("entry {n}: duplicate ({os}, {version})") });
                continue;
            }
            entries.push(EolEntry { os, version, eol, extended_until, source });
        }
    }

    if errors.is_empty() {
        Ok(OsEol { schema_version, entries })
    } else {
        Err(errors)
    }
}

/// Checks whether a row's `notes` carries a valid `demoted: <YYYY-MM-DD> eol` token (REQ-004).
pub fn has_demoted_eol_note(notes: &str) -> bool {
    let mut rest = notes;
    while let Some(pos) = rest.find("demoted:") {
        let after = rest[pos + "demoted:".len()..].trim_start();
        let mut parts = after.split_whitespace();
        if let (Some(date_s), Some(reason)) = (parts.next(), parts.next()) {
            let reason_clean = reason.trim_end_matches(['.', ',', ';', ')']);
            if Date::parse(date_s).is_some() && reason_clean == "eol" {
                return true;
            }
        }
        rest = &rest[pos + "demoted:".len()..];
    }
    false
}

fn row_matches_entry(row: &Row, entry: &EolEntry) -> bool {
    if row.os != entry.os {
        return false;
    }
    entry.version >= row.version_min
        && match &row.version_max {
            Some(max) => entry.version < *max,
            None => true,
        }
}

/// Evaluates `eol` against `matrix` on calendar date `today` (optional `phase` filter for
/// effective Tier 1; when `None`, every `tier1` row is treated as effective Tier 1).
pub fn evaluate(matrix: &Matrix, eol: &OsEol, today: Date, phase: Option<Phase>) -> CheckOutcome {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    let mut rows_checked = 0usize;

    for (idx, row) in matrix.rows.iter().enumerate() {
        let row_num = idx + 1;
        if row.tier != Tier::Tier1 && row.tier != Tier::Tier2 {
            continue;
        }
        rows_checked += 1;

        let matching: Vec<&EolEntry> = eol.entries.iter().filter(|e| row_matches_entry(row, e)).collect();
        if matching.is_empty() {
            errors.push(EolError::MissingEolEntry {
                row: row_num,
                os: row.os,
                version: row.version_min.to_string(),
                tier: row.tier,
            });
            continue;
        }

        for entry in matching {
            let eff_end = entry.effective_end();
            let days_remaining = eff_end.days_since_epoch() - today.days_since_epoch();
            if (0..=WARN_LEAD_DAYS).contains(&days_remaining) {
                warnings.push(EolWarning {
                    row: row_num,
                    os: row.os,
                    version: row.version_min.to_string(),
                    arch: row.arch.to_string(),
                    effective_end: eff_end,
                    days_remaining,
                });
            } else if days_remaining < 0 {
                let is_effective_tier1 = row.tier == Tier::Tier1 && phase.is_none_or(|p| p >= row.since_phase);
                if is_effective_tier1 && !has_demoted_eol_note(&row.notes) {
                    errors.push(EolError::PastEndOfSupport {
                        row: row_num,
                        os: row.os,
                        version: row.version_min.to_string(),
                        arch: row.arch.to_string(),
                        effective_end: eff_end,
                        today,
                    });
                }
            }
        }
    }

    CheckOutcome { rows_checked, entries: eol.entries.len(), warnings, errors }
}

/// Validates both files from bytes and evaluates at `today`.
pub fn check_bytes(
    matrix_bytes: &[u8],
    eol_bytes: &[u8],
    today: Date,
    phase: Option<Phase>,
) -> Result<CheckOutcome, Vec<String>> {
    let m = match matrix::parse(matrix_bytes) {
        Ok(m) => m,
        Err(err) => return Err(vec![format!("matrix: {err}")]),
    };
    let e = match parse(eol_bytes) {
        Ok(e) => e,
        Err(errs) => return Err(errs.into_iter().map(|e| format!("{e}")).collect()),
    };
    Ok(evaluate(&m, &e, today, phase))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_parse_and_diff() {
        let d1 = Date::parse("2026-01-15");
        let d2 = Date::parse("2026-03-01");
        assert!(d1.is_some() && d2.is_some());
        if let (Some(a), Some(b)) = (d1, d2) {
            assert_eq!(b.days_since_epoch() - a.days_since_epoch(), 45);
        }
        assert!(Date::parse("2024-02-29").is_some());
        assert!(Date::parse("2026-02-29").is_none());
        assert!(Date::parse("2026-13-01").is_none());
        assert!(Date::parse("not-a-date").is_none());
    }

    #[test]
    fn demoted_note_detection() {
        assert!(has_demoted_eol_note("demoted: 2026-03-02 eol"));
        assert!(has_demoted_eol_note("fixture; demoted: 2026-03-02 eol."));
        assert!(!has_demoted_eol_note("demoted: 2026-02-29 eol"));
        assert!(!has_demoted_eol_note("demoted: 2026-03-02 other"));
        assert!(!has_demoted_eol_note("fixture: Windows 10"));
    }
}
