---
id: VIZ.RENDER.LOD.TRAV
title: Screen-space-error traversal, budget and point counts
level: 4
parent: VIZ.RENDER.LOD
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.LOD-001, REQ-VIZ.RENDER.LOD-003, REQ-VIZ.RENDER.LOD-004, REQ-VIZ.RENDER.LOD-006]
covers: [PW.VIZ.01]
depends_on: [VIZ.RENDER.SCENE.PROJ, VIZ.RENDER.FRAME.GRAPH, GEO.TYPES.POINTS]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.LOD.TRAV: Screen-space-error traversal, budget and point counts

## Purpose

Decide, each frame, which chunks of a point-cloud hierarchy to draw: traverse by screen-space error under a per-frame point budget, never leave a region empty while finer levels load, and publish honest displayed and total counts. It works over a `ChunkHierarchy` trait so P1a can use in-memory hierarchies of small CMM point sets, and the streaming source lands with `VIZ.RENDER.LOD.CACHE`.

## Scope

**In:** the `ChunkHierarchy` trait; an in-memory hierarchy builder for clouds that fit in RAM; SSE traversal with a priority queue; budget enforcement; coarsest-loaded fallback; `LodStats`.
**Out:** GPU chunk cache and streaming (`VIZ.RENDER.LOD.CACHE`); raster passes (`VIZ.RENDER.LOD.RASTER`); attribute modes (`VIZ.RENDER.LOD.ATTR`); octree building on disk (`GEO.OOC.LAYOUT`, `GEO.OOC.LOD`).

## Requirements

- **REQ-VIZ.RENDER.LOD.TRAV-001** While a cloud exceeds the per-frame point budget (default 5 × 10^6 [unverified]), traversal shall select chunks in descending screen-space error so that the drawn point count never exceeds the budget.
- **REQ-VIZ.RENDER.LOD.TRAV-002** Screen-space error shall be computed from the chunk's f64 bounds, its geometric error and the camera projection, with ties broken by ascending chunk ID.
- **REQ-VIZ.RENDER.LOD.TRAV-003** While a selected chunk is not resident, traversal shall draw its nearest resident ancestor rather than leave the region empty.
- **REQ-VIZ.RENDER.LOD.TRAV-004** Traversal shall publish `LodStats { displayed, total, budget }` for every presented frame, where total is the cloud's full point count.
- **REQ-VIZ.RENDER.LOD.TRAV-005** Traversal of a hierarchy of 10^8 points in 10^5 chunks shall take under 2 ms per frame on the PLAT reference mid-range profile [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/cloud-1m` (10^6 points, seed 31) and budget 10^5, when traversal runs for each of 20 stored cameras, then displayed ≤ 10^5 for every camera and the selected chunk lists equal the stored expectations.
- **AC-2** Given the same fixture with half of the level-3 chunks marked non-resident, then every region is covered by a resident ancestor (checked by a CPU coverage test of chunk bounds).
- **AC-3** Given any frame, then `LodStats.total` equals 10^6 and `displayed` equals the sum of points of drawn chunks.
- **AC-4** Given a synthetic hierarchy of 10^5 chunks, when traversal is benchmarked, then the time is reported against the 2 ms budget [unverified until measured on PLAT reference hardware].

## Design

- **Registry command(s):** none; library. Budget is setting `viz.render.point_budget` (declared via `CORE.STO.SET`).
- **Data structures:** `trait ChunkHierarchy { fn root(&self) -> ChunkId; fn children(&self, c: ChunkId) -> &[ChunkId]; fn bounds_m(&self, c) -> Aabb64; fn geometric_error_m(&self, c) -> f64; fn point_count(&self, c) -> u64; fn resident(&self, c) -> bool; }`; `InMemoryHierarchy` built from `GEO.TYPES.POINTS` by a fixed-depth octree with deterministic per-node subsampling; `LodStats`.
- **Algorithm:** max-heap by SSE (f64), tie by chunk ID; pop while budget allows; replace non-resident with nearest resident ancestor (deduplicated); emit draw list sorted by chunk ID.
- **Errors:** `EmptyHierarchy`.
- **Determinism:** D1 (f64 SSE on the CPU; draw list identical for identical inputs on one platform).
- **Performance budget:** REQ-005.
- **Crate:** `fid-render` (module `lod::trav`).

## Test fixtures

`fixtures/viz/cloud-1m`: synthetic cloud generated from seed 31 with 20 stored cameras and expected chunk lists; project-authored, CC0.

## Build prompt

> In crate `fid-render`, add module `lod::trav`. Define trait `ChunkHierarchy` (root, children, f64 bounds, geometric error, point count, resident flag) and `InMemoryHierarchy` built from an in-memory point set with a fixed-depth octree and deterministic subsampling. Each frame, traverse by screen-space error (f64, from bounds, geometric error and projection; ties by chunk ID) with a max-heap under the point budget (setting `viz.render.point_budget`, default 5e6), substitute the nearest resident ancestor for non-resident chunks, and publish `LodStats { displayed, total, budget }`. Tests AC-1..AC-4 with `fixtures/viz/cloud-1m`. Rules: CONSTITUTION §8; C2 (LOD data is display-only and never used for values); no std HashMap.

## Open questions

- 10^8-point performance is reached only with streaming (`VIZ.RENDER.LOD.CACHE`, P1b).

## Children

_Leaf: none._
