//! Kernel uncertainty types of Metrology Maven (spec node `UNC`). Pure Rust, no I/O, no
//! `unsafe`; kernel graph, KNP-1 (floating-point work only through `fid-num`).
//!
//! Implemented leaves: `UNC.UEC.STMT` (the [`EvidenceClass`] enumeration and the validated
//! [`UncertaintyStatement`] with its canonical JSON and digest, see [`statement`]),
//! `UNC.UEC.CLASS` (evidence class assignment, rule set `uec-assign@1`, see [`uec`]),
//! `UNC.UEC.QIF` (pure QIF 3.0 uncertainty statement mapping `qif-unc-map@1`, see
//! [`uec::qif_map`]),
//! `UNC.MODEL.DIST` (input distributions, moments, seeded sampling and the correlation check,
//! see [`dist`]), `UNC.MODEL.GRAPH.SCHEMA` (the Uncertainty Model graph types, the
//! `uncertainty-model/v1` document, its canonical form and digest, see [`model`]),
//! `UNC.MODEL.GRAPH.CHECK` (structural validation into a `ValidModel`, see
//! [`model::validate`]),
//! `UNC.PROP.MCM.BLOCK` (trial-block sampling and per-trial kernel evaluation, see
//! [`prop::mcm::block`]), `UNC.PROP.MCM.INTVL` (Monte Carlo coverage intervals and estimate
//! outputs from samples, see [`prop::mcm::interval`]), `UNC.PROP.MCM.ADAPT` (the adaptive
//! stopping rule `mcm-adapt@1` and trial cap, see [`prop::mcm::adapt`]),
//! `UNC.PROP.MCM.SAMP` (retained sample sets `mcm-samples@1` and their digest, see
//! [`prop::mcm::samples`]), `UNC.PROP.VALID` (validation of LPU against Monte Carlo,
//! `unc.prop.validate_lpu@1.0.0`, see [`prop::valid`]) and `UNC.PROP.LPU.TAN` (the operator
//! tangent contract, the tangent table and the `tangent-coverage@1` report, see
//! [`prop::lpu::tan`]; its test-only central-difference harness is `tangent_harness`, behind
//! the `tangent-harness` feature) and `UNC.PROP.MVAR.PARAM` (the feature covariance type and
//! the `fcov-param@1` parameterisations, see [`prop::mvar::param`]), `UNC.PROP.MVAR.VMCM`
//! (joint Monte Carlo sample covariance and Mahalanobis coverage region of a characteristic
//! group, see [`prop::mvar::vmcm`]) and `UNC.PROP.RANK` (contributor ranking,
//! `unc.prop.rank@1.0.0`, see [`prop::rank`]).
//!
//! The `statement` module never computes an uncertainty. It holds what other nodes computed
//! or a person declared, and refuses any statement that breaks the invariants.

pub mod dist;
pub mod model;
pub mod prop;
pub mod statement;
#[cfg(feature = "tangent-harness")]
pub mod tangent_harness;
pub mod uec;

pub use statement::{
    Contributor, CoverageInterval, EvidenceClass, Expanded, IntervalKind, Method, ModelRef, NlinVerdict, Nonlinearity,
    PrincipalKind, Provenance, SourceKind, SourceRef, StatementParts, StmtError, Uec0Reason, UncertaintyStatement,
};
