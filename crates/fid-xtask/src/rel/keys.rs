//! `cargo xtask rel keys-check`: the published public keys agree with `KEYS.md` (spec node
//! `ECO.REL.SIGN.GUIDE`, REQ-ECO.REL.SIGN.GUIDE-001).
//!
//! `KEYS.md` holds one Markdown table whose header row is exactly
//! `| File | Class | Fingerprint | Valid from | Valid until | Status |`. Each row names a file in
//! the keys directory (backticks around a cell are ignored), its class (`release`, `nightly` or
//! `packaging`), its fingerprint (lowercase hex SHA-256 of the SubjectPublicKeyInfo DER, the
//! `TRUST.SIGN.DSSE` key fingerprint), its validity dates (`YYYY-MM-DD`, inclusive) and its
//! status (`active` or `retired`). Text outside the table is free prose.
//!
//! Every regular file in the keys directory except `README.md` and `allowed_signers` is a key
//! and must be listed; every listed key must exist, hold exactly one
//! `-----BEGIN PUBLIC KEY-----` block (explanatory text around it is allowed, RFC 7468) and
//! fingerprint to the listed value; an `active` key must not be past its `Valid until` date.
//! The OpenSSH `allowed_signers` file is not cross-checked against the PEM files.
//!
//! The check reads files only and never touches keys' private halves (none exist in the
//! repository). The result is a pure function of the files and the check date (D0 when
//! `--date` is given; otherwise the date is today's UTC date).

use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

/// Files in the keys directory that are not keys.
pub const NOT_KEYS: [&str; 2] = ["README.md", "allowed_signers"];
/// Key classes.
pub const CLASSES: [&str; 3] = ["release", "nightly", "packaging"];
/// Key statuses.
pub const STATUSES: [&str; 2] = ["active", "retired"];
/// The required table header cells.
pub const HEADER: [&str; 6] = ["File", "Class", "Fingerprint", "Valid from", "Valid until", "Status"];
/// Largest key file or `KEYS.md` read, in bytes.
pub const MAX_FILE_BYTES: u64 = 1 << 20;

/// A failure of `rel keys-check`. [`fmt::Display`] gives the line the tool prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeysError {
    /// `KEYS-UNLISTED <file>`: a file in the keys directory that `KEYS.md` does not list.
    Unlisted(String),
    /// `KEYS-FINGERPRINT <file> (listed <hex>, actual <hex>)`.
    Fingerprint { file: String, listed: String, actual: String },
    /// `KEYS-EXPIRED-IN-USE <file> (valid until <date>, checked <date>)`: an `active` key
    /// past its validity.
    ExpiredInUse { file: String, until: String, date: String },
    /// `KEYS-MISSING <file>`: listed in `KEYS.md`, absent from the keys directory.
    Missing(String),
    /// `KEYS-PEM <file>: <reason>`: a listed key that is not one PEM public key.
    Pem { file: String, reason: String },
    /// `KEYS-DOC: <reason>`: `KEYS.md` has no valid key table, or a row is malformed.
    Doc(String),
    /// `KEYS-IO <path>: <reason>`: a file or directory could not be read.
    Io(String),
}

impl KeysError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            KeysError::Unlisted(_) => "KEYS-UNLISTED",
            KeysError::Fingerprint { .. } => "KEYS-FINGERPRINT",
            KeysError::ExpiredInUse { .. } => "KEYS-EXPIRED-IN-USE",
            KeysError::Missing(_) => "KEYS-MISSING",
            KeysError::Pem { .. } => "KEYS-PEM",
            KeysError::Doc(_) => "KEYS-DOC",
            KeysError::Io(_) => "KEYS-IO",
        }
    }

    /// Process exit code: 2 when the inputs cannot be read as a key table, 1 otherwise.
    pub fn exit_code(&self) -> u8 {
        match self {
            KeysError::Doc(_) | KeysError::Io(_) => 2,
            _ => 1,
        }
    }
}

