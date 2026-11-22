//! Selection set algebra and grow or shrink (spec node `GEO.SEL.ALGEBRA`).
//!
//! Pure, deterministic (D0), KNP-1 operations producing new [`IndexSet`]s or [`Selection`]s
//! without modifying their inputs (REQ-GEO.SEL.ALGEBRA-001):
//!
//! - **Range set operations** ([`union`], [`intersection`], [`difference`],
//!   [`sym_difference`], [`invert`] and their [`Selection`] wrappers): two-pointer merge walks
//!   over canonical half-open ranges, strictly $O(|a| + |b|)$ time and $O(\text{ranges})$
//!   memory (REQ-005). Mismatched target digests fail with [`SelError::ElementMismatch`];
//!   mismatched domains fail with [`SelError::SetDomainMismatch`] (`code == "domain_mismatch"`,
//!   REQ-002).
//! - **Triangle grow / shrink** ([`grow_triangles`], [`shrink_triangles`]): $n$ rings of
//!   [`Adjacency::Edge`] or [`Adjacency::Vertex`] adjacency using sorted CSR incidence tables
//!   (no hash maps) and breadth-first expansion in ascending triangle order (REQ-003).
//!   - Under [`Adjacency::Edge`], two non-degenerate triangles are neighbours when they share
//!     an undirected edge `(lo, hi)` (`lo < hi`); an undirected edge used by exactly one
//!     non-degenerate triangle is a mesh boundary edge.
//!   - Under [`Adjacency::Vertex`], two triangles are neighbours when they share a vertex; a
//!     vertex incident to any mesh boundary edge is a mesh boundary vertex.
//!   - `grow` adds every triangle within `rings` adjacency steps of a selected triangle.
//!   - `shrink` removes every selected triangle within `rings` steps of an unselected triangle
//!     or a mesh boundary (edge boundary for `Edge`, vertex boundary for `Vertex`).
//! - **Point grow / shrink** ([`grow_points`], [`shrink_points`]): radius queries through
//!   [`PointIndex::radius`] using the exact pinned squared-distance rule of `GEO.INDEX.KDTREE`
//!   (REQ-004). `grow` adds every point within `radius` of a selected point; `shrink` removes
//!   every selected point within `radius` of an unselected point.

use crate::model::{apply, Domain, IndexSet, IndexSetBuilder, SelError, Selection, SelectionBody};
use fid_geo_index::{PointIndex, PointIndexError, COORD_LIMIT};
use fid_geo_types::{ElementKind, GeometryElement, Mesh, PointBuffer};

/// Adjacency relation for mesh triangle grow and shrink (REQ-GEO.SEL.ALGEBRA-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Adjacency {
    /// Two triangles are neighbours if they share an undirected edge.
    Edge,
    /// Two triangles are neighbours if they share a vertex.
    Vertex,
}

/// Growth or shrinkage specification for [`grow`] and [`shrink`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GrowSpec {
    /// Mesh triangles: expand or erode by `rings` adjacency steps.
    Rings(u32, Adjacency),
    /// Points: expand or erode by Euclidean radius in metres (`>= 0`, finite, `<= COORD_LIMIT`).
    Radius(f64),
}

// ------------------------------------------------------------------ range helpers

fn push_coalesced(out: &mut Vec<(u64, u64)>, s: u64, e: u64) -> Result<(), SelError> {
    if s >= e {
        return Ok(());
    }
    if let Some(last) = out.last_mut() {
        if s <= last.1 {
            if e > last.1 {
                last.1 = e;
            }
            return Ok(());
        }
    }
    out.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
    out.push((s, e));
    Ok(())
}

fn check_same_domain(a: &IndexSet, b: &IndexSet) -> Result<Domain, SelError> {
    if a.domain() != b.domain() {
        return Err(SelError::SetDomainMismatch { left: a.domain(), right: b.domain() });
    }
    Ok(a.domain())
}

fn extract_pair<'a>(a: &'a Selection, b: &'a Selection) -> Result<(&'a IndexSet, &'a IndexSet), SelError> {
    if a.target_digest() != b.target_digest() {
        return Err(SelError::ElementMismatch { left: a.target_digest(), right: b.target_digest() });
    }
    let sa = match a.body() {
        SelectionBody::IndexSet(s) => s,
        SelectionBody::Query(_) => return Err(SelError::NotIndexSet),
    };
    let sb = match b.body() {
        SelectionBody::IndexSet(s) => s,
        SelectionBody::Query(_) => return Err(SelError::NotIndexSet),
    };
    check_same_domain(sa, sb)?;
    Ok((sa, sb))
}

