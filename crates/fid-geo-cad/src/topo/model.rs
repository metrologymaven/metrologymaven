//! B-rep arena, typed stable IDs, the builder and its topology checks. Spec node
//! `GEO.CAD.TOPO.MODEL`.
//!
//! An importer adds entities to a [`BodyBuilder`] in any order, referring to earlier or later
//! entities by their builder index (the position returned by the `add_*` call of that kind).
//! [`BodyBuilder::build`] checks the description, puts every entity in canonical order and
//! assigns IDs; a [`Body`] that exists satisfies every check below, and the decoder in
//! [`super::encode`] goes through the same `build`.
//!
//! # Check order (first failure wins)
//!
//! 0. Total entity count at most [`MAX_ENTITIES`] (`TooLarge`).
//! 1. Per entity, kinds in the order vertices, edges, coedges, loops, faces, shells, each in
//!    builder order, fields in declaration order: label length (`BadLabel`), finite values
//!    (`NonFinite`), edge range `t0 < t1` (`BadParamRange`), references in range
//!    (`BadReference`), non-empty loop (`EmptyLoop`), at most one outer loop per face
//!    (`MultipleOuter`), non-empty shell (`EmptyShell`).
//! 2. Ownership: walking shells, then their faces, then their loops in builder order, each
//!    face, loop and coedge must have exactly one owner (`Reused` for the second owner), then
//!    faces, loops, coedges, edges and vertices in builder order must all be reached
//!    (`Orphan`).
//! 3. Loop closure, loops in builder order: the end vertex of coedge `k` (respecting its
//!    sense) is the start vertex of coedge `k + 1`, cyclically (`OpenLoop`).
//! 4. Coedge pairing, edges in builder order (`BadPairing`): at most two coedges per edge;
//!    two coedges have opposite senses and lie in one shell; in a closed shell every edge
//!    has exactly two.
//! 5. Canonical order and IDs; the first repeated ID, inserting shells, faces, loops,
//!    coedges, edges, vertices in canonical order, fails `DuplicateId`.

use core::fmt;
use std::collections::BTreeMap;

use super::geom::{canon, canon3, Curve2, CurveDef, SurfaceDef};
use super::uuid::{format_uuid, uuid_v5};

/// Largest number of entities (all kinds together) in one body (REQ-007).
pub const MAX_ENTITIES: usize = 10_000_000;

/// Longest imported label in UTF-8 bytes (REQ-001).
pub const MAX_LABEL_BYTES: usize = 256;

/// The project namespace of derived topology IDs: UUIDv5 of
/// `https://github.com/metrologymaven/metrologymaven/spec/geo/cad/topo/model#topology-id-v1`
/// in the RFC 9562 URL namespace, i.e. `0932dc8b-f062-5d17-b3eb-8089eddd9e6e` (checked by a test).
pub const TOPOLOGY_NAMESPACE: [u8; 16] =
    [0x09, 0x32, 0xdc, 0x8b, 0xf0, 0x62, 0x5d, 0x17, 0xb3, 0xeb, 0x80, 0x89, 0xed, 0xdd, 0x9e, 0x6e];

/// The kind of a topological entity, with its stable one-byte code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntityKind {
    Shell,
    Face,
    Loop,
    Coedge,
    Edge,
    Vertex,
}

impl EntityKind {
    /// Code in the UUIDv5 name: shell 1, face 2, loop 3, coedge 4, edge 5, vertex 6.
    pub fn code(self) -> u8 {
        match self {
            EntityKind::Shell => 1,
            EntityKind::Face => 2,
            EntityKind::Loop => 3,
            EntityKind::Coedge => 4,
            EntityKind::Edge => 5,
            EntityKind::Vertex => 6,
        }
    }

    /// Lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            EntityKind::Shell => "shell",
            EntityKind::Face => "face",
            EntityKind::Loop => "loop",
            EntityKind::Coedge => "coedge",
            EntityKind::Edge => "edge",
            EntityKind::Vertex => "vertex",
        }
    }
}

/// Orientation of a face against its surface normal, or of a coedge against its edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sense {
    Same,
    Reversed,
}

/// An imported label: 1 to [`MAX_LABEL_BYTES`] bytes of UTF-8, held as untrusted data
/// (CONSTITUTION C6). Ordered bytewise.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Label(String);

impl Label {
    /// `None` when the length is outside `1..=MAX_LABEL_BYTES`.
    pub fn new(s: &str) -> Option<Label> {
        if s.is_empty() || s.len() > MAX_LABEL_BYTES {
            None
        } else {
            Some(Label(s.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A stable topology ID. The derived order is the total order of REQ-008: every imported
/// label before every derived UUID, then bytewise.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TopologyId {
    Imported(Label),
    Derived([u8; 16]),
}

impl fmt::Display for TopologyId {
    /// A label is shown quoted and escaped (`{:?}`), never raw (C6); a UUID hyphenated.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TopologyId::Imported(l) => write!(f, "{:?}", l.as_str()),
            TopologyId::Derived(b) => f.write_str(&format_uuid(b)),
        }
    }
}

macro_rules! dense_index {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(u32);

        impl $name {
            /// Position in canonical order.
            pub fn index(self) -> usize {
                self.0 as usize
            }

            /// The index at canonical position `i` (callers pass positions of an existing
            /// body, which are below [`MAX_ENTITIES`]).
            #[allow(dead_code)]
            pub(crate) fn at(i: usize) -> $name {
                $name(ix32(i))
            }
        }
    };
}

dense_index!(
    /// Dense shell index in canonical order.
    ShellIx
);
dense_index!(
    /// Dense face index in canonical order.
    FaceIx
);
dense_index!(
    /// Dense loop index in canonical order.
    LoopIx
);
dense_index!(
    /// Dense coedge index in canonical order.
    CoedgeIx
);
dense_index!(
    /// Dense edge index in canonical order.
    EdgeIx
);
dense_index!(
    /// Dense vertex index in canonical order.
    VertexIx
);

/// Any entity of a body, by dense index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntityIx {
    Shell(ShellIx),
    Face(FaceIx),
    Loop(LoopIx),
    Coedge(CoedgeIx),
    Edge(EdgeIx),
    Vertex(VertexIx),
}

