//! Drawing revision import and sheet coordinates (`RPT.BALLOON.SHEET.IMPORT`).
//!
//! - `report.add_drawing_balloon@1` (`W2`): resolves an approved import-root handle
//!   (`IO.REG.ROOTS`), delegates parsing to `fid-io-draw` (`IO.REG.PLUGIN.DRAW`), stores source
//!   and raster pixel blobs by SHA-256 digest (`CORE.STO.CAS`), creates a UUIDv7
//!   [`DrawingRevision`] and ordered [`DrawingSheet`]s in integer `0.01 mm` top-left origin
//!   coordinates, retains reader warnings, and commits one journal event.
//! - `report.list_drawing_balloon@1` (`R0`): lists revisions and sheets ordered by drawing
//!   number, revision label, then ID.
//! - `report.set_size_drawing_balloon@1` (`W2`): states physical width (`0.01 mm`) or DPI on a
//!   `needs_size` raster sheet using exact integer half-even arithmetic (`px × 2540 / dpi`).
//! - Offers [`SheetInfo`] for downstream balloon register and drawing block modules.

pub mod command;
pub mod defs;
pub mod error;
pub mod event;
pub mod model;
pub mod replay;
pub mod store;

pub use command::{
    add_drawing_balloon, execute, list_drawing_balloon, list_drawing_value, set_size_drawing_balloon, AddDrawingParams,
    AddDrawingResult, DeterministicV7Gen, ImportEnv, ListDrawingParams, ListedDrawingRevision, SetSizeParams,
    UuidV7Gen, MAX_FILE_PARAM_BYTES,
};
pub use defs::command_defs;
pub use error::SheetError;
pub use event::{AddDrawingEvent, DrawingEvent, ADD_DRAWING_BALLOON, LIST_DRAWING_BALLOON, SET_SIZE_DRAWING_BALLOON};
pub use model::{
    compute_raster_bounds_cmm, parse_drawing_text, round_half_even_u128, BalloonId, DrawingRevId, DrawingRevision,
    DrawingSheet, ReaderWarning, RevisionId, SheetId, SheetInfo, SheetStatus, SizeSpec, MAX_DPI,
    MAX_DRAWING_TEXT_BYTES, MAX_SHEET_DIM_CMM,
};
pub use replay::{replay_exact, SheetReplayRun};
pub use store::{
    digest_bytes, CasDrawingBlobs, DrawingBlobBackend, DrawingStore, MemoryDrawingBlobs, MAX_CAS_READ_BYTES,
};
