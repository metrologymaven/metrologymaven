//! Cloud polygonization kernel of Metrology Maven (`SCAN.POLY.SURF`). Pure Rust, `f64`
//! metres, no I/O, no `unsafe`; kernel layer, KNP-1.
//!
//! Implemented leaf: `SCAN.POLY.SURF.PREP`, the vertex set the reconstructor triangulates:
//! one measured representative per grid cell and, when smoothing is on, a bounded move of
//! each representative onto its local least-squares plane (see [`prep`]).

pub mod prep;

pub use prep::{
    prepare, PolyError, PrepParams, PrepVertex, VertexSet, VertexStatus, CLAMP_FACTOR, DEFAULT_EDGE_ANGLE_RAD,
    MAX_CELL_INDEX, MAX_H, MAX_THREADS, MIN_NEIGHBOURS, PLANE_RANK_TOL,
};
