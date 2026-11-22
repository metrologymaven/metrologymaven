//! Degeneracy checks: point count, numerical rank (collinear, coplanar) and angular coverage
//! (REQ-KRN.LSQ.TRAIT-004).
//!
//! # Pinned rules
//!
//! All checks run on the centred, scaled data of [`Centred`] (RMS distance in `[1, 2)`).
//!
//! 1. **Count.** `n < min_points` of the primitive ([`Primitive::requirement`]) is
//!    [`FitError::TooFewPoints`]. Checked before anything else.
//! 2. **Triangular factor (blocked QR).** The `n x K` data matrix is reduced to a `K x K`
//!    upper-triangular `R` with `R^T R = A^T A` (never forming `A^T A`) by folding
//!    `fid_num::la::qr` (Householder, no pivoting, `rel_tol = 0`) over row blocks: the first
//!    block is rows `0 .. min(n, MAX_DIM)`; each later block is the previous `R` (`K` rows)
//!    followed by the next `min(rest, MAX_DIM - K)` data rows in point order; a block with
//!    fewer than `K` rows is padded with zero rows; the result is `Qr::r()` of the last block.
//! 3. **Singular values.** One-sided Jacobi on the columns of the `3 x 3` factor of the
//!    centred data: sweeps over the pairs `(0,1), (0,2), (1,2)`; for a pair, `a = |c_i|^2`,
//!    `b = |c_j|^2`, `g = c_i . c_j` (each a `red::dot`); the pair is skipped when `g == 0` or
//!    `|g| <= JACOBI_TOL * (red::norm2(c_i) * red::norm2(c_j))`, or when
//!    `zeta = (b - a) / (2 g)` is not finite (a rotation angle below `2^-1024`); otherwise
//!    `t = sign(zeta) / (|zeta| + hypot(1, zeta))` (`sign(0) = +1`), `c = 1 / hypot(1, t)`,
//!    `s = c t`, and columns `c_i, c_j` (and the same columns of `V`, which starts as the
//!    identity) become `c c_i - s c_j` and `s c_i + c c_j`, component by component. A
//!    rotation that leaves both data columns bit-identical is not applied and does not count.
//!    A sweep without a counted rotation ends the iteration; [`JACOBI_MAX_SWEEPS`] sweeps without that is
//!    [`FitError::NotConverged`]. The singular values are `red::norm2` of the final columns,
//!    ordered by descending total order (ties keep the lower column index); the principal
//!    axes are the matching columns of `V` through
//!    [`super::primitive::normalize_direction`].
//! 4. **Rank and condition.** `rank = fid_num::la::rank_by_rule(s, rank_tol)` (count of
//!    `s_i > rank_tol * s_0`). With `k` the rank the primitive needs, `cond = s_0 / s_(k-1)`
//!    (`+inf` when that value is zero or `s_0` is zero; `1` when `k == 0`). `rank < k` is
//!    [`FitError::Collinear`] when `rank <= 1` and [`FitError::Coplanar`] when `rank == 2`.
//! 5. **Angular coverage** (circle-type primitives). Each point is projected on the two
//!    leading principal axes, `u_i = red::dot(q_i, e_0)`, `v_i = red::dot(q_i, e_1)`. An
//!    algebraic circle `u^2 + v^2 + D u + E v + F = 0` is fitted by least squares: the blocked
//!    QR of rows `[u_i, v_i, 1, -(u_i u_i + v_i v_i)]` gives a `4 x 4` factor whose leading
//!    `3 x 3` block and first three entries of column 3 go to `fid_num::la::lstsq` with
//!    `rel_tol = rank_tol`. The centre is `(-D / 2, -E / 2)`. The angles
//!    `f::atan2(v_i - v_c, u_i - u_c)` are sorted in total order; the largest gap is the
//!    total-order maximum of `(theta_0 + 2 pi) - theta_(n-1)` and the consecutive differences;
//!    `coverage = max(0, 2 pi - gap)` in total order. `coverage < min_coverage` is
//!    [`FitError::InsufficientCoverage`].
//!
//! Every buffer is reserved fallibly; every loop is bounded by `n <= MAX_POINTS` and
//! [`JACOBI_MAX_SWEEPS`].

