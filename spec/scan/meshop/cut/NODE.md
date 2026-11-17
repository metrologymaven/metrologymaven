---
id: SCAN.MESHOP.CUT
title: Cut, clip, merge and invert
level: 3
parent: SCAN.MESHOP
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP-001, REQ-SCAN.MESHOP-005]
covers: [GOM.MSH.08, GOM.MSH.09, GOM.MSH.10, PW.PCL.04]
depends_on: [SCAN.MESHOP.PROV, GEO.SEL, GEO.FRAME]
cli: [fid mesh cut-*, fid mesh clip-*, fid mesh merge-meshes, fid mesh invert-normals]
mcp: [mesh_cut_*, mesh_clip_*, mesh_merge_meshes, mesh_invert_normals]
journal: [mesh.cut_*, mesh.clip_*, mesh.merge_meshes@1, mesh.invert_normals@1]
updated: 2026-11-17
---

# SCAN.MESHOP.CUT: Cut, clip, merge and invert

## Purpose

The topological editing operations that change which part of a mesh exists or how it is oriented: cutting a mesh along a plane or closed curve, clipping it to a half-space or box, cutting out a selection, merging several meshes into one, and inverting normals. They share the need to split triangles exactly along a cut and to keep the synthesized flag correct on split triangles.

## Scope

**In:**
- Cut with a plane (keep one side, the other, or both as separate meshes)
- Clip to a half-space, to an axis-aligned or oriented box
- Cut along a closed curve projected onto the mesh along a stated direction
- Cut out a GEO selection (keep or remove the selected triangles, with clean boundaries)
- Merge two or more meshes in the same frame into one, optionally welding coincident boundary vertices within a tolerance in metres
- Invert normals of a whole mesh or of a selection

**Out** (and which node owns it instead):
- Delete and isolate of a selection without re-triangulating the cut boundary (owned by `SCAN.MESHOP.SEL`)
- Hole filling after a cut (owned by `SCAN.MESHOP.HOLES`)
- Section curves for comparison (owned by `SCAN.SECT`)
- Selection storage (owned by `GEO.SEL`)

## Requirements

- **REQ-SCAN.MESHOP.CUT-001** Every cut, clip, merge and invert command shall create new mesh versions through the `MeshOp` contract and shall leave all inputs bit-identical.
- **REQ-SCAN.MESHOP.CUT-002** When a mesh is cut by a plane, every new boundary vertex shall lie on the plane within 1e-12 m and the union of the kept parts shall equal the input surface area within 1e-9 relative.
- **REQ-SCAN.MESHOP.CUT-003** Triangles split by a cut shall be created with `derive_triangle` so that they keep the synthesized flag of their source triangle.
- **REQ-SCAN.MESHOP.CUT-004** When meshes are merged with a weld tolerance w in metres, boundary vertices closer than w shall be welded into one vertex, and the result shall report the number of welded vertices.
- **REQ-SCAN.MESHOP.CUT-005** If merge inputs are in different frames, then the command shall fail with the GEO.FRAME frame-mismatch error and create no mesh.
- **REQ-SCAN.MESHOP.CUT-006** Inverting normals shall reverse the winding of exactly the targeted triangles, and inverting twice shall reproduce the input digest.
- **REQ-SCAN.MESHOP.CUT-007** All commands in this node shall be determinism class D0.

## Interfaces

- **Offers:** `mesh.cut_plane@1`, `mesh.clip_halfspace@1`, `mesh.clip_box@1`, `mesh.cut_curve@1`, `mesh.cut_selection@1`, `mesh.merge_meshes@1`, `mesh.invert_normals@1` (all W2).
- **Consumes:** SCAN.MESHOP.PROV (`MeshOp`), GEO.SEL (selections), GEO.FRAME (frame checks).

## Decomposition hints

PLANE (plane cut and half-space and box clip), CURVE (closed-curve and selection cut), MERGE (merge and weld), INVERT (normal inversion; likely a small leaf).

## Open questions

- Robust exact predicates for triangle splitting near the cut plane must stay within KNP-1 (no FMA); confirm with KRN.NUM whether adaptive-precision predicates are available.
- **At split (2026-11-17):** children assume exact-sign predicates without FMA from `KRN.NUM` (`fid_num::pred`). `KRN.NUM` has no predicates child yet (the only implementation is inside `KRN.MZ.VOR`); this cross-domain gap is shared with `SCAN.MESHOP.REPAIR` and `SCAN.POLY.SURF`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| SCAN.MESHOP.CUT.PLANE | Plane cut, half-space clip and box clip (leaf) | 001 002 003 007 | GOM.MSH.08 |
| SCAN.MESHOP.CUT.CURVE | Closed-curve cut and selection cut-out (leaf) | 001 003 007 | GOM.MSH.08, PW.PCL.04 |
| SCAN.MESHOP.CUT.MERGE | Merge meshes with boundary welding (leaf) | 001 004 005 007 | GOM.MSH.10 |
| SCAN.MESHOP.CUT.INVERT | Invert normals of a mesh or selection (leaf) | 001 006 007 | GOM.MSH.09, PW.PCL.04 |
