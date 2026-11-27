//! Local tangent-plane Delaunay fans. Spec node `SCAN.POLY.SURF.FAN`.
//!
//! [`fans`] gives every vertex of a [`VertexSet`] its fan: the triangles around it in the 2D
//! Delaunay triangulation of its own neighbourhood, projected onto its tangent plane, keeping
//! only triangles whose edges are all shorter than the maximum gap `g`. Each fan depends only
//! on the vertex set and `g`, never on other fans, the vertex-set order or the thread count
//! (REQ-SCAN.POLY.SURF.FAN-001, -004). Every value-affecting choice is pinned below; the
//! independent reference `fixtures/scan/poly/make_fan_fixtures.py` repeats the floating-point
//! steps and evaluates every predicate in exact integer arithmetic.
//!
//! # Vertex identity
//!
//! A vertex is identified by its `source` (the index of its input point), which must be
//! unique in the set. Neighbour ties, the symbolic perturbation and the output all use the
//! source, so shuffling `vs.vertices` permutes the output and changes no fan (AC-4).
//!
//! # Input checks (first failure wins)
//!
//! 1. `g` finite with [`MIN_GAP`] = 2^-100 m `<= g <=` [`MAX_GAP`] = 2^100 m (`g`);
//! 2. `1 <= threads <= MAX_THREADS` (`threads`);
//! 3. every position finite with every coordinate at most `COORD_LIMIT` = 2^500 in magnitude
//!    (`vs.position`);
//! 4. every normal a unit normal by GEO.TYPES `is_unit_normal` (`vs.normal`);
//! 5. no two vertices share a source (`vs.source`).
//!
//! Each check covers the whole set before the next starts, so which check fails does not
//! depend on the vertex order.
//!
//! Each is [`PolyError::InvalidParameter`] (`scan.poly.invalid_parameter`).
//!
//! # One fan (centre `c`, position `p`, normal `n`)
//!
//! 1. Neighbourhood: every other vertex `q` with `dist2(q, p) <= g*g` (the GEO.INDEX.KDTREE
//!    radius query, `dist2 = (dx*dx + dy*dy) + dz*dz`) and `(n0*m0 + n1*m1) + n2*m2 >= 0.5`
//!    for its normal `m` (within 60 degrees), ordered by (`dist2` in total order, source); the
//!    first [`MAX_NEIGHBOURS`] = 32 are kept.
//! 2. Basis: `a` = the axis with the smallest `|n_a|` (lowest axis on ties),
//!    `t = n x e_a`, `e1 = t / sqrt((t0*t0 + t1*t1) + t2*t2)` per component, `e2 = n x e1`,
//!    with `x` the plain cross product `(a1*b2 - a2*b1, a2*b0 - a0*b2, a0*b1 - a1*b0)`. So
//!    `(e1, e2, n)` is right-handed and counter-clockwise in the plane is counter-clockwise
//!    about `n`.
//! 3. Projection: `d = q - p` per axis, `u = (d0*e1_0 + d1*e1_1) + d2*e1_2`, `v` likewise with
//!    `e2`; the centre is `(0, 0)`. A coordinate with `|u| <` [`FLUSH`] = 2^-200 m becomes
//!    `+0`, so no exact predicate meets an underflowing product (with `g <= 2^100` none
//!    overflows either) and the predicates are exact on the projected points.
//! 4. Delaunay: the triangles `(c, j, k)` over neighbours `j != k` with `orient2d(c, j, k) > 0`
//!    and no other point of the neighbourhood inside their circumcircle by the perturbed
//!    in-circle test below. This is the unique Delaunay triangulation of the perturbed
//!    neighbourhood, so it does not depend on the order the points are visited in.
//! 5. Edges: a triangle is kept when its three edges are short: `dist2(a, b) <
//!    (g*g) * EDGE_MARGIN` on the 3D positions, [`EDGE_MARGIN`] = 1 - 2^-46. The margin
//!    covers the rounding of `dist2` and `g*g` (each below 2^-49 relative), so every kept edge
//!    is shorter than `g` in exact arithmetic (REQ-SCAN.POLY.SURF.FAN-003).
//! 6. Order: by the angle from `+e1` in `[0, 2 pi)`: first the half plane `v > 0` or
//!    (`v = 0`, `u > 0`), then by the exact sign of the cross product. `neighbours` are the
//!    vertices of the kept triangles in this order, `triangles` are `[c, j, k]` (counter-
//!    clockwise about `n`) in the order of `j`.
//! 7. Status ([`FanStatus`], first match wins): [`FanStatus::Hidden`] when a neighbour with
//!    a smaller source projects exactly onto `(0, 0)` (`by` = the smallest such source; the
//!    perturbation then makes that twin the Delaunay vertex and the centre has no triangle);
//!    [`FanStatus::NoNeighbours`] with fewer than 2 neighbours; [`FanStatus::Degenerate`]
//!    when every `orient2d(c, j, k)` is 0 (the neighbourhood is collinear through the centre
//!    in projection); [`FanStatus::Ok`] when a triangle is kept; else
//!    [`FanStatus::AllEdgesLong`] (the centre has Delaunay triangles, each with an edge of at
//!    least `g`). Only an `Ok` fan has triangles. A vertex whose status is not `Ok` is not
//!    silently dropped: its fan says why it has no triangles, for STITCH and QUAL to report.
//!
//! # Predicates (REQ-SCAN.POLY.SURF.FAN-002)
//!
//! `orient2d` and `incircle` are the exact-sign adaptive predicates of `fid_fit::vor`
//! (Shewchuk expansion arithmetic from IEEE `+ - *`, no fused multiply-add), the only
//! implementation in the workspace until KRN.NUM.PRED exists. A zero in-circle determinant
//! (cocircular points) is resolved by a symbolic perturbation ordered by source: the lifted
//! coordinate of point `i` is `x_i^2 + y_i^2 + eps^r_i` with `r_i` smaller for a larger
//! source, so the largest source is lifted most. The perturbed sign is the sign of the
//! first non-zero cofactor taken in descending source order:
//! `+orient(b, c, d)` for `a`, `-orient(a, c, d)` for `b`, `+orient(a, b, d)` for `c`,
//! `-orient(a, b, c)` for `d`. For a counter-clockwise `(a, b, c)` the last is never zero,
//! so the perturbed test never ties. In a cocircular square the diagonal avoiding the
//! corner with the largest source is chosen; of two vertices that project onto the same
//! point the one with the smaller source is the Delaunay vertex.
//!
//! # Determinism (D0) and threads
//!
//! Each fan is a pure function of the vertex set and `g`. With `threads > 1` the vertices are
//! split into contiguous blocks, one scoped thread per block (a block whose thread cannot
//! start runs on the calling thread), joined in order, so the output and the first error do
//! not depend on the thread count. Sums have a fixed order, there is no fused multiply-add,
//! no hash container and no `f64` min or max.