use fid_num::la::{lstsq, qr, rank_by_rule, Mat, MAX_DIM};
use fid_num::{f, red};

use super::error::{alloc, FitError, NotConvergedReason};
use super::normalise::Centred;
use super::params::{Thresholds, TWO_PI};
use super::points::PointsView;
use super::primitive::{normalize_direction, Primitive, V3};

const OP: &str = "fit.degeneracy";

/// Largest number of Jacobi sweeps.
pub const JACOBI_MAX_SWEEPS: usize = 32;

/// Relative orthogonality tolerance of the Jacobi iteration (`2^-52`).
pub const JACOBI_TOL: f64 = f64::EPSILON;

/// Singular values and principal axes of centred, scaled data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DataShape {
    /// Singular values of the centred, scaled `n x 3` data matrix, descending.
    pub singular_values: [f64; 3],
    /// The matching right singular vectors (unit, sign-normalised).
    pub axes: [V3; 3],
}

/// What the degeneracy checks measured on data that passed them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Degeneracy {
    /// Number of points.
    pub n: usize,
    /// Numerical rank of the centred data under `Thresholds::rank_tol`.
    pub rank: usize,
    /// `s_0 / s_(k-1)` for the rank `k` the primitive needs (1 when `k == 0`).
    pub cond: f64,
    pub shape: DataShape,
    /// Angular coverage in radians, for circle-type primitives.
    pub coverage: Option<f64>,
}

/// The blocked QR fold of rule 2 over rows produced by `row(i, out)`. Also used by `init`
/// (KRN.LSQ.INIT) for its candidate circle fits.
pub(crate) fn tsqr<const K: usize, F>(n: usize, mut row: F) -> Result<[[f64; K]; K], FitError>
where
    F: FnMut(usize, &mut [f64; K]) -> Result<(), FitError>,
{
    let mut r: Option<[[f64; K]; K]> = None;
    let mut next = 0usize;
    loop {
        let prev = if r.is_some() { K } else { 0 };
        // Integer bounds written out (no `min`/`max` calls, so a KNP-1 lint has nothing to
        // confuse with the forbidden `f64::min`/`f64::max`).
        let room = MAX_DIM - prev;
        let take = if n - next < room { n - next } else { room };
        let rows = if prev + take < K { K } else { prev + take };
        let mut data = alloc(OP, rows * K)?;
        if let Some(rp) = &r {
            for rr in rp {
                data.extend_from_slice(rr);
            }
        }
        let mut tmp = [0.0; K];
        for i in next..next + take {
            row(i, &mut tmp)?;
            data.extend_from_slice(&tmp);
        }
        data.resize(rows * K, 0.0);
        next += take;
        let a = Mat::new(rows, K, data)?;
        let rm = qr(&a, 0.0)?.r()?;
        let mut out = [[0.0; K]; K];
        for (i, o) in out.iter_mut().enumerate() {
            o.copy_from_slice(rm.row(i)?);
        }
        r = Some(out);
        if next >= n {
            return Ok(out);
        }
    }
}

fn rotate(m: &mut [V3; 3], i: usize, j: usize, c: f64, s: f64) {
    let (x, y) = (m[i], m[j]);
    let (mut ni, mut nj) = ([0.0; 3], [0.0; 3]);
    for (((a, b), xk), yk) in ni.iter_mut().zip(nj.iter_mut()).zip(x).zip(y) {
        *a = c * xk - s * yk;
        *b = s * xk + c * yk;
    }
    m[i] = ni;
    m[j] = nj;
}

