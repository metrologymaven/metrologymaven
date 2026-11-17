---
id: VIZ.PICK.RESOLVE
title: Single pick resolved on the CPU
level: 3
parent: VIZ.PICK
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.PICK-001, REQ-VIZ.PICK-002, REQ-VIZ.PICK-003, REQ-VIZ.PICK-008]
covers: []
depends_on: [VIZ.RENDER.SCENE, VIZ.RENDER.FRAME, GEO.INDEX]
side_effect: R1
determinism: D0
cli: [fid view pick-point]
mcp: [view_pick_point]
journal: [view.pick_point@1]
updated: 2026-11-16
---

# VIZ.PICK.RESOLVE: Single pick resolved on the CPU

## Purpose

Turn a pointer position into a semantic hit: the GPU ID buffer proposes a render ID and primitive or point index, and the CPU refines it against f64 geometry through GEO spatial indexes into an entity ID plus an f64 point (and face parameters where they exist). Nothing downstream ever sees screen coordinates or GPU buffer contents.

## Scope

**In:** ID-buffer readback at a pixel (1 px or a 5×5 neighbourhood), CPU ray construction from the f64 camera, f64 refinement against the candidate entity, empty-hit handling, `view.pick_point@1`.
**Out:** snapping (`VIZ.PICK.SNAP`); lassos (`VIZ.PICK.LASSO`); selection state (`VIZ.PICK.SELSET`); readouts (`VIZ.PICK.READOUT`); the ID buffer itself (`VIZ.RENDER.FRAME`).

## Requirements

- **REQ-VIZ.PICK.RESOLVE-001** When a pick completes, the system shall resolve the ID-buffer candidate on the CPU against f64 geometry before returning a hit.
- **REQ-VIZ.PICK.RESOLVE-002** A hit shall contain only an entity ID, an optional sub-element index, an f64 point in metres in the document frame and optional face parameters; it shall contain no screen coordinates, render IDs or GPU buffer data.
- **REQ-VIZ.PICK.RESOLVE-003** A resolved surface point shall lie within 1e-9 m of the f64 geometry it was resolved against [unverified].
- **REQ-VIZ.PICK.RESOLVE-004** If the ID buffer holds no candidate at the pick position or CPU refinement fails, then the command shall return an empty hit and shall not fall back to depth-buffer or screen-space data.
- **REQ-VIZ.PICK.RESOLVE-005** Resolving the same pick on the same document state and view shall return an identical hit.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/pick-sphere-plane` (triangulated sphere mesh of radius 0.05 m with 20 000 triangles on a plane mesh, stored view `front`), when `fid view pick-point --view front --x 960 --y 540 --json` runs headlessly at 1920×1080, then the result names the sphere's entity ID and a point that lies inside the hit triangle and within 1e-9 m of its f64 plane.
- **AC-2** Given the same fixture, when the pick is at a background pixel, then the result is an empty hit and no error.
- **AC-3** Given the result of AC-1, then the hit JSON contains no keys for pixel, render ID or buffer data (schema check).
- **AC-4** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.
- **AC-5** (error path) Given a view ID that does not exist, when the command runs, then it fails with `UnknownView` and journals nothing.

## Design

- **Registry command(s):** `view.pick_point@1`, R1. Parameters: `view: ViewRef` (viewport or NamedView), `x: u32`, `y: u32` (device pixels in that view's raster), `neighbourhood: One|Five`, `snap: SnapMode` (default `None`; modes implemented by `VIZ.PICK.SNAP`). Result: `PickHit { entity: EntityId?, sub: u32?, point_m: [f64; 3]?, uv: [f64; 2]? }`. The pixel arguments are inputs only; they never appear in the result or in any selection.
- **Algorithm:** read the ID buffer (async readback, 1 px or 5×5, closest depth wins); map render ID to entity through the scene tables; build the f64 pick ray from the camera; intersect with the candidate entity's f64 geometry through the GEO BVH (triangle or point by index in P1a; CAD faces once GEO.CAD lands in P1b).
- **Errors:** `UnknownView`, `PixelOutOfRange`, `GeometryUnavailable`.
- **Determinism:** D0 (CPU f64 refinement on fixed inputs; the ID-buffer candidate is deterministic for a fixed view on a given adapter, and the refined result is independent of the candidate within the same entity).
- **Performance budget:** hit returned within 2 frames plus 10 ms CPU refinement on the reference profile [unverified].
- **Crate:** `fid-pick`.

## Test fixtures

`fixtures/viz/pick-sphere-plane`: generated sphere and plane meshes (seeded) plus a stored view; in-repo, no external licence.

## Build prompt

> In crate `fid-pick`, implement `view.pick_point@1` (R1; CLI `fid view pick-point`, MCP `view_pick_point`). Inputs: a view reference, pixel x/y in that view's raster, neighbourhood (1 px or 5×5) and a snap mode (only `None` here). Read the ID buffer from `fid-render`, map render ID to entity ID via the scene tables, build the f64 pick ray from the view's f64 camera, and refine against the entity's f64 geometry using GEO's BVH. Return `PickHit { entity, sub, point_m, uv }` with no pixel, render-ID or buffer data. Return an empty hit when nothing is hit; never fall back to screen-space data. Tests: sphere-mesh pick inside the hit triangle and within 1e-9 m of its plane; background pick empty; schema has no screen keys; exact replay `IDENTICAL`; `UnknownView` error journals nothing. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; GPU output is only a candidate (CONSTITUTION C2).

## Open questions

- None blocking.

## Children

_Leaf: none._
