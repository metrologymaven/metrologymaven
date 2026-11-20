//! Kernel call interface and digests. Spec node `KRN.MAN.KCI`.
//!
//! [`evaluate`] is the kernel's single, pure entry point. It probes the floating-point
//! environment, looks the algorithm up, checks that it has a spec card, refuses an input
//! count outside the card's declared `input_arity`, re-hashes every input and refuses a
//! mismatch before any algorithm code runs, validates the parameters,
//! runs the algorithm and hashes the parameters and output itself. Every digest in a
//! [`KernelResult`] is therefore produced inside this module.
//!
//! **Self-report query** (`kernel.manifest_core_digest`, spec node `KRN.MAN.MANI.GEN`). The
//! module name [`RESERVED_MODULE`] (`kernel`) belongs to queries the kernel answers itself; no
//! algorithm may be registered under it ([`KernelError::ReservedAlgorithm`]). The one such
//! query is [`MANIFEST_CORE_DIGEST_QUERY`]: no inputs, empty parameters and no seed, and its
//! output is the wire form (`sha256:<64 hex>`, ASCII) of the manifest core digest the assembly
//! crate attached with [`AlgorithmTable::with_manifest_core_digest`], a compiled-in constant,
//! so the answer needs no file. It goes through [`evaluate`] like every call (after the FP
//! probe), has no spec card and is never listed in the manifest's algorithms, so the digest it
//! reports does not feed back into the core it describes. Its accepted calls are fixed at
//! version 1.0.0; any change to them needs a new version (D-046 rule 2).

pub mod json;
pub mod sha256;

use std::fmt;

use crate::cards::{AlgorithmId, CardRegistry, InputArity};
use fid_num::err::{probe_fp_env, NumError};
pub use fid_types::Digest;
use json::{fixed_object, Flat};
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

/// Module name reserved for queries the kernel answers itself (see the module docs).
pub const RESERVED_MODULE: &str = "kernel";

/// The self-report query: the manifest core digest compiled into this artifact.
pub const MANIFEST_CORE_DIGEST_QUERY: &str = "kernel.manifest.core_digest@1.0.0";

/// Input count of [`MANIFEST_CORE_DIGEST_QUERY`]: none.
pub const MANIFEST_CORE_DIGEST_ARITY: InputArity = InputArity::exactly(0);

/// Member keys of [`KernelResult::canonical_json`], in UTF-16 order (unit-tested in `json`).
pub(crate) const RESULT_KEYS: [&str; 6] =
    ["algorithm", "input_digests", "output", "output_digest", "params_digest", "seed"];

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
        let [k_alg, k_inputs, k_out, k_out_digest, k_params_digest, k_seed] = RESULT_KEYS;
        let mut members = vec![
            (k_alg, Flat::Str(&algorithm)),
            (k_inputs, Flat::List(&inputs)),
            (k_out, Flat::Str(&output)),
            (k_out_digest, Flat::Str(self.output_digest.as_str())),
            (k_params_digest, Flat::Str(self.params_digest.as_str())),
        ];
        if let Some(s) = &seed {
            members.push((k_seed, Flat::Str(s)));
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
    /// The call's input count is outside the `input_arity` its spec card declares
    /// (REQ-KRN.MAN.KCI-008). Raised before any input is hashed or any algorithm code runs.
    InputArityMismatch { expected: InputArity, got: usize },
    /// An algorithm's own [`KernelAlgorithm::input_arity`] differs from its card's
    /// `input_arity`; the table refuses to build (REQ-KRN.MAN.KCI-009).
    CardArityDrift { id: AlgorithmId, card: InputArity, implementation: InputArity },
    /// An algorithm was registered under the reserved module [`RESERVED_MODULE`]; the table
    /// refuses to build.
    ReservedAlgorithm(AlgorithmId),
    /// [`MANIFEST_CORE_DIGEST_QUERY`] was called on a table without a manifest core digest
    /// (a build that is not a release build).
    ManifestCoreDigestUnset,
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
            KernelError::InputArityMismatch { .. } => "input_arity_mismatch",
            KernelError::CardArityDrift { .. } => "card_arity_drift",
            KernelError::ReservedAlgorithm(_) => "reserved_algorithm",
            KernelError::ManifestCoreDigestUnset => "manifest_core_digest_unset",
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
            KernelError::InputArityMismatch { expected, got } => {
                write!(f, "input_arity_mismatch: card declares {expected} inputs, call has {got}")
            }
            KernelError::CardArityDrift { id, card, implementation } => {
                write!(f, "card_arity_drift: {id}: card declares {card} inputs, implementation {implementation}")
            }
            KernelError::ReservedAlgorithm(id) => write!(f, "reserved_algorithm: {id}"),
            KernelError::ManifestCoreDigestUnset => write!(f, "manifest_core_digest_unset"),
        }
    }
}

