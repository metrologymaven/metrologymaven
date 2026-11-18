//! Kernel call interface for Metrology Maven. Modules land one spec leaf at a time;
//! see `spec/krn/man/`.

pub mod cards;
pub mod kci;

pub use cards::{check, parse_card, AlgorithmId, CardError, CardRegistry, CheckReport, SpecCard};
pub use kci::{
    evaluate, AlgorithmFailure, AlgorithmTable, Digest, KernelAlgorithm, KernelCall, KernelError, KernelResult,
};
