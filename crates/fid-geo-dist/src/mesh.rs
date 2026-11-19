//! Point-to-triangle and point-to-mesh closest point. Spec node `GEO.DIST.MESH`.
//!
//! [`closest_on_triangle`] returns the foot point on one triangle, its squared distance and
//! the [`Feature`] the foot lies on. [`MeshTarget`] (or the one-shot [`closest_on_mesh`])
//! searches a whole [`Mesh`] through its [`PrimitiveBvh`] and returns a [`MeshProjection`]:
//! the closest triangle, foot, distance and feature, or `NoHit` beyond a maximum distance.
//! [`MeshTarget::closest_brute`] is the same search over every triangle with no index; the
//! indexed search returns bit-identical results (see "Why pruning loses nothing").
//!
//! Every value-affecting choice below is pinned. The independent reference
//! `tools/fixtures/geo_dist.py` models it bit for bit in Python floats and checks it against
//! exact rational arithmetic (`fractions`).
//!
//! # Notation (pinned)
//!
//! `sub(x, y) = [x0 - y0, x1 - y1, x2 - y2]`; `dot(x, y) = (x0*y0 + x1*y1) + x2*y2`;
//! `cross(x, y) = [x1*y2 - x2*y1, x2*y0 - x0*y2, x0*y1 - x1*y0]`; `axpy(s, t, e) = s_k + t*e_k`
//! per axis. No fused multiply-add, no `f64::min`/`f64::max` (KNP-1).
//!
//! # Input domain
//!
//! Every query coordinate and triangle corner is finite and at most [`COORD_LIMIT`] `= 2^250`
//! in magnitude. Then every difference is below `2^251`, every dot or cross product below
//! `2^504` and every product of two of them below `2^1010`, so nothing overflows and no NaN can
//! arise. A non-finite query is [`DistError::NonFiniteQuery`]; a finite one beyond the limit is
//! [`DistError::QueryOutOfRange`]; a bad corner is [`DistError::BadCorner`] (single triangle) or
//! [`DistError::BadTriangle`] (mesh).
//!
//! There is no lower bound on the input, but the accuracy bound below assumes no underflow:
//! every non-zero edge length (and query offset) of at least `2^-240` m, so the quartic gate
//! terms (`2^-40 * |e|^4 >= 2^-1000`) and numerators stay normal. Below that the gate or a
//! numerator may underflow to zero; results stay finite and NaN-free and the foot stays in the
//! triangle's box, so the distance error is at most the box diagonal (that is, below
//! `2^-238` m), but the feature and the relative bound are not guaranteed.
//!
//! # Point to triangle (pinned)
//!
//! Corners `a, b, c` are vertex 0, 1, 2; edge 0 is `a-b`, edge 1 is `b-c`, edge 2 is `c-a`.
//!
//! 1. `ab = sub(b, a)`, `ac = sub(c, a)`, `bc = sub(c, b)`, `n = cross(ab, ac)`,
//!    `nn = dot(n, n)`; `la = dot(ab, ab)`, `lc = dot(ac, ac)`, `lb = dot(bc, bc)`;
//!    `m = la*lc`, replaced by `la*lb` if that is `>`, then by `lc*lb` if that is `>`. If
//!    `nn <= GATE * m` with [`DEGENERACY_GATE`] `= 2^-40`, use the segment path (step 3). The
//!    gate is symmetric in the corners: `nn / m` is the squared sine of the triangle's smallest
//!    angle, so zero area, collinear or repeated corners and any sliver with an angle of
//!    `sin^2 <= 2^-40` at any corner take the segment path.
//! 2. Voronoi regions, after Ericson, *Real-Time Collision Detection*, §5.1.5 (cited, not
//!    copied), tested in this order; the barycentric numerators are triple products of the
//!    edge vectors, `vc = dot(n, cross(ab, ap))`, `vb = dot(n, cross(ap, ac))`,
//!    `va = dot(n, cross(bc, bp))` (equal in exact arithmetic to Ericson's 2x2 determinants).
//!    Their relative rounding error still grows like `u / sin(smallest angle)`; the gate caps
//!    that growth and the accuracy bound below states it:
//!    - `ap = sub(p, a)`, `d1 = dot(ab, ap)`, `d2 = dot(ac, ap)`; `d1 <= 0 && d2 <= 0`:
//!      vertex 0, foot `a`.
//!    - `bp = sub(p, b)`, `d3 = dot(ab, bp)`, `d4 = dot(ac, bp)`; `d3 >= 0 && d4 <= d3`:
//!      vertex 1, foot `b`.
//!    - `vc <= 0 && d1 >= 0 && d3 <= 0`: edge 0, `t = d1 / (d1 - d3)`, foot `axpy(a, t, ab)`.
//!    - `cp = sub(p, c)`, `d5 = dot(ab, cp)`, `d6 = dot(ac, cp)`; `d6 >= 0 && d5 <= d6`:
//!      vertex 2, foot `c`.
//!    - `vb <= 0 && d2 >= 0 && d6 <= 0`: edge 2, `t = d2 / (d2 - d6)`, foot `axpy(a, t, ac)`.
//!    - `e1 = d4 - d3`, `e2 = d5 - d6`; `va <= 0 && e1 >= 0 && e2 >= 0`: edge 1,
//!      `t = e1 / (e1 + e2)`, foot `axpy(b, t, bc)`.
//!    - Otherwise interior: `den = (va + vb) + vc`, `v = vb / den`, `w = vc / den`, foot
//!      `(a_k + ab_k*v) + ac_k*w`.
//!
//!    Guards: an edge whose denominator is not `> 0`, or an interior branch where `va`, `vb`
//!    or `vc` is not `> 0`, can only arise from rounding near a degenerate configuration; the
//!    whole triangle then takes the segment path. So no division is by zero and every `t`
//!    lies in `[0, 1]`.
//! 3. Segment path: for edges 0, 1, 2 in order with start `s`, end `e` (`a-b`, `b-c`, `c-a`):
//!    `ev = sub(e, s)`, `tn = dot(ev, sub(p, s))`; `tn <= 0`: the start vertex; else
//!    `ee = dot(ev, ev)`; `tn >= ee`: the end vertex; else edge, `t = tn / ee`, foot
//!    `axpy(s, t, ev)`. Each candidate is finished as in step 4; the smallest squared
//!    distance wins, ties to the lowest [`Feature::code`]. A coincident edge (`ee = 0`) gives
//!    `tn = 0`, its start vertex, so nothing divides by zero.
//! 4. Finish: the foot is clamped per axis into the triangle's box (corner minimum and
//!    maximum by `<` and `>`), `dp = sub(p, foot)`, `dist2 = dot(dp, dp)`.
//!
//! On region boundaries the test order above gives the lowest feature code
//! (vertex 0 < 1 < 2 < edge 0 < 1 < 2 < interior), which is also the rule in exact arithmetic:
//! the reported feature is the lowest-coded closed feature that contains the exact foot.
//!
//! # Accuracy bound (pinned, checked by the fixtures)
//!
//! With `S` the largest coordinate magnitude of `p, a, b, c`, `L` the longest edge and
//! `sigma = |ab x ac|^2 / max(|ab|^2 |ac|^2, |ab|^2 |bc|^2, |ac|^2 |bc|^2)` (exact; the squared
//! sine of the smallest angle), the distance differs from the exact distance to the closed
//! triangle by at most `2^-46 * max(S, L)`, plus `2^-19 * L` when `0 < sigma <= 2^-39` (segment
//! path for a sliver: its width is at most `2^-20 L`), or plus `2^-46 * L / sqrt(sigma)` when
//! `sigma > 2^-39` (Voronoi path). The bound holds for every order of the corners; the reference
//! asserts it for all six orders of every fuzz triangle.
//!
//! # Point to mesh (pinned)
//!
//! The result is the triangle with the smallest `dist2`, ties to the lowest triangle index
//! (REQ-GEO.DIST.MESH-004; the feature code would come next, but one triangle has one
//! result). With a maximum distance `d` (finite, `>= 0`), the result is `NoHit` when that
//! smallest `dist2 > d*d`. With a subset, only its triangles count; an empty subset, or a
//! subset none of whose triangles lies within `d`, gives `NoHit`. `dist` is the correctly
//! rounded square root of `dist2` (`fid_num::f::sqrt`).
//!
//! # Why pruning loses nothing
//!
//! The indexed search visits leaves by ascending lower bound `lb` and stops at the first leaf
//! with `lb > best dist2` or `lb > d*d`. The clamped foot lies in the triangle's box, which
//! [`MeshTarget::new`] checks lies in the box the hierarchy stores. So per axis
//! `|p_k - foot_k| >= ` the box gap, and as rounding is monotone the computed `dist2` is at
//! least the hierarchy's pinned squared box distance, which is at least `lb`. A triangle in a
//! skipped leaf therefore has `dist2 > best`, cannot win or tie, and the result equals the
//! brute-force scan bit for bit. Equal-`lb` leaves are still visited, so ties are kept.

