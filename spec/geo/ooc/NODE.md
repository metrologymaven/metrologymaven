---
id: GEO.OOC
title: Out-of-core point clouds and level of detail
level: 2
parent: GEO
kind: subsystem
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-GEO-004, REQ-GEO-005]
covers: []
depends_on: [GEO.TYPES, GEO.INDEX, CORE]
cli: [fid geo *-chunks, fid geo build-lod]
mcp: [geo_*_chunks, geo_build_lod]
journal: [geo.*_chunks@*, geo.build_lod@*]
updated: 2026-11-16
---

# GEO.OOC: Out-of-core point clouds and level of detail

## Purpose

Point clouds of 10⁸ points and more that do not fit in memory: a chunked on-disk representation in the CORE content-addressed store, streaming iteration for kernel operations, spatial queries that span chunks, and level-of-detail summaries that the viewer streams. It keeps very large scans usable without making any value of record depend on the LOD or on chunk boundaries.

## Scope

**In:**
- Spatially partitioned chunk layout (e.g. octree cells) with per-chunk digests
- Chunk cache with a configurable memory budget and eviction
- Streaming iteration over all points, in a fixed order, for kernel operations
- Cross-chunk k-nearest and radius queries composed from per-chunk GEO.INDEX indexes
- LOD summaries (representative subsets per node) for display
- Conversion between in-memory `PointCloud` and chunked form

**Out** (and which node owns it instead):
- Reading LAS, LAZ, COPC, E57 files (owned by `IO`; IO may write directly into chunks)
- GPU streaming and rendering of LOD (owned by `VIZ`)
- In-memory index structures (owned by `GEO.INDEX`)
- Content-addressed storage itself (owned by `CORE`)

## Requirements

- **REQ-GEO.OOC-001** The system shall hold point clouds of at least 10⁹ points with peak resident memory bounded by a configured budget, default 4 GiB [unverified].
- **REQ-GEO.OOC-002** Streaming iteration over a chunked cloud shall visit every point exactly once in a fixed order that depends only on the cloud's content digest.
- **REQ-GEO.OOC-003** Cross-chunk k-nearest and radius queries shall return the same results as the equivalent in-memory GEO.INDEX query on the same points.
- **REQ-GEO.OOC-004** The chunked cloud shall provide LOD summaries such that a display request for N points returns at most N points chosen deterministically from the full cloud.
- **REQ-GEO.OOC-005** No value of record shall be computed from LOD summaries; any kernel operation on a chunked cloud shall read full-resolution points.
- **REQ-GEO.OOC-006** If a chunk fails its digest check when loaded, then the system shall fail the operation with a typed integrity error naming the chunk.
- **REQ-GEO.OOC-007** Converting an in-memory cloud to chunked form and back shall reproduce the original element digest.

## Interfaces

- **Offers:** `ChunkedCloud`, `ChunkId`, `ChunkCache`, `PointStream`, `LodNode`, `LodQuery`; commands `geo.build_lod@1`, `geo.convert_chunks@1`.
- **Consumes:** CORE content-addressed store and jobs/cancellation; GEO.INDEX per-chunk indexes; GEO.TYPES `PointCloud`.

## Decomposition hints

LAYOUT (partitioning and on-disk chunk format), CACHE (memory budget and eviction), STREAM (ordered iteration), QUERY (cross-chunk queries), LOD (summaries for display).

## Open questions

- Whether the on-disk chunk format should follow COPC so IO can memory-map files directly; that couples GEO to a file format owned by IO.
- Phase set to P1b; VIZ (P1a) may need LOD earlier for large imports. Confirm in review.

## Children

_None yet: status `open`._
