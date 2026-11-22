//! Memory budgets, exact pre-allocation estimates, and a digest-keyed LRU cache for spatial
//! indexes (spec node `GEO.INDEX.BUDGET`).
//!
//! Pure Rust, no I/O, no clock access, no `unsafe`, no `HashMap` (KNP-1, CONSTITUTION §4 & §8).
//! Observational build time (`elapsed_ns` in [`BuildReport`]) is measured by the caller outside
//! the kernel and attached after the build; it never influences cache behaviour, eviction order,
//! or query results (REQ-GEO.INDEX.BUDGET-005).

use core::cmp::Ordering;
use core::fmt;
use std::collections::BTreeMap;
use std::sync::Arc;

use fid_geo_types::digest::ElementDigest;
use fid_geo_types::{Aabb, FrameRef, Mesh, PointBuffer};

use crate::bvh::{bvh_build_bytes, BvhError, BvhNode, PrimitiveBvh, LEAF_SIZE, MAX_TRIANGLES};
use crate::kdtree::{PointIndex, PointIndexError, PointTree, BUCKET, MAX_POINTS};

/// Declared memory budget in bytes for an index build or an [`IndexCache`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemoryBudget(pub u64);

impl MemoryBudget {
    /// An unbounded budget (`u64::MAX` bytes).
    pub const UNLIMITED: Self = Self(u64::MAX);

    /// Creates a budget of `bytes` bytes.
    pub const fn new(bytes: u64) -> Self {
        Self(bytes)
    }

    /// The budget in bytes.
    pub const fn bytes(self) -> u64 {
        self.0
    }
}

/// The spatial index algorithm selected by a build or cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum IndexKind {
    /// Point k-d tree (`GEO.INDEX.KDTREE`: [`PointIndex`] / [`PointTree`]).
    KdTree = 1,
    /// Triangle bounding volume hierarchy (`GEO.INDEX.BVH`: [`PrimitiveBvh`]).
    Bvh = 2,
}

impl IndexKind {
    /// Stable single-byte discriminant used in canonical key ordering.
    pub const fn code(self) -> u8 {
        match self {
            IndexKind::KdTree => 1,
            IndexKind::Bvh => 2,
        }
    }

    /// Pinned maximum leaf size implemented by this crate for this index kind.
    pub const fn pinned_leaf_size(self) -> u16 {
        match self {
            IndexKind::KdTree => BUCKET as u16,
            IndexKind::Bvh => LEAF_SIZE as u16,
        }
    }
}

/// Build parameters carried in [`IndexKey`] and passed to [`estimate_bytes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuildParams {
    /// Maximum leaf size. Must match [`BUCKET`] (16) for [`IndexKind::KdTree`] or
    /// [`LEAF_SIZE`] (8) for [`IndexKind::Bvh`] when building with this crate.
    pub leaf_size: u16,
    /// Requested thread count (`>= 1`; sequential in the kernel, does not alter index bytes).
    pub threads: u32,
}

impl BuildParams {
    /// Default parameters for [`IndexKind::KdTree`] (`leaf_size = 16`, `threads = 1`).
    pub const fn kdtree() -> Self {
        Self { leaf_size: BUCKET as u16, threads: 1 }
    }

    /// Default parameters for [`IndexKind::Bvh`] (`leaf_size = 8`, `threads = 1`).
    pub const fn bvh() -> Self {
        Self { leaf_size: LEAF_SIZE as u16, threads: 1 }
    }

    /// Default parameters for the given index kind.
    pub const fn default_for(kind: IndexKind) -> Self {
        match kind {
            IndexKind::KdTree => Self::kdtree(),
            IndexKind::Bvh => Self::bvh(),
        }
    }

    /// Returns a copy with `threads` set.
    pub const fn with_threads(mut self, threads: u32) -> Self {
        self.threads = threads;
        self
    }

    /// Canonical 6-byte big-endian encoding (`leaf_size`, `threads`) used for key ordering.
    pub const fn to_bytes(self) -> [u8; 6] {
        let l = self.leaf_size.to_be_bytes();
        let t = self.threads.to_be_bytes();
        [l[0], l[1], t[0], t[1], t[2], t[3]]
    }
}

impl Ord for BuildParams {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_bytes().cmp(&other.to_bytes())
    }
}

