---
id: GEO.DIST.FACE.BODY
title: Face-set and body projection targets
level: 4
parent: GEO.DIST.FACE
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GEO.DIST.FACE-001, REQ-GEO.DIST.FACE-003, REQ-GEO.DIST.FACE-004, REQ-GEO.DIST.FACE-005, REQ-GEO.DIST.FACE-006]
covers: []
depends_on: [GEO.DIST.FACE.ONE, GEO.DIST.FACE.INCID, GEO.DIST.BATCH, GEO.INDEX.BVH, GEO.CAD.ASSY.MODEL, GEO.FRAME.XFORM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.DIST.FACE.BODY: Face-set and body projection targets

## Purpose

Implements GEO.DIST.BATCH's `ProjectionTarget` for a single face, a face set and a whole body (or a placed instance): conservative candidate search over face bounds built from exact geometry, the minimum over candidate faces with the lowest-face-ID tie-break, the signed distance through GEO.DIST.FACE.INCID, and transforms between the instance frame and the definition, so batch projection and `geo.project_points@1` work on CAD unchanged.

## Scope

**In:**
- Face bounds without tessellation: NURBS control-point boxes; analytic faces bounded from their boundary edges' boxes plus the surface's closed-form extent within the face's parameter ranges
- Candidate search: a `PrimitiveBvh` over face boxes, visited in ascending lower-bound order with best-distance pruning
- `FaceTarget`, `FaceSetTarget` (a face `IndexSet` or ID list), `BodyTarget`, `InstanceTarget`
- Tie-break: equal distances within 1e-15 m resolve to the lowest face ID
- Directional projection (`project_along`) by intersecting the line with candidate faces (surface-line intersection by Newton on Bézier patches and closed forms on analytic surfaces), keeping the nearest intersection inside the trim
- Instance handling: transform query into the definition frame, project, transform foot and normal back, scale distance by the similarity scale

**Out** (and which node owns it instead):
- Single-face candidates (owned by `GEO.DIST.FACE.ONE`)
- Sign at edges and vertices (owned by `GEO.DIST.FACE.INCID`)
- Batching, options and frame check (owned by `GEO.DIST.BATCH`)

## Requirements

- **REQ-GEO.DIST.FACE.BODY-001** For each query point, the body target shall return the closest point over all faces in the target with distance, signed distance, face ID and surface parameters.
- **REQ-GEO.DIST.FACE.BODY-002** A face shall be skipped only when its bound's lower distance is strictly greater than the current best distance, so pruning never changes the result relative to testing every face.
- **REQ-GEO.DIST.FACE.BODY-003** When two faces give distances equal within 1e-15 m, the target shall return the one with the lowest face ID.
- **REQ-GEO.DIST.FACE.BODY-004** Face bounds and candidate search shall be computed from exact geometry only, never from a tessellation.
- **REQ-GEO.DIST.FACE.BODY-005** For an instance, the result shall equal projecting the transformed query onto the definition and transforming back, with distances multiplied by the similarity scale.
- **REQ-GEO.DIST.FACE.BODY-006** Results shall be deterministic class D0, independent of thread count and batch partitioning (the batch driver supplies the parallelism).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/dist/face/ref_v1` body queries, when projected, then foot points and face IDs match the oracle within 1e-9 m.
- **AC-2** Given 10 000 seeded queries (seed 23) against the `analytic_set` bodies, when compared with a brute-force loop over all faces, then results are bit-identical.
- **AC-3** Given a query equidistant from two faces of a cube (on the diagonal plane of an edge), then the lower face ID is returned.
- **AC-4** Given an instance placed with a rotation and scale 2, then results equal the definition-frame computation with distance doubled.
- **AC-5** Given `project_along` queries on the cylinder-with-hole, then the nearest intersection inside the trim is returned, and lines through the hole return `NoHit`.
- **AC-6** Given batch projection of 10⁵ points with 1 and 8 threads, then results are identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `FaceBounds { boxes: Vec<Aabb> }` in face-index order; `BodyTarget { body, bvh, bounds, incid }`; `impl ProjectionTarget for FaceTarget | FaceSetTarget | BodyTarget | InstanceTarget`.
- **Algorithm:** `PrimitiveBvh::ordered_visit` over face boxes; per candidate face call `face::one::project_face`; keep the best with the tie rule; sign via `face::incid`; instances via GEO.FRAME.XFORM inverse and forward.
- **Errors:** `FrameMismatch` (from BATCH), `NonFiniteQuery`, `EmptyTarget`.
- **Determinism:** D0.
- **Performance budget:** 10⁶ points onto a 1 000-face body under 10 s on 8 threads [unverified].
- **Crate:** `fid-geo-dist` (L0, kernel graph, KNP-1 applies).

## Test fixtures

- `ref_v1` (GEO.DIST.FACE.REF), `analytic_set` (GEO.CAD.TESS fixtures, read as body descriptions). CC-BY-4.0.

## Build prompt

> In crate `fid-geo-dist` (L0, no I/O, KNP-1), add module `face::body` implementing `ProjectionTarget` (from `dist::batch`) for `FaceTarget`, `FaceSetTarget`, `BodyTarget` and `InstanceTarget`. Bound each face from exact geometry (NURBS control-point boxes; analytic faces from boundary-edge boxes plus closed-form surface extent), build a `fid-geo-index` `PrimitiveBvh` over the boxes, visit in ascending lower bound, skip a face only if its lower bound is strictly greater than the best distance, project with `face::one`, break ties within 1e-15 m by lowest face ID, sign with `face::incid`. `project_along` intersects the line with candidate faces and keeps the nearest in-trim hit. Instances: transform the query into the definition frame with `fid-geo-frame`, project, transform back, scale distances. Acceptance: AC-1 oracle; AC-2 bit-identical to brute force on 10 000 queries; AC-3 tie-break; AC-4 instance scale; AC-5 directional; AC-6 thread independence. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1, C2 (CPU only). Files: `crates/fid-geo-dist/src/face/body.rs`, `tests/face_body.rs`.

## Open questions

- GEO.INDEX.BVH's build prompt takes a `Mesh`; this leaf needs a constructor from a list of boxes (`PrimitiveBvh::from_boxes`). If GEO.INDEX.BVH does not add it, this leaf falls back to a linear scan in ascending lower-bound order, which is correct but slower. Raised for GEO.INDEX.BVH.

## Children

_Leaf: none._
