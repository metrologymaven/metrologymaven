//! Householder QR, column pivoting and least squares. Spec node `KRN.NUM.LA.QR`.
//!
//! Every value-affecting choice is pinned here; the Python model in
//! `fixtures/num/la/make_qr_fixtures.py` reproduces it bit for bit, and an exact rational
//! oracle there checks the REQ-006 bounds on every fixture factor.
//!
//! # Factorization (REQ-KRN.NUM.LA.QR-001, -002, -007)
//!
//! `A` (`m x n`, `m >= n`) is copied column by column into a column-major work array `W`, so
//! every inner product runs over a contiguous slice in ascending row order. The permutation
//! starts as the identity. For `k = 0 .. n-1`:
//!
//! 1. **Pivot** ([`qr_pivoted`] only): the 2-norms of the trailing parts `W[j][k..m]` of
//!    columns `j = k .. n-1` are recomputed from scratch with [`crate::red::norm2`] (never
//!    downdated), in ascending `j`; column `j` replaces the current best only if its norm is
//!    strictly greater in total order, so ties go to the lowest column index. The chosen
//!    column is swapped with column `k` (all `m` rows) and so are the permutation entries.
//!    `xnorm` is the chosen norm. [`qr`] takes `xnorm = norm2(W[k][k..m])`.
//! 2. **Zero column:** if `xnorm == 0`, `tau[k] = 0` and the column is left untouched (its
//!    diagonal is the stored `+0.0` or `-0.0`); the step is the identity.
//! 3. **Reflector:** `alpha = W[k][k]`; `beta = -xnorm` if `alpha > 0`, else `+xnorm` (so
//!    `r_kk` has the opposite sign of `a_kk`, and is positive when `a_kk` is `+0.0` or `-0.0`);
//!    `u0 = alpha - beta`; `v = [1, W[k][k+1] / u0, .., W[k][m-1] / u0]` (one division each,
//!    `|v_i| <= 1`); `tau = (beta - alpha) / beta` (in `[1, 2]`). `W[k][k] = beta` and
//!    `W[k][k+1..m]` stores `v[1..]` (LAPACK-style compact storage; `v[0] = 1` is implicit).
//! 4. **Update:** for each later column `j = k+1 .. n-1` in ascending order:
//!    `s = tau * red::dot(v, W[j][k..m])`, then `W[j][i] = W[j][i] - s * v[i - k]` for
//!    `i = k .. m-1` (one product and one subtraction each; no FMA).
//!
//! Applying the reflectors to a vector uses the same update with the stored `v` and `tau`:
//! `Q^T b` applies steps `k = 0 .. n-1`, `Q y` steps `k = n-1 .. 0`. A step with `tau == 0`
//! is skipped entirely, so it cannot change a signed zero. The thin `Q` (`m x n`) has column
//! `j` equal to `Q e_j`.
//!
//! # Diagnostics (REQ-KRN.NUM.LA.QR-003)
//!
//! `R` is the `n x n` upper triangle of the packed factor. `rank` is [`super::rank_by_rule`]
//! on the diagonal magnitudes `|r_kk|` sorted in descending total order, so it counts the
//! `|r_kk| > rel_tol * max_j |r_jj|` (the same threshold as the triangular solves'
//! singularity test). If `rank == n`, `cond` is [`super::cond_triangular`]`(R, rel_tol)`;
//! if `rank < n` (a singular factor has no finite condition number), or the estimate is beyond
//! the `f64` range, `cond` is `f64::INFINITY`. Rank deficiency never fails `qr` or
//! `qr_pivoted`: it is reported in `rank` (and `cond` is infinite), and [`lstsq`] turns it into
//! [`LaError::RankDeficient`].
//!
//! # Least squares (REQ-KRN.NUM.LA.QR-004)
//!
//! [`lstsq`] factors with column pivoting, fails with `RankDeficient { rank }` when
//! `rank < n`, then computes `c = Q^T b`, solves `R y = c[0..n]` with
//! [`super::solve_upper`]`(R, c[0..n], rel_tol)`, un-permutes `x[perm[i]] = y[i]`, and reports
//! `residual_norm = red::norm2(c[n..m])` (`+0.0` when `m == n`).
//!
//! # Errors (REQ-KRN.NUM.LA.QR-005)
//!
//! Check order, `op` `la.qr`, `la.qr_pivoted`, `la.lstsq`, `la.apply_qt`, `la.apply_q`:
//!
//! - `m < n` (and, for `lstsq`, `b.len() != m`): `BadShape`. Entries of `A` are finite by
//!   construction of [`Mat`].
//! - `lstsq`: the first NaN or infinity of `b` (`NotANumber` / `Infinite` with its index).
//! - `rel_tol`: NaN (`NotANumber { index: None }`), infinity (`Infinite { index: None }`),
//!   negative (`BadTolerance`); `-0.0` acts as zero.
//! - **Subnormal input:** a matrix or vector that is not all zeros but whose largest magnitude
//!   is below `2^-1022` is [`LaError::Subnormal`]. Every entry of such an input has fewer than
//!   53 significant bits, so the REQ-006 bounds, which are relative to `||A||_F`, cannot hold
//!   and the result would be silently inaccurate. A subnormal entry in an input with a normal
//!   largest magnitude is accepted: its absolute error of at most `2^-1075` is below
//!   `u * max |a_ij|`. Checked for `A`, then `b` (`lstsq`), and for the vector of
//!   `apply_qt` / `apply_q` (after its length and finiteness).
//! - **Overflow:** a norm, `u0`, a reflector product `s` or an updated entry that is not finite
//!   at step `k` is `Infinite { index: Some(k) }`. In `lstsq` an overflowing unknown `y[i]` is
//!   `Infinite { index: Some(perm[i]) }` (the original column) and an overflowing residual norm
//!   `Infinite { index: None }`. Rank deficiency is checked before `Q^T b` is formed.
//! - Every buffer is reserved with `try_reserve_exact` for exactly the elements it holds:
//!   failure is [`LaError::Allocation`], never an abort. Peak use of `qr_pivoted`: the work
//!   array and the packed factor (`2 m n` values), `R` and the estimator's two `n x n`
//!   scratch matrices, plus `O(m)` vectors. Every loop is bounded by `m, n <= MAX_DIM`.

