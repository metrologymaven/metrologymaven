---
id: SCAN.MESHOP.HOLES.FAIR
title: Curvature-continuous fill by refinement and fairing
level: 4
parent: SCAN.MESHOP.HOLES
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP.HOLES-004, REQ-SCAN.MESHOP.HOLES-005, REQ-SCAN.MESHOP.HOLES-008]
covers: [GOM.MSH.05]
depends_on: [SCAN.MESHOP.PROV, KRN.NUM.LA.QR, KRN.NUM.RED]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.MESHOP.HOLES.FAIR: Curvature-continuous fill by refinement and fairing

## Purpose

Turns a flat hole patch into one that continues the surrounding surface: refine the patch so its edge lengths match the neighbourhood, then move only the new interior vertices to minimise a discrete bi-Laplacian energy with the boundary ring and one ring outside held fixed. It is the `curvature` shape of the fill commands.

## Scope

**In:**
- `fair_patch(mesh, patch, opts)`: refinement by edge split and flip to the mean boundary edge length (or `max_edge_length`), then bi-Laplacian (uniform weights) fairing of interior patch vertices
- Linear solve with a deterministic solver: dense Householder QR for up to 3 000 unknown vertices, fixed-count Jacobi-preconditioned conjugate gradient (fixed iteration count, fixed-order reductions) above that

**Out** (and which node owns it instead):
- Loop triangulation and the commands (owned by `SCAN.MESHOP.HOLES.FLAT`)

## Requirements

- **REQ-SCAN.MESHOP.HOLES.FAIR-001** Refinement shall split patch edges longer than 4/3 of the target length at their midpoints, in descending length order (ties lowest vertex indexes), until none remains, and every created triangle shall be synthesized.
- **REQ-SCAN.MESHOP.HOLES.FAIR-002** Fairing shall move only interior patch vertices, solving Δ²x = 0 with the boundary loop and its outer ring fixed, by QR for at most 3 000 unknowns and otherwise by a fixed 500-iteration preconditioned conjugate gradient [unverified iteration count].
- **REQ-SCAN.MESHOP.HOLES.FAIR-003** On fixture `sphere_hole` (a 20 mm hole in a 50 mm-radius sphere mesh), every faired vertex shall lie within 50 µm of the sphere [unverified].
- **REQ-SCAN.MESHOP.HOLES.FAIR-004** The function shall report the patch area after fairing, shall be pure and determinism class D0.

## Acceptance criteria

- **AC-1** Given `fixtures/scan/mesh/sphere_hole`, `fill-hole --shape curvature` meets REQ-003 and reports the faired area.
- **AC-2** Given a planar hole, the faired patch is planar within 1e-12 m.
- **AC-3** Given 1 and 16 threads, the output digest is identical.

## Design

- **API** (`fid-scan-mesh::holes::fair`): `fn fair_patch(mesh: &MeshView, patch: &PatchRef, opts: &FairOptions, out: &mut MeshOpBuilder) -> Result<FairStats, MeshError>`.
- **Algorithm:** split-and-flip refinement (Liepa 2003 style) [unverified citation detail]; uniform-weight Laplacian matrix assembled in vertex order; QR via `fid-num` LA; CG with `fid-num` fixed-order dot products.
- **Errors:** `scan.mesh.fair_failed` (non-finite solution).
- **Determinism:** D0.
- **Crate:** `fid-scan-mesh`.

## Test fixtures

`sphere_hole`, `plane_hole` (generated, CC0).

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `holes::fair`, implement `fair_patch(mesh, patch, opts, out)`: refine the patch by splitting edges longer than 4/3 of the target length (descending length, ties by lowest indexes) with edge flips to restore Delaunay-like angles, all new triangles via `create_triangle`; then solve the uniform bi-Laplacian system for interior patch vertices with the boundary and one outer ring fixed (dense QR from `fid-num` up to 3 000 unknowns, else 500-iteration Jacobi-preconditioned CG with fixed-order reductions). Report faired area. Constraints: KNP-1 (no FMA, no std HashMap/HashSet), §8, D0. Tests: AC-1..AC-3.

## Open questions

- Cotangent weights would follow curvature better on irregular meshes; uniform weights are chosen for robustness and can be revisited with measured results.

## Children

_Leaf: none._
