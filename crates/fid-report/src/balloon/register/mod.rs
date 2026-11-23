//! Balloon register and renumbering (`RPT.BALLOON.REG`).
//!
//! - Commands: `report.add_balloon@1`, `report.move_balloon@1`, `report.renumber_balloon@1`
//!   (`set`, `insert`, `compact`), `report.relink_balloon@1`, `report.remove_balloon@1` (`W2`)
//!   and `report.list_balloon@1` (`R0`, ascending number order).
//! - Pure integer `SheetPoint` (`0.01 mm` top-left origin), `BTreeMap`-only indexes, permanent
//!   ID retirement on removal, one-balloon-per-characteristic invariant, and exact replay.

pub mod defs;
pub mod error;
pub mod event;
pub mod model;
pub mod store;

pub use defs::{command_defs, execute};
pub use error::RegisterError;
pub use event::{
    AddBalloonEvent, BalloonEvent, ADD_BALLOON, LIST_BALLOON, MOVE_BALLOON, RELINK_BALLOON, REMOVE_BALLOON,
    RENUMBER_BALLOON,
};
pub use model::{parse_zone, Balloon, RenumberChange, RenumberMode, SheetPoint, MAX_ZONE_BYTES};
pub use store::{
    replay_exact, AddBalloonParams, BalloonRegister, BalloonReplayRun, MoveBalloonParams, RegisterBalloonSink,
    RelinkBalloonParams, RenumberBalloonParams,
};
