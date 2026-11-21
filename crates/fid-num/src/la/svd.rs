//! One-sided (Hestenes) Jacobi singular value decomposition. Spec node `KRN.NUM.LA.SVD`.
//!
//! Every value-affecting choice is pinned here; the Python model in
//! `fixtures/num/la/make_svd_fixtures.py` reproduces it bit for bit, and an independent
//! 400-bit fixed-point oracle there checks the REQ-005 bounds on every fixture matrix.
//!
//! # Setup
//!
//! `A` is `m x n`. With `k = min(m, n)`, the iteration runs on `B = A` (`r x c = m x n`) when
//! `m >= n` and on `B = A^T` (`r x c = n x m`) when `m < n`.
//!
//! 1. **Checks** (op `la.svd`): `rel_tol` (NaN: `NotANumber { index: None }`, infinity:
//!    `Infinite { index: None }`, negative: [`LaError::BadTolerance`]; `-0.0` acts as zero),
//!    then a matrix that is not all zeros but whose largest magnitude is below `2^-1022` is
//!    [`LaError::Subnormal`] (as for QR: every entry has fewer than 53 significant bits).
//!    Entries are finite by construction of [`Mat`], so a NaN or infinity is refused when the
//!    `Mat` is built (`la.mat`), before `svd` can see it.
//! 2. **Scaling:** with `mx` the total-order maximum of `|a_ij|` and `e` its binary exponent
//!    (`2^e <= mx < 2^(e+1)`), every entry is scaled as `(a * d1) * d2` with the power-of-two
//!    factors of `red::norm2` (`d1 * d2 = 2^-e`); for the zero matrix `e = 0` and
//!    `d1 = d2 = 1`. Scaled entries are below 2 in magnitude, so no rotation can overflow.
//!    `B` is copied column by column into a column-major work array `W` (`c` columns of
//!    length `r`); `V` starts as the `c x c` identity, also column-major.
//!
//! # Sweeps (REQ-KRN.NUM.LA.SVD-001, -002)
//!
//! A sweep visits the pairs `(p, q)`, `p < q`, in the order `(0,1), (0,2), .., (0,c-1),
//! (1,2), .., (c-2,c-1)`. For each pair, with `a_p`, `a_q` the current columns of `W`
//! (the test and the angle use scaled quantities only, as LAPACK `dgesvj` does, so columns
//! whose squares underflow are still orthogonalised):
//!
//! 0. **Deflation floor:** before the first sweep, `floor = red::norm2(W) * 2^-600` (all
//!    entries of `W`, column-major order; about `1e-181 ||A||_F` after scaling).
//! 1. `np = red::norm2(a_p)`, then `nq = red::norm2(a_q)`. A column whose norm is nonzero but
//!    at or below `floor` is set to all `+0.0` and its norm to `0`: it is numerically zero (far
//!    below every accuracy bound), and an exactly rank-deficient block otherwise decays
//!    geometrically without its normalised directions ever converging. **Skip** the pair (it
//!    counts as converged) only when `np == 0` or `nq == 0`: a zero column is orthogonal to
//!    everything. Columns down to the floor (for example `1e-170` next to `1`) are still
//!    orthogonalised.
//! 2. `g = red::dot(h_p, h_q)` with the normalised columns `h_p[i] = a_p[i] / np`,
//!    `h_q[i] = a_q[i] / nq` (one division each). **Skip** when `|g| <= c * u`
//!    (`u = 2^-53`): the test `|a_p . a_q| <= n u ||a_p|| ||a_q||`.
//! 3. Otherwise `zeta = ((nq / np) - (np / nq)) / (2 * g)`, which is
//!    `(beta - alpha) / (2 gamma)` without forming any square. If `zeta` is finite and
//!    `|zeta| >= 2^256`, `t = 0.5 / zeta`; if it is finite and smaller,
//!    `t = sg / (|zeta| + sqrt(1 + zeta * zeta))` with `sg = -1` if `zeta < 0`, else `+1`
//!    (so `zeta = +-0` gives `t = 1`). If `zeta` is not finite (a norm ratio overflowed), the
//!    same asymptote without the overflowing ratio: `t = -(g * (nq / np))` if `np >= nq`,
//!    else `t = g * (np / nq)`. Then `cs = 1 / sqrt(1 + t * t)`, `sn = cs * t`
//!    ([`crate::f::sqrt`]).
//! 4. **Rotate** columns `p` and `q` of `W` (all `r` rows), then of `V` (all `c` rows), in
//!    ascending row order: `x_p' = cs * x_p - sn * x_q`, `x_q' = sn * x_p + cs * x_q` (two
//!    products and one addition or subtraction each, no FMA).
//!
//! A sweep in which every pair was skipped is the last one; `sweeps` counts it. Every pair of
//! nonzero columns therefore passed the test of step 2 in the last sweep: a result is never
//! returned before that. If [`MAX_SWEEPS`] sweeps (or the limit passed to
//! [`svd_with_sweep_limit`]) all rotated, the result is [`NumError::NoConvergence`] (decoded
//! by [`LaError::of`] as [`LaError::NoConvergence`]) with `sweeps` the limit, and no factors.
//!
//! # Factors (REQ-KRN.NUM.LA.SVD-003)
//!
//! 1. For each column `j` of `W` (ascending): `nrm_j = red::norm2(a_j)` and
//!    `sigma_j = nrm_j * 2^e` (one product; a non-finite value is `Infinite { index: Some(j) }`).
//! 2. The columns are ordered by `sigma` descending in total order, ties by ascending `j`
//!    (a stable sort).
//! 3. For output position `k` taking column `j`: the left vector is `a_j[i] / nrm_j` (one
//!    division each), or a *zero column* when `nrm_j == 0`; the right vector is column `j` of
//!    `V`. For `m >= n` the left vector is the U column and the right vector the V column; for
//!    `m < n` they swap (`A = V_B S U_B^T`), so the zero columns are in U for a tall matrix and
//!    in V for a wide one.
//! 4. **Sign:** where the V column is not a zero column, its largest-magnitude entry
//!    (scanning ascending, a later entry wins only if strictly larger, so ties go to the
//!    lowest index) must be positive: if it is negative, every entry of the V column and of a
//!    U column that is not a zero column is negated.
//! 5. **Basis completion:** each zero column (its singular value is 0, so it does not change
//!    `U diag(sigma) V^T`) is replaced, in ascending position, by a unit vector orthogonal to
//!    the other columns of its factor. With `L` the column length (`m` for U, `n` for V), the
//!    candidates are `e_0, .., e_(L-1)` in index order. A candidate `x` is reduced by
//!    modified Gram-Schmidt, twice: per pass, for every column of the factor that is not a
//!    zero column or was already completed, in ascending position,
//!    `x[i] = x[i] - red::dot(b, x) * b[i]`. Its residual is `red::norm2(x)`. The first
//!    candidate whose residual is above `0.5` is accepted; if none is, the candidate with the
//!    largest residual (lowest index on ties) is (a positive residual always exists in exact
//!    arithmetic, since fewer than `L` unit vectors cannot span `R^L`). The column is
//!    `x[i] / residual`, then negated if its largest-magnitude entry (lowest index on ties)
//!    is negative. A largest residual of zero is `Domain { detail: "basis_completion" }`.
//!
//! `U` is `m x k`, `V` is `n x k`, both row-major with orthonormal columns; `sigma` has `k`
//! values.
//!
//! # Diagnostics (REQ-KRN.NUM.LA.SVD-004)
//!
//! `rank = rank_by_rule(sigma, rel_tol)` (the `KRN.NUM.LA.MAT` rule). With `smin = sigma[k-1]`,
//! [`Svd::singular`] is `smin == 0`; `cond = sigma[0] / smin` (one division), or
//! `f64::INFINITY` when `smin == 0` or the quotient is beyond the `f64` range.
//!
//! Every buffer is reserved with `try_reserve_exact`; failure is [`LaError::Allocation`].
//! Peak use: `W` (`m n` values), `V` (`k^2`), the column-major factors, `U` and `V` outputs
//! and `O(m + n)` vectors.

