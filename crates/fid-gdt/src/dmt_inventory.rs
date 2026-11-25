//! DMT inventory format (`dmt-inventory/v1`), strict bounded loader, and
//! deterministic coverage check (spec node `GDT.RES.ROWASME.INV`).
//!
//! An inventory file lists, per GD&T dialect, edition set, and delivery phase,
//! the [`DmtKey`]s a release must contain, the family node that owns each key,
//! and the minimum [`RequiredStatus`] (`supported_or_partial` or
//! `unsupported_ok`). Inventory files hold keys only — never clause numbers,
//! standards text, or interpretation.
//!
//! ```json
//! {
//!   "schema": "dmt-inventory/v1",
//!   "dialect": "asme",
//!   "editions": ["y14.5-2018", "y14.5.1-2019"],
//!   "phase": "P1a",
//!   "entries": [
//!     { "key": "dmt:asme/flatness/none@1", "family": "FORM", "required_status": "supported_or_partial" }
//!   ]
//! }
//! ```
//!
//! [`coverage`] walks the inventory in deterministic `DmtKey` byte order (D0)
//! and reports every missing key (`missing_row`) and every key whose row status
//! fails `required_status` (`status_below_required`).

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_gps::dmt::ident::is_label;
use fid_gps::dmt::{Dialect, DmtKey, DmtTable, RowStatus};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Schema identifier for every DMT inventory document.
pub const INVENTORY_SCHEMA: &str = "dmt-inventory/v1";
/// Largest accepted inventory file, in bytes (4 MiB).
pub const MAX_INVENTORY_BYTES: usize = 4 * 1024 * 1024;
/// Deepest accepted JSON nesting in an inventory file.
pub const MAX_INVENTORY_DEPTH: usize = 6;
/// Most entries in one inventory file.
pub const MAX_ENTRIES: usize = 10_000;
/// Most edition labels in one inventory file.
pub const MAX_EDITIONS: usize = 32;
/// Longest edition label, in bytes.
pub const MAX_EDITION_LEN: usize = 64;
/// Longest phase tag, in bytes.
pub const MAX_PHASE_LEN: usize = 32;
/// Longest text echoed inside an error.
const MAX_ECHO: usize = 160;

const ROOT_FIELDS: [&str; 5] = ["dialect", "editions", "entries", "phase", "schema"];
const ENTRY_FIELDS: [&str; 3] = ["family", "key", "required_status"];

/// Family node owning a DMT inventory key.
///
/// ASME and ISO inventories share `SIZE` and `DPROF`; `GEOTOL`, `DATUM`, and
/// `MATREQ` are reserved for ISO inventories (`GDT.RES.ROWISO`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InventoryFamily {
    /// Dimensional and size rows (`GDT.RES.ROWASME.SIZE` / `GDT.RES.ROWISO.SIZE`).
    Size,
    /// Form rows (`GDT.RES.ROWASME.FORM`).
    Form,
    /// Orientation rows (`GDT.RES.ROWASME.ORIENT`).
    Orient,
    /// Position rows (`GDT.RES.ROWASME.POS`).
    Pos,
    /// Datum and DRF rows (`GDT.RES.ROWASME.DRF`).
    Drf,
    /// Defaults profile rows (`GDT.RES.ROWASME.DPROF` / `GDT.RES.ROWISO.DPROF`).
    Dprof,
    /// ISO geometric tolerance rows (`GDT.RES.ROWISO.GEOTOL`).
    Geotol,
    /// ISO datum rows (`GDT.RES.ROWISO.DATUM`).
    Datum,
    /// ISO material requirement rows (`GDT.RES.ROWISO.MATREQ`).
    Matreq,
}

impl InventoryFamily {
    /// All nine valid family names across ASME and ISO inventories.
    pub const ALL: [InventoryFamily; 9] = [
        InventoryFamily::Size,
        InventoryFamily::Form,
        InventoryFamily::Orient,
        InventoryFamily::Pos,
        InventoryFamily::Drf,
        InventoryFamily::Dprof,
        InventoryFamily::Geotol,
        InventoryFamily::Datum,
        InventoryFamily::Matreq,
    ];

    /// The five ASME P1a row family children of `GDT.RES.ROWASME`.
    pub const ASME_P1A: [InventoryFamily; 5] = [
        InventoryFamily::Size,
        InventoryFamily::Form,
        InventoryFamily::Orient,
        InventoryFamily::Pos,
        InventoryFamily::Drf,
    ];

