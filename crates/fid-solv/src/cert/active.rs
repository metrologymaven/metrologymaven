//! Active-set extraction, candidate enumeration and tie-break order (spec node
//! KRN.SOLV.CERT.AS).
//!
//! [`extract`] reads a converged conic solution of a problem whose constraint rows all lie in
//! the nonnegative orthant (`Ax + s = b`, `s >= 0`) and classifies each row by its slack
//! `s_i` and multiplier `y_i` against the caller's relative thresholds, scaled by recorded
//! data scales (`scale_s = max |b_i|`, or 1 if that is 0; `scale_y = max |y_i|`):
//!
//! - **strongly active**: `s_i <= slack_rel * scale_s` and `y_i >= mult_rel * scale_y` and
//!   `y_i > 0`;
//! - **ambiguous**: `s_i <= slack_rel * scale_s` but not strongly active;
//! - **inactive**: `s_i > slack_rel * scale_s`.
//!
//! The slack test is one-sided: a negative slack (an infeasible iterate) counts as small
//! slack however large its magnitude, so such a row is strongly active or ambiguous, never
//! inactive. A candidate built on it is still only a proposal; recomputation and the KKT
//! check decide.
//!
//! **Every comparison is exact.** The products `slack_rel * scale_s` and
//! `mult_rel * scale_y` are never rounded: each side is compared as an exact rational number
//! (integer significands and binary exponents, see `cmp_scaled`), so a row lies on the side of
//! a threshold that real arithmetic puts it on, with no rounding band. The only tolerances are
//! the caller's two thresholds, recorded in the output with the scales. No floating-point
//! arithmetic other than `abs` is performed, so the output is bit-identical on every target.
//!
//! **Path independence.** The candidate sets are every subset of the expected size of the
//! small-slack rows (strongly active and ambiguous together), sorted by label and listed in
//! ascending lexicographic order of their sorted labels (the tie-break order of
//! REQ-KRN.SOLV.CERT-004: lowest point indices first). The multipliers only split the
//! small-slack rows into the two diagnostic lists; they do not change the sets. So a vertex
//! solution and an interior solution of the same tied problem, which differ in their
//! multipliers, give the same candidate list, and the lowest certifiable basis the
//! certificate pipeline finds does not depend on the solver's path.
//!
//! **Uncertified.** The result is a [`Proposal`]: its sets are guesses until the
//! certificate pipeline (`KRN.SOLV.CERT.CT`, `certify`) recomputes the optimum from one of
//! them and passes the KKT check. Outside this crate the sets are readable only through
//! [`Proposal::uncertified_sets`], and a `Proposal` cannot be built.

use core::cmp::Ordering;

use fid_num::f::total_max;

use super::{CertError, Undetermined};
use crate::conic::{Cone, ConicProblem, ConicSolution};

/// The most candidate sets [`extract`] lists (REQ-KRN.SOLV.CERT.AS-003, cap [unverified]).
pub const MAX_CANDIDATES: usize = 256;

/// The caller's label of a constraint row: the point it belongs to and which side of the
/// zone it bounds (one point may own several rows). Labels order by `point`, then `side`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RowLabel {
    /// Index of the point that owns the row.
    pub point: u32,
    /// Which constraint of the point (caller-defined, for example 0 = lower, 1 = upper).
    pub side: u8,
}

/// Relative classification thresholds, each finite and nonnegative. The absolute thresholds
/// are `slack_rel * scale_s` and `mult_rel * scale_y` (see the module docs), compared exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    /// Relative slack threshold.
    pub slack_rel: f64,
    /// Relative multiplier threshold.
    pub mult_rel: f64,
}

/// The **uncertified** candidate active sets of one solution, in try order. Only the
/// certificate pipeline in this crate turns a candidate into a certified active set; nothing
/// outside `fid-solv` may treat a candidate as an active set. The fields are crate-private,
/// so a `Proposal` is made only by [`extract`].
#[derive(Debug, Clone, PartialEq)]
#[must_use]
pub struct Proposal {
    pub(crate) sets: Vec<Vec<RowLabel>>,
    pub(crate) strongly_active: Vec<RowLabel>,
    pub(crate) ambiguous: Vec<RowLabel>,
    pub(crate) thresholds: Thresholds,
    pub(crate) scales: (f64, f64),
    pub(crate) tie_broken: bool,
    pub(crate) over_determined: bool,
}

