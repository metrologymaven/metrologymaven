//! Propagation engine functions. Spec node `UNC.PROP`.
//!
//! Only the kernel-side (L0) engine functions live here: pure, single-threaded and I/O-free
//! (KNP-1). Scheduling, storage and commands are in the L2 command crate.
//!
//! Implemented leaves: `UNC.PROP.MCM.BLOCK` ([`mcm::block`]), `UNC.PROP.MCM.INTVL`
//! ([`mcm::interval`]), `UNC.PROP.MCM.ADAPT` ([`mcm::adapt`]), `UNC.PROP.MCM.SAMP`
//! ([`mcm::samples`]) and `UNC.PROP.VALID` ([`valid`], [`validate_lpu`]).

pub mod mcm;
pub mod valid;

pub use valid::{
    delta_for_u, validate_lpu, ValidateError, ValidateInput, ValidateOutput, ALGORITHM_ID as VALIDATE_LPU_ALGORITHM_ID,
    DEFAULT_N_DIG,
};
