//! `fid-view`: view state and transient camera navigation for Metrology Maven (spec node
//! `VIZ.VIEW`).
//!
//! - [`state`] (spec leaf `VIZ.VIEW.STATE`): the single serialisable description of what a
//!   viewport shows, with validation, its RFC 8785 canonical form and its SHA-256 digest.
//! - [`nav`] (spec leaf `VIZ.VIEW.CAMERA.NAV`): transient camera navigation (`CameraController`),
//!   standard orientations relative to the active `GEO.FRAME` frame, deterministic f64 fit-to-view,
//!   reduced-motion transitions and keyboard palette bindings.
//!
//! The crate is side-effect class R0 and determinism class D0 for end cameras: no I/O, no clock,
//! no randomness, no hash maps. Nothing here panics (CONSTITUTION §8).

pub mod nav;
pub mod state;
