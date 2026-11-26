//! Cross-chunk neighbour queries (spec node `GEO.OOC.QUERY`): k-nearest, radius and box
//! queries over a chunked cloud whose results are exactly those of the in-memory GEO.INDEX
//! k-d tree ([`fid_geo_index::PointIndex`]) on the same cloud: the same original indexes, in
//! the same order, with bit-identical squared distances (REQ-GEO.OOC.QUERY-001).
//!
//! # Chunk bounds (pinned, exact)
//!
//! A chunk's points all lie in its octree cell, so the cell bounds every point of the chunk.
//! The partition quantises a coordinate with three rounded IEEE 754 operations
//! ([`Grid::quantise`]), so the real interval of a cell is not a simple expression of the
//! manifest bounds. It is found exactly instead: `quantise` is non-decreasing in `v` (each
//! of its steps is a monotone rounding of a monotone function, and the clamps are monotone),
//! so on each axis the points of a cell whose quantised range is `a ..= b` satisfy
//! `lo(a) <= v <= hi(b)`, where `lo(a)` is the smallest `f64` in the manifest bounds with
//! `quantise(v) >= a` and `hi(b)` the largest with `quantise(v) <= b`. Both are found by a
//! binary search over the `f64` values in totalOrder (at most 64 steps; `a = 0` gives the
//! manifest minimum and `b = 2^21 - 1` the maximum without a search). Every point of a
//! verified manifest lies in the manifest bounds and in its cell (GEO.OOC.STREAM checks it),
//! so these are true bounds, with no tolerance.
//!
//! # Lower bound (pinned, exact)
//!
//! The lower bound of a chunk for a query point `q` is `(gx*gx + gy*gy) + gz*gz` with the
//! per-axis gap `g_k = lo_k - q_k` when `q_k < lo_k`, `q_k - hi_k` when `q_k > hi_k`, else 0,
//! no fused multiply-add. For a point `p` of the chunk, `|p_k - q_k| >= g_k` exactly and
//! rounding is monotone, so `|fl(p_k - q_k)| >= g_k`, and the computed squared distance of
//! GEO.INDEX ([`fid_geo_index::dist2`]) is at least the lower bound. This is the argument the
//! k-d tree uses for its own pruning; no inflation is needed and none is used.
//!
//! # Queries (REQ-GEO.OOC.QUERY-002)
//!
//! * The chunks are visited in ascending `(lower bound in IEEE totalOrder, raw cell code)`.
//! * `knn(q, k)`: each visited chunk's [`PointTree`] returns its `min(k, count)` nearest by
//!   `(dist2, local index)`; the local index maps to the original index, and a bounded
//!   max-heap keeps the `k` smallest `(dist2 totalOrder, original index)`. The visit stops at
//!   the first chunk whose lower bound is greater than the current `k`-th distance: every
//!   later chunk has a lower bound at least as large, so none of its points can enter the
//!   result, ties included (a tie needs `dist2 == k-th`, hence a lower bound `<= k-th`).
//! * `radius(q, r)`: the chunks with lower bound `<= r*r` (`r*r` rounded once, as in
//!   GEO.INDEX), merged and sorted by `(dist2, original index)`.
//! * `in_box(b)`: the chunks whose cell bounds meet `b` (numeric comparisons), merged and
//!   sorted by original index. A box query has no query point, so every chunk it meets has
//!   lower bound 0 and the visit order is the cell-code order.
//!
//! Inside a chunk the points are ordered by original index (the chunk format), so the local
//! index order is the original index order and per-chunk tie breaking agrees with the
//! in-memory tree's.
//!
//! Query arguments are checked as GEO.INDEX checks them, in the same order and with the same
//! [`PointIndexError`] values, before any chunk is loaded: the query point (finite, at most
//! [`COORD_LIMIT`] in magnitude, per axis x, y, z), then `1 <= k <= 1024`, or the radius.
//!
//! # Per-chunk indexes (REQ-GEO.OOC.QUERY-003)
//!
//! Each chunk's [`PointTree`] is built on its first use through a GEO.INDEX.BUDGET
//! [`IndexCache`] keyed by the chunk's store address (its content digest), so it is built at
//! most once while it stays cached and every tree counts against the index memory budget
//! ([`QueryConfig::index_budget`]). A tree that does not fit the budget is refused with
//! [`BudgetError::BudgetExceeded`] before it is built. Chunks themselves are loaded through
//! the [`ChunkCache`], which verifies them against the manifest.
//!
//! # Verification
//!
//! A query result equals the in-memory result only if the manifest holds every original
//! index exactly once and every point lies in its cell. As for spatial streaming, a
//! [`ChunkedIndex`] built from an [`Provenance::Untrusted`] manifest refuses every query
//! with [`OocError::UnverifiedManifest`] until [`ChunkedIndex::verify`] has run the
//! GEO.OOC.STREAM original-order proof without error.
//!
//! # Batches and threads (REQ-GEO.OOC.QUERY-004)
//!
//! The batch queries split the query list into `threads` contiguous ranges, run each range in
//! order on its own scoped thread, and return the results in query order. Each result is a
//! pure function of the query and the cloud, so the output does not depend on `threads`. If
//! a thread cannot be started its range runs on the calling thread. On error the batch
//! returns the error of the first failing query in query order. Which chunks and trees are
//! resident can depend on scheduling, and so can [`OocError::CacheExhausted`] when the
//! chunk cache budget is too small for the threads' pins; result values never do.