impl Proposal {
    /// The candidate sets, **uncertified**: each of the expected size and sorted by label, in
    /// ascending lexicographic order; at least one and at most [`MAX_CANDIDATES`]. None of
    /// them is an active set until `certify` has recomputed and KKT-checked it.
    pub fn uncertified_sets(&self) -> &[Vec<RowLabel>] {
        &self.sets
    }
    /// Strongly active rows, sorted (diagnostic; depends on the multipliers).
    pub fn strongly_active(&self) -> &[RowLabel] {
        &self.strongly_active
    }
    /// Ambiguous rows (small slack, multiplier not clearly positive), sorted (diagnostic;
    /// depends on the multipliers).
    pub fn ambiguous(&self) -> &[RowLabel] {
        &self.ambiguous
    }
    /// The thresholds used.
    pub fn thresholds(&self) -> Thresholds {
        self.thresholds
    }
    /// The scales used: `(scale_s, scale_y)`.
    pub fn scales(&self) -> (f64, f64) {
        self.scales
    }
    /// More than one candidate set exists (more small-slack rows than the expected size).
    pub fn tie_broken(&self) -> bool {
        self.tie_broken
    }
    /// More rows are strongly active than the expected size: a primal-degenerate optimum
    /// such as two points at equal deviation with both multipliers positive (what an
    /// interior-point method returns for a tie). Diagnostic only: it depends on the
    /// multipliers and does not change the sets, so it must not enter a certificate digest.
    pub fn over_determined(&self) -> bool {
        self.over_determined
    }
}

/// Extracts the candidate active sets of `solution`, a solution of `problem`.
///
/// `labels[i]` labels row `i`; `expected` is the size of an active set (for example the
/// number of parameters of a minimum-zone problem). The candidates are every subset of size
/// `expected` of the small-slack rows (strongly active and ambiguous), in lexicographic
/// order (REQ-KRN.SOLV.CERT.AS-002, -003).
///
/// Inputs are checked in this order: thresholds and `expected` (`InvalidInput`), cones
/// (`InvalidInput { field: "cones" }` unless every cone is nonnegative), lengths of `labels`,
/// `y` and `s` (`Shape`), finiteness of `y` and `s` (`InvalidInput`), duplicate labels
/// (`DuplicateLabel`).
///
/// # Errors
///
/// The input errors above, and [`CertError::ActiveSetUndetermined`] when fewer rows than
/// `expected` have small slack (`TooFewRows`), or when more than [`MAX_CANDIDATES`] sets
/// would be listed (`OverCap`).
pub fn extract(
    problem: &ConicProblem,
    solution: &ConicSolution,
    labels: &[RowLabel],
    expected: usize,
    thresholds: &Thresholds,
) -> Result<Proposal, CertError> {
    let th = *thresholds;
    let valid = |t: f64| t.is_finite() && t >= 0.0;
    if !valid(th.slack_rel) || !valid(th.mult_rel) {
        return Err(CertError::InvalidInput { field: "thresholds" });
    }
    if expected == 0 {
        return Err(CertError::InvalidInput { field: "expected" });
    }
    if !problem.cones().iter().all(|c| matches!(c, Cone::Nonneg(_))) {
        return Err(CertError::InvalidInput { field: "cones" });
    }
    let m = problem.m();
    for (field, got) in [("labels", labels.len()), ("y", solution.y.len()), ("s", solution.s.len())] {
        if got != m {
            return Err(CertError::Shape { field, expected: m, got });
        }
    }
    if !solution.y.iter().all(|v| v.is_finite()) {
        return Err(CertError::InvalidInput { field: "y" });
    }
    if !solution.s.iter().all(|v| v.is_finite()) {
        return Err(CertError::InvalidInput { field: "s" });
    }
    let mut sorted = labels.to_vec();
    sorted.sort_unstable();
    for w in sorted.windows(2) {
        if let [a, b] = w {
            if a == b {
                return Err(CertError::DuplicateLabel { label: *a });
            }
        }
    }

    let max_abs = |field: &'static str, xs: &[f64]| -> Result<f64, CertError> {
        xs.iter().try_fold(0.0_f64, |acc, v| total_max(acc, v.abs())).map_err(|_| CertError::InvalidInput { field })
    };
    let scale_b = max_abs("b", problem.b())?;
    let scale_s = if scale_b == 0.0 { 1.0 } else { scale_b };
    let scale_y = max_abs("y", &solution.y)?;

    let mut strongly_active = Vec::new();
    let mut ambiguous = Vec::new();
    let mut small = Vec::new();
    for ((label, &s), &y) in labels.iter().zip(&solution.s).zip(&solution.y) {
        if cmp_scaled(s, th.slack_rel, scale_s)? == Ordering::Greater {
            continue;
        }
        small.push(*label);
        if y > 0.0 && cmp_scaled(y, th.mult_rel, scale_y)? != Ordering::Less {
            strongly_active.push(*label);
        } else {
            ambiguous.push(*label);
        }
    }
    strongly_active.sort_unstable();
    ambiguous.sort_unstable();
    small.sort_unstable();

    let cause = if small.len() < expected {
        Some(Undetermined::TooFewRows)
    } else if !binomial_within_cap(small.len(), expected) {
        Some(Undetermined::OverCap)
    } else {
        None
    };
    if let Some(cause) = cause {
        return Err(CertError::ActiveSetUndetermined { strongly_active, ambiguous, cause });
    }
    let mut sets = combinations(&small, expected);
    // Already in order (k-subsets of a sorted pool taken in lexicographic index order); sorted
    // again so the order never rests on that argument alone.
    sets.sort_unstable();
    let tie_broken = sets.len() > 1;
    let over_determined = strongly_active.len() > expected;
    Ok(Proposal {
        sets,
        strongly_active,
        ambiguous,
        thresholds: th,
        scales: (scale_s, scale_y),
        tie_broken,
        over_determined,
    })
}

