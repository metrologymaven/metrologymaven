//! Fan agreement, manifold assembly and orientation. Spec node `SCAN.POLY.SURF.STITCH`.
//!
//! [`stitch`] turns the fans of [`crate::fan::fans`] into one indexed [`Mesh`]: a triangle is
//! kept when the fans of its vertices agree on it, an edge used by more than two kept
//! triangles keeps the two that fit the vertex normals best, and every triangle is wound so
//! its face normal points the way the vertex normals do. Holes wider than the maximum gap
//! stay open, because no fan proposes a triangle across them (REQ-SCAN.POLY.SURF.STITCH-004).
//! [`reconstruct_in_memory`] composes `prep::prepare`, `fan::fans` and [`stitch`].
//!
//! Every value-affecting choice is pinned below; the independent reference
//! `fixtures/scan/poly/make_stitch_fixtures.py` repeats each step in Python.
//!
//! # Input checks (first failure wins)
//!
//! 1. every vertex position finite with every coordinate at most `COORD_LIMIT` = 2^500 in
//!    magnitude (`vs.position`);
//! 2. every vertex normal a unit normal by GEO.TYPES `is_unit_normal` (`vs.normal`);
//! 3. no two vertices share a source (`vs.source`);
//! 4. exactly one fan per vertex: as many fans as vertices, every centre a vertex source, no
//!    two fans with the same centre (`fans.centre`);
//! 5. a fan has triangles exactly when its status is `Ok` (`fans.status`);
//! 6. every fan triangle is `[centre, j, k]` with three distinct vertex sources, and no fan
//!    holds the same vertex triple twice (`fans.triangles`).
//!
//! Each check covers the whole input before the next starts, so which check fails does not
//! depend on the order of the fans or of the vertices. Each is
//! [`PolyError::InvalidParameter`] (`scan.poly.invalid_parameter`). A missing fan is refused
//! rather than read as "no votes", so no vertex is silently dropped.
//!
//! # Vertex numbering
//!
//! Mesh vertex `i` is the vertex with the `i`-th smallest source; its position and normal are
//! copied bit for bit from the vertex set. Every vertex of the set is in the mesh, also one no
//! kept triangle uses (it is listed in [`StitchReport::unused`] with its fan's status). The
//! numbering, and so the whole mesh, depends only on the vertex set's contents and the set of
//! fans, not on either order (REQ-SCAN.POLY.SURF.STITCH-005, AC-3). A triple below is the
//! three mesh indices in ascending order, which is also ascending source order.
//!
//! # Steps
//!
//! 1. **Votes.** Every fan triangle becomes its ascending triple; the triples are sorted and
//!    counted. A triple's vote count is the number of its vertices whose fan contains it
//!    (1, 2 or 3; each fan holds a triple at most once and every triangle contains its
//!    centre). A one-vote triple is rejected ([`RejectReason::SingleVote`]).
//! 2. **Orientation.** For a triple `(a, b, c)` with at least two votes, with positions
//!    `pa, pb, pc` and normals `na, nb, nc`: `u = pb - pa`, `v = pc - pa` per axis,
//!    `f = u x v = (u1*v2 - u2*v1, u2*v0 - u0*v2, u0*v1 - u1*v0)`, `s = (f0*m0 + f1*m1) + f2*m2`
//!    with `m = (na + nb) + nc` per axis. `s > 0` keeps the winding `(a, b, c)`; `s < 0` winds
//!    it `(a, c, b)` and negates `f` (exact: swapping `b` and `c` negates every cross-product
//!    component exactly). `s = 0` (a zero-area triangle, or one standing perpendicular to its
//!    normals) is rejected ([`RejectReason::Unorientable`]) instead of being wound by chance
//!    (REQ-SCAN.POLY.SURF.STITCH-003). With coordinates at most 2^500 nothing overflows.
//! 3. **Fit key.** The oriented `f` is scaled by its largest absolute component (`w = f / M`,
//!    so no square overflows or underflows), `l = sqrt((w0*w0 + w1*w1) + w2*w2)`,
//!    `e = w / l` per axis, and the key is the smallest of `(e0*n0 + e1*n1) + e2*n2` over the
//!    three vertex normals `n` in the order a, b, c (total-order minimum). A larger key is a
//!    smaller maximum angle between the face normal and the vertex normals.
//! 4. **Three votes.** Every orientable three-vote triple is accepted, and adds one use to
//!    each of its edges `[a, b]`, `[a, c]`, `[b, c]`.
//! 5. **Two votes.** The orientable two-vote triples, in ascending triple order: a triple is
//!    rejected when one of its edges, checked in the order `[a, b]`, `[a, c]`, `[b, c]`, is
//!    already used twice ([`RejectReason::EdgeFull`] names the first); otherwise it is
//!    accepted and adds its edge uses (REQ-SCAN.POLY.SURF.STITCH-001).
//! 6. **Cleanup.** The edges of the accepted triangles, in ascending `[lo, hi]` order, one
//!    pass: on an edge still used by more than two accepted triangles, they are ranked by key
//!    (descending, total order), then triple (ascending), and all but the first two are
//!    removed ([`RejectReason::NonManifold`] names the edge). Removals only lower the use of
//!    other edges, so after the pass no edge is used more than twice
//!    (REQ-SCAN.POLY.SURF.STITCH-002); the mesh's own topology facts are checked to confirm it.
//! 7. **Output.** The surviving triangles, wound as in step 2, in ascending triple order. No
//!    surviving triangle is [`PolyError::EmptyMesh`] (`scan.poly.empty_mesh`).
//!
//! # Nothing is dropped silently
//!
//! [`StitchReport`] lists every rejected triple with its vote count and reason, every vertex
//! no output triangle uses with its fan status, and every vertex's PREP status: a smoothed
//! or clamped vertex is a computed position, not a measured point, and the report says so
//! (D-045).
//!
//! # Determinism (D0)
//!
//! Sorting is by integer keys, then by a total-order float key; every float step has a fixed
//! operation order with no fused multiply-add, no hash container and no `f64` min or max.

