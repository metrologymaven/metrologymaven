//! Test-only central-difference harness for tangent providers. Spec node
//! `UNC.PROP.LPU.TAN` (REQ-UNC.PROP.LPU.TAN-005).
//!
//! Compiled only with the `tangent-harness` feature, which only dev-dependencies enable: the
//! engine never differentiates numerically (REQ-003), so this module lives outside
//! `prop::lpu` and the engine cannot call it.
//!
//! [`check_provider`] evaluates the operator (through a caller-supplied evaluation function,
//! for example the kernel call interface) at every registered sample point of the provider
//! and compares every Jacobian entry with a central difference.
//!
//! # Rule (pinned)
//!
//! - Step for input component `j`: `h_j = 2^-17 * s_j` with `s_j = |x_j|` when `|x_j| > 1`,
//!   else `1`. The difference quotient divides by the realised step
//!   `(x_j + h_j) - (x_j - h_j)`, not by `2 h_j`.
//! - Oracle entry: `D_ij = (f_i(x + h_j e_j) - f_i(x - h_j e_j)) / ((x_j + h_j) - (x_j - h_j))`.
//! - Row scale: `r_i = max_k |D_ik|`.
//! - Entry `(i, j)` passes when `|J_ij - D_ij| <= REL_TOL * max(|J_ij|, |D_ij|, r_i)` with
//!   [`REL_TOL`] `= 1e-6` [unverified]. The row-scale floor keeps the check relative for
//!   entries far below the rest of their row, where the central difference has only absolute
//!   accuracy. A row whose oracle is exactly zero must be exactly zero.
//! - The first failing entry in (point, row, column) order is reported.

use crate::prop::lpu::tan::{OperatorTangent, ProviderError, SamplePoint};

/// Relative tolerance between a provider entry and the central difference [unverified].
pub const REL_TOL: f64 = 1e-6;
/// Base step `2^-17` (about the cube root of the f64 epsilon), exact in binary.
pub const STEP: f64 = 1.0 / 131_072.0;

/// Evaluates the operator: parameter bytes and input values in, output values out.
pub type Eval<'a> = &'a dyn Fn(&[u8], &[f64]) -> Result<Vec<f64>, String>;

/// Why the harness rejected a provider.
#[derive(Debug, Clone, PartialEq)]
pub enum HarnessError {
    /// The provider registers no sample point.
    NoSamplePoints,
    /// The provider failed at sample point `point`.
    Provider { point: usize, error: ProviderError },
    /// The operator evaluation failed (or returned a different output count) at `point`.
    Eval { point: usize, detail: String },
    /// An input, output or difference quotient is not finite at `point`.
    NonFinite { point: usize },
    /// The provider's Jacobian shape differs from (outputs, inputs) at `point`.
    Shape { point: usize, expected: (usize, usize), actual: (usize, usize) },
    /// Entry (`row`, `col`) disagrees with the central difference at `point`.
    Entry { point: usize, row: usize, col: usize, provider: f64, oracle: f64 },
}

/// What the harness checked when it accepts a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessReport {
    /// Sample points checked.
    pub points: usize,
    /// Jacobian entries checked, over all points.
    pub entries: usize,
}

fn larger(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// Checks `provider` at its registered sample points (REQ-005).
pub fn check_provider(provider: &dyn OperatorTangent, eval: Eval<'_>) -> Result<HarnessReport, HarnessError> {
    check_provider_at(provider, &provider.sample_points(), eval)
}

/// Checks `provider` at the given points (its registered ones, or extra points).
pub fn check_provider_at(
    provider: &dyn OperatorTangent,
    points: &[SamplePoint],
    eval: Eval<'_>,
) -> Result<HarnessReport, HarnessError> {
    if points.is_empty() {
        return Err(HarnessError::NoSamplePoints);
    }
    let mut entries = 0;
    for (point, sp) in points.iter().enumerate() {
        entries += check_point(provider, point, sp, eval)?;
    }
    Ok(HarnessReport { points: points.len(), entries })
}

fn run(eval: Eval<'_>, point: usize, params: &[u8], x: &[f64], rows: Option<usize>) -> Result<Vec<f64>, HarnessError> {
    let y = eval(params, x).map_err(|detail| HarnessError::Eval { point, detail })?;
    if let Some(r) = rows {
        if y.len() != r {
            return Err(HarnessError::Eval { point, detail: format!("{} outputs, expected {r}", y.len()) });
        }
    }
    if y.iter().any(|v| !v.is_finite()) {
        return Err(HarnessError::NonFinite { point });
    }
    Ok(y)
}

fn check_point(
    provider: &dyn OperatorTangent,
    point: usize,
    sp: &SamplePoint,
    eval: Eval<'_>,
) -> Result<usize, HarnessError> {
    let x: Vec<f64> = sp.inputs.iter().map(|s| s.value).collect();
    if x.iter().any(|v| !v.is_finite()) {
        return Err(HarnessError::NonFinite { point });
    }
    let rows = run(eval, point, &sp.params, &x, None)?.len();
    let cols = x.len();
    let out = provider.jacobian(&sp.params, &sp.inputs).map_err(|error| HarnessError::Provider { point, error })?;
    let actual = (out.jac.rows(), out.jac.cols());
    if actual != (rows, cols) {
        return Err(HarnessError::Shape { point, expected: (rows, cols), actual });
    }
    // Central differences, stored row-major like the Jacobian.
    let mut d = vec![0.0; rows * cols];
    for (j, &xj) in x.iter().enumerate() {
        let s = if xj.abs() > 1.0 { xj.abs() } else { 1.0 };
        let h = STEP * s;
        let (up, down) = (xj + h, xj - h);
        let span = up - down;
        let mut xp = x.clone();
        xp[j] = up;
        let mut xm = x.clone();
        xm[j] = down;
        let fp = run(eval, point, &sp.params, &xp, Some(rows))?;
        let fm = run(eval, point, &sp.params, &xm, Some(rows))?;
        for (i, (a, b)) in fp.iter().zip(&fm).enumerate() {
            let q = (a - b) / span;
            if !q.is_finite() {
                return Err(HarnessError::NonFinite { point });
            }
            d[i * cols + j] = q;
        }
    }
    let jac = out.jac.data();
    for (i, (oracle_row, provider_row)) in d.chunks(cols).zip(jac.chunks(cols)).enumerate() {
        let scale = oracle_row.iter().fold(0.0, |m: f64, v| larger(m, v.abs()));
        for (j, (&o, &p)) in oracle_row.iter().zip(provider_row).enumerate() {
            let bound = REL_TOL * larger(larger(p.abs(), o.abs()), scale);
            if (p - o).abs() > bound {
                return Err(HarnessError::Entry { point, row: i, col: j, provider: p, oracle: o });
            }
        }
    }
    Ok(rows * cols)
}