use core::fmt;
use core::ops::ControlFlow;

use fid_geo_index::{BvhError, PrimitiveBvh};
use fid_geo_types::Mesh;

/// Largest accepted magnitude of a query coordinate or triangle corner: `2^250` metres.
pub const COORD_LIMIT: f64 = f64::from_bits((1023 + 250) << 52);

/// Relative squared-area threshold below which a triangle takes the segment path: `2^-40`.
pub const DEGENERACY_GATE: f64 = f64::from_bits((1023 - 40) << 52);

type V3 = [f64; 3];

// ------------------------------------------------------------------ types

/// The feature of a triangle the foot point lies on. Indexes: vertex `i` is corner `i`
/// (`a, b, c`); edge 0 is `a-b`, edge 1 `b-c`, edge 2 `c-a`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    /// The open interior of the triangle.
    Interior,
    /// The open edge with this index (0, 1 or 2).
    Edge(u8),
    /// The vertex with this index (0, 1 or 2).
    Vertex(u8),
}

impl Feature {
    /// The tie-break code: vertex `i` is `i`, edge `i` is `3 + i`, interior is 6.
    pub fn code(self) -> u8 {
        match self {
            Feature::Vertex(i) => i,
            Feature::Edge(i) => 3u8.saturating_add(i),
            Feature::Interior => 6,
        }
    }
}