use core::cmp::Ordering;

use super::{alloc, alloc_filled, check_tol, cond_triangular, rank_by_rule, solve_upper, Factorization, LaError, Mat};
use crate::err::{check_finite_slice, NumError};
use crate::{f, red};

/// A Householder QR factorization `A P = Q R` of an `m x n` matrix, `m >= n`.
///
/// The fields are private so that their shapes always agree: `packed` is `m x n`, `tau` and
/// `perm` have `n` entries and `perm` is a permutation of `0..n`.
#[derive(Debug, Clone, PartialEq)]
pub struct Qr {
    packed: Mat,
    tau: Vec<f64>,
    perm: Vec<usize>,
    diag: Factorization,
}

/// The result of [`lstsq`]: the least-squares solution, the residual norm `||Q^T b||` over rows
/// `n..m` and the factorization diagnostics (`rank == n`).
#[derive(Debug, Clone, PartialEq)]
pub struct LstsqResult {
    /// The solution `x` (length `n`), in the original column order.
    pub x: Vec<f64>,
    /// `red::norm2` of the last `m - n` entries of `Q^T b`.
    pub residual_norm: f64,
    /// Rank, condition estimate and tolerance of the pivoted factorization.
    pub diag: Factorization,
}

/// Refuses a nonzero input whose largest magnitude is subnormal (see the module docs).
fn check_scale(op: &'static str, xs: &[f64]) -> Result<(), NumError> {
    let mut m = 0.0_f64;
    for &x in xs {
        m = f::total_max(m, x.abs())?;
    }
    if m != 0.0 && m < f64::MIN_POSITIVE {
        return Err(LaError::Subnormal.at(op));
    }
    Ok(())
}

fn overflow(op: &'static str, k: usize) -> NumError {
    NumError::Infinite { op, index: Some(k) }
}

