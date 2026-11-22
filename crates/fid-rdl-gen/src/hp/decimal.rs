//! Exact decimal parsing, exact ties-to-even `round16`, and >=50-digit reference formatting
//! for [`Hp`] (REQ-UNC.RDL.GEN.HPREC-001, REQ-UNC.RDL.GEN.HPREC-003).
//!
//! Uses a small pure-Rust unsigned big integer (`BigUint`) so decimal-to-binary and
//! binary-to-decimal conversions round with exact ties-to-even semantics and zero floating-point
//! rounding shortcuts.

use std::cmp::Ordering;

use super::{Hp, HpError};

/// Minimum significant digits written for reference outputs (REQ-UNC.RDL.GEN.HPREC-003).
pub const MIN_REFERENCE_DIGITS: usize = 50;

/// Result of [`round16`]: exact 16-significant-digit decimal (ties to even) and its nearest `f64`.
#[derive(Debug, Clone, PartialEq)]
pub struct Decimal16 {
    /// Canonical normalized 16-significant-digit decimal string (e.g. `1.234567890123456e+00`).
    pub decimal: String,
    /// The IEEE-754 `f64` nearest to `decimal` (ties to even).
    pub value: f64,
    /// Lowercase `0x<16 hex>` bit encoding of `value`.
    pub hex: String,
}

/// High-precision reference output formatted with at least 50 significant decimal digits plus
/// IEEE-754 binary64 hex (REQ-UNC.RDL.GEN.HPREC-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceValue {
    pub decimal: String,
    pub hex: String,
}

impl ReferenceValue {
    /// Formats `x` with `digits` significant decimal digits (`digits >= 50`) and its IEEE-754 hex.
    pub fn from_hp(x: &Hp, digits: usize) -> Result<Self, HpError> {
        if digits < MIN_REFERENCE_DIGITS {
            return Err(HpError::Domain("reference value requires at least 50 significant digits"));
        }
        let decimal = format_decimal_sig(x, digits)?;
        let hex = x.to_hex_f64();
        Ok(Self { decimal, hex })
    }
}

/// Rounds `x` to the nearest 16-significant-digit decimal (ties to even), plus the `f64`
/// nearest to that decimal (REQ-UNC.RDL.GEN.HPREC-001, AC-1).
pub fn round16(x: &Hp) -> Decimal16 {
    let decimal = format_decimal_sig(x, 16).unwrap_or_else(|_| "0.000000000000000e+00".to_string());
    let parsed = decimal.parse::<f64>().unwrap_or(0.0);
    let value = if parsed == 0.0 { 0.0 } else { parsed };
    let hex = format!("0x{:016x}", value.to_bits());
    Decimal16 { decimal, value, hex }
}

/// Formats `x` to `digits` (`>= 1`) significant decimal digits, exact ties-to-even, in
/// canonical scientific notation `[-]d.ddd...e[+-]dd`.
pub fn format_decimal_sig(x: &Hp, digits: usize) -> Result<String, HpError> {
    if digits == 0 || digits > 1000 {
        return Err(HpError::Domain("decimal digits must be in 1..=1000"));
    }
    if x.is_zero() {
        let mut s = String::with_capacity(digits + 6);
        s.push('0');
        if digits > 1 {
            s.push('.');
            for _ in 1..digits {
                s.push('0');
            }
        }
        s.push_str("e+00");
        return Ok(s);
    }

    // Exact dyadic value: |x| = sig * 2^(exp - 255).
    let bin_exp = (x.exp as i64) - 255;
    let sig_big = BigUint::from_u256(x.sig);

    // Estimate e10 = floor(log10(|x|)) using log10(2) ≈ 0.3010299956639812.
    let mut e10 = ((x.exp as f64) * std::f64::consts::LOG10_2).floor() as i64;

    // Verify 10^e10 <= |x| < 10^(e10 + 1) using exact big-int comparisons.
    loop {
        if cmp_dyadic_pow10(&sig_big, bin_exp, e10) == Ordering::Less {
            e10 -= 1;
        } else if cmp_dyadic_pow10(&sig_big, bin_exp, e10 + 1) != Ordering::Less {
            e10 += 1;
        } else {
            break;
        }
    }

    // Compute scaled value = |x| * 10^(digits - 1 - e10) = num / den.
    let dec_shift = (digits as i64) - 1 - e10;
    let mut num = sig_big;
    let mut den = BigUint::one();

    // Combine powers of 2: total power of 2 = bin_exp + dec_shift.
    let pow2 = bin_exp + dec_shift;
    if pow2 >= 0 {
        num.shl_assign(pow2 as u32);
    } else {
        den.shl_assign((-pow2) as u32);
    }
    // Powers of 5: dec_shift.
    if dec_shift >= 0 {
        num.mul_pow5(dec_shift as u32);
    } else {
        den.mul_pow5((-dec_shift) as u32);
    }

    let (mut q, r) = BigUint::div_rem(&num, &den);
    // Exact ties-to-even: compare 2*r vs den.
    let mut two_r = r;
    two_r.shl_assign(1);
    let cmp_half = two_r.cmp(&den);
    let round_up = match cmp_half {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => q.is_odd(),
    };
    if round_up {
        q.add_u64(1);
    }

    // Check overflow from 999...9 -> 1000...0 (10^digits).
    let pow10_d = BigUint::pow10(digits as u32);
    if q >= pow10_d {
        let (q_next, _) = BigUint::div_rem(&q, &BigUint::from_u64(10));
        q = q_next;
        e10 += 1;
    }

    let digit_str = q.to_decimal_string();
    let mut out = String::with_capacity(digits + 10);
    if x.neg {
        out.push('-');
    }
    let bytes = digit_str.as_bytes();
    if let Some(&first) = bytes.first() {
        out.push(first as char);
    }
    if bytes.len() > 1 {
        out.push('.');
        for &b in &bytes[1..] {
            out.push(b as char);
        }
    }
    if e10 >= 0 {
        out.push_str(&format!("e+{e10:02}"));
    } else {
        out.push_str(&format!("e-{:02}", -e10));
    }
    Ok(out)
}

