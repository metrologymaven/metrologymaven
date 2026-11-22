---
id: VIZ.RENDER.LOD.RASTER
title: Point raster tiers writing point indices
level: 4
parent: VIZ.RENDER.LOD
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.LOD-005, REQ-VIZ.RENDER.LOD-006]
covers: [PW.VIZ.01]
depends_on: [VIZ.RENDER.LOD.TRAV, VIZ.RENDER.FRAME.GRAPH, VIZ.RENDER.FRAME.TIER]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# VIZ.RENDER.LOD.RASTER: Point raster tiers writing point indices

## Purpose

Draw the selected points with the fastest path the tier allows: a compute raster with 64-bit depth-and-index atomic min on T0, a two-pass 32-bit compute raster on T1, and instanced sprites on T2. Every path writes the render ID and point index into the ID buffer so picking works the same on every tier.

## Scope

**In:** three point passes registered in the frame graph; path selection from the tier; point size in pixels; resolve pass into colour, depth and ID targets.
**Out:** which chunks to draw (`VIZ.RENDER.LOD.TRAV`); colours per attribute and eye-dome lighting (`VIZ.RENDER.LOD.ATTR`); CPU reference images (`VIZ.RENDER.ORACLE.CPURAST`).

## Requirements

- **REQ-VIZ.RENDER.LOD.RASTER-001** The renderer shall use the compute64 path on T0, the twopass32 path on T1 and the sprites path on T2, and a setting shall allow forcing a lower path for testing.
- **REQ-VIZ.RENDER.LOD.RASTER-002** Every path shall write, for each covered pixel, the render ID and the index of the nearest point into the ID buffer and its depth into the depth buffer.
- **REQ-VIZ.RENDER.LOD.RASTER-003** For the same scene and camera, the three paths shall select the same nearest point index at every pixel where depths differ by more than the path's declared depth tolerance.
- **REQ-VIZ.RENDER.LOD.RASTER-004** While displaying 10^8 points through LOD on the reference mid-range profile, the point passes shall take under 20 ms per frame on T0 [unverified].
- **REQ-VIZ.RENDER.LOD.RASTER-005** On T0 adapters exposing WGSL subgroups, `compute64` shall reduce same-pixel candidates within a subgroup before global atomic writes, and the renderer shall support an optional High-Quality Splatting (HQS) oriented-surfel display mode for zoomed-in scans.
- **REQ-VIZ.RENDER.LOD.RASTER-006** Uploaded point chunks shall use a compact GPU layout (chunk-local quantised offsets plus 16-bit octahedral normals and packed kernel bin/class flags) to maximise LRU VRAM residency without altering CPU f64 coordinates.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/points-grid` (a 512 × 512 grid of points at distinct depths) on the software adapter, when rendered with each available path, then the ID buffer at 100 sampled pixels holds the expected point indices.
- **AC-2** Given the same fixture, when the paths' ID buffers are compared, then they agree at every pixel outside the declared depth-tolerance mask.
- **AC-3** Given each path, then its output matches the `VIZ.RENDER.ORACLE.CPURAST` point reference within the path's tolerance mask.
- **AC-4** Given the benchmark scene on reference hardware, then frame times are reported against REQ-004 [unverified until measured].
- **AC-5** Given the compact GPU chunk layout, then per-point VRAM size is at most 12 bytes (excluding optional auxiliary attribute streams) and picking still resolves against unmodified CPU f64 positions.

## Design

- **Registry command(s):** none; library. Settings `viz.render.point_path` (`auto`, `compute64`, `twopass32`, `sprites`) and `viz.render.point_splat` (`pixel`, `hqs_surfel`).
- **Data structures:** `PointPath` enum; `PackedGpuPoint` (10–12 B: quantised chunk-local offset + 16-bit octahedral normal + packed u16 kernel bin/class/pass); per-path pass structs implementing `Pass` writing to the Unified Visibility Buffer (`research/viz-ux-sota/GPU-ARCHITECTURE.md`).
- **Algorithm:** `compute64` performs subgroup-level pixel deduplication (`enable subgroups;` when available) then packs `(reversed_z as u32 << 32) | vis_id` into a u64 storage image with `atomicMax`, feeding the deferred Visibility Buffer shading pass; optional HQS mode runs depth prepass + epsilon-band weighted surfel resolve using octahedral normals; `twopass32` writes depth with atomic max in pass 1 and index where depth matches in pass 2; `sprites` use instanced quads with depth test and `Rg32Uint` output.
- **Errors:** `PathUnsupported` when forced above the tier.
- **Determinism:** N (GPU); ties are bounded by the tolerance mask.
- **Performance budget:** REQ-004.
- **Crate:** `fid-render` (module `lod::raster`).

## Test fixtures

`fixtures/viz/points-grid`: generated in-repo, CC0.

## Build prompt

> In crate `fid-render`, add module `lod::raster` with three point passes registered in the frame graph: `compute64` (u64 atomicMax of reversed-Z|vis_id with WGSL subgroup reduction when supported, T0), `twopass32` (depth atomic pass then index pass, T1) and instanced `sprites` (T2), chosen from the capability tier or forced by setting `viz.render.point_path`. Pack GPU points in a compact ≤ 12 B chunk-local layout (quantised chunk-local offsets, 16-bit octahedral normals, packed kernel bin/class flags) and add an optional HQS oriented-surfel display mode (`viz.render.point_splat`) per `research/viz-ux-sota/GPU-ARCHITECTURE.md` (D-051). Every path writes render ID and nearest point index into the `Rg32Uint` ID / Visibility Buffer and reversed-Z depth into the depth buffer. Tests AC-1..AC-5 with `fixtures/viz/points-grid` and the CPU oracle. Rules: CONSTITUTION §8; C2 (shaders map kernel data to pixels only; no value computed on the GPU).

## Open questions

- Whether `compute64` is P1a depends on PLAT tier-1 targets; the `sprites` path is the P1a minimum. Subgroup reduction, HQS surfels, and compact GPU point layout are governed by D-051.

## Children

_Leaf: none._
