//! Point k-d tree with exact k-nearest, radius and box queries. Spec node `GEO.INDEX.KDTREE`.
//!
//! [`PointIndex`] is an implicit k-d tree over a borrowed [`PointBuffer`]: a permutation of the
//! point indexes, one split value and one axis byte per internal node, and nothing else. Every
//! query result equals a brute-force scan with the same pinned arithmetic, ties included.
//!
//! Every value-affecting choice below is pinned in the leaf's `NODE.md` (Design) and modelled
//! bit for bit by the independent reference `tools/fixtures/geo_index.py`.
//!
//! # Input domain (refused with typed errors, first offender wins)
//!
//! Build: `threads >= 1` ([`PointIndexError::ZeroThreads`]); the y and z columns as long as x
//! ([`PointIndexError::ColumnLength`]); at most [`MAX_POINTS`] points; then per point in index
//! order, per axis x, y, z: finite ([`PointIndexError::NonFinitePoint`]) and
//! `|x| <= COORD_LIMIT` = 2^500 ([`PointIndexError::PointOutOfRange`]). Empty buffers,
//! duplicate and coincident points, signed zeros and subnormal coordinates are accepted.
//!
//! Queries: the query point first, per axis: finite ([`PointIndexError::NonFiniteQuery`]),
//! then at most `COORD_LIMIT` in magnitude ([`PointIndexError::QueryOutOfRange`]); then
//! `1 <= k <= 1024` ([`PointIndexError::BadK`]) or a radius that is finite, `>= 0` (`-0.0` is
//! accepted) and at most `COORD_LIMIT` ([`PointIndexError::BadRadius`]). The limit keeps every
//! difference, square and sum of three squares finite, so no distance overflows or is NaN.
//!
//! # Squared distance (pinned, REQ-GEO.INDEX.KDTREE-003)
//!
//! `d2(p, q) = (dx*dx + dy*dy) + dz*dz` with `dx = p.x - q.x` (the sign does not change the
//! value), left to right, no fused multiply-add. Every "nearest", "within" and tie in this
//! module is decided on this computed value. Consequence, by design: two distinct points whose
//! squared differences underflow (subnormal coordinates, `|dx| < 2^-537`) have `d2 = 0` and tie
//! like coincident points; the returned `dist2` shows it.
//!
//! # Build (pinned)
//!
//! - Leaf depth `L` = the smallest `d >= 0` with `ceil(n / 2^d) <= 16` ([`BUCKET`]). The tree is
//!   perfect: nodes `0 .. 2^L - 1` are internal (children of `i` are `2i + 1` and `2i + 2`), all
//!   `2^L` leaves are at depth `L`. A node over `c` points gives its left child the first
//!   `c / 2` (floor) and its right child the rest, so every node at depth `d` holds
//!   `floor(n / 2^d)` or `ceil(n / 2^d)` points and every leaf holds 8 to 16 points once
//!   `n > 16` (`n <= 16`: the root is the only leaf).
//! - Internal nodes in index order (breadth-first): extents `max - min` per axis of the node's
//!   points (totalOrder min and max); split axis = largest extent, lowest axis on ties
//!   (x < y < z); the node's points are ordered by (coordinate on that axis in IEEE totalOrder,
//!   point index) and the first `c / 2` go left; the split value is the coordinate of the first
//!   right point (the `c / 2`-th in that order, 0-based). So every left point has coordinate
//!   `<= split` and every right point `>= split`, numerically.
//! - Each leaf's points are stored in ascending index order, so the permutation is a pure
//!   function of the input (it does not depend on how `select_nth_unstable_by` arranges a
//!   partition).
//! - The build is sequential: KNP-1 forbids threads in the kernel, so `threads` cannot change
//!   the result (REQ-GEO.INDEX.KDTREE-004), as for `GEO.INDEX.BVH`.
//!
//! # Queries (pinned)
//!
//! - `knn(q, k)`: the `min(k, n)` smallest pairs `(d2, index)` in lexicographic order, sorted
//!   ascending (REQ-001, REQ-002).
//! - `radius(q, r)`: every point with `d2 <= r*r` (`r*r` rounded once), sorted by
//!   `(d2, index)`. `r = 0` returns the points with `d2 = 0`.
//! - `in_box(b)`: every point with `min_k <= p_k <= max_k` on all axes (numeric comparisons,
//!   `-0.0 == +0.0`), sorted by index.
//!
//! Why pruning is exact: a subtree is skipped only when a lower bound `lb` of the computed `d2`
//! of all its points exceeds the current k-th `(d2, index)` (or `r*r`). `lb` is
//! `(gx*gx + gy*gy) + gz*gz` over per-axis gaps `g_k`, each the largest of `fl(|q_k - s|)` over
//! the splits `s` on axis `k` whose far side the subtree lies on (or 0). For any point `p` in
//! the subtree `|p_k - q_k| >= |s - q_k|` exactly, and rounding is monotone, so
//! `|fl(p_k - q_k)| >= g_k`; squaring and adding non-negative values are monotone too, so
//! `d2(p, q) >= lb`. No inflation is needed and none is used. Box pruning uses exact
//! comparisons with the split values.
//!
//! # Bounds
//!
//! No recursion. Queries use a fixed 64-entry stack (depth is at most 28 for 2^32 points); the
//! k-nearest heap holds at most `k + 1 <= 1025` entries. Every allocation sized from input is
//! fallible ([`PointIndexError::AllocationFailed`]). Nothing panics.