// Per-axis loops index several 3-arrays at once, mirroring the pinned formulas above.
#![allow(clippy::needless_range_loop)]

use crate::cache::ChunkCache;
use crate::cell::{CellCode, Grid, GRID_CELLS, MAX_DEPTH};
use crate::error::OocError;
use crate::format::ChunkManifest;
use crate::store::ChunkStore;
use crate::stream::{ChunkStream, Provenance, StreamConfig, StreamOrder};
use core::cmp::Ordering;
use core::fmt;
use fid_geo_index::{
    BudgetError, BuildParams, IndexCache, MemoryBudget, Neighbour, PointIndexError, PointTree, COORD_LIMIT, MAX_K,
};
use fid_geo_types::digest::ElementDigest;
use fid_geo_types::Aabb;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Default index memory budget, 1 GiB [unverified]; to become a user setting.
pub const DEFAULT_INDEX_BUDGET: u64 = 1 << 30;
/// Largest `threads` a batch query accepts.
pub const MAX_QUERY_THREADS: usize = 256;

/// Largest quantised coordinate.
const QMAX: u64 = GRID_CELLS - 1;

/// Every error of a cross-chunk query. Errors are values; nothing here panics.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryError {
    /// A chunked-cloud error: manifest validation, chunk loads and integrity
    /// ([`OocError::ChunkIntegrity`]), the chunk cache budget, an unverified manifest,
    /// allocation failures, invalid parameters.
    Ooc(OocError),
    /// A query argument error, exactly as GEO.INDEX returns it (and the GEO.INDEX build
    /// errors of a chunk, with point indexes mapped to original indexes).
    Index(PointIndexError),
    /// The index memory budget refused a per-chunk tree (GEO.INDEX.BUDGET).
    Budget(BudgetError),
}

