//! Exact decimal type and comparator. Spec node `KRN.NUM.KDEC`.
//!
//! Drawing limits stay exact decimals. [`cmp`] orders a finite `f64` (an exact dyadic
//! rational) against a [`Decimal`] by exact integer arithmetic on a fixed-capacity,
//! stack-allocated big integer, so a value one ulp beside a limit is never put on the
//! wrong side and never reported as equal to it.

use core::cmp::Ordering;
use core::fmt;
use core::str::FromStr;

use crate::err::{check_finite, NumError};

/// Most significant digits [`Decimal::parse`] accepts (REQ-KRN.NUM.KDEC-001).
pub const MAX_DIGITS: u32 = 40;
/// Smallest normalised base-10 exponent of a [`Decimal`] (and of a written exponent).
pub const EXP_MIN: i32 = -400;
/// Largest normalised base-10 exponent of a [`Decimal`] (and of a written exponent).
pub const EXP_MAX: i32 = 400;
/// Longest input, in bytes, that [`Decimal::parse`] reads.
pub const MAX_INPUT_LEN: usize = 1024;

/// Why a decimal operation failed. Carried in [`NumError::Domain`] as `detail`
/// (see [`KdecError::detail`] and [`KdecError::of`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdecError {
    /// The input is not `[+-]digits[.digits][(e|E)[+-]digits]`.
    Syntax,
    /// More than [`MAX_DIGITS`] significant digits, or more than [`MAX_INPUT_LEN`] bytes.
    TooManyDigits,
    /// A written or normalised exponent is outside [`EXP_MIN`]..=[`EXP_MAX`].
    ExponentRange,
    /// The normalised coefficient does not fit in `u128`.
    CoefficientOverflow,
    /// The internal big integer of [`cmp`] ran out of limbs. Unreachable for any
    /// [`Decimal`] (the capacity is derived from the limits, see `LIMBS`); kept so that an
    /// impossible case is reported as an error and never as a wrong ordering.
    Capacity,
}

impl KdecError {
    /// The `detail` string used in [`NumError::Domain`].
    pub const fn detail(self) -> &'static str {
        match self {
            KdecError::Syntax => "syntax",
            KdecError::TooManyDigits => "too_many_digits",
            KdecError::ExponentRange => "exponent_range",
            KdecError::CoefficientOverflow => "coefficient_overflow",
            KdecError::Capacity => "big_integer_capacity",
        }
    }

    /// Recovers the kind from a [`NumError`] produced by this module (an `op` starting
    /// with `kdec.`); `None` for any other error.
    pub fn of(e: &NumError) -> Option<KdecError> {
        let NumError::Domain { op, detail } = e else { return None };
        if !op.starts_with("kdec.") {
            return None;
        }
        [
            KdecError::Syntax,
            KdecError::TooManyDigits,
            KdecError::ExponentRange,
            KdecError::CoefficientOverflow,
            KdecError::Capacity,
        ]
        .into_iter()
        .find(|k| k.detail() == *detail)
    }

    fn at(self, op: &'static str) -> NumError {
        NumError::Domain { op, detail: self.detail() }
    }
}

/// An exact decimal `(-1)^neg * coeff * 10^exp10`.
///
/// Always normalised: `coeff` has no trailing zeros, zero is `(false, 0, 0)` (the sign of
/// zero is dropped), and `exp10` lies in [`EXP_MIN`]..=[`EXP_MAX`]. Structural equality is
/// therefore numeric equality.
///
/// Zero has no exponent: [`Decimal::new`], [`Decimal::shift10`] and [`Decimal::mul`] map
/// any zero to [`Decimal::ZERO`] whatever the exponent. Only [`Decimal::parse`] checks the
/// *written* exponent of every input, zero included, as an input-size rule (`0e401` is
/// rejected as text, while `Decimal::new(false, 0, 401)` is zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Decimal {
    neg: bool,
    coeff: u128,
    exp10: i16,
}

