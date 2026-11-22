//! Exact Voronoi-based minimum-zone 2D circle (spec node `KRN.MZ.VOR`).
//!
//! Computes the global minimum-zone (Chebyshev) 2D circle combinatorially by constructing:
//! 1. The nearest-point Voronoi diagram (dual of deterministic incremental Bowyer-Watson
//!    Delaunay triangulation after lexicographic total-order sort);
//! 2. The convex hull (Andrew's monotone chain) and farthest-point Voronoi diagram of hull
//!    vertices (dual of farthest-point Delaunay triangulation);
//! 3. Exact candidate evaluation across every nearest-diagram vertex, every farthest-diagram
//!    vertex, and every nearest-edge x farthest-edge intersection (Roy & Zhang 1992).
//!
//! Geometric predicates (`orient2d`, `incircle`) use adaptive-precision expansion arithmetic
//! built from plain IEEE-754 binary64 `+`, `-`, `*` without FMA (Shewchuk 1997).
//!
//! Algorithm ID: `fit.circle2d.mz_vor@1.0.0` (spec card in `cards/fit.circle2d.mz_vor@1.0.0.toml`).
//! Constraints: KNP-1, D0, zero `unsafe`, zero panics, no `HashMap`/`HashSet`, no `f64::min`/`max`.

mod pred;
mod tri;

pub use pred::{incircle, orient2d};
pub use tri::{VoronoiDiagram, VoronoiEdge, VoronoiEdgeKind, VoronoiVertex};

use core::cmp::Ordering;
use fid_kci::AlgorithmId;
use fid_num::{f, red};

use crate::contract::{FitError, PointsView};

/// Algorithm identifier string (`fit.circle2d.mz_vor@1.0.0`).
pub const ALGORITHM_ID: &str = "fit.circle2d.mz_vor@1.0.0";

/// Returns the parsed [`AlgorithmId`] for `fit.circle2d.mz_vor@1.0.0`.
pub fn algorithm_id() -> AlgorithmId {
    AlgorithmId {
        module: "fit".to_string(),
        feature: "circle2d".to_string(),
        method: "mz_vor".to_string(),
        version: (1, 0, 0),
    }
}

/// Minimum-zone 2D circle solution returned by [`mz_circle2d`].
#[derive(Debug, Clone, PartialEq)]
pub struct MzCircle {
    /// Optimal centre `[cx, cy]` in SI metres.
    pub center: [f64; 2],
    /// Minimum radial distance `R_min` from `center` to the input points (metres).
    pub r_inner: f64,
    /// Maximum radial distance `R_max` from `center` to the input points (metres).
    pub r_outer: f64,
    /// Mean radius `0.5 * (r_inner + r_outer)` (metres).
    pub r_mid: f64,
    /// Minimum zone width `r_outer - r_inner` (metres).
    pub width: f64,
    /// Original point indices touching the inner boundary circle (sorted ascending).
    pub inner_contacts: Vec<usize>,
    /// Original point indices touching the outer boundary circle (sorted ascending).
    pub outer_contacts: Vec<usize>,
    /// Algorithm identifier (`fit.circle2d.mz_vor@1.0.0`).
    pub algorithm: AlgorithmId,
}

/// Computes the strict CCW convex hull vertex indices (in input indexing) of planar points.
pub fn convex_hull(points: &[[f64; 2]]) -> Result<Vec<usize>, FitError> {
    let prep = prepare_points(points, 3)?;
    let hull = tri::monotone_hull(&prep.scaled)?;
    Ok(hull.into_iter().map(|i| prep.orig[i as usize]).collect())
}

/// Builds the nearest-point Voronoi diagram of planar `points` (coordinates in original units).
pub fn nearest_voronoi(points: &[[f64; 2]]) -> Result<VoronoiDiagram, FitError> {
    let prep = prepare_points(points, 3)?;
    let _hull = tri::monotone_hull(&prep.scaled)?;
    let tris = tri::delaunay_nearest(&prep.scaled)?;
    let mut vor = tri::build_voronoi_dual(&tris, &prep.scaled, &prep.orig, true)?;
    unscale_voronoi(&mut vor, prep.mean, prep.scale);
    Ok(vor)
}

