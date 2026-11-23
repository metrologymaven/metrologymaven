//! Canonical journal events for `RPT.BALLOON.SHEET.IMPORT` (`AC-1`, `AC-6`).

use super::error::SheetError;
use super::model::{parse_drawing_text, DrawingRevId, ReaderWarning, RevisionId, SheetId, SizeSpec};
use super::store::digest_bytes;
use crate::doc::model::obj;
use crate::doc::text::{Text, Untrusted};
use fid_canon::{to_canonical_bytes, CValue};
use fid_types::Digest;
use std::collections::BTreeMap;

/// Command / event ID for importing a drawing revision (`W2`).
pub const ADD_DRAWING_BALLOON: &str = "report.add_drawing_balloon@1";
/// Command ID for listing drawing revisions (`R0`).
pub const LIST_DRAWING_BALLOON: &str = "report.list_drawing_balloon@1";
/// Command / event ID for setting physical size or DPI on a raster sheet (`W2`).
pub const SET_SIZE_DRAWING_BALLOON: &str = "report.set_size_drawing_balloon@1";

/// Payload of [`DrawingEvent::AddDrawingBalloon`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddDrawingEvent {
    pub drawing_rev: DrawingRevId,
    pub part_rev: RevisionId,
    pub drawing_number: Untrusted<Text>,
    pub revision_label: Untrusted<Text>,
    pub title: Option<Untrusted<Text>>,
    pub source_digest: Digest,
    pub source_blob_digest: Digest,
    pub sheet_ids: Vec<SheetId>,
    pub warnings: Vec<ReaderWarning>,
    pub result_digest: Digest,
}

/// Canonical journal event emitted by `report.add_drawing_balloon@1` and
/// `report.set_size_drawing_balloon@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawingEvent {
    AddDrawingBalloon(Box<AddDrawingEvent>),
    SetSizeDrawingBalloon { sheet: SheetId, size: SizeSpec, width_cmm: i64, height_cmm: i64, result_digest: Digest },
}

