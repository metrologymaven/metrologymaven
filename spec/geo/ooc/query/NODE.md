---
id: GEO.OOC.QUERY
title: Cross-chunk neighbour queries
level: 3
parent: GEO.OOC
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-ooc]
built_pr: 651
satisfies: [REQ-GEO.OOC-003]
covers: []
depends_on: [GEO.OOC.CACHE, GEO.INDEX.KDTREE, GEO.INDEX.BUDGET]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-26
---

# GEO.OOC.QUERY: Cross-chunk neighbour queries

## Purpose

k-nearest, radius and box queries over a chunked cloud that return exactly what the in-memory GEO.INDEX query would return on the same points, by searching per-chunk indexes in order of distance and merging with the same total order.

## Scope

**In:**
- Per-chunk `PointIndex` built lazily and cached by chunk digest (GEO.INDEX.BUDGET)
- Chunk selection by cell bounds and lower-bound distance
- Merge of per-chunk results by (squared distance, original index)
- k-nearest (1..1024), radius and box queries; batch query interface with a fixed processing order

**Out:**
- In-memory indexes (owned by `GEO.INDEX`)
- Streaming (owned by `GEO.OOC.STREAM`)

## Requirements

- **REQ-GEO.OOC.QUERY-001** Every cross-chunk k-nearest, radius and box query shall return the same original indexes, in the same order and with bit-identical squared distances, as the GEO.INDEX query on the same cloud held in memory.
- **REQ-GEO.OOC.QUERY-002** The query shall visit chunks in order of lower-bound distance (ties by cell code) and stop when no unvisited chunk can improve the result.
- **REQ-GEO.OOC.QUERY-003** Per-chunk indexes shall be built at most once per chunk digest while cached and shall count against the index memory budget.
- **REQ-GEO.OOC.QUERY-004** Batch queries shall return results in query order regardless of thread count.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/ooc/cloud_5e6` both in memory and chunked (65 536 points per chunk), when 10⁴ seeded queries (k ∈ {1, 16, 1024}, radius 1 mm, boxes) run on both, then all results are identical, including queries near chunk boundaries.
- **AC-2** Given fixture `fixtures/geo/ooc/boundary_ties` (points mirrored across a chunk boundary at equal distance), when k = 2 queries run on the boundary, then the order by original index matches the in-memory result.
- **AC-3** Given 10⁴ queries with 1 and 8 threads, then results are byte-identical.
- **AC-4** Given repeated queries, then the per-chunk index build counter does not exceed the number of distinct chunks touched.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `ChunkedIndex { manifest, cache, index_cache }`; neighbours as `Neighbour { index: original u64, dist2 }`.
- **Algorithm:** a priority queue of chunks by (box lower-bound distance², cell code); per chunk run the GEO.INDEX query with the current k-th distance as an upper bound; merge with the same bounded heap ordering as GEO.INDEX.KDTREE.
- **Errors:** cache, budget and integrity errors propagated; query argument errors as in GEO.INDEX.
- **Determinism:** D0.
- **Performance budget:** k = 16 median under 50 µs with warm chunks [unverified].
- **Crate:** `fid-geo-ooc`.

## Test fixtures

- `cloud_5e6` and `boundary_ties` (seed 62). CC-BY-4.0.

## Build prompt

> In crate `fid-geo-ooc`, add module `query`: `ChunkedIndex` over a `ChunkManifest` and `ChunkCache` exposing `knn(q, k)`, `radius(q, r)`, `in_box(b)` and batch variants. Visit chunks by (lower-bound distance², cell code), build each chunk's `PointIndex` lazily through the GEO.INDEX `IndexCache` keyed by chunk digest, pass the current k-th distance as a bound, and merge with the (dist2, original index) total order used by `fid-geo-index`. Results must be identical to the in-memory `PointIndex` on the same cloud. Acceptance: AC-1 identity with in-memory results on 10⁴ queries; AC-2 boundary ties; AC-3 thread-count independence; AC-4 build counter bound. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), squared distances computed exactly as in `fid-geo-index` (no FMA). Files: `crates/fid-geo-ooc/src/query.rs`, `tests/query.rs`.

## Build notes

Built in `fid-geo-ooc` module `query` (`ChunkedIndex`). Judgement calls, stated openly:

- **Chunk lower bounds are exact cell bounds.** The partition's quantisation is three rounded operations, so a cell's real interval is found by a binary search over `f64` values in totalOrder for the first value quantising into the cell and the last one (the quantisation is monotone). The lower bound `(gx*gx + gy*gy) + gz*gz` of those bounds never exceeds the GEO.INDEX squared distance of any point in the chunk, with no tolerance.
- **No k-th distance bound is passed to the per-chunk tree.** `fid-geo-index`'s `PointTree::knn` has no bound parameter, so each visited chunk returns its own `min(k, count)` nearest and the merge keeps the `k` best. Results are unchanged; only the per-chunk work is larger than the Design's. Chunks are still pruned by lower bound (REQ-002).
- **Verified manifests only.** As for GEO.OOC.STREAM spatial order, a `ChunkedIndex` over a `Provenance::Untrusted` manifest refuses queries with `unverified_manifest` until `verify()` has run the original-order exactly-once proof (identity with the in-memory result needs every index exactly once and every point in its cell).
- **Box queries** visit the chunks whose cell bounds meet the box in cell-code order (their lower bound is 0, so this is REQ-002's tie order).
- **Batches** split the queries into contiguous ranges on scoped threads (1 to 256) and need a `Sync` store; a failing batch reports the first failing query in query order.
- **Errors**: query argument errors are GEO.INDEX's own values, checked in the same order before any chunk is loaded. A GEO.INDEX build error inside a chunk is reported with the original point index.
- **Coordinate limit**: `ChunkedIndex::new` refuses a manifest whose bounds exceed GEO.INDEX's `COORD_LIMIT` (2^500) on any axis with `point_out_of_range` (index `u64::MAX`, since only the bounds are read), so every query on such a cloud fails, as the in-memory build does. Bounds wider than the points are refused too (fail closed).
- **Identity scope**: REQ-001 identity covers only clouds the in-memory index accepts (at most `MAX_POINTS` = 2^32 points, coordinates within `COORD_LIMIT`).
- **Index budget**: cached trees stay within the budget, but a tree evicted while a query uses it lives until that query ends, so the budget can be exceeded by at most one tree per thread querying at once.
- **Builds under the lock**: per-chunk trees are built while the index-cache lock is held (one build per chunk digest); concurrent queries wait for each other's builds. Performance only.
- **AC-1 sizes**: as for GEO.OOC.LAYOUT AC-1, plain `cargo test` runs the 10^4 queries on the 200 000-point prefix of `cloud_5e6` (chunk sizes 65 536 and 4 096); CI's existing `FID_OOC_FULL=1` release step runs them on the full 5 x 10^6 points with 65 536-point chunks. Radius 1 mm is 0.001 (GEO coordinates are metres). Sixty further queries on the prefix are pinned by the independent brute-force reference `fixtures/geo/ooc/make_query_fixtures.py`, which also writes `boundary_ties/cases.json` (AC-2).

## Open questions

- GEO.INDEX.KDTREE: a bounded `knn` (an upper bound on the k-th squared distance) would let QUERY pass the current k-th distance as the Design says. Performance only.

## Children

_Leaf: none._
