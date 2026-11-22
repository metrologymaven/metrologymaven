//! Spatial indexes for Metrology Maven (spec node `GEO.INDEX`). Pure Rust, `f64` metres, no I/O,
//! no `unsafe`, no threads; kernel graph, KNP-1 (CONSTITUTION §4).
//!
//! Implemented leaves:
//!
//! - [`budget`]: memory budgets, exact pre-allocation estimates and a digest-keyed LRU cache
//!   [`IndexCache`] (`GEO.INDEX.BUDGET`): refusal before allocation when the estimate exceeds
//!   the declared budget, deterministic monotonic-counter LRU eviction with ties by key bytes,
//!   and observational [`BuildReport`].
//! - [`bvh`]: the triangle bounding volume hierarchy [`PrimitiveBvh`] (`GEO.INDEX.BVH`): a
//!   deterministic median-split build, conservative sphere, box and ray candidate queries with
//!   results in ascending triangle order, the nearest triangle box, and the ordered leaf visitor
//!   used by closest-point search.
//! - [`kdtree`]: the point k-d tree [`PointIndex`] / [`PointTree`] (`GEO.INDEX.KDTREE`): an
//!   implicit tree over a point buffer with exact k-nearest, radius and box queries whose results
//!   equal a brute-force scan, ties ordered by point index.

pub mod budget;
pub mod bvh;
pub mod kdtree;

pub use budget::{
    build_bvh, build_bvh_triangles, build_point_index, build_point_tree, checked_estimate_bytes, estimate_bytes,
    BudgetError, BuildParams, BuildReport, CacheLookup, CachedIndex, IndexCache, IndexKey, IndexKind, MemoryBudget,
};
pub use bvh::{
    bvh_build_bytes, BvhError, BvhNode, LeafVisit, NearestBox, PrimitiveBvh, QueryField, COORD_LIMIT, LEAF_SIZE,
    MAX_TRIANGLES, ROUNDOFF_MARGIN, UNDERFLOW_MARGIN,
};
pub use kdtree::{
    dist2, leaf_depth, overhead_bytes, Neighbour, PointIndex, PointIndexError, PointTree, BUCKET, MAX_K, MAX_POINTS,
};