impl Decimal {
    /// The decimal zero.
    pub const ZERO: Decimal = Decimal { neg: false, coeff: 0, exp10: 0 };

    /// Builds `(-1)^neg * coeff * 10^exp10`, normalising it.
    pub fn new(neg: bool, coeff: u128, exp10: i32) -> Result<Decimal, NumError> {
        Self::build("kdec.new", neg, coeff, i64::from(exp10))
    }

    fn build(op: &'static str, neg: bool, mut coeff: u128, mut exp10: i64) -> Result<Decimal, NumError> {
        if coeff == 0 {
            return Ok(Decimal::ZERO);
        }
        while coeff % 10 == 0 {
            coeff /= 10;
            exp10 += 1;
        }
        if !(i64::from(EXP_MIN)..=i64::from(EXP_MAX)).contains(&exp10) {
            return Err(KdecError::ExponentRange.at(op));
        }
        let exp10 = i16::try_from(exp10).map_err(|_| KdecError::ExponentRange.at(op))?;
        Ok(Decimal { neg, coeff, exp10 })
    }

    /// True for a negative, non-zero decimal.
    pub fn is_negative(&self) -> bool {
        self.neg
    }

    /// True for zero.
    pub fn is_zero(&self) -> bool {
        self.coeff == 0
    }

    /// The normalised integer coefficient (no trailing zeros unless zero).
    pub fn coeff(&self) -> u128 {
        self.coeff
    }

    /// The normalised base-10 exponent.
    pub fn exp10(&self) -> i32 {
        i32::from(self.exp10)
    }

    /// Strict, bounded parser (REQ-KRN.NUM.KDEC-001). Grammar:
    /// `[+-] digits [ "." digits ] [ ("e"|"E") [+-] digits ]`, ASCII only, no whitespace.
    ///
    /// Limits: at most [`MAX_INPUT_LEN`] bytes, at most [`MAX_DIGITS`] significant digits
    /// (from the first non-zero digit to the last digit written), a written exponent and a
    /// normalised exponent within [`EXP_MIN`]..=[`EXP_MAX`], and a normalised coefficient
    /// that fits in `u128`. Errors are checked in that order after syntax; nothing is
    /// allocated.
    pub fn parse(s: &str) -> Result<Decimal, NumError> {
        const OP: &str = "kdec.parse";
        let b = s.as_bytes();
        if b.len() > MAX_INPUT_LEN {
            return Err(KdecError::TooManyDigits.at(OP));
        }
        let digit = |i: usize| b.get(i).copied().filter(u8::is_ascii_digit).map(|c| c - b'0');
        let mut i = 0;
        let mut neg = false;
        if let Some(&c @ (b'+' | b'-')) = b.get(i) {
            neg = c == b'-';
            i += 1;
        }
        let mut acc = Accum::default();
        let start = i;
        while let Some(d) = digit(i) {
            acc.push(d);
            i += 1;
        }
        if i == start {
            return Err(KdecError::Syntax.at(OP));
        }
        let mut frac_digits: i64 = 0;
        if b.get(i) == Some(&b'.') {
            i += 1;
            while let Some(d) = digit(i) {
                acc.push(d);
                frac_digits += 1;
                i += 1;
            }
            if frac_digits == 0 {
                return Err(KdecError::Syntax.at(OP));
            }
        }
        let mut exp: i64 = 0;
        if matches!(b.get(i), Some(b'e' | b'E')) {
            i += 1;
            let mut eneg = false;
            if let Some(&c @ (b'+' | b'-')) = b.get(i) {
                eneg = c == b'-';
                i += 1;
            }
            let start = i;
            while let Some(d) = digit(i) {
                // Saturate well above the limit; the range check below rejects it.
                if exp <= 100_000 {
                    exp = exp * 10 + i64::from(d);
                }
                i += 1;
            }
            if i == start {
                return Err(KdecError::Syntax.at(OP));
            }
            if eneg {
                exp = -exp;
            }
        }
        if i != b.len() {
            return Err(KdecError::Syntax.at(OP));
        }
        if let Some(e) = acc.err {
            return Err(e.at(OP));
        }
        if !(i64::from(EXP_MIN)..=i64::from(EXP_MAX)).contains(&exp) {
            return Err(KdecError::ExponentRange.at(OP));
        }
        Self::build(OP, neg, acc.coeff, exp - frac_digits + i64::from(acc.pending_zeros))
    }

