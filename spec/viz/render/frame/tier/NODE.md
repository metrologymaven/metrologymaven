---
id: VIZ.RENDER.FRAME.TIER
title: Capability tiers and render-info command
level: 4
parent: VIZ.RENDER.FRAME
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-render, fid-view-cmd]
satisfies: [REQ-VIZ.RENDER.FRAME-002, REQ-VIZ.RENDER.FRAME-003]
covers: []
depends_on: [VIZ.RENDER.FRAME.GRAPH, CORE.REG.DEF, CORE.JRN.CANON]
side_effect: R0
determinism: D1
cli: [fid view get-render-info]
mcp: [view_get_render_info]
journal: [view.get_render_info@1]
updated: 2026-11-17
---

# VIZ.RENDER.FRAME.TIER: Capability tiers and render-info command

## Purpose

Classify the running renderer into capability tier T0 (native with 64-bit atomics), T1 (WebGPU-class), T2 (WebGL2 or GL without compute raster) or T3 (server stills), record the backend, adapter, point-raster path and transparency path, and report them with budgets and primitive counts through the read command `view.get_render_info@1`. The status bar, `fid doctor` and agents read the same answer.

## Scope

**In:** the `CapabilityTier` type and classification rules; the render-info snapshot; `view.get_render_info@1` (R0).
**Out:** which hardware is supported at which tier (`PLAT.GPU.POLICY`); displaying it (`VIZ.SHELL.DOCK.STATBAR`); the counts themselves (`VIZ.RENDER.LOD.TRAV`, `VIZ.RENDER.SHADE.MESH`).

## Requirements

- **REQ-VIZ.RENDER.FRAME.TIER-001** At device creation the renderer shall classify itself as T0 when the adapter supports 64-bit storage atomics and compute, T1 when it supports compute without 64-bit atomics, T2 when it lacks compute, and T3 when running headless for server stills.
- **REQ-VIZ.RENDER.FRAME.TIER-002** The renderer shall record backend, adapter name, driver info, tier, point-raster path (`compute64`, `twopass32`, `sprites`) and transparency path (`wboit`, `peel`, `sorted`).
- **REQ-VIZ.RENDER.FRAME.TIER-003** `view.get_render_info@1` shall return tier, backend, adapter name, point-raster path, transparency path, frame-time budget in ms, GPU memory budget in bytes, and displayed and total point and triangle counts of the last presented frame, as canonical JSON.
- **REQ-VIZ.RENDER.FRAME.TIER-004** `view.get_render_info@1` shall change no document, view or render state.

## Acceptance criteria

- **AC-1** Given the software adapter with fixture `fixtures/viz/golden-cube` rendered once, when `fid view get-render-info --json` runs, then the result validates against `schemas/render-info-1.json`, the triangle counts are 12 displayed and 12 total, and the adapter is the software adapter.
- **AC-2** Given recorded adapter feature sets in `fixtures/viz/adapters.json`, when classified, then each yields the expected tier in the fixture.
- **AC-3** Given two calls with no frame in between, then both results are byte-identical and the journal and Activity Log outside a recording session are unchanged.
- **AC-4** (error path) Given no device yet, then the command returns `RendererNotReady`.

## Design

- **Registry command(s):** `view.get_render_info@1`, R0, MCP exposed; params `{ viewport?: ViewportId }`; result `RenderInfo { tier, backend, adapter, driver, point_raster, transparency, frame_budget_ms, mem_budget_bytes, points: { displayed, total }, triangles: { displayed, total } }`. Per D-031 it is written to the Activity Log only while a recording session is active.
- **Data structures:** `CapabilityTier { T0, T1, T2, T3 }`; `RenderInfo`.
- **Algorithm:** inspect `wgpu::Features` and limits; map to tier; snapshot counts from the last frame's stats published by LOD and SHADE; serialise with the canonical JSON writer.
- **Errors:** `RendererNotReady`, `UnknownViewport`.
- **Determinism:** D1 (depends on the adapter; identical for the same adapter and frame).
- **Performance budget:** under 1 ms.
- **Crate:** `fid-render` (module `frame::tier`), command declared in `fid-view-cmd`.

## Test fixtures

`fixtures/viz/adapters.json` (recorded feature sets, project-authored); `fixtures/viz/golden-cube`; schema `schemas/render-info-1.json` published by this leaf.

## Build prompt

> In crate `fid-render`, add module `frame::tier` with `CapabilityTier { T0, T1, T2, T3 }` classified from wgpu features (64-bit storage atomics + compute = T0; compute only = T1; no compute = T2; headless server stills = T3), recording backend, adapter, driver, point-raster path and transparency path. Register registry command `view.get_render_info@1` (R0; CLI `fid view get-render-info`; MCP `view_get_render_info`) returning `RenderInfo` (tier, paths, budgets, displayed/total point and triangle counts of the last frame) as canonical JSON; publish `schemas/render-info-1.json`. It mutates nothing and goes to the Activity Log only during recording (D-031). Tests AC-1..AC-4. Rules: CONSTITUTION §8; C3 (generated surfaces from the registry).

## Open questions

- Tier names follow the concept doc; PLAT.GPU owns which hardware maps to which tier.

## Children

_Leaf: none._