use core::cmp::Ordering;

use super::{alloc, alloc_filled, check_tol, rank_by_rule, Factorization, LaError, Mat};
use crate::err::NumError;
use crate::{f, red};

/// The sweep limit of [`svd`] (REQ-KRN.NUM.LA.SVD-002, `[unverified]` in the spec).
pub const MAX_SWEEPS: u16 = 60;

const OP: &str = "la.svd";

/// Deflation floor exponent: columns with norm at or below `2^-600 ||W||_F` are zeroed.
const DEFLATE_EXP: i32 = -600;

/// Unit roundoff `2^-53`.
const U: f64 = 1.0 / 9_007_199_254_740_992.0;

/// A singular value decomposition `A = U diag(sigma) V^T` (thin: `U` is `m x k`, `V` is
/// `n x k`, `k = min(m, n)`), with the pinned sort, sign and completion rules of the module
/// docs.
///
/// The fields are private so that their shapes always agree.
#[derive(Debug, Clone, PartialEq)]
pub struct Svd {
    u: Mat,
    sigma: Vec<f64>,
    v: Mat,
    diag: Factorization,
    sweeps: u16,
}

impl Svd {
    /// The thin left factor (`m x k`, row-major).
    pub fn u(&self) -> &Mat {
        &self.u
    }

