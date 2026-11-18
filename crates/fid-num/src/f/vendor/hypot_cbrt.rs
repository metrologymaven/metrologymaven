//! hypot and cbrt: musl `hypot.c` (exact squares by Dekker splitting, no FMA) and
//! `cbrt.c` (FreeBSD/fdlibm). Inputs are finite; hypot returns +inf on overflow and the
//! facade reports that as an error.

use super::pow2;

/// 2^27 + 1, Dekker's splitting constant.
const SPLIT: f64 = 134_217_729.0;

/// x * x = hi + lo exactly (|x| scaled so that neither overflows nor underflows).
fn sq(x: f64) -> (f64, f64) {
    let xc = x * SPLIT;
    let xh = (x - xc) + xc;
    let xl = x - xh;
    let hi = x * x;
    let lo = ((xh * xh - hi) + 2.0 * xh * xl) + xl * xl;
    (hi, lo)
}

pub(crate) fn hypot(x: f64, y: f64) -> f64 {
    let mut ux = x.to_bits() & (u64::MAX >> 1);
    let mut uy = y.to_bits() & (u64::MAX >> 1);
    if ux < uy {
        core::mem::swap(&mut ux, &mut uy);
    }
    let ex = (ux >> 52) as i32;
    let ey = (uy >> 52) as i32;
    let mut x = f64::from_bits(ux);
    let mut y = f64::from_bits(uy);
    if uy == 0 {
        return x;
    }
    if ex - ey > 64 {
        return x + y;
    }
    let mut z = 1.0;
    if ex > 0x3FF + 510 {
        z = pow2(700);
        x *= pow2(-700);
        y *= pow2(-700);
    } else if ey < 0x3FF - 450 {
        z = pow2(-700);
        x *= pow2(700);
        y *= pow2(700);
    }
    let (hx, lx) = sq(x);
    let (hy, ly) = sq(y);
    z * (((ly + lx) + hy) + hx).sqrt()
}

const B1: u32 = 715_094_163; // (1023 - 1023/3 - 0.03306235651) * 2^20
const B2: u32 = 696_219_795; // (1023 - 1023/3 - 54/3 - 0.03306235651) * 2^20
const CP0: f64 = 1.87595182427177; // 0x3FFE03E60F61E692
const CP1: f64 = -1.8849797954337717; // 0xBFFE28E092F02420
const CP2: f64 = 1.6214297201053545; // 0x3FF9F1604A49D6C2
const CP3: f64 = -0.758397934778766; // 0xBFE844CBBEE751D9
const CP4: f64 = 0.14599619288661245; // 0x3FC2B000D4E4EDD7

pub(crate) fn cbrt(x: f64) -> f64 {
    let ux = x.to_bits();
    let mut hx = ((ux >> 32) as u32) & 0x7FFF_FFFF;
    if hx < 0x0010_0000 {
        // zero or subnormal
        hx = (((x * pow2(54)).to_bits() >> 32) as u32) & 0x7FFF_FFFF;
        if hx == 0 {
            return x;
        }
        hx = hx / 3 + B2;
    } else {
        hx = hx / 3 + B1;
    }
    let mut t = f64::from_bits((ux & (1u64 << 63)) | (u64::from(hx) << 32));
    // First approximation to 23 bits, then round t to 21 bits so t*t is exact.
    let r = (t * t) * (t / x);
    t *= (CP0 + r * (CP1 + r * CP2)) + ((r * r) * r) * (CP3 + r * CP4);
    t = f64::from_bits((t.to_bits().wrapping_add(0x8000_0000)) & 0xFFFF_FFFF_C000_0000);
    // One Newton step to 53 bits.
    let s = t * t;
    let r = x / s;
    let w = t + t;
    let r = (r - t) / (w + r);
    t + t * r
}
