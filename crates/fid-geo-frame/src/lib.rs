//! Coordinate frames, transforms and units for Metrology Maven (spec node `GEO.FRAME`).
//!
//! Pure Rust, `f64`, no I/O, no `unsafe`, kernel graph (KNP-1, CONSTITUTION §4). Modules land
//! one spec leaf at a time:
//!
//! - [`units`]: authored lengths and angles, converted to SI metres or radians with exactly
//!   one rounding and back to display units with exactly one rounding (`GEO.FRAME.UNITS`).

mod bigint;
pub mod units;

pub use units::{AngleUnit, AuthoredQuantity, LengthUnit, Unit, UnitsError};
