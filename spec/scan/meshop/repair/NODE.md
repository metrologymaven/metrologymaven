---
id: SCAN.MESHOP.REPAIR
title: Self-intersection and non-manifold repair
level: 3
parent: SCAN.MESHOP
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP-001, REQ-SCAN.MESHOP-006, REQ-SCAN.MESHOP-010]
covers: [GOM.MSH.13]
depends_on: [SCAN.MESHOP.PROV, SCAN.POLY.QUAL, GEO.INDEX]
cli: [fid mesh repair, fid mesh check-defects]
mcp: [mesh_repair, mesh_check_defects]
journal: [mesh.repair@1]
updated: 2026-11-17
---

# SCAN.MESHOP.REPAIR: Self-intersection and non-manifold repair

## Purpose

Finds and fixes the topological and geometric defects that break downstream operations: self-intersecting triangles, non-manifold edges and vertices, duplicate and degenerate triangles, and inconsistent orientation. It reports defect counts before and after so the user can see exactly what changed.

## Scope

**In:**
- Defect detection: self-intersections (triangle pairs), non-manifold edges and vertices, duplicate triangles, degenerate triangles, inconsistent winding between neighbours
- Read-only defect check command with locations
- Repair: remove or re-triangulate intersecting regions, split non-manifold vertices, remove fins and duplicates, unify orientation per component
- Before/after defect report

**Out** (and which node owns it instead):
- Topological counts without self-intersection (owned by `SCAN.POLY.QUAL`)
- Hole filling that may follow a repair (owned by `SCAN.MESHOP.HOLES`)

## Requirements

- **REQ-SCAN.MESHOP.REPAIR-001** When `mesh.repair@1` runs, the system shall create a new mesh version through the `MeshOp` contract and shall leave the input mesh bit-identical.
- **REQ-SCAN.MESHOP.REPAIR-002** The defect check shall report the counts and locations of self-intersecting triangle pairs, non-manifold edges, non-manifold vertices, duplicate triangles, degenerate triangles and winding inconsistencies, and shall be a read-only command.
- **REQ-SCAN.MESHOP.REPAIR-003** When a repair runs, the result shall report each defect count before and after the repair.
- **REQ-SCAN.MESHOP.REPAIR-004** Self-intersection detection shall find every intersecting pair of non-adjacent triangles, verified against a brute-force reference on the defect fixture with zero misses.
- **REQ-SCAN.MESHOP.REPAIR-005** Triangles re-created during repair shall be flagged synthesized.
- **REQ-SCAN.MESHOP.REPAIR-006** Repair shall be determinism class D0.
- **REQ-SCAN.MESHOP.REPAIR-007** Defect check and repair on a 10⁸-triangle mesh shall complete within a published time and memory budget on reference hardware [unverified].

## Interfaces

- **Offers:** `mesh.check_defects@1` (R1), `mesh.repair@1` (W2); `DefectReport`; a kernel defect-detection API used by `SCAN.MESHOP.HOLES` (close).
- **Consumes:** SCAN.MESHOP.PROV, SCAN.POLY.QUAL, GEO.INDEX (triangle BVH for intersection candidates).

## Decomposition hints

DETECT (defect detection and check command), FIXTOPO (non-manifold, duplicate, degenerate, orientation fixes), FIXSELF (self-intersection removal and re-triangulation).

## Open questions

- Exact triangle-triangle intersection predicates under KNP-1 (no FMA) need a robust-arithmetic approach; shared concern with `SCAN.MESHOP.CUT`.
- **At split (2026-11-17):** DETECT assumes exact-sign predicates from `KRN.NUM`, which has no predicates child yet (cross-domain gap). FIXSELF removes and refills intersecting regions using the loop triangulation of `SCAN.MESHOP.HOLES.FLAT`; `SCAN.MESHOP.HOLES.CLOSE` in turn calls DETECT and FIXTOPO, so the two modules depend on each other's leaves but not in a cycle.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.MESHOP.REPAIR.DETECT | Mesh defect detection and check command (leaf) | 002 004 006 007 | GOM.MSH.13 |
| SCAN.MESHOP.REPAIR.FIXTOPO | Non-manifold, duplicate, degenerate and orientation fixes (leaf) | 005 006 | none |
| SCAN.MESHOP.REPAIR.FIXSELF | Self-intersection removal and re-triangulation (leaf) | 005 006 | none |
| SCAN.MESHOP.REPAIR.RCMD | Mesh repair command and before/after report (leaf) | 001 003 005 006 007 | GOM.MSH.13 |
