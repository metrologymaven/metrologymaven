//! Pure-Rust 256-bit (~77 decimal digits) deterministic binary floating-point type [`Hp`].
//!
//! Representation:
//! - Zero: `neg = false, exp = 0, sig = [0, 0, 0, 0]`.
//! - Non-zero: `(-1)^neg * (sig / 2^255) * 2^exp`, where `sig` is a 256-bit unsigned integer
//!   in `[2^255, 2^256)` (little-endian limbs `sig[0]`..`sig[3]`, so `sig[3] >> 63 == 1`).
//!
//! Every arithmetic operation is integer-backed (D0 deterministic across x86-64, ARM64 and
//! wasm32) and uses round-to-nearest-even rounding.

use std::cmp::Ordering;
use std::ops::{Add, Div, Mul, Neg, Sub};

use super::HpError;

/// Significand precision in bits (~77.06 decimal digits, exceeding the 200-bit / 60-digit floor).
pub const PRECISION_BITS: u32 = 256;

const MSB: u64 = 1u64 << 63;

/// 256-bit normalized binary floating-point real number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hp {
    pub(crate) neg: bool,
    pub(crate) exp: i32,
    pub(crate) sig: [u64; 4],
}

impl Hp {
    pub const ZERO: Hp = Hp { neg: false, exp: 0, sig: [0, 0, 0, 0] };
    pub const ONE: Hp = Hp { neg: false, exp: 0, sig: [0, 0, 0, MSB] };
    pub const TWO: Hp = Hp { neg: false, exp: 1, sig: [0, 0, 0, MSB] };
    pub const HALF: Hp = Hp { neg: false, exp: -1, sig: [0, 0, 0, MSB] };

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.sig == [0, 0, 0, 0]
    }

    #[inline]
    pub fn is_negative(&self) -> bool {
        !self.is_zero() && self.neg
    }

    #[inline]
    pub fn abs(self) -> Hp {
        if self.is_zero() {
            Hp::ZERO
        } else {
            Hp { neg: false, exp: self.exp, sig: self.sig }
        }
    }

    #[inline]
    pub fn negate(self) -> Hp {
        if self.is_zero() {
            Hp::ZERO
        } else {
            Hp { neg: !self.neg, exp: self.exp, sig: self.sig }
        }
    }

    /// Exact multiplication by `2^k`.
    pub fn scale_pow2(self, k: i32) -> Hp {
        if self.is_zero() {
            Hp::ZERO
        } else {
            Hp { neg: self.neg, exp: self.exp.saturating_add(k), sig: self.sig }
        }
    }

    /// Exact conversion from `u64`.
    pub fn from_u64(v: u64) -> Hp {
        if v == 0 {
            return Hp::ZERO;
        }
        let lz = v.leading_zeros();
        Hp { neg: false, exp: (63 - lz) as i32, sig: [0, 0, 0, v << lz] }
    }

    /// Exact conversion from `i64`.
    pub fn from_i64(v: i64) -> Hp {
        if v == 0 {
            return Hp::ZERO;
        }
        let mag = v.unsigned_abs();
        let mut h = Hp::from_u64(mag);
        h.neg = v < 0;
        h
    }

    /// Exact lossless conversion from any finite IEEE-754 `f64` (including subnormals).
    pub fn from_f64(v: f64) -> Result<Hp, HpError> {
        if !v.is_finite() {
            return Err(HpError::Domain("non-finite f64"));
        }
        if v == 0.0 {
            return Ok(Hp::ZERO);
        }
        let bits = v.to_bits();
        let neg = (bits >> 63) != 0;
        let raw_exp = ((bits >> 52) & 0x7ff) as i32;
        let frac = bits & ((1u64 << 52) - 1);
        if raw_exp == 0 {
            // Subnormal: value = frac * 2^-1074
            let lz = frac.leading_zeros();
            let exp = -1074 + (63 - lz as i32);
            Ok(Hp { neg, exp, sig: [0, 0, 0, frac << lz] })
        } else {
            // Normal: value = (1 + frac * 2^-52) * 2^(raw_exp - 1023)
            let mant = (1u64 << 52) | frac;
            Ok(Hp { neg, exp: raw_exp - 1023, sig: [0, 0, 0, mant << 11] })
        }
    }

    /// Exact round-to-nearest-even conversion to IEEE-754 `f64`.
    pub fn to_f64(self) -> f64 {
        if self.is_zero() {
            return 0.0;
        }
        let sign_bit = if self.neg { 1u64 << 63 } else { 0 };
        if self.exp > 1023 {
            return f64::from_bits(sign_bit | (0x7ffu64 << 52));
        }
        if self.exp < -1075 {
            return f64::from_bits(sign_bit);
        }
        if self.exp >= -1022 {
            // Normal range: top 53 bits are in sig[3] >> 11.
            let mut mant = self.sig[3] >> 11;
            let guard = (self.sig[3] >> 10) & 1;
            let sticky = ((self.sig[3] & ((1u64 << 10) - 1)) | self.sig[2] | self.sig[1] | self.sig[0]) != 0;
            if guard == 1 && (sticky || (mant & 1) == 1) {
                mant += 1;
            }
            let mut exp = self.exp;
            if mant == (1u64 << 53) {
                mant >>= 1;
                exp += 1;
            }
            if exp > 1023 {
                return f64::from_bits(sign_bit | (0x7ffu64 << 52));
            }
            let biased = ((exp + 1023) as u64) << 52;
            f64::from_bits(sign_bit | biased | (mant & ((1u64 << 52) - 1)))
        } else {
            // Subnormal range: shift right by (-1022 - exp) + 11 = -1011 - exp in [12..=64]
            let total_shift = (-1022 - self.exp) as u32 + 11;
            let low_sticky = (self.sig[2] | self.sig[1] | self.sig[0]) != 0;
            let (mut mant, guard, sticky) = if total_shift < 64 {
                let m = self.sig[3] >> total_shift;
                let g = (self.sig[3] >> (total_shift - 1)) & 1;
                let rem_mask = if total_shift >= 2 { (1u64 << (total_shift - 1)) - 1 } else { 0 };
                let s = ((self.sig[3] & rem_mask) != 0) || low_sticky;
                (m, g, s)
            } else if total_shift == 64 {
                let g = self.sig[3] >> 63;
                let s = ((self.sig[3] & (MSB - 1)) != 0) || low_sticky;
                (0u64, g, s)
            } else {
                (0u64, 0u64, true)
            };
            if guard == 1 && (sticky || (mant & 1) == 1) {
                mant += 1;
            }
            if mant == (1u64 << 52) {
                // Rounded up to smallest normal.
                f64::from_bits(sign_bit | (1u64 << 52))
            } else {
                f64::from_bits(sign_bit | mant)
            }
        }
    }

    /// Lowercase `0x<16 hex>` IEEE-754 binary64 encoding of [`Self::to_f64`].
    pub fn to_hex_f64(self) -> String {
        let v = self.to_f64();
        let bits = if v == 0.0 { 0u64 } else { v.to_bits() };
        format!("0x{bits:016x}")
    }

    /// Exact 256-bit `pi`, computed via Machin's formula `4 * (4 * atan(1/5) - atan(1/239))`.
    pub fn pi() -> Hp {
        let a = atan_recip(5);
        let b = atan_recip(239);
        (a.scale_pow2(2) - b).scale_pow2(2)
    }

    /// 256-bit square root via Newton-Raphson seeded from `f64::sqrt`.
    pub fn sqrt(self) -> Result<Hp, HpError> {
        if self.is_zero() {
            return Ok(Hp::ZERO);
        }
        if self.neg {
            return Err(HpError::Domain("sqrt of negative number"));
        }
        // Reduce to a in [1, 4) with even exponent: self = a * 2^(2k).
        let k = self.exp.div_euclid(2);
        let rem = self.exp.rem_euclid(2);
        let a = Hp { neg: false, exp: rem, sig: self.sig };
        let seed = a.to_f64().sqrt();
        let mut x = Hp::from_f64(seed)?;
        // 53 -> 106 -> 212 -> 256 -> 256 exact.
        for _ in 0..4 {
            x = (x + (a / x)?).scale_pow2(-1);
        }
        Ok(x.scale_pow2(k))
    }

    /// 256-bit sine via `pi/2` quadrant reduction + Taylor series.
    pub fn sin(self) -> Hp {
        let (s, _) = self.sin_cos();
        s
    }

    /// 256-bit cosine via `pi/2` quadrant reduction + Taylor series.
    pub fn cos(self) -> Hp {
        let (_, c) = self.sin_cos();
        c
    }

    /// Simultaneous 256-bit `(sin(x), cos(x))`.
    pub fn sin_cos(self) -> (Hp, Hp) {
        if self.is_zero() {
            return (Hp::ZERO, Hp::ONE);
        }
        let pi = Hp::pi();
        let half_pi = pi.scale_pow2(-1);
        // k = round(self / (pi/2))
        let q = self / half_pi;
        let Ok(q) = q else {
            return (Hp::ZERO, Hp::ONE);
        };
        let k_int = q.round_to_i64_mod4();
        let k_hp = q.round_to_integer();
        let r = self - k_hp * half_pi;
        let (sr, cr) = sin_cos_reduced(r);
        match k_int {
            0 => (sr, cr),
            1 => (cr, sr.neg()),
            2 => (sr.neg(), cr.neg()),
            _ => (cr.neg(), sr),
        }
    }

    /// 256-bit arctangent `atan(x)` in `[-pi/2, pi/2]`.
    pub fn atan(self) -> Hp {
        if self.is_zero() {
            return Hp::ZERO;
        }
        let sign = self.neg;
        let mut x = self.abs();
        let pi_half = Hp::pi().scale_pow2(-1);
        let invert = x > Hp::ONE;
        if invert {
            if let Ok(inv) = Hp::ONE / x {
                x = inv;
            }
        }
        // Apply half-angle reduction 5 times: atan(t) = 2 * atan(t / (1 + sqrt(1 + t^2))).
        // After 5 steps, |x| <= tan(pi/128) < 0.0246, so Taylor converges in <= 25 terms.
        for _ in 0..5 {
            let denom = match (Hp::ONE + x * x).sqrt() {
                Ok(s) => Hp::ONE + s,
                Err(_) => Hp::TWO,
            };
            if let Ok(next) = x / denom {
                x = next;
            }
        }
        let mut acc = atan_taylor(x).scale_pow2(5);
        if invert {
            acc = pi_half - acc;
        }
        if sign {
            acc.neg()
        } else {
            acc
        }
    }

    /// 256-bit `atan2(y, x)` in `(-pi, pi]`.
    pub fn atan2(y: Hp, x: Hp) -> Result<Hp, HpError> {
        let pi = Hp::pi();
        let half_pi = pi.scale_pow2(-1);
        if x.is_zero() {
            if y.is_zero() {
                return Ok(Hp::ZERO);
            }
            return Ok(if y.neg { half_pi.neg() } else { half_pi });
        }
        let base = (y.abs() / x.abs())?.atan();
        let res = if !x.neg {
            if y.neg {
                base.neg()
            } else {
                base
            }
        } else if y.neg {
            (pi - base).neg()
        } else {
            pi - base
        };
        Ok(res)
    }

    /// Rounds `self` to the nearest integer represented as `Hp` (ties away from zero for reduction).
    fn round_to_integer(self) -> Hp {
        if self.is_zero() {
            return Hp::ZERO;
        }
        if self.exp < -1 {
            return Hp::ZERO;
        }
        if self.exp >= 255 {
            return self;
        }
        let frac_bits = (255 - self.exp) as u32;
        let shifted = shr256(self.sig, frac_bits);
        let half_bit = get_bit256(self.sig, frac_bits - 1);
        let mut int_sig = shifted;
        if half_bit == 1 {
            let (s, carry) = add256(int_sig, [1, 0, 0, 0]);
            if carry {
                return Hp { neg: self.neg, exp: self.exp + 1, sig: [0, 0, 0, MSB] };
            }
            int_sig = s;
        }
        if int_sig == [0; 4] {
            return Hp::ZERO;
        }
        let lz = clz256(int_sig);
        let norm = shl256(int_sig, lz);
        let exp = (255 - lz) as i32;
        Hp { neg: self.neg, exp, sig: norm }
    }

    fn round_to_i64_mod4(self) -> u8 {
        let r = self.round_to_integer();
        if r.is_zero() || r.exp < 0 {
            return 0;
        }
        if r.exp >= 257 {
            return 0;
        }
        let frac_bits = (255 - r.exp) as u32;
        let low = shr256(r.sig, frac_bits)[0] & 3;
        if r.neg {
            ((4 - low) & 3) as u8
        } else {
            low as u8
        }
    }

    /// Compares magnitudes `|self|` vs `|other|`.
    pub(crate) fn cmp_abs(&self, other: &Hp) -> Ordering {
        match (self.is_zero(), other.is_zero()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => self.exp.cmp(&other.exp).then_with(|| cmp256(self.sig, other.sig)),
        }
    }

    pub(crate) fn add_hp(self, rhs: Hp) -> Hp {
        if self.is_zero() {
            return rhs;
        }
        if rhs.is_zero() {
            return self;
        }
        if self.neg == rhs.neg {
            add_same_sign(self, rhs)
        } else {
            sub_diff_sign(self, rhs)
        }
    }

    pub(crate) fn mul_hp(self, rhs: Hp) -> Hp {
        if self.is_zero() || rhs.is_zero() {
            return Hp::ZERO;
        }
        let neg = self.neg ^ rhs.neg;
        let prod = mul256x256(self.sig, rhs.sig);
        // Product is in [2^510, 2^512): top bit is either bit 511 or bit 510.
        let top_bit_511 = (prod[7] & MSB) != 0;
        let (mut sig, guard, sticky, mut exp) = if top_bit_511 {
            // Shift right by 256 -> limbs prod[4..8]; guard = bit 255 = prod[3]>>63
            let s = [prod[4], prod[5], prod[6], prod[7]];
            let g = prod[3] >> 63;
            let st = ((prod[3] & (MSB - 1)) | prod[2] | prod[1] | prod[0]) != 0;
            (s, g, st, self.exp.saturating_add(rhs.exp).saturating_add(1))
        } else {
            // Shift right by 255: bit 510 becomes bit 255.
            let s = [
                (prod[4] << 1) | (prod[3] >> 63),
                (prod[5] << 1) | (prod[4] >> 63),
                (prod[6] << 1) | (prod[5] >> 63),
                (prod[7] << 1) | (prod[6] >> 63),
            ];
            let g = (prod[3] >> 62) & 1;
            let st = ((prod[3] & ((1u64 << 62) - 1)) | prod[2] | prod[1] | prod[0]) != 0;
            (s, g, st, self.exp.saturating_add(rhs.exp))
        };
        if guard == 1 && (sticky || (sig[0] & 1) == 1) {
            let (inc, carry) = add256(sig, [1, 0, 0, 0]);
            if carry {
                sig = [0, 0, 0, MSB];
                exp = exp.saturating_add(1);
            } else {
                sig = inc;
            }
        }
        Hp { neg, exp, sig }
    }

    pub(crate) fn div_hp(self, rhs: Hp) -> Result<Hp, HpError> {
        if rhs.is_zero() {
            return Err(HpError::Domain("division by zero"));
        }
        if self.is_zero() {
            return Ok(Hp::ZERO);
        }
        let neg = self.neg ^ rhs.neg;
        let mut exp = self.exp.saturating_sub(rhs.exp);
        let mut rem = [self.sig[0], self.sig[1], self.sig[2], self.sig[3], 0u64];
        let den = [rhs.sig[0], rhs.sig[1], rhs.sig[2], rhs.sig[3], 0u64];

        // If self.sig < rhs.sig, shift rem left by 1 and decrement exp so quotient bit 255 is 1.
        if cmp320(rem, den) == Ordering::Less {
            rem = shl320_1(rem);
            exp = exp.saturating_sub(1);
        }

        // Produce 256 quotient bits + 1 guard bit via restoring binary long division.
        let mut sig = [0u64; 4];
        for bit in (0..256u32).rev() {
            if cmp320(rem, den) != Ordering::Less {
                rem = sub320(rem, den);
                let limb = (bit / 64) as usize;
                sig[limb] |= 1u64 << (bit % 64);
            }
            rem = shl320_1(rem);
        }
        let guard = if cmp320(rem, den) != Ordering::Less {
            rem = sub320(rem, den);
            1u64
        } else {
            0u64
        };
        let sticky = rem != [0; 5];
        if guard == 1 && (sticky || (sig[0] & 1) == 1) {
            let (inc, carry) = add256(sig, [1, 0, 0, 0]);
            if carry {
                sig = [0, 0, 0, MSB];
                exp = exp.saturating_add(1);
            } else {
                sig = inc;
            }
        }
        Ok(Hp { neg, exp, sig })
    }
}

