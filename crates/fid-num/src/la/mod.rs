//! Dense linear algebra. Spec nodes `KRN.NUM.LA.MAT` (this base) and `KRN.NUM.LA.QR`
//! (Householder QR, column pivoting and least squares: [`qr`], [`qr_pivoted`], [`lstsq`]; the
//! pinned rules are in the `qr` module source).
//!
//! Row-major `f64` matrices with checked construction ([`Mat`]), products routed through
//! [`crate::red::dot`] ([`mat_vec`], [`mat_t_vec`], [`mat_mul`]), triangular solves
//! ([`solve_upper`], [`solve_lower`]), the numerical-rank rule ([`rank_by_rule`]), a
//! deterministic 2-norm condition estimator for triangular factors ([`cond_triangular`]) and
//! the shared diagnostics record [`Factorization`].
//!
//! Every value-affecting choice is pinned in the leaf's Design section
//! (`spec/krn/num/la/mat/NODE.md`); the summary below is normative for this module and the
//! Python model in `fixtures/num/la/make_la_fixtures.py` reproduces it bit for bit.
//!
//! # Arithmetic (REQ-KRN.NUM.LA.MAT-002, -007)
//!
//! - Every inner product is one call of [`crate::red::dot`] over two slices in ascending index
//!   order. Columns are gathered into a scratch vector first. There is no FMA, no `mul_add`
//!   and no other accumulation; every other operation is a single IEEE-754 `-`, `*` or `/`.
//! - `mat_vec`: `y[i] = dot(A[i, 0..k], x)`. `mat_t_vec`: `y[j] = dot(A[0..m, j], x)`.
//!   `mat_mul`: `C[i][j] = dot(A[i, 0..k], B[0..k, j])`.
//! - `solve_upper` (back substitution, `i = n-1` down to `0`):
//!   `x[i] = (b[i] - dot(R[i, i+1..n], x[i+1..n])) / R[i][i]`.
//!   `solve_lower` (forward substitution, `i = 0` up to `n-1`):
//!   `x[i] = (b[i] - dot(L[i, 0..i], x[0..i])) / L[i][i]`.
//!   An empty `dot` is `+0.0`. Only the named triangle (diagonal included) is read; the other
//!   strict triangle is ignored, so a factor stored in a full matrix needs no clean-up.
//!
//! # Singularity and rank (REQ-KRN.NUM.LA.MAT-003, -004)
//!
//! With `m` the total-order maximum of `|R[j][j]|`, the diagonal is scanned in ascending
//! index order before any arithmetic, and the first `i` with `|R[i][i]| <= rel_tol * m` (one
//! rounded product) is reported as [`NumError::Singular`] whose `rank` field carries that
//! index (decoded by [`LaError::of`] as [`LaError::Singular`]). Every diagonal entry before
//! it passed the test. [`rank_by_rule`] counts the values strictly greater than
//! `rel_tol * d[0]` in a non-increasing list of non-negative values.
//!
//! # Errors (REQ-KRN.NUM.LA.MAT-001)
//!
//! Every buffer is reserved with `try_reserve_exact` for exactly the elements it will hold, so
//! allocation failure is [`LaError::Allocation`] (never an abort) and no buffer has slack.
//!
//! Shape, tolerance and allocation errors are [`NumError::Domain`] with a [`LaError`] detail; inputs
//! with a NaN or infinity are [`NumError::NotANumber`] / [`NumError::Infinite`] naming the
//! first offending (row-major) index; an overflow is [`NumError::Infinite`] naming the first
//! output element (in computation order) that overflowed. The `op` is `la.<function>`.

mod cond;
mod mat;
mod qr;
mod tri;

pub use cond::{cond_triangular, ITERATIONS as COND_ITERATIONS};
pub use mat::{mat_mul, mat_t_vec, mat_vec, Mat, MAX_DIM, MAX_MAT_BYTES};
pub use qr::{lstsq, qr, qr_pivoted, LstsqResult, Qr};
pub use tri::{rank_by_rule, solve_lower, solve_upper};

use crate::err::NumError;
use crate::f;

/// Diagnostics every factorization reports (REQ-KRN.NUM.LA-003): the numerical rank under
/// the caller's explicit relative tolerance and a 2-norm condition-number estimate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Factorization {
    /// Number of diagonal values (R diagonal magnitudes or singular values) above
    /// `rel_tol` times the largest, see [`rank_by_rule`].
    pub rank: usize,
    /// 2-norm condition-number estimate. [`cond_triangular`] itself is always finite (a
    /// rank-deficient factor is an error there); a QR factorization reports `f64::INFINITY`
    /// here when `rank` is below the column count or the estimate is beyond the `f64` range.
    pub cond: f64,
    /// The relative tolerance the rank was computed with, as passed by the caller.
    pub rel_tol: f64,
}

