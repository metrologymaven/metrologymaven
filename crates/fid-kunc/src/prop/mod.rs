//! Propagation engine functions. Spec node `UNC.PROP`.
//!
//! Only the kernel-side (L0) engine functions live here: pure, single-threaded and I/O-free
//! (KNP-1). Scheduling, storage and commands are in the L2 command crate.
//!
//! Implemented leaves: `UNC.PROP.MCM.BLOCK` ([`mcm::block`]), `UNC.PROP.MCM.INTVL`
//! ([`mcm::interval`]), `UNC.PROP.MCM.ADAPT` ([`mcm::adapt`]) and `UNC.PROP.MCM.SAMP`
//! ([`mcm::samples`]).

pub mod mcm;
