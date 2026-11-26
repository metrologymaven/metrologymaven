//! Exact decimals of the evaluator and the small unsigned big integer behind them.
//!
//! A [`Dec`] is `coef * 10^-scale` with `|coef| <= 10^38 - 1` and `-400 <= scale <= 400`.
//! The scale is kept as written (`2.50` has scale 2), so text survives arithmetic. Exact
//! results are formed on [`Big`] and then fitted by [`Dec::fit`]; nothing here rounds.

use std::cmp::Ordering;

/// Most significant digits a [`Dec`] holds (REQ-AUTO.PROG.EXPR.EVAL-002).
pub const MAX_DEC_DIGITS: u32 = 38;
/// Largest `|scale|` of a [`Dec`].
pub const MAX_SCALE: i32 = 400;
/// `10^38 - 1`, the largest coefficient magnitude.
const MAX_COEF: u128 = 99_999_999_999_999_999_999_999_999_999_999_999_999;
/// Most 32-bit limbs a [`Big`] may hold. The largest value the evaluator forms is about
/// `10^840 * 2^1400` (an exact comparison of an `f64` against a decimal with an angle
/// factor), far below `2^(32 * 512)`; a larger one is refused, never computed.
const MAX_LIMBS: usize = 512;

/// Unsigned big integer: little-endian 32-bit limbs, no high zero limb (zero is empty).
/// Every limb step is done in `u64`, where `(2^32 - 1)^2 + 2 (2^32 - 1) < 2^64`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Big(Vec<u32>);

impl Big {
    pub(crate) fn from_u128(v: u128) -> Big {
        let mut b = Big(Vec::new());
        let mut v = v;
        while v != 0 {
            b.0.push((v & 0xffff_ffff) as u32);
            v >>= 32;
        }
        b
    }

    pub(crate) fn from_limbs(limbs: &[u32]) -> Big {
        let mut b = Big(limbs.to_vec());
        b.trim();
        b
    }