impl QueryError {
    /// A stable machine-readable code (the code of the wrapped error).
    pub fn code(&self) -> &'static str {
        match self {
            QueryError::Ooc(e) => e.code(),
            QueryError::Index(e) => e.code(),
            QueryError::Budget(e) => e.code(),
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::Ooc(e) => write!(f, "{e}"),
            QueryError::Index(e) => write!(f, "{e}"),
            QueryError::Budget(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for QueryError {}

impl From<OocError> for QueryError {
    fn from(e: OocError) -> QueryError {
        QueryError::Ooc(e)
    }
}

impl From<PointIndexError> for QueryError {
    fn from(e: PointIndexError) -> QueryError {
        QueryError::Index(e)
    }
}

/// Configuration of a [`ChunkedIndex`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryConfig {
    /// Byte budget of the per-chunk index cache (GEO.INDEX.BUDGET `MemoryBudget`).
    pub index_budget: u64,
}

impl Default for QueryConfig {
    fn default() -> QueryConfig {
        QueryConfig { index_budget: DEFAULT_INDEX_BUDGET }
    }
}

/// A snapshot of a [`ChunkedIndex`]'s counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueryStats {
    /// Per-chunk trees built (REQ-GEO.OOC.QUERY-003, AC-4).
    pub index_builds: u64,
    /// Per-chunk trees served from the index cache.
    pub index_hits: u64,
    /// Trees evicted to stay within the index budget.
    pub index_evictions: u64,
    /// Bytes of the cached trees.
    pub index_bytes: u64,
    /// Distinct chunks (by store address) any query has visited.
    pub chunks_touched: u64,
    /// Chunk visits over all queries.
    pub chunk_visits: u64,
}

/// The real bounds of one chunk's cell: every point of the chunk lies in it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bounds {
    lo: [f64; 3],
    hi: [f64; 3],
}

struct IndexState {
    cache: IndexCache,
    touched: BTreeSet<[u8; 32]>,
    visits: u64,
}

/// Cross-chunk k-nearest, radius and box queries over one chunked cloud. See the
/// [module documentation](self).
pub struct ChunkedIndex<'a, S: ChunkStore> {
    manifest: &'a ChunkManifest,
    store: &'a S,
    cache: &'a ChunkCache,
    bounds: Vec<Bounds>,
    index_cache: Mutex<IndexState>,
    verified: bool,
}

impl<S: ChunkStore> fmt::Debug for ChunkedIndex<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChunkedIndex")
            .field("n_points", &self.manifest.n_points)
            .field("chunks", &self.manifest.chunks.len())
            .field("verified", &self.verified)
            .field("stats", &self.stats())
            .finish()
    }
}

/// Order-preserving key of a finite `f64` in IEEE 754 totalOrder.
fn key_of(v: f64) -> u64 {
    let b = v.to_bits();
    if b >> 63 == 1 {
        !b
    } else {
        b | (1 << 63)
    }
}

fn from_key(k: u64) -> f64 {
    if k >> 63 == 1 {
        f64::from_bits(k & !(1 << 63))
    } else {
        f64::from_bits(!k)
    }
}

/// The smallest `v` in `[lo, hi]` (totalOrder) with `quantise(v) >= a`, or `None`.
fn first_at_least(grid: &Grid, k: usize, lo: f64, hi: f64, a: u64) -> Option<f64> {
    let (mut l, mut h) = (key_of(lo), key_of(hi));
    if grid.quantise(hi, k) < a {
        return None;
    }
    // Invariant: quantise(from_key(h)) >= a; every key below l fails.
    while l < h {
        let m = l + (h - l) / 2;
        if grid.quantise(from_key(m), k) >= a {
            h = m;
        } else {
            l = m + 1;
        }
    }
    Some(from_key(h))
}

/// The largest `v` in `[lo, hi]` (totalOrder) with `quantise(v) <= b`, or `None`.
fn last_at_most(grid: &Grid, k: usize, lo: f64, hi: f64, b: u64) -> Option<f64> {
    let (mut l, mut h) = (key_of(lo), key_of(hi));
    if grid.quantise(lo, k) > b {
        return None;
    }
    // Invariant: quantise(from_key(l)) <= b; every key above h fails.
    while l < h {
        let m = l + (h - l).div_ceil(2);
        if grid.quantise(from_key(m), k) <= b {
            l = m;
        } else {
            h = m - 1;
        }
    }
    Some(from_key(l))
}

/// Axis `k`'s top `depth` bits of the quantised coordinate of every point in `cell`.
fn axis_prefix(cell: CellCode, k: u32) -> u64 {
    let (d, p) = (u32::from(cell.depth()), cell.prefix());
    (0..d).fold(0u64, |acc, j| acc | (((p >> (3 * j + k)) & 1) << j))
}

