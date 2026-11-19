//! Fixed-capacity unsigned big integer for exact unit conversion (`GEO.FRAME.UNITS`).
//!
//! Little-endian 32-bit limbs in a fixed array: no heap, no recursion, every loop bounded by
//! [`LIMBS`]. Running out of limbs never wraps: it sets a sticky `overflow` flag that callers
//! turn into an error, so an impossible case is reported and never computed wrongly.
//!
//! Capacity. The largest intermediate in `units` is below 2^1560 (see the bound in
//! `units::round_rational` and `units::format`); 72 limbs hold 2304 bits.

use core::cmp::Ordering;

/// Limb count (72 x 32 = 2304 bits).
pub(crate) const LIMBS: usize = 72;

/// Unsigned integer of at most `32 * LIMBS` bits.
#[derive(Clone, Copy)]
pub(crate) struct Big {
    limbs: [u32; LIMBS],
    len: usize,
    overflow: bool,
}

impl Big {
    pub(crate) const ZERO: Big = Big { limbs: [0; LIMBS], len: 0, overflow: false };

    pub(crate) fn from_u64(v: u64) -> Big {
        let mut b = Big::ZERO;
        if let Some(l) = b.limbs.get_mut(0) {
            *l = v as u32;
        }
        if let Some(l) = b.limbs.get_mut(1) {
            *l = (v >> 32) as u32;
        }
        b.len = 2;
        b.trim();
        b
    }

    /// From little-endian 32-bit limbs.
    pub(crate) fn from_limbs(src: &[u32]) -> Big {
        let mut b = Big::ZERO;
        for (i, &v) in src.iter().enumerate() {
            match b.limbs.get_mut(i) {
                Some(l) => *l = v,
                None => b.overflow = v != 0 || b.overflow,
            }
        }
        b.len = src.len().min(LIMBS);
        b.trim();
        b
    }

