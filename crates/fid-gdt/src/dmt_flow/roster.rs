//! DMT reviewer roster file `dmt-reviewers/v1` and role lookup (spec node `GDT.RES.ROWFLOW.ROSTER`).
//!
//! The roster records who holds the DMT-reviewer role (D-009, D-016): a reviewer ID, a display
//! name, the principal identities bound to it, the dialects it may review, the basis of the role
//! and a validity window. It deliberately has no field for credential documents, contact details
//! or licence keys (REQ-005); any such key is an unknown field and is refused.
//!
//! File format (JSON, UTF-8):
//!
//! ```json
//! {
//!   "version": "dmt-reviewers/v1",
//!   "entries": [
//!     {
//!       "id": "rev-alpha",
//!       "display_name": "Alpha Example (fictional)",
//!       "principals": ["human:fictional-alpha"],
//!       "dialects": ["asme"],
//!       "basis": "paid_author",
//!       "valid_from": "2026-01-01",
//!       "valid_until": "2027-01-01"
//!     }
//!   ]
//! }
//! ```
//!
//! `valid_until` is optional and exclusive. Loading is strict and bounded: unknown keys, missing
//! keys, wrong types, bad dates and out-of-range sizes fail with `roster_invalid{field}`; a
//! repeated reviewer ID or a principal bound twice fails with `roster_duplicate{id}`; a display
//! name over 80 characters (or an over-long ID or principal) fails with `field_too_long{field}`.
//!
//! The roster digest is the SHA-256 of the RFC 8785 canonical JSON of the *normalised* roster:
//! `{"entries": {<id>: {...}}, "version": ...}` with principals and dialects sorted, so neither
//! the order of entries in the file nor the order inside an entry's lists changes it (AC-3).

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_types::Digest;
use std::borrow::Borrow;
use std::collections::BTreeMap;
use std::fmt;

/// The only accepted value of the `version` field.
pub const FORMAT: &str = "dmt-reviewers/v1";
/// Largest accepted roster file, in bytes.
pub const MAX_ROSTER_BYTES: usize = 1024 * 1024;
/// Most entries a roster may hold.
pub const MAX_ENTRIES: usize = 4096;
/// Most principal identities one entry may bind.
pub const MAX_PRINCIPALS_PER_ENTRY: usize = 16;
/// Longest display name, in Unicode scalar values (REQ-005).
pub const MAX_DISPLAY_NAME_CHARS: usize = 80;
/// Longest reviewer ID, in characters.
pub const MAX_ID_CHARS: usize = 64;
/// Longest principal identity, in characters.
pub const MAX_PRINCIPAL_CHARS: usize = 128;

/// JSON nesting the format needs: object > array > object > array.
const MAX_JSON_DEPTH: usize = 4;
/// Longest unknown key echoed back in an error.
const MAX_ERROR_FIELD_CHARS: usize = 64;
/// Field name used for errors about the document as a whole (syntax, size, top-level type).
pub const DOCUMENT_FIELD: &str = "$";

const TOP_KEYS: [&str; 2] = ["entries", "version"];
const ENTRY_KEYS: [&str; 7] = ["basis", "dialects", "display_name", "id", "principals", "valid_from", "valid_until"];

// ------------------------------------------------------------------ errors

/// Loader errors with stable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosterError {
    /// `roster_invalid{field}`: wrong version, unknown, missing or malformed field, size limit.
    Invalid { field: String },
    /// `roster_duplicate{id}`: a reviewer ID repeated, or a principal identity bound twice.
    Duplicate { id: String },
    /// `field_too_long{field}`: a free-text or identifier field over its limit.
    FieldTooLong { field: String },
}

impl RosterError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            RosterError::Invalid { .. } => "roster_invalid",
            RosterError::Duplicate { .. } => "roster_duplicate",
            RosterError::FieldTooLong { .. } => "field_too_long",
        }
    }

    /// The field or identity the error names.
    pub fn subject(&self) -> &str {
        match self {
            RosterError::Invalid { field } | RosterError::FieldTooLong { field } => field,
            RosterError::Duplicate { id } => id,
        }
    }
}

