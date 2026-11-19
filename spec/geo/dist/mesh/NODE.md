---
id: GEO.DIST.MESH
title: Point-to-triangle and point-to-mesh closest point
level: 3
parent: GEO.DIST
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-dist]
built_pr: 287
satisfies: [REQ-GEO.DIST-001, REQ-GEO.DIST-005, REQ-GEO.DIST-008]
covers: []
depends_on: [GEO.INDEX.BVH, GEO.TYPES.MESH, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.DIST.MESH: Point-to-triangle and point-to-mesh closest point

## Purpose

The exact closest point from a query point to a triangle and to a whole mesh, with the reference triangle, the feature of the triangle the foot point lies on (interior, edge or vertex) and the unsigned distance. Sign is added by GEO.DIST.SIGN; batching and options by GEO.DIST.BATCH.

## Scope

**In:**
- Point-to-triangle closest point with Voronoi-region classification (interior, edge 0/1/2, vertex 0/1/2)
- Point-to-mesh search using the BVH ordered visitor with best-distance pruning
- Maximum search distance with a typed no-hit result
- Restriction to a triangle subset (an index set)
- Tie-break by lowest triangle index, then lowest feature code

**Out:**
- Sign and pseudo-normals (owned by `GEO.DIST.SIGN`)
- Directional projection, batching and frame checks (owned by `GEO.DIST.BATCH`)
- BVH construction (owned by `GEO.INDEX.BVH`)

## Requirements

- **REQ-GEO.DIST.MESH-001** For each query point, the system shall return the foot point, unsigned distance, reference triangle index and foot feature (interior, edge or vertex) of the closest point on the mesh.
- **REQ-GEO.DIST.MESH-002** The point-to-triangle routine shall classify the foot point by Voronoi region and compute it with a fixed operation order and no fused multiply-add, so results are bit-identical on x86-64, ARM64 and wasm32.
- **REQ-GEO.DIST.MESH-003** When a maximum distance d is given and no triangle lies within d, the system shall return `NoHit` for that point.
- **REQ-GEO.DIST.MESH-004** When two triangles give exactly equal squared distance, the system shall choose the lower triangle index.
- **REQ-GEO.DIST.MESH-005** When a triangle subset is given, the search shall consider only triangles in the subset.
- **REQ-GEO.DIST.MESH-006** Degenerate triangles (zero area) shall be handled as segments or points without producing NaN.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/dist/mesh_sphere_1e5` (from GEO.INDEX) and 10⁵ seeded query points, when projected, then foot points, distances and triangle indexes equal a brute-force reference over all triangles exactly.
- **AC-2** Given fixture `fixtures/geo/dist/triangle_regions.json` (queries in each of the 7 Voronoi regions of 50 triangles, with exact rational references), when projected, then features match and foot points are within 1e-15 m relative to triangle size.
- **AC-3** Given `max_distance = 1 mm` and points 2 mm from the mesh, when projected, then each result is `NoHit`.
- **AC-4** Given fixture `fixtures/geo/dist/shared_vertex_ties` (points equidistant from several triangles), when projected, then the lowest triangle index is chosen.
- **AC-5** Given a degenerate (collinear) triangle in the mesh, when projected, then no NaN occurs and the result equals the segment distance.
- **AC-6** Given AC-1 inputs on three targets, then the result digest is identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `MeshHit { foot: [f64; 3], dist: f64, tri: u64, feature: Feature }`; `enum Feature { Interior, Edge(u8), Vertex(u8) }`; `enum MeshProjection { Hit(MeshHit), NoHit }`.
- **Algorithm:** Voronoi-region closest point on triangle after Ericson, *Real-Time Collision Detection*, §5.1.5 (cited, not copied); mesh search via `PrimitiveBvh::ordered_visit`, pruning nodes whose lower bound exceeds the current best squared distance or d².
- **Errors:** `NonFiniteQuery`.
- **Determinism:** D0.
- **Performance budget:** median 2 µs per query on a 10⁶-triangle mesh [unverified].
- **Crate:** `fid-geo-dist` (kernel graph, KNP-1 applies).

## Test fixtures

- `triangle_regions.json` and `shared_vertex_ties`: generated with exact rational references by `tools/fixtures/geo_dist.py` (Python `fractions`); `mesh_sphere_1e5` shared with GEO.INDEX. CC-BY-4.0.

## Build prompt

> Create crate `fid-geo-dist` (pure Rust, f64, no I/O, kernel graph, KNP-1) with module `mesh`: `closest_on_triangle(p, a, b, c) -> (foot, dist2, Feature)` using Voronoi-region classification (after Ericson §5.1.5; write your own code), degenerate triangles treated as segments or points without NaN; `closest_on_mesh(p, mesh, bvh, opts) -> MeshProjection` using `PrimitiveBvh::ordered_visit` with pruning, optional `max_distance` (`NoHit`), optional triangle subset, and ties broken by lowest triangle index then feature code. Fixed operation order, no `mul_add`, sqrt via `fid-num`. Acceptance: AC-1 equality with brute force on 10⁵ queries; AC-2 Voronoi features and feet against rational references; AC-3 `NoHit`; AC-4 tie rule; AC-5 degenerate triangle; AC-6 identical digest on three targets. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1 (no FMA, no HashMap). Files: `crates/fid-geo-dist/src/mesh.rs`, `tests/mesh.rs`, `tools/fixtures/geo_dist.py`, `fixtures/geo/dist/*`.

## Open questions

- ALN may need point-to-mesh in P1a (parent open question). Moving this leaf earlier than its P1b parent needs a phase decision (CONSTITUTION §7, D-006); it has no CAD dependency, so it could start earlier without technical risk.

## Children

_Leaf: none._
