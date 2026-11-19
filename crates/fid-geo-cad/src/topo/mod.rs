//! B-rep topology (spec node `GEO.CAD.TOPO`).
//!
//! - [`geom`]: the curve and surface definitions faces, edges and coedges hold.
//! - [`model`]: arena types, typed IDs and indexes, [`BodyBuilder`] and its checks
//!   (`GEO.CAD.TOPO.MODEL`).
//! - [`encode`]: canonical encoding v1, the bounded decoder and [`body_digest`].
//! - [`adj`]: [`Adjacency`], the derived adjacency index and its queries (`GEO.CAD.TOPO.ADJ`).
//!
//! The pinned encoding and check order are documented in `spec/geo/cad/topo/model/NODE.md`
//! (Design) and implemented independently by `fixtures/geo/cad/topo/make_topo_fixtures.py`.

pub mod adj;
pub mod encode;
pub mod geom;
pub mod model;
mod uuid;

pub use adj::{AdjError, Adjacency};
pub use encode::{body_digest, decode, encode, DecodeError, MAX_ENCODED_BYTES, VERSION};
pub use geom::{Curve2, CurveDef, CurveParams, Frame, GeomError, SurfaceDef, SurfaceParams};
pub use model::{
    Body, BodyBuilder, Coedge, CoedgeIx, Edge, EdgeIx, EntityIx, EntityKind, Face, FaceIx, IndexPath, Label, Loop,
    LoopIx, PairingFault, Sense, Shell, ShellIx, TopoBuildError, TopologyId, Vertex, VertexIx, MAX_ENTITIES,
    MAX_LABEL_BYTES, TOPOLOGY_NAMESPACE,
};