/// The update of step `k`: `c[i] = c[i] - (tau * dot(v, c)) * v[i]`, every result finite.
fn reflect(op: &'static str, k: usize, c: &mut [f64], v: &[f64], tau: f64) -> Result<(), NumError> {
    let s = tau * red::dot(v, c).map_err(|_| overflow(op, k))?;
    if !s.is_finite() {
        return Err(overflow(op, k));
    }
    for (ci, &vi) in c.iter_mut().zip(v) {
        let y = *ci - s * vi;
        if !y.is_finite() {
            return Err(overflow(op, k));
        }
        *ci = y;
    }
    Ok(())
}

fn identity_perm(op: &'static str, n: usize) -> Result<Vec<usize>, NumError> {
    let mut p = Vec::new();
    p.try_reserve_exact(n).map_err(|_| LaError::Allocation.at(op))?;
    p.extend(0..n);
    Ok(p)
}

/// Shared checks of `qr` and `qr_pivoted` (shape, tolerance, scale).
fn check_qr(op: &'static str, a: &Mat, rel_tol: f64) -> Result<(), NumError> {
    if a.rows() < a.cols() {
        return Err(LaError::BadShape.at(op));
    }
    check_tol(op, rel_tol)?;
    check_scale(op, a.data())
}

/// Steps 1-4 of the module docs, then the diagnostics.
fn factor(op: &'static str, a: &Mat, pivot: bool, rel_tol: f64) -> Result<Qr, NumError> {
    let (m, n) = (a.rows(), a.cols());
    let data = a.data();
    let mut w = alloc(op, m * n)?;
    for j in 0..n {
        for i in 0..m {
            w.push(data[i * n + j]);
        }
    }
    let mut tau = alloc(op, n)?;
    let mut perm = identity_perm(op, n)?;
    let mut v = alloc(op, m)?;
    for k in 0..n {
        let xnorm = if pivot {
            let mut best = k;
            let mut best_norm = red::norm2(&w[k * m + k..(k + 1) * m]).map_err(|_| overflow(op, k))?;
            for j in k + 1..n {
                let c = red::norm2(&w[j * m + k..(j + 1) * m]).map_err(|_| overflow(op, k))?;
                if f::total_cmp(c, best_norm)? == Ordering::Greater {
                    best = j;
                    best_norm = c;
                }
            }
            if best != k {
                for i in 0..m {
                    w.swap(k * m + i, best * m + i);
                }
                perm.swap(k, best);
            }
            best_norm
        } else {
            red::norm2(&w[k * m + k..(k + 1) * m]).map_err(|_| overflow(op, k))?
        };
        if xnorm == 0.0 {
            tau.push(0.0);
            continue;
        }
        let alpha = w[k * m + k];
        let beta = if alpha > 0.0 { -xnorm } else { xnorm };
        let u0 = alpha - beta;
        if !u0.is_finite() {
            return Err(overflow(op, k));
        }
        v.clear();
        v.push(1.0);
        for i in k + 1..m {
            v.push(w[k * m + i] / u0);
        }
        let t = (beta - alpha) / beta;
        tau.push(t);
        w[k * m + k] = beta;
        w[k * m + k + 1..(k + 1) * m].copy_from_slice(&v[1..]);
        for j in k + 1..n {
            reflect(op, k, &mut w[j * m + k..(j + 1) * m], &v, t)?;
        }
    }
    let mut packed = alloc(op, m * n)?;
    for i in 0..m {
        for j in 0..n {
            packed.push(w[j * m + i]);
        }
    }
    drop(w);
    let packed = Mat::new(m, n, packed)?;
    let r = upper(op, &packed)?;
    let diag = diagnostics(op, &r, rel_tol)?;
    Ok(Qr { packed, tau, perm, diag })
}

/// The `n x n` upper triangle of an `m x n` packed factor, zeros below the diagonal.
fn upper(op: &'static str, packed: &Mat) -> Result<Mat, NumError> {
    let n = packed.cols();
    let p = packed.data();
    let mut d = alloc(op, n * n)?;
    for i in 0..n {
        for j in 0..n {
            d.push(if j >= i { p[i * n + j] } else { 0.0 });
        }
    }
    Mat::new(n, n, d)
}

