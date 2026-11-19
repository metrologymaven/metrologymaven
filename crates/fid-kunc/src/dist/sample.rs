//! Deterministic seeded sampling (REQ-UNC.MODEL.DIST-004, -006).
//!
//! # RNG addressing (pinned)
//!
//! Every draw is one Philox4x32-10 block `fid_num::rng::block(seed, stream, index)`, words
//! `w0 .. w3`. A user stream must be below [`STREAM_LIMIT`] = `2^62`; the two top stream
//! bits are reserved for sub-streams:
//!
//! - **Primary draw** of sample `i` of stream `s`: block `(seed, s, i)`, uniform from
//!   `(w1, w0)`. This is the same 64-bit value as `fid_num::rng::Cursor::next_u64` at `i`.
//! - **Gamma sub-stream** (scaled-t): attempt `a` (`0 <= a <` [`GAMMA_ATTEMPTS`] `= 256`) of
//!   sample `i` is block `(seed, s | 2^63, i * 256 + a)`; the normal of the attempt comes
//!   from `(w1, w0)` and the acceptance uniform from `(w3, w2)`. Requires
//!   `i <=` [`MAX_GAMMA_INDEX`] `= 2^56 - 1`.
//! - **Multivariate sub-stream**: standard normal `k` of sample `i` of a multivariate normal
//!   is block `(seed, s | 2^62, i * 4096 + k)`, uniform from `(w1, w0)`; `k < 4096` (the
//!   `fid-num` matrix limit). Requires `i <=` [`MAX_MV_INDEX`] `= 2^52 - 1`.
//!
//! An address outside these limits is `invalid_parameter{stream}` or
//! `invalid_parameter{index}`. Sample `i` therefore depends on `(seed, s, i)` only.
//!
//! # Uniform on the open interval
//!
//! [`open_unit`]: with `x = hi << 32 | lo` and `k = x >> 12` (top 52 bits),
//! `u = (2k + 1) * 2^-53`, exactly. `u` lies in `[2^-53, 1 - 2^-53]`, is never 0 or 1, and
//! `1 - u` is exact.
//!
//! # Transforms (pinned operation order; no FMA)
//!
//! - normal: `mu + sigma * ppnd16(u)`.
//! - rectangular: `a + (b - a) * u`.
//! - triangular: `w = b - a`, `fc = (c - a) / w`; if `u < fc`:
//!   `a + sqrt((u * w) * (c - a))`, else `b - sqrt(((1 - u) * w) * (b - c))`.
//! - arcsine: `h = (b - a) * 0.5`, `m = a + h`, `m - h * cos(pi * u)`.
//! - trapezoidal: `h = (b - a) * 0.5`, `m = a + h`, `f1 = (1 - beta) / (2 * (1 + beta))`,
//!   `w = (1 - beta) * (1 + beta)`; `y = -1 + sqrt((2 * u) * w)` if `u < f1`,
//!   `y = (u - 0.5) * (1 + beta)` if `u <= 1 - f1`, else `y = 1 - sqrt((2 * (1 - u)) * w)`;
//!   result `m + h * y`.
//! - scaled-t: `z = ppnd16(u)` from the primary draw, `g` from the gamma sampler with shape
//!   `alpha = nu * 0.5`, `w = (2 * g) / nu`, result `mu + s * (z / sqrt(w))`.
//! - gamma (Marsaglia and Tsang 2000, shape `alpha > 1`): `d = alpha - 1/3`,
//!   `c = 1 / sqrt(9 * d)`; per attempt `x = ppnd16(u_a)`, `t = 1 + c * x`; reject if
//!   `t <= 0`; `v = (t * t) * t`, `x2 = x * x`; accept `d * v` if
//!   `u_b < 1 - 0.0331 * (x2 * x2)` or `ln(u_b) < 0.5 * x2 + d * ((1 - v) + ln(v))`.
//!   All 256 attempts rejected is `sampling_exhausted`.
//!
//! `sqrt`, `ln` and `cos` are `fid_num::f`; `pi` is `core::f64::consts::PI`.

use super::normal::ppnd16;
use super::{DistError, Kind, Params};
use fid_num::{f, rng};

/// User streams must be below `2^62`.
pub const STREAM_LIMIT: u64 = 1 << 62;
/// The stream bit of the gamma sub-stream.
pub const GAMMA_STREAM_BIT: u64 = 1 << 63;
/// The stream bit of the multivariate sub-stream.
pub const MV_STREAM_BIT: u64 = 1 << 62;
/// Rejection attempts per scaled-t sample.
pub const GAMMA_ATTEMPTS: u64 = 256;
const GAMMA_SHIFT: u32 = 8;
/// Largest scaled-t sample index.
pub const MAX_GAMMA_INDEX: u64 = (1 << (64 - GAMMA_SHIFT)) - 1;
pub(crate) const MV_SHIFT: u32 = 12;
/// Largest multivariate sample index.
pub const MAX_MV_INDEX: u64 = (1 << (64 - MV_SHIFT)) - 1;

