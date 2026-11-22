//! Triangle bounding volume hierarchy. Spec node `GEO.INDEX.BVH`.
//!
//! [`PrimitiveBvh`] indexes the axis-aligned bounding boxes of triangles (from a measured
//! [`Mesh`] or from a CAD tessellation given as corner triples) and answers sphere, box and ray
//! *candidate* queries: every triangle whose box meets the query is returned (no false
//! negatives), sorted by ascending triangle index. Exact triangle distances are `GEO.DIST`'s job.
//!
//! Every value-affecting choice below is pinned in the leaf's `NODE.md` (Design) and modelled
//! bit for bit by the independent reference `fixtures/geo/index/make_bvh_fixtures.py`.
//!
//! # Input domain (refused with typed errors, first offender wins)
//!
//! Build: `threads >= 1` ([`BvhError::ZeroThreads`]); at least one triangle
//! ([`BvhError::EmptyMesh`]); at most [`MAX_TRIANGLES`]; one payload ID per triangle. Then per
//! triangle in index order, per corner, per axis: the coordinate is finite
//! ([`BvhError::NonFinitePrimitive`]) and `|x| <= COORD_LIMIT` = 2^500
//! ([`BvhError::PrimitiveOutOfRange`]). Degenerate triangles (slivers, collinear or coincident
//! corners, repeated vertex indexes, common in STL meshes) are accepted: their boxes are well
//! defined, so they are indexed and returned like any other triangle.
//!
//! Queries: every coordinate, radius and ray parameter is finite ([`BvhError::NonFiniteQuery`])
//! and at most `COORD_LIMIT` in magnitude ([`BvhError::QueryOutOfRange`]), including both ends
//! of the ray's `t` range (an infinite `t_max` is refused, not treated as "unbounded"). Then:
//! radius `>= 0`; box `min <= max` per axis; ray direction not the zero vector; `t_min <= t_max`.
//! The limit keeps every intermediate (differences, squares, sums of three squares) finite, so
//! no test can overflow to infinity or produce NaN.
//!
//! # Build (pinned)
//!
//! - Triangle box: per axis the totalOrder minimum and maximum of the three corners.
//! - Split key: `(a + b) + c` per axis (three times the centroid, corner order a, b, c).
//! - Breadth-first: node 0 is the root over all triangles; nodes are processed in index order.
//!   A node over at most [`LEAF_SIZE`] triangles is a leaf; its triangles are stored in
//!   ascending index order. Otherwise the split axis is the axis of largest extent
//!   `max - min` of the node box, the lowest axis on ties; the node's triangles are ordered by
//!   (key on that axis, triangle index) with keys compared numerically (`-0.0 == +0.0`); the
//!   first `count / 2` go to the left child and the rest to the right; the two children get the
//!   next two node indexes (left, then right). Node boxes are totalOrder unions.
//! - The result is a pure function of the input; the build is sequential (KNP-1 forbids threads
//!   in the kernel), so it is the same for every `threads` value (REQ-GEO.INDEX.BVH-004).
//!
//! # Per-box tests (pinned; `ε = 2^-52`, `C4 = 4ε = 2^-50`, `TINY = 2^-1022`)
//!
//! - Box query: `lo_k <= qmax_k && qmin_k <= hi_k` on all axes. Comparisons are exact.
//! - Sphere `(c, r)`: per axis `g = lo - c` if `c < lo`, `c - hi` if `c > hi`, else `0`;
//!   `d2 = (gx*gx + gy*gy) + gz*gz`; hit iff `d2 <= (r*r)*(1 + C4) + TINY`.
//! - Ray `o + t d`, `t` in `[t_min, t_max]`: start `a = t_min`, `b = t_max`; per axis in order
//!   x, y, z: if `d_k == 0`, miss unless `lo_k <= o_k <= hi_k`; else `ta = (lo_k - o_k) / d_k`,
//!   `tb = (hi_k - o_k) / d_k`, `(near, far) = (ta, tb)` if `ta <= tb` else `(tb, ta)`,
//!   `a = totalmax(a, down(near))`, `b = totalmin(b, up(far))`; hit iff `a <= b`. With
//!   `up(t) = t + (|t|*C4 + TINY)` and `down(t) = t - (|t|*C4 + TINY)` for finite `t`, and
//!   `up(t) = down(t) = t` for `t = ±∞` (a quotient may overflow when `d_k` is tiny).
//! - Ordered-visit lower bound of a box: `lb = d2*(1 - C4) - TINY`, replaced by `+0.0` if `<= 0`.
//!
//! Why conservative (REQ-GEO.INDEX.BVH-002): `d2` carries at most 5 roundings (relative
//! `5u`, `u = 2^-53`), the threshold `(r*r)*(1 + C4)` at least `r^2 (1 + 8u)(1 - u)^2`, so a box
//! within exact distance `r` always passes; `t` quotients carry 2 roundings (`2u`) and the
//! widening adds at least `|t| (8u)(1 - u)^2`; `TINY` covers the absolute error of underflow.
//! Box tests are exact. The rational-arithmetic oracle checks this on every grazing case.
//!
//! Why the hierarchy loses nothing (REQ-GEO.INDEX.BVH-001): every test is monotone in the box
//! (rounding is monotone), so a node box containing a triangle box passes whenever the triangle
//! box passes. A query therefore returns exactly the brute-force set of the same test.
//!
//! # Bounds
//!
//! No recursion. Queries use a fixed 64-entry stack (depth is at most 32 for `2^31`
//! triangles, as every split halves the count); the ordered visitor uses a binary heap of at
//! most the node count. Every allocation sized from input is fallible
//! ([`BvhError::AllocationFailed`]). Nothing panics.

