//! Joint Monte Carlo samples of a characteristic group: sample mean, sample covariance and a
//! sample-based coverage region. Spec node `UNC.PROP.MVAR.VMCM`, algorithm
//! [`ALGORITHM_ID`] = `unc.prop.mvar_vmcm@1.0.0` (JCGM 102:2011, the Monte Carlo form of
//! multivariate propagation [unverified: clause 7 numbering]).
//!
//! All outputs of a group are evaluated in the same trial, so their joint sample carries the
//! correlation that shared inputs induce (REQ-UNC.PROP.MVAR-002). [`propagate_vector_mcm`]
//! reads the joint sample from an encoded `mcm-samples@1` set ([`super::super::mcm::samples`],
//! `UNC.PROP.MCM.SAMP`) and returns a [`VectorMcmResult`].
//!
//! The result is a model-based (simulated) Monte Carlo summary, never a measured value; its
//! canonical JSON says so in the required member `"basis":"simulated"` (D-045). Nothing here
//! makes an uncertainty statement or assigns an evidence class.
//!
//! # Failed trials (REQ-UNC.PROP.MVAR.VMCM-001)
//!
//! A trial that fails in `UNC.PROP.MCM.BLOCK` (at any operator, including one that feeds only
//! one output of the group) fails as a whole: it is excluded from every output column, and
//! `mcm-samples@1` keeps one value per output per successful trial plus the failed trial
//! indices. So a trial in which any output failed is excluded for every output of the group.
//! [`VectorMcmResult::excluded`] lists every excluded trial (the set's failed indices, in
//! ascending order; the per-trial failure codes are in the blocks and the run record, not in
//! the set), and [`VectorMcmResult::m`] is the number of trials used, `trials - excluded`.
//!
//! # Rules (pinned; first failure wins)
//!
//! 1. `p` finite with `0 < p < 1`, else [`VmcmError::InvalidP`].
//! 2. The group: `1 ..= MAX_DIM` (7) output IDs, else `invalid_group{size}`; no ID twice,
//!    else `invalid_group{duplicate}` with the index of the second occurrence. The group's
//!    order is the component order of the mean, the covariance and the distances.
//! 3. The bytes decode as `mcm-samples@1` under `cap` (every refusal of
//!    [`super::super::mcm::samples::decode`], as [`VmcmError::Samples`]). `samples_ref` is the
//!    SHA-256 of the bytes; decoding refuses every non-canonical header and every length that
//!    disagrees with the counts, so equal sets have equal references.
//! 4. Every group ID is an output of the set, else `invalid_group{unknown_output}` with its
//!    index in the group.
//! 5. With `k` the group size and `M = trials - failed`: `M >= max(k + 1, n_min)`, where
//!    `n_min` is the smallest `M` with `ceil(p M) < M` (so at least one trial lies outside the
//!    region), computed exactly in integers from the binary64 value of `p`; else
//!    [`VmcmError::TooFewTrials`] with that minimum.
//! 6. Mean of output `i`: `red::mean` of its column (fixed-order sum, one division).
//!    Deviations `x_ir - mean_i`, one rounded subtraction each, in trial order.
//! 7. Covariance: `s_ij = red::dot(dev_i, dev_j) / (M - 1)` for `i <= j`, mirrored, so the
//!    matrix is exactly symmetric. Entry `(i, j)` is in `unit_i * unit_j` of the model's
//!    output nodes; it is the `k = 1` (standard-uncertainty) covariance.
//! 8. The covariance check of `UNC.PROP.MVAR.PARAM` ([`check_covariance`], correlation-scaled):
//!    a refusal is `invalid_covariance` with [`CovarianceRefusal::Defect`]. An accepted matrix
//!    flagged [`CovarianceCheck::rank_deficient`] (a constant output, outputs that are exact
//!    linear combinations of each other, or a smallest scaled eigenvalue at most the PSD
//!    tolerance times the largest) is refused as `invalid_covariance` with
//!    [`CovarianceRefusal::RankDeficient`] and the check: a singular sample covariance has no
//!    Mahalanobis metric, and a flagged covariance is never returned as a confident one.
//! 9. Scaling: `d_i = f::sqrt(s_ii)`, `r_ij = (s_ij / d_i) / d_j`. Pivoted Cholesky of `R`
//!    ([`chol_pivoted`]) with the relative stop tolerance `tol = total_max(PSD_REL_TOL,
//!    16 k 2^-53)` (the check's, never below rounding resolution). A rank below `k` is
//!    `invalid_covariance` with [`CovarianceRefusal::CholeskyRank`].
//! 10. Squared Mahalanobis distance of trial `r` (trial order): `y_i = dev_ir / d_i`,
//!     `b_q = y_perm[q]`, forward substitution `z_q = (b_q - red::dot(L[q][0..q], z[0..q])) /
//!     L[q][q]` for ascending `q`, `d2_r = red::sum_sq(z)`. This equals
//!     `(x_r - mean)^T S^-1 (x_r - mean)` in exact arithmetic (the metric is scale invariant;
//!     scaling keeps mixed units well conditioned). A non-finite value is `numeric`.
//! 11. Region (REQ-UNC.PROP.MVAR.VMCM-002): rank `n_p = ceil(p M)` exactly in integers; the
//!     radius of record `r2_p` is the `n_p`-th smallest `d2` in IEEE-754 total order (its
//!     value does not depend on how ties among the distances are ordered). The region is
//!     `{x : (x - mean)^T S^-1 (x - mean) <= r2_p}`. [`SampleRegion::contained`] counts the
//!     trials with `d2 <= r2_p` (at least `n_p`), [`SampleRegion::at_radius`] those with
//!     `d2 == r2_p` (ties at the boundary, reported, never broken silently); `r_p =
//!     f::sqrt(r2_p)` is the correctly rounded distance for display, `r2_p` stays the value of
//!     record.
//!
//! The rank rule `ceil(p M)` is the spec's [unverified] reading of JCGM 102:2011 and must be
//! checked by an expert. All doubles are normalised so that `-0.0` is stored and written as
//! `+0.0`.
//!
//! # Canonical encoding ([`VectorMcmResult::canonical_json`])
//!
//! RFC 8785 bytes of `{"algorithm","basis":"simulated","cov":[[<f64>...]...],
//! "excluded":[<u64>...],"mean":[<f64>...],"outputs":[<id>...],"region":{"at_radius",
//! "contained","m","p","r2_p","r_p","rank"},"samples_ref","trials"}`, every member required,
//! integers as decimal strings and every double as its 16 lowercase hex digits of IEEE-754
//! bits (the kernel convention). Determinism: D2 (a function of the seeded sample set; D0
//! given the bytes).
//!
//! KNP-1: no FMA, no `f64::min`/`max`, reductions through `fid_num::red`, a NaN is always an
//! error, no hash containers, no panics.