    fn trim(&mut self) {
        while self.0.last() == Some(&0) {
            self.0.pop();
        }
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether the value grew past the capacity bound (the caller then fails closed).
    pub(crate) fn too_big(&self) -> bool {
        self.0.len() > MAX_LIMBS
    }

    pub(crate) fn mul_u32(&mut self, m: u32) {
        let mut carry: u64 = 0;
        for limb in self.0.iter_mut() {
            let t = u64::from(*limb) * u64::from(m) + carry;
            *limb = (t & 0xffff_ffff) as u32;
            carry = t >> 32;
        }
        if carry != 0 {
            self.0.push(carry as u32);
        }
        self.trim();
    }

    /// `self * 10^k`. Stops growing once past the capacity bound.
    pub(crate) fn mul_pow10(&mut self, k: u32) {
        let mut k = k;
        while k >= 9 && !self.too_big() {
            self.mul_u32(1_000_000_000);
            k -= 9;
        }
        let mut m: u32 = 1;
        for _ in 0..k {
            m *= 10;
        }
        self.mul_u32(m);
    }

    /// `self * 2^bits`. Stops growing once past the capacity bound.
    pub(crate) fn shl(&mut self, bits: u32) {
        if self.is_zero() {
            return;
        }
        let words = (bits / 32) as usize;
        if words > MAX_LIMBS {
            self.0 = vec![1; MAX_LIMBS + 1];
            return;
        }
        let bits = bits % 32;
        if bits != 0 {
            let mut carry: u32 = 0;
            for limb in self.0.iter_mut() {
                let t = (u64::from(*limb) << bits) | u64::from(carry);
                *limb = (t & 0xffff_ffff) as u32;
                carry = (t >> 32) as u32;
            }
            if carry != 0 {
                self.0.push(carry);
            }
        }
        let mut out = vec![0u32; words];
        out.append(&mut self.0);
        self.0 = out;
    }

    pub(crate) fn mul(&self, o: &Big) -> Big {
        if self.is_zero() || o.is_zero() {
            return Big(Vec::new());
        }
        if self.0.len() + o.0.len() > MAX_LIMBS + 1 {
            return Big(vec![1; MAX_LIMBS + 1]);
        }
        let mut out = vec![0u32; self.0.len() + o.0.len()];
        for (i, a) in self.0.iter().enumerate() {
            let mut carry: u64 = 0;
            for (j, b) in o.0.iter().enumerate() {
                if let Some(slot) = out.get_mut(i + j) {
                    let t = u64::from(*a) * u64::from(*b) + u64::from(*slot) + carry;
                    *slot = (t & 0xffff_ffff) as u32;
                    carry = t >> 32;
                }
            }
            let mut k = i + o.0.len();
            while carry != 0 {
                match out.get_mut(k) {
                    Some(slot) => {
                        let t = u64::from(*slot) + carry;
                        *slot = (t & 0xffff_ffff) as u32;
                        carry = t >> 32;
                    }
                    None => break,
                }
                k += 1;
            }
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    pub(crate) fn add(&self, o: &Big) -> Big {
        let n = self.0.len().max(o.0.len());
        let mut out = Vec::with_capacity(n + 1);
        let mut carry: u64 = 0;
        for i in 0..n {
            let a = u64::from(self.0.get(i).copied().unwrap_or(0));
            let b = u64::from(o.0.get(i).copied().unwrap_or(0));
            let t = a + b + carry;
            out.push((t & 0xffff_ffff) as u32);
            carry = t >> 32;
        }
        if carry != 0 {
            out.push(carry as u32);
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    /// `self - o`; the caller guarantees `self >= o`.
    pub(crate) fn sub(&self, o: &Big) -> Big {
        let mut out = Vec::with_capacity(self.0.len());
        let mut borrow: i64 = 0;
        for (i, a) in self.0.iter().enumerate() {
            let b = i64::from(o.0.get(i).copied().unwrap_or(0));
            let mut t = i64::from(*a) - b - borrow;
            borrow = 0;
            if t < 0 {
                t += 1 << 32;
                borrow = 1;
            }
            out.push((t & 0xffff_ffff) as u32);
        }
        let mut b = Big(out);
        b.trim();
        b
    }

    pub(crate) fn cmp_mag(&self, o: &Big) -> Ordering {
        if self.0.len() != o.0.len() {
            return self.0.len().cmp(&o.0.len());
        }
        for (a, b) in self.0.iter().rev().zip(o.0.iter().rev()) {
            if a != b {
                return a.cmp(b);
            }
        }
        Ordering::Equal
    }

    /// Divides in place by `d` (non-zero) and returns the remainder.
    pub(crate) fn divrem_u32(&mut self, d: u32) -> u32 {
        if d == 0 {
            return 0;
        }
        let mut rem: u64 = 0;
        for limb in self.0.iter_mut().rev() {
            let cur = (rem << 32) | u64::from(*limb);
            *limb = (cur / u64::from(d)) as u32;
            rem = cur % u64::from(d);
        }
        self.trim();
        rem as u32
    }

    pub(crate) fn to_u128(&self) -> Option<u128> {
        if self.0.len() > 4 {
            return None;
        }
        let mut v: u128 = 0;
        for limb in self.0.iter().rev() {
            v = (v << 32) | u128::from(*limb);
        }
        Some(v)
    }
}

/// A signed big integer, for the exact sums of [`Dec`].
#[derive(Debug, Clone)]
pub(crate) struct SBig {
    pub(crate) neg: bool,
    pub(crate) mag: Big,
}

impl SBig {
    fn of(c: i128) -> SBig {
        SBig { neg: c < 0, mag: Big::from_u128(c.unsigned_abs()) }
    }

    fn add(&self, o: &SBig) -> SBig {
        if self.neg == o.neg {
            return SBig { neg: self.neg, mag: self.mag.add(&o.mag) };
        }
        match self.mag.cmp_mag(&o.mag) {
            Ordering::Less => SBig { neg: o.neg, mag: o.mag.sub(&self.mag) },
            _ => SBig { neg: self.neg, mag: self.mag.sub(&o.mag) },
        }
    }
}

/// Why an exact decimal operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecFail {
    /// The exact result needs more than [`MAX_DEC_DIGITS`] digits or a scale beyond
    /// [`MAX_SCALE`]: `decimal_overflow`.
    Overflow,
    /// Division or remainder by zero: `div_zero`.
    DivZero,
    /// Text outside the decimal grammar.
    Syntax,
}

/// An exact decimal `coef * 10^-scale`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dec {
    coef: i128,
    scale: i16,
}

impl Dec {
    /// The exact value of an integer (scale 0).
    pub fn from_i64(v: i64) -> Dec {
        Dec { coef: i128::from(v), scale: 0 }
    }

    /// Parses `<digits>` or `<digits>.<digits>` (the grammar's number text, sign apart),
    /// keeping the written scale; [`Dec::fit`] then applies the bounds. Text outside that
    /// grammar, or longer than 4096 bytes, is [`DecFail::Syntax`].
    pub(crate) fn from_text(text: &str, negative: bool) -> Result<Dec, DecFail> {
        let (int, frac) = match text.split_once('.') {
            Some((i, f)) => (i, f),
            None => (text, ""),
        };
        let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if text.len() > 4096
            || int.is_empty()
            || !digits(int)
            || !digits(frac)
            || (text.contains('.') && frac.is_empty())
        {
            return Err(DecFail::Syntax);
        }
        let scale = i64::try_from(frac.len()).map_err(|_| DecFail::Syntax)?;
        let mut mag = Big(Vec::new());
        for b in int.bytes().chain(frac.bytes()) {
            mag.mul_u32(10);
            mag = mag.add(&Big::from_u128(u128::from(b.wrapping_sub(b'0'))));
        }
        Dec::fit(SBig { neg: negative, mag }, scale)
    }

    /// Fits an exact value `c * 10^-scale` (REQ-AUTO.PROG.EXPR.EVAL-002): trailing zeros are
    /// dropped (coefficient / 10, scale - 1) only while the coefficient has more than
    /// [`MAX_DEC_DIGITS`] digits or the scale is above [`MAX_SCALE`]; zeros are appended
    /// (coefficient * 10, scale + 1) only while the scale is below `-MAX_SCALE` and the
    /// coefficient stays within 38 digits. If the bounds still fail, the value is
    /// [`DecFail::Overflow`].
    pub(crate) fn fit(c: SBig, scale: i64) -> Result<Dec, DecFail> {
        let SBig { neg, mut mag } = c;
        let mut scale = scale;
        if mag.too_big() {
            return Err(DecFail::Overflow);
        }
        let max = Big::from_u128(MAX_COEF);
        loop {
            let wide = mag.cmp_mag(&max) == Ordering::Greater;
            if !(wide || scale > i64::from(MAX_SCALE)) {
                break;
            }
            let mut q = mag.clone();
            if q.divrem_u32(10) != 0 {
                return Err(DecFail::Overflow);
            }
            mag = q;
            scale -= 1;
        }
        while scale < -i64::from(MAX_SCALE) {
            let mut t = mag.clone();
            t.mul_u32(10);
            if t.cmp_mag(&max) == Ordering::Greater {
                return Err(DecFail::Overflow);
            }
            mag = t;
            scale += 1;
        }
        let coef = mag.to_u128().and_then(|m| i128::try_from(m).ok()).ok_or(DecFail::Overflow)?;
        let scale = i16::try_from(scale).map_err(|_| DecFail::Overflow)?;
        Ok(Dec { coef: if neg { -coef } else { coef }, scale })
    }

    pub fn coef(&self) -> i128 {
        self.coef
    }

    pub fn scale(&self) -> i32 {
        i32::from(self.scale)
    }

    pub fn is_zero(&self) -> bool {
        self.coef == 0
    }

    pub fn is_negative(&self) -> bool {
        self.coef < 0
    }

    pub(crate) fn sbig(&self) -> SBig {
        SBig::of(self.coef)
    }

    /// The coefficient scaled to `scale` (>= own scale), exactly.
    fn at(&self, scale: i64) -> SBig {
        let mut s = self.sbig();
        let k = scale - i64::from(self.scale);
        s.mag.mul_pow10(u32::try_from(k).unwrap_or(u32::MAX));
        s
    }

    pub(crate) fn neg(&self) -> Dec {
        Dec { coef: -self.coef, scale: self.scale }
    }

    pub(crate) fn abs(&self) -> Dec {
        Dec { coef: self.coef.abs(), scale: self.scale }
    }

    /// Exact sum; the result scale is the larger scale (then fitted).
    pub(crate) fn add(&self, o: &Dec) -> Result<Dec, DecFail> {
        let s = i64::from(self.scale.max(o.scale));
        Dec::fit(self.at(s).add(&o.at(s)), s)
    }

    pub(crate) fn sub(&self, o: &Dec) -> Result<Dec, DecFail> {
        self.add(&o.neg())
    }

    /// Exact product; the result scale is the sum of the scales (then fitted).
    pub(crate) fn mul(&self, o: &Dec) -> Result<Dec, DecFail> {
        let a = self.sbig();
        let b = o.sbig();
        let p = SBig { neg: (a.neg != b.neg) && !(a.mag.is_zero() || b.mag.is_zero()), mag: a.mag.mul(&b.mag) };
        Dec::fit(p, i64::from(self.scale) + i64::from(o.scale))
    }

    /// Exact truncated remainder (the sign of the dividend), at the larger scale.
    pub(crate) fn rem(&self, o: &Dec) -> Result<Dec, DecFail> {
        if o.is_zero() {
            return Err(DecFail::DivZero);
        }
        let s = i64::from(self.scale.max(o.scale));
        let a = self.at(s);
        let b = o.at(s);
        // |a| mod |b| by repeated subtraction of shifted |b| (binary long division).
        let mut r = a.mag.clone();
        let mut d = b.mag.clone();
        let mut shifts: u32 = 0;
        while d.cmp_mag(&r) != Ordering::Greater && !d.too_big() {
            d.shl(1);
            shifts += 1;
        }
        for _ in 0..=shifts {
            if r.cmp_mag(&d) != Ordering::Less {
                r = r.sub(&d);
            }
            d.divrem_u32(2);
        }
        Dec::fit(SBig { neg: a.neg && !r.is_zero(), mag: r }, s)
    }

    /// Exact order.
    pub(crate) fn cmp(&self, o: &Dec) -> Ordering {
        let s = i64::from(self.scale.max(o.scale));
        let a = self.at(s);
        let b = o.at(s);
        let sign = |x: &SBig| {
            if x.mag.is_zero() {
                0
            } else if x.neg {
                -1
            } else {
                1
            }
        };
        let (sa, sb) = (sign(&a), sign(&b));
        if sa != sb || sa == 0 {
            return sa.cmp(&sb);
        }
        let m = a.mag.cmp_mag(&b.mag);
        if sa < 0 {
            m.reverse()
        } else {
            m
        }
    }

    /// The written form: `[-]int.frac` with exactly `scale` digits after the point, or the
    /// coefficient followed by `-scale` zeros (no point) when the scale is not positive.
    pub fn text(&self) -> String {
        let sign = if self.coef < 0 { "-" } else { "" };
        let digits = self.coef.unsigned_abs().to_string();
        let scale = self.scale();
        if scale <= 0 {
            return format!("{sign}{digits}{}", "0".repeat(scale.unsigned_abs() as usize));
        }
        let s = scale.unsigned_abs() as usize;
        let padded = if digits.len() <= s { format!("{}{digits}", "0".repeat(s + 1 - digits.len())) } else { digits };
        let split = padded.len() - s;
        let (i, f) = padded.split_at(split);
        format!("{sign}{i}.{f}")
    }

    /// `floor`, `ceil` or round half to even to an integer, exactly; `None` if the result
    /// does not fit `i64`.
    pub(crate) fn to_int(&self, mode: IntMode) -> Option<i64> {
        let scale = self.scale();
        if scale <= 0 {
            let mut m = Big::from_u128(self.coef.unsigned_abs());
            m.mul_pow10(scale.unsigned_abs());
            let v = i128::try_from(m.to_u128()?).ok()?;
            return i64::try_from(if self.coef < 0 { -v } else { v }).ok();
        }
        let mut q = Big::from_u128(self.coef.unsigned_abs());
        // Remainder digits: compare the dropped part with one half exactly.
        let mut dropped = Big(Vec::new());
        let mut place = Big::from_u128(1);
        for _ in 0..scale {
            let d = q.divrem_u32(10);
            let mut term = place.clone();
            term.mul_u32(d);
            dropped = dropped.add(&term);
            place.mul_u32(10);
        }
        let tq = i128::try_from(q.to_u128()?).ok()?;
        let frac_zero = dropped.is_zero();
        let mut half = place.clone();
        half.divrem_u32(2);
        let neg = self.coef < 0;
        let up = match mode {
            IntMode::Floor => neg && !frac_zero,
            IntMode::Ceil => !neg && !frac_zero,
            IntMode::HalfEven => match dropped.cmp_mag(&half) {
                Ordering::Greater => true,
                Ordering::Less => false,
                Ordering::Equal => tq % 2 == 1,
            },
        };
        // `up` moves the magnitude by one for floor of negatives, ceil of positives, and
        // rounding away from zero.
        let mag = if up { tq.checked_add(1)? } else { tq };
        i64::try_from(if neg { -mag } else { mag }).ok()
    }
}

/// How [`Dec::to_int`] and the `f64` equivalent pick an integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntMode {
    Floor,
    Ceil,
    HalfEven,
}
