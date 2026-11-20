//! Exact tied-pool basis reduction for linear problems (spec node KRN.SOLV.CERT.AS,
//! REQ-KRN.SOLV.CERT.AS-007).
//!
//! Given the small-slack rows of an LP solution (the *pool*) whose candidate count exceeds
//! [`super::active::MAX_CANDIDATES`], [`reduce`] picks **one** basis: `n` linearly independent
//! pool rows with nonnegative multipliers, the Carathéodory reduction of the multiplier system
//!
//! ```text
//! sum_k y_k a_k = -q,   y >= 0      (a_k = row k of A, k over the pool; q = the LP objective)
//! ```
//!
//! that is, the dual feasibility condition `A^T y + q = 0` of `min q^T x, A x + s = b, s >= 0`
//! restricted to the pool. The basis is the result of a phase-1 simplex with **Bland's rule** on
//! that system, columns taken in label order, from the all-artificial start: entering column =
//! the lowest-indexed column with a negative reduced cost, leaving row = the lowest-indexed
//! basic variable among the ratio-test ties. Every pivot decision is a sign or an order of exact
//! integers: the `f64` data are exact dyadic rationals, each column is scaled by its own power of
//! two to integers (a positive scale changes no sign), and the tableau is updated by
//! fraction-free (Bareiss) pivoting with exact division, so there is no rounding anywhere and no
//! sign is ever guessed. The result depends only on the problem data and the pool (the
//! multipliers `y` and slacks `s` of the solution enter only through the classification), so a
//! vertex and an interior solution of the same tied LP give the same basis.
//!
//! The basis is still only a proposal: `KRN.SOLV.CERT.RECOMP` and `KRN.SOLV.CERT.KKT` decide
//! whether it is certified. `None` (fail closed) when the pool has no basis with nonnegative
//! multipliers, its rows do not span `R^n`, or the iteration limit is reached.

use core::cmp::Ordering;

/// Pivots before the reduction gives up (fail closed). Bland's rule cannot cycle, so this only
/// bounds the work; a phase 1 on `n` rows needs far fewer.
const MAX_PIVOTS: usize = 100_000;

// ------------------------------------------------------------------ exact integers

/// A signed integer of unbounded size: sign and little-endian 32-bit limbs without trailing
/// zero limbs (zero is `neg = false`, no limbs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Int {
    neg: bool,
    mag: Vec<u32>,
}

fn trim(v: &mut Vec<u32>) {
    while v.last() == Some(&0) {
        v.pop();
    }
}

fn cmp_mag(a: &[u32], b: &[u32]) -> Ordering {
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

fn add_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut out = Vec::with_capacity(long.len() + 1);
    let mut carry = 0_u64;
    for (i, &x) in long.iter().enumerate() {
        let s = u64::from(x) + u64::from(short.get(i).copied().unwrap_or(0)) + carry;
        out.push(s as u32);
        carry = s >> 32;
    }
    if carry > 0 {
        out.push(carry as u32);
    }
    out
}

/// `a - b` for `|a| >= |b|`.
fn sub_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0_i64;
    for (i, &x) in a.iter().enumerate() {
        let mut d = i64::from(x) - i64::from(b.get(i).copied().unwrap_or(0)) - borrow;
        borrow = 0;
        if d < 0 {
            d += 1 << 32;
            borrow = 1;
        }
        out.push(d as u32);
    }
    trim(&mut out);
    out
}

fn mul_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0_u32; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0_u64;
        for (j, &y) in b.iter().enumerate() {
            let k = i + j;
            let t = u64::from(x) * u64::from(y) + u64::from(out.get(k).copied().unwrap_or(0)) + carry;
            if let Some(slot) = out.get_mut(k) {
                *slot = t as u32;
            }
            carry = t >> 32;
        }
        let mut k = i + b.len();
        while carry > 0 {
            let Some(slot) = out.get_mut(k) else { break };
            let t = u64::from(*slot) + carry;
            *slot = t as u32;
            carry = t >> 32;
            k += 1;
        }
    }
    trim(&mut out);
    out
}