    /// Canonical wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            InventoryFamily::Size => "SIZE",
            InventoryFamily::Form => "FORM",
            InventoryFamily::Orient => "ORIENT",
            InventoryFamily::Pos => "POS",
            InventoryFamily::Drf => "DRF",
            InventoryFamily::Dprof => "DPROF",
            InventoryFamily::Geotol => "GEOTOL",
            InventoryFamily::Datum => "DATUM",
            InventoryFamily::Matreq => "MATREQ",
        }
    }

    /// Parses the exact wire spelling (case-sensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "SIZE" => Some(InventoryFamily::Size),
            "FORM" => Some(InventoryFamily::Form),
            "ORIENT" => Some(InventoryFamily::Orient),
            "POS" => Some(InventoryFamily::Pos),
            "DRF" => Some(InventoryFamily::Drf),
            "DPROF" => Some(InventoryFamily::Dprof),
            "GEOTOL" => Some(InventoryFamily::Geotol),
            "DATUM" => Some(InventoryFamily::Datum),
            "MATREQ" => Some(InventoryFamily::Matreq),
            _ => None,
        }
    }
}

impl fmt::Display for InventoryFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Minimum row support status required for an inventory key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RequiredStatus {
    /// Row must be `supported` or `partial` (`unsupported` fails coverage).
    SupportedOrPartial,
    /// Any row status (`supported`, `partial`, or `unsupported`) satisfies coverage.
    UnsupportedOk,
}

impl RequiredStatus {
    /// Canonical wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            RequiredStatus::SupportedOrPartial => "supported_or_partial",
            RequiredStatus::UnsupportedOk => "unsupported_ok",
        }
    }

    /// Parses the exact wire spelling.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "supported_or_partial" => Some(RequiredStatus::SupportedOrPartial),
            "unsupported_ok" => Some(RequiredStatus::UnsupportedOk),
            _ => None,
        }
    }

    /// Returns `true` when `status` satisfies this requirement.
    pub const fn is_met_by(self, status: RowStatus) -> bool {
        match self {
            RequiredStatus::SupportedOrPartial => matches!(status, RowStatus::Supported | RowStatus::Partial),
            RequiredStatus::UnsupportedOk => true,
        }
    }
}

impl fmt::Display for RequiredStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Metadata attached to one key in a DMT [`Inventory`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryEntry {
    pub family: InventoryFamily,
    pub required_status: RequiredStatus,
}

/// Errors returned when loading or constructing a DMT [`Inventory`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    /// A key did not match the DMT key grammar (REQ-GDT.RES.ROWASME.INV-001).
    BadDmtKey { key: String },
    /// Any structural, semantic, duplicate-key, unknown-family, or limit violation
    /// (REQ-GDT.RES.ROWASME.INV-001, REQ-GDT.RES.ROWASME.INV-005).
    InventoryInvalid { field: &'static str, detail: String },
}

impl InventoryError {
    /// Stable machine-readable error code (`bad_dmt_key` or `inventory_invalid`).
    pub const fn code(&self) -> &'static str {
        match self {
            InventoryError::BadDmtKey { .. } => "bad_dmt_key",
            InventoryError::InventoryInvalid { .. } => "inventory_invalid",
        }
    }

    /// The offending field for `inventory_invalid`, if any.
    pub const fn field(&self) -> Option<&'static str> {
        match self {
            InventoryError::BadDmtKey { .. } => None,
            InventoryError::InventoryInvalid { field, .. } => Some(field),
        }
    }
}

impl fmt::Display for InventoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InventoryError::BadDmtKey { key } => write!(f, "bad_dmt_key{{{}}}", key.escape_debug()),
            InventoryError::InventoryInvalid { field, detail } => {
                write!(f, "inventory_invalid{{{field}}}: {detail}")
            }
        }
    }
}

impl std::error::Error for InventoryError {}

/// A loaded, validated `dmt-inventory/v1` document in canonical key order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    dialect: Dialect,
    editions: Vec<String>,
    phase: String,
    entries: BTreeMap<DmtKey, InventoryEntry>,
    canonical: Vec<u8>,
    digest: Digest,
}

