//! Geometry element types of Metrology Maven (spec node `GEO.TYPES`). Pure Rust, `f64`
//! metres, no I/O, no `unsafe`; kernel graph, KNP-1 (no third-party dependencies).
//!
//! This version implements leaf `GEO.TYPES.ENV`: the [`GeometryElement`] envelope, the
//! closed [`ElementKind`] set, [`FrameRef`], [`Aabb`], [`CadBodyRef`], the shared
//! [`GeoTypeError`], the two sanctioned kind conversions and the [`PointSource`] trait.
//! [`PointSet`], [`PointCloud`] and [`Mesh`] hold only the minimal fields the envelope
//! needs; `GEO.TYPES.POINTS` and `GEO.TYPES.MESH` extend them.
//!
//! # Compile-time guarantees (AC-5)
//!
//! The kinds are distinct types with no `From`, `Into` or `Deref` between them
//! (REQ-GEO.TYPES.ENV-001), and no element has a `Default` (REQ-GEO.TYPES.ENV-002).
//! Two compiling control blocks (the named conversions, and construction with a frame)
//! use the same imports and shapes as the `compile_fail` blocks, so each failure is the
//! guarantee and not an unrelated mistake; each `compile_fail` block also names its
//! expected error code.
//!
//! The only conversions are the named functions:
//!
//! ```
//! use fid_geo_types::{FrameRef, Mesh, PointCloud, PointSet};
//! fn convert(mesh: Mesh, cloud: PointCloud) -> (PointSet, PointSet) {
//!     let p: PointSet = mesh.vertices_to_point_set();
//!     let q: PointSet = cloud.to_point_set();
//!     (p, q)
//! }
//! let frame = FrameRef::from_bytes([7; 16]);
//! let mesh = Mesh::new(frame, &[[0.0, 0.0, 0.0]], &[]).unwrap();
//! let cloud = PointCloud::from_points(frame, &[[1.0, 2.0, 3.0]]).unwrap();
//! let (p, q) = convert(mesh, cloud);
//! assert_eq!((p.frame(), q.frame()), (frame, frame));
//! ```
//!
//! A mesh does not convert into a point set implicitly:
//!
//! ```compile_fail,E0277
//! use fid_geo_types::{Mesh, PointSet};
//! fn convert(mesh: Mesh) -> PointSet {
//!     let p: PointSet = mesh.into();
//!     p
//! }
//! ```
//!
//! Nor does a point cloud, and a cloud does not deref to a point set:
//!
//! ```compile_fail,E0277
//! use fid_geo_types::{PointCloud, PointSet};
//! fn convert(cloud: PointCloud) -> PointSet {
//!     let p: PointSet = cloud.into();
//!     p
//! }
//! ```
//!
//! ```compile_fail,E0308
//! use fid_geo_types::{PointCloud, PointSet};
//! fn view(cloud: &PointCloud) -> &PointSet {
//!     let p: &PointSet = cloud;
//!     p
//! }
//! ```
//!
//! Nor a point set into a mesh or a CAD body handle:
//!
//! ```compile_fail,E0277
//! use fid_geo_types::{Mesh, PointSet};
//! fn convert(points: PointSet) -> Mesh {
//!     let m: Mesh = points.into();
//!     m
//! }
//! ```
//!
//! ```compile_fail,E0277
//! use fid_geo_types::{CadBodyRef, PointSet};
//! fn convert(points: PointSet) -> CadBodyRef {
//!     let c: CadBodyRef = points.into();
//!     c
//! }
//! ```
//!
//! Every element is built with a frame:
//!
//! ```
//! use fid_geo_types::{FrameRef, PointSet};
//! let p = PointSet::new(FrameRef::from_bytes([0; 16]), vec![], vec![], vec![]).unwrap();
//! assert!(p.is_empty());
//! ```
//!
//! and there is no `Default` that could skip it:
//!
//! ```compile_fail,E0599
//! use fid_geo_types::PointSet;
//! let p = PointSet::default();
//! ```
//!
//! ```compile_fail,E0599
//! use fid_geo_types::PointCloud;
//! let p = PointCloud::default();
//! ```
//!
//! ```compile_fail,E0599
//! use fid_geo_types::Mesh;
//! let m = Mesh::default();
//! ```
//!
//! ```compile_fail,E0599
//! use fid_geo_types::CadBodyRef;
//! let c = CadBodyRef::default();
//! ```
//!
//! ```compile_fail,E0599
//! use fid_geo_types::FrameRef;
//! let f = FrameRef::default();
//! ```

pub mod envelope;
pub mod error;
pub mod mesh;
pub mod points;
pub mod source;

pub use envelope::{Aabb, CadBodyRef, ElementKind, FrameRef, GeometryElement};
pub use error::GeoTypeError;
pub use mesh::Mesh;
pub use points::{PointCloud, PointSet};
pub use source::{Channel, InMemorySource, PointBlock, PointSource};
