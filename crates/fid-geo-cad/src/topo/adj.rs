//! Topology adjacency queries ("what touches what"). Spec node `GEO.CAD.TOPO.ADJ`.
//!
//! [`Adjacency::build`] derives an index from a checked [`Body`] in one pass over its
//! coedges, edges and loops: sorted, deduplicated incidence pairs, looked up by binary search.
//! The index is derived data only: it is not part of the body, its encoding or its digest, and
//! two builds from equal bodies are equal.
//!
//! # Pinned rules
//!
//! - Every query returns entity IDs in ascending [`TopologyId`] order (REQ-GEO.CAD.TOPO.MODEL-008:
//!   imported labels first, bytewise, then derived IDs, bytewise) with no duplicates
//!   (REQ-001). An entity reached twice appears once: a face adjacent across two edges, the one
//!   face of a seam edge whose two coedges lie on it, the one edge of a closed edge whose start
//!   is its end.
//! - `faces_of_edge(e)`: the faces owning a loop with a coedge of `e`. `coedges_of_edge(e)`:
//!   those coedges. `edges_of_face(f)`: the edges of the coedges of every loop of `f` (outer and
//!   inner). `loops_of_face(f)`: its loops. `faces_of_body()`: every face of every shell.
//!   `edges_of_vertex(v)`: edges starting or ending at `v`. `faces_of_vertex(v)`: faces of
//!   those edges. `face_neighbours(f)`: faces sharing at least one edge with `f`, never `f`
//!   itself, even across a seam (REQ-004).
//! - A face without loops (a closed periodic surface) has no edges, loops or neighbours.
//! - A non-manifold vertex (faces meeting only at a point) lists all its faces; those faces
//!   are not neighbours, as they share no edge. A non-manifold edge (more than two coedges)
//!   cannot occur: `BodyBuilder::build` refuses it (`BadPairing`), so every edge here has one
//!   or two coedges and at most two faces. The queries do not rely on that bound; detecting
//!   such defects belongs to `GEO.CAD.TOPO.VALID`.
//! - An ID not in the body is [`AdjError::UnknownEntity`]; an ID of another kind than the
//!   query takes is [`AdjError::WrongKind`] (REQ-002), never an empty list.
//!
//! Building sorts O(n) pairs in the number of coedges, O(n log n) time, and uses vectors only
//! (REQ-003); results are a function of the body alone (D0).

use core::fmt;

use super::model::{Body, EntityIx, EntityKind, TopologyId};

/// Why a query refused its argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdjError {
    /// The ID names no entity of the body.
    UnknownEntity { id: TopologyId },
    /// The ID names an entity of another kind than the query takes.
    WrongKind { id: TopologyId, expected: EntityKind, found: EntityKind },
}

impl AdjError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            AdjError::UnknownEntity { .. } => "UnknownEntity",
            AdjError::WrongKind { .. } => "WrongKind",
        }
    }
}

impl fmt::Display for AdjError {
    /// Labels are shown escaped (C6), through `TopologyId`'s `Display`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdjError::UnknownEntity { id } => write!(f, "{id} is not an entity of this body"),
            AdjError::WrongKind { id, expected, found } => {
                write!(f, "{id} has kind {}, expected {}", found.as_str(), expected.as_str())
            }
        }
    }
}

impl core::error::Error for AdjError {}

/// Sorted, deduplicated `(key, rank)` pairs: `key` a dense index of the queried kind, `rank`
/// the position of the answer entity in ascending-ID order of its kind. A range of equal keys
/// is therefore already in ascending-ID order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Incidence(Vec<(u32, u32)>);

impl Incidence {
    fn from_pairs(mut pairs: Vec<(u32, u32)>) -> Incidence {
        pairs.sort_unstable();
        pairs.dedup();
        Incidence(pairs)
    }

    /// The ranks paired with `key`, ascending.
    fn of(&self, key: u32) -> &[(u32, u32)] {
        let lo = self.0.partition_point(|p| p.0 < key);
        let hi = self.0.partition_point(|p| p.0 <= key);
        self.0.get(lo..hi).unwrap_or(&[])
    }
}

/// Ascending-ID order of one kind: `rank[dense] = position`, `by_rank[position] = dense`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Order {
    rank: Vec<u32>,
    by_rank: Vec<u32>,
}

impl Order {
    fn rank(&self, dense: usize) -> u32 {
        self.rank.get(dense).copied().unwrap_or(u32::MAX)
    }
}