/// Rule 3 on the `3 x 3` factor `r` (row-major).
fn jacobi(r: [[f64; 3]; 3]) -> Result<DataShape, FitError> {
    let mut col = [[0.0; 3]; 3];
    for (i, rr) in r.iter().enumerate() {
        for (j, &x) in rr.iter().enumerate() {
            col[j][i] = x;
        }
    }
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut converged = false;
    for _ in 0..JACOBI_MAX_SWEEPS {
        let mut rotated = false;
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            let a = red::dot(&col[i], &col[i])?;
            let b = red::dot(&col[j], &col[j])?;
            let g = red::dot(&col[i], &col[j])?;
            // The tolerance uses the column norms from `red::norm2` (overflow- and
            // underflow-safe), not `sqrt(a) * sqrt(b)`: a column whose squared norm
            // underflows to zero still gets a non-zero tolerance.
            let (na, nb) = (red::norm2(&col[i])?, red::norm2(&col[j])?);
            if g == 0.0 || g.abs() <= JACOBI_TOL * (na * nb) {
                continue;
            }
            let zeta = (b - a) / (2.0 * g);
            if !zeta.is_finite() {
                continue;
            }
            let mag = 1.0 / (zeta.abs() + f::hypot(1.0, zeta)?);
            let t = if zeta < 0.0 { -mag } else { mag };
            let c = 1.0 / f::hypot(1.0, t)?;
            let s = c * t;
            let mut trial = col;
            rotate(&mut trial, i, j, c, s);
            // A rotation counts only if it changes a data column bit for bit; one that rounds
            // back to the same columns is not applied (to `V` either), so the sweep loop
            // cannot stall on rotations that change nothing.
            let same = |x: &V3, y: &V3| x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits());
            if same(&trial[i], &col[i]) && same(&trial[j], &col[j]) {
                continue;
            }
            col = trial;
            rotate(&mut v, i, j, c, s);
            rotated = true;
        }
        if !rotated {
            converged = true;
            break;
        }
    }
    if !converged {
        return Err(FitError::NotConverged {
            iterations: JACOBI_MAX_SWEEPS,
            reason: NotConvergedReason::JacobiSweeps,
            last: None,
        });
    }
    let sv = [red::norm2(&col[0])?, red::norm2(&col[1])?, red::norm2(&col[2])?];
    // Insertion sort of the indices by descending total order; ties keep the lower index.
    let mut order = [0usize, 1, 2];
    for k in 1..3 {
        let mut p = k;
        while p > 0 && sv[order[p]].total_cmp(&sv[order[p - 1]]) == core::cmp::Ordering::Greater {
            order.swap(p, p - 1);
            p -= 1;
        }
    }
    let singular_values = [sv[order[0]], sv[order[1]], sv[order[2]]];
    let axes =
        [normalize_direction(v[order[0]])?, normalize_direction(v[order[1]])?, normalize_direction(v[order[2]])?];
    Ok(DataShape { singular_values, axes })
}

/// Singular values and principal axes of centred, scaled data (rules 2 and 3).
pub fn data_shape(c: &Centred) -> Result<DataShape, FitError> {
    shape_of_rows(c.xyz())
}

/// Rules 2 and 3 on any interleaved `n x 3` buffer (for example unit normals, whose smallest
/// right singular vector is the smallest-eigenvalue eigenvector of their scatter; used by
/// `init`, KRN.LSQ.INIT).
pub(crate) fn shape_of_rows(xyz: &[f64]) -> Result<DataShape, FitError> {
    let r = tsqr::<3, _>(xyz.len() / 3, |i, out| {
        out.copy_from_slice(xyz.get(3 * i..3 * i + 3).ok_or(FitError::BadLength { len: xyz.len() })?);
        Ok(())
    })?;
    jacobi(r)
}