use core::fmt;

use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::sha256_digest;
use fid_num::la::{chol_pivoted, Mat};
use fid_num::{f, red, NumError};
use fid_types::Digest;

use super::param::{check_covariance, CovarianceCheck, CovarianceDefect, ParamError, MAX_DIM, PSD_REL_TOL};
use crate::prop::mcm::samples::decode;
use crate::prop::mcm::McmError;

/// Algorithm ID of this rule set.
pub const ALGORITHM_ID: &str = "unc.prop.mvar_vmcm@1.0.0";

/// The unit roundoff `2^-53`.
const U: f64 = 1.0 / 9_007_199_254_740_992.0;

/// Why a sample covariance was refused (`invalid_covariance`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CovarianceRefusal {
    /// The covariance check of `UNC.PROP.MVAR.PARAM` refused the matrix.
    Defect(CovarianceDefect),
    /// The check accepted the matrix but flagged it rank deficient (rule 8).
    RankDeficient(CovarianceCheck),
    /// The pivoted Cholesky of the scaled matrix stopped at `rank < dim` (rule 9).
    CholeskyRank { rank: usize, dim: usize },
}

/// Errors of [`propagate_vector_mcm`].
#[derive(Debug, Clone, PartialEq)]
pub enum VmcmError {
    /// `invalid_p`: `p` is not finite with `0 < p < 1` (NaN included).
    InvalidP,
    /// `invalid_group`: `detail` is `size`, `duplicate` or `unknown_output`; `index` is the
    /// offending position in the group (`None` for `size`).
    Group { detail: &'static str, index: Option<usize> },
    /// The sample set was refused (`samples_invalid{detail}` and the other decoder errors).
    Samples(McmError),
    /// `too_few_trials`: `m` successful trials, at least `min` needed (rule 5).
    TooFewTrials { m: u64, min: u64 },
    /// `invalid_covariance`: the sample covariance is singular or invalid (rules 8 and 9).
    InvalidCovariance(CovarianceRefusal),
    /// `numeric`: a `fid-num` operation failed (overflow, non-finite intermediate).
    Num(NumError),
    /// `allocation_failed`: a buffer could not be reserved.
    Allocation,
    /// `internal`: an invariant did not hold (`detail` names it).
    Internal { detail: &'static str },
}

impl VmcmError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            VmcmError::InvalidP => "invalid_p",
            VmcmError::Group { .. } => "invalid_group",
            VmcmError::Samples(e) => e.code(),
            VmcmError::TooFewTrials { .. } => "too_few_trials",
            VmcmError::InvalidCovariance(_) => "invalid_covariance",
            VmcmError::Num(_) => "numeric",
            VmcmError::Allocation => "allocation_failed",
            VmcmError::Internal { .. } => "internal",
        }
    }
}