use fid_geo_index::COORD_LIMIT;
use fid_geo_types::{is_unit_normal, AttributeSet, FrameRef, GeoTypeError, Mesh, PointBuffer, PointCloud};

use crate::fan::{fans, Fan, FanStatus};
use crate::prep::{prepare, PolyError, PrepParams, VertexSet, VertexStatus};

/// Why a candidate triangle is not in the mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RejectReason {
    /// Only one of its vertices' fans contains it (step 1).
    SingleVote,
    /// Its face normal has a zero dot product with the sum of its vertex normals (step 2).
    Unorientable,
    /// A two-vote triangle with an edge already used by two accepted triangles (step 5).
    EdgeFull {
        /// The first such edge, as two vertex sources in ascending order.
        edge: [u64; 2],
    },
    /// Removed from an edge used by more than two accepted triangles (step 6).
    NonManifold {
        /// The edge, as two vertex sources in ascending order.
        edge: [u64; 2],
    },
}

impl RejectReason {
    /// A stable machine-readable name (without the edge).
    pub fn as_str(self) -> &'static str {
        match self {
            RejectReason::SingleVote => "single_vote",
            RejectReason::Unorientable => "unorientable",
            RejectReason::EdgeFull { .. } => "edge_full",
            RejectReason::NonManifold { .. } => "non_manifold",
        }
    }
}

/// A candidate triangle that is not in the mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rejected {
    /// Its vertex sources in ascending order.
    pub triangle: [u64; 3],
    /// How many of its vertices' fans contain it (1 to 3).
    pub votes: u8,
    /// Why it was rejected.
    pub reason: RejectReason,
}

/// A vertex that no output triangle uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnusedVertex {
    /// Its source.
    pub source: u64,
    /// The status of its own fan (an `Ok` fan whose triangles were all rejected shows `Ok`;
    /// the rejected list says why).
    pub fan: FanStatus,
}

