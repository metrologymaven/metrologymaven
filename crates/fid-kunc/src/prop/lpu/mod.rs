//! Linear propagation with analytic sensitivities. Spec node `UNC.PROP.LPU`.
//!
//! Sensitivities come from analytic tangents only, never from finite differences
//! (REQ-UNC.PROP.LPU.TAN-003). A lint test (`crates/fid-kunc/tests/lpu_tan.rs`) lexes every
//! file under this directory and holds it to an allowlist of `use` and crate paths (no Monte
//! Carlo block, no kernel call interface), no evaluation callbacks, no `impl Trait` or `dyn`
//! other than `dyn OperatorTangent`, no trait other than `OperatorTangent`, no generic type
//! parameters and no perturbation identifiers. Adding a path to the allowlist is a reviewed
//! change.
//!
//! Implemented leaves: `UNC.PROP.LPU.TAN` (the operator tangent contract, the tangent table
//! and the `tangent-coverage@1` report, see [`tan`]).

pub mod tan;

pub use tan::{
    coverage_report, input_sources, CoverageReport, Kink, OperatorTangent, ProviderError, SamplePoint, ScalarView,
    TanError, TangentOut, TangentTable, COVERAGE_VERSION, MAX_KINKS,
};
