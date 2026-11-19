//! Correlation matrices and the multivariate normal (REQ-UNC.MODEL.DIST-001, -005).
//!
//! # Correlation check (pinned order; first failure wins)
//!
//! For an `n x n` row-major matrix `C`:
//!
//! 1. `n == 0`, `n > fid_num::la::MAX_DIM` or `data.len() != n * n`: reason `shape`.
//! 2. The first NaN or infinity (row-major): reason `non_finite`.
//! 3. `C[i][j] != C[j][i]` for some `i < j` (exact comparison): `asymmetric`.
//! 4. `C[i][i] != 1.0`: `diagonal`.
//! 5. `|C[i][j]| > 1`: `off_diagonal_range`.
//! 6. Eigenvalue test. `tau = 1e-12 * n` (one rounded product). `B` is `C` with every
//!    diagonal entry replaced by `1.0 + tau` (rounded). `min eig(C) >= -tau` exactly when `B`
//!    is positive semi-definite, which is decided by `fid_num::la::chol_pivoted(B, 0.0)`: if
//!    its rank is `n`, `C` passes. Otherwise, with `r` the rank, `L` the factor and `perm`
//!    its permutation, the trailing Schur complement
//!    `S[i][j] = B[perm[i]][perm[j]] - red::dot(L[i][0..r], L[j][0..r])` is formed for
//!    `r <= j <= i < n` in row-major order, and `C` fails with `indefinite` if any
//!    `|S[i][j]| > eps`, with `eps = n * 2^-50` ([`CORR_EPS_FACTOR`] times `n`). (Every
//!    diagonal of `S` is `<= 0` at the stop, so an `S` within `eps` of zero has no
//!    eigenvalue below about `-n * eps`; an `S` with an off-diagonal entry above `eps` and
//!    diagonal entries at most `0 < eps` has a negative 2 x 2 minor.) The decision is
//!    exact up to rounding of order `n * 2^-52`, far below `tau`.
//!
//! # Multivariate normal
//!
//! `MvNormal(mu, sigma, R)`: component `i` has mean `mu[i]`, standard deviation
//! `sigma[i] > 0` (same unit as `mu[i]`) and the validated correlation `R`; the covariance is
//! `Sigma[i][j] = (sigma[i] * sigma[j]) * R[i][j]`. The sampling factor is
//! `chol_pivoted(R, n * 2^-52)` ([`MV_REL_TOL_FACTOR`] times `n`) with rank `r`, so a
//! singular or barely indefinite `R` gives a rank-`r` factor. Sample `i` draws standard
//! normals `z[0..r]` from the multivariate sub-stream (see the `sample` module) and sets,
//! for each factor row `p` in ascending order,
//! `x[perm[p]] = mu[perm[p]] + sigma[perm[p]] * red::dot(L[p][0..r], z[0..r])`.

use super::sample::{self, MV_SHIFT};
use super::{DistError, Kind, Quantity, UnitCode};
use core::ops::Range;
use fid_num::la::{chol_pivoted, CholPivoted, Mat, MAX_DIM};
use fid_num::red;

/// `eps = CORR_EPS_FACTOR * n` in the indefiniteness test: `2^-50`.
pub const CORR_EPS_FACTOR: f64 = 1.0 / 1_125_899_906_842_624.0;
/// Relative stop tolerance of the sampling factor per dimension: `2^-52`.
pub const MV_REL_TOL_FACTOR: f64 = f64::EPSILON;

fn bad(reason: &'static str) -> DistError {
    DistError::InvalidCorrelation { reason }
}

/// Validates a correlation matrix (REQ-UNC.MODEL.DIST-005); see the module docs.
pub fn validate_correlation(n: usize, data: &[f64]) -> Result<(), DistError> {
    if n == 0 || n > MAX_DIM || data.len() != n * n {
        return Err(bad("shape"));
    }
    if data.iter().any(|x| !x.is_finite()) {
        return Err(bad("non_finite"));
    }
    for i in 0..n {
        for j in i + 1..n {
            if data[i * n + j] != data[j * n + i] {
                return Err(bad("asymmetric"));
            }
        }
    }
    if (0..n).any(|i| data[i * n + i] != 1.0) {
        return Err(bad("diagonal"));
    }
    if data.iter().any(|x| x.abs() > 1.0) {
        return Err(bad("off_diagonal_range"));
    }
    let tau = 1e-12 * n as f64;
    let mut b = Vec::new();
    b.try_reserve_exact(n * n).map_err(|_| DistError::Allocation)?;
    b.extend_from_slice(data);
    for i in 0..n {
        b[i * n + i] = 1.0 + tau;
    }
    let bm = Mat::new(n, n, b)?;
    let ch = chol_pivoted(&bm, 0.0)?;
    let r = ch.rank();
    if r == n {
        return Ok(());
    }
    let eps = CORR_EPS_FACTOR * n as f64;
    let l = ch.l().data();
    let perm = ch.perm();
    let bd = bm.data();
    for (i, &pi) in perm.iter().enumerate().skip(r) {
        for (j, &pj) in perm.iter().enumerate().take(i + 1).skip(r) {
            let s = bd[pi * n + pj] - red::dot(&l[i * n..i * n + r], &l[j * n..j * n + r])?;
            if s.abs() > eps {
                return Err(bad("indefinite"));
            }
        }
    }
    Ok(())
}

/// A validated correlation matrix.
#[derive(Debug, Clone, PartialEq)]
pub struct Correlation {
    n: usize,
    data: Vec<f64>,
}

impl Correlation {
    /// Validates and wraps an `n x n` row-major matrix.
    pub fn new(n: usize, data: Vec<f64>) -> Result<Correlation, DistError> {
        validate_correlation(n, &data)?;
        Ok(Correlation { n, data })
    }

