//! Symmetric eigendecomposition of the small scatter matrices of the closed-form fits, by
//! cyclic Jacobi.
//!
//! The leaf's Design names `fid_num::la::sym_eig` (spec node `KRN.NUM.LA.EIG`), which is not
//! built yet. This is an interim in-crate implementation of that node's pinned method for
//! `N x N` matrices with `N` in {2, 3}, so the fits can later switch to `sym_eig` without a
//! change of method. It differs from `KRN.NUM.LA.EIG` only where that node leaves a choice
//! open (the skip test, the large-angle tangent and the sign of the vectors), each pinned
//! below.
//!
//! # Pinned rule
//!
//! 1. Only the upper triangle of the input is read; the working matrix `m` is its symmetric
//!    completion. Every entry must be finite ([`FitError::Num`] otherwise).
//! 2. `fro = red::norm2` of the `N * N` entries of `m` in row-major order;
//!    `tol = (N * U) * fro` with `U = 2^-53`.
//! 3. Before each sweep, `off = red::norm2` of the strict upper triangle in row-major order.
//!    The iteration stops when `off <= tol`. After [`EIG_MAX_SWEEPS`] sweeps without that,
//!    the result is [`FitError::NotConverged`] (reason `JacobiSweeps`).
//! 4. A sweep visits the pairs `(p, q)`, `p < q`, in row-major order. A pair with
//!    `m[p][q] == 0` is skipped. Otherwise, with `a = m[p][q]`:
//!    `theta = (m[q][q] - m[p][p]) / (2 a)`;
//!    `t = 0.5 / theta` when `|theta| >= 2^256`, else
//!    `t = sign(theta) / (|theta| + sqrt(theta * theta + 1))` with `sign(-0.0) = sign(+0.0) = +1`;
//!    `c = 1 / sqrt(t * t + 1)`; `s = t * c` (square roots through `fid_num::f::sqrt`).
//!    Then, for every `r` not in `{p, q}` in ascending order, with the old `m[r][p]` and
//!    `m[r][q]`: `m[r][p] = m[p][r] = c m[r][p] - s m[r][q]` and
//!    `m[r][q] = m[q][r] = s m[r][p] + c m[r][q]`; then `m[p][p] = m[p][p] - t a`,
//!    `m[q][q] = m[q][q] + t a`, `m[p][q] = m[q][p] = 0`; then for every row `k` of the
//!    accumulated vectors `V` (starting as the identity), `V[k][p] = c V[k][p] - s V[k][q]` and
//!    `V[k][q] = s V[k][p] + c V[k][q]` with the old values. Every product and sum is one
//!    IEEE-754 operation (no FMA).
//! 5. The eigenvalues are the diagonal of `m`, ordered ascending in total order with ties in
//!    index order; eigenvector `k` is the matching column of `V`, as returned (callers
//!    normalise directions themselves).

use fid_num::{check_finite_slice, f, red};

use crate::contract::{FitError, NotConvergedReason};

/// Largest number of Jacobi sweeps (`KRN.NUM.LA.EIG` REQ-003).
pub const EIG_MAX_SWEEPS: usize = 50;

/// Unit roundoff `2^-53`.
const U: f64 = f64::EPSILON / 2.0;

/// `2^256`: above this `|theta|`, `theta * theta + 1` is replaced by the first-order tangent.
const BIG: f64 = 1.157_920_892_373_162e77;

/// Eigenvalues (ascending) and eigenvectors of a symmetric `N x N` matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SymEig<const N: usize> {
    /// Eigenvalues, ascending in total order (ties in index order).
    pub values: [f64; N],
    /// `vectors[k]` is the eigenvector of `values[k]` (unit length up to rounding).
    pub vectors: [[f64; N]; N],
    /// Sweeps run.
    pub sweeps: usize,
}