impl Inventory {
    /// Loads and validates a `dmt-inventory/v1` JSON document from bytes.
    pub fn load(bytes: &[u8]) -> Result<Self, InventoryError> {
        if bytes.len() > MAX_INVENTORY_BYTES {
            return Err(inv_invalid("inventory", "exceeds MAX_INVENTORY_BYTES"));
        }
        let val = parse_with_limits(bytes, MAX_INVENTORY_BYTES, MAX_INVENTORY_DEPTH)
            .map_err(|e| inv_invalid("json", e.code()))?;
        Self::from_value(&val)
    }

    /// Validates and builds an [`Inventory`] from a parsed [`CValue`].
    pub fn from_value(v: &CValue) -> Result<Self, InventoryError> {
        let obj = as_strict_obj(v, &ROOT_FIELDS, "inventory")?;

        let schema = req_str(obj, "schema")?;
        if schema != INVENTORY_SCHEMA {
            return Err(inv_invalid("schema", format!("expected '{INVENTORY_SCHEMA}', got '{}'", echo(schema))));
        }

        let dialect_s = req_str(obj, "dialect")?;
        let dialect = Dialect::parse(dialect_s)
            .ok_or_else(|| inv_invalid("dialect", format!("unknown dialect '{}'", echo(dialect_s))))?;

        let editions_arr = req_arr(obj, "editions")?;
        if editions_arr.is_empty() || editions_arr.len() > MAX_EDITIONS {
            return Err(inv_invalid("editions", "must contain 1..=32 edition labels"));
        }
        let mut editions_set = BTreeSet::new();
        for item in editions_arr {
            let CValue::Str(s) = item else {
                return Err(inv_invalid("editions", "edition label must be a string"));
            };
            if !is_label(s, MAX_EDITION_LEN) {
                return Err(inv_invalid("editions", format!("invalid edition label '{}'", echo(s))));
            }
            if !editions_set.insert(s.clone()) {
                return Err(inv_invalid("editions", format!("duplicate edition label '{}'", echo(s))));
            }
        }
        let editions: Vec<String> = editions_set.into_iter().collect();

        let phase = req_str(obj, "phase")?;
        if !is_phase_tag(phase) {
            return Err(inv_invalid("phase", format!("invalid phase tag '{}'", echo(phase))));
        }

        let entries_arr = req_arr(obj, "entries")?;
        if entries_arr.len() > MAX_ENTRIES {
            return Err(inv_invalid("entries", "exceeds MAX_ENTRIES"));
        }
        let mut entries = BTreeMap::new();
        for item in entries_arr {
            let eobj = as_strict_obj(item, &ENTRY_FIELDS, "entries")?;
            let key_s = req_str(eobj, "key")?;
            let key = DmtKey::parse(key_s).map_err(|_| InventoryError::BadDmtKey { key: echo(key_s) })?;
            if key.dialect() != dialect {
                return Err(inv_invalid(
                    "entries.key",
                    format!(
                        "key dialect '{}' does not match inventory dialect '{}'",
                        key.dialect().as_str(),
                        dialect.as_str()
                    ),
                ));
            }
            let fam_s = req_str(eobj, "family")?;
            let family = InventoryFamily::parse(fam_s)
                .ok_or_else(|| inv_invalid("entries.family", format!("unknown family '{}'", echo(fam_s))))?;
            let req_s = req_str(eobj, "required_status")?;
            let required_status = RequiredStatus::parse(req_s).ok_or_else(|| {
                inv_invalid("entries.required_status", format!("unknown required_status '{}'", echo(req_s)))
            })?;
            if entries.contains_key(&key) {
                return Err(inv_invalid("entries.key", format!("duplicate inventory key '{}'", key.as_str())));
            }
            entries.insert(key, InventoryEntry { family, required_status });
        }

        Self::build(dialect, editions, phase.to_owned(), entries)
    }

    /// Builds an [`Inventory`] from validated parts and computes its canonical JSON and digest.
    pub fn new(
        dialect: Dialect,
        editions: Vec<String>,
        phase: String,
        entries: BTreeMap<DmtKey, InventoryEntry>,
    ) -> Result<Self, InventoryError> {
        if editions.is_empty() || editions.len() > MAX_EDITIONS {
            return Err(inv_invalid("editions", "must contain 1..=32 edition labels"));
        }
        let mut ed_set = BTreeSet::new();
        for e in &editions {
            if !is_label(e, MAX_EDITION_LEN) || !ed_set.insert(e.clone()) {
                return Err(inv_invalid("editions", "invalid or duplicate edition label"));
            }
        }
        if !is_phase_tag(&phase) {
            return Err(inv_invalid("phase", "invalid phase tag"));
        }
        if entries.len() > MAX_ENTRIES {
            return Err(inv_invalid("entries", "exceeds MAX_ENTRIES"));
        }
        for k in entries.keys() {
            if k.dialect() != dialect {
                return Err(inv_invalid("entries.key", "key dialect does not match inventory dialect"));
            }
        }
        Self::build(dialect, ed_set.into_iter().collect(), phase, entries)
    }

