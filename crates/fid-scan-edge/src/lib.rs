//! Scan edge analysis of Metrology Maven (spec node `SCAN.EDGE`). Pure Rust, `f64` metres, no
//! I/O, no `unsafe`; kernel graph, KNP-1 (no third-party dependencies).
//!
//! Implemented leaves:
//!
//! * `SCAN.EDGE.EST.EFIX`: the synthetic edge fixture generator and the named fixture set, in
//!   [`testkit`] behind the `testkit` feature (test support only; never enabled in a release
//!   build).
//!
//! * `SCAN.EDGE.NOMEDGE`: [`nominal::stations`], the nominal edge chain of CAD boundary edges
//!   on one face (or a constructed curve on its face) and its stations at a stated arc-length
//!   spacing, each with the orthonormal frame (tangent, outward face normal, outward in-surface
//!   binormal) on exact curve and surface evaluation.
//!
//! The edge-estimation methods (`SCAN.EDGE.EST.TANG`, `SCAN.EDGE.EST.SILH`), their default
//! (`SCAN.EDGE.EST.DEFS`) and the extraction interface (`SCAN.EDGE.MESHBND`) are not built yet.

pub mod nominal;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use nominal::{
    stations, CadResolver, ChainPiece, EdgeError, FootFault, MaterialSide, NominalEdge, NominalStations, Station,
};
