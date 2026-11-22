//! Deterministic Delaunay triangulations, convex hull, and Voronoi duals (`KRN.MZ.VOR`).
//!
//! All data structures use flat `Vec`s indexed by `u32` (no `HashMap`/`HashSet`), exact-sign
//! predicates from [`super::pred`], `fid_num::f` for square roots/hypot/total order, bounded
//! loops with zero recursion, zero `unsafe`, and zero panics (KNP-1).

use crate::contract::FitError;
use crate::vor::pred::{incircle, orient2d};
use fid_num::f;

const NONE: u32 = u32::MAX;

/// A vertex of a Voronoi diagram (circumcenter of a Delaunay triangle).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoronoiVertex {
    pub x: f64,
    pub y: f64,
    /// Original point indices of the three defining sites.
    pub sites: [usize; 3],
}

/// Geometry of a Voronoi edge (finite segment or unbounded ray).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VoronoiEdgeKind {
    /// Finite segment connecting `vertices[start]` and `vertices[end]`.
    Segment { start: usize, end: usize },
    /// Unbounded ray starting at `vertices[start]` in unit direction `dir`.
    Ray { start: usize, dir: [f64; 2] },
}

/// An edge of a Voronoi diagram bisecting two input sites.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoronoiEdge {
    /// Original point indices of the two sites bisected by this edge.
    pub sites: [usize; 2],
    pub kind: VoronoiEdgeKind,
}

/// Dual Voronoi diagram (nearest-point or farthest-point).
#[derive(Debug, Clone, PartialEq)]
pub struct VoronoiDiagram {
    pub vertices: Vec<VoronoiVertex>,
    pub edges: Vec<VoronoiEdge>,
}

/// One triangle in an indexed adjacency mesh (`adj[k]` lies across edge `(v[k], v[(k+1)%3])`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tri {
    pub v: [u32; 3],
    pub adj: [u32; 3],
    pub alive: bool,
}

impl Tri {
    fn new(a: u32, b: u32, c: u32) -> Self {
        Tri { v: [a, b, c], adj: [NONE, NONE, NONE], alive: true }
    }
}

/// Andrew's monotone chain strict convex hull on points already sorted lexicographically by
/// `(x, y)` using exact `orient2d`. Returns CCW indices into `pts`.
pub(crate) fn monotone_hull(pts: &[[f64; 2]]) -> Result<Vec<u32>, FitError> {
    let n = pts.len();
    if n < 3 {
        return Err(FitError::TooFewPoints { n, need: 3 });
    }
    let mut lower: Vec<u32> = Vec::new();
    for (i, &p) in pts.iter().enumerate() {
        while lower.len() >= 2 {
            let a = pts[lower[lower.len() - 2] as usize];
            let b = pts[lower[lower.len() - 1] as usize];
            if orient2d(a, b, p) <= 0 {
                lower.pop();
            } else {
                break;
            }
        }
        lower.push(i as u32);
    }
    let mut upper: Vec<u32> = Vec::new();
    for (i, &p) in pts.iter().enumerate().rev() {
        while upper.len() >= 2 {
            let a = pts[upper[upper.len() - 2] as usize];
            let b = pts[upper[upper.len() - 1] as usize];
            if orient2d(a, b, p) <= 0 {
                upper.pop();
            } else {
                break;
            }
        }
        upper.push(i as u32);
    }
    lower.pop();
    upper.pop();
    lower.extend_from_slice(&upper);
    if lower.len() < 3 {
        return Err(FitError::Collinear { n, rank: 1, cond: f64::INFINITY });
    }
    Ok(lower)
}

