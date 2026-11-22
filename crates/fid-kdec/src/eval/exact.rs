//! Exact sign of a sum of products of binary64 values, decimals and integers.
//!
//! Every finite `f64` is the dyadic rational `m * 2^e`, every [`Decimal`] is
//! `c * 10^k = c * 2^k * 5^k` and every integer is itself, so a signed sum of products of
//! them is a rational number whose sign can be decided with integer arithmetic only. Each
//! term is written `±N * 2^a * 5^b`; all terms are scaled to the smallest `a` and `b`, the
//! positive and negative magnitudes are added separately and compared. Nothing is rounded,
//! so a value one ulp beside a limit is never put on the wrong side.
//!
//! The big integer is a little-endian `Vec<u32>`. Its size is bounded by the inputs: the
//! exponent spread of finite `f64` values (`-1074 ..= 971`) and of [`Decimal`] values
//! (`-400 ..= 400`), at most two factors per term, so at most a few thousand bits. A spread
//! beyond [`MAX_SHIFT`] is refused rather than computed.

use core::cmp::Ordering;

use fid_num::kdec::Decimal;

/// Largest exponent spread (in powers of 2 or of 5) between two terms. Far above anything a
/// valid input produces (about 3000 for powers of 2 and 1600 for powers of 5).
const MAX_SHIFT: u64 = 8192;

/// Why an exact sum could not be decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExactError {
    /// A binary64 factor is NaN or infinite.
    NonFinite,
    /// The exponent spread exceeds [`MAX_SHIFT`] (unreachable for valid inputs).
    Capacity,
}

/// One factor of a product.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Factor {
    /// A finite binary64 value.
    F(f64),
    /// An exact decimal.
    D(Decimal),
    /// A non-negative integer.
    I(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Big {
    limbs: Vec<u32>,
}

impl Big {
    fn from_u128(mut v: u128) -> Big {
        let mut limbs = Vec::new();
        while v > 0 {
            limbs.push((v & 0xffff_ffff) as u32);
            v >>= 32;
        }
        Big { limbs }
    }

    fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    fn mul(&self, other: &Big) -> Big {
        if self.is_zero() || other.is_zero() {
            return Big { limbs: Vec::new() };
        }
        let mut out = vec![0u32; self.limbs.len() + other.limbs.len()];
        for (i, &a) in self.limbs.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &b) in other.limbs.iter().enumerate() {
                let slot = &mut out[i + j];
                let t = u64::from(*slot) + u64::from(a) * u64::from(b) + carry;
                *slot = (t & 0xffff_ffff) as u32;
                carry = t >> 32;
            }
            let mut k = i + other.limbs.len();
            while carry > 0 {
                let slot = &mut out[k];
                let t = u64::from(*slot) + carry;
                *slot = (t & 0xffff_ffff) as u32;
                carry = t >> 32;
                k += 1;
            }
        }
        let mut r = Big { limbs: out };
        r.trim();
        r
    }

    fn mul_small(&mut self, m: u32) {
        let mut carry = 0u64;
        for l in &mut self.limbs {
            let t = u64::from(*l) * u64::from(m) + carry;
            *l = (t & 0xffff_ffff) as u32;
            carry = t >> 32;
        }
        if carry > 0 {
            self.limbs.push(carry as u32);
        }
    }

    fn mul_pow5(&mut self, mut n: u64) {
        // 5^13 = 1 220 703 125 < 2^32.
        while n >= 13 {
            self.mul_small(1_220_703_125);
            n -= 13;
        }
        let mut f = 1u32;
        for _ in 0..n {
            f *= 5;
        }
        if f > 1 {
            self.mul_small(f);
        }
    }

    fn shl(&mut self, bits: u64) {
        if self.is_zero() {
            return;
        }
        let words = (bits / 32) as usize;
        let rem = (bits % 32) as u32;
        if rem > 0 {
            let mut carry = 0u32;
            for l in &mut self.limbs {
                let v = *l;
                *l = (v << rem) | carry;
                carry = v >> (32 - rem);
            }
            if carry > 0 {
                self.limbs.push(carry);
            }
        }
        if words > 0 {
            let mut v = vec![0u32; words];
            v.extend_from_slice(&self.limbs);
            self.limbs = v;
        }
    }

    fn add(&mut self, other: &Big) {
        if self.limbs.len() < other.limbs.len() {
            self.limbs.resize(other.limbs.len(), 0);
        }
        let mut carry = 0u64;
        for (i, l) in self.limbs.iter_mut().enumerate() {
            let b = other.limbs.get(i).copied().unwrap_or(0);
            let t = u64::from(*l) + u64::from(b) + carry;
            *l = (t & 0xffff_ffff) as u32;
            carry = t >> 32;
        }
        if carry > 0 {
            self.limbs.push(carry as u32);
        }
    }

    fn cmp_mag(&self, other: &Big) -> Ordering {
        let n = self.limbs.len().cmp(&other.limbs.len());
        if n != Ordering::Equal {
            return n;
        }
        for (a, b) in self.limbs.iter().rev().zip(other.limbs.iter().rev()) {
            let c = a.cmp(b);
            if c != Ordering::Equal {
                return c;
            }
        }
        Ordering::Equal
    }
}

