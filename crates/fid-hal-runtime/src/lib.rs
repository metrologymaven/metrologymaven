//! Device runtime and acquisition manager of Metrology Maven (spec node `HAL.ACQ`). Modules
//! land one spec leaf at a time; see `spec/hal/acq/`.
//!
//! - [`conv`]: deterministic conversion of declared encodings to f64 SI values
//!   (`HAL.ACQ.CONV`).
//! - [`time`]: session timebase, clock offset and drift estimation, and the
//!   `time_sync_insufficient` fusion rule (`HAL.ACQ.TIME`).

pub mod conv;
pub mod time;

pub use conv::{decode, ConvError, Decoded, RawArray, ValidityMask, MAX_EXACT_INT};
