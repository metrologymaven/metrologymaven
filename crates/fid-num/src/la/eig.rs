//! Cyclic Jacobi symmetric eigendecomposition. Spec node `KRN.NUM.LA.EIG`.
//!
//! Every value-affecting choice is pinned here; the Python model in
//! `fixtures/num/la/make_eig_fixtures.py` reproduces it bit for bit, and an independent
//! 400-bit fixed-point oracle there checks the REQ-006 bounds on every fixture matrix.
//! The same cyclic Jacobi path is used for every size including 3 x 3 (REQ-007).
//!
//! # Setup (REQ-KRN.NUM.LA.EIG-001)
//!
//! `A` is `n x n`.
//!
//! 1. **Checks** (op `la.sym_eig`): non-square input is [`LaError::BadShape`]; `rel_tol`
//!    (NaN: `NotANumber { index: None }`, infinity: `Infinite { index: None }`, negative:
//!    [`LaError::BadTolerance`]; `-0.0` acts as zero); then a matrix that is not all zeros
//!    but whose largest magnitude `mx` (total-order maximum of `|a_ij|`) is below `2^-1022`
//!    is [`LaError::Subnormal`] (as for QR and SVD: every entry has fewer than 53
//!    significant bits). Entries are finite by construction of [`Mat`], so a NaN or infinity
//!    is refused when the `Mat` is built (`la.mat`), before `sym_eig` can see it.
//! 2. **Scaling:** with `e` the binary exponent of `mx` (`2^e <= mx < 2^(e+1)`), every entry
//!    is scaled as `(a * d1) * d2` with the power-of-two factors of `red::norm2`
//!    (`d1 * d2 = 2^-e`); for the zero matrix `e = 0` and `d1 = d2 = 1`. Scaled entries have
//!    magnitude below 2, so differences and rotations cannot overflow.
//! 3. **Symmetry:** with `mx_s = (mx * d1) * d2` and `sym_tol = (n * u) * mx_s` (`u = 2^-53`),
//!    pairs `(i, j)` with `i < j` are scanned in row-major order. If
//!    `|(a_ij * d1 * d2) - (a_ji * d1 * d2)| > sym_tol`, `sym_eig` returns
//!    [`NumError::NotSymmetric`] (decoded by [`LaError::of`] as [`LaError::NotSymmetric`])
//!    naming the first offending `(i, j)`.
//! 4. **Upper triangle only:** the work matrix `W` (`n x n` row-major) is populated from the
//!    scaled upper triangle `i <= j` only, mirrored to the lower triangle (`W[j][i] = W[i][j]`).
//!    `V` starts as the `n x n` identity (column-major).
//!
//! # Sweeps (REQ-KRN.NUM.LA.EIG-002, -003)
//!
//! Before sweep 1:
//! - `fro_a = red::norm2(W)` over all `n^2` entries of the scaled symmetric `W`;
//! - `tol_off = (n * u) * fro_a`, the REQ-003 stopping bound;
//! - `floor = fro_a * 2^-600`, the deflation floor below which an off-diagonal entry is
//!   numerically zero;
//! - `sqrt2 = f::sqrt(2.0)`.
//!
//! The off-diagonal Frobenius norm is `off(W) = red::norm2(upper) * sqrt2`, where `upper`
//! collects the strict upper triangle `W[i][j]` (`i < j`) in row-major order.
//!
//! Each sweep `sweep = 1 ..= max_sweeps` computes the sweep threshold:
//! `thresh = off(W) / (5 * n)` for `sweep <= 3` (using `off(W)` at the start of the sweep),
//! and `0.0` for `sweep > 3`. It then visits every pair `(p, q)`, `p < q`, in row-major order
//! `(0, 1), (0, 2), .., (0, n-1), (1, 2), .., (n-2, n-1)`:
//!
//! 1. **Threshold skip:** let `apq = W[p][q]`, `app = W[p][p]`, `aqq = W[q][q]`. Skip `(p, q)`
//!    when `|apq| == 0` or `|apq| <= floor`, or when `sweep <= 3 && |apq| < thresh`, or when
//!    `sweep > 3 && |apq| <= (0.01 * u) * total_min(|app|, |aqq|)`.
//! 2. **Rotation parameters:** `diff = aqq - app`, `theta = diff / (2 * apq)`. If `theta` is
//!    not finite (tiny `apq` relative to `diff`), `t = apq / diff`; else if `|theta| >= 2^256`,
//!    `t = 0.5 / theta`; else `t = sg / (|theta| + f::sqrt(1 + theta * theta))` with
//!    `sg = -1` if `theta < 0`, else `+1` (so `theta = +-0` gives `t = 1`). Then
//!    `cs = 1 / f::sqrt(1 + t * t)`, `sn = cs * t`, `tau = sn / (1 + cs)` (Rutishauser form).
//! 3. **Update:** `h = t * apq`; `W[p][p] = app - h`, `W[q][q] = aqq + h`,
//!    `W[p][q] = W[q][p] = 0.0`. For `k = 0 .. n` in ascending order (`k != p, q`):
//!    `g = W[k][p]`, `hk = W[k][q]`, `np = g - sn * (hk + tau * g)`,
//!    `nq = hk + sn * (g - tau * hk)`, written symmetrically to `(k, p)/(p, k)` and
//!    `(k, q)/(q, k)`. Then for `i = 0 .. n` in ascending order on columns `p`, `q` of `V`:
//!    `vp' = vp - sn * (vq + tau * vp)`, `vq' = vq + sn * (vp - tau * vq)`. All operations
//!    are plain IEEE-754 operations (no FMA).
//!
//! After the sweep, recompute `off(W)`. Stop after the first sweep with `off(W) <= tol_off`;
//! `sweeps` counts that sweep (at least 1). If `max_sweeps` sweeps (50 by default,
//! [`EIG_MAX_SWEEPS`]) finish without `off(W) <= tol_off`, return [`NumError::NoConvergence`]
//! (decoded by [`LaError::of`] as [`LaError::NoConvergence`]) with `sweeps = max_sweeps` and
//! no factors.
//!
//! # Output, sign and diagnostics (REQ-KRN.NUM.LA.EIG-004, -005)
//!
//! 1. **Unscale:** `raw[j] = W[j][j] * 2^e` for `j = 0 .. n` (one product; a non-finite value
//!    is `Infinite { index: Some(j) }`).
//! 2. **Sort:** ascending in `f64::total_cmp`, ties broken by original diagonal index `j`
//!    (stable).
//! 3. **Sign:** each eigenvector column is signed so that its largest-magnitude entry
//!    (scanning ascending; a later entry wins only if strictly larger, so ties go to the
//!    lowest index) is positive: if negative, every entry of the column is negated.
//! 4. **Diagnostics:** `abs_desc` is `|values|` sorted descending in `f64::total_cmp`.
//!    `rank = rank_by_rule(&abs_desc, rel_tol)`. With `max_abs = abs_desc[0]` and
//!    `min_abs = abs_desc[n-1]`, `cond = max_abs / min_abs` (one division), or
//!    `f64::INFINITY` when `min_abs == 0` or the quotient is not finite.
//!
//! Every buffer is reserved with `try_reserve_exact`; failure is [`LaError::Allocation`].

