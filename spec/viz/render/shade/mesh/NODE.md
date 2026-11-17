---
id: VIZ.RENDER.SHADE.MESH
title: Mesh shading, display modes and triangle IDs
level: 4
parent: VIZ.RENDER.SHADE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.SHADE-001, REQ-VIZ.RENDER.SHADE-006]
covers: []
depends_on: [VIZ.RENDER.SCENE.PROJ, VIZ.RENDER.FRAME.GRAPH, GEO.TYPES.MESH]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.SHADE.MESH: Mesh shading, display modes and triangle IDs

## Purpose

Draw triangle meshes (scans, STL nominals, tessellations) with lit shading, feature edges and silhouettes, in shaded, shaded-with-edges, wireframe and hidden-line modes, writing render ID and primitive index for every triangle, and publishing displayed and total triangle counts.

## Scope

**In:** the opaque surface pass; four display modes; feature-edge extraction by dihedral angle (CPU, cached per mesh digest); silhouette pass; per-vertex colours from kernel-supplied bins; triangle counts.
**Out:** CAD faces, B-rep edges and assemblies (`VIZ.RENDER.SHADE.CAD`); transparency (`VIZ.RENDER.SHADE.OIT`); clipping (`VIZ.RENDER.SHADE.CLIPR`); mesh data (`GEO.TYPES.MESH`).

## Requirements

- **REQ-VIZ.RENDER.SHADE.MESH-001** The renderer shall draw meshes in shaded, shaded-with-edges, wireframe and hidden-line modes, selectable per scene node.
- **REQ-VIZ.RENDER.SHADE.MESH-002** Every drawn triangle shall write its render ID and primitive index to the ID buffer in every display mode.
- **REQ-VIZ.RENDER.SHADE.MESH-003** Feature edges shall be extracted on the CPU at a dihedral threshold (default 30° [unverified]) and cached by mesh digest.
- **REQ-VIZ.RENDER.SHADE.MESH-004** The renderer shall publish displayed and total triangle counts for every presented frame.
- **REQ-VIZ.RENDER.SHADE.MESH-005** While displaying a mesh of 10^7 triangles on the reference mid-range profile, the renderer shall sustain at least 30 fps [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube` and `fixtures/viz/mesh-bunny-like` (a 10^5-triangle synthetic mesh, seed 41), when rendered in each mode on the software adapter, then each image matches its golden within the tolerance mask.
- **AC-2** Given the ID buffer of the shaded cube, then pixels on each face hold one of that face's two primitive indices.
- **AC-3** Given the cube, then 12 feature edges are extracted at 30°.
- **AC-4** Given a 10^7-triangle synthetic mesh on reference hardware, then frame time is reported against REQ-005 [unverified until measured].

## Design

- **Registry command(s):** none; the display mode is a style on the node.
- **Data structures:** `MeshDisplayMode`; `EdgeCache` keyed by mesh digest (BTreeMap); `ShadeStats { tri_displayed, tri_total }`.
- **Algorithm:** indexed draw with vertex positions as f32 offsets from the chunk origin (`VIZ.RENDER.PREC`); `primitive_index` builtin written to the ID target; hidden-line as depth prepass plus line pass; silhouettes from depth and normal discontinuity post-pass.
- **Errors:** `MeshTooLarge` when exceeding buffer limits (split into chunks first).
- **Determinism:** N (GPU).
- **Performance budget:** REQ-005.
- **Crate:** `fid-render` (module `shade::mesh`).

## Test fixtures

`fixtures/viz/golden-cube`; `fixtures/viz/mesh-bunny-like` generated from seed 41 (no third-party scan data), CC0.

## Build prompt

> In crate `fid-render`, add module `shade::mesh`: an opaque surface pass with shaded, shaded-with-edges, wireframe and hidden-line modes; every triangle writes render ID and primitive index to the `Rg32Uint` ID target; feature edges extracted on the CPU at a dihedral threshold (default 30°) cached by mesh digest; silhouette post-pass; displayed/total triangle counts per frame. Positions are f32 offsets from f64 chunk origins. Tests AC-1..AC-4 with `fixtures/viz/golden-cube` and generated `mesh-bunny-like` (seed 41). Rules: CONSTITUTION §8; C2.

## Open questions

- None.

## Children

_Leaf: none._