/// Deterministic incremental Bowyer-Watson Delaunay triangulation on deduplicated,
/// lexicographically sorted points `pts` scaled near `[-2, 2]^2`.
pub(crate) fn delaunay_nearest(pts: &[[f64; 2]]) -> Result<Vec<Tri>, FitError> {
    let n = pts.len();
    if n < 3 {
        return Err(FitError::TooFewPoints { n, need: 3 });
    }
    let mut all_pts: Vec<[f64; 2]> = Vec::with_capacity(n + 3);
    all_pts.extend_from_slice(pts);

    // Compute a safe super-triangle enclosing all points (exact integers in f64).
    let mut max_abs = 1.0_f64;
    for &[x, y] in pts {
        max_abs = f::total_max(max_abs, x.abs()).map_err(FitError::Num)?;
        max_abs = f::total_max(max_abs, y.abs()).map_err(FitError::Num)?;
    }
    // Round up to a clean power of two * 1e6 so super-triangle is huge yet well within exact f64 range.
    let r = max_abs * 1_048_576.0;
    let s0 = n as u32;
    let s1 = (n + 1) as u32;
    let s2 = (n + 2) as u32;
    all_pts.push([-2.0 * r, -r]);
    all_pts.push([2.0 * r, -r]);
    all_pts.push([0.0, 3.0 * r]);

    let mut tris: Vec<Tri> = Vec::with_capacity(2 * n + 8);
    tris.push(Tri::new(s0, s1, s2));

    let mut stamp: Vec<u32> = vec![0; 2 * n + 8];
    let mut cur_stamp: u32 = 0;
    let mut cavity: Vec<u32> = Vec::with_capacity(64);
    let mut queue: Vec<u32> = Vec::with_capacity(64);
    let mut boundary: Vec<(u32, u32, u32)> = Vec::with_capacity(64);
    let mut ring: Vec<(u32, u32, u32)> = Vec::with_capacity(64);
    let mut new_ids: Vec<u32> = Vec::with_capacity(64);

    let mut last_tri: u32 = 0;

    for p_idx in 0..n {
        let p = all_pts[p_idx];
        let start = locate_triangle(&tris, &all_pts, last_tri, p)?;

        cur_stamp = cur_stamp.wrapping_add(1);
        if cur_stamp == 0 {
            stamp.fill(0);
            cur_stamp = 1;
        }
        if stamp.len() < tris.len() {
            stamp.resize(tris.len(), 0);
        }

        cavity.clear();
        queue.clear();

        stamp[start as usize] = cur_stamp;
        queue.push(start);

        // If `p` lies exactly on an edge of `start`, also seed the adjacent triangle across that edge
        // so the cavity boundary stays strictly star-shaped with no zero-area triangles.
        let st = tris[start as usize];
        for k in 0..3 {
            let a = all_pts[st.v[k] as usize];
            let b = all_pts[st.v[(k + 1) % 3] as usize];
            if orient2d(a, b, p) == 0 {
                let nb = st.adj[k];
                if nb != NONE && tris[nb as usize].alive && stamp[nb as usize] != cur_stamp {
                    stamp[nb as usize] = cur_stamp;
                    queue.push(nb);
                }
            }
        }

        let mut head = 0usize;
        while head < queue.len() {
            let t_id = queue[head];
            head += 1;
            cavity.push(t_id);
            let t = tris[t_id as usize];
            for &nb in &t.adj {
                if nb == NONE {
                    continue;
                }
                let nb_idx = nb as usize;
                if !tris[nb_idx].alive || stamp[nb_idx] == cur_stamp {
                    continue;
                }
                let nt = tris[nb_idx];
                let a = all_pts[nt.v[0] as usize];
                let b = all_pts[nt.v[1] as usize];
                let c = all_pts[nt.v[2] as usize];
                if incircle(a, b, c, p) > 0 {
                    stamp[nb_idx] = cur_stamp;
                    queue.push(nb);
                }
            }
        }

        // Collect boundary edges (u, v, outside_neighbor) of the cavity.
        boundary.clear();
        for &t_id in &cavity {
            tris[t_id as usize].alive = false;
        }
        for &t_id in &cavity {
            let t = tris[t_id as usize];
            for k in 0..3 {
                let nb = t.adj[k];
                let outside = nb == NONE || stamp[nb as usize] != cur_stamp;
                if outside {
                    boundary.push((t.v[k], t.v[(k + 1) % 3], nb));
                }
            }
        }

        // Order boundary edges into a CCW ring: edge[i].1 == edge[i+1].0.
        ring.clear();
        if !boundary.is_empty() {
            ring.push(boundary[0]);
            let b_len = boundary.len();
            let mut used = vec![false; b_len];
            used[0] = true;
            for _ in 1..b_len {
                let target = ring[ring.len() - 1].1;
                let mut found = None;
                for j in 1..b_len {
                    if !used[j] && boundary[j].0 == target {
                        found = Some(j);
                        break;
                    }
                }
                if let Some(j) = found {
                    used[j] = true;
                    ring.push(boundary[j]);
                } else {
                    break;
                }
            }
        }

        // Fan-triangulate the cavity around `p_idx`.
        let k_ring = ring.len();
        new_ids.clear();
        for &(u, v, out_nb) in &ring {
            let tid = tris.len() as u32;
            let mut nt = Tri::new(u, v, p_idx as u32);
            nt.adj[0] = out_nb;
            tris.push(nt);
            new_ids.push(tid);
        }
        if stamp.len() < tris.len() {
            stamp.resize(tris.len(), 0);
        }

        for i in 0..k_ring {
            let tid = new_ids[i];
            let next_tid = new_ids[(i + 1) % k_ring];
            let prev_tid = new_ids[(i + k_ring - 1) % k_ring];
            tris[tid as usize].adj[1] = next_tid;
            tris[tid as usize].adj[2] = prev_tid;

            let (u, v, out_nb) = ring[i];
            if out_nb != NONE {
                replace_neighbor(&mut tris[out_nb as usize], v, u, tid);
            }
        }

        if let Some(&first_new) = new_ids.first() {
            last_tri = first_new;
        }
    }

    // Strip all triangles touching any super-triangle vertex (`>= n`).
    let n_u32 = n as u32;
    for t in &mut tris {
        if t.alive && (t.v[0] >= n_u32 || t.v[1] >= n_u32 || t.v[2] >= n_u32) {
            t.alive = false;
        }
    }
    // Clear adjacency links to dead triangles.
    let alive_flags: Vec<bool> = tris.iter().map(|t| t.alive).collect();
    for t in &mut tris {
        if !t.alive {
            continue;
        }
        for nb in &mut t.adj {
            if *nb != NONE && !alive_flags[*nb as usize] {
                *nb = NONE;
            }
        }
    }

    // Verify/legalize all surviving interior edges for nearest-point Delaunay (`incircle <= 0`).
    legalize_mesh(&mut tris, pts, true);

    Ok(compact_mesh(&tris))
}