impl PartialOrd for BuildParams {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Cache key identifying a built index by source element content digest, index kind and build
/// parameters.
///
/// [`Ord`] compares the canonical 39-byte layout [`IndexKey::to_bytes`] lexicographically so
/// `BTreeMap` order and LRU tie-breaking by key bytes are deterministic (D0,
/// REQ-GEO.INDEX.BUDGET-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IndexKey {
    /// Canonical SHA-256 content digest of the source geometry element (`GEO.TYPES.DIGEST`).
    pub digest: ElementDigest,
    /// Spatial index kind.
    pub kind: IndexKind,
    /// Build parameters.
    pub params: BuildParams,
}

impl IndexKey {
    /// Creates a cache key.
    pub const fn new(digest: ElementDigest, kind: IndexKind, params: BuildParams) -> Self {
        Self { digest, kind, params }
    }

    /// Canonical 39-byte representation: 32 digest bytes, 1 kind byte, 6 parameter bytes.
    pub const fn to_bytes(&self) -> [u8; 39] {
        let d = self.digest.to_bytes();
        let p = self.params.to_bytes();
        let mut out = [0u8; 39];
        let mut i = 0;
        while i < 32 {
            out[i] = d[i];
            i += 1;
        }
        out[32] = self.kind.code();
        let mut j = 0;
        while j < 6 {
            out[33 + j] = p[j];
            j += 1;
        }
        out
    }
}

impl Ord for IndexKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_bytes().cmp(&other.to_bytes())
    }
}

impl PartialOrd for IndexKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Build cost report returned with each budgeted build or cache lookup.
///
/// `bytes` is deterministic (D0) and verified against [`estimate_bytes`]. `elapsed_ns` is
/// observational only (class N), supplied by callers outside the kernel via
/// [`BuildReport::with_elapsed_ns`], and is never stored in any value of record
/// (REQ-GEO.INDEX.BUDGET-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildReport {
    /// Actual memory in bytes reported by the built index (`<= estimate_bytes`).
    pub bytes: u64,
    /// Observational build time in nanoseconds supplied by the caller (0 inside the kernel).
    pub elapsed_ns: u64,
}

impl BuildReport {
    /// Creates a report with `bytes` and `elapsed_ns = 0`.
    pub const fn new(bytes: u64) -> Self {
        Self { bytes, elapsed_ns: 0 }
    }

    /// Attaches caller-measured observational duration in nanoseconds.
    pub const fn with_elapsed_ns(mut self, elapsed_ns: u64) -> Self {
        self.elapsed_ns = elapsed_ns;
        self
    }
}

/// Every error returned by budgeted builds and [`IndexCache`]. Errors are values; nothing panics.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetError {
    /// The pre-allocation estimate exceeds the declared budget (REQ-GEO.INDEX.BUDGET-002).
    BudgetExceeded {
        /// Estimated bytes needed.
        needed: u64,
        /// Declared budget in bytes.
        budget: u64,
    },
    /// The requested leaf size is not the pinned constant supported by this crate's builder.
    UnsupportedLeafSize {
        /// Index kind.
        kind: IndexKind,
        /// Requested leaf size.
        requested: u16,
        /// Supported pinned leaf size.
        supported: u16,
    },
    /// The builder closure passed to [`IndexCache::get_or_build`] returned an index whose kind
    /// did not match `key.kind`.
    KindMismatch {
        /// Kind declared in the key.
        expected: IndexKind,
        /// Kind returned by the builder.
        found: IndexKind,
    },
    /// Build accounting check failed: actual bytes exceeded the pre-allocation estimate
    /// (unreachable for well-formed estimates; guards REQ-GEO.INDEX.BUDGET-001).
    EstimateUnderestimated {
        /// Actual bytes reported by the built index.
        actual: u64,
        /// Pre-allocation estimate in bytes.
        estimate: u64,
    },
    /// Underlying [`PointIndex`] / [`PointTree`] build error.
    PointIndex(PointIndexError),
    /// Underlying [`PrimitiveBvh`] build error.
    Bvh(BvhError),
}