impl fmt::Display for KeysError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeysError::Unlisted(n) | KeysError::Missing(n) => write!(f, "{} {n}", self.code()),
            KeysError::Fingerprint { file, listed, actual } => {
                write!(f, "{} {file} (listed {listed}, actual {actual})", self.code())
            }
            KeysError::ExpiredInUse { file, until, date } => {
                write!(f, "{} {file} (valid until {until}, checked {date})", self.code())
            }
            KeysError::Pem { file, reason } => write!(f, "{} {file}: {reason}", self.code()),
            KeysError::Doc(m) | KeysError::Io(m) => write!(f, "{}: {m}", self.code()),
        }
    }
}

/// One row of the `KEYS.md` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRow {
    pub file: String,
    pub class: String,
    pub fingerprint: String,
    pub valid_from: String,
    pub valid_until: String,
    pub status: String,
}

fn cells(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    let inner = t.strip_prefix('|')?;
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    Some(
        inner
            .split('|')
            .map(|c| {
                let c = c.trim();
                let c = c.strip_prefix('`').and_then(|x| x.strip_suffix('`')).unwrap_or(c);
                c.to_string()
            })
            .collect(),
    )
}

fn is_separator(cells: &[String]) -> bool {
    cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
}

/// `YYYY-MM-DD` with a real calendar day.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = s.get(r)?;
        if part.bytes().all(|c| c.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
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

fn is_key_file_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('.')
        && s.ends_with(".pem")
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// Parses the key table of `KEYS.md`. Every malformed row is a `KEYS-DOC` finding.
pub fn parse_doc(text: &str) -> Result<Vec<KeyRow>, Vec<KeysError>> {
    let lines: Vec<&str> = text.lines().collect();
    let header = lines.iter().position(|l| cells(l).is_some_and(|c| c == HEADER));
    let Some(h) = header else {
        return Err(vec![KeysError::Doc(format!("no table with header | {} |", HEADER.join(" | ")))]);
    };
    let separator_ok = match lines.get(h + 1).copied().and_then(cells) {
        Some(c) => c.len() == HEADER.len() && is_separator(&c),
        None => false,
    };
    if !separator_ok {
        return Err(vec![KeysError::Doc(format!("line {}: expected the table separator row", h + 2))]);
    }
    let mut rows = Vec::new();
    let mut errs = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, line) in lines.iter().enumerate().skip(h + 2) {
        let Some(c) = cells(line) else { break };
        let n = i + 1;
        if c.len() != HEADER.len() {
            errs.push(KeysError::Doc(format!("line {n}: expected {} cells, found {}", HEADER.len(), c.len())));
            continue;
        }
        let row = KeyRow {
            file: c[0].clone(),
            class: c[1].clone(),
            fingerprint: c[2].clone(),
            valid_from: c[3].clone(),
            valid_until: c[4].clone(),
            status: c[5].clone(),
        };
        let before = errs.len();
        if !is_key_file_name(&row.file) {
            errs.push(KeysError::Doc(format!("line {n}: file {:?} is not a [A-Za-z0-9._-] name ending .pem", row.file)));
        } else if !seen.insert(row.file.clone()) {
            errs.push(KeysError::Doc(format!("line {n}: {} is listed twice", row.file)));
        }
        if !CLASSES.contains(&row.class.as_str()) {
            errs.push(KeysError::Doc(format!("line {n}: class {:?} is not one of {}", row.class, CLASSES.join(", "))));
        }
        if row.fingerprint.len() != 64 || !row.fingerprint.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            errs.push(KeysError::Doc(format!("line {n}: fingerprint is not 64 lowercase hex digits")));
        }
        if !is_date(&row.valid_from) || !is_date(&row.valid_until) {
            errs.push(KeysError::Doc(format!("line {n}: validity dates must be YYYY-MM-DD")));
        } else if row.valid_until < row.valid_from {
            errs.push(KeysError::Doc(format!("line {n}: valid until is before valid from")));
        }
        if !STATUSES.contains(&row.status.as_str()) {
            errs.push(KeysError::Doc(format!("line {n}: status {:?} is not active or retired", row.status)));
        }
        if errs.len() == before {
            rows.push(row);
        }
    }
    if errs.is_empty() {
        Ok(rows)
    } else {
        Err(errs)
    }
}

