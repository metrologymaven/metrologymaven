//! Core types, errors and canonical JSON serialisation for `RPT.BALLOON.AUTO.TEXT`.
//!
//! Pure integer geometry (`0.01 mm`, top-left origin, millidegrees): zero floating-point types
//! or operations (RPT float rule, `RPT.MODEL.FMT` REQ-002).

use crate::balloon::sheet::SheetId;
use crate::doc::model::obj;
use crate::doc::text::{Text, Untrusted};
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;

/// Hard cap on visible text runs extracted per sheet (`Scope`, `REQ-002`, `AC-3`).
pub const MAX_TEXT_RUNS_PER_SHEET: usize = 50_000;
/// Hard cap on total extracted UTF-8 text bytes per sheet (`Scope`, `REQ-002`, `AC-3`: 10 MiB).
pub const MAX_TEXT_BYTES_PER_SHEET: usize = 10 * 1024 * 1024;
/// Hard cap on display tile DPI (`REQ-005`).
pub const MAX_TILE_DPI: u32 = 300;
/// Default display tile DPI (`Scope`).
pub const DEFAULT_TILE_DPI: u32 = 150;
/// Hard cap on rendered display tile pixels per sheet (`Scope`, `REQ-005`: 64 Mpx).
pub const MAX_TILE_PIXELS_PER_SHEET: u64 = 64_000_000;

/// Typed errors of `RPT.BALLOON.AUTO.TEXT` (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoTextError {
    /// `report.not_found`: sheet or source blob was not found.
    NotFound(&'static str),
    /// `report.limit_exceeded`: run count, text bytes, operator budget, stream bomb, or tile cap exceeded.
    LimitExceeded { limit: &'static str, detail: String },
    /// `report.pdf_malformed`: malformed PDF syntax or content stream with byte offset.
    PdfMalformed { at: u64, detail: String },
    /// Invalid command parameters.
    BadParams(&'static str),
}

impl AutoTextError {
    pub fn code(&self) -> &'static str {
        match self {
            AutoTextError::NotFound(_) => "report.not_found",
            AutoTextError::LimitExceeded { .. } => "report.limit_exceeded",
            AutoTextError::PdfMalformed { .. } => "report.pdf_malformed",
            AutoTextError::BadParams(_) => "report.bad_params",
        }
    }
}

impl fmt::Display for AutoTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AutoTextError::NotFound(what) => write!(f, "report.not_found: {what}"),
            AutoTextError::LimitExceeded { limit, detail } => {
                write!(f, "report.limit_exceeded ({limit}): {detail}")
            }
            AutoTextError::PdfMalformed { at, detail } => {
                write!(f, "report.pdf_malformed at byte {at}: {detail}")
            }
            AutoTextError::BadParams(reason) => write!(f, "report.bad_params: {reason}"),
        }
    }
}

impl std::error::Error for AutoTextError {}

/// Axis-aligned rectangle in integer `0.01 mm` sheet coordinates (origin top-left, `Design`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SheetRect {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

impl SheetRect {
    pub fn new(x0: i64, y0: i64, x1: i64, y1: i64) -> Self {
        SheetRect { x0: x0.min(x1), y0: y0.min(y1), x1: x0.max(x1), y1: y0.max(y1) }
    }

    pub fn width(self) -> i64 {
        self.x1.saturating_sub(self.x0)
    }

    pub fn height(self) -> i64 {
        self.y1.saturating_sub(self.y0)
    }

    pub fn to_value(self) -> CValue {
        obj([
            ("x0", CValue::Str(self.x0.to_string())),
            ("x1", CValue::Str(self.x1.to_string())),
            ("y0", CValue::Str(self.y0.to_string())),
            ("y1", CValue::Str(self.y1.to_string())),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Self, AutoTextError> {
        let CValue::Obj(m) = v else {
            return Err(AutoTextError::BadParams("rect must be an object"));
        };
        let x0 = req_i64(m, "x0")?;
        let y0 = req_i64(m, "y0")?;
        let x1 = req_i64(m, "x1")?;
        let y1 = req_i64(m, "y1")?;
        Ok(SheetRect::new(x0, y0, x1, y1))
    }
}

/// Classification of a sheet's content (`REQ-003`, `Design`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKind {
    Vector,
    Mixed,
    RasterOnly,
}

impl PageKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            PageKind::Vector => "vector",
            PageKind::Mixed => "mixed",
            PageKind::RasterOnly => "raster_only",
        }
    }
}

/// One extracted visible vector text run (`REQ-001`, `Design: Data structures`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRun {
    /// Deterministic 1-based sequence ID assigned after sorting by `(y0, x0, raw_order)`.
    pub id: u32,
    /// Extracted UTF-8 string typed as `Untrusted<Text>` (CONSTITUTION C6).
    pub text: Untrusted<Text>,
    /// Bounding box in integer `0.01 mm` sheet coordinates (top-left origin).
    pub bbox: SheetRect,
    /// Font height in integer `0.01 mm`.
    pub font_h_cmm: i64,
    /// Counter-clockwise text baseline angle in integer millidegrees (`0..360_000`).
    pub angle_mdeg: i32,
}

impl TextRun {
    pub fn to_value(&self) -> CValue {
        obj([
            ("angle_mdeg", CValue::Str(self.angle_mdeg.to_string())),
            ("bbox", self.bbox.to_value()),
            ("font_h_cmm", CValue::Str(self.font_h_cmm.to_string())),
            ("id", CValue::Str(self.id.to_string())),
            ("text", self.text.to_value()),
        ])
    }
}

/// Complete deterministic extraction output of `report.extract_drawing_text@1` (`Design`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawingTextMap {
    pub sheet: SheetId,
    pub page_kind: PageKind,
    pub runs: Vec<TextRun>,
    pub frame_boxes: Vec<SheetRect>,
    pub warnings: Vec<String>,
}

impl DrawingTextMap {
    pub fn to_value(&self) -> CValue {
        obj([
            ("frame_boxes", CValue::Arr(self.frame_boxes.iter().map(|b| b.to_value()).collect())),
            ("page_kind", CValue::Str(self.page_kind.as_str().into())),
            ("runs", CValue::Arr(self.runs.iter().map(TextRun::to_value).collect())),
            ("sheet", CValue::Str(self.sheet.to_string())),
            ("warnings", CValue::Arr(self.warnings.iter().map(|w| CValue::Str(w.clone())).collect())),
        ])
    }
}

/// Bounded RGBA8 display tile / cropped snippet returned for Proposal Inbox previews (`REQ-005`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayTile {
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
    pub crop: SheetRect,
    /// RGBA8 pixel buffer (`width * height * 4` bytes).
    pub rgba: Vec<u8>,
}

fn req_i64(m: &BTreeMap<String, CValue>, k: &str) -> Result<i64, AutoTextError> {
    match m.get(k) {
        Some(CValue::Str(s)) => s.parse::<i64>().map_err(|_| AutoTextError::BadParams("invalid integer field")),
        _ => Err(AutoTextError::BadParams("missing integer field")),
    }
}
