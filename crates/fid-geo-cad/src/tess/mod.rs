//! Controlled, watertight tessellation of CAD bodies (spec node `GEO.CAD.TESS`).
//!
//! - [`edge`]: [`tessellate_edges`], every edge sampled once under the chordal and angular
//!   tolerances, one shared vertex index per body vertex and per interior edge sample, and the
//!   samples mapped into each coedge's face chart (`GEO.CAD.TESS.EDGE`).
//!
//! Face meshing (`GEO.CAD.TESS.FACE`), refinement (`GEO.CAD.TESS.REFINE`), the body driver
//! (`GEO.CAD.TESS.BODY`) and the commands (`GEO.CAD.TESS.CMD`) are other leaves.

pub mod edge;

pub use edge::{
    sample_curve, tessellate_edges, CoedgeUv, CurveSamples, EdgeReport, EdgeSamples, EdgeTess, EdgeTessError,
    EdgeTessParams, SampleError, UvError, DEFAULT_MAX_SAMPLES, MAX_DEPTH, ON_CURVE_TOL_M,
};
