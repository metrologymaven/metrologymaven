//! Unsigned big integers for the exact conversion of `CORE.BUS.UNIT`.
//!
//! Little-endian 32-bit limbs in a `Vec`, always trimmed (no high zero limb; zero is empty).
//! Only what `unit` needs: build from a decimal digit string, a `u64` or limbs, multiply,
//! shift left, compare and subtract. Every limb step is done in `u64`, where
//! `(2^32 - 1)^2 + 2 (2^32 - 1) = 2^64 - 1` cannot overflow, and no slice is indexed, so
//! nothing here can panic. Sizes are bounded by the caller's parse limits (at most a few
//! thousand bits); `unit` refuses a shift outside its proven range before calling [`Big::shl`].

use core::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Big {
    limbs: Vec<u32>,
}

impl Big {
    pub(crate) fn zero() -> Big {
        Big { limbs: Vec::new() }
    }

    pub(crate) fn from_u64(v: u64) -> Big {
        let mut b = Big { limbs: vec![v as u32, (v >> 32) as u32] };
        b.trim();
        b
    }

    /// From little-endian 32-bit limbs.
    pub(crate) fn from_limbs(src: &[u32]) -> Big {
        let mut b = Big { limbs: src.to_vec() };
        b.trim();
        b
    }

    /// From ASCII decimal digits; `None` if a byte is not a digit.
    pub(crate) fn from_decimal(digits: &[u8]) -> Option<Big> {
        let mut b = Big::zero();
        for chunk in digits.chunks(9) {
            let mut v: u32 = 0;
            let mut scale: u32 = 1;
            for &c in chunk {
                if !c.is_ascii_digit() {
                    return None;
                }
                v = v * 10 + u32::from(c - b'0');
                scale *= 10;
            }
            b.mul_small_add(scale, v);
        }
        Some(b)
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// Number of significant bits (0 for zero).
    pub(crate) fn bit_len(&self) -> u64 {
        match self.limbs.last() {
            Some(top) => (self.limbs.len() as u64 - 1) * 32 + u64::from(32 - top.leading_zeros()),
            None => 0,
        }
    }

    /// `self = self * f + a`.
    pub(crate) fn mul_small_add(&mut self, f: u32, a: u32) {
        let mut carry = u64::from(a);
        for l in &mut self.limbs {
            let t = u64::from(*l) * u64::from(f) + carry;
            *l = t as u32;
            carry = t >> 32;
        }
        if carry != 0 {
            self.limbs.push(carry as u32);
        }
        self.trim();
    }

    /// `self * 5^n`.
    pub(crate) fn mul_pow5(&mut self, mut n: u64) {
        // 5^13 = 1_220_703_125 < 2^32.
        while n >= 13 {
            self.mul_small_add(1_220_703_125, 0);
            n -= 13;
        }
        let mut f: u32 = 1;
        for _ in 0..n {
            f *= 5;
        }
        self.mul_small_add(f, 0);
    }

    /// `self * other`.
    pub(crate) fn mul(&self, other: &Big) -> Big {
        let mut out = vec![0u32; self.limbs.len() + other.limbs.len()];
        for (i, &a) in self.limbs.iter().enumerate() {
            let mut carry: u64 = 0;
            for (o, &b) in out.iter_mut().skip(i).zip(other.limbs.iter()) {
                let t = u64::from(a) * u64::from(b) + u64::from(*o) + carry;
                *o = t as u32;
                carry = t >> 32;
            }
            if let Some(o) = out.get_mut(i + other.limbs.len()) {
                *o = carry as u32;
            }
        }
        let mut b = Big { limbs: out };
        b.trim();
        b
    }

    /// `self * 2^n`.
    pub(crate) fn shl(&self, n: u64) -> Big {
        if self.is_zero() {
            return Big::zero();
        }
        let words = (n / 32) as usize;
        let bits = (n % 32) as u32;
        let mut out = vec![0u32; words];
        out.reserve(self.limbs.len() + 1);
        if bits == 0 {
            out.extend_from_slice(&self.limbs);
        } else {
            let mut carry: u32 = 0;
            for &l in &self.limbs {
                out.push((l << bits) | carry);
                carry = l >> (32 - bits);
            }
            out.push(carry);
        }
        let mut b = Big { limbs: out };
        b.trim();
        b
    }

    pub(crate) fn cmp_mag(&self, other: &Big) -> Ordering {
        match self.limbs.len().cmp(&other.limbs.len()) {
            Ordering::Equal => {}
            o => return o,
        }
        for (a, b) in self.limbs.iter().rev().zip(other.limbs.iter().rev()) {
            match a.cmp(b) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        Ordering::Equal
    }

    /// `self - other`, or `None` when `other > self`.
    pub(crate) fn checked_sub(&self, other: &Big) -> Option<Big> {
        if self.cmp_mag(other) == Ordering::Less {
            return None;
        }
        let mut out = Vec::with_capacity(self.limbs.len());
        let mut borrow = false;
        let mut rhs = other.limbs.iter();
        for &a in &self.limbs {
            let b = rhs.next().copied().unwrap_or(0);
            let (d1, o1) = a.overflowing_sub(b);
            let (d2, o2) = d1.overflowing_sub(u32::from(borrow));
            out.push(d2);
            borrow = o1 || o2;
        }
        let mut b = Big { limbs: out };
        b.trim();
        Some(b)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn decimal_and_bits() {
        let b = Big::from_decimal(b"18446744073709551616").unwrap(); // 2^64
        assert_eq!(b, Big::from_u64(1).shl(64));
        assert_eq!(b.bit_len(), 65);
        assert_eq!(Big::from_decimal(b"000").unwrap(), Big::zero());
        assert_eq!(Big::zero().bit_len(), 0);
        assert!(Big::from_decimal(b"12a").is_none());
    }

    #[test]
    fn mul_sub_cmp() {
        let a = Big::from_u64(u64::MAX);
        let sq = a.mul(&a); // (2^64 - 1)^2 = 2^128 - 2^65 + 1
        let expect = Big::from_u64(1).shl(128).checked_sub(&Big::from_u64(1).shl(65)).unwrap();
        let expect = {
            let mut e = expect;
            e.mul_small_add(1, 1);
            e
        };
        assert_eq!(sq, expect);
        assert_eq!(Big::from_u64(3).checked_sub(&Big::from_u64(4)), None);
        assert_eq!(Big::from_u64(4).checked_sub(&Big::from_u64(4)).unwrap(), Big::zero());
        let mut p = Big::from_u64(1);
        p.mul_pow5(27);
        assert_eq!(p, Big::from_decimal(b"7450580596923828125").unwrap());
    }
}
