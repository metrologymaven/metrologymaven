//! Validation of LPU against Monte Carlo (spec node `UNC.PROP.VALID`, algorithm
//! [`ALGORITHM_ID`] = `unc.prop.validate_lpu@1.0.0`).
//!
//! Compares an LPU coverage interval `[y - U_p, y + U_p]` against a Monte Carlo coverage
//! interval `[y_low, y_high]` at the same coverage probability `p`, in the manner of
//! JCGM 101:2008 §8.1:
//!
//! 1. Express the MCM standard uncertainty `u_mcm` rounded to `n_dig` significant decimal
//!    digits (default 2) as `c * 10^l` using exact integer/decimal extraction via
//!    [`crate::prop::mcm::adapt::tolerance`] (no `log10`, no target-dependent power loops).
//! 2. The numerical tolerance is `delta = 0.5 * 10^l`.
//! 3. Endpoint differences are computed with plain IEEE-754 binary64 arithmetic (no FMA):
//!    `d_low = |(y - U_p) - y_low|` and `d_high = |(y + U_p) - y_high|`.
//! 4. `validated = true` iff both `d_low <= delta` and `d_high <= delta`, compared exactly
//!    against the exact decimal `0.5 * 10^l`.
//!
//! Pure Rust, KNP-1, no I/O, no `unsafe`, no panics, no FMA, no `f64::min`/`max`, no
//! `HashMap`/`HashSet`; bit-identical across x86-64, ARM64 and wasm32.

use core::fmt;

use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::sha256_digest;
use fid_types::Digest;

use crate::prop::mcm::adapt::{tolerance as adapt_tolerance, Tolerance as ExactTolerance, MAX_N_DIG};

/// Algorithm identifier (CONSTITUTION §4, spec card `unc.prop.validate_lpu@1.0.0.toml`).
pub const ALGORITHM_ID: &str = "unc.prop.validate_lpu@1.0.0";

/// Default number of significant decimal digits of `u(y)` (REQ-UNC.PROP.VALID-001).
pub const DEFAULT_N_DIG: u8 = 2;

/// Inputs to [`validate_lpu`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValidateInput {
    /// LPU output estimate `y`.
    pub y: f64,
    /// LPU expanded uncertainty `U_p` at coverage probability `p_lpu` (`U_p > 0`).
    pub u_p: f64,
    /// Coverage probability of the LPU interval (`0 < p_lpu < 1`).
    pub p_lpu: f64,
    /// Lower endpoint of the Monte Carlo coverage interval at `p_mcm`.
    pub y_low: f64,
    /// Upper endpoint of the Monte Carlo coverage interval at `p_mcm` (`y_low < y_high`).
    pub y_high: f64,
    /// Coverage probability of the Monte Carlo interval (`0 < p_mcm < 1`).
    pub p_mcm: f64,
    /// Monte Carlo standard uncertainty `u(y)` (`u_mcm >= 0`).
    pub u_mcm: f64,
    /// Significant decimal digits of `u_mcm` used to compute `delta` (default [`DEFAULT_N_DIG`] = 2).
    pub n_dig: u8,
}

impl ValidateInput {
    /// Builds an input using the default `n_dig = 2`.
    pub const fn new(y: f64, u_p: f64, p_lpu: f64, y_low: f64, y_high: f64, p_mcm: f64, u_mcm: f64) -> Self {
        Self { y, u_p, p_lpu, y_low, y_high, p_mcm, u_mcm, n_dig: DEFAULT_N_DIG }
    }

    /// Overrides the number of significant decimal digits `n_dig`.
    pub const fn with_n_dig(mut self, n_dig: u8) -> Self {
        self.n_dig = n_dig;
        self
    }
}

/// Output of [`validate_lpu`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValidateOutput {
    /// `true` iff both `d_low <= delta` and `d_high <= delta` (exact comparison).
    pub validated: bool,
    /// Lower endpoint difference `|(y - U_p) - y_low|`.
    pub d_low: f64,
    /// Upper endpoint difference `|(y + U_p) - y_high|`.
    pub d_high: f64,
    /// Binary64 nearest to the exact numerical tolerance `delta = 0.5 * 10^l`.
    pub delta: f64,
    /// Exact decimal exponent `l` of the last retained digit (`None` when `u_mcm == 0`).
    pub exp10: Option<i32>,
}

impl ValidateOutput {
    /// Encodes the validation output as RFC 8785 canonical JSON with binary64 numbers as
    /// `"0x..."` IEEE-754 hex strings for cross-target bit-identity verification (AC-5).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, ValidateError> {
        let mut fields = Vec::with_capacity(6);
        fields.push(("algorithm".to_owned(), Json::Str(ALGORITHM_ID.to_owned())));
        fields.push(("d_high".to_owned(), Json::Str(format!("0x{:016x}", self.d_high.to_bits()))));
        fields.push(("d_low".to_owned(), Json::Str(format!("0x{:016x}", self.d_low.to_bits()))));
        fields.push(("delta".to_owned(), Json::Str(format!("0x{:016x}", self.delta.to_bits()))));
        if let Some(l) = self.exp10 {
            fields.push(("exp10".to_owned(), Json::Str(l.to_string())));
        }
        fields.push(("validated".to_owned(), Json::Str(if self.validated { "true" } else { "false" }.to_owned())));
        to_canonical(&Json::Obj(fields)).map_err(|_| ValidateError::Incomparable { reason: "canonical_json" })
    }

    /// SHA-256 digest of [`Self::to_canonical_bytes`].
    pub fn digest(&self) -> Result<Digest, ValidateError> {
        let bytes = self.to_canonical_bytes()?;
        Ok(sha256_digest(&bytes))
    }
}

