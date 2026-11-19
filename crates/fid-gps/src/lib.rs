//! The GD&T evaluation library of Metrology Maven (CONSTITUTION §2.1 L0: `fid-gps`). It grows
//! one spec leaf at a time; today it holds:
//!
//! - [`dmt`] (`GDT.RES.SCHEMA`): the Dialect Mapping Table schema (D-009): row keys, rows,
//!   operator chains in the ISO 17450 operator vocabulary, the versioned table container,
//!   its strict bounded loader, the row, chain and table digests, the exported JSON Schema
//!   and the defaults-profile key vocabulary.
//! - [`resolve`] (`GDT.RES.RESOLVE`): the pure interpretation resolver from a characteristic
//!   and its context to a DMT row, operator chain and digests, or `UNRESOLVED` /
//!   `UNSUPPORTED`, and the exact row lookup of `gdt.get_dmt_row@1`.
//! - [`drf`] (`GDT.DRF.BUILD`): DRF construction from simulated datums and DoF bookkeeping.
//!
//! Pure: no I/O (callers pass bytes), no floating-point arithmetic outside [`drf`], no `HashMap`/`HashSet`,
//! and nothing here panics on any input: every outcome is a returned `Result`.

pub mod dmt;
pub mod drf;
pub mod resolve;
