//! Mesh format readers, writers and shared builder of Metrology Maven (spec node
//! `IO.MESH`).
//!
//! Implemented leaves:
//! - `IO.MESH.MBLD`: [`MeshBuilder`], [`AttrTable`], [`ScalarColumn`], [`SourcePrecision`],
//!   [`MeshImportCounts`], [`BuiltMesh`], [`BuiltPointSet`], [`BuiltGeometry`],
//!   [`PrecisionReport`], [`f32_rounding_report`], [`count_non_manifold_edges`] and
//!   [`MeshBuildError`].
//!
//! Every mesh reader (STL, PLY, OBJ, 3MF) feeds [`MeshBuilder`]: vertices and faces are
//! kept in exact input order without merging, reordering, repairing or dropping elements
//! (REQ-IO.MESH.MBLD-001). Degenerate faces, non-manifold edges and unreferenced vertices
//! are counted exactly for the import report (REQ-IO.MESH.MBLD-002), out-of-range vertex
//! indices fail with `malformed` naming the offending face index (REQ-IO.MESH.MBLD-003),
//! and [`f32_rounding_report`] measures the maximum single-precision coordinate rounding
//! error in metres in `f64` (REQ-IO.MESH.MBLD-004).

pub mod attrs;
pub mod builder;
pub mod error;
pub mod precision;

pub use attrs::{AttrTable, CustomScalarType, ScalarColumn};
pub use builder::{
    count_non_manifold_edges, BuiltGeometry, BuiltMesh, BuiltPointSet, MeshBuilder, MeshImportCounts, SourcePrecision,
};
pub use error::MeshBuildError;
pub use precision::{f32_rounding_report, f32_rounding_report_buffer, f32_rounding_report_point_set, PrecisionReport};
