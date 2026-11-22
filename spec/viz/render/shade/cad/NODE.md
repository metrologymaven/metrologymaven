---
id: VIZ.RENDER.SHADE.CAD
title: CAD faces, edges and large-assembly display
level: 4
parent: VIZ.RENDER.SHADE
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.SHADE-002, REQ-VIZ.RENDER.SHADE-006]
covers: [SA.VIZ.01]
depends_on: [VIZ.RENDER.SHADE.MESH, GEO.CAD.TESS, GEO.CAD.ASSY]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# VIZ.RENDER.SHADE.CAD: CAD faces, edges and large-assembly display

## Purpose

Show CAD models and large assemblies quickly: part bounding boxes within a second of opening, then progressive refinement to shaded tessellations, per-face identity (render ID plus face index) for picking, B-rep edges drawn from GEO's edge polylines, frustum and small-feature culling per part, and instancing of repeated parts.

## Scope

**In:** assembly traversal; bounding-box first pixels; progressive tessellation upload ordered by screen size; instancing by tessellation digest; frustum and screen-size culling; B-rep edge lines; face-index mapping in the ID buffer.
**Out:** tessellating B-reps (`GEO.CAD.TESS`); assembly structure (`GEO.CAD.ASSY`); shading itself (`VIZ.RENDER.SHADE.MESH`).

## Requirements

- **REQ-VIZ.RENDER.SHADE.CAD-001** When a CAD assembly is opened, the renderer shall draw every part's bounding box within 1 s and then refine parts to shaded tessellations in descending projected screen size [unverified].
- **REQ-VIZ.RENDER.SHADE.CAD-002** Parts sharing a tessellation digest shall be drawn by instancing with one GPU copy of the tessellation.
- **REQ-VIZ.RENDER.SHADE.CAD-003** Parts outside the frustum, occluded in the Hierarchical-Z depth pyramid, or projected smaller than a configurable pixel size (default 2 px) shall be culled (on the GPU via compute + indirect draw on T0/T1, with CPU fallback on T2) and counted as culled.
- **REQ-VIZ.RENDER.SHADE.CAD-004** Every CAD triangle shall write its part's render ID and a primitive index that maps on the CPU to its B-rep face ID.
- **REQ-VIZ.RENDER.SHADE.CAD-005** B-rep edges shall render with analytical screen-space anti-aliasing so edges stay crisp at HiDPI scale factors up to 200 %.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/assy-5k` (5 000 parts, 200 unique tessellations, generated seed 43), when opened on the software adapter, then the first frame with all boxes is presented within 1 s and the log shows refinement in descending screen size.
- **AC-2** Given the same fixture, then GPU tessellation buffers number 200, not 5 000 (or one sub-allocated mega-buffer holding 200 unique slices).
- **AC-3** Given a pixel probe on a face of part 42, then the primitive index maps to that face's B-rep face ID through the mapping table.
- **AC-4** Given a camera looking away from half the parts or blocked by an opaque bulkhead, then culled counts are reported and those parts issue zero rasterised triangles.

## Design

- **Registry command(s):** none.
- **Data structures:** `AssemblyDraw { parts: Vec<PartDraw>, instance_pool: GpuInstanceBuffer, indirect_args: GpuIndirectBuffer }`; `FaceMap` (primitive range → face ID).
- **Algorithm:** bounding boxes as instanced wireframe first; priority queue by projected area; upload tessellations from `GEO.CAD.TESS` asynchronously into a sub-allocated geometry pool; on T0/T1 run a WGSL compute pass performing frustum + Hi-Z occlusion + projected-size culling into compacted `DrawIndexedIndirectArgs` executed via `multi_draw_indexed_indirect_count` (CPU culling fallback on T2); render B-rep edge polylines as analytical SDF anti-aliased quad strips (`research/viz-ux-sota/GPU-ARCHITECTURE.md`).
- **Errors:** `TessellationUnavailable` (part stays as box with a diagnostic).
- **Determinism:** N.
- **Performance budget:** REQ-001.
- **Crate:** `fid-render` (module `shade::cad`).

## Test fixtures

`fixtures/viz/assy-5k`: synthetic assembly generated from seed 43, CC0.

## Build prompt

> In crate `fid-render`, add module `shade::cad`: draw all part bounding boxes within 1 s, then progressively upload and draw tessellations from `GEO.CAD.TESS` in descending projected size into a deduplicated geometry pool (one copy per tessellation digest); on T0/T1 perform GPU compute frustum + Hi-Z occlusion + projected-size (default 2 px) culling emitting `multi_draw_indexed_indirect_count` draws (CPU fallback on T2) with culled counts; write render ID and primitive index to the Visibility Buffer with a CPU face map to B-rep face IDs; draw B-rep edges with analytical SDF anti-aliasing. Tests AC-1..AC-4 with generated `fixtures/viz/assy-5k` (seed 43). Rules: CONSTITUTION §8; C2; D-051.

## Open questions

- Tagged P1b with GEO.CAD; GPU-driven indirect culling and Hi-Z follow D-051.

## Children

_Leaf: none._
