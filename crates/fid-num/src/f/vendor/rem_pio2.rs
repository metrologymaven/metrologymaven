//! Argument reduction x = n * pi/2 + (y0 + y1), |y0 + y1| <= ~pi/4.
//!
//! Medium path: musl `__rem_pio2` (Cody-Waite with three pi/2 pieces). Large path
//! (|x| >= 2^20 * pi/2): a project replacement for `__rem_pio2_large`, Payne-Hanek in
//! exact integer arithmetic over a 1280-bit table of 2/pi (see VENDOR.md).

use super::{hi_word, pow2};
use core::f64::consts::{FRAC_2_PI, FRAC_PI_4};

const TOINT: f64 = 6_755_399_441_055_744.0; // 1.5 * 2^52
const PIO2_1: f64 = 1.5707963267341256; // 0x3FF921FB54400000
const PIO2_1T: f64 = 6.077100506506192e-11; // 0x3DD0B4611A626331
const PIO2_2: f64 = 6.077100506303966e-11; // 0x3DD0B4611A600000
const PIO2_2T: f64 = 2.0222662487959506e-21; // 0x3BA3198A2E037073
const PIO2_3: f64 = 2.0222662487111665e-21; // 0x3BA3198A2E000000
const PIO2_3T: f64 = 8.4784276603689e-32; // 0x397B839A252049C1

/// Bits 1..1280 after the binary point of 2/pi, 32 per word, most significant first.
const TWO_OVER_PI: [u32; 40] = [
    0xA2F9_836E,
    0x4E44_1529,
    0xFC27_57D1,
    0xF534_DDC0,
    0xDB62_9599,
    0x3C43_9041,
    0xFE51_63AB,
    0xDEBB_C561,
    0xB724_6E3A,
    0x424D_D2E0,
    0x0649_2EEA,
    0x09D1_921C,
    0xFE1D_EB1C,
    0xB129_A73E,
    0xE882_35F5,
    0x2EBB_4484,
    0xE99C_7026,
    0xB45F_7E41,
    0x3991_D639,
    0x8353_39F4,
    0x9C84_5F8B,
    0xBDF9_283B,
    0x1FF8_97FF,
    0xDE05_980F,
    0xEF2F_118B,
    0x5A0A_6D1F,
    0x6D36_7ECF,
    0x27CB_09B7,
    0x4F46_3F66,
    0x9E5F_EA2D,
    0x7527_BAC7,
    0xEBE5_F17B,
    0x3D07_39F7,
    0x8A52_92EA,
    0x6BFB_5FB1,
    0x1F8D_5D08,
    0x5603_3046,
    0xFC7B_6BAB,
    0xF0CF_BC20,
    0x9AF4_361D,
];
/// floor(pi/2 * 2^127).
const PIO2_FIX: u128 = 0xC90F_DAA2_2168_C234_C4C6_628B_80DC_1CD1;

/// Returns (n mod 4, y0, y1) for finite x with |x| > pi/4.
pub(super) fn rem_pio2(x: f64) -> (i32, f64, f64) {
    let ix = hi_word(x) & 0x7FFF_FFFF;
    if ix >= 0x4139_21FB {
        return rem_pio2_large(x);
    }
    let mut fnv = (x * FRAC_2_PI + TOINT) - TOINT;
    let mut n = fnv as i32;
    let mut r = x - fnv * PIO2_1;
    let mut w = fnv * PIO2_1T;
    if r - w < -FRAC_PI_4 {
        n -= 1;
        fnv -= 1.0;
        r = x - fnv * PIO2_1;
        w = fnv * PIO2_1T;
    } else if r - w > FRAC_PI_4 {
        n += 1;
        fnv += 1.0;
        r = x - fnv * PIO2_1;
        w = fnv * PIO2_1T;
    }
    let mut y0 = r - w;
    let ex = (ix >> 20) as i32;
    let mut ey = ((y0.to_bits() >> 52) & 0x7FF) as i32;
    if ex - ey > 16 {
        let t = r;
        w = fnv * PIO2_2;
        r = t - w;
        w = fnv * PIO2_2T - ((t - r) - w);
        y0 = r - w;
        ey = ((y0.to_bits() >> 52) & 0x7FF) as i32;
        if ex - ey > 49 {
            let t = r;
            w = fnv * PIO2_3;
            r = t - w;
            w = fnv * PIO2_3T - ((t - r) - w);
            y0 = r - w;
        }
    }
    let y1 = (r - y0) - w;
    (n & 3, y0, y1)
}

