//! Location, orientation and run-out support (spec node `GDT.LOC`). Today it holds:
//!
//! - [`derive`] (`GDT.LOC.DERIV.TYPES`): the derived-element type (axis, median line, median
//!   surface, median points) with the producing algorithm ID, the `derive()` dispatch on the
//!   chain's reduction operator through a registry keyed by algorithm ID, and the typed
//!   derivation failure reasons.
//! - [`derive::axis`] (`GDT.LOC.DERIV.AXIS`): the axis of a cylinder or cone associated with
//!   the `fid-fit` fit the chain names, oriented to the nominal direction and bounded by the
//!   extent of the points, registered in [`derive::DeriveRegistry::builtin`].

pub mod derive;
