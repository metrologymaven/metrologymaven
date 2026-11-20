//! The inputs of the register: the journal extract, `gdt-published/v1` with the trusted
//! rosters it admits, and the conflict-of-interest inputs. Every loader is strict and bounded:
//! unknown or missing fields, duplicate keys and out-of-range sizes are refused.

use super::{StatusError, PROPOSE_COMMAND};
use crate::chars::dprof::ProfileRef;
use crate::dmt_flow::roster::Roster;
use fid_bus::principal::PrincipalKind;
use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// Schema tag of the journal extract.
pub const JOURNAL_SCHEMA: &str = "gdt-rowflow-journal/v1";
/// Schema tag of the project-published digest list.
pub const PUBLISHED_SCHEMA: &str = "gdt-published/v1";
/// The reviewer register of `OUTR.GDTC.REVIEW` (TOML, read as an artifact).
pub const REVIEWER_REGISTER_SCHEMA: &str = "gdt-reviewer-register/v1";
/// The profile registry of `OUTR.GDTC.PROFILE` (TOML, read as an artifact).
pub const PROFILE_REGISTRY_SCHEMA: &str = "gdt-profiles/v1";
/// Largest journal extract, in bytes.
pub const MAX_JOURNAL_BYTES: usize = 256 * 1024 * 1024;
/// Most entries in one journal extract.
pub const MAX_JOURNAL_ENTRIES: usize = 4 * 1024 * 1024;
/// Largest `gdt-published/v1` file, in bytes.
pub const MAX_PUBLISHED_BYTES: usize = 16 * 1024 * 1024;
/// Most entries in `gdt-published/v1`.
pub const MAX_PUBLISHED_ENTRIES: usize = 100_000;
/// Largest conflict-of-interest input file, in bytes.
pub const MAX_COI_BYTES: usize = 4 * 1024 * 1024;
/// Longest principal identity in a journal entry, in bytes.
pub const MAX_PRINCIPAL_ID_LEN: usize = 256;
/// Longest reviewer or rule ID read from the conflict inputs, in bytes.
pub const MAX_COI_ID_LEN: usize = 128;
/// Most recusals one reviewer may declare.
pub const MAX_RECUSALS: usize = 1024;
/// JSON nesting a journal extract needs (extract > entries > entry > row > variants > chain >
/// steps > step > params > value), with room to spare.
const JOURNAL_DEPTH: usize = 32;
const PUBLISHED_DEPTH: usize = 3;

fn journal(reason: &'static str) -> StatusError {
    StatusError::JournalUnreadable { reason }
}

fn published(reason: &'static str) -> StatusError {
    StatusError::PublishedUnreadable { reason }
}

fn coi(reason: &'static str) -> StatusError {
    StatusError::CoiInputsUnreadable { reason }
}

/// The fields of a JSON object, refusing any key outside `allowed`.
fn fields<'a>(
    v: &'a CValue,
    allowed: &[&str],
    err: fn(&'static str) -> StatusError,
) -> Result<&'a BTreeMap<String, CValue>, StatusError> {
    match v {
        CValue::Obj(m) if m.keys().all(|k| allowed.contains(&k.as_str())) => Ok(m),
        CValue::Obj(_) => Err(err("unknown_field")),
        _ => Err(err("not_an_object")),
    }
}

fn text<'a>(
    m: &'a BTreeMap<String, CValue>,
    key: &str,
    err: fn(&'static str) -> StatusError,
) -> Result<&'a str, StatusError> {
    match m.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        Some(_) => Err(err("wrong_type")),
        None => Err(err("missing_field")),
    }
}