impl EntityIx {
    /// The kind of this entity.
    pub fn kind(self) -> EntityKind {
        match self {
            EntityIx::Shell(_) => EntityKind::Shell,
            EntityIx::Face(_) => EntityKind::Face,
            EntityIx::Loop(_) => EntityKind::Loop,
            EntityIx::Coedge(_) => EntityKind::Coedge,
            EntityIx::Edge(_) => EntityKind::Edge,
            EntityIx::Vertex(_) => EntityKind::Vertex,
        }
    }
}

/// A canonical index path: shell `[s]`, face `[s, f]`, loop `[s, f, l]`, coedge
/// `[s, f, l, c]` (positions within the parent's list); an edge takes the path of the first
/// coedge that uses it, a vertex that path plus `0` (edge start) or `1` (edge end) of its
/// first use.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IndexPath {
    pub kind: EntityKind,
    pub steps: Vec<u32>,
}

impl fmt::Display for IndexPath {
    /// `face[0,3]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[", self.kind.as_str())?;
        for (i, p) in self.steps.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            write!(f, "{p}")?;
        }
        f.write_str("]")
    }
}

/// Which pairing rule an edge breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingFault {
    /// More than two coedges use the edge.
    TooMany,
    /// The edge's two coedges have the same sense.
    SameSense,
    /// The edge's two coedges lie in different shells.
    CrossShell,
    /// The edge is used once in the closed shell with this builder index.
    Unpaired { shell: usize },
}

/// Why [`BodyBuilder::build`] refused a description. Indexes are builder indexes.
#[derive(Debug, Clone, PartialEq)]
pub enum TopoBuildError {
    /// The description holds more than [`MAX_ENTITIES`] entities.
    TooLarge { entities: usize },
    /// A label is empty or longer than [`MAX_LABEL_BYTES`] bytes.
    BadLabel { kind: EntityKind, index: usize, len: usize },
    /// A vertex point or edge parameter is NaN or infinite.
    NonFinite { kind: EntityKind, index: usize },
    /// An edge parameter range is not strictly increasing.
    BadParamRange { edge: usize },
    /// `referrer` names a `target` index that does not exist.
    BadReference { kind: EntityKind, index: usize, target: EntityKind, target_index: usize },
    /// A loop has no coedges.
    EmptyLoop { r#loop: usize },
    /// A face has more than one loop marked outer.
    MultipleOuter { face: usize },
    /// A shell has no faces.
    EmptyShell { shell: usize },
    /// A face, loop or coedge is listed by two owners (or twice by one).
    Reused { kind: EntityKind, index: usize, owner: EntityKind, first: usize, second: usize },
    /// An entity is not reachable from any shell.
    Orphan { kind: EntityKind, index: usize },
    /// The coedge at `position` of the loop does not end where the next one starts.
    OpenLoop { r#loop: usize, position: usize },
    /// An edge breaks a pairing rule.
    BadPairing { edge: usize, fault: PairingFault },
    /// Two entities would get the same ID.
    DuplicateId { id: TopologyId, first: IndexPath, second: IndexPath },
}

impl TopoBuildError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            TopoBuildError::TooLarge { .. } => "TooLarge",
            TopoBuildError::BadLabel { .. } => "BadLabel",
            TopoBuildError::NonFinite { .. } => "NonFinite",
            TopoBuildError::BadParamRange { .. } => "BadParamRange",
            TopoBuildError::BadReference { .. } => "BadReference",
            TopoBuildError::EmptyLoop { .. } => "EmptyLoop",
            TopoBuildError::MultipleOuter { .. } => "MultipleOuter",
            TopoBuildError::EmptyShell { .. } => "EmptyShell",
            TopoBuildError::Reused { .. } => "Reused",
            TopoBuildError::Orphan { .. } => "Orphan",
            TopoBuildError::OpenLoop { .. } => "OpenLoop",
            TopoBuildError::BadPairing { .. } => "BadPairing",
            TopoBuildError::DuplicateId { .. } => "DuplicateId",
        }
    }
}