/// The closest point on one triangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TriangleFoot {
    /// The foot point.
    pub foot: [f64; 3],
    /// The squared distance from the query point to `foot`.
    pub dist2: f64,
    /// The feature the foot lies on.
    pub feature: Feature,
}

/// The closest point on a mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshHit {
    /// The foot point.
    pub foot: [f64; 3],
    /// The unsigned distance, `sqrt(dist2)` correctly rounded.
    pub dist: f64,
    /// The squared distance as computed (the value the tie rule compares).
    pub dist2: f64,
    /// The reference triangle index.
    pub tri: u64,
    /// The feature of `tri` the foot lies on.
    pub feature: Feature,
}

/// The result of a mesh projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MeshProjection {
    /// The closest point.
    Hit(MeshHit),
    /// No (allowed) triangle lies within the maximum distance.
    NoHit,
}

/// A validated triangle subset: strictly ascending triangle indexes below the mesh's
/// triangle count. Build it once and reuse it for many queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriangleSubset<'a> {
    ids: &'a [u64],
    triangle_count: u64,
}

impl<'a> TriangleSubset<'a> {
    /// Checks that `ids` is strictly ascending and every index is below `triangle_count`.
    pub fn new(ids: &'a [u64], triangle_count: u64) -> Result<TriangleSubset<'a>, DistError> {
        let mut prev: Option<u64> = None;
        for (i, &t) in ids.iter().enumerate() {
            let ok = t < triangle_count && prev.map_or(true, |q| q < t);
            if !ok {
                return Err(DistError::InvalidSubset { position: i as u64 });
            }
            prev = Some(t);
        }
        Ok(TriangleSubset { ids, triangle_count })
    }

    /// The triangle indexes, ascending.
    pub fn ids(&self) -> &'a [u64] {
        self.ids
    }

    fn contains(&self, t: u64) -> bool {
        self.ids.binary_search(&t).is_ok()
    }
}

/// Search options for a mesh projection.
#[derive(Debug, Clone, Copy, Default)]
pub struct MeshQuery<'s, 'a> {
    /// Maximum search distance in metres (finite, `>= 0`); `None` searches without limit.
    pub max_distance: Option<f64>,
    /// Restrict the search to these triangles.
    pub subset: Option<&'s TriangleSubset<'a>>,
}