impl Ord for Hp {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.is_zero() && other.is_zero() {
            return Ordering::Equal;
        }
        match (self.is_negative(), other.is_negative()) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => self.cmp_abs(other),
            (true, true) => other.cmp_abs(self),
        }
    }
}

impl PartialOrd for Hp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Add for Hp {
    type Output = Hp;
    fn add(self, rhs: Hp) -> Hp {
        self.add_hp(rhs)
    }
}

impl Sub for Hp {
    type Output = Hp;
    fn sub(self, rhs: Hp) -> Hp {
        self.add_hp(rhs.neg())
    }
}

impl Mul for Hp {
    type Output = Hp;
    fn mul(self, rhs: Hp) -> Hp {
        self.mul_hp(rhs)
    }
}

impl Div for Hp {
    type Output = Result<Hp, HpError>;
    fn div(self, rhs: Hp) -> Result<Hp, HpError> {
        self.div_hp(rhs)
    }
}

impl Neg for Hp {
    type Output = Hp;
    fn neg(self) -> Hp {
        self.negate()
    }
}

// ============================================================================
// Transcendental series helpers
// ============================================================================

/// `atan(1 / n)` for integer `n >= 2` via alternating Taylor series.
fn atan_recip(n: u64) -> Hp {
    let n_hp = Hp::from_u64(n);
    let n2 = Hp::from_u64(n * n);
    let Ok(mut term) = Hp::ONE / n_hp else {
        return Hp::ZERO;
    };
    let mut sum = term;
    let mut k = 1u64;
    while k < 400 {
        let Ok(next) = term / n2 else {
            break;
        };
        term = next;
        let denom = Hp::from_u64(2 * k + 1);
        let Ok(addend) = term / denom else {
            break;
        };
        if addend.is_zero() || addend.exp < sum.exp - 262 {
            break;
        }
        if k % 2 == 1 {
            sum = sum - addend;
        } else {
            sum = sum + addend;
        }
        k += 1;
    }
    sum
}