    /// The singular values, descending.
    pub fn sigma(&self) -> &[f64] {
        &self.sigma
    }

    /// The right factor (`n x k`, row-major).
    pub fn v(&self) -> &Mat {
        &self.v
    }

    /// Rank under the caller's `rel_tol` and `cond = sigma_1 / sigma_k` (infinite if
    /// [`Svd::singular`] or beyond the `f64` range).
    pub fn diag(&self) -> Factorization {
        self.diag
    }

    /// Number of sweeps run, the last one included (at least 1).
    pub fn sweeps(&self) -> u16 {
        self.sweeps
    }

    /// The `Singular` diagnostic flag: the smallest singular value is exactly zero.
    pub fn singular(&self) -> bool {
        self.sigma.last().is_some_and(|&s| s == 0.0)
    }

    /// `(u, sigma, v)`.
    pub fn into_parts(self) -> (Mat, Vec<f64>, Mat) {
        (self.u, self.sigma, self.v)
    }
}

/// `2^k` for `k` in `[-1022, 1023]` (always a normal number, so exact).
fn pow2(k: i32) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
}

fn overflow(index: Option<usize>) -> NumError {
    NumError::Infinite { op: OP, index }
}

/// A reduction over the scaled work array; it cannot overflow (entries are below 2, or unit
/// vectors), but any error is reported as an overflow rather than ignored.
fn red_err(_: NumError) -> NumError {
    overflow(None)
}

/// Rotates columns `p` and `q` (each `len` long) of the column-major array `x`.
fn rotate(x: &mut [f64], len: usize, p: usize, q: usize, cs: f64, sn: f64) {
    for i in 0..len {
        let xp = x[p * len + i];
        let xq = x[q * len + i];
        x[p * len + i] = cs * xp - sn * xq;
        x[q * len + i] = sn * xp + cs * xq;
    }
}

/// `out = x / nx`, one division per element.
fn normalise_into(out: &mut Vec<f64>, x: &[f64], nx: f64) {
    out.clear();
    out.extend(x.iter().map(|&v| v / nx));
}

/// `red::norm2` of column `j`; a nonzero norm at or below `floor` makes the column an exact
/// zero column (all `+0.0`) and returns `0.0` (deflation, step 1 of the sweep rules).
fn deflated_norm(w: &mut [f64], r: usize, j: usize, floor: f64) -> Result<f64, NumError> {
    let col = &mut w[j * r..(j + 1) * r];
    let nj = red::norm2(col).map_err(red_err)?;
    if nj != 0.0 && nj <= floor {
        col.fill(0.0);
        return Ok(0.0);
    }
    Ok(nj)
}