/// Every error of this module. Errors are values; nothing panics.
#[derive(Debug, Clone, PartialEq)]
pub enum DistError {
    /// A query coordinate is NaN or infinite.
    NonFiniteQuery,
    /// A query coordinate exceeds [`COORD_LIMIT`] in magnitude.
    QueryOutOfRange,
    /// A corner of the single triangle is non-finite or exceeds [`COORD_LIMIT`].
    BadCorner {
        /// Corner index (0, 1, 2).
        corner: u8,
    },
    /// A corner of a mesh triangle is non-finite or exceeds [`COORD_LIMIT`].
    BadTriangle {
        /// Triangle index.
        triangle: u64,
        /// Corner index (0, 1, 2).
        corner: u8,
    },
    /// The maximum distance is NaN, infinite or negative.
    InvalidMaxDistance,
    /// The subset is not strictly ascending or names a triangle out of range.
    InvalidSubset {
        /// Position of the first offending entry.
        position: u64,
    },
    /// The subset was built for a different triangle count.
    SubsetMismatch,
    /// The hierarchy does not index this mesh (frame, triangle count, or a triangle box that
    /// does not contain the triangle).
    IndexMismatch {
        /// The first offending triangle, when the mismatch is per triangle.
        triangle: Option<u64>,
    },
    /// The hierarchy refused a query.
    Bvh(BvhError),
    /// An internal invariant failed (never expected).
    Inconsistent,
}

impl DistError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            DistError::NonFiniteQuery => "non_finite_query",
            DistError::QueryOutOfRange => "query_out_of_range",
            DistError::BadCorner { .. } => "bad_corner",
            DistError::BadTriangle { .. } => "bad_triangle",
            DistError::InvalidMaxDistance => "invalid_max_distance",
            DistError::InvalidSubset { .. } => "invalid_subset",
            DistError::SubsetMismatch => "subset_mismatch",
            DistError::IndexMismatch { .. } => "index_mismatch",
            DistError::Bvh(_) => "bvh",
            DistError::Inconsistent => "inconsistent",
        }
    }
}

impl fmt::Display for DistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DistError::NonFiniteQuery => write!(f, "query point is not finite"),
            DistError::QueryOutOfRange => write!(f, "query coordinate exceeds 2^250 m"),
            DistError::BadCorner { corner } => write!(f, "triangle corner {corner} is non-finite or exceeds 2^250 m"),
            DistError::BadTriangle { triangle, corner } => {
                write!(f, "corner {corner} of triangle {triangle} is non-finite or exceeds 2^250 m")
            }
            DistError::InvalidMaxDistance => write!(f, "maximum distance must be finite and >= 0"),
            DistError::InvalidSubset { position } => {
                write!(f, "triangle subset entry {position} is out of range or not strictly ascending")
            }
            DistError::SubsetMismatch => write!(f, "triangle subset was built for another mesh"),
            DistError::IndexMismatch { triangle: Some(t) } => {
                write!(f, "hierarchy box of triangle {t} does not contain the triangle")
            }
            DistError::IndexMismatch { triangle: None } => {
                write!(f, "hierarchy frame or triangle count differs from the mesh")
            }
            DistError::Bvh(e) => write!(f, "hierarchy: {e}"),
            DistError::Inconsistent => write!(f, "internal invariant failed"),
        }
    }
}

impl std::error::Error for DistError {}

// ------------------------------------------------------------------ arithmetic

#[inline]
fn sub(x: &V3, y: &V3) -> V3 {
    [x[0] - y[0], x[1] - y[1], x[2] - y[2]]
}

#[inline]
fn dot(x: &V3, y: &V3) -> f64 {
    (x[0] * y[0] + x[1] * y[1]) + x[2] * y[2]
}

#[inline]
fn cross(x: &V3, y: &V3) -> V3 {
    [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]]
}

#[inline]
fn axpy(s: &V3, t: f64, e: &V3) -> V3 {
    [s[0] + t * e[0], s[1] + t * e[1], s[2] + t * e[2]]
}

fn in_range(p: &V3) -> bool {
    p.iter().all(|&x| (-COORD_LIMIT..=COORD_LIMIT).contains(&x))
}