    /// Exact multiplication by `10^k` (REQ-KRN.NUM.KDEC-004), for example `k = -3` for mm to m.
    pub fn shift10(&self, k: i32) -> Result<Decimal, NumError> {
        Self::build("kdec.shift10", self.neg, self.coeff, i64::from(self.exp10) + i64::from(k))
    }

    /// Exact product (REQ-KRN.NUM.KDEC-004), for example by the inch factor `0.0254`.
    /// Fails with [`KdecError::CoefficientOverflow`] if the product of the coefficients
    /// does not fit in `u128`, and with [`KdecError::ExponentRange`] if the result is out of range.
    #[allow(clippy::should_implement_trait)]
    pub fn mul(&self, other: &Decimal) -> Result<Decimal, NumError> {
        const OP: &str = "kdec.mul";
        if self.is_zero() || other.is_zero() {
            return Ok(Decimal::ZERO);
        }
        let c = self.coeff.checked_mul(other.coeff).ok_or_else(|| KdecError::CoefficientOverflow.at(OP))?;
        Self::build(OP, self.neg != other.neg, c, i64::from(self.exp10) + i64::from(other.exp10))
    }

    /// The correctly rounded (round-half-even) binary64 value (REQ-KRN.NUM.KDEC-005), by
    /// `core` float parsing of the canonical string. Returns [`NumError::Infinite`] when the
    /// decimal is beyond the largest finite `f64`. Values below half the smallest subnormal
    /// round to a signed zero.
    pub fn to_f64(&self) -> Result<f64, NumError> {
        const OP: &str = "kdec.to_f64";
        // Canonical form fits in 1 + 39 + 2 + 3 = 45 bytes; formatted on the stack.
        let mut buf = StackBuf { bytes: [0; 64], len: 0 };
        fmt::write(&mut buf, format_args!("{self}")).map_err(|_| KdecError::Capacity.at(OP))?;
        let text = buf.bytes.get(..buf.len).and_then(|b| core::str::from_utf8(b).ok());
        let x: f64 = text.and_then(|t| t.parse().ok()).ok_or_else(|| KdecError::Syntax.at(OP))?;
        check_finite(OP, x)
    }
}

/// Fixed-size text buffer for [`Decimal::to_f64`]; fails instead of growing.
struct StackBuf {
    bytes: [u8; 64],
    len: usize,
}

impl fmt::Write for StackBuf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        let dst = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        dst.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// Coefficient accumulator for the parser: digits are added one at a time, zeros after the
/// last non-zero digit are held back so that they never overflow the coefficient.
#[derive(Default)]
struct Accum {
    coeff: u128,
    sig: u32,
    pending_zeros: u32,
    err: Option<KdecError>,
}

impl Accum {
    fn push(&mut self, d: u8) {
        if self.sig == 0 && d == 0 {
            return; // leading zero
        }
        // Too many digits wins over an earlier coefficient overflow.
        self.sig = self.sig.saturating_add(1);
        if self.sig > MAX_DIGITS {
            self.err = Some(KdecError::TooManyDigits);
            return;
        }
        if self.err.is_some() {
            return;
        }
        if d == 0 {
            self.pending_zeros += 1;
            return;
        }
        let mut c = Some(self.coeff);
        for _ in 0..=self.pending_zeros {
            c = c.and_then(|c| c.checked_mul(10));
        }
        match c.and_then(|c| c.checked_add(u128::from(d))) {
            Some(c) => {
                self.coeff = c;
                self.pending_zeros = 0;
            }
            None => self.err = Some(KdecError::CoefficientOverflow),
        }
    }
}