// Per-axis loops index several 3-arrays at once; written as index loops to mirror the pinned
// formulas in NODE.md.
#![allow(clippy::needless_range_loop)]

use core::cmp::Ordering;
use core::fmt;
use std::collections::BinaryHeap;

use fid_geo_types::{Aabb, PointBuffer};

use crate::bvh::COORD_LIMIT;

/// The largest number of points in a leaf.
pub const BUCKET: usize = 16;
/// The largest `k` of a k-nearest query.
pub const MAX_K: usize = 1024;
/// The largest point count (the permutation is `u32`).
pub const MAX_POINTS: u64 = 1 << 32;

const STACK: usize = 64;
const MAGIC: &[u8; 8] = b"FIDKDT01";

/// One query hit: a point index and its pinned squared distance to the query point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Neighbour {
    /// The point index in the buffer.
    pub index: u64,
    /// The pinned squared distance `(dx*dx + dy*dy) + dz*dz` (never negative, never `-0.0`).
    pub dist2: f64,
}

/// Every error of building or querying a [`PointIndex`]. Errors are values; nothing panics.
#[derive(Debug, Clone, PartialEq)]
pub enum PointIndexError {
    /// `threads` was 0.
    ZeroThreads,
    /// The y (`axis` 1) or z (`axis` 2) column does not have one value per x value.
    ColumnLength {
        /// Axis 1 = y, 2 = z.
        axis: u8,
        /// The x column length.
        expected: u64,
        /// The column length.
        found: u64,
    },
    /// More than [`MAX_POINTS`] points.
    TooManyPoints {
        /// The point count.
        count: u64,
    },
    /// A point coordinate is NaN or infinite.
    NonFinitePoint {
        /// Point index.
        index: u64,
        /// Axis 0 = x, 1 = y, 2 = z.
        axis: u8,
    },
    /// A point coordinate exceeds [`COORD_LIMIT`] in magnitude.
    PointOutOfRange {
        /// Point index.
        index: u64,
        /// Axis 0 = x, 1 = y, 2 = z.
        axis: u8,
    },
    /// A query coordinate is NaN or infinite.
    NonFiniteQuery {
        /// Axis 0 = x, 1 = y, 2 = z.
        axis: u8,
    },
    /// A query coordinate exceeds [`COORD_LIMIT`] in magnitude.
    QueryOutOfRange {
        /// Axis 0 = x, 1 = y, 2 = z.
        axis: u8,
    },
    /// `k` is 0 or above [`MAX_K`].
    BadK {
        /// The requested `k` (saturating).
        k: u64,
    },
    /// The radius is NaN, infinite, negative or above [`COORD_LIMIT`].
    BadRadius,
    /// A working allocation of `bytes` bytes failed.
    AllocationFailed {
        /// Requested size in bytes (saturating).
        bytes: u64,
    },
    /// An internal invariant does not hold (unreachable for values built by this crate).
    Inconsistent,
}