/// `(a / b, a % b)` of magnitudes, `b` nonzero (Knuth, TAOCP 4.3.1, algorithm D).
fn divmod_mag(a: &[u32], b: &[u32]) -> Option<(Vec<u32>, Vec<u32>)> {
    let n = b.len();
    let top = *b.last()?;
    if cmp_mag(a, b) == Ordering::Less {
        return Some((Vec::new(), a.to_vec()));
    }
    if n == 1 {
        let d = u64::from(top);
        let mut q = vec![0_u32; a.len()];
        let mut r = 0_u64;
        for i in (0..a.len()).rev() {
            let cur = (r << 32) | u64::from(a.get(i).copied().unwrap_or(0));
            if let Some(slot) = q.get_mut(i) {
                *slot = (cur / d) as u32;
            }
            r = cur % d;
        }
        trim(&mut q);
        let mut rem = vec![r as u32];
        trim(&mut rem);
        return Some((q, rem));
    }
    // Normalise so that the divisor's top limb has its high bit set.
    let s = top.leading_zeros();
    let shl = |x: &[u32], extra: usize| -> Vec<u32> {
        let mut out = vec![0_u32; x.len() + extra];
        for (i, &v) in x.iter().enumerate() {
            let w = u64::from(v) << s;
            if let Some(slot) = out.get_mut(i) {
                *slot |= w as u32;
            }
            if let Some(slot) = out.get_mut(i + 1) {
                *slot |= (w >> 32) as u32;
            }
        }
        out
    };
    let v = {
        let mut v = shl(b, 0);
        v.truncate(n);
        v
    };
    let mut u = shl(a, 1);
    let m = a.len() - n;
    let mut q = vec![0_u32; m + 1];
    let vn1 = u64::from(*v.get(n - 1)?);
    let vn2 = u64::from(*v.get(n - 2)?);
    for j in (0..=m).rev() {
        let ujn = u64::from(*u.get(j + n)?);
        let ujn1 = u64::from(*u.get(j + n - 1)?);
        let ujn2 = u64::from(*u.get(j + n - 2)?);
        let num = (ujn << 32) | ujn1;
        let mut qhat = num / vn1;
        let mut rhat = num % vn1;
        while qhat >= (1 << 32) || u128::from(qhat) * u128::from(vn2) > ((u128::from(rhat) << 32) | u128::from(ujn2)) {
            qhat -= 1;
            rhat += vn1;
            if rhat >= (1 << 32) {
                break;
            }
        }
        // Multiply and subtract.
        let mut borrow = 0_i64;
        let mut carry = 0_u64;
        for i in 0..n {
            let p = qhat * u64::from(*v.get(i)?) + carry;
            carry = p >> 32;
            let t = i64::from(*u.get(i + j)?) - borrow - (p & 0xffff_ffff) as i64;
            if let Some(slot) = u.get_mut(i + j) {
                *slot = t as u32;
            }
            borrow = if t < 0 { 1 } else { 0 };
        }
        let t = i64::from(*u.get(j + n)?) - borrow - carry as i64;
        if let Some(slot) = u.get_mut(j + n) {
            *slot = t as u32;
        }
        if t < 0 {
            // Add back.
            qhat -= 1;
            let mut c = 0_u64;
            for i in 0..n {
                let s2 = u64::from(*u.get(i + j)?) + u64::from(*v.get(i)?) + c;
                if let Some(slot) = u.get_mut(i + j) {
                    *slot = s2 as u32;
                }
                c = s2 >> 32;
            }
            if let Some(slot) = u.get_mut(j + n) {
                *slot = slot.wrapping_add(c as u32);
            }
        }
        if let Some(slot) = q.get_mut(j) {
            *slot = qhat as u32;
        }
    }
    trim(&mut q);
    // Remainder: u[0..n] shifted back right by s.
    let mut r = vec![0_u32; n];
    for i in 0..n {
        let lo = u64::from(*u.get(i)?) >> s;
        let hi = if s == 0 { 0 } else { u64::from(*u.get(i + 1)?) << (32 - s) };
        if let Some(slot) = r.get_mut(i) {
            *slot = (lo | hi) as u32;
        }
    }
    trim(&mut r);
    Some((q, r))
}

impl Int {
    pub(crate) fn zero() -> Int {
        Int { neg: false, mag: Vec::new() }
    }

