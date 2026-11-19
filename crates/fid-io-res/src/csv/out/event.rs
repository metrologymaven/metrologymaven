//! The journal event of `export.results_csv@1`.

use super::write::WriteReceipt;
use super::{OutError, COMMAND};
use crate::csv::map::{MapIdentity, Version};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Largest event accepted by [`ExportEvent::from_json`], in bytes.
const MAX_EVENT_BYTES: usize = 1024 * 1024;
const MAX_EVENT_DEPTH: usize = 4;

/// The journal event of one export (REQ-IO.RES.CSV.MAP-001, REQ-IO.RES.CSV.OUT-002):
///
/// ```json
/// { "command": "export.results_csv@1",
///   "map": { "id": "fid.results.long", "version": "1.0.0", "sha256": "sha256:.." },
///   "records_sha256": "sha256:..", "target": "root://out/r.csv",
///   "characteristics": null, "overwrite": false, "dry_run": false,
///   "lossy": [], "rows": "13",
///   "output": { "sha256": "sha256:..", "bytes": "2048" }, "report_sha256": "sha256:.." }
/// ```
///
/// Serialised as RFC 8785 canonical JSON; 64-bit integers are decimal strings (the
/// `fid-canon` project rule). `lossy` lists the omitted lossy-check fields in
/// [`crate::csv::map::Field::LOSSY_CHECK`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEvent {
    /// The map's ID, version and digest.
    pub map: MapIdentity,
    /// [`super::records_digest`] of the exported rows.
    pub records_sha256: Digest,
    /// The target handle.
    pub target: String,
    /// The characteristic filter, if any.
    pub characteristics: Option<Vec<String>>,
    /// The `overwrite` parameter.
    pub overwrite: bool,
    /// The `dry_run` parameter.
    pub dry_run: bool,
    /// Lossy-check fields the map omits.
    pub lossy: Vec<String>,
    /// Data rows written.
    pub rows: u64,
    /// Digest and size of the file.
    pub output: WriteReceipt,
    /// Digest of the canonical export report.
    pub report_sha256: Digest,
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn bad(reason: &'static str) -> OutError {
    OutError::Event { reason }
}

type Obj = BTreeMap<String, CValue>;

fn obj(v: &CValue) -> Result<&Obj, OutError> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err(bad("wrong_type")),
    }
}

fn keys(o: &Obj, want: &[&str]) -> Result<(), OutError> {
    if o.len() != want.len() || want.iter().any(|k| !o.contains_key(*k)) {
        return Err(bad("bad_keys"));
    }
    Ok(())
}

fn get<'a>(o: &'a Obj, k: &str) -> Result<&'a CValue, OutError> {
    o.get(k).ok_or_else(|| bad("bad_keys"))
}

fn text<'a>(o: &'a Obj, k: &str) -> Result<&'a str, OutError> {
    match get(o, k)? {
        CValue::Str(v) => Ok(v),
        _ => Err(bad("wrong_type")),
    }
}

fn flag(o: &Obj, k: &str) -> Result<bool, OutError> {
    match get(o, k)? {
        CValue::Bool(b) => Ok(*b),
        _ => Err(bad("wrong_type")),
    }
}

fn digest(o: &Obj, k: &str) -> Result<Digest, OutError> {
    Digest::parse(text(o, k)?).map_err(|_| bad("bad_digest"))
}

fn count(o: &Obj, k: &str) -> Result<u64, OutError> {
    let t = text(o, k)?;
    let canonical = !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) && !(t.len() > 1 && t.starts_with('0'));
    if !canonical {
        return Err(bad("bad_integer"));
    }
    t.parse().map_err(|_| bad("bad_integer"))
}

fn strings(v: &CValue) -> Result<Vec<String>, OutError> {
    match v {
        CValue::Arr(a) => a
            .iter()
            .map(|x| match x {
                CValue::Str(t) => Ok(t.clone()),
                _ => Err(bad("wrong_type")),
            })
            .collect(),
        _ => Err(bad("wrong_type")),
    }
}

impl ExportEvent {
    /// The event as a canonical-JSON value.
    pub fn to_cvalue(&self) -> CValue {
        let mut output = BTreeMap::new();
        output.insert("sha256".to_owned(), s(self.output.sha256.as_str()));
        output.insert("bytes".to_owned(), CValue::U64(self.output.bytes));
        let mut m = BTreeMap::new();
        m.insert("command".to_owned(), s(COMMAND));
        m.insert("map".to_owned(), self.map.to_cvalue());
        m.insert("records_sha256".to_owned(), s(self.records_sha256.as_str()));
        m.insert("target".to_owned(), s(&self.target));
        m.insert(
            "characteristics".to_owned(),
            self.characteristics.as_ref().map_or(CValue::Null, |c| CValue::Arr(c.iter().map(|x| s(x)).collect())),
        );
        m.insert("overwrite".to_owned(), CValue::Bool(self.overwrite));
        m.insert("dry_run".to_owned(), CValue::Bool(self.dry_run));
        m.insert("lossy".to_owned(), CValue::Arr(self.lossy.iter().map(|x| s(x)).collect()));
        m.insert("rows".to_owned(), CValue::U64(self.rows));
        m.insert("output".to_owned(), CValue::Obj(output));
        m.insert("report_sha256".to_owned(), s(self.report_sha256.as_str()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, OutError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| bad("json_syntax"))
    }

    /// Reads an event back, strictly (every key present, no other key). Fails
    /// `event_invalid` with reasons `json_syntax`, `wrong_type`, `bad_keys`,
    /// `wrong_command`, `bad_digest`, `bad_version` or `bad_integer`.
    pub fn from_json(bytes: &[u8]) -> Result<ExportEvent, OutError> {
        let v =
            fid_canon::parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| bad("json_syntax"))?;
        let o = obj(&v)?;
        keys(
            o,
            &[
                "characteristics",
                "command",
                "dry_run",
                "lossy",
                "map",
                "output",
                "overwrite",
                "records_sha256",
                "report_sha256",
                "rows",
                "target",
            ],
        )?;
        if text(o, "command")? != COMMAND {
            return Err(bad("wrong_command"));
        }
        let m = obj(get(o, "map")?)?;
        keys(m, &["id", "sha256", "version"])?;
        let map = MapIdentity {
            id: text(m, "id")?.to_owned(),
            version: Version::parse(text(m, "version")?).ok_or_else(|| bad("bad_version"))?,
            digest: digest(m, "sha256")?,
        };
        let out = obj(get(o, "output")?)?;
        keys(out, &["bytes", "sha256"])?;
        let characteristics = match get(o, "characteristics")? {
            CValue::Null => None,
            other => Some(strings(other)?),
        };
        Ok(ExportEvent {
            map,
            records_sha256: digest(o, "records_sha256")?,
            target: text(o, "target")?.to_owned(),
            characteristics,
            overwrite: flag(o, "overwrite")?,
            dry_run: flag(o, "dry_run")?,
            lossy: strings(get(o, "lossy")?)?,
            rows: count(o, "rows")?,
            output: WriteReceipt { sha256: digest(out, "sha256")?, bytes: count(out, "bytes")? },
            report_sha256: digest(o, "report_sha256")?,
        })
    }
}
