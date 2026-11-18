//! B-spline and NURBS curves and surfaces for Metrology Maven. Pure Rust, `f64`, no I/O,
//! no `unsafe`, no dependencies (kernel layer, KNP-1, CONSTITUTION §4).
//!
//! Modules land one spec leaf at a time; see `spec/geo/nurbs/`.
//!
//! - [`data`]: validated knot vectors, curves and surfaces (`GEO.NURBS.DATA`).

pub mod data;

pub use data::{Dir, KnotVector, NurbsCurve, NurbsCurveParts, NurbsError, NurbsSurface, NurbsSurfaceParts, MAX_DEGREE};