/// Typed errors of [`validate_lpu`] and [`delta_for_u`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidateError {
    /// `incomparable{reason}`: inputs cannot be validly compared (REQ-UNC.PROP.VALID-003).
    Incomparable {
        /// Short static reason code.
        reason: &'static str,
    },
}

impl ValidateError {
    /// Stable error code (`"incomparable"`).
    pub const fn code(self) -> &'static str {
        match self {
            ValidateError::Incomparable { .. } => "incomparable",
        }
    }

    /// Short static reason code.
    pub const fn reason(self) -> &'static str {
        match self {
            ValidateError::Incomparable { reason } => reason,
        }
    }
}

impl fmt::Display for ValidateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidateError::Incomparable { reason } => write!(f, "incomparable{{{reason}}}"),
        }
    }
}

impl std::error::Error for ValidateError {}

/// Computes the exact numerical tolerance `delta = 0.5 * 10^l` for standard uncertainty
/// `u_mcm` and `n_dig` significant decimal digits using exact integer/decimal extraction
/// (REQ-UNC.PROP.VALID-001, -004, AC-3).
pub fn delta_for_u(u_mcm: f64, n_dig: u8) -> Result<ExactTolerance, ValidateError> {
    if !u_mcm.is_finite() {
        return Err(ValidateError::Incomparable { reason: "non_finite" });
    }
    if u_mcm < 0.0 {
        return Err(ValidateError::Incomparable { reason: "negative_u_mcm" });
    }
    let n = u32::from(n_dig);
    if !(1..=MAX_N_DIG).contains(&n) {
        return Err(ValidateError::Incomparable { reason: "invalid_n_dig" });
    }
    adapt_tolerance(u_mcm, n).map_err(|_| ValidateError::Incomparable { reason: "invalid_tolerance" })
}

/// Validates an LPU coverage interval against an MCM coverage interval (algorithm
/// `unc.prop.validate_lpu@1.0.0`, REQ-UNC.PROP.VALID-001..004, AC-1..AC-5).
pub fn validate_lpu(input: &ValidateInput) -> Result<ValidateOutput, ValidateError> {
    // 1. All inputs must be finite.
    let all = [input.y, input.u_p, input.p_lpu, input.y_low, input.y_high, input.p_mcm, input.u_mcm];
    for v in all {
        if !v.is_finite() {
            return Err(ValidateError::Incomparable { reason: "non_finite" });
        }
    }

    // 2. Coverage probabilities must be in (0, 1) and equal.
    if !(0.0 < input.p_lpu && input.p_lpu < 1.0) || !(0.0 < input.p_mcm && input.p_mcm < 1.0) {
        return Err(ValidateError::Incomparable { reason: "coverage_probability_range" });
    }
    if input.p_lpu != input.p_mcm {
        return Err(ValidateError::Incomparable { reason: "coverage_probability_mismatch" });
    }

    // 3. Both intervals must be strictly non-empty and not reversed.
    if input.u_p <= 0.0 {
        return Err(ValidateError::Incomparable { reason: "empty_or_reversed_lpu_interval" });
    }
    let lpu_low = input.y - input.u_p;
    let lpu_high = input.y + input.u_p;
    if !lpu_low.is_finite() || !lpu_high.is_finite() {
        return Err(ValidateError::Incomparable { reason: "overflow" });
    }
    if lpu_low >= lpu_high {
        return Err(ValidateError::Incomparable { reason: "empty_or_reversed_lpu_interval" });
    }
    if input.y_low >= input.y_high {
        return Err(ValidateError::Incomparable { reason: "empty_or_reversed_mcm_interval" });
    }

    // 4. Compute exact numerical tolerance delta = 0.5 * 10^l from u_mcm and n_dig.
    let tol = delta_for_u(input.u_mcm, input.n_dig)?;

    // 5. Compute endpoint differences with plain IEEE-754 subtraction (no FMA).
    let d_low = (lpu_low - input.y_low).abs();
    let d_high = (lpu_high - input.y_high).abs();
    if !d_low.is_finite() || !d_high.is_finite() {
        return Err(ValidateError::Incomparable { reason: "overflow" });
    }

    // 6. Exact comparison against exact decimal delta = 0.5 * 10^l.
    let validated = tol.within_delta(d_low) && tol.within_delta(d_high);

    Ok(ValidateOutput { validated, d_low, d_high, delta: tol.delta(), exp10: tol.exp10() })
}
