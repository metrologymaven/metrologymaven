---
id: GEO.CAD.TESS.FACE
title: Constrained parameter-space meshing of trimmed faces
level: 4
parent: GEO.CAD.TESS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.CAD.TESS-001, REQ-GEO.CAD.TESS-005, REQ-GEO.CAD.TESS-006]
covers: []
depends_on: [GEO.CAD.TESS.EDGE, GEO.CAD.TOPO.TRIM, GEO.NURBS.EVAL]
uses_artifacts: [GEO.CAD.TESS.SPIKE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.TESS.FACE: Constrained parameter-space meshing of trimmed faces

## Purpose

Meshes one trimmed face: its loops, already discretised by GEO.CAD.TESS.EDGE, become constrained boundaries in the face's parameter chart; a constrained Delaunay triangulation fills the region between them, seeded with interior points spaced by the surface's curvature; every interior vertex is evaluated on the exact surface. The result is a conforming initial mesh that GEO.CAD.TESS.REFINE then refines to tolerance.

## Scope

**In:**
- Boundary polygon per loop from `CoedgeUv`, using the shared vertex indices
- Constrained Delaunay triangulation in a metric-scaled uv chart (scale factors from the surface's first fundamental form at the chart centre, to avoid slivers on stretched parametrisations)
- Interior seed points on a grid whose spacing follows the curvature bound for chordal tolerance t
- Exact robust orientation and in-circle predicates (adaptive-precision, after Shewchuk 1997 [unverified reference details])
- Triangle orientation from the face sense, so outward normals agree across the body
- Per-face failure result instead of a partial mesh

**Out** (and which node owns it instead):
- Edge sampling (owned by `GEO.CAD.TESS.EDGE`)
- Tolerance-driven refinement (owned by `GEO.CAD.TESS.REFINE`)
- Assembling the body mesh (owned by `GEO.CAD.TESS.BODY`)

## Requirements

- **REQ-GEO.CAD.TESS.FACE-001** Every interior vertex the face mesher creates shall be S(u, v) evaluated at its own parameters, so it lies on the exact surface within 1e-9 m [unverified].
- **REQ-GEO.CAD.TESS.FACE-002** The face mesh shall contain every boundary segment from GEO.CAD.TESS.EDGE as a triangle edge and shall not insert vertices on boundary segments.
- **REQ-GEO.CAD.TESS.FACE-003** Every triangle shall have its centroid classified `Inside` by GEO.CAD.TOPO.TRIM and shall be oriented so its normal agrees in sign with the face's outward normal at the centroid.
- **REQ-GEO.CAD.TESS.FACE-004** Each produced triangle shall carry the source face's `TopologyId`.
- **REQ-GEO.CAD.TESS.FACE-005** If the boundary polygons self-intersect, a loop has fewer than three distinct vertices or triangulation fails, then the mesher shall return `FaceFailed { face, reason }` and no triangles for that face.
- **REQ-GEO.CAD.TESS.FACE-006** Meshing shall be deterministic class D0: point insertion in fixed order and exact predicates, so the same face and parameters give a bit-identical triangle list.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/tess/analytic_set`, when every face is meshed at t = 1e-5 m, then every interior vertex is within 1e-9 m of its surface, every boundary segment appears exactly once as a triangle edge, and all centroids are `Inside`.
- **AC-2** Given the cylinder-with-hole's planar end faces, then the hole region contains no triangle.
- **AC-3** Given fixture `fixtures/geo/cad/tess/stretched_patch.json` (a NURBS patch with a 1:1000 parametrisation aspect), then no triangle has a 3D minimum angle below 5° [unverified threshold].
- **AC-4** (error path) Given fixture `fixtures/geo/cad/tess/bowtie_loop.json`, then `FaceFailed { reason: SelfIntersectingBoundary }`.
- **AC-5** Given AC-1 on three OSes, then triangle lists are bit-identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `FaceMesh { face: TopologyId, verts: Vec<VertexRef>, tris: Vec<[u64; 3]> }` where `VertexRef` is a shared boundary index or a new interior point with (u, v, xyz); `FaceFailed { face, reason }`.
- **Algorithm:** build boundary polygons; constrained Delaunay by incremental insertion (boundary vertices in index order, then interior seeds in row-major grid order) with constraint recovery by edge flips; remove triangles outside the trimmed region by flood fill from constrained edges; orient by face sense.
- **Errors:** `FaceFailed` (reported per face).
- **Determinism:** D0.
- **Performance budget:** 10⁵ triangles per second per thread [unverified].
- **Crate:** `fid-geo-cad` (L0, KNP-1 applies; buildable for wasm32).

## Test fixtures

- `analytic_set` (from GEO.CAD.TESS.EDGE), `stretched_patch.json` (seed 17), `bowtie_loop.json` (hand-written). CC-BY-4.0.

## Build prompt

> In crate `fid-geo-cad` (L0, no I/O, KNP-1, wasm32-buildable), add module `tess::face` with `mesh_face(&Body, face, &EdgeTessellation, params) -> Result<FaceMesh, FaceFailed>`. Read `docs/spikes/geo-cad-tess-base.md` first. Build boundary polygons from `tess::edge` coedge uv samples (shared vertex indices), triangulate with constrained Delaunay in a metric-scaled uv chart using exact adaptive orientation and in-circle predicates, insert curvature-spaced interior seeds in row-major order, keep only triangles whose centroid `topo::trim` classifies `Inside`, never split boundary segments, orient by face sense, evaluate interior vertices on the exact surface, tag triangles with the face ID. Failures return `FaceFailed { face, reason }`. Acceptance: AC-1 vertices on surface, boundary conformity, centroids inside; AC-2 holes empty; AC-3 no slivers on a stretched patch; AC-4 bow-tie failure; AC-5 bit-identical on three OSes. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1 (no FMA; exact predicates with fixed operation order). Files: `crates/fid-geo-cad/src/tess/face.rs`, `src/tess/predicates.rs`, `tests/tess_face.rs`, `fixtures/geo/cad/tess/`.

## Open questions

- Whether the exact predicates are written from the paper or borrowed from an allowlisted crate depends on the spike report and the ECO.RULES allowlist.

## Children

_Leaf: none._
