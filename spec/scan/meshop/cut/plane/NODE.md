---
id: SCAN.MESHOP.CUT.PLANE
title: Plane cut, half-space clip and box clip
level: 4
parent: SCAN.MESHOP.CUT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP.CUT-001, REQ-SCAN.MESHOP.CUT-002, REQ-SCAN.MESHOP.CUT-003, REQ-SCAN.MESHOP.CUT-007]
covers: [GOM.MSH.08]
depends_on: [SCAN.MESHOP.PROV, GEO.FRAME.XFORM, GEO.TYPES.MESH, KRN.NUM]
side_effect: [W2, W2, W2]
determinism: D0
cli: [fid mesh cut-plane, fid mesh clip-halfspace, fid mesh clip-box]
mcp: [mesh_cut_plane, mesh_clip_halfspace, mesh_clip_box]
journal: [mesh.cut_plane@1, mesh.clip_halfspace@1, mesh.clip_box@1]
updated: 2026-11-17
---

# SCAN.MESHOP.CUT.PLANE: Plane cut, half-space clip and box clip

## Purpose

Splits a mesh exactly along a plane, keeping one side, the other, or both as separate meshes, and clips a mesh to a half-space or to an axis-aligned or oriented box (six half-spaces). It also owns the shared triangle splitter (`split_by_plane`) that `SCAN.MESHOP.CUT.CURVE` reuses, so split triangles keep their synthesized flag in one place.

## Scope

**In:**
- `mesh.cut_plane@1` (side `positive` | `negative` | `both`), `mesh.clip_halfspace@1`, `mesh.clip_box@1` (axis-aligned or oriented)
- `split_by_plane`: exact-sign vertex classification and triangle splitting with shared new vertices on cut edges
- Boundary vertices placed on the plane; new triangles through `derive_triangle`

**Out** (and which node owns it instead):
- Curve and selection cuts (owned by `SCAN.MESHOP.CUT.CURVE`)
- Capping the cut with new faces (owned by `SCAN.MESHOP.HOLES`)

## Requirements

- **REQ-SCAN.MESHOP.CUT.PLANE-001** Each vertex shall be classified against the plane by an exact-sign orientation predicate (KRN.NUM, no FMA) as positive, negative or on-plane, and on-plane vertices shall be shared by both sides without splitting their triangles.
- **REQ-SCAN.MESHOP.CUT.PLANE-002** Every mesh edge crossing the plane shall get exactly one new vertex, shared by all triangles using that edge, at the edge–plane intersection, lying on the plane within 1e-12 m.
- **REQ-SCAN.MESHOP.CUT.PLANE-003** The union of the kept parts of a `both` cut shall equal the input surface area within 1e-9 relative, and each split triangle shall be produced with `derive_triangle` from its source.
- **REQ-SCAN.MESHOP.CUT.PLANE-004** `mesh.clip_box@1` shall equal successive half-space clips by the six box faces in a fixed order (−x, +x, −y, +y, −z, +z of the box frame), and an oriented box shall be given as a rigid transform plus half extents in metres.
- **REQ-SCAN.MESHOP.CUT.PLANE-005** Each command shall create new mesh versions through `MeshOp`, leave the input bit-identical, return an empty-result error `scan.mesh.empty_result` instead of an empty mesh, and be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/mesh/sphere_ico5` (r = 50 mm) and the plane z = 7 mm, when `fid mesh cut-plane --mesh <m> --point 0,0,0.007 --normal 0,0,1 --side both --json` runs, then two meshes result, every new boundary vertex has |z − 0.007| ≤ 1e-12 m and their areas sum to the input area within 1e-9 relative.
- **AC-2** Given a plane through existing vertices of `fixtures/scan/mesh/grid_plate`, no triangle is split and on-plane vertices appear in both outputs.
- **AC-3** Given a synthesized triangle crossed by the plane, all its pieces are flagged synthesized.
- **AC-4** Given an oriented box, the result equals six successive half-space clips bit-for-bit.
- **AC-5** (error path) Given a plane missing the mesh with side `positive` empty, the command fails `scan.mesh.empty_result` and journals nothing.
- **AC-6** (replay) Replay of AC-1 in exact mode is `IDENTICAL`.

## Design

- **Registry commands** (W2, D0): `mesh.cut_plane@1` (`mesh`, `point` m, `normal`, `side`), `mesh.clip_halfspace@1` (`mesh`, `point`, `normal`; keeps the side opposite the normal), `mesh.clip_box@1` (`mesh`, `transform?`, `min`/`max` or `half_extents` m).
- **Algorithm:** classify vertices with `orient3d` of three plane points derived from (point, normal) or, for an analytic plane, the exact sign of n·(x − p) evaluated with an error-free expansion (KRN.NUM); new vertex on each crossing edge keyed by the sorted vertex pair in a sorted vector (no hash map); intersection parameter computed from the edge endpoint farther from the plane, then snapped so the residual is ≤ 1e-12 m; split cases 1-2 and 2-1 into one and two triangles with the shorter diagonal (ties lowest vertex index).
- **Errors:** `scan.mesh.empty_result`, `scan.mesh.invalid_parameter` (zero normal, inverted box).
- **Determinism:** D0.
- **Performance budget:** 10⁷-triangle mesh in under 5 s [unverified].
- **Crate:** `fid-scan-mesh` (kernel tier, KNP-1).

## Test fixtures

`sphere_ico5`, `grid_plate` (generated, CC0); `mesh_open_cube` (shared).

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `cut::plane`, implement `split_by_plane(mesh, plane) -> SplitResult` and register `mesh.cut_plane@1`, `mesh.clip_halfspace@1`, `mesh.clip_box@1` (all W2, D0; CLI `fid mesh cut-plane|clip-halfspace|clip-box ... [--json]`, MCP `mesh_cut_plane`, `mesh_clip_halfspace`, `mesh_clip_box`). Classify vertices with exact-sign predicates from `fid-num` (no FMA); create one shared vertex per crossing edge on the plane (≤ 1e-12 m); split triangles with `MeshOpBuilder::derive_triangle` and copy others with `copy_triangle`; keep on-plane vertices in both parts; box clip = six half-space clips in the fixed order. Empty results fail `scan.mesh.empty_result`. Outputs via `MeshOp`; inputs untouched.
>
> Constraints: KNP-1 (no FMA, no std HashMap/HashSet, total-order ties), CONSTITUTION §3, §8. Tests: AC-1..AC-6.

## Open questions

- Exact-sign predicates without FMA are assumed to come from `KRN.NUM` (`fid_num::pred`), but `KRN.NUM` has no predicates child yet; today the only implementation is inside `KRN.MZ.VOR`. A KRN.NUM owner should add one (shared with `SCAN.MESHOP.REPAIR` and `SCAN.POLY.SURF`).

## Children

_Leaf: none._