// Per-axis loops index several 3-arrays at once; written as index loops to mirror the pinned
// formulas in NODE.md.
#![allow(clippy::needless_range_loop)]

use core::cmp::{Ordering, Reverse};
use core::fmt;
use core::ops::ControlFlow;
use std::collections::BinaryHeap;

use fid_geo_frame::{Transform, XformError};
use fid_geo_types::{Aabb, FrameRef, Mesh};

/// Largest number of triangles in a leaf.
pub const LEAF_SIZE: usize = 8;
/// Largest triangle count (node and position indexes are `u32`).
pub const MAX_TRIANGLES: u64 = 1 << 31;
/// The relative inflation of the rounding-sensitive tests: 4 units of f64 roundoff, `4 ε = 2^-50`.
pub const ROUNDOFF_MARGIN: f64 = 4.0 * f64::EPSILON;
/// The absolute inflation that covers underflow: the smallest positive normal, `2^-1022`.
pub const UNDERFLOW_MARGIN: f64 = f64::MIN_POSITIVE;
/// Largest accepted magnitude of any coordinate, radius or ray parameter: `2^500` (about 3e150 m).
pub const COORD_LIMIT: f64 = f64::from_bits((1023 + 500) << 52);

const STACK: usize = 64;
const MAGIC: &[u8; 8] = b"FIDBVH01";
const NODE_BYTES: u64 = 56;

/// Which query argument an error refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryField {
    /// Sphere centre or nearest/visit point.
    Point,
    /// Sphere radius.
    Radius,
    /// Box minimum corner.
    BoxMin,
    /// Box maximum corner.
    BoxMax,
    /// Ray origin.
    Origin,
    /// Ray direction.
    Direction,
    /// Lower end of the ray's parameter range.
    TMin,
    /// Upper end of the ray's parameter range.
    TMax,
}

impl QueryField {
    fn as_str(self) -> &'static str {
        match self {
            QueryField::Point => "point",
            QueryField::Radius => "radius",
            QueryField::BoxMin => "box_min",
            QueryField::BoxMax => "box_max",
            QueryField::Origin => "origin",
            QueryField::Direction => "direction",
            QueryField::TMin => "t_min",
            QueryField::TMax => "t_max",
        }
    }
}

/// Every error of building or querying a [`PrimitiveBvh`]. Errors are values; nothing panics.
#[derive(Debug, Clone, PartialEq)]
pub enum BvhError {
    /// `threads` was 0.
    ZeroThreads,
    /// The mesh has no triangles.
    EmptyMesh,
    /// More than [`MAX_TRIANGLES`] triangles.
    TooManyTriangles {
        /// The triangle count.
        count: u64,
    },
    /// The payload does not have one ID per triangle.
    PayloadLength {
        /// The triangle count.
        expected: u64,
        /// The payload length.
        found: u64,
    },
    /// A corner coordinate is NaN or infinite.
    NonFinitePrimitive {
        /// Triangle index.
        triangle: u64,
        /// Corner 0, 1 or 2.
        corner: u8,
        /// Axis 0 = x, 1 = y, 2 = z.
        axis: u8,
    },
    /// A corner coordinate exceeds [`COORD_LIMIT`] in magnitude.
    PrimitiveOutOfRange {
        /// Triangle index.
        triangle: u64,
        /// Corner 0, 1 or 2.
        corner: u8,
        /// Axis 0 = x, 1 = y, 2 = z.
        axis: u8,
    },
    /// A query value is NaN or infinite.
    NonFiniteQuery {
        /// The argument.
        field: QueryField,
    },
    /// A query value exceeds [`COORD_LIMIT`] in magnitude.
    QueryOutOfRange {
        /// The argument.
        field: QueryField,
    },
    /// The sphere radius is negative.
    NegativeRadius,
    /// The query box has `min > max` on `axis`.
    InvertedQueryBox {
        /// Axis 0 = x, 1 = y, 2 = z.
        axis: u8,
    },
    /// The ray direction is the zero vector.
    ZeroDirection,
    /// The ray has `t_min > t_max`.
    InvertedRange,
    /// Mapping a ray with a GEO.FRAME transform failed.
    Transform(XformError),
    /// A working allocation of `bytes` bytes failed.
    AllocationFailed {
        /// Requested size in bytes (saturating).
        bytes: u64,
    },
    /// An internal invariant does not hold (unreachable for values built by this crate).
    Inconsistent,
}