impl fmt::Display for TopoBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TopoBuildError::TooLarge { entities } => {
                write!(f, "body has {entities} entities, above {MAX_ENTITIES}")
            }
            TopoBuildError::BadLabel { kind, index, len } => {
                write!(f, "{} {index} label is {len} bytes, outside 1..={MAX_LABEL_BYTES}", kind.as_str())
            }
            TopoBuildError::NonFinite { kind, index } => {
                write!(f, "{} {index} has a non-finite coordinate or parameter", kind.as_str())
            }
            TopoBuildError::BadParamRange { edge } => write!(f, "edge {edge} parameter range is not increasing"),
            TopoBuildError::BadReference { kind, index, target, target_index } => write!(
                f,
                "{} {index} references {} {target_index}, which does not exist",
                kind.as_str(),
                target.as_str()
            ),
            TopoBuildError::EmptyLoop { r#loop } => write!(f, "loop {loop} has no coedges"),
            TopoBuildError::MultipleOuter { face } => write!(f, "face {face} has more than one outer loop"),
            TopoBuildError::EmptyShell { shell } => write!(f, "shell {shell} has no faces"),
            TopoBuildError::Reused { kind, index, owner, first, second } => write!(
                f,
                "{} {index} is used by {} {first} and {} {second}",
                kind.as_str(),
                owner.as_str(),
                owner.as_str()
            ),
            TopoBuildError::Orphan { kind, index } => {
                write!(f, "{} {index} is not reachable from any shell", kind.as_str())
            }
            TopoBuildError::OpenLoop { r#loop, position } => {
                write!(f, "loop {loop} is open after coedge position {position}")
            }
            TopoBuildError::BadPairing { edge, fault } => match fault {
                PairingFault::TooMany => write!(f, "edge {edge}: used by more than two coedges"),
                PairingFault::SameSense => write!(f, "edge {edge}: its two coedges have the same sense"),
                PairingFault::CrossShell => write!(f, "edge {edge}: its two coedges are in different shells"),
                PairingFault::Unpaired { shell } => write!(f, "edge {edge}: used once in closed shell {shell}"),
            },
            TopoBuildError::DuplicateId { id, first, second } => {
                write!(f, "duplicate id {id} at {first} and {second}")
            }
        }
    }
}

impl core::error::Error for TopoBuildError {}

// ---------------------------------------------------------------- arena types

/// A connected set of faces; `closed` declares a closed (solid-bounding) shell.
#[derive(Debug, Clone, PartialEq)]
pub struct Shell {
    id: TopologyId,
    faces: Vec<FaceIx>,
    closed: bool,
}

impl Shell {
    pub fn id(&self) -> &TopologyId {
        &self.id
    }
    pub fn faces(&self) -> &[FaceIx] {
        &self.faces
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
}

/// A bounded region of one surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    id: TopologyId,
    surface: SurfaceDef,
    sense: Sense,
    loops: Vec<LoopIx>,
    shell: ShellIx,
}

impl Face {
    pub fn id(&self) -> &TopologyId {
        &self.id
    }
    pub fn surface(&self) -> &SurfaceDef {
        &self.surface
    }
    pub fn sense(&self) -> Sense {
        self.sense
    }
    pub fn loops(&self) -> &[LoopIx] {
        &self.loops
    }
    pub fn shell(&self) -> ShellIx {
        self.shell
    }
}

/// A closed cycle of coedges bounding a face.
#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    id: TopologyId,
    coedges: Vec<CoedgeIx>,
    outer: bool,
    face: FaceIx,
}

impl Loop {
    pub fn id(&self) -> &TopologyId {
        &self.id
    }
    pub fn coedges(&self) -> &[CoedgeIx] {
        &self.coedges
    }
    pub fn outer(&self) -> bool {
        self.outer
    }
    pub fn face(&self) -> FaceIx {
        self.face
    }
}

/// One use of an edge by a loop.
#[derive(Debug, Clone, PartialEq)]
pub struct Coedge {
    id: TopologyId,
    edge: EdgeIx,
    sense: Sense,
    pcurve: Option<Curve2>,
    r#loop: LoopIx,
}

impl Coedge {
    pub fn id(&self) -> &TopologyId {
        &self.id
    }
    pub fn edge(&self) -> EdgeIx {
        self.edge
    }
    pub fn sense(&self) -> Sense {
        self.sense
    }
    pub fn pcurve(&self) -> Option<&Curve2> {
        self.pcurve.as_ref()
    }
    pub fn r#loop(&self) -> LoopIx {
        self.r#loop
    }
}

/// A bounded piece of a curve between two vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    id: TopologyId,
    curve: Option<CurveDef>,
    start: VertexIx,
    end: VertexIx,
    param_range: [f64; 2],
}

impl Edge {
    pub fn id(&self) -> &TopologyId {
        &self.id
    }
    pub fn curve(&self) -> Option<&CurveDef> {
        self.curve.as_ref()
    }
    pub fn start(&self) -> VertexIx {
        self.start
    }
    pub fn end(&self) -> VertexIx {
        self.end
    }
    pub fn param_range(&self) -> [f64; 2] {
        self.param_range
    }
}

/// A point.
#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    id: TopologyId,
    point: [f64; 3],
}

impl Vertex {
    pub fn id(&self) -> &TopologyId {
        &self.id
    }
    pub fn point(&self) -> [f64; 3] {
        self.point
    }
}

/// A checked body with every entity in canonical order. Only [`BodyBuilder::build`] (and
/// the decoder, which calls it) makes one.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    id: [u8; 16],
    shells: Vec<Shell>,
    faces: Vec<Face>,
    loops: Vec<Loop>,
    coedges: Vec<Coedge>,
    edges: Vec<Edge>,
    vertices: Vec<Vertex>,
    ids: BTreeMap<TopologyId, EntityIx>,
}