/// The bounds of every cell of `m`, sharing searches between cells on the same planes.
fn cell_bounds(m: &ChunkManifest, grid: &Grid) -> Result<Vec<Bounds>, OocError> {
    let (mlo, mhi) = (m.aabb.min(), m.aabb.max());
    let mut out: Vec<Bounds> = Vec::new();
    out.try_reserve_exact(m.chunks.len()).map_err(|_| OocError::AllocationFailed {
        needed: (m.chunks.len() as u64).saturating_mul(core::mem::size_of::<Bounds>() as u64),
    })?;
    let mut lows: BTreeMap<(usize, u64), f64> = BTreeMap::new();
    let mut highs: BTreeMap<(usize, u64), f64> = BTreeMap::new();
    for e in &m.chunks {
        let d = u32::from(e.cell.depth());
        let shift = u32::from(MAX_DEPTH) - d;
        let mut b = Bounds { lo: mlo, hi: mhi };
        for k in 0..3usize {
            let c = axis_prefix(e.cell, k as u32);
            let a = c << shift;
            let z = ((c + 1) << shift) - 1;
            if a > 0 {
                // No value of the bounds reaches the cell on this axis (a zero extent): such
                // a cell holds no point of a verified manifest, and the manifest minimum is
                // still a true bound.
                b.lo[k] =
                    *lows.entry((k, a)).or_insert_with(|| first_at_least(grid, k, mlo[k], mhi[k], a).unwrap_or(mlo[k]));
            }
            if z < QMAX {
                b.hi[k] =
                    *highs.entry((k, z)).or_insert_with(|| last_at_most(grid, k, mlo[k], mhi[k], z).unwrap_or(mhi[k]));
            }
        }
        out.push(b);
    }
    Ok(out)
}

/// The pinned lower bound of the squared distance from `q` to any point in `b`.
fn lower_bound(b: &Bounds, q: &[f64; 3]) -> f64 {
    let mut g = [0.0f64; 3];
    for k in 0..3 {
        if q[k] < b.lo[k] {
            g[k] = b.lo[k] - q[k];
        } else if q[k] > b.hi[k] {
            g[k] = q[k] - b.hi[k];
        }
    }
    (g[0] * g[0] + g[1] * g[1]) + g[2] * g[2]
}

/// GEO.INDEX's query point check, same order and values.
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

fn alloc_err<T>(n: usize) -> OocError {
    OocError::AllocationFailed { needed: (n as u64).saturating_mul(core::mem::size_of::<T>() as u64) }
}

fn push<T>(v: &mut Vec<T>, x: T) -> Result<(), OocError> {
    v.try_reserve(1).map_err(|_| alloc_err::<T>(v.len().saturating_add(1)))?;
    v.push(x);
    Ok(())
}

/// Heap key: (totalOrder `dist2`, original index). Max-heap top = current worst.
#[derive(Clone, Copy)]
struct Key(f64, u64);

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

/// Maps a chunk-local point index of a GEO.INDEX build error to the original index.
fn map_build_error(e: PointIndexError, index: &[u64]) -> PointIndexError {
    let orig = |i: u64| usize::try_from(i).ok().and_then(|j| index.get(j).copied());
    match e {
        PointIndexError::NonFinitePoint { index: i, axis } => match orig(i) {
            Some(index) => PointIndexError::NonFinitePoint { index, axis },
            None => PointIndexError::Inconsistent,
        },
        PointIndexError::PointOutOfRange { index: i, axis } => match orig(i) {
            Some(index) => PointIndexError::PointOutOfRange { index, axis },
            None => PointIndexError::Inconsistent,
        },
        other => other,
    }
}