fn b64_value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Strict standard base64 with padding (RFC 4648 section 4); non-zero trailing bits are refused.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(b.len() / 4 * 3);
    let quads = b.len() / 4;
    for (qi, q) in b.chunks(4).enumerate() {
        let last = qi + 1 == quads;
        let pad = q.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut v: u32 = 0;
        for &c in &q[..4 - pad] {
            v = (v << 6) | b64_value(c)?;
        }
        v <<= 6 * pad as u32;
        let bytes = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
        let keep = 3 - pad;
        if bytes[keep..].iter().any(|&x| x != 0) {
            return None;
        }
        out.extend_from_slice(&bytes[..keep]);
    }
    Some(out)
}

/// Total length of the DER element at the start of `der` (tag, length and content), for the
/// definite lengths DER allows.
fn der_total_len(der: &[u8]) -> Option<usize> {
    let first = *der.get(1)?;
    if first < 0x80 {
        return Some(2 + usize::from(first));
    }
    let n = usize::from(first & 0x7f);
    if n == 0 || n > 4 {
        return None;
    }
    let mut len = 0usize;
    for i in 0..n {
        len = (len << 8) | usize::from(*der.get(2 + i)?);
    }
    Some(2 + n + len)
}

/// The SubjectPublicKeyInfo DER inside a PEM `PUBLIC KEY` file.
pub fn spki_der(pem: &str) -> Result<Vec<u8>, String> {
    const BEGIN: &str = "-----BEGIN PUBLIC KEY-----";
    const END: &str = "-----END PUBLIC KEY-----";
    let mut body: Option<String> = None;
    let mut inside = false;
    let mut blocks = 0;
    for line in pem.lines() {
        let l = line.trim_end_matches('\r').trim();
        if l == BEGIN {
            if inside {
                return Err("BEGIN without END".into());
            }
            inside = true;
            blocks += 1;
            body = Some(String::new());
        } else if l == END {
            if !inside {
                return Err("END without BEGIN".into());
            }
            inside = false;
        } else if inside {
            if l.starts_with("-----") {
                return Err(format!("unexpected boundary {l:?}"));
            }
            if let Some(b) = body.as_mut() {
                b.push_str(l);
            }
        } else if l.starts_with("-----BEGIN") {
            return Err(format!("not a public key block: {l:?}"));
        }
    }
    if inside {
        return Err("BEGIN without END".into());
    }
    if blocks != 1 {
        return Err(format!("expected one PUBLIC KEY block, found {blocks}"));
    }
    let der = body.as_deref().and_then(base64_decode).ok_or("body is not strict base64")?;
    if der.first() != Some(&0x30) || der_total_len(&der) != Some(der.len()) {
        return Err("body is not one DER SEQUENCE".into());
    }
    Ok(der)
}

/// Lowercase hex SHA-256 of `der`.
pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect()
}

fn read_text(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!("larger than {MAX_FILE_BYTES} bytes"));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|_| "not UTF-8".to_string())
}

