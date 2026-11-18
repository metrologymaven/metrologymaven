//! Kernel call interface and digests. Spec node `KRN.MAN.KCI`.
//!
//! [`evaluate`] is the kernel's single, pure entry point. It probes the floating-point
//! environment, looks the algorithm up, checks that it has a spec card, re-hashes every
//! input and refuses a mismatch before any algorithm code runs, validates the parameters,
//! runs the algorithm and hashes the parameters and output itself. Every digest in a
//! [`KernelResult`] is therefore produced inside this module.

pub mod json;
pub mod sha256;

use std::fmt;

use crate::cards::{AlgorithmId, CardRegistry};
use fid_num::err::{probe_fp_env, NumError};
use json::{fixed_object, Flat};
pub use fid_types::Digest;
pub use sha256::{sha256, sha256_digest, to_hex, Sha256};

/// One kernel call (REQ-KRN.MAN.KCI-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelCall {
    /// Algorithm ID with exact semver.
    pub algorithm: AlgorithmId,
    /// Canonical parameter bytes.
    pub params: Vec<u8>,
    /// Optional seed.
    pub seed: Option<u64>,
    /// Input blobs, each with its claimed SHA-256 digest (shared `fid_types::Digest`).
    pub inputs: Vec<(Vec<u8>, Digest)>,
}

/// The kernel-owned result envelope (REQ-KRN.MAN.KCI-005). Only [`evaluate`] builds one,
/// so every field is computed inside the kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelResult {
    algorithm: AlgorithmId,
    params_digest: Digest,
    input_digests: Vec<Digest>,
    seed: Option<u64>,
    output: Vec<u8>,
    output_digest: Digest,
}

impl KernelResult {
    /// The algorithm that ran.
    pub fn algorithm(&self) -> &AlgorithmId {
        &self.algorithm
    }
    /// SHA-256 of the parameter bytes.
    pub fn params_digest(&self) -> &Digest {
        &self.params_digest
    }
    /// SHA-256 of each input, in call order, as computed by the kernel.
    pub fn input_digests(&self) -> &[Digest] {
        &self.input_digests
    }
    /// The seed passed to the algorithm.
    pub fn seed(&self) -> Option<u64> {
        self.seed
    }
    /// The algorithm's canonical output bytes.
    pub fn output(&self) -> &[u8] {
        &self.output
    }
    /// SHA-256 of the output bytes.
    pub fn output_digest(&self) -> &Digest {
        &self.output_digest
    }

    /// Canonical JSON of the envelope: `{"algorithm","input_digests","output","output_digest",
    /// "params_digest","seed"}` with digests as `sha256:<hex>`, the output as lowercase hex and
    /// the seed as a decimal string (the member is omitted when there is no seed).
    pub fn canonical_json(&self) -> Vec<u8> {
        let algorithm = self.algorithm.to_string();
        let inputs: Vec<String> = self.input_digests.iter().map(|d| d.as_str().to_string()).collect();
        let output = to_hex(&self.output);
        let seed = self.seed.map(|s| s.to_string());
        let mut members = vec![
            ("algorithm", Flat::Str(&algorithm)),
            ("input_digests", Flat::List(&inputs)),
            ("output", Flat::Str(&output)),
            ("output_digest", Flat::Str(self.output_digest.as_str())),
            ("params_digest", Flat::Str(self.params_digest.as_str())),
        ];
        if let Some(s) = &seed {
            members.push(("seed", Flat::Str(s)));
        }
        fixed_object(&members)
    }
}

/// Why an algorithm failed, without free text (the design's "String-free typed inner").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlgorithmFailure {
    /// Stable snake_case code chosen by the algorithm.
    pub code: &'static str,
}

/// Kernel call errors, each with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelError {
    /// The computed SHA-256 of input `input_index` differs from its claimed digest.
    DigestMismatch { input_index: usize },
    /// The algorithm ID is not in the table.
    UnknownAlgorithm(AlgorithmId),
    /// The algorithm is in the table but has no spec card.
    MissingSpecCard(AlgorithmId),
    /// The parameters failed the algorithm's validation.
    InvalidParams { reason: &'static str },
    /// The floating-point environment is not IEEE round-to-nearest-even without flush-to-zero.
    FpEnvironment(NumError),
    /// Two algorithms in one table share an ID.
    DuplicateAlgorithm(AlgorithmId),
    /// The algorithm itself failed.
    Algorithm(AlgorithmFailure),
}

