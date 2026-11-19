//! Mesh quality report (spec node `SCAN.POLY.QUAL`).
//!
//! [`mesh_quality`] measures a GEO.TYPES [`Mesh`] and returns a fixed-shape
//! [`MeshQualityReport`]. Polygonization attaches it to every result and `SCAN.MESHOP`
//! reuses it for before/after reports, so these definitions are the only ones.
//!
//! # Metric definitions (pinned)
//!
//! * **Triangles** and **vertices** are the mesh's triangle and vertex counts; a vertex no
//!   triangle uses still counts.
//! * A triangle with a repeated vertex index is **index-degenerate**: it counts in
//!   `triangles` and `degenerate` and takes no further part (no edges, no component, no
//!   fan, no angle), as in GEO.TYPES.MESH.
//! * Every other triangle `(a, b, c)` has the **area** `0.5 * sqrt((cx*cx + cy*cy) + cz*cz)`
//!   with `(cx, cy, cz) = (b - a) × (c - a)`, evaluated in exactly that order (no FMA). It is
//!   **degenerate** when `area < degenerate_area` (strict; REQ-004). A degenerate triangle
//!   leaves only the angle statistics; it still has edges, a component and fans.
//! * **Edges** are the undirected vertex pairs of the non-index-degenerate triangles. An
//!   edge's uses are the triangles that have it (duplicate triangles count separately). A
//!   **boundary edge** has one use; a **non-manifold edge** has three or more (REQ-002).
//! * **Components** are the classes of non-index-degenerate triangles linked by a shared
//!   edge; triangles meeting only at a vertex are separate components.
//! * A **non-manifold vertex** has incident triangles that form more than one fan, two
//!   incident triangles being in one fan when a chain of incident triangles links them,
//!   each consecutive pair sharing an edge that contains the vertex (REQ-002). Vertices
//!   with no triangle have no fan. The endpoints of a non-manifold edge are not non-manifold
//!   on that account (the three triangles of a fin share the edge, so form one fan).
//! * **Boundary loops** come from a deterministic walk over the boundary edges, undirected
//!   (so an inconsistently oriented mesh such as a Möbius strip still closes its loop). A
//!   walk starts at the smallest vertex that still has an unused boundary edge; at every
//!   step it takes the unused boundary edge at the current vertex whose other end is
//!   smallest; it ends when it is back at its start vertex (a closed loop) or when the
//!   current vertex has no unused boundary edge (an open chain, which happens only at
//!   vertices with an odd number of boundary edges). Every walk counts as one loop. When
//!   every boundary vertex has exactly two boundary edges, this is the number of boundary
//!   cycles. At a non-manifold vertex with four or more boundary edges the "smallest other
//!   end" rule decides how the cycles are split (two fans of a bow tie give two loops).
//! * **Minimum angle** of a non-degenerate triangle: at each corner `p` in stored order
//!   (`a`, `b`, `c`), with `u` and `v` the vectors to the next and the previous corner,
//!   `cos = dot(u, v) / (sqrt(dot(u, u)) * sqrt(dot(v, v)))` with
//!   `dot(p, q) = (p.x*q.x + p.y*q.y) + p.z*q.z`, clamped to `[-1, 1]` by comparison, and
//!   the angle is `fid_num::f::acos(cos)` in radians; the smallest of the three (REQ-003).
//!   `acos` of a clamped cosine is used rather than `atan2(|u × v|, u · v)` because the
//!   spec's Design names it; for the slivers where the two differ in the last bits the
//!   triangle is degenerate by area anyway.
//! * **Angle statistics** over the non-degenerate triangles: `count`, `min`, and the
//!   nearest-rank percentiles `p1`, `p5` and `median` (p50): the value at 1-based rank
//!   `ceil(p * n / 100)` of the ascending list (IEEE total order). With no non-degenerate
//!   triangle the four values are absent (`null`). No histogram is reported.
//!
//! # Errors
//!
//! Checked in triangle order; the first failure is returned and no report is made.
//! `degenerate_area` must be finite and greater than zero (`scan.mesh.invalid_threshold`;
//! zero is refused so that a zero-area triangle, whose cosine would be `0/0`, is always
//! degenerate). A vertex index not below the vertex count is `scan.mesh.invalid_index`
//! (GEO.TYPES already refuses such a mesh, so this only fails closed). A non-finite
//! coordinate, area or cosine (for example coordinates near `1e200`, whose squares
//! overflow) is `scan.mesh.non_finite`. A failed working allocation is
//! `scan.mesh.allocation_failed`; nothing allocates infallibly on a size from the input.
//!
//! # Determinism (REQ-005, D0)
//!
//! Integer work over sorted vectors (`sort_unstable` on keys that are unique, so the order
//! is fully determined), union-find whose root is always the smaller index, one fixed
//! evaluation order for every float expression, no FMA, no `f64` min/max, no hash
//! containers, no threads. The report and its [`MeshQualityReport::canonical_json`] are
//! therefore bit-identical on every platform and for any number of caller threads.
//!
//! Working memory is about 200 bytes per triangle on 64-bit targets: three edge records
//! (24 bytes each), three vertex incidences (16 bytes) with their union-find parents
//! (8 bytes), one triangle parent (8 bytes) and one angle (8 bytes).