/// Builds the farthest-point Voronoi diagram of the convex-hull vertices of planar `points`.
pub fn farthest_voronoi(points: &[[f64; 2]]) -> Result<VoronoiDiagram, FitError> {
    let prep = prepare_points(points, 3)?;
    let hull = tri::monotone_hull(&prep.scaled)?;
    let tris = tri::delaunay_farthest(&prep.scaled, &hull)?;
    let mut vor = tri::build_voronoi_dual(&tris, &prep.scaled, &prep.orig, false)?;
    unscale_voronoi(&mut vor, prep.mean, prep.scale);
    Ok(vor)
}

/// Computes the exact Voronoi-based minimum-zone 2D circle from a planar [`PointsView`]
/// (uses the `(x, y)` coordinates of each point in SI metres).
pub fn mz_circle2d(pts: &PointsView<'_>) -> Result<MzCircle, FitError> {
    let n = pts.len();
    if n < 4 {
        return Err(FitError::TooFewPoints { n, need: 4 });
    }
    let mut xy = Vec::with_capacity(n);
    for i in 0..n {
        if let Some(p) = pts.point(i) {
            xy.push([p[0], p[1]]);
        }
    }
    mz_circle2d_xy(&xy)
}

/// Computes the exact Voronoi-based minimum-zone 2D circle from planar `[x, y]` points (metres).
pub fn mz_circle2d_xy(points: &[[f64; 2]]) -> Result<MzCircle, FitError> {
    let prep = prepare_points(points, 4)?;
    let hull = tri::monotone_hull(&prep.scaled)?;
    let hull_pts: Vec<[f64; 2]> = hull.iter().map(|&idx| prep.scaled[idx as usize]).collect();

    let near_tris = tri::delaunay_nearest(&prep.scaled)?;
    let far_tris = tri::delaunay_farthest(&prep.scaled, &hull)?;

    // Build duals indexed into `prep.scaled` (0..m) for O(1) site coordinate lookup during search.
    let identity_idx: Vec<usize> = (0..prep.scaled.len()).collect();
    let near_vor = tri::build_voronoi_dual(&near_tris, &prep.scaled, &identity_idx, true)?;
    let far_vor = tri::build_voronoi_dual(&far_tris, &prep.scaled, &identity_idx, false)?;

    let mut best: Option<BestCandidate> = None;

    // Filter margin so floating-point rounding never prunes a tied or optimal candidate.
    const PRUNE_MARGIN: f64 = 1.0e-8;

    // Phase 1: Nearest-edge x Farthest-edge intersections (2-2 contacts; O(1) lower bound).
    let near_boxes = build_edge_boxes(&near_vor);
    let far_boxes = build_edge_boxes(&far_vor);

    // Sweep-line on `min_x`: sort both edge box lists by `min_x` (total_cmp) with deterministic
    // tie-break on edge index.
    let mut near_order: Vec<usize> = (0..near_boxes.len()).collect();
    near_order.sort_by(|&a, &b| cmp_box_min_x(&near_boxes[a], &near_boxes[b], a, b));
    let mut far_order: Vec<usize> = (0..far_boxes.len()).collect();
    far_order.sort_by(|&a, &b| cmp_box_min_x(&far_boxes[a], &far_boxes[b], a, b));

    for &ni in &near_order {
        let nb = &near_boxes[ni];
        let ne = &near_vor.edges[ni];
        let nu = prep.scaled[ne.sites[0]];
        let nv = prep.scaled[ne.sites[1]];

        for &fi in &far_order {
            let fb = &far_boxes[fi];
            if fb.min_x > nb.max_x {
                break;
            }
            if fb.max_x < nb.min_x || fb.min_y > nb.max_y || fb.max_y < nb.min_y {
                continue;
            }
            let fe = &far_vor.edges[fi];
            let fp = prep.scaled[fe.sites[0]];
            let fq = prep.scaled[fe.sites[1]];

            let Some(c) = intersect_bisectors(nu, nv, fp, fq) else {
                continue;
            };
            if !point_on_vor_edge(c, ne, &near_vor.vertices) || !point_on_vor_edge(c, fe, &far_vor.vertices) {
                continue;
            }

            let r_min_est = f::hypot(nu[0] - c[0], nu[1] - c[1]).map_err(FitError::Num)?;
            let r_max_est = f::hypot(fp[0] - c[0], fp[1] - c[1]).map_err(FitError::Num)?;
            let w_est = r_max_est - r_min_est;
            if let Some(ref b) = best {
                if w_est > b.width + PRUNE_MARGIN {
                    continue;
                }
            }
            consider_candidate(&mut best, c, &prep.scaled, &hull_pts, prep.mean, prep.scale)?;
        }
    }

    // Phase 2: Nearest-point Voronoi vertices (3-1 contacts).
    for v in &near_vor.vertices {
        let c = [v.x, v.y];
        let s0 = prep.scaled[v.sites[0]];
        let r_min_est = f::hypot(s0[0] - c[0], s0[1] - c[1]).map_err(FitError::Num)?;
        if let Some(ref b) = best {
            let limit = b.width + PRUNE_MARGIN;
            let mut pruned = false;
            for &hp in &hull_pts {
                let d = f::hypot(hp[0] - c[0], hp[1] - c[1]).map_err(FitError::Num)?;
                if d - r_min_est > limit {
                    pruned = true;
                    break;
                }
            }
            if pruned {
                continue;
            }
        }
        consider_candidate(&mut best, c, &prep.scaled, &hull_pts, prep.mean, prep.scale)?;
    }

    // Phase 3: Farthest-point Voronoi vertices (1-3 contacts).
    for v in &far_vor.vertices {
        let c = [v.x, v.y];
        let s0 = prep.scaled[v.sites[0]];
        let r_max_est = f::hypot(s0[0] - c[0], s0[1] - c[1]).map_err(FitError::Num)?;
        if let Some(ref b) = best {
            let limit = b.width + PRUNE_MARGIN;
            let mut pruned = false;
            for &pt in &prep.scaled {
                let d = f::hypot(pt[0] - c[0], pt[1] - c[1]).map_err(FitError::Num)?;
                if r_max_est - d > limit {
                    pruned = true;
                    break;
                }
            }
            if pruned {
                continue;
            }
        }
        consider_candidate(&mut best, c, &prep.scaled, &hull_pts, prep.mean, prep.scale)?;
    }

    let best = best.ok_or(FitError::Collinear { n: points.len(), rank: 1, cond: f64::INFINITY })?;

    // Collect contacts across ALL original input points (sorted by original index).
    let contact_tol = 1.0e-13 * best.r_max;
    let mut inner_contacts = Vec::new();
    let mut outer_contacts = Vec::new();
    for (orig_i, &[x, y]) in points.iter().enumerate() {
        let qx = (x - prep.mean[0]) / prep.scale;
        let qy = (y - prep.mean[1]) / prep.scale;
        let d = f::hypot(qx - best.cx, qy - best.cy).map_err(FitError::Num)?;
        if (d - best.r_min).abs() <= contact_tol {
            inner_contacts.push(orig_i);
        }
        if (best.r_max - d).abs() <= contact_tol {
            outer_contacts.push(orig_i);
        }
    }

    let r_inner = best.r_min * prep.scale;
    let r_outer = best.r_max * prep.scale;
    let width = (best.r_max - best.r_min) * prep.scale;
    let r_mid = 0.5 * (r_inner + r_outer);

    Ok(MzCircle {
        center: [best.orig_cx, best.orig_cy],
        r_inner,
        r_outer,
        r_mid,
        width,
        inner_contacts,
        outer_contacts,
        algorithm: algorithm_id(),
    })
}

