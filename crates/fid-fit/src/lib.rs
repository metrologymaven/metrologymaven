//! Kernel fitting for Metrology Maven. Pure Rust, `f64`, single-threaded, no I/O, no
//! `unsafe`; floating-point primitives only through `fid-num` (KNP-1, CONSTITUTION §4).
//!
//! Modules land one spec leaf at a time; see `spec/krn/lsq/`. This crate starts with the
//! shared fit contract ([`contract`], spec node `KRN.LSQ.TRAIT`) that every fitting method
//! (least squares, minimum zone, envelope) implements, and the closed-form least-squares point,
//! line and plane fits ([`ls_lin`], spec node `KRN.LSQ.LIN`). [`tan_rock`] (spec node
//! `KRN.ENV.TAN.ROCK`) holds the tangential variant table and the rocking report shared by the
//! tangential fits; [`tan_plane`] (spec node `KRN.ENV.TAN.PLANE`) is the tangential plane in
//! its three variants. [`init`] (spec node `KRN.LSQ.INIT`) builds the deterministic starts of the
//! axial fits (cylinder, cone, torus), and [`ls_cyl`] (spec node `KRN.LSQ.AXIS.CYL`) is the
//! least-squares cylinder run from those starts. [`mcc_cyl`] (spec node `KRN.ENV.MCC.CYL`) is the
//! certified minimum circumscribed cylinder. [`ls_rnd`] (spec node `KRN.LSQ.RND`) holds the
//! least-squares 2D circle, 3D circle and sphere. [`ls_cone`] (spec node `KRN.LSQ.AXIS.CONE`)
//! is the least-squares cone, run from INIT's cone starts. [`plan2d_assign`] (spec node
//! `KRN.LSQ.PLAN2D.ASSIGN`) provides deterministic segment assignment and per-segment
//! residual statistics for compound planar profiles (slots, rounded rectangles).

// `FitError` wraps `fid_solv::nlls::SolverError` by value (about 150 bytes, see the note in
// `fid-solv`). It is returned at most once per fit, never in a loop, and boxing it would add an
// infallible allocation on the error path, so the size lint is allowed for this crate.
#![allow(clippy::result_large_err)]

pub mod contract;
pub mod init;
pub mod ls_cone;
pub mod ls_cyl;
pub mod ls_lin;
pub mod ls_rnd;
pub mod mcc_cyl;
pub mod mz_flat;
pub mod mz_line3;
pub mod plan2d_assign;
pub mod tan_plane;
pub mod tan_rock;
pub mod vor;