fn locate_triangle(tris: &[Tri], pts: &[[f64; 2]], start: u32, p: [f64; 2]) -> Result<u32, FitError> {
    let mut cur = start;
    if cur as usize >= tris.len() || !tris[cur as usize].alive {
        cur = tris.iter().position(|t| t.alive).map_or(0, |i| i as u32);
    }

    let max_steps = tris.len().saturating_add(1);
    for _ in 0..max_steps {
        let t = tris[cur as usize];
        let mut stepped = false;
        for k in 0..3 {
            let a = pts[t.v[k] as usize];
            let b = pts[t.v[(k + 1) % 3] as usize];
            if orient2d(a, b, p) < 0 {
                let nb = t.adj[k];
                if nb != NONE && tris[nb as usize].alive {
                    cur = nb;
                    stepped = true;
                    break;
                }
            }
        }
        if !stepped {
            // Confirm `cur` contains `p` (all 3 orientations >= 0).
            let a = pts[t.v[0] as usize];
            let b = pts[t.v[1] as usize];
            let c = pts[t.v[2] as usize];
            if orient2d(a, b, p) >= 0 && orient2d(b, c, p) >= 0 && orient2d(c, a, p) >= 0 {
                return Ok(cur);
            }
            break;
        }
    }

    // Deterministic fallback scan (bounded by `tris.len()`).
    for (idx, t) in tris.iter().enumerate() {
        if !t.alive {
            continue;
        }
        let a = pts[t.v[0] as usize];
        let b = pts[t.v[1] as usize];
        let c = pts[t.v[2] as usize];
        if orient2d(a, b, p) >= 0 && orient2d(b, c, p) >= 0 && orient2d(c, a, p) >= 0 {
            return Ok(idx as u32);
        }
    }
    Err(FitError::Collinear { n: pts.len().saturating_sub(3), rank: 1, cond: f64::INFINITY })
}

/// Farthest-point Delaunay triangulation of a strictly convex CCW polygon `hull[0..h]`.
///
/// Initialises with an iterative balanced-chord triangulation (depth `ceil(log2 h)`) and then
/// runs Lawson edge flips until every interior diagonal satisfies `incircle(a, b, c, d) >= 0`.
pub(crate) fn delaunay_farthest(pts: &[[f64; 2]], hull: &[u32]) -> Result<Vec<Tri>, FitError> {
    let h = hull.len();
    if h < 3 {
        return Err(FitError::Collinear { n: pts.len(), rank: 1, cond: f64::INFINITY });
    }

    let mut tris: Vec<Tri> = Vec::with_capacity(h - 2);
    // Explicit stack items: (lo, hi, parent_tri, parent_edge)
    let mut stack: Vec<(usize, usize, u32, usize)> = Vec::with_capacity(h);
    stack.push((0, h - 1, NONE, 0));

    while let Some((lo, hi, parent_tri, parent_edge)) = stack.pop() {
        if hi <= lo + 1 {
            continue;
        }
        let mid = lo + (hi - lo) / 2;
        let tid = tris.len() as u32;
        let mut t = Tri::new(hull[lo], hull[mid], hull[hi]);
        if parent_tri != NONE {
            // Edge 2 is (hull[hi], hull[lo]), shared with `parent_tri`'s `parent_edge`.
            t.adj[2] = parent_tri;
            tris[parent_tri as usize].adj[parent_edge] = tid;
        }
        tris.push(t);
        if mid > lo + 1 {
            // Edge 0 is (hull[lo], hull[mid]).
            stack.push((lo, mid, tid, 0));
        }
        if hi > mid + 1 {
            // Edge 1 is (hull[mid], hull[hi]).
            stack.push((mid, hi, tid, 1));
        }
    }

    legalize_mesh(&mut tris, pts, false);
    Ok(tris)
}

