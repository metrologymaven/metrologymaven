---
id: GEO.INDEX
title: Spatial indexes and neighbour queries
level: 2
parent: GEO
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-GEO-004]
covers: []
depends_on: [GEO.TYPES]
cli: [fid geo query-*, fid geo build-index]
mcp: [geo_query_*, geo_build_index]
journal: [geo.query_*@*, geo.build_index@*]
updated: 2026-11-16
---

# GEO.INDEX: Spatial indexes and neighbour queries

## Purpose

In-memory spatial acceleration structures for points and triangles, and the deterministic queries built on them: k-nearest, radius, box and ray-against-primitives. Fitting, alignment, distance computation and scan processing all need neighbour queries with reproducible results, so the index and its tie-breaking rules live in one subsystem.

## Scope

**In:**
- Point index (k-d tree or equivalent) over a point set or cloud chunk
- Primitive index (bounding volume hierarchy) over triangles and over tessellated CAD faces
- k-nearest, fixed-radius, axis-aligned box and ray queries
- Deterministic tie-breaking and result ordering
- Memory budget declaration and accounting per index
- Index build cost reporting and caching by element digest

**Out** (and which node owns it instead):
- Exact closest point on a mesh or CAD face (owned by `GEO.DIST`, which uses these indexes)
- Indexes spanning out-of-core chunks (owned by `GEO.OOC`, which composes per-chunk indexes)
- GPU acceleration structures for rendering (owned by `VIZ`)

## Requirements

- **REQ-GEO.INDEX-001** The point index shall answer k-nearest queries for any k from 1 to 1024 and fixed-radius queries, returning exact results (no approximation) by default.
- **REQ-GEO.INDEX-002** When two candidates are at equal distance, the index shall order them by ascending point index, so query results are identical across runs and platforms.
- **REQ-GEO.INDEX-003** Index construction and queries shall be deterministic class D0 or D1 and independent of thread count.
- **REQ-GEO.INDEX-004** The point index over 10⁸ points shall fit within a memory overhead of 32 bytes per point beyond the coordinates [unverified].
- **REQ-GEO.INDEX-005** A k-nearest query with k = 16 on a 10⁸-point index shall complete in under 10 µs median on reference hardware [unverified].
- **REQ-GEO.INDEX-006** If building an index would exceed its declared memory budget, then the system shall fail with a typed budget error before allocating, rather than exhaust memory.
- **REQ-GEO.INDEX-007** The primitive index shall return all triangles whose bounding volumes intersect a query sphere, box or ray, with no false negatives.
- **REQ-GEO.INDEX-008** Built indexes shall be keyed by the digest of the source element and reused while that digest is unchanged.

## Interfaces

- **Offers:** `SpatialIndex` trait, `PointIndex`, `PrimitiveBvh`, `KnnQuery`, `RadiusQuery`, `RayQuery`, `MemoryBudget`; commands `geo.build_index@1`, `geo.query_knn@1`, `geo.query_radius@1` (R1).
- **Consumes:** GEO.TYPES elements and digests.

## Decomposition hints

KDTREE (point index and queries), BVH (primitive index), BUDGET (memory accounting and caching), BENCH (reference hardware and performance fixtures).

## Open questions

- Reference hardware for the performance targets is not defined yet; it is likely a PLAT concern.

## Children

_None yet: status `open`._
