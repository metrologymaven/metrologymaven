//! Mesh operations of Metrology Maven shared by polygonization (`SCAN.POLY`) and mesh
//! editing (`SCAN.MESHOP`). Pure Rust, `f64` metres, no I/O, no `unsafe`, no threads;
//! kernel layer, KNP-1.
//!
//! Implemented leaves: `SCAN.POLY.QUAL` (the [`MeshQualityReport`] of a GEO.TYPES
//! [`Mesh`](fid_geo_types::Mesh) and [`mesh_quality`] that computes it, see [`quality`]).

mod number;
pub mod quality;

pub use quality::{mesh_quality, AngleStats, MeshQualityReport, QualityError, DEFAULT_DEGENERATE_AREA};