impl BvhError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            BvhError::ZeroThreads => "zero_threads",
            BvhError::EmptyMesh => "empty_mesh",
            BvhError::TooManyTriangles { .. } => "too_many_triangles",
            BvhError::PayloadLength { .. } => "payload_length",
            BvhError::NonFinitePrimitive { .. } => "non_finite_primitive",
            BvhError::PrimitiveOutOfRange { .. } => "primitive_out_of_range",
            BvhError::NonFiniteQuery { .. } => "non_finite_query",
            BvhError::QueryOutOfRange { .. } => "query_out_of_range",
            BvhError::NegativeRadius => "negative_radius",
            BvhError::InvertedQueryBox { .. } => "inverted_query_box",
            BvhError::ZeroDirection => "zero_direction",
            BvhError::InvertedRange => "inverted_range",
            BvhError::Transform(_) => "transform",
            BvhError::AllocationFailed { .. } => "allocation_failed",
            BvhError::Inconsistent => "inconsistent",
        }
    }
}

impl fmt::Display for BvhError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BvhError::TooManyTriangles { count } => write!(f, "too_many_triangles ({count} > {MAX_TRIANGLES})"),
            BvhError::PayloadLength { expected, found } => {
                write!(f, "payload_length (expected {expected}, found {found})")
            }
            BvhError::NonFinitePrimitive { triangle, corner, axis }
            | BvhError::PrimitiveOutOfRange { triangle, corner, axis } => {
                write!(f, "{} (triangle {triangle}, corner {corner}, axis {axis})", self.code())
            }
            BvhError::NonFiniteQuery { field } | BvhError::QueryOutOfRange { field } => {
                write!(f, "{} ({})", self.code(), field.as_str())
            }
            BvhError::InvertedQueryBox { axis } => write!(f, "inverted_query_box (axis {axis})"),
            BvhError::Transform(e) => write!(f, "transform: {e}"),
            BvhError::AllocationFailed { bytes } => write!(f, "allocation_failed ({bytes} bytes)"),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for BvhError {}

impl From<XformError> for BvhError {
    fn from(e: XformError) -> Self {
        BvhError::Transform(e)
    }
}

/// One hierarchy node: its box, and either its left child (right child = left + 1) or the
/// first of its `count` positions in the triangle order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BvhNode {
    aabb: Aabb,
    left_or_first: u32,
    count: u16,
    axis: u8,
}

impl BvhNode {
    /// The node box (totalOrder union of its triangles' boxes).
    pub fn aabb(&self) -> Aabb {
        self.aabb
    }

    /// True for a leaf.
    pub fn is_leaf(&self) -> bool {
        self.count > 0
    }

    /// Left child index (internal node) or first triangle-order position (leaf).
    pub fn left_or_first(&self) -> u32 {
        self.left_or_first
    }

    /// Number of triangles in a leaf; 0 for an internal node.
    pub fn count(&self) -> u16 {
        self.count
    }

    /// Split axis of an internal node; 0 for a leaf.
    pub fn axis(&self) -> u8 {
        self.axis
    }
}

/// One leaf handed to the [`PrimitiveBvh::ordered_visit`] visitor.
#[derive(Debug, Clone, Copy)]
pub struct LeafVisit<'a> {
    node: u32,
    lower_bound_sq: f64,
    triangles: &'a [u32],
}

impl LeafVisit<'_> {
    /// The leaf's node index.
    pub fn node(&self) -> u32 {
        self.node
    }

    /// A lower bound of the squared exact distance from the point to any triangle box in the leaf.
    pub fn lower_bound_sq(&self) -> f64 {
        self.lower_bound_sq
    }

    /// The leaf's triangle indexes, ascending.
    pub fn triangles(&self) -> impl Iterator<Item = u64> + '_ {
        self.triangles.iter().map(|&t| u64::from(t))
    }
}

/// The triangle whose box is nearest to a point by the pinned squared box distance `d2`
/// (ties: smallest triangle index).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NearestBox {
    /// Triangle index.
    pub triangle: u64,
    /// The pinned squared distance `d2` from the point to the triangle's box (0 inside).
    pub distance_sq: f64,
}

