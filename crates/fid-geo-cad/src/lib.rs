//! CAD boundary representation for Metrology Maven (spec node `GEO.CAD`). Pure Rust, no I/O,
//! no `unsafe`; kernel graph, KNP-1 (no third-party dependencies, no FMA, no `f64::min` or
//! `f64::max`, no hash containers, NaN is always an error).
//!
//! Implemented leaf: `GEO.CAD.TOPO.MODEL` (see [`topo`]): the B-rep arena of bodies, shells,
//! faces, loops, coedges, edges and vertices over GEO.NURBS curve and surface definitions,
//! typed stable IDs, the [`BodyBuilder`] importers call, and the canonical binary encoding v1
//! with its SHA-256 [`body_digest`].
//!
//! Adjacency (`GEO.CAD.TOPO.ADJ`), pcurve derivation (`GEO.CAD.TOPO.PCURVE`), trim
//! classification (`GEO.CAD.TOPO.TRIM`), geometric validation (`GEO.CAD.TOPO.VALID`) and the
//! commands (`GEO.CAD.TOPO.CMD`) are other leaves.

pub mod topo;

pub use topo::{
    body_digest, decode, encode, Body, BodyBuilder, Coedge, CoedgeIx, Curve2, CurveDef, CurveParams, DecodeError, Edge,
    EdgeIx, EntityIx, EntityKind, Face, FaceIx, Frame, GeomError, IndexPath, Label, Loop, LoopIx, PairingFault, Sense,
    Shell, ShellIx, SurfaceDef, SurfaceParams, TopoBuildError, TopologyId, Vertex, VertexIx,
};
