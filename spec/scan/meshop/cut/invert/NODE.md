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
built_in: [fid-scan-mesh, fid-scan-mesh-cmd]
built_pr: 582
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
- **REQ-SCAN.MESHOP.CUT.INVERT-003** Inverting the same target twice shall reproduce the input's GEO.TYPES element digest, and also its version digest when the input carries the `synthesized` attribute.
- **REQ-SCAN.MESHOP.CUT.INVERT-004** The command shall use `MeshOp` (`copy_triangle`, flags kept), leave the input bit-identical and be D0.

## Acceptance criteria

- **AC-1** Given `fixtures/scan/mesh_closed_cube`, inverting the whole mesh makes every face normal point inward; inverting again reproduces the input's GEO.TYPES element digest (reported as `element_digest` in the command result), and also its version digest when the input carries the `synthesized` attribute.
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

- **Maintainer decision (2026-11-24, #582 review):** REQ-003 and AC-1 read "the input's GEO.TYPES element digest, and its version digest when the input carries the `synthesized` attribute". A raw input comes back with an all-false attribute, and `SCAN.MESHOP.PROV` gives every output a derived-mesh digest no raw mesh can have, so the raw input's version digest cannot recur.
- **`MeshStore` is an in-memory stand-in, not persisted.** The project document has no store for derived mesh versions (mesh plus flags) yet; `fid-scan-mesh-cmd` keeps them in a `MeshStore` value until it does. `MeshStore::insert_raw` is the trusted import path for measured meshes only.
- **`derive_triangle` in place of `copy_triangle` (REQ-004).** `copy_triangle` copies the index order and cannot reverse a winding, so a reversed triangle is written with `derive_triangle` with that one triangle as its only source (its flag is the source's flag, so flags are kept). Untargeted triangles use `copy_triangle`.
- **Crate-private attribute setters.** The `SCAN.MESHOP.PROV` builder gained `pub(crate)` `set_vertex_attrs` and `set_triangle_attrs` so this operation keeps vertex normals (REQ-002) and the other channels; the public `MeshOp` contract is unchanged and neither setter can set a `synthesized` flag. Other `fid-scan-mesh` operations may use them; a public channel API would be a PROV change.
- **A selection is bound to one mesh version.** A selection made on a version and reused on the new version an inversion creates fails with `scan.mesh.selection_target` (it targets the old entity ID); one whose target ID matches but whose stored target digest is not the mesh's element digest fails with `scan.mesh.stale_selection`.
- Built 2026-11-24: the kernel operation is in `fid-scan-mesh` (`cut::invert`); the registry definition, CLI and MCP are in the engine-layer adapter `fid-scan-mesh-cmd`, because a kernel crate cannot depend on the registry. AC-2's misoriented pairs are read from the GEO.TYPES topology facts of the output mesh: the `SCAN.POLY.QUAL` report has no orientation field.

## Children

_Leaf: none._