impl BudgetError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            BudgetError::BudgetExceeded { .. } => "budget_exceeded",
            BudgetError::UnsupportedLeafSize { .. } => "unsupported_leaf_size",
            BudgetError::KindMismatch { .. } => "kind_mismatch",
            BudgetError::EstimateUnderestimated { .. } => "estimate_underestimated",
            BudgetError::PointIndex(e) => e.code(),
            BudgetError::Bvh(e) => e.code(),
        }
    }
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BudgetError::BudgetExceeded { needed, budget } => {
                write!(f, "budget_exceeded (needed {needed} bytes > budget {budget} bytes)")
            }
            BudgetError::UnsupportedLeafSize { kind, requested, supported } => {
                write!(f, "unsupported_leaf_size ({kind:?}: requested {requested}, supported {supported})")
            }
            BudgetError::KindMismatch { expected, found } => {
                write!(f, "kind_mismatch (expected {expected:?}, found {found:?})")
            }
            BudgetError::EstimateUnderestimated { actual, estimate } => {
                write!(f, "estimate_underestimated (actual {actual} bytes > estimate {estimate} bytes)")
            }
            BudgetError::PointIndex(e) => write!(f, "point_index: {e}"),
            BudgetError::Bvh(e) => write!(f, "bvh: {e}"),
        }
    }
}

impl std::error::Error for BudgetError {}

impl From<PointIndexError> for BudgetError {
    fn from(e: PointIndexError) -> Self {
        BudgetError::PointIndex(e)
    }
}

impl From<BvhError> for BudgetError {
    fn from(e: BvhError) -> Self {
        BudgetError::Bvh(e)
    }
}

// ------------------------------------------------------------------ exact upper-bound estimates

/// Leaf depth for `n` points with maximum leaf size `bucket >= 1`: smallest `d` with
/// `ceil(n / 2^d) <= bucket`.
fn leaf_depth_with(n: u64, bucket: u64) -> u32 {
    let mut d = 0u32;
    while d < 63 && (n.saturating_add((1u64 << d) - 1) >> d) > bucket {
        d += 1;
    }
    d
}

