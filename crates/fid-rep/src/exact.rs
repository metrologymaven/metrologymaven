//! Exact comparison of `|x - y|` with a bound (REQ-UNC.RDL.REP-006).
//!
//! Every finite `f64` is `±m * 2^e` and every [`Decimal`] is `±c * 10^k`, so each quantity
//! here is a sum of terms `±n * 2^p2 * 10^p10` with a natural `n`. A sign test of such a
//! sum is done in integers: every term is scaled to the smallest `p2` and `p10` present
//! and the naturals are added and subtracted exactly. No rounded `f64` subtraction is ever
//! used to decide a bound.

use core::cmp::Ordering;

use fid_num::kdec::Decimal;

/// A natural number, little-endian 32-bit limbs, no trailing zero limbs.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Nat(Vec<u32>);

impl Nat {
    fn from_u128(mut v: u128) -> Nat {
        let mut limbs = Vec::new();
        while v != 0 {
            limbs.push((v & 0xffff_ffff) as u32);
            v >>= 32;
        }
        Nat(limbs)
    }

    fn trim(&mut self) {
        while self.0.last() == Some(&0) {
            self.0.pop();
        }
    }

    fn mul_small(&mut self, f: u32) {
        let mut carry: u64 = 0;
        for limb in &mut self.0 {
            let t = u64::from(*limb) * u64::from(f) + carry;
            *limb = (t & 0xffff_ffff) as u32;
            carry = t >> 32;
        }
        if carry != 0 {
            self.0.push(carry as u32);
        }
        self.trim();
    }

    fn mul(&self, other: &Nat) -> Nat {
        let mut out = vec![0u32; self.0.len() + other.0.len()];
        for (i, &a) in self.0.iter().enumerate() {
            let mut carry: u64 = 0;
            for (j, &b) in other.0.iter().enumerate() {
                let slot = &mut out[i + j];
                let t = u64::from(a) * u64::from(b) + u64::from(*slot) + carry;
                *slot = (t & 0xffff_ffff) as u32;
                carry = t >> 32;
            }
            let mut k = i + other.0.len();
            while carry != 0 {
                let t = u64::from(out[k]) + carry;
                out[k] = (t & 0xffff_ffff) as u32;
                carry = t >> 32;
                k += 1;
            }
        }
        let mut n = Nat(out);
        n.trim();
        n
    }

    fn shl(&mut self, bits: u32) {
        if self.0.is_empty() {
            return;
        }
        let limbs = (bits / 32) as usize;
        let rest = bits % 32;
        if rest != 0 {
            let mut carry = 0u32;
            for limb in &mut self.0 {
                let v = *limb;
                *limb = (v << rest) | carry;
                carry = v >> (32 - rest);
            }
            if carry != 0 {
                self.0.push(carry);
            }
        }
        if limbs != 0 {
            let mut v = vec![0u32; limbs];
            v.append(&mut self.0);
            self.0 = v;
        }
    }

    fn mul_pow10(&mut self, mut k: u32) {
        // 10^9 fits in a u32 limb.
        while k >= 9 {
            self.mul_small(1_000_000_000);
            k -= 9;
        }
        let mut f = 1u32;
        for _ in 0..k {
            f *= 10;
        }
        if f != 1 {
            self.mul_small(f);
        }
    }

    fn cmp_mag(&self, other: &Nat) -> Ordering {
        match self.0.len().cmp(&other.0.len()) {
            Ordering::Equal => {}
            o => return o,
        }
        for (a, b) in self.0.iter().rev().zip(other.0.iter().rev()) {
            match a.cmp(b) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        Ordering::Equal
    }

    fn add(&mut self, other: &Nat) {
        if self.0.len() < other.0.len() {
            self.0.resize(other.0.len(), 0);
        }
        let mut carry: u64 = 0;
        for (i, limb) in self.0.iter_mut().enumerate() {
            let b = other.0.get(i).copied().map_or(0, u64::from);
            let t = u64::from(*limb) + b + carry;
            *limb = (t & 0xffff_ffff) as u32;
            carry = t >> 32;
        }
        if carry != 0 {
            self.0.push(carry as u32);
        }
    }

    /// `self - other`; the caller guarantees `self >= other`.
    fn sub(&mut self, other: &Nat) {
        let mut borrow: i64 = 0;
        for (i, limb) in self.0.iter_mut().enumerate() {
            let b = other.0.get(i).copied().map_or(0, i64::from);
            let mut t = i64::from(*limb) - b - borrow;
            borrow = 0;
            if t < 0 {
                t += 1 << 32;
                borrow = 1;
            }
            *limb = t as u32;
        }
        self.trim();
    }
}

/// `±n * 2^p2 * 10^p10`.
#[derive(Clone, Debug)]
pub(crate) struct Term {
    neg: bool,
    n: Nat,
    p2: i32,
    p10: i32,
}

impl Term {
    /// The exact value of a finite `f64`; `None` for NaN or an infinity.
    pub(crate) fn from_f64(x: f64) -> Option<Term> {
        if !x.is_finite() {
            return None;
        }
        let bits = x.to_bits();
        let neg = bits >> 63 == 1;
        let bexp = ((bits >> 52) & 0x7ff) as i32;
        let frac = bits & ((1u64 << 52) - 1);
        let (m, e) = if bexp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), bexp - 1075) };
        Some(Term { neg, n: Nat::from_u128(u128::from(m)), p2: e, p10: 0 })
    }

    pub(crate) fn from_decimal(d: &Decimal) -> Term {
        Term { neg: d.is_negative(), n: Nat::from_u128(d.coeff()), p2: 0, p10: d.exp10() }
    }

    pub(crate) fn negated(mut self) -> Term {
        self.neg = !self.neg;
        self
    }

    pub(crate) fn abs(mut self) -> Term {
        self.neg = false;
        self
    }

    pub(crate) fn mul(&self, other: &Term) -> Term {
        Term { neg: self.neg != other.neg, n: self.n.mul(&other.n), p2: self.p2 + other.p2, p10: self.p10 + other.p10 }
    }
}