// ------------------------------------------------------------------ preparation & scaling

struct PreparedPoints {
    mean: [f64; 2],
    scale: f64,
    /// Deduplicated, centred, power-of-two scaled points in lexicographic `(x, y)` total order.
    scaled: Vec<[f64; 2]>,
    /// Lowest original point index for each deduplicated point.
    orig: Vec<usize>,
}

fn prepare_points(points: &[[f64; 2]], min_distinct: usize) -> Result<PreparedPoints, FitError> {
    let n = points.len();
    for (i, &[x, y]) in points.iter().enumerate() {
        if !x.is_finite() || !y.is_finite() {
            return Err(FitError::NonFinite { index: i });
        }
        if x.is_subnormal() || y.is_subnormal() {
            return Err(FitError::Subnormal { index: i });
        }
    }
    if n < min_distinct {
        return Err(FitError::TooFewPoints { n, need: min_distinct });
    }

    let xs: Vec<f64> = points.iter().map(|p| p[0]).collect();
    let ys: Vec<f64> = points.iter().map(|p| p[1]).collect();
    let mx = red::mean(&xs).map_err(FitError::Num)?;
    let my = red::mean(&ys).map_err(FitError::Num)?;

    let mut max_dev = 0.0_f64;
    for &[x, y] in points {
        max_dev = f::total_max(max_dev, (x - mx).abs()).map_err(FitError::Num)?;
        max_dev = f::total_max(max_dev, (y - my).abs()).map_err(FitError::Num)?;
    }
    if max_dev == 0.0 {
        return Err(FitError::Collinear { n, rank: 0, cond: f64::INFINITY });
    }

    // Power-of-two scale near `max_dev` (exact exponent shift, zero mantissa rounding).
    let scale = power_of_two_near(max_dev);

    let mut paired: Vec<([f64; 2], usize)> = Vec::with_capacity(n);
    for (i, &[x, y]) in points.iter().enumerate() {
        paired.push(([(x - mx) / scale, (y - my) / scale], i));
    }

    // Sort lexicographically by total order on (x, y), breaking exact duplicates by original index.
    paired.sort_by(|(a, ia), (b, ib)| {
        let cx = f::total_cmp(a[0], b[0]).unwrap_or(Ordering::Equal);
        if cx != Ordering::Equal {
            return cx;
        }
        let cy = f::total_cmp(a[1], b[1]).unwrap_or(Ordering::Equal);
        if cy != Ordering::Equal {
            return cy;
        }
        ia.cmp(ib)
    });

    // Deduplicate exact equal points (keeping lowest original index).
    let mut scaled: Vec<[f64; 2]> = Vec::with_capacity(n);
    let mut orig = Vec::with_capacity(n);
    for (p, idx) in paired {
        if let Some(&prev) = scaled.last() {
            if prev[0] == p[0] && prev[1] == p[1] {
                continue;
            }
        }
        scaled.push(p);
        orig.push(idx);
    }

    if scaled.len() < min_distinct {
        return Err(FitError::TooFewPoints { n: scaled.len(), need: min_distinct });
    }

    // Numerical collinearity check (matches KRN.LSQ.TRAIT `DEFAULT_RANK_TOL = 1e-12`):
    // catches floating-point-rounded lines (e.g. `1 + 3 * (i * 0.01)`) whose 1-ULP noise
    // would otherwise pass exact `orient2d` with a huge ~1e15 m radius.
    let sx: Vec<f64> = scaled.iter().map(|p| p[0]).collect();
    let sy: Vec<f64> = scaled.iter().map(|p| p[1]).collect();
    let cxx = red::sum_sq(&sx).map_err(FitError::Num)?;
    let cyy = red::sum_sq(&sy).map_err(FitError::Num)?;
    let cxy = red::dot(&sx, &sy).map_err(FitError::Num)?;
    let tr = cxx + cyy;
    let disc = f::hypot(cxx - cyy, 2.0 * cxy).map_err(FitError::Num)?;
    let lam_max = 0.5 * (tr + disc);
    let lam_min = 0.5 * (tr - disc);
    let rank_tol_sq = crate::contract::DEFAULT_RANK_TOL * crate::contract::DEFAULT_RANK_TOL;
    if lam_max <= 0.0 || lam_min <= lam_max * rank_tol_sq {
        let cond = if lam_min <= 0.0 { f64::INFINITY } else { f::sqrt(lam_max / lam_min).unwrap_or(f64::INFINITY) };
        return Err(FitError::Collinear { n, rank: 1, cond });
    }

    Ok(PreparedPoints { mean: [mx, my], scale, scaled, orig })
}