fn digest_field(
    m: &BTreeMap<String, CValue>,
    key: &str,
    err: fn(&'static str) -> StatusError,
) -> Result<Digest, StatusError> {
    Digest::parse(text(m, key, err)?).map_err(|_| err("bad_digest"))
}

// ------------------------------------------------------------------ UTC timestamps

/// A UTC timestamp `YYYY-MM-DDTHH:MM:SSZ`, as journal entries carry it. Ordered as text,
/// which is chronological for this fixed form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Utc(String);

impl Utc {
    /// Parses and range-checks a timestamp (month 1..12, day valid for the month and year,
    /// hour 0..23, minute and second 0..59).
    pub fn parse(s: &str) -> Option<Utc> {
        let b = s.as_bytes();
        if b.len() != 20 {
            return None;
        }
        let num = |from: usize, to: usize| -> Option<u32> {
            let part = b.get(from..to)?;
            if !part.iter().all(u8::is_ascii_digit) {
                return None;
            }
            Some(part.iter().fold(0u32, |n, d| n * 10 + u32::from(d - b'0')))
        };
        let sep = |i: usize, c: u8| b.get(i) == Some(&c);
        if !(sep(4, b'-') && sep(7, b'-') && sep(10, b'T') && sep(13, b':') && sep(16, b':') && sep(19, b'Z')) {
            return None;
        }
        let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
        let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        if year == 0 || day == 0 || day > days || hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        Some(Utc(s.to_owned()))
    }

    /// The text form.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The date part `YYYY-MM-DD`.
    pub fn date(&self) -> &str {
        self.0.get(..10).unwrap_or("")
    }
}

// ------------------------------------------------------------------ journal extract

/// One journal entry as the register reads it: the entry hash and UTC time, the command, the
/// principal of the entry header (kind and identity, as the bus wrote them) and the payload.
/// A `gdt.propose_dmt_row@1` entry also carries the proposed row.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalEntry {
    hash: Digest,
    utc: Utc,
    command: String,
    principal_kind: PrincipalKind,
    principal_id: String,
    record: CValue,
    row: Option<CValue>,
}

impl JournalEntry {
    /// The entry hash.
    pub fn hash(&self) -> Digest {
        self.hash
    }

    /// The entry time.
    pub fn utc(&self) -> &Utc {
        &self.utc
    }

    /// The command ID, for example `gdt.review_dmt_row@1`.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Principal kind of the entry header.
    pub fn principal_kind(&self) -> PrincipalKind {
        self.principal_kind
    }

    /// Principal identity of the entry header.
    pub fn principal_id(&self) -> &str {
        &self.principal_id
    }

    /// The journaled payload (for ROWFLOW commands, STATE's `FlowRecord`).
    pub fn record(&self) -> &CValue {
        &self.record
    }

    /// The proposed row (proposals only).
    pub fn row(&self) -> Option<&CValue> {
        self.row.as_ref()
    }
}

/// The journal entries the register is built from, in journal order (`gdt-rowflow-journal/v1`):
/// `{"schema": .., "entries": [{"hash", "utc", "command", "principal": {"kind", "id"},
/// "record", "row"?}]}`. Entry hashes are unique and times never go back. Verifying the hash
/// chain itself is the journal's job (`CORE.JRN`); a register built from a tampered extract
/// does not match the published digest.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalExtract {
    entries: Vec<JournalEntry>,
}

impl JournalExtract {
    /// Loads an extract (at most [`MAX_JOURNAL_BYTES`] and [`MAX_JOURNAL_ENTRIES`]).
    pub fn load(bytes: &[u8]) -> Result<JournalExtract, StatusError> {
        let v = parse_with_limits(bytes, MAX_JOURNAL_BYTES, JOURNAL_DEPTH).map_err(|e| journal(e.code()))?;
        let top = fields(&v, &["entries", "schema"], journal)?;
        if text(top, "schema", journal)? != JOURNAL_SCHEMA {
            return Err(journal("wrong_schema"));
        }
        let CValue::Arr(items) = top.get("entries").ok_or(journal("missing_field"))? else {
            return Err(journal("wrong_type"));
        };
        if items.len() > MAX_JOURNAL_ENTRIES {
            return Err(journal("too_many_entries"));
        }
        let mut seen = BTreeSet::new();
        let mut entries: Vec<JournalEntry> = Vec::with_capacity(items.len());
        for item in items {
            let e = entry(item)?;
            if !seen.insert(e.hash) {
                return Err(journal("duplicate_hash"));
            }
            if entries.last().is_some_and(|p| p.utc > e.utc) {
                return Err(journal("time_goes_back"));
            }
            entries.push(e);
        }
        Ok(JournalExtract { entries })
    }

    /// Every entry, in journal order.
    pub fn entries(&self) -> &[JournalEntry] {
        &self.entries
    }

    /// The hash of the last entry, if any.
    pub fn head(&self) -> Option<Digest> {
        self.entries.last().map(|e| e.hash)
    }

    /// The entries up to and including the one with hash `head`
    /// (`journal_unreadable{unknown_head}` if none has it).
    pub fn up_to(&self, head: Digest) -> Result<&[JournalEntry], StatusError> {
        let i = self.entries.iter().position(|e| e.hash == head).ok_or(journal("unknown_head"))?;
        self.entries.get(..=i).ok_or(journal("unknown_head"))
    }
}

