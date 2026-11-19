//! The DMT table container `fid.dmt-table/1`: a version and rows keyed by [`DmtKey`], its
//! strict bounded loader, and its canonical form and digest.
//!
//! ```json
//! { "schema": "fid.dmt-table/1", "version": "1.0.0", "rows": [ <row>, ... ] }
//! ```
//!
//! Rows may appear in any order in the file; the canonical form lists them by key, byte by
//! byte, so the digest does not depend on file order (AC-2).

use super::ident::{DmtKey, SemVer};
use super::row::{as_obj, canonical_and_digest, row_from_value, DmtRow};
use super::DmtError;
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeMap;

/// The `schema` value of a table document; bumped on any change to the row format.
pub const TABLE_SCHEMA: &str = "fid.dmt-table/1";
/// Largest accepted table file, in bytes.
pub const MAX_TABLE_BYTES: usize = 32 * 1024 * 1024;
/// Deepest accepted nesting: table, rows, row, variants, chain, steps, operator, params.
pub const MAX_TABLE_DEPTH: usize = 8;
/// Most rows in one table.
pub const MAX_ROWS: usize = 10_000;

const TABLE_FIELDS: [&str; 3] = ["rows", "schema", "version"];

/// A loaded, validated DMT table. Fields are private and read through getters, so the
/// cached canonical form and digest always match the rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmtTable {
    version: SemVer,
    rows: BTreeMap<DmtKey, DmtRow>,
    canonical: Vec<u8>,
    digest: Digest,
}

impl DmtTable {
    /// Loads a table from file bytes: at most [`MAX_TABLE_BYTES`], UTF-8 JSON with no
    /// duplicate member name in any object and nesting at most [`MAX_TABLE_DEPTH`]
    /// (`malformed_table` with the `fid-canon` code as reason), then [`DmtTable::from_value`].
    pub fn load(bytes: &[u8]) -> Result<DmtTable, DmtError> {
        if bytes.len() > MAX_TABLE_BYTES {
            return Err(DmtError::malformed("too_large"));
        }
        let v = fid_canon::parse_with_limits(bytes, MAX_TABLE_BYTES, MAX_TABLE_DEPTH)
            .map_err(|e| DmtError::malformed(e.code()))?;
        DmtTable::from_value(&v)
    }

    /// Builds a table from a parsed JSON value. Checks the table fields (`unknown_field`,
    /// `missing_field`, `bad_schema`, `bad_version`, `too_many_rows`), then parses the rows in
    /// file order; each row is fully checked before its key is compared with the earlier rows
    /// (`duplicate_dmt_key`). The first failure is returned.
    pub fn from_value(v: &CValue) -> Result<DmtTable, DmtError> {
        let o = as_obj(v, &TABLE_FIELDS, &TABLE_FIELDS)?;
        match o.get("schema") {
            Some(CValue::Str(s)) if s == TABLE_SCHEMA => {}
            Some(CValue::Str(_)) => return Err(DmtError::malformed("bad_schema")),
            _ => return Err(DmtError::malformed("wrong_type")),
        }
        let version = match o.get("version") {
            Some(CValue::Str(s)) => SemVer::parse(s).ok_or_else(|| DmtError::malformed("bad_version"))?,
            _ => return Err(DmtError::malformed("wrong_type")),
        };
        let raw = match o.get("rows") {
            Some(CValue::Arr(a)) => a,
            _ => return Err(DmtError::malformed("wrong_type")),
        };
        if raw.len() > MAX_ROWS {
            return Err(DmtError::malformed("too_many_rows"));
        }
        let mut rows = BTreeMap::new();
        for rv in raw {
            let row = row_from_value(rv)?;
            if rows.contains_key(row.key()) {
                return Err(DmtError::duplicate_key(row.key().as_str()));
            }
            rows.insert(row.key().clone(), row);
        }
        DmtTable::build(version, rows)
    }

    /// Builds a table from rows (each stored under its own key) and computes its canonical
    /// form and digest. Every row is re-validated against the full schema rules by parsing
    /// its JSON form again with [`row_from_value`]; any difference fails
    /// `malformed_table{row_invalid}` (or the parser's own error). Also fails
    /// `malformed_table{key_mismatch}` when a map key differs from its row's key, and
    /// `malformed_table{too_many_rows}`.
    pub fn new(version: SemVer, rows: BTreeMap<DmtKey, DmtRow>) -> Result<DmtTable, DmtError> {
        if rows.len() > MAX_ROWS {
            return Err(DmtError::malformed("too_many_rows"));
        }
        for (k, r) in &rows {
            if k != r.key() {
                return Err(DmtError::malformed("key_mismatch"));
            }
            if row_from_value(&r.to_cvalue())? != *r {
                return Err(DmtError::malformed("row_invalid"));
            }
        }
        DmtTable::build(version, rows)
    }

    /// Computes the canonical form of rows already validated by [`row_from_value`].
    fn build(version: SemVer, rows: BTreeMap<DmtKey, DmtRow>) -> Result<DmtTable, DmtError> {
        let mut t = DmtTable { version, rows, canonical: Vec::new(), digest: Digest::TOMBSTONE };
        let (canonical, digest) = canonical_and_digest(&t.to_cvalue())?;
        t.canonical = canonical;
        t.digest = digest;
        Ok(t)
    }

    /// The table as a JSON value: `{rows: [rows by key], schema, version}`.
    pub fn to_cvalue(&self) -> CValue {
        let rows = self.rows.values().map(DmtRow::to_cvalue).collect();
        CValue::Obj(
            [
                ("rows".to_owned(), CValue::Arr(rows)),
                ("schema".to_owned(), CValue::Str(TABLE_SCHEMA.to_owned())),
                ("version".to_owned(), CValue::Str(self.version.to_string())),
            ]
            .into_iter()
            .collect(),
        )
    }

    /// RFC 8785 canonical JSON of [`DmtTable::to_cvalue`].
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    /// The table digest: SHA-256 of [`DmtTable::canonical_bytes`] (REQ-004).
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// Table version.
    pub fn version(&self) -> SemVer {
        self.version
    }

    /// Rows by key.
    pub fn rows(&self) -> &BTreeMap<DmtKey, DmtRow> {
        &self.rows
    }

    /// The row with this key text, if any.
    pub fn get(&self, key: &str) -> Option<&DmtRow> {
        DmtKey::parse(key).ok().and_then(|k| self.rows.get(&k))
    }
}
