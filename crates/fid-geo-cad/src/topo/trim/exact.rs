//! Exact comparisons and 2-D orientation signs for GEO.CAD.TOPO.TRIM.
//!
//! Every input is a [`Shifted`] number `x + k p`: a binary64 `x`, plus `k` whole periods `p`
//! (`k = 0` for a plain binary64 value). The decisions are those of exact real arithmetic on
//! these values, never of a rounded floating comparison:
//!
//! - [`cmp`] orders two values; [`orient`] gives the sign of
//!   `(b - a) x (q - a) = (bx - ax)(qy - ay) - (by - ay)(qx - ax)` (`Greater`: `q` lies to the
//!   left of the directed line `a -> b`; `Equal`: the three points are collinear).
//! - **Filter.** When every `k` is zero, [`orient`] first evaluates the determinant in
//!   binary64 and accepts its sign when `|det| > CCW_BOUND * (|detleft| + |detright|)` with
//!   `CCW_BOUND = (3 + 16 eps) eps`, `eps = 2^-53` (Shewchuk's `ccwerrboundA`, "Adaptive
//!   precision floating-point arithmetic and fast robust geometric predicates", 1997), and
//!   only when `|detleft| + |detright|` is finite and at least `2^-960`, so no product can have
//!   underflowed below the bound. [`cmp`] of shifted values first compares `x + k p` evaluated
//!   in binary64 with a margin of `2^-50 (|x| + |k p|) + 2^-1070`, far above its rounding error.
//! - **Exact.** Otherwise every value is turned into an integer multiple of `2^e0` (`e0` the
//!   least binary exponent among the operands; a binary64 is `m 2^e` with `m < 2^53`,
//!   `e >= -1074`), and the determinant or difference is evaluated with sign-magnitude
//!   integers of 32-bit limbs (schoolbook multiplication). Nothing is rounded.
//!
//! The functions return `None` for a non-finite input or a `|k|` above `2^40`. Nothing
//! panics: every limb access is bounds-checked or an iterator.

use core::cmp::Ordering;

/// `2^-53`.
const EPS: f64 = 1.0 / 9_007_199_254_740_992.0;
/// Shewchuk's `ccwerrboundA = (3 + 16 eps) eps`.
pub const CCW_BOUND: f64 = (3.0 + 16.0 * EPS) * EPS;
/// `2^-960`: below this the filter is not trusted (possible underflow).
const FILTER_MIN: f64 = f64::from_bits(63 << 52);
/// Largest `|k|` accepted.
pub const MAX_K: i64 = 1 << 40;

/// The real number `x + k p`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shifted {
    pub x: f64,
    pub k: i64,
    pub p: f64,
}

impl Shifted {
    /// A plain binary64 value (`k = 0`).
    pub fn plain(x: f64) -> Shifted {
        Shifted { x, k: 0, p: 0.0 }
    }

    fn ok(&self) -> bool {
        self.x.is_finite() && (self.k == 0 || (self.p.is_finite() && self.k.abs() <= MAX_K))
    }

    /// `x + k p` rounded, and a bound on its error.
    fn approx(&self) -> (f64, f64) {
        if self.k == 0 {
            return (self.x, 0.0);
        }
        let kp = self.k as f64 * self.p;
        let y = self.x + kp;
        (y, MARGIN * (self.x.abs() + kp.abs()) + TINY)
    }
}

/// `2^-50`.
const MARGIN: f64 = 1.0 / 1_125_899_906_842_624.0;
/// `2^-1070`.
const TINY: f64 = f64::from_bits(16);

/// Exact order of `a` and `b`.
pub fn cmp(a: Shifted, b: Shifted) -> Option<Ordering> {
    if !(a.ok() && b.ok()) {
        return None;
    }
    if a.k == 0 && b.k == 0 {
        return a.x.partial_cmp(&b.x);
    }
    let (ya, ea) = a.approx();
    let (yb, eb) = b.approx();
    let gap = ya - yb;
    let err = ea + eb;
    if gap.is_finite() && err.is_finite() {
        if gap > err {
            return Some(Ordering::Greater);
        }
        if 0.0 - gap > err {
            return Some(Ordering::Less);
        }
    }
    let e0 = least_exponent(&[a, b]);
    let d = big(a, e0).sub(&big(b, e0));
    Some(d.sign())
}

