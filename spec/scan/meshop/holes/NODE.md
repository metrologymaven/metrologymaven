---
id: SCAN.MESHOP.HOLES
title: Hole filling and closing
level: 3
parent: SCAN.MESHOP
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP-001, REQ-SCAN.MESHOP-004, REQ-SCAN.MESHOP-005, REQ-SCAN.MESHOP-007]
covers: [GOM.MSH.05, GOM.MSH.07]
depends_on: [SCAN.MESHOP.PROV, SCAN.MESHOP.REPAIR]
cli: [fid mesh fill-*, fid mesh close]
mcp: [mesh_fill_*, mesh_close]
journal: [mesh.fill_*, mesh.close@1]
updated: 2026-11-17
---

# SCAN.MESHOP.HOLES: Hole filling and closing

## Purpose

Fills gaps in scanned meshes and closes a mesh into a watertight body. Filling covers both the automatic mode (every hole below a perimeter) and the interactive mode (one chosen boundary loop); closing goes further and guarantees zero boundary and non-manifold edges or fails with the obstruction named. All created triangles are flagged synthesized so comparison can exclude them.

## Scope

**In:**
- Automatic fill of every boundary loop below a maximum perimeter in metres
- Fill of one chosen boundary loop (by loop ID from the quality report or by a selected boundary edge)
- Fill shape options: flat (minimal area) or curvature-continuous with the surrounding surface
- Per-hole report: loop ID, perimeter, filled area, triangles created
- Close: fill all loops and resolve remaining defects into a watertight body, or fail with a typed obstruction

**Out** (and which node owns it instead):
- Detection and repair of self-intersections and non-manifold geometry (owned by `SCAN.MESHOP.REPAIR`, which close calls)
- Boundary loop counting (owned by `SCAN.POLY.QUAL`)
- Synthesized flag mechanics (owned by `SCAN.MESHOP.PROV`)

## Requirements

- **REQ-SCAN.MESHOP.HOLES-001** When a fill or close command runs, the system shall create a new mesh version through the `MeshOp` contract and shall leave the input mesh bit-identical.
- **REQ-SCAN.MESHOP.HOLES-002** When automatic fill runs with maximum perimeter P in metres, the system shall fill every boundary loop whose perimeter is below P and no other loop.
- **REQ-SCAN.MESHOP.HOLES-003** When a single loop is chosen, the system shall fill exactly that loop, or fail with a typed error if the loop ID does not exist in the input.
- **REQ-SCAN.MESHOP.HOLES-004** The result shall report, for each filled hole, its loop ID, perimeter in metres, filled area in square metres and the number of triangles created.
- **REQ-SCAN.MESHOP.HOLES-005** Every triangle created by filling or closing shall be flagged synthesized.
- **REQ-SCAN.MESHOP.HOLES-006** When `mesh.close@1` succeeds, the output shall have zero boundary edges and zero non-manifold edges as measured by `SCAN.POLY.QUAL`.
- **REQ-SCAN.MESHOP.HOLES-007** If closing cannot produce a watertight result, then the command shall fail with a typed error naming the obstruction (for example a self-intersecting fill or a non-manifold vertex it cannot resolve) and its location, and shall create no mesh.
- **REQ-SCAN.MESHOP.HOLES-008** Fill and close shall be determinism class D0.

## Interfaces

- **Offers:** `mesh.fill_holes@1` (automatic), `mesh.fill_hole@1` (single loop), `mesh.close@1` (all W2); `HoleFillReport`.
- **Consumes:** SCAN.MESHOP.PROV (`MeshOp`, synthesized flag), SCAN.MESHOP.REPAIR (defect detection and repair used by close), SCAN.POLY.QUAL (loop IDs and counts).

## Decomposition hints

FILL (loop triangulation, flat and curvature-continuous), CLOSE (watertight closing on top of FILL and REPAIR).

## Open questions

- Loop IDs must be stable for the interactive mode; proposed: the lowest vertex index on the loop, as reported by QUAL.
- Curvature-continuous fill method (for example minimum-energy fairing) is a leaf-level choice.
- **Resolved at split (2026-11-17):** loop IDs are the lowest vertex index on the loop (`SCAN.MESHOP.HOLES.FLAT`); the curvature-continuous shape is refinement plus uniform bi-Laplacian fairing (`SCAN.MESHOP.HOLES.FAIR`). `SCAN.MESHOP.REPAIR.FIXSELF` reuses FLAT's loop triangulation.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.MESHOP.HOLES.FLAT | Hole fill commands and minimal-area loop triangulation (leaf) | 001 002 003 004 005 008 | GOM.MSH.05 |
| SCAN.MESHOP.HOLES.FAIR | Curvature-continuous fill by refinement and fairing (leaf) | 004 005 008 | GOM.MSH.05 |
| SCAN.MESHOP.HOLES.CLOSE | Close a mesh into a watertight body (leaf) | 001 005 006 007 008 | GOM.MSH.07 |
