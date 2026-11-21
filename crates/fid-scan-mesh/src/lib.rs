//! Mesh operations of Metrology Maven shared by polygonization (`SCAN.POLY`) and mesh
//! editing (`SCAN.MESHOP`). Pure Rust, `f64` metres, no I/O, no `unsafe`, no threads;
//! kernel layer, KNP-1.
//!
//! Implemented leaves: `SCAN.POLY.QUAL` (the [`MeshQualityReport`] of a GEO.TYPES
//! [`Mesh`](fid_geo_types::Mesh) and [`mesh_quality`] that computes it, see [`quality`]) and
//! `SCAN.MESHOP.PROV` (the [`MeshOp`] contract, its [`MeshOpBuilder`], the per-triangle
//! [`Synthesized`] flag and the [`MeshOpOutput`] provenance record, see [`op`]).

mod number;
pub mod op;
pub mod quality;

pub use op::{
    check_determinism, check_input_intact, derived_digest, MeshOp, MeshOpBuilder, MeshOpError, MeshOpLimits,
    MeshOpOutput, MeshView, OpParams, ParamsWriter, Registered, RunOptions, Synthesized, TriRef, VerifiedMeshView,
};
pub use quality::{mesh_quality, AngleStats, MeshQualityReport, QualityError, DEFAULT_DEGENERATE_AREA};