impl fmt::Display for RosterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{{{}}}", self.code(), self.subject())
    }
}

impl std::error::Error for RosterError {}

fn invalid(field: &str) -> RosterError {
    RosterError::Invalid { field: bounded(field) }
}

fn too_long(field: &str) -> RosterError {
    RosterError::FieldTooLong { field: field.to_string() }
}

/// Untrusted text echoed in an error is cut to a bounded length.
fn bounded(s: &str) -> String {
    s.chars().take(MAX_ERROR_FIELD_CHARS).collect()
}

// ------------------------------------------------------------------ value types

/// A reviewer ID: 1 to 64 characters of `[a-z0-9._-]`, starting with a letter or digit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReviewerId(String);

impl ReviewerId {
    /// Parses and validates a reviewer ID.
    pub fn parse(s: &str) -> Result<ReviewerId, RosterError> {
        if s.chars().count() > MAX_ID_CHARS {
            return Err(too_long("id"));
        }
        let b = s.as_bytes();
        let first_ok = matches!(b.first(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit());
        let rest_ok = b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'));
        if !first_ok || !rest_ok {
            return Err(invalid("id"));
        }
        Ok(ReviewerId(s.to_string()))
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ReviewerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Borrow<str> for ReviewerId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// A principal identity as authenticated by the bus (`CORE.BUS`); opaque here.
/// 1 to 128 printable ASCII characters, no spaces.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrincipalIdentity(String);

impl PrincipalIdentity {
    /// Parses and validates a principal identity.
    pub fn parse(s: &str) -> Result<PrincipalIdentity, RosterError> {
        if s.chars().count() > MAX_PRINCIPAL_CHARS {
            return Err(too_long("principals"));
        }
        if s.is_empty() || !s.bytes().all(|c| c.is_ascii_graphic()) {
            return Err(invalid("principals"));
        }
        Ok(PrincipalIdentity(s.to_string()))
    }

    /// The identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for PrincipalIdentity {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// GD&T dialect a reviewer may review rows of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dialect {
    Asme,
    Iso,
}

impl Dialect {
    /// Wire form: `asme` or `iso`.
    pub fn as_str(self) -> &'static str {
        match self {
            Dialect::Asme => "asme",
            Dialect::Iso => "iso",
        }
    }

    /// Parses the wire form.
    pub fn parse(s: &str) -> Option<Dialect> {
        match s {
            "asme" => Some(Dialect::Asme),
            "iso" => Some(Dialect::Iso),
            _ => None,
        }
    }
}

/// Basis on which a reviewer holds the role (D-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Basis {
    PaidAuthor,
    Partner,
    Community,
}

impl Basis {
    /// Wire form: `paid_author`, `partner` or `community`.
    pub fn as_str(self) -> &'static str {
        match self {
            Basis::PaidAuthor => "paid_author",
            Basis::Partner => "partner",
            Basis::Community => "community",
        }
    }

    /// Parses the wire form.
    pub fn parse(s: &str) -> Option<Basis> {
        match s {
            "paid_author" => Some(Basis::PaidAuthor),
            "partner" => Some(Basis::Partner),
            "community" => Some(Basis::Community),
            _ => None,
        }
    }
}

/// A calendar date (proleptic Gregorian), written `YYYY-MM-DD`, year 0001 to 9999.
/// Ordering is chronological.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

