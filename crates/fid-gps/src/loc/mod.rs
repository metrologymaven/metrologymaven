//! Location, orientation and run-out support (spec node `GDT.LOC`). Today it holds:
//!
//! - [`derive`] (`GDT.LOC.DERIV.TYPES`): the derived-element type (axis, median line, median
//!   surface, median points) with the producing algorithm ID, the `derive()` dispatch on the
//!   chain's reduction operator through a registry keyed by algorithm ID, and the typed
//!   derivation failure reasons.

pub mod derive;