use core::fmt;
use fid_geo_types::Mesh;

use crate::number::es_number;

/// The default degenerate-area threshold in square metres (REQ-SCAN.POLY.QUAL-004).
pub const DEFAULT_DEGENERATE_AREA: f64 = 1e-18;

/// Minimum-angle statistics in radians over the non-degenerate triangles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AngleStats {
    /// The number of triangles the statistics cover.
    pub count: u64,
    /// The smallest minimum angle; `None` when `count` is 0.
    pub min: Option<f64>,
    /// The nearest-rank 1st percentile; `None` when `count` is 0.
    pub p1: Option<f64>,
    /// The nearest-rank 5th percentile; `None` when `count` is 0.
    pub p5: Option<f64>,
    /// The nearest-rank median (50th percentile); `None` when `count` is 0.
    pub median: Option<f64>,
}

/// The fixed-shape quality report of a mesh. See the [module documentation](self) for the
/// definitions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshQualityReport {
    /// Triangles in the mesh, degenerate ones included.
    pub triangles: u64,
    /// Vertices in the mesh, unreferenced ones included.
    pub vertices: u64,
    /// Distinct undirected edges of the non-index-degenerate triangles.
    pub edges: u64,
    /// Edges used by exactly one triangle.
    pub boundary_edges: u64,
    /// Walks over the boundary edges.
    pub boundary_loops: u64,
    /// Edges used by three or more triangles.
    pub nonmanifold_edges: u64,
    /// Vertices whose incident triangles form more than one fan.
    pub nonmanifold_vertices: u64,
    /// Edge-connected classes of non-index-degenerate triangles.
    pub components: u64,
    /// Triangles with a repeated index or an area below the threshold.
    pub degenerate: u64,
    /// Minimum-angle statistics of the other triangles.
    pub min_angle: AngleStats,
}

/// Every error [`mesh_quality`] can return. Errors are values; nothing here panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityError {
    /// A triangle refers to a vertex that does not exist. Code `scan.mesh.invalid_index`.
    InvalidIndex { triangle: u64, index: u64 },
    /// A coordinate, area or cosine of the triangle is NaN or infinite. Code
    /// `scan.mesh.non_finite`.
    NonFinite { triangle: u64 },
    /// The degenerate-area threshold is not finite or not greater than zero. Code
    /// `scan.mesh.invalid_threshold`.
    InvalidThreshold,
    /// A working allocation failed. Code `scan.mesh.allocation_failed`.
    AllocationFailed,
}

impl QualityError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            QualityError::InvalidIndex { .. } => "scan.mesh.invalid_index",
            QualityError::NonFinite { .. } => "scan.mesh.non_finite",
            QualityError::InvalidThreshold => "scan.mesh.invalid_threshold",
            QualityError::AllocationFailed => "scan.mesh.allocation_failed",
        }
    }
}