/// A bounding volume hierarchy over triangles with one payload ID per triangle.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimitiveBvh {
    frame: FrameRef,
    nodes: Vec<BvhNode>,
    prim_order: Vec<u32>,
    prim_ids: Vec<u64>,
    boxes: Vec<Aabb>,
}

/// Exact upper bound on the bytes allocated to build a [`PrimitiveBvh`] over `n` triangles:
/// the struct itself, the retained node, order, payload and triangle-box buffers, plus the
/// temporary centroid-key and range-queue working buffers held during construction.
/// Returns `None` if `n > MAX_TRIANGLES` or arithmetic overflows.
pub fn bvh_build_bytes(n: u64) -> Option<u64> {
    if n > MAX_TRIANGLES {
        return None;
    }
    let inline = u64::try_from(core::mem::size_of::<PrimitiveBvh>()).ok()?;
    if n == 0 {
        return Some(inline);
    }
    let cap = (n / 4).checked_mul(2)?.checked_add(1)?;
    let per_tri = u64::try_from(
        core::mem::size_of::<Aabb>()
            + core::mem::size_of::<[f64; 3]>()
            + core::mem::size_of::<u32>()
            + core::mem::size_of::<u64>(),
    )
    .ok()?;
    let per_node = u64::try_from(core::mem::size_of::<BvhNode>() + core::mem::size_of::<(usize, usize)>()).ok()?;
    inline.checked_add(n.checked_mul(per_tri)?)?.checked_add(cap.checked_mul(per_node)?)
}

// ------------------------------------------------------------------ helpers

fn total_min(a: f64, b: f64) -> f64 {
    if b.total_cmp(&a) == Ordering::Less {
        b
    } else {
        a
    }
}

fn total_max(a: f64, b: f64) -> f64 {
    if b.total_cmp(&a) == Ordering::Greater {
        b
    } else {
        a
    }
}

/// Numeric order of non-NaN values (`-0.0 == +0.0`).
fn num_cmp(a: f64, b: f64) -> Ordering {
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

fn bytes_of<T>(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX).saturating_mul(u64::try_from(core::mem::size_of::<T>()).unwrap_or(u64::MAX))
}

fn reserve<T>(v: &mut Vec<T>, n: usize) -> Result<(), BvhError> {
    v.try_reserve_exact(n).map_err(|_| BvhError::AllocationFailed { bytes: bytes_of::<T>(n) })
}

fn push<T>(v: &mut Vec<T>, x: T) -> Result<(), BvhError> {
    reserve(v, 1)?;
    v.push(x);
    Ok(())
}

fn check_value(x: f64, field: QueryField) -> Result<(), BvhError> {
    if !x.is_finite() {
        return Err(BvhError::NonFiniteQuery { field });
    }
    if x.abs() > COORD_LIMIT {
        return Err(BvhError::QueryOutOfRange { field });
    }
    Ok(())
}

fn check_point(p: &[f64; 3], field: QueryField) -> Result<(), BvhError> {
    p.iter().try_for_each(|&x| check_value(x, field))
}

fn gap(c: f64, lo: f64, hi: f64) -> f64 {
    if c < lo {
        lo - c
    } else if c > hi {
        c - hi
    } else {
        0.0
    }
}

/// The pinned squared distance from `c` to a box: `(gx*gx + gy*gy) + gz*gz`.
fn dist2(c: &[f64; 3], b: &Aabb) -> f64 {
    let (lo, hi) = (b.min(), b.max());
    let gx = gap(c[0], lo[0], hi[0]);
    let gy = gap(c[1], lo[1], hi[1]);
    let gz = gap(c[2], lo[2], hi[2]);
    (gx * gx + gy * gy) + gz * gz
}

fn lower_bound(d2: f64) -> f64 {
    let lb = d2 * (1.0 - ROUNDOFF_MARGIN) - UNDERFLOW_MARGIN;
    if lb <= 0.0 {
        0.0
    } else {
        lb
    }
}

fn widen_up(t: f64) -> f64 {
    if t.is_finite() {
        t + (t.abs() * ROUNDOFF_MARGIN + UNDERFLOW_MARGIN)
    } else {
        t
    }
}

fn widen_down(t: f64) -> f64 {
    if t.is_finite() {
        t - (t.abs() * ROUNDOFF_MARGIN + UNDERFLOW_MARGIN)
    } else {
        t
    }
}

fn box_hits(b: &Aabb, qmin: &[f64; 3], qmax: &[f64; 3]) -> bool {
    let (lo, hi) = (b.min(), b.max());
    (0..3).all(|k| lo[k] <= qmax[k] && qmin[k] <= hi[k])
}

#[derive(Clone, Copy)]
struct Ray {
    o: [f64; 3],
    d: [f64; 3],
    t_min: f64,
    t_max: f64,
}