impl Body {
    /// The body ID (namespace of its derived IDs).
    pub fn id(&self) -> &[u8; 16] {
        &self.id
    }
    pub fn shells(&self) -> &[Shell] {
        &self.shells
    }
    pub fn faces(&self) -> &[Face] {
        &self.faces
    }
    pub fn loops(&self) -> &[Loop] {
        &self.loops
    }
    pub fn coedges(&self) -> &[Coedge] {
        &self.coedges
    }
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }
    pub fn shell(&self, ix: ShellIx) -> Option<&Shell> {
        self.shells.get(ix.index())
    }
    pub fn face(&self, ix: FaceIx) -> Option<&Face> {
        self.faces.get(ix.index())
    }
    pub fn r#loop(&self, ix: LoopIx) -> Option<&Loop> {
        self.loops.get(ix.index())
    }
    pub fn coedge(&self, ix: CoedgeIx) -> Option<&Coedge> {
        self.coedges.get(ix.index())
    }
    pub fn edge(&self, ix: EdgeIx) -> Option<&Edge> {
        self.edges.get(ix.index())
    }
    pub fn vertex(&self, ix: VertexIx) -> Option<&Vertex> {
        self.vertices.get(ix.index())
    }

    /// The entity with this ID.
    pub fn lookup(&self, id: &TopologyId) -> Option<EntityIx> {
        self.ids.get(id).copied()
    }

    /// Every ID with its entity, in ascending ID order (REQ-008).
    pub fn ids(&self) -> &BTreeMap<TopologyId, EntityIx> {
        &self.ids
    }

    /// The ID of any entity.
    pub fn id_of(&self, ix: EntityIx) -> Option<&TopologyId> {
        match ix {
            EntityIx::Shell(i) => self.shell(i).map(Shell::id),
            EntityIx::Face(i) => self.face(i).map(Face::id),
            EntityIx::Loop(i) => self.r#loop(i).map(Loop::id),
            EntityIx::Coedge(i) => self.coedge(i).map(Coedge::id),
            EntityIx::Edge(i) => self.edge(i).map(Edge::id),
            EntityIx::Vertex(i) => self.vertex(i).map(Vertex::id),
        }
    }

    /// Total number of entities.
    pub fn entity_count(&self) -> usize {
        self.shells.len()
            + self.faces.len()
            + self.loops.len()
            + self.coedges.len()
            + self.edges.len()
            + self.vertices.len()
    }
}

// ---------------------------------------------------------------- builder

#[derive(Debug, Clone)]
struct BVertex {
    point: [f64; 3],
    label: Option<String>,
}

#[derive(Debug, Clone)]
struct BEdge {
    curve: Option<CurveDef>,
    start: usize,
    end: usize,
    range: [f64; 2],
    label: Option<String>,
}

#[derive(Debug, Clone)]
struct BCoedge {
    edge: usize,
    sense: Sense,
    pcurve: Option<Curve2>,
    label: Option<String>,
}

#[derive(Debug, Clone)]
struct BLoop {
    coedges: Vec<usize>,
    outer: bool,
    label: Option<String>,
}

#[derive(Debug, Clone)]
struct BFace {
    surface: SurfaceDef,
    sense: Sense,
    loops: Vec<usize>,
    label: Option<String>,
}

#[derive(Debug, Clone)]
struct BShell {
    faces: Vec<usize>,
    closed: bool,
}

/// Collects an importer's description of one body. Each `add_*` returns the builder index
/// of the new entity within its kind; references are builder indexes and may point forward
/// (they are checked by [`build`](Self::build)). Labels are optional; without one the
/// entity gets a derived UUIDv5.
#[derive(Debug, Clone)]
pub struct BodyBuilder {
    body_id: [u8; 16],
    vertices: Vec<BVertex>,
    edges: Vec<BEdge>,
    coedges: Vec<BCoedge>,
    loops: Vec<BLoop>,
    faces: Vec<BFace>,
    shells: Vec<BShell>,
}

fn label_of(label: Option<&str>) -> Option<String> {
    label.map(str::to_string)
}

impl BodyBuilder {
    /// An empty description of the body `body_id`.
    pub fn new(body_id: [u8; 16]) -> BodyBuilder {
        BodyBuilder {
            body_id,
            vertices: Vec::new(),
            edges: Vec::new(),
            coedges: Vec::new(),
            loops: Vec::new(),
            faces: Vec::new(),
            shells: Vec::new(),
        }
    }

    pub fn add_vertex(&mut self, point: [f64; 3], label: Option<&str>) -> usize {
        self.vertices.push(BVertex { point, label: label_of(label) });
        self.vertices.len() - 1
    }

    /// An edge from vertex `start` to vertex `end` over `param_range` of `curve` (`None`
    /// when the source gives no curve).
    pub fn add_edge(
        &mut self,
        curve: Option<CurveDef>,
        start: usize,
        end: usize,
        param_range: [f64; 2],
        label: Option<&str>,
    ) -> usize {
        self.edges.push(BEdge { curve, start, end, range: param_range, label: label_of(label) });
        self.edges.len() - 1
    }

