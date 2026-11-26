//! B-rep topology (spec node `GEO.CAD.TOPO`).
//!
//! - [`geom`]: the curve and surface definitions faces, edges and coedges hold.
//! - [`model`]: arena types, typed IDs and indexes, [`BodyBuilder`] and its checks
//!   (`GEO.CAD.TOPO.MODEL`).
//! - [`encode`]: canonical encoding v1, the bounded decoder and [`body_digest`].
//! - [`adj`]: [`Adjacency`], the derived adjacency index and its queries (`GEO.CAD.TOPO.ADJ`).
//! - [`pcurve`]: [`derive_pcurves`], a trimming curve for every coedge and a parameter chart
//!   for every face (`GEO.CAD.TOPO.PCURVE`).
//! - [`trim`]: [`TrimIndex`] and [`classify_uv`], inside, outside or on the boundary of a
//!   trimmed face (`GEO.CAD.TOPO.TRIM`).
//! - [`valid`]: [`validate`], every defect of a built body, never a repair
//!   (`GEO.CAD.TOPO.VALID`).
//!
//! The pinned encoding and check order are documented in `spec/geo/cad/topo/model/NODE.md`
//! (Design) and implemented independently by `fixtures/geo/cad/topo/make_topo_fixtures.py`.

pub mod adj;
pub mod encode;
pub mod geom;
pub mod model;
pub mod pcurve;
pub mod trim;
pub(crate) mod uuid;
pub mod valid;

pub use adj::{AdjError, Adjacency};
pub use encode::{body_digest, decode, encode, DecodeError, MAX_ENCODED_BYTES, VERSION};
pub use geom::{Curve2, CurveDef, CurveParams, Frame, GeomError, SurfaceDef, SurfaceParams};
pub use model::{
    Body, BodyBuilder, Coedge, CoedgeIx, Edge, EdgeIx, EntityIx, EntityKind, Face, FaceIx, IndexPath, Label, Loop,
    LoopIx, PairingFault, Sense, Shell, ShellIx, TopoBuildError, TopologyId, Vertex, VertexIx, MAX_ENTITIES,
    MAX_LABEL_BYTES, TOPOLOGY_NAMESPACE,
};
pub use pcurve::{
    derive_coedge, derive_pcurves, pcurve_disagreement, DisagreementError, FaceChart, FailReason, InversionFault,
    PcurveError, Pcurves, TrimCurve, DEFAULT_TOL_M,
};
pub use trim::{classify_uv, TrimClass, TrimError, TrimIndex, TrimSeg, Undecidable};
pub use valid::{
    validate, Defect, DefectKind, Unchecked, ValidError, ValidationReport, DEFAULT_VALID_TOL_M, EDGE_SAMPLES,
};