impl fmt::Display for QualityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QualityError::InvalidIndex { triangle, index } => {
                write!(f, "{}: triangle {triangle} uses vertex {index}", self.code())
            }
            QualityError::NonFinite { triangle } => write!(f, "{}: triangle {triangle}", self.code()),
            QualityError::InvalidThreshold | QualityError::AllocationFailed => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for QualityError {}

impl MeshQualityReport {
    /// The report as kernel canonical JSON (RFC 8785 rules; every number written as a
    /// string, integers in decimal and angles in the ECMAScript number form; an absent
    /// statistic is `null`). Keys, in order: `boundary_edges`, `boundary_loops`,
    /// `components`, `degenerate`, `edges`, `min_angle` (`count`, `median`, `min`, `p1`,
    /// `p5`), `nonmanifold_edges`, `nonmanifold_vertices`, `triangles`, `vertices`.
    ///
    /// Fails with `scan.mesh.non_finite` only for a hand-built report holding a
    /// non-finite angle; [`mesh_quality`] never produces one.
    pub fn canonical_json(&self) -> Result<Vec<u8>, QualityError> {
        let s = &self.min_angle;
        let angle = |v: Option<f64>| -> Result<String, QualityError> {
            match v {
                None => Ok("null".to_string()),
                Some(x) => es_number(x).map(quoted).ok_or(QualityError::NonFinite { triangle: 0 }),
            }
        };
        let stats = format!(
            "{{\"count\":{},\"median\":{},\"min\":{},\"p1\":{},\"p5\":{}}}",
            quoted(s.count.to_string()),
            angle(s.median)?,
            angle(s.min)?,
            angle(s.p1)?,
            angle(s.p5)?,
        );
        let n = |v: u64| quoted(v.to_string());
        let out = format!(
            "{{\"boundary_edges\":{},\"boundary_loops\":{},\"components\":{},\"degenerate\":{},\"edges\":{},\
             \"min_angle\":{},\"nonmanifold_edges\":{},\"nonmanifold_vertices\":{},\"triangles\":{},\"vertices\":{}}}",
            n(self.boundary_edges),
            n(self.boundary_loops),
            n(self.components),
            n(self.degenerate),
            n(self.edges),
            stats,
            n(self.nonmanifold_edges),
            n(self.nonmanifold_vertices),
            n(self.triangles),
            n(self.vertices),
        );
        Ok(out.into_bytes())
    }
}

/// A JSON string of text that needs no escaping (digits, signs, `.`, `e`).
fn quoted(s: String) -> String {
    format!("\"{s}\"")
}

