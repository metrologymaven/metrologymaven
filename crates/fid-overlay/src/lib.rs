//! `fid-overlay`: what the viewer draws on top of geometry (spec domain `VIZ.OVERLAY`). Viewer
//! layer; no GPU, windowing or UI-toolkit dependency.
//!
//! - [`status`]: status markers, the locked status-palette guard, provenance badges and the
//!   out-of-tolerance pattern definitions (`VIZ.OVERLAY.STATUS`).

pub mod status;
