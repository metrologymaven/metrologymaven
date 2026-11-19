---
id: GEO.INDEX.KDTREE
title: Point k-d tree with exact k-nearest, radius and box queries
level: 3
parent: GEO.INDEX
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-geo-index]
satisfies: [REQ-GEO.INDEX-001, REQ-GEO.INDEX-002, REQ-GEO.INDEX-003, REQ-GEO.INDEX-004]
covers: []
depends_on: [GEO.TYPES.POINTS, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.INDEX.KDTREE: Point k-d tree with exact k-nearest, radius and box queries

## Purpose

The point index every fit, alignment and scan operation uses for neighbour search: a compact implicit k-d tree over a point buffer, exact k-nearest, fixed-radius and box queries, and a total-order tie-break so that results never depend on platform, run or thread count.

## Scope

**In:**
- Implicit k-d tree: a permutation of point indexes plus split planes, leaf bucket size 16
- Build by median selection on (coordinate, point index), parallel over subtrees with an identical result
- `knn(q, k)` for k in 1..=1024, `radius(q, r)`, `in_box(aabb)`
- Ordering of results by (squared distance, point index) ascending
- Byte-exact memory accounting of the built index

**Out:**
- Triangle and primitive indexes (owned by `GEO.INDEX.BVH`)
- Budget enforcement and caching (owned by `GEO.INDEX.BUDGET`)
- Approximate search (not provided; exact by default per parent, approximate modes need a new leaf)
- Cross-chunk queries (owned by `GEO.OOC.QUERY`)

## Requirements

- **REQ-GEO.INDEX.KDTREE-001** The k-nearest query shall return exactly the k points with the smallest squared Euclidean distance for any k from 1 to 1024, or all points if fewer than k exist.
- **REQ-GEO.INDEX.KDTREE-002** When candidates have equal squared distance, the index shall order them by ascending point index, both inside the result list and when deciding which enter a k-limited result.
- **REQ-GEO.INDEX.KDTREE-003** Squared distances shall be computed as dx·dx + dy·dy + dz·dz evaluated left to right with no fused multiply-add, so every comparison is bit-identical across targets.
- **REQ-GEO.INDEX.KDTREE-004** Building the tree with 1, 2 or N threads shall produce a byte-identical index.
- **REQ-GEO.INDEX.KDTREE-005** The built index shall use at most 12 bytes per point beyond the coordinate buffer for 10⁸ points, as counted by its own memory accounting.
- **REQ-GEO.INDEX.KDTREE-006** If a query point is non-finite, k is 0 or above 1024, or the radius is negative or non-finite, then the query shall fail with a typed error.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/index/uniform_1e5` (10⁵ points, seed 21) and 1 000 query points, when knn with k ∈ {1, 16, 1024} and radius 2 mm run, then results equal a brute-force reference exactly (same indexes, same order).
- **AC-2** Given fixture `fixtures/geo/index/lattice_ties` (a 50³ integer lattice scaled by 1 mm, where many distances tie), when knn k = 26 runs at lattice points, then results equal the brute-force reference including tie order by index.
- **AC-3** Given `uniform_1e5`, when the index is built with 1, 4 and 16 threads, then the three serialised indexes are byte-identical, and the result digest of AC-1 is identical on x86-64, ARM64 and wasm32.
- **AC-4** Given a seeded 10⁸-point cloud on a runner with sufficient memory, when built, then the index's reported overhead is ≤ 12 bytes per point.
- **AC-5** (error path) Given k = 0, k = 1025 or a NaN query, when queried, then the typed error is returned.

## Design

- **Registry command(s):** none; library leaf (commands in GEO.INDEX.CMD).
- **Data structures:** `PointIndex { perm: Vec<u32> or Vec<u64> (by point count), splits: Vec<f64>, axes: Vec<u8>, bucket: u8 }`; `Neighbour { index: u64, dist2: f64 }`.
- **Algorithm:** recursive build choosing the axis of largest extent (ties: x < y < z), median by `select_nth_unstable_by` on the total order (coordinate, index); subtrees built in parallel with a fixed partition so output does not depend on scheduling; k-nearest by depth-first search with a bounded max-heap ordered by (dist2, index); pruning uses the same squared-distance arithmetic.
- **Errors:** `NonFiniteQuery`, `BadK`, `BadRadius`.
- **Determinism:** D0.
- **Performance budget:** see GEO.INDEX.BENCH; build of 10⁸ points under 30 s on 8 threads [unverified].
- **Crate:** `fid-geo-index` (kernel graph, KNP-1 applies).
- **Pinned choices (value-affecting).** The module docs of `crates/fid-geo-index/src/kdtree.rs` repeat these; `tools/fixtures/geo_index.py` models them bit for bit. Conventions shared with `GEO.INDEX.BVH`: breadth-first build, IEEE totalOrder, ties by index, a fixed 64-entry query stack, the 2^500 coordinate limit, sequential build.
  - **Input domain, first offender wins:** `threads >= 1` (`ZeroThreads`); y and z columns as long as x (`ColumnLength { axis }`); at most 2^32 points (`TooManyPoints`; the permutation is `u32`); then per point in index order, per axis x, y, z: finite (`NonFinitePoint`), then `|x| <= 2^500` (`PointOutOfRange`). Empty buffers, duplicate and coincident points, signed zeros and subnormal coordinates are **accepted**. The index borrows the `PointBuffer` (`PointIndex<'a>`), so it cannot be queried against other coordinates.
  - **Query domain:** query point per axis finite (`NonFiniteQuery { axis }`), then `|x| <= 2^500` (`QueryOutOfRange { axis }`); then `1 <= k <= 1024` (`BadK`), or radius finite, `>= 0` (`-0.0` accepted) and `<= 2^500` (`BadRadius`). The limit keeps every difference, square and three-term sum finite.
  - **Squared distance:** `d2 = (dx*dx + dy*dy) + dz*dz`, `dx = p.x - q.x`; every nearest, within and tie decision uses this computed value (REQ-003). Distinct points whose squared differences underflow (`|dx| < 2^-537`) therefore have `d2 = 0` and tie like coincident points; the returned `dist2` shows it.
  - **Tree shape:** leaf depth `L` = smallest `d >= 0` with `ceil(n / 2^d) <= 16`; a perfect implicit tree with internal nodes `0 .. 2^L - 1` (children `2i + 1`, `2i + 2`) and all `2^L` leaves at depth `L`; a node over `c` points gives the first `c / 2` (floor) to the left child. Leaves hold 8 to 16 points once `n > 16`; for `n <= 16` the root is the only leaf.
  - **Split rule:** per internal node in index order: extents `max - min` per axis (totalOrder min and max of its points); axis = largest extent, ties x < y < z; points ordered by (coordinate in totalOrder, point index); the first `c / 2` go left; the split value is the coordinate of the first right point. Each leaf's points are then stored in ascending index order, so the permutation does not depend on how the selection algorithm arranges a partition.
  - **Queries:** `knn(q, k)` returns the `min(k, n)` smallest `(d2, index)` pairs ascending; `radius(q, r)` every point with `d2 <= r*r` (`r*r` rounded once), sorted by `(d2, index)`, so `r = 0` returns the points with `d2 = 0`; `in_box(aabb)` every point with `min_k <= p_k <= max_k` on all axes (numeric, `-0.0 == +0.0`), as point indexes ascending (a box has no query point, so no `dist2`).
  - **Exact pruning (no inflation):** a subtree is skipped only if `lb = (gx*gx + gy*gy) + gz*gz > ` the current k-th `d2` (or `r*r`), where `g_k` is the largest `fl(|q_k - s|)` over the splits on axis `k` whose far side the subtree is on. For any point in it `|p_k - q_k| >= |s - q_k|` exactly and rounding is monotone, so its computed `d2 >= lb`; a point tying the k-th distance is never pruned. Box descent compares exactly with the split values.
  - **Memory (REQ-005):** `memory_bytes()` = struct size + permutation capacity x 4 + split capacity x 8 + axis capacity x 1, allocated with `try_reserve_exact`; that is `4n + 9 (2^L - 1)` bytes plus the struct, at most about 5.2 bytes per point for any `n` (4.75 at 10^8).
  - **Serialisation (`to_bytes`, AC-3):** `FIDKDT01`; point count (u64 LE); `L` and bucket 16 (u32 LE each); split values (f64 bits, u64 LE each); axis bytes; permutation (u32 LE each).
  - **Bounds:** no recursion; a fixed 64-entry stack (depth <= 28 for 2^32 points); the k-nearest heap holds at most `k + 1` entries; every allocation sized from input is fallible (`AllocationFailed`); nothing panics.

## Test fixtures

- `fixtures/geo/index/uniform_1e5` and `lattice_ties`: generated by the seeded synthetic generator (seeds 21 and 22). CC-BY-4.0. The brute-force reference is a test-only function in the same crate.

## Build prompt

> In crate `fid-geo-index` (pure Rust, f64, no I/O, kernel graph, KNP-1), implement `PointIndex::build(points: &PointBuffer, threads: usize) -> PointIndex` as an implicit k-d tree (permutation array `u32` or `u64` by count, split values, axis bytes, bucket size 16), plus `knn(q, k)`, `radius(q, r)` and `in_box(aabb)` returning `Vec<Neighbour { index: u64, dist2: f64 }>` sorted by (dist2, index). Axis choice: largest extent, ties x<y<z; median by total order on (coordinate, index); parallel subtree build with a result independent of thread count. Squared distance written as `dx*dx + dy*dy + dz*dz` left to right, no `mul_add`. Errors: `NonFiniteQuery`, `BadK` (k outside 1..=1024), `BadRadius`. Provide `memory_bytes()` that counts every allocation exactly. Acceptance: AC-1 equality with brute force on `uniform_1e5`; AC-2 tie order on `lattice_ties`; AC-3 byte-identical index for 1/4/16 threads and identical result digest on three targets; AC-4 ≤ 12 bytes/point at 10⁸ points; AC-5 typed errors. Constraints: CONSTITUTION §8 (no panic, no `unsafe`, deny `unwrap`/`expect`), KNP-1 (no FMA, total-order comparisons via `fid-num`, no std HashMap/HashSet, no runtime SIMD dispatch). Files: `crates/fid-geo-index/src/kdtree.rs`, `tests/kdtree.rs`, fixture generator `tools/fixtures/geo_index.py`.

## Open questions

- None.

## Children

_Leaf: none._
