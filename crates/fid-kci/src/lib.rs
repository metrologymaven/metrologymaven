//! Kernel call interface for Metrology Maven. Modules land one spec leaf at a time;
//! see `spec/krn/man/`.

pub mod cards;
pub mod kci;
pub mod manifest;

pub use cards::{
    check, parse_card, AlgorithmId, CardError, CardRegistry, CheckReport, InputArity, SpecCard, MAX_ARITY_BOUND,
};
pub use kci::{
    evaluate, AlgorithmFailure, AlgorithmTable, Digest, KernelAlgorithm, KernelCall, KernelError, KernelResult,
    MANIFEST_CORE_DIGEST_QUERY,
};
pub use manifest::{KernelManifest, ManifestCore, ManifestError};