fn ray_hits(b: &Aabb, r: &Ray) -> bool {
    let (lo, hi) = (b.min(), b.max());
    let mut a = r.t_min;
    let mut z = r.t_max;
    for k in 0..3 {
        let (o, d) = (r.o[k], r.d[k]);
        if d == 0.0 {
            if !(lo[k]..=hi[k]).contains(&o) {
                return false;
            }
            continue;
        }
        let ta = (lo[k] - o) / d;
        let tb = (hi[k] - o) / d;
        let (near, far) = if ta <= tb { (ta, tb) } else { (tb, ta) };
        a = total_max(a, widen_down(near));
        z = total_min(z, widen_up(far));
    }
    a <= z
}

/// Leaf callback of the best-first traversal: (node index, lower bound, leaf triangles).
type LeafFn<'a> = dyn FnMut(u32, f64, &[u32]) -> ControlFlow<()> + 'a;

/// Heap key: totalOrder on the lower bound, then node index.
#[derive(Clone, Copy)]
struct Key(f64, u32);

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Key {}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0).then(self.1.cmp(&other.1))
    }
}

// ------------------------------------------------------------------ build

impl PrimitiveBvh {
    /// Builds the hierarchy over the triangles of `mesh`, with `payload[t]` the ID of triangle
    /// `t` (triangle index, or CAD face ID for a tessellation). `threads` must be at least 1;
    /// the build is sequential and its result does not depend on it.
    pub fn build(mesh: &Mesh, payload: &[u64], threads: usize) -> Result<PrimitiveBvh, BvhError> {
        let n = mesh.triangle_count();
        Self::build_with(mesh.frame(), n, payload, threads, |t| {
            let tri = mesh.triangle(t).ok_or(BvhError::Inconsistent)?;
            let mut c = [[0.0; 3]; 3];
            for (slot, &v) in c.iter_mut().zip(tri.iter()) {
                *slot = mesh.vertex(v).ok_or(BvhError::Inconsistent)?;
            }
            Ok(c)
        })
    }

    /// Builds the hierarchy over triangles given as corner triples `[a, b, c]` in `frame`
    /// (for example a CAD tessellation), with the same rules as [`PrimitiveBvh::build`].
    pub fn build_triangles(
        frame: FrameRef,
        triangles: &[[[f64; 3]; 3]],
        payload: &[u64],
        threads: usize,
    ) -> Result<PrimitiveBvh, BvhError> {
        let n = u64::try_from(triangles.len()).unwrap_or(u64::MAX);
        Self::build_with(frame, n, payload, threads, |t| {
            usize::try_from(t).ok().and_then(|i| triangles.get(i)).copied().ok_or(BvhError::Inconsistent)
        })
    }

