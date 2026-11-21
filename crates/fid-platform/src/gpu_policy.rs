//! The GPU backend policy and known-bad driver list (spec node `PLAT.GPU.POLICY`).
//!
//! The schemas, validators, safe defaults and known-bad lookup live in crate `fid-gpu-policy`
//! (layer `model`, below the tool layer, so `PLAT.GPU.DETECT` and VIZ can use them) and are
//! re-exported here unchanged, so there is one validator. This module keeps what needs the
//! whole support matrix: [`check_coverage`] (REQ-PLAT.GPU.POLICY-002), and the
//! `plat-gpu validate` defaults [`MATRIX_PATH`] and [`DEFAULT_PHASE`].

pub use fid_gpu_policy::policy::*;

use crate::matrix::{Matrix, Phase, Tier};
use std::collections::BTreeSet;

/// Path of the shipped support matrix, relative to the repository root.
pub const MATRIX_PATH: &str = "platform/support-matrix.toml";
/// Phase used by `plat-gpu validate` when `--phase` is not given (the node's phase).
pub const DEFAULT_PHASE: Phase = Phase::P1a;

/// REQ-PLAT.GPU.POLICY-002: every support-matrix row whose tier is Tier 1 or Tier 2 and
/// effective at `phase` (`phase >= since_phase`) needs a policy row for its OS (via
/// [`GpuOs::from_family`]) and architecture. Every OS family of matrix schema 1 is a desktop
/// OS. Returns one `NoPolicyRow` per missing (os, arch), sorted by ([`GpuOs`], [`Arch`](crate::matrix::Arch)).
pub fn check_coverage(policy: &GpuPolicy, matrix: &Matrix, phase: Phase) -> Vec<GpuPolicyError> {
    let mut missing = BTreeSet::new();
    for r in &matrix.rows {
        let effective = matches!(r.tier, Tier::Tier1 | Tier::Tier2) && phase >= r.since_phase;
        let os = GpuOs::from_family(r.os);
        if effective && policy.row(os, r.arch).is_none() {
            missing.insert((os, r.arch));
        }
    }
    missing.into_iter().map(|(os, arch)| GpuPolicyError::NoPolicyRow { os, arch }).collect()
}