impl From<NumError> for VmcmError {
    fn from(e: NumError) -> Self {
        VmcmError::Num(e)
    }
}

impl fmt::Display for VmcmError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmcmError::InvalidP => write!(out, "invalid_p"),
            VmcmError::Group { detail, index: Some(i) } => write!(out, "invalid_group: {detail} at {i}"),
            VmcmError::Group { detail, index: None } => write!(out, "invalid_group: {detail}"),
            VmcmError::Samples(e) => write!(out, "{e}"),
            VmcmError::TooFewTrials { m, min } => write!(out, "too_few_trials: {m} trials, need at least {min}"),
            VmcmError::InvalidCovariance(CovarianceRefusal::Defect(d)) => {
                write!(out, "invalid_covariance: {}", ParamError::InvalidCovariance(*d))
            }
            VmcmError::InvalidCovariance(CovarianceRefusal::RankDeficient(c)) => {
                write!(out, "invalid_covariance: rank deficient (condition {:e})", c.cond)
            }
            VmcmError::InvalidCovariance(CovarianceRefusal::CholeskyRank { rank, dim }) => {
                write!(out, "invalid_covariance: Cholesky rank {rank} of {dim}")
            }
            VmcmError::Num(e) => write!(out, "numeric: {e}"),
            VmcmError::Allocation => write!(out, "allocation_failed"),
            VmcmError::Internal { detail } => write!(out, "internal: {detail}"),
        }
    }
}

impl core::error::Error for VmcmError {}

fn alloc<T>(v: &mut Vec<T>, n: usize) -> Result<(), VmcmError> {
    v.try_reserve_exact(n).map_err(|_| VmcmError::Allocation)
}

fn group_err(detail: &'static str, index: Option<usize>) -> VmcmError {
    VmcmError::Group { detail, index }
}

fn internal(detail: &'static str) -> VmcmError {
    VmcmError::Internal { detail }
}

/// `-0.0` to `+0.0`; every other value unchanged.
fn pos_zero(x: f64) -> f64 {
    x + 0.0
}

/// The sample-based coverage region (rule 11). Fields are private: only
/// [`propagate_vector_mcm`] makes one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleRegion {
    p: f64,
    m: u64,
    rank: u64,
    r2_p: f64,
    r_p: f64,
    contained: u64,
    at_radius: u64,
}

impl SampleRegion {
    /// The coverage probability.
    pub fn p(&self) -> f64 {
        self.p
    }

    /// Trials used, `M`.
    pub fn m(&self) -> u64 {
        self.m
    }

    /// The 1-based rank `ceil(p M)` of the radius.
    pub fn rank(&self) -> u64 {
        self.rank
    }

    /// The squared Mahalanobis radius of record.
    pub fn r2_p(&self) -> f64 {
        self.r2_p
    }

    /// `f::sqrt(r2_p)`, correctly rounded (for display).
    pub fn r_p(&self) -> f64 {
        self.r_p
    }

    /// Trials with squared distance `<= r2_p` (at least [`Self::rank`]).
    pub fn contained(&self) -> u64 {
        self.contained
    }

    /// Trials with squared distance exactly `r2_p` (ties at the boundary).
    pub fn at_radius(&self) -> u64 {
        self.at_radius
    }
}

