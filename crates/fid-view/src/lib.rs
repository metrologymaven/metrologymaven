//! `fid-view`: view state for Metrology Maven (spec node `VIZ.VIEW`).
//!
//! So far: the [`state`] module (spec leaf `VIZ.VIEW.STATE`), the single serialisable description
//! of what a viewport shows, with validation, its RFC 8785 canonical form and its SHA-256 digest.
//! The crate is side-effect class R0 and determinism class D0: no I/O, no clock, no randomness,
//! no hash maps. Nothing here panics (CONSTITUTION §8).

pub mod state;
