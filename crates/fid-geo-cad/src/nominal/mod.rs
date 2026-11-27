//! Nominal elements from CAD faces, edges and vertices (spec node `GEO.CAD.NOMINAL`).
//!
//! - [`types`]: [`NominalElement`], [`NominalKind`], [`NominalParams`], [`Extent`],
//!   [`Material`], [`Method`] and [`DirectResult`] (`GEO.CAD.NOMINAL.DIRECT`).
//! - [`direct`]: [`map_face`], [`map_edge`] and [`map_vertex`], analytic geometry copied
//!   exactly, never approximated (`GEO.CAD.NOMINAL.DIRECT`).
//!
//! NURBS recognition (`RECOG`), merging (`MERGE`) and the command (`CMD`) are other leaves.

pub mod direct;
pub mod types;

pub use direct::{map_edge, map_face, map_vertex, DirectError};
pub use types::{DirectResult, Extent, Material, Method, NominalElement, NominalKind, NominalParams, UnsupportedKind};