impl PointIndexError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            PointIndexError::ZeroThreads => "zero_threads",
            PointIndexError::ColumnLength { .. } => "column_length",
            PointIndexError::TooManyPoints { .. } => "too_many_points",
            PointIndexError::NonFinitePoint { .. } => "non_finite_point",
            PointIndexError::PointOutOfRange { .. } => "point_out_of_range",
            PointIndexError::NonFiniteQuery { .. } => "non_finite_query",
            PointIndexError::QueryOutOfRange { .. } => "query_out_of_range",
            PointIndexError::BadK { .. } => "bad_k",
            PointIndexError::BadRadius => "bad_radius",
            PointIndexError::AllocationFailed { .. } => "allocation_failed",
            PointIndexError::Inconsistent => "inconsistent",
        }
    }
}

impl fmt::Display for PointIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PointIndexError::ColumnLength { axis, expected, found } => {
                write!(f, "column_length (axis {axis}: expected {expected}, found {found})")
            }
            PointIndexError::TooManyPoints { count } => write!(f, "too_many_points ({count} > {MAX_POINTS})"),
            PointIndexError::NonFinitePoint { index, axis } | PointIndexError::PointOutOfRange { index, axis } => {
                write!(f, "{} (point {index}, axis {axis})", self.code())
            }
            PointIndexError::NonFiniteQuery { axis } | PointIndexError::QueryOutOfRange { axis } => {
                write!(f, "{} (axis {axis})", self.code())
            }
            PointIndexError::BadK { k } => write!(f, "bad_k ({k} not in 1..={MAX_K})"),
            PointIndexError::AllocationFailed { bytes } => write!(f, "allocation_failed ({bytes} bytes)"),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for PointIndexError {}

/// The pinned squared distance `(dx*dx + dy*dy) + dz*dz`, `dx = p.x - q.x`, no fused
/// multiply-add (REQ-GEO.INDEX.KDTREE-003). Exposed so callers compare with the same value.
pub fn dist2(p: [f64; 3], q: [f64; 3]) -> f64 {
    let dx = p[0] - q[0];
    let dy = p[1] - q[1];
    let dz = p[2] - q[2];
    (dx * dx + dy * dy) + dz * dz
}

/// The leaf depth `L` of a tree over `n` points: the smallest `d` with `ceil(n / 2^d) <= 16`.
pub fn leaf_depth(n: u64) -> u32 {
    let mut d = 0u32;
    while d < 63 && (n.saturating_add((1u64 << d) - 1) >> d) > BUCKET as u64 {
        d += 1;
    }
    d
}

/// The exact bytes [`PointIndex::memory_bytes`] reports for `n` points: the struct itself,
/// 4 bytes per point of permutation and 9 bytes (split `f64` and axis `u8`) per internal node.
/// `None` above [`MAX_POINTS`].
pub fn overhead_bytes(n: u64) -> Option<u64> {
    if n > MAX_POINTS {
        return None;
    }
    let internal = (1u64 << leaf_depth(n)) - 1;
    let inline = u64::try_from(core::mem::size_of::<PointIndex<'static>>()).ok()?;
    inline.checked_add(n.checked_mul(4)?)?.checked_add(internal.checked_mul(9)?)
}

/// `(start, count)` of node `j` (0-based, left to right) at depth `d` of a tree over `n` points.
fn node_range(n: usize, d: u32, j: usize) -> (usize, usize) {
    let (mut s, mut c) = (0usize, n);
    for b in (0..d).rev() {
        let h = c / 2;
        if (j >> b) & 1 == 1 {
            s += h;
            c -= h;
        } else {
            c = h;
        }
    }
    (s, c)
}

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

fn bytes_of<T>(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX).saturating_mul(u64::try_from(core::mem::size_of::<T>()).unwrap_or(u64::MAX))
}

