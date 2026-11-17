---
id: VIZ.RENDER
title: Renderer core and scene
level: 2
parent: VIZ
kind: subsystem
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-VIZ-001, REQ-VIZ-002, REQ-VIZ-006]
covers: [PW.VIZ.01, SA.VIZ.01, SA.VIZ.02]
depends_on: [GEO, CORE]
cli: [fid view get-render-info]
mcp: [view_get_render_info]
journal: []
updated: 2026-11-16
---

# VIZ.RENDER: Renderer core and scene

## Purpose

The display engine every other VIZ child draws through: the semantic scene graph that binds every drawable to a stable entity ID, the wgpu frame graph, camera-relative precision, LOD streaming of very large clouds, mesh and CAD shading, the drawing of instruments, probes and machine models, transparency and clip-plane rasterization, capability-tier detection, the headless (off-screen) renderer, and recovery from GPU device loss. It exists as a separate unit because it is the only part of VIZ that talks to the GPU, and it must stay free of UI-toolkit choices (D-005) and of any value-of-record logic (C2).

## Scope

**In:**
- Semantic scene graph: display nodes bound to entity IDs (QIF persistent IDs or derived IDs), dirty-set re-projection from the dependency graph, stale-state marking
- wgpu frame graph and render passes on the Vulkan, Metal, DX12, WebGPU and GL backends that wgpu exposes
- Camera-relative (relative-to-eye) rendering with f64 chunk origins on the CPU and f32 offsets on the GPU; reversed-Z depth
- LOD traversal and out-of-core streaming of point-cloud chunks supplied by GEO, under a frame and memory budget
- Mesh and CAD tessellation shading, edges, silhouettes; large assemblies
- Point rasterization tiers and their fallbacks (compute raster where atomics allow, sprite fallback)
- Order-independent transparency and clip-plane / section-plane rasterization (the state comes from VIZ.VIEW)
- Drawing of instruments, stations, probes, styli, machine and fixture models as scene nodes
- ID buffer output consumed by VIZ.PICK
- Capability-tier detection and reporting (feeds `fid doctor` in PLAT)
- Headless off-screen rendering for reports, CI golden images and server-rendered stills
- GPU device-loss detection and recovery
- CPU reference rasterizer used as a test oracle for GPU output

**Out** (and which node owns it instead):
- Colour maps, legends, labels, whiskers, vector plots (`VIZ.OVERLAY`)
- Camera, saved-view, visibility, clipping and transparency state and commands (`VIZ.VIEW`)
- Resolving picks to entities (`VIZ.PICK`)
- Frame readback, encoding and the view track (`VIZ.CAPTURE`)
- The WASM build, package loader and web fallbacks policy (`VIZ.WEB`)
- The OS / GPU support matrix and minimum hardware profiles (`PLAT`)
- Geometry types, spatial indexes and out-of-core chunking of the data itself (`GEO`)

## Requirements

- **REQ-VIZ.RENDER-001** While displaying a cloud of 10⁸ points and a mesh of 10⁷ triangles on the PLAT reference mid-range GPU profile, the renderer shall sustain at least 30 fps through LOD streaming [unverified].
- **REQ-VIZ.RENDER-002** While the LOD budget limits what is drawn, the renderer shall publish the displayed and total primitive counts so the UI can announce the reduction.
- **REQ-VIZ.RENDER-003** The renderer shall keep world coordinates in f64 on the CPU and upload only f32 offsets from per-chunk origins, with chunk half-extents of at most 1 m [unverified].
- **REQ-VIZ.RENDER-004** When a 1 µm step placed 10 km from the world origin is rendered in a headless camera sweep, the renderer shall show no vertex jitter above one pixel [unverified].
- **REQ-VIZ.RENDER-005** Every drawable shall resolve to an entity ID or a derived ID (source ID plus view operation); a drawable without one shall be rejected at scene-build time.
- **REQ-VIZ.RENDER-006** No shader shall compute a value that is shown as a number, a pass/fail state or a colour bin; shaders shall only map kernel-supplied data to pixels.
- **REQ-VIZ.RENDER-007** When an upstream input of a displayed result changes, the renderer shall show that result as stale until the dependency graph has recomputed it.
- **REQ-VIZ.RENDER-008** When the GPU device is lost, the renderer shall rebuild its GPU resources from the scene graph without discarding any committed command, and shall resume within 5 s on the reference profile [unverified].
- **REQ-VIZ.RENDER-009** At start-up the renderer shall detect and expose its capability tier (backend, point-raster path, transparency path).
- **REQ-VIZ.RENDER-010** The renderer shall render any scene off-screen at a requested resolution without a window, for reports, CI and server-rendered stills.
- **REQ-VIZ.RENDER-011** For every GPU render path, a CPU reference rasterizer shall exist, and CI shall compare GPU output against it with a declared per-pixel tolerance mask.
- **REQ-VIZ.RENDER-012** The renderer shall draw instruments, stations and probes as scene nodes placed by kernel-supplied f64 transforms, never by transforms inferred on the GPU.

## Interfaces

- **Offers:** the render crate (scene graph, frame graph, headless renderer); `view.get_render_info@1` (R0: tier, backend, budgets, counts); ID buffer and depth buffer for VIZ.PICK; off-screen frame API for VIZ.CAPTURE and RPT.
- **Consumes:** GEO chunks, tessellations, frames and transforms; CORE dependency-graph dirty sets; VIZ.VIEW view state; VIZ.OVERLAY display buffers (kernel bin indices, label glyphs).

## Decomposition hints

SCENE (scene graph and ID tables), FRAME (frame graph, passes, backends), PREC (camera-relative precision and CI guard), LOD (cloud streaming), SHADE (mesh/CAD shading, edges, OIT, clipping raster), GLYPH (instruments, probes, machines), HEADLESS, RECOVER (device loss), ORACLE (CPU reference rasterizer).

## Open questions

- The reference mid-range GPU profile is defined by PLAT; the 10⁸-point budget is a hypothesis until measured there.
- Whether the point compute-raster path is P1a or P1b depends on PLAT's tier-1 targets.

## Children

_None yet: status `open`._