/// The sweeps of the module docs on `w` (`c` columns of length `r`) and `v` (`c x c`).
fn jacobi(r: usize, c: usize, w: &mut [f64], v: &mut [f64], max_sweeps: u16) -> Result<u16, NumError> {
    // c <= MAX_DIM, so the product is exact.
    let tol = c as f64 * U;
    let z_big = pow2(256);
    let mut hp = alloc(OP, r)?;
    let mut hq = alloc(OP, r)?;
    let floor = red::norm2(w).map_err(red_err)? * pow2(DEFLATE_EXP);
    for sweep in 1..=max_sweeps {
        let mut rotated = false;
        for p in 0..c {
            for q in p + 1..c {
                let np = deflated_norm(w, r, p, floor)?;
                let nq = deflated_norm(w, r, q, floor)?;
                if np == 0.0 || nq == 0.0 {
                    continue;
                }
                let g = {
                    let ap = &w[p * r..(p + 1) * r];
                    let aq = &w[q * r..(q + 1) * r];
                    normalise_into(&mut hp, ap, np);
                    normalise_into(&mut hq, aq, nq);
                    red::dot(&hp, &hq).map_err(red_err)?
                };
                if g.abs() <= tol {
                    continue;
                }
                let zeta = ((nq / np) - (np / nq)) / (2.0 * g);
                let t = if !zeta.is_finite() {
                    if np >= nq {
                        -(g * (nq / np))
                    } else {
                        g * (np / nq)
                    }
                } else if zeta.abs() >= z_big {
                    0.5 / zeta
                } else {
                    let sg = if zeta < 0.0 { -1.0 } else { 1.0 };
                    sg / (zeta.abs() + f::sqrt(1.0 + zeta * zeta)?)
                };
                let cs = 1.0 / f::sqrt(1.0 + t * t)?;
                let sn = cs * t;
                rotate(w, r, p, q, cs, sn);
                rotate(v, c, p, q, cs, sn);
                rotated = true;
            }
        }
        if !rotated {
            return Ok(sweep);
        }
    }
    Err(LaError::NoConvergence { sweeps: usize::from(max_sweeps) }.at(OP))
}

/// Index of the largest-magnitude entry, lowest index on ties.
fn largest(x: &[f64]) -> usize {
    let mut best = 0;
    for (i, v) in x.iter().enumerate() {
        if v.abs() > x[best].abs() {
            best = i;
        }
    }
    best
}

/// Step 5 of the module docs on a column-major factor (`k` columns of length `len`) whose
/// zero columns are flagged in `zero`.
fn complete(f: &mut [f64], len: usize, k: usize, zero: &[bool]) -> Result<(), NumError> {
    let mut done: Vec<bool> = Vec::new();
    done.try_reserve_exact(k).map_err(|_| LaError::Allocation.at(OP))?;
    done.extend(zero.iter().map(|&z| !z));
    let mut x = alloc(OP, len)?;
    let mut best = alloc(OP, len)?;
    for pos in 0..k {
        if done[pos] {
            continue;
        }
        let mut best_res = -1.0_f64;
        let mut accepted = false;
        for cand in 0..len {
            x.clear();
            x.extend((0..len).map(|i| if i == cand { 1.0 } else { 0.0 }));
            for _ in 0..2 {
                for b in 0..k {
                    if !done[b] {
                        continue;
                    }
                    let col = &f[b * len..(b + 1) * len];
                    let d = red::dot(col, &x).map_err(red_err)?;
                    for (xi, &bi) in x.iter_mut().zip(col) {
                        *xi -= d * bi;
                    }
                }
            }
            let res = red::norm2(&x).map_err(red_err)?;
            if res > 0.5 {
                best.clear();
                best.extend_from_slice(&x);
                best_res = res;
                accepted = true;
                break;
            }
            if res.total_cmp(&best_res) == Ordering::Greater {
                best.clear();
                best.extend_from_slice(&x);
                best_res = res;
            }
        }
        if !accepted && best_res <= 0.0 {
            return Err(NumError::Domain { op: OP, detail: "basis_completion" });
        }
        let col = &mut f[pos * len..(pos + 1) * len];
        for (ci, &xi) in col.iter_mut().zip(&best) {
            *ci = xi / best_res;
        }
        let i = largest(col);
        if col[i] < 0.0 {
            for ci in col.iter_mut() {
                *ci = -*ci;
            }
        }
        done[pos] = true;
    }
    Ok(())
}

/// Row-major `len x k` matrix from `k` columns of length `len`.
fn to_rows(f: &[f64], len: usize, k: usize) -> Result<Mat, NumError> {
    let mut d = alloc(OP, len * k)?;
    for i in 0..len {
        for j in 0..k {
            d.push(f[j * len + i]);
        }
    }
    Mat::new(len, k, d)
}

/// Singular value decomposition by one-sided Jacobi with at most [`MAX_SWEEPS`] sweeps
/// (rules in the module docs).
pub fn svd(a: &Mat, rel_tol: f64) -> Result<Svd, NumError> {
    svd_with_sweep_limit(a, rel_tol, MAX_SWEEPS)
}