/// Returns the power of two `2^e` with `2^e <= x < 2^(e+1)` for positive finite normal `x`.
fn power_of_two_near(x: f64) -> f64 {
    let bits = x.to_bits();
    let exp_bits = bits & 0x7ff0_0000_0000_0000_u64;
    if exp_bits == 0 {
        1.0
    } else {
        f64::from_bits(exp_bits)
    }
}

fn unscale_voronoi(vor: &mut VoronoiDiagram, mean: [f64; 2], scale: f64) {
    for v in &mut vor.vertices {
        v.x = mean[0] + v.x * scale;
        v.y = mean[1] + v.y * scale;
    }
}

// ------------------------------------------------------------------ candidate search helpers

#[derive(Debug, Clone, Copy)]
struct BestCandidate {
    width: f64,
    r_min: f64,
    r_max: f64,
    cx: f64,
    cy: f64,
    orig_cx: f64,
    orig_cy: f64,
}

fn consider_candidate(
    best: &mut Option<BestCandidate>,
    c: [f64; 2],
    all_pts: &[[f64; 2]],
    hull_pts: &[[f64; 2]],
    mean: [f64; 2],
    scale: f64,
) -> Result<(), FitError> {
    if !c[0].is_finite() || !c[1].is_finite() {
        return Ok(());
    }
    // Use cancellation-free key `|p|^2 - 2 (p . c)` to locate the exact farthest and nearest
    // points even when `|c| >> 1`, then evaluate `r_max - r_min` without catastrophic cancellation.
    let mut max_key = f64::NEG_INFINITY;
    let mut max_pt = hull_pts[0];
    for &hp in hull_pts {
        let k = (hp[0] * hp[0] + hp[1] * hp[1]) - 2.0 * (hp[0] * c[0] + hp[1] * c[1]);
        if !k.is_finite() {
            return Ok(());
        }
        if k > max_key {
            max_key = k;
            max_pt = hp;
        }
    }
    let mut min_key = f64::INFINITY;
    let mut min_pt = all_pts[0];
    for &pt in all_pts {
        let k = (pt[0] * pt[0] + pt[1] * pt[1]) - 2.0 * (pt[0] * c[0] + pt[1] * c[1]);
        if !k.is_finite() {
            return Ok(());
        }
        if k < min_key {
            min_key = k;
            min_pt = pt;
        }
    }
    let r_max = f::hypot(max_pt[0] - c[0], max_pt[1] - c[1]).map_err(FitError::Num)?;
    let r_min = f::hypot(min_pt[0] - c[0], min_pt[1] - c[1]).map_err(FitError::Num)?;
    let denom = r_max + r_min;
    let width = if denom > 0.0 {
        let diff_sq = ((max_pt[0] - min_pt[0]) * (max_pt[0] + min_pt[0] - 2.0 * c[0]))
            + ((max_pt[1] - min_pt[1]) * (max_pt[1] + min_pt[1] - 2.0 * c[1]));
        let w = diff_sq / denom;
        if w < 0.0 {
            0.0
        } else {
            w
        }
    } else {
        0.0
    };
    let orig_cx = mean[0] + c[0] * scale;
    let orig_cy = mean[1] + c[1] * scale;
    if !orig_cx.is_finite() || !orig_cy.is_finite() || !width.is_finite() {
        return Ok(());
    }

    let cand = BestCandidate { width, r_min, r_max, cx: c[0], cy: c[1], orig_cx, orig_cy };
    let replace = match best {
        None => true,
        Some(cur) => compare_candidates(&cand, cur)? == Ordering::Less,
    };
    if replace {
        *best = Some(cand);
    }
    Ok(())
}