// ------------------------------------------------------------------ IndexSet set algebra

/// Union $A \cup B$ in $O(|a| + |b|)$ time and $O(\text{ranges})$ memory.
pub fn union(a: &IndexSet, b: &IndexSet) -> Result<IndexSet, SelError> {
    let domain = check_same_domain(a, b)?;
    let (ra, rb) = (a.ranges(), b.ranges());
    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < ra.len() && j < rb.len() {
        let (s, e) = if ra[i].0 <= rb[j].0 {
            let r = ra[i];
            i += 1;
            r
        } else {
            let r = rb[j];
            j += 1;
            r
        };
        push_coalesced(&mut out, s, e)?;
    }
    while i < ra.len() {
        push_coalesced(&mut out, ra[i].0, ra[i].1)?;
        i += 1;
    }
    while j < rb.len() {
        push_coalesced(&mut out, rb[j].0, rb[j].1)?;
        j += 1;
    }
    Ok(IndexSet::from_canonical_ranges(domain, out))
}

/// Intersection $A \cap B$ in $O(|a| + |b|)$ time and $O(\text{ranges})$ memory.
pub fn intersection(a: &IndexSet, b: &IndexSet) -> Result<IndexSet, SelError> {
    let domain = check_same_domain(a, b)?;
    let (ra, rb) = (a.ranges(), b.ranges());
    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < ra.len() && j < rb.len() {
        let lo = if ra[i].0 >= rb[j].0 { ra[i].0 } else { rb[j].0 };
        let hi = if ra[i].1 <= rb[j].1 { ra[i].1 } else { rb[j].1 };
        if lo < hi {
            push_coalesced(&mut out, lo, hi)?;
        }
        if ra[i].1 < rb[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    Ok(IndexSet::from_canonical_ranges(domain, out))
}

/// Set difference $A \setminus B$ in $O(|a| + |b|)$ time and $O(\text{ranges})$ memory.
pub fn difference(a: &IndexSet, b: &IndexSet) -> Result<IndexSet, SelError> {
    let domain = check_same_domain(a, b)?;
    let (ra, rb) = (a.ranges(), b.ranges());
    let mut out = Vec::new();
    let mut j = 0usize;
    for &(mut cur_s, cur_e) in ra {
        while j < rb.len() && rb[j].1 <= cur_s {
            j += 1;
        }
        while j < rb.len() && rb[j].0 < cur_e {
            if rb[j].0 > cur_s {
                push_coalesced(&mut out, cur_s, rb[j].0)?;
            }
            if rb[j].1 >= cur_e {
                cur_s = cur_e;
                break;
            }
            cur_s = rb[j].1;
            j += 1;
        }
        if cur_s < cur_e {
            push_coalesced(&mut out, cur_s, cur_e)?;
        }
    }
    Ok(IndexSet::from_canonical_ranges(domain, out))
}

/// Symmetric difference $A \triangle B = (A \setminus B) \cup (B \setminus A)$ in $O(|a| + |b|)$.
pub fn sym_difference(a: &IndexSet, b: &IndexSet) -> Result<IndexSet, SelError> {
    check_same_domain(a, b)?;
    let left = difference(a, b)?;
    let right = difference(b, a)?;
    union(&left, &right)
}

/// Complement within `[0, domain_count)` in $O(|set|)$ time and $O(\text{ranges})$ memory.
/// Fails with [`SelError::IndexOutOfRange`] if `set` contains any index `>= domain_count`.
pub fn invert(set: &IndexSet, domain_count: u64) -> Result<IndexSet, SelError> {
    set.check_bound(domain_count)?;
    let mut out = Vec::new();
    let mut cur = 0u64;
    for &(s, e) in set.ranges() {
        if cur < s {
            push_coalesced(&mut out, cur, s)?;
        }
        cur = e;
    }
    if cur < domain_count {
        push_coalesced(&mut out, cur, domain_count)?;
    }
    Ok(IndexSet::from_canonical_ranges(set.domain(), out))
}

// ------------------------------------------------------------------ Selection set algebra

/// Union of two index-set selections on the same element and domain.
pub fn union_selections(a: &Selection, b: &Selection) -> Result<Selection, SelError> {
    let (sa, sb) = extract_pair(a, b)?;
    Ok(Selection::new(a.target(), a.target_digest(), SelectionBody::IndexSet(union(sa, sb)?)))
}

/// Intersection of two index-set selections on the same element and domain.
pub fn intersection_selections(a: &Selection, b: &Selection) -> Result<Selection, SelError> {
    let (sa, sb) = extract_pair(a, b)?;
    Ok(Selection::new(a.target(), a.target_digest(), SelectionBody::IndexSet(intersection(sa, sb)?)))
}

/// Difference `a \ b` of two index-set selections on the same element and domain.
pub fn difference_selections(a: &Selection, b: &Selection) -> Result<Selection, SelError> {
    let (sa, sb) = extract_pair(a, b)?;
    Ok(Selection::new(a.target(), a.target_digest(), SelectionBody::IndexSet(difference(sa, sb)?)))
}

/// Symmetric difference of two index-set selections on the same element and domain.
pub fn sym_difference_selections(a: &Selection, b: &Selection) -> Result<Selection, SelError> {
    let (sa, sb) = extract_pair(a, b)?;
    Ok(Selection::new(a.target(), a.target_digest(), SelectionBody::IndexSet(sym_difference(sa, sb)?)))
}

/// Complement of an index-set selection within `[0, domain_count)`.
pub fn invert_selection(sel: &Selection, domain_count: u64) -> Result<Selection, SelError> {
    let set = match sel.body() {
        SelectionBody::IndexSet(s) => s,
        SelectionBody::Query(_) => return Err(SelError::NotIndexSet),
    };
    Ok(Selection::new(sel.target(), sel.target_digest(), SelectionBody::IndexSet(invert(set, domain_count)?)))
}

// ------------------------------------------------------------------ mesh incidence tables

fn alloc_vec<T>(n: usize) -> Result<Vec<T>, SelError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| SelError::AllocationFailed)?;
    Ok(v)
}