/// What [`stitch`] kept, rejected and left unused.
#[derive(Debug, Clone, PartialEq)]
pub struct StitchReport {
    /// The source of each mesh vertex, by mesh index (ascending).
    pub vertex_sources: Vec<u64>,
    /// The PREP status of each mesh vertex, by mesh index. A `moved()` status is a computed
    /// position within `h/2` of its source point, not a measured point (D-045).
    pub vertex_status: Vec<VertexStatus>,
    /// How many mesh vertices are moved (smoothed or clamped) positions.
    pub moved_vertices: u64,
    /// Distinct candidate triples over all fans.
    pub candidates: u64,
    /// Output triangles that had three votes.
    pub kept_three_vote: u64,
    /// Output triangles that had two votes.
    pub kept_two_vote: u64,
    /// Every candidate not in the output, in ascending triple order. With the two kept
    /// counts it accounts for every candidate.
    pub rejected: Vec<Rejected>,
    /// Every vertex no output triangle uses, in ascending source order.
    pub unused: Vec<UnusedVertex>,
}

/// The stitched mesh and its report.
#[derive(Debug, Clone, PartialEq)]
pub struct Stitched {
    /// The mesh: every vertex of the set, the kept triangles, and the vertex normals as the
    /// `normal` channel.
    pub mesh: Mesh,
    /// What was kept, rejected and left unused, and why.
    pub report: StitchReport,
}

/// The parameters of [`reconstruct_in_memory`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReconParams {
    /// The vertex-set preparation (its `threads` also runs the fans).
    pub prep: PrepParams,
    /// The maximum gap `g` in metres.
    pub g: f64,
}

fn invalid(name: &'static str, reason: &'static str) -> PolyError {
    PolyError::InvalidParameter { name, reason }
}

fn mesh_error(e: GeoTypeError) -> PolyError {
    match e {
        GeoTypeError::AllocationBudget { .. } | GeoTypeError::AllocationFailed { .. } => PolyError::Allocation,
        _ => PolyError::Inconsistent,
    }
}

fn to_u64(i: usize) -> Result<u64, PolyError> {
    u64::try_from(i).map_err(|_| PolyError::Inconsistent)
}

fn to_usize(i: u64) -> Result<usize, PolyError> {
    usize::try_from(i).map_err(|_| PolyError::Inconsistent)
}

fn sorted3(t: [u64; 3]) -> [u64; 3] {
    let mut s = t;
    s.sort_unstable();
    s
}

/// The edges of an ascending triple in the pinned order `[a, b]`, `[a, c]`, `[b, c]`.
fn edges_of(t: [u64; 3]) -> [[u64; 2]; 3] {
    [[t[0], t[1]], [t[0], t[2]], [t[1], t[2]]]
}

/// A candidate after steps 1 to 3.
#[derive(Debug, Clone, Copy)]
struct Cand {
    tri: [u64; 3],
    votes: u8,
    /// `None` when unorientable; else (flip to `(a, c, b)`, fit key).
    orient: Option<(bool, f64)>,
}

/// Mesh vertex data in ascending source order.
struct Verts {
    sources: Vec<u64>,
    pos: Vec<[f64; 3]>,
    nrm: Vec<[f64; 3]>,
    status: Vec<VertexStatus>,
}

impl Verts {
    fn index(&self, source: u64) -> Option<u64> {
        self.sources.binary_search(&source).ok().and_then(|i| u64::try_from(i).ok())
    }