/// Sign of the exact sum of `terms`.
pub(crate) fn sign_of_sum(terms: &[Term]) -> Ordering {
    let live: Vec<&Term> = terms.iter().filter(|t| !t.n.0.is_empty()).collect();
    let (Some(p2), Some(p10)) = (live.iter().map(|t| t.p2).min(), live.iter().map(|t| t.p10).min()) else {
        return Ordering::Equal;
    };
    // Running sum in sign-magnitude form.
    let mut neg = false;
    let mut acc = Nat(Vec::new());
    for t in live {
        let mut n = t.n.clone();
        n.mul_pow10((t.p10 - p10).unsigned_abs());
        n.shl((t.p2 - p2).unsigned_abs());
        if acc.0.is_empty() {
            neg = t.neg;
            acc = n;
        } else if neg == t.neg {
            acc.add(&n);
        } else if acc.cmp_mag(&n) != Ordering::Less {
            acc.sub(&n);
        } else {
            n.sub(&acc);
            acc = n;
            neg = t.neg;
        }
    }
    match (acc.0.is_empty(), neg) {
        (true, _) => Ordering::Equal,
        (false, true) => Ordering::Less,
        (false, false) => Ordering::Greater,
    }
}

/// `|x - y| <= bound`, exactly. `bound` is non-negative.
pub(crate) fn abs_diff_le(x: &Term, y: &Term, bound: &Term) -> bool {
    let d = sign_of_sum(&[x.clone(), y.clone().negated()]);
    let terms = match d {
        Ordering::Less => [y.clone(), x.clone().negated(), bound.clone().negated()],
        _ => [x.clone(), y.clone().negated(), bound.clone().negated()],
    };
    sign_of_sum(&terms) != Ordering::Greater
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn f(x: f64) -> Term {
        Term::from_f64(x).unwrap()
    }

    fn d(s: &str) -> Term {
        Term::from_decimal(&Decimal::parse(s).unwrap())
    }

    #[test]
    fn signs() {
        assert_eq!(sign_of_sum(&[f(1.0), f(-1.0)]), Ordering::Equal);
        assert_eq!(sign_of_sum(&[f(0.1), d("0.1").negated()]), Ordering::Greater); // fl(0.1) > 0.1
        assert_eq!(sign_of_sum(&[f(0.3), d("0.3").negated()]), Ordering::Less); // fl(0.3) < 0.3
        assert_eq!(sign_of_sum(&[]), Ordering::Equal);
        assert_eq!(sign_of_sum(&[f(5e-324), f(1.7976931348623157e308).negated()]), Ordering::Less);
        assert_eq!(sign_of_sum(&[d("1e400"), f(1.7976931348623157e308).negated()]), Ordering::Greater);
    }

    #[test]
    fn bounds() {
        assert!(abs_diff_le(&f(1.0), &f(1.0), &d("0")));
        assert!(abs_diff_le(&f(1.0), &f(1.5), &d("0.5")));
        assert!(!abs_diff_le(&f(1.0), &f(1.5), &d("0.4999999999999999999999")));
        assert!(abs_diff_le(&f(1.5), &f(1.0), &d("0.5")));
        assert!(!abs_diff_le(&f(0.0), &f(1e-9), &d("1e-9")));
        assert!(abs_diff_le(&f(2.0), &f(3.0), &f(0.5).mul(&d("2"))));
    }

    #[test]
    fn nat_ops() {
        let mut a = Nat::from_u128(u128::MAX);
        a.add(&Nat::from_u128(1));
        let mut b = Nat::from_u128(1);
        b.shl(128);
        assert_eq!(a, b);
        a.sub(&Nat::from_u128(1));
        assert_eq!(a, Nat::from_u128(u128::MAX));
        let mut p = Nat::from_u128(1);
        p.mul_pow10(20);
        assert_eq!(p, Nat::from_u128(100_000_000_000_000_000_000));
        let sq = Nat::from_u128(u128::from(u64::MAX)).mul(&Nat::from_u128(u128::from(u64::MAX)));
        assert_eq!(sq, Nat::from_u128(u128::from(u64::MAX) * u128::from(u64::MAX)));
    }
}
