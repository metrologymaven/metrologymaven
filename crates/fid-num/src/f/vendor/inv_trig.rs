//! asin, acos, atan, atan2: musl `asin.c`, `acos.c`, `atan.c`, `atan2.c` (fdlibm), with
//! the project change to atan2 described in VENDOR.md. asin/acos take |x| <= 1; all
//! inputs are finite.

use super::{hi_word, pow2, with_lo_zero};
use core::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

const PIO2_LO: f64 = 6.123233995736766e-17; // 0x3C91A62633145C07
const PI_LO: f64 = 1.2246467991473532e-16; // 0x3CA1A62633145C07

const PS0: f64 = 0.16666666666666666; // 0x3FC5555555555555
const PS1: f64 = -0.3255658186224009; // 0xBFD4D61203EB6F7D
const PS2: f64 = 0.20121253213486293; // 0x3FC9C1550E884455
const PS3: f64 = -0.04005553450067941; // 0xBFA48228B5688F3B
const PS4: f64 = 0.0007915349942898145; // 0x3F49EFE07501B288
const PS5: f64 = 3.479331075960212e-05; // 0x3F023DE10DFDF709
const QS1: f64 = -2.403394911734414; // 0xC0033A271C8A2D4B
const QS2: f64 = 2.0209457602335057; // 0x40002AE59C598AC8
const QS3: f64 = -0.6882839716054533; // 0xBFE6066C1B8D0159
const QS4: f64 = 0.07703815055590194; // 0x3FB3B8C5B12E9282

const ATANHI: [f64; 4] = [
    0.4636476090008061, // atan(0.5) hi, 0x3FDDAC670561BB4F
    FRAC_PI_4,          // atan(1.0) hi
    0.982793723247329,  // atan(1.5) hi, 0x3FEF730BD281F69B
    FRAC_PI_2,          // atan(inf) hi
];
const ATANLO: [f64; 4] = [
    2.2698777452961687e-17, // 0x3C7A2B7F222F65E2
    3.061616997868383e-17,  // 0x3C81A62633145C07
    1.3903311031230998e-17, // 0x3C7007887AF0CBBD
    6.123233995736766e-17,  // 0x3C91A62633145C07
];
const AT: [f64; 11] = [
    0.3333333333333293,    // 0x3FD555555555550D
    -0.19999999999876483,  // 0xBFC999999998EBC4
    0.14285714272503466,   // 0x3FC24924920083FF
    -0.11111110405462356,  // 0xBFBC71C6FE231671
    0.09090887133436507,   // 0x3FB745CDC54C206E
    -0.0769187620504483,   // 0xBFB3B0F2AF749A6D
    0.06661073137387531,   // 0x3FB10D66A0D03D51
    -0.058335701337905735, // 0xBFADDE2D52DEFD9A
    0.049768779946159324,  // 0x3FA97B4B24760DEB
    -0.036531572744216916, // 0xBFA2B4442C6A6C2F
    0.016285820115365782,  // 0x3F90AD3AE322DA11
];

/// 2^27 + 1, Dekker's splitting constant.
const SPLIT: f64 = 134_217_729.0;

fn big_r(z: f64) -> f64 {
    let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
    let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
    p / q
}

pub(crate) fn asin(x: f64) -> f64 {
    let hx = hi_word(x);
    let ix = hx & 0x7FFF_FFFF;
    let neg = hx >> 31 == 1;
    if ix >= 0x3FF0_0000 {
        // |x| == 1 (the facade rejects |x| > 1).
        return if neg { -FRAC_PI_2 } else { FRAC_PI_2 };
    }
    if ix < 0x3FE0_0000 {
        if ix < 0x3E50_0000 {
            return x;
        }
        return x + x * big_r(x * x);
    }
    let z = (1.0 - x.abs()) * 0.5;
    let s = z.sqrt();
    let r = big_r(z);
    let y = if ix >= 0x3FEF_3333 {
        FRAC_PI_2 - (2.0 * (s + s * r) - PIO2_LO)
    } else {
        let f = with_lo_zero(s);
        let c = (z - f * f) / (s + f);
        0.5 * FRAC_PI_2 - (2.0 * s * r - (PIO2_LO - 2.0 * c) - (0.5 * FRAC_PI_2 - 2.0 * f))
    };
    if neg {
        -y
    } else {
        y
    }
}

pub(crate) fn acos(x: f64) -> f64 {
    let hx = hi_word(x);
    let ix = hx & 0x7FFF_FFFF;
    let neg = hx >> 31 == 1;
    if ix >= 0x3FF0_0000 {
        return if neg { PI } else { 0.0 };
    }
    if ix < 0x3FE0_0000 {
        if ix <= 0x3C60_0000 {
            return FRAC_PI_2;
        }
        return FRAC_PI_2 - (x - (PIO2_LO - x * big_r(x * x)));
    }
    if neg {
        let z = (1.0 + x) * 0.5;
        let s = z.sqrt();
        let w = big_r(z) * s - PIO2_LO;
        return 2.0 * (FRAC_PI_2 - (s + w));
    }
    let z = (1.0 - x) * 0.5;
    let s = z.sqrt();
    let df = with_lo_zero(s);
    let c = (z - df * df) / (s + df);
    let w = big_r(z) * s + c;
    2.0 * (df + w)
}

