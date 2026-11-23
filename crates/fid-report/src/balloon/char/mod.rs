//! Characteristics from balloons (`RPT.BALLOON.CHAR`).
//!
//! Atomic composite `W2` command `report.add_char_balloon@1` creating a drawing-sourced GDT
//! characteristic (with exact authored decimal literals and one-rounding SI conversion via
//! `unit.authored_to_si@1`) and placing its linked balloon in one journaled transaction.

pub mod model;
pub mod store;

pub use model::{
    AuthoredQuantity, AuthoredUnit, CharBalloonError, Dialect, DrawingCharInput, DrawingCharKind,
    DrawingCharacteristic, MAX_CHAR_TEXT_BYTES, UNIT_CONVERSION_ALGO,
};
pub use store::{
    command_def, make_char_input_from_literals, replay_exact, require_dialect, AddCharBalloonParams,
    AddCharBalloonResult, CharBalloonEvent, CharBalloonReplayRun, CharBalloonStore, ADD_CHAR_BALLOON,
};
