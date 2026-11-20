//! Kernel optimisation drivers for Metrology Maven. Pure Rust, `f64`, single-threaded, no
//! I/O, no `unsafe`; floating-point primitives only through `fid-num` (KNP-1,
//! CONSTITUTION §4, REQ-KRN.SOLV-001).
//!
//! Modules land one spec leaf at a time; see `spec/krn/solv/`.

pub mod cert;
pub mod conic;
pub mod mstart;
pub mod nlls;
pub mod seq;
