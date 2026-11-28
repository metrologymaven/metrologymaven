//! Contributor ranking. Spec node `UNC.PROP.RANK`, algorithm [`ALGORITHM_ID`] =
//! `unc.prop.rank@1.0.0`.
//!
//! Ranks the inputs of an evaluated model by how much each contributes to the output
//! uncertainty. [`rank`] takes either the LPU sensitivities and the input covariance
//! ([`RankInput::Lpu`]) or the Monte Carlo input and output samples
//! ([`RankInput::Samples`]) and returns a [`RankOutput`]. Computing the sensitivities and
//! the samples is `UNC.PROP.LPU` and `UNC.PROP.MCM`; showing the ranking is `UNC.UEC.EXPL`.
//!
//! # LPU form (REQ-UNC.PROP.RANK-001, -004; rules pinned, first failure wins)
//!
//! With `n` inputs, sensitivities `c` and the covariance `S` given row-major as `n * n`
//! values:
//!
//! 1. `n >= 1`, else `no_inputs`. `ids` has `n` entries and `cov` has `n * n`, else
//!    `dimension_mismatch` naming `ids` or `cov`.
//! 2. Every ID is a token (1 to 256 printable ASCII bytes, no space, quote or backslash; the
//!    statement's rule, so the IDs of a ranking always convert), else `invalid_id{index}`; no ID
//!    twice, else `duplicate_id` with the lowest index that repeats an earlier ID.
//! 3. `c`, then `cov`, finite, else `numeric` with the `fid-num` error.
//! 4. `S` exactly symmetric (`S_ij == S_ji` as values), else `not_symmetric{i, j}` for the
//!    first pair `i < j` in row-major order. Every `S_ii >= 0`, else
//!    `negative_variance{index: i}`.
//!    Then `S` must be positive semi-definite, else `not_positive_semidefinite{i, j}`:
//!    - Cauchy-Schwarz, for every pair `i < j` in row-major order, with `s = S_ij`: refused
//!      when `s != 0` and `S_ii == 0` or `S_jj == 0`, or when
//!      `s * s > (S_ii * S_jj) * (1 + 4 * 2^-53)`. That comparison is evaluated as written
//!      when both sides are finite and at least `2^-1022`; otherwise as
//!      `q * q > 1 + 4 * 2^-53` with `q = (|s| / sqrt(S_ii)) / sqrt(S_jj)`, so an overflow or
//!      an underflow never decides the test.
//!    - Pivoted Cholesky `fid_num::la::chol_pivoted(S, 0)` (`n` at most `la::MAX_DIM` = 4096,
//!      else `numeric`). If it stops at rank `r < n`, every remaining Schur-complement entry
//!      `s_ij = S[p_i][p_j] - red::dot(L_i[0..r], L_j[0..r])` (positions `r <= i <= j < n`,
//!      `p` the pivot permutation) must satisfy `|s_ij| <= e_ij`, where
//!      `e_ij = (4 (r + 1) 2^-53) (|S[p_i][p_j]| + red::dot(|L_i[0..r]|, |L_j[0..r]|))`
//!      plus `(4 r + 4) 2^-1074`; else the pair of original indices is named, smaller
//!      first. A singular positive semi-definite matrix leaves only rounding there; an
//!      indefinite one that the pairwise test misses (three inputs with pairwise
//!      correlations 0.9, 0.9 and -0.9) leaves an entry beyond the bound. The factorization
//!      is `O(n^3)` for an input of `n^2` values.
//! 5. `u_i(y) = |c_i| * sqrt(S_ii)` (correctly rounded `f::sqrt`, one rounded product).
//! 6. `w_i = red::dot(S_i, c)` for ascending `i`, `v = red::dot(c, w)` (`v` is `c^T S c`).
//!    The rounding tolerance of `v` is `tol = (4 (n + 1) 2^-53) a + (4 n + 4) 2^-1074` with
//!    `a = red::dot(|c|, b)`, `b_i = red::dot(|S_i|, |c|)`: the relative part bounds the
//!    rounding error of the two dot products in the normal range, the absolute part the
//!    error of products that underflow (each is off by up to `2^-1075`). So the zero test of
//!    REQ-004 is never tighter than the arithmetic can resolve, and a subnormal `v` (for
//!    `c = (1e-155, 9.9999998e-156)` with perfectly anti-correlated unit inputs) is degenerate,
//!    not a confident pair of shares near `+-4e5`.
//! 7. `v < -tol` is `negative_variance{index: None}` (a final guard behind the tests of rule 4).
//!    `v <= tol` is degenerate (REQ-004): [`Degenerate::ZeroVariance`] when `v == 0`, else
//!    [`Degenerate::VarianceAtRoundingResolution`]; every share is absent, nothing is divided
//!    by `v`, and [`Shares::Degenerate`] says why.
//! 8. Otherwise `share_i = (c_i * w_i) / v` (the correlation-aware Euler share
//!    `c_i (S c)_i / (c^T S c)`; it is negative when an input's correlation with the others
//!    reduces the output variance) and `share_sum = red::sum(shares)`. If
//!    `|share_sum - 1| > 1e-12` ([`SHARE_SUM_TOL`]) the shares are dropped as
//!    [`Shares::SumOutOfTolerance`] with `share_sum` still reported; else
//!    [`Shares::Present`].
//!
//! # Samples form (REQ-UNC.PROP.RANK-002, -004; rules pinned, first failure wins)
//!
//! 1. At least one input, else `no_inputs`; `ids` has one entry per input, else
//!    `dimension_mismatch{ids}`; at least 2 output samples `M`, else `too_few_samples`; every
//!    input has `M` samples (trial `r` of every column is the same trial), else
//!    `dimension_mismatch{input, index}`.
//! 2. IDs as in LPU rule 2; the output, then each input in order, finite, else `numeric`.
//! 3. Output: a column whose values all equal its first value, or whose deviations have a
//!    zero sum of squares, is degenerate ([`Degenerate::ZeroOutputVariance`]): every input's
//!    `r2` is absent ([`ScoreAbsent::DegenerateOutput`]). A bitwise-constant column is caught
//!    before any mean, because the rounded mean of a constant (three values `0.1`) need not
//!    equal it and would leave pure rounding deviations.
//! 4. Otherwise, per column: `m = red::mean(x)`, deviations `d_r = x_r - m` in trial order (one
//!    rounded subtraction each; an overflow is `numeric`), `s_xx = red::sum_sq(d)`. An input
//!    column that is constant or has `s_xx == 0` gets no `r2` ([`ScoreAbsent::ZeroInputVariance`]).
//!    A sum of squares below `2^-1022` (the output's `s_yy` for every input, or an input's
//!    `s_xx`) holds subnormal squares whose relative error is unbounded, so that `r2` is absent
//!    too ([`ScoreAbsent::BelowRoundingResolution`]).
//! 5. `s_xy = red::dot(d_x, d_y)`, `r = (s_xy / sqrt(s_xx)) / sqrt(s_yy)` (this order cannot
//!    overflow, since `|s_xy| <= sqrt(s_xx s_yy)`), `r2 = r * r`, and a rounded `r2` above 1
//!    is reported as 1 (the exact value is at most 1).
//!
//! The result is labelled `method = approximate_src2` ([`RankMethod::ApproximateSrc2`]): the
//! squared Pearson correlation equals the variance share only for a linear model with
//! independent inputs. Its canonical JSON also carries the required member
//! `"basis":"simulated"`, so a sample-based ranking never reads as measured (D-045).
//!
//! # Order (REQ-UNC.PROP.RANK-003)
//!
//! Descending key (`u_i(y)` for LPU, `r2` for samples, an absent `r2` after every present
//! one) in IEEE-754 total order, ties by input ID bytes ascending. Every tie is reported: an
//! entry whose key equals the previous entry's key has [`Ranked::tie_broken_by_id`] set.
//!
//! # Canonical encoding ([`RankOutput::canonical_json`])
//!
//! RFC 8785 bytes, every member required, every double as its 16 lowercase hex digits of
//! IEEE-754 bits, integers as decimal strings, a missing value as `"absent"`:
//!
//! - LPU: `{"algorithm","contributors":[{"input_id","rank","sensitivity","share",
//!   "tie_broken_by_id","u_contrib"}...],"degenerate","method":"lpu","share_sum","shares"}`
//! - samples: `{"algorithm","basis":"simulated","contributors":[{"input_id","r2","r2_absent",
//!   "rank","tie_broken_by_id"}...],"degenerate","method":"approximate_src2","trials"}`
//!
//! `degenerate` is `none` or a [`Degenerate`] code; `rank` is the 1-based position.
//!
//! # Into a statement ([`RankOutput::statement_contributors`])
//!
//! `UncertaintyStatement` contributors hold a variance share in `[0, 1]`, sorted by input ID.
//! Only an LPU ranking with [`Shares::Present`] and every share in `[0, 1]` converts; a
//! sample-based ranking (`approximate_src2`, no variance share), absent shares and a
//! negative share are refused as `not_convertible`, never clamped.
//!
//! Every output double is normalised so that `-0.0` is `+0.0`. Determinism: D0. KNP-1: no
//! FMA, no `f64::min`/`max`, reductions through `fid_num::red`, a NaN is always an error, no
//! hash containers, no panics.