/// Exact closed-form upper bound on the bytes allocated by building `kind` over `count`
/// primitives with `params`, returning `None` if `count` exceeds the index limit, `leaf_size`
/// is 0, or arithmetic overflows.
///
/// - For [`IndexKind::KdTree`] with `leaf_size == BUCKET`: equals [`crate::overhead_bytes`],
///   which is exact for [`PointIndex::memory_bytes`] and strictly bounds both
///   [`PointTree::memory_bytes`] and the heap bytes allocated during build.
/// - For [`IndexKind::Bvh`] with `leaf_size == LEAF_SIZE`: equals [`crate::bvh::bvh_build_bytes`],
///   bounding both peak heap allocation during construction (including temporary centroid keys
///   and range queue) and retained [`PrimitiveBvh::memory_bytes`].
pub fn checked_estimate_bytes(kind: IndexKind, count: u64, params: BuildParams) -> Option<u64> {
    if params.leaf_size == 0 {
        return None;
    }
    match kind {
        IndexKind::KdTree => {
            if count > MAX_POINTS {
                return None;
            }
            let d = leaf_depth_with(count, u64::from(params.leaf_size));
            let internal = 1u64.checked_shl(d)?.checked_sub(1)?;
            let inline = u64::try_from(core::mem::size_of::<PointIndex<'static>>()).ok()?;
            inline.checked_add(count.checked_mul(4)?)?.checked_add(internal.checked_mul(9)?)
        }
        IndexKind::Bvh => {
            if count > MAX_TRIANGLES {
                return None;
            }
            if params.leaf_size == LEAF_SIZE as u16 {
                return bvh_build_bytes(count);
            }
            let inline = u64::try_from(core::mem::size_of::<PrimitiveBvh>()).ok()?;
            if count == 0 {
                return Some(inline);
            }
            let min_leaf = u64::from(params.leaf_size / 2).max(1);
            let cap = (count / min_leaf).checked_mul(2)?.checked_add(1)?;
            let per_tri = u64::try_from(
                core::mem::size_of::<Aabb>()
                    + core::mem::size_of::<[f64; 3]>()
                    + core::mem::size_of::<u32>()
                    + core::mem::size_of::<u64>(),
            )
            .ok()?;
            let per_node =
                u64::try_from(core::mem::size_of::<BvhNode>() + core::mem::size_of::<(usize, usize)>()).ok()?;
            inline.checked_add(count.checked_mul(per_tri)?)?.checked_add(cap.checked_mul(per_node)?)
        }
    }
}

/// Exact upper bound on the bytes allocated by building `kind` over `count` primitives with
/// `params` (REQ-GEO.INDEX.BUDGET-001). Saturates to `u64::MAX` when `count` exceeds the index
/// limit, `leaf_size` is 0, or arithmetic overflows.
pub fn estimate_bytes(kind: IndexKind, count: u64, params: BuildParams) -> u64 {
    checked_estimate_bytes(kind, count, params).unwrap_or(u64::MAX)
}

fn check_leaf_size(kind: IndexKind, params: BuildParams) -> Result<(), BudgetError> {
    let supported = kind.pinned_leaf_size();
    if params.leaf_size != supported {
        return Err(BudgetError::UnsupportedLeafSize { kind, requested: params.leaf_size, supported });
    }
    Ok(())
}

fn check_budget(kind: IndexKind, count: u64, params: BuildParams, budget: MemoryBudget) -> Result<u64, BudgetError> {
    check_leaf_size(kind, params)?;
    let needed = estimate_bytes(kind, count, params);
    if needed > budget.bytes() {
        return Err(BudgetError::BudgetExceeded { needed, budget: budget.bytes() });
    }
    Ok(needed)
}

fn verify_accounting(actual: u64, estimate: u64) -> Result<(), BudgetError> {
    if actual > estimate {
        return Err(BudgetError::EstimateUnderestimated { actual, estimate });
    }
    Ok(())
}

// ------------------------------------------------------------------ budgeted standalone builds

/// Builds a borrowed [`PointIndex`] under `budget`, refusing with
/// [`BudgetError::BudgetExceeded`] before allocating any index memory if the estimate exceeds
/// `budget` (REQ-GEO.INDEX.BUDGET-001, -002).
pub fn build_point_index<'a>(
    points: &'a PointBuffer,
    params: BuildParams,
    budget: MemoryBudget,
) -> Result<(PointIndex<'a>, BuildReport), BudgetError> {
    let count = u64::try_from(points.x.len()).unwrap_or(u64::MAX);
    let estimate = check_budget(IndexKind::KdTree, count, params, budget)?;
    let idx = PointIndex::build(points, params.threads as usize)?;
    let actual = idx.memory_bytes();
    verify_accounting(actual, estimate)?;
    Ok((idx, BuildReport::new(actual)))
}

/// Builds an owned [`PointTree`] under `budget`, refusing before allocation if the estimate
/// exceeds `budget`.
pub fn build_point_tree(
    points: &PointBuffer,
    params: BuildParams,
    budget: MemoryBudget,
) -> Result<(PointTree, BuildReport), BudgetError> {
    let count = u64::try_from(points.x.len()).unwrap_or(u64::MAX);
    let estimate = check_budget(IndexKind::KdTree, count, params, budget)?;
    let tree = PointTree::build(points, params.threads as usize)?;
    let actual = tree.memory_bytes();
    verify_accounting(actual, estimate)?;
    Ok((tree, BuildReport::new(actual)))
}

/// Builds a [`PrimitiveBvh`] over `mesh` under `budget`, refusing with
/// [`BudgetError::BudgetExceeded`] before allocating any index memory if the estimate exceeds
/// `budget` (REQ-GEO.INDEX.BUDGET-001, -002).
pub fn build_bvh(
    mesh: &Mesh,
    payload: &[u64],
    params: BuildParams,
    budget: MemoryBudget,
) -> Result<(PrimitiveBvh, BuildReport), BudgetError> {
    let count = mesh.triangle_count();
    let estimate = check_budget(IndexKind::Bvh, count, params, budget)?;
    let bvh = PrimitiveBvh::build(mesh, payload, params.threads as usize)?;
    let actual = bvh.memory_bytes();
    verify_accounting(actual, estimate)?;
    Ok((bvh, BuildReport::new(actual)))
}