fn entry(v: &CValue) -> Result<JournalEntry, StatusError> {
    let m = fields(v, &["command", "hash", "principal", "record", "row", "utc"], journal)?;
    let hash = digest_field(m, "hash", journal)?;
    let utc = Utc::parse(text(m, "utc", journal)?).ok_or(journal("bad_utc"))?;
    let command = text(m, "command", journal)?.to_owned();
    if command.is_empty() || command.len() > 128 || !command.bytes().all(|c| c.is_ascii_graphic()) {
        return Err(journal("bad_command"));
    }
    let p = fields(m.get("principal").ok_or(journal("missing_field"))?, &["id", "kind"], journal)?;
    let principal_kind = PrincipalKind::parse(text(p, "kind", journal)?).map_err(|_| journal("bad_principal"))?;
    let principal_id = text(p, "id", journal)?.to_owned();
    if principal_id.is_empty()
        || principal_id.len() > MAX_PRINCIPAL_ID_LEN
        || principal_id.chars().any(char::is_control)
    {
        return Err(journal("bad_principal"));
    }
    let record = m.get("record").ok_or(journal("missing_field"))?.clone();
    if !matches!(record, CValue::Obj(_)) {
        return Err(journal("wrong_type"));
    }
    let row = m.get("row").cloned();
    if row.is_some() != (command == PROPOSE_COMMAND) {
        return Err(journal("row_only_on_propose"));
    }
    Ok(JournalEntry { hash, utc, command, principal_kind, principal_id, record, row })
}

// ------------------------------------------------------------------ gdt-published/v1

/// One published register: its digest and the digest of the roster it was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PublishedEntry {
    /// SHA-256 of the register file.
    pub register_digest: Digest,
    /// Digest of the `dmt-reviewers/v1` roster it was built from.
    pub roster_digest: Digest,
}

/// The project-published digest list `gdt-published/v1`: an ordered, append-only list of
/// register digests, each with its roster digest, appended by the project's CI on its main
/// branch only. Its last entry is the latest register. A register digest appears at most once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    entries: Vec<PublishedEntry>,
}

impl Published {
    /// Loads `{"schema": "gdt-published/v1", "entries": [{"register_digest", "roster_digest"}]}`.
    pub fn load(bytes: &[u8]) -> Result<Published, StatusError> {
        let v = parse_with_limits(bytes, MAX_PUBLISHED_BYTES, PUBLISHED_DEPTH).map_err(|e| published(e.code()))?;
        let top = fields(&v, &["entries", "schema"], published)?;
        if text(top, "schema", published)? != PUBLISHED_SCHEMA {
            return Err(published("wrong_schema"));
        }
        let CValue::Arr(items) = top.get("entries").ok_or(published("missing_field"))? else {
            return Err(published("wrong_type"));
        };
        if items.len() > MAX_PUBLISHED_ENTRIES {
            return Err(published("too_many_entries"));
        }
        let mut seen = BTreeSet::new();
        let mut entries = Vec::with_capacity(items.len());
        for item in items {
            let m = fields(item, &["register_digest", "roster_digest"], published)?;
            let e = PublishedEntry {
                register_digest: digest_field(m, "register_digest", published)?,
                roster_digest: digest_field(m, "roster_digest", published)?,
            };
            if !seen.insert(e.register_digest) {
                return Err(published("duplicate_register"));
            }
            entries.push(e);
        }
        Ok(Published { entries })
    }

    /// The entries, oldest first.
    pub fn entries(&self) -> &[PublishedEntry] {
        &self.entries
    }

    /// The latest register, if any.
    pub fn latest(&self) -> Option<&PublishedEntry> {
        self.entries.last()
    }

    /// Whether some entry lists `roster` as its roster digest.
    pub fn lists_roster(&self, roster: Digest) -> bool {
        self.entries.iter().any(|e| e.roster_digest == roster)
    }

    /// The position and entry of a register digest.
    pub fn find(&self, register: Digest) -> Option<(usize, &PublishedEntry)> {
        self.entries.iter().enumerate().find(|(_, e)| e.register_digest == register)
    }

    /// Checks that this list is `earlier` with entries appended, nothing removed, changed or
    /// reordered (`published_unreadable{not_append_only}` otherwise; REQ-010).
    pub fn check_extends(&self, earlier: &Published) -> Result<(), StatusError> {
        if self.entries.starts_with(&earlier.entries) {
            Ok(())
        } else {
            Err(published("not_append_only"))
        }
    }

