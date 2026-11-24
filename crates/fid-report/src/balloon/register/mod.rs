//! Balloon register and renumbering (`RPT.BALLOON.REG`).
//!
//! - `report.add_balloon@1`, `report.move_balloon@1`, `report.renumber_balloon@1` (modes
//!   `set`, `insert`, `compact`), `report.relink_balloon@1`, `report.remove_balloon@1`
//!   (all `W2`, one journal event each; a failed command journals nothing) and
//!   `report.list_balloon@1` (`R0`, ascending number).
//! - Balloon IDs are UUIDv7 made by the handler, read back from the journal on replay and
//!   never reused. Numbers are unique per drawing revision; a characteristic has at most one
//!   balloon per drawing revision; renumbering never changes IDs or links.
//! - Positions are integer `0.01 mm` sheet coordinates checked against [`SheetInfo`] bounds
//!   (`RPT.BALLOON.SHEET`); the zone is untrusted text of at most 16 bytes.
//!
//! [`SheetInfo`]: crate::balloon::sheet::SheetInfo

pub mod command;
pub mod defs;
pub mod error;
pub mod event;
pub mod model;
pub mod replay;
pub mod store;

pub use command::{execute, RenumberBalloonParams};
pub use defs::command_defs;
pub use error::RegisterError;
pub use event::{
    AddEvent, BalloonEvent, ADD_BALLOON, LIST_BALLOON, MOVE_BALLOON, RELINK_BALLOON, REMOVE_BALLOON, RENUMBER_BALLOON,
};
pub use model::{
    changes_value, list_value, parse_zone, Balloon, CharacteristicIndex, RenumberChange, RenumberMode, SheetPoint,
    MAX_ZONE_BYTES,
};
pub use replay::{replay_exact, BalloonReplayRun};
pub use store::{AddBalloonParams, BalloonRegister, MoveBalloonParams, RegisterEnv};