/// Whether `n choose k` (with `k <= n`) is at most [`MAX_CANDIDATES`]. Exact: with
/// `j = min(k, n - k)`, `C(n, i)` is nondecreasing for `i <= j`, so the running value
/// `C(n, i)` passes the cap before `i = j` only if the result does; each step
/// `C(n, i + 1) = C(n, i) * (n - i) / (i + 1)` is an exact integer division.
fn binomial_within_cap(n: usize, k: usize) -> bool {
    let Some(rest) = n.checked_sub(k) else { return false };
    let j = if k < rest { k } else { rest };
    let cap = MAX_CANDIDATES as u128;
    let mut c: u128 = 1;
    for i in 0..j {
        // c <= cap and n - i < 2^64, so the product fits in u128.
        c = c * (n - i) as u128 / (i + 1) as u128;
        if c > cap {
            return false;
        }
    }
    true
}

/// Every `k`-subset of `pool` (sorted), each sorted, taken in lexicographic order of their
/// pool indices by an iterative generator. `k <= pool.len()`.
fn combinations(pool: &[RowLabel], k: usize) -> Vec<Vec<RowLabel>> {
    let n = pool.len();
    let mut sets = Vec::new();
    let mut idx: Vec<usize> = (0..k).collect();
    loop {
        let mut set: Vec<RowLabel> = idx.iter().filter_map(|&i| pool.get(i).copied()).collect();
        set.sort_unstable();
        sets.push(set);
        // Advance to the next combination: the rightmost index that can still grow grows by
        // one and every index after it follows on consecutively.
        let mut advanced = false;
        for i in (0..k).rev() {
            let limit = n - k + i;
            let Some(v) = idx.get(i).copied() else { break };
            if v < limit {
                for (off, slot) in idx.iter_mut().skip(i).enumerate() {
                    *slot = v + 1 + off;
                }
                advanced = true;
                break;
            }
        }
        if !advanced {
            return sets;
        }
    }
}

/// A finite `x` as `(negative, significand, exponent)` with `|x| = significand * 2^exponent`
/// exactly.
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