/// Parses an arbitrary-precision decimal string (e.g. `"-123.456789e-12"`) into exact
/// nearest-even 256-bit [`Hp`].
pub fn parse_decimal_hp(input: &str) -> Result<Hp, HpError> {
    let s = input.trim();
    if s.is_empty() || s.len() > 2048 {
        return Err(HpError::Parse("empty or overly long decimal string"));
    }
    let bytes = s.as_bytes();
    let mut idx = 0usize;
    let neg = match bytes[idx] {
        b'-' => {
            idx += 1;
            true
        }
        b'+' => {
            idx += 1;
            false
        }
        _ => false,
    };
    if idx >= bytes.len() {
        return Err(HpError::Parse("missing digits"));
    }

    let mut digits = Vec::new();
    let mut seen_dot = false;
    let mut frac_digits = 0i64;
    let mut seen_digit = false;

    while idx < bytes.len() {
        let b = bytes[idx];
        if b.is_ascii_digit() {
            seen_digit = true;
            digits.push(b - b'0');
            if seen_dot {
                frac_digits += 1;
            }
            idx += 1;
        } else if b == b'.' && !seen_dot {
            seen_dot = true;
            idx += 1;
        } else if b == b'e' || b == b'E' {
            idx += 1;
            break;
        } else {
            return Err(HpError::Parse("unexpected character in decimal mantissa"));
        }
    }
    if !seen_digit {
        return Err(HpError::Parse("no digits in decimal mantissa"));
    }

    let mut exp10: i64 = 0;
    if idx < bytes.len() || bytes.last().is_some_and(|&c| c == b'e' || c == b'E') {
        if idx >= bytes.len() {
            return Err(HpError::Parse("missing exponent digits"));
        }
        let exp_str = &s[idx..];
        exp10 = exp_str.parse::<i64>().map_err(|_| HpError::Parse("invalid decimal exponent"))?;
    }
    let total_exp10 = exp10.saturating_sub(frac_digits);
    if !(-10_000..=10_000).contains(&total_exp10) {
        return Err(HpError::Parse("decimal exponent out of supported range"));
    }

    // Build exact integer mantissa.
    let mut mant = BigUint::zero();
    for d in digits {
        mant.mul_u64(10);
        mant.add_u64(u64::from(d));
    }
    if mant.is_zero() {
        return Ok(Hp::ZERO);
    }

    // Value = mant * 10^total_exp10 = num / den.
    let mut num = mant;
    let mut den = BigUint::one();
    if total_exp10 >= 0 {
        num.mul_pow5(total_exp10 as u32);
        num.shl_assign(total_exp10 as u32);
    } else {
        let p = (-total_exp10) as u32;
        den.mul_pow5(p);
        den.shl_assign(p);
    }

    // Find binary exponent `exp` such that 2^exp <= num/den < 2^(exp+1),
    // i.e. shift so that 2^255 <= (num * 2^(255 - exp)) / den < 2^256.
    let mut exp = (num.bit_len() as i64) - (den.bit_len() as i64);
    // Refine exact binary exponent by comparing num vs den * 2^exp.
    loop {
        if cmp_ratio_pow2(&num, &den, exp) == Ordering::Less {
            exp -= 1;
        } else if cmp_ratio_pow2(&num, &den, exp + 1) != Ordering::Less {
            exp += 1;
        } else {
            break;
        }
    }

    let shift = 255 - exp;
    if shift >= 0 {
        num.shl_assign(shift as u32);
    } else {
        den.shl_assign((-shift) as u32);
    }
    let (mut q, r) = BigUint::div_rem(&num, &den);
    let mut two_r = r;
    two_r.shl_assign(1);
    let round_up = match two_r.cmp(&den) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => q.is_odd(),
    };
    if round_up {
        q.add_u64(1);
    }

    let mut final_exp = exp as i32;
    let sig = if q.bit_len() > 256 {
        final_exp = final_exp.saturating_add(1);
        [0, 0, 0, 1u64 << 63]
    } else {
        q.to_u256()
    };
    Ok(Hp { neg, exp: final_exp, sig })
}

