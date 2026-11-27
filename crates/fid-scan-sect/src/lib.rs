//! Sections of Metrology Maven (spec node `SCAN.SECT`). Pure Rust, `f64` metres and radians,
//! no I/O, no `unsafe`; kernel layer, KNP-1 (no FMA, trigonometry and norms only through
//! `fid-num`, no std `HashMap` or `HashSet`, NaN is an error).
//!
//! Implemented leaves: `SCAN.SECT.DEF` (the [`SectionDef`] type, its validation, its expansion
//! into an ordered list of cutting planes and its canonical digest, see [`def`]). Cutting data
//! and CAD with those planes (`SCAN.SECT.CUT`), section comparison (`SCAN.SECT.WHISK`) and cloud
//! slicing (`SCAN.SECT.SLICE`) are not built yet.

pub mod def;

pub use def::{
    canonical_json, digest, expand, validate, ElementRef, ElementResolver, FromElement, ModelResolver, NoElements,
    Parallel, ParallelSpan, Plane, Radial, RadialSpan, Reason, ResolveError, Resolved, ResolvedGeometry,
    ResolverBuildError, SectError, SectionDef,
};
