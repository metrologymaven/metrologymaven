//! Standalone verifier replay of Metrology Maven Result Packages (spec node `TRUST.REPLAY`).
//!
//! Modules land one spec leaf at a time:
//! - [`compare`]: replay outcome classification and package aggregation
//!   (`TRUST.REPLAY.COMPARE`);
//! - [`select`]: replayable-set selection, the replay plan and `NOT_REPLAYABLE` reasons
//!   (`TRUST.REPLAY.SELECT`);
//! - [`kart`]: offline kernel artifact lookup chain (`package -> bundled -> cache`,
//!   `TRUST.REPLAY.KART.LOOKUP`).

pub mod compare;
pub mod kart;
pub mod select;
