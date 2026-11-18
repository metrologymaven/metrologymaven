// Licence notices of the upstream sources this file is ported from (kept as required):
//
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunPro, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================
//
// musl libc, src/math (MIT licence):
//
// Copyright © 2005-2020 Rich Felker, et al.
//
// Permission is hereby granted, free of charge, to any person obtaining
// a copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
// TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
// SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

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