use core::cmp::Ordering;
use core::fmt;

use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::sha256_digest;
use fid_num::la::{chol_pivoted, Mat};
use fid_num::{check_finite_slice, f, red, NumError};
use fid_types::Digest;

use crate::statement::{is_token, Contributor};

/// Algorithm ID of this ranking (spec card `unc.prop.rank@1.0.0.toml`).
pub const ALGORITHM_ID: &str = "unc.prop.rank@1.0.0";

/// Largest accepted `|share_sum - 1|` (REQ-UNC.PROP.RANK-001).
pub const SHARE_SUM_TOL: f64 = 1e-12;

/// The unit roundoff `2^-53`.
const U: f64 = 1.0 / 9_007_199_254_740_992.0;

/// The smallest subnormal, `2^-1074`.
const TINY: f64 = f64::from_bits(1);

/// Bits of `1.0`.
const ONE_BITS: u64 = 0x3ff0_0000_0000_0000;

/// What [`rank`] ranks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RankInput<'a> {
    /// LPU: sensitivities `c`, the input covariance `cov` row-major (`n * n`, exactly
    /// symmetric) and one ID per input.
    Lpu { c: &'a [f64], cov: &'a [f64], ids: &'a [&'a str] },
    /// Monte Carlo: one sample column per input, the output column (trial `r` of each column
    /// is the same trial) and one ID per input.
    Samples { inputs: &'a [&'a [f64]], output: &'a [f64], ids: &'a [&'a str] },
}