fn reserve_exact<T>(v: &mut Vec<T>, n: usize) -> Result<(), PointIndexError> {
    v.try_reserve_exact(n).map_err(|_| PointIndexError::AllocationFailed { bytes: bytes_of::<T>(n) })
}

fn push<T>(v: &mut Vec<T>, x: T) -> Result<(), PointIndexError> {
    v.try_reserve(1)
        .map_err(|_| PointIndexError::AllocationFailed { bytes: bytes_of::<T>(v.len().saturating_add(1)) })?;
    v.push(x);
    Ok(())
}

fn check_query(q: &[f64; 3]) -> Result<(), PointIndexError> {
    for (axis, &x) in q.iter().enumerate() {
        let axis = axis as u8;
        if !x.is_finite() {
            return Err(PointIndexError::NonFiniteQuery { axis });
        }
        if x.abs() > COORD_LIMIT {
            return Err(PointIndexError::QueryOutOfRange { axis });
        }
    }
    Ok(())
}

/// Heap key: the pinned order (totalOrder `dist2`, then index). Max-heap top = current worst.
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

/// One pending subtree of a traversal.
#[derive(Clone, Copy)]
struct Frame {
    node: usize,
    depth: u32,
    start: usize,
    count: usize,
    gap: [f64; 3],
}

const EMPTY_FRAME: Frame = Frame { node: 0, depth: 0, start: 0, count: 0, gap: [0.0; 3] };

fn bound(g: &[f64; 3]) -> f64 {
    (g[0] * g[0] + g[1] * g[1]) + g[2] * g[2]
}

/// Owned implicit k-d tree structure (permutation, splits and split axes) without borrowing the
/// source [`PointBuffer`], so it can be held in [`crate::budget::IndexCache`] without cloning
/// coordinate columns.
#[derive(Debug, Clone, PartialEq)]
pub struct PointTree {
    depth: u32,
    perm: Vec<u32>,
    splits: Vec<f64>,
    axes: Vec<u8>,
}