/// The exact order of `v` against the real product `t * w`, for finite `v` and finite,
/// nonnegative `t` and `w` (the sign bits of `t` and `w` are ignored, so `-0.0` counts as 0).
/// No rounding happens: the significand product has at most 106 bits and is held in a
/// `u128`, and the two sides are aligned by shifts that the equal-magnitude case keeps below
/// 107 bits (see the comments). The error is for a shift out of range, which these bounds
/// rule out; it is kept so that a defect fails closed instead of misclassifying a row.
fn cmp_scaled(v: f64, t: f64, w: f64) -> Result<Ordering, CertError> {
    let internal = CertError::InvalidInput { field: "compare" };
    let (v_neg, v_m, v_e) = decompose(v);
    let (_, t_m, t_e) = decompose(t);
    let (_, w_m, w_e) = decompose(w);
    let p_m = u128::from(t_m) * u128::from(w_m);
    let p_e = t_e + w_e;
    if v_m == 0 {
        return Ok(if p_m == 0 { Ordering::Equal } else { Ordering::Less });
    }
    if v_neg {
        return Ok(Ordering::Less);
    }
    if p_m == 0 {
        return Ok(Ordering::Greater);
    }
    // Both positive. Compare the binary magnitudes (exponent of the leading bit) first.
    let v_len = 64 - v_m.leading_zeros() as i32;
    let p_len = 128 - p_m.leading_zeros() as i32;
    let (v_top, p_top) = (v_e + v_len, p_e + p_len);
    if v_top != p_top {
        return Ok(v_top.cmp(&p_top));
    }
    // Same leading-bit position: align the lower exponent's side. If v_e >= p_e the shift is
    // p_len - v_len < 106 and the shifted v has p_len <= 106 bits; otherwise the shift is
    // v_len - p_len < 64 and the shifted product has v_len <= 64 bits.
    let v_m = u128::from(v_m);
    let shift = |x: u128, by: i32| u32::try_from(by).ok().and_then(|by| x.checked_shl(by));
    if v_e >= p_e {
        let v_al = shift(v_m, v_e - p_e).ok_or(internal)?;
        Ok(v_al.cmp(&p_m))
    } else {
        let p_al = shift(p_m, p_e - v_e).ok_or(internal)?;
        Ok(v_m.cmp(&p_al))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binomial_cap_is_exact() {
        // C(256, 1) = 256 is within the cap, C(257, 1) is not; C(10, 3) = 120, C(10, 4) = 210,
        // C(10, 5) = 252, C(11, 5) = 462; C(n, 0) = 1; k > n has no subset.
        assert!(binomial_within_cap(256, 1));
        assert!(!binomial_within_cap(257, 1));
        assert!(binomial_within_cap(256, 255));
        assert!(!binomial_within_cap(257, 256));
        assert!(binomial_within_cap(10, 5));
        assert!(!binomial_within_cap(11, 5));
        assert!(binomial_within_cap(usize::MAX, 0));
        assert!(!binomial_within_cap(usize::MAX, 1));
        assert!(!binomial_within_cap(3, 4));
    }

    #[test]
    fn combinations_in_lexicographic_order() {
        let l = |point| RowLabel { point, side: 0 };
        let pool = [l(1), l(3), l(4), l(5)];
        let got = combinations(&pool, 3);
        let want: Vec<Vec<RowLabel>> =
            vec![vec![l(1), l(3), l(4)], vec![l(1), l(3), l(5)], vec![l(1), l(4), l(5)], vec![l(3), l(4), l(5)]];
        assert_eq!(got, want);
        assert_eq!(combinations(&pool, 4), vec![pool.to_vec()]);
        assert_eq!(combinations(&pool, 1).len(), 4);
    }

    #[test]
    fn cmp_scaled_is_exact() {
        use Ordering::{Equal, Greater, Less};
        assert_eq!(cmp_scaled(0.0, 0.0, 5.0), Ok(Equal));
        assert_eq!(cmp_scaled(-0.0, 1.0, 0.0), Ok(Equal));
        assert_eq!(cmp_scaled(-1e-300, 0.0, 0.0), Ok(Less));
        assert_eq!(cmp_scaled(1e-300, 0.0, 1.0), Ok(Greater));
        assert_eq!(cmp_scaled(6.0, 2.0, 3.0), Ok(Equal));
        // 0.1 * 3 rounds to 0.30000000000000004, but the exact product of the two doubles is
        // below that double and above 0.3's double.
        let p = 0.1_f64 * 3.0;
        assert_eq!(cmp_scaled(p, 0.1, 3.0), Ok(Greater));
        assert_eq!(cmp_scaled(0.3, 0.1, 3.0), Ok(Less));
        // Extreme exponents: the smallest subnormal and the largest finite value.
        let tiny = f64::from_bits(1);
        assert_eq!(cmp_scaled(tiny, tiny, 1.0), Ok(Equal));
        assert_eq!(cmp_scaled(tiny, tiny, tiny), Ok(Greater));
        assert_eq!(cmp_scaled(f64::MAX, f64::MAX, 2.0), Ok(Less));
        assert_eq!(cmp_scaled(f64::MAX, f64::MAX, 1.0), Ok(Equal));
        assert_eq!(cmp_scaled(f64::MAX, 0.5, 2.0), Ok(Greater));
    }
}
