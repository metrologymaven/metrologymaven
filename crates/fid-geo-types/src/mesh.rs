//! Indexed triangle mesh and topology facts (spec node `GEO.TYPES.MESH`).
//!
//! A [`Mesh`] is a frame, a vertex buffer ([`PointBuffer`], metres), per-vertex attributes
//! ([`AttributeSet`]), an indexed triangle list and optional per-triangle attributes
//! ([`TriangleAttrs`]). [`Mesh::new`] (and [`Mesh::with_options`]) validates everything
//! once, in a fixed order, and returns the first failure; no element is created on error:
//!
//! 1. the vertex columns have equal lengths (`AttributeLength`, names `y`, `z`), then
//!    every vertex coordinate is finite, in vertex order (`NonFinite { index }`);
//! 2. the vertex attributes, exactly as for a point set (`AttributeLength`, `BadNormal`,
//!    `ScanOrigin`, `GridShape`; see [`crate::points`]);
//! 3. every triangle index is less than the vertex count, in triangle order and corner
//!    order (`IndexOutOfRange { triangle, index }`, the first offender);
//! 4. every present per-triangle channel has one value per triangle (`AttributeLength`,
//!    names `triangle_scanner_id`, `triangle_quality`);
//! 5. the working memory for storage and topology fits the optional budget
//!    (`AllocationBudget`), and every working allocation succeeds (`AllocationFailed`);
//!    nothing allocates infallibly on a size taken from the input.
//!
//! The frame needs no run-time check: it is a required argument of every constructor and
//! has no `Default` (REQ-GEO.TYPES-002, REQ-GEO.TYPES.ENV-002).
//!
//! Construction then computes [`TopologyFacts`] and never alters a vertex or triangle
//! (REQ-GEO.TYPES.MESH-003). Facts are data, never repairs; degenerate and duplicate
//! triangles are kept and reported:
//!
//! * a triangle is degenerate when two of its indexes are equal; it is counted in
//!   `degenerate` and takes no further part (no edges, no component);
//! * every other triangle `(a, b, c)` contributes the directed edges `a->b`, `b->c`,
//!   `c->a`; an undirected edge used once is a boundary edge, more than twice a
//!   non-manifold edge, exactly twice in the same direction a misoriented pair;
//! * a duplicated triangle therefore shows up in these edge counts (as misoriented pairs
//!   when both copies are alone on their edges, as non-manifold edges inside a surface);
//! * `components` counts the connected classes of non-degenerate triangles that share an
//!   undirected edge;
//! * `closed` is true when there is no boundary and no non-manifold edge and at least one
//!   non-degenerate triangle; `oriented` is true when there is no misoriented pair and no
//!   non-manifold edge.
//!
//! The edge table is a list of half-edges sorted by a stable merge sort over a total
//! order, and components come from union-find over triangle indexes (REQ-005): no hash
//! containers, integer work only, so the facts are identical on every platform.
//!
//! Triangle indexes are stored as `u32` when the vertex count is at most 2³² and as
//! `u64` otherwise; the API always exposes `u64` (REQ-002). Conversions from `u64` to
//! `usize` are checked, so the code is correct on 32-bit targets such as `wasm32`.

use crate::envelope::{Aabb, FrameRef};
use crate::error::GeoTypeError;
use crate::points::{AttributeSet, PointBuffer, PointSet};
use core::mem::size_of;

/// One half-edge of the topology table: `[lo, hi, (triangle << 1) | forward]`, where
/// `lo < hi` are the edge's vertex indexes and `forward` is 1 when the triangle runs the
/// edge from `lo` to `hi`. Sorted lexicographically, which is a total order.
type HalfEdge = [u64; 3];

/// Budgeted bytes per half-edge: the record plus its merge-sort scratch copy.
pub const HALF_EDGE_COST: u64 = 2 * size_of::<HalfEdge>() as u64;
/// Budgeted bytes per triangle for the union-find parent array.
pub const PARENT_COST: u64 = size_of::<usize>() as u64;
/// Budgeted bytes per triangle for `u32` index storage (zero extra for `u64` storage,
/// which reuses the caller's vector).
pub const U32_TRIANGLE_COST: u64 = size_of::<[u32; 3]>() as u64;

