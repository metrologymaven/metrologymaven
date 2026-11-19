//! Knot span search and B-spline basis functions with derivatives. Spec node `GEO.NURBS.EVAL`.
//!
//! Every value-affecting choice is pinned in `spec/geo/nurbs/eval/NODE.md` (Design) and mirrored
//! bit for bit by `fixtures/geo/nurbs/eval/make_eval_fixtures.py`. Plain IEEE 754 binary64
//! `+ - * /` only, each written out (no `mul_add`), in the order given here.
//!
//! Everything is on fixed-size stack arrays (degree at most [`MAX_DEGREE`]); nothing allocates.

use crate::data::MAX_DEGREE;
use crate::eval::EvalError;

/// Basis functions per order: at most degree + 1 are non-zero on a span.
pub(crate) const W: usize = MAX_DEGREE + 1;

/// Highest derivative order the evaluator returns.
pub const MAX_ORDER: usize = 2;

/// Basis values and derivatives on one span: `[d][j]` is the `d`-th derivative of
/// `N_{s-p+j,p}` for `d <= order`, `j <= p`; other entries are zero.
pub(crate) type BasisTable = [[f64; W]; MAX_ORDER + 1];

/// Knot span `s` for parameter `t` in `[knots[p], knots[n]]`, `n` control points:
/// right-continuous (`knots[s] <= t < knots[s+1]`, the largest such `s` by binary search), and
/// for `t >= knots[n]` the last non-empty span (step down from `n - 1` while
/// `knots[s] == knots[s+1]`). Always `p <= s < n` and `knots[s] < knots[s+1]` for validated
/// knots. `None` only for inconsistent input (never for a validated curve or surface).
pub(crate) fn find_span(knots: &[f64], p: usize, n: usize, t: f64) -> Option<usize> {
    if n <= p || knots.len() < n + 1 {
        return None;
    }
    let at = |i: usize| knots.get(i).copied();
    if t >= at(n)? {
        let mut s = n - 1;
        while s > p && at(s)? == at(s + 1)? {
            s -= 1;
        }
        return Some(s);
    }
    let (mut lo, mut hi) = (p, n);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if t < at(mid)? {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(lo)
}

/// Basis functions of degree `p` on span `s` and their derivatives up to `order`
/// (`order <= MAX_ORDER`). `NonFinite` when the knot window `knots[s+p] - knots[s-p]` is not
/// finite (every knot difference the recurrences use lies inside it, so none can overflow);
/// `Inconsistent` only for input a validated curve or surface never gives.
pub(crate) fn basis(knots: &[f64], p: usize, s: usize, t: f64, order: usize) -> Result<BasisTable, EvalError> {
    if p == 0 || p > MAX_DEGREE || order > MAX_ORDER {
        return Err(EvalError::Inconsistent);
    }
    // The knots that matter: knots[s-p ..= s+p]. In this window the span index is `p`, so every
    // index below lies in 0 ..= 2p = window.len() - 1.
    let lo = s.checked_sub(p).ok_or(EvalError::Inconsistent)?;
    let window = knots.get(lo..=s + p).ok_or(EvalError::Inconsistent)?;
    if !(window[2 * p] - window[0]).is_finite() {
        return Err(EvalError::NonFinite);
    }
    Ok(basis_window(window, p, t, order))
}

fn basis_window(u: &[f64], p: usize, t: f64, order: usize) -> BasisTable {
    let s = p;
    // Cox-de Boor triangle: lv[k][j] = N_{s-k+j,k}(t), k = 0..=p, j = 0..=k.
    let mut lv = [[0.0_f64; W]; W];
    lv[0][0] = 1.0;
    for k in 1..=p {
        let mut saved = 0.0_f64;
        for r in 0..k {
            let den = u[s + r + 1] - u[s + r + 1 - k];
            let temp = lv[k - 1][r] / den;
            lv[k][r] = saved + (u[s + r + 1] - t) * temp;
            saved = (t - u[s + r + 1 - k]) * temp;
        }
        lv[k][k] = saved;
    }
    let mut out = [[0.0_f64; W]; MAX_ORDER + 1];
    out[0] = lv[p];
    if order >= 1 {
        out[1] = lower_derivative(u, s, &lv[p - 1], p);
    }
    if order >= 2 {
        // Derivative of the degree p-1 functions (zero for degree 0), then once more.
        let lower = if p >= 2 { lower_derivative(u, s, &lv[p - 2], p - 1) } else { [0.0_f64; W] };
        out[2] = lower_derivative(u, s, &lower, p);
    }
    out
}

/// One derivative step: given values `prev[m]` of the degree `k-1` functions `N_{s-k+1+m}`
/// (`m < k`), returns for `j = 0..=k`
/// `k * (prev[j-1] / (u[s+j] - u[s+j-k]) - prev[j] / (u[s+j+1] - u[s+j+1-k]))`,
/// an absent term being the literal `0.0`. Denominators are positive on a non-empty span.
fn lower_derivative(u: &[f64], s: usize, prev: &[f64; W], k: usize) -> [f64; W] {
    let kf = k as f64;
    let mut out = [0.0_f64; W];
    for (j, slot) in out.iter_mut().enumerate().take(k + 1) {
        let a = if j >= 1 { prev[j - 1] / (u[s + j] - u[s + j - k]) } else { 0.0 };
        let b = if j < k { prev[j] / (u[s + j + 1] - u[s + j + 1 - k]) } else { 0.0 };
        *slot = kf * (a - b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadratic_bezier_at_half_is_hand_checked() {
        let u = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let s = find_span(&u, 2, 3, 0.5).unwrap_or(99);
        assert_eq!(s, 2);
        let b = basis(&u, 2, s, 0.5, 2).unwrap_or([[9.0; W]; 3]);
        assert_eq!(&b[0][..3], &[0.25, 0.5, 0.25]);
        assert_eq!(&b[1][..3], &[-1.0, 0.0, 1.0]);
        assert_eq!(&b[2][..3], &[2.0, -4.0, 2.0]);
    }

    #[test]
    fn span_rule_at_repeated_knot_and_domain_end() {
        // degree 1, interior knot 0.5 of multiplicity 2 (REQ-GEO.NURBS.EVAL-006)
        let u = [0.0, 0.0, 0.5, 0.5, 1.0, 1.0];
        assert_eq!(find_span(&u, 1, 4, 0.5), Some(3));
        assert_eq!(find_span(&u, 1, 4, 0.4999), Some(1));
        assert_eq!(find_span(&u, 1, 4, 1.0), Some(3));
        assert_eq!(find_span(&u, 1, 4, 0.0), Some(1));
        // inconsistent input is refused, not indexed out of bounds
        assert_eq!(find_span(&u, 4, 4, 0.5), None);
        assert_eq!(find_span(&u[..3], 1, 4, 0.5), None);
        assert_eq!(basis(&u, 0, 1, 0.5, 0), Err(EvalError::Inconsistent));
        assert_eq!(basis(&u, 1, 9, 0.5, 0), Err(EvalError::Inconsistent));
        assert_eq!(basis(&u, 1, 1, 0.5, 3), Err(EvalError::Inconsistent));
    }

    #[test]
    fn overflowing_knot_window_is_non_finite() {
        let u = [-1e308, -1e308, 0.0, 1.0, 1e308, 1e308];
        assert_eq!(basis(&u, 2, 2, 0.5, 0), Err(EvalError::NonFinite));
    }
}