/// How the ranking was computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankMethod {
    /// From LPU sensitivities and the input covariance.
    Lpu,
    /// Squared Pearson correlation of samples; an approximation (REQ-002).
    ApproximateSrc2,
}

impl RankMethod {
    /// Stable code: `lpu` or `approximate_src2`.
    pub const fn code(self) -> &'static str {
        match self {
            RankMethod::Lpu => "lpu",
            RankMethod::ApproximateSrc2 => "approximate_src2",
        }
    }
}

/// Why a ranking is degenerate (REQ-UNC.PROP.RANK-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Degenerate {
    /// LPU: `c^T S c` is exactly zero.
    ZeroVariance,
    /// LPU: `c^T S c` is within the rounding tolerance of zero (module rule 6).
    VarianceAtRoundingResolution,
    /// Samples: the output column is constant or its deviations have a zero sum of squares.
    ZeroOutputVariance,
}

impl Degenerate {
    /// Stable code.
    pub const fn code(self) -> &'static str {
        match self {
            Degenerate::ZeroVariance => "zero_variance",
            Degenerate::VarianceAtRoundingResolution => "variance_at_rounding_resolution",
            Degenerate::ZeroOutputVariance => "zero_output_variance",
        }
    }
}

/// Whether an LPU ranking carries variance shares, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shares {
    /// Every contributor has a share and the shares sum to 1 within [`SHARE_SUM_TOL`].
    Present,
    /// The ranking is degenerate; no share was computed.
    Degenerate,
    /// The shares were computed but their sum is not within [`SHARE_SUM_TOL`] of 1; they are
    /// dropped and [`RankOutput::share_sum`] reports the sum.
    SumOutOfTolerance,
}

impl Shares {
    /// Stable code.
    pub const fn code(self) -> &'static str {
        match self {
            Shares::Present => "present",
            Shares::Degenerate => "degenerate",
            Shares::SumOutOfTolerance => "sum_out_of_tolerance",
        }
    }
}

/// Why a sample-based contributor has no `r2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreAbsent {
    /// The input column is constant or its deviations have a zero sum of squares.
    ZeroInputVariance,
    /// The output is degenerate ([`Degenerate::ZeroOutputVariance`]).
    DegenerateOutput,
    /// The input's `s_xx` or the output's `s_yy` is positive but below `2^-1022`.
    BelowRoundingResolution,
}

impl ScoreAbsent {
    /// Stable code.
    pub const fn code(self) -> &'static str {
        match self {
            ScoreAbsent::ZeroInputVariance => "zero_input_variance",
            ScoreAbsent::DegenerateOutput => "degenerate_output",
            ScoreAbsent::BelowRoundingResolution => "below_rounding_resolution",
        }
    }
}

/// One ranked contributor.
#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    input_id: String,
    sensitivity: Option<f64>,
    u_contrib: Option<f64>,
    share: Option<f64>,
    r2: Option<f64>,
    r2_absent: Option<ScoreAbsent>,
    tie_broken_by_id: bool,
}

impl Ranked {
    fn lpu(input_id: &str, sensitivity: f64, u_contrib: f64, share: Option<f64>) -> Ranked {
        Ranked {
            input_id: input_id.to_owned(),
            sensitivity: Some(sensitivity),
            u_contrib: Some(u_contrib),
            share,
            r2: None,
            r2_absent: None,
            tie_broken_by_id: false,
        }
    }

    fn samples(input_id: &str, r2: Option<f64>, r2_absent: Option<ScoreAbsent>) -> Ranked {
        Ranked {
            input_id: input_id.to_owned(),
            sensitivity: None,
            u_contrib: None,
            share: None,
            r2,
            r2_absent,
            tie_broken_by_id: false,
        }
    }

    /// The model input ID.
    pub fn input_id(&self) -> &str {
        &self.input_id
    }