use super::{alloc, alloc_filled, check_tol, rank_by_rule, Factorization, LaError, Mat};
use crate::err::NumError;
use crate::{f, red};

/// The default sweep limit of [`sym_eig`] (REQ-KRN.NUM.LA.EIG-003, `[unverified]` in the spec).
pub const EIG_MAX_SWEEPS: u16 = 50;

const OP: &str = "la.sym_eig";

/// Deflation floor exponent: off-diagonal entries with `|a_pq| <= 2^-600 ||W||_F` are skipped.
const DEFLATE_EXP: i32 = -600;

/// Unit roundoff `2^-53`.
const U: f64 = 1.0 / 9_007_199_254_740_992.0;

/// Symmetric eigendecomposition `A V = V diag(values)` (`values` ascending in total order,
/// `vectors` an `n x n` row-major matrix with orthonormal eigenvectors as columns).
///
/// Fields are private so their shapes always agree.
#[derive(Debug, Clone, PartialEq)]
pub struct SymEig {
    values: Vec<f64>,
    vectors: Mat,
    diag: Factorization,
    sweeps: u16,
}

impl SymEig {
    /// Eigenvalues, ascending in IEEE-754 total order.
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// Orthonormal eigenvectors as columns (`n x n`, row-major).
    pub fn vectors(&self) -> &Mat {
        &self.vectors
    }

    /// Rank under the caller's `rel_tol` (on `|values|` descending) and
    /// `cond = max|lambda| / min|lambda|` (infinite when `min|lambda| == 0` or beyond `f64`).
    pub fn diag(&self) -> Factorization {
        self.diag
    }