impl KernelError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            KernelError::DigestMismatch { .. } => "digest_mismatch",
            KernelError::UnknownAlgorithm(_) => "unknown_algorithm",
            KernelError::MissingSpecCard(_) => "missing_spec_card",
            KernelError::InvalidParams { .. } => "invalid_params",
            KernelError::FpEnvironment(_) => "fp_environment",
            KernelError::DuplicateAlgorithm(_) => "duplicate_algorithm",
            KernelError::Algorithm(_) => "algorithm_failed",
        }
    }
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KernelError::DigestMismatch { input_index } => write!(f, "digest_mismatch: input {input_index}"),
            KernelError::UnknownAlgorithm(id) => write!(f, "unknown_algorithm: {id}"),
            KernelError::MissingSpecCard(id) => write!(f, "missing_spec_card: {id}"),
            KernelError::InvalidParams { reason } => write!(f, "invalid_params: {reason}"),
            KernelError::FpEnvironment(e) => write!(f, "fp_environment: {e}"),
            KernelError::DuplicateAlgorithm(id) => write!(f, "duplicate_algorithm: {id}"),
            KernelError::Algorithm(a) => write!(f, "algorithm_failed: {}", a.code),
        }
    }
}

impl std::error::Error for KernelError {}

/// A kernel algorithm. Implementations must be pure: no I/O, clock, environment or global state.
pub trait KernelAlgorithm {
    /// The algorithm's ID.
    fn id(&self) -> AlgorithmId;
    /// Checks the canonical parameter bytes.
    fn validate(&self, params: &[u8]) -> Result<(), KernelError>;
    /// Computes the canonical output bytes.
    fn run(&self, params: &[u8], seed: Option<u64>, inputs: &[&[u8]]) -> Result<Vec<u8>, KernelError>;
}

/// The algorithms an assembly crate registers, sorted by ID and searched by binary search,
/// together with the spec cards they are checked against.
pub struct AlgorithmTable {
    entries: Vec<(AlgorithmId, Box<dyn KernelAlgorithm>)>,
    cards: CardRegistry,
}

impl fmt::Debug for AlgorithmTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AlgorithmTable").field("ids", &self.ids()).field("cards", &self.cards).finish()
    }
}

impl AlgorithmTable {
    /// Builds a table; fails on two algorithms with the same ID.
    pub fn new(algorithms: Vec<Box<dyn KernelAlgorithm>>, cards: CardRegistry) -> Result<Self, KernelError> {
        let mut entries: Vec<(AlgorithmId, Box<dyn KernelAlgorithm>)> =
            algorithms.into_iter().map(|a| (a.id(), a)).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for pair in entries.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(KernelError::DuplicateAlgorithm(pair[0].0.clone()));
            }
        }
        Ok(AlgorithmTable { entries, cards })
    }

    /// Registered IDs, sorted (input to `cards::check`).
    pub fn ids(&self) -> Vec<AlgorithmId> {
        self.entries.iter().map(|(id, _)| id.clone()).collect()
    }

    /// The spec cards the table was built with.
    pub fn cards(&self) -> &CardRegistry {
        &self.cards
    }

    fn get(&self, id: &AlgorithmId) -> Option<&dyn KernelAlgorithm> {
        self.entries.binary_search_by(|(k, _)| k.cmp(id)).ok().map(|i| self.entries[i].1.as_ref())
    }
}

/// The kernel's single evaluation function (REQ-KRN.MAN.KCI-001 .. -006).
pub fn evaluate(table: &AlgorithmTable, call: &KernelCall) -> Result<KernelResult, KernelError> {
    probe_fp_env().map_err(KernelError::FpEnvironment)?;
    let alg = table.get(&call.algorithm).ok_or_else(|| KernelError::UnknownAlgorithm(call.algorithm.clone()))?;
    if table.cards.get(&call.algorithm).is_none() {
        return Err(KernelError::MissingSpecCard(call.algorithm.clone()));
    }
    let mut input_digests = Vec::with_capacity(call.inputs.len());
    for (input_index, (bytes, claimed)) in call.inputs.iter().enumerate() {
        let d = sha256_digest(bytes);
        if &d != claimed {
            return Err(KernelError::DigestMismatch { input_index });
        }
        input_digests.push(d);
    }
    alg.validate(&call.params)?;
    let inputs: Vec<&[u8]> = call.inputs.iter().map(|(b, _)| b.as_slice()).collect();
    let output = alg.run(&call.params, call.seed, &inputs)?;
    Ok(KernelResult {
        algorithm: call.algorithm.clone(),
        params_digest: sha256_digest(&call.params),
        input_digests,
        seed: call.seed,
        output_digest: sha256_digest(&output),
        output,
    })
}
