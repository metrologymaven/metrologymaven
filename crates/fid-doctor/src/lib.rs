//! `fid-doctor`: environment diagnostics (domain `PLAT.DOCTOR`).
//!
//! - [`probe`]: offline, bounded environment probes behind the [`probe::Env`] trait,
//!   [`probe::RealEnv`] for Tier 1 OSes, [`probe::FakeEnv`] fed from recorded JSON snapshots,
//!   and the [`probe::EnvSnapshot`] schema (spec node `PLAT.DOCTOR.PROBE`).
//! - [`lookup`]: the pure tier and hardware-profile verdicts, [`lookup::assess`], and the
//!   overall outcome with its exit code (spec node `PLAT.DOCTOR.LOOKUP`).
//!
//! Constraints: CONSTITUTION §8 (no `unsafe`, no panics, bounded inputs), C7 (offline, zero
//! network requests, no user name / host name / serial numbers / MAC addresses).

#![forbid(unsafe_code)]

pub mod lookup;
pub mod probe;
