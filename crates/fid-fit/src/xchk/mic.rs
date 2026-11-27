//! Maximum inscribed 2D circle with the centre restricted to a convex polygon, from the
//! nearest-point Voronoi diagram (REQ-KRN.ENV.XCHK-003, -004, -005).
//!
//! # Pinned rules
//!
//! 1. **Region.** A region is a polygon given as at least 3 finite vertices in
//!    counter-clockwise order that is strictly convex: every consecutive triple turns left and
//!    every fan triangle `(v_0, v_i, v_(i+1))` is counter-clockwise, both by the exact-sign
//!    `orient2d` of `KRN.MZ.VOR` (the fan test refuses star polygons, whose turns are all
//!    left). Anything else is `InvalidParams{field: "region"}`.
//! 2. **Tolerance.** `boundary_tol` must be finite and at least [`region_tol_floor`],
//!    `(16 * EPSILON) * m` with `m` the total-order maximum of `|v_ij|` over the region's
//!    vertex coordinates (the rounding resolution of the boundary distance); below it, `0`
//!    and `-0.0` included, it is `InvalidParams{field: "boundary_tol"}`, never a silent clamp.
//! 3. **Points.** Checked and triangulated by `vor::nearest_voronoi` (`TooFewPoints` below 3
//!    distinct points, `Collinear`, `NonFinite`, `Subnormal`); more than `MAX_POINTS` points is
//!    `TooLarge`.
//! 4. **Candidates** (REQ-003). Within a Voronoi cell the distance to the cell's site is
//!    convex, so the largest empty circle centred in the region has its centre at a vertex of
//!    some cell intersected with the region: (a) every Voronoi vertex inside the closed region
//!    (exact `orient2d >= 0` against every edge); (b) every crossing of a Voronoi edge (segment
//!    or ray) with a region edge, located on the region edge at the parameter `s` of the
//!    line-line intersection, accepted for `s` in `[-1e-9, 1 + 1e-9]` and edge parameter
//!    `t >= -1e-9` (`<= 1 + 1e-9` for a segment), then clamped to `[0, 1]`; parallel pairs are
//!    skipped, their ends being candidates (a) or (c); (c) every region vertex. Extra
//!    candidates are harmless: every candidate is a feasible centre and is scored directly.
//! 5. **Score.** A candidate's radius is the total-order minimum over all points of
//!    `f::hypot(p - c)`. The best candidate has the largest radius; ties go to the smaller
//!    centre in total order on `(x, y)`.
//! 6. **Result.** `contacts` are the points with `|d - r| <= (64 * EPSILON) * (m_p + r)`,
//!    `m_p` the largest `|coordinate|` of the points, ascending. `boundary_distance_m` is the
//!    total-order minimum over the region edges of the signed distance
//!    `cross(q - p, c - p) / hypot(q - p)` (positive inside); `on_region_boundary` is
//!    `boundary_distance_m <= boundary_tol` (REQ-004).
//!
//! [`intersect_convex`] clips one region by another (Sutherland-Hodgman, inclusion by exact
//! `orient2d`), drops repeated vertices and vertices that no longer turn left, and returns
//! `InvalidParams{field: "region_intersection"}` when fewer than 3 vertices remain.

use core::cmp::Ordering;

use fid_kci::AlgorithmId;
use fid_num::{f, NumError};

use crate::contract::{FitError, MAX_POINTS, V2};
use crate::vor::{convex_hull, nearest_voronoi, orient2d, VoronoiEdgeKind};

use super::mic_circle2d_id;

const OP: &str = "xchk.mic";
/// Parameter slack of the edge crossings (rule 4).
const PARAM_SLACK: f64 = 1e-9;

/// A maximum inscribed circle in a convex region.
#[derive(Debug, Clone, PartialEq)]
pub struct Inscribed {
    /// Centre in metres.
    pub centre: V2,
    /// Radius in metres: the distance from the centre to the nearest point.
    pub radius: f64,
    /// Indices of the points at the radius (rule 6), ascending.
    pub contacts: Vec<u32>,
    /// Signed distance of the centre from the region's boundary (metres, positive inside).
    pub boundary_distance_m: f64,
    /// The optimum lies on the region's boundary (within `boundary_tol`): the envelope is not
    /// bounded by the data alone (REQ-KRN.ENV.XCHK-004).
    pub on_region_boundary: bool,
    /// `fit.circle2d.mic_vor@1.0.0`.
    pub algorithm: AlgorithmId,
}

fn alloc<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    Ok(v)
}

fn infinite() -> FitError {
    FitError::Num(NumError::Infinite { op: OP, index: None })
}

fn cross(a: V2, b: V2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}