/// Counts and indexes as `u64`; lossless on every supported target, saturating otherwise.
fn count_of(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// How triangle indexes are stored (REQ-GEO.TYPES.MESH-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexWidth {
    /// Three `u32` per triangle (vertex count at most 2³²).
    U32,
    /// Three `u64` per triangle.
    U64,
}

/// Triangle index storage.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TriIndex {
    U32(Vec<[u32; 3]>),
    U64(Vec<[u64; 3]>),
}

impl TriIndex {
    fn len(&self) -> usize {
        match self {
            TriIndex::U32(v) => v.len(),
            TriIndex::U64(v) => v.len(),
        }
    }

    fn get(&self, i: usize) -> Option<[u64; 3]> {
        match self {
            TriIndex::U32(v) => v.get(i).map(|t| [u64::from(t[0]), u64::from(t[1]), u64::from(t[2])]),
            TriIndex::U64(v) => v.get(i).copied(),
        }
    }

    fn iter(&self) -> impl Iterator<Item = [u64; 3]> + '_ {
        (0..self.len()).filter_map(move |i| self.get(i))
    }
}

/// Optional per-triangle attributes (one value per triangle when present).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TriangleAttrs {
    /// The scanner (or scan) each triangle came from.
    pub scanner_id: Option<Vec<u32>>,
    /// Quality flags per triangle.
    pub quality: Option<Vec<u16>>,
}

impl TriangleAttrs {
    fn validate(&self, count: u64) -> Result<(), GeoTypeError> {
        let check = |name: &str, found: usize| {
            let found = count_of(found);
            if found == count {
                Ok(())
            } else {
                Err(GeoTypeError::AttributeLength { name: name.to_string(), expected: count, found })
            }
        };
        if let Some(v) = &self.scanner_id {
            check("triangle_scanner_id", v.len())?;
        }
        if let Some(v) = &self.quality {
            check("triangle_quality", v.len())?;
        }
        Ok(())
    }
}

/// Construction options beyond the four required parts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MeshOptions {
    /// Per-triangle attributes.
    pub triangle_attrs: TriangleAttrs,
    /// Store indexes as `u64` even when they fit in `u32`. Facts and the API are
    /// unaffected (AC-5).
    pub force_u64_indexes: bool,
    /// The most bytes construction may allocate for index storage and the topology
    /// tables ([`Mesh::work_bytes`]). `None` means bounded only by available memory
    /// (REQ-GEO.TYPES-006); every allocation is fallible either way.
    pub work_budget_bytes: Option<u64>,
}

/// Topology facts of a mesh, as exact counts (REQ-GEO.TYPES.MESH-004). See the
/// [module documentation](crate::mesh) for the definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopologyFacts {
    /// Triangles with a repeated vertex index.
    pub degenerate: u64,
    /// Undirected edges used by exactly one non-degenerate triangle.
    pub boundary_edges: u64,
    /// Undirected edges used by more than two non-degenerate triangles.
    pub nonmanifold_edges: u64,
    /// Undirected edges used by exactly two triangles in the same direction.
    pub misoriented_pairs: u64,
    /// Connected classes of non-degenerate triangles sharing an edge.
    pub components: u64,
    /// No boundary edge, no non-manifold edge, and at least one non-degenerate triangle.
    pub closed: bool,
    /// No misoriented pair and no non-manifold edge.
    pub oriented: bool,
}

/// An indexed triangle mesh in one frame, validated at construction, with its
/// [`TopologyFacts`].
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    frame: FrameRef,
    vertices: PointBuffer,
    vertex_attrs: AttributeSet,
    triangles: TriIndex,
    triangle_attrs: TriangleAttrs,
    facts: TopologyFacts,
}

fn is_degenerate(t: [u64; 3]) -> bool {
    t[0] == t[1] || t[1] == t[2] || t[0] == t[2]
}

impl Mesh {
    /// A mesh in `frame` from vertices (metres), per-vertex attributes and triangles.
    /// Validates as described in the [module documentation](crate::mesh), returns the
    /// first failure, and computes the topology facts. Nothing is changed or repaired.
    pub fn new(
        frame: FrameRef,
        vertices: PointBuffer,
        vertex_attrs: AttributeSet,
        triangles: Vec<[u64; 3]>,
    ) -> Result<Mesh, GeoTypeError> {
        Mesh::with_options(frame, vertices, vertex_attrs, triangles, MeshOptions::default())
    }

