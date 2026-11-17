---
id: VIZ.RENDER.SHADE.CLIPR
title: Clip-plane raster and display caps
level: 4
parent: VIZ.RENDER.SHADE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.SHADE-004, REQ-VIZ.RENDER.SHADE-005]
covers: []
depends_on: [VIZ.RENDER.SHADE.MESH, VIZ.VIEW.STATE]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.SHADE.CLIPR: Clip-plane raster and display caps

## Purpose

Apply the clip planes and boxes from the ViewState to every surface and point pass, and fill the cut of closed solids with stencil caps that are styled distinctly from kernel-computed sections and never carry a metrology label.

## Scope

**In:** clip uniforms shared by all passes; up to 8 planes per viewport; clip boxes as 6 planes; stencil capping of closed solids; cap style.
**Out:** clip state and commands (`VIZ.VIEW.CLIP`); kernel sections (`SCAN.SECT`).

## Requirements

- **REQ-VIZ.RENDER.SHADE.CLIPR-001** The renderer shall clip every surface, edge and point pass against at least 6 simultaneous clip planes per viewport, read from the ViewState.
- **REQ-VIZ.RENDER.SHADE.CLIPR-002** Where a clip plane cuts a mesh marked closed, the renderer shall draw a stencil cap in the plane.
- **REQ-VIZ.RENDER.SHADE.CLIPR-003** Caps shall use the hatched display-cap style, distinct from the section style, shall write a derived render ID with view operation `ClipCap`, and shall carry no label or metrology value.
- **REQ-VIZ.RENDER.SHADE.CLIPR-004** Plane equations shall be computed on the CPU in f64 relative to the chunk origin and uploaded as f32 per chunk.

## Acceptance criteria

- **AC-1** Given `fixtures/viz/golden-cube` with one plane through its centre, when rendered, then the image matches golden `cube-clipcap.png` within the mask and the cap pixels hold a `ClipCap` derived ID.
- **AC-2** Given 6 planes forming a box, then points of `fixtures/viz/cloud-1m` outside the box are absent from the ID buffer.
- **AC-3** Given an open mesh, then no cap is drawn.
- **AC-4** Given a cap, then the pixel-probe colour equals the display-cap style and differs from the section style.

## Design

- **Registry command(s):** none.
- **Algorithm:** clip distances in vertex stage from per-chunk f32 planes; stencil increment/decrement for back and front faces per plane, then cap quad where stencil is odd.
- **Errors:** `TooManyPlanes` when the tier's limit is below the request (reported to `VIZ.VIEW.CLIP`).
- **Determinism:** N.
- **Performance budget:** 6 planes with caps add under 2 ms per frame on the reference profile [unverified].
- **Crate:** `fid-render` (module `shade::clipr`).

## Test fixtures

`fixtures/viz/golden-cube`, `fixtures/viz/cloud-1m`; golden `cube-clipcap.png` regenerated only by the ORACLE maintainer command.

## Build prompt

> In crate `fid-render`, add module `shade::clipr`: clip all surface, edge and point passes against up to 8 planes per viewport read from the ViewState (planes computed on the CPU in f64 relative to each chunk origin, uploaded as f32); boxes become 6 planes; closed meshes get stencil caps drawn in a hatched display-cap style with a `ClipCap` derived render ID and no label. Tests AC-1..AC-4 with `fixtures/viz/golden-cube` and `cloud-1m`. Rules: CONSTITUTION §8; C2 (a display clip is never a section result).

## Open questions

- None.

## Children

_Leaf: none._
