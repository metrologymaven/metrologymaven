//! Coverage intervals and estimate outputs from Monte Carlo samples. Spec node
//! `UNC.PROP.MCM.INTVL`, algorithm [`ALGORITHM_ID`] = `unc.prop.mcm_interval@1.0.0`.
//!
//! [`summarize`] turns the output values of a Monte Carlo run (or of one block of it, for
//! the adaptive rule of `UNC.PROP.MCM.ADAPT`) into the summary JCGM 101:2008 describes
//! (§7.7.1 to §7.7.3): mean, standard uncertainty, probabilistically
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
//!    `M >= 2` and `M >= ceil(1 / (1 - p))` (the latter evaluated in binary64 as
//!    `(1.0 / (1.0 - p)).ceil()`), else [`McmError::TooFewTrials`] with `min` the larger
//!    of the two bounds (REQ-UNC.PROP.MCM.INTVL-003).
//! 2. The values are copied and stably sorted by IEEE-754 totalOrder (`-0.0 < +0.0`).
//! 3. Ranks are 1-based integers, as in JCGM 101:2008 §7.7.1 and §7.7.2.
//!    `q = floor(p M + 1/2)`, computed exactly in integers from the binary64 value of `p`
//!    (no floating-point rounding; [`coverage_count`]). `r = ceil((M - q) / 2)`, that is
//!    `(M - q) / 2` when `M - q` is even and `(M - q + 1) / 2` when it is odd. The
//!    symmetric interval is `[x_(r), x_(r + q)]` (REQ-UNC.PROP.MCM.INTVL-002). After
//!    validation `1 <= r` and `r + q <= M` always hold.
//! 4. The shortest interval (§7.7.3) is `[x_(s), x_(s + q)]` for the start rank `s` in
//!    `1..=M - q` with the smallest `x_(s + q) - x_(s)`. The lowest `s` wins ties (a strict
//!    `<` scan in ascending `s`): a documented deterministic tie-break.
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

/// `floor(p * m + 1/2)` computed exactly: `p` is a binary64 `mant * 2^e` with `e < 0`
/// for `0 < p < 1`, so the result is `(mant * m + 2^(shift - 1)) >> shift` with
/// `shift = -e`, in 128-bit integers (`mant * m < 2^117`). Requires `0 < p < 1` finite.
pub fn coverage_count(p: f64, m: u64) -> u64 {
    let bits = p.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    let (mant, e) = if exp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), exp - 1075) };
    // p < 1 gives e <= -1, so shift >= 1.
    let shift = (-e) as u32;
    if shift >= 120 {
        // mant * m < 2^117, so p * m < 2^-3 and the rounded count is zero.
        return 0;
    }
    let prod = u128::from(mant) * u128::from(m);
    ((prod + (1u128 << (shift - 1))) >> shift) as u64
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
    let ceil_bound = (1.0 / (1.0 - p)).ceil();
    // `ceil_bound` is at most 2^53 (1 - p >= 2^-53), so the conversion is exact.
    let need = if ceil_bound < 2.0 { 2u64 } else { ceil_bound as u64 };
    if (m_len as u64) < need {
        return Err(McmError::TooFewTrials { m: m_len as u64, min: need });
    }

    let mut xs: Vec<f64> = Vec::new();
    xs.try_reserve_exact(m_len).map_err(|_| McmError::Allocation)?;
    xs.extend_from_slice(values);
    xs.sort_by(|a, b| a.total_cmp(b));

    // Rule 3, in integers. Validation gives M (1 - p) >= 1, hence p M <= M - 1 and
    // q <= M - 1; the bound below is kept anyway so no index can leave the slice.
    let mm = m_len as u64;
    let q_raw = coverage_count(p, mm);
    let q = if q_raw >= mm { mm - 1 } else { q_raw };
    let r = (mm - q).div_ceil(2);
    let sym_lo = r as usize;
    let sym_hi = (r + q) as usize;
    let width = q as usize;

    // Rule 4.
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