fn alloc_filled<T: Clone>(n: usize, val: T) -> Result<Vec<T>, SelError> {
    let mut v = alloc_vec(n)?;
    v.resize(n, val);
    Ok(v)
}

/// Sorted edge incidence table for a mesh: each unique undirected edge `(lo, hi)` (`lo < hi`)
/// of non-degenerate triangles gets an edge ID `0..num_edges`, with a CSR slice of incident
/// triangles in ascending triangle order, `tri_edges[t]` giving the 0..3 edge IDs of triangle `t`,
/// and `boundary_edge[e]` true iff edge `e` is used by exactly one non-degenerate triangle.
struct EdgeTable {
    offsets: Vec<usize>,
    tris: Vec<usize>,
    tri_edges: Vec<[Option<usize>; 3]>,
    boundary_edge: Vec<bool>,
    boundary_vertices: Vec<u64>,
}

fn build_edge_table(mesh: &Mesh) -> Result<EdgeTable, SelError> {
    let n_tri = usize::try_from(mesh.triangle_count()).map_err(|_| SelError::AllocationFailed)?;
    let mut records: Vec<(u64, u64, usize, u8)> = alloc_vec(n_tri.saturating_mul(3))?;
    for (t, [a, b, c]) in mesh.triangles().enumerate() {
        if a == b || b == c || c == a {
            continue;
        }
        let edges = [(a, b), (b, c), (c, a)];
        for (slot, (u, v)) in edges.into_iter().enumerate() {
            let (lo, hi) = if u < v { (u, v) } else { (v, u) };
            records.push((lo, hi, t, slot as u8));
        }
    }
    records.sort_unstable();

    // Count unique edges and build CSR.
    let mut offsets: Vec<usize> = Vec::new();
    let mut tris: Vec<usize> = alloc_vec(records.len())?;
    let mut tri_edges: Vec<[Option<usize>; 3]> = alloc_filled(n_tri, [None; 3])?;
    let mut boundary_edge: Vec<bool> = Vec::new();
    let mut boundary_vertices: Vec<u64> = Vec::new();

    let mut i = 0usize;
    while i < records.len() {
        let (lo, hi, _, _) = records[i];
        let mut j = i + 1;
        while j < records.len() && records[j].0 == lo && records[j].1 == hi {
            j += 1;
        }
        let eid = boundary_edge.len();
        offsets.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
        offsets.push(tris.len());
        let is_boundary = (j - i) == 1;
        boundary_edge.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
        boundary_edge.push(is_boundary);
        if is_boundary {
            boundary_vertices.try_reserve(2).map_err(|_| SelError::AllocationFailed)?;
            boundary_vertices.push(lo);
            boundary_vertices.push(hi);
        }
        for rec in &records[i..j] {
            let t = rec.2;
            let slot = usize::from(rec.3);
            if let Some(te) = tri_edges.get_mut(t) {
                if let Some(s) = te.get_mut(slot) {
                    *s = Some(eid);
                }
            }
            // Deduplicate triangle within the same edge run (cannot happen for non-degenerate).
            if tris.last().copied() != Some(t) || tris.len() == offsets[eid] {
                tris.push(t);
            }
        }
        i = j;
    }
    offsets.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
    offsets.push(tris.len());
    boundary_vertices.sort_unstable();
    boundary_vertices.dedup();

    Ok(EdgeTable { offsets, tris, tri_edges, boundary_edge, boundary_vertices })
}

