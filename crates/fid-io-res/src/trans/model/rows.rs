//! The rows file (`transcription-rows@1`): source rows and source facts as JSON, for
//! fixtures and for importers that hand rows over as data.
//!
//! ```json
//! { "schema": "transcription-rows@1",
//!   "source": { "software": "VendorX", "version": null, "file_sha256": "sha256:<64 hex>",
//!               "imported_at": "2026-11-17T10:00:00Z", "format": "csv" },
//!   "rows": [ { "characteristic": "C1", "value": "10.012", "unit": "mm", "status": "pass" } ] }
//! ```
//!
//! Row members are the [`SourceRow`] field names; each is optional. Text members are
//! strings or `null` (a number is refused, so authored decimal text is never lost);
//! `status` is `pass`, `fail`, `undecided`, `unrecognized` or `null`; `has_points` and
//! `has_features` are booleans. Refusals (`TransError::RowsInvalid`): first the parse stage
//! (`too_large`, `invalid_utf8`, `json_syntax`, `duplicate_key`), then per object
//! `case_duplicate_key` (two keys equal when ASCII-lowercased) before `unknown_key`, then
//! `missing_key`, `wrong_type`, `unsupported_schema`, `bad_digest`, `too_many_rows` (more
//! than `MAX_ROWS`) and `bad_status`, each naming the row position when it is in a row.

use std::collections::{BTreeMap, BTreeSet};

use fid_canon::{CValue, CanonError};
use fid_types::Digest;

use super::{SourceInfo, SourceRow, SourceStatus, TransError};

/// Schema tag of a rows file.
pub const ROWS_SCHEMA: &str = "transcription-rows@1";
/// Largest rows file, in bytes.
pub const MAX_ROWS_FILE_BYTES: usize = 256 * 1024 * 1024;
/// Deepest nesting a rows file needs (top, rows array, row object, member).
const MAX_DEPTH: usize = 4;

const TOP_KEYS: [&str; 3] = ["rows", "schema", "source"];
const SOURCE_KEYS: [&str; 5] = ["file_sha256", "format", "imported_at", "software", "version"];
const ROW_KEYS: [&str; 14] = [
    "characteristic",
    "decision_rule",
    "has_features",
    "has_points",
    "lower_limit",
    "measurand",
    "outcome",
    "part",
    "status",
    "status_text",
    "uncertainty",
    "unit",
    "upper_limit",
    "value",
];

fn bad(reason: &'static str, row: Option<usize>) -> TransError {
    TransError::RowsInvalid { reason, row }
}

type Obj = BTreeMap<String, CValue>;

fn keys(o: &Obj, allowed: &[&str], row: Option<usize>) -> Result<(), TransError> {
    let mut seen = BTreeSet::new();
    for k in o.keys() {
        if !seen.insert(k.to_ascii_lowercase()) {
            return Err(bad("case_duplicate_key", row));
        }
    }
    if o.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(bad("unknown_key", row));
    }
    Ok(())
}

fn text(o: &Obj, key: &str, row: Option<usize>) -> Result<Option<String>, TransError> {
    match o.get(key) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(bad("wrong_type", row)),
    }
}

fn flag(o: &Obj, key: &str, row: Option<usize>) -> Result<bool, TransError> {
    match o.get(key) {
        None => Ok(false),
        Some(CValue::Bool(b)) => Ok(*b),
        Some(_) => Err(bad("wrong_type", row)),
    }
}

fn required(o: &Obj, key: &str) -> Result<String, TransError> {
    text(o, key, None)?.ok_or_else(|| bad("missing_key", None))
}

fn row_of(item: &CValue, n: usize) -> Result<SourceRow, TransError> {
    let at = Some(n);
    let CValue::Obj(o) = item else { return Err(bad("wrong_type", at)) };
    keys(o, &ROW_KEYS, at)?;
    let status = match text(o, "status", at)?.as_deref() {
        None => SourceStatus::Absent,
        Some("pass") => SourceStatus::Pass,
        Some("fail") => SourceStatus::Fail,
        Some("undecided") => SourceStatus::Undecided,
        Some("unrecognized") => SourceStatus::Unrecognized,
        Some(_) => return Err(bad("bad_status", at)),
    };
    Ok(SourceRow {
        characteristic: text(o, "characteristic", at)?,
        part: text(o, "part", at)?,
        measurand: text(o, "measurand", at)?,
        value: text(o, "value", at)?,
        unit: text(o, "unit", at)?,
        status,
        status_text: text(o, "status_text", at)?,
        uncertainty: text(o, "uncertainty", at)?,
        decision_rule: text(o, "decision_rule", at)?,
        outcome: text(o, "outcome", at)?,
        lower_limit: text(o, "lower_limit", at)?,
        upper_limit: text(o, "upper_limit", at)?,
        has_points: flag(o, "has_points", at)?,
        has_features: flag(o, "has_features", at)?,
    })
}

/// Reads a rows file into the source facts and rows. A UTF-8 BOM is refused as
/// `json_syntax`. The rows themselves are checked by [`super::transcribe`], not here.
pub fn load_rows(bytes: &[u8]) -> Result<(SourceInfo, Vec<SourceRow>), TransError> {
    if bytes.len() > MAX_ROWS_FILE_BYTES {
        return Err(bad("too_large", None));
    }
    let value = fid_canon::parse_with_limits(bytes, MAX_ROWS_FILE_BYTES, MAX_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => bad("too_large", None),
        CanonError::InvalidUtf8 => bad("invalid_utf8", None),
        CanonError::DuplicateKey(_) => bad("duplicate_key", None),
        CanonError::Syntax(_) | CanonError::NonFiniteNumber => bad("json_syntax", None),
    })?;
    let CValue::Obj(top) = &value else { return Err(bad("wrong_type", None)) };
    keys(top, &TOP_KEYS, None)?;
    match top.get("schema") {
        Some(CValue::Str(s)) if s == ROWS_SCHEMA => {}
        Some(CValue::Str(_)) => return Err(bad("unsupported_schema", None)),
        Some(_) => return Err(bad("wrong_type", None)),
        None => return Err(bad("missing_key", None)),
    }
    let src = match top.get("source") {
        Some(CValue::Obj(o)) => o,
        Some(_) => return Err(bad("wrong_type", None)),
        None => return Err(bad("missing_key", None)),
    };
    keys(src, &SOURCE_KEYS, None)?;
    let source = SourceInfo {
        software: required(src, "software")?,
        version: text(src, "version", None)?,
        file_sha256: Digest::parse(&required(src, "file_sha256")?).map_err(|_| bad("bad_digest", None))?,
        imported_at: required(src, "imported_at")?,
        format: required(src, "format")?,
    };
    let items = match top.get("rows") {
        Some(CValue::Arr(a)) => a,
        Some(_) => return Err(bad("wrong_type", None)),
        None => return Err(bad("missing_key", None)),
    };
    if items.len() > super::MAX_ROWS {
        return Err(bad("too_many_rows", None));
    }
    let rows = items.iter().enumerate().map(|(n, item)| row_of(item, n)).collect::<Result<Vec<_>, _>>()?;
    Ok((source, rows))
}