fn check_query(p: &V3) -> Result<(), DistError> {
    if !p.iter().all(|x| x.is_finite()) {
        return Err(DistError::NonFiniteQuery);
    }
    if !in_range(p) {
        return Err(DistError::QueryOutOfRange);
    }
    Ok(())
}

// ------------------------------------------------------------------ point to triangle

/// The closest point on triangle `(a, b, c)` to `p`, with its squared distance and feature,
/// computed by the pinned rules in the [module documentation](self).
pub fn closest_on_triangle(p: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Result<TriangleFoot, DistError> {
    check_query(&p)?;
    for (k, v) in [a, b, c].iter().enumerate() {
        if !in_range(v) {
            return Err(DistError::BadCorner { corner: k as u8 });
        }
    }
    Ok(closest_unchecked(&p, &a, &b, &c))
}

fn closest_unchecked(p: &V3, a: &V3, b: &V3, c: &V3) -> TriangleFoot {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let bc = sub(c, b);
    let n = cross(&ab, &ac);
    let nn = dot(&n, &n);
    if nn <= DEGENERACY_GATE * gate_scale(&ab, &ac, &bc) {
        return segments(p, a, b, c);
    }
    let e = Edges { ab, ac, bc, n };
    match voronoi(p, a, b, c, &e) {
        Some((foot, feature)) => finish(p, a, b, c, foot, feature),
        None => segments(p, a, b, c),
    }
}

/// `max(|ab|^2 |ac|^2, |ab|^2 |bc|^2, |ac|^2 |bc|^2)`, taken in this order by `>`: the gate is
/// symmetric in the corners, so a sliver is caught whichever corner its small angle is at.
fn gate_scale(ab: &V3, ac: &V3, bc: &V3) -> f64 {
    let (la, lc, lb) = (dot(ab, ab), dot(ac, ac), dot(bc, bc));
    let mut m = la * lc;
    for x in [la * lb, lc * lb] {
        if x > m {
            m = x;
        }
    }
    m
}

/// The edge vectors and the unnormalised normal of one triangle.
struct Edges {
    ab: V3,
    ac: V3,
    bc: V3,
    n: V3,
}

fn voronoi(p: &V3, a: &V3, b: &V3, c: &V3, e: &Edges) -> Option<(V3, Feature)> {
    let (ab, ac, bc, n) = (&e.ab, &e.ac, &e.bc, &e.n);
    let ap = sub(p, a);
    let d1 = dot(ab, &ap);
    let d2 = dot(ac, &ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return Some((*a, Feature::Vertex(0)));
    }
    let bp = sub(p, b);
    let d3 = dot(ab, &bp);
    let d4 = dot(ac, &bp);
    if d3 >= 0.0 && d4 <= d3 {
        return Some((*b, Feature::Vertex(1)));
    }
    let vc = dot(n, &cross(ab, &ap));
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let den = d1 - d3;
        if den <= 0.0 {
            return None;
        }
        return Some((axpy(a, d1 / den, ab), Feature::Edge(0)));
    }
    let cp = sub(p, c);
    let d5 = dot(ab, &cp);
    let d6 = dot(ac, &cp);
    if d6 >= 0.0 && d5 <= d6 {
        return Some((*c, Feature::Vertex(2)));
    }
    let vb = dot(n, &cross(&ap, ac));
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let den = d2 - d6;
        if den <= 0.0 {
            return None;
        }
        return Some((axpy(a, d2 / den, ac), Feature::Edge(2)));
    }
    let va = dot(n, &cross(bc, &bp));
    let e1 = d4 - d3;
    let e2 = d5 - d6;
    if va <= 0.0 && e1 >= 0.0 && e2 >= 0.0 {
        let den = e1 + e2;
        if den <= 0.0 {
            return None;
        }
        return Some((axpy(b, e1 / den, bc), Feature::Edge(1)));
    }
    if va <= 0.0 || vb <= 0.0 || vc <= 0.0 {
        return None;
    }
    let den = (va + vb) + vc;
    let v = vb / den;
    let w = vc / den;
    let foot = [(a[0] + ab[0] * v) + ac[0] * w, (a[1] + ab[1] * v) + ac[1] * w, (a[2] + ab[2] * v) + ac[2] * w];
    Some((foot, Feature::Interior))
}