/// Checks the keys directory against `KEYS.md` on `date` (`YYYY-MM-DD`). Returns the number of
/// listed keys, or every finding sorted by file then code.
pub fn check(keys_dir: &Path, doc: &Path, date: &str) -> Result<usize, Vec<KeysError>> {
    if !is_date(date) {
        return Err(vec![KeysError::Doc(format!("check date {date:?} is not YYYY-MM-DD"))]);
    }
    let text = read_text(doc).map_err(|e| vec![KeysError::Io(format!("{}: {e}", doc.display()))])?;
    let rows = parse_doc(&text)?;

    // Files in the keys directory: name -> is a regular file.
    let mut present: BTreeMap<String, bool> = BTreeMap::new();
    let entries =
        std::fs::read_dir(keys_dir).map_err(|e| vec![KeysError::Io(format!("{}: {e}", keys_dir.display()))])?;
    for entry in entries {
        let entry = entry.map_err(|e| vec![KeysError::Io(format!("{}: {e}", keys_dir.display()))])?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // symlink_metadata: a link is never followed.
        let regular = std::fs::symlink_metadata(entry.path()).map(|m| m.is_file()).unwrap_or(false);
        present.insert(name, regular);
    }

    let mut findings: Vec<(String, KeysError)> = Vec::new();
    let listed: BTreeSet<&str> = rows.iter().map(|r| r.file.as_str()).collect();
    for (name, &regular) in &present {
        if regular && NOT_KEYS.contains(&name.as_str()) {
            continue;
        }
        if !listed.contains(name.as_str()) {
            findings.push((name.clone(), KeysError::Unlisted(name.clone())));
        } else if !regular {
            findings
                .push((name.clone(), KeysError::Pem { file: name.clone(), reason: "not a regular file".into() }));
        }
    }
    for row in &rows {
        let file = &row.file;
        match present.get(file) {
            None => findings.push((file.clone(), KeysError::Missing(file.clone()))),
            Some(false) => {}
            Some(true) => match read_text(&keys_dir.join(file)).and_then(|t| spki_der(&t)) {
                Err(reason) => findings.push((file.clone(), KeysError::Pem { file: file.clone(), reason })),
                Ok(der) => {
                    let actual = fingerprint(&der);
                    if actual != row.fingerprint {
                        findings.push((
                            file.clone(),
                            KeysError::Fingerprint { file: file.clone(), listed: row.fingerprint.clone(), actual },
                        ));
                    }
                }
            },
        }
        if row.status == "active" && date > row.valid_until.as_str() {
            findings.push((
                file.clone(),
                KeysError::ExpiredInUse { file: file.clone(), until: row.valid_until.clone(), date: date.to_string() },
            ));
        }
    }
    if findings.is_empty() {
        Ok(rows.len())
    } else {
        findings.sort_by(|a, b| (a.0.as_str(), a.1.code()).cmp(&(b.0.as_str(), b.1.code())));
        Err(findings.into_iter().map(|(_, e)| e).collect())
    }
}

/// The civil (proleptic Gregorian, UTC) date `days` days after 1970-01-01, as `YYYY-MM-DD`
/// (H. Hinnant's `civil_from_days`).
pub fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Today's UTC date, or `None` if the clock is before 1970.
pub fn today_utc() -> Option<String> {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs / 86_400).ok().map(civil_date)
}

/// Process exit code for a list of findings: the largest of their exit codes.
pub fn exit_code(findings: &[KeysError]) -> u8 {
    findings.iter().map(KeysError::exit_code).max().unwrap_or(1)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn base64_strict() {
        assert_eq!(base64_decode("TWFu").unwrap(), b"Man");
        assert_eq!(base64_decode("TWE=").unwrap(), b"Ma");
        assert_eq!(base64_decode("TQ==").unwrap(), b"M");
        assert!(base64_decode("TR==").is_none(), "non-zero trailing bits");
        assert!(base64_decode("TQ=").is_none());
        assert!(base64_decode("T=Q=").is_none());
        assert!(base64_decode("TQ==TWFu").is_none());
        assert!(base64_decode("").is_none());
    }

    #[test]
    fn dates() {
        assert!(is_date("2024-02-29"));
        assert!(!is_date("2023-02-29"));
        assert!(!is_date("2026-13-01"));
        assert!(!is_date("2026-1-01"));
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(11_016), "2000-02-29");
        assert_eq!(civil_date(20_775), "2026-11-18");
    }
}