/// Sorted vertex incidence table: maps each vertex `v` (`0..n_vert`) to its incident triangles
/// in ascending triangle order, with no hash maps.
struct VertexTable {
    offsets: Vec<usize>,
    tris: Vec<usize>,
}

fn build_vertex_table(mesh: &Mesh) -> Result<VertexTable, SelError> {
    let n_vert = usize::try_from(mesh.vertex_count()).map_err(|_| SelError::AllocationFailed)?;
    let n_tri = usize::try_from(mesh.triangle_count()).map_err(|_| SelError::AllocationFailed)?;
    let mut pairs: Vec<(usize, usize)> = alloc_vec(n_tri.saturating_mul(3))?;
    for (t, [a, b, c]) in mesh.triangles().enumerate() {
        let va = usize::try_from(a).map_err(|_| SelError::AllocationFailed)?;
        let vb = usize::try_from(b).map_err(|_| SelError::AllocationFailed)?;
        let vc = usize::try_from(c).map_err(|_| SelError::AllocationFailed)?;
        pairs.push((va, t));
        if vb != va {
            pairs.push((vb, t));
        }
        if vc != va && vc != vb {
            pairs.push((vc, t));
        }
    }
    pairs.sort_unstable();

    let mut offsets = alloc_filled(n_vert.saturating_add(1), 0usize)?;
    for &(v, _) in &pairs {
        if let Some(slot) = offsets.get_mut(v + 1) {
            *slot += 1;
        }
    }
    for v in 0..n_vert {
        offsets[v + 1] += offsets[v];
    }
    let mut tris = alloc_vec(pairs.len())?;
    for &(_, t) in &pairs {
        tris.push(t);
    }
    Ok(VertexTable { offsets, tris })
}

fn index_set_from_mask(domain: Domain, mask: &[bool]) -> Result<IndexSet, SelError> {
    let count = u64::try_from(mask.len()).unwrap_or(u64::MAX);
    let mut builder = IndexSetBuilder::new(domain, count);
    for (i, &keep) in mask.iter().enumerate() {
        if keep {
            let idx = u64::try_from(i).map_err(|_| SelError::AllocationFailed)?;
            builder.push(idx)?;
        }
    }
    Ok(builder.finish())
}

// ------------------------------------------------------------------ triangle grow / shrink

