//! Pinned pure-Rust libm port (fdlibm / musl algorithms), generic path only.
//!
//! Provenance, licence notices and every project change are in `VENDOR.md` next to this
//! file. Rules for this tree: plain IEEE-754 binary64 `+ - * /` and `f64::sqrt`
//! (correctly rounded) only; no `mul_add`, no intrinsics, no `cfg(target_arch)`, no
//! platform libm. The facade (`super`) rejects NaN, infinities and out-of-domain arguments
//! before calling in here, so the special-value branches of the C originals are omitted.

mod exp_log;
mod hypot_cbrt;
mod inv_trig;
mod rem_pio2;
mod trig;

pub(crate) use exp_log::{exp, log};
pub(crate) use hypot_cbrt::{cbrt, hypot};
pub(crate) use inv_trig::{acos, asin, atan, atan2};
pub(crate) use trig::{cos, sin, tan};

/// Version of this port. Bump it whenever any output bit can change.
pub const VENDOR_VERSION: &str = "fid-num-libm 1";

/// High 32 bits of the representation.
fn hi_word(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32
}

/// `x` with the low 32 bits of its representation cleared.
fn with_lo_zero(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & 0xFFFF_FFFF_0000_0000)
}

/// 2^k for -1022 <= k <= 1023 (callers stay in range).
fn pow2(k: i32) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
}