/// Runs a bounded Lawson flip pass over all interior edges of `tris`.
/// - `nearest == true`: legal iff `incircle(a, b, c, d) <= 0` (flips when `> 0`).
/// - `nearest == false`: legal iff `incircle(a, b, c, d) >= 0` (flips when `< 0`).
fn legalize_mesh(tris: &mut [Tri], pts: &[[f64; 2]], nearest: bool) {
    let m = tris.len();
    if m < 2 {
        return;
    }
    let mut work: Vec<(u32, usize)> = Vec::with_capacity(m * 3);
    for (ti, t) in tris.iter().enumerate() {
        if !t.alive {
            continue;
        }
        for (k, &nb) in t.adj.iter().enumerate() {
            if nb != NONE && (ti as u32) < nb {
                work.push((ti as u32, k));
            }
        }
    }

    let max_flips = m.saturating_mul(m).saturating_add(m);
    let mut flips = 0usize;

    while let Some((t1_id, e1)) = work.pop() {
        if flips >= max_flips {
            break;
        }
        let t1 = tris[t1_id as usize];
        if !t1.alive {
            continue;
        }
        let t2_id = t1.adj[e1];
        if t2_id == NONE {
            continue;
        }
        let t2 = tris[t2_id as usize];
        if !t2.alive {
            continue;
        }

        // Edge in t1: a -> b, opposite vertex c.
        let a = t1.v[e1];
        let b = t1.v[(e1 + 1) % 3];
        let c = t1.v[(e1 + 2) % 3];

        // Find matching edge b -> a in t2, opposite vertex d.
        let Some(e2) = find_edge(&t2, b, a) else {
            continue;
        };
        let d = t2.v[(e2 + 2) % 3];

        let ic = incircle(pts[a as usize], pts[b as usize], pts[c as usize], pts[d as usize]);
        let illegal = if nearest { ic > 0 } else { ic < 0 };
        if !illegal {
            continue;
        }
        // Only flip strictly convex quads (c, a, d, b) so orientation stays CCW.
        if orient2d(pts[c as usize], pts[d as usize], pts[b as usize]) <= 0
            || orient2d(pts[d as usize], pts[c as usize], pts[a as usize]) <= 0
        {
            continue;
        }

        // Outer neighbors of the quad:
        // t1 edges: e1=(a,b), (e1+1)%3=(b,c) -> nb_bc, (e1+2)%3=(c,a) -> nb_ca
        // t2 edges: e2=(b,a), (e2+1)%3=(a,d) -> nb_ad, (e2+2)%3=(d,b) -> nb_db
        let nb_bc = t1.adj[(e1 + 1) % 3];
        let nb_ca = t1.adj[(e1 + 2) % 3];
        let nb_ad = t2.adj[(e2 + 1) % 3];
        let nb_db = t2.adj[(e2 + 2) % 3];

        // New t1: (c, d, b) with edges (c,d)->t2, (d,b)->nb_db, (b,c)->nb_bc
        // New t2: (d, c, a) with edges (d,c)->t1, (c,a)->nb_ca, (a,d)->nb_ad
        tris[t1_id as usize] = Tri { v: [c, d, b], adj: [t2_id, nb_db, nb_bc], alive: true };
        tris[t2_id as usize] = Tri { v: [d, c, a], adj: [t1_id, nb_ca, nb_ad], alive: true };

        if nb_db != NONE {
            replace_neighbor(&mut tris[nb_db as usize], b, d, t1_id);
        }
        if nb_ca != NONE {
            replace_neighbor(&mut tris[nb_ca as usize], a, c, t2_id);
        }

        // Re-check the 4 outer edges of the flipped quad.
        if nb_db != NONE {
            work.push((t1_id, 1));
        }
        if nb_bc != NONE {
            work.push((t1_id, 2));
        }
        if nb_ca != NONE {
            work.push((t2_id, 1));
        }
        if nb_ad != NONE {
            work.push((t2_id, 2));
        }
        flips += 1;
    }
}