fn edge(poly: &[V2], k: usize) -> (V2, V2) {
    let m = poly.len();
    let a = poly.get(k % m).copied().unwrap_or([0.0, 0.0]);
    let b = poly.get((k + 1) % m).copied().unwrap_or([0.0, 0.0]);
    (a, b)
}

/// Rule 1.
fn check_region(region: &[V2]) -> Result<(), FitError> {
    let bad = FitError::InvalidParams { field: "region" };
    let m = region.len();
    if m < 3 || region.iter().any(|v| !(v[0].is_finite() && v[1].is_finite())) {
        return Err(bad);
    }
    for k in 0..m {
        let (a, b) = edge(region, k);
        let (_, c) = edge(region, k + 1);
        if orient2d(a, b, c) <= 0 {
            return Err(bad);
        }
    }
    let v0 = region[0];
    for w in region[1..].windows(2) {
        if orient2d(v0, w[0], w[1]) <= 0 {
            return Err(bad);
        }
    }
    Ok(())
}

fn max_abs(pts: &[V2]) -> Result<f64, FitError> {
    let mut m = 0.0_f64;
    for p in pts {
        m = f::total_max(m, p[0].abs())?;
        m = f::total_max(m, p[1].abs())?;
    }
    Ok(m)
}

/// The smallest `boundary_tol` [`mic_circle2d_vor`] accepts for `region` (rule 2). The region
/// must pass rule 1.
pub fn region_tol_floor(region: &[V2]) -> Result<f64, FitError> {
    check_region(region)?;
    let floor = (16.0 * f64::EPSILON) * max_abs(region)?;
    if !floor.is_finite() {
        return Err(infinite());
    }
    Ok(floor)
}

/// The strict convex hull of the points as a region (counter-clockwise, from
/// `vor::convex_hull`).
pub fn hull_region(points: &[V2]) -> Result<Vec<V2>, FitError> {
    let idx = convex_hull(points)?;
    let mut out = alloc(idx.len())?;
    for i in idx {
        out.push(*points.get(i).ok_or(FitError::InvalidParams { field: "region" })?);
    }
    Ok(out)
}

/// The intersection of two regions (rules 1 and the module note), counter-clockwise.
pub fn intersect_convex(a: &[V2], b: &[V2]) -> Result<Vec<V2>, FitError> {
    check_region(a)?;
    check_region(b)?;
    let mut cur: Vec<V2> = alloc(a.len() + b.len())?;
    cur.extend_from_slice(a);
    for k in 0..b.len() {
        let (p, q) = edge(b, k);
        let mut next: Vec<V2> = alloc(cur.len() + 1)?;
        let m = cur.len();
        for (j, &c) in cur.iter().enumerate() {
            let pr = cur[(j + m - 1) % m];
            let in_c = orient2d(p, q, c) >= 0;
            let in_p = orient2d(p, q, pr) >= 0;
            if in_c != in_p {
                let sc = cross(sub(q, p), sub(c, p));
                let sp = cross(sub(q, p), sub(pr, p));
                let den = sp - sc;
                if den != 0.0 {
                    let t = sp / den;
                    let x = [pr[0] + t * (c[0] - pr[0]), pr[1] + t * (c[1] - pr[1])];
                    if !(x[0].is_finite() && x[1].is_finite()) {
                        return Err(infinite());
                    }
                    next.push(x);
                }
            }
            if in_c {
                next.push(c);
            }
        }
        cur = next;
        if cur.is_empty() {
            break;
        }
    }
    // Drop repeated vertices, then (one at a time, first in order) vertices that do not turn
    // left: collinear middles and rounding-sized reflex turns.
    let mut poly: Vec<V2> = alloc(cur.len())?;
    for v in cur {
        if poly.last() != Some(&v) {
            poly.push(v);
        }
    }
    while poly.len() >= 2 && poly.first() == poly.last() {
        poly.pop();
    }
    let bound = poly.len();
    for _ in 0..bound {
        let m = poly.len();
        if m < 3 {
            break;
        }
        let bad = (0..m).find(|&j| orient2d(poly[(j + m - 1) % m], poly[j], poly[(j + 1) % m]) <= 0);
        match bad {
            Some(j) => {
                poly.remove(j);
            }
            None => break,
        }
    }
    let cur = poly;
    if cur.len() < 3 || check_region(&cur).is_err() {
        return Err(FitError::InvalidParams { field: "region_intersection" });
    }
    Ok(cur)
}

/// The radius of the empty circle about `c` (rule 5).
fn score(points: &[V2], c: V2) -> Result<f64, FitError> {
    let mut best = f64::INFINITY;
    for p in points {
        let d = f::hypot(p[0] - c[0], p[1] - c[1])?;
        best = f::total_min(best, d)?;
    }
    Ok(best)
}

struct Best {
    r: f64,
    c: V2,
}

