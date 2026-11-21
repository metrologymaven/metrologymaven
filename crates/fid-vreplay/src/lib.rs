//! Standalone verifier replay of Metrology Maven Result Packages (spec node `TRUST.REPLAY`).
//!
//! Modules land one spec leaf at a time:
//! - [`compare`]: replay outcome classification and package aggregation
//!   (`TRUST.REPLAY.COMPARE`).

pub mod compare;