impl PointTree {
    /// Builds the tree over `points`. `threads` must be at least 1; the build is sequential and
    /// its result does not depend on it.
    pub fn build(points: &PointBuffer, threads: usize) -> Result<PointTree, PointIndexError> {
        if threads == 0 {
            return Err(PointIndexError::ZeroThreads);
        }
        let n = points.x.len();
        let n64 = u64::try_from(n).unwrap_or(u64::MAX);
        for (axis, col) in [(1u8, &points.y), (2u8, &points.z)] {
            let found = u64::try_from(col.len()).unwrap_or(u64::MAX);
            if found != n64 {
                return Err(PointIndexError::ColumnLength { axis, expected: n64, found });
            }
        }
        if n64 > MAX_POINTS {
            return Err(PointIndexError::TooManyPoints { count: n64 });
        }
        let cols: [&[f64]; 3] = [&points.x, &points.y, &points.z];
        for i in 0..n {
            for (axis, col) in cols.iter().enumerate() {
                let x = col.get(i).copied().ok_or(PointIndexError::Inconsistent)?;
                let (index, axis) = (i as u64, axis as u8);
                if !x.is_finite() {
                    return Err(PointIndexError::NonFinitePoint { index, axis });
                }
                if x.abs() > COORD_LIMIT {
                    return Err(PointIndexError::PointOutOfRange { index, axis });
                }
            }
        }

        let depth = leaf_depth(n64);
        let internal = (1usize << depth) - 1;
        let mut perm: Vec<u32> = Vec::new();
        let mut splits: Vec<f64> = Vec::new();
        let mut axes: Vec<u8> = Vec::new();
        reserve_exact(&mut perm, n)?;
        reserve_exact(&mut splits, internal)?;
        reserve_exact(&mut axes, internal)?;
        for i in 0..n {
            perm.push(u32::try_from(i).map_err(|_| PointIndexError::Inconsistent)?);
        }

        for i in 0..internal {
            let d = (i + 1).ilog2();
            let (s, c) = node_range(n, d, i + 1 - (1usize << d));
            let slice = perm.get_mut(s..s + c).ok_or(PointIndexError::Inconsistent)?;
            let first = *slice.first().ok_or(PointIndexError::Inconsistent)? as usize;
            let mut lo = [0.0f64; 3];
            let mut hi = [0.0f64; 3];
            for k in 0..3 {
                let v = cols[k].get(first).copied().ok_or(PointIndexError::Inconsistent)?;
                lo[k] = v;
                hi[k] = v;
            }
            for &p in slice.iter() {
                for k in 0..3 {
                    let v = cols[k].get(p as usize).copied().ok_or(PointIndexError::Inconsistent)?;
                    lo[k] = total_min(lo[k], v);
                    hi[k] = total_max(hi[k], v);
                }
            }
            let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
            let mut axis = 0usize;
            for k in 1..3 {
                if ext[k] > ext[axis] {
                    axis = k;
                }
            }
            let col = cols[axis];
            // Indexes are checked above; the fallback is never used.
            let key = |p: u32| col.get(p as usize).copied().unwrap_or(0.0);
            let h = c / 2;
            slice.select_nth_unstable_by(h, |&a, &b| key(a).total_cmp(&key(b)).then(a.cmp(&b)));
            let split = key(*slice.get(h).ok_or(PointIndexError::Inconsistent)?);
            splits.push(split);
            axes.push(axis as u8);
        }
        for j in 0..=internal {
            let (s, c) = node_range(n, depth, j);
            perm.get_mut(s..s + c).ok_or(PointIndexError::Inconsistent)?.sort_unstable();
        }
        Ok(PointTree { depth, perm, splits, axes })
    }

    /// The number of indexed points.
    pub fn len(&self) -> u64 {
        u64::try_from(self.perm.len()).unwrap_or(u64::MAX)
    }

    /// Whether the tree has no points.
    pub fn is_empty(&self) -> bool {
        self.perm.is_empty()
    }

    /// The leaf depth `L` (every leaf is at this depth; 0 when the root is the only leaf).
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// The largest leaf size, [`BUCKET`].
    pub fn bucket(&self) -> usize {
        BUCKET
    }

    /// Point indexes in leaf order (each leaf's range ascending).
    pub fn perm(&self) -> &[u32] {
        &self.perm
    }

    /// The split value of each internal node, in node order.
    pub fn splits(&self) -> &[f64] {
        &self.splits
    }

    /// The split axis (0 = x, 1 = y, 2 = z) of each internal node, in node order.
    pub fn axes(&self) -> &[u8] {
        &self.axes
    }

    /// Heap bytes allocated for the permutation, split and axis buffers.
    pub(crate) fn heap_bytes(&self) -> u64 {
        bytes_of::<u32>(self.perm.capacity())
            .saturating_add(bytes_of::<f64>(self.splits.capacity()))
            .saturating_add(bytes_of::<u8>(self.axes.capacity()))
    }

    /// Bytes held by this tree: the struct itself plus the exact capacity of its permutation,
    /// split and axis buffers.
    pub fn memory_bytes(&self) -> u64 {
        bytes_of::<PointTree>(1).saturating_add(self.heap_bytes())
    }