    fn build(
        dialect: Dialect,
        editions: Vec<String>,
        phase: String,
        entries: BTreeMap<DmtKey, InventoryEntry>,
    ) -> Result<Self, InventoryError> {
        let mut inv = Self { dialect, editions, phase, entries, canonical: Vec::new(), digest: Digest::TOMBSTONE };
        let canonical = to_canonical_bytes(&inv.to_cvalue())
            .map_err(|_| inv_invalid("inventory", "inventory has no canonical JSON form"))?;
        let d = digest_bytes(&canonical);
        inv.digest = Digest::from_bytes(d.0);
        inv.canonical = canonical;
        Ok(inv)
    }

    /// Dialect of every key in this inventory (`asme` or `iso`).
    pub const fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Sorted unique edition labels covered by this inventory.
    pub fn editions(&self) -> &[String] {
        &self.editions
    }

    /// Delivery phase tag (e.g. `P1a`).
    pub fn phase(&self) -> &str {
        &self.phase
    }

    /// Inventory entries keyed and sorted by [`DmtKey`].
    pub fn entries(&self) -> &BTreeMap<DmtKey, InventoryEntry> {
        &self.entries
    }

    /// Number of keys in this inventory.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if this inventory has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// RFC 8785 canonical JSON bytes of this inventory.
    pub fn canonical_json(&self) -> &[u8] {
        &self.canonical
    }

    /// SHA-256 digest of [`Self::canonical_json`].
    pub const fn digest(&self) -> Digest {
        self.digest
    }

    /// Converts this inventory to its canonical [`CValue`].
    pub fn to_cvalue(&self) -> CValue {
        let entries = self
            .entries
            .iter()
            .map(|(k, e)| {
                let mut m = BTreeMap::new();
                m.insert("family".to_owned(), CValue::Str(e.family.as_str().to_owned()));
                m.insert("key".to_owned(), CValue::Str(k.as_str().to_owned()));
                m.insert("required_status".to_owned(), CValue::Str(e.required_status.as_str().to_owned()));
                CValue::Obj(m)
            })
            .collect();
        let mut root = BTreeMap::new();
        root.insert("dialect".to_owned(), CValue::Str(self.dialect.as_str().to_owned()));
        root.insert("editions".to_owned(), CValue::Arr(self.editions.iter().map(|s| CValue::Str(s.clone())).collect()));
        root.insert("entries".to_owned(), CValue::Arr(entries));
        root.insert("phase".to_owned(), CValue::Str(self.phase.clone()));
        root.insert("schema".to_owned(), CValue::Str(INVENTORY_SCHEMA.to_owned()));
        CValue::Obj(root)
    }
}

/// Kind of coverage gap reported by [`coverage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CoverageFindingKind {
    /// Inventory key is absent from the DMT table (`missing_row`).
    MissingRow,
    /// Inventory key is present in the DMT table, but its status does not meet `required_status`
    /// (`status_below_required`).
    StatusBelowRequired,
}

impl CoverageFindingKind {
    /// Stable finding code.
    pub const fn as_str(self) -> &'static str {
        match self {
            CoverageFindingKind::MissingRow => "missing_row",
            CoverageFindingKind::StatusBelowRequired => "status_below_required",
        }
    }
}

impl fmt::Display for CoverageFindingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One deterministic coverage finding returned by [`coverage`]
/// (REQ-GDT.RES.ROWASME.INV-002, REQ-GDT.RES.ROWASME.INV-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageFinding {
    /// `missing_row{key}`: inventory key has no row in the table.
    MissingRow { key: DmtKey },
    /// `status_below_required{key, status}`: row exists, but `status` does not satisfy `required`.
    StatusBelowRequired { key: DmtKey, status: RowStatus, required: RequiredStatus },
}

impl CoverageFinding {
    /// Finding kind.
    pub const fn kind(&self) -> CoverageFindingKind {
        match self {
            CoverageFinding::MissingRow { .. } => CoverageFindingKind::MissingRow,
            CoverageFinding::StatusBelowRequired { .. } => CoverageFindingKind::StatusBelowRequired,
        }
    }

