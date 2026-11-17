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
updated: 2026-11-17
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
- **REQ-VIZ.RENDER.SHADE.CAD-003** Parts outside the frustum or projected smaller than a configurable pixel size (default 2 px) shall not be drawn and shall be counted as culled.
- **REQ-VIZ.RENDER.SHADE.CAD-004** Every CAD triangle shall write its part's render ID and a primitive index that maps on the CPU to its B-rep face ID.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/assy-5k` (5 000 parts, 200 unique tessellations, generated seed 43), when opened on the software adapter, then the first frame with all boxes is presented within 1 s and the log shows refinement in descending screen size.
- **AC-2** Given the same fixture, then GPU tessellation buffers number 200, not 5 000.
- **AC-3** Given a pixel probe on a face of part 42, then the primitive index maps to that face's B-rep face ID through the mapping table.
- **AC-4** Given a camera looking away from half the parts, then culled counts are reported and those parts issue no draws.

## Design

- **Registry command(s):** none.
- **Data structures:** `AssemblyDraw { parts: Vec<PartDraw>, instances: BTreeMap<Digest, InstanceBuffer> }`; `FaceMap` (primitive range → face ID).
- **Algorithm:** bounding boxes as line instances first; priority queue by projected area; upload tessellations from `GEO.CAD.TESS` asynchronously; instanced draw per digest with per-instance f64-origin offsets.
- **Errors:** `TessellationUnavailable` (part stays as box with a diagnostic).
- **Determinism:** N.
- **Performance budget:** REQ-001.
- **Crate:** `fid-render` (module `shade::cad`).

## Test fixtures

`fixtures/viz/assy-5k`: synthetic assembly generated from seed 43, CC0.

## Build prompt

> In crate `fid-render`, add module `shade::cad`: draw all part bounding boxes within 1 s, then progressively upload and draw tessellations from `GEO.CAD.TESS` in descending projected size; instance parts sharing a tessellation digest; cull by frustum and by projected size (default 2 px) with culled counts; write render ID and primitive index with a CPU face map to B-rep face IDs; draw B-rep edges as lines. Tests AC-1..AC-4 with generated `fixtures/viz/assy-5k` (seed 43). Rules: CONSTITUTION §8; C2.

## Open questions

- Tagged P1b with GEO.CAD.

## Children

_Leaf: none._