impl Hp {
    /// Parses a decimal string into exact nearest-even 256-bit `Hp`.
    pub fn parse_decimal(s: &str) -> Result<Hp, HpError> {
        parse_decimal_hp(s)
    }

    /// Formats `self` to `digits` significant decimal digits with exact ties-to-even rounding.
    pub fn to_decimal_sig(&self, digits: usize) -> Result<String, HpError> {
        format_decimal_sig(self, digits)
    }
}

/// Compares `sig * 2^bin_exp` against `10^e10`.
fn cmp_dyadic_pow10(sig: &BigUint, bin_exp: i64, e10: i64) -> Ordering {
    // Compare sig * 2^(bin_exp - e10) vs 5^e10.
    let mut lhs = sig.clone();
    let mut rhs = BigUint::one();
    let pow2 = bin_exp - e10;
    if pow2 >= 0 {
        lhs.shl_assign(pow2 as u32);
    } else {
        rhs.shl_assign((-pow2) as u32);
    }
    if e10 >= 0 {
        rhs.mul_pow5(e10 as u32);
    } else {
        lhs.mul_pow5((-e10) as u32);
    }
    lhs.cmp(&rhs)
}

/// Compares `num / den` against `2^exp`.
fn cmp_ratio_pow2(num: &BigUint, den: &BigUint, exp: i64) -> Ordering {
    if exp >= 0 {
        let mut rhs = den.clone();
        rhs.shl_assign(exp as u32);
        num.cmp(&rhs)
    } else {
        let mut lhs = num.clone();
        lhs.shl_assign((-exp) as u32);
        lhs.cmp(den)
    }
}

// ============================================================================
// Minimal pure-Rust arbitrary-precision unsigned integer (little-endian u64 limbs)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
struct BigUint {
    limbs: Vec<u64>,
}

impl BigUint {
    fn zero() -> Self {
        Self { limbs: Vec::new() }
    }

    fn one() -> Self {
        Self { limbs: vec![1] }
    }

    fn from_u64(v: u64) -> Self {
        if v == 0 {
            Self::zero()
        } else {
            Self { limbs: vec![v] }
        }
    }

    fn from_u256(a: [u64; 4]) -> Self {
        let mut b = Self { limbs: a.to_vec() };
        b.trim();
        b
    }

