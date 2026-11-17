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
satisfies: [REQ-GEO.OOC-003]
covers: []
depends_on: [GEO.OOC.CACHE, GEO.INDEX.KDTREE, GEO.INDEX.BUDGET]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
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

## Open questions

- None.

## Children

_Leaf: none._