fn is_leap(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    /// A validated date, or `None` if it does not exist.
    pub fn new(year: u16, month: u8, day: u8) -> Option<Date> {
        let ok = (1..=9999).contains(&year) && (1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day);
        if ok {
            Some(Date { year, month, day })
        } else {
            None
        }
    }

    /// Parses exactly `YYYY-MM-DD`.
    pub fn parse(s: &str) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let num = |range: std::ops::Range<usize>| -> Option<u16> {
            let mut v: u16 = 0;
            for &c in b.get(range)? {
                if !c.is_ascii_digit() {
                    return None;
                }
                v = v * 10 + u16::from(c - b'0');
            }
            Some(v)
        };
        let year = num(0..4)?;
        let month = u8::try_from(num(5..7)?).ok()?;
        let day = u8::try_from(num(8..10)?).ok()?;
        Date::new(year, month, day)
    }

    pub fn year(self) -> u16 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

// ------------------------------------------------------------------ roster

/// One reviewer entry. Lists are sorted and free of duplicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub display_name: String,
    pub principals: Vec<PrincipalIdentity>,
    pub dialects: Vec<Dialect>,
    pub basis: Basis,
    pub valid_from: Date,
    /// Exclusive end of the validity window; `None` means open-ended.
    pub valid_until: Option<Date>,
}

impl Entry {
    /// Whether the entry lists `dialect` and `valid_from <= at < valid_until`.
    pub fn grants(&self, dialect: Dialect, at: Date) -> bool {
        self.dialects.contains(&dialect) && self.valid_from <= at && self.valid_until.is_none_or(|u| at < u)
    }
}

/// A loaded, validated `dmt-reviewers/v1` roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roster {
    entries: BTreeMap<ReviewerId, Entry>,
    principals: BTreeMap<PrincipalIdentity, ReviewerId>,
    canonical: Vec<u8>,
    digest: Digest,
}

/// What the review state machine (`GDT.RES.ROWFLOW.STATE`) needs from a roster.
pub trait RosterView {
    /// The reviewer ID `principal` holds for `dialect` on date `at`, if any (REQ-003).
    fn has_role(&self, principal: &str, dialect: Dialect, at: Date) -> Option<ReviewerId>;
    /// Digest of the roster, to be recorded on every review record (REQ-004).
    fn roster_digest(&self) -> Digest;
}

impl Roster {
    /// Loads a roster file. Strict and bounded; see the module documentation.
    pub fn load(bytes: &[u8]) -> Result<Roster, RosterError> {
        let doc = parse_with_limits(bytes, MAX_ROSTER_BYTES, MAX_JSON_DEPTH).map_err(|e| match e {
            CanonError::DuplicateKey(k) => invalid(&k),
            _ => invalid(DOCUMENT_FIELD),
        })?;
        let top = as_obj(&doc, DOCUMENT_FIELD)?;
        check_keys(top, &TOP_KEYS)?;
        if req_str(top, "version")? != FORMAT {
            return Err(invalid("version"));
        }
        let list = match top.get("entries") {
            Some(CValue::Arr(a)) if a.len() <= MAX_ENTRIES => a,
            _ => return Err(invalid("entries")),
        };
        let mut entries: BTreeMap<ReviewerId, Entry> = BTreeMap::new();
        let mut principals: BTreeMap<PrincipalIdentity, ReviewerId> = BTreeMap::new();
        for item in list {
            let (id, entry) = parse_entry(item)?;
            if entries.contains_key(&id) {
                return Err(RosterError::Duplicate { id: id.0 });
            }
            for p in &entry.principals {
                if principals.contains_key(p) {
                    return Err(RosterError::Duplicate { id: p.0.clone() });
                }
                principals.insert(p.clone(), id.clone());
            }
            entries.insert(id, entry);
        }
        let canonical = to_canonical_bytes(&normalised(&entries)).map_err(|_| invalid(DOCUMENT_FIELD))?;
        let digest = Digest::from_bytes(digest_bytes(&canonical).0);
        Ok(Roster { entries, principals, canonical, digest })
    }

