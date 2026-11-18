//! exp and log: musl `exp.c` (the fdlibm-derived version), `scalbn.c` and `log.c`
//! (fdlibm). Inputs are finite; log takes x > 0. exp returns +inf on overflow and the
//! facade reports that as an error.

use super::pow2;
use core::f64::consts::LOG2_E;

const LN2HI: f64 = 0.6931471803691238; // 0x3FE62E42FEE00000
const LN2LO: f64 = 1.9082149292705877e-10; // 0x3DEA39EF35793C76
const P1: f64 = 0.16666666666666602; // 0x3FC555555555553E
const P2: f64 = -0.0027777777777015593; // 0xBF66C16C16BEBD93
const P3: f64 = 6.613756321437934e-05; // 0x3F11566AAF25DE2C
const P4: f64 = -1.6533902205465252e-06; // 0xBEBBBD41C5D26BF1
const P5: f64 = 4.1381367970572385e-08; // 0x3E66376972BEA4D0
/// Largest x with finite exp(x), 0x40862E42FEFA39EF.
const EXP_HI: f64 = 709.782712893384;
/// Below this exp(x) rounds to zero.
const EXP_LO: f64 = -745.1332191019411;

const LG1: f64 = 0.6666666666666735; // 0x3FE5555555555593
const LG2: f64 = 0.3999999999940942; // 0x3FD999999997FA04
const LG3: f64 = 0.2857142874366239; // 0x3FD2492494229359
const LG4: f64 = 0.22222198432149784; // 0x3FCC71C51D8E78AF
const LG5: f64 = 0.1818357216161805; // 0x3FC7466496CB03DE
const LG6: f64 = 0.15313837699209373; // 0x3FC39A09D078C69F
const LG7: f64 = 0.14798198605116586; // 0x3FC2F112DF3E5244

/// y * 2^n, exact except for the final rounding into the subnormal range.
fn scalbn(y: f64, n: i32) -> f64 {
    let (mut y, mut n) = (y, n);
    if n > 1023 {
        y *= pow2(1023);
        n -= 1023;
        if n > 1023 {
            y *= pow2(1023);
            n -= 1023;
            if n > 1023 {
                n = 1023;
            }
        }
    } else if n < -1022 {
        // Keep the final n below -53 to avoid double rounding in the subnormal range.
        y *= pow2(-1022) * pow2(53);
        n += 1022 - 53;
        if n < -1022 {
            y *= pow2(-1022) * pow2(53);
            n += 1022 - 53;
            if n < -1022 {
                n = -1022;
            }
        }
    }
    y * pow2(n)
}

pub(crate) fn exp(x: f64) -> f64 {
    let hx0 = (x.to_bits() >> 32) as u32;
    let sign = (hx0 >> 31) as i32;
    let hx = hx0 & 0x7FFF_FFFF;
    if hx >= 0x4086_232B {
        // |x| >= 708.39
        if x > EXP_HI {
            return f64::INFINITY;
        }
        if x < EXP_LO {
            return 0.0;
        }
    }
    let (k, hi, lo, xr) = if hx > 0x3FD6_2E42 {
        // |x| > 0.5 ln2
        let k = if hx >= 0x3FF0_A2B2 {
            (LOG2_E * x + if sign == 1 { -0.5 } else { 0.5 }) as i32
        } else {
            1 - sign - sign
        };
        let hi = x - f64::from(k) * LN2HI; // exact
        let lo = f64::from(k) * LN2LO;
        (k, hi, lo, hi - lo)
    } else if hx > 0x3E30_0000 {
        // |x| > 2^-28
        (0, x, 0.0, x)
    } else {
        return 1.0 + x;
    };
    let xx = xr * xr;
    let c = xr - xx * (P1 + xx * (P2 + xx * (P3 + xx * (P4 + xx * P5))));
    let y = 1.0 + (((xr * c) / (2.0 - c) - lo) + hi);
    if k == 0 {
        y
    } else {
        scalbn(y, k)
    }
}

/// Natural logarithm for finite x > 0.
pub(crate) fn log(x: f64) -> f64 {
    let mut u = x.to_bits();
    let mut hx = (u >> 32) as u32;
    let mut k: i32 = 0;
    if hx < 0x0010_0000 {
        // subnormal: scale up
        k -= 54;
        u = (x * pow2(54)).to_bits();
        hx = (u >> 32) as u32;
    } else if hx == 0x3FF0_0000 && (u & 0xFFFF_FFFF) == 0 {
        return 0.0;
    }
    // Reduce x into [sqrt(2)/2, sqrt(2)].
    hx = hx.wrapping_add(0x3FF0_0000 - 0x3FE6_A09E);
    k += (hx >> 20) as i32 - 0x3FF;
    hx = (hx & 0x000F_FFFF) + 0x3FE6_A09E;
    let xr = f64::from_bits((u64::from(hx) << 32) | (u & 0xFFFF_FFFF));
    let f = xr - 1.0;
    let hfsq = 0.5 * f * f;
    let s = f / (2.0 + f);
    let z = s * s;
    let w = z * z;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    let r = t2 + t1;
    let dk = f64::from(k);
    (((s * (hfsq + r) + dk * LN2LO) - hfsq) + f) + dk * LN2HI
}
