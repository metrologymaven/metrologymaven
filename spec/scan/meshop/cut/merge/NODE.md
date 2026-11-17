---
id: SCAN.MESHOP.CUT.MERGE
title: Merge meshes with boundary welding
level: 4
parent: SCAN.MESHOP.CUT
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP.CUT-001, REQ-SCAN.MESHOP.CUT-004, REQ-SCAN.MESHOP.CUT-005, REQ-SCAN.MESHOP.CUT-007]
covers: [GOM.MSH.10]
depends_on: [SCAN.MESHOP.PROV, GEO.FRAME.APPLY, GEO.INDEX.KDTREE]
side_effect: W2
determinism: D0
cli: [fid mesh merge-meshes]
mcp: [mesh_merge_meshes]
journal: [mesh.merge_meshes@1]
updated: 2026-11-17
---

# SCAN.MESHOP.CUT.MERGE: Merge meshes with boundary welding

## Purpose

Combines two or more meshes in the same frame into one mesh, in the stated order, and optionally welds boundary vertices that coincide within a tolerance so that pieces cut apart (or scanned separately and trimmed) join into one connected surface.

## Scope

**In:**
- `mesh.merge_meshes@1`: ordered list of meshes, weld tolerance w in metres (0 = no welding)
- Welding of boundary vertices only, across and within inputs
- Report: input counts, welded vertex count, removed degenerate triangles

**Out** (and which node owns it instead):
- Merging clouds (owned by `SCAN.CLOUD.MGMT`)
- Repair of resulting non-manifold geometry (owned by `SCAN.MESHOP.REPAIR`)

## Requirements

- **REQ-SCAN.MESHOP.CUT.MERGE-001** The command shall concatenate vertices and triangles of the inputs in the stated order, copying triangles with their synthesized flags.
- **REQ-SCAN.MESHOP.CUT.MERGE-002** When w > 0, boundary vertices closer than w shall be grouped by single-linkage clustering, each group replaced by its lowest-index member, and the result shall report the number of welded vertices.
- **REQ-SCAN.MESHOP.CUT.MERGE-003** Triangles that become degenerate through welding (two equal vertex indexes) shall be removed and counted.
- **REQ-SCAN.MESHOP.CUT.MERGE-004** If inputs are in different frames, then the command shall fail with `geo.frame.mismatch` and create no mesh.
- **REQ-SCAN.MESHOP.CUT.MERGE-005** The command shall create the output through `MeshOp`, leave inputs bit-identical and be D0.

## Acceptance criteria

- **AC-1** Given the two halves from PLANE AC-1, `fid mesh merge-meshes --meshes <a>,<b> --weld "1e-9 m" --json` yields one closed mesh with welded count equal to the number of cut-boundary vertices and zero boundary edges per SCAN.POLY.QUAL.
- **AC-2** Given w = 0, the output has the boundary vertices duplicated and the boundary-loop count is 2.
- **AC-3** (error path) Given meshes in different frames, the command fails `geo.frame.mismatch` and journals nothing.
- **AC-4** Given the inputs in reversed order, vertex order differs but the welded count is the same.

## Design

- **Registry command:** `mesh.merge_meshes@1` (W2, D0): `meshes` [handles] (≥ 2), `weld_tolerance` (authored length, default 0).
- **Algorithm:** `require_same_frame` (GEO.FRAME.APPLY); concatenate; collect boundary vertices; k-d tree radius queries; union-find with lowest-index roots; remap indexes; drop degenerate triangles.
- **Errors:** `geo.frame.mismatch`, `scan.mesh.invalid_parameter`.
- **Determinism:** D0.
- **Crate:** `fid-scan-mesh`.

## Test fixtures

Outputs of PLANE AC-1 regenerated in the test; `mesh_open_cube` (shared), CC0.

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `cut::merge`, register `mesh.merge_meshes@1` (W2, D0; CLI `fid mesh merge-meshes --meshes <h>,<h>... [--weld L] [--json]`, MCP `mesh_merge_meshes`). Check frames (`geo.frame.mismatch`), concatenate in order with `copy_triangle`, then if w > 0 weld boundary vertices within w by k-d tree radius query and union-find (lowest index wins), remove and count degenerate triangles, and report the welded count. Constraints: KNP-1 (no std HashMap/HashSet), §8. Tests: AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