    pub(crate) fn overflowed(&self) -> bool {
        self.overflow
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn is_odd(&self) -> bool {
        self.limbs.first().is_some_and(|l| l & 1 == 1)
    }

    /// The low 64 bits.
    pub(crate) fn low_u64(&self) -> u64 {
        let lo = u64::from(self.limbs.first().copied().unwrap_or(0));
        let hi = u64::from(self.limbs.get(1).copied().unwrap_or(0));
        (hi << 32) | lo
    }

    /// Number of significant bits (0 for zero).
    pub(crate) fn bit_len(&self) -> u32 {
        match self.len.checked_sub(1).and_then(|i| self.limbs.get(i)) {
            Some(top) => (self.len as u32 - 1) * 32 + (32 - top.leading_zeros()),
            None => 0,
        }
    }

    fn trim(&mut self) {
        while self.len > 0 && self.limbs.get(self.len - 1) == Some(&0) {
            self.len -= 1;
        }
    }

    fn bit(&self, i: u32) -> bool {
        self.limbs.get((i / 32) as usize).is_some_and(|l| (l >> (i % 32)) & 1 == 1)
    }

    fn set_bit(&mut self, i: u32) {
        let w = (i / 32) as usize;
        match self.limbs.get_mut(w) {
            Some(l) => {
                *l |= 1 << (i % 32);
                self.len = self.len.max(w + 1);
            }
            None => self.overflow = true,
        }
    }

    /// `self = self * f + a`.
    pub(crate) fn mul_add_small(&mut self, f: u32, a: u32) {
        let mut carry = u64::from(a);
        for limb in self.limbs.iter_mut().take(self.len) {
            // (2^32-1)^2 + (2^32-1) < 2^64: never wraps.
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

    /// `self = self * 10^n`.
    pub(crate) fn mul_pow10(&mut self, mut n: u32) {
        while n >= 9 && !self.overflow {
            self.mul_add_small(1_000_000_000, 0);
            n -= 9;
        }
        if n > 0 {
            self.mul_add_small(10u32.pow(n), 0);
        }
    }

    /// Full product. Sets `overflow` (conservatively) when `self.len + other.len > LIMBS`.
    pub(crate) fn mul(&self, other: &Big) -> Big {
        let mut out = Big::ZERO;
        out.overflow = self.overflow || other.overflow;
        if self.len + other.len > LIMBS {
            out.overflow = true;
            return out;
        }
        for (i, &a) in self.limbs.iter().enumerate().take(self.len) {
            let mut carry = 0u64;
            for (j, &b) in other.limbs.iter().enumerate().take(other.len) {
                if let Some(slot) = out.limbs.get_mut(i + j) {
                    // (2^32-1)^2 + 2 (2^32-1) = 2^64 - 1: never wraps.
                    let t = u64::from(*slot) + u64::from(a) * u64::from(b) + carry;
                    *slot = t as u32;
                    carry = t >> 32;
                }
            }
            if let Some(slot) = out.limbs.get_mut(i + other.len) {
                *slot = carry as u32;
            }
        }
        out.len = self.len + other.len;
        out.trim();
        out
    }

    /// `self = self * 2^n`.
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

    pub(crate) fn cmp_mag(&self, other: &Big) -> Ordering {
        self.len.cmp(&other.len).then_with(|| {
            let a = self.limbs.iter().take(self.len).rev();
            let b = other.limbs.iter().take(other.len).rev();
            a.cmp(b)
        })
    }

    /// `self += 1`.
    pub(crate) fn inc(&mut self) {
        self.mul_add_small(1, 1);
    }

    /// `self -= other`; requires `self >= other` (otherwise sets `overflow`).
    fn sub_assign(&mut self, other: &Big) {
        if other.len > self.len {
            self.overflow = true;
            return;
        }
        let mut borrow = 0u64;
        for (i, limb) in self.limbs.iter_mut().enumerate().take(self.len) {
            let b = u64::from(other.limbs.get(i).copied().unwrap_or(0)) + borrow;
            let a = u64::from(*limb);
            if a >= b {
                *limb = (a - b) as u32;
                borrow = 0;
            } else {
                *limb = ((1u64 << 32) + a - b) as u32;
                borrow = 1;
            }
        }
        if borrow != 0 {
            self.overflow = true;
        }
        self.trim();
    }

    /// `self = 2 * self + bit`.
    fn shl1_with(&mut self, bit: bool) {
        let mut carry = u32::from(bit);
        for limb in self.limbs.iter_mut().take(self.len) {
            let out = *limb >> 31;
            *limb = (*limb << 1) | carry;
            carry = out;
        }
        if carry != 0 {
            match self.limbs.get_mut(self.len) {
                Some(l) => {
                    *l = carry;
                    self.len += 1;
                }
                None => self.overflow = true,
            }
        }
    }

    /// Restoring binary long division: `(floor(self / d), self mod d)`, `None` for `d = 0`.
    /// At most `32 * LIMBS` iterations on fixed arrays.
    pub(crate) fn div_rem(&self, d: &Big) -> Option<(Big, Big)> {
        if d.is_zero() {
            return None;
        }
        let mut q = Big::ZERO;
        let mut r = Big::ZERO;
        q.overflow = self.overflow || d.overflow;
        for i in (0..self.bit_len()).rev() {
            r.shl1_with(self.bit(i));
            if r.cmp_mag(d) != Ordering::Less {
                r.sub_assign(d);
                q.set_bit(i);
            }
        }
        q.overflow |= r.overflow;
        Some((q, r))
    }

    /// `self = floor(self / d)`, returning `self mod d`; `d = 0` sets `overflow`.
    pub(crate) fn div_small(&mut self, d: u32) -> u32 {
        if d == 0 {
            self.overflow = true;
            return 0;
        }
        let mut rem = 0u64;
        for limb in self.limbs.iter_mut().take(self.len).rev() {
            let cur = (rem << 32) | u64::from(*limb);
            *limb = (cur / u64::from(d)) as u32;
            rem = cur % u64::from(d);
        }
        self.trim();
        rem as u32
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn to_u128(b: &Big) -> u128 {
        b.limbs.iter().take(4).rev().fold(0u128, |acc, &l| (acc << 32) | u128::from(l))
    }

    #[test]
    fn mul_matches_u128() {
        let a = Big::from_u64(0xFFFF_FFFF_FFFF_FFFF);
        let b = Big::from_u64(0xFFFF_FFFF_FFFF_FFFF);
        let p = a.mul(&b);
        assert_eq!(to_u128(&p), u128::from(u64::MAX) * u128::from(u64::MAX));
        assert!(Big::ZERO.mul(&a).is_zero());
        let mut c = Big::from_u64(12_345);
        c.mul_pow10(20);
        assert_eq!(to_u128(&c), 12_345 * 10u128.pow(20));
    }

    #[test]
    fn div_rem_matches_u128() {
        let cases: [(u128, u64); 5] =
            [(0, 1), (7, 2), (u128::MAX, 3), (1u128 << 100, 1 << 63), (98_765_432_109_876, 1)];
        for (n, d) in cases {
            let nb = Big::from_limbs(&[n as u32, (n >> 32) as u32, (n >> 64) as u32, (n >> 96) as u32]);
            let (q, r) = nb.div_rem(&Big::from_u64(d)).unwrap();
            assert_eq!((to_u128(&q), to_u128(&r)), (n / u128::from(d), n % u128::from(d)));
        }
        assert!(Big::from_u64(1).div_rem(&Big::ZERO).is_none());
    }

    #[test]
    fn capacity_is_reported() {
        let mut a = Big::from_u64(1);
        a.shl(32 * LIMBS as u32 - 1);
        assert!(!a.overflowed());
        assert_eq!(a.bit_len(), 32 * LIMBS as u32);
        a.shl(1);
        assert!(a.overflowed());
        let mut b = Big::from_u64(3);
        b.sub_assign(&Big::from_u64(5));
        assert!(b.overflowed());
        let mut z = Big::from_u64(9);
        assert_eq!(z.div_small(0), 0);
        assert!(z.overflowed());
    }
}