/// The adjacency index of one body. Built once by [`Adjacency::build`]; every query borrows
/// the body to map its arguments and answers to IDs.
#[derive(Debug, Clone, PartialEq)]
pub struct Adjacency<'b> {
    body: &'b Body,
    faces: Order,
    loops: Order,
    coedges: Order,
    edges: Order,
    edge_faces: Incidence,
    edge_coedges: Incidence,
    face_edges: Incidence,
    face_loops: Incidence,
    face_faces: Incidence,
    vertex_edges: Incidence,
    vertex_faces: Incidence,
}

/// Body indexes are below `MAX_ENTITIES` (10⁷), so they fit `u32`.
fn ix32(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

impl<'b> Adjacency<'b> {
    /// Derives the index: one pass over coedges (and edges and loops) emitting incidence
    /// pairs, then a sort and deduplication of each pair list.
    pub fn build(body: &'b Body) -> Adjacency<'b> {
        let mut order = [Order::default(), Order::default(), Order::default(), Order::default()];
        let counts = [body.faces().len(), body.loops().len(), body.coedges().len(), body.edges().len()];
        for (o, &n) in order.iter_mut().zip(&counts) {
            o.rank = vec![u32::MAX; n];
            o.by_rank = Vec::with_capacity(n);
        }
        // `ids()` iterates in ascending ID order, which fixes each kind's ranks.
        for ix in body.ids().values() {
            let (slot, dense) = match *ix {
                EntityIx::Face(i) => (0, i.index()),
                EntityIx::Loop(i) => (1, i.index()),
                EntityIx::Coedge(i) => (2, i.index()),
                EntityIx::Edge(i) => (3, i.index()),
                EntityIx::Shell(_) | EntityIx::Vertex(_) => continue,
            };
            if let Some(o) = order.get_mut(slot) {
                let r = ix32(o.by_rank.len());
                if let Some(x) = o.rank.get_mut(dense) {
                    *x = r;
                }
                o.by_rank.push(ix32(dense));
            }
        }
        let [faces, loops, coedges, edges] = order;

        let n = body.coedges().len();
        let mut edge_faces = Vec::with_capacity(n);
        let mut edge_coedges = Vec::with_capacity(n);
        let mut face_edges = Vec::with_capacity(n);
        let mut vertex_faces = Vec::with_capacity(2 * n);
        for (c, co) in body.coedges().iter().enumerate() {
            let e = co.edge();
            let Some(f) = body.r#loop(co.r#loop()).map(|l| l.face()) else { continue };
            let Some(edge) = body.edge(e) else { continue };
            edge_faces.push((ix32(e.index()), faces.rank(f.index())));
            edge_coedges.push((ix32(e.index()), coedges.rank(c)));
            face_edges.push((ix32(f.index()), edges.rank(e.index())));
            for v in [edge.start(), edge.end()] {
                vertex_faces.push((ix32(v.index()), faces.rank(f.index())));
            }
        }
        let edge_faces = Incidence::from_pairs(edge_faces);

        let mut vertex_edges = Vec::with_capacity(2 * body.edges().len());
        for (e, edge) in body.edges().iter().enumerate() {
            for v in [edge.start(), edge.end()] {
                vertex_edges.push((ix32(v.index()), edges.rank(e)));
            }
        }

        let face_loops: Vec<(u32, u32)> =
            body.loops().iter().enumerate().map(|(l, lp)| (ix32(lp.face().index()), loops.rank(l))).collect();

        // Faces sharing an edge: every ordered pair of distinct faces of one edge. The face's
        // dense index is recovered from its rank through `by_rank`.
        let mut face_faces = Vec::with_capacity(n);
        let mut i = 0;
        while let Some(&(e, _)) = edge_faces.0.get(i) {
            let group = edge_faces.of(e);
            for &(_, a) in group {
                let Some(&fa) = faces.by_rank.get(a as usize) else { continue };
                for &(_, b) in group {
                    if a != b {
                        face_faces.push((fa, b));
                    }
                }
            }
            if group.is_empty() {
                break;
            }
            i += group.len();
        }

        Adjacency {
            body,
            edge_faces,
            edge_coedges: Incidence::from_pairs(edge_coedges),
            face_edges: Incidence::from_pairs(face_edges),
            face_loops: Incidence::from_pairs(face_loops),
            face_faces: Incidence::from_pairs(face_faces),
            vertex_edges: Incidence::from_pairs(vertex_edges),
            vertex_faces: Incidence::from_pairs(vertex_faces),
            faces,
            loops,
            coedges,
            edges,
        }
    }

    /// The body this index was built from.
    pub fn body(&self) -> &'b Body {
        self.body
    }

    /// Dense index of `id`, which must name an entity of `kind`.
    fn dense(&self, id: &TopologyId, kind: EntityKind) -> Result<u32, AdjError> {
        let ix = self.body.lookup(id).ok_or_else(|| AdjError::UnknownEntity { id: id.clone() })?;
        let found = ix.kind();
        let dense = match ix {
            EntityIx::Face(i) if kind == EntityKind::Face => i.index(),
            EntityIx::Edge(i) if kind == EntityKind::Edge => i.index(),
            EntityIx::Vertex(i) if kind == EntityKind::Vertex => i.index(),
            _ => return Err(AdjError::WrongKind { id: id.clone(), expected: kind, found }),
        };
        Ok(ix32(dense))
    }

    /// IDs of the ranks paired with `key`, ascending.
    fn ids(&self, inc: &Incidence, key: u32, kind: EntityKind) -> Vec<TopologyId> {
        let order = match kind {
            EntityKind::Face => &self.faces,
            EntityKind::Loop => &self.loops,
            EntityKind::Coedge => &self.coedges,
            _ => &self.edges,
        };
        inc.of(key)
            .iter()
            .filter_map(|&(_, r)| {
                let dense = *order.by_rank.get(r as usize)? as usize;
                let id = match kind {
                    EntityKind::Face => self.body.faces().get(dense)?.id(),
                    EntityKind::Loop => self.body.loops().get(dense)?.id(),
                    EntityKind::Coedge => self.body.coedges().get(dense)?.id(),
                    _ => self.body.edges().get(dense)?.id(),
                };
                Some(id.clone())
            })
            .collect()
    }

    fn query(
        &self,
        id: &TopologyId,
        arg: EntityKind,
        inc: &Incidence,
        out: EntityKind,
    ) -> Result<Vec<TopologyId>, AdjError> {
        let key = self.dense(id, arg)?;
        Ok(self.ids(inc, key, out))
    }

    /// Faces using edge `id` (one for a seam or a boundary edge, two for a manifold edge).
    pub fn faces_of_edge(&self, id: &TopologyId) -> Result<Vec<TopologyId>, AdjError> {
        self.query(id, EntityKind::Edge, &self.edge_faces, EntityKind::Face)
    }

    /// Coedges of edge `id`.
    pub fn coedges_of_edge(&self, id: &TopologyId) -> Result<Vec<TopologyId>, AdjError> {
        self.query(id, EntityKind::Edge, &self.edge_coedges, EntityKind::Coedge)
    }

    /// Edges of all loops of face `id`; a seam edge once.
    pub fn edges_of_face(&self, id: &TopologyId) -> Result<Vec<TopologyId>, AdjError> {
        self.query(id, EntityKind::Face, &self.face_edges, EntityKind::Edge)
    }

    /// Loops of face `id`, outer and inner.
    pub fn loops_of_face(&self, id: &TopologyId) -> Result<Vec<TopologyId>, AdjError> {
        self.query(id, EntityKind::Face, &self.face_loops, EntityKind::Loop)
    }

    /// Faces sharing at least one edge with face `id`, excluding `id` itself (REQ-004).
    pub fn face_neighbours(&self, id: &TopologyId) -> Result<Vec<TopologyId>, AdjError> {
        self.query(id, EntityKind::Face, &self.face_faces, EntityKind::Face)
    }

    /// Faces with an edge that starts or ends at vertex `id`.
    pub fn faces_of_vertex(&self, id: &TopologyId) -> Result<Vec<TopologyId>, AdjError> {
        self.query(id, EntityKind::Vertex, &self.vertex_faces, EntityKind::Face)
    }

    /// Edges starting or ending at vertex `id`; a closed edge once.
    pub fn edges_of_vertex(&self, id: &TopologyId) -> Result<Vec<TopologyId>, AdjError> {
        self.query(id, EntityKind::Vertex, &self.vertex_edges, EntityKind::Edge)
    }

    /// Every face of the body (all shells), ascending.
    pub fn faces_of_body(&self) -> Vec<TopologyId> {
        self.body.ids().iter().filter(|(_, ix)| matches!(ix, EntityIx::Face(_))).map(|(id, _)| id.clone()).collect()
    }
}
