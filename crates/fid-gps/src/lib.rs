//! The GD&T evaluation library of Metrology Maven (CONSTITUTION §2.1 L0: `fid-gps`). It grows
//! one spec leaf at a time; today it holds:
//!
//! - [`dmt`] (`GDT.RES.SCHEMA`): the Dialect Mapping Table schema (D-009): row keys, rows,
//!   operator chains in the ISO 17450 operator vocabulary, the versioned table container,
//!   its strict bounded loader, the row, chain and table digests, the exported JSON Schema
//!   and the defaults-profile key vocabulary.
//!
//! Pure: no I/O (callers pass bytes), no floating-point arithmetic, no `HashMap`/`HashSet`,
//! and nothing here panics on any input: every outcome is a returned `Result`.

pub mod dmt;
