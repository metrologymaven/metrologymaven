//! `fid-doctor`: environment diagnostics (domain `PLAT.DOCTOR`).
//!
//! - [`probe`]: offline, bounded environment probes behind the [`probe::Env`] trait,
//!   [`probe::RealEnv`] for Tier 1 OSes, [`probe::FakeEnv`] fed from recorded JSON snapshots,
//!   and the [`probe::EnvSnapshot`] schema (spec node `PLAT.DOCTOR.PROBE`).
//!
//! Constraints: CONSTITUTION §8 (no `unsafe`, no panics, bounded inputs), C7 (offline, zero
//! network requests, no user name / host name / serial numbers / MAC addresses).

#![forbid(unsafe_code)]

pub mod probe;