use core::cmp::Ordering;

use fid_fit::vor::{incircle, orient2d};
use fid_geo_index::{dist2, PointIndex, COORD_LIMIT};
use fid_geo_types::{is_unit_normal, PointBuffer};

use crate::prep::{PolyError, VertexSet, MAX_THREADS};

/// The most neighbours a fan is built from (the centre not counted).
pub const MAX_NEIGHBOURS: usize = 32;
/// The normal-compatibility threshold: `cos(60 degrees)`, exact.
pub const NORMAL_COS: f64 = 0.5;
/// The smallest accepted maximum gap `g`, 2^-100 m.
pub const MIN_GAP: f64 = f64::from_bits((1023 - 100) << 52);
/// The largest accepted maximum gap `g`, 2^100 m.
pub const MAX_GAP: f64 = f64::from_bits((1023 + 100) << 52);
/// Projected coordinates smaller than this in magnitude, 2^-200 m, become zero.
pub const FLUSH: f64 = f64::from_bits((1023 - 200) << 52);
/// The short-edge factor `1 - 2^-46` on `g*g` (module documentation, step 5).
pub const EDGE_MARGIN: f64 = 1.0 - f64::from_bits((1023 - 46) << 52);

/// Why a fan has triangles or not (module documentation, step 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FanStatus {
    /// At least one triangle is kept.
    Ok,
    /// Fewer than 2 compatible neighbours within `g`.
    NoNeighbours,
    /// A neighbour with a smaller source projects onto the centre and takes its place.
    Hidden {
        /// The smallest such neighbour's source.
        by: u64,
    },
    /// The neighbourhood is collinear through the centre in projection.
    Degenerate,
    /// Every Delaunay triangle at the centre has an edge of at least `g`.
    AllEdgesLong,
}