/// Measures `mesh` with the degenerate-area threshold `degenerate_area` (m², use
/// [`DEFAULT_DEGENERATE_AREA`] for the default). Pure and D0; see the
/// [module documentation](self) for every definition and error.
pub fn mesh_quality(mesh: &Mesh, degenerate_area: f64) -> Result<MeshQualityReport, QualityError> {
    if !(degenerate_area.is_finite() && degenerate_area > 0.0) {
        return Err(QualityError::InvalidThreshold);
    }
    let vertices = mesh.vertex_count();
    let triangles = mesh.triangle_count();
    let t_len = usize::try_from(triangles).map_err(|_| QualityError::AllocationFailed)?;

    // Pass 1, in triangle order: indexes, degeneracy, areas and minimum angles.
    let mut angles: Vec<f64> = alloc(t_len)?;
    let mut live: Vec<bool> = alloc(t_len)?;
    let mut live_count = 0usize;
    let mut degenerate = 0u64;
    for (t, tri) in mesh.triangles().enumerate() {
        let t64 = t as u64;
        let mut p = [[0.0f64; 3]; 3];
        for (slot, &i) in p.iter_mut().zip(tri.iter()) {
            *slot = mesh.vertex(i).ok_or(QualityError::InvalidIndex { triangle: t64, index: i })?;
        }
        if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
            degenerate += 1;
            live.push(false);
            continue;
        }
        live.push(true);
        live_count += 1;
        if p.iter().flatten().any(|x| !x.is_finite()) {
            return Err(QualityError::NonFinite { triangle: t64 });
        }
        let non_finite = |_| QualityError::NonFinite { triangle: t64 };
        let a = area(p[0], p[1], p[2]).map_err(non_finite)?;
        if a < degenerate_area {
            degenerate += 1;
            continue;
        }
        angles.push(min_angle(p[0], p[1], p[2]).map_err(non_finite)?);
    }

    // Edge records `[lo, hi, triangle]` of the live triangles, sorted (keys are unique: a
    // live triangle has three distinct undirected edges).
    let mut edges: Vec<[u64; 3]> = alloc(live_count.checked_mul(3).ok_or(QualityError::AllocationFailed)?)?;
    // Vertex incidences `[vertex, triangle]`, sorted (unique for the same reason).
    let mut inc: Vec<[u64; 2]> = alloc(live_count.checked_mul(3).ok_or(QualityError::AllocationFailed)?)?;
    for ((t, tri), &is_live) in mesh.triangles().enumerate().zip(live.iter()) {
        if !is_live {
            continue;
        }
        let t = t as u64;
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            edges.push(if a < b { [a, b, t] } else { [b, a, t] });
        }
        for v in tri {
            inc.push([v, t]);
        }
    }
    edges.sort_unstable();
    inc.sort_unstable();

    let mut tri_parent: Vec<usize> = alloc(t_len)?;
    tri_parent.extend(0..t_len);
    let mut fan_parent: Vec<usize> = alloc(inc.len())?;
    fan_parent.extend(0..inc.len());

    let (mut edge_count, mut boundary_count, mut nonmanifold_edges) = (0u64, 0usize, 0u64);
    for run in edges.chunk_by(|x, y| x[0] == y[0] && x[1] == y[1]) {
        edge_count += 1;
        match run.len() {
            1 => boundary_count += 1,
            2 => {}
            _ => nonmanifold_edges += 1,
        }
        let Some((first, rest)) = run.split_first() else { continue };
        let t0 = to_usize(first[2])?;
        let lo0 = incidence(&inc, first[0], first[2])?;
        let hi0 = incidence(&inc, first[1], first[2])?;
        for h in rest {
            union(&mut tri_parent, t0, to_usize(h[2])?);
            union(&mut fan_parent, lo0, incidence(&inc, h[0], h[2])?);
            union(&mut fan_parent, hi0, incidence(&inc, h[1], h[2])?);
        }
    }

    let mut components = 0u64;
    for (t, &is_live) in live.iter().enumerate() {
        if is_live && find(&mut tri_parent, t) == t {
            components += 1;
        }
    }

    let mut nonmanifold_vertices = 0u64;
    let mut base = 0usize;
    for group in inc.chunk_by(|x, y| x[0] == y[0]) {
        let mut fans = 0u64;
        for k in base..base + group.len() {
            if find(&mut fan_parent, k) == k {
                fans += 1;
            }
        }
        if fans > 1 {
            nonmanifold_vertices += 1;
        }
        base += group.len();
    }
    drop(fan_parent);
    drop(inc);

    let boundary_loops = boundary_walks(&edges, boundary_count)?;

    Ok(MeshQualityReport {
        triangles,
        vertices,
        edges: edge_count,
        boundary_edges: boundary_count as u64,
        boundary_loops,
        nonmanifold_edges,
        nonmanifold_vertices,
        components,
        degenerate,
        min_angle: angle_stats(&mut angles)?,
    })
}

/// An empty vector with room for `n` elements, or `AllocationFailed`.
fn alloc<T>(n: usize) -> Result<Vec<T>, QualityError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| QualityError::AllocationFailed)?;
    Ok(v)
}

fn to_usize(x: u64) -> Result<usize, QualityError> {
    usize::try_from(x).map_err(|_| QualityError::AllocationFailed)
}