/// Builds a [`PrimitiveBvh`] over corner triples under `budget`, refusing before allocation if
/// the estimate exceeds `budget`.
pub fn build_bvh_triangles(
    frame: FrameRef,
    triangles: &[[[f64; 3]; 3]],
    payload: &[u64],
    params: BuildParams,
    budget: MemoryBudget,
) -> Result<(PrimitiveBvh, BuildReport), BudgetError> {
    let count = u64::try_from(triangles.len()).unwrap_or(u64::MAX);
    let estimate = check_budget(IndexKind::Bvh, count, params, budget)?;
    let bvh = PrimitiveBvh::build_triangles(frame, triangles, payload, params.threads as usize)?;
    let actual = bvh.memory_bytes();
    verify_accounting(actual, estimate)?;
    Ok((bvh, BuildReport::new(actual)))
}

// ------------------------------------------------------------------ digest-keyed LRU cache

/// Shared read-only handle to a cached spatial index.
#[derive(Debug, Clone)]
pub enum CachedIndex {
    /// Cached point k-d tree (`GEO.INDEX.KDTREE`).
    Point(Arc<PointTree>),
    /// Cached triangle bounding volume hierarchy (`GEO.INDEX.BVH`).
    Primitive(Arc<PrimitiveBvh>),
}

impl CachedIndex {
    /// The kind of index held.
    pub fn kind(&self) -> IndexKind {
        match self {
            CachedIndex::Point(_) => IndexKind::KdTree,
            CachedIndex::Primitive(_) => IndexKind::Bvh,
        }
    }

    /// Bytes retained by the cached index.
    pub fn memory_bytes(&self) -> u64 {
        match self {
            CachedIndex::Point(t) => t.memory_bytes(),
            CachedIndex::Primitive(b) => b.memory_bytes(),
        }
    }