/// Exact sign of `(b - a) x (q - a)`.
pub fn orient(a: [Shifted; 2], b: [Shifted; 2], q: [Shifted; 2]) -> Option<Ordering> {
    let all = [a[0], a[1], b[0], b[1], q[0], q[1]];
    if !all.iter().all(Shifted::ok) {
        return None;
    }
    if all.iter().all(|s| s.k == 0) {
        if let Some(o) = filter([a[0].x, a[1].x], [b[0].x, b[1].x], [q[0].x, q[1].x]) {
            return Some(o);
        }
    }
    let e0 = least_exponent(&all);
    let [ax, ay, bx, by, qx, qy] = all.map(|s| big(s, e0));
    let left = bx.sub(&ax).mul(&qy.sub(&ay));
    let right = by.sub(&ay).mul(&qx.sub(&ax));
    Some(left.sub(&right).sign())
}

/// [`orient`] of plain binary64 points.
pub fn orient_f64(a: [f64; 2], b: [f64; 2], q: [f64; 2]) -> Option<Ordering> {
    let p = |v: [f64; 2]| [Shifted::plain(v[0]), Shifted::plain(v[1])];
    orient(p(a), p(b), p(q))
}

/// The floating-point filter; `None` when it cannot decide.
fn filter(a: [f64; 2], b: [f64; 2], q: [f64; 2]) -> Option<Ordering> {
    let left = (b[0] - a[0]) * (q[1] - a[1]);
    let right = (b[1] - a[1]) * (q[0] - a[0]);
    let det = left - right;
    let sum = left.abs() + right.abs();
    if !(sum.is_finite() && sum >= FILTER_MIN) {
        return None;
    }
    let bound = CCW_BOUND * sum;
    if det > bound {
        Some(Ordering::Greater)
    } else if 0.0 - det > bound {
        Some(Ordering::Less)
    } else {
        None
    }
}

// ---------------------------------------------------------------- binary64 decomposition

/// `(negative, m, e)` with `x = (-1)^negative m 2^e`, `m < 2^53`; `x` finite.
fn decompose(x: f64) -> (bool, u64, i32) {
    let bits = x.to_bits();
    let neg = (bits >> 63) == 1;
    let field = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    if field == 0 {
        (neg, frac, -1074)
    } else {
        (neg, frac | (1u64 << 52), field - 1075)
    }
}

/// The least exponent of a non-zero `x` or `p` (with `k != 0`) among the operands.
fn least_exponent(vals: &[Shifted]) -> i32 {
    let mut e0: Option<i32> = None;
    let mut see = |x: f64| {
        let (_, m, e) = decompose(x);
        if m != 0 && e0.is_none_or(|c| e < c) {
            e0 = Some(e);
        }
    };
    for v in vals {
        see(v.x);
        if v.k != 0 {
            see(v.p);
        }
    }
    e0.unwrap_or(0)
}

/// `x + k p` as an integer multiple of `2^e0`.
fn big(s: Shifted, e0: i32) -> Big {
    let mut r = Big::of_f64(s.x, e0);
    if s.k != 0 {
        let k = s.k.unsigned_abs();
        let kb = Big::from_parts(s.k < 0, vec![k as u32, (k >> 32) as u32]);
        r = r.add(&Big::of_f64(s.p, e0).mul(&kb));
    }
    r
}

// ---------------------------------------------------------------- sign-magnitude integers

/// A signed integer: `neg` and little-endian 32-bit limbs without trailing zeros (zero is
/// the empty magnitude, never negative).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Big {
    neg: bool,
    mag: Vec<u32>,
}