/// The joint Monte Carlo result of a group (`VectorMcmResult` of the Design). Fields are
/// private: only [`propagate_vector_mcm`] makes one, so they always agree.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorMcmResult {
    outputs: Vec<String>,
    mean: Vec<f64>,
    cov: Mat,
    check: CovarianceCheck,
    region: SampleRegion,
    trials: u64,
    excluded: Vec<u64>,
    samples_ref: Digest,
}

impl VectorMcmResult {
    /// The group's output IDs, in group (component) order.
    pub fn outputs(&self) -> &[String] {
        &self.outputs
    }

    /// The sample mean per component.
    pub fn mean(&self) -> &[f64] {
        &self.mean
    }

    /// The `k x k` sample covariance (exactly symmetric, `k = 1` standard-uncertainty form).
    pub fn cov(&self) -> &Mat {
        &self.cov
    }

    /// What the covariance check measured (never flagged rank deficient here).
    pub fn check(&self) -> &CovarianceCheck {
        &self.check
    }

    /// The coverage region.
    pub fn region(&self) -> &SampleRegion {
        &self.region
    }

    /// Trials in the run, excluded ones included.
    pub fn trials(&self) -> u64 {
        self.trials
    }

    /// Trials used, `trials - excluded`.
    pub fn m(&self) -> u64 {
        self.region.m
    }

    /// Excluded trials (failed in the run), ascending global indices.
    pub fn excluded(&self) -> &[u64] {
        &self.excluded
    }

    /// SHA-256 of the `mcm-samples@1` bytes the result was computed from.
    pub fn samples_ref(&self) -> Digest {
        self.samples_ref
    }

    /// RFC 8785 bytes (see the module documentation).
    pub fn canonical_json(&self) -> Result<Vec<u8>, VmcmError> {
        let hex = |x: f64| Json::Str(format!("{:016x}", x.to_bits()));
        let int = |x: u64| Json::Str(x.to_string());
        let k = self.outputs.len();
        let mut cov = Vec::new();
        alloc(&mut cov, k)?;
        for i in 0..k {
            cov.push(Json::Arr(self.cov.row(i)?.iter().map(|&x| hex(x)).collect()));
        }
        let mut excluded = Vec::new();
        alloc(&mut excluded, self.excluded.len())?;
        excluded.extend(self.excluded.iter().map(|&t| int(t)));
        let r = &self.region;
        let region = Json::Obj(vec![
            ("at_radius".to_owned(), int(r.at_radius)),
            ("contained".to_owned(), int(r.contained)),
            ("m".to_owned(), int(r.m)),
            ("p".to_owned(), hex(r.p)),
            ("r2_p".to_owned(), hex(r.r2_p)),
            ("r_p".to_owned(), hex(r.r_p)),
            ("rank".to_owned(), int(r.rank)),
        ]);
        let doc = Json::Obj(vec![
            ("algorithm".to_owned(), Json::Str(ALGORITHM_ID.to_owned())),
            ("basis".to_owned(), Json::Str("simulated".to_owned())),
            ("cov".to_owned(), Json::Arr(cov)),
            ("excluded".to_owned(), Json::Arr(excluded)),
            ("mean".to_owned(), Json::Arr(self.mean.iter().map(|&x| hex(x)).collect())),
            ("outputs".to_owned(), Json::Arr(self.outputs.iter().map(|s| Json::Str(s.clone())).collect())),
            ("region".to_owned(), region),
            ("samples_ref".to_owned(), Json::Str(self.samples_ref.as_str().to_owned())),
            ("trials".to_owned(), int(self.trials)),
        ]);
        to_canonical(&doc).map_err(|_| internal("result_encoding"))
    }
}

/// `p = mant / 2^shift` with `shift >= 1`, for finite `0 < p < 1`.
fn split(p: f64) -> (u64, u32) {
    let bits = p.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    let (mant, e) = if exp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), exp - 1075) };
    (mant, e.unsigned_abs())
}

/// `ceil(p m)` exactly, for finite `0 < p < 1` and `m >= 1`.
fn ceil_count(p: f64, m: u64) -> u64 {
    let (mant, shift) = split(p);
    if shift >= 120 {
        // mant * m < 2^117 < 2^shift, so 0 < p m < 1.
        return 1;
    }
    let prod = u128::from(mant) * u128::from(m);
    let den = 1u128 << shift;
    let q = prod >> shift;
    let q = if prod & (den - 1) == 0 { q } else { q + 1 };
    u64::try_from(q).unwrap_or(u64::MAX)
}