    /// LPU: the sensitivity coefficient `c_i`; `None` for a sample-based ranking.
    pub fn sensitivity(&self) -> Option<f64> {
        self.sensitivity
    }

    /// LPU: `u_i(y) = |c_i| sqrt(S_ii)`; `None` for a sample-based ranking.
    pub fn u_contrib(&self) -> Option<f64> {
        self.u_contrib
    }

    /// LPU: the Euler variance share, when [`RankOutput::shares`] is [`Shares::Present`].
    pub fn share(&self) -> Option<f64> {
        self.share
    }

    /// Samples: the squared Pearson correlation with the output, unless
    /// [`Ranked::r2_absent`] says why not.
    pub fn r2(&self) -> Option<f64> {
        self.r2
    }

    /// Samples: why [`Ranked::r2`] is absent.
    pub fn r2_absent(&self) -> Option<ScoreAbsent> {
        self.r2_absent
    }

    /// True when this entry's key equals the previous entry's, so the input ID decided their
    /// order (REQ-003).
    pub fn tie_broken_by_id(&self) -> bool {
        self.tie_broken_by_id
    }
}

/// The result of [`rank`]. Fields are private: only [`rank`] builds one.
#[derive(Debug, Clone, PartialEq)]
pub struct RankOutput {
    method: RankMethod,
    contributors: Vec<Ranked>,
    degenerate: Option<Degenerate>,
    shares: Option<Shares>,
    share_sum: Option<f64>,
    trials: Option<u64>,
}

impl RankOutput {
    /// How the ranking was computed.
    pub fn method(&self) -> RankMethod {
        self.method
    }

    /// The contributors in rank order (REQ-003).
    pub fn contributors(&self) -> &[Ranked] {
        &self.contributors
    }

    /// The `degenerate` flag of REQ-004, with its reason.
    pub fn degenerate(&self) -> Option<Degenerate> {
        self.degenerate
    }

    /// LPU: whether the shares are present; `None` for a sample-based ranking.
    pub fn shares(&self) -> Option<Shares> {
        self.shares
    }

    /// LPU: the fixed-order sum of the shares, whenever they were computed (also when they
    /// were dropped as [`Shares::SumOutOfTolerance`]).
    pub fn share_sum(&self) -> Option<f64> {
        self.share_sum
    }

    /// Samples: the number of trials `M`.
    pub fn trials(&self) -> Option<u64> {
        self.trials
    }

    /// RFC 8785 bytes of the ranking (see the module documentation).
    pub fn canonical_json(&self) -> Result<Vec<u8>, RankError> {
        let hex = |x: f64| Json::Str(format!("{:016x}", x.to_bits()));
        let opt = |x: Option<f64>| x.map_or_else(|| Json::Str("absent".to_owned()), hex);
        let s = |x: &str| Json::Str(x.to_owned());
        let flag = |b: bool| s(if b { "true" } else { "false" });
        let degenerate = s(self.degenerate.map_or("none", Degenerate::code));
        let mut cs = Vec::with_capacity(self.contributors.len());
        let mut fields = Vec::with_capacity(7);
        fields.push(("algorithm".to_owned(), s(ALGORITHM_ID)));
        for (i, r) in self.contributors.iter().enumerate() {
            let rank = Json::Str((i + 1).to_string());
            let members = match self.method {
                RankMethod::Lpu => vec![
                    ("input_id".to_owned(), s(&r.input_id)),
                    ("rank".to_owned(), rank),
                    ("sensitivity".to_owned(), opt(r.sensitivity)),
                    ("share".to_owned(), opt(r.share)),
                    ("tie_broken_by_id".to_owned(), flag(r.tie_broken_by_id)),
                    ("u_contrib".to_owned(), opt(r.u_contrib)),
                ],
                RankMethod::ApproximateSrc2 => vec![
                    ("input_id".to_owned(), s(&r.input_id)),
                    ("r2".to_owned(), opt(r.r2)),
                    ("r2_absent".to_owned(), s(r.r2_absent.map_or("none", ScoreAbsent::code))),
                    ("rank".to_owned(), rank),
                    ("tie_broken_by_id".to_owned(), flag(r.tie_broken_by_id)),
                ],
            };
            cs.push(Json::Obj(members));
        }
        fields.push(("contributors".to_owned(), Json::Arr(cs)));
        fields.push(("degenerate".to_owned(), degenerate));
        fields.push(("method".to_owned(), s(self.method.code())));
        match self.method {
            RankMethod::Lpu => {
                fields.push(("share_sum".to_owned(), opt(self.share_sum)));
                fields.push(("shares".to_owned(), s(self.shares.map_or("absent", Shares::code))));
            }
            RankMethod::ApproximateSrc2 => {
                fields.push(("basis".to_owned(), s("simulated")));
                let trials = self.trials.map_or_else(|| "absent".to_owned(), |t| t.to_string());
                fields.push(("trials".to_owned(), Json::Str(trials)));
            }
        }
        to_canonical(&Json::Obj(fields)).map_err(|_| RankError::Encoding)
    }