    /// A mesh in `frame` from `[x, y, z]` vertices and triangles, without attributes.
    pub fn from_points(frame: FrameRef, vertices: &[[f64; 3]], triangles: &[[u64; 3]]) -> Result<Mesh, GeoTypeError> {
        let mut tris = Vec::new();
        reserve(&mut tris, triangles.len(), count_of(triangles.len()))?;
        tris.extend_from_slice(triangles);
        Mesh::new(frame, PointBuffer::from_points(vertices), AttributeSet::default(), tris)
    }

    /// [`Mesh::new`] with per-triangle attributes, forced `u64` storage or a working
    /// memory budget.
    pub fn with_options(
        frame: FrameRef,
        vertices: PointBuffer,
        vertex_attrs: AttributeSet,
        triangles: Vec<[u64; 3]>,
        options: MeshOptions,
    ) -> Result<Mesh, GeoTypeError> {
        // 1 and 2: vertices and their attributes, as for a point set.
        vertices.validate()?;
        let n = vertices.len();
        vertex_attrs.validate(n)?;
        // 3: index range, first offender in triangle then corner order.
        let mut degenerate: u64 = 0;
        for (t, tri) in triangles.iter().enumerate() {
            for &v in tri {
                if v >= n {
                    return Err(GeoTypeError::IndexOutOfRange { triangle: count_of(t), index: v });
                }
            }
            if is_degenerate(*tri) {
                degenerate += 1;
            }
        }
        // 4: per-triangle attributes.
        let t_count = count_of(triangles.len());
        options.triangle_attrs.validate(t_count)?;
        // 5: budget, then storage.
        let use_u32 = !options.force_u64_indexes && n <= (1u64 << 32);
        let needed = Mesh::work_bytes(t_count, t_count - degenerate, use_u32);
        if let Some(limit) = options.work_budget_bytes {
            match needed {
                Some(b) if b <= limit => {}
                _ => return Err(GeoTypeError::AllocationBudget { needed: needed.unwrap_or(u64::MAX), limit }),
            }
        }
        let needed = needed.unwrap_or(u64::MAX);
        let triangles = if use_u32 {
            let mut v: Vec<[u32; 3]> = Vec::new();
            reserve(&mut v, triangles.len(), needed)?;
            for (t, tri) in triangles.iter().enumerate() {
                let mut out = [0u32; 3];
                for (o, &i) in out.iter_mut().zip(tri) {
                    // Cannot fail (i < n <= 2^32); fails closed if it ever could.
                    *o = u32::try_from(i)
                        .map_err(|_| GeoTypeError::IndexOutOfRange { triangle: count_of(t), index: i })?;
                }
                v.push(out);
            }
            TriIndex::U32(v)
        } else {
            TriIndex::U64(triangles)
        };
        let facts = topology(&triangles, degenerate, needed)?;
        Ok(Mesh { frame, vertices, vertex_attrs, triangles, triangle_attrs: options.triangle_attrs, facts })
    }

    /// The bytes construction allocates beyond its inputs for `triangles` triangles of
    /// which `nondegenerate` are not degenerate: `u32` storage (when `u32_storage`), three
    /// half-edges per non-degenerate triangle with their sort scratch, and one union-find
    /// parent per triangle. `None` if the count overflows `u64`.
    pub fn work_bytes(triangles: u64, nondegenerate: u64, u32_storage: bool) -> Option<u64> {
        let store = if u32_storage { triangles.checked_mul(U32_TRIANGLE_COST)? } else { 0 };
        let edges = nondegenerate.checked_mul(3)?.checked_mul(HALF_EDGE_COST)?;
        let parents = triangles.checked_mul(PARENT_COST)?;
        store.checked_add(edges)?.checked_add(parents)
    }

    /// The frame the vertices are expressed in.
    pub fn frame(&self) -> FrameRef {
        self.frame
    }

    /// The vertex buffer.
    pub fn vertices(&self) -> &PointBuffer {
        &self.vertices
    }

    /// The per-vertex attributes.
    pub fn vertex_attrs(&self) -> &AttributeSet {
        &self.vertex_attrs
    }

    /// The per-triangle attributes.
    pub fn triangle_attrs(&self) -> &TriangleAttrs {
        &self.triangle_attrs
    }

    /// The number of vertices.
    pub fn vertex_count(&self) -> u64 {
        self.vertices.len()
    }