/// Angular coverage (rule 5) of the points around the algebraic circle in the plane of the
/// axes `e0`, `e1`, in radians.
pub fn angular_coverage(c: &Centred, e0: V3, e1: V3, rank_tol: f64) -> Result<f64, FitError> {
    let n = c.len();
    let xyz = c.xyz();
    let mut u = alloc(OP, n)?;
    let mut v = alloc(OP, n)?;
    for q in xyz.chunks_exact(3) {
        u.push(red::dot(q, &e0)?);
        v.push(red::dot(q, &e1)?);
    }
    let r = tsqr::<4, _>(n, |i, out| {
        let (ui, vi) = (u[i], v[i]);
        *out = [ui, vi, 1.0, -(ui * ui + vi * vi)];
        Ok(())
    })?;
    let a = Mat::new(3, 3, {
        let mut d = alloc(OP, 9)?;
        for rr in &r[..3] {
            d.extend_from_slice(&rr[..3]);
        }
        d
    })?;
    let b = [r[0][3], r[1][3], r[2][3]];
    let sol = lstsq(&a, &b, rank_tol)?;
    let (uc, vc) = (-sol.x[0] / 2.0, -sol.x[1] / 2.0);
    let mut th = alloc(OP, n)?;
    for (ui, vi) in u.iter().zip(&v) {
        th.push(f::atan2(vi - vc, ui - uc)?);
    }
    th.sort_unstable_by(|a, b| a.total_cmp(b));
    let (Some(&first), Some(&last)) = (th.first(), th.last()) else {
        return Ok(0.0);
    };
    let mut gap = (first + TWO_PI) - last;
    for w in th.windows(2) {
        gap = f::total_max(gap, w[1] - w[0])?;
    }
    Ok(f::total_max(0.0, TWO_PI - gap)?)
}

/// Runs the checks of the module documentation on already centred data, for `primitive`
/// under `t`. Invalid thresholds are [`FitError::InvalidParams`].
pub fn check(primitive: Primitive, c: &Centred, t: &Thresholds) -> Result<Degeneracy, FitError> {
    check_thresholds(t)?;
    let req = primitive.requirement();
    let n = c.len();
    if n < req.min_points {
        return Err(FitError::TooFewPoints { n, need: req.min_points });
    }
    let shape = data_shape(c)?;
    let s = shape.singular_values;
    let rank = rank_by_rule(&s, t.rank_tol)?;
    let cond = match req.rank {
        0 => 1.0,
        _ if s[0] == 0.0 => f64::INFINITY,
        k => s[0] / s[k - 1],
    };
    if rank < req.rank {
        return Err(if rank <= 1 {
            FitError::Collinear { n, rank, cond }
        } else {
            FitError::Coplanar { n, rank, cond }
        });
    }
    let coverage = if req.coverage {
        let cov = angular_coverage(c, shape.axes[0], shape.axes[1], t.rank_tol)?;
        if cov < t.min_coverage {
            return Err(FitError::InsufficientCoverage { n, coverage: cov, need: t.min_coverage });
        }
        Some(cov)
    } else {
        None
    };
    Ok(Degeneracy { n, rank, cond, shape, coverage })
}

/// Counts, centres and checks a view: [`FitError::TooFewPoints`] before centring, then
/// [`Centred::new`] and [`check`]. Returns the centred data for the fit to use.
pub fn check_points(
    primitive: Primitive,
    pts: &PointsView<'_>,
    t: &Thresholds,
) -> Result<(Centred, Degeneracy), FitError> {
    check_thresholds(t)?;
    let need = primitive.requirement().min_points;
    if pts.len() < need {
        return Err(FitError::TooFewPoints { n: pts.len(), need });
    }
    let c = Centred::new(pts)?;
    let d = check(primitive, &c, t)?;
    Ok((c, d))
}

fn check_thresholds(t: &Thresholds) -> Result<(), FitError> {
    if !((0.0..1.0).contains(&t.rank_tol)) {
        return Err(FitError::InvalidParams { field: "thresholds.rank_tol" });
    }
    if !((0.0..=TWO_PI).contains(&t.min_coverage)) {
        return Err(FitError::InvalidParams { field: "thresholds.min_coverage" });
    }
    if !((0.0..1.0).contains(&t.eig_ratio_max)) {
        return Err(FitError::InvalidParams { field: "thresholds.eig_ratio_max" });
    }
    if !(t.planarity_ratio_min.is_finite() && t.planarity_ratio_min >= 1.0) {
        return Err(FitError::InvalidParams { field: "thresholds.planarity_ratio_min" });
    }
    if !(t.cond_max.is_finite() && t.cond_max >= 1.0) {
        return Err(FitError::InvalidParams { field: "thresholds.cond_max" });
    }
    Ok(())
}