    /// SHA-256 of [`RankOutput::canonical_json`].
    pub fn digest(&self) -> Result<Digest, RankError> {
        Ok(sha256_digest(&self.canonical_json()?))
    }

    /// The contributors as `UncertaintyStatement` contributors (sorted by input ID). Only an
    /// LPU ranking with [`Shares::Present`] and every share in `[0, 1]` converts; anything
    /// else is `not_convertible` with the reason `approximate_src2`, `shares_absent` or
    /// `share_outside_unit_interval` (naming the input).
    pub fn statement_contributors(&self) -> Result<Vec<Contributor>, RankError> {
        if self.method != RankMethod::Lpu {
            return Err(RankError::NotConvertible { reason: "approximate_src2", input_id: None });
        }
        if self.shares != Some(Shares::Present) {
            return Err(RankError::NotConvertible { reason: "shares_absent", input_id: None });
        }
        let mut out = Vec::with_capacity(self.contributors.len());
        for r in &self.contributors {
            let (Some(u_contrib), Some(share)) = (r.u_contrib, r.share) else {
                return Err(RankError::NotConvertible { reason: "shares_absent", input_id: Some(r.input_id.clone()) });
            };
            if !(share >= 0.0 && share.to_bits() <= ONE_BITS) {
                return Err(RankError::NotConvertible {
                    reason: "share_outside_unit_interval",
                    input_id: Some(r.input_id.clone()),
                });
            }
            out.push(Contributor {
                input_id: r.input_id.clone(),
                sensitivity: r.sensitivity,
                u_contrib,
                variance_share: share,
            });
        }
        out.sort_by(|a, b| a.input_id.as_bytes().cmp(b.input_id.as_bytes()));
        Ok(out)
    }
}

/// Errors of [`rank`] and [`RankOutput::statement_contributors`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RankError {
    /// `no_inputs`: there is nothing to rank.
    NoInputs,
    /// `dimension_mismatch`: `what` (`ids`, `cov` or `input`) has `found` entries where
    /// `expected` are needed; `index` names the input column for `input`.
    DimensionMismatch { what: &'static str, expected: usize, found: usize, index: Option<usize> },
    /// `invalid_id`: the ID at `index` is not a token.
    InvalidId { index: usize },
    /// `duplicate_id`: the ID at `index` repeats an earlier one.
    DuplicateId { index: usize },
    /// `too_few_samples`: fewer than 2 output samples.
    TooFewSamples { found: usize },
    /// `not_symmetric`: `S_ij != S_ji` for this first pair `i < j`.
    NotSymmetric { i: usize, j: usize },
    /// `negative_variance`: `S_ii < 0` at `index`, or (`index: None`) `c^T S c` below minus
    /// its rounding tolerance, so `S` is not positive semi-definite.
    NegativeVariance { index: Option<usize> },
    /// `not_positive_semidefinite`: the pair `(i, j)` (`i <= j`, original indices) fails the
    /// Cauchy-Schwarz test or leaves a Cholesky remainder beyond rounding (module rule 4).
    NotPositiveSemidefinite { i: usize, j: usize },
    /// `numeric`: a `fid-num` error; `input` names the input (index) it concerns, if one.
    Numeric { input: Option<usize>, error: NumError },
    /// `not_convertible`: the ranking cannot become statement contributors.
    NotConvertible { reason: &'static str, input_id: Option<String> },
    /// `encoding`: the canonical JSON writer refused the document (not expected).
    Encoding,
}

impl RankError {
    /// Stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            RankError::NoInputs => "no_inputs",
            RankError::DimensionMismatch { .. } => "dimension_mismatch",
            RankError::InvalidId { .. } => "invalid_id",
            RankError::DuplicateId { .. } => "duplicate_id",
            RankError::TooFewSamples { .. } => "too_few_samples",
            RankError::NotSymmetric { .. } => "not_symmetric",
            RankError::NegativeVariance { .. } => "negative_variance",
            RankError::NotPositiveSemidefinite { .. } => "not_positive_semidefinite",
            RankError::Numeric { .. } => "numeric",
            RankError::NotConvertible { .. } => "not_convertible",
            RankError::Encoding => "encoding",
        }
    }
}

