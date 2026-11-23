//! Kernel constructions for Metrology Maven. Pure Rust, `f64`, single-threaded, no I/O, no
//! `unsafe`; floating-point primitives only through `fid-num` (KNP-1, CONSTITUTION §4).
//!
//! Modules land one spec leaf at a time; see `spec/krn/cons/`. This crate starts with the
//! construction contract ([`contract`], spec node `KRN.CONS.DECL`) that every construction
//! (operations, element builders, derived elements, relationships) implements.

pub mod contract;
pub mod ops_mid;
pub mod ops_offs;