/// `atan(x)` Taylor series for small `|x| < 0.05`.
fn atan_taylor(x: Hp) -> Hp {
    let x2 = x * x;
    let mut pow = x;
    let mut sum = x;
    let mut k = 1u64;
    while k < 120 {
        pow = pow * x2;
        let Ok(term) = pow / Hp::from_u64(2 * k + 1) else {
            break;
        };
        if term.is_zero() || term.exp < sum.exp - 262 {
            break;
        }
        if k % 2 == 1 {
            sum = sum - term;
        } else {
            sum = sum + term;
        }
        k += 1;
    }
    sum
}

/// Simultaneous `(sin(r), cos(r))` for `|r| <= pi/4`.
fn sin_cos_reduced(r: Hp) -> (Hp, Hp) {
    let r2 = r * r;
    // sin(r) = r - r^3/3! + r^5/5! - ...
    let mut s_term = r;
    let mut s_sum = r;
    let mut k = 1u64;
    while k < 80 {
        let denom = Hp::from_u64((2 * k) * (2 * k + 1));
        let Ok(next) = (s_term * r2) / denom else {
            break;
        };
        s_term = next;
        if s_term.is_zero() || (!s_sum.is_zero() && s_term.exp < s_sum.exp - 262) {
            break;
        }
        if k % 2 == 1 {
            s_sum = s_sum - s_term;
        } else {
            s_sum = s_sum + s_term;
        }
        k += 1;
    }
    // cos(r) = 1 - r^2/2! + r^4/4! - ...
    let mut c_term = Hp::ONE;
    let mut c_sum = Hp::ONE;
    let mut j = 1u64;
    while j < 80 {
        let denom = Hp::from_u64((2 * j - 1) * (2 * j));
        let Ok(next) = (c_term * r2) / denom else {
            break;
        };
        c_term = next;
        if c_term.is_zero() || c_term.exp < c_sum.exp - 262 {
            break;
        }
        if j % 2 == 1 {
            c_sum = c_sum - c_term;
        } else {
            c_sum = c_sum + c_term;
        }
        j += 1;
    }
    (s_sum, c_sum)
}

