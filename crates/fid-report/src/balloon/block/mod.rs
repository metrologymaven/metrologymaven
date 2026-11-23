//! Ballooned-drawing report block (`RPT.BALLOON.BLOCK`, `REQ-001`..`REQ-007`, `AC-1`..`AC-6`).
//!
//! - Block kind `drawing` (version 1) registered with [`crate::doc::KindRegistry`] via
//!   [`DrawingBlockKind`] / [`register_drawing_kind`].
//! - Command `report.set_drawing_balloon@1` (`W2`, one journal event, `--dry-run`).
//! - Pure integer rational resolver [`resolve`] emitting vector [`BalloonGlyph`] primitives
//!   separate from the sheet image, reading balloon numbers live from the register at the
//!   generation head, and marking outcomes through `RPT.MODEL.FMT` and `design/tokens/status.json`.

pub mod command;
pub mod kind;
pub mod model;
pub mod resolve;

pub use command::{set_drawing_balloon, set_drawing_balloon_def, SetDrawingBalloonParams};
pub use kind::{register_drawing_kind, DrawingBlockKind};
pub use model::{
    drawing_kind_id, Affine2Int, BalloonGlyph, BalloonShape, BalloonStyle, BlockFrame, BlockPoint, DrawingBlockContent,
    DrawingBlockError, DrawingPayload, FitMode, OutcomeMark, SheetRect, DRAWING_BLOCK_KIND, DRAWING_BLOCK_VERSION,
    MAX_STYLE_BYTES,
};
pub use resolve::{outcome_mark, resolve, round_half_even_i128};