    /// Vertex `i`, if it exists.
    pub fn vertex(&self, i: u64) -> Option<[f64; 3]> {
        self.vertices.point(i)
    }

    /// The number of triangles.
    pub fn triangle_count(&self) -> u64 {
        count_of(self.triangles.len())
    }

    /// Triangle `i` as three vertex indexes, if it exists.
    pub fn triangle(&self, i: u64) -> Option<[u64; 3]> {
        self.triangles.get(usize::try_from(i).ok()?)
    }

    /// The triangles in index order, as `u64` index triples.
    pub fn triangles(&self) -> impl Iterator<Item = [u64; 3]> + '_ {
        self.triangles.iter()
    }

    /// How the indexes are stored.
    pub fn index_width(&self) -> IndexWidth {
        match self.triangles {
            TriIndex::U32(_) => IndexWidth::U32,
            TriIndex::U64(_) => IndexWidth::U64,
        }
    }

    /// The topology facts computed at construction.
    pub fn facts(&self) -> TopologyFacts {
        self.facts
    }

    /// The bounding box of the vertices; `Empty` when there are none.
    pub fn aabb(&self) -> Result<Aabb, GeoTypeError> {
        self.vertices.aabb()
    }

    /// The vertices and their attributes, in vertex order, as a new [`PointSet`] in the
    /// same frame (REQ-GEO.TYPES.ENV-004).
    pub fn vertices_to_point_set(&self) -> PointSet {
        PointSet::from_validated(self.frame, self.vertices.clone(), self.vertex_attrs.clone())
    }
}

/// Reserves room for `n` more elements or fails closed.
fn reserve<T>(v: &mut Vec<T>, n: usize, needed: u64) -> Result<(), GeoTypeError> {
    v.try_reserve_exact(n).map_err(|_| GeoTypeError::AllocationFailed { needed })
}

/// The topology facts of validated triangles (`degenerate` already counted).
fn topology(tris: &TriIndex, degenerate: u64, needed: u64) -> Result<TopologyFacts, GeoTypeError> {
    let t_len = tris.len();
    let live = t_len.saturating_sub(usize::try_from(degenerate).unwrap_or(usize::MAX));
    let he_len = live.checked_mul(3).ok_or(GeoTypeError::AllocationFailed { needed })?;
    let mut edges: Vec<HalfEdge> = Vec::new();
    reserve(&mut edges, he_len, needed)?;
    for (t, tri) in tris.iter().enumerate() {
        if is_degenerate(tri) {
            continue;
        }
        let tag = count_of(t).checked_mul(2).ok_or(GeoTypeError::AllocationFailed { needed })?;
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            edges.push(if a < b { [a, b, tag | 1] } else { [b, a, tag] });
        }
    }
    let mut scratch: Vec<HalfEdge> = Vec::new();
    reserve(&mut scratch, edges.len(), needed)?;
    scratch.extend_from_slice(&edges);
    merge_sort(&mut edges, &mut scratch);
    drop(scratch);

    let mut parent: Vec<usize> = Vec::new();
    reserve(&mut parent, t_len, needed)?;
    parent.extend(0..t_len);

    let (mut boundary, mut nonmanifold, mut misoriented) = (0u64, 0u64, 0u64);
    for run in edges.chunk_by(|x, y| x[0] == y[0] && x[1] == y[1]) {
        match run {
            [_] => boundary += 1,
            [x, y] => {
                if (x[2] & 1) == (y[2] & 1) {
                    misoriented += 1;
                }
            }
            _ => nonmanifold += 1,
        }
        if let Some((first, rest)) = run.split_first() {
            let a = tri_of(first)?;
            for h in rest {
                union(&mut parent, a, tri_of(h)?);
            }
        }
    }
    let mut components = 0u64;
    for (t, tri) in tris.iter().enumerate() {
        if !is_degenerate(tri) && parent.get(t) == Some(&t) {
            components += 1;
        }
    }
    let live = count_of(live);
    Ok(TopologyFacts {
        degenerate,
        boundary_edges: boundary,
        nonmanifold_edges: nonmanifold,
        misoriented_pairs: misoriented,
        components,
        closed: boundary == 0 && nonmanifold == 0 && live > 0,
        oriented: misoriented == 0 && nonmanifold == 0,
    })
}

