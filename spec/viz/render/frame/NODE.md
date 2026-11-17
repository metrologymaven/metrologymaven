---
id: VIZ.RENDER.FRAME
title: Frame graph, backends and capability tiers
level: 3
parent: VIZ.RENDER
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER-006, REQ-VIZ.RENDER-009]
covers: []
depends_on: [VIZ.RENDER.SCENE]
cli: [fid view get-render-info]
mcp: [view_get_render_info]
journal: [view.get_render_info@*]
updated: 2026-11-16
---

# VIZ.RENDER.FRAME: Frame graph, backends and capability tiers

## Purpose

The wgpu frame graph: backend selection, render passes, the ID and depth buffers for picking, capability-tier detection and the `view.get_render_info` read command, and the shader manifest that enforces "shaders map kernel data to pixels and nothing else" (C2). It is the GPU entry point every drawing module (LOD, SHADE, GLYPH, overlays) plugs passes into.

## Scope

**In:**
- Backend and adapter selection on Vulkan, Metal, DX12, WebGPU and GL as exposed by the pinned wgpu version
- Pass ordering and resource lifetimes for one frame
- ID buffer (render ID plus primitive or point index) and depth buffer output
- Capability tiers T0 to T3 (point-raster path, transparency path, limits) detected at start-up
- `view.get_render_info@1` (R0)
- Shader manifest and CI lint for shader inputs and outputs
- Drawing into a wgpu texture with no UI-toolkit dependency

**Out** (and which node owns it instead):
- Specific draw passes: points (`VIZ.RENDER.LOD`), meshes, CAD, transparency and clip raster (`VIZ.RENDER.SHADE`), instruments (`VIZ.RENDER.GLYPH`)
- Off-screen targets (`VIZ.RENDER.HEADLESS`)
- Device-loss recovery (`VIZ.RENDER.RECOVER`)
- The OS and GPU support matrix and `fid doctor` (`PLAT`)

## Requirements

- **REQ-VIZ.RENDER.FRAME-001** The frame graph shall run on every backend the pinned wgpu version exposes on the host, choosing the backend at start-up by a documented preference order that a setting can override.
- **REQ-VIZ.RENDER.FRAME-002** At start-up the renderer shall classify itself into capability tier T0, T1, T2 or T3 and record backend, adapter name, point-raster path and transparency path.
- **REQ-VIZ.RENDER.FRAME-003** `view.get_render_info@1` (R0) shall return tier, backend, adapter name, frame-time and memory budgets, and displayed and total primitive counts, in canonical JSON.
- **REQ-VIZ.RENDER.FRAME-004** The frame graph shall write an ID buffer (render ID plus primitive or point index) and a depth buffer for every presented frame.
- **REQ-VIZ.RENDER.FRAME-005** Every shader module shall be listed in a shader manifest declaring its inputs and outputs, and CI shall fail if any shader output other than colour, depth or ID targets is read back by non-test code.
- **REQ-VIZ.RENDER.FRAME-006** The renderer crate shall draw into a wgpu texture and shall not depend on any UI-toolkit crate, enforced by a build-time dependency check.

## Interfaces

- **Offers:** frame-graph pass API for other RENDER modules; ID and depth buffers for `VIZ.PICK`; `CapabilityTier` type; `view.get_render_info@1`.
- **Consumes:** `VIZ.RENDER.SCENE` nodes; displayed and total counts from `VIZ.RENDER.LOD` and `VIZ.RENDER.SHADE`.

## Decomposition hints

BACKEND (selection and limits), TIER (classification), PASSES (graph and lifetimes), IDBUF (ID and depth outputs), SHLINT (shader manifest and CI lint), INFO (`view.get_render_info`, likely a leaf).

## Open questions

- The tier names follow the concept doc (T0 native atomics, T1 WebGPU, T2 WebGL2/GL, T3 server stills); PLAT.GPU owns which hardware maps to which tier.

## Children

_None yet: status `open`._