    /// The pinned serialisation (AC-3): `FIDKDT01`; point count (u64 LE); leaf depth and
    /// bucket size (u32 LE each); the split values (f64 bits, u64 LE each); the axis bytes; the
    /// permutation (u32 LE each).
    pub fn to_bytes(&self) -> Result<Vec<u8>, PointIndexError> {
        let size = 24u64
            .checked_add(bytes_of::<f64>(self.splits.len()))
            .and_then(|x| x.checked_add(bytes_of::<u8>(self.axes.len())))
            .and_then(|x| x.checked_add(bytes_of::<u32>(self.perm.len())))
            .ok_or(PointIndexError::AllocationFailed { bytes: u64::MAX })?;
        let size_us = usize::try_from(size).map_err(|_| PointIndexError::AllocationFailed { bytes: size })?;
        let mut out: Vec<u8> = Vec::new();
        out.try_reserve_exact(size_us).map_err(|_| PointIndexError::AllocationFailed { bytes: size })?;
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.len().to_le_bytes());
        out.extend_from_slice(&self.depth.to_le_bytes());
        out.extend_from_slice(&(BUCKET as u32).to_le_bytes());
        for s in &self.splits {
            out.extend_from_slice(&s.to_bits().to_le_bytes());
        }
        out.extend_from_slice(&self.axes);
        for p in &self.perm {
            out.extend_from_slice(&p.to_le_bytes());
        }
        Ok(out)
    }

    fn check_points(&self, points: &PointBuffer) -> Result<(), PointIndexError> {
        let n = self.perm.len();
        if points.x.len() != n || points.y.len() != n || points.z.len() != n {
            return Err(PointIndexError::Inconsistent);
        }
        Ok(())
    }

    fn point(points: &PointBuffer, p: u32) -> Result<[f64; 3], PointIndexError> {
        points.point(u64::from(p)).ok_or(PointIndexError::Inconsistent)
    }

    fn node(&self, i: usize) -> Result<(f64, usize), PointIndexError> {
        let s = *self.splits.get(i).ok_or(PointIndexError::Inconsistent)?;
        let a = *self.axes.get(i).ok_or(PointIndexError::Inconsistent)? as usize;
        if a > 2 {
            return Err(PointIndexError::Inconsistent);
        }
        Ok((s, a))
    }

    /// Depth-first traversal shared by the three queries. `prune(lower bound)` says whether a
    /// subtree can be skipped; `leaf(points)` consumes a leaf's indexes. For box queries
    /// `boxq` restricts the descent by exact comparisons with the split values instead.
    fn traverse(
        &self,
        q: &[f64; 3],
        boxq: Option<(&[f64; 3], &[f64; 3])>,
        prune: &dyn Fn(f64) -> bool,
        leaf: &mut dyn FnMut(&[u32]) -> Result<(), PointIndexError>,
    ) -> Result<(), PointIndexError> {
        let mut stack = [EMPTY_FRAME; STACK];
        stack[0] = Frame { node: 0, depth: 0, start: 0, count: self.perm.len(), gap: [0.0; 3] };
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let f = *stack.get(sp).ok_or(PointIndexError::Inconsistent)?;
            if boxq.is_none() && prune(bound(&f.gap)) {
                continue;
            }
            if f.depth == self.depth {
                let pts = self.perm.get(f.start..f.start + f.count).ok_or(PointIndexError::Inconsistent)?;
                leaf(pts)?;
                continue;
            }
            let (s, a) = self.node(f.node)?;
            let h = f.count / 2;
            let left = Frame { node: 2 * f.node + 1, depth: f.depth + 1, start: f.start, count: h, gap: f.gap };
            let right =
                Frame { node: 2 * f.node + 2, depth: f.depth + 1, start: f.start + h, count: f.count - h, gap: f.gap };
            let (near, far, far_gap) = if let Some((lo, hi)) = boxq {
                // Exact: left points are <= s and right points >= s on axis a.
                let (go_left, go_right) = (lo[a] <= s, hi[a] >= s);
                let mut pushed = [None, None];
                if go_right {
                    pushed[0] = Some(right);
                }
                if go_left {
                    pushed[1] = Some(left);
                }
                for fr in pushed.into_iter().flatten() {
                    let slot = stack.get_mut(sp).ok_or(PointIndexError::Inconsistent)?;
                    *slot = fr;
                    sp += 1;
                }
                continue;
            } else if q[a] < s {
                // Far = right: every right point has p_a >= s > q_a.
                (left, right, s - q[a])
            } else {
                // Far = left: every left point has p_a <= s <= q_a.
                (right, left, q[a] - s)
            };
            let mut far = far;
            if far_gap > far.gap[a] {
                far.gap[a] = far_gap;
            }
            for fr in [far, near] {
                let slot = stack.get_mut(sp).ok_or(PointIndexError::Inconsistent)?;
                *slot = fr;
                sp += 1;
            }
        }
        Ok(())
    }

    /// The `min(k, n)` points in `points` nearest to `q` by `(dist2, index)`, ascending
    /// (REQ-GEO.INDEX.KDTREE-001, -002). `points` must be the buffer this tree was built from.
    pub fn knn(&self, points: &PointBuffer, q: [f64; 3], k: usize) -> Result<Vec<Neighbour>, PointIndexError> {
        self.check_points(points)?;
        check_query(&q)?;
        if k == 0 || k > MAX_K {
            return Err(PointIndexError::BadK { k: u64::try_from(k).unwrap_or(u64::MAX) });
        }
        let mut heap: BinaryHeap<Key> = BinaryHeap::new();
        heap.try_reserve_exact(k + 1)
            .map_err(|_| PointIndexError::AllocationFailed { bytes: bytes_of::<Key>(k + 1) })?;
        // The current k-th key, shared with the pruning test.
        let worst = core::cell::Cell::new(None::<f64>);
        let prune = |lb: f64| worst.get().is_some_and(|w| lb > w);
        let mut leaf = |pts: &[u32]| -> Result<(), PointIndexError> {
            for &p in pts {
                let key = Key(dist2(Self::point(points, p)?, q), p);
                if heap.len() < k {
                    heap.push(key);
                } else if heap.peek().is_some_and(|top| key < *top) {
                    heap.push(key);
                    heap.pop();
                }
                if heap.len() == k {
                    worst.set(heap.peek().map(|t| t.0));
                }
            }
            Ok(())
        };
        self.traverse(&q, None, &prune, &mut leaf)?;
        let mut out: Vec<Neighbour> = Vec::new();
        reserve_exact(&mut out, heap.len())?;
        out.extend(heap.into_sorted_vec().into_iter().map(|Key(d, i)| Neighbour { index: u64::from(i), dist2: d }));
        Ok(out)
    }

    /// Every point in `points` with `dist2(p, q) <= r*r`, sorted by `(dist2, index)`.
    pub fn radius(&self, points: &PointBuffer, q: [f64; 3], r: f64) -> Result<Vec<Neighbour>, PointIndexError> {
        self.check_points(points)?;
        check_query(&q)?;
        if !r.is_finite() || r < 0.0 || r > COORD_LIMIT {
            return Err(PointIndexError::BadRadius);
        }
        let r2 = r * r;
        let mut out: Vec<Neighbour> = Vec::new();
        let prune = |lb: f64| lb > r2;
        let mut leaf = |pts: &[u32]| -> Result<(), PointIndexError> {
            for &p in pts {
                let d = dist2(Self::point(points, p)?, q);
                if d <= r2 {
                    push(&mut out, Neighbour { index: u64::from(p), dist2: d })?;
                }
            }
            Ok(())
        };
        self.traverse(&q, None, &prune, &mut leaf)?;
        out.sort_unstable_by(|a, b| a.dist2.total_cmp(&b.dist2).then(a.index.cmp(&b.index)));
        Ok(out)
    }

    /// Every point in `points` inside `b` (boundaries included, numeric comparisons), ascending by
    /// index.
    pub fn in_box(&self, points: &PointBuffer, b: &Aabb) -> Result<Vec<u64>, PointIndexError> {
        self.check_points(points)?;
        let (lo, hi) = (b.min(), b.max());
        let mut out: Vec<u64> = Vec::new();
        let mut leaf = |pts: &[u32]| -> Result<(), PointIndexError> {
            for &p in pts {
                let x = Self::point(points, p)?;
                if (0..3).all(|k| lo[k] <= x[k] && x[k] <= hi[k]) {
                    push(&mut out, u64::from(p))?;
                }
            }
            Ok(())
        };
        self.traverse(&[0.0; 3], Some((&lo, &hi)), &|_| false, &mut leaf)?;
        out.sort_unstable();
        Ok(out)
    }
}