fn find_edge(t: &Tri, u: u32, v: u32) -> Option<usize> {
    (0..3).find(|&k| t.v[k] == u && t.v[(k + 1) % 3] == v)
}

fn replace_neighbor(t: &mut Tri, u: u32, v: u32, new_nb: u32) {
    if let Some(k) = find_edge(t, u, v) {
        t.adj[k] = new_nb;
    }
}

/// Compacts alive triangles into a dense vector `0..k`, remapping `adj` indices.
fn compact_mesh(tris: &[Tri]) -> Vec<Tri> {
    let mut remap = vec![NONE; tris.len()];
    let mut out = Vec::new();
    for (i, t) in tris.iter().enumerate() {
        if t.alive {
            remap[i] = out.len() as u32;
            out.push(*t);
        }
    }
    for t in &mut out {
        for nb in &mut t.adj {
            if *nb != NONE {
                *nb = remap[*nb as usize];
            }
        }
    }
    out
}

/// Computes the circumcenter of a non-collinear triangle `(a, b, c)`.
pub(crate) fn circumcenter(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 2]> {
    let bx = b[0] - a[0];
    let by = b[1] - a[1];
    let cx = c[0] - a[0];
    let cy = c[1] - a[1];
    let d = 2.0 * (bx * cy - by * cx);
    if d == 0.0 || !d.is_finite() {
        return None;
    }
    let b2 = bx * bx + by * by;
    let c2 = cx * cx + cy * cy;
    let ux = (cy * b2 - by * c2) / d;
    let uy = (bx * c2 - cx * b2) / d;
    let x = a[0] + ux;
    let y = a[1] + uy;
    if x.is_finite() && y.is_finite() {
        Some([x, y])
    } else {
        None
    }
}

/// Builds the dual Voronoi diagram from a compacted Delaunay mesh (`nearest` controls ray direction:
/// outward normal for nearest-point Voronoi, inward normal for farthest-point Voronoi).
pub(crate) fn build_voronoi_dual(
    tris: &[Tri],
    pts: &[[f64; 2]],
    orig_indices: &[usize],
    nearest: bool,
) -> Result<VoronoiDiagram, FitError> {
    let mut vertices: Vec<VoronoiVertex> = Vec::with_capacity(tris.len());
    let mut tri_to_vtx: Vec<usize> = vec![usize::MAX; tris.len()];

    for (ti, t) in tris.iter().enumerate() {
        let a = pts[t.v[0] as usize];
        let b = pts[t.v[1] as usize];
        let c = pts[t.v[2] as usize];
        if let Some([x, y]) = circumcenter(a, b, c) {
            tri_to_vtx[ti] = vertices.len();
            vertices.push(VoronoiVertex {
                x,
                y,
                sites: [orig_indices[t.v[0] as usize], orig_indices[t.v[1] as usize], orig_indices[t.v[2] as usize]],
            });
        }
    }

    let mut edges: Vec<VoronoiEdge> = Vec::with_capacity(tris.len().saturating_mul(2));
    for (ti, t) in tris.iter().enumerate() {
        let v_start = tri_to_vtx[ti];
        if v_start == usize::MAX {
            continue;
        }
        for k in 0..3 {
            let u = t.v[k] as usize;
            let v = t.v[(k + 1) % 3] as usize;
            let sites = [orig_indices[u], orig_indices[v]];
            let nb = t.adj[k];
            if nb != NONE {
                let nb_idx = nb as usize;
                if ti < nb_idx {
                    let v_end = tri_to_vtx[nb_idx];
                    if v_end != usize::MAX {
                        edges
                            .push(VoronoiEdge { sites, kind: VoronoiEdgeKind::Segment { start: v_start, end: v_end } });
                    }
                }
            } else {
                // Boundary hull edge (u -> v, CCW):
                // Nearest Voronoi ray points along outward normal (dy, -dx).
                // Farthest Voronoi ray points along inward normal (-dy, dx).
                let dx = pts[v][0] - pts[u][0];
                let dy = pts[v][1] - pts[u][1];
                let (nx, ny) = if nearest { (dy, -dx) } else { (-dy, dx) };
                let len = f::hypot(nx, ny).map_err(FitError::Num)?;
                if len > 0.0 && len.is_finite() {
                    edges.push(VoronoiEdge {
                        sites,
                        kind: VoronoiEdgeKind::Ray { start: v_start, dir: [nx / len, ny / len] },
                    });
                }
            }
        }
    }

    Ok(VoronoiDiagram { vertices, edges })
}
