//! One-sided (Hestenes) Jacobi singular value decomposition. Spec node `KRN.NUM.LA.SVD`.
//!
//! Every value-affecting choice is pinned here; the Python model in
//! `fixtures/num/la/make_svd_fixtures.py` reproduces it bit for bit, and an independent
//! 50-digit `decimal` oracle there checks the REQ-005 bounds on every fixture matrix.
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
//!    `d1 = d2 = 1`. Scaled entries are below 2 in magnitude, so no square, dot or rotation can
//!    overflow. `B` is copied column by column into a column-major work array `W` (`c`
//!    columns of length `r`); `V` starts as the `c x c` identity, also column-major.
//!
//! # Sweeps (REQ-KRN.NUM.LA.SVD-001, -002)
//!
//! A sweep visits the pairs `(p, q)`, `p < q`, in the order `(0,1), (0,2), .., (0,c-1),
//! (1,2), .., (c-2,c-1)`. For each pair, with `a_p`, `a_q` the current columns of `W`:
//!
//! 1. `alpha = dot(a_p, a_p)`, `beta = dot(a_q, a_q)`, `gamma = dot(a_p, a_q)`, each one call
//!    of [`crate::red::dot`].
//! 2. **Skip** (the pair counts as converged) when `alpha == 0`, or `beta == 0`, or
//!    `|gamma| <= (c * u) * (sqrt(alpha) * sqrt(beta))` with `u = 2^-53` and
//!    [`crate::f::sqrt`]. This is the test `|a_p . a_q| <= n u ||a_p|| ||a_q||`.
//! 3. Otherwise `zeta = (beta - alpha) / (2 * gamma)`. A non-finite `zeta` (possible only when
//!    `gamma` is subnormal) also skips the pair. If `|zeta| >= 2^256`, `t = 0.5 / zeta`; else
//!    `t = sg / (|zeta| + sqrt(1 + zeta * zeta))` with `sg = -1` if `zeta < 0`, else `+1`
//!    (so `zeta = +-0` gives `t = 1`).
//!    `cs = 1 / sqrt(1 + t * t)`, `sn = cs * t`.
//! 4. **Rotate** columns `p` and `q` of `W` (all `r` rows), then of `V` (all `c` rows), in
//!    ascending row order: `x_p' = cs * x_p - sn * x_q`, `x_q' = sn * x_p + cs * x_q` (two
//!    products and one addition or subtraction each, no FMA).
//!
//! A sweep in which every pair was skipped is the last one; `sweeps` counts it. If
//! [`MAX_SWEEPS`] sweeps (or the limit passed to [`svd_with_sweep_limit`]) all rotated, the
//! result is [`NumError::NoConvergence`] (decoded by [`LaError::of`] as
//! [`LaError::NoConvergence`]) with `sweeps` the limit, and no factors.
//!
//! # Factors (REQ-KRN.NUM.LA.SVD-003)
//!
//! 1. For each column `j` of `W` (ascending): `nrm_j = red::norm2(a_j)` and
//!    `sigma_j = nrm_j * 2^e` (one product; a non-finite value is `Infinite { index: Some(j) }`).
//! 2. The columns are ordered by `sigma` descending in total order, ties by ascending `j`
//!    (a stable sort).
//! 3. For output position `k` taking column `j`: the left vector is `a_j[i] / nrm_j` (one
//!    division each), or all `+0.0` when `nrm_j == 0`; the right vector is column `j` of `V`.
//!    For `m >= n` the left vector is the U column and the right vector the V column; for
//!    `m < n` they swap (`A = V_B S U_B^T`), so a zero singular value of a wide matrix has a
//!    zero V column instead of a zero U column.
//! 4. **Sign:** the V column's largest-magnitude entry (scanning ascending, a later entry wins
//!    only if strictly larger, so ties go to the lowest index) must be positive: if it is
//!    negative, every entry of the V column and of the U column is negated. A zero column
//!    is never negated (it stays `+0.0`).
//!
//! `U` is `m x k`, `V` is `n x k`, both row-major; `sigma` has `k` values.
//!
//! # Diagnostics (REQ-KRN.NUM.LA.SVD-004)
//!
//! `rank = rank_by_rule(sigma, rel_tol)` (the `KRN.NUM.LA.MAT` rule). With `smin = sigma[k-1]`,
//! [`Svd::singular`] is `smin == 0`; `cond = sigma[0] / smin` (one division), or
//! `f64::INFINITY` when `smin == 0` or the quotient is beyond the `f64` range.
//!
//! Every buffer is reserved with `try_reserve_exact`; failure is [`LaError::Allocation`].
//! Peak use: `W` (`m n` values), `V` (`k^2`), `U` and `V` outputs and `O(k)` vectors.