/// The triangle index of a half-edge, as a checked `usize` (32-bit targets included).
fn tri_of(h: &HalfEdge) -> Result<usize, GeoTypeError> {
    let t = h[2] >> 1;
    usize::try_from(t).map_err(|_| GeoTypeError::IndexOutOfRange { triangle: t, index: t })
}

/// The root of `x`, with path halving. A parent never exceeds its child's index, so the
/// walk strictly decreases and ends.
fn find(parent: &mut [usize], mut x: usize) -> usize {
    loop {
        let p = match parent.get(x) {
            Some(&p) => p,
            None => return x,
        };
        if p == x {
            return x;
        }
        let gp = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(x) {
            *slot = gp;
        }
        x = gp;
    }
}

/// Joins the classes of `a` and `b`; the smaller root index becomes the root, so the
/// result does not depend on the order of unions.
fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
        if let Some(slot) = parent.get_mut(hi) {
            *slot = lo;
        }
    }
}

/// Stable bottom-up merge sort of `data`, using `scratch` (a copy of `data`, allocated
/// by the caller) so that nothing allocates here.
fn merge_sort(data: &mut [HalfEdge], scratch: &mut [HalfEdge]) {
    let n = data.len();
    if scratch.len() != n {
        // Callers always pass equal lengths; std's stable sort keeps the result correct.
        data.sort();
        return;
    }
    let mut width = 1usize;
    let mut in_data = true;
    while width < n {
        let span = width.saturating_mul(2);
        {
            let (src, dst): (&[HalfEdge], &mut [HalfEdge]) =
                if in_data { (&*data, &mut *scratch) } else { (&*scratch, &mut *data) };
            for (s, d) in src.chunks(span).zip(dst.chunks_mut(span)) {
                let (a, b) = s.split_at(if width < s.len() { width } else { s.len() });
                merge(a, b, d);
            }
        }
        in_data = !in_data;
        width = span;
    }
    if !in_data {
        data.copy_from_slice(scratch);
    }
}

/// Merges sorted `a` and `b` into `out` (`a.len() + b.len() == out.len()`); ties take
/// from `a`, which keeps the sort stable.
fn merge(a: &[HalfEdge], b: &[HalfEdge], out: &mut [HalfEdge]) {
    let (mut i, mut j) = (0usize, 0usize);
    for o in out.iter_mut() {
        match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) if x <= y => {
                *o = *x;
                i += 1;
            }
            (Some(x), None) => {
                *o = *x;
                i += 1;
            }
            (_, Some(y)) => {
                *o = *y;
                j += 1;
            }
            (None, None) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// Allocation budgets: every per-item cost covers at least the item's size (the
    /// half-edge cost also covers its sort-scratch copy).
    #[test]
    fn costs_cover_sizes() {
        let pairs = [
            ("half-edge", HALF_EDGE_COST, 2 * size_of::<HalfEdge>()),
            ("parent", PARENT_COST, size_of::<usize>()),
            ("u32 triangle", U32_TRIANGLE_COST, size_of::<[u32; 3]>()),
        ];
        for (name, cost, size) in pairs {
            assert!(cost >= size as u64, "{name}: cost {cost} < size {size}");
        }
    }

    /// The merge sort agrees with std's stable sort on scrambled input with tied keys.
    #[test]
    fn merge_sort_matches_std() {
        for n in [0usize, 1, 2, 3, 5, 8, 13, 64, 100, 257] {
            let mut x: u64 = 12345;
            let mut v: Vec<HalfEdge> = (0..n)
                .map(|i| {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    [(x >> 60) & 3, (x >> 50) & 3, i as u64]
                })
                .collect();
            let mut want = v.clone();
            want.sort();
            let mut scratch = v.clone();
            merge_sort(&mut v, &mut scratch);
            assert_eq!(v, want, "n = {n}");
        }
    }

    /// Union-find picks the smallest index as root regardless of union order.
    #[test]
    fn union_find_roots() {
        let mut p: Vec<usize> = (0..6).collect();
        union(&mut p, 5, 3);
        union(&mut p, 3, 4);
        union(&mut p, 1, 0);
        assert_eq!(find(&mut p, 5), 3);
        assert_eq!(find(&mut p, 4), 3);
        assert_eq!(find(&mut p, 1), 0);
        assert_eq!(find(&mut p, 2), 2);
        union(&mut p, 4, 1);
        assert_eq!(find(&mut p, 5), 0);
    }
}
