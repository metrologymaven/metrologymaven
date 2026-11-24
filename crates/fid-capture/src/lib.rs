//! `fid-capture`: screenshots, recordings and their governance (spec domain `VIZ.CAPTURE`).
//! Viewer layer.
//!
//! - [`rec::policy`]: the org recording-policy settings and the fail-closed evaluation every
//!   capture medium asks before it captures anything (`VIZ.CAPTURE.REC.POLICY`).
//!
//! Nothing in this crate sends anything off the machine (CONSTITUTION §3.1).

#![deny(clippy::todo, clippy::unimplemented, clippy::unreachable)]

pub mod rec;
