//! The rows an export writes: one per result, every value as authored text.

use super::OutError;
use crate::csv::map::{strip_bom, DecimalMark, Delimiter, Dialect, Field, FieldType, LineEnding};
use fid_canon::{CValue, CanonError};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// Schema tag of a records file.
pub const RECORDS_SCHEMA: &str = "results_csv.records@1";
/// Largest records file, in bytes.
pub const MAX_RECORDS_BYTES: usize = 256 * 1024 * 1024;
/// Deepest JSON nesting of a records file (it needs 3).
pub const MAX_RECORDS_DEPTH: usize = 4;
/// The `record_status` of a transcribed result (`IO.RES.TRANS`).
pub const TRANSCRIBED: &str = "transcribed";
/// Key of the result ID in a records file.
pub const RESULT_ID_KEY: &str = "result_id";

/// One result to export: its ID and the value of each vocabulary [`Field`] it has, as
/// authored text. Decimal fields hold `.`-form decimal text (the exporter only swaps the
/// mark); nothing is ever parsed to a float.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRow {
    result_id: String,
    values: BTreeMap<Field, String>,
}

impl ResultRow {
    /// A row with no values yet.
    pub fn new(result_id: &str) -> ResultRow {
        ResultRow { result_id: result_id.to_owned(), values: BTreeMap::new() }
    }

    /// Sets `field` to `value` (checked when the row is exported).
    pub fn with(mut self, field: Field, value: &str) -> ResultRow {
        self.values.insert(field, value.to_owned());
        self
    }

    /// The result ID.
    pub fn result_id(&self) -> &str {
        &self.result_id
    }

    /// The value of `field`, if set.
    pub fn get(&self, field: Field) -> Option<&str> {
        self.values.get(&field).map(String::as_str)
    }

    /// Whether `record_status` is `transcribed`.
    pub fn is_transcribed(&self) -> bool {
        self.get(Field::RecordStatus) == Some(TRANSCRIBED)
    }

    /// The characteristic ID ("" if unset; [`check_rows`] refuses that).
    pub(crate) fn characteristic(&self) -> &str {
        self.get(Field::CharacteristicId).unwrap_or("")
    }

    /// The part ID, "" if unset.
    pub(crate) fn part(&self) -> &str {
        self.get(Field::PartId).unwrap_or("")
    }

    fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert(RESULT_ID_KEY.to_owned(), CValue::Str(self.result_id.clone()));
        for (f, v) in &self.values {
            m.insert(f.as_str().to_owned(), CValue::Str(v.clone()));
        }
        CValue::Obj(m)
    }
}

fn bad(reason: &'static str, record: Option<usize>) -> OutError {
    OutError::Records { reason, record }
}

