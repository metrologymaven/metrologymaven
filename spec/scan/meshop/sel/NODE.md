---
id: SCAN.MESHOP.SEL
title: Geometric mesh selection, delete and isolate
level: 3
parent: SCAN.MESHOP
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.MESHOP-001, REQ-SCAN.MESHOP-008]
covers: [GOM.MSH.11, GOM.MSH.12, PW.PCL.04]
depends_on: [SCAN.MESHOP.PROV, GEO.SEL, GEO.INDEX, GEO.DIST]
side_effect: W2
determinism: D0
cli: [fid mesh select-patch, fid mesh select-curvature, fid mesh select-on-surface, fid mesh select-visible, fid mesh delete-selection, fid mesh isolate-selection]
mcp: [mesh_select_patch, mesh_select_curvature, mesh_select_on_surface, mesh_select_visible, mesh_delete_selection, mesh_isolate_selection]
journal: [mesh.select_patch@1, mesh.select_curvature@1, mesh.select_on_surface@1, mesh.select_visible@1, mesh.delete_selection@1, mesh.isolate_selection@1]
updated: 2026-11-16
---

# SCAN.MESHOP.SEL: Geometric mesh selection, delete and isolate

## Purpose

The geometry-driven selection tools scan-inspection users rely on to pick parts of a mesh (connected patch, curvature range, on-surface region near a reference, visible-only filtering of a pick), and the two operations that consume a selection: delete it, or isolate it (keep only it). Every selection is resolved on the CPU into a GEO selection before journaling, so no screen state reaches the record.

## Scope

**In:**
- Patch selection: region growing from a seed triangle across edges whose dihedral angle is below a threshold
- Curvature selection: triangles whose estimated mean curvature lies in a stated range
- On-surface selection: triangles whose centroid lies within a distance of a reference mesh or CAD face set
- Visible-only filter: reduce a CPU-resolved through-selection to triangles visible from the stated camera
- Delete selection and isolate selection, as new mesh versions

**Out** (and which node owns it instead):
- Screen-space picking and lasso interaction (owned by `VIZ`)
- Lasso and pick-ray resolution into a through-selection, selection storage and set algebra (owned by `GEO.SEL`)
- Cutting with clean re-triangulated boundaries (owned by `SCAN.MESHOP.CUT`)

## Requirements