/// [`svd`] with an explicit sweep limit instead of [`MAX_SWEEPS`] (for tests of the
/// `NoConvergence` path; a limit of 0 always fails). With `max_sweeps = MAX_SWEEPS` it is
/// exactly [`svd`].
pub fn svd_with_sweep_limit(a: &Mat, rel_tol: f64, max_sweeps: u16) -> Result<Svd, NumError> {
    check_tol(OP, rel_tol)?;
    let (m, n) = (a.rows(), a.cols());
    let data = a.data();
    let mut mx = 0.0_f64;
    for &x in data {
        mx = f::total_max(mx, x.abs())?;
    }
    if mx != 0.0 && mx < f64::MIN_POSITIVE {
        return Err(LaError::Subnormal.at(OP));
    }
    let (e, d1, d2) = if mx == 0.0 {
        (0, 1.0, 1.0)
    } else {
        let e = red::exponent(mx);
        let (d1, d2) = red::pow2_scale_factors(e);
        (e, d1, d2)
    };

    let wide = m < n;
    let (r, c) = if wide { (n, m) } else { (m, n) };
    let mut w = alloc(OP, r * c)?;
    for j in 0..c {
        for i in 0..r {
            let x = if wide { data[j * n + i] } else { data[i * n + j] };
            w.push(x * d1 * d2);
        }
    }
    let mut v = alloc_filled(OP, c * c, 0.0)?;
    for k in 0..c {
        v[k * c + k] = 1.0;
    }

    let sweeps = jacobi(r, c, &mut w, &mut v, max_sweeps)?;

    let scale = pow2(e);
    let mut nrm = alloc(OP, c)?;
    let mut sig = alloc(OP, c)?;
    for j in 0..c {
        let x = red::norm2(&w[j * r..(j + 1) * r]).map_err(red_err)?;
        let s = x * scale;
        if !s.is_finite() {
            return Err(overflow(Some(j)));
        }
        nrm.push(x);
        sig.push(s);
    }
    let mut order = Vec::new();
    order.try_reserve_exact(c).map_err(|_| LaError::Allocation.at(OP))?;
    order.extend(0..c);
    // Finite non-negative keys with the index as the tie-break: a total order, so the
    // unstable sort gives the stable result.
    order.sort_unstable_by(|&i, &j| sig[j].total_cmp(&sig[i]).then(i.cmp(&j)));

    let k = c;
    // Column-major factors: left (k columns of length r), right (k columns of length c).
    let mut lf = alloc_filled(OP, r * k, 0.0)?;
    let mut rf = alloc_filled(OP, c * k, 0.0)?;
    let mut zero: Vec<bool> = Vec::new();
    zero.try_reserve_exact(k).map_err(|_| LaError::Allocation.at(OP))?;
    let mut sigma = alloc(OP, k)?;
    for (pos, &j) in order.iter().enumerate() {
        sigma.push(sig[j]);
        let z = nrm[j] == 0.0;
        zero.push(z);
        let lcol = &mut lf[pos * r..(pos + 1) * r];
        if !z {
            for (o, &x) in lcol.iter_mut().zip(&w[j * r..(j + 1) * r]) {
                *o = x / nrm[j];
            }
        }
        rf[pos * c..(pos + 1) * c].copy_from_slice(&v[j * c..(j + 1) * c]);
        // The V column is the right vector (tall) or the left vector (wide, zero if `z`).
        let flip = if wide {
            !z && lcol[largest(lcol)] < 0.0
        } else {
            let rcol = &rf[pos * c..(pos + 1) * c];
            rcol[largest(rcol)] < 0.0
        };
        if flip {
            for x in lf[pos * r..(pos + 1) * r].iter_mut().chain(rf[pos * c..(pos + 1) * c].iter_mut()) {
                *x = -*x;
            }
        }
    }
    drop(w);
    drop(v);
    complete(&mut lf, r, k, &zero)?;
    let (u, v) =
        if wide { (to_rows(&rf, c, k)?, to_rows(&lf, r, k)?) } else { (to_rows(&lf, r, k)?, to_rows(&rf, c, k)?) };

    let rank = rank_by_rule(&sigma, rel_tol)?;
    let smin = sigma[k - 1];
    let cond = if smin == 0.0 {
        f64::INFINITY
    } else {
        let q = sigma[0] / smin;
        if q.is_finite() {
            q
        } else {
            f64::INFINITY
        }
    };
    Ok(Svd { u, sigma, v, diag: Factorization { rank, cond, rel_tol }, sweeps })
}