/// Why a linear-algebra call failed, decoded from a [`NumError`] by [`LaError::of`].
/// Carried in [`NumError::Domain`] as `detail`, except [`LaError::Singular`], which is a
/// [`NumError::Singular`] whose `rank` field is the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaError {
    /// A zero dimension, a ragged row list, a data length that is not `rows * cols`, a
    /// non-square triangular factor, an out-of-range row or column, or operands whose
    /// dimensions do not agree.
    BadShape,
    /// More than [`MAX_DIM`] rows or columns.
    TooLarge,
    /// A negative `rel_tol`.
    BadTolerance,
    /// A rank-rule input that is negative or not in non-increasing order.
    BadSpectrum,
    /// A result or scratch buffer could not be reserved (`Vec::try_reserve_exact` failed);
    /// no operation in this module aborts on allocation failure.
    Allocation,
    /// The first diagonal entry (ascending index) with `|r_ii| <= rel_tol * max |r_jj|`.
    Singular {
        /// Zero-based diagonal index.
        index: usize,
    },
    /// An input that is not all zeros but whose largest magnitude is subnormal (below
    /// `2^-1022`): every entry has fewer than 53 significant bits, so a factorization could not
    /// meet its accuracy bounds (`KRN.NUM.LA.QR`).
    Subnormal,
    /// `lstsq`: the numerical rank under `rel_tol` is below the number of columns. Carried as
    /// [`NumError::Singular`] with op `la.lstsq` and `rank` the rank.
    RankDeficient {
        /// The numerical rank.
        rank: usize,
    },
}

const SINGULAR_OPS: [&str; 3] = ["la.solve_upper", "la.solve_lower", "la.cond_triangular"];

/// The operation whose [`NumError::Singular`] is [`LaError::RankDeficient`].
const RANK_DEFICIENT_OP: &str = "la.lstsq";

impl LaError {
    /// The `detail` string used in [`NumError::Domain`] (`singular` and `rank_deficient` for
    /// the two kinds carried as [`NumError::Singular`], never as a detail).
    pub const fn detail(self) -> &'static str {
        match self {
            LaError::BadShape => "bad_shape",
            LaError::TooLarge => "too_large",
            LaError::BadTolerance => "bad_tolerance",
            LaError::BadSpectrum => "bad_spectrum",
            LaError::Allocation => "allocation_failed",
            LaError::Singular { .. } => "singular",
            LaError::Subnormal => "subnormal_input",
            LaError::RankDeficient { .. } => "rank_deficient",
        }
    }

    /// Recovers the kind from an error returned by this module, or `None` for any other
    /// error (NaN, infinity, overflow).
    pub fn of(e: &NumError) -> Option<LaError> {
        match e {
            NumError::Singular { op, rank } if SINGULAR_OPS.contains(op) => Some(LaError::Singular { index: *rank }),
            NumError::Singular { op, rank } if *op == RANK_DEFICIENT_OP => Some(LaError::RankDeficient { rank: *rank }),
            NumError::Domain { op, detail } if op.starts_with("la.") => [
                LaError::BadShape,
                LaError::TooLarge,
                LaError::BadTolerance,
                LaError::BadSpectrum,
                LaError::Allocation,
                LaError::Subnormal,
            ]
            .into_iter()
            .find(|k| k.detail() == *detail),
            _ => None,
        }
    }

    pub(crate) fn at(self, op: &'static str) -> NumError {
        match self {
            LaError::Singular { index } => NumError::Singular { op, rank: index },
            LaError::RankDeficient { rank } => NumError::Singular { op, rank },
            k => NumError::Domain { op, detail: k.detail() },
        }
    }
}

/// An empty vector with capacity for exactly `n` elements, reserved fallibly
/// (`try_reserve_exact`): allocation failure is [`LaError::Allocation`], never an abort.
pub(crate) fn alloc(op: &'static str, n: usize) -> Result<Vec<f64>, NumError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| LaError::Allocation.at(op))?;
    Ok(v)
}

/// `n` copies of `value` in an exact-capacity vector, reserved fallibly.
pub(crate) fn alloc_filled(op: &'static str, n: usize, value: f64) -> Result<Vec<f64>, NumError> {
    let mut v = alloc(op, n)?;
    v.resize(n, value);
    Ok(v)
}

/// Checks a relative tolerance: NaN and infinity are errors (index `None`), a negative
/// value is [`LaError::BadTolerance`]. `-0.0` is accepted and acts as zero.
pub(crate) fn check_tol(op: &'static str, rel_tol: f64) -> Result<(), NumError> {
    if rel_tol.is_nan() {
        return Err(NumError::NotANumber { op, index: None });
    }
    if rel_tol.is_infinite() {
        return Err(NumError::Infinite { op, index: None });
    }
    if rel_tol < 0.0 {
        return Err(LaError::BadTolerance.at(op));
    }
    Ok(())
}

/// The singularity scan of REQ-KRN.NUM.LA.MAT-003 over the diagonal of a square row-major
/// `n x n` matrix (finite entries).
pub(crate) fn singular_scan(op: &'static str, n: usize, data: &[f64], rel_tol: f64) -> Result<(), NumError> {
    let mut m = 0.0_f64;
    for i in 0..n {
        m = f::total_max(m, data[i * n + i].abs())?;
    }
    let thr = rel_tol * m;
    for i in 0..n {
        if data[i * n + i].abs() <= thr {
            return Err(LaError::Singular { index: i }.at(op));
        }
    }
    Ok(())
}