/// The position of the incidence `[v, t]` in the sorted list. Every edge endpoint of a live
/// triangle is one of its incidences, so a miss cannot happen; it fails closed.
fn incidence(inc: &[[u64; 2]], v: u64, t: u64) -> Result<usize, QualityError> {
    inc.binary_search(&[v, t]).map_err(|_| QualityError::InvalidIndex { triangle: t, index: v })
}

fn sub(p: [f64; 3], q: [f64; 3]) -> [f64; 3] {
    [p[0] - q[0], p[1] - q[1], p[2] - q[2]]
}

fn dot(p: [f64; 3], q: [f64; 3]) -> f64 {
    (p[0] * q[0] + p[1] * q[1]) + p[2] * q[2]
}

/// `0.5 * |(b - a) × (c - a)|`, in the pinned evaluation order.
fn area(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Result<f64, fid_num::NumError> {
    let u = sub(b, a);
    let v = sub(c, a);
    let cx = u[1] * v[2] - u[2] * v[1];
    let cy = u[2] * v[0] - u[0] * v[2];
    let cz = u[0] * v[1] - u[1] * v[0];
    let r = fid_num::f::sqrt((cx * cx + cy * cy) + cz * cz)?;
    Ok(0.5 * r)
}

/// The interior angle at `p` between the directions to `next` and `prev`.
fn corner_angle(p: [f64; 3], next: [f64; 3], prev: [f64; 3]) -> Result<f64, fid_num::NumError> {
    let u = sub(next, p);
    let v = sub(prev, p);
    let mut c = dot(u, v) / (fid_num::f::sqrt(dot(u, u))? * fid_num::f::sqrt(dot(v, v))?);
    // NaN passes both comparisons and is refused by `acos`.
    if c > 1.0 {
        c = 1.0;
    }
    if c < -1.0 {
        c = -1.0;
    }
    fid_num::f::acos(c)
}

/// The smallest interior angle, corners in stored order.
fn min_angle(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Result<f64, fid_num::NumError> {
    let mut m = corner_angle(a, b, c)?;
    let y = corner_angle(b, c, a)?;
    if y < m {
        m = y;
    }
    let z = corner_angle(c, a, b)?;
    if z < m {
        m = z;
    }
    Ok(m)
}

/// The 1-based nearest rank `ceil(p * n / 100)`, at least 1, as a 0-based index.
fn nearest_rank_index(p: u64, n: usize) -> usize {
    let rank = (u128::from(p) * n as u128).div_ceil(100);
    let idx = rank.saturating_sub(1);
    usize::try_from(idx).unwrap_or(usize::MAX)
}

fn angle_stats(angles: &mut [f64]) -> Result<AngleStats, QualityError> {
    angles.sort_unstable_by(|a, b| a.total_cmp(b));
    let n = angles.len();
    let at = |p: u64| angles.get(nearest_rank_index(p, n)).copied();
    Ok(AngleStats { count: n as u64, min: angles.first().copied(), p1: at(1), p5: at(5), median: at(50) })
}

/// The number of walks over the boundary edges (runs of length 1 in the sorted edge list),
/// as pinned in the module documentation.
fn boundary_walks(edges: &[[u64; 3]], boundary_count: usize) -> Result<u64, QualityError> {
    // Both directions of every boundary edge: `(from, to, edge id)`, sorted.
    let len = boundary_count.checked_mul(2).ok_or(QualityError::AllocationFailed)?;
    let mut adj: Vec<(u64, u64, usize)> = alloc(len)?;
    let mut id = 0usize;
    for run in edges.chunk_by(|x, y| x[0] == y[0] && x[1] == y[1]) {
        if let [e] = run {
            adj.push((e[0], e[1], id));
            adj.push((e[1], e[0], id));
            id += 1;
        }
    }
    adj.sort_unstable();
    // Vertex groups: the distinct `from` vertices, where each group starts, and a cursor
    // that only moves forward past used edges.
    let mut verts: Vec<u64> = alloc(len)?;
    let mut start: Vec<usize> = alloc(len.checked_add(1).ok_or(QualityError::AllocationFailed)?)?;
    let mut base = 0usize;
    for group in adj.chunk_by(|x, y| x.0 == y.0) {
        if let Some(first) = group.first() {
            verts.push(first.0);
            start.push(base);
        }
        base += group.len();
    }
    start.push(base);
    let mut cursor: Vec<usize> = alloc(verts.len())?;
    cursor.extend(start.iter().take(verts.len()).copied());
    let mut used: Vec<bool> = alloc(id)?;
    used.resize(id, false);

    let mut loops = 0u64;
    for g in 0..verts.len() {
        let Some(&origin) = verts.get(g) else { break };
        while next_unused(&adj, &start, &mut cursor, &used, g).is_some() {
            loops += 1;
            let mut cur = g;
            while let Some(pos) = next_unused(&adj, &start, &mut cursor, &used, cur) {
                let Some(&(_, to, e)) = adj.get(pos) else { break };
                if let Some(u) = used.get_mut(e) {
                    *u = true;
                }
                cur = verts.binary_search(&to).map_err(|_| QualityError::InvalidIndex { triangle: 0, index: to })?;
                if to == origin {
                    break;
                }
            }
        }
    }
    Ok(loops)
}

/// The position of the first unused boundary edge of vertex group `g` (smallest other end),
/// advancing the group's cursor past used edges.
fn next_unused(
    adj: &[(u64, u64, usize)],
    start: &[usize],
    cursor: &mut [usize],
    used: &[bool],
    g: usize,
) -> Option<usize> {
    let end = *start.get(g + 1)?;
    let c = cursor.get_mut(g)?;
    while *c < end {
        let (_, _, e) = *adj.get(*c)?;
        if !*used.get(e)? {
            return Some(*c);
        }
        *c += 1;
    }
    None
}

/// The root of `x`, with path halving. A parent never exceeds its child's index.
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

/// Joins the classes of `a` and `b`; the smaller root becomes the root, so the result does
/// not depend on the order of unions.
fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
        if let Some(slot) = parent.get_mut(hi) {
            *slot = lo;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// Nearest rank, hand-labelled: ceil(p * n / 100), at least 1.
    #[test]
    fn nearest_rank() {
        let cases = [
            (1, 1, 0),
            (50, 1, 0),
            (1, 99, 0),
            (1, 100, 0),
            (1, 101, 1),
            (5, 200, 9),
            (50, 200, 99),
            (50, 7, 3),
            (5, 7, 0),
            (50, 0, 0),
        ];
        for (p, n, want) in cases {
            assert_eq!(nearest_rank_index(p, n), want, "p = {p}, n = {n}");
        }
    }

    /// The statistics of a hand-made list.
    #[test]
    fn stats_of_list() {
        let mut v: Vec<f64> = (0..200).rev().map(|i| f64::from(i) / 100.0).collect();
        let s = angle_stats(&mut v).unwrap();
        assert_eq!(s.count, 200);
        assert_eq!(s.min, Some(0.0));
        assert_eq!(s.p1, Some(0.01));
        assert_eq!(s.p5, Some(0.09));
        assert_eq!(s.median, Some(0.99));
        let s = angle_stats(&mut []).unwrap();
        assert_eq!((s.count, s.min, s.p1, s.p5, s.median), (0, None, None, None, None));
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
        union(&mut p, 4, 1);
        assert_eq!(find(&mut p, 5), 0);
        assert_eq!(find(&mut p, 2), 2);
    }

    /// Area and angle of a 3-4-5 right triangle, hand-computed.
    #[test]
    fn right_triangle() {
        let (a, b, c) = ([0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 3.0, 0.0]);
        assert_eq!(area(a, b, c).unwrap(), 6.0);
        let m = min_angle(a, b, c).unwrap();
        assert!((m - 0.6435011087932844).abs() < 1e-15, "{m}");
    }
}