/// Grows a triangle selection on `mesh` by `rings` steps of `adjacency` (REQ-GEO.SEL.ALGEBRA-003).
pub fn grow_triangles(mesh: &Mesh, set: &IndexSet, rings: u32, adjacency: Adjacency) -> Result<IndexSet, SelError> {
    if set.domain() != Domain::Triangles {
        return Err(SelError::DomainMismatch { domain: set.domain(), kind: ElementKind::Mesh });
    }
    let n_tri_u64 = mesh.triangle_count();
    set.check_bound(n_tri_u64)?;
    if rings == 0 || n_tri_u64 == 0 || set.is_empty() {
        return Ok(set.clone());
    }
    let n_tri = usize::try_from(n_tri_u64).map_err(|_| SelError::AllocationFailed)?;
    let mut selected = alloc_filled(n_tri, false)?;
    let mut frontier: Vec<usize> = Vec::new();
    for idx in set.iter() {
        let t = usize::try_from(idx).map_err(|_| SelError::AllocationFailed)?;
        if let Some(slot) = selected.get_mut(t) {
            if !*slot {
                *slot = true;
                frontier.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
                frontier.push(t);
            }
        }
    }

    match adjacency {
        Adjacency::Edge => {
            let et = build_edge_table(mesh)?;
            let mut edge_seen = alloc_filled(et.boundary_edge.len(), false)?;
            let mut next: Vec<usize> = Vec::new();
            for _ in 0..rings {
                if frontier.is_empty() {
                    break;
                }
                next.clear();
                for &t in &frontier {
                    if let Some(edges) = et.tri_edges.get(t) {
                        for eid in edges.iter().flatten().copied() {
                            if let Some(seen) = edge_seen.get_mut(eid) {
                                if *seen {
                                    continue;
                                }
                                *seen = true;
                            }
                            let start = et.offsets[eid];
                            let end = et.offsets[eid + 1];
                            for &nb in &et.tris[start..end] {
                                if let Some(sel) = selected.get_mut(nb) {
                                    if !*sel {
                                        *sel = true;
                                        next.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
                                        next.push(nb);
                                    }
                                }
                            }
                        }
                    }
                }
                next.sort_unstable();
                core::mem::swap(&mut frontier, &mut next);
            }
        }
        Adjacency::Vertex => {
            let vt = build_vertex_table(mesh)?;
            let n_vert = usize::try_from(mesh.vertex_count()).map_err(|_| SelError::AllocationFailed)?;
            let mut vert_seen = alloc_filled(n_vert, false)?;
            let mut next: Vec<usize> = Vec::new();
            for _ in 0..rings {
                if frontier.is_empty() {
                    break;
                }
                next.clear();
                for &t in &frontier {
                    let t_u64 = u64::try_from(t).map_err(|_| SelError::AllocationFailed)?;
                    if let Some(verts) = mesh.triangle(t_u64) {
                        for v_u64 in verts {
                            let v = usize::try_from(v_u64).map_err(|_| SelError::AllocationFailed)?;
                            if let Some(seen) = vert_seen.get_mut(v) {
                                if *seen {
                                    continue;
                                }
                                *seen = true;
                            }
                            let start = vt.offsets[v];
                            let end = vt.offsets[v + 1];
                            for &nb in &vt.tris[start..end] {
                                if let Some(sel) = selected.get_mut(nb) {
                                    if !*sel {
                                        *sel = true;
                                        next.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
                                        next.push(nb);
                                    }
                                }
                            }
                        }
                    }
                }
                next.sort_unstable();
                core::mem::swap(&mut frontier, &mut next);
            }
        }
    }

    index_set_from_mask(Domain::Triangles, &selected)
}

