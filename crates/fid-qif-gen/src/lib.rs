//! Build-time QIF 3.0 code generator (spec node IO.QIF.GEN).
//!
//! Implemented so far:
//! - [`pin`]: the pinned XSD manifest and digest check (IO.QIF.GEN.PIN). Nothing downstream
//!   reads a schema file that [`pin::PinnedSet::verify`] has not checked.
//!
//! The real DMSC QIF 3.0 manifest is not committed yet: see `xsd/qif3/README.md`.

pub mod pin;
