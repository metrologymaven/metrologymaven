//! Standalone verifier replay of Metrology Maven Result Packages (spec node `TRUST.REPLAY`).
//!
//! Modules land one spec leaf at a time:
//! - [`compare`]: replay outcome classification and package aggregation
//!   (`TRUST.REPLAY.COMPARE`);
//! - [`select`]: replayable-set selection, the replay plan and `NOT_REPLAYABLE` reasons
//!   (`TRUST.REPLAY.SELECT`).

pub mod compare;
pub mod select;