    fn to_u256(&self) -> [u64; 4] {
        let mut out = [0u64; 4];
        for (i, &w) in self.limbs.iter().take(4).enumerate() {
            out[i] = w;
        }
        out
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    fn is_odd(&self) -> bool {
        self.limbs.first().is_some_and(|&w| (w & 1) == 1)
    }

    fn bit_len(&self) -> usize {
        let Some(&top) = self.limbs.last() else {
            return 0;
        };
        (self.limbs.len() - 1) * 64 + (64 - top.leading_zeros() as usize)
    }

    fn get_bit(&self, bit: usize) -> u64 {
        let word = bit / 64;
        if word >= self.limbs.len() {
            return 0;
        }
        (self.limbs[word] >> (bit % 64)) & 1
    }

    fn set_bit(&mut self, bit: usize) {
        let word = bit / 64;
        if word >= self.limbs.len() {
            self.limbs.resize(word + 1, 0);
        }
        self.limbs[word] |= 1u64 << (bit % 64);
    }

    fn add_u64(&mut self, v: u64) {
        if v == 0 {
            return;
        }
        let mut carry = u128::from(v);
        for w in &mut self.limbs {
            let sum = u128::from(*w) + carry;
            *w = sum as u64;
            carry = sum >> 64;
            if carry == 0 {
                return;
            }
        }
        if carry != 0 {
            self.limbs.push(carry as u64);
        }
    }

    fn mul_u64(&mut self, m: u64) {
        if m == 0 || self.is_zero() {
            self.limbs.clear();
            return;
        }
        if m == 1 {
            return;
        }
        let mut carry = 0u128;
        for w in &mut self.limbs {
            let acc = u128::from(*w) * u128::from(m) + carry;
            *w = acc as u64;
            carry = acc >> 64;
        }
        if carry != 0 {
            self.limbs.push(carry as u64);
        }
    }

    fn mul_pow5(&mut self, mut p: u32) {
        // 5^27 fits in u64 (7_450_580_596_923_828_125).
        const POW5_27: u64 = 7_450_580_596_923_828_125;
        while p >= 27 {
            self.mul_u64(POW5_27);
            p -= 27;
        }
        if p > 0 {
            let mut factor = 1u64;
            for _ in 0..p {
                factor *= 5;
            }
            self.mul_u64(factor);
        }
    }

    fn pow10(p: u32) -> Self {
        let mut b = Self::one();
        b.mul_pow5(p);
        b.shl_assign(p);
        b
    }

    fn shl_assign(&mut self, shift: u32) {
        if shift == 0 || self.is_zero() {
            return;
        }
        let word_shift = (shift / 64) as usize;
        let bit_shift = shift % 64;
        if bit_shift == 0 {
            let mut out = vec![0u64; word_shift];
            out.extend_from_slice(&self.limbs);
            self.limbs = out;
            return;
        }
        let old_len = self.limbs.len();
        let mut out = vec![0u64; word_shift + old_len + 1];
        let mut carry = 0u64;
        for i in 0..old_len {
            let w = self.limbs[i];
            out[word_shift + i] = (w << bit_shift) | carry;
            carry = w >> (64 - bit_shift);
        }
        out[word_shift + old_len] = carry;
        self.limbs = out;
        self.trim();
    }

    fn sub_assign(&mut self, rhs: &Self) {
        let mut borrow = 0u64;
        for i in 0..self.limbs.len() {
            let b = rhs.limbs.get(i).copied().unwrap_or(0);
            let (d1, b1) = self.limbs[i].overflowing_sub(b);
            let (d2, b2) = d1.overflowing_sub(borrow);
            self.limbs[i] = d2;
            borrow = u64::from(b1 || b2);
        }
        self.trim();
    }

    fn div_rem_u64(&self, d: u64) -> (Self, u64) {
        if self.is_zero() || d == 0 {
            return (Self::zero(), 0);
        }
        let mut q = vec![0u64; self.limbs.len()];
        let mut rem = 0u128;
        let d128 = u128::from(d);
        for i in (0..self.limbs.len()).rev() {
            let cur = (rem << 64) | u128::from(self.limbs[i]);
            q[i] = (cur / d128) as u64;
            rem = cur % d128;
        }
        let mut out = Self { limbs: q };
        out.trim();
        (out, rem as u64)
    }

    /// Exact binary long division returning `(quotient, remainder)`.
    fn div_rem(num: &Self, den: &Self) -> (Self, Self) {
        if num < den {
            return (Self::zero(), num.clone());
        }
        if den.limbs.len() == 1 {
            let (q, r) = num.div_rem_u64(den.limbs[0]);
            return (q, Self::from_u64(r));
        }
        let mut q = Self::zero();
        let mut r = Self::zero();
        for bit in (0..num.bit_len()).rev() {
            r.shl_assign(1);
            if num.get_bit(bit) == 1 {
                if r.limbs.is_empty() {
                    r.limbs.push(1);
                } else {
                    r.limbs[0] |= 1;
                }
            }
            if r >= *den {
                r.sub_assign(den);
                q.set_bit(bit);
            }
        }
        q.trim();
        r.trim();
        (q, r)
    }

    fn to_decimal_string(&self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        const CHUNK_BASE: u64 = 1_000_000_000;
        let mut chunks = Vec::new();
        let mut cur = self.clone();
        while !cur.is_zero() {
            let (next, rem) = cur.div_rem_u64(CHUNK_BASE);
            chunks.push(rem);
            cur = next;
        }
        let mut out = String::new();
        if let Some(&first) = chunks.last() {
            out.push_str(&first.to_string());
        }
        for &c in chunks.iter().rev().skip(1) {
            out.push_str(&format!("{c:09}"));
        }
        out
    }
}

impl Ord for BigUint {
    fn cmp(&self, other: &Self) -> Ordering {
        self.limbs.len().cmp(&other.limbs.len()).then_with(|| {
            for i in (0..self.limbs.len()).rev() {
                let c = self.limbs[i].cmp(&other.limbs[i]);
                if c != Ordering::Equal {
                    return c;
                }
            }
            Ordering::Equal
        })
    }
}

impl PartialOrd for BigUint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