impl FromStr for Decimal {
    type Err = NumError;
    fn from_str(s: &str) -> Result<Decimal, NumError> {
        Decimal::parse(s)
    }
}

/// Canonical form `[-]<coeff>e<exp10>`, for example `1005e-5`; zero is `0e0`.
impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}e{}", if self.neg { "-" } else { "" }, self.coeff, self.exp10)
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Decimal) -> Ordering {
        cmp_dec(self, other)
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Decimal) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn signum(zero: bool, neg: bool) -> i8 {
    match (zero, neg) {
        (true, _) => 0,
        (false, true) => -1,
        (false, false) => 1,
    }
}

/// Exact ordering of the real numbers `x` and `d` (REQ-KRN.NUM.KDEC-002, -003).
///
/// `Equal` only when both denote the same real number. `-0.0` equals zero. Non-finite `x`
/// gives [`NumError::NotANumber`] or [`NumError::Infinite`]. [`KdecError::Capacity`] is
/// documented for completeness and cannot occur for a valid [`Decimal`].
pub fn cmp(x: f64, d: &Decimal) -> Result<Ordering, NumError> {
    const OP: &str = "kdec.cmp";
    let x = check_finite(OP, x)?;
    let bits = x.to_bits();
    let xneg = bits >> 63 == 1;
    let bexp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    // x = m * 2^e exactly.
    let (m, e) = if bexp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), bexp - 1075) };
    let xs = signum(m == 0, xneg);
    let ds = signum(d.is_zero(), d.neg);
    if xs != ds || xs == 0 {
        return Ok(xs.cmp(&ds));
    }
    // Same sign, both non-zero: compare m * 2^e with c * 10^k after moving every factor to a
    // non-negative exponent: left = m * 2^max(e,0) * 10^max(-k,0), right = c * 10^max(k,0) * 2^max(-e,0).
    let k = d.exp10();
    let mut left = Big::from_u128(u128::from(m));
    left.shl(e.max(0).unsigned_abs());
    left.mul_pow10((-k).max(0).unsigned_abs());
    let mut right = Big::from_u128(d.coeff);
    right.mul_pow10(k.max(0).unsigned_abs());
    right.shl((-e).max(0).unsigned_abs());
    if left.overflow || right.overflow {
        // Unreachable for any `Decimal` (see `LIMBS`); reported, never wrong.
        return Err(KdecError::Capacity.at(OP));
    }
    let mag = left.cmp_mag(&right);
    Ok(if xs < 0 { mag.reverse() } else { mag })
}

/// Exact ordering of two decimals. Total; consistent with `==`.
pub fn cmp_dec(a: &Decimal, b: &Decimal) -> Ordering {
    let sa = signum(a.is_zero(), a.neg);
    let sb = signum(b.is_zero(), b.neg);
    if sa != sb || sa == 0 {
        return sa.cmp(&sb);
    }
    // Decimal magnitude: digits(c) + k - 1 is the position of the leading digit.
    let pa = ndigits(a.coeff) + a.exp10();
    let pb = ndigits(b.coeff) + b.exp10();
    let mag = if pa != pb {
        pa.cmp(&pb)
    } else {
        // Same leading position, so |k_a - k_b| <= 38 and the scaled value stays below
        // 2^128 * 10^38 < 2^255: well inside the big-integer capacity.
        let (ka, kb) = (a.exp10(), b.exp10());
        let lo = ka.min(kb);
        let mut x = Big::from_u128(a.coeff);
        x.mul_pow10((ka - lo).unsigned_abs());
        let mut y = Big::from_u128(b.coeff);
        y.mul_pow10((kb - lo).unsigned_abs());
        x.cmp_mag(&y)
    };
    if sa < 0 {
        mag.reverse()
    } else {
        mag
    }
}

fn ndigits(mut c: u128) -> i32 {
    let mut n = 0;
    while c > 0 {
        c /= 10;
        n += 1;
    }
    n
}