// ============================================================================
// Low-level 256-bit / 320-bit / 512-bit integer arithmetic
// ============================================================================

fn add_same_sign(a: Hp, b: Hp) -> Hp {
    let (hi, lo) = if a.cmp_abs(&b) != Ordering::Less { (a, b) } else { (b, a) };
    let diff = (hi.exp as i64) - (lo.exp as i64);
    if diff >= 258 {
        return hi;
    }
    let shift = diff as u32;
    let (lo_shifted, guard, round, sticky) = shr256_grs(lo.sig, shift);
    let (mut sig, carry) = add256(hi.sig, lo_shifted);
    let mut exp = hi.exp;
    let (final_g, final_s) = if carry {
        let new_g = sig[0] & 1;
        let new_s = guard != 0 || round != 0 || sticky;
        sig = shr256_1_with_msb(sig);
        exp = exp.saturating_add(1);
        (new_g, new_s)
    } else {
        (guard, round != 0 || sticky)
    };
    if final_g == 1 && (final_s || (sig[0] & 1) == 1) {
        let (inc, c) = add256(sig, [1, 0, 0, 0]);
        if c {
            sig = [0, 0, 0, MSB];
            exp = exp.saturating_add(1);
        } else {
            sig = inc;
        }
    }
    Hp { neg: hi.neg, exp, sig }
}

