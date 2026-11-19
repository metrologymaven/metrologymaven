//! Column-pivoted Householder QR applied to one right-hand side (`KRN.SOLV.NLLS` Design,
//! "Step solve"). It follows the rules pinned for `KRN.NUM.LA.QR` (reflector sign, pivot
//! choice, norms recomputed at every step, inner products through `fid_num::red`) so that
//! the driver can move onto `fid_num::la::qr` once that leaf is built.

use fid_num::la::{cond_triangular, solve_upper, LaError, Mat};
use fid_num::{red, NumError};

/// A factored system: `R` (upper triangular, `n x n`), the column permutation and the first
/// `n` entries of `Q^T b`.
pub(crate) struct Factored {
    pub r: Mat,
    pub perm: Vec<usize>,
    pub qtb: Vec<f64>,
}

/// Pivoted Householder QR of the column-major `cols` (`n` columns of length `rows`, `rows >=
/// n`) applied to `b` (length `rows`). `cols` and `b` are overwritten. `v` is scratch with
/// capacity of at least `rows` (no reallocation).
///
/// For `k = 0..n`: the 2-norms of the trailing parts `cols[j][k..]` (`j >= k`) are recomputed
/// with `red::norm2`, and the largest (strictly greater, so ties go to the lowest index) is
/// swapped into column `k` (whole columns). With `alpha = cols[k][k]` and `xnorm` its norm,
/// `beta = -xnorm` if `alpha > 0`, else `xnorm`; `u0 = alpha - beta`; `v = [1, x[1..] / u0]`;
/// `tau = (beta - alpha) / beta`. Column `k` becomes `beta` on the diagonal and zeros below;
/// each later column `c` (ascending) and then `b` get `s = tau * red::dot(v, c[k..])` and
/// `c[i] = c[i] - s * v[i - k]`. A zero `xnorm` leaves the column as it is (a zero diagonal).
pub(crate) fn factor(
    cols: &mut [f64],
    rows: usize,
    n: usize,
    b: &mut [f64],
    v: &mut Vec<f64>,
) -> Result<Factored, NumError> {
    let mut perm = super::alloc_usize(n)?;
    for k in 0..n {
        let mut best = k;
        let mut best_norm = red::norm2(&cols[k * rows + k..(k + 1) * rows])?;
        for j in k + 1..n {
            let c = red::norm2(&cols[j * rows + k..(j + 1) * rows])?;
            if c > best_norm {
                best = j;
                best_norm = c;
            }
        }
        if best != k {
            for i in 0..rows {
                cols.swap(k * rows + i, best * rows + i);
            }
            perm.swap(k, best);
        }
        if best_norm == 0.0 {
            continue;
        }
        let alpha = cols[k * rows + k];
        let beta = if alpha > 0.0 { -best_norm } else { best_norm };
        let u0 = alpha - beta;
        v.clear();
        v.push(1.0);
        for i in k + 1..rows {
            v.push(cols[k * rows + i] / u0);
        }
        let tau = (beta - alpha) / beta;
        cols[k * rows + k] = beta;
        for x in &mut cols[k * rows + k + 1..(k + 1) * rows] {
            *x = 0.0;
        }
        for j in k + 1..n {
            reflect(&mut cols[j * rows + k..(j + 1) * rows], v, tau)?;
        }
        reflect(&mut b[k..rows], v, tau)?;
    }
    let mut data = super::alloc(n * n)?;
    for i in 0..n {
        for j in 0..n {
            data.push(if j >= i { cols[j * rows + i] } else { 0.0 });
        }
    }
    let r = Mat::new(n, n, data)?;
    let mut qtb = super::alloc(n)?;
    qtb.extend_from_slice(&b[..n]);
    Ok(Factored { r, perm, qtb })
}

fn reflect(c: &mut [f64], v: &[f64], tau: f64) -> Result<(), NumError> {
    let s = tau * red::dot(v, c)?;
    for (ci, vi) in c.iter_mut().zip(v) {
        *ci -= s * *vi;
    }
    Ok(())
}

/// Solves `R y = Q^T b` by `fid_num::la::solve_upper` (relative tolerance 0: only an exact
/// zero diagonal is singular) and un-permutes: `z[perm[i]] = y[i]`.
pub(crate) fn solve(f: &Factored, z: &mut [f64]) -> Result<(), NumError> {
    let y = solve_upper(&f.r, &f.qtb, 0.0)?;
    for (i, &yi) in y.iter().enumerate() {
        z[f.perm[i]] = yi;
    }
    Ok(())
}

/// The numerical rank: the first `k` (ascending) with `|r_kk| <= rank_tol * max_j |r_jj|`,
/// or `n` if there is none.
pub(crate) fn rank(f: &Factored, rank_tol: f64) -> usize {
    let n = f.r.rows();
    let d = f.r.data();
    let mut mx = 0.0_f64;
    for k in 0..n {
        let a = d[k * n + k].abs();
        if a > mx {
            mx = a;
        }
    }
    let thr = rank_tol * mx;
    (0..n).find(|&k| d[k * n + k].abs() <= thr).unwrap_or(n)
}

/// The condition estimate of `R` by `fid_num::la::cond_triangular` with relative tolerance 0:
/// `None` when it is not estimable (an exact zero diagonal or an estimate beyond `f64`).
/// Allocation failure is an error.
pub(crate) fn cond(f: &Factored) -> Result<Option<f64>, NumError> {
    match cond_triangular(&f.r, 0.0) {
        Ok(c) => Ok(Some(c)),
        Err(e) if LaError::of(&e) == Some(LaError::Allocation) => Err(e),
        Err(_) => Ok(None),
    }
}