/// Rank by the `KRN.NUM.LA.MAT` rule on `|r_kk|`; condition estimate when of full rank.
fn diagnostics(op: &'static str, r: &Mat, rel_tol: f64) -> Result<Factorization, NumError> {
    let n = r.rows();
    let rd = r.data();
    let mut d = alloc(op, n)?;
    for k in 0..n {
        d.push(rd[k * n + k].abs());
    }
    // Finite, non-negative values: total order is numeric order, and values that compare
    // equal are bit-identical, so the unstable in-place sort is deterministic.
    d.sort_unstable_by(|x, y| y.total_cmp(x));
    let rank = rank_by_rule(&d, rel_tol)?;
    let cond = if rank < n {
        f64::INFINITY
    } else {
        match cond_triangular(r, rel_tol) {
            Ok(c) => c,
            Err(e) if LaError::of(&e) == Some(LaError::Allocation) => return Err(LaError::Allocation.at(op)),
            // Beyond the f64 range (`Infinite`); `Singular` cannot occur because the rank
            // rule and the singularity test use the same threshold.
            Err(_) => f64::INFINITY,
        }
    };
    Ok(Factorization { rank, cond, rel_tol })
}

/// Householder QR without pivoting: `A = Q R` (`perm` is the identity).
///
/// Errors (in this order): `m < n` (`BadShape`), `rel_tol` (NaN, infinity, negative), a
/// subnormal-scale `A` (`Subnormal`), an overflow at step `k` (`Infinite { index: Some(k) }`),
/// allocation failure. A rank-deficient `A` is not an error: see [`Qr::diag`].
pub fn qr(a: &Mat, rel_tol: f64) -> Result<Qr, NumError> {
    const OP: &str = "la.qr";
    check_qr(OP, a, rel_tol)?;
    factor(OP, a, false, rel_tol)
}

/// Householder QR with column pivoting: `A P = Q R`, pivot by the largest remaining column
/// norm (recomputed every step), ties to the lowest column index. Errors as [`qr`].
pub fn qr_pivoted(a: &Mat, rel_tol: f64) -> Result<Qr, NumError> {
    const OP: &str = "la.qr_pivoted";
    check_qr(OP, a, rel_tol)?;
    factor(OP, a, true, rel_tol)
}

impl Qr {
    /// Number of rows `m` of the factored matrix.
    pub fn rows(&self) -> usize {
        self.packed.rows()
    }

    /// Number of columns `n` of the factored matrix.
    pub fn cols(&self) -> usize {
        self.packed.cols()
    }

    /// The compact factor (`m x n`): `R` on and above the diagonal, the Householder vectors
    /// `v[1..]` below it (`v[0] = 1` implicit).
    pub fn packed(&self) -> &Mat {
        &self.packed
    }

    /// The reflector scalars `tau` (length `n`); `0` marks a skipped (identity) step.
    pub fn tau(&self) -> &[f64] {
        &self.tau
    }

    /// The column permutation: column `k` of `A P` is column `perm[k]` of `A`.
    pub fn perm(&self) -> &[usize] {
        &self.perm
    }

    /// Rank under the caller's `rel_tol` and the condition estimate (`f64::INFINITY` when
    /// `rank < n` or beyond the `f64` range).
    pub fn diag(&self) -> Factorization {
        self.diag
    }

    /// `R` as an `n x n` matrix with zeros below the diagonal.
    pub fn r(&self) -> Result<Mat, NumError> {
        upper("la.qr_r", &self.packed)
    }

    /// The reflector steps on `c` (length `m`): forward (`Q^T c`) or backward (`Q c`).
    fn reflect_all(&self, op: &'static str, c: &mut [f64], v: &mut Vec<f64>, forward: bool) -> Result<(), NumError> {
        let (m, n) = (self.rows(), self.cols());
        let p = self.packed.data();
        for step in 0..n {
            let k = if forward { step } else { n - 1 - step };
            let t = self.tau[k];
            if t == 0.0 {
                continue;
            }
            v.clear();
            v.push(1.0);
            for i in k + 1..m {
                v.push(p[i * n + k]);
            }
            reflect(op, k, &mut c[k..], v, t)?;
        }
        Ok(())
    }