fn sub_diff_sign(a: Hp, b: Hp) -> Hp {
    let cmp = a.cmp_abs(&b);
    if cmp == Ordering::Equal {
        return Hp::ZERO;
    }
    let (hi, lo) = if cmp == Ordering::Greater { (a, b) } else { (b, a) };
    let diff = (hi.exp as i64) - (lo.exp as i64);
    if diff >= 259 {
        return hi;
    }
    let shift = diff as u32;
    // Perform subtraction at 259 bits (sig << 3) so guard, round, sticky are exact even with 1-bit borrow.
    let (lo_shifted, g, r, s) = shr256_grs(lo.sig, shift);
    let lo_tail: u64 = (g << 2) | (r << 1) | u64::from(s);
    let (tail_res, borrow) = if lo_tail == 0 { (0u64, false) } else { (8 - lo_tail, true) };
    let (mut sig, _) = sub256_borrow(hi.sig, lo_shifted, borrow);
    let mut tail = tail_res;
    let mut exp = hi.exp;

    if sig == [0; 4] && tail == 0 {
        return Hp::ZERO;
    }
    // Normalize left until MSB of sig is 1.
    while (sig[3] & MSB) == 0 {
        let top_tail = (tail >> 2) & 1;
        sig = shl256_1_with_lsb(sig, top_tail);
        tail = (tail << 1) & 7;
        exp = exp.saturating_sub(1);
    }
    let guard = (tail >> 2) & 1;
    let sticky = (tail & 3) != 0;
    if guard == 1 && (sticky || (sig[0] & 1) == 1) {
        let (inc, c) = add256(sig, [1, 0, 0, 0]);
        if c {
            sig = [0, 0, 0, MSB];
            exp = exp.saturating_add(1);
        } else {
            sig = inc;
        }
    }
    Hp { neg: hi.neg, exp, sig }
}