/// musl atan on `ax >= 0`, returned as (hi, lo) with hi + lo rounding to musl's result when
/// `c == 0`. `c` is a small correction folded in before the final rounding (atan2 only).
fn atan_parts(ax: f64, c: f64) -> (f64, f64) {
    let ix = hi_word(ax);
    if ix >= 0x4410_0000 {
        return (FRAC_PI_2, 0.0); // |x| >= 2^66
    }
    let mut x = ax;
    let id: Option<usize> = if ix < 0x3FDC_0000 {
        if ix < 0x3E40_0000 {
            return (x, 0.0); // |x| < 2^-27
        }
        None
    } else if ix < 0x3FF3_0000 {
        if ix < 0x3FE6_0000 {
            x = (2.0 * x - 1.0) / (2.0 + x);
            Some(0)
        } else {
            x = (x - 1.0) / (x + 1.0);
            Some(1)
        }
    } else if ix < 0x4003_8000 {
        x = (x - 1.5) / (1.0 + 1.5 * x);
        Some(2)
    } else {
        x = -1.0 / x;
        Some(3)
    };
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    match id {
        None => (x, -(x * (s1 + s2) - c)),
        Some(i) => (ATANHI[i], -(((x * (s1 + s2) - ATANLO[i]) - x) - c)),
    }
}

pub(crate) fn atan(x: f64) -> f64 {
    let (h, l) = atan_parts(x.abs(), 0.0);
    let z = h + l;
    if hi_word(x) >> 31 == 1 {
        -z
    } else {
        z
    }
}

/// Dekker's exact product: a * b = p + e (no FMA); |a|, |b| well inside the range.
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let ca = a * SPLIT;
    let ah = ca - (ca - a);
    let al = a - ah;
    let cb = b * SPLIT;
    let bh = cb - (cb - b);
    let bl = b - bh;
    let p = a * b;
    let e = (((ah * bh - p) + ah * bl) + al * bh) + al * bl;
    (p, e)
}

/// (ay/ax - t) / (1 + t^2) for t = RN(ay/ax) in [2^-27, 2^27], else 0: the first-order
/// effect of the rounded division on atan (project addition, see VENDOR.md).
fn div_corr(ay: f64, ax: f64, t: f64) -> f64 {
    if !(pow2(-27)..=pow2(27)).contains(&t) {
        return 0.0;
    }
    let (mut ax, mut ay) = (ax, ay);
    if ax < pow2(-1022) {
        ax *= pow2(54);
        ay *= pow2(54);
    }
    // Scale both by 2^-k (exact) so that ax lands in [1, 2).
    let k = (ax.to_bits() >> 52) as i32 - 1023;
    let h1 = (-k) >> 1;
    let h2 = -k - h1;
    let sx = (ax * pow2(h1)) * pow2(h2);
    let sy = (ay * pow2(h1)) * pow2(h2);
    let (p, e) = two_prod(t, sx);
    let r = (sy - p) - e;
    (r / sx) / (1.0 + t * t)
}

pub(crate) fn atan2(y: f64, x: f64) -> f64 {
    let (ux, uy) = (x.to_bits(), y.to_bits());
    let (mut ix, lx) = ((ux >> 32) as u32, ux as u32);
    let (mut iy, ly) = ((uy >> 32) as u32, uy as u32);
    if (ix.wrapping_sub(0x3FF0_0000) | lx) == 0 {
        return atan(y); // x == 1
    }
    let m = ((iy >> 31) & 1) | ((ix >> 30) & 2); // 2 * sign(x) + sign(y)
    ix &= 0x7FFF_FFFF;
    iy &= 0x7FFF_FFFF;
    if (iy | ly) == 0 {
        return match m {
            0 | 1 => y,
            2 => PI,
            _ => -PI,
        };
    }
    if (ix | lx) == 0 || ix + (64 << 20) < iy {
        return if m & 1 == 1 { -FRAC_PI_2 } else { FRAC_PI_2 };
    }
    let (h, l) = if m & 2 == 2 && iy + (64 << 20) < ix {
        (0.0, 0.0)
    } else {
        let t = (y / x).abs();
        atan_parts(t, div_corr(y.abs(), x.abs(), t))
    };
    match m {
        0 => h + l,
        1 => -(h + l),
        _ => {
            // pi - (h + l) with one final rounding (project change; musl rounds h + l first).
            let s = PI - h;
            let err = (PI - s) - h;
            let z = s + ((err + PI_LO) - l);
            if m == 3 {
                -z
            } else {
                z
            }
        }
    }
}
