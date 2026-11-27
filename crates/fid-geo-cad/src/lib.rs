//! CAD boundary representation for Metrology Maven (spec node `GEO.CAD`). Pure Rust, no I/O,
//! no `unsafe`; kernel graph, KNP-1 (no third-party dependencies, no FMA, no `f64::min` or
//! `f64::max`, no hash containers, NaN is always an error).
//!
//! Implemented leaf: `GEO.CAD.TOPO.MODEL` (see [`topo`]): the B-rep arena of bodies, shells,
//! faces, loops, coedges, edges and vertices over GEO.NURBS curve and surface definitions,
//! typed stable IDs, the [`BodyBuilder`] importers call, and the canonical binary encoding v1
//! with its SHA-256 [`body_digest`].
//!
//! Implemented leaf: `GEO.CAD.TOPO.ADJ` (see [`topo::adj`]): [`Adjacency`], "what touches
//! what" on a built body, every answer in ascending `TopologyId` order.
//!
//! Implemented leaf: `GEO.CAD.TOPO.PCURVE` (see [`topo::pcurve`]): [`derive_pcurves`], the
//! imported or derived trimming curve of every coedge, with its measured 3-D deviation, and
//! the per-face parameter chart that unwraps loops on periodic surfaces.
//!
//! Implemented leaf: `GEO.CAD.TOPO.TRIM` (see [`topo::trim`]): [`TrimIndex`] and
//! [`classify_uv`], whether a parameter lies inside, outside or on the boundary of a trimmed
//! face, with exact crossing decisions and a boundary band stated in metres.
//!
//! Implemented leaf: `GEO.CAD.ASSY.MODEL` (see [`assy::model`]): [`Instance`], one placed
//! occurrence of a definition body with its frame and [`InstancePath`]; per-instance
//! [`InstanceEntityId`]s for faces, edges and vertices (UUIDv5 over the path and the
//! definition entity's ID); the sorted two-way [`InstanceIndex`]; and [`InstanceView`], the
//! definition geometry plus the instance frame, never a transformed copy.
//!
//! Implemented leaf: `GEO.CAD.TOPO.VALID` (see [`topo::valid`]): [`validate`], every defect
//! of a built body (open or self-intersecting loops, dangling and non-manifold edges,
//! inconsistent orientation, edge curves off their surfaces, pcurve disagreement, PCURVE
//! failures and doubly periodic windings) by kind and entity ID, without changing the body.
//!
//! Implemented leaf: `GEO.CAD.TESS.EDGE` (see [`tess::edge`]): [`tessellate_edges`], every edge
//! sampled once under chordal and angular tolerances by fixed left-to-right bisection, one
//! shared mesh vertex per body vertex and interior sample, the samples in every coedge's face
//! chart, and the per-edge surface gap.
//!
//! Implemented leaf: `GEO.CAD.NOMINAL.DIRECT` (see [`nominal`]): [`map_face`], [`map_edge`]
//! and [`map_vertex`], typed [`NominalElement`]s whose placement and sizes are copied
//! bit-identically from analytic faces, edges and vertices, with the material side from the
//! face sense and extents from the boundary in closed form; NURBS gives `NotAnalytic` and an
//! ellipse `UnsupportedKind`, never an approximation.
//!
//! The commands (`GEO.CAD.TOPO.CMD`) are another leaf.

pub mod assy;
pub mod nominal;
pub mod tess;
pub mod topo;

pub use assy::{
    AssyModelError, BodyId, Instance, InstanceEntityId, InstanceId, InstanceIndex, InstanceIx, InstancePath,
    InstanceView, ASSY_NAMESPACE, MAX_INSTANCE_ENTITIES, MAX_PATH_DEPTH,
};
pub use nominal::{
    map_edge, map_face, map_vertex, DirectError, DirectResult, Extent, Material, Method, NominalElement, NominalKind,
    NominalParams, UnsupportedKind,
};
pub use tess::{
    sample_curve, tessellate_edges, CoedgeUv, CurveSamples, EdgeReport, EdgeSamples, EdgeTess, EdgeTessError,
    EdgeTessParams, SampleError, UvError,
};
pub use topo::{
    body_digest, classify_uv, decode, derive_coedge, derive_pcurves, encode, pcurve_disagreement, AdjError, Adjacency,
    Body, BodyBuilder, Coedge, CoedgeIx, Curve2, CurveDef, CurveParams, DecodeError, DisagreementError, Edge, EdgeIx,
    EntityIx, EntityKind, Face, FaceChart, FaceIx, FailReason, Frame, GeomError, IndexPath, InversionFault, Label,
    Loop, LoopIx, PairingFault, PcurveError, Pcurves, Sense, Shell, ShellIx, SurfaceDef, SurfaceParams, TopoBuildError,
    TopologyId, TrimClass, TrimCurve, TrimError, TrimIndex, TrimSeg, Undecidable, Vertex, VertexIx, DEFAULT_TOL_M,
};
pub use topo::{validate, Defect, DefectKind, Unchecked, ValidError, ValidationReport, DEFAULT_VALID_TOL_M};
