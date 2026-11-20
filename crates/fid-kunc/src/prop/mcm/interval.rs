//! Coverage intervals and estimate outputs from Monte Carlo samples. Spec node
//! `UNC.PROP.MCM.INTVL`, algorithm [`ALGORITHM_ID`] = `unc.prop.mcm_interval@1.0.0`.
//!
//! [`summarize`] turns the output values of a Monte Carlo run (or of one block of it, for
//! the adaptive rule of `UNC.PROP.MCM.ADAPT`) into the summary JCGM 101:2008 describes
//! [unverified: clause 7.7 numbering]: mean, standard uncertainty, probabilistically
//! symmetric and shortest coverage intervals, and the difference between the MC mean and
//! the kernel value `f(x)` at the measured inputs (for the estimator policy of
//! `UNC.PROP.SELECT`). Per-block summaries are this same function applied to one block's
//! values.
//!
//! # Rules (pinned; D0)
//!
//! 1. Validation, in this order: `p` finite with `0 < p < 1`, else [`McmError::InvalidP`];
//!    `fx` finite, else `Num` (op `mcm.interval.fx`); every value finite, else `Num` (op
//!    `mcm.interval.values`, index of the first offending value in input order); then
//!    `M >= ceil(1 / (1 - p))` (evaluated in binary64 as `(1.0 / (1.0 - p)).ceil()`),
//!    else [`McmError::TooFewTrials`] (REQ-UNC.PROP.MCM.INTVL-003).
//! 2. The values are copied and stably sorted by IEEE-754 totalOrder (`-0.0 < +0.0`).
//! 3. Ranks are 1-based. `m = M as f64`, `r = ((m * (1.0 - p)) * 0.5).round()`,
//!    `q = (p * m).round()` (`round` is half away from zero). The symmetric interval is
//!    `[x_(r), x_(r + q)]`, each rank clamped to `1..=M` independently
//!    (REQ-UNC.PROP.MCM.INTVL-002).
//! 4. The shortest interval is `[x_(s), x_(s + q)]` for the start rank `s` in `1..=M - q`
//!    with the smallest `x_(s + q) - x_(s)`; the lowest `s` wins ties (strict `<` scan in
//!    ascending `s`). It always covers the same number of ranks as `q`.
//! 5. `mean = red::mean(sorted)`; `u = red::norm2(sorted - mean) / sqrt(M - 1)` with the
//!    deviations in sorted order and `sqrt` = `fid_num::f::sqrt` (overflow-safe; an
//!    overflow is `Num`); `mean_minus_fx = mean - fx`, which must be finite.
//!
//! KNP-1: no FMA, no `f64::min`/`max`/`clamp`, reductions through `fid_num::red`, a NaN is
//! always an error, no panics, no hash containers.

use super::McmError;
use crate::statement::{CoverageInterval, IntervalKind};
use fid_num::{check_finite, check_finite_slice, f, red};

/// Algorithm ID of this rule set.
pub const ALGORITHM_ID: &str = "unc.prop.mcm_interval@1.0.0";

/// Summary of a Monte Carlo value set (REQ-UNC.PROP.MCM.INTVL-001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McmSummary {
    /// Mean of the values (the MC mean, not the reported value; see `UNC.PROP.SELECT`).
    pub mean: f64,
    /// Standard deviation of the values (divisor `M - 1`): the standard uncertainty.
    pub u: f64,
    /// Probabilistically symmetric coverage interval at `p`.
    pub sym: CoverageInterval,
    /// Shortest coverage interval at `p`.
    pub shortest: CoverageInterval,
    /// `mean - fx`, for bias disclosure by the estimator policy (REQ-UNC.PROP.MCM-006).
    pub mean_minus_fx: f64,
    /// Number of values `M`.
    pub m: u64,
    /// 1-based ranks `(low, high)` of the symmetric interval endpoints.
    pub sym_ranks: (u64, u64),
    /// 1-based ranks `(low, high)` of the shortest interval endpoints.
    pub shortest_ranks: (u64, u64),
}

/// Clamps a rank to `1..=m` without `min`/`max`.
fn clamp_rank(rank: usize, m: usize) -> usize {
    if rank < 1 {
        1
    } else if rank > m {
        m
    } else {
        rank
    }
}

/// Summarises `values` at coverage probability `p` with kernel value `fx` (rules in the
/// module documentation). The input order of `values` does not matter.
pub fn summarize(values: &[f64], p: f64, fx: f64) -> Result<McmSummary, McmError> {
    if !p.is_finite() || p <= 0.0 || p >= 1.0 {
        return Err(McmError::InvalidP);
    }
    check_finite("mcm.interval.fx", fx)?;
    check_finite_slice("mcm.interval.values", values)?;
    let m_len = values.len();
    // Slice lengths of f64 never exceed 2^53 on supported targets, so this is exact.
    let m = m_len as f64;
    let need = (1.0 / (1.0 - p)).ceil();
    if m < need {
        // `need` is at most 2^53 (1 - p >= 2^-53), so the conversion is exact.
        return Err(McmError::TooFewTrials { m: m_len as u64, min: need as u64 });
    }

    let mut xs: Vec<f64> = Vec::new();
    xs.try_reserve_exact(m_len).map_err(|_| McmError::Allocation)?;
    xs.extend_from_slice(values);
    xs.sort_by(|a, b| a.total_cmp(b));

    // Rank arithmetic (rule 3). Both products are finite and non-negative, so `as usize`
    // is exact after `round`.
    let r = ((m * (1.0 - p)) * 0.5).round() as usize;
    let q = (p * m).round() as usize;
    let sym_lo = clamp_rank(r, m_len);
    let sym_hi = clamp_rank(r.saturating_add(q), m_len);

    // Rule 4. `M >= 2` and `q <= M - 1` hold after validation (M (1 - p) >= 1 gives
    // p M <= M - 1), but the window width is still bounded defensively.
    let width = if q >= m_len { m_len - 1 } else { q };
    let mut best_start = 0usize;
    let mut best_len = xs[width] - xs[0];
    let mut s = 1usize;
    while s + width < m_len {
        let len = xs[s + width] - xs[s];
        if len.total_cmp(&best_len).is_lt() {
            best_len = len;
            best_start = s;
        }
        s += 1;
    }

    // Rule 5.
    let mean = red::mean(&xs)?;
    let mut dev: Vec<f64> = Vec::new();
    dev.try_reserve_exact(m_len).map_err(|_| McmError::Allocation)?;
    dev.extend(xs.iter().map(|&x| x - mean));
    let u = red::norm2(&dev)? / f::sqrt(m - 1.0)?;
    let u = check_finite("mcm.interval.u", u)?;
    let mean_minus_fx = check_finite("mcm.interval.mean_minus_fx", mean - fx)?;

    Ok(McmSummary {
        mean,
        u,
        sym: CoverageInterval { low: xs[sym_lo - 1], high: xs[sym_hi - 1], p, kind: IntervalKind::ProbSymmetric },
        shortest: CoverageInterval {
            low: xs[best_start],
            high: xs[best_start + width],
            p,
            kind: IntervalKind::Shortest,
        },
        mean_minus_fx,
        m: m_len as u64,
        sym_ranks: (sym_lo as u64, sym_hi as u64),
        shortest_ranks: ((best_start + 1) as u64, (best_start + width + 1) as u64),
    })
}