- **REQ-SCAN.MESHOP.SEL-001** When a selection command runs, the system shall create a GEO selection (index set over triangles with the mesh digest) on the CPU and shall not modify the mesh.
- **REQ-SCAN.MESHOP.SEL-002** When `mesh.select_patch@1` runs from seed triangle s with angle threshold a in radians, the selection shall be exactly the set of triangles reachable from s across edges whose dihedral angle is at most a.
- **REQ-SCAN.MESHOP.SEL-003** When `mesh.select_curvature@1` runs with range [κ_min, κ_max] in 1/m, the selection shall be exactly the triangles whose mean of vertex mean-curvature estimates lies within the range.
- **REQ-SCAN.MESHOP.SEL-004** When `mesh.select_on_surface@1` runs with a reference element and distance d in metres, the selection shall be exactly the triangles whose centroid is within d of the reference as computed by GEO.DIST.
- **REQ-SCAN.MESHOP.SEL-005** When `mesh.select_visible@1` runs on a through-selection with camera parameters, the system shall keep exactly the selected triangles that are the first hit of a ray from the camera through their centroid.
- **REQ-SCAN.MESHOP.SEL-006** When `mesh.delete_selection@1` or `mesh.isolate_selection@1` runs, the system shall create a new mesh version through the `MeshOp` contract that removes, or keeps only, the selected triangles, copying the rest unchanged.
- **REQ-SCAN.MESHOP.SEL-007** If a selection was made on a mesh version whose digest differs from the target mesh, then delete and isolate shall fail with the GEO.SEL stale-selection error and create no mesh.
- **REQ-SCAN.MESHOP.SEL-008** All commands in this node shall be determinism class D0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/mesh_cube_filleted` (100 mm cube, 5 mm edge fillets, 4·10⁵ triangles), when `fid mesh select-patch --seed <top-face triangle> --angle 0.035 --json` runs, then the selection equals the analytically known top-face triangle set (all triangles with centroid on the top face and none on fillets).
- **AC-2** Given the same fixture, when `fid mesh select-curvature --min 80 --max 120` runs (a 5 mm cylindrical fillet has mean curvature 1/(2·0.005) = 100 1/m), then the selection agrees with the fixture's labelled edge-fillet triangle set on at least 98 % of triangles, and contains no flat-face triangle [unverified threshold].
- **AC-3** Given fixture `fixtures/scan/mesh_two_shells` (outer and inner concentric spheres) and a through-selection of a lasso covering both, when `fid mesh select-visible` runs with the fixture camera, then only outer-shell triangles remain.
- **AC-4** Given any selection from AC-1, when `fid mesh delete-selection` and `fid mesh isolate-selection` run, then their triangle counts sum to the input count and the input digest is unchanged.
- **AC-5** (error path) Given a selection made on a previous mesh version, delete fails with `geo.selection.stale` and journals nothing.
- **AC-6** (replay) Exact replay of AC-1 to AC-4 yields `IDENTICAL`.

## Design

- **Registry commands** (all W2, D0): `mesh.select_patch@1` (`mesh`, `seed_triangle` u64, `max_dihedral` rad); `mesh.select_curvature@1` (`mesh`, `min` 1/m, `max` 1/m); `mesh.select_on_surface@1` (`mesh`, `reference` element, `distance` m, `faces?` selection on the reference); `mesh.select_visible@1` (`mesh`, `selection`, `camera` {eye, direction, up, projection} in metres); `mesh.delete_selection@1`, `mesh.isolate_selection@1` (`mesh`, `selection`). Selection commands return a selection handle; delete and isolate return a new mesh version.
- **Curvature estimate:** discrete mean curvature at each vertex by the cotangent Laplacian (Meyer et al., 2003) [unverified citation detail], averaged over a triangle's three vertices; boundary vertices excluded from the selection.
- **Patch:** breadth-first growth with a queue processed in ascending triangle index at each level.
- **Visible:** ray from camera eye through each selected centroid against the mesh BVH (GEO.INDEX ray query); keep if the first hit (ties: lowest triangle ID) is the triangle itself.
- **Errors:** `geo.selection.stale`, `scan.mesh.invalid_parameter`, `scan.mesh.invalid_index`.
- **Determinism:** D0.
- **Performance budget:** curvature selection on 10⁷ triangles within 20 s [unverified].
- **Crate:** `fid-scan-mesh`.

## Test fixtures

- `fixtures/scan/mesh_cube_filleted` (generated with labelled face, fillet and corner triangle sets) and `fixtures/scan/mesh_two_shells` (generated, with a stored camera), CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-mesh`, implement six W2, D0 registry commands: `mesh.select_patch@1` (BFS growth from a seed triangle across edges with dihedral angle ≤ threshold), `mesh.select_curvature@1` (triangles whose averaged vertex mean curvature, from the cotangent Laplacian, lies in [min, max] 1/m; boundary vertices excluded), `mesh.select_on_surface@1` (triangle centroids within a distance of a reference mesh or CAD face set, via the GEO.DIST API), `mesh.select_visible@1` (keep selected triangles that are the first ray hit from the camera eye through their centroid, via the GEO.INDEX BVH ray query, ties by lowest triangle ID), `mesh.delete_selection@1` and `mesh.isolate_selection@1` (new mesh version via the `MeshOp` builder, copying the kept triangles). Selections are GEO.SEL index sets carrying the mesh digest; a selection whose digest does not match fails with `geo.selection.stale`. CLI `fid mesh select-patch|select-curvature|select-on-surface|select-visible|delete-selection|isolate-selection ... [--json]`; MCP names with underscores.
>
> Constraints: CPU only (CONSTITUTION C2: no GPU picking results enter the journal), KNP-1 (no FMA, no std HashMap/HashSet, fixed order), no panic/unwrap/unsafe, inputs never modified. Tests: filleted cube patch equals labelled top face; curvature selection agrees with labelled fillets; two-shell visible filter keeps only the outer shell; delete + isolate counts sum to input; stale selection fails; exact replay IDENTICAL.

## Open questions

- AC-2 thresholds depend on the fixture's fillet radius and noise; the fixture generator must publish the labelled set and the expected curvature band [unverified].

## Children

_Leaf: none._
