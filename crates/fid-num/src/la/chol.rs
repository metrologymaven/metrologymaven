//! Cholesky factorization with symmetric (diagonal) pivoting, rank-revealing for positive
//! semi-definite matrices. Added for spec node `UNC.MODEL.DIST` (correlated normal sampling
//! and the correlation-matrix check), whose Design names "Cholesky with symmetric pivoting
//! from `fid-num`"; the rules below are pinned there too.
//!
//! # Factorization (pinned)
//!
//! Only the lower triangle of the square `n x n` input `A` is read: entry `(i, j)` means
//! `A[max(i, j)][min(i, j)]`, so a factor of an exactly symmetric matrix does not depend on
//! which triangle a caller filled. The permutation `perm` starts as the identity and `L` as
//! the `n x n` zero matrix (row-major). For `k = 0 .. n-1`:
//!
//! 1. **Remaining diagonals:** for `j = k .. n-1` in ascending order,
//!    `d_j = a(perm[j], perm[j]) - red::sum_sq(L[j][0..k])` (one subtraction; the empty sum is
//!    `+0.0`). A `d_j` that is not finite is [`NumError::Infinite`] with `index: Some(k)`.
//! 2. **Pivot:** `p` is the first `j` with the largest `d_j` (a later `j` replaces the best only
//!    if `d_j > d_best`, so ties go to the lowest position). At `k = 0`, `thr = rel_tol * d_p`
//!    (one rounded product) is fixed for the whole factorization.
//! 3. **Stop:** if `d_p <= 0` or `d_p <= thr`, the factorization stops with `rank = k`; the
//!    rows `k .. n-1` of `L` stay zero in columns `k ..`. A matrix that is not positive
//!    semi-definite (in exact arithmetic) stops at the first non-positive remaining diagonal.
//! 4. **Swap:** positions `k` and `p` of `perm` are exchanged, and so are rows `k` and `p` of
//!    the already computed columns `L[.][0..k]`.
//! 5. **Column:** `L[k][k] = f::sqrt(d_p)`; for `i = k+1 .. n-1` in ascending order,
//!    `L[i][k] = (a(perm[i], perm[k]) - red::dot(L[i][0..k], L[k][0..k])) / L[k][k]`. A dot
//!    that overflows or a result that is not finite is [`NumError::Infinite`] with
//!    `index: Some(k)`.
//!
//! If no step stops, `rank = n`. In exact arithmetic `L L^T` equals the leading `rank`
//! Schur-complement part of `P A P^T`, where `(P A P^T)[i][j] = A[perm[i]][perm[j]]`.
//! There is no FMA; every operation is a plain IEEE-754 operation, [`crate::red`] or
//! [`crate::f::sqrt`], so the factor is bit-identical on x86-64, ARM64 and wasm32.
//!
//! # Errors
//!
//! `op` is `la.chol_pivoted`. Check order: a non-square input (`BadShape`), `rel_tol` (NaN,
//! infinity, negative), then the overflow and allocation errors above. Entries of `A` are
//! finite by construction of [`Mat`].

use super::{alloc_filled, check_tol, LaError, Mat};
use crate::err::NumError;
use crate::{f, red};

/// A pivoted Cholesky factor: `P A P^T ~ L L^T` with `L` lower triangular, zero beyond
/// column `rank - 1`.
#[derive(Debug, Clone, PartialEq)]
pub struct CholPivoted {
    l: Mat,
    perm: Vec<usize>,
    rank: usize,
}

impl CholPivoted {
    /// The `n x n` lower-triangular factor (row `i` belongs to original index `perm[i]`).
    pub fn l(&self) -> &Mat {
        &self.l
    }

    /// The symmetric permutation: position `i` of the factor is original index `perm[i]`.
    pub fn perm(&self) -> &[usize] {
        &self.perm
    }

    /// Number of pivots taken before the stop rule fired (`n` if it never did).
    pub fn rank(&self) -> usize {
        self.rank
    }
}

/// Pivoted Cholesky of the lower triangle of the square matrix `a` under the relative stop
/// tolerance `rel_tol` (see the module documentation for the pinned rules).
pub fn chol_pivoted(a: &Mat, rel_tol: f64) -> Result<CholPivoted, NumError> {
    const OP: &str = "la.chol_pivoted";
    let n = a.rows();
    if a.cols() != n {
        return Err(LaError::BadShape.at(OP));
    }
    check_tol(OP, rel_tol)?;
    let src = a.data();
    let at = |i: usize, j: usize| if i >= j { src[i * n + j] } else { src[j * n + i] };
    let mut l = alloc_filled(OP, n * n, 0.0)?;
    let mut perm: Vec<usize> = Vec::new();
    perm.try_reserve_exact(n).map_err(|_| LaError::Allocation.at(OP))?;
    perm.extend(0..n);
    let mut thr = 0.0_f64;
    let mut rank = n;
    for k in 0..n {
        let mut p = k;
        let mut best = 0.0_f64;
        for (j, &pj) in perm.iter().enumerate().skip(k) {
            let s = red::sum_sq(&l[j * n..j * n + k]).map_err(|_| NumError::Infinite { op: OP, index: Some(k) })?;
            let d = at(pj, pj) - s;
            if !d.is_finite() {
                return Err(NumError::Infinite { op: OP, index: Some(k) });
            }
            if j == k || d > best {
                p = j;
                best = d;
            }
        }
        if k == 0 {
            thr = rel_tol * best;
        }
        if best <= 0.0 || best <= thr {
            rank = k;
            break;
        }
        if p != k {
            perm.swap(k, p);
            for c in 0..k {
                l.swap(k * n + c, p * n + c);
            }
        }
        let lkk = f::sqrt(best)?;
        l[k * n + k] = lkk;
        let pk = perm[k];
        for (i, &pi) in perm.iter().enumerate().skip(k + 1) {
            let s = red::dot(&l[i * n..i * n + k], &l[k * n..k * n + k])
                .map_err(|_| NumError::Infinite { op: OP, index: Some(k) })?;
            let v = (at(pi, pk) - s) / lkk;
            if !v.is_finite() {
                return Err(NumError::Infinite { op: OP, index: Some(k) });
            }
            l[i * n + k] = v;
        }
    }
    Ok(CholPivoted { l: Mat::new(n, n, l)?, perm, rank })
}