impl FanStatus {
    /// A stable machine-readable name (`Hidden` without its source).
    pub fn as_str(self) -> &'static str {
        match self {
            FanStatus::Ok => "ok",
            FanStatus::NoNeighbours => "no_neighbours",
            FanStatus::Hidden { .. } => "hidden",
            FanStatus::Degenerate => "degenerate",
            FanStatus::AllEdgesLong => "all_edges_long",
        }
    }
}

/// The fan of one vertex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fan {
    /// The centre vertex's source.
    pub centre: u64,
    /// Why the fan has triangles or not; only [`FanStatus::Ok`] has any.
    pub status: FanStatus,
    /// The vertices of the kept triangles, by angle about the centre's normal.
    pub neighbours: Vec<u64>,
    /// The kept triangles `[centre, j, k]`, counter-clockwise about the centre's normal, in
    /// the angular order of `j`.
    pub triangles: Vec<[u64; 3]>,
}

fn invalid(name: &'static str, reason: &'static str) -> PolyError {
    PolyError::InvalidParameter { name, reason }
}

/// The fans of every vertex of `vs`, in vertex-set order, for the maximum gap `g` in metres
/// (module documentation).
pub fn fans(vs: &VertexSet, g: f64, threads: usize) -> Result<Vec<Fan>, PolyError> {
    if !(g.is_finite() && (MIN_GAP..=MAX_GAP).contains(&g)) {
        return Err(invalid("g", "not a finite value in [2^-100, 2^100] m"));
    }
    if threads == 0 || threads > MAX_THREADS {
        return Err(invalid("threads", "not in 1..=256"));
    }
    let n = vs.vertices.len();
    let mut sources: Vec<u64> = Vec::new();
    sources.try_reserve_exact(n).map_err(|_| PolyError::Allocation)?;
    if !vs.vertices.iter().all(|v| v.position.iter().all(|x| x.is_finite() && x.abs() <= COORD_LIMIT)) {
        return Err(invalid("vs.position", "not finite or beyond 2^500 m"));
    }
    if !vs.vertices.iter().all(|v| is_unit_normal(v.normal)) {
        return Err(invalid("vs.normal", "not a unit normal"));
    }
    sources.extend(vs.vertices.iter().map(|v| v.source));
    sources.sort_unstable();
    if sources.windows(2).any(|w| w[0] == w[1]) {
        return Err(invalid("vs.source", "two vertices share a source"));
    }
    if n == 0 {
        return Ok(Vec::new());
    }

    let mut cols = [Vec::new(), Vec::new(), Vec::new()];
    for c in cols.iter_mut() {
        c.try_reserve_exact(n).map_err(|_| PolyError::Allocation)?;
    }
    for v in &vs.vertices {
        for (c, &x) in cols.iter_mut().zip(&v.position) {
            c.push(x);
        }
    }
    let [x, y, z] = cols;
    let buf = PointBuffer::new(x, y, z);
    let index = PointIndex::build(&buf, 1)?;
    let ctx = Ctx { vs, index: &index, g, g2_short: (g * g) * EDGE_MARGIN };

    let workers = threads.min(n).max(1);
    let all: Vec<usize> = (0..n).collect();
    if workers == 1 {
        return fan_block(&ctx, &all);
    }
    let block = n.div_ceil(workers);
    let results: Vec<Result<Vec<Fan>, PolyError>> = std::thread::scope(|s| {
        let ctx = &ctx;
        let mut started = Vec::new();
        for part in all.chunks(block) {
            let handle = std::thread::Builder::new().spawn_scoped(s, move || fan_block(ctx, part)).ok();
            started.push((handle, part));
        }
        started
            .into_iter()
            .map(|(handle, part)| match handle {
                Some(h) => h.join().unwrap_or(Err(PolyError::WorkerFailed)),
                None => fan_block(ctx, part),
            })
            .collect()
    });
    let mut out = Vec::new();
    out.try_reserve_exact(n).map_err(|_| PolyError::Allocation)?;
    for r in results {
        out.extend(r?);
    }
    Ok(out)
}

/// Read-only context shared by the workers.
struct Ctx<'a> {
    vs: &'a VertexSet,
    index: &'a PointIndex<'a>,
    g: f64,
    g2_short: f64,
}

fn fan_block(ctx: &Ctx<'_>, part: &[usize]) -> Result<Vec<Fan>, PolyError> {
    let mut out = Vec::new();
    out.try_reserve_exact(part.len()).map_err(|_| PolyError::Allocation)?;
    for &k in part {
        out.push(fan_one(ctx, k)?);
    }
    Ok(out)
}