    /// `(-1)^neg * m * 2^sh`.
    fn from_parts(neg: bool, m: u64, sh: u32) -> Int {
        let (limbs, bits) = ((sh / 32) as usize, sh % 32);
        let mut mag = vec![0_u32; limbs];
        let w = u128::from(m) << bits;
        mag.push(w as u32);
        mag.push((w >> 32) as u32);
        mag.push((w >> 64) as u32);
        trim(&mut mag);
        Int { neg: neg && !mag.is_empty(), mag }
    }

    pub(crate) fn sign(&self) -> Ordering {
        if self.mag.is_empty() {
            Ordering::Equal
        } else if self.neg {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    }

    fn norm(neg: bool, mag: Vec<u32>) -> Int {
        Int { neg: neg && !mag.is_empty(), mag }
    }

    pub(crate) fn negated(&self) -> Int {
        Int::norm(!self.neg, self.mag.clone())
    }

    pub(crate) fn plus(&self, o: &Int) -> Int {
        if self.neg == o.neg {
            return Int::norm(self.neg, add_mag(&self.mag, &o.mag));
        }
        match cmp_mag(&self.mag, &o.mag) {
            Ordering::Equal => Int::zero(),
            Ordering::Greater => Int::norm(self.neg, sub_mag(&self.mag, &o.mag)),
            Ordering::Less => Int::norm(o.neg, sub_mag(&o.mag, &self.mag)),
        }
    }

    pub(crate) fn minus(&self, o: &Int) -> Int {
        self.plus(&o.negated())
    }

    pub(crate) fn times(&self, o: &Int) -> Int {
        Int::norm(self.neg != o.neg, mul_mag(&self.mag, &o.mag))
    }

    /// `self / o` when the division is exact; `None` for `o = 0` or a nonzero remainder.
    pub(crate) fn div_exact(&self, o: &Int) -> Option<Int> {
        let (q, r) = divmod_mag(&self.mag, &o.mag)?;
        if !r.is_empty() {
            return None;
        }
        Some(Int::norm(self.neg != o.neg, q))
    }

    pub(crate) fn compare(&self, o: &Int) -> Ordering {
        match (self.neg, o.neg) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => cmp_mag(&self.mag, &o.mag),
            (true, true) => cmp_mag(&o.mag, &self.mag),
        }
    }
}

/// A finite `x` as `(negative, significand, exponent)` with `|x| = significand * 2^exponent`.
fn decompose(x: f64) -> (bool, u64, i32) {
    let bits = x.to_bits();
    let negative = bits >> 63 == 1;
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1_u64 << 52) - 1);
    if biased == 0 {
        (negative, fraction, -1074)
    } else {
        (negative, fraction | (1_u64 << 52), biased - 1075)
    }
}

/// The finite column `xs` times one power of two, as exact integers (positive scale).
fn scaled_column(xs: &[f64]) -> Option<Vec<Int>> {
    // Odd significands (trailing zero bits moved into the exponent) keep the integers small.
    let parts: Vec<(bool, u64, i32)> = xs
        .iter()
        .map(|&x| {
            let (neg, m, e) = decompose(x);
            if m == 0 {
                (neg, 0, e)
            } else {
                let tz = m.trailing_zeros();
                (neg, m >> tz, e + tz as i32)
            }
        })
        .collect();
    if !xs.iter().all(|x| x.is_finite()) {
        return None;
    }
    let emin = parts.iter().filter(|p| p.1 != 0).map(|p| p.2).min().unwrap_or(0);
    parts
        .iter()
        .map(|&(neg, m, e)| {
            if m == 0 {
                Some(Int::zero())
            } else {
                u32::try_from(e - emin).ok().map(|sh| Int::from_parts(neg, m, sh))
            }
        })
        .collect()
}

// ------------------------------------------------------------------ Bland phase 1