pub(crate) fn cmp256(a: [u64; 4], b: [u64; 4]) -> Ordering {
    for i in (0..4).rev() {
        let c = a[i].cmp(&b[i]);
        if c != Ordering::Equal {
            return c;
        }
    }
    Ordering::Equal
}

fn add256(a: [u64; 4], b: [u64; 4]) -> ([u64; 4], bool) {
    let mut out = [0u64; 4];
    let mut carry = 0u128;
    for i in 0..4 {
        let sum = u128::from(a[i]) + u128::from(b[i]) + carry;
        out[i] = sum as u64;
        carry = sum >> 64;
    }
    (out, carry != 0)
}

fn sub256_borrow(a: [u64; 4], b: [u64; 4], init_borrow: bool) -> ([u64; 4], bool) {
    let mut out = [0u64; 4];
    let mut borrow = u64::from(init_borrow);
    for i in 0..4 {
        let (d1, b1) = a[i].overflowing_sub(b[i]);
        let (d2, b2) = d1.overflowing_sub(borrow);
        out[i] = d2;
        borrow = u64::from(b1 || b2);
    }
    (out, borrow != 0)
}

fn shr256_1_with_msb(a: [u64; 4]) -> [u64; 4] {
    [(a[0] >> 1) | (a[1] << 63), (a[1] >> 1) | (a[2] << 63), (a[2] >> 1) | (a[3] << 63), (a[3] >> 1) | MSB]
}

fn shl256_1_with_lsb(a: [u64; 4], lsb: u64) -> [u64; 4] {
    [(a[0] << 1) | (lsb & 1), (a[1] << 1) | (a[0] >> 63), (a[2] << 1) | (a[1] >> 63), (a[3] << 1) | (a[2] >> 63)]
}

