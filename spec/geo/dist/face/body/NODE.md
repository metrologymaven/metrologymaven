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

- GEO.INDEX.BVH's build prompt takes a `Mesh`; this leaf needs a constructor from a list of boxes (`PrimitiveBvh::from_boxes`). If GEO.INDEX.BVH does not add it, this leaf falls back to a linear scan in ascending lower-bound order, which is correct but slower. Raised for GEO.INDEX.BVH. GEO.INDEX.BVH has no `from_boxes`; `fid-geo-dist` `face::body` stores each face box `[lo, hi]` as the degenerate triangle `(lo, hi, hi)` through `PrimitiveBvh::build_triangles`, whose triangle box is exactly `[lo, hi]`, so `ordered_visit` and `ray` work unchanged.
- **Partial build (no `built_in` yet).** `fid-geo-dist` `face::body` implements this leaf (`FaceTarget`, `FaceSetTarget`, `BodyTarget`, `InstanceTarget`, `FaceBounds`, `BodyError`). AC-2 to AC-6 are met; **AC-1 is open for one body**: the torus band `torus_segment.json` face F1 is refused by GEO.CAD.TOPO.TRIM (`Undecidable(DoublyPeriodicSeam)`, the gap recorded by GEO.DIST.FACE.ONE), so all 160 torus queries fail closed with `BodyError::Face` (none is answered); every query of the other 8 `ref_v1` bodies matches the oracle within 1e-9 m (test `ac1_ref_v1_body_queries`, which asserts the 160 failures and flips when the gap closes).
- On a `FaceTarget`, a `FaceSetTarget` or a subset-restricted target the sign is relative to the target's faces (the GEO.DIST.SIGN rule at the target's closest point), not whether the point is inside the body; only an unrestricted `BodyTarget` signs inside/outside of the body.
- AC-1 builds each `ref_v1` body with one open shell per face and that face's own copies of its edges and vertices (labelled `<face>/<label>`): the fixture bodies give both coedges of some shared closed edges the same sense, which GEO.CAD.TOPO.MODEL's pairing check refuses. The geometry is unchanged; signs at shared edges are then `Boundary` (AC-1 compares feet, faces and features only).
- AC-2 uses invented bodies (`cube_labelled.json`, the INCID `tube.json`, `lblock.json`, `cone_c20.json`, `cone_c30.json`) and four `ref_v1` bodies, because the GEO.CAD.TESS `analytic_set` bodies do not exist yet; re-point when they land. AC-5 uses the INCID `tube.json` (cylinder with a hole) with the oracle `fixtures/geo/dist/face/body/make_body_cases.py`, which also labels 600 points around the concave L-block (distance and inside/outside, near the concave edge and corners).
- Directional projection intersects planes, cylinders, spheres and cones in closed form. A torus or NURBS face that the line's candidate search reaches is refused with `BodyError::AlongUnsupported` (fail closed) instead of the Design's quartic and Newton-on-Bézier-patch intersection; a line lying in a face's surface is `AlongDegenerate`. Both remain open items (not ACs).
- Face bounds: boundary edge boxes (whole circles and ellipses), imported pcurve images, and the closed-form critical points of each coordinate on the surface (sphere and torus points and the cone apex kept unless the trim classifies them `Outside`; cylinder generators when the axis is perpendicular to a coordinate axis); a plane, cylinder or cone face that cannot be shown bounded (a far parameter point must classify `Outside`), or whose trim or edge geometry is missing, gets the whole-space box and is never pruned. Pad `max(1e-6 m, 64 topo_tol) + 2^-40` of the coordinate magnitude [unverified].
- Choice: the face of lowest ID among those within 1e-15 m of the minimum over all projected faces, which does not depend on the visiting order; a face is skipped when its lower bound exceeds `min(best + 1e-15, max_distance)` (stricter than REQ-002 asks), so pruned and brute-force results are bit-identical. Any error of a face that cannot be pruned fails the query (fail closed); a pruned face's error is not reported.
- GEO.CAD.TOPO.TRIM does not treat a cone's apex line as boundary, so GEO.DIST.FACE.ONE could return a foot on the other nappe of a cone face that contains its apex (found here at the INCID cone C30). `face::one` now drops interior candidates on the other nappe when the face's trimming segments lie on one nappe (`cone_nappe`); the directional search applies the same rule. Raised for GEO.CAD.TOPO.TRIM.
- `FaceSetTarget` takes face indexes or topology IDs, not a `fid-geo-sel` `IndexSet` (no new dependency); an `IndexSet`'s indexes convert directly. GEO.DIST.BATCH gained `HitSign::Cad` (CAD sign status, same record code as a mesh status) and `ProjectError::Body`; `face::one`'s edge-curve reference is now `dyn Curve + Sync` so prepared faces can be shared by batch threads.
- The performance budget (10^6 points onto 1 000 faces under 10 s) is not measured.

## Children

_Leaf: none._
