//! Residual statistics, fit diagnostics and certificates (REQ-KRN.LSQ.TRAIT-005, -006).

use fid_num::{f, red, NumError};
use fid_solv::nlls::{NllsResult, StopReason as NllsStop};

use super::error::{alloc, FitError};

const OP: &str = "fit.stats";

/// Statistics of the signed residuals, all through `fid_num::red` (REQ-KRN.LSQ.TRAIT-005).
///
/// Pinned rules, with `n` residuals `r`:
/// `rms = red::norm2(r) / f::sqrt(n)`; `max` and `min` are the total-order maximum and
/// minimum (`f::total_max`, `f::total_min`, so `+0.0 > -0.0`); `mean = red::mean(r)`;
/// `std_dev = red::norm2(r - mean) / f::sqrt(n)` (population form, divisor `n`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResidualStats {
    pub n: usize,
    pub rms: f64,
    pub max: f64,
    pub min: f64,
    pub mean: f64,
    pub std_dev: f64,
}

fn finite(x: f64) -> Result<f64, FitError> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(FitError::Num(NumError::Infinite { op: OP, index: None }))
    }
}

impl ResidualStats {
    /// Statistics of `r`. An empty slice is [`FitError::Num`] (`EmptyInput`); a NaN or
    /// infinity, or an overflow, is [`FitError::Num`].
    pub fn of(r: &[f64]) -> Result<ResidualStats, FitError> {
        let n = r.len();
        let mean = red::mean(r)?;
        let sqrt_n = f::sqrt(n as f64)?;
        let rms = finite(red::norm2(r)? / sqrt_n)?;
        let (mut max, mut min) = (mean, mean);
        if let Some(&first) = r.first() {
            max = first;
            min = first;
        }
        for &x in r {
            max = f::total_max(max, x)?;
            min = f::total_min(min, x)?;
        }
        let mut dev = alloc(OP, n)?;
        for &x in r {
            dev.push(finite(x - mean)?);
        }
        let std_dev = finite(red::norm2(&dev)? / sqrt_n)?;
        Ok(ResidualStats { n, rms, max, min, mean, std_dev })
    }
}

/// Why a fit stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// A direct (non-iterative) method.
    ClosedForm,
    /// `fid_solv::nlls`: the residuals are exactly zero.
    ZeroResidual,
    /// `fid_solv::nlls`: the gradient test held.
    Gradient,
    /// `fid_solv::nlls`: the step test held.
    Step,
    /// `fid_solv::nlls`: the cost test held.
    Cost,
    /// An exchange or active-set method found no improving exchange (minimum zone,
    /// envelope).
    Exchange,
}

impl StopReason {
    /// The wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            StopReason::ClosedForm => "closed_form",
            StopReason::ZeroResidual => "zero_residual",
            StopReason::Gradient => "gradient",
            StopReason::Step => "step",
            StopReason::Cost => "cost",
            StopReason::Exchange => "exchange",
        }
    }
}

impl From<NllsStop> for StopReason {
    fn from(s: NllsStop) -> Self {
        match s {
            NllsStop::ZeroResidual => StopReason::ZeroResidual,
            NllsStop::Gradient => StopReason::Gradient,
            NllsStop::Step => StopReason::Step,
            NllsStop::Cost => StopReason::Cost,
        }
    }
}

/// Solver diagnostics of a fit (REQ-KRN.LSQ.TRAIT-006).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitDiagnostics {
    /// Norm of the objective gradient at the solution (in the fit's internal scaled units).
    pub grad_norm: f64,
    /// Numerical rank of the Jacobian (or of the design matrix of a direct method).
    pub rank: usize,
    /// Condition estimate of that matrix (`+inf` when singular or beyond range).
    pub cond: f64,
    /// Accepted iterations (0 for a direct method).
    pub iterations: usize,
    pub stop: StopReason,
}

impl FitDiagnostics {
    /// The diagnostics of a converged `fid_solv::nlls` run.
    pub fn from_nlls(r: &NllsResult) -> FitDiagnostics {
        FitDiagnostics {
            grad_norm: r.grad_norm,
            rank: r.rank,
            cond: r.cond,
            iterations: r.iterations,
            stop: r.stop_reason.into(),
        }
    }
}

/// Evidence that a result is optimal, for methods that produce it (minimum zone, envelope):
/// the indices of the supporting points and the remaining optimality gap (metres).
#[derive(Debug, Clone, PartialEq)]
pub struct Certificate {
    pub support: Vec<usize>,
    pub gap: f64,
}