impl fmt::Display for RankError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RankError::NoInputs => write!(f, "no_inputs"),
            RankError::DimensionMismatch { what, expected, found, index } => match index {
                Some(i) => write!(f, "dimension_mismatch{{{what}[{i}]: expected {expected}, found {found}}}"),
                None => write!(f, "dimension_mismatch{{{what}: expected {expected}, found {found}}}"),
            },
            RankError::InvalidId { index } => write!(f, "invalid_id{{{index}}}"),
            RankError::DuplicateId { index } => write!(f, "duplicate_id{{{index}}}"),
            RankError::TooFewSamples { found } => write!(f, "too_few_samples{{{found}}}"),
            RankError::NotSymmetric { i, j } => write!(f, "not_symmetric{{{i}, {j}}}"),
            RankError::NegativeVariance { index: Some(i) } => write!(f, "negative_variance{{{i}}}"),
            RankError::NegativeVariance { index: None } => write!(f, "negative_variance{{c^T S c}}"),
            RankError::NotPositiveSemidefinite { i, j } => write!(f, "not_positive_semidefinite{{{i}, {j}}}"),
            RankError::Numeric { input: Some(i), error } => write!(f, "numeric{{input {i}: {error}}}"),
            RankError::Numeric { input: None, error } => write!(f, "numeric{{{error}}}"),
            RankError::NotConvertible { reason, input_id: Some(id) } => write!(f, "not_convertible{{{reason}: {id}}}"),
            RankError::NotConvertible { reason, input_id: None } => write!(f, "not_convertible{{{reason}}}"),
            RankError::Encoding => write!(f, "encoding"),
        }
    }
}

impl std::error::Error for RankError {}

fn num(input: Option<usize>) -> impl Fn(NumError) -> RankError {
    move |error| RankError::Numeric { input, error }
}

