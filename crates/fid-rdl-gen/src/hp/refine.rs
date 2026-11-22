//! High-precision Gauss-Newton refinement with Householder QR in 256-bit [`Hp`]
//! (REQ-UNC.RDL.GEN.HPREC-002, AC-2).
//!
//! Stops only when the relative gradient norm `||J^T r||_2 / max(1, ||J||_F * ||r||_2)` falls
//! strictly below `1e-50`, and fails with [`HpError::NotConverged`] (`hp_not_converged`) after
//! [`MAX_REFINE_ITERATIONS`] (50) iterations.

use std::fmt;

use super::Hp;

/// Maximum Gauss-Newton iterations before failing `hp_not_converged` (REQ-002).
pub const MAX_REFINE_ITERATIONS: u32 = 50;

/// Errors returned by `Hp` arithmetic, decimal parsing, and `hp_refine`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HpError {
    /// `hp_refine` did not reach relative gradient norm `< 1e-50` within 50 iterations.
    NotConverged { iterations: u32, grad_norm: Hp },
    /// Jacobian is rank-deficient or singular during QR solve.
    Singular,
    /// Mathematical domain refusal (e.g. division by zero, negative sqrt, non-finite f64).
    Domain(&'static str),
    /// Decimal string parse refusal.
    Parse(&'static str),
}

impl HpError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            HpError::NotConverged { .. } => "hp_not_converged",
            HpError::Singular => "hp_singular",
            HpError::Domain(_) => "hp_domain",
            HpError::Parse(_) => "hp_parse",
        }
    }
}

impl fmt::Display for HpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HpError::NotConverged { iterations, .. } => {
                write!(f, "hp_not_converged: failed to reach < 1e-50 in {iterations} iterations")
            }
            HpError::Singular => write!(f, "hp_singular: rank-deficient Jacobian"),
            HpError::Domain(msg) => write!(f, "hp_domain: {msg}"),
            HpError::Parse(msg) => write!(f, "hp_parse: {msg}"),
        }
    }
}

impl std::error::Error for HpError {}

/// Convergence certificate returned by [`hp_refine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HpCert {
    /// Final relative gradient norm (`< 1e-50`).
    pub grad_norm: Hp,
    /// Number of Gauss-Newton steps taken (`0..=50`).
    pub iterations: u32,
}

/// Threshold `1e-50` in 256-bit `Hp`.
pub fn tol_1e_50() -> Result<Hp, HpError> {
    Hp::parse_decimal("1e-50")
}

/// Singularity floor `1e-65` relative to max R diagonal.
fn sing_floor() -> Result<Hp, HpError> {
    Hp::parse_decimal("1e-65")
}

/// High-precision Gauss-Newton refinement with Householder QR in [`Hp`]
/// (REQ-UNC.RDL.GEN.HPREC-002).
///
/// `eval(p)` returns `(residuals, jacobian)` where `residuals` has length `m` and `jacobian`
/// has `m` rows of length `k = p.len()` (`m >= k >= 1`).
pub fn hp_refine<F>(eval: F, p0: &[Hp]) -> Result<(Vec<Hp>, HpCert), HpError>
where
    F: Fn(&[Hp]) -> Result<(Vec<Hp>, Vec<Vec<Hp>>), HpError>,
{
    let k = p0.len();
    if k == 0 {
        return Err(HpError::Singular);
    }
    let tol = tol_1e_50()?;
    let mut p = p0.to_vec();

    for iter in 0..=MAX_REFINE_ITERATIONS {
        let (r, j) = eval(&p)?;
        let m = r.len();
        if m < k || j.len() != m || j.iter().any(|row| row.len() != k) {
            return Err(HpError::Singular);
        }

        let rel_grad = relative_grad_norm(&j, &r)?;
        if rel_grad < tol {
            return Ok((p, HpCert { grad_norm: rel_grad, iterations: iter }));
        }
        if iter == MAX_REFINE_ITERATIONS {
            return Err(HpError::NotConverged { iterations: MAX_REFINE_ITERATIONS, grad_norm: rel_grad });
        }

        // Solve least-squares min ||J * dp + r||_2 via Householder QR of J.
        let neg_r: Vec<Hp> = r.into_iter().map(|v| -v).collect();
        let dp = qr_least_squares(j, neg_r)?;
        for (pi, dpi) in p.iter_mut().zip(dp.iter()) {
            *pi = *pi + *dpi;
        }
    }

    Err(HpError::NotConverged { iterations: MAX_REFINE_ITERATIONS, grad_norm: Hp::ONE })
}