    fn build_with(
        frame: FrameRef,
        n: u64,
        payload: &[u64],
        threads: usize,
        corners: impl Fn(u64) -> Result<[[f64; 3]; 3], BvhError>,
    ) -> Result<PrimitiveBvh, BvhError> {
        if threads == 0 {
            return Err(BvhError::ZeroThreads);
        }
        if n == 0 {
            return Err(BvhError::EmptyMesh);
        }
        if n > MAX_TRIANGLES {
            return Err(BvhError::TooManyTriangles { count: n });
        }
        let found = u64::try_from(payload.len()).unwrap_or(u64::MAX);
        if found != n {
            return Err(BvhError::PayloadLength { expected: n, found });
        }
        let nu = usize::try_from(n).map_err(|_| BvhError::TooManyTriangles { count: n })?;

        let mut boxes: Vec<Aabb> = Vec::new();
        let mut keys: Vec<[f64; 3]> = Vec::new();
        reserve(&mut boxes, nu)?;
        reserve(&mut keys, nu)?;
        for t in 0..n {
            let c = corners(t)?;
            for (ci, corner) in c.iter().enumerate() {
                for (ai, &x) in corner.iter().enumerate() {
                    let (corner, axis) = (ci as u8, ai as u8);
                    if !x.is_finite() {
                        return Err(BvhError::NonFinitePrimitive { triangle: t, corner, axis });
                    }
                    if x.abs() > COORD_LIMIT {
                        return Err(BvhError::PrimitiveOutOfRange { triangle: t, corner, axis });
                    }
                }
            }
            let [a, b, cc] = c;
            let lo = [0, 1, 2].map(|k| total_min(total_min(a[k], b[k]), cc[k]));
            let hi = [0, 1, 2].map(|k| total_max(total_max(a[k], b[k]), cc[k]));
            boxes.push(Aabb::new(lo, hi).map_err(|_| BvhError::Inconsistent)?);
            keys.push([0, 1, 2].map(|k| (a[k] + b[k]) + cc[k]));
        }

        let mut prim_order: Vec<u32> = Vec::new();
        reserve(&mut prim_order, nu)?;
        for t in 0..n {
            prim_order.push(u32::try_from(t).map_err(|_| BvhError::Inconsistent)?);
        }
        // Leaves hold 4..=8 triangles once n > 8, so there are at most n / 4 leaves and fewer
        // than 2 (n / 4) + 1 nodes.
        let cap = (nu / 4).saturating_mul(2).saturating_add(1);
        let mut nodes: Vec<BvhNode> = Vec::new();
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        reserve(&mut nodes, cap)?;
        reserve(&mut ranges, cap)?;
        ranges.push((0, nu));

        let mut i = 0;
        while let Some(&(s, e)) = ranges.get(i) {
            let slice = prim_order.get_mut(s..e).ok_or(BvhError::Inconsistent)?;
            let mut aabb: Option<Aabb> = None;
            for &p in slice.iter() {
                let b = boxes.get(p as usize).ok_or(BvhError::Inconsistent)?;
                aabb = Some(match aabb {
                    None => *b,
                    Some(acc) => acc.union(b),
                });
            }
            let aabb = aabb.ok_or(BvhError::Inconsistent)?;
            let count = e - s;
            if count <= LEAF_SIZE {
                slice.sort_unstable();
                let first = u32::try_from(s).map_err(|_| BvhError::Inconsistent)?;
                let count = u16::try_from(count).map_err(|_| BvhError::Inconsistent)?;
                push(&mut nodes, BvhNode { aabb, left_or_first: first, count, axis: 0 })?;
            } else {
                let (lo, hi) = (aabb.min(), aabb.max());
                let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
                let mut axis = 0usize;
                for k in 1..3 {
                    if ext[k] > ext[axis] {
                        axis = k;
                    }
                }
                let key = |p: u32| keys.get(p as usize).map_or(0.0, |k| k[axis]);
                let half = count / 2;
                slice.select_nth_unstable_by(half, |&p, &q| num_cmp(key(p), key(q)).then(p.cmp(&q)));
                let left = u32::try_from(ranges.len()).map_err(|_| BvhError::Inconsistent)?;
                push(&mut ranges, (s, s + half))?;
                push(&mut ranges, (s + half, e))?;
                push(&mut nodes, BvhNode { aabb, left_or_first: left, count: 0, axis: axis as u8 })?;
            }
            i += 1;
        }

        let mut prim_ids: Vec<u64> = Vec::new();
        reserve(&mut prim_ids, nu)?;
        prim_ids.extend_from_slice(payload);
        Ok(PrimitiveBvh { frame, nodes, prim_order, prim_ids, boxes })
    }

    // ------------------------------------------------------------------ accessors

    /// The frame the triangles (and every query) are expressed in.
    pub fn frame(&self) -> FrameRef {
        self.frame
    }

    /// The nodes in breadth-first order (node 0 is the root).
    pub fn nodes(&self) -> &[BvhNode] {
        &self.nodes
    }

    /// Triangle indexes in leaf order (each leaf's range ascending).
    pub fn prim_order(&self) -> &[u32] {
        &self.prim_order
    }

    /// Number of triangles.
    pub fn triangle_count(&self) -> u64 {
        u64::try_from(self.prim_ids.len()).unwrap_or(u64::MAX)
    }

    /// The payload ID of triangle `t`.
    pub fn payload(&self, t: u64) -> Option<u64> {
        usize::try_from(t).ok().and_then(|i| self.prim_ids.get(i)).copied()
    }

    /// The box of triangle `t`.
    pub fn triangle_aabb(&self, t: u64) -> Option<Aabb> {
        usize::try_from(t).ok().and_then(|i| self.boxes.get(i)).copied()
    }

    /// The root box (all triangles).
    pub fn bounds(&self) -> Result<Aabb, BvhError> {
        self.nodes.first().map(|n| n.aabb).ok_or(BvhError::Inconsistent)
    }

    /// Bytes held by the hierarchy: the struct itself plus the exact capacity of its node,
    /// triangle-order, payload-ID and triangle-box buffers.
    pub fn memory_bytes(&self) -> u64 {
        bytes_of::<PrimitiveBvh>(1)
            .saturating_add(bytes_of::<BvhNode>(self.nodes.capacity()))
            .saturating_add(bytes_of::<u32>(self.prim_order.capacity()))
            .saturating_add(bytes_of::<u64>(self.prim_ids.capacity()))
            .saturating_add(bytes_of::<Aabb>(self.boxes.capacity()))
    }

