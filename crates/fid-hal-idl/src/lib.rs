//! Device interface definitions of Metrology Maven (spec node `HAL.IDL`). Every capability
//! package, driver and simulator builds against these types. Modules land one spec leaf at a
//! time; see `spec/hal/idl/`.
//!
//! - [`units`]: SI quantities, declared exact encodings, frames and timestamp types
//!   (`HAL.IDL.UNITS`).

pub mod units;

pub use units::{
    EncodingDescriptor, Frame, FrameEdge, FrameGraph, FrameId, FrameKind, NumericType, Quantity, RawValue, SiUnit,
    TimeSource, Timestamp, UnitVector3, UnitsError,
};