    /// Number of sweeps run, the stopping sweep included (at least 1).
    pub fn sweeps(&self) -> u16 {
        self.sweeps
    }

    /// `(values, vectors)`.
    pub fn into_parts(self) -> (Vec<f64>, Mat) {
        (self.values, self.vectors)
    }
}

/// `2^k` for `k` in `[-1022, 1023]` (always a normal number, so exact).
fn pow2(k: i32) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
}

fn overflow(index: Option<usize>) -> NumError {
    NumError::Infinite { op: OP, index }
}

/// A reduction over the scaled work array; entries are bounded, but any error is mapped cleanly.
fn red_err(_: NumError) -> NumError {
    overflow(None)
}

/// Off-diagonal Frobenius norm of the symmetric `n x n` matrix `w`, computed via `red::norm2`
/// over the strict upper triangle in row-major order, scaled by `sqrt(2)`.
fn off_fro(w: &[f64], n: usize, upper: &mut Vec<f64>, sqrt2: f64) -> Result<f64, NumError> {
    upper.clear();
    for i in 0..n {
        let row = i * n;
        for j in (i + 1)..n {
            upper.push(w[row + j]);
        }
    }
    if upper.is_empty() {
        return Ok(0.0);
    }
    let u_norm = red::norm2(upper).map_err(red_err)?;
    let off = u_norm * sqrt2;
    if !off.is_finite() {
        return Err(overflow(None));
    }
    Ok(off)
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

/// Cyclic Jacobi sweeps on scaled symmetric `w` (`n x n` row-major) and `v` (`n x n`
/// column-major, initialized to identity).
fn jacobi(n: usize, w: &mut [f64], v: &mut [f64], max_sweeps: u16) -> Result<u16, NumError> {
    let fro_a = red::norm2(w).map_err(red_err)?;
    let tol_off = (n as f64 * U) * fro_a;
    let floor = fro_a * pow2(DEFLATE_EXP);
    let z_big = pow2(256);
    let small_ratio = 0.01 * U;
    let sqrt2 = f::sqrt(2.0)?;
    let upper_len = n.saturating_sub(1) * n / 2;
    let mut upper = alloc(OP, upper_len)?;
    let mut cur_off = off_fro(w, n, &mut upper, sqrt2)?;

    for sweep in 1..=max_sweeps {
        let thresh = if sweep <= 3 { cur_off / (5.0 * n as f64) } else { 0.0 };
        for p in 0..n {
            for q in (p + 1)..n {
                let apq = w[p * n + q];
                let apq_abs = apq.abs();
                if apq_abs == 0.0 || apq_abs <= floor {
                    continue;
                }
                if sweep <= 3 && apq_abs < thresh {
                    continue;
                }
                let app = w[p * n + p];
                let aqq = w[q * n + q];
                if sweep > 3 {
                    let min_diag = f::total_min(app.abs(), aqq.abs())?;
                    if apq_abs <= small_ratio * min_diag {
                        continue;
                    }
                }
                let diff = aqq - app;
                let theta = diff / (2.0 * apq);
                let t = if !theta.is_finite() {
                    apq / diff
                } else if theta.abs() >= z_big {
                    0.5 / theta
                } else {
                    let sg = if theta < 0.0 { -1.0 } else { 1.0 };
                    sg / (theta.abs() + f::sqrt(1.0 + theta * theta)?)
                };
                let cs = 1.0 / f::sqrt(1.0 + t * t)?;
                let sn = cs * t;
                let tau = sn / (1.0 + cs);

                let h = t * apq;
                w[p * n + p] = app - h;
                w[q * n + q] = aqq + h;
                w[p * n + q] = 0.0;
                w[q * n + p] = 0.0;

                for k in 0..n {
                    if k == p || k == q {
                        continue;
                    }
                    let g = w[k * n + p];
                    let hk = w[k * n + q];
                    let np = g - sn * (hk + tau * g);
                    let nq = hk + sn * (g - tau * hk);
                    w[k * n + p] = np;
                    w[p * n + k] = np;
                    w[k * n + q] = nq;
                    w[q * n + k] = nq;
                }

                for i in 0..n {
                    let vp = v[p * n + i];
                    let vq = v[q * n + i];
                    v[p * n + i] = vp - sn * (vq + tau * vp);
                    v[q * n + i] = vq + sn * (vp - tau * vq);
                }
            }
        }
        cur_off = off_fro(w, n, &mut upper, sqrt2)?;
        if cur_off <= tol_off {
            return Ok(sweep);
        }
    }
    Err(LaError::NoConvergence { sweeps: usize::from(max_sweeps) }.at(OP))
}

/// Symmetric eigendecomposition by cyclic Jacobi with at most [`EIG_MAX_SWEEPS`] sweeps
/// (rules in the module docs).
pub fn sym_eig(a: &Mat, rel_tol: f64) -> Result<SymEig, NumError> {
    sym_eig_with_sweep_limit(a, rel_tol, EIG_MAX_SWEEPS)
}

/// [`sym_eig`] with an explicit sweep limit instead of [`EIG_MAX_SWEEPS`] (for tests of the
/// `NoConvergence` path; a limit of 0 always fails). With `max_sweeps = EIG_MAX_SWEEPS` it is
/// identical to [`sym_eig`].
pub fn sym_eig_with_sweep_limit(a: &Mat, rel_tol: f64, max_sweeps: u16) -> Result<SymEig, NumError> {
    let n = a.rows();
    if a.cols() != n {
        return Err(LaError::BadShape.at(OP));
    }
    check_tol(OP, rel_tol)?;

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

    let mx_s = mx * d1 * d2;
    let sym_tol = (n as f64 * U) * mx_s;
    for i in 0..n {
        for j in (i + 1)..n {
            let sij = data[i * n + j] * d1 * d2;
            let sji = data[j * n + i] * d1 * d2;
            if (sij - sji).abs() > sym_tol {
                return Err(LaError::NotSymmetric { i, j }.at(OP));
            }
        }
    }

    // Populate W from the scaled upper triangle only (REQ-001).
    let mut w = alloc_filled(OP, n * n, 0.0)?;
    for i in 0..n {
        w[i * n + i] = data[i * n + i] * d1 * d2;
        for j in (i + 1)..n {
            let val = data[i * n + j] * d1 * d2;
            w[i * n + j] = val;
            w[j * n + i] = val;
        }
    }

    let mut v = alloc_filled(OP, n * n, 0.0)?;
    for k in 0..n {
        v[k * n + k] = 1.0;
    }

    let sweeps = jacobi(n, &mut w, &mut v, max_sweeps)?;

    let scale = pow2(e);
    let mut raw = alloc(OP, n)?;
    for j in 0..n {
        let val = w[j * n + j] * scale;
        if !val.is_finite() {
            return Err(overflow(Some(j)));
        }
        raw.push(val);
    }
    drop(w);

    let mut order: Vec<usize> = Vec::new();
    order.try_reserve_exact(n).map_err(|_| LaError::Allocation.at(OP))?;
    order.extend(0..n);
    // Ascending IEEE-754 total order, ties kept in original diagonal order (REQ-004).
    order.sort_unstable_by(|&i, &j| raw[i].total_cmp(&raw[j]).then(i.cmp(&j)));

    let mut values = alloc(OP, n)?;
    let mut vcols = alloc_filled(OP, n * n, 0.0)?;
    for (pos, &j) in order.iter().enumerate() {
        values.push(raw[j]);
        let dst = &mut vcols[pos * n..(pos + 1) * n];
        dst.copy_from_slice(&v[j * n..(j + 1) * n]);
        let best = largest(dst);
        if dst[best] < 0.0 {
            for x in dst.iter_mut() {
                *x = -*x;
            }
        }
    }
    drop(v);

    // Pack column-major eigenvectors into row-major Mat (columns are eigenvectors).
    let mut vrows = alloc(OP, n * n)?;
    for i in 0..n {
        for j in 0..n {
            vrows.push(vcols[j * n + i]);
        }
    }
    let vectors = Mat::new(n, n, vrows)?;

    // Diagnostics: rank on |lambda| sorted descending, cond = max|lambda| / min|lambda| (REQ-005).
    let mut abs_desc = alloc(OP, n)?;
    for &x in &values {
        abs_desc.push(x.abs());
    }
    abs_desc.sort_unstable_by(|a, b| b.total_cmp(a));
    let rank = rank_by_rule(&abs_desc, rel_tol)?;
    let min_abs = abs_desc[n - 1];
    let cond = if min_abs == 0.0 {
        f64::INFINITY
    } else {
        let q = abs_desc[0] / min_abs;
        if q.is_finite() {
            q
        } else {
            f64::INFINITY
        }
    };

    Ok(SymEig { values, vectors, diag: Factorization { rank, cond, rel_tol }, sweeps })
}