    /// Stable machine-readable finding code (`missing_row` or `status_below_required`).
    pub const fn code(&self) -> &'static str {
        self.kind().as_str()
    }

    /// The DMT key this finding refers to.
    pub const fn key(&self) -> &DmtKey {
        match self {
            CoverageFinding::MissingRow { key } | CoverageFinding::StatusBelowRequired { key, .. } => key,
        }
    }

    /// Human-readable detail string.
    pub fn detail(&self) -> String {
        match self {
            CoverageFinding::MissingRow { key } => format!("key '{}' is missing from DMT table", key.as_str()),
            CoverageFinding::StatusBelowRequired { key, status, required } => format!(
                "key '{}' has status '{}', below required '{}'",
                key.as_str(),
                status.as_str(),
                required.as_str()
            ),
        }
    }

    /// Converts this finding to a canonical [`CValue`].
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("key".to_owned(), CValue::Str(self.key().as_str().to_owned()));
        m.insert("kind".to_owned(), CValue::Str(self.code().to_owned()));
        if let CoverageFinding::StatusBelowRequired { status, required, .. } = self {
            m.insert("required_status".to_owned(), CValue::Str(required.as_str().to_owned()));
            m.insert("status".to_owned(), CValue::Str(status.as_str().to_owned()));
        }
        CValue::Obj(m)
    }
}

impl fmt::Display for CoverageFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoverageFinding::MissingRow { key } => write!(f, "missing_row{{{}}}", key.as_str()),
            CoverageFinding::StatusBelowRequired { key, status, .. } => {
                write!(f, "status_below_required{{{}, {}}}", key.as_str(), status.as_str())
            }
        }
    }
}

/// Compares `table` against `inv` in deterministic [`DmtKey`] byte order (D0),
/// returning `missing_row{key}` for every absent key and
/// `status_below_required{key, status}` for every row whose status fails
/// `required_status` (REQ-GDT.RES.ROWASME.INV-002, REQ-GDT.RES.ROWASME.INV-003).
pub fn coverage(table: &DmtTable, inv: &Inventory) -> Vec<CoverageFinding> {
    let mut findings = Vec::new();
    for (key, entry) in inv.entries() {
        match table.get(key.as_str()) {
            None => findings.push(CoverageFinding::MissingRow { key: key.clone() }),
            Some(row) if !entry.required_status.is_met_by(row.status()) => {
                findings.push(CoverageFinding::StatusBelowRequired {
                    key: key.clone(),
                    status: row.status(),
                    required: entry.required_status,
                });
            }
            Some(_) => {}
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// Internal helpers

fn inv_invalid(field: &'static str, detail: impl Into<String>) -> InventoryError {
    InventoryError::InventoryInvalid { field, detail: detail.into() }
}

fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

fn is_phase_tag(s: &str) -> bool {
    let b = s.as_bytes();
    match b.split_first() {
        Some((&first, tail)) => {
            b.len() <= MAX_PHASE_LEN
                && first.is_ascii_alphanumeric()
                && tail.iter().all(|&c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-' || c == b'_')
        }
        None => false,
    }
}

fn as_strict_obj<'a>(
    v: &'a CValue,
    allowed: &[&'static str],
    field: &'static str,
) -> Result<&'a BTreeMap<String, CValue>, InventoryError> {
    let CValue::Obj(m) = v else {
        return Err(inv_invalid(field, "expected JSON object"));
    };
    for k in m.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(inv_invalid(field, format!("unknown field '{}'", echo(k))));
        }
    }
    for req in allowed {
        if !m.contains_key(*req) {
            return Err(inv_invalid(req, "missing required field"));
        }
    }
    Ok(m)
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, field: &'static str) -> Result<&'a str, InventoryError> {
    match m.get(field) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        Some(_) => Err(inv_invalid(field, "expected string")),
        None => Err(inv_invalid(field, "missing required field")),
    }
}

fn req_arr<'a>(m: &'a BTreeMap<String, CValue>, field: &'static str) -> Result<&'a [CValue], InventoryError> {
    match m.get(field) {
        Some(CValue::Arr(a)) => Ok(a.as_slice()),
        Some(_) => Err(inv_invalid(field, "expected array")),
        None => Err(inv_invalid(field, "missing required field")),
    }
}
