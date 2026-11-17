---
id: GEO.OOC
title: Out-of-core point clouds and level of detail
level: 2
parent: GEO
kind: subsystem
status: split
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

## Split rationale

The split follows the **life of a chunked cloud**: how it is laid out and built (LAYOUT), how chunks are loaded under a memory budget with integrity checks (CACHE), how kernel code reads every point in a fixed order (STREAM), how neighbour queries span chunks with in-memory-identical results (QUERY), how display summaries are made without ever feeding values of record (LOD), and the registry commands (CMD, in `fid-geo-cmd`). All six are leaves. `fid-geo-ooc` sits outside the kernel graph because it does storage I/O; kernel code reads chunked clouds only through the `PointSource` trait defined in `fid-geo-types` (GEO.TYPES.ENV).

The open COPC question is resolved in GEO.OOC.LAYOUT for now: an own chunk format inside the CORE store, with IO converting COPC through the streaming writer, so GEO does not depend on a format IO owns.

Alternatives considered:
- **LOD inside LAYOUT** (one octree for both). Rejected: LOD must be independent of chunk size (REQ-GEO.OOC.LOD-002) and type-separated from full-resolution data (REQ-GEO.OOC-005).
- **Memory-mapped COPC as the native layout.** Deferred as above.
- **Stream and query in one child.** Rejected: different oracles (exactly-once order versus identity with GEO.INDEX).

## Open questions

- Whether the on-disk chunk format should follow COPC so IO can memory-map files directly; that couples GEO to a file format owned by IO.
- Phase set to P1b; VIZ (P1a) may need LOD earlier for large imports. Confirm in review.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GEO.OOC.LAYOUT | Chunk layout, manifest and conversion (leaf) | 001 007 | none |
| GEO.OOC.CACHE | Chunk cache, memory budget and integrity checks (leaf) | 001 006 | none |
| GEO.OOC.STREAM | Ordered full-resolution point streaming (leaf) | 002 005 | none |
| GEO.OOC.QUERY | Cross-chunk neighbour queries (leaf) | 003 | none |
| GEO.OOC.LOD | Level-of-detail summaries for display (leaf) | 004 005 | none |
| GEO.OOC.CMD | Chunked cloud and LOD commands (leaf) | 001 006 007 | none |