/// The decomposition of the module documentation. `N` must be 2 or 3
/// ([`FitError::InvalidParams`] with field `eig.size` otherwise).
pub fn sym_eig<const N: usize>(a: &[[f64; N]; N]) -> Result<SymEig<N>, FitError> {
    if !(2..=3).contains(&N) {
        return Err(FitError::InvalidParams { field: "eig.size" });
    }
    let mut m = [[0.0; N]; N];
    for i in 0..N {
        for j in i..N {
            m[i][j] = a[i][j];
            m[j][i] = a[i][j];
        }
        check_finite_slice("fit.ls_lin.eig", &m[i])?;
    }
    let fro = red::norm2(m.as_flattened())?;
    let tol = (N as f64 * U) * fro;
    let mut v = [[0.0; N]; N];
    for (k, row) in v.iter_mut().enumerate() {
        row[k] = 1.0;
    }
    let mut sweeps = 0usize;
    // The strict upper triangle has N (N - 1) / 2 <= 3 entries.
    let mut upper = [0.0; 3];
    loop {
        let mut len = 0usize;
        for p in 0..N {
            for q in p + 1..N {
                upper[len] = m[p][q];
                len += 1;
            }
        }
        let off = red::norm2(&upper[..len])?;
        if off <= tol {
            break;
        }
        if sweeps == EIG_MAX_SWEEPS {
            return Err(FitError::NotConverged {
                iterations: EIG_MAX_SWEEPS,
                reason: NotConvergedReason::JacobiSweeps,
                last: None,
            });
        }
        for p in 0..N {
            for q in p + 1..N {
                rotate(&mut m, &mut v, p, q)?;
            }
        }
        sweeps += 1;
    }
    let mut order = [0usize; N];
    for (k, o) in order.iter_mut().enumerate() {
        *o = k;
    }
    // Insertion sort by ascending total order; ties keep the lower index (stable).
    for k in 1..N {
        let mut p = k;
        while p > 0 && m[order[p]][order[p]].total_cmp(&m[order[p - 1]][order[p - 1]]) == core::cmp::Ordering::Less {
            order.swap(p, p - 1);
            p -= 1;
        }
    }
    let mut values = [0.0; N];
    let mut vectors = [[0.0; N]; N];
    for (k, &j) in order.iter().enumerate() {
        values[k] = m[j][j];
        for (r, row) in v.iter().enumerate() {
            vectors[k][r] = row[j];
        }
    }
    Ok(SymEig { values, vectors, sweeps })
}

/// One rotation of rule 4 on the pair `(p, q)`.
fn rotate<const N: usize>(m: &mut [[f64; N]; N], v: &mut [[f64; N]; N], p: usize, q: usize) -> Result<(), FitError> {
    let a = m[p][q];
    if a == 0.0 {
        return Ok(());
    }
    let theta = (m[q][q] - m[p][p]) / (2.0 * a);
    let t = if theta.abs() >= BIG {
        0.5 / theta
    } else {
        let mag = 1.0 / (theta.abs() + f::sqrt(theta * theta + 1.0)?);
        if theta < 0.0 {
            -mag
        } else {
            mag
        }
    };
    let c = 1.0 / f::sqrt(t * t + 1.0)?;
    let s = t * c;
    for r in 0..N {
        if r == p || r == q {
            continue;
        }
        let (arp, arq) = (m[r][p], m[r][q]);
        let nrp = c * arp - s * arq;
        let nrq = s * arp + c * arq;
        m[r][p] = nrp;
        m[p][r] = nrp;
        m[r][q] = nrq;
        m[q][r] = nrq;
    }
    m[p][p] -= t * a;
    m[q][q] += t * a;
    m[p][q] = 0.0;
    m[q][p] = 0.0;
    for row in v.iter_mut() {
        let (vp, vq) = (row[p], row[q]);
        row[p] = c * vp - s * vq;
        row[q] = s * vp + c * vq;
    }
    Ok(())
}