    /// The verified loader of [`TrustedRosters`]: every roster given must have its digest
    /// listed (`foreign_roster` otherwise). The current roster is the one the latest entry
    /// names, when it is among those given.
    pub fn trusted_rosters(&self, rosters: Vec<Roster>) -> Result<TrustedRosters, StatusError> {
        let mut map = BTreeMap::new();
        for r in rosters {
            if !self.lists_roster(r.digest()) {
                return Err(StatusError::ForeignRoster);
            }
            map.insert(r.digest(), r);
        }
        let current = self.latest().map(|e| e.roster_digest).filter(|d| map.contains_key(d));
        Ok(TrustedRosters { rosters: map, current })
    }
}

/// Rosters whose digests `gdt-published/v1` lists. The only constructor is
/// [`Published::trusted_rosters`], so holding one proves the check ran; the state machine's
/// `transition` and `replay` take this type instead of a bare roster.
///
/// ```compile_fail
/// let _ = fid_gdt::dmt_flow::status::TrustedRosters { rosters: Default::default(), current: None };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedRosters {
    rosters: BTreeMap<Digest, Roster>,
    current: Option<Digest>,
}

impl TrustedRosters {
    /// The roster with this digest.
    pub fn get(&self, digest: Digest) -> Option<&Roster> {
        self.rosters.get(&digest)
    }

    /// The roster the latest published register was built from: the one a new review is
    /// checked against. `None` when that roster was not given to the loader.
    pub fn current(&self) -> Option<&Roster> {
        self.current.and_then(|d| self.rosters.get(&d))
    }

    /// The digests held, ascending.
    pub fn digests(&self) -> impl Iterator<Item = &Digest> {
        self.rosters.keys()
    }
}

// ------------------------------------------------------------------ conflict-of-interest inputs

/// What the conflict rules read about one reviewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewerCoi {
    /// Pseudonymous organisation ID (`org-` and digits).
    pub org_id: String,
    /// Rule and profile IDs the reviewer is recused from.
    pub recusals: BTreeSet<String>,
}

/// The conflict-of-interest inputs (REQ-003): the reviewer register `gdt-reviewer-register/v1`
/// (organisation IDs and recusals) and the profile registry `gdt-profiles/v1` (sponsoring
/// organisations), both owned by `OUTR.GDTC` and read here as artifacts. Only the fields the
/// conflict rules need are read; names, consent flags and summaries are never read or kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoiInputs {
    reviewers: Option<BTreeMap<String, ReviewerCoi>>,
    sponsors: BTreeMap<String, String>,
}

impl CoiInputs {
    /// No conflict inputs (the spec's interim state): every approval that passes the other
    /// checks counts. The register records this in its `coi_digest`.
    pub fn none() -> CoiInputs {
        CoiInputs { reviewers: None, sponsors: BTreeMap::new() }
    }

    /// Loads the reviewer register and the profile registry (TOML; duplicate keys, duplicate
    /// IDs and IDs differing only in case refused). A reviewer needs `id`, `org_id` and
    /// `recusals`; a profile needs `id` (a full `dp:` reference) and `sponsor_org`.
    pub fn load(reviewer_register: &[u8], profile_registry: &[u8]) -> Result<CoiInputs, StatusError> {
        let reviewers_table = toml_table(reviewer_register, REVIEWER_REGISTER_SCHEMA, "reviewer")?;
        let mut reviewers = BTreeMap::new();
        for t in &reviewers_table {
            let id = coi_id(t, "id")?.to_ascii_lowercase();
            let org_id = org(t, "org_id")?;
            let recusals = match t.get("recusals") {
                Some(toml::Value::Array(a)) if a.len() <= MAX_RECUSALS => a
                    .iter()
                    .map(|x| match x {
                        toml::Value::String(s) if !s.is_empty() && s.len() <= MAX_COI_ID_LEN => Ok(s.clone()),
                        _ => Err(coi("bad_recusal")),
                    })
                    .collect::<Result<BTreeSet<_>, _>>()?,
                Some(_) => return Err(coi("bad_recusal")),
                None => return Err(coi("missing_field")),
            };
            if reviewers.insert(id, ReviewerCoi { org_id, recusals }).is_some() {
                return Err(coi("duplicate_reviewer"));
            }
        }
        let profiles_table = toml_table(profile_registry, PROFILE_REGISTRY_SCHEMA, "profile")?;
        let mut sponsors = BTreeMap::new();
        for t in &profiles_table {
            let r = ProfileRef::parse(coi_id(t, "id")?).ok_or(coi("bad_profile_id"))?;
            if sponsors.insert(r.id().to_string(), org(t, "sponsor_org")?).is_some() {
                return Err(coi("duplicate_profile"));
            }
        }
        Ok(CoiInputs { reviewers: Some(reviewers), sponsors })
    }

