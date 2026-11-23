//! Bounded PDF text runs, sheet bounding boxes and display tiles (`RPT.BALLOON.AUTO.TEXT`).
//!
//! - `report.extract_drawing_text@1` (`R1`, D0): extracts visible vector text runs (`Untrusted<Text>`,
//!   top-left origin integer `0.01 mm` `SheetRect`, font height, millidegree angle) and callout
//!   frame boxes from imported PDF drawing sheets, classifying pages as `Vector`, `Mixed`, or
//!   `RasterOnly`.
//! - Bounded display-tile / snippet RGBA8 rasteriser (`render_sheet_tile`, `render_display_tile`)
//!   at up to 300 dpi and 64 Mpx per sheet for Proposal Inbox previews and sheet viewing.
//! - Zero floating-point types or operations, zero `HashMap`, zero panics/unwrap/expect.

pub mod command;
pub mod defs;
pub(crate) mod flate;
pub mod model;
pub mod pdf_extract;
pub mod tile;

pub use command::{
    execute, extract_drawing_page, extract_drawing_text, render_sheet_tile, ExtractDrawingTextParams,
    EXTRACT_DRAWING_TEXT,
};
pub use defs::command_def;
pub use model::{
    AutoTextError, DisplayTile, DrawingTextMap, PageKind, SheetRect, TextRun, DEFAULT_TILE_DPI,
    MAX_TEXT_BYTES_PER_SHEET, MAX_TEXT_RUNS_PER_SHEET, MAX_TILE_DPI, MAX_TILE_PIXELS_PER_SHEET,
};
pub use pdf_extract::{extract_pdf_page, ExtractedPage};
pub use tile::{render_display_tile, SheetSegment};
