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
updated: 2026-11-22
---

# VIZ.RENDER.SHADE.MESH: Mesh shading, display modes and triangle IDs

## Purpose

Draw triangle meshes (scans, STL nominals, tessellations) with lit shading, feature edges and silhouettes, in shaded, shaded-with-edges, wireframe and hidden-line modes, writing render ID and primitive index for every triangle, and publishing displayed and total triangle counts.

## Scope

**In:** the opaque surface pass; four display modes; feature-edge extraction by dihedral angle (CPU, cached per mesh digest); silhouette pass; per-vertex colours from kernel-supplied bins; triangle counts.
**Out:** CAD faces, B-rep edges and assemblies (`VIZ.RENDER.SHADE.CAD`); transparency (`VIZ.RENDER.SHADE.OIT`); clipping (`VIZ.RENDER.SHADE.CLIPR`); mesh data (`GEO.TYPES.MESH`).

## Requirements

- **REQ-VIZ.RENDER.SHADE.MESH-001** The renderer shall draw meshes in shaded, shaded-with-edges, wireframe, hidden-line, studio MatCap, zebra/isophote reflection lines and screen-space curvature display modes, selectable per scene node.
- **REQ-VIZ.RENDER.SHADE.MESH-002** Every drawn triangle shall write its render ID and primitive index to the ID / Visibility Buffer in every display mode.
- **REQ-VIZ.RENDER.SHADE.MESH-003** Feature edges shall be extracted on the CPU at a dihedral threshold (default 30° [unverified]), cached by mesh digest, and rendered with analytical screen-space anti-aliasing.
- **REQ-VIZ.RENDER.SHADE.MESH-004** The renderer shall publish displayed and total triangle counts for every presented frame.
- **REQ-VIZ.RENDER.SHADE.MESH-005** While displaying a mesh of 10^7 triangles on the reference mid-range profile, the renderer shall sustain at least 30 fps through GPU-driven meshlet cluster LOD culling and indirect drawing on T0/T1 [unverified].
- **REQ-VIZ.RENDER.SHADE.MESH-006** When display-only deviation exaggeration (`1×` to `500×`) is enabled for a deviation-mapped mesh, the shader shall displace vertices along kernel-supplied normals for visual inspection only, emit an active-exaggeration HUD flag, and never alter CPU f64 pick resolution.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube` and `fixtures/viz/mesh-bunny-like` (a 10^5-triangle synthetic mesh, seed 41), when rendered in each mode on the software adapter, then each image matches its golden within the tolerance mask.
- **AC-2** Given the ID buffer of the shaded cube, then pixels on each face hold one of that face's two primitive indices.
- **AC-3** Given the cube, then 12 feature edges are extracted at 30°.
- **AC-4** Given a 10^7-triangle synthetic mesh on reference hardware, then meshlet culling stats and frame time are reported against REQ-005 [unverified until measured].
- **AC-5** Given deviation exaggeration set to `100×`, then the HUD state reports `exaggerated = 100` and picking resolves to the undisplaced f64 surface.

## Design

- **Registry command(s):** none; display mode and exaggeration factor are display styles on the node.
- **Data structures:** `MeshDisplayMode { Shaded, ShadedEdges, Wireframe, HiddenLine, MatCap, Zebra, Curvature }`; `MeshletChunk` (64–128 tri clusters with bounding sphere, normal cone, SSE); `EdgeCache` keyed by mesh digest (BTreeMap); `ShadeStats { tri_displayed, tri_total, meshlets_culled }`.
- **Algorithm:** partition large meshes into 64–128 tri meshlets; on T0/T1 run GPU compute frustum + backface-cone + Hi-Z occlusion culling into `DrawIndexedIndirectArgs` (plus `compute64` software raster for sub-pixel meshlets on T0), writing into the Unified Visibility Buffer; evaluate PBR/MatCap/Zebra/Curvature shading once per visible pixel in linear `Rgba16Float`; draw feature edges as analytical SDF anti-aliased quad strips (`fwidth`-normalised); fall back to chunked indexed draw on T2 (`research/viz-ux-sota/GPU-ARCHITECTURE.md`).
- **Errors:** `MeshTooLarge` when exceeding chunked device limits.
- **Determinism:** N (GPU).
- **Performance budget:** REQ-005.
- **Crate:** `fid-render` (module `shade::mesh`).

## Test fixtures

`fixtures/viz/golden-cube`; `fixtures/viz/mesh-bunny-like` generated from seed 41 (no third-party scan data), CC0.

## Build prompt

> In crate `fid-render`, add module `shade::mesh`: an opaque surface pass feeding the Unified Visibility Buffer with shaded, shaded-with-edges, wireframe, hidden-line, MatCap, zebra/isophote and curvature display modes, plus display-only normal-displacement deviation exaggeration (`1×`–`500×`, flagging the HUD and never affecting CPU picking); every triangle writes render ID and primitive index to the `Rg32Uint` ID / Visibility target; partition large meshes into 64–128 tri meshlets with GPU frustum + backface-cone + Hi-Z culling and indirect draws on T0/T1 (chunked indexed fallback on T2); extract feature edges on CPU at a 30° dihedral threshold cached by mesh digest and render them with analytical SDF anti-aliasing; publish displayed/total triangle and culled meshlet counts per frame. Positions are f32 offsets from f64 chunk origins. Tests AC-1..AC-5 with `fixtures/viz/golden-cube` and generated `mesh-bunny-like` (seed 41). Rules: CONSTITUTION §8; C2; D-051.

## Open questions

- Meshlet DAG LOD and GPU-driven indirect culling are governed by D-051 (`research/viz-ux-sota/GPU-ARCHITECTURE.md`); P1a minimum may land chunked indexed draws first and add the meshlet cull pass behind the same `Pass` interface.

## Children

_Leaf: none._