impl<'a, S: ChunkStore> ChunkedIndex<'a, S> {
    /// Queries over the cloud of `manifest`, loading chunks from `store` through `cache`.
    /// Validates the manifest (digest included) and computes the cell bounds; does no I/O.
    /// `provenance` sets the initial [`ChunkedIndex::verified`] state, as for
    /// [`ChunkStream`].
    ///
    /// Errors: the manifest's validation errors ([`OocError::CorruptManifest`]),
    /// [`OocError::BoundsTooLarge`], [`OocError::AllocationFailed`].
    pub fn new(
        manifest: &'a ChunkManifest,
        store: &'a S,
        cache: &'a ChunkCache,
        config: QueryConfig,
        provenance: Provenance,
    ) -> Result<ChunkedIndex<'a, S>, QueryError> {
        manifest.validate()?;
        let grid = Grid::new(&manifest.aabb)?;
        let bounds = cell_bounds(manifest, &grid)?;
        Ok(ChunkedIndex {
            manifest,
            store,
            cache,
            bounds,
            index_cache: Mutex::new(IndexState {
                cache: IndexCache::new(MemoryBudget::new(config.index_budget)),
                touched: BTreeSet::new(),
                visits: 0,
            }),
            verified: provenance == Provenance::TrustedWriter,
        })
    }

    /// The manifest.
    pub fn manifest(&self) -> &ChunkManifest {
        self.manifest
    }

    /// Whether queries are allowed: the manifest is trusted by provenance or was proven by
    /// [`ChunkedIndex::verify`].
    pub fn verified(&self) -> bool {
        self.verified
    }

    /// Runs the GEO.OOC.STREAM original-order proof over the manifest (every index `0..n`
    /// exactly once, every point inside its cell) through this index's store and cache, and
    /// records the result. Returns at once if already verified. On error the index stays
    /// unverified.
    pub fn verify(&mut self) -> Result<(), QueryError> {
        if self.verified {
            return Ok(());
        }
        let store: &dyn ChunkStore = self.store;
        let cfg = StreamConfig::new(StreamOrder::Original);
        let mut s = ChunkStream::new(self.manifest, store, self.cache, None, cfg, Provenance::Untrusted)?;
        s.verify()?;
        self.verified = s.verified();
        Ok(())
    }

    /// The real bounds `(min, max)` of the cell of chunk `chunk` (manifest position), which
    /// hold every point of the chunk (see the module documentation).
    pub fn chunk_bounds(&self, chunk: usize) -> Option<([f64; 3], [f64; 3])> {
        self.bounds.get(chunk).map(|b| (b.lo, b.hi))
    }

    /// A snapshot of the counters.
    pub fn stats(&self) -> QueryStats {
        let st = self.lock();
        QueryStats {
            index_builds: st.cache.build_count(),
            index_hits: st.cache.hit_count(),
            index_evictions: st.cache.eviction_count(),
            index_bytes: st.cache.used_bytes(),
            chunks_touched: st.touched.len() as u64,
            chunk_visits: st.visits,
        }
    }

    // Nothing panics while the lock is held; recover a poisoned lock as ChunkCache does.
    fn lock(&self) -> MutexGuard<'_, IndexState> {
        self.index_cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn ready(&self) -> Result<(), QueryError> {
        if self.verified {
            Ok(())
        } else {
            Err(QueryError::Ooc(OocError::UnverifiedManifest))
        }
    }

    /// Chunk positions sorted by `(lower bound totalOrder, raw cell code)`, with their lower
    /// bounds.
    fn order(&self, q: &[f64; 3]) -> Result<Vec<(f64, u64, usize)>, QueryError> {
        let mut v: Vec<(f64, u64, usize)> = Vec::new();
        v.try_reserve_exact(self.bounds.len()).map_err(|_| alloc_err::<(f64, u64, usize)>(self.bounds.len()))?;
        for (i, (b, e)) in self.bounds.iter().zip(&self.manifest.chunks).enumerate() {
            v.push((lower_bound(b, q), e.cell.raw(), i));
        }
        v.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        Ok(v)
    }

    /// Visits chunk `pos`: pins it and gets (or builds) its tree, then runs `f`.
    fn visit<T>(
        &self,
        pos: usize,
        f: impl FnOnce(&PointTree, &crate::format::Chunk) -> Result<T, PointIndexError>,
    ) -> Result<T, QueryError> {
        let store: &dyn ChunkStore = self.store;
        let handle = self.cache.get(store, self.manifest, pos)?;
        let address = handle.address();
        let tree: Arc<PointTree> = {
            let mut st = self.lock();
            st.visits = st.visits.saturating_add(1);
            st.touched.insert(address);
            let looked = st.cache.get_or_build_point(ElementDigest(address), &handle.points, BuildParams::kdtree());
            match looked {
                Ok(l) => match l.index.as_point() {
                    Some(t) => Arc::clone(t),
                    None => return Err(QueryError::Index(PointIndexError::Inconsistent)),
                },
                Err(BudgetError::PointIndex(e)) => return Err(QueryError::Index(map_build_error(e, &handle.index))),
                Err(e) => return Err(QueryError::Budget(e)),
            }
        };
        f(tree.as_ref(), handle.chunk()).map_err(QueryError::Index)
    }

    /// The `min(k, n)` points nearest to `q` by `(dist2, original index)`, ascending: the
    /// result of GEO.INDEX `PointIndex::knn` on the in-memory cloud (REQ-GEO.OOC.QUERY-001).
    pub fn knn(&self, q: [f64; 3], k: usize) -> Result<Vec<Neighbour>, QueryError> {
        self.ready()?;
        check_query(&q)?;
        if k == 0 || k > MAX_K {
            return Err(QueryError::Index(PointIndexError::BadK { k: u64::try_from(k).unwrap_or(u64::MAX) }));
        }
        let order = self.order(&q)?;
        let mut heap: BinaryHeap<Key> = BinaryHeap::new();
        heap.try_reserve_exact(k + 1).map_err(|_| alloc_err::<Key>(k + 1))?;
        for (lb, _, pos) in order {
            if heap.len() == k && heap.peek().is_some_and(|w| lb > w.0) {
                break;
            }
            let local = self.visit(pos, |tree, chunk| {
                let found = tree.knn(&chunk.points, q, k)?;
                let mut out: Vec<Key> = Vec::new();
                out.try_reserve_exact(found.len()).map_err(|_| PointIndexError::AllocationFailed {
                    bytes: (found.len() as u64).saturating_mul(16),
                })?;
                for n in found {
                    let i = usize::try_from(n.index).map_err(|_| PointIndexError::Inconsistent)?;
                    let orig = *chunk.index.get(i).ok_or(PointIndexError::Inconsistent)?;
                    out.push(Key(n.dist2, orig));
                }
                Ok(out)
            })?;
            for key in local {
                if heap.len() < k {
                    heap.push(key);
                } else if heap.peek().is_some_and(|top| key < *top) {
                    heap.push(key);
                    heap.pop();
                }
            }
        }
        let mut out: Vec<Neighbour> = Vec::new();
        out.try_reserve_exact(heap.len()).map_err(|_| alloc_err::<Neighbour>(heap.len()))?;
        out.extend(heap.into_sorted_vec().into_iter().map(|Key(d, i)| Neighbour { index: i, dist2: d }));
        Ok(out)
    }

    /// Every point with `dist2(p, q) <= r*r`, sorted by `(dist2, original index)`: the result
    /// of GEO.INDEX `PointIndex::radius` on the in-memory cloud.
    pub fn radius(&self, q: [f64; 3], r: f64) -> Result<Vec<Neighbour>, QueryError> {
        self.ready()?;
        check_query(&q)?;
        if !r.is_finite() || r < 0.0 || r > COORD_LIMIT {
            return Err(QueryError::Index(PointIndexError::BadRadius));
        }
        let r2 = r * r;
        let order = self.order(&q)?;
        let mut out: Vec<Neighbour> = Vec::new();
        for (lb, _, pos) in order {
            if lb > r2 {
                break;
            }
            let found = self.visit(pos, |tree, chunk| {
                let mut v = tree.radius(&chunk.points, q, r)?;
                for n in v.iter_mut() {
                    let i = usize::try_from(n.index).map_err(|_| PointIndexError::Inconsistent)?;
                    n.index = *chunk.index.get(i).ok_or(PointIndexError::Inconsistent)?;
                }
                Ok(v)
            })?;
            out.try_reserve(found.len()).map_err(|_| alloc_err::<Neighbour>(out.len().saturating_add(found.len())))?;
            out.extend(found);
        }
        out.sort_unstable_by(|a, b| a.dist2.total_cmp(&b.dist2).then(a.index.cmp(&b.index)));
        Ok(out)
    }

    /// Every point inside `b` (boundaries included, numeric comparisons), ascending by
    /// original index: the result of GEO.INDEX `PointIndex::in_box` on the in-memory cloud.
    pub fn in_box(&self, b: &Aabb) -> Result<Vec<u64>, QueryError> {
        self.ready()?;
        let (lo, hi) = (b.min(), b.max());
        let mut hits: Vec<(u64, usize)> = Vec::new();
        for (i, (cb, e)) in self.bounds.iter().zip(&self.manifest.chunks).enumerate() {
            if (0..3).all(|k| lo[k] <= cb.hi[k] && cb.lo[k] <= hi[k]) {
                push(&mut hits, (e.cell.raw(), i))?;
            }
        }
        hits.sort_unstable();
        let mut out: Vec<u64> = Vec::new();
        for (_, pos) in hits {
            let found = self.visit(pos, |tree, chunk| {
                let mut v = tree.in_box(&chunk.points, b)?;
                for x in v.iter_mut() {
                    let i = usize::try_from(*x).map_err(|_| PointIndexError::Inconsistent)?;
                    *x = *chunk.index.get(i).ok_or(PointIndexError::Inconsistent)?;
                }
                Ok(v)
            })?;
            out.try_reserve(found.len()).map_err(|_| alloc_err::<u64>(out.len().saturating_add(found.len())))?;
            out.extend(found);
        }
        out.sort_unstable();
        Ok(out)
    }
}

