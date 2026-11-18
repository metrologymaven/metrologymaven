//! Kernel numerics for Metrology Maven. Pure Rust, `f64`, no I/O, no `unsafe`,
//! no dependencies (KNP-1, CONSTITUTION §4).
//!
//! Modules land one spec leaf at a time; see `spec/krn/num/`.

pub mod err;
pub mod rng;

pub use err::{check_finite, check_finite_slice, probe_fp_env, NumError};
