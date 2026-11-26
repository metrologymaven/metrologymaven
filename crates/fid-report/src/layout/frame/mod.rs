//! Block frames, move, resize and grid snapping (`RPT.DESIGN.PLACE.FRAME`).
//!
//! - A [`BlockFrame`] is four `i64` counts of `0.01 mm` (`x`, `y`, `w`, `h`) relative to the
//!   page body origin of `RPT.DESIGN.PAGE` (REQ-001). A report's frames are a
//!   [`ReportFrames`] map keyed by block ID, stored beside the report document like the page
//!   setup, never inside it.
//! - `report.move_block_layout@1` (`W2`, one journal event, a failed command journals
//!   nothing; REQ-003, REQ-004) sets a block's frame from absolute coordinates or a delta,
//!   optionally snapped to the grid, and optionally moves the block to another page. It
//!   changes only the frame and, for a page move, the block's page membership: never a
//!   payload, binding, decision or formatter choice (REQ-005, [`payloads_digest`]).
//! - Snapping rounds each edge (left, top, right, bottom) to the nearest multiple of the
//!   grid pitch, ties away from zero, then recomputes `w` and `h` (REQ-002, [`snap_edge`]).
//!   The pitch is the user-scope setting `report.grid_pitch` ([`GridPitch`],
//!   [`report_settings`]): at least `0.1 mm` (10 units), default `1 mm`.
//! - The resulting frame must be at least `1 mm` wide and high (REQ-004); it may lie outside
//!   the body area, which is the overflow check's business (REQ-006).
//! - Integer arithmetic only (D0); no float anywhere (RPT float rule).
//! - Exact replay ([`replay_exact`]) reads one journal of report, page-setup and frame
//!   events in order (REQ-003).

pub mod backend;
pub mod command;
pub mod defs;
pub mod event;
pub mod model;
pub mod pitch;
pub mod replay;

pub use backend::FrameBackend;
pub use command::{
    execute, move_block_layout, parse_signed_length, preview_move_block_layout, FrameTarget, MoveBlockLayout,
    MoveOutcome,
};
pub use defs::command_defs;
pub use event::{is_frame_event, FrameEvent, MOVE_BLOCK_LAYOUT};
pub use model::{payloads_digest, snap_edge, BlockFrame, ReportFrames, MAX_COORD, MAX_SIDE, MIN_SIDE};
pub use pitch::{report_settings, GridPitch, PitchSetting, DEFAULT_PITCH, GRID_PITCH_KEY, MAX_PITCH, MIN_PITCH};
pub use replay::{replay_exact, FrameReplayRun};
