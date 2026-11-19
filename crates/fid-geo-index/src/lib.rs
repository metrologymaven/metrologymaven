//! Spatial indexes for Metrology Maven (spec node `GEO.INDEX`). Pure Rust, `f64` metres, no I/O,
//! no `unsafe`, no threads; kernel graph, KNP-1 (CONSTITUTION §4).
//!
//! Implemented leaves:
//!
//! - [`bvh`]: the triangle bounding volume hierarchy [`PrimitiveBvh`] (`GEO.INDEX.BVH`): a
//!   deterministic median-split build, conservative sphere, box and ray candidate queries with
//!   results in ascending triangle order, the nearest triangle box, and the ordered leaf visitor
//!   used by closest-point search.

pub mod bvh;

pub use bvh::{
    BvhError, BvhNode, LeafVisit, NearestBox, PrimitiveBvh, QueryField, COORD_LIMIT, LEAF_SIZE, MAX_TRIANGLES,
    ROUNDOFF_MARGIN, UNDERFLOW_MARGIN,
};