fn consider(best: &mut Option<Best>, points: &[V2], c: V2) -> Result<(), FitError> {
    if !(c[0].is_finite() && c[1].is_finite()) {
        return Err(infinite());
    }
    let r = score(points, c)?;
    let better = match best {
        None => true,
        Some(b) => match f::total_cmp(r, b.r)? {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => match f::total_cmp(c[0], b.c[0])? {
                Ordering::Less => true,
                Ordering::Greater => false,
                Ordering::Equal => f::total_cmp(c[1], b.c[1])? == Ordering::Less,
            },
        },
    };
    if better {
        *best = Some(Best { r, c });
    }
    Ok(())
}

fn inside_closed(region: &[V2], c: V2) -> bool {
    (0..region.len()).all(|k| {
        let (a, b) = edge(region, k);
        orient2d(a, b, c) >= 0
    })
}

/// The crossing of the line `a + t d` with the region edge `p q` (rule 4).
fn crossing(a: V2, d: V2, ray: bool, p: V2, q: V2) -> Option<V2> {
    let e = sub(q, p);
    let den = cross(d, e);
    if den == 0.0 || !den.is_finite() {
        return None;
    }
    let w = sub(p, a);
    let t = cross(w, e) / den;
    let s = cross(w, d) / den;
    if !(t.is_finite() && s.is_finite()) {
        return None;
    }
    if t < -PARAM_SLACK || (!ray && t > 1.0 + PARAM_SLACK) || !(-PARAM_SLACK..=1.0 + PARAM_SLACK).contains(&s) {
        return None;
    }
    let s = f::total_clamp(s, 0.0, 1.0).ok()?;
    Some([p[0] + s * e[0], p[1] + s * e[1]])
}

/// The maximum inscribed circle of `points` with its centre in the convex `region`
/// (rules 1 to 6).
pub fn mic_circle2d_vor(points: &[V2], region: &[V2], boundary_tol: f64) -> Result<Inscribed, FitError> {
    let floor = region_tol_floor(region)?;
    if !(boundary_tol.is_finite() && boundary_tol >= floor) {
        return Err(FitError::InvalidParams { field: "boundary_tol" });
    }
    if points.len() > MAX_POINTS {
        return Err(FitError::TooLarge { n: points.len(), max: MAX_POINTS });
    }
    let vd = nearest_voronoi(points)?;
    let mut best: Option<Best> = None;

    // (a) Voronoi vertices inside the closed region.
    for v in &vd.vertices {
        let c = [v.x, v.y];
        if inside_closed(region, c) {
            consider(&mut best, points, c)?;
        }
    }
    // (b) Voronoi edges crossing region edges.
    for e in &vd.edges {
        let (a, d, ray) = match e.kind {
            VoronoiEdgeKind::Segment { start, end } => {
                let (Some(s), Some(t)) = (vd.vertices.get(start), vd.vertices.get(end)) else {
                    return Err(FitError::Num(NumError::Domain { op: OP, detail: "voronoi_index" }));
                };
                ([s.x, s.y], [t.x - s.x, t.y - s.y], false)
            }
            VoronoiEdgeKind::Ray { start, dir } => {
                let Some(s) = vd.vertices.get(start) else {
                    return Err(FitError::Num(NumError::Domain { op: OP, detail: "voronoi_index" }));
                };
                ([s.x, s.y], dir, true)
            }
        };
        for k in 0..region.len() {
            let (p, q) = edge(region, k);
            if let Some(c) = crossing(a, d, ray, p, q) {
                consider(&mut best, points, c)?;
            }
        }
    }
    // (c) Region vertices.
    for v in region {
        consider(&mut best, points, *v)?;
    }
    let Best { r, c } = best.ok_or(FitError::InvalidParams { field: "region" })?;

    let tol = (64.0 * f64::EPSILON) * (max_abs(points)? + r);
    let mut contacts: Vec<u32> = alloc(points.len())?;
    for (i, p) in points.iter().enumerate() {
        let d = f::hypot(p[0] - c[0], p[1] - c[1])?;
        if (d - r).abs() <= tol {
            contacts.push(i as u32);
        }
    }
    let mut dist = f64::INFINITY;
    for k in 0..region.len() {
        let (p, q) = edge(region, k);
        let e = sub(q, p);
        let len = f::hypot(e[0], e[1])?;
        let sd = cross(e, sub(c, p)) / len;
        if !sd.is_finite() {
            return Err(infinite());
        }
        dist = f::total_min(dist, sd)?;
    }
    Ok(Inscribed {
        centre: c,
        radius: r,
        contacts,
        boundary_distance_m: dist,
        on_region_boundary: dist <= boundary_tol,
        algorithm: mic_circle2d_id(),
    })
}