impl<S: ChunkStore + Sync> ChunkedIndex<'_, S> {
    /// [`ChunkedIndex::knn`] for every query point, on `threads` threads (1 to
    /// [`MAX_QUERY_THREADS`]); results in query order, independent of `threads`.
    pub fn batch_knn(&self, queries: &[[f64; 3]], k: usize, threads: usize) -> Result<Vec<Vec<Neighbour>>, QueryError> {
        run_batch(queries, threads, |q| self.knn(*q, k))
    }

    /// [`ChunkedIndex::radius`] for every query point, on `threads` threads; results in
    /// query order, independent of `threads`.
    pub fn batch_radius(
        &self,
        queries: &[[f64; 3]],
        r: f64,
        threads: usize,
    ) -> Result<Vec<Vec<Neighbour>>, QueryError> {
        run_batch(queries, threads, |q| self.radius(*q, r))
    }

    /// [`ChunkedIndex::in_box`] for every box, on `threads` threads; results in query
    /// order, independent of `threads`.
    pub fn batch_in_box(&self, boxes: &[Aabb], threads: usize) -> Result<Vec<Vec<u64>>, QueryError> {
        run_batch(boxes, threads, |b| self.in_box(b))
    }
}

/// Runs `f` over `items` in `threads` contiguous ranges; results in item order. On error,
/// the error of the first failing item in item order.
fn run_batch<I, T, F>(items: &[I], threads: usize, f: F) -> Result<Vec<T>, QueryError>
where
    I: Sync,
    T: Send,
    F: Fn(&I) -> Result<T, QueryError> + Sync,
{
    if threads == 0 {
        return Err(QueryError::Index(PointIndexError::ZeroThreads));
    }
    if threads > MAX_QUERY_THREADS {
        return Err(QueryError::Ooc(OocError::InvalidParams { what: "threads" }));
    }
    let run = |range: &[I]| -> Result<Vec<T>, QueryError> {
        let mut v: Vec<T> = Vec::new();
        v.try_reserve_exact(range.len()).map_err(|_| alloc_err::<T>(range.len()))?;
        for it in range {
            v.push(f(it)?);
        }
        Ok(v)
    };
    let n = items.len();
    let parts = threads.min(n).max(1);
    if parts == 1 {
        return run(items);
    }
    let per = n.div_ceil(parts);
    let ranges: Vec<&[I]> = items.chunks(per).collect();
    let results: Vec<Result<Vec<T>, QueryError>> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        let mut inline: Vec<(usize, Result<Vec<T>, QueryError>)> = Vec::new();
        for (i, r) in ranges.iter().enumerate() {
            let run = &run;
            let r: &[I] = r;
            match std::thread::Builder::new().spawn_scoped(s, move || run(r)) {
                Ok(h) => handles.push((i, h)),
                // A thread that cannot be started: run its range here.
                Err(_) => inline.push((i, run(r))),
            }
        }
        let mut all: Vec<(usize, Result<Vec<T>, QueryError>)> = inline;
        for (i, h) in handles {
            let r = h.join().unwrap_or(Err(QueryError::Index(PointIndexError::Inconsistent)));
            all.push((i, r));
        }
        all.sort_by_key(|(i, _)| *i);
        all.into_iter().map(|(_, r)| r).collect()
    });
    let mut out: Vec<T> = Vec::new();
    out.try_reserve_exact(n).map_err(|_| alloc_err::<T>(n))?;
    for r in results {
        out.extend(r?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn total_order_keys_round_trip_and_order() {
        let vals = [-f64::MAX, -1.5, -f64::MIN_POSITIVE, -5e-324, -0.0, 0.0, 5e-324, 1.0, f64::MAX];
        for w in vals.windows(2) {
            assert!(key_of(w[0]) < key_of(w[1]), "{} < {}", w[0], w[1]);
        }
        for v in vals {
            assert_eq!(from_key(key_of(v)).to_bits(), v.to_bits());
        }
    }

    #[test]
    fn axis_prefix_deinterleaves() {
        // Depth 2, octant path 5 (x and z upper) then 2 (y upper): x = 10b, y = 01b, z = 10b.
        let c = CellCode::ROOT.child(5).unwrap().child(2).unwrap();
        assert_eq!((axis_prefix(c, 0), axis_prefix(c, 1), axis_prefix(c, 2)), (0b10, 0b01, 0b10));
        assert_eq!(axis_prefix(CellCode::ROOT, 0), 0);
    }

    #[test]
    fn searches_find_exact_cell_edges() {
        // Bounds with a non-dyadic extent, so the edges are not simple fractions.
        let b = Aabb::new([-0.3, 0.1, 7.0], [1.7, 0.1 + 1.0 / 3.0, 7.0 + 1e-3]).unwrap();
        let g = Grid::new(&b).unwrap();
        for k in 0..3 {
            let (lo, hi) = (b.min()[k], b.max()[k]);
            for a in [1u64, 2, 1 << 10, 1 << 20, 3 << 19, QMAX] {
                let v = first_at_least(&g, k, lo, hi, a).unwrap();
                assert!(g.quantise(v, k) >= a);
                let prev = from_key(key_of(v) - 1);
                assert!(g.quantise(prev, k) < a, "axis {k} a {a}");
                let w = last_at_most(&g, k, lo, hi, a - 1).unwrap();
                assert!(g.quantise(w, k) < a);
                assert!(g.quantise(from_key(key_of(w) + 1), k) >= a);
            }
        }
    }

    #[test]
    fn lower_bound_is_zero_inside_and_pinned_outside() {
        let b = Bounds { lo: [0.0, 0.0, 0.0], hi: [1.0, 1.0, 1.0] };
        assert_eq!(lower_bound(&b, &[0.5, 0.5, 0.5]), 0.0);
        assert_eq!(lower_bound(&b, &[2.0, -1.0, 0.5]), 2.0);
        assert_eq!(lower_bound(&b, &[1.0, 1.0, 1.0]), 0.0);
    }
}