    /// The format version, always [`FORMAT`].
    pub fn version(&self) -> &'static str {
        FORMAT
    }

    /// Entries by reviewer ID.
    pub fn entries(&self) -> &BTreeMap<ReviewerId, Entry> {
        &self.entries
    }

    /// The entry for a reviewer ID.
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.get(id)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the roster has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The reviewer ID bound to `principal`, regardless of dialect or date.
    pub fn reviewer_of(&self, principal: &str) -> Option<&ReviewerId> {
        self.principals.get(principal)
    }

    /// The reviewer ID `principal` holds for `dialect` on `at`: exactly when an entry binds the
    /// principal, lists the dialect and `valid_from <= at` and (`valid_until` absent or
    /// `at < valid_until`) (REQ-003).
    pub fn has_role(&self, principal: &str, dialect: Dialect, at: Date) -> Option<ReviewerId> {
        let id = self.principals.get(principal)?;
        let entry = self.entries.get(id)?;
        if entry.grants(dialect, at) {
            Some(id.clone())
        } else {
            None
        }
    }

    /// RFC 8785 canonical JSON of the normalised roster (the bytes the digest covers).
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    /// `sha256:` digest of [`Roster::canonical_bytes`] (REQ-004).
    pub fn digest(&self) -> Digest {
        self.digest
    }
}

impl RosterView for Roster {
    fn has_role(&self, principal: &str, dialect: Dialect, at: Date) -> Option<ReviewerId> {
        Roster::has_role(self, principal, dialect, at)
    }

    fn roster_digest(&self) -> Digest {
        self.digest
    }
}

// ------------------------------------------------------------------ parsing helpers

fn as_obj<'a>(v: &'a CValue, field: &str) -> Result<&'a BTreeMap<String, CValue>, RosterError> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(invalid(field)),
    }
}

/// Refuses the first unknown key (keys are visited in sorted order, so the report is stable).
fn check_keys(m: &BTreeMap<String, CValue>, known: &[&str]) -> Result<(), RosterError> {
    match m.keys().find(|k| !known.contains(&k.as_str())) {
        Some(k) => Err(invalid(k)),
        None => Ok(()),
    }
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, RosterError> {
    match m.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(invalid(key)),
    }
}

fn req_str_list<'a>(m: &'a BTreeMap<String, CValue>, key: &str, max: usize) -> Result<Vec<&'a str>, RosterError> {
    let items = match m.get(key) {
        Some(CValue::Arr(a)) if !a.is_empty() && a.len() <= max => a,
        _ => return Err(invalid(key)),
    };
    items
        .iter()
        .map(|v| match v {
            CValue::Str(s) => Ok(s.as_str()),
            _ => Err(invalid(key)),
        })
        .collect()
}

fn parse_display_name(s: &str) -> Result<String, RosterError> {
    if s.chars().count() > MAX_DISPLAY_NAME_CHARS {
        return Err(too_long("display_name"));
    }
    if s.trim().is_empty() || s.chars().any(char::is_control) {
        return Err(invalid("display_name"));
    }
    Ok(s.to_string())
}

fn parse_entry(v: &CValue) -> Result<(ReviewerId, Entry), RosterError> {
    let m = as_obj(v, "entries")?;
    check_keys(m, &ENTRY_KEYS)?;
    let id = ReviewerId::parse(req_str(m, "id")?)?;
    let display_name = parse_display_name(req_str(m, "display_name")?)?;

    let mut principals = req_str_list(m, "principals", MAX_PRINCIPALS_PER_ENTRY)?
        .into_iter()
        .map(PrincipalIdentity::parse)
        .collect::<Result<Vec<_>, _>>()?;
    principals.sort();
    if let Some(w) = principals.windows(2).find(|w| w[0] == w[1]) {
        return Err(RosterError::Duplicate { id: w[0].0.clone() });
    }

    // At most one entry per known dialect, so the list bound is the dialect count.
    let mut dialects = req_str_list(m, "dialects", 2)?
        .into_iter()
        .map(|s| Dialect::parse(s).ok_or_else(|| invalid("dialects")))
        .collect::<Result<Vec<_>, _>>()?;
    dialects.sort();
    if dialects.windows(2).any(|w| w[0] == w[1]) {
        return Err(invalid("dialects"));
    }

    let basis = Basis::parse(req_str(m, "basis")?).ok_or_else(|| invalid("basis"))?;
    let valid_from = Date::parse(req_str(m, "valid_from")?).ok_or_else(|| invalid("valid_from"))?;
    let valid_until = match m.get("valid_until") {
        None => None,
        Some(CValue::Str(s)) => Some(Date::parse(s).ok_or_else(|| invalid("valid_until"))?),
        Some(_) => return Err(invalid("valid_until")),
    };
    if valid_until.is_some_and(|u| u <= valid_from) {
        return Err(invalid("valid_until"));
    }
    Ok((id, Entry { display_name, principals, dialects, basis, valid_from, valid_until }))
}

