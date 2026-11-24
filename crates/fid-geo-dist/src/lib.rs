//! Point-to-geometry distance for Metrology Maven (spec node GEO.DIST). Pure Rust, `f64`,
//! no I/O, no `unsafe`, no third-party dependencies: only the kernel-layer workspace crates
//! `fid-geo-types`, `fid-geo-index`, `fid-geo-frame`, `fid-geo-nurbs`, `fid-geo-cad` and
//! `fid-num` (KNP-1, CONSTITUTION §4).
//!
//! Modules land one spec leaf at a time; see `spec/geo/dist/`.
//!
//! - [`mesh`] (GEO.DIST.MESH): closest point on a triangle (Voronoi-region classification with
//!   pinned evaluation order and degenerate fallback) and on a whole mesh through the
//!   `fid-geo-index` hierarchy, bit-identical to a brute-force scan.
//! - [`sign`] (GEO.DIST.SIGN): angle-weighted pseudo-normals, topology statuses and the certified
//!   sign of a mesh projection (outside, inside, on the surface, undecidable, or undefined at
//!   boundary, non-manifold, misoriented or degenerate features), and the same rule for CAD feet.
//! - [`face::one`] (GEO.DIST.FACE.ONE): the closest point on one trimmed CAD face from its
//!   exact surface, edge curves and trimming loops: interior stationary points kept by the
//!   GEO.CAD.TOPO.TRIM classification, boundary edges and corners, with fixed tie rules.
//! - [`batch`] (GEO.DIST.BATCH): the [`ProjectionTarget`] trait (implemented for meshes by
//!   [`SignTarget`]), projection options (maximum distance, subset, direction), deterministic
//!   multi-threaded [`project_batch`] with cancellation, and fixed-order [`summary`]
//!   statistics.
//!
//! The crate also holds the reference fixtures for trimmed-face projection
//! (GEO.DIST.FACE.REF): the data lives in
//! `fixtures/geo/dist/face/ref_v1/`, is produced by the independent oracle
//! `tools/fixtures/face_proj_oracle.py`, and is read and compared in
//! `tests/face_ref_compare.rs`. GEO.DIST.FACE.ONE and GEO.DIST.FACE.BODY plug their
//! projectors into that comparison.

pub mod batch;
pub mod face;
pub mod mesh;
pub mod sign;

pub use batch::{
    encode, project_batch, summary, BatchConfig, BatchError, BatchSummary, HitSign, ProjectError, Projection,
    ProjectionHit, ProjectionOptions, ProjectionTarget,
};

pub use face::one::{
    project_face, FaceError, FaceFeature, FaceHit, FaceProjection, FaceProjector, FaceQueryOptions, Unsupported,
};
pub use mesh::{
    closest_on_mesh, closest_on_triangle, DistError, Feature, MeshHit, MeshProjection, MeshQuery, MeshTarget,
    TriangleFoot, TriangleSubset,
};
pub use sign::{
    cad_sign, EdgeNormal, FeatureNormal, PseudoNormals, SignError, SignStatus, SignTarget, SignUndefined, Signed,
    Topology,
};

/// Comparison tolerance for foot points and distances against the face reference fixtures,
/// in metres (REQ-GEO.DIST.FACE.REF-005, matching REQ-GEO.DIST.FACE-002).
pub const FACE_REF_FOOT_TOL_M: f64 = 1e-9;

/// Window, in metres, within which two faces count as equally close; the lower face ID wins
/// (REQ-GEO.DIST.FACE-004).
pub const FACE_TIE_WINDOW_M: f64 = 1e-15;
