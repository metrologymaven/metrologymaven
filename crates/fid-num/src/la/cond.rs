//! Triangular 2-norm condition estimator (`KRN.NUM.LA.MAT`, REQ-005, REQ-006).
//!
//! Pinned rule (the leaf's Design section; the Python model reproduces it bit for bit):
//!
//! 1. Checks: `R` square (`BadShape`), `rel_tol` (NaN, infinity, negative), then the
//!    singularity scan of the diagonal (`Singular { index }`).
//! 2. Scaling: with `m` the total-order maximum of `|r_ij|` over the upper triangle and `e`
//!    its binary exponent (`2^e <= m < 2^(e+1)`), `S = R * 2^-e` entry by entry, as
//!    `(r * d1) * d2` with the same power-of-two factors as `red::norm2`. The condition
//!    number is invariant under this scaling; it keeps every intermediate in range.
//!    `S^T` is formed explicitly so every triangular row is contiguous.
//! 3. For an operator `P` with adjoint `Q` (first `P = S`, `Q = S^T`; then `P = S^-1`,
//!    `Q = S^-T`), from `x = [1.0; n]`, [`ITERATIONS`] times: `u = x / ||x||`, `y = P u`,
//!    `v = y / ||y||`, `x = Q v`. Then `u = x / ||x||` and the estimate is `||P u||`.
//!    Norms are `red::norm2`; `u = x / ||x||` is one division per element. `S u` is
//!    `dot(S[i, i..n], u[i..n])`, `S^T v` is `dot(S^T[j, 0..=j], v[0..=j])`, `S^-1` is back
//!    substitution on `S` and `S^-T` forward substitution on `S^T` (module `tri`).
//! 4. The result is `est(S) * est(S^-1)`, one product.
//!
//! A zero norm, an overflowing product or solve, or a non-finite or zero result means the
//! condition number is beyond the `f64` range: `Infinite { index: None }`. Never infinity.

use super::tri::{back_sub, fwd_sub};
use super::{check_tol, singular_scan, LaError, Mat};
use crate::err::NumError;
use crate::{f, red};

/// Power iterations per norm estimate (REQ-KRN.NUM.LA.MAT-005).
pub const ITERATIONS: usize = 8;

const OP: &str = "la.cond_triangular";

fn overflow() -> NumError {
    NumError::Infinite { op: OP, index: None }
}

fn normalize(x: &[f64]) -> Result<Vec<f64>, NumError> {
    let nu = red::norm2(x).map_err(|_| overflow())?;
    if nu == 0.0 {
        return Err(overflow());
    }
    Ok(x.iter().map(|&xi| xi / nu).collect())
}

/// `S u` on the upper triangle.
fn upper_mul(n: usize, s: &[f64], u: &[f64]) -> Result<Vec<f64>, NumError> {
    (0..n).map(|i| red::dot(&s[i * n + i..(i + 1) * n], &u[i..]).map_err(|_| overflow())).collect()
}

/// `S^T v` on the lower triangle of the explicit transpose `st`.
fn lower_mul(n: usize, st: &[f64], v: &[f64]) -> Result<Vec<f64>, NumError> {
    (0..n).map(|j| red::dot(&st[j * n..j * n + j + 1], &v[..=j]).map_err(|_| overflow())).collect()
}

fn estimate<P, Q>(n: usize, p: P, q: Q) -> Result<f64, NumError>
where
    P: Fn(&[f64]) -> Result<Vec<f64>, NumError>,
    Q: Fn(&[f64]) -> Result<Vec<f64>, NumError>,
{
    let mut x = vec![1.0_f64; n];
    for _ in 0..ITERATIONS {
        let u = normalize(&x)?;
        let v = normalize(&p(&u)?)?;
        x = q(&v)?;
    }
    let u = normalize(&x)?;
    red::norm2(&p(&u)?).map_err(|_| overflow())
}

/// Estimate of the 2-norm condition number of the upper triangular `r` (only the upper
/// triangle, diagonal included, is read), by the pinned power-method rule above. Returns
/// `Singular { index }` (see [`LaError::of`]) when `r` is rank-deficient under `rel_tol`.
pub fn cond_triangular(r: &Mat, rel_tol: f64) -> Result<f64, NumError> {
    let n = r.rows();
    if r.cols() != n {
        return Err(LaError::BadShape.at(OP));
    }
    check_tol(OP, rel_tol)?;
    let data = r.data();
    singular_scan(OP, n, data, rel_tol)?;

    let mut m = 0.0_f64;
    for i in 0..n {
        for j in i..n {
            m = f::total_max(m, data[i * n + j].abs())?;
        }
    }
    let (d1, d2) = red::pow2_scale_factors(red::exponent(m));
    let mut s = vec![0.0_f64; n * n];
    let mut st = vec![0.0_f64; n * n];
    for i in 0..n {
        for j in i..n {
            let v = data[i * n + j] * d1 * d2;
            s[i * n + j] = v;
            st[j * n + i] = v;
        }
    }

    let norm_s = estimate(n, |u| upper_mul(n, &s, u), |v| lower_mul(n, &st, v))?;
    let norm_inv = estimate(
        n,
        |u| back_sub(n, &s, u).map_err(|_| overflow()),
        |v| fwd_sub(n, &st, v).map_err(|_| overflow()),
    )?;
    let c = norm_s * norm_inv;
    if c.is_finite() && c > 0.0 {
        Ok(c)
    } else {
        Err(overflow())
    }
}
