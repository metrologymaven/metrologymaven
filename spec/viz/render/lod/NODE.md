---
id: VIZ.RENDER.LOD
title: Point-cloud LOD streaming and point rasterization
level: 3
parent: VIZ.RENDER
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER-001, REQ-VIZ.RENDER-002]
covers: [PW.VIZ.01]
depends_on: [VIZ.RENDER.SCENE, VIZ.RENDER.FRAME, GEO.OOC]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.LOD: Point-cloud LOD streaming and point rasterization

## Purpose

Draw very large clouds at interactive rates: screen-space-error LOD traversal over the chunk hierarchy that GEO supplies, an LRU GPU chunk cache under a memory budget, progressive refinement, the three point-raster tiers (compute, two-pass WebGPU, sprites) that all write point indices to the ID buffer, attribute display modes, and the honest displayed-versus-total counts.

## Scope

**In:**
- LOD traversal by screen-space error under a per-frame point budget
- GPU chunk cache with LRU eviction under a VRAM fraction
- Progressive refinement without holes
- Point raster tiers T0 (u64 depth+index atomic min), T1 (two-pass u32), T2 (instanced sprites)
- Eye-dome lighting and normal shading
- Attribute modes: RGB, intensity, deviation bin, scan pass, timestamp, uncertainty, kernel classification
- Displayed and total point counts per frame

**Out** (and which node owns it instead):
- Chunking, octree building and COPC I/O of the data (`GEO.OOC`, `IO.CLOUD`)
- Colour LUTs and legends for deviation bins (`VIZ.OVERLAY`)
- Meshes and CAD (`VIZ.RENDER.SHADE`)

## Requirements

- **REQ-VIZ.RENDER.LOD-001** While a cloud exceeds the per-frame point budget, LOD traversal shall select chunks by screen-space error so that the drawn point count never exceeds the budget.
- **REQ-VIZ.RENDER.LOD-002** The GPU chunk cache shall be bounded by a configurable fraction of reported device memory, default 70 % [unverified], and shall evict least-recently-used chunks.
- **REQ-VIZ.RENDER.LOD-003** While refinement is pending, the renderer shall draw the coarsest loaded level for a region rather than leave it empty.
- **REQ-VIZ.RENDER.LOD-004** The renderer shall publish the displayed and total point counts for every presented frame.
- **REQ-VIZ.RENDER.LOD-005** The point raster shall use the compute path on T0, the two-pass path on T1 and instanced sprites on T2, and every path shall write the point index to the ID buffer.
- **REQ-VIZ.RENDER.LOD-006** While displaying 10⁸ points on the reference mid-range profile, the renderer shall sustain at least 30 fps [unverified].
- **REQ-VIZ.RENDER.LOD-007** Points the kernel classified as excluded shall be drawn in a distinct desaturated style and shall not be hidden unless the user hides that class explicitly.

## Interfaces

- **Offers:** point passes for the frame graph; `LodStats { displayed, total, budget }` for render info and the status bar.
- **Consumes:** `GEO.OOC` chunk hierarchy and memory-mapped chunks; `VIZ.RENDER.SCENE` cloud nodes; kernel bin indices and classification attributes as data.

## Decomposition hints

SSE (traversal and budget), CACHE (GPU cache and eviction), RASTER (three tiers), ATTR (attribute modes and excluded style), EDL (eye-dome lighting), STATS (counts).

## Open questions

- GEO.OOC (out-of-core streaming) is tagged P1b; P1a work on this node starts with in-memory chunk hierarchies for small CMM point sets, and streaming lands with GEO.OOC.
- The 10⁸-point budget is a hypothesis until measured on PLAT's reference profile; whether T0 compute raster is P1a or P1b depends on PLAT tier-1 targets.
- Applied: P1a work uses in-memory hierarchies (TRAV, RASTER, ATTR); streaming from `GEO.OOC.LOD` lands in CACHE (P1b).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.RENDER.LOD.TRAV | Screen-space-error traversal, budget and point counts (leaf) | 001 003 004 006 | PW.VIZ.01 |
| VIZ.RENDER.LOD.CACHE | GPU chunk cache and out-of-core streaming source (leaf, P1b) | 002 003 | none |
| VIZ.RENDER.LOD.RASTER | Point raster tiers writing point indices (leaf) | 005 006 | PW.VIZ.01 |
| VIZ.RENDER.LOD.ATTR | Point attribute modes, eye-dome lighting and excluded style (leaf) | 007 | none |
