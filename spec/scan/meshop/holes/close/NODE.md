---
id: SCAN.MESHOP.HOLES.CLOSE
title: Close a mesh into a watertight body
level: 4
parent: SCAN.MESHOP.HOLES
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP.HOLES-001, REQ-SCAN.MESHOP.HOLES-005, REQ-SCAN.MESHOP.HOLES-006, REQ-SCAN.MESHOP.HOLES-007, REQ-SCAN.MESHOP.HOLES-008]
covers: [GOM.MSH.07]
depends_on: [SCAN.MESHOP.HOLES.FLAT, SCAN.MESHOP.HOLES.FAIR, SCAN.MESHOP.REPAIR.DETECT, SCAN.MESHOP.REPAIR.FIXTOPO, SCAN.MESHOP.PROV, SCAN.POLY.QUAL]
side_effect: W2
determinism: D0
cli: [fid mesh close]
mcp: [mesh_close]
journal: [mesh.close@1]
updated: 2026-11-17
---

# SCAN.MESHOP.HOLES.CLOSE: Close a mesh into a watertight body

## Purpose

Goes beyond filling: fix topology, fill every loop, check for defects the fill introduced, and either deliver a mesh with zero boundary and zero non-manifold edges or fail and say exactly what stood in the way and where.

## Scope

**In:**
- `mesh.close@1`: pipeline FIXTOPO → fill all loops (shape as stated) → `detect_defects` → FIXTOPO if needed → final QUAL check
- Typed obstruction errors with location
- Report: holes filled (FLAT report rows), topology fixes, final QUAL report

**Out** (and which node owns it instead):
- Loop triangulation and fairing (owned by `SCAN.MESHOP.HOLES.FLAT`, `SCAN.MESHOP.HOLES.FAIR`)
- Self-intersection repair (owned by `SCAN.MESHOP.REPAIR`; close does not remove measured triangles)

## Requirements

- **REQ-SCAN.MESHOP.HOLES.CLOSE-001** When `mesh.close@1` succeeds, the output shall have zero boundary edges and zero non-manifold edges as measured by `SCAN.POLY.QUAL`, and shall be created through `MeshOp` with the input bit-identical.
- **REQ-SCAN.MESHOP.HOLES.CLOSE-002** Every triangle created by closing shall be flagged synthesized.
- **REQ-SCAN.MESHOP.HOLES.CLOSE-003** If a fill patch intersects existing triangles, then the command shall fail with `scan.mesh.close_obstructed { kind: self_intersecting_fill, loop_id, location }` and create no mesh.
- **REQ-SCAN.MESHOP.HOLES.CLOSE-004** If a non-manifold vertex or edge remains after FIXTOPO, or a component is non-orientable, then the command shall fail with `scan.mesh.close_obstructed` naming the kind and location and create no mesh.
- **REQ-SCAN.MESHOP.HOLES.CLOSE-005** The command shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/mesh/box_holes` (a closed box mesh with three holes), when `fid mesh close --mesh <m> --json` runs, then QUAL reports zero boundary and zero non-manifold edges and all new triangles are synthesized.
- **AC-2** Given fixture `close_crossing_fill` (a hole whose flat fill would cut through an adjacent flap), the command fails `scan.mesh.close_obstructed` with kind `self_intersecting_fill` and the loop ID, and journals nothing.
- **AC-3** Given the Möbius fixture, the command fails with kind `non_orientable`.
- **AC-4** (replay) Replay of AC-1 in exact mode is `IDENTICAL`.

## Design

- **Registry command:** `mesh.close@1` (W2, D0): `mesh`, `shape` (`flat` default | `curvature`), `max_edge_length?`.
- **Algorithm:** `fix_topology`; fill each loop in ascending loop ID with `triangulate_loop` (+ `fair_patch`); `detect_defects` restricted to patches and their neighbours; obstruction → error; final `mesh_quality`.
- **Errors:** `scan.mesh.close_obstructed { kind: self_intersecting_fill | non_manifold_vertex | non_manifold_edge | non_orientable, location }`.
- **Determinism:** D0.
- **Crate:** `fid-scan-mesh`.

## Test fixtures

`box_holes`, `close_crossing_fill` (generated), `mobius` (shared with REPAIR), CC0.

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `holes::close`, register `mesh.close@1` (W2, D0; CLI `fid mesh close --mesh <h> [--shape flat|curvature] [--max-edge-length L] [--json]`, MCP `mesh_close`). Run `repair::topo::fix_topology`, fill every loop in ascending loop ID (`holes::flat::triangulate_loop`, plus `holes::fair::fair_patch` for curvature) via `create_triangle`, check the patches with `defects::detect_defects`, and verify with `mesh_quality` that boundary and non-manifold edge counts are zero. Any obstruction fails `scan.mesh.close_obstructed` with kind and location and creates nothing. Constraints: KNP-1, §3, §8. Tests: AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
