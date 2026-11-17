---
id: SCAN.MESHOP.CUT.INVERT
title: Invert normals of a mesh or selection
level: 4
parent: SCAN.MESHOP.CUT
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP.CUT-001, REQ-SCAN.MESHOP.CUT-006, REQ-SCAN.MESHOP.CUT-007]
covers: [GOM.MSH.09, PW.PCL.04]
depends_on: [SCAN.MESHOP.PROV, GEO.SEL.MODEL]
side_effect: W2
determinism: D0
cli: [fid mesh invert-normals]
mcp: [mesh_invert_normals]
journal: [mesh.invert_normals@1]
updated: 2026-11-17
---

# SCAN.MESHOP.CUT.INVERT: Invert normals of a mesh or selection

## Purpose

Reverses the orientation of a whole mesh or of a selected set of triangles by reversing their winding (and negating stored vertex normals where every incident triangle is inverted). Orientation decides the sign of every deviation, so the operation must be exact and self-inverse.

## Scope

**In:**
- `mesh.invert_normals@1` with optional triangle selection
- Winding reversal (swap of the second and third index) and vertex-normal negation rule

**Out** (and which node owns it instead):
- Automatic orientation repair (owned by `SCAN.MESHOP.REPAIR`)

## Requirements

- **REQ-SCAN.MESHOP.CUT.INVERT-001** The command shall reverse the winding of exactly the targeted triangles by swapping their second and third vertex indexes and shall leave every other triangle unchanged.
- **REQ-SCAN.MESHOP.CUT.INVERT-002** A stored vertex normal shall be negated exactly when every triangle incident to that vertex is targeted.
- **REQ-SCAN.MESHOP.CUT.INVERT-003** Inverting the same target twice shall reproduce the input mesh digest.
- **REQ-SCAN.MESHOP.CUT.INVERT-004** The command shall use `MeshOp` (`copy_triangle`, flags kept), leave the input bit-identical and be D0.

## Acceptance criteria

- **AC-1** Given `fixtures/scan/mesh_closed_cube`, inverting the whole mesh makes every face normal point inward; inverting again reproduces the input digest.
- **AC-2** Given a selection of one face (2 triangles), only those triangles change, and SCAN.POLY.QUAL reports the misoriented pairs along that face's border.
- **AC-3** (error path) Given a stale selection, the command fails `scan.mesh.stale_selection` and journals nothing.

## Design

- **Registry command:** `mesh.invert_normals@1` (W2, D0): `mesh`, `selection?`.
- **Errors:** `scan.mesh.stale_selection`.
- **Determinism:** D0.
- **Crate:** `fid-scan-mesh`.

## Test fixtures

`mesh_closed_cube` (shared), CC0.

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `cut::invert`, register `mesh.invert_normals@1` (W2, D0; CLI `fid mesh invert-normals --mesh <h> [--selection <h>] [--json]`, MCP `mesh_invert_normals`). Swap the second and third index of every targeted triangle (all when no selection), negate stored vertex normals whose incident triangles are all targeted, copy through `MeshOp` keeping flags. Inverting twice must reproduce the input digest. Stale selections fail `scan.mesh.stale_selection`. Constraints: §8, KNP-1. Tests: AC-1..AC-3.

## Open questions

- None.

## Children

_Leaf: none._