/// The normalised roster: entries keyed by ID, sorted lists, `valid_until` omitted when absent.
fn normalised(entries: &BTreeMap<ReviewerId, Entry>) -> CValue {
    let mut by_id = BTreeMap::new();
    for (id, e) in entries {
        let mut o = BTreeMap::new();
        o.insert("basis".to_string(), CValue::Str(e.basis.as_str().to_string()));
        o.insert(
            "dialects".to_string(),
            CValue::Arr(e.dialects.iter().map(|d| CValue::Str(d.as_str().to_string())).collect()),
        );
        o.insert("display_name".to_string(), CValue::Str(e.display_name.clone()));
        o.insert("principals".to_string(), CValue::Arr(e.principals.iter().map(|p| CValue::Str(p.0.clone())).collect()));
        o.insert("valid_from".to_string(), CValue::Str(e.valid_from.to_string()));
        if let Some(u) = e.valid_until {
            o.insert("valid_until".to_string(), CValue::Str(u.to_string()));
        }
        by_id.insert(id.0.clone(), CValue::Obj(o));
    }
    let mut top = BTreeMap::new();
    top.insert("entries".to_string(), CValue::Obj(by_id));
    top.insert("version".to_string(), CValue::Str(FORMAT.to_string()));
    CValue::Obj(top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_parse_strictly() {
        assert_eq!(Date::parse("2024-02-29").map(|d| d.to_string()), Some("2024-02-29".to_string()));
        for bad in ["2026-02-29", "1900-02-29", "0000-01-01", "2026-13-01", "2026-00-10", "2026-04-31", "2026-1-01", "2026/01/01", "+026-01-01", "2026-01-01 "] {
            assert_eq!(Date::parse(bad), None, "{bad}");
        }
        assert!(Date::parse("2000-02-29").is_some());
        assert!(Date::parse("2026-11-20") < Date::parse("2026-12-01"));
    }

    #[test]
    fn reviewer_ids() {
        assert!(ReviewerId::parse("rev-alpha.1_x").is_ok());
        assert_eq!(ReviewerId::parse("").map_err(|e| e.code()), Err("roster_invalid"));
        assert_eq!(ReviewerId::parse("-x").map_err(|e| e.code()), Err("roster_invalid"));
        assert_eq!(ReviewerId::parse("Rev").map_err(|e| e.code()), Err("roster_invalid"));
        assert_eq!(ReviewerId::parse(&"a".repeat(65)).map_err(|e| e.code()), Err("field_too_long"));
        assert!(ReviewerId::parse(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn error_display() {
        assert_eq!(invalid("email").to_string(), "roster_invalid{email}");
        assert_eq!(RosterError::Duplicate { id: "rev-a".into() }.to_string(), "roster_duplicate{rev-a}");
        assert_eq!(too_long("display_name").to_string(), "field_too_long{display_name}");
        assert_eq!(invalid(&"k".repeat(500)).subject().len(), MAX_ERROR_FIELD_CHARS);
    }
}