/// Reads a records file:
///
/// ```json
/// { "schema": "results_csv.records@1",
///   "records": [ { "result_id": "R1", "characteristic_id": "C1", "actual": "10.012",
///                  "unit": "mm", "record_status": "evaluated", "comment": null } ] }
/// ```
///
/// Every record needs `result_id`; every other key is a field wire name whose value is a
/// string or `null` (absent). Decimal values are strings, so their authored text is kept.
/// Errors are `records_invalid` with reasons `json_syntax`, `invalid_utf8`,
/// `duplicate_key`, `too_large`, `wrong_type`, `missing_key`, `unknown_key`,
/// `unsupported_schema`, and the [`check_rows`] reasons.
pub fn load_records(bytes: &[u8]) -> Result<Vec<ResultRow>, OutError> {
    let body = strip_bom(bytes);
    if body.len() > MAX_RECORDS_BYTES {
        return Err(bad("too_large", None));
    }
    let value = fid_canon::parse_with_limits(body, MAX_RECORDS_BYTES, MAX_RECORDS_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => bad("too_large", None),
        CanonError::DuplicateKey(_) => bad("duplicate_key", None),
        CanonError::InvalidUtf8 => bad("invalid_utf8", None),
        CanonError::Syntax(_) | CanonError::NonFiniteNumber => bad("json_syntax", None),
    })?;
    let top = match &value {
        CValue::Obj(o) => o,
        _ => return Err(bad("wrong_type", None)),
    };
    if top.keys().any(|k| k != "schema" && k != "records") {
        return Err(bad("unknown_key", None));
    }
    match top.get("schema") {
        Some(CValue::Str(s)) if s == RECORDS_SCHEMA => {}
        Some(CValue::Str(_)) => return Err(bad("unsupported_schema", None)),
        Some(_) => return Err(bad("wrong_type", None)),
        None => return Err(bad("missing_key", None)),
    }
    let items = match top.get("records") {
        Some(CValue::Arr(a)) => a,
        Some(_) => return Err(bad("wrong_type", None)),
        None => return Err(bad("missing_key", None)),
    };
    let mut rows = Vec::with_capacity(items.len());
    for (n, item) in items.iter().enumerate() {
        let at = Some(n);
        let o = match item {
            CValue::Obj(o) => o,
            _ => return Err(bad("wrong_type", at)),
        };
        let id = match o.get(RESULT_ID_KEY) {
            Some(CValue::Str(s)) => s,
            Some(_) => return Err(bad("wrong_type", at)),
            None => return Err(bad("missing_key", at)),
        };
        let mut row = ResultRow::new(id);
        for (k, v) in o {
            if k == RESULT_ID_KEY {
                continue;
            }
            let field = Field::parse(k).ok_or_else(|| bad("unknown_key", at))?;
            match v {
                CValue::Null => {}
                CValue::Str(s) => row = row.with(field, s),
                _ => return Err(bad("wrong_type", at)),
            }
        }
        rows.push(row);
    }
    check_rows(&rows)?;
    Ok(rows)
}

/// Checks rows before export. Reasons (`records_invalid`, naming the row position):
/// `missing_result_id`, `duplicate_result_id`, `missing_characteristic_id`,
/// `bad_decimal` (not `[+-]? digit+ (. digit+)? ([eE] [+-]? digit+)?`), `bad_enum` (a
/// value outside the field's closed list) and `transcription_source_missing` (a
/// transcribed result without `source_software` or `source_version`).
pub fn check_rows(rows: &[ResultRow]) -> Result<(), OutError> {
    let point = Dialect::new(Delimiter::Comma, DecimalMark::Point, true, LineEnding::Lf)
        .map_err(|_| bad("bad_decimal", None))?;
    let mut ids = BTreeSet::new();
    for (n, row) in rows.iter().enumerate() {
        let at = Some(n);
        if row.result_id.is_empty() {
            return Err(bad("missing_result_id", at));
        }
        if !ids.insert(row.result_id.as_str()) {
            return Err(bad("duplicate_result_id", at));
        }
        if row.characteristic().is_empty() {
            return Err(bad("missing_characteristic_id", at));
        }
        for (f, v) in &row.values {
            match f.field_type() {
                FieldType::Decimal => {
                    point.from_point_decimal(v).map_err(|_| bad("bad_decimal", at))?;
                }
                FieldType::Enum(values) if !values.contains(&v.as_str()) => return Err(bad("bad_enum", at)),
                _ => {}
            }
        }
        let has = |f: Field| row.get(f).is_some_and(|s| !s.is_empty());
        if row.is_transcribed() && !(has(Field::SourceSoftware) && has(Field::SourceVersion)) {
            return Err(bad("transcription_source_missing", at));
        }
    }
    Ok(())
}

/// SHA-256 of the RFC 8785 canonical form of `rows` as a records file (null values
/// dropped), in the given order. Journaled so a replay can tell its input is the same.
pub fn records_digest(rows: &[ResultRow]) -> Result<Digest, OutError> {
    let mut top = BTreeMap::new();
    top.insert("schema".to_owned(), CValue::Str(RECORDS_SCHEMA.to_owned()));
    top.insert("records".to_owned(), CValue::Arr(rows.iter().map(ResultRow::to_cvalue).collect()));
    let bytes = fid_canon::to_canonical_bytes(&CValue::Obj(top)).map_err(|_| bad("json_syntax", None))?;
    Ok(Digest::from_bytes(fid_canon::digest_bytes(&bytes).0))
}