fn shr256(a: [u64; 4], shift: u32) -> [u64; 4] {
    if shift == 0 {
        return a;
    }
    if shift >= 256 {
        return [0; 4];
    }
    let word_shift = (shift / 64) as usize;
    let bit_shift = shift % 64;
    let mut out = [0u64; 4];
    for i in 0..(4 - word_shift) {
        let lo = a[i + word_shift] >> bit_shift;
        let hi = if bit_shift > 0 && i + word_shift + 1 < 4 { a[i + word_shift + 1] << (64 - bit_shift) } else { 0 };
        out[i] = lo | hi;
    }
    out
}

fn shl256(a: [u64; 4], shift: u32) -> [u64; 4] {
    if shift == 0 {
        return a;
    }
    if shift >= 256 {
        return [0; 4];
    }
    let word_shift = (shift / 64) as usize;
    let bit_shift = shift % 64;
    let mut out = [0u64; 4];
    for i in word_shift..4 {
        let hi = a[i - word_shift] << bit_shift;
        let lo = if bit_shift > 0 && i > word_shift { a[i - word_shift - 1] >> (64 - bit_shift) } else { 0 };
        out[i] = hi | lo;
    }
    out
}

fn get_bit256(a: [u64; 4], bit: u32) -> u64 {
    if bit >= 256 {
        return 0;
    }
    (a[(bit / 64) as usize] >> (bit % 64)) & 1
}

fn any_bit_below256(a: [u64; 4], count: u32) -> bool {
    if count == 0 {
        return false;
    }
    if count >= 256 {
        return a != [0; 4];
    }
    let full_words = (count / 64) as usize;
    let rem_bits = count % 64;
    for limb in a.iter().take(full_words) {
        if *limb != 0 {
            return true;
        }
    }
    rem_bits > 0 && (a[full_words] & ((1u64 << rem_bits) - 1)) != 0
}

/// Shifts `a` right by `shift` and returns `(shifted, guard, round, sticky)`.
fn shr256_grs(a: [u64; 4], shift: u32) -> ([u64; 4], u64, u64, bool) {
    if shift == 0 {
        return (a, 0, 0, false);
    }
    let shifted = shr256(a, shift);
    let guard = if shift <= 256 { get_bit256(a, shift - 1) } else { 0 };
    let round = if shift >= 2 && shift - 1 <= 256 { get_bit256(a, shift - 2) } else { 0 };
    let sticky = if shift >= 3 { any_bit_below256(a, shift - 2) } else { false };
    (shifted, guard, round, sticky)
}

fn clz256(a: [u64; 4]) -> u32 {
    for i in (0..4).rev() {
        if a[i] != 0 {
            return ((3 - i) as u32) * 64 + a[i].leading_zeros();
        }
    }
    256
}

fn mul256x256(a: [u64; 4], b: [u64; 4]) -> [u64; 8] {
    let mut out = [0u64; 8];
    for (i, &ai) in a.iter().enumerate() {
        let mut carry = 0u128;
        for (j, &bj) in b.iter().enumerate() {
            let acc = u128::from(ai) * u128::from(bj) + u128::from(out[i + j]) + carry;
            out[i + j] = acc as u64;
            carry = acc >> 64;
        }
        out[i + 4] = carry as u64;
    }
    out
}

fn cmp320(a: [u64; 5], b: [u64; 5]) -> Ordering {
    for i in (0..5).rev() {
        let c = a[i].cmp(&b[i]);
        if c != Ordering::Equal {
            return c;
        }
    }
    Ordering::Equal
}

fn sub320(a: [u64; 5], b: [u64; 5]) -> [u64; 5] {
    let mut out = [0u64; 5];
    let mut borrow = 0u64;
    for i in 0..5 {
        let (d1, b1) = a[i].overflowing_sub(b[i]);
        let (d2, b2) = d1.overflowing_sub(borrow);
        out[i] = d2;
        borrow = u64::from(b1 || b2);
    }
    out
}

fn shl320_1(a: [u64; 5]) -> [u64; 5] {
    [
        a[0] << 1,
        (a[1] << 1) | (a[0] >> 63),
        (a[2] << 1) | (a[1] >> 63),
        (a[3] << 1) | (a[2] >> 63),
        (a[4] << 1) | (a[3] >> 63),
    ]
}