/// One point of a neighbourhood: its source, 3D position and projection.
#[derive(Debug, Clone, Copy)]
struct Local {
    id: u64,
    pos: [f64; 3],
    uv: [f64; 2],
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The tangent basis `(e1, e2)` of the unit normal `n` (step 2).
fn basis(n: [f64; 3]) -> Result<([f64; 3], [f64; 3]), PolyError> {
    let (mut axis, mut least) = (0usize, n[0].abs());
    for (i, v) in n.iter().enumerate().skip(1) {
        if v.abs() < least {
            (axis, least) = (i, v.abs());
        }
    }
    let e = match axis {
        0 => [1.0, 0.0, 0.0],
        1 => [0.0, 1.0, 0.0],
        _ => [0.0, 0.0, 1.0],
    };
    let t = cross(n, e);
    let len = fid_num::f::sqrt((t[0] * t[0] + t[1] * t[1]) + t[2] * t[2])?;
    // A unit normal has |n_axis| <= 1/sqrt(3) + 1e-9, so len >= 0.8.
    if len <= 0.5 {
        return Err(PolyError::Inconsistent);
    }
    let e1 = [t[0] / len, t[1] / len, t[2] / len];
    Ok((e1, cross(n, e1)))
}

/// Step 3's flush to `+0`.
fn flush(x: f64) -> f64 {
    if x.abs() < FLUSH {
        0.0
    } else {
        x
    }
}

fn fan_one(ctx: &Ctx<'_>, k: usize) -> Result<Fan, PolyError> {
    let verts = &ctx.vs.vertices;
    let c = verts.get(k).ok_or(PolyError::Inconsistent)?;
    let hits = ctx.index.radius(c.position, ctx.g)?;
    let mut cand: Vec<(f64, u64, usize)> = Vec::new();
    cand.try_reserve_exact(hits.len()).map_err(|_| PolyError::Allocation)?;
    for hit in &hits {
        let m = usize::try_from(hit.index).map_err(|_| PolyError::Inconsistent)?;
        if m == k {
            continue;
        }
        let q = verts.get(m).ok_or(PolyError::Inconsistent)?;
        if dot(c.normal, q.normal) >= NORMAL_COS {
            cand.push((hit.dist2, q.source, m));
        }
    }
    cand.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    cand.truncate(MAX_NEIGHBOURS);

    let (e1, e2) = basis(c.normal)?;
    let mut pts: Vec<Local> = Vec::new();
    pts.try_reserve_exact(cand.len() + 1).map_err(|_| PolyError::Allocation)?;
    pts.push(Local { id: c.source, pos: c.position, uv: [0.0, 0.0] });
    for &(_, id, m) in &cand {
        let q = verts.get(m).ok_or(PolyError::Inconsistent)?;
        let d = [q.position[0] - c.position[0], q.position[1] - c.position[1], q.position[2] - c.position[2]];
        let uv = [flush(dot(d, e1)), flush(dot(d, e2))];
        if !(uv[0].is_finite() && uv[1].is_finite()) {
            return Err(PolyError::Inconsistent);
        }
        pts.push(Local { id, pos: q.position, uv });
    }
    star(&pts, ctx.g2_short)
}

/// The kept Delaunay star of `pts[0]` (steps 4 to 6).
fn star(pts: &[Local], g2_short: f64) -> Result<Fan, PolyError> {
    let centre = pts.first().ok_or(PolyError::Inconsistent)?;
    let short = |a: &Local, b: &Local| dist2(a.pos, b.pos) < g2_short;
    let mut tris: Vec<(usize, usize)> = Vec::new();
    for (j, pj) in pts.iter().enumerate().skip(1) {
        if !short(centre, pj) {
            continue;
        }
        for (kk, pk) in pts.iter().enumerate().skip(1) {
            if kk == j || orient2d(centre.uv, pj.uv, pk.uv) <= 0 || !short(centre, pk) || !short(pj, pk) {
                continue;
            }
            let empty = pts
                .iter()
                .enumerate()
                .skip(1)
                .filter(|(l, _)| *l != j && *l != kk)
                .all(|(_, pl)| perturbed_incircle([centre, pj, pk, pl]) < 0);
            if empty {
                tris.try_reserve(1).map_err(|_| PolyError::Allocation)?;
                tris.push((j, kk));
            }
        }
    }

    let mut link: Vec<usize> = tris.iter().flat_map(|&(j, kk)| [j, kk]).collect();
    link.sort_unstable();
    link.dedup();
    let mut keyed: Vec<(usize, &Local)> = Vec::new();
    keyed.try_reserve_exact(link.len()).map_err(|_| PolyError::Allocation)?;
    for &i in &link {
        keyed.push((i, pts.get(i).ok_or(PolyError::Inconsistent)?));
    }
    keyed.sort_unstable_by(|a, b| angular(a.1, b.1));
    let mut rank = vec![0usize; pts.len()];
    for (r, (i, _)) in keyed.iter().enumerate() {
        *rank.get_mut(*i).ok_or(PolyError::Inconsistent)? = r;
    }
    tris.sort_unstable_by_key(|&(j, _)| rank.get(j).copied().unwrap_or(usize::MAX));

    let id = |i: usize| pts.get(i).map(|p| p.id).ok_or(PolyError::Inconsistent);
    let neighbours = keyed.iter().map(|(_, p)| p.id).collect();
    let mut triangles = Vec::new();
    triangles.try_reserve_exact(tris.len()).map_err(|_| PolyError::Allocation)?;
    for &(j, kk) in &tris {
        triangles.push([centre.id, id(j)?, id(kk)?]);
    }
    let status = status_of(pts, !triangles.is_empty())?;
    Ok(Fan { centre: centre.id, status, neighbours, triangles })
}

/// Step 7. `kept` says whether a triangle survived; a contradiction with the geometry (a
/// hidden or degenerate centre with triangles) is [`PolyError::Inconsistent`], never `Ok`.
fn status_of(pts: &[Local], kept: bool) -> Result<FanStatus, PolyError> {
    let centre = pts.first().ok_or(PolyError::Inconsistent)?;
    let others = pts.get(1..).unwrap_or(&[]);
    let twin = others.iter().filter(|q| q.uv == [0.0, 0.0] && q.id < centre.id).map(|q| q.id).min();
    let status = if let Some(by) = twin {
        FanStatus::Hidden { by }
    } else if others.len() < 2 {
        FanStatus::NoNeighbours
    } else if others.iter().all(|a| others.iter().all(|b| orient2d(centre.uv, a.uv, b.uv) == 0)) {
        FanStatus::Degenerate
    } else if kept {
        return Ok(FanStatus::Ok);
    } else {
        FanStatus::AllEdgesLong
    };
    if kept {
        return Err(PolyError::Inconsistent);
    }
    Ok(status)
}

/// The half plane of step 6: 0 for `v > 0` or (`v = 0`, `u > 0`), else 1.
fn half(p: [f64; 2]) -> u8 {
    if p[1] > 0.0 || (p[1] == 0.0 && p[0] > 0.0) {
        0
    } else {
        1
    }
}

/// The angular order of step 6 about the origin, then source (a tie is not reachable for
/// the vertices of a Delaunay star).
fn angular(a: &Local, b: &Local) -> Ordering {
    half(a.uv).cmp(&half(b.uv)).then_with(|| match orient2d([0.0, 0.0], a.uv, b.uv) {
        1 => Ordering::Less,
        -1 => Ordering::Greater,
        _ => a.id.cmp(&b.id),
    })
}

/// The in-circle sign of `d` against `(a, b, c)` under the source-ordered symbolic
/// perturbation (module documentation, Predicates): positive inside, negative outside.
fn perturbed_incircle(p: [&Local; 4]) -> i8 {
    let [a, b, c, d] = p;
    let s = incircle(a.uv, b.uv, c.uv, d.uv);
    if s != 0 {
        return s;
    }
    let mut order = [0usize, 1, 2, 3];
    order.sort_unstable_by(|&x, &y| p[y].id.cmp(&p[x].id));
    for r in order {
        let s = match r {
            0 => orient2d(b.uv, c.uv, d.uv),
            1 => -orient2d(a.uv, c.uv, d.uv),
            2 => orient2d(a.uv, b.uv, d.uv),
            _ => -orient2d(a.uv, b.uv, c.uv),
        };
        if s != 0 {
            return s;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn pt(id: u64, u: f64, v: f64) -> Local {
        Local { id, pos: [u, v, 0.0], uv: [u, v] }
    }

    #[test]
    fn constants() {
        assert_eq!(MIN_GAP, 2f64.powi(-100));
        assert_eq!(MAX_GAP, 2f64.powi(100));
        assert_eq!(FLUSH, 2f64.powi(-200));
        assert_eq!(EDGE_MARGIN, 1.0 - 2f64.powi(-46));
    }

    #[test]
    fn flush_to_positive_zero() {
        assert_eq!(flush(2f64.powi(-201)).to_bits(), 0);
        assert_eq!(flush(-2f64.powi(-201)).to_bits(), 0);
        assert_eq!(flush(-0.0).to_bits(), 0);
        assert_eq!(flush(2f64.powi(-200)), 2f64.powi(-200));
    }

    #[test]
    fn basis_is_right_handed() {
        let (e1, e2) = basis([0.0, 0.0, 1.0]).unwrap();
        assert_eq!(e1, [0.0, 1.0, 0.0]);
        assert_eq!(e2, [-1.0, 0.0, 0.0]);
        let (e1, e2) = basis([0.0, -1.0, 0.0]).unwrap();
        // Axis x (|n_x| = |n_z| = 0, lowest first): e1 = n x (1,0,0) = (0, 0, 1).
        assert_eq!(e1, [0.0, 0.0, 1.0]);
        assert_eq!(cross(e1, e2), [0.0, -1.0, 0.0]);
    }

    #[test]
    fn unit_square_follows_largest_source() {
        // Square c(0,0) a(1,0) d(1,1) b(0,1): cocircular. Is d inside circle (c, a, b)?
        let sq = |ic, ia, id, ib| {
            let (c, a, d, b) = (pt(ic, 0.0, 0.0), pt(ia, 1.0, 0.0), pt(id, 1.0, 1.0), pt(ib, 0.0, 1.0));
            perturbed_incircle([&c, &a, &b, &d])
        };
        // Largest source at the centre or at d: the diagonal a-b is chosen, d is outside.
        assert_eq!(sq(9, 1, 2, 3), -1);
        assert_eq!(sq(1, 2, 9, 3), -1);
        // Largest source at a or b: the diagonal c-d is chosen, d is inside.
        assert_eq!(sq(1, 9, 2, 3), 1);
        assert_eq!(sq(1, 2, 3, 9), 1);
    }

    #[test]
    fn duplicate_keeps_smaller_source() {
        // j and its duplicate j2 project onto the same point.
        let (c, k) = (pt(5, 0.0, 0.0), pt(6, 0.0, 1.0));
        let (j_small, j_large) = (pt(1, 1.0, 0.0), pt(8, 1.0, 0.0));
        // The larger source is hidden: it is inside the circle of the smaller one's triangle,
        // and the smaller one is outside the circle of the larger one's triangle.
        assert_eq!(perturbed_incircle([&c, &j_large, &k, &j_small]), 1);
        assert_eq!(perturbed_incircle([&c, &j_small, &k, &j_large]), -1);
    }

    #[test]
    fn star_of_square_centre() {
        // Centre 0 at the middle of a 3 x 3 grid; ids rising row by row.
        let mut pts = vec![pt(4, 0.0, 0.0)];
        let mut id = 0;
        for j in -1i32..=1 {
            for i in -1i32..=1 {
                if i != 0 || j != 0 {
                    pts.push(pt(id, f64::from(i), f64::from(j)));
                }
                id += 1;
            }
        }
        let fan = star(&pts, 1.5 * 1.5 * 1.5).unwrap();
        // Right (5) at angle 0 first; ccw: 5, 8, 7, 6, 3, 0, 1, 2.
        assert_eq!(fan.centre, 4);
        // Squares (4,5,8,7): largest 8 -> diagonal 5-7. (4,7,6,3): largest 7 -> diagonal 4-6.
        // (4,3,0,1): largest 4 -> diagonal 3-1. (4,1,2,5): largest 5 -> diagonal 4-2.
        assert_eq!(fan.triangles, vec![[4, 5, 7], [4, 7, 6], [4, 6, 3], [4, 3, 1], [4, 1, 2], [4, 2, 5]]);
        assert_eq!(fan.neighbours, vec![5, 7, 6, 3, 1, 2]);
    }

    #[test]
    fn long_edges_are_dropped() {
        // Equilateral triangle with side 1 around the centre; g^2 just above and below 1.
        let h = 3f64.sqrt() / 2.0;
        let pts = [pt(0, 0.0, 0.0), pt(1, 1.0, 0.0), pt(2, 0.5, h)];
        assert_eq!(star(&pts, 1.0 * 1.0 * EDGE_MARGIN).unwrap().triangles.len(), 0);
        assert_eq!(star(&pts, 1.01 * 1.01 * EDGE_MARGIN).unwrap().triangles, vec![[0, 1, 2]]);
    }
}
