//! Adaptive Monte Carlo propagation (JCGM 101:2008). Spec node `UNC.PROP.MCM`.
//!
//! Engine functions only (BLOCK, ADAPT, INTVL, SAMP are L0 in this crate); the command
//! `unc.propagate_mcm@1` and its block scheduling are in the L2 command crate.
//!
//! Implemented: [`block`] (trial-block sampling and per-trial kernel evaluation,
//! `UNC.PROP.MCM.BLOCK`), [`interval`] (`UNC.PROP.MCM.INTVL`), [`adapt`] (the adaptive
//! stopping rule, `UNC.PROP.MCM.ADAPT`) and [`samples`] (retained sample sets
//! `mcm-samples@1` and their digest, `UNC.PROP.MCM.SAMP`).
//!
//! Nothing in this module produces an uncertainty statement or an evidence class. A
//! Monte Carlo summary is a model-based (simulated) result; the class and the claim rules
//! are applied by `UNC.UEC.CLASS` and `UNC.PROP.SELECT` when a statement is made from it.

use crate::dist::DistError;
use core::fmt;
use fid_num::NumError;

pub mod adapt;
pub mod block;
pub mod interval;
pub mod samples;

/// Errors of the Monte Carlo engine functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McmError {
    /// `invalid_p`: the coverage probability is not a finite number strictly between 0
    /// and 1 (NaN included).
    InvalidP,
    /// `too_few_trials`: the value set has `m` values but at least `min`, the larger of 2
    /// and `ceil(1 / (1 - p))`, are needed (REQ-UNC.PROP.MCM.INTVL-003).
    TooFewTrials { m: u64, min: u64 },
    /// A result buffer could not be reserved.
    Allocation,
    /// `invalid_profile`: an adaptive-rule profile field is out of range (`detail` names
    /// it: `p`, `n_dig` or `cap`; REQ-UNC.PROP.MCM.ADAPT).
    InvalidProfile { detail: &'static str },
    /// `invalid_summary`: a block summary has a negative `u` or a lower endpoint above the
    /// upper one (`block` is its 0-based index), or the overall `u(y)` is negative
    /// (`block` is `None`).
    InvalidSummary { block: Option<u64> },
    /// `cap_exceeded`: more completed blocks than the profile's cap allows.
    CapExceeded { blocks: u64, max: u64 },
    /// A numeric error from `fid-num`: a NaN or infinite value (with the index of the first
    /// offending element of the value set, in input order) or an overflow in a reduction.
    Num(NumError),
    /// `provenance`: the model, the release manifest and the running kernel are not bound
    /// to each other (`detail`: `model_manifest`, `kernel_manifest_unset`,
    /// `kernel_manifest_mismatch` or `kernel_unavailable`; `UNC.PROP.MCM.BLOCK`).
    Provenance { detail: &'static str },
    /// `unsupported_model`: the block engine cannot evaluate this model (`detail`:
    /// `operator_not_in_kernel`, `operator_arity` or `correlated_non_normal`).
    UnsupportedModel { detail: &'static str },
    /// `invalid_block`: a block spec with `block_size` 0 or trial indices outside the
    /// addressable range (`detail`: `block_size` or `index`).
    InvalidBlock { detail: &'static str },
    /// `block_spec_mismatch`: the block spec's `model_digest` or `inputs_digest` differs from
    /// the prepared model or sampling plan.
    BlockSpecMismatch { field: &'static str },
    /// `kernel`: a kernel error that is not a per-trial algorithm failure (floating-point
    /// environment, lookup, card, arity, digest or parameter validation), raised by the call
    /// of global trial `trial`; `code` is the kernel's code.
    Kernel { code: &'static str, trial: u64 },
    /// A distribution error that is not a per-trial failure (`UNC.MODEL.DIST`).
    Dist(DistError),
    /// `internal`: an invariant of a validated model did not hold (`detail` names it).
    Internal { detail: &'static str },
    /// `samples_not_retained`: the encoded sample set would be `size` bytes, more than the
    /// retention cap; no bytes were produced (REQ-UNC.PROP.MCM.SAMP-002).
    SamplesNotRetained { size: u64 },
    /// `samples_invalid`: a sample set (or the blocks and header given to the encoder) is
    /// inconsistent (`detail` names the check; `UNC.PROP.MCM.SAMP`).
    SamplesInvalid { detail: &'static str },
}

impl McmError {
    /// The stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            McmError::InvalidP => "invalid_p",
            McmError::TooFewTrials { .. } => "too_few_trials",
            McmError::Allocation => "allocation_failed",
            McmError::InvalidProfile { .. } => "invalid_profile",
            McmError::InvalidSummary { .. } => "invalid_summary",
            McmError::CapExceeded { .. } => "cap_exceeded",
            McmError::Num(_) => "numeric",
            McmError::Provenance { .. } => "provenance",
            McmError::UnsupportedModel { .. } => "unsupported_model",
            McmError::InvalidBlock { .. } => "invalid_block",
            McmError::BlockSpecMismatch { .. } => "block_spec_mismatch",
            McmError::Kernel { .. } => "kernel",
            McmError::Dist(_) => "distribution",
            McmError::Internal { .. } => "internal",
            McmError::SamplesNotRetained { .. } => "samples_not_retained",
            McmError::SamplesInvalid { .. } => "samples_invalid",
        }
    }
}

impl From<NumError> for McmError {
    fn from(e: NumError) -> Self {
        McmError::Num(e)
    }
}

impl fmt::Display for McmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            McmError::InvalidP => write!(f, "invalid_p"),
            McmError::TooFewTrials { m, min } => write!(f, "too_few_trials: {m} values, need at least {min}"),
            McmError::Allocation => write!(f, "allocation_failed"),
            McmError::InvalidProfile { detail } => write!(f, "invalid_profile: {detail}"),
            McmError::InvalidSummary { block: Some(b) } => write!(f, "invalid_summary: block {b}"),
            McmError::InvalidSummary { block: None } => write!(f, "invalid_summary: overall u"),
            McmError::CapExceeded { blocks, max } => write!(f, "cap_exceeded: {blocks} blocks, at most {max}"),
            McmError::Num(e) => write!(f, "numeric: {e}"),
            McmError::Provenance { detail } => write!(f, "provenance: {detail}"),
            McmError::UnsupportedModel { detail } => write!(f, "unsupported_model: {detail}"),
            McmError::InvalidBlock { detail } => write!(f, "invalid_block: {detail}"),
            McmError::BlockSpecMismatch { field } => write!(f, "block_spec_mismatch: {field}"),
            McmError::Kernel { code, trial } => write!(f, "kernel: {code} at trial {trial}"),
            McmError::Dist(e) => write!(f, "distribution: {e}"),
            McmError::Internal { detail } => write!(f, "internal: {detail}"),
            McmError::SamplesNotRetained { size } => write!(f, "samples_not_retained: {size} bytes over the cap"),
            McmError::SamplesInvalid { detail } => write!(f, "samples_invalid: {detail}"),
        }
    }
}

impl core::error::Error for McmError {}