/// Shrinks a triangle selection on `mesh` by `rings` steps of `adjacency`, removing every
/// selected triangle within `rings` steps of an unselected triangle or a mesh boundary
/// (REQ-GEO.SEL.ALGEBRA-003).
pub fn shrink_triangles(mesh: &Mesh, set: &IndexSet, rings: u32, adjacency: Adjacency) -> Result<IndexSet, SelError> {
    if set.domain() != Domain::Triangles {
        return Err(SelError::DomainMismatch { domain: set.domain(), kind: ElementKind::Mesh });
    }
    let n_tri_u64 = mesh.triangle_count();
    set.check_bound(n_tri_u64)?;
    if rings == 0 || n_tri_u64 == 0 || set.is_empty() {
        return Ok(set.clone());
    }
    let n_tri = usize::try_from(n_tri_u64).map_err(|_| SelError::AllocationFailed)?;
    // `eroded[t]` is true when triangle `t` is unselected or has been removed by shrink.
    let mut eroded = alloc_filled(n_tri, true)?;
    for idx in set.iter() {
        let t = usize::try_from(idx).map_err(|_| SelError::AllocationFailed)?;
        if let Some(slot) = eroded.get_mut(t) {
            *slot = false;
        }
    }

    // Ring-0 frontier: all initially unselected triangles.
    let mut frontier: Vec<usize> = Vec::new();
    for (t, &is_unselected) in eroded.iter().enumerate() {
        if is_unselected {
            frontier.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
            frontier.push(t);
        }
    }

    let et = build_edge_table(mesh)?;
    match adjacency {
        Adjacency::Edge => {
            let mut edge_seen = alloc_filled(et.boundary_edge.len(), false)?;
            let mut next: Vec<usize> = Vec::new();
            for step in 0..rings {
                next.clear();
                // Step 0 also erodes any selected triangle incident to a mesh boundary edge.
                if step == 0 {
                    for (eid, &is_bnd) in et.boundary_edge.iter().enumerate() {
                        if !is_bnd {
                            continue;
                        }
                        if let Some(seen) = edge_seen.get_mut(eid) {
                            *seen = true;
                        }
                        let start = et.offsets[eid];
                        let end = et.offsets[eid + 1];
                        for &t in &et.tris[start..end] {
                            if let Some(er) = eroded.get_mut(t) {
                                if !*er {
                                    *er = true;
                                    next.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
                                    next.push(t);
                                }
                            }
                        }
                    }
                }
                for &t in &frontier {
                    if let Some(edges) = et.tri_edges.get(t) {
                        for eid in edges.iter().flatten().copied() {
                            if let Some(seen) = edge_seen.get_mut(eid) {
                                if *seen {
                                    continue;
                                }
                                *seen = true;
                            }
                            let start = et.offsets[eid];
                            let end = et.offsets[eid + 1];
                            for &nb in &et.tris[start..end] {
                                if let Some(er) = eroded.get_mut(nb) {
                                    if !*er {
                                        *er = true;
                                        next.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
                                        next.push(nb);
                                    }
                                }
                            }
                        }
                    }
                }
                if next.is_empty() {
                    break;
                }
                next.sort_unstable();
                core::mem::swap(&mut frontier, &mut next);
            }
        }
        Adjacency::Vertex => {
            let vt = build_vertex_table(mesh)?;
            let n_vert = usize::try_from(mesh.vertex_count()).map_err(|_| SelError::AllocationFailed)?;
            let mut vert_seen = alloc_filled(n_vert, false)?;
            let mut next: Vec<usize> = Vec::new();
            for step in 0..rings {
                next.clear();
                // Step 0 also erodes any selected triangle touching a mesh boundary vertex.
                if step == 0 {
                    for &v_u64 in &et.boundary_vertices {
                        let v = usize::try_from(v_u64).map_err(|_| SelError::AllocationFailed)?;
                        if let Some(seen) = vert_seen.get_mut(v) {
                            *seen = true;
                        }
                        let start = vt.offsets[v];
                        let end = vt.offsets[v + 1];
                        for &t in &vt.tris[start..end] {
                            if let Some(er) = eroded.get_mut(t) {
                                if !*er {
                                    *er = true;
                                    next.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
                                    next.push(t);
                                }
                            }
                        }
                    }
                }
                for &t in &frontier {
                    let t_u64 = u64::try_from(t).map_err(|_| SelError::AllocationFailed)?;
                    if let Some(verts) = mesh.triangle(t_u64) {
                        for v_u64 in verts {
                            let v = usize::try_from(v_u64).map_err(|_| SelError::AllocationFailed)?;
                            if let Some(seen) = vert_seen.get_mut(v) {
                                if *seen {
                                    continue;
                                }
                                *seen = true;
                            }
                            let start = vt.offsets[v];
                            let end = vt.offsets[v + 1];
                            for &nb in &vt.tris[start..end] {
                                if let Some(er) = eroded.get_mut(nb) {
                                    if !*er {
                                        *er = true;
                                        next.try_reserve(1).map_err(|_| SelError::AllocationFailed)?;
                                        next.push(nb);
                                    }
                                }
                            }
                        }
                    }
                }
                if next.is_empty() {
                    break;
                }
                next.sort_unstable();
                core::mem::swap(&mut frontier, &mut next);
            }
        }
    }

    let mut kept = alloc_vec(n_tri)?;
    for &er in &eroded {
        kept.push(!er);
    }
    index_set_from_mask(Domain::Triangles, &kept)
}

// ------------------------------------------------------------------ point grow / shrink

fn map_index_err(e: PointIndexError) -> SelError {
    match e {
        PointIndexError::BadRadius => SelError::BadRadius,
        PointIndexError::AllocationFailed { .. } => SelError::AllocationFailed,
        PointIndexError::NonFinitePoint { .. } | PointIndexError::NonFiniteQuery { .. } => {
            SelError::InvalidPointIndex { reason: "non-finite coordinate" }
        }
        PointIndexError::PointOutOfRange { .. } | PointIndexError::QueryOutOfRange { .. } => {
            SelError::InvalidPointIndex { reason: "coordinate exceeds COORD_LIMIT" }
        }
        PointIndexError::TooManyPoints { .. } => SelError::InvalidPointIndex { reason: "too many points" },
        PointIndexError::ColumnLength { .. } => SelError::InvalidPointIndex { reason: "column length mismatch" },
        PointIndexError::ZeroThreads | PointIndexError::BadK { .. } | PointIndexError::Inconsistent => {
            SelError::InvalidPointIndex { reason: "inconsistent point index" }
        }
    }
}