impl DrawingEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            DrawingEvent::AddDrawingBalloon(_) => ADD_DRAWING_BALLOON,
            DrawingEvent::SetSizeDrawingBalloon { .. } => SET_SIZE_DRAWING_BALLOON,
        }
    }

    pub fn to_value(&self) -> CValue {
        match self {
            DrawingEvent::AddDrawingBalloon(ev) => obj([
                ("drawing_number", ev.drawing_number.to_value()),
                ("drawing_rev", CValue::Str(ev.drawing_rev.to_string())),
                ("kind", CValue::Str(ADD_DRAWING_BALLOON.into())),
                ("part_rev", CValue::Str(ev.part_rev.to_string())),
                ("result_digest", CValue::Str(ev.result_digest.to_string())),
                ("revision_label", ev.revision_label.to_value()),
                ("sheet_ids", CValue::Arr(ev.sheet_ids.iter().map(|s| CValue::Str(s.to_string())).collect())),
                ("source_blob_digest", CValue::Str(ev.source_blob_digest.to_string())),
                ("source_digest", CValue::Str(ev.source_digest.to_string())),
                ("title", ev.title.as_ref().map_or(CValue::Null, Untrusted::to_value)),
                ("warnings", CValue::Arr(ev.warnings.iter().map(ReaderWarning::to_value).collect())),
            ]),
            DrawingEvent::SetSizeDrawingBalloon { sheet, size, width_cmm, height_cmm, result_digest } => obj([
                ("height_cmm", CValue::Str(height_cmm.to_string())),
                ("kind", CValue::Str(SET_SIZE_DRAWING_BALLOON.into())),
                ("result_digest", CValue::Str(result_digest.to_string())),
                ("sheet", CValue::Str(sheet.to_string())),
                ("size", size.to_value()),
                ("width_cmm", CValue::Str(width_cmm.to_string())),
            ]),
        }
    }

    pub fn from_value(v: &CValue) -> Result<Self, SheetError> {
        let CValue::Obj(m) = v else {
            return Err(SheetError::Store("event is not an object".into()));
        };
        let kind = req_str(m, "kind")?;
        match kind {
            ADD_DRAWING_BALLOON => {
                let drawing_rev = DrawingRevId::parse(req_str(m, "drawing_rev")?)?;
                let part_rev = RevisionId::parse(req_str(m, "part_rev")?)?;
                let dn = read_untrusted(m, "drawing_number", true)?;
                let rl = read_untrusted(m, "revision_label", true)?;
                let title = match m.get("title") {
                    Some(CValue::Null) | None => None,
                    Some(tv) => Some(read_untrusted_val(tv, false)?),
                };
                let source_digest = Digest::parse(req_str(m, "source_digest")?)
                    .map_err(|_| SheetError::Store("bad source_digest".into()))?;
                let source_blob_digest = Digest::parse(req_str(m, "source_blob_digest")?)
                    .map_err(|_| SheetError::Store("bad source_blob_digest".into()))?;
                let result_digest = Digest::parse(req_str(m, "result_digest")?)
                    .map_err(|_| SheetError::Store("bad result_digest".into()))?;
                let sheet_ids = match m.get("sheet_ids") {
                    Some(CValue::Arr(arr)) => {
                        let mut out = Vec::with_capacity(arr.len());
                        for item in arr {
                            let CValue::Str(s) = item else {
                                return Err(SheetError::Store("sheet_id must be string".into()));
                            };
                            out.push(SheetId::parse(s)?);
                        }
                        out
                    }
                    _ => return Err(SheetError::Store("missing sheet_ids".into())),
                };
                let warnings = match m.get("warnings") {
                    Some(CValue::Arr(arr)) => {
                        let mut out = Vec::with_capacity(arr.len());
                        for item in arr {
                            out.push(ReaderWarning::from_value(item)?);
                        }
                        out
                    }
                    _ => return Err(SheetError::Store("missing warnings".into())),
                };
                Ok(DrawingEvent::AddDrawingBalloon(Box::new(AddDrawingEvent {
                    drawing_rev,
                    part_rev,
                    drawing_number: dn,
                    revision_label: rl,
                    title,
                    source_digest,
                    source_blob_digest,
                    sheet_ids,
                    warnings,
                    result_digest,
                })))
            }
            SET_SIZE_DRAWING_BALLOON => {
                let sheet = SheetId::parse(req_str(m, "sheet")?)?;
                let size_v = m.get("size").ok_or_else(|| SheetError::Store("missing size".into()))?;
                let size = SizeSpec::from_value(size_v)?;
                let width_cmm = req_str(m, "width_cmm")?
                    .parse::<i64>()
                    .map_err(|_| SheetError::Store("invalid width_cmm".into()))?;
                let height_cmm = req_str(m, "height_cmm")?
                    .parse::<i64>()
                    .map_err(|_| SheetError::Store("invalid height_cmm".into()))?;
                let result_digest = Digest::parse(req_str(m, "result_digest")?)
                    .map_err(|_| SheetError::Store("bad result_digest".into()))?;
                Ok(DrawingEvent::SetSizeDrawingBalloon { sheet, size, width_cmm, height_cmm, result_digest })
            }
            _ => Err(SheetError::Store("unknown drawing event kind".into())),
        }
    }
}

/// Computes the canonical RFC 8785 digest of a `CValue` payload for exact replay comparison.
pub(crate) fn canonical_digest(v: &CValue) -> Result<Digest, SheetError> {
    let bytes = to_canonical_bytes(v).map_err(|e| SheetError::Store(format!("canonicalise failed: {e}")))?;
    Ok(digest_bytes(&bytes))
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, SheetError> {
    match m.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(SheetError::Store("missing or non-string event field".into())),
    }
}

fn read_untrusted(
    m: &BTreeMap<String, CValue>,
    k: &str,
    require_non_empty: bool,
) -> Result<Untrusted<Text>, SheetError> {
    let v = m.get(k).ok_or_else(|| SheetError::Store("missing untrusted text".into()))?;
    read_untrusted_val(v, require_non_empty)
}

fn read_untrusted_val(v: &CValue, require_non_empty: bool) -> Result<Untrusted<Text>, SheetError> {
    let ut = Untrusted::<Text>::from_value(v)?;
    parse_drawing_text(ut.untrusted_value().as_str(), require_non_empty)
}
