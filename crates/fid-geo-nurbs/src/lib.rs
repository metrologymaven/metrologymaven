//! B-spline and NURBS curves and surfaces for Metrology Maven. Pure Rust, `f64`, no I/O,
//! no `unsafe`, kernel layer (KNP-1, CONSTITUTION §4); the only dependency is `fid-num` (num
//! layer) for the correctly rounded square root and the pinned trigonometry.
//!
//! Modules land one spec leaf at a time; see `spec/geo/nurbs/`.
//!
//! - [`data`]: validated knot vectors, curves and surfaces (`GEO.NURBS.DATA`).
//! - [`eval`]: the [`Curve`] and [`Surface`] traits; position, derivatives to second order,
//!   tangent, normal, curvatures and principal directions (`GEO.NURBS.EVAL`).
//! - [`analytic`]: closed-form lines, circles, ellipses, planes, cylinders, cones, spheres and
//!   tori behind the same traits, with closed-form inversion (`GEO.NURBS.ANALYTIC`).

pub mod analytic;
mod basis;
pub mod data;
pub mod eval;

pub use analytic::{
    AnalyticCurve, AnalyticError, AnalyticSurface, Circle, Cone, CurveFoot, Cylinder, Ellipse, Line, Placement,
    PlacementFault, Plane, Sphere, SurfaceFoot, Torus, Uniqueness,
};
pub use basis::MAX_ORDER;
pub use data::{Dir, KnotVector, NurbsCurve, NurbsCurveParts, NurbsError, NurbsSurface, NurbsSurfaceParts, MAX_DEGREE};
pub use eval::{
    curve_geometry, surface_curvature, surface_normal, Curvature, Curve, CurveDerivs, CurveGeometry, EvalError,
    Regularity, Surface, SurfaceDerivs,
};