/// The basis of the module docs: positions into `rows` (pool order), ascending. `rows[k]` is
/// row `k` of `A` restricted to the `n` columns, `q` the objective (length `n`).
pub(crate) fn reduce(rows: &[Vec<f64>], q: &[f64]) -> Option<Vec<usize>> {
    let n = q.len();
    let m = rows.len();
    if n == 0 || m < n || rows.iter().any(|r| r.len() != n) {
        return None;
    }
    // Columns of the system: structural k < m (pool rows), artificial m + j, right-hand side.
    let mut cols: Vec<Vec<Int>> = Vec::with_capacity(m + 1);
    for r in rows {
        cols.push(scaled_column(r)?);
    }
    let rhs: Vec<f64> = q.iter().map(|v| -v).collect();
    let rhs = scaled_column(&rhs)?;
    // Tableau rows 0..n (constraints), row n = phase-1 cost. Column layout: m structural, n
    // artificial, 1 right-hand side. Rows with a negative right-hand side are negated.
    let width = m + n + 1;
    let mut t: Vec<Vec<Int>> = Vec::with_capacity(n + 1);
    for j in 0..n {
        let flip = rhs.get(j)?.sign() == Ordering::Less;
        let mut row = Vec::with_capacity(width);
        for c in &cols {
            let v = c.get(j)?.clone();
            row.push(if flip { v.negated() } else { v });
        }
        for a in 0..n {
            row.push(if a == j { Int::from_parts(false, 1, 0) } else { Int::zero() });
        }
        let v = rhs.get(j)?.clone();
        row.push(if flip { v.negated() } else { v });
        t.push(row);
    }
    // Phase-1 reduced costs: c_k - sum_i c_B(i) T_ik with c = 1 on the artificials.
    let mut cost = vec![Int::zero(); width];
    for (k, slot) in cost.iter_mut().enumerate() {
        if (m..m + n).contains(&k) {
            continue;
        }
        let mut acc = Int::zero();
        for row in &t {
            acc = acc.minus(row.get(k)?);
        }
        *slot = acc;
    }
    t.push(cost);
    let mut basis: Vec<usize> = (m..m + n).collect();
    let mut d = Int::from_parts(false, 1, 0);

    let pivot = |t: &mut [Vec<Int>], d: &mut Int, r: usize, s: usize| -> Option<()> {
        let prs = t.get(r)?.get(s)?.clone();
        let pivot_row = t.get(r)?.clone();
        for (i, row) in t.iter_mut().enumerate() {
            if i == r {
                continue;
            }
            let ais = row.get(s)?.clone();
            for (k, slot) in row.iter_mut().enumerate() {
                let v = prs.times(slot).minus(&ais.times(pivot_row.get(k)?));
                *slot = v.div_exact(d)?;
            }
        }
        *d = prs;
        Some(())
    };
    // The sign of the actual tableau value `x / d`.
    let actual = |x: &Int, d: &Int| -> Ordering {
        match (x.sign(), d.sign()) {
            (Ordering::Equal, _) => Ordering::Equal,
            (a, Ordering::Greater) => a,
            (a, _) => a.reverse(),
        }
    };

    for _ in 0..MAX_PIVOTS {
        // Bland: lowest column with a negative reduced cost (right-hand side excluded).
        let cost_row = t.get(n)?;
        let Some(s) = (0..m + n).find(|&k| cost_row.get(k).is_some_and(|c| actual(c, &d) == Ordering::Less)) else {
            break;
        };
        // Ratio test over rows with a positive entry; ties to the lowest basic index.
        let mut best: Option<usize> = None;
        for i in 0..n {
            let ais = t.get(i)?.get(s)?;
            if actual(ais, &d) != Ordering::Greater {
                continue;
            }
            best = match best {
                None => Some(i),
                Some(b) => {
                    // rhs_i / a_is vs rhs_b / a_bs, both denominators of the same sign as d.
                    let (ri, rb) = (t.get(i)?.get(width - 1)?, t.get(b)?.get(width - 1)?);
                    let abs_ = t.get(b)?.get(s)?;
                    let mut ord = ri.times(abs_).compare(&rb.times(ais));
                    if ais.times(abs_).sign() == Ordering::Less {
                        ord = ord.reverse();
                    }
                    match ord {
                        Ordering::Less => Some(i),
                        Ordering::Greater => Some(b),
                        Ordering::Equal => {
                            if basis.get(i)? < basis.get(b)? {
                                Some(i)
                            } else {
                                Some(b)
                            }
                        }
                    }
                }
            };
        }
        // Phase 1 is bounded below by 0, so an entering column always has a leaving row.
        let r = best?;
        pivot(&mut t, &mut d, r, s)?;
        *basis.get_mut(r)? = s;
    }
    // Feasible only if the phase-1 objective is 0.
    if t.get(n)?.get(width - 1)?.sign() != Ordering::Equal {
        return None;
    }
    // Drive the remaining (zero-level) artificials out: lowest structural column with a nonzero
    // entry in the row; none means the pool rows do not span R^n.
    for r in 0..n {
        if *basis.get(r)? < m {
            continue;
        }
        let row = t.get(r)?;
        let s = (0..m).find(|&k| !basis.contains(&k) && row.get(k).is_some_and(|v| v.sign() != Ordering::Equal))?;
        pivot(&mut t, &mut d, r, s)?;
        *basis.get_mut(r)? = s;
    }
    basis.sort_unstable();
    if basis.iter().any(|&k| k >= m) {
        return None;
    }
    Some(basis)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn int(v: i64) -> Int {
        Int::from_parts(v < 0, v.unsigned_abs(), 0)
    }

    #[test]
    fn integer_arithmetic() {
        let a = Int::from_parts(false, u64::MAX, 70);
        let b = Int::from_parts(true, 12345, 3);
        let p = a.times(&b);
        assert_eq!(p.div_exact(&b), Some(a.clone()));
        assert_eq!(p.div_exact(&a), Some(b.clone()));
        assert_eq!(a.plus(&b).minus(&b), a);
        assert_eq!(int(7).div_exact(&int(2)), None);
        assert_eq!(int(-12).div_exact(&int(4)), Some(int(-3)));
        assert_eq!(int(0).div_exact(&int(0)), None);
        assert_eq!(int(-3).compare(&int(2)), Ordering::Less);
        assert_eq!(int(5).minus(&int(5)), Int::zero());
        // Multi-limb divisor (algorithm D with add-back paths exercised by many shapes).
        let mut x = Int::from_parts(false, 0xdead_beef_1234_5678, 0);
        for k in 1..40_u64 {
            let y = Int::from_parts(k % 3 == 0, k.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1, (k * 7) as u32);
            let z = x.times(&y);
            assert_eq!(z.div_exact(&y), Some(x.clone()), "{k}");
            assert_eq!(z.plus(&int(1)).div_exact(&y), None, "{k}");
            x = z.plus(&int(k as i64));
        }
    }

    #[test]
    fn scaled_columns_are_exact() {
        let c = scaled_column(&[0.5, -3.0, 0.0]).unwrap();
        assert_eq!(c, vec![int(1), int(-6), int(0)]);
        assert!(scaled_column(&[f64::NAN]).is_none());
    }

    /// A planar Carathéodory example: -q = (0, 1) from four rows; the lowest feasible pair in
    /// Bland order.
    #[test]
    fn reduces_to_a_nonnegative_basis() {
        let rows = vec![vec![1.0, 1.0], vec![-1.0, 1.0], vec![0.0, 1.0], vec![2.0, 1.0]];
        let q = [0.0, -1.0];
        let b = reduce(&rows, &q).unwrap();
        assert_eq!(b.len(), 2);
        // Check y >= 0 for the chosen pair by Cramer's rule.
        let (r0, r1) = (&rows[b[0]], &rows[b[1]]);
        let det = r0[0] * r1[1] - r1[0] * r0[1];
        let y0 = (-q[0] * r1[1] - r1[0] * -q[1]) / det;
        let y1 = (r0[0] * -q[1] - (-q[0]) * r0[1]) / det;
        assert!(det != 0.0 && y0 >= 0.0 && y1 >= 0.0, "{b:?}");
        // No nonnegative combination: -q outside the cone of the rows.
        assert_eq!(reduce(&[vec![1.0, 0.0], vec![0.0, 1.0]], &[1.0, 1.0]), None);
        // Rows that do not span R^2.
        assert_eq!(reduce(&[vec![1.0, 0.0], vec![2.0, 0.0]], &[-1.0, 0.0]), None);
    }
}