/// `±N * 2^e2 * 5^e5`.
struct Term {
    neg: bool,
    mag: Big,
    e2: i64,
    e5: i64,
}

fn factor_parts(f: Factor) -> Result<(bool, Big, i64, i64), ExactError> {
    match f {
        Factor::F(x) => {
            if !x.is_finite() {
                return Err(ExactError::NonFinite);
            }
            let bits = x.to_bits();
            let neg = bits >> 63 == 1;
            let bexp = ((bits >> 52) & 0x7ff) as i64;
            let frac = bits & ((1u64 << 52) - 1);
            let (m, e) = if bexp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), bexp - 1075) };
            Ok((neg && m != 0, Big::from_u128(u128::from(m)), e, 0))
        }
        Factor::D(d) => {
            let k = i64::from(d.exp10());
            Ok((d.is_negative(), Big::from_u128(d.coeff()), k, k))
        }
        Factor::I(n) => Ok((false, Big::from_u128(u128::from(n)), 0, 0)),
    }
}

fn term(neg: bool, factors: &[Factor]) -> Result<Term, ExactError> {
    let mut t = Term { neg, mag: Big::from_u128(1), e2: 0, e5: 0 };
    for &f in factors {
        let (n, m, e2, e5) = factor_parts(f)?;
        t.neg ^= n;
        t.mag = t.mag.mul(&m);
        t.e2 += e2;
        t.e5 += e5;
    }
    Ok(t)
}

/// The sign of `sum_i (neg_i ? -1 : 1) * prod(factors_i)`, exactly.
pub(crate) fn sign_of_sum(terms: &[(bool, &[Factor])]) -> Result<Ordering, ExactError> {
    let mut ts = Vec::with_capacity(terms.len());
    for (neg, fs) in terms {
        let t = term(*neg, fs)?;
        if !t.mag.is_zero() {
            ts.push(t);
        }
    }
    let Some(first) = ts.first() else { return Ok(Ordering::Equal) };
    let mut min2 = first.e2;
    let mut min5 = first.e5;
    for t in &ts {
        min2 = min2.min(t.e2);
        min5 = min5.min(t.e5);
    }
    let mut pos = Big { limbs: Vec::new() };
    let mut negs = Big { limbs: Vec::new() };
    for t in ts {
        let s2 = u64::try_from(t.e2 - min2).map_err(|_| ExactError::Capacity)?;
        let s5 = u64::try_from(t.e5 - min5).map_err(|_| ExactError::Capacity)?;
        if s2 > MAX_SHIFT || s5 > MAX_SHIFT {
            return Err(ExactError::Capacity);
        }
        let mut m = t.mag;
        m.mul_pow5(s5);
        m.shl(s2);
        if t.neg {
            negs.add(&m);
        } else {
            pos.add(&m);
        }
    }
    Ok(pos.cmp_mag(&negs))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::parse(s).unwrap()
    }

    #[test]
    fn signs() {
        use Factor::{D, F, I};
        // 0.1 (f64) is slightly above the decimal 0.1.
        assert_eq!(sign_of_sum(&[(false, &[F(0.1)]), (true, &[D(d("0.1"))])]), Ok(Ordering::Greater));
        // 0.5 * 2 - 1 = 0
        assert_eq!(sign_of_sum(&[(false, &[D(d("0.5")), I(2)]), (true, &[I(1)])]), Ok(Ordering::Equal));
        // 0.25 * 4.0 - 1 = 0 with a negative zero term.
        assert_eq!(
            sign_of_sum(&[(false, &[D(d("0.25")), F(4.0)]), (true, &[I(1)]), (false, &[F(-0.0)])]),
            Ok(Ordering::Equal)
        );
        // tiny vs huge exponents
        assert_eq!(sign_of_sum(&[(false, &[F(f64::from_bits(1))]), (true, &[D(d("1e-400"))])]), Ok(Ordering::Greater));
        assert_eq!(sign_of_sum(&[(false, &[D(d("1e400")), D(d("1e-400"))]), (true, &[I(1)])]), Ok(Ordering::Equal));
        assert_eq!(sign_of_sum(&[(true, &[F(f64::MAX)]), (false, &[D(d("1e400"))])]), Ok(Ordering::Greater));
        assert_eq!(sign_of_sum(&[(false, &[F(f64::NAN)])]), Err(ExactError::NonFinite));
        assert_eq!(sign_of_sum(&[]), Ok(Ordering::Equal));
    }
}
