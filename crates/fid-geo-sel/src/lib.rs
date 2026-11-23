//! Selection model and algebra of Metrology Maven (spec node `GEO.SEL`). Pure Rust, no I/O,
//! no `unsafe`; kernel graph, KNP-1 (no third-party dependencies).
//!
//! Implemented leaves:
//! - `GEO.SEL.MODEL` (see [`model`]): the [`IndexSet`] of sorted unique indexes stored as
//!   coalesced half-open ranges over one [`Domain`] of an element, the [`Selection`] that
//!   binds an index set (or an opaque [`SelectionQuery`]) to a target element ID and that
//!   element's [`ElementDigest`](fid_geo_types::ElementDigest), the canonical encoding and
//!   SHA-256 [`SelectionDigest`] of an index set, and [`apply`] with the stale-selection check.
//! - `GEO.SEL.ALGEBRA` (see [`algebra`]): immutable linear-time range set operations
//!   ([`union`], [`intersection`], [`difference`], [`sym_difference`], [`invert`] and their
//!   [`Selection`] counterparts), triangle [`grow`]/[`shrink`] by `n` rings of edge or vertex
//!   adjacency, and point [`grow`]/[`shrink`] by radius through `fid-geo-index`.
//!
//! - `GEO.SEL.RESOLVE` (see [`resolve`] and [`raster`]): deterministic CPU resolution of
//!   pixel picks and lasso polygons given explicit [`Camera`] parameters, in `Through` and
//!   `Visible` (CPU scanline depth buffer) modes, across point, triangle and face domains.
//!
//! - `GEO.SEL.QUERY.GRAMMAR` (see [`query`]): closed v1 JSON AST grammar, bounded iterative
//!   parser, canonical form and SHA-256 digest (`QueryDigest`).
//!
//! Query evaluation (`GEO.SEL.QUERY.EVAL`, `NEAR`, `CAD`) and commands (`GEO.SEL.CMD`) are other leaves.

pub mod algebra;
pub mod model;
pub mod query;
pub mod raster;
pub mod resolve;

pub use algebra::{
    difference, difference_selections, grow, grow_points, grow_selection, grow_triangles, intersection,
    intersection_selections, invert, invert_selection, shrink, shrink_points, shrink_selection, shrink_triangles,
    sym_difference, sym_difference_selections, union, union_selections, Adjacency, GrowSpec,
};
pub use model::{
    apply, apply_with_count, domain_count, Domain, EntityId, IndexSet, IndexSetBuilder, SelError, Selection,
    SelectionBody, SelectionDigest, SelectionQuery,
};
pub use raster::{clip_triangle_depth, DepthBuffer, MAX_VIEWPORT_PIXELS};
pub use resolve::{
    point_in_polygon, project_point, ray_triangle_intersect, resolve_lasso, resolve_lasso_faces, resolve_pick,
    resolve_pick_faces, resolve_pick_hit, Camera, Lasso, PickHit, ProjectedPoint, Projection, ResolveMode,
    TriangleRule, DEFAULT_DEPTH_TOL_FRACTION, MAX_PIXEL_COORD, MAX_POLYGON_VERTICES, MIN_POLYGON_VERTICES,
};