    pub fn add_coedge(&mut self, edge: usize, sense: Sense, pcurve: Option<Curve2>, label: Option<&str>) -> usize {
        self.coedges.push(BCoedge { edge, sense, pcurve, label: label_of(label) });
        self.coedges.len() - 1
    }

    /// A loop of coedges in traversal order.
    pub fn add_loop(&mut self, coedges: Vec<usize>, outer: bool, label: Option<&str>) -> usize {
        self.loops.push(BLoop { coedges, outer, label: label_of(label) });
        self.loops.len() - 1
    }

    pub fn add_face(&mut self, surface: SurfaceDef, sense: Sense, loops: Vec<usize>, label: Option<&str>) -> usize {
        self.faces.push(BFace { surface, sense, loops, label: label_of(label) });
        self.faces.len() - 1
    }

    /// A shell of faces; shells take derived IDs only.
    pub fn add_shell(&mut self, faces: Vec<usize>, closed: bool) -> usize {
        self.shells.push(BShell { faces, closed });
        self.shells.len() - 1
    }

    /// Checks the description (module documentation), canonicalises and assigns IDs.
    pub fn build(&self) -> Result<Body, TopoBuildError> {
        self.check_count()?;
        self.check_entities()?;
        let owners = self.check_ownership()?;
        self.check_closure()?;
        self.check_pairing(&owners)?;
        self.canonicalise()
    }

    fn check_count(&self) -> Result<(), TopoBuildError> {
        let entities = [
            self.vertices.len(),
            self.edges.len(),
            self.coedges.len(),
            self.loops.len(),
            self.faces.len(),
            self.shells.len(),
        ]
        .iter()
        .fold(0usize, |a, &n| a.saturating_add(n));
        if entities > MAX_ENTITIES {
            return Err(TopoBuildError::TooLarge { entities });
        }
        Ok(())
    }

