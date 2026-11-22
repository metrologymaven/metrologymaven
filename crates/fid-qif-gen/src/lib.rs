//! Build-time QIF 3.0 code generator (spec node IO.QIF.GEN).
//!
//! Implemented so far:
//! - [`pin`]: the pinned XSD manifest and digest check (IO.QIF.GEN.PIN). Nothing downstream
//!   reads a schema file that [`pin::PinnedSet::verify`] has not checked.
//! - [`parse`]: streaming XSD 1.0 parser and Raw Schema IR (IO.QIF.GEN.PARSE), built on
//!   `fid-xml` under `fid-io-reg` limits.
//!
//! - [`flat`]: Schema IR normalisation, anonymous-type naming, substitution groups, derived
//!   count rules, identity tables, and `qif-ir.json` canonical output (IO.QIF.GEN.FLAT).
//!
//! The real DMSC QIF 3.0 manifest is not committed yet: see `xsd/qif3/README.md`.

pub mod flat;
pub mod parse;
pub mod pin;
