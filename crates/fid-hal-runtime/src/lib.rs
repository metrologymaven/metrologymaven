//! Device runtime and acquisition manager of Metrology Maven (spec node `HAL.ACQ`). Modules
//! land one spec leaf at a time; see `spec/hal/acq/`.
//!
//! - [`conv`]: deterministic conversion of declared encodings to f64 SI values
//!   (`HAL.ACQ.CONV`).
//! - [`time`]: session timebase, clock offset and drift estimation, and the
//!   `time_sync_insufficient` fusion rule (`HAL.ACQ.TIME`).
//! - [`registry`]: the device registry of serving sources and their kinds, and
//!   `device.show_devices@1` (`HAL.ACQ.SESS.REG`).
//! - [`ingest`]: data-plane ingest; so far the credit ledger for flow control
//!   (`HAL.ACQ.INGEST.CREDIT`).

pub mod conv;
pub mod ingest;
pub mod registry;
pub mod time;

pub use conv::{decode, ConvError, Decoded, RawArray, ValidityMask, MAX_EXACT_INT};