/// The smallest `M` with `ceil(p M) < M`, that is `ceil(1 / (1 - p))`, exactly.
fn region_min(p: f64) -> u64 {
    let (mant, shift) = split(p);
    if shift > 125 {
        // p < 2^-72, so 1 < 1 / (1 - p) < 2.
        return 2;
    }
    let num = 1u128 << shift;
    let den = num - u128::from(mant);
    let q = num.div_ceil(den);
    u64::try_from(q).unwrap_or(u64::MAX)
}

/// Computes the joint sample mean, covariance and coverage region of the group `group` of
/// the `mcm-samples@1` set `samples` (decoded under `cap`) at coverage probability `p`
/// (REQ-UNC.PROP.MVAR.VMCM-001, -002; rules in the module documentation).
pub fn propagate_vector_mcm(samples: &[u8], cap: u64, group: &[&str], p: f64) -> Result<VectorMcmResult, VmcmError> {
    // Rule 1.
    if !p.is_finite() || p <= 0.0 || p >= 1.0 {
        return Err(VmcmError::InvalidP);
    }
    // Rule 2.
    let k = group.len();
    if k == 0 || k > MAX_DIM {
        return Err(group_err("size", None));
    }
    for (i, id) in group.iter().enumerate() {
        if group.iter().take(i).any(|g| g == id) {
            return Err(group_err("duplicate", Some(i)));
        }
    }
    // Rule 3.
    let set = decode(samples, cap).map_err(VmcmError::Samples)?;
    let samples_ref = sha256_digest(samples);
    // Rule 4.
    let mut cols: Vec<&[f64]> = Vec::new();
    alloc(&mut cols, k)?;
    for (i, id) in group.iter().enumerate() {
        let at = set
            .header
            .outputs
            .binary_search_by(|o| o.as_bytes().cmp(id.as_bytes()))
            .map_err(|_| group_err("unknown_output", Some(i)))?;
        cols.push(set.outputs.get(at).ok_or(internal("column"))?);
    }
    // Rule 5.
    let m = set.header.trials.checked_sub(set.header.failed).ok_or(internal("counts"))?;
    let k64 = u64::try_from(k).map_err(|_| internal("group_size"))?;
    let min = region_min(p).max(k64 + 1);
    if m < min {
        return Err(VmcmError::TooFewTrials { m, min });
    }
    let mu = usize::try_from(m).map_err(|_| VmcmError::Allocation)?;
    if cols.iter().any(|c| c.len() != mu) {
        return Err(internal("column_length"));
    }

    // Rule 6.
    let mut mean = Vec::new();
    alloc(&mut mean, k)?;
    let mut dev: Vec<Vec<f64>> = Vec::new();
    alloc(&mut dev, k)?;
    for c in &cols {
        let mc = pos_zero(red::mean(c)?);
        let mut d = Vec::new();
        alloc(&mut d, mu)?;
        d.extend(c.iter().map(|&x| x - mc));
        mean.push(mc);
        dev.push(d);
    }

    // Rule 7. M - 1 converts exactly up to 2^53 trials (8 * 2^53 bytes per output column,
    // far beyond any retained set); `red::mean` divides by the length the same way.
    let denom = (m - 1) as f64;
    let mut s = vec![0.0; k * k];
    for i in 0..k {
        for j in i..k {
            let (a, b) = (dev.get(i).ok_or(internal("dev"))?, dev.get(j).ok_or(internal("dev"))?);
            let x = pos_zero(red::dot(a, b)? / denom);
            for at in [i * k + j, j * k + i] {
                *s.get_mut(at).ok_or(internal("cov"))? = x;
            }
        }
    }
    let cov = Mat::new(k, k, s)?;

    // Rule 8.
    let (cov, check) = check_covariance(&cov, k).map_err(|e| match e {
        ParamError::InvalidCovariance(d) => VmcmError::InvalidCovariance(CovarianceRefusal::Defect(d)),
        ParamError::Num(n) => VmcmError::Num(n),
        _ => internal("covariance_check"),
    })?;
    if check.rank_deficient {
        return Err(VmcmError::InvalidCovariance(CovarianceRefusal::RankDeficient(check)));
    }

    // Rule 9.
    let mut scale = [1.0_f64; MAX_DIM];
    for (i, slot) in scale.iter_mut().enumerate().take(k) {
        *slot = f::sqrt(cov.get(i, i)?)?;
    }
    let d = |i: usize| scale.get(i).copied().ok_or(internal("scale"));
    let mut r = vec![0.0; k * k];
    for i in 0..k {
        for j in 0..k {
            *r.get_mut(i * k + j).ok_or(internal("scaled"))? = (cov.get(i, j)? / d(i)?) / d(j)?;
        }
    }
    let tol = f::total_max(PSD_REL_TOL, 16.0 * (k as f64) * U)?;
    let chol = chol_pivoted(&Mat::new(k, k, r)?, tol)?;
    if chol.rank() < k {
        return Err(VmcmError::InvalidCovariance(CovarianceRefusal::CholeskyRank { rank: chol.rank(), dim: k }));
    }
    let l = chol.l();
    let perm = chol.perm();

    // Rule 10.
    let mut d2 = Vec::new();
    alloc(&mut d2, mu)?;
    let mut z = [0.0_f64; MAX_DIM];
    for t in 0..mu {
        for q in 0..k {
            let src = *perm.get(q).ok_or(internal("perm"))?;
            let y = *dev.get(src).and_then(|c| c.get(t)).ok_or(internal("dev"))? / d(src)?;
            let row = l.row(q)?;
            let lq = row.get(..q).ok_or(internal("chol_row"))?;
            let dot = red::dot(lq, z.get(..q).ok_or(internal("z"))?)?;
            let zq = (y - dot) / *row.get(q).ok_or(internal("chol_diag"))?;
            if !zq.is_finite() {
                return Err(VmcmError::Num(NumError::Infinite { op: "vmcm.distance", index: Some(t) }));
            }
            *z.get_mut(q).ok_or(internal("z"))? = zq;
        }
        d2.push(red::sum_sq(z.get(..k).ok_or(internal("z"))?)?);
    }

    // Rule 11.
    let rank = ceil_count(p, m);
    let at = usize::try_from(rank).map_err(|_| internal("rank"))?.checked_sub(1).ok_or(internal("rank"))?;
    if at >= mu {
        return Err(internal("rank"));
    }
    let mut order = Vec::new();
    alloc(&mut order, mu)?;
    order.extend_from_slice(&d2);
    let (_, &mut r2_p, _) = order.select_nth_unstable_by(at, |a, b| a.total_cmp(b));
    let contained = d2.iter().filter(|&&x| x <= r2_p).count() as u64;
    let at_radius = d2.iter().filter(|&&x| x == r2_p).count() as u64;
    let r_p = f::sqrt(r2_p)?;

    let mut outputs = Vec::new();
    alloc(&mut outputs, k)?;
    outputs.extend(group.iter().map(|s| (*s).to_owned()));
    Ok(VectorMcmResult {
        outputs,
        mean,
        cov,
        check,
        region: SampleRegion { p, m, rank, r2_p, r_p, contained, at_radius },
        trials: set.header.trials,
        excluded: set.failed,
        samples_ref,
    })
}

#[cfg(test)]
mod tests {
    use super::{ceil_count, region_min};

    #[test]
    fn exact_counts() {
        assert_eq!(ceil_count(0.95, 20), 19);
        assert_eq!(ceil_count(0.95, 21), 20);
        assert_eq!(ceil_count(0.5, 10), 5);
        assert_eq!(ceil_count(0.5, 11), 6);
        assert_eq!(ceil_count(f64::MIN_POSITIVE, u64::MAX), 1);
        // 0.1 is slightly above 1/10 in binary64, so ceil(0.1 * 10) is 2, not 1.
        assert_eq!(ceil_count(0.1, 10), 2);
        assert_eq!(region_min(0.5), 2);
        assert_eq!(region_min(0.75), 4);
        // 0.95 is slightly below 19/20, so 1 / (1 - p) is just under 20.
        assert_eq!(region_min(0.95), 20);
        assert_eq!(region_min(f64::MIN_POSITIVE), 2);
        // 0.9 is slightly above 9/10, so 1 / (1 - p) is just over 10.
        assert_eq!(region_min(0.9), 11);
    }
}