    /// The dimension `n`.
    pub fn dim(&self) -> usize {
        self.n
    }

    /// The row-major entries.
    pub fn data(&self) -> &[f64] {
        &self.data
    }
}

/// A validated multivariate normal (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct MvNormal {
    mu: Vec<f64>,
    sigma: Vec<f64>,
    units: Vec<UnitCode>,
    corr: Correlation,
    factor: CholPivoted,
}

fn bad_param(name: &'static str) -> DistError {
    DistError::InvalidParameter { name }
}

impl MvNormal {
    /// multivariate-normal(mu, sigma, R). Check order: `mu` empty (`mu`), `sigma` length
    /// (`sigma`), `R` dimension (`corr`); then per component in index order `mu[i]` finite
    /// (`mu`), `sigma[i]` finite with the unit of `mu[i]` (`sigma`), `sigma[i] > 0`
    /// (`sigma`).
    pub fn new(mu: &[Quantity], sigma: &[Quantity], corr: Correlation) -> Result<MvNormal, DistError> {
        let n = mu.len();
        if n == 0 {
            return Err(bad_param("mu"));
        }
        if sigma.len() != n {
            return Err(bad_param("sigma"));
        }
        if corr.dim() != n {
            return Err(bad_param("corr"));
        }
        let mut m = Vec::new();
        let mut s = Vec::new();
        let mut u = Vec::new();
        m.try_reserve_exact(n).map_err(|_| DistError::Allocation)?;
        s.try_reserve_exact(n).map_err(|_| DistError::Allocation)?;
        u.try_reserve_exact(n).map_err(|_| DistError::Allocation)?;
        for (q, sq) in mu.iter().zip(sigma) {
            if !q.value.is_finite() {
                return Err(bad_param("mu"));
            }
            if !sq.value.is_finite() || sq.unit != q.unit || sq.value <= 0.0 {
                return Err(bad_param("sigma"));
            }
            m.push(q.value);
            s.push(sq.value);
            u.push(q.unit);
        }
        let cm = Mat::new(n, n, corr.data().to_vec())?;
        let factor = chol_pivoted(&cm, MV_REL_TOL_FACTOR * n as f64)?;
        Ok(MvNormal { mu: m, sigma: s, units: u, corr, factor })
    }

    /// The kind ([`Kind::MvNormal`]).
    pub fn kind(&self) -> Kind {
        Kind::MvNormal
    }

    /// The dimension.
    pub fn dim(&self) -> usize {
        self.mu.len()
    }

    /// The expectation vector `mu`.
    pub fn expectation(&self) -> &[f64] {
        &self.mu
    }

    /// The standard uncertainties `sigma`.
    pub fn std_uncertainty(&self) -> &[f64] {
        &self.sigma
    }

    /// The unit of each component.
    pub fn units(&self) -> &[UnitCode] {
        &self.units
    }

    /// The correlation matrix.
    pub fn correlation(&self) -> &Correlation {
        &self.corr
    }

    /// The rank of the sampling factor.
    pub fn rank(&self) -> usize {
        self.factor.rank()
    }

    /// The covariance `(sigma[i] * sigma[j]) * R[i][j]`, row-major.
    pub fn covariance(&self) -> Result<Vec<f64>, DistError> {
        let n = self.dim();
        let mut out = Vec::new();
        out.try_reserve_exact(n * n).map_err(|_| DistError::Allocation)?;
        for (si, row) in self.sigma.iter().zip(self.corr.data().chunks_exact(n)) {
            for (sj, rho) in self.sigma.iter().zip(row) {
                out.push((si * sj) * rho);
            }
        }
        Ok(out)
    }

    fn draw_into(&self, seed: u64, stream: u64, index: u64, z: &mut [f64], out: &mut [f64]) -> Result<(), DistError> {
        let n = self.dim();
        let r = self.factor.rank();
        for (k, zk) in z.iter_mut().enumerate().take(r) {
            *zk = sample::mv_normal(seed, stream, index, k)?;
        }
        let l = self.factor.l().data();
        let perm = self.factor.perm();
        for (p, &o) in perm.iter().enumerate() {
            let y = red::dot(&l[p * n..p * n + r], &z[..r])?;
            out[o] = fid_num::check_finite("unc.dist.sample", self.mu[o] + self.sigma[o] * y)?;
        }
        Ok(())
    }

    /// Samples `range` of `stream` under `seed`: `range.len() * dim` values, row-major
    /// (sample by sample). Each sample depends on `(seed, stream, index)` only.
    pub fn sample(&self, seed: u64, stream: u64, range: Range<u64>) -> Result<Vec<f64>, DistError> {
        if range.end < range.start {
            return Err(bad_param("index"));
        }
        let last = if range.end > range.start { range.end - 1 } else { range.start };
        sample::check_address(stream, last, Kind::MvNormal)?;
        if self.dim() > 1 << MV_SHIFT {
            return Err(bad_param("mu"));
        }
        let n = self.dim();
        let count = usize::try_from(range.end - range.start).map_err(|_| DistError::Allocation)?;
        let total = count.checked_mul(n).ok_or(DistError::Allocation)?;
        let mut out = Vec::new();
        out.try_reserve_exact(total).map_err(|_| DistError::Allocation)?;
        out.resize(total, 0.0);
        let mut z = Vec::new();
        z.try_reserve_exact(n).map_err(|_| DistError::Allocation)?;
        z.resize(n, 0.0);
        for (row, i) in range.enumerate() {
            self.draw_into(seed, stream, i, &mut z, &mut out[row * n..(row + 1) * n])?;
        }
        Ok(out)
    }
}