fn segments(p: &V3, a: &V3, b: &V3, c: &V3) -> TriangleFoot {
    let edges: [(&V3, &V3, u8, u8); 3] = [(a, b, 0, 1), (b, c, 1, 2), (c, a, 2, 0)];
    let mut best: Option<TriangleFoot> = None;
    for (k, (s, e, is, ie)) in edges.into_iter().enumerate() {
        let ev = sub(e, s);
        let tn = dot(&ev, &sub(p, s));
        let (foot, feature) = if tn <= 0.0 {
            (*s, Feature::Vertex(is))
        } else {
            let ee = dot(&ev, &ev);
            if tn >= ee {
                (*e, Feature::Vertex(ie))
            } else {
                (axpy(s, tn / ee, &ev), Feature::Edge(k as u8))
            }
        };
        let r = finish(p, a, b, c, foot, feature);
        let better = match best {
            None => true,
            Some(q) => r.dist2 < q.dist2 || (r.dist2 == q.dist2 && r.feature.code() < q.feature.code()),
        };
        if better {
            best = Some(r);
        }
    }
    // Three candidates were considered, so `best` is set; the fallback is unreachable.
    best.unwrap_or(TriangleFoot { foot: *a, dist2: dot(&sub(p, a), &sub(p, a)), feature: Feature::Vertex(0) })
}

#[allow(clippy::needless_range_loop)] // per-axis clamp over four arrays, as pinned
fn finish(p: &V3, a: &V3, b: &V3, c: &V3, foot: V3, feature: Feature) -> TriangleFoot {
    let mut f = foot;
    for k in 0..3 {
        let mut lo = a[k];
        let mut hi = a[k];
        for v in [b[k], c[k]] {
            if v < lo {
                lo = v;
            }
            if v > hi {
                hi = v;
            }
        }
        if f[k] < lo {
            f[k] = lo;
        } else if f[k] > hi {
            f[k] = hi;
        }
    }
    let dp = sub(p, &f);
    TriangleFoot { foot: f, dist2: dot(&dp, &dp), feature }
}

// ------------------------------------------------------------------ point to mesh

/// A mesh and its hierarchy, checked once for many queries.
#[derive(Debug, Clone, Copy)]
pub struct MeshTarget<'m> {
    mesh: &'m Mesh,
    bvh: &'m PrimitiveBvh,
}

