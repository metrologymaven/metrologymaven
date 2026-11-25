//! Sensor capability packages (spec node `HAL.IDL.SENS`). Modules land one spec leaf at a
//! time; see `spec/hal/idl/sens/`.
//!
//! - [`stream`]: the shared streaming template every sensor package is built with, the
//!   vendor-extension rule and the `Stream-common@1` conformance profile
//!   (`HAL.IDL.SENS.STREAM`).
//! - [`linescan`]: the `LineScan@1` package, its columnar profile batches and the
//!   `LineScan-core@1` conformance profile (`HAL.IDL.SENS.LINE`).

pub mod linescan;
pub mod stream;
