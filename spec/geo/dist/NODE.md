---
id: GEO.DIST
title: Distance and closest-point projection
level: 2
parent: GEO
kind: subsystem
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GEO-003]
covers: []
depends_on: [GEO.INDEX, GEO.NURBS, GEO.CAD, GEO.FRAME]
cli: [fid geo project-*]
mcp: [geo_project_*]
journal: [geo.project_*@*]
updated: 2026-11-16
---

# GEO.DIST: Distance and closest-point projection

## Purpose

The CPU primitive that turns "where is this point relative to that surface" into numbers of record: closest point, signed distance and the reference entity (triangle or face) for points against meshes, trimmed CAD faces and whole CAD bodies. Surface comparison, alignment and GD&T all consume it, so its numerics and determinism are defined once.

## Scope

**In:**
- Point-to-triangle and point-to-mesh closest point
- Point-to-trimmed-face and point-to-body closest point, using exact NURBS geometry, not tessellation
- Signed distance by surface normal orientation, with defined behaviour at edges and vertices
- Search options: maximum distance, restriction to a face or element subset, normal-direction projection along a vector
- Batch projection of large point sets with deterministic results
- Per-result metadata: foot point, distance, sign, reference triangle or face ID, surface parameters

**Out** (and which node owns it instead):
- Deviation maps, comparison points, tolerancing and statistics (owned by `SCAN`)
- Spatial index construction (owned by `GEO.INDEX`)
- Single-surface inversion (owned by `GEO.NURBS`)
- Alignment objective functions (owned by `ALN`)

## Requirements

- **REQ-GEO.DIST-001** The system shall compute, for each query point, the closest point on a mesh, a trimmed CAD face or a CAD body, together with the distance and the ID of the reference triangle or face.
- **REQ-GEO.DIST-002** Point-to-CAD projection shall use the exact surface geometry, and its foot point shall lie on the trimmed face within 1e-9 m [unverified].
- **REQ-GEO.DIST-003** Signed distance shall be positive on the side of the outward surface normal, and when the foot point lies on an edge or vertex the sign shall follow a documented pseudo-normal rule.
- **REQ-GEO.DIST-004** Projection shall run on the CPU and be deterministic class D0 or D1, independent of thread count and batch partitioning.
- **REQ-GEO.DIST-005** When a maximum search distance is given and no surface lies within it, the system shall return a typed no-hit result for that point rather than a distant match.
- **REQ-GEO.DIST-006** If the query point and the target are in different frames, then the system shall fail with the GEO.FRAME frame-mismatch error.
- **REQ-GEO.DIST-007** Batch projection of 10⁷ points onto a 10⁶-triangle mesh shall complete in under 10 s on reference hardware [unverified].
- **REQ-GEO.DIST-008** When two candidate foot points are at equal distance, the system shall choose the one with the lowest reference entity ID.

## Interfaces

- **Offers:** `Project::closest(point, target, options) -> Projection`, batch variants, `SignedDistance`, `ProjectionOptions`; commands `geo.project_points@1` (R1).
- **Consumes:** GEO.INDEX primitive BVH; GEO.NURBS inversion; GEO.CAD trimmed faces and tessellation (for candidate search); GEO.FRAME.

## Decomposition hints

MESH (point-to-mesh), FACE (point-to-trimmed-face), SIGN (sign rules and pseudo-normals), BATCH (parallel deterministic batches and options).

## Split rationale

The split follows **target kind and pipeline stage**: point-to-mesh geometry (MESH), the sign convention shared by meshes and CAD (SIGN), exact projection onto trimmed faces and bodies (FACE), the deterministic batch driver with options and the frame check (BATCH), and the registry command (CMD, in `fid-geo-cmd`). MESH, SIGN, BATCH and CMD are leaves; FACE stays open until GEO.CAD.TOPO fixes the trimming API. FACE does not use tessellation, so it is not blocked by D-024.

Alternatives considered:
- **Sign inside MESH and FACE separately.** Rejected: REQ-GEO.DIST-003 asks for one documented rule; two implementations could disagree at edges.
- **Batch logic inside each target.** Rejected: thread-count independence (REQ-GEO.DIST-004) and options are the same for every target, so they are written once behind the `ProjectionTarget` trait.
- **Candidate search through tessellation for CAD.** Rejected: it would tie values of record to the D-024 engine; face bounding boxes from control hulls are enough.

## Open questions

- Phase set to P1b to match scan-to-CAD in CONSTITUTION §7; point-to-mesh may be needed earlier by ALN in P1a. Confirm in review.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GEO.DIST.MESH | Point-to-triangle and point-to-mesh closest point (leaf) | 001 005 008 | none |
| GEO.DIST.SIGN | Signed distance and pseudo-normal rules (leaf) | 003 | none |
| GEO.DIST.FACE | Point-to-trimmed-face and point-to-body projection | 001 002 008 | none |
| GEO.DIST.BATCH | Deterministic batch projection and options (leaf) | 004 005 006 007 | none |
| GEO.DIST.CMD | Point projection command (leaf) | 001 006 | none |