/// 2^-53, exact.
const TWO_POW_M53: f64 = 1.0 / 9_007_199_254_740_992.0;

/// The open-interval uniform of two RNG words (`hi << 32 | lo`), see the module docs.
pub fn open_unit(hi: u32, lo: u32) -> f64 {
    let x = (u64::from(hi) << 32) | u64::from(lo);
    ((x >> 12) * 2 + 1) as f64 * TWO_POW_M53
}

/// Checks a stream and the largest index of a request.
pub(crate) fn check_address(stream: u64, last_index: u64, kind: Kind) -> Result<(), DistError> {
    if stream >= STREAM_LIMIT {
        return Err(DistError::InvalidParameter { name: "stream" });
    }
    let max = match kind {
        Kind::ScaledT => MAX_GAMMA_INDEX,
        Kind::MvNormal => MAX_MV_INDEX,
        _ => u64::MAX,
    };
    if last_index > max {
        return Err(DistError::InvalidParameter { name: "index" });
    }
    Ok(())
}

fn primary(seed: u64, stream: u64, index: u64) -> f64 {
    let w = rng::block(seed, stream, index);
    open_unit(w[1], w[0])
}

/// Standard normal `k` of multivariate sample `index` (caller checked the limits).
pub(crate) fn mv_normal(seed: u64, stream: u64, index: u64, k: usize) -> Result<f64, DistError> {
    let w = rng::block(seed, stream | MV_STREAM_BIT, (index << MV_SHIFT) | k as u64);
    Ok(ppnd16(open_unit(w[1], w[0]))?)
}

/// Marsaglia-Tsang gamma variate of shape `alpha = nu / 2 > 1`, scale 1.
fn gamma(seed: u64, stream: u64, index: u64, alpha: f64) -> Result<f64, DistError> {
    let d = alpha - 1.0 / 3.0;
    let c = 1.0 / f::sqrt(9.0 * d)?;
    for a in 0..GAMMA_ATTEMPTS {
        let w = rng::block(seed, stream | GAMMA_STREAM_BIT, (index << GAMMA_SHIFT) | a);
        let x = ppnd16(open_unit(w[1], w[0]))?;
        let ub = open_unit(w[3], w[2]);
        let t = 1.0 + c * x;
        if t <= 0.0 {
            continue;
        }
        let v = (t * t) * t;
        let x2 = x * x;
        if ub < 1.0 - 0.0331 * (x2 * x2) {
            return Ok(d * v);
        }
        if f::ln(ub)? < 0.5 * x2 + d * ((1.0 - v) + f::ln(v)?) {
            return Ok(d * v);
        }
    }
    Err(DistError::SamplingExhausted { index })
}

/// One univariate sample (caller checked the address limits).
pub(crate) fn draw(p: &Params, seed: u64, stream: u64, index: u64) -> Result<f64, DistError> {
    let u = primary(seed, stream, index);
    let x = match *p {
        Params::Normal { mu, sigma } => mu + sigma * ppnd16(u)?,
        Params::Rect { a, b } => a + (b - a) * u,
        Params::Tri { a, b, c } => {
            let w = b - a;
            let fc = (c - a) / w;
            if u < fc {
                a + f::sqrt((u * w) * (c - a))?
            } else {
                b - f::sqrt(((1.0 - u) * w) * (b - c))?
            }
        }
        Params::Arcsine { a, b } => {
            let h = (b - a) * 0.5;
            let m = a + h;
            m - h * f::cos(core::f64::consts::PI * u)?
        }
        Params::Trap { a, b, beta } => {
            let h = (b - a) * 0.5;
            let m = a + h;
            let f1 = (1.0 - beta) / (2.0 * (1.0 + beta));
            let w = (1.0 - beta) * (1.0 + beta);
            let y = if u < f1 {
                -1.0 + f::sqrt((2.0 * u) * w)?
            } else if u <= 1.0 - f1 {
                (u - 0.5) * (1.0 + beta)
            } else {
                1.0 - f::sqrt((2.0 * (1.0 - u)) * w)?
            };
            m + h * y
        }
        Params::ScaledT { mu, s, nu } => {
            let z = ppnd16(u)?;
            let g = gamma(seed, stream, index, nu * 0.5)?;
            let w = (2.0 * g) / nu;
            mu + s * (z / f::sqrt(w)?)
        }
    };
    Ok(fid_num::check_finite("unc.dist.sample", x)?)
}