    /// True when both handles point to the exact same allocated index object (AC-2).
    pub fn ptr_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (CachedIndex::Point(a), CachedIndex::Point(b)) => Arc::ptr_eq(a, b),
            (CachedIndex::Primitive(a), CachedIndex::Primitive(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Borrows the [`Arc<PointTree>`] if this is a point index.
    pub fn as_point(&self) -> Option<&Arc<PointTree>> {
        match self {
            CachedIndex::Point(t) => Some(t),
            CachedIndex::Primitive(_) => None,
        }
    }

    /// Borrows the [`Arc<PrimitiveBvh>`] if this is a triangle BVH.
    pub fn as_primitive(&self) -> Option<&Arc<PrimitiveBvh>> {
        match self {
            CachedIndex::Primitive(b) => Some(b),
            CachedIndex::Point(_) => None,
        }
    }
}

/// Outcome of a lookup-or-build request on [`IndexCache`].
#[derive(Debug, Clone)]
pub struct CacheLookup {
    /// Shared handle to the cached index.
    pub index: CachedIndex,
    /// `true` if returned from cache without rebuilding; `false` if freshly built.
    pub hit: bool,
    /// Cost report (`bytes` of the index; `elapsed_ns` is 0 until attached by the caller).
    pub report: BuildReport,
}

#[derive(Debug)]
struct Entry {
    index: CachedIndex,
    bytes: u64,
    last_used: u64,
}

/// Digest-keyed spatial index cache with a declared memory budget and deterministic LRU eviction
/// (REQ-GEO.INDEX.BUDGET-003, -004, -005).
///
/// Uses a [`BTreeMap`] and a monotonic `tick` counter (no wall clock, no `HashMap`). When the
/// retained bytes exceed `budget`, entries are evicted in ascending `(last_used, key.to_bytes())`
/// order until `used_bytes <= budget`.
#[derive(Debug)]
pub struct IndexCache {
    entries: BTreeMap<IndexKey, Entry>,
    tick: u64,
    budget: MemoryBudget,
    used_bytes: u64,
    build_count: u64,
    hit_count: u64,
    eviction_count: u64,
}

impl IndexCache {
    /// Creates an empty cache with `budget`.
    pub fn new(budget: MemoryBudget) -> Self {
        Self {
            entries: BTreeMap::new(),
            tick: 0,
            budget,
            used_bytes: 0,
            build_count: 0,
            hit_count: 0,
            eviction_count: 0,
        }
    }

    /// The cache's memory budget.
    pub fn budget(&self) -> MemoryBudget {
        self.budget
    }

    /// Retained bytes across all currently cached entries.
    pub fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    /// Number of currently cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total number of index builds performed by this cache (unchanged on cache hits, AC-2).
    pub fn build_count(&self) -> u64 {
        self.build_count
    }

    /// Total number of cache hits served.
    pub fn hit_count(&self) -> u64 {
        self.hit_count
    }

    /// Total number of entries evicted to stay within budget.
    pub fn eviction_count(&self) -> u64 {
        self.eviction_count
    }

    /// Whether `key` is currently cached (does not change LRU state).
    pub fn contains(&self, key: &IndexKey) -> bool {
        self.entries.contains_key(key)
    }

    /// Cached keys in ascending canonical key-byte order.
    pub fn keys(&self) -> impl Iterator<Item = &IndexKey> {
        self.entries.keys()
    }

    /// Read-only inspection of a cached entry without updating LRU recency.
    pub fn peek(&self, key: &IndexKey) -> Option<&CachedIndex> {
        self.entries.get(key).map(|e| &e.index)
    }

    /// Looks up `key`, marking it most-recently-used on a hit.
    pub fn get(&mut self, key: &IndexKey) -> Option<CachedIndex> {
        let entry = self.entries.get_mut(key)?;
        self.tick = self.tick.saturating_add(1);
        entry.last_used = self.tick;
        self.hit_count = self.hit_count.saturating_add(1);
        Some(entry.index.clone())
    }

    /// Removes `key` if present and returns its handle.
    pub fn remove(&mut self, key: &IndexKey) -> Option<CachedIndex> {
        let entry = self.entries.remove(key)?;
        self.used_bytes = self.used_bytes.saturating_sub(entry.bytes);
        Some(entry.index)
    }

    /// Removes all cached entries (preserves lifetime counters).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.used_bytes = 0;
    }

    /// Updates the cache budget, immediately evicting LRU entries (ties by key bytes) until the
    /// cache fits.
    pub fn set_budget(&mut self, budget: MemoryBudget) {
        self.budget = budget;
        self.evict_to_fit(None);
    }

    /// Returns the cached index for `key`, or checks the budget against
    /// `estimate_bytes(key.kind, count, key.params)` before calling `build`, verifies build
    /// accounting, caches the result, and evicts LRU entries (ties by key bytes) until within
    /// budget.
    pub fn get_or_build<F>(&mut self, key: IndexKey, count: u64, build: F) -> Result<CacheLookup, BudgetError>
    where
        F: FnOnce() -> Result<CachedIndex, BudgetError>,
    {
        if let Some(entry) = self.entries.get_mut(&key) {
            self.tick = self.tick.saturating_add(1);
            entry.last_used = self.tick;
            self.hit_count = self.hit_count.saturating_add(1);
            return Ok(CacheLookup { index: entry.index.clone(), hit: true, report: BuildReport::new(entry.bytes) });
        }

        let estimate = check_budget(key.kind, count, key.params, self.budget)?;
        let index = build()?;
        if index.kind() != key.kind {
            return Err(BudgetError::KindMismatch { expected: key.kind, found: index.kind() });
        }
        let actual = index.memory_bytes();
        verify_accounting(actual, estimate)?;

        self.tick = self.tick.saturating_add(1);
        self.build_count = self.build_count.saturating_add(1);
        self.entries.insert(key, Entry { index: index.clone(), bytes: actual, last_used: self.tick });
        self.used_bytes = self.used_bytes.saturating_add(actual);
        self.evict_to_fit(Some(&key));

        Ok(CacheLookup { index, hit: false, report: BuildReport::new(actual) })
    }

    /// Looks up or builds a [`PointTree`] for `(digest, IndexKind::KdTree, params)`.
    pub fn get_or_build_point(
        &mut self,
        digest: ElementDigest,
        points: &PointBuffer,
        params: BuildParams,
    ) -> Result<CacheLookup, BudgetError> {
        let key = IndexKey::new(digest, IndexKind::KdTree, params);
        let count = u64::try_from(points.x.len()).unwrap_or(u64::MAX);
        self.get_or_build(key, count, || {
            let tree = PointTree::build(points, params.threads as usize)?;
            Ok(CachedIndex::Point(Arc::new(tree)))
        })
    }

    /// Looks up or builds a [`PrimitiveBvh`] for `(digest, IndexKind::Bvh, params)`.
    pub fn get_or_build_bvh(
        &mut self,
        digest: ElementDigest,
        mesh: &Mesh,
        payload: &[u64],
        params: BuildParams,
    ) -> Result<CacheLookup, BudgetError> {
        let key = IndexKey::new(digest, IndexKind::Bvh, params);
        let count = mesh.triangle_count();
        self.get_or_build(key, count, || {
            let bvh = PrimitiveBvh::build(mesh, payload, params.threads as usize)?;
            Ok(CachedIndex::Primitive(Arc::new(bvh)))
        })
    }

    /// Looks up or builds a [`PrimitiveBvh`] over corner triples for
    /// `(digest, IndexKind::Bvh, params)`.
    pub fn get_or_build_bvh_triangles(
        &mut self,
        digest: ElementDigest,
        frame: FrameRef,
        triangles: &[[[f64; 3]; 3]],
        payload: &[u64],
        params: BuildParams,
    ) -> Result<CacheLookup, BudgetError> {
        let key = IndexKey::new(digest, IndexKind::Bvh, params);
        let count = u64::try_from(triangles.len()).unwrap_or(u64::MAX);
        self.get_or_build(key, count, || {
            let bvh = PrimitiveBvh::build_triangles(frame, triangles, payload, params.threads as usize)?;
            Ok(CachedIndex::Primitive(Arc::new(bvh)))
        })
    }

    /// Evicts entries in ascending `(last_used, key.to_bytes())` order until
    /// `self.used_bytes <= self.budget.bytes()`.
    fn evict_to_fit(&mut self, protect: Option<&IndexKey>) {
        while self.used_bytes > self.budget.bytes() {
            let mut victim: Option<(IndexKey, u64)> = None;
            // BTreeMap iterates in ascending IndexKey::to_bytes() order, so keeping strict `<`
            // on `last_used` breaks equal-tick ties by smallest key bytes (REQ-004).
            for (k, entry) in &self.entries {
                if protect.is_some_and(|p| p == k) {
                    continue;
                }
                let better = match victim {
                    None => true,
                    Some((_, best_tick)) => entry.last_used < best_tick,
                };
                if better {
                    victim = Some((*k, entry.last_used));
                }
            }
            let Some((victim_key, _)) = victim else {
                break;
            };
            if let Some(removed) = self.entries.remove(&victim_key) {
                self.used_bytes = self.used_bytes.saturating_sub(removed.bytes);
                self.eviction_count = self.eviction_count.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn lru_tie_breaks_by_key_bytes() {
        let pts = PointBuffer::from_points(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        let params = BuildParams::kdtree();
        let one = estimate_bytes(IndexKind::KdTree, 2, params);

        let k_low = IndexKey::new(ElementDigest([0x01; 32]), IndexKind::KdTree, params);
        let k_mid = IndexKey::new(ElementDigest([0x02; 32]), IndexKind::KdTree, params);
        let k_high = IndexKey::new(ElementDigest([0x03; 32]), IndexKind::KdTree, params);
        assert!(k_low.to_bytes() < k_mid.to_bytes());
        assert!(k_mid.to_bytes() < k_high.to_bytes());

        // Saturate tick so all inserted entries share the same `last_used == u64::MAX`,
        // forcing pure key-byte tie-breaking on eviction (REQ-GEO.INDEX.BUDGET-004).
        let mut cache = IndexCache::new(MemoryBudget(one * 2));
        cache.tick = u64::MAX;

        cache.get_or_build_point(k_high.digest, &pts, params).unwrap();
        cache.get_or_build_point(k_low.digest, &pts, params).unwrap();
        assert!(cache.contains(&k_high));
        assert!(cache.contains(&k_low));

        // Inserting k_mid exceeds 2*one; between k_low and k_high (same last_used), k_low has
        // the smaller key bytes and must be evicted first.
        cache.get_or_build_point(k_mid.digest, &pts, params).unwrap();
        assert!(!cache.contains(&k_low), "smallest key bytes must be evicted on tie");
        assert!(cache.contains(&k_high));
        assert!(cache.contains(&k_mid));
        assert_eq!(cache.eviction_count(), 1);
    }
}