    /// The pinned serialisation (REQ-GEO.INDEX.BVH-004): `FIDBVH01`, triangle count and node
    /// count (u64 LE), the 16 frame bytes; per node min x, y, z and max x, y, z (f64 bits,
    /// u64 LE), `left_or_first` (u32 LE), `count` (u16 LE), `axis` (u8) and a zero byte; the
    /// triangle order (u32 LE each); the payload IDs in triangle order (u64 LE each).
    pub fn to_bytes(&self) -> Result<Vec<u8>, BvhError> {
        let m = u64::try_from(self.nodes.len()).unwrap_or(u64::MAX);
        let n = self.triangle_count();
        let size = 40u64
            .checked_add(m.checked_mul(NODE_BYTES).ok_or(BvhError::AllocationFailed { bytes: u64::MAX })?)
            .and_then(|x| x.checked_add(n.checked_mul(12)?))
            .ok_or(BvhError::AllocationFailed { bytes: u64::MAX })?;
        let size_us = usize::try_from(size).map_err(|_| BvhError::AllocationFailed { bytes: size })?;
        let mut out: Vec<u8> = Vec::new();
        out.try_reserve_exact(size_us).map_err(|_| BvhError::AllocationFailed { bytes: size })?;
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&m.to_le_bytes());
        out.extend_from_slice(&self.frame.to_bytes());
        for node in &self.nodes {
            for x in node.aabb.min().iter().chain(node.aabb.max().iter()) {
                out.extend_from_slice(&x.to_bits().to_le_bytes());
            }
            out.extend_from_slice(&node.left_or_first.to_le_bytes());
            out.extend_from_slice(&node.count.to_le_bytes());
            out.push(node.axis);
            out.push(0);
        }
        for &t in &self.prim_order {
            out.extend_from_slice(&t.to_le_bytes());
        }
        for &id in &self.prim_ids {
            out.extend_from_slice(&id.to_le_bytes());
        }
        Ok(out)
    }

    // ------------------------------------------------------------------ queries

    /// Every triangle whose box intersects `b` (boundaries included), ascending.
    pub fn aabb(&self, b: &Aabb) -> Result<Vec<u64>, BvhError> {
        self.box_query(b.min(), b.max())
    }

    /// Every triangle whose box intersects the box `[min, max]` (boundaries included), ascending.
    pub fn box_query(&self, min: [f64; 3], max: [f64; 3]) -> Result<Vec<u64>, BvhError> {
        check_point(&min, QueryField::BoxMin)?;
        check_point(&max, QueryField::BoxMax)?;
        for k in 0..3 {
            if min[k] > max[k] {
                return Err(BvhError::InvertedQueryBox { axis: k as u8 });
            }
        }
        self.collect(|b| box_hits(b, &min, &max))
    }

    /// Every triangle whose box is within distance `r` of `c` (conservatively), ascending.
    pub fn sphere(&self, c: [f64; 3], r: f64) -> Result<Vec<u64>, BvhError> {
        check_point(&c, QueryField::Point)?;
        check_value(r, QueryField::Radius)?;
        if r < 0.0 {
            return Err(BvhError::NegativeRadius);
        }
        let thr = r * r * (1.0 + ROUNDOFF_MARGIN) + UNDERFLOW_MARGIN;
        self.collect(|b| dist2(&c, b) <= thr)
    }

    /// Every triangle whose box meets the segment `o + t d`, `t` in `[t_min, t_max]`
    /// (conservatively), ascending. `d` need not be normalised.
    pub fn ray(&self, o: [f64; 3], d: [f64; 3], t_min: f64, t_max: f64) -> Result<Vec<u64>, BvhError> {
        check_point(&o, QueryField::Origin)?;
        check_point(&d, QueryField::Direction)?;
        check_value(t_min, QueryField::TMin)?;
        check_value(t_max, QueryField::TMax)?;
        if d.iter().all(|&x| x == 0.0) {
            return Err(BvhError::ZeroDirection);
        }
        if t_min > t_max {
            return Err(BvhError::InvertedRange);
        }
        let ray = Ray { o, d, t_min, t_max };
        self.collect(|b| ray_hits(b, &ray))
    }

    /// [`PrimitiveBvh::ray`] for a ray given in another frame: `xf` maps that frame into the
    /// mesh frame (GEO.FRAME.XFORM). The origin maps as a point, `o' = s R o + t`; the direction
    /// as `d'_i = s (R d)_i`, so the parameter range is unchanged. The inputs are checked first
    /// (finite, within range), then the mapped ray is queried as given: candidates are
    /// conservative for the mapped ray, not for rounding in the mapping itself.
    pub fn ray_transformed(
        &self,
        xf: &Transform,
        o: [f64; 3],
        d: [f64; 3],
        t_min: f64,
        t_max: f64,
    ) -> Result<Vec<u64>, BvhError> {
        check_point(&o, QueryField::Origin)?;
        check_point(&d, QueryField::Direction)?;
        check_value(t_min, QueryField::TMin)?;
        check_value(t_max, QueryField::TMax)?;
        let o2 = xf.apply_point(&o)?;
        let s = xf.scale();
        let d2 = xf.apply_dir(&d)?.map(|x| s * x);
        self.ray(o2, d2, t_min, t_max)
    }

    /// The triangle whose box is nearest to `p` by the pinned squared box distance, ties
    /// broken by the smallest triangle index.
    pub fn nearest_box(&self, p: [f64; 3]) -> Result<NearestBox, BvhError> {
        check_point(&p, QueryField::Point)?;
        let mut best: Option<NearestBox> = None;
        let mut fail: Option<BvhError> = None;
        self.visit(&p, &mut |_, lb, tris| {
            if let Some(b) = best {
                if lb > b.distance_sq {
                    return ControlFlow::Break(());
                }
            }
            for &t in tris {
                let Some(bx) = self.boxes.get(t as usize) else {
                    fail = Some(BvhError::Inconsistent);
                    return ControlFlow::Break(());
                };
                let d2 = dist2(&p, bx);
                let t = u64::from(t);
                let better = match best {
                    None => true,
                    Some(b) => d2 < b.distance_sq || (d2 == b.distance_sq && t < b.triangle),
                };
                if better {
                    best = Some(NearestBox { triangle: t, distance_sq: d2 });
                }
            }
            ControlFlow::Continue(())
        })?;
        if let Some(e) = fail {
            return Err(e);
        }
        best.ok_or(BvhError::Inconsistent)
    }

    /// Visits the leaves in ascending order of (lower bound of the squared distance from `p`
    /// to the leaf box, node index) (REQ-GEO.INDEX.BVH-005) until `visitor` breaks.
    pub fn ordered_visit<F>(&self, p: [f64; 3], mut visitor: F) -> Result<(), BvhError>
    where
        F: FnMut(LeafVisit<'_>) -> ControlFlow<()>,
    {
        check_point(&p, QueryField::Point)?;
        self.visit(&p, &mut |node, lower_bound_sq, triangles| visitor(LeafVisit { node, lower_bound_sq, triangles }))
    }

    fn visit(&self, p: &[f64; 3], f: &mut LeafFn<'_>) -> Result<(), BvhError> {
        let root = self.nodes.first().ok_or(BvhError::Inconsistent)?;
        let mut heap: BinaryHeap<Reverse<Key>> = BinaryHeap::new();
        heap.try_reserve(STACK).map_err(|_| BvhError::AllocationFailed { bytes: bytes_of::<Key>(STACK) })?;
        heap.push(Reverse(Key(lower_bound(dist2(p, &root.aabb)), 0)));
        while let Some(Reverse(Key(lb, i))) = heap.pop() {
            let node = self.nodes.get(i as usize).ok_or(BvhError::Inconsistent)?;
            if node.is_leaf() {
                let s = node.left_or_first as usize;
                let tris = self.prim_order.get(s..s + usize::from(node.count)).ok_or(BvhError::Inconsistent)?;
                if f(i, lb, tris).is_break() {
                    return Ok(());
                }
            } else {
                for c in [node.left_or_first, node.left_or_first.wrapping_add(1)] {
                    let child = self.nodes.get(c as usize).ok_or(BvhError::Inconsistent)?;
                    heap.try_reserve(1).map_err(|_| BvhError::AllocationFailed { bytes: bytes_of::<Key>(1) })?;
                    heap.push(Reverse(Key(lower_bound(dist2(p, &child.aabb)), c)));
                }
            }
        }
        Ok(())
    }

    /// Depth-first traversal with a fixed stack; returns the passing triangles, ascending.
    fn collect(&self, test: impl Fn(&Aabb) -> bool) -> Result<Vec<u64>, BvhError> {
        let mut stack = [0u32; STACK];
        let mut sp = 1usize;
        let mut out: Vec<u64> = Vec::new();
        while sp > 0 {
            sp -= 1;
            let i = *stack.get(sp).ok_or(BvhError::Inconsistent)?;
            let node = self.nodes.get(i as usize).ok_or(BvhError::Inconsistent)?;
            if !test(&node.aabb) {
                continue;
            }
            if node.is_leaf() {
                let s = node.left_or_first as usize;
                let tris = self.prim_order.get(s..s + usize::from(node.count)).ok_or(BvhError::Inconsistent)?;
                for &t in tris {
                    let b = self.boxes.get(t as usize).ok_or(BvhError::Inconsistent)?;
                    if test(b) {
                        push(&mut out, u64::from(t))?;
                    }
                }
            } else {
                let slots = stack.get_mut(sp..sp + 2).ok_or(BvhError::Inconsistent)?;
                slots[0] = node.left_or_first.wrapping_add(1);
                slots[1] = node.left_or_first;
                sp += 2;
            }
        }
        out.sort_unstable();
        Ok(out)
    }
}
