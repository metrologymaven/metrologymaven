---
id: GEO.DIST.FACE
title: Point-to-trimmed-face and point-to-body projection
level: 3
parent: GEO.DIST
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GEO.DIST-001, REQ-GEO.DIST-002, REQ-GEO.DIST-008]
covers: []
depends_on: [GEO.CAD.TOPO, GEO.NURBS.INVERT, GEO.NURBS.ANALYTIC, GEO.INDEX.BVH, GEO.DIST.SIGN, GEO.DIST.BATCH]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.DIST.FACE: Point-to-trimmed-face and point-to-body projection

## Purpose

The exact closest point from a query point to a trimmed CAD face and to a whole CAD body, using the exact NURBS or analytic surface and the trimming loops, never the tessellation. It implements the `ProjectionTarget` trait from GEO.DIST.BATCH for faces, face sets and bodies, so batch projection, options and the command work unchanged for CAD.

## Scope

**In:**
- Candidate faces from a BVH over face bounding boxes (computed from control hulls or analytic bounds, so no tessellation is needed)
- Per face: single-surface inversion; accept if the foot parameter is inside the trimmed region (GEO.CAD.TOPO classification), otherwise project onto the face boundary (edge curves and vertices)
- Body and face-set projection: minimum over candidate faces with pruning, tie-break by lowest face ID
- Sign through GEO.DIST.SIGN's CAD rule with face orientation; incident normals and angles at edges and vertices
- Surface parameters of the foot point in the result
- Instances: projection in the instance frame (GEO.CAD.ASSY)

**Out** (and which node owns it instead):
- Single-surface inversion (owned by `GEO.NURBS.INVERT` and `GEO.NURBS.ANALYTIC`)
- Trimming classification (owned by `GEO.CAD.TOPO`)
- Batching and the command (owned by `GEO.DIST.BATCH` and `GEO.DIST.CMD`)

## Requirements

- **REQ-GEO.DIST.FACE-001** For each query point, the system shall return the closest point on the trimmed face, face set or body, with distance, face ID and surface parameters.
- **REQ-GEO.DIST.FACE-002** The foot point shall lie on the exact face surface within 1e-9 m and inside or on the trimmed boundary within the topology tolerance [unverified tolerance].
- **REQ-GEO.DIST.FACE-003** Candidate search shall be conservative, so a face is skipped only if its bound is provably farther than the current best distance.
- **REQ-GEO.DIST.FACE-004** When two faces give equal distance within 1e-15 m, the system shall choose the lowest face ID.
- **REQ-GEO.DIST.FACE-005** Face projection shall not depend on tessellation, and therefore not on the outcome of D-024.
- **REQ-GEO.DIST.FACE-006** Face projection shall be deterministic class D0 or D1 and follow KNP-1.

## Interfaces

- **Offers:** `FaceTarget`, `BodyTarget` implementing `ProjectionTarget`; `FaceHit { foot, distance, face, uv, feature }`.
- **Consumes:** GEO.CAD.TOPO (faces, loops, `classify_uv`, orientation), GEO.NURBS (inversion, evaluation), GEO.INDEX.BVH (over face boxes), GEO.DIST.SIGN (CAD rule).

## Decomposition hints

FACE1 (single trimmed face: inversion plus trim test plus boundary fallback), BODY (candidate search and minimum over faces), SIGNCAD (incident normals and angles at edges and vertices), REF (reference fixtures from analytic bodies with known closest points).

## Open questions

- Waits for GEO.CAD.TOPO to define the trimming and orientation API; it does not wait for D-024.
- Reference fixtures for trimmed NURBS faces need either analytic bodies with known answers or an independent oracle; GEO.NURBS.REF's approach should extend to trimmed faces.

## Children

_None yet: status `open`._
