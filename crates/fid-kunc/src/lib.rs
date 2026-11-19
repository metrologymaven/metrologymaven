//! Kernel uncertainty types of Metrology Maven (spec node `UNC`). Pure Rust, no I/O, no
//! `unsafe`; kernel graph, KNP-1 (floating-point work only through `fid-num`).
//!
//! Implemented leaves: `UNC.UEC.STMT` (the [`EvidenceClass`] enumeration and the validated
//! [`UncertaintyStatement`] with its canonical JSON and digest, see [`statement`]);
//! `UNC.MODEL.DIST` (input distributions, moments, seeded sampling and the correlation check,
//! see [`dist`]).
//!
//! The `statement` module never computes an uncertainty. It holds what other nodes computed
//! or a person declared, and refuses any statement that breaks the invariants.

pub mod dist;
pub mod statement;

pub use statement::{
    Contributor, CoverageInterval, EvidenceClass, Expanded, IntervalKind, Method, ModelRef, NlinVerdict, Nonlinearity,
    PrincipalKind, Provenance, SourceKind, SourceRef, StatementParts, StmtError, Uec0Reason, UncertaintyStatement,
};
