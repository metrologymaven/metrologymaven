//! Kernel uncertainty types of Metrology Maven (spec node `UNC`). Pure Rust, no I/O, no
//! `unsafe`, no floating-point arithmetic in this crate so far; kernel graph, KNP-1.
//!
//! Implemented leaves: `UNC.UEC.STMT` (the [`EvidenceClass`] enumeration and the validated
//! [`UncertaintyStatement`] with its canonical JSON and digest, see [`statement`]).
//!
//! This crate never computes an uncertainty. It holds what other nodes computed or a
//! person declared, and refuses any statement that breaks the invariants.

pub mod statement;

pub use statement::{
    Contributor, CoverageInterval, EvidenceClass, Expanded, IntervalKind, Method, ModelRef, NlinVerdict, Nonlinearity,
    PrincipalKind, Provenance, SourceKind, SourceRef, StatementParts, StmtError, Uec0Reason, UncertaintyStatement,
};
