---
id: GEO.OOC.LOD
title: Level-of-detail summaries for display
level: 3
parent: GEO.OOC
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.OOC-004, REQ-GEO.OOC-005]
covers: []
depends_on: [GEO.OOC.STREAM, CORE.STO]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.OOC.LOD: Level-of-detail summaries for display

## Purpose

Lets the viewer show a billion-point cloud: per octree node, a deterministic representative subset of points, stored in the evictable CORE cache store, and a query that returns at most N points for a region, chosen the same way every time. LOD data has its own type so that nothing of record can be computed from it.

## Scope

**In:**
- `LodPoints`: a display-only type, not a `GeometryElement` and not a `PointSource`
- Per-node representative selection by an integer hash of the original index (seeded, counter-based), up to `node_budget` points per node, parents built from children
- LOD build by one spatial-order pass, stored in the CORE.STO cache store keyed by manifest digest and parameters
- `lod_query(region, max_points)` returning ≤ N points breadth-first by octree level, ties by cell code

**Out:**
- GPU streaming and rendering (owned by `VIZ`)
- Any kernel computation (forbidden on LOD by REQ-GEO.OOC-005)

## Requirements

- **REQ-GEO.OOC.LOD-001** When a display request for at most N points over a region is made, the LOD query shall return at most N points, and the same request on the same cloud and parameters shall return the same points in the same order.
- **REQ-GEO.OOC.LOD-002** Representatives shall be chosen by ranking points by a 64-bit integer hash of (seed, original index), so selection is D0 and independent of chunking.
- **REQ-GEO.OOC.LOD-003** `LodPoints` shall not implement `PointSource` and shall offer no conversion to any geometry element.
- **REQ-GEO.OOC.LOD-004** LOD data shall be stored only in the CORE.STO cache store, and deleting it shall change no document state.
- **REQ-GEO.OOC.LOD-005** Every LOD point shall be an exact copy of a full-resolution point (no averaging), carrying its original index.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/ooc/cloud_5e6`, when the LOD is built and `lod_query(whole, 100 000)` runs twice and on two platforms, then each result has ≤ 100 000 points and the four results are identical.
- **AC-2** Given the cloud chunked with two different `max_chunk_points`, when LODs are built with the same seed, then `lod_query` results are identical.
- **AC-3** Given any LOD point, when looked up by original index in the full cloud, then coordinates are bit-identical.
- **AC-4** Given the LOD cache entry deleted, when the document digest is computed, then it is unchanged, and the next query rebuilds the same LOD.
- **AC-5** A compile-fail test shows `LodPoints` cannot be passed to a function taking `impl PointSource` or `GeometryElement`.

## Design

- **Registry command(s):** none; library leaf (the `geo.build_lod@1` command is in GEO.OOC.CMD).
- **Data structures:** `LodNode { cell, points: LodPoints }`; `LodPoints { original_index: Vec<u64>, xyz: Vec<[f64; 3]>, colour: Option<..>, intensity: Option<..> }`; `LodParams { seed, node_budget }`.
- **Algorithm:** for each leaf cell keep the `node_budget` points with the smallest `splitmix64(seed ⊕ original index)`; a parent keeps the smallest-hash points among its children's representatives; selection is therefore independent of chunk boundaries as long as the octree cells are equal, and AC-2 uses a fixed LOD octree independent of chunking.
- **Errors:** cache store errors; `UnknownCloud`.
- **Determinism:** D0.
- **Performance budget:** LOD build at ≥ 50 M points/s with cached chunks [unverified].
- **Crate:** `fid-geo-ooc`.

## Test fixtures

- `cloud_5e6` from GEO.OOC.LAYOUT.

## Build prompt

> In crate `fid-geo-ooc`, add module `lod`: display-only `LodPoints` (original index, xyz, optional colour and intensity; no `PointSource`, no conversion to `GeometryElement`), `build_lod(manifest, params { seed, node_budget })` in one spatial-order pass over a fixed LOD octree independent of chunking, choosing per node the points with the smallest `splitmix64(seed ^ original_index)` and parents from children's representatives; store results in the CORE.STO cache store keyed by (manifest digest, params); `lod_query(region, max_points)` returns ≤ max_points breadth-first by level, ties by cell code. LOD points are exact copies of full-resolution points. Acceptance: AC-1 repeatable results across runs and platforms; AC-2 independence from chunk size; AC-3 bit-identical copies; AC-4 cache deletion changes no document state and rebuilds identically; AC-5 compile-fail tests. Constraints: CONSTITUTION C1/C2 (no value of record from LOD), §8 (no panic, no `unsafe`). Files: `crates/fid-geo-ooc/src/lod.rs`, `tests/lod.rs`.

## Open questions

- VIZ (P1a) may need LOD for large imports before this P1b leaf; moving it earlier needs a phase decision (CONSTITUTION §7, D-006).

## Children

_Leaf: none._
