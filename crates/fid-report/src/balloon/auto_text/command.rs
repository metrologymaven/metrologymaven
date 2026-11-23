//! Command handler for `report.extract_drawing_text@1` (R1, `RPT.BALLOON.AUTO.TEXT`).

use super::model::{AutoTextError, DisplayTile, DrawingTextMap, PageKind, SheetRect};
use super::pdf_extract::{extract_pdf_page, ExtractedPage};
use super::tile::render_display_tile;
use crate::balloon::sheet::{DrawingBlobBackend, DrawingStore, SheetId};
use fid_canon::CValue;

/// Canonical command ID for bounded drawing text extraction (`R1`, Activity Log only).
pub const EXTRACT_DRAWING_TEXT: &str = "report.extract_drawing_text@1";

/// Parameters of `report.extract_drawing_text@1` (`Design: Registry command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractDrawingTextParams {
    pub sheet: SheetId,
    pub include_frame_boxes: bool,
}

impl ExtractDrawingTextParams {
    pub fn from_value(v: &CValue) -> Result<Self, AutoTextError> {
        let CValue::Obj(m) = v else {
            return Err(AutoTextError::BadParams("params must be an object"));
        };
        let sheet_str = match m.get("sheet") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => return Err(AutoTextError::BadParams("missing sheet")),
        };
        let sheet = SheetId::parse(sheet_str).map_err(|_| AutoTextError::BadParams("invalid sheet uuid"))?;
        let include_frame_boxes = match m.get("include_frame_boxes") {
            Some(CValue::Bool(b)) => *b,
            None => true,
            _ => return Err(AutoTextError::BadParams("include_frame_boxes must be boolean")),
        };
        Ok(ExtractDrawingTextParams { sheet, include_frame_boxes })
    }
}

/// Executes `report.extract_drawing_text@1` (R1, D0, `REQ-001..004`, `AC-1..AC-4`).
pub fn extract_drawing_text(
    store: &DrawingStore,
    blobs: &dyn DrawingBlobBackend,
    p: &ExtractDrawingTextParams,
) -> Result<DrawingTextMap, AutoTextError> {
    Ok(extract_drawing_page(store, blobs, p)?.map)
}

/// Extracts both the [`DrawingTextMap`] and vector path segments needed to render bounded display
/// tiles for `p.sheet` (`REQ-005`).
pub fn extract_drawing_page(
    store: &DrawingStore,
    blobs: &dyn DrawingBlobBackend,
    p: &ExtractDrawingTextParams,
) -> Result<ExtractedPage, AutoTextError> {
    let sheet = store.sheet(p.sheet).ok_or(AutoTextError::NotFound("sheet"))?;
    let rev = store.get_revision(sheet.drawing_rev).ok_or(AutoTextError::NotFound("drawing_rev"))?;

    // Pure raster sheets (PNG/TIFF) have no PDF content stream -> RasterOnly with empty runs.
    if sheet.image.is_some() {
        return Ok(ExtractedPage {
            map: DrawingTextMap {
                sheet: sheet.id,
                page_kind: PageKind::RasterOnly,
                runs: Vec::new(),
                frame_boxes: Vec::new(),
                warnings: Vec::new(),
            },
            segments: Vec::new(),
            sheet_bounds_cmm: (sheet.width_cmm, sheet.height_cmm),
        });
    }

    let raw = blobs
        .get_blob(&rev.source_digest)
        .map_err(|e| AutoTextError::PdfMalformed { at: 0, detail: e.to_string() })?
        .ok_or(AutoTextError::NotFound("source_blob"))?;

    extract_pdf_page(sheet.id, &raw, sheet.index, p.include_frame_boxes)
}

/// Renders a bounded RGBA8 snippet/tile crop for `sheet` at `dpi` (capped at 300 dpi and 64 Mpx,
/// `REQ-005`).
pub fn render_sheet_tile(
    store: &DrawingStore,
    blobs: &dyn DrawingBlobBackend,
    sheet: SheetId,
    crop: SheetRect,
    dpi: u32,
) -> Result<DisplayTile, AutoTextError> {
    let extracted = extract_drawing_page(store, blobs, &ExtractDrawingTextParams { sheet, include_frame_boxes: true })?;
    render_display_tile(extracted.sheet_bounds_cmm, &extracted.map, &extracted.segments, crop, dpi)
}

/// Dispatches `report.extract_drawing_text@1` from canonical JSON params to canonical JSON result.
pub fn execute(
    command: &str,
    params: &CValue,
    store: &DrawingStore,
    blobs: &dyn DrawingBlobBackend,
) -> Result<CValue, AutoTextError> {
    if command != EXTRACT_DRAWING_TEXT {
        return Err(AutoTextError::BadParams("unknown drawing text command"));
    }
    let p = ExtractDrawingTextParams::from_value(params)?;
    let map = extract_drawing_text(store, blobs, &p)?;
    Ok(map.to_value())
}