/// An implicit k-d tree over a borrowed point buffer (`GEO.INDEX.KDTREE`).
#[derive(Debug, Clone, PartialEq)]
pub struct PointIndex<'a> {
    points: &'a PointBuffer,
    tree: PointTree,
}

impl<'a> PointIndex<'a> {
    /// Builds the index over `points`. `threads` must be at least 1; the build is sequential
    /// and its result does not depend on it.
    pub fn build(points: &'a PointBuffer, threads: usize) -> Result<PointIndex<'a>, PointIndexError> {
        let tree = PointTree::build(points, threads)?;
        Ok(PointIndex { points, tree })
    }

    // ------------------------------------------------------------------ accessors

    /// The indexed point buffer.
    pub fn points(&self) -> &'a PointBuffer {
        self.points
    }

    /// The underlying owned tree structure.
    pub fn tree(&self) -> &PointTree {
        &self.tree
    }

    /// Consumes the borrowed wrapper and returns the owned [`PointTree`] without reallocating.
    pub fn into_tree(self) -> PointTree {
        self.tree
    }

    /// The number of points.
    pub fn len(&self) -> u64 {
        self.tree.len()
    }

    /// Whether the index has no points.
    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    /// The leaf depth `L` (every leaf is at this depth; 0 when the root is the only leaf).
    pub fn depth(&self) -> u32 {
        self.tree.depth()
    }