/// Total order on candidates (REQ-KRN.MZ.VOR-003): width first, then total order on centre `(cx, cy)`.
fn compare_candidates(a: &BestCandidate, b: &BestCandidate) -> Result<Ordering, FitError> {
    let cw = f::total_cmp(a.width, b.width).map_err(FitError::Num)?;
    if cw != Ordering::Equal {
        return Ok(cw);
    }
    let cx = f::total_cmp(a.orig_cx, b.orig_cx).map_err(FitError::Num)?;
    if cx != Ordering::Equal {
        return Ok(cx);
    }
    f::total_cmp(a.orig_cy, b.orig_cy).map_err(FitError::Num)
}

#[derive(Debug, Clone, Copy)]
struct EdgeBox {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

const RAY_BOX_BOUND: f64 = 1.0e100;
const BOX_PAD: f64 = 1.0e-7;

fn build_edge_boxes(vor: &VoronoiDiagram) -> Vec<EdgeBox> {
    let mut out = Vec::with_capacity(vor.edges.len());
    for e in &vor.edges {
        let b = match e.kind {
            VoronoiEdgeKind::Segment { start, end } => {
                let a = vor.vertices[start];
                let b = vor.vertices[end];
                let min_x = f::total_min(a.x, b.x).unwrap_or(a.x) - BOX_PAD;
                let max_x = f::total_max(a.x, b.x).unwrap_or(a.x) + BOX_PAD;
                let min_y = f::total_min(a.y, b.y).unwrap_or(a.y) - BOX_PAD;
                let max_y = f::total_max(a.y, b.y).unwrap_or(a.y) + BOX_PAD;
                EdgeBox { min_x, max_x, min_y, max_y }
            }
            VoronoiEdgeKind::Ray { start, dir } => {
                let a = vor.vertices[start];
                let (min_x, max_x) = if dir[0] > 0.0 {
                    (a.x - BOX_PAD, RAY_BOX_BOUND)
                } else if dir[0] < 0.0 {
                    (-RAY_BOX_BOUND, a.x + BOX_PAD)
                } else {
                    (a.x - BOX_PAD, a.x + BOX_PAD)
                };
                let (min_y, max_y) = if dir[1] > 0.0 {
                    (a.y - BOX_PAD, RAY_BOX_BOUND)
                } else if dir[1] < 0.0 {
                    (-RAY_BOX_BOUND, a.y + BOX_PAD)
                } else {
                    (a.y - BOX_PAD, a.y + BOX_PAD)
                };
                EdgeBox { min_x, max_x, min_y, max_y }
            }
        };
        out.push(b);
    }
    out
}

fn cmp_box_min_x(a: &EdgeBox, b: &EdgeBox, ia: usize, ib: usize) -> Ordering {
    let c = f::total_cmp(a.min_x, b.min_x).unwrap_or(Ordering::Equal);
    if c != Ordering::Equal {
        c
    } else {
        ia.cmp(&ib)
    }
}

/// Intersects the perpendicular bisector of `(u, v)` with the perpendicular bisector of `(p, q)`.
fn intersect_bisectors(u: [f64; 2], v: [f64; 2], p: [f64; 2], q: [f64; 2]) -> Option<[f64; 2]> {
    let a1 = 2.0 * (v[0] - u[0]);
    let b1 = 2.0 * (v[1] - u[1]);
    let rhs1 = (v[0] - u[0]) * (v[0] + u[0]) + (v[1] - u[1]) * (v[1] + u[1]);

    let a2 = 2.0 * (q[0] - p[0]);
    let b2 = 2.0 * (q[1] - p[1]);
    let rhs2 = (q[0] - p[0]) * (q[0] + p[0]) + (q[1] - p[1]) * (q[1] + p[1]);

    let det = a1 * b2 - a2 * b1;
    if det.abs() <= 1.0e-24 || !det.is_finite() {
        return None;
    }
    let cx = (rhs1 * b2 - rhs2 * b1) / det;
    let cy = (a1 * rhs2 - a2 * rhs1) / det;
    if cx.is_finite() && cy.is_finite() {
        Some([cx, cy])
    } else {
        None
    }
}

/// Checks whether a point `c` on the supporting bisector line lies within `edge` (segment or ray).
fn point_on_vor_edge(c: [f64; 2], edge: &VoronoiEdge, vertices: &[VoronoiVertex]) -> bool {
    const PARAM_TOL: f64 = 1.0e-7;
    match edge.kind {
        VoronoiEdgeKind::Segment { start, end } => {
            let a = vertices[start];
            let b = vertices[end];
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let len2 = dx * dx + dy * dy;
            if len2 == 0.0 {
                let d2 = (c[0] - a.x) * (c[0] - a.x) + (c[1] - a.y) * (c[1] - a.y);
                return d2 <= 1.0e-14;
            }
            let t = ((c[0] - a.x) * dx + (c[1] - a.y) * dy) / len2;
            (-PARAM_TOL..=1.0 + PARAM_TOL).contains(&t)
        }
        VoronoiEdgeKind::Ray { start, dir } => {
            let a = vertices[start];
            let t = (c[0] - a.x) * dir[0] + (c[1] - a.y) * dir[1];
            t >= -PARAM_TOL
        }
    }
}
