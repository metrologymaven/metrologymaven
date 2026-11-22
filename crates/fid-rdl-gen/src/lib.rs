//! Tier-G Reference Data Library generator foundation (spec node `UNC.RDL.GEN.HPREC`).
//!
//! Provides pure-Rust 256-bit (~77 decimal digits) deterministic floating-point arithmetic
//! ([`hp::Hp`]), exact 16-significant-digit ties-to-even rounding ([`hp::round16`]),
//! high-precision Gauss-Newton refinement with Householder QR ([`hp::hp_refine`]), seeded
//! geometry draws via `fid_num::rng::Cursor` (`KRN.NUM.RNG`), and canonical
//! [`hp::GeneratorRecord`] + [`hp::ReferenceValue`] formatting.
//!
//! Intentionally depends on **no** kernel fit or solver crate (REQ-UNC.RDL.GEN.HPREC-004).

pub mod hp;

pub use hp::{
    geometry_rng, hp_refine, hp_uniform, round16, Decimal16, GeneratorRecord, Hp, HpCert, HpError, ReferenceValue,
    MAX_REFINE_ITERATIONS, MIN_REFERENCE_DIGITS, PRECISION_BITS,
};