use super::{alloc, alloc_filled, check_tol, rank_by_rule, Factorization, LaError, Mat};
use crate::err::NumError;
use crate::{f, red};

/// The sweep limit of [`svd`] (REQ-KRN.NUM.LA.SVD-002, `[unverified]` in the spec).
pub const MAX_SWEEPS: u16 = 60;

const OP: &str = "la.svd";

/// Unit roundoff `2^-53`.
const U: f64 = 1.0 / 9_007_199_254_740_992.0;

/// A singular value decomposition `A = U diag(sigma) V^T` (thin: `U` is `m x k`, `V` is
/// `n x k`, `k = min(m, n)`), with the pinned sort and sign rules of the module docs.
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

/// A reduction over the scaled work array; it cannot overflow (entries are below 2), but
/// any error is reported as an overflow rather than ignored.
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

/// The sweeps of the module docs on `w` (`c` columns of length `r`) and `v` (`c x c`).
fn jacobi(r: usize, c: usize, w: &mut [f64], v: &mut [f64], max_sweeps: u16) -> Result<u16, NumError> {
    // c <= MAX_DIM, so the product is exact.
    let tol = c as f64 * U;
    let z_big = pow2(256);
    for sweep in 1..=max_sweeps {
        let mut rotated = false;
        for p in 0..c {
            for q in p + 1..c {
                let (alpha, beta, gamma) = {
                    let ap = &w[p * r..(p + 1) * r];
                    let aq = &w[q * r..(q + 1) * r];
                    (
                        red::dot(ap, ap).map_err(red_err)?,
                        red::dot(aq, aq).map_err(red_err)?,
                        red::dot(ap, aq).map_err(red_err)?,
                    )
                };
                if alpha == 0.0 || beta == 0.0 {
                    continue;
                }
                let thr = tol * (f::sqrt(alpha)? * f::sqrt(beta)?);
                if gamma.abs() <= thr {
                    continue;
                }
                let zeta = (beta - alpha) / (2.0 * gamma);
                if !zeta.is_finite() {
                    continue;
                }
                let t = if zeta.abs() >= z_big {
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
    let mut ud = alloc_filled(OP, m * k, 0.0)?;
    let mut vd = alloc_filled(OP, n * k, 0.0)?;
    let mut sigma = alloc(OP, k)?;
    let mut left = alloc(OP, r)?;
    for (pos, &j) in order.iter().enumerate() {
        sigma.push(sig[j]);
        left.clear();
        let zero = nrm[j] == 0.0;
        for i in 0..r {
            left.push(if zero { 0.0 } else { w[j * r + i] / nrm[j] });
        }
        let right = &v[j * c..(j + 1) * c];
        let (ucol, vcol): (&[f64], &[f64]) = if wide { (right, &left) } else { (&left, right) };
        let mut best = 0;
        for (i, x) in vcol.iter().enumerate() {
            if x.abs() > vcol[best].abs() {
                best = i;
            }
        }
        // A zero column (only ever the left vector) is all +0.0 and its V column, if it is
        // the V column, has no negative entry, so it is never negated.
        let flip = vcol[best] < 0.0;
        let (uzero, vzero) = if wide { (false, zero) } else { (zero, false) };
        for (i, &x) in ucol.iter().enumerate() {
            ud[i * k + pos] = if flip && !uzero { -x } else { x };
        }
        for (i, &x) in vcol.iter().enumerate() {
            vd[i * k + pos] = if flip && !vzero { -x } else { x };
        }
    }
    let u = Mat::new(m, k, ud)?;
    let v = Mat::new(n, k, vd)?;

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