/// `-0.0` as `+0.0`.
fn pos0(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

/// Ranks the contributors of an evaluated model (`unc.prop.rank@1.0.0`,
/// REQ-UNC.PROP.RANK-001..004). See the module documentation for the pinned rules.
pub fn rank(input: &RankInput<'_>) -> Result<RankOutput, RankError> {
    match *input {
        RankInput::Lpu { c, cov, ids } => rank_lpu(c, cov, ids),
        RankInput::Samples { inputs, output, ids } => rank_samples(inputs, output, ids),
    }
}

fn check_id_count(ids: &[&str], n: usize) -> Result<(), RankError> {
    if ids.len() == n {
        Ok(())
    } else {
        Err(RankError::DimensionMismatch { what: "ids", expected: n, found: ids.len(), index: None })
    }
}

/// Rule 2: tokens, then no duplicate (the lowest index that repeats an earlier ID).
fn check_ids(ids: &[&str]) -> Result<(), RankError> {
    for (index, id) in ids.iter().enumerate() {
        if !is_token(id) {
            return Err(RankError::InvalidId { index });
        }
    }
    let mut sorted: Vec<(&[u8], usize)> = ids.iter().enumerate().map(|(i, s)| (s.as_bytes(), i)).collect();
    sorted.sort_unstable();
    let mut dup: Option<usize> = None;
    for pair in sorted.windows(2) {
        if let [(a, _), (b, later)] = pair {
            if a == b && dup.map_or(true, |d| *later < d) {
                dup = Some(*later);
            }
        }
    }
    match dup {
        Some(index) => Err(RankError::DuplicateId { index }),
        None => Ok(()),
    }
}

/// Descending key in total order, an absent key last.
fn desc(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => y.total_cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Sorts by descending key, ties by ID bytes, and flags every entry whose key equals the
/// previous entry's (REQ-003).
fn order(list: &mut [Ranked], key: fn(&Ranked) -> Option<f64>) {
    list.sort_by(|a, b| desc(key(a), key(b)).then_with(|| a.input_id.as_bytes().cmp(b.input_id.as_bytes())));
    let mut prev: Option<Option<u64>> = None;
    for r in list.iter_mut() {
        let k = key(r).map(f64::to_bits);
        r.tie_broken_by_id = prev == Some(k);
        prev = Some(k);
    }
}

fn rank_lpu(c: &[f64], cov: &[f64], ids: &[&str]) -> Result<RankOutput, RankError> {
    let n = c.len();
    if n == 0 {
        return Err(RankError::NoInputs);
    }
    check_id_count(ids, n)?;
    if n.checked_mul(n) != Some(cov.len()) {
        return Err(RankError::DimensionMismatch {
            what: "cov",
            expected: n.saturating_mul(n),
            found: cov.len(),
            index: None,
        });
    }
    check_ids(ids)?;
    check_finite_slice("rank.c", c).map_err(num(None))?;
    check_finite_slice("rank.cov", cov).map_err(num(None))?;
    let rows: Vec<&[f64]> = cov.chunks_exact(n).collect();

    // Rule 4: exact symmetry, then non-negative diagonal.
    for (i, row) in rows.iter().enumerate() {
        for (j, &x) in row.iter().enumerate().skip(i + 1) {
            if rows.get(j).and_then(|r| r.get(i)).copied() != Some(x) {
                return Err(RankError::NotSymmetric { i, j });
            }
        }
    }
    let mut diag = Vec::with_capacity(n);
    for (i, row) in rows.iter().enumerate() {
        match row.get(i).copied() {
            Some(d) if d >= 0.0 => diag.push(d),
            _ => return Err(RankError::NegativeVariance { index: Some(i) }),
        }
    }
    check_cauchy_schwarz(&rows, &diag)?;
    check_cholesky(cov, n)?;

    // Rule 5: u_i(y).
    let mut u = Vec::with_capacity(n);
    for (i, (&ci, &d)) in c.iter().zip(&diag).enumerate() {
        let ui = ci.abs() * f::sqrt(d).map_err(num(Some(i)))?;
        if !ui.is_finite() {
            return Err(RankError::Numeric {
                input: Some(i),
                error: NumError::Infinite { op: "rank.u_contrib", index: Some(i) },
            });
        }
        u.push(pos0(ui));
    }

    // Rule 6: w = S c, v = c^T S c and its rounding tolerance.
    let mut w = Vec::with_capacity(n);
    for row in &rows {
        w.push(red::dot(row, c).map_err(num(None))?);
    }
    let v = red::dot(c, &w).map_err(num(None))?;
    let abs_c: Vec<f64> = c.iter().map(|x| x.abs()).collect();
    let mut b = Vec::with_capacity(n);
    for row in &rows {
        let abs_row: Vec<f64> = row.iter().map(|x| x.abs()).collect();
        b.push(red::dot(&abs_row, &abs_c).map_err(num(None))?);
    }
    let a = red::dot(&abs_c, &b).map_err(num(None))?;
    // n < 2^53 (it indexes a slice of n * n doubles), so the conversion is exact.
    let tol = (4.0 * (n as f64 + 1.0) * U) * a + (4.0 * n as f64 + 4.0) * TINY;

    // Rules 7 and 8.
    if v < -tol {
        return Err(RankError::NegativeVariance { index: None });
    }
    let (degenerate, shares, share_sum, share_values) = if v <= tol {
        let why = if v == 0.0 { Degenerate::ZeroVariance } else { Degenerate::VarianceAtRoundingResolution };
        (Some(why), Shares::Degenerate, None, None)
    } else {
        let mut sh = Vec::with_capacity(n);
        for (i, (&ci, &wi)) in c.iter().zip(&w).enumerate() {
            let s = (ci * wi) / v;
            if !s.is_finite() {
                return Err(RankError::Numeric {
                    input: Some(i),
                    error: NumError::Infinite { op: "rank.share", index: Some(i) },
                });
            }
            sh.push(pos0(s));
        }
        let sum = pos0(red::sum(&sh).map_err(num(None))?);
        if (sum - 1.0).abs() > SHARE_SUM_TOL {
            (None, Shares::SumOutOfTolerance, Some(sum), None)
        } else {
            (None, Shares::Present, Some(sum), Some(sh))
        }
    };

    let mut list = Vec::with_capacity(n);
    for (i, ((&id, &ci), &ui)) in ids.iter().zip(c).zip(&u).enumerate() {
        let share = share_values.as_ref().and_then(|sh| sh.get(i).copied());
        list.push(Ranked::lpu(id, pos0(ci), ui, share));
    }
    order(&mut list, |r| r.u_contrib);
    Ok(RankOutput {
        method: RankMethod::Lpu,
        contributors: list,
        degenerate,
        shares: Some(shares),
        share_sum,
        trials: None,
    })
}

/// Rule 4, first test: the pairwise Cauchy-Schwarz test, first failing pair `i < j` in row-major order.
fn check_cauchy_schwarz(rows: &[&[f64]], diag: &[f64]) -> Result<(), RankError> {
    let one_plus = 1.0 + 4.0 * U;
    for (i, (row, &dii)) in rows.iter().zip(diag).enumerate() {
        for (j, (&s, &djj)) in row.iter().zip(diag).enumerate().skip(i + 1) {
            if s == 0.0 {
                continue;
            }
            let violated = if dii == 0.0 || djj == 0.0 {
                true
            } else {
                let lhs = s * s;
                let rhs = (dii * djj) * one_plus;
                let normal = |x: f64| x.is_finite() && x >= f64::MIN_POSITIVE;
                if normal(lhs) && normal(rhs) {
                    lhs > rhs
                } else {
                    let q = (s.abs() / f::sqrt(dii).map_err(num(Some(i)))?) / f::sqrt(djj).map_err(num(Some(j)))?;
                    q * q > one_plus
                }
            };
            if violated {
                return Err(RankError::NotPositiveSemidefinite { i, j });
            }
        }
    }
    Ok(())
}

/// Rule 4, second test: the remainder of a pivoted Cholesky that stopped early must be rounding only.
fn check_cholesky(cov: &[f64], n: usize) -> Result<(), RankError> {
    let mat = Mat::new(n, n, cov.to_vec()).map_err(num(None))?;
    let ch = chol_pivoted(&mat, 0.0).map_err(num(None))?;
    let r = ch.rank();
    if r >= n {
        return Ok(());
    }
    let (l, perm) = (ch.l(), ch.perm());
    let factor = 4.0 * (r as f64 + 1.0) * U;
    let floor = (4.0 * r as f64 + 4.0) * TINY;
    let head = |i: usize| -> Result<&[f64], RankError> {
        l.row(i).map_err(num(None))?.get(..r).ok_or(RankError::Numeric {
            input: None,
            error: NumError::Domain { op: "rank.cholesky", detail: "factor row shorter than its rank" },
        })
    };
    let original = |i: usize| -> Result<usize, RankError> {
        perm.get(i).copied().ok_or(RankError::Numeric {
            input: None,
            error: NumError::Domain { op: "rank.cholesky", detail: "permutation shorter than the matrix" },
        })
    };
    for i in r..n {
        let li = head(i)?;
        let abs_li: Vec<f64> = li.iter().map(|x| x.abs()).collect();
        let pi = original(i)?;
        for j in i..n {
            let lj = head(j)?;
            let abs_lj: Vec<f64> = lj.iter().map(|x| x.abs()).collect();
            let pj = original(j)?;
            let a = mat.get(pi, pj).map_err(num(None))?;
            let rem = a - red::dot(li, lj).map_err(num(None))?;
            let bound = factor * (a.abs() + red::dot(&abs_li, &abs_lj).map_err(num(None))?) + floor;
            if rem.abs() > bound {
                let (lo, hi) = if pi <= pj { (pi, pj) } else { (pj, pi) };
                return Err(RankError::NotPositiveSemidefinite { i: lo, j: hi });
            }
        }
    }
    Ok(())
}

/// Rule 3/4 of the samples form: `None` for a constant column or a zero sum of squares, else
/// the deviations and their sum of squares.
fn deviations(col: &[f64], input: Option<usize>) -> Result<Option<(Vec<f64>, f64)>, RankError> {
    let Some(&first) = col.first() else {
        return Ok(None);
    };
    if col.iter().all(|&x| x == first) {
        return Ok(None);
    }
    let m = red::mean(col).map_err(num(input))?;
    let mut d = Vec::with_capacity(col.len());
    for (r, &x) in col.iter().enumerate() {
        let e = x - m;
        if !e.is_finite() {
            return Err(RankError::Numeric {
                input,
                error: NumError::Infinite { op: "rank.deviation", index: Some(r) },
            });
        }
        d.push(e);
    }
    let s = red::sum_sq(&d).map_err(num(input))?;
    Ok(if s == 0.0 { None } else { Some((d, s)) })
}

fn rank_samples(inputs: &[&[f64]], output: &[f64], ids: &[&str]) -> Result<RankOutput, RankError> {
    let k = inputs.len();
    if k == 0 {
        return Err(RankError::NoInputs);
    }
    check_id_count(ids, k)?;
    let m = output.len();
    if m < 2 {
        return Err(RankError::TooFewSamples { found: m });
    }
    for (i, col) in inputs.iter().enumerate() {
        if col.len() != m {
            return Err(RankError::DimensionMismatch { what: "input", expected: m, found: col.len(), index: Some(i) });
        }
    }
    check_ids(ids)?;
    check_finite_slice("rank.output", output).map_err(num(None))?;
    for (i, col) in inputs.iter().enumerate() {
        check_finite_slice("rank.input", col).map_err(num(Some(i)))?;
    }

    let out_dev = deviations(output, None)?;
    let degenerate = if out_dev.is_none() { Some(Degenerate::ZeroOutputVariance) } else { None };
    let mut list = Vec::with_capacity(k);
    for (i, (&id, col)) in ids.iter().zip(inputs).enumerate() {
        let entry = match &out_dev {
            None => Ranked::samples(id, None, Some(ScoreAbsent::DegenerateOutput)),
            Some((_, syy)) if *syy < f64::MIN_POSITIVE => {
                Ranked::samples(id, None, Some(ScoreAbsent::BelowRoundingResolution))
            }
            Some((dy, syy)) => match deviations(col, Some(i))? {
                None => Ranked::samples(id, None, Some(ScoreAbsent::ZeroInputVariance)),
                Some((_, sxx)) if sxx < f64::MIN_POSITIVE => {
                    Ranked::samples(id, None, Some(ScoreAbsent::BelowRoundingResolution))
                }
                Some((dx, sxx)) => {
                    let sxy = red::dot(&dx, dy).map_err(num(Some(i)))?;
                    let r = (sxy / f::sqrt(sxx).map_err(num(Some(i)))?) / f::sqrt(*syy).map_err(num(None))?;
                    let r2 = r * r;
                    if !r2.is_finite() {
                        return Err(RankError::Numeric {
                            input: Some(i),
                            error: NumError::Infinite { op: "rank.r2", index: Some(i) },
                        });
                    }
                    Ranked::samples(id, Some(if r2 > 1.0 { 1.0 } else { pos0(r2) }), None)
                }
            },
        };
        list.push(entry);
    }
    order(&mut list, |r| r.r2);
    Ok(RankOutput {
        method: RankMethod::ApproximateSrc2,
        contributors: list,
        degenerate,
        shares: None,
        share_sum: None,
        trials: Some(m as u64),
    })
}