fn trim(v: &mut Vec<u32>) {
    while v.last() == Some(&0) {
        v.pop();
    }
}

fn mag_cmp(a: &[u32], b: &[u32]) -> Ordering {
    if a.len() != b.len() {
        return a.len().cmp(&b.len());
    }
    for (x, y) in a.iter().rev().zip(b.iter().rev()) {
        if x != y {
            return x.cmp(y);
        }
    }
    Ordering::Equal
}

fn mag_add(a: &[u32], b: &[u32]) -> Vec<u32> {
    let n = if a.len() > b.len() { a.len() } else { b.len() };
    let mut out = Vec::with_capacity(n + 1);
    let mut carry = 0u64;
    for i in 0..n {
        let s = u64::from(a.get(i).copied().unwrap_or(0)) + u64::from(b.get(i).copied().unwrap_or(0)) + carry;
        out.push(s as u32);
        carry = s >> 32;
    }
    if carry != 0 {
        out.push(carry as u32);
    }
    out
}

/// `a - b` for `a >= b`.
fn mag_sub(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0i64;
    for (i, &x) in a.iter().enumerate() {
        let mut d = i64::from(x) - i64::from(b.get(i).copied().unwrap_or(0)) - borrow;
        if d < 0 {
            d += 1i64 << 32;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out.push(d as u32);
    }
    trim(&mut out);
    out
}

fn mag_mul(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0u32; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u64;
        for (j, &y) in b.iter().enumerate() {
            if let Some(slot) = out.get_mut(i + j) {
                let t = u64::from(x) * u64::from(y) + u64::from(*slot) + carry;
                *slot = t as u32;
                carry = t >> 32;
            }
        }
        let mut k = i + b.len();
        while carry != 0 {
            match out.get_mut(k) {
                Some(slot) => {
                    let t = u64::from(*slot) + carry;
                    *slot = t as u32;
                    carry = t >> 32;
                    k += 1;
                }
                // Unreachable: the product of an `n`-limb and an `m`-limb number fits in `n + m` limbs.
                None => break,
            }
        }
    }
    trim(&mut out);
    out
}

impl Big {
    fn from_parts(neg: bool, mut mag: Vec<u32>) -> Big {
        trim(&mut mag);
        let neg = neg && !mag.is_empty();
        Big { neg, mag }
    }

    /// `x / 2^e0` for a finite `x` whose exponent is at least `e0` (or `x = 0`).
    fn of_f64(x: f64, e0: i32) -> Big {
        let (neg, m, e) = decompose(x);
        if m == 0 {
            return Big::from_parts(false, Vec::new());
        }
        let sh = if e > e0 { (e - e0) as usize } else { 0 };
        let (w, b) = (sh / 32, sh % 32);
        let v = u128::from(m) << b;
        let mut mag = vec![0u32; w];
        mag.push(v as u32);
        mag.push((v >> 32) as u32);
        mag.push((v >> 64) as u32);
        Big::from_parts(neg, mag)
    }