fn check_radius(radius: f64) -> Result<(), SelError> {
    if !radius.is_finite() || radius < 0.0 || radius > COORD_LIMIT {
        return Err(SelError::BadRadius);
    }
    Ok(())
}

/// Grows a point selection using `index` (`PointIndex::radius`), adding every point within
/// `radius` of a selected point with the exact `GEO.INDEX.KDTREE` distance comparison
/// (REQ-GEO.SEL.ALGEBRA-004).
pub fn grow_points(
    points: &PointBuffer,
    index: &PointIndex<'_>,
    set: &IndexSet,
    radius: f64,
) -> Result<IndexSet, SelError> {
    if set.domain() != Domain::Points {
        return Err(SelError::DomainMismatch { domain: set.domain(), kind: ElementKind::PointSet });
    }
    check_radius(radius)?;
    let n_u64 = points.len();
    if index.len() != n_u64 {
        return Err(SelError::InvalidPointIndex { reason: "point index length does not match buffer" });
    }
    set.check_bound(n_u64)?;
    if n_u64 == 0 || set.is_empty() {
        return Ok(set.clone());
    }
    let n = usize::try_from(n_u64).map_err(|_| SelError::AllocationFailed)?;
    let mut selected = alloc_filled(n, false)?;
    for p in set.iter() {
        let pu = usize::try_from(p).map_err(|_| SelError::AllocationFailed)?;
        if let Some(slot) = selected.get_mut(pu) {
            *slot = true;
        }
        let coord = points.point(p).ok_or(SelError::IndexOutOfRange { index: p, count: n_u64 })?;
        let hits = index.radius(coord, radius).map_err(map_index_err)?;
        for nb in hits {
            let nu = usize::try_from(nb.index).map_err(|_| SelError::AllocationFailed)?;
            if let Some(slot) = selected.get_mut(nu) {
                *slot = true;
            }
        }
    }
    index_set_from_mask(Domain::Points, &selected)
}

/// Shrinks a point selection using `index` (`PointIndex::radius`), removing every selected
/// point within `radius` of an unselected point with the exact `GEO.INDEX.KDTREE` distance
/// comparison (REQ-GEO.SEL.ALGEBRA-004).
pub fn shrink_points(
    points: &PointBuffer,
    index: &PointIndex<'_>,
    set: &IndexSet,
    radius: f64,
) -> Result<IndexSet, SelError> {
    if set.domain() != Domain::Points {
        return Err(SelError::DomainMismatch { domain: set.domain(), kind: ElementKind::PointSet });
    }
    check_radius(radius)?;
    let n_u64 = points.len();
    if index.len() != n_u64 {
        return Err(SelError::InvalidPointIndex { reason: "point index length does not match buffer" });
    }
    set.check_bound(n_u64)?;
    let sel_count = set.len();
    if n_u64 == 0 || sel_count == 0 || sel_count == n_u64 {
        return Ok(set.clone());
    }
    let n = usize::try_from(n_u64).map_err(|_| SelError::AllocationFailed)?;
    let mut is_selected = alloc_filled(n, false)?;
    for p in set.iter() {
        let pu = usize::try_from(p).map_err(|_| SelError::AllocationFailed)?;
        if let Some(slot) = is_selected.get_mut(pu) {
            *slot = true;
        }
    }
    let mut kept = is_selected.clone();

    // `dist2(p, q)` is symmetric in `GEO.INDEX.KDTREE`, so we can query from the smaller side.
    let unsel_count = n_u64 - sel_count;
    if unsel_count <= sel_count {
        for (qu, &sel) in is_selected.iter().enumerate() {
            if sel {
                continue;
            }
            let q = u64::try_from(qu).map_err(|_| SelError::AllocationFailed)?;
            let coord = points.point(q).ok_or(SelError::IndexOutOfRange { index: q, count: n_u64 })?;
            let hits = index.radius(coord, radius).map_err(map_index_err)?;
            for nb in hits {
                let nu = usize::try_from(nb.index).map_err(|_| SelError::AllocationFailed)?;
                if let Some(slot) = kept.get_mut(nu) {
                    *slot = false;
                }
            }
        }
    } else {
        for p in set.iter() {
            let coord = points.point(p).ok_or(SelError::IndexOutOfRange { index: p, count: n_u64 })?;
            let hits = index.radius(coord, radius).map_err(map_index_err)?;
            let mut near_unselected = false;
            for nb in hits {
                let nu = usize::try_from(nb.index).map_err(|_| SelError::AllocationFailed)?;
                if is_selected.get(nu).copied() == Some(false) {
                    near_unselected = true;
                    break;
                }
            }
            if near_unselected {
                let pu = usize::try_from(p).map_err(|_| SelError::AllocationFailed)?;
                if let Some(slot) = kept.get_mut(pu) {
                    *slot = false;
                }
            }
        }
    }

    index_set_from_mask(Domain::Points, &kept)
}

