//! Triangular solves and the rank rule (`KRN.NUM.LA.MAT`, REQ-002, REQ-003, REQ-004).

use super::{alloc, alloc_filled, check_tol, singular_scan, LaError, Mat};
use crate::err::{check_finite_slice, NumError};
use crate::red;

/// Why a substitution stopped: an overflow at unknown `i`, or a failed reservation.
pub(crate) enum SubError {
    Overflow(usize),
    Allocation,
}

impl SubError {
    /// The solves' mapping: overflow at `i` is `Infinite { index: Some(i) }`.
    fn at(self, op: &'static str) -> NumError {
        match self {
            SubError::Overflow(i) => NumError::Infinite { op, index: Some(i) },
            SubError::Allocation => LaError::Allocation.at(op),
        }
    }
}

/// Back substitution on the upper triangle of the square row-major `n x n` matrix `r`:
/// `x[i] = (b[i] - dot(r[i, i+1..n], x[i+1..n])) / r[i][i]` for `i = n-1` down to `0`.
/// Fails at the first `i` (in solve order) whose dot overflows or whose `x[i]` is not finite.
pub(crate) fn back_sub(n: usize, r: &[f64], b: &[f64]) -> Result<Vec<f64>, SubError> {
    let mut x = alloc_filled("la.back_sub", n, 0.0).map_err(|_| SubError::Allocation)?;
    for i in (0..n).rev() {
        let s = red::dot(&r[i * n + i + 1..(i + 1) * n], &x[i + 1..]).map_err(|_| SubError::Overflow(i))?;
        let xi = (b[i] - s) / r[i * n + i];
        if !xi.is_finite() {
            return Err(SubError::Overflow(i));
        }
        x[i] = xi;
    }
    Ok(x)
}

/// Forward substitution on the lower triangle of the square row-major `n x n` matrix `l`:
/// `x[i] = (b[i] - dot(l[i, 0..i], x[0..i])) / l[i][i]` for `i = 0` up to `n-1`.
/// Fails at the first `i` whose dot overflows or whose `x[i]` is not finite.
pub(crate) fn fwd_sub(n: usize, l: &[f64], b: &[f64]) -> Result<Vec<f64>, SubError> {
    let mut x = alloc("la.fwd_sub", n).map_err(|_| SubError::Allocation)?;
    for i in 0..n {
        let s = red::dot(&l[i * n..i * n + i], &x[..i]).map_err(|_| SubError::Overflow(i))?;
        let xi = (b[i] - s) / l[i * n + i];
        if !xi.is_finite() {
            return Err(SubError::Overflow(i));
        }
        x.push(xi);
    }
    Ok(x)
}

/// Shared checks of the solves, in the pinned order: `r` square and `b.len() == n`
/// (`BadShape`), the first NaN or infinity of `b`, `rel_tol` (NaN, infinity, negative), then
/// the singularity scan of the diagonal.
fn check_solve(op: &'static str, r: &Mat, b: &[f64], rel_tol: f64) -> Result<usize, NumError> {
    let n = r.rows();
    if r.cols() != n || b.len() != n {
        return Err(LaError::BadShape.at(op));
    }
    check_finite_slice(op, b)?;
    check_tol(op, rel_tol)?;
    singular_scan(op, n, r.data(), rel_tol)?;
    Ok(n)
}

/// Solves `R x = b` for upper triangular `R` by back substitution (only the upper triangle,
/// diagonal included, is read). `Singular { index }` (as [`NumError::Singular`] with `rank`
/// = index, see [`LaError::of`]) if `|r_ii| <= rel_tol * max_j |r_jj|`, checked before any
/// arithmetic; `Infinite { index: Some(i) }` for the first unknown, in solve order
/// (descending `i`), that overflows.
pub fn solve_upper(r: &Mat, b: &[f64], rel_tol: f64) -> Result<Vec<f64>, NumError> {
    const OP: &str = "la.solve_upper";
    let n = check_solve(OP, r, b, rel_tol)?;
    back_sub(n, r.data(), b).map_err(|e| e.at(OP))
}

/// Solves `L x = b` for lower triangular `L` by forward substitution (only the lower
/// triangle, diagonal included, is read). Errors as [`solve_upper`], with solve order
/// ascending `i`.
pub fn solve_lower(l: &Mat, b: &[f64], rel_tol: f64) -> Result<Vec<f64>, NumError> {
    const OP: &str = "la.solve_lower";
    let n = check_solve(OP, l, b, rel_tol)?;
    fwd_sub(n, l.data(), b).map_err(|e| e.at(OP))
}

/// The rank rule (REQ-KRN.NUM.LA.MAT-004): the number of values in `diag_desc` strictly
/// greater than `rel_tol * diag_desc[0]` (one rounded product). `diag_desc` holds R
/// diagonal magnitudes or singular values, non-negative and non-increasing.
///
/// Check order: empty (`EmptyInput`), the first NaN or infinity, `rel_tol` (NaN, infinity,
/// negative: `BadTolerance`), then a negative value or an increase `d[i] < d[i + 1]`
/// (`BadSpectrum`). `-0.0` counts as zero.
pub fn rank_by_rule(diag_desc: &[f64], rel_tol: f64) -> Result<usize, NumError> {
    const OP: &str = "la.rank_by_rule";
    let Some(&first) = diag_desc.first() else {
        return Err(NumError::EmptyInput { op: OP });
    };
    check_finite_slice(OP, diag_desc)?;
    check_tol(OP, rel_tol)?;
    if diag_desc.iter().any(|&d| d < 0.0) || diag_desc.windows(2).any(|w| w[0] < w[1]) {
        return Err(LaError::BadSpectrum.at(OP));
    }
    let thr = rel_tol * first;
    Ok(diag_desc.iter().filter(|&&d| d > thr).count())
}
