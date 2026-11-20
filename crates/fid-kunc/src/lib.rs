//! Kernel uncertainty types of Metrology Maven (spec node `UNC`). Pure Rust, no I/O, no
//! `unsafe`; kernel graph, KNP-1 (floating-point work only through `fid-num`).
//!
//! Implemented leaves: `UNC.UEC.STMT` (the [`EvidenceClass`] enumeration and the validated
//! [`UncertaintyStatement`] with its canonical JSON and digest, see [`statement`]),
//! `UNC.UEC.CLASS` (evidence class assignment, rule set `uec-assign@1`, see [`uec`]),
//! `UNC.MODEL.DIST` (input distributions, moments, seeded sampling and the correlation check,
//! see [`dist`]), `UNC.MODEL.GRAPH.SCHEMA` (the Uncertainty Model graph types, the
//! `uncertainty-model/v1` document, its canonical form and digest, see [`model`]),
//! `UNC.PROP.MCM.INTVL` (Monte Carlo coverage intervals and estimate outputs from samples,
//! see [`prop::mcm::interval`]) and `UNC.PROP.MCM.ADAPT` (the adaptive stopping rule
//! `mcm-adapt@1` and trial cap, see [`prop::mcm::adapt`]).
//!
//! The `statement` module never computes an uncertainty. It holds what other nodes computed
//! or a person declared, and refuses any statement that breaks the invariants.

pub mod dist;
pub mod model;
pub mod prop;
pub mod statement;
pub mod uec;

pub use statement::{
    Contributor, CoverageInterval, EvidenceClass, Expanded, IntervalKind, Method, ModelRef, NlinVerdict, Nonlinearity,
    PrincipalKind, Provenance, SourceKind, SourceRef, StatementParts, StmtError, Uec0Reason, UncertaintyStatement,
};