    fn apply(&self, op: &'static str, x: &[f64], forward: bool) -> Result<Vec<f64>, NumError> {
        let m = self.rows();
        if x.len() != m {
            return Err(LaError::BadShape.at(op));
        }
        check_finite_slice(op, x)?;
        check_scale(op, x)?;
        let mut c = alloc(op, m)?;
        c.extend_from_slice(x);
        let mut v = alloc(op, m)?;
        self.reflect_all(op, &mut c, &mut v, forward)?;
        Ok(c)
    }

    /// `Q^T b` (length `m`) without forming `Q`. Errors: `b.len() != m` (`BadShape`), the
    /// first NaN or infinity of `b`, a subnormal-scale `b`, an overflow at step `k`.
    pub fn apply_qt(&self, b: &[f64]) -> Result<Vec<f64>, NumError> {
        self.apply("la.apply_qt", b, true)
    }

    /// `Q y` (length `m`) without forming `Q`. Errors as [`Qr::apply_qt`].
    pub fn apply_q(&self, y: &[f64]) -> Result<Vec<f64>, NumError> {
        self.apply("la.apply_q", y, false)
    }

    /// The thin `Q` (`m x n`, orthonormal columns): column `j` is `Q e_j`.
    pub fn thin_q(&self) -> Result<Mat, NumError> {
        const OP: &str = "la.thin_q";
        let (m, n) = (self.rows(), self.cols());
        let mut out = alloc_filled(OP, m * n, 0.0)?;
        let mut c = alloc_filled(OP, m, 0.0)?;
        let mut v = alloc(OP, m)?;
        for j in 0..n {
            for (i, ci) in c.iter_mut().enumerate() {
                *ci = if i == j { 1.0 } else { 0.0 };
            }
            self.reflect_all(OP, &mut c, &mut v, false)?;
            for (i, &ci) in c.iter().enumerate() {
                out[i * n + j] = ci;
            }
        }
        Mat::new(m, n, out)
    }
}

/// Least-squares solution of `min ||A x - b||_2` through [`qr_pivoted`] (module docs, "Least
/// squares"). Errors (in this order): `m < n` or `b.len() != m` (`BadShape`), the first NaN
/// or infinity of `b`, `rel_tol`, a subnormal-scale `A`, then `b` (`Subnormal`), an overflow
/// in the factorization, `rank < n` (`RankDeficient { rank }`, decoded by [`LaError::of`]),
/// an overflow in `Q^T b`, the solve or the residual norm, allocation failure.
pub fn lstsq(a: &Mat, b: &[f64], rel_tol: f64) -> Result<LstsqResult, NumError> {
    const OP: &str = "la.lstsq";
    let (m, n) = (a.rows(), a.cols());
    if m < n || b.len() != m {
        return Err(LaError::BadShape.at(OP));
    }
    check_finite_slice(OP, b)?;
    check_tol(OP, rel_tol)?;
    check_scale(OP, a.data())?;
    check_scale(OP, b)?;
    let q = factor(OP, a, true, rel_tol)?;
    if q.diag.rank < n {
        return Err(LaError::RankDeficient { rank: q.diag.rank }.at(OP));
    }
    let mut c = alloc(OP, m)?;
    c.extend_from_slice(b);
    let mut v = alloc(OP, m)?;
    q.reflect_all(OP, &mut c, &mut v, true)?;
    let r = upper(OP, &q.packed)?;
    let y = solve_upper(&r, &c[..n], rel_tol).map_err(|e| match e {
        NumError::Infinite { index: Some(i), .. } => NumError::Infinite { op: OP, index: q.perm.get(i).copied() },
        _ if LaError::of(&e) == Some(LaError::Allocation) => LaError::Allocation.at(OP),
        other => other,
    })?;
    let mut x = alloc_filled(OP, n, 0.0)?;
    for (&p, &yi) in q.perm.iter().zip(&y) {
        x[p] = yi;
    }
    let residual_norm = red::norm2(&c[n..]).map_err(|_| NumError::Infinite { op: OP, index: None })?;
    Ok(LstsqResult { x, residual_norm, diag: q.diag })
}
