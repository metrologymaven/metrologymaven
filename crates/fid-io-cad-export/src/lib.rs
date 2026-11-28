//! CAD export of geometric elements for Metrology Maven (spec node `IO.CAD.EXPORT`).
//!
//! Modules land one spec leaf at a time; see `spec/io/cad/export/`.
//!
//! - [`bound`]: the export plan (`IO.CAD.EXPORT.BOUND`). Selected elements are moved into
//!   the caller's target frame through the frame tree, and every unbounded element (line,
//!   plane, cylinder, cone) is given a finite extent, the caller's or the min/max projection
//!   of its own fitted points widened by a margin, so the STEP and the IGES writers export the
//!   same bounded geometry and report the same extents. Every refusal is a typed
//!   [`PlanError`] naming the element; nothing is clamped, defaulted or dropped.
//!
//! The STEP and IGES writers (`IO.CAD.EXPORT.STEPW`, `IO.CAD.EXPORT.IGESW`) are not built
//! yet. The plan does no I/O and has no side effects.

pub mod bound;

pub use bound::{
    plan, Bounds, ElementId, ElementInput, ExportPlan, Extent, ExtentFault, ExtentKind, ExtentSource, GeomKind,
    Geometry, NameFault, PlanError, PlanItem, PointsUnused, TransformFault, UntrustedName, MAX_ELEMENTS,
    MAX_NAME_BYTES,
};