impl<'m> MeshTarget<'m> {
    /// Checks, in order: the hierarchy's frame and triangle count equal the mesh's
    /// ([`DistError::IndexMismatch`] with no triangle); then per triangle in index order, per
    /// corner, the corner is within [`COORD_LIMIT`] ([`DistError::BadTriangle`]) and lies in
    /// the box the hierarchy stores for that triangle ([`DistError::IndexMismatch`]).
    /// Cost `O(triangles)`.
    pub fn new(mesh: &'m Mesh, bvh: &'m PrimitiveBvh) -> Result<MeshTarget<'m>, DistError> {
        if bvh.frame() != mesh.frame() || bvh.triangle_count() != mesh.triangle_count() {
            return Err(DistError::IndexMismatch { triangle: None });
        }
        for t in 0..mesh.triangle_count() {
            let corners = corners_checked(mesh, t)?;
            let bx = bvh.triangle_aabb(t).ok_or(DistError::Inconsistent)?;
            if !corners.iter().all(|v| bx.contains(*v)) {
                return Err(DistError::IndexMismatch { triangle: Some(t) });
            }
        }
        Ok(MeshTarget { mesh, bvh })
    }

    /// The mesh.
    pub fn mesh(&self) -> &'m Mesh {
        self.mesh
    }

    /// The closest point on the mesh, searched through the hierarchy with pruning.
    pub fn closest(&self, p: [f64; 3], q: &MeshQuery<'_, '_>) -> Result<MeshProjection, DistError> {
        let (limit2, subset) = self.prepare(&p, q)?;
        let mut best: Option<(TriangleFoot, u64)> = None;
        let mut fail: Option<DistError> = None;
        self.bvh
            .ordered_visit(p, |leaf| {
                let lb = leaf.lower_bound_sq();
                if limit2.is_some_and(|l2| lb > l2) || best.is_some_and(|(r, _)| lb > r.dist2) {
                    return ControlFlow::Break(());
                }
                for t in leaf.triangles() {
                    if subset.is_some_and(|s| !s.contains(t)) {
                        continue;
                    }
                    match corners(self.mesh, t) {
                        Ok([a, b, c]) => consider(&mut best, closest_unchecked(&p, &a, &b, &c), t),
                        Err(e) => {
                            fail = Some(e);
                            return ControlFlow::Break(());
                        }
                    }
                }
                ControlFlow::Continue(())
            })
            .map_err(DistError::Bvh)?;
        if let Some(e) = fail {
            return Err(e);
        }
        conclude(best, limit2)
    }

    /// The same result as [`MeshTarget::closest`], by scanning every (allowed) triangle in
    /// ascending index order without the hierarchy. The reference for tests.
    pub fn closest_brute(&self, p: [f64; 3], q: &MeshQuery<'_, '_>) -> Result<MeshProjection, DistError> {
        let (limit2, subset) = self.prepare(&p, q)?;
        let mut best: Option<(TriangleFoot, u64)> = None;
        let mut one = |t: u64| -> Result<(), DistError> {
            let [a, b, c] = corners(self.mesh, t)?;
            consider(&mut best, closest_unchecked(&p, &a, &b, &c), t);
            Ok(())
        };
        match subset {
            Some(s) => s.ids().iter().try_for_each(|&t| one(t))?,
            None => (0..self.mesh.triangle_count()).try_for_each(&mut one)?,
        }
        conclude(best, limit2)
    }

    fn prepare<'s, 'a>(
        &self,
        p: &V3,
        q: &MeshQuery<'s, 'a>,
    ) -> Result<(Option<f64>, Option<&'s TriangleSubset<'a>>), DistError> {
        check_query(p)?;
        let limit2 = match q.max_distance {
            None => None,
            Some(d) if d.is_finite() && d >= 0.0 => Some(d * d),
            Some(_) => return Err(DistError::InvalidMaxDistance),
        };
        if let Some(s) = q.subset {
            if s.triangle_count != self.mesh.triangle_count() {
                return Err(DistError::SubsetMismatch);
            }
        }
        Ok((limit2, q.subset))
    }
}

/// One-shot form of [`MeshTarget::new`] followed by [`MeshTarget::closest`]. The check is
/// `O(triangles)`; for many queries build a [`MeshTarget`] once.
pub fn closest_on_mesh(
    p: [f64; 3],
    mesh: &Mesh,
    bvh: &PrimitiveBvh,
    q: &MeshQuery<'_, '_>,
) -> Result<MeshProjection, DistError> {
    MeshTarget::new(mesh, bvh)?.closest(p, q)
}

fn corners(mesh: &Mesh, t: u64) -> Result<[V3; 3], DistError> {
    let tri = mesh.triangle(t).ok_or(DistError::Inconsistent)?;
    let mut c = [[0.0; 3]; 3];
    for (slot, &v) in c.iter_mut().zip(tri.iter()) {
        *slot = mesh.vertex(v).ok_or(DistError::Inconsistent)?;
    }
    Ok(c)
}

fn corners_checked(mesh: &Mesh, t: u64) -> Result<[V3; 3], DistError> {
    let c = corners(mesh, t)?;
    for (k, v) in c.iter().enumerate() {
        if !in_range(v) {
            return Err(DistError::BadTriangle { triangle: t, corner: k as u8 });
        }
    }
    Ok(c)
}

fn consider(best: &mut Option<(TriangleFoot, u64)>, r: TriangleFoot, t: u64) {
    let better = match best {
        None => true,
        Some((b, bt)) => r.dist2 < b.dist2 || (r.dist2 == b.dist2 && t < *bt),
    };
    if better {
        *best = Some((r, t));
    }
}

fn conclude(best: Option<(TriangleFoot, u64)>, limit2: Option<f64>) -> Result<MeshProjection, DistError> {
    let Some((r, tri)) = best else {
        return Ok(MeshProjection::NoHit);
    };
    if limit2.is_some_and(|l2| r.dist2 > l2) {
        return Ok(MeshProjection::NoHit);
    }
    let dist = fid_num::f::sqrt(r.dist2).map_err(|_| DistError::Inconsistent)?;
    Ok(MeshProjection::Hit(MeshHit { foot: r.foot, dist, dist2: r.dist2, tri, feature: r.feature }))
}