/// Limb count of [`Big`]. Worst case within the parse limits is the right side of [`cmp`]
/// with a 128-bit coefficient, `k = 400` and `e = -1074` (smallest subnormal):
/// `128 + ceil(400 * log2 10) + 1074 = 128 + 1329 + 1074 = 2531` bits, since
/// `400 * log2 10 = 1328.77`. The left side is at most `53 + 971 + 1329 = 2353` bits.
/// 80 limbs of 32 bits = 2560 bits cover both.
const LIMBS: usize = 80;

/// Fixed-capacity unsigned big integer, little-endian 32-bit limbs, no heap, no recursion.
/// Shared with `KRN.NUM.FMT`.
#[derive(Clone, Copy)]
pub(crate) struct Big {
    limbs: [u32; LIMBS],
    len: usize,
    overflow: bool,
}

impl Big {
    pub(crate) fn from_u128(mut v: u128) -> Big {
        let mut b = Big { limbs: [0; LIMBS], len: 0, overflow: false };
        for limb in b.limbs.iter_mut().take(4) {
            if v == 0 {
                break;
            }
            *limb = v as u32;
            v >>= 32;
            b.len += 1;
        }
        b
    }

    pub(crate) fn mul_small(&mut self, f: u32) {
        let mut carry: u64 = 0;
        for limb in self.limbs.iter_mut().take(self.len) {
            let p = u64::from(*limb) * u64::from(f) + carry;
            *limb = p as u32;
            carry = p >> 32;
        }
        if carry != 0 {
            match self.limbs.get_mut(self.len) {
                Some(l) => {
                    *l = carry as u32;
                    self.len += 1;
                }
                None => self.overflow = true,
            }
        }
        self.trim();
    }

    pub(crate) fn mul_pow10(&mut self, mut n: u32) {
        while n >= 9 {
            self.mul_small(1_000_000_000);
            n -= 9;
        }
        if n > 0 {
            self.mul_small(10u32.pow(n));
        }
    }

    pub(crate) fn shl(&mut self, n: u32) {
        if self.len == 0 || n == 0 {
            return;
        }
        let words = (n / 32) as usize;
        let bits = n % 32;
        let mut out = [0u32; LIMBS];
        for (i, &v) in self.limbs.iter().enumerate().take(self.len) {
            let wide = u64::from(v) << bits;
            for (j, part) in [(i + words, wide as u32), (i + words + 1, (wide >> 32) as u32)] {
                if part == 0 {
                    continue;
                }
                match out.get_mut(j) {
                    Some(o) => *o |= part,
                    None => self.overflow = true,
                }
            }
        }
        self.limbs = out;
        self.len = (self.len + words + 1).min(LIMBS);
        self.trim();
    }

    fn trim(&mut self) {
        while self.len > 0 && self.limbs.get(self.len - 1) == Some(&0) {
            self.len -= 1;
        }
    }

    pub(crate) fn cmp_mag(&self, other: &Big) -> Ordering {
        self.len.cmp(&other.len).then_with(|| {
            let a = self.limbs.iter().take(self.len).rev();
            let b = other.limbs.iter().take(other.len).rev();
            a.cmp(b)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_shift_and_scale() {
        let mut a = Big::from_u128(1);
        a.shl(64);
        let b = Big::from_u128(1u128 << 64);
        assert_eq!(a.cmp_mag(&b), Ordering::Equal);
        let mut c = Big::from_u128(1);
        c.mul_pow10(38);
        assert_eq!(c.cmp_mag(&Big::from_u128(10u128.pow(38))), Ordering::Equal);
        let mut d = Big::from_u128(u128::MAX);
        d.mul_pow10(400);
        d.shl(1074);
        assert!(!d.overflow, "worst case must fit");
        d.shl(32);
        assert!(d.overflow, "one limb more must report overflow");
    }

    #[test]
    fn zero_big_is_inert() {
        let mut z = Big::from_u128(0);
        z.shl(100);
        z.mul_pow10(50);
        assert_eq!(z.len, 0);
        assert!(!z.overflow);
    }
}