fn word(i: usize) -> u32 {
    TWO_OVER_PI.get(i).copied().unwrap_or(0)
}

/// 32 bits of 2/pi starting at 1-based bit position `a` (bits at positions <= 0 are 0).
fn chunk(a: i32) -> u32 {
    let pos = a - 1;
    if pos < 0 {
        return if pos <= -32 { 0 } else { word(0) >> (-pos) as u32 };
    }
    let w = (pos / 32) as usize;
    let sh = (pos % 32) as u32;
    if sh == 0 {
        word(w)
    } else {
        (word(w) << sh) | (word(w + 1) >> (32 - sh))
    }
}

/// 128 x 128 -> 256-bit product as (high, low).
fn mul128(a: u128, b: u128) -> (u128, u128) {
    const M: u128 = u64::MAX as u128;
    let (a1, a0, b1, b0) = (a >> 64, a & M, b >> 64, b & M);
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let mid = (p00 >> 64) + (p01 & M) + (p10 & M);
    let lo = (p00 & M) | (mid << 64);
    let hi = p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64);
    (hi, lo)
}

fn join(w: &[u32]) -> u128 {
    w.iter().fold(0u128, |acc, &x| (acc << 32) | u128::from(x))
}

/// Payne-Hanek for |x| >= 2^20 * pi/2. |x| = m * 2^e with m < 2^53; with F = 254 fraction
/// bits, P = m * (floor(2/pi * 2^(e+F)) mod 2^(F+2)) mod 2^(F+2) holds (x * 2/pi mod 4)
/// to within 2^(53-F). The fraction, folded to [-1/2, 1/2], is multiplied by pi/2 in fixed
/// point and rounded once to a double-double.
fn rem_pio2_large(x: f64) -> (i32, f64, f64) {
    let u = x.to_bits();
    let m = (u & ((1u64 << 52) - 1)) | (1u64 << 52);
    let e = ((u >> 52) & 0x7FF) as i32 - 1075;
    let a0 = e - 1; // bit position of the most significant bit of the 256-bit window
    let mut p = [0u32; 8];
    let mut carry: u128 = 0;
    for (i, limb) in p.iter_mut().enumerate().rev() {
        let g = u128::from(chunk(a0 + 32 * i as i32));
        let acc = g * u128::from(m) + carry;
        *limb = acc as u32;
        carry = acc >> 32;
    }
    let mut n = (p[0] >> 30) as i32;
    let neg = (p[0] >> 29) & 1 == 1;
    p[0] &= 0x3FFF_FFFF;
    let mut f_hi = join(&p[0..4]);
    let mut f_lo = join(&p[4..8]);
    if neg {
        // fraction >= 1/2: use fraction - 1, i.e. magnitude 2^254 - f, and round n up.
        n += 1;
        let (nlo, borrow) = 0u128.overflowing_sub(f_lo);
        f_hi = 0u128.wrapping_sub(f_hi).wrapping_sub(u128::from(borrow)) & ((1u128 << 126) - 1);
        f_lo = nlo;
    }
    n &= 3;
    let sign = (u >> 63) == 1;
    let n_out = if sign { (-n) & 3 } else { n };
    if f_hi == 0 && f_lo == 0 {
        return (n_out, 0.0, 0.0);
    }
    let lz = if f_hi != 0 { f_hi.leading_zeros() } else { 128 + f_lo.leading_zeros() };
    let lead = 255 - lz as i32;
    // Top 128 bits of f, normalised (f < 2^254, so lz >= 2).
    let mm = if lz >= 128 { f_lo << (lz - 128) } else { (f_hi << lz) | (f_lo >> (128 - lz)) };
    let (qh, ql) = mul128(mm, PIO2_FIX);
    let (h, ex) = if qh >> 127 == 1 { (qh, lead - 380) } else { ((qh << 1) | (ql >> 127), lead - 381) };
    let mut t = h >> 75;
    let rest = h & ((1u128 << 75) - 1);
    let half = 1u128 << 74;
    if rest > half || (rest == half && t & 1 == 1) {
        t += 1;
    }
    let r = h.wrapping_sub(t << 75) as i128;
    let mut y0 = (t as f64) * pow2(ex + 75);
    let mut y1 = (r as f64) * pow2(ex);
    if neg {
        y0 = -y0;
        y1 = -y1;
    }
    if sign {
        (n_out, -y0, -y1)
    } else {
        (n_out, y0, y1)
    }
}
