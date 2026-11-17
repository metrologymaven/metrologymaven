---
id: SCAN.MESHOP.HOLES.FLAT
title: Hole fill commands and minimal-area loop triangulation
level: 4
parent: SCAN.MESHOP.HOLES
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP.HOLES-001, REQ-SCAN.MESHOP.HOLES-002, REQ-SCAN.MESHOP.HOLES-003, REQ-SCAN.MESHOP.HOLES-004, REQ-SCAN.MESHOP.HOLES-005, REQ-SCAN.MESHOP.HOLES-008]
covers: [GOM.MSH.05]
depends_on: [SCAN.MESHOP.PROV, SCAN.POLY.QUAL]
side_effect: [W2, W2]
determinism: D0
cli: [fid mesh fill-holes, fid mesh fill-hole]
mcp: [mesh_fill_holes, mesh_fill_hole]
journal: [mesh.fill_holes@1, mesh.fill_hole@1]
updated: 2026-11-17
---

# SCAN.MESHOP.HOLES.FLAT: Hole fill commands and minimal-area loop triangulation

## Purpose

The two fill commands (automatic by maximum perimeter, and one chosen loop) and the core loop triangulation they share: a minimal-area triangulation of the boundary polygon with a dihedral-angle tie-break, created entirely through `create_triangle` so every fill triangle is synthesized. Shape `curvature` hands the flat patch to `SCAN.MESHOP.HOLES.FAIR` for refinement and fairing.

## Scope

**In:**
- `mesh.fill_holes@1` (every loop with perimeter < P) and `mesh.fill_hole@1` (one loop by ID or by a selected boundary edge)
- Stable loop IDs: the lowest vertex index on the loop, as walked by SCAN.POLY.QUAL
- `triangulate_loop`: dynamic-programming minimal-area triangulation (weight = area, tie-break by worst dihedral to the neighbours), for loops up to 2 000 edges; larger loops are split by the shortest chord first
- `HoleFillReport` per hole: loop ID, perimeter (m), filled area (m²), triangles created, shape

**Out** (and which node owns it instead):
- Curvature-continuous fairing (owned by `SCAN.MESHOP.HOLES.FAIR`)
- Watertight closing (owned by `SCAN.MESHOP.HOLES.CLOSE`)
- Loop detection metrics (owned by `SCAN.POLY.QUAL`)

## Requirements

- **REQ-SCAN.MESHOP.HOLES.FLAT-001** When `mesh.fill_holes@1` runs with maximum perimeter P in metres, the system shall fill every boundary loop whose perimeter is strictly below P and no other loop.
- **REQ-SCAN.MESHOP.HOLES.FLAT-002** Loop IDs shall be the lowest vertex index on the loop; `mesh.fill_hole@1` shall fill exactly the loop with the given ID (or containing the given boundary edge), or fail with `scan.mesh.unknown_loop`.
- **REQ-SCAN.MESHOP.HOLES.FLAT-003** `triangulate_loop` shall return the triangulation of the loop polygon with minimal total area among those found by the dynamic programme, ties by smaller worst dihedral angle then lowest vertex indexes, using only the loop's vertices.
- **REQ-SCAN.MESHOP.HOLES.FLAT-004** Every triangle created by filling shall be created with `create_triangle` (synthesized) and oriented consistently with the surrounding triangles.
- **REQ-SCAN.MESHOP.HOLES.FLAT-005** The result shall report, per filled hole, loop ID, perimeter in metres, filled area in square metres, triangles created and shape.
- **REQ-SCAN.MESHOP.HOLES.FLAT-006** Both commands shall create a new mesh version through `MeshOp`, leave the input bit-identical, and be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/mesh/plate_holes` (holes with perimeters 10, 30 and 80 mm), when `fid mesh fill-holes --mesh <m> --max-perimeter "50 mm" --json` runs, then exactly the first two are filled, each reported area equals the analytic planar area within 1e-9 relative, and all created triangles are synthesized.
- **AC-2** Given `fid mesh fill-hole --loop <id of 80 mm hole>`, only that hole is filled.
- **AC-3** (error path) Given a loop ID that is not a loop's lowest vertex, the command fails `scan.mesh.unknown_loop` and journals nothing.
- **AC-4** Given `fixtures/scan/mesh_open_cube`, filling the missing face gives 2 triangles, winding consistent, and SCAN.POLY.QUAL reports zero boundary loops.
- **AC-5** (replay) Replay of AC-1 in exact mode is `IDENTICAL`.

## Design

- **Registry commands** (W2, D0): `mesh.fill_holes@1` (`mesh`, `max_perimeter` length, `shape` `flat` | `curvature`, `max_edge_length?`); `mesh.fill_hole@1` (`mesh`, `loop_id` or `boundary_edge` selection, `shape`, `max_edge_length?`).
- **Algorithm:** loops from the QUAL boundary walk; O(n³) DP (Barequet–Sharir style, as used by Liepa 2003) [unverified citation detail]; loops over 2 000 edges split recursively by the shortest chord between vertices at least n/4 apart; orientation from the loop's direction relative to existing triangles; for `curvature`, call `holes::fair::fair_patch`.
- **Errors:** `scan.mesh.unknown_loop`, `scan.mesh.invalid_parameter`.
- **Determinism:** D0.
- **Crate:** `fid-scan-mesh`.

## Test fixtures

`plate_holes` (generated), `mesh_open_cube` (shared), CC0.

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `holes::flat`, implement `triangulate_loop(loop_vertices, mesh) -> Vec<[u64;3]>` (minimal-area DP, ties by worst dihedral then lowest indexes; chord split above 2 000 edges) and register `mesh.fill_holes@1` and `mesh.fill_hole@1` (W2, D0; CLI `fid mesh fill-holes --mesh <h> --max-perimeter L [--shape flat|curvature]`, `fid mesh fill-hole --mesh <h> --loop ID|--edge <sel> [--shape ...]`; MCP `mesh_fill_holes`, `mesh_fill_hole`). Loop IDs are the lowest vertex index on each loop (QUAL walk). Fill loops with perimeter < P (or the one chosen; unknown fails `scan.mesh.unknown_loop`) through `MeshOpBuilder::create_triangle`, oriented to match neighbours; shape `curvature` passes the patch to `holes::fair::fair_patch`. Report loop ID, perimeter, area, triangle count and shape per hole. Constraints: KNP-1, §8. Tests: AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