    fn get(&self, i: u64) -> Result<([f64; 3], [f64; 3]), PolyError> {
        let i = to_usize(i)?;
        match (self.pos.get(i), self.nrm.get(i)) {
            (Some(&p), Some(&n)) => Ok((p, n)),
            _ => Err(PolyError::Inconsistent),
        }
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Steps 2 and 3 for the ascending triple `t`.
fn orient(v: &Verts, t: [u64; 3]) -> Result<Option<(bool, f64)>, PolyError> {
    let (pa, na) = v.get(t[0])?;
    let (pb, nb) = v.get(t[1])?;
    let (pc, nc) = v.get(t[2])?;
    let (u, w) = (sub(pb, pa), sub(pc, pa));
    let mut f = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
    let m = [(na[0] + nb[0]) + nc[0], (na[1] + nb[1]) + nc[1], (na[2] + nb[2]) + nc[2]];
    let s = dot(f, m);
    if !(s.is_finite() && f.iter().all(|x| x.is_finite())) {
        return Err(PolyError::Inconsistent);
    }
    if s == 0.0 {
        return Ok(None);
    }
    let flip = s < 0.0;
    if flip {
        f = [-f[0], -f[1], -f[2]];
    }
    let mut big = 0.0f64;
    for x in f {
        big = fid_num::f::total_max(big, x.abs())?;
    }
    // s != 0 implies some component is non-zero.
    if big <= 0.0 {
        return Err(PolyError::Inconsistent);
    }
    let w = [f[0] / big, f[1] / big, f[2] / big];
    let l = fid_num::f::sqrt((w[0] * w[0] + w[1] * w[1]) + w[2] * w[2])?;
    if l < 1.0 {
        return Err(PolyError::Inconsistent);
    }
    let e = [w[0] / l, w[1] / l, w[2] / l];
    let mut key = dot(e, na);
    key = fid_num::f::total_min(key, dot(e, nb))?;
    key = fid_num::f::total_min(key, dot(e, nc))?;
    Ok(Some((flip, key)))
}

/// Input checks 1 to 3 and the vertices in ascending source order.
fn check_vertices(vs: &VertexSet) -> Result<Verts, PolyError> {
    if !vs.vertices.iter().all(|v| v.position.iter().all(|x| x.is_finite() && x.abs() <= COORD_LIMIT)) {
        return Err(invalid("vs.position", "not finite or beyond 2^500 m"));
    }
    if !vs.vertices.iter().all(|v| is_unit_normal(v.normal)) {
        return Err(invalid("vs.normal", "not a unit normal"));
    }
    let n = vs.vertices.len();
    let mut order: Vec<(u64, usize)> = Vec::new();
    order.try_reserve_exact(n).map_err(|_| PolyError::Allocation)?;
    order.extend(vs.vertices.iter().enumerate().map(|(i, v)| (v.source, i)));
    order.sort_unstable();
    if order.windows(2).any(|w| w[0].0 == w[1].0) {
        return Err(invalid("vs.source", "two vertices share a source"));
    }
    let mut v = Verts { sources: Vec::new(), pos: Vec::new(), nrm: Vec::new(), status: Vec::new() };
    v.sources.try_reserve_exact(n).map_err(|_| PolyError::Allocation)?;
    v.pos.try_reserve_exact(n).map_err(|_| PolyError::Allocation)?;
    v.nrm.try_reserve_exact(n).map_err(|_| PolyError::Allocation)?;
    v.status.try_reserve_exact(n).map_err(|_| PolyError::Allocation)?;
    for &(source, i) in &order {
        let p = vs.vertices.get(i).ok_or(PolyError::Inconsistent)?;
        v.sources.push(source);
        v.pos.push(p.position);
        v.nrm.push(p.normal);
        v.status.push(p.status);
    }
    Ok(v)
}

/// Input checks 4 to 6 and the fan status by mesh index.
fn check_fans(v: &Verts, fans: &[Fan]) -> Result<Vec<FanStatus>, PolyError> {
    let n = v.sources.len();
    let mut status: Vec<Option<FanStatus>> = Vec::new();
    status.try_reserve_exact(n).map_err(|_| PolyError::Allocation)?;
    status.resize(n, None);
    let mut centres_ok = fans.len() == n;
    for fan in fans {
        match v.index(fan.centre).and_then(|i| usize::try_from(i).ok()).and_then(|i| status.get_mut(i)) {
            Some(slot @ None) => *slot = Some(fan.status),
            _ => centres_ok = false,
        }
    }
    if !centres_ok {
        return Err(invalid("fans.centre", "not exactly one fan per vertex"));
    }
    if !fans.iter().all(|f| (f.status == FanStatus::Ok) == !f.triangles.is_empty()) {
        return Err(invalid("fans.status", "a fan has triangles exactly when its status is ok"));
    }
    let mut scratch: Vec<[u64; 3]> = Vec::new();
    for fan in fans {
        scratch.clear();
        scratch.try_reserve(fan.triangles.len()).map_err(|_| PolyError::Allocation)?;
        for t in &fan.triangles {
            let distinct = t[0] != t[1] && t[0] != t[2] && t[1] != t[2];
            let known = t.iter().all(|&s| v.index(s).is_some());
            if t[0] != fan.centre || !distinct || !known {
                return Err(invalid("fans.triangles", "not [centre, j, k] over three distinct vertices"));
            }
            scratch.push(sorted3(*t));
        }
        scratch.sort_unstable();
        if scratch.windows(2).any(|w| w[0] == w[1]) {
            return Err(invalid("fans.triangles", "a fan holds the same triangle twice"));
        }
    }
    let mut out = Vec::new();
    out.try_reserve_exact(n).map_err(|_| PolyError::Allocation)?;
    for s in status {
        out.push(s.ok_or(PolyError::Inconsistent)?);
    }
    Ok(out)
}

/// Stitches `fans` (one per vertex of `vs`, from [`crate::fan::fans`]) into a mesh in
/// `frame` (module documentation).
pub fn stitch(fans: &[Fan], vs: &VertexSet, frame: FrameRef) -> Result<Stitched, PolyError> {
    let v = check_vertices(vs)?;
    let fan_status = check_fans(&v, fans)?;

    // Step 1: votes over ascending triples of mesh indices.
    let total: usize = fans.iter().map(|f| f.triangles.len()).sum();
    let mut all: Vec<[u64; 3]> = Vec::new();
    all.try_reserve_exact(total).map_err(|_| PolyError::Allocation)?;
    for fan in fans {
        for t in &fan.triangles {
            let mut m = [0u64; 3];
            for (slot, &s) in m.iter_mut().zip(t) {
                *slot = v.index(s).ok_or(PolyError::Inconsistent)?;
            }
            all.push(sorted3(m));
        }
    }
    all.sort_unstable();
    let mut cands: Vec<Cand> = Vec::new();
    for group in all.chunk_by(|a, b| a == b) {
        let tri = *group.first().ok_or(PolyError::Inconsistent)?;
        let votes = match group.len() {
            1 => 1u8,
            2 => 2,
            3 => 3,
            _ => return Err(PolyError::Inconsistent),
        };
        // Steps 2 and 3.
        let orient = if votes >= 2 { orient(&v, tri)? } else { None };
        cands.try_reserve(1).map_err(|_| PolyError::Allocation)?;
        cands.push(Cand { tri, votes, orient });
    }
    drop(all);

    // Edge-use table over the edges of the orientable candidates with two or three votes.
    let mut edges: Vec<[u64; 2]> = Vec::new();
    for c in cands.iter().filter(|c| c.votes >= 2 && c.orient.is_some()) {
        edges.try_reserve(3).map_err(|_| PolyError::Allocation)?;
        edges.extend(edges_of(c.tri));
    }
    edges.sort_unstable();
    edges.dedup();
    let mut uses: Vec<u32> = Vec::new();
    uses.try_reserve_exact(edges.len()).map_err(|_| PolyError::Allocation)?;
    uses.resize(edges.len(), 0);
    let slot = |e: [u64; 2]| edges.binary_search(&e).map_err(|_| PolyError::Inconsistent);

    // `fate[k]`: None = accepted (so far), Some(reason) = rejected.
    let mut fate: Vec<Option<RejectReason>> = Vec::new();
    fate.try_reserve_exact(cands.len()).map_err(|_| PolyError::Allocation)?;
    for c in &cands {
        fate.push(match (c.votes, c.orient) {
            (1, _) => Some(RejectReason::SingleVote),
            (_, None) => Some(RejectReason::Unorientable),
            _ => None,
        });
    }
    // Step 4: three votes.
    for (c, fate) in cands.iter().zip(&fate) {
        if c.votes == 3 && fate.is_none() {
            for e in edges_of(c.tri) {
                let u = uses.get_mut(slot(e)?).ok_or(PolyError::Inconsistent)?;
                *u = u.saturating_add(1);
            }
        }
    }
    // Step 5: two votes, ascending.
    for (c, fate) in cands.iter().zip(fate.iter_mut()) {
        if c.votes != 2 || fate.is_some() {
            continue;
        }
        let mut full = None;
        for e in edges_of(c.tri) {
            if *uses.get(slot(e)?).ok_or(PolyError::Inconsistent)? >= 2 {
                full = Some(e);
                break;
            }
        }
        match full {
            Some(e) => *fate = Some(RejectReason::EdgeFull { edge: source_edge(&v, e)? }),
            None => {
                for e in edges_of(c.tri) {
                    let u = uses.get_mut(slot(e)?).ok_or(PolyError::Inconsistent)?;
                    *u = u.saturating_add(1);
                }
            }
        }
    }

    // Step 6: cleanup, one pass over the edges in ascending order.
    let mut half: Vec<([u64; 2], usize)> = Vec::new();
    for (k, (c, fate)) in cands.iter().zip(&fate).enumerate() {
        if fate.is_none() {
            half.try_reserve(3).map_err(|_| PolyError::Allocation)?;
            half.extend(edges_of(c.tri).into_iter().map(|e| (e, k)));
        }
    }
    half.sort_unstable();
    let mut on_edge: Vec<(f64, [u64; 3], usize)> = Vec::new();
    for group in half.chunk_by(|a, b| a.0 == b.0) {
        if group.len() <= 2 {
            continue;
        }
        on_edge.clear();
        for &(_, k) in group {
            let c = cands.get(k).ok_or(PolyError::Inconsistent)?;
            if fate.get(k).ok_or(PolyError::Inconsistent)?.is_none() {
                let (_, key) = c.orient.ok_or(PolyError::Inconsistent)?;
                on_edge.try_reserve(1).map_err(|_| PolyError::Allocation)?;
                on_edge.push((key, c.tri, k));
            }
        }
        if on_edge.len() <= 2 {
            continue;
        }
        on_edge.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let edge = source_edge(&v, group.first().ok_or(PolyError::Inconsistent)?.0)?;
        for &(_, _, k) in on_edge.iter().skip(2) {
            *fate.get_mut(k).ok_or(PolyError::Inconsistent)? = Some(RejectReason::NonManifold { edge });
        }
    }
    drop(half);

    // Step 7: output.
    let n = v.sources.len();
    let mut used = Vec::new();
    used.try_reserve_exact(n).map_err(|_| PolyError::Allocation)?;
    used.resize(n, false);
    let (mut kept_three, mut kept_two) = (0u64, 0u64);
    let mut tris: Vec<[u64; 3]> = Vec::new();
    let mut rejected: Vec<Rejected> = Vec::new();
    for (c, fate) in cands.iter().zip(&fate) {
        match fate {
            Some(reason) => {
                let mut triangle = [0u64; 3];
                for (slot, &i) in triangle.iter_mut().zip(&c.tri) {
                    *slot = *v.sources.get(to_usize(i)?).ok_or(PolyError::Inconsistent)?;
                }
                rejected.try_reserve(1).map_err(|_| PolyError::Allocation)?;
                rejected.push(Rejected { triangle, votes: c.votes, reason: *reason });
            }
            None => {
                let (flip, _) = c.orient.ok_or(PolyError::Inconsistent)?;
                let [a, b, cc] = c.tri;
                tris.try_reserve(1).map_err(|_| PolyError::Allocation)?;
                tris.push(if flip { [a, cc, b] } else { [a, b, cc] });
                for i in c.tri {
                    *used.get_mut(to_usize(i)?).ok_or(PolyError::Inconsistent)? = true;
                }
                if c.votes == 3 {
                    kept_three += 1;
                } else {
                    kept_two += 1;
                }
            }
        }
    }
    let candidates = to_u64(cands.len())?;
    if tris.is_empty() {
        let ok_fans = to_u64(fan_status.iter().filter(|s| **s == FanStatus::Ok).count())?;
        return Err(PolyError::EmptyMesh { candidates, ok_fans });
    }
    let mut unused = Vec::new();
    for (i, u) in used.iter().enumerate() {
        if !*u {
            let source = *v.sources.get(i).ok_or(PolyError::Inconsistent)?;
            let fan = *fan_status.get(i).ok_or(PolyError::Inconsistent)?;
            unused.try_reserve(1).map_err(|_| PolyError::Allocation)?;
            unused.push(UnusedVertex { source, fan });
        }
    }

    let mut cols: [Vec<f64>; 6] = Default::default();
    for c in cols.iter_mut() {
        c.try_reserve_exact(n).map_err(|_| PolyError::Allocation)?;
    }
    for (p, q) in v.pos.iter().zip(&v.nrm) {
        for (k, c) in cols.iter_mut().enumerate() {
            c.push(if k < 3 { p[k] } else { q[k - 3] });
        }
    }
    let [x, y, z, nx, ny, nz] = cols;
    let attrs = AttributeSet { normal: Some([nx, ny, nz]), ..AttributeSet::default() };
    let mesh = Mesh::new(frame, PointBuffer::new(x, y, z), attrs, tris).map_err(mesh_error)?;
    if mesh.facts().nonmanifold_edges != 0 || mesh.facts().degenerate != 0 {
        return Err(PolyError::Inconsistent);
    }
    let moved_vertices = to_u64(v.status.iter().filter(|s| s.moved()).count())?;
    let report = StitchReport {
        vertex_sources: v.sources,
        vertex_status: v.status,
        moved_vertices,
        candidates,
        kept_three_vote: kept_three,
        kept_two_vote: kept_two,
        rejected,
        unused,
    };
    Ok(Stitched { mesh, report })
}

/// An edge of mesh indices as vertex sources.
fn source_edge(v: &Verts, e: [u64; 2]) -> Result<[u64; 2], PolyError> {
    let s = |i: u64| v.sources.get(to_usize(i)?).copied().ok_or(PolyError::Inconsistent);
    Ok([s(e[0])?, s(e[1])?])
}

/// PREP, FAN and STITCH on one cloud in memory: `prep::prepare(cloud, valid, params.prep)`,
/// `fan::fans(.., params.g, params.prep.threads)` and [`stitch`] in the cloud's frame. The
/// maximum gap is checked before any work starts.
pub fn reconstruct_in_memory(cloud: &PointCloud, valid: &[bool], params: &ReconParams) -> Result<Stitched, PolyError> {
    crate::fan::check_gap(params.g)?;
    let vs = prepare(cloud, valid, &params.prep)?;
    let f = fans(&vs, params.g, params.prep.threads)?;
    stitch(&f, &vs, cloud.frame())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::prep::PrepVertex;

    fn vs(points: &[([f64; 3], [f64; 3])]) -> VertexSet {
        let vertices = points
            .iter()
            .enumerate()
            .map(|(i, &(position, normal))| PrepVertex {
                source: 10 * i as u64,
                cell: [0; 3],
                position,
                normal,
                displacement_m: 0.0,
                status: VertexStatus::Unmoved,
            })
            .collect();
        VertexSet { vertices, h: 1e-3, smoothing: false, max_displacement_m: 0.0 }
    }

    #[test]
    fn orientation_follows_normals() {
        let z = [0.0, 0.0, 1.0];
        let set = vs(&[([0.0; 3], z), ([1.0, 0.0, 0.0], z), ([0.0, 1.0, 0.0], z)]);
        let v = check_vertices(&set).unwrap();
        assert_eq!(orient(&v, [0, 1, 2]).unwrap(), Some((false, 1.0)));
        let down = [0.0, 0.0, -1.0];
        let set = vs(&[([0.0; 3], down), ([1.0, 0.0, 0.0], down), ([0.0, 1.0, 0.0], down)]);
        let v = check_vertices(&set).unwrap();
        assert_eq!(orient(&v, [0, 1, 2]).unwrap(), Some((true, 1.0)));
        // Collinear: zero area.
        let set = vs(&[([0.0; 3], z), ([1.0, 0.0, 0.0], z), ([2.0, 0.0, 0.0], z)]);
        let v = check_vertices(&set).unwrap();
        assert_eq!(orient(&v, [0, 1, 2]).unwrap(), None);
    }

    #[test]
    fn huge_and_tiny_triangles_have_a_key() {
        let z = [0.0, 0.0, 1.0];
        for s in [2f64.powi(499), 2f64.powi(-1070)] {
            let set = vs(&[([0.0; 3], z), ([s, 0.0, 0.0], z), ([0.0, s, 0.0], z)]);
            let v = check_vertices(&set).unwrap();
            let got = orient(&v, [0, 1, 2]).unwrap();
            if s > 1.0 {
                assert_eq!(got, Some((false, 1.0)));
            } else {
                // The cross product underflows to zero: reported, never wound by chance.
                assert_eq!(got, None);
            }
        }
    }

    #[test]
    fn edge_order_is_pinned() {
        assert_eq!(edges_of([1, 5, 9]), [[1, 5], [1, 9], [5, 9]]);
    }
}
