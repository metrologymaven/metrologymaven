//! Linear propagation with analytic sensitivities. Spec node `UNC.PROP.LPU`.
//!
//! Sensitivities come from analytic tangents only, never from finite differences
//! (REQ-UNC.PROP.LPU.TAN-003). A lint test (`crates/fid-kunc/tests/lpu_tan.rs`) scans every
//! file under this directory and fails if one evaluates a kernel operator or carries a
//! perturbation step.
//!
//! Implemented leaves: `UNC.PROP.LPU.TAN` (the operator tangent contract, the tangent table
//! and the `tangent-coverage@1` report, see [`tan`]).

pub mod tan;

pub use tan::{
    coverage_report, CoverageReport, Kink, OperatorTangent, ProviderError, SamplePoint, ScalarView, TanError,
    TangentOut, TangentTable, COVERAGE_VERSION, MAX_KINKS,
};
