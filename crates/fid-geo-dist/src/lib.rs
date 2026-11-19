//! Point-to-geometry distance for Metrology Maven (spec node GEO.DIST). Pure Rust, `f64`,
//! no I/O, no `unsafe`, no third-party dependencies: only the kernel-layer workspace crates
//! `fid-geo-types`, `fid-geo-index` and `fid-num` (KNP-1, CONSTITUTION §4).
//!
//! Modules land one spec leaf at a time; see `spec/geo/dist/`.
//!
//! - [`mesh`] (GEO.DIST.MESH): closest point on a triangle (Voronoi-region classification with
//!   pinned evaluation order and degenerate fallback) and on a whole mesh through the
//!   `fid-geo-index` hierarchy, bit-identical to a brute-force scan.
//!
//! The crate also holds the reference fixtures for trimmed-face projection
//! (GEO.DIST.FACE.REF): the data lives in
//! `fixtures/geo/dist/face/ref_v1/`, is produced by the independent oracle
//! `tools/fixtures/face_proj_oracle.py`, and is read and compared in
//! `tests/face_ref_compare.rs`. GEO.DIST.FACE.ONE and GEO.DIST.FACE.BODY plug their
//! projectors into that comparison.

pub mod mesh;

pub use mesh::{
    closest_on_mesh, closest_on_triangle, DistError, Feature, MeshHit, MeshProjection, MeshQuery, MeshTarget,
    TriangleFoot, TriangleSubset,
};

/// Comparison tolerance for foot points and distances against the face reference fixtures,
/// in metres (REQ-GEO.DIST.FACE.REF-005, matching REQ-GEO.DIST.FACE-002).
pub const FACE_REF_FOOT_TOL_M: f64 = 1e-9;

/// Window, in metres, within which two faces count as equally close; the lower face ID wins
/// (REQ-GEO.DIST.FACE-004).
pub const FACE_TIE_WINDOW_M: f64 = 1e-15;
