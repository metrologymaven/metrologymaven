//! Adaptive Monte Carlo propagation (JCGM 101:2008). Spec node `UNC.PROP.MCM`.
//!
//! Engine functions only (BLOCK, ADAPT, INTVL, SAMP are L0 in this crate); the command
//! `unc.propagate_mcm@1` and its block scheduling are in the L2 command crate.
//!
//! Implemented: [`interval`] (`UNC.PROP.MCM.INTVL`) and [`adapt`] (the adaptive stopping
//! rule, `UNC.PROP.MCM.ADAPT`). Not yet built: trial blocks (`UNC.PROP.MCM.BLOCK`, blocked
//! on `UNC.MODEL.GRAPH`) and retained sample sets (`UNC.PROP.MCM.SAMP`).
//!
//! Nothing in this module produces an uncertainty statement or an evidence class. A
//! Monte Carlo summary is a model-based (simulated) result; the class and the claim rules
//! are applied by `UNC.UEC.CLASS` and `UNC.PROP.SELECT` when a statement is made from it.

use core::fmt;
use fid_num::NumError;

pub mod adapt;
pub mod interval;

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
        }
    }
}

impl core::error::Error for McmError {}