    /// The conflict data of a reviewer: `None` when no reviewer register is loaded,
    /// `Some(None)` when the register does not list the reviewer.
    pub fn reviewer(&self, id: &str) -> Option<Option<&ReviewerCoi>> {
        self.reviewers.as_ref().map(|m| m.get(&id.to_ascii_lowercase()))
    }

    /// The sponsoring organisation ID of a profile (by profile ID without digest).
    pub fn sponsor_of(&self, profile_id: &str) -> Option<&str> {
        self.sponsors.get(profile_id).map(String::as_str)
    }

    /// The inputs as canonical JSON: `{"profiles": {<id>: {"sponsor_org"}}, "reviewers": null |
    /// {<id>: {"org_id", "recusals": [..]}}}`, reviewer IDs lowercased, lists sorted.
    pub fn to_cvalue(&self) -> CValue {
        let profiles = self
            .sponsors
            .iter()
            .map(|(k, v)| (k.clone(), CValue::Obj(BTreeMap::from([("sponsor_org".to_owned(), s(v))]))))
            .collect();
        let reviewers = match &self.reviewers {
            None => CValue::Null,
            Some(m) => CValue::Obj(
                m.iter()
                    .map(|(k, r)| {
                        let o = BTreeMap::from([
                            ("org_id".to_owned(), s(&r.org_id)),
                            ("recusals".to_owned(), CValue::Arr(r.recusals.iter().map(|x| s(x)).collect())),
                        ]);
                        (k.clone(), CValue::Obj(o))
                    })
                    .collect(),
            ),
        };
        CValue::Obj(BTreeMap::from([
            ("profiles".to_owned(), CValue::Obj(profiles)),
            ("reviewers".to_owned(), reviewers),
        ]))
    }

    /// SHA-256 of the canonical JSON of [`CoiInputs::to_cvalue`].
    pub fn digest(&self) -> Result<Digest, StatusError> {
        let bytes = to_canonical_bytes(&self.to_cvalue()).map_err(|e| coi(e.code()))?;
        Ok(Digest::from_bytes(digest_bytes(&bytes).0))
    }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

/// The array of tables `[[<list>]]` of a TOML document whose only other top-level key may be
/// `schema` (which must then equal `schema`).
fn toml_table(bytes: &[u8], schema: &str, list: &str) -> Result<Vec<toml::Table>, StatusError> {
    if bytes.len() > MAX_COI_BYTES {
        return Err(coi("too_large"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| coi("invalid_utf8"))?;
    let table = text.parse::<toml::Table>().map_err(|_| coi("bad_toml"))?;
    if table.keys().any(|k| k != list && k != "schema") {
        return Err(coi("unknown_field"));
    }
    match table.get("schema") {
        None => {}
        Some(toml::Value::String(v)) if v == schema => {}
        Some(_) => return Err(coi("wrong_schema")),
    }
    match table.get(list) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(a)) => a
            .iter()
            .map(|x| match x {
                toml::Value::Table(t) => Ok(t.clone()),
                _ => Err(coi("wrong_type")),
            })
            .collect(),
        Some(_) => Err(coi("wrong_type")),
    }
}

fn coi_id<'a>(t: &'a toml::Table, key: &str) -> Result<&'a str, StatusError> {
    match t.get(key) {
        Some(toml::Value::String(v)) if !v.is_empty() && v.len() <= MAX_COI_ID_LEN && v.is_ascii() => Ok(v.as_str()),
        Some(_) => Err(coi("bad_id")),
        None => Err(coi("missing_field")),
    }
}

/// An organisation ID: `org-` and 1 to 16 decimal digits.
fn org(t: &toml::Table, key: &str) -> Result<String, StatusError> {
    let v = coi_id(t, key)?;
    let digits = v.strip_prefix("org-").ok_or(coi("bad_org_id"))?;
    if digits.is_empty() || digits.len() > 16 || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return Err(coi("bad_org_id"));
    }
    Ok(v.to_owned())
}
