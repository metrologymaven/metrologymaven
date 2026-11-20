//! Kernel fitting for Metrology Maven. Pure Rust, `f64`, single-threaded, no I/O, no
//! `unsafe`; floating-point primitives only through `fid-num` (KNP-1, CONSTITUTION §4).
//!
//! Modules land one spec leaf at a time; see `spec/krn/lsq/`. This crate starts with the
//! shared fit contract ([`contract`], spec node `KRN.LSQ.TRAIT`) that every fitting method
//! (least squares, minimum zone, envelope) implements, and the closed-form least-squares point,
//! line and plane fits ([`ls_lin`], spec node `KRN.LSQ.LIN`).

// `FitError` wraps `fid_solv::nlls::SolverError` by value (about 150 bytes, see the note in
// `fid-solv`). It is returned at most once per fit, never in a loop, and boxing it would add an
// infallible allocation on the error path, so the size lint is allowed for this crate.
#![allow(clippy::result_large_err)]

pub mod contract;
pub mod ls_lin;