// ------------------------------------------------------------------ unified element dispatch

fn point_buffer_of(element: &GeometryElement) -> Option<&PointBuffer> {
    match element {
        GeometryElement::PointSet(p) => Some(p.buffer()),
        GeometryElement::PointCloud(p) => Some(p.buffer()),
        GeometryElement::Mesh(m) => Some(m.vertices()),
        GeometryElement::CadBody(_) => None,
    }
}

/// Grows an [`IndexSet`] on `element` according to `spec`.
pub fn grow(element: &GeometryElement, set: &IndexSet, spec: GrowSpec) -> Result<IndexSet, SelError> {
    match spec {
        GrowSpec::Rings(rings, adjacency) => match element {
            GeometryElement::Mesh(mesh) => grow_triangles(mesh, set, rings, adjacency),
            other => Err(SelError::DomainMismatch { domain: set.domain(), kind: other.kind() }),
        },
        GrowSpec::Radius(radius) => {
            if set.domain() != Domain::Points {
                return Err(SelError::DomainMismatch { domain: set.domain(), kind: element.kind() });
            }
            check_radius(radius)?;
            let buf = point_buffer_of(element)
                .ok_or_else(|| SelError::DomainMismatch { domain: set.domain(), kind: element.kind() })?;
            set.check_bound(buf.len())?;
            if buf.is_empty() || set.is_empty() {
                return Ok(set.clone());
            }
            let index = PointIndex::build(buf, 1).map_err(map_index_err)?;
            grow_points(buf, &index, set, radius)
        }
    }
}

/// Shrinks an [`IndexSet`] on `element` according to `spec`.
pub fn shrink(element: &GeometryElement, set: &IndexSet, spec: GrowSpec) -> Result<IndexSet, SelError> {
    match spec {
        GrowSpec::Rings(rings, adjacency) => match element {
            GeometryElement::Mesh(mesh) => shrink_triangles(mesh, set, rings, adjacency),
            other => Err(SelError::DomainMismatch { domain: set.domain(), kind: other.kind() }),
        },
        GrowSpec::Radius(radius) => {
            if set.domain() != Domain::Points {
                return Err(SelError::DomainMismatch { domain: set.domain(), kind: element.kind() });
            }
            check_radius(radius)?;
            let buf = point_buffer_of(element)
                .ok_or_else(|| SelError::DomainMismatch { domain: set.domain(), kind: element.kind() })?;
            set.check_bound(buf.len())?;
            if buf.is_empty() || set.is_empty() || set.len() == buf.len() {
                return Ok(set.clone());
            }
            let index = PointIndex::build(buf, 1).map_err(map_index_err)?;
            shrink_points(buf, &index, set, radius)
        }
    }
}

/// Validates `sel` against `element` (stale check via [`apply`]) and returns a new grown
/// [`Selection`] bound to the same target element.
pub fn grow_selection(element: &GeometryElement, sel: &Selection, spec: GrowSpec) -> Result<Selection, SelError> {
    let set = apply(sel, element)?;
    let grown = grow(element, set, spec)?;
    Ok(Selection::new(sel.target(), sel.target_digest(), SelectionBody::IndexSet(grown)))
}

/// Validates `sel` against `element` (stale check via [`apply`]) and returns a new shrunk
/// [`Selection`] bound to the same target element.
pub fn shrink_selection(element: &GeometryElement, sel: &Selection, spec: GrowSpec) -> Result<Selection, SelError> {
    let set = apply(sel, element)?;
    let shrunk = shrink(element, set, spec)?;
    Ok(Selection::new(sel.target(), sel.target_digest(), SelectionBody::IndexSet(shrunk)))
}