    /// The largest leaf size, [`BUCKET`].
    pub fn bucket(&self) -> usize {
        self.tree.bucket()
    }

    /// Point indexes in leaf order (each leaf's range ascending).
    pub fn perm(&self) -> &[u32] {
        self.tree.perm()
    }

    /// The split value of each internal node, in node order.
    pub fn splits(&self) -> &[f64] {
        self.tree.splits()
    }

    /// The split axis (0 = x, 1 = y, 2 = z) of each internal node, in node order.
    pub fn axes(&self) -> &[u8] {
        self.tree.axes()
    }

    /// Bytes held by the index beyond the coordinate buffer: the struct itself plus the exact
    /// capacity of the permutation, split and axis allocations (REQ-GEO.INDEX.KDTREE-005).
    pub fn memory_bytes(&self) -> u64 {
        bytes_of::<PointIndex<'static>>(1).saturating_add(self.tree.heap_bytes())
    }

    /// The pinned serialisation (AC-3): `FIDKDT01`; point count (u64 LE); leaf depth and
    /// bucket size (u32 LE each); the split values (f64 bits, u64 LE each); the axis bytes; the
    /// permutation (u32 LE each).
    pub fn to_bytes(&self) -> Result<Vec<u8>, PointIndexError> {
        self.tree.to_bytes()
    }

    // ------------------------------------------------------------------ queries

    /// The `min(k, n)` points nearest to `q` by `(dist2, index)`, ascending
    /// (REQ-GEO.INDEX.KDTREE-001, -002). `k` must be in `1..=1024`.
    pub fn knn(&self, q: [f64; 3], k: usize) -> Result<Vec<Neighbour>, PointIndexError> {
        self.tree.knn(self.points, q, k)
    }

    /// Every point with `dist2(p, q) <= r*r`, sorted by `(dist2, index)`. `r` must be finite,
    /// `>= 0` and at most `COORD_LIMIT`.
    pub fn radius(&self, q: [f64; 3], r: f64) -> Result<Vec<Neighbour>, PointIndexError> {
        self.tree.radius(self.points, q, r)
    }

    /// Every point inside `b` (boundaries included, numeric comparisons), ascending by index.
    pub fn in_box(&self, b: &Aabb) -> Result<Vec<u64>, PointIndexError> {
        self.tree.in_box(self.points, b)
    }
}