/// Computes `||J^T r||_2 / max(1, ||J||_F * ||r||_2)`.
fn relative_grad_norm(j: &[Vec<Hp>], r: &[Hp]) -> Result<Hp, HpError> {
    let k = j.first().map_or(0, Vec::len);
    let mut grad_sq = Hp::ZERO;
    for col in 0..k {
        let mut g = Hp::ZERO;
        for (row, &ri) in j.iter().zip(r.iter()) {
            g = g + row[col] * ri;
        }
        grad_sq = grad_sq + g * g;
    }
    let g_norm = grad_sq.sqrt()?;

    let mut r_sq = Hp::ZERO;
    for &ri in r {
        r_sq = r_sq + ri * ri;
    }
    let r_norm = r_sq.sqrt()?;

    let mut j_sq = Hp::ZERO;
    for row in j {
        for &v in row {
            j_sq = j_sq + v * v;
        }
    }
    let j_norm = j_sq.sqrt()?;
    if j_norm.is_zero() {
        return Err(HpError::Singular);
    }

    let scale = (j_norm * r_norm).max(Hp::ONE);
    g_norm / scale
}

/// Solves `min ||A x - b||_2` for `m x k` matrix `A` (`m >= k`) using Householder QR in [`Hp`].
fn qr_least_squares(mut a: Vec<Vec<Hp>>, mut b: Vec<Hp>) -> Result<Vec<Hp>, HpError> {
    let m = a.len();
    let k = a.first().map_or(0, Vec::len);
    if k == 0 || m < k {
        return Err(HpError::Singular);
    }

    for col in 0..k {
        // Compute norm of subcolumn a[col..m][col].
        let mut col_sq = Hp::ZERO;
        for row in a.iter().skip(col) {
            col_sq = col_sq + row[col] * row[col];
        }
        if col_sq.is_zero() {
            return Err(HpError::Singular);
        }
        let norm = col_sq.sqrt()?;
        let x0 = a[col][col];
        // Choose sign opposite to x0 to avoid cancellation.
        let alpha = if x0.is_negative() { norm } else { -norm };
        let v0 = x0 - alpha;

        let mut v = Vec::with_capacity(m - col);
        v.push(v0);
        for row in a.iter().skip(col + 1) {
            v.push(row[col]);
        }

        let mut v_sq = Hp::ZERO;
        for &vi in &v {
            v_sq = v_sq + vi * vi;
        }
        if v_sq.is_zero() {
            continue;
        }
        let beta = (Hp::TWO / v_sq)?;

        // Apply H = I - beta * v * v^T to remaining columns of A.
        a[col][col] = alpha;
        for row in a.iter_mut().skip(col + 1) {
            row[col] = Hp::ZERO;
        }
        for c in (col + 1)..k {
            let mut dot = Hp::ZERO;
            for (offset, &vi) in v.iter().enumerate() {
                dot = dot + vi * a[col + offset][c];
            }
            let factor = beta * dot;
            for (offset, &vi) in v.iter().enumerate() {
                a[col + offset][c] = a[col + offset][c] - factor * vi;
            }
        }

        // Apply H to RHS vector b.
        let mut dot_b = Hp::ZERO;
        for (offset, &vi) in v.iter().enumerate() {
            dot_b = dot_b + vi * b[col + offset];
        }
        let factor_b = beta * dot_b;
        for (offset, &vi) in v.iter().enumerate() {
            b[col + offset] = b[col + offset] - factor_b * vi;
        }
    }

    // Check relative diagonal conditioning before back-substitution.
    let mut max_diag = Hp::ZERO;
    for (i, row) in a.iter().enumerate().take(k) {
        let d = row[i].abs();
        if d > max_diag {
            max_diag = d;
        }
    }
    if max_diag.is_zero() {
        return Err(HpError::Singular);
    }
    let floor = max_diag * sing_floor()?;
    for (i, row) in a.iter().enumerate().take(k) {
        if row[i].abs() <= floor {
            return Err(HpError::Singular);
        }
    }

    // Back-substitution on upper triangular R (k x k).
    let mut x = vec![Hp::ZERO; k];
    for i in (0..k).rev() {
        let mut rhs = b[i];
        for j in (i + 1)..k {
            rhs = rhs - a[i][j] * x[j];
        }
        x[i] = (rhs / a[i][i])?;
    }
    Ok(x)
}