    fn check_entities(&self) -> Result<(), TopoBuildError> {
        use EntityKind as K;
        let label = |kind, index, l: &Option<String>| match l {
            Some(s) if s.is_empty() || s.len() > MAX_LABEL_BYTES => {
                Err(TopoBuildError::BadLabel { kind, index, len: s.len() })
            }
            _ => Ok(()),
        };
        let reference = |kind, index, target, target_index: usize, len: usize| {
            if target_index < len {
                Ok(())
            } else {
                Err(TopoBuildError::BadReference { kind, index, target, target_index })
            }
        };
        for (i, v) in self.vertices.iter().enumerate() {
            label(K::Vertex, i, &v.label)?;
            if !v.point.iter().all(|x| x.is_finite()) {
                return Err(TopoBuildError::NonFinite { kind: K::Vertex, index: i });
            }
        }
        for (i, e) in self.edges.iter().enumerate() {
            label(K::Edge, i, &e.label)?;
            if !(e.range[0].is_finite() && e.range[1].is_finite()) {
                return Err(TopoBuildError::NonFinite { kind: K::Edge, index: i });
            }
            if e.range[0] >= e.range[1] {
                return Err(TopoBuildError::BadParamRange { edge: i });
            }
            reference(K::Edge, i, K::Vertex, e.start, self.vertices.len())?;
            reference(K::Edge, i, K::Vertex, e.end, self.vertices.len())?;
        }
        for (i, c) in self.coedges.iter().enumerate() {
            label(K::Coedge, i, &c.label)?;
            reference(K::Coedge, i, K::Edge, c.edge, self.edges.len())?;
        }
        for (i, l) in self.loops.iter().enumerate() {
            label(K::Loop, i, &l.label)?;
            for &c in &l.coedges {
                reference(K::Loop, i, K::Coedge, c, self.coedges.len())?;
            }
            if l.coedges.is_empty() {
                return Err(TopoBuildError::EmptyLoop { r#loop: i });
            }
        }
        for (i, f) in self.faces.iter().enumerate() {
            label(K::Face, i, &f.label)?;
            for &l in &f.loops {
                reference(K::Face, i, K::Loop, l, self.loops.len())?;
            }
            let outer = f.loops.iter().filter(|&&l| self.loops.get(l).is_some_and(|x| x.outer)).count();
            if outer > 1 {
                return Err(TopoBuildError::MultipleOuter { face: i });
            }
        }
        for (i, s) in self.shells.iter().enumerate() {
            for &f in &s.faces {
                reference(K::Shell, i, K::Face, f, self.faces.len())?;
            }
            if s.faces.is_empty() {
                return Err(TopoBuildError::EmptyShell { shell: i });
            }
        }
        Ok(())
    }

    /// Owner of every face (shell), loop (face) and coedge (loop); all references are in
    /// range here.
    fn check_ownership(&self) -> Result<Owners, TopoBuildError> {
        use EntityKind as K;
        let mut face_shell: Vec<Option<usize>> = vec![None; self.faces.len()];
        let mut loop_face: Vec<Option<usize>> = vec![None; self.loops.len()];
        let mut coedge_loop: Vec<Option<usize>> = vec![None; self.coedges.len()];
        fn own(
            slots: &mut [Option<usize>],
            kind: EntityKind,
            index: usize,
            owner: EntityKind,
            by: usize,
        ) -> Result<(), TopoBuildError> {
            match slots.get_mut(index) {
                Some(Some(first)) => Err(TopoBuildError::Reused { kind, index, owner, first: *first, second: by }),
                Some(slot) => {
                    *slot = Some(by);
                    Ok(())
                }
                None => Err(TopoBuildError::BadReference { kind: owner, index: by, target: kind, target_index: index }),
            }
        }
        for (s, shell) in self.shells.iter().enumerate() {
            for &f in &shell.faces {
                own(&mut face_shell, K::Face, f, K::Shell, s)?;
            }
        }
        for shell in &self.shells {
            for &f in &shell.faces {
                if let Some(face) = self.faces.get(f) {
                    for &l in &face.loops {
                        own(&mut loop_face, K::Loop, l, K::Face, f)?;
                    }
                }
            }
        }
        for shell in &self.shells {
            for &f in &shell.faces {
                if let Some(face) = self.faces.get(f) {
                    for &l in &face.loops {
                        if let Some(lp) = self.loops.get(l) {
                            for &c in &lp.coedges {
                                own(&mut coedge_loop, K::Coedge, c, K::Loop, l)?;
                            }
                        }
                    }
                }
            }
        }
        let orphan = |slots: &[Option<usize>], kind| match slots.iter().position(Option::is_none) {
            Some(index) => Err(TopoBuildError::Orphan { kind, index }),
            None => Ok(()),
        };
        orphan(&face_shell, K::Face)?;
        orphan(&loop_face, K::Loop)?;
        orphan(&coedge_loop, K::Coedge)?;
        let mut edge_used = vec![false; self.edges.len()];
        for c in &self.coedges {
            if let Some(u) = edge_used.get_mut(c.edge) {
                *u = true;
            }
        }
        if let Some(index) = edge_used.iter().position(|u| !u) {
            return Err(TopoBuildError::Orphan { kind: K::Edge, index });
        }
        let mut vertex_used = vec![false; self.vertices.len()];
        for e in &self.edges {
            for v in [e.start, e.end] {
                if let Some(u) = vertex_used.get_mut(v) {
                    *u = true;
                }
            }
        }
        if let Some(index) = vertex_used.iter().position(|u| !u) {
            return Err(TopoBuildError::Orphan { kind: K::Vertex, index });
        }
        Ok(Owners {
            face_shell: face_shell.into_iter().flatten().collect(),
            loop_face: loop_face.into_iter().flatten().collect(),
            coedge_loop: coedge_loop.into_iter().flatten().collect(),
        })
    }

    /// Start and end vertex of a coedge, respecting its sense.
    fn ends(&self, c: usize) -> Option<(usize, usize)> {
        let co = self.coedges.get(c)?;
        let e = self.edges.get(co.edge)?;
        Some(match co.sense {
            Sense::Same => (e.start, e.end),
            Sense::Reversed => (e.end, e.start),
        })
    }

    fn check_closure(&self) -> Result<(), TopoBuildError> {
        for (li, l) in self.loops.iter().enumerate() {
            let n = l.coedges.len();
            for k in 0..n {
                let here = l.coedges.get(k).and_then(|&c| self.ends(c));
                let next = l.coedges.get((k + 1) % n).and_then(|&c| self.ends(c));
                match (here, next) {
                    (Some((_, end)), Some((start, _))) if end == start => {}
                    _ => return Err(TopoBuildError::OpenLoop { r#loop: li, position: k }),
                }
            }
        }
        Ok(())
    }

    fn check_pairing(&self, owners: &Owners) -> Result<(), TopoBuildError> {
        let mut uses: Vec<Vec<usize>> = vec![Vec::new(); self.edges.len()];
        for (c, co) in self.coedges.iter().enumerate() {
            if let Some(u) = uses.get_mut(co.edge) {
                u.push(c);
            }
        }
        let shell_of = |c: usize| -> Option<usize> {
            let l = *owners.coedge_loop.get(c)?;
            let f = *owners.loop_face.get(l)?;
            owners.face_shell.get(f).copied()
        };
        for (edge, u) in uses.iter().enumerate() {
            let fault = match u.as_slice() {
                [a, b] => {
                    let sa = self.coedges.get(*a).map(|x| x.sense);
                    let sb = self.coedges.get(*b).map(|x| x.sense);
                    if sa == sb {
                        Some(PairingFault::SameSense)
                    } else if shell_of(*a) != shell_of(*b) {
                        Some(PairingFault::CrossShell)
                    } else {
                        None
                    }
                }
                [a] => match shell_of(*a) {
                    Some(shell) if self.shells.get(shell).is_some_and(|s| s.closed) => {
                        Some(PairingFault::Unpaired { shell })
                    }
                    _ => None,
                },
                [] => None,
                _ => Some(PairingFault::TooMany),
            };
            if let Some(fault) = fault {
                return Err(TopoBuildError::BadPairing { edge, fault });
            }
        }
        Ok(())
    }

    /// Canonical order, IDs and the arena. All checks above have passed, so every lookup
    /// below succeeds; a failed lookup is still reported (as `BadReference`) rather than
    /// panicking.
    fn canonicalise(&self) -> Result<Body, TopoBuildError> {
        let mut shells: Vec<Shell> = Vec::with_capacity(self.shells.len());
        let mut faces: Vec<Face> = Vec::with_capacity(self.faces.len());
        let mut loops: Vec<Loop> = Vec::with_capacity(self.loops.len());
        let mut coedges: Vec<Coedge> = Vec::with_capacity(self.coedges.len());
        // Builder edge and vertex index to canonical index, assigned at first use.
        let mut edge_new: Vec<Option<u32>> = vec![None; self.edges.len()];
        let mut vertex_new: Vec<Option<u32>> = vec![None; self.vertices.len()];
        let mut edge_order: Vec<usize> = Vec::with_capacity(self.edges.len());
        let mut vertex_order: Vec<usize> = Vec::with_capacity(self.vertices.len());
        let mut paths: Paths = Paths::default();
        let lost = |kind: EntityKind, index: usize| TopoBuildError::BadReference {
            kind,
            index,
            target: kind,
            target_index: index,
        };
        for (s_pos, bs) in self.shells.iter().enumerate() {
            let s_ix = ix32(shells.len());
            let mut shell_faces = Vec::with_capacity(bs.faces.len());
            for (f_pos, &bf_ix) in bs.faces.iter().enumerate() {
                let bf = self.faces.get(bf_ix).ok_or_else(|| lost(EntityKind::Face, bf_ix))?;
                let f_ix = ix32(faces.len());
                shell_faces.push(FaceIx(f_ix));
                let mut face_loops = Vec::with_capacity(bf.loops.len());
                for (l_pos, &bl_ix) in bf.loops.iter().enumerate() {
                    let bl = self.loops.get(bl_ix).ok_or_else(|| lost(EntityKind::Loop, bl_ix))?;
                    let l_ix = ix32(loops.len());
                    face_loops.push(LoopIx(l_ix));
                    let mut loop_coedges = Vec::with_capacity(bl.coedges.len());
                    for (c_pos, &bc_ix) in bl.coedges.iter().enumerate() {
                        let bc = self.coedges.get(bc_ix).ok_or_else(|| lost(EntityKind::Coedge, bc_ix))?;
                        let ix_path = [ix32(s_pos), ix32(f_pos), ix32(l_pos), ix32(c_pos)];
                        let slot = edge_new.get_mut(bc.edge).ok_or_else(|| lost(EntityKind::Edge, bc.edge))?;
                        let e_new = match *slot {
                            Some(e) => e,
                            None => {
                                let e = ix32(edge_order.len());
                                *slot = Some(e);
                                edge_order.push(bc.edge);
                                paths.edges.push(ix_path.to_vec());
                                let be = self.edges.get(bc.edge).ok_or_else(|| lost(EntityKind::Edge, bc.edge))?;
                                for (end, v) in [(0u32, be.start), (1u32, be.end)] {
                                    let vslot = vertex_new.get_mut(v).ok_or_else(|| lost(EntityKind::Vertex, v))?;
                                    if vslot.is_none() {
                                        *vslot = Some(ix32(vertex_order.len()));
                                        vertex_order.push(v);
                                        let mut p = ix_path.to_vec();
                                        p.push(end);
                                        paths.vertices.push(p);
                                    }
                                }
                                e
                            }
                        };
                        loop_coedges.push(CoedgeIx(ix32(coedges.len())));
                        paths.coedges.push(ix_path.to_vec());
                        coedges.push(Coedge {
                            id: TopologyId::Derived([0; 16]),
                            edge: EdgeIx(e_new),
                            sense: bc.sense,
                            pcurve: bc.pcurve.clone(),
                            r#loop: LoopIx(l_ix),
                        });
                        paths.coedge_labels.push(bc.label.clone());
                    }
                    paths.loops.push(vec![ix32(s_pos), ix32(f_pos), ix32(l_pos)]);
                    paths.loop_labels.push(bl.label.clone());
                    loops.push(Loop {
                        id: TopologyId::Derived([0; 16]),
                        coedges: loop_coedges,
                        outer: bl.outer,
                        face: FaceIx(f_ix),
                    });
                }
                paths.faces.push(vec![ix32(s_pos), ix32(f_pos)]);
                paths.face_labels.push(bf.label.clone());
                faces.push(Face {
                    id: TopologyId::Derived([0; 16]),
                    surface: bf.surface.clone(),
                    sense: bf.sense,
                    loops: face_loops,
                    shell: ShellIx(s_ix),
                });
            }
            paths.shells.push(vec![ix32(s_pos)]);
            shells.push(Shell { id: TopologyId::Derived([0; 16]), faces: shell_faces, closed: bs.closed });
        }
        let mut edges: Vec<Edge> = Vec::with_capacity(edge_order.len());
        for &be_ix in &edge_order {
            let be = self.edges.get(be_ix).ok_or_else(|| lost(EntityKind::Edge, be_ix))?;
            let v = |b: usize| vertex_new.get(b).copied().flatten().ok_or_else(|| lost(EntityKind::Vertex, b));
            edges.push(Edge {
                id: TopologyId::Derived([0; 16]),
                curve: be.curve.clone(),
                start: VertexIx(v(be.start)?),
                end: VertexIx(v(be.end)?),
                param_range: [canon(be.range[0]), canon(be.range[1])],
            });
            paths.edge_labels.push(be.label.clone());
        }
        let mut vertices: Vec<Vertex> = Vec::with_capacity(vertex_order.len());
        for &bv_ix in &vertex_order {
            let bv = self.vertices.get(bv_ix).ok_or_else(|| lost(EntityKind::Vertex, bv_ix))?;
            vertices.push(Vertex { id: TopologyId::Derived([0; 16]), point: canon3(bv.point) });
            paths.vertex_labels.push(bv.label.clone());
        }

        // IDs, inserted kind by kind in canonical order.
        let mut ids: BTreeMap<TopologyId, EntityIx> = BTreeMap::new();
        let mut first_path: BTreeMap<TopologyId, IndexPath> = BTreeMap::new();
        let body_id = self.body_id;
        let mut assign = |kind: EntityKind,
                          ix_path: &[u32],
                          label: Option<&String>,
                          ix: EntityIx|
         -> Result<TopologyId, TopoBuildError> {
            let id = match label.and_then(|l| Label::new(l)) {
                Some(l) => TopologyId::Imported(l),
                None => TopologyId::Derived(derive_id(&body_id, kind, ix_path)),
            };
            let here = IndexPath { kind, steps: ix_path.to_vec() };
            if let Some(first) = first_path.get(&id) {
                return Err(TopoBuildError::DuplicateId { id, first: first.clone(), second: here });
            }
            first_path.insert(id.clone(), here);
            ids.insert(id.clone(), ix);
            Ok(id)
        };
        for (i, (s, p)) in shells.iter_mut().zip(&paths.shells).enumerate() {
            s.id = assign(EntityKind::Shell, p, None, EntityIx::Shell(ShellIx(ix32(i))))?;
        }
        for (i, ((f, p), l)) in faces.iter_mut().zip(&paths.faces).zip(&paths.face_labels).enumerate() {
            f.id = assign(EntityKind::Face, p, l.as_ref(), EntityIx::Face(FaceIx(ix32(i))))?;
        }
        for (i, ((x, p), l)) in loops.iter_mut().zip(&paths.loops).zip(&paths.loop_labels).enumerate() {
            x.id = assign(EntityKind::Loop, p, l.as_ref(), EntityIx::Loop(LoopIx(ix32(i))))?;
        }
        for (i, ((x, p), l)) in coedges.iter_mut().zip(&paths.coedges).zip(&paths.coedge_labels).enumerate() {
            x.id = assign(EntityKind::Coedge, p, l.as_ref(), EntityIx::Coedge(CoedgeIx(ix32(i))))?;
        }
        for (i, ((x, p), l)) in edges.iter_mut().zip(&paths.edges).zip(&paths.edge_labels).enumerate() {
            x.id = assign(EntityKind::Edge, p, l.as_ref(), EntityIx::Edge(EdgeIx(ix32(i))))?;
        }
        for (i, ((x, p), l)) in vertices.iter_mut().zip(&paths.vertices).zip(&paths.vertex_labels).enumerate() {
            x.id = assign(EntityKind::Vertex, p, l.as_ref(), EntityIx::Vertex(VertexIx(ix32(i))))?;
        }
        Ok(Body { id: self.body_id, shells, faces, loops, coedges, edges, vertices, ids })
    }
}

struct Owners {
    face_shell: Vec<usize>,
    loop_face: Vec<usize>,
    coedge_loop: Vec<usize>,
}

#[derive(Default)]
struct Paths {
    shells: Vec<Vec<u32>>,
    faces: Vec<Vec<u32>>,
    loops: Vec<Vec<u32>>,
    coedges: Vec<Vec<u32>>,
    edges: Vec<Vec<u32>>,
    vertices: Vec<Vec<u32>>,
    face_labels: Vec<Option<String>>,
    loop_labels: Vec<Option<String>>,
    coedge_labels: Vec<Option<String>>,
    edge_labels: Vec<Option<String>>,
    vertex_labels: Vec<Option<String>>,
}

/// Index below [`MAX_ENTITIES`] (checked first by `build`), so it fits `u32`.
fn ix32(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// UUIDv5 in [`TOPOLOGY_NAMESPACE`] of `body_id | kind code | path`, each path component a
/// `u32` little-endian (REQ-002).
pub(crate) fn derive_id(body_id: &[u8; 16], kind: EntityKind, steps: &[u32]) -> [u8; 16] {
    let mut name = Vec::with_capacity(17 + 4 * steps.len());
    name.extend_from_slice(body_id);
    name.push(kind.code());
    for p in steps {
        name.extend_from_slice(&p.to_le_bytes());
    }
    uuid_v5(&TOPOLOGY_NAMESPACE, &name)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn namespace_is_derived() {
        let url = [0x6b, 0xa7, 0xb8, 0x11, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8];
        let ns =
            uuid_v5(&url, b"https://github.com/metrologymaven/metrologymaven/spec/geo/cad/topo/model#topology-id-v1");
        assert_eq!(format_uuid(&ns), format_uuid(&TOPOLOGY_NAMESPACE));
    }

    #[test]
    fn id_order_imported_first_then_bytewise() {
        let a = TopologyId::Imported(Label::new("#2").unwrap());
        let b = TopologyId::Imported(Label::new("#10").unwrap());
        let d0 = TopologyId::Derived([0; 16]);
        let d1 = TopologyId::Derived([255; 16]);
        let mut v = vec![d1.clone(), a.clone(), d0.clone(), b.clone()];
        v.sort();
        assert_eq!(v, vec![b, a, d0, d1]);
    }

    #[test]
    fn label_bounds() {
        assert!(Label::new("").is_none());
        assert!(Label::new(&"x".repeat(256)).is_some());
        assert!(Label::new(&"x".repeat(257)).is_none());
    }
}