    fn sign(&self) -> Ordering {
        if self.mag.is_empty() {
            Ordering::Equal
        } else if self.neg {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }

    fn add(&self, o: &Big) -> Big {
        if self.neg == o.neg {
            return Big::from_parts(self.neg, mag_add(&self.mag, &o.mag));
        }
        match mag_cmp(&self.mag, &o.mag) {
            Ordering::Equal => Big::from_parts(false, Vec::new()),
            Ordering::Greater => Big::from_parts(self.neg, mag_sub(&self.mag, &o.mag)),
            Ordering::Less => Big::from_parts(o.neg, mag_sub(&o.mag, &self.mag)),
        }
    }

    fn sub(&self, o: &Big) -> Big {
        let neg_o = Big { neg: !o.neg && !o.mag.is_empty(), mag: o.mag.clone() };
        self.add(&neg_o)
    }

    fn mul(&self, o: &Big) -> Big {
        Big::from_parts(self.neg != o.neg, mag_mul(&self.mag, &o.mag))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn constants() {
        assert_eq!(FILTER_MIN, 2f64.powi(-960));
        assert_eq!(TINY, 2f64.powi(-1070));
        assert_eq!(MARGIN, 2f64.powi(-50));
        assert_eq!(CCW_BOUND, (3.0 + 16.0 * 2f64.powi(-53)) * 2f64.powi(-53));
    }

    #[test]
    fn big_arithmetic() {
        // 3 = (3 2^51) 2^-51 and 5 = (5 2^50) 2^-50: common exponent -51.
        let e0 = -51;
        let a = Big::of_f64(3.0, e0);
        let b = Big::of_f64(-5.0, e0);
        assert_eq!(a.add(&b), Big::of_f64(-2.0, e0));
        // (3 2^51)(-5 2^51) = -15 2^102 = -960 2^96.
        assert_eq!(a.mul(&b), Big::from_parts(true, vec![0, 0, 0, 960]));
        assert_eq!(b.sub(&b.add(&b)), Big::of_f64(5.0, e0));
        assert_eq!(a.sub(&a).sign(), Ordering::Equal);
        let big = Big::of_f64(2f64.powi(100), 0);
        assert_eq!(big.mag, vec![0, 0, 0, 16]);
        let sq = big.mul(&big);
        assert_eq!(sq.mag.len(), 7);
        assert_eq!(sq.mag.last(), Some(&256));
        // Subnormal and tiny values at a common exponent.
        let t = Big::of_f64(f64::from_bits(1), -1074);
        assert_eq!(t.mag, vec![1]);
    }

    #[test]
    fn exact_beats_rounding() {
        // Points on the line y = x through huge offsets: rounded arithmetic says collinear,
        // exact arithmetic sees the 1-ulp step.
        let a = [0.5, 0.5];
        let b = [12.0, 12.0];
        let q = [24.0, 24.0 + 2f64.powi(-48)];
        assert_eq!(orient_f64(a, b, q), Some(Ordering::Greater));
        assert_eq!(orient_f64(a, b, [24.0, 24.0]), Some(Ordering::Equal));
        assert_eq!(orient_f64(b, a, q), Some(Ordering::Less));
        // Tiny coordinates whose products underflow binary64.
        let s = 1e-200;
        assert_eq!(orient_f64([0.0, 0.0], [s, 0.0], [0.0, s]), Some(Ordering::Greater));
        assert_eq!(orient_f64([0.0, 0.0], [s, s], [2.0 * s, 2.0 * s]), Some(Ordering::Equal));
        assert_eq!(orient_f64([0.0, f64::NAN], [s, s], [s, s]), None);
    }

    #[test]
    fn shifted_values() {
        let tau = core::f64::consts::TAU;
        let a = Shifted { x: 0.5, k: 1, p: tau };
        assert_eq!(cmp(a, a), Some(Ordering::Equal));
        assert_eq!(cmp(Shifted { x: 0.0, k: 1, p: tau }, Shifted::plain(tau)), Some(Ordering::Equal));
        assert_eq!(cmp(Shifted { x: -tau, k: 1, p: tau }, Shifted::plain(0.0)), Some(Ordering::Equal));
        // tau + 2^-60 rounds to tau in binary64; the exact comparison still sees it.
        let tiny = 2f64.powi(-60);
        assert_eq!(cmp(Shifted { x: tiny, k: 1, p: tau }, Shifted::plain(tau)), Some(Ordering::Greater));
        assert_eq!(cmp(Shifted { x: -tiny, k: 1, p: tau }, Shifted::plain(tau)), Some(Ordering::Less));
        assert_eq!(cmp(Shifted { x: 1.0, k: -2, p: tau }, Shifted::plain(-12.0)), Some(Ordering::Greater));
        assert_eq!(cmp(Shifted { x: 0.0, k: MAX_K + 1, p: 1.0 }, Shifted::plain(0.0)), None);
    }
}