impl std::error::Error for KernelError {}

/// A kernel algorithm. Implementations must be pure: no I/O, clock, environment or global state.
pub trait KernelAlgorithm {
    /// The algorithm's ID.
    fn id(&self) -> AlgorithmId;
    /// How many input blobs `run` takes. Must equal the spec card's `input_arity`, or
    /// [`AlgorithmTable::new`] refuses the table with [`KernelError::CardArityDrift`].
    fn input_arity(&self) -> InputArity;
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
    manifest_core_digest: Option<Digest>,
}

impl fmt::Debug for AlgorithmTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AlgorithmTable")
            .field("ids", &self.ids())
            .field("cards", &self.cards)
            .field("manifest_core_digest", &self.manifest_core_digest)
            .finish()
    }
}

impl AlgorithmTable {
    /// Builds a table; fails on two algorithms with the same ID, and on an algorithm whose
    /// [`KernelAlgorithm::input_arity`] differs from its card's `input_arity`
    /// (REQ-KRN.MAN.KCI-009), and on an algorithm under the reserved module
    /// [`RESERVED_MODULE`] (`ReservedAlgorithm`). An algorithm without a card is accepted here
    /// and refused by [`evaluate`] with `MissingSpecCard`. The table has no manifest core digest
    /// until [`AlgorithmTable::with_manifest_core_digest`] attaches one.
    pub fn new(algorithms: Vec<Box<dyn KernelAlgorithm>>, cards: CardRegistry) -> Result<Self, KernelError> {
        let mut entries: Vec<(AlgorithmId, Box<dyn KernelAlgorithm>)> =
            algorithms.into_iter().map(|a| (a.id(), a)).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for pair in entries.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(KernelError::DuplicateAlgorithm(pair[0].0.clone()));
            }
        }
        if let Some((id, _)) = entries.iter().find(|(id, _)| id.module == RESERVED_MODULE) {
            return Err(KernelError::ReservedAlgorithm(id.clone()));
        }
        for (id, alg) in &entries {
            if let Some(card) = cards.get(id) {
                let implementation = alg.input_arity();
                if card.input_arity != implementation {
                    return Err(KernelError::CardArityDrift { id: id.clone(), card: card.input_arity, implementation });
                }
            }
        }
        Ok(AlgorithmTable { entries, cards, manifest_core_digest: None })
    }

    /// Attaches the manifest core digest this artifact was built from (the generated
    /// constant of the assembly crate); [`MANIFEST_CORE_DIGEST_QUERY`] reports it.
    pub fn with_manifest_core_digest(mut self, digest: Digest) -> Self {
        self.manifest_core_digest = Some(digest);
        self
    }

    /// The attached manifest core digest, if any.
    pub fn manifest_core_digest(&self) -> Option<&Digest> {
        self.manifest_core_digest.as_ref()
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

/// The kernel's single evaluation function (REQ-KRN.MAN.KCI-001 .. -006, -008). Calls under
/// the reserved module are the kernel's own queries ([`MANIFEST_CORE_DIGEST_QUERY`]).
pub fn evaluate(table: &AlgorithmTable, call: &KernelCall) -> Result<KernelResult, KernelError> {
    probe_fp_env().map_err(KernelError::FpEnvironment)?;
    if call.algorithm.module == RESERVED_MODULE {
        return self_report(table, call);
    }
    let alg = table.get(&call.algorithm).ok_or_else(|| KernelError::UnknownAlgorithm(call.algorithm.clone()))?;
    let card = table.cards.get(&call.algorithm).ok_or_else(|| KernelError::MissingSpecCard(call.algorithm.clone()))?;
    // Fail closed on the input count before hashing anything or calling the algorithm.
    if !card.input_arity.accepts(call.inputs.len()) {
        return Err(KernelError::InputArityMismatch { expected: card.input_arity, got: call.inputs.len() });
    }
    let mut input_digests = Vec::with_capacity(call.inputs.len());
    for (input_index, (bytes, claimed)) in call.inputs.iter().enumerate() {
        let d = sha256_digest(bytes);
        if &d != claimed {
            return Err(KernelError::DigestMismatch { input_index });
        }
        input_digests.push(d);
    }
    alg.validate(&call.params).map_err(confine)?;
    let inputs: Vec<&[u8]> = call.inputs.iter().map(|(b, _)| b.as_slice()).collect();
    let output = alg.run(&call.params, call.seed, &inputs).map_err(confine)?;
    Ok(KernelResult {
        algorithm: call.algorithm.clone(),
        params_digest: sha256_digest(&call.params),
        input_digests,
        seed: call.seed,
        output_digest: sha256_digest(&output),
        output,
    })
}

fn is_manifest_core_digest_query(id: &AlgorithmId) -> bool {
    id.module == RESERVED_MODULE && id.feature == "manifest" && id.method == "core_digest" && id.version == (1, 0, 0)
}

/// Answers [`MANIFEST_CORE_DIGEST_QUERY`] from the table's compiled-in digest; any other ID in
/// the reserved module is unknown. Checks in the order of [`evaluate`]: input count, then the
/// (empty) parameters and the absent seed, then the digest itself.
fn self_report(table: &AlgorithmTable, call: &KernelCall) -> Result<KernelResult, KernelError> {
    if !is_manifest_core_digest_query(&call.algorithm) {
        return Err(KernelError::UnknownAlgorithm(call.algorithm.clone()));
    }
    if !MANIFEST_CORE_DIGEST_ARITY.accepts(call.inputs.len()) {
        return Err(KernelError::InputArityMismatch { expected: MANIFEST_CORE_DIGEST_ARITY, got: call.inputs.len() });
    }
    if !call.params.is_empty() {
        return Err(KernelError::InvalidParams { reason: "params_not_empty" });
    }
    if call.seed.is_some() {
        return Err(KernelError::InvalidParams { reason: "seed_not_accepted" });
    }
    let digest = table.manifest_core_digest.as_ref().ok_or(KernelError::ManifestCoreDigestUnset)?;
    let output = digest.as_str().as_bytes().to_vec();
    Ok(KernelResult {
        algorithm: call.algorithm.clone(),
        params_digest: sha256_digest(&call.params),
        input_digests: Vec::new(),
        seed: None,
        output_digest: sha256_digest(&output),
        output,
    })
}

/// Code given to an algorithm error that tried to look like a kernel check.
pub const IMPERSONATION_CODE: &str = "algorithm_returned_kernel_error";

/// Algorithms may only report `InvalidParams` or `Algorithm(..)`; any other variant
/// (digest, lookup, card, FP or table errors belong to the kernel) becomes
/// `Algorithm(IMPERSONATION_CODE)`, so an algorithm cannot impersonate a kernel check.
fn confine(e: KernelError) -> KernelError {
    match e {
        KernelError::InvalidParams { .. } | KernelError::Algorithm(_) => e,
        _ => KernelError::Algorithm(AlgorithmFailure { code: IMPERSONATION_CODE }),
    }
}
