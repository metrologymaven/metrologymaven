---
id: VIZ.RENDER.GLYPH.ACTOR
title: Instrument, station, machine and fixture nodes
level: 4
parent: VIZ.RENDER.GLYPH
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.GLYPH-001, REQ-VIZ.RENDER.GLYPH-003]
covers: [SA.VIZ.02]
depends_on: [VIZ.RENDER.SCENE.PROJ, VIZ.RENDER.SHADE.MESH, GEO.FRAME.XFORM, GEO.FRAME.TREE]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.GLYPH.ACTOR: Instrument, station, machine and fixture nodes

## Purpose

Draw the physical actors of a measurement (laser trackers, total stations, articulated arms, scanner heads, CMM structures, fixtures) as scene nodes: built-in parametric symbols for instruments and stations with their frame triads, and imported meshes for machines and fixtures. Every pose is a kernel-supplied f64 transform resolved through the frame tree; a pose change appears in the next presented frame.

## Scope

**In:** node kinds `Instrument`, `Station`, `Machine`, `Fixture`; original parametric symbols (no incumbent icons, C9); frame triads; pose updates from `GEO.FRAME` transforms; level-of-detail for symbols by screen size.
**Out:** computing poses, bundle adjustment or kinematics (`LVM`, `CMM`); probes and styli (`VIZ.RENDER.GLYPH.STYLUS`); path polylines (`VIZ.RENDER.GLYPH.PATH`).

## Requirements

- **REQ-VIZ.RENDER.GLYPH.ACTOR-001** The renderer shall place every instrument, station, machine and fixture node by the f64 transform from its frame to world supplied by `GEO.FRAME`, and shall never derive a transform on the GPU.
- **REQ-VIZ.RENDER.GLYPH.ACTOR-002** If a node's frame does not resolve to world, then the node shall be drawn at no position, flagged `UnresolvedFrame` in diagnostics and omitted from the frame.
- **REQ-VIZ.RENDER.GLYPH.ACTOR-003** When a kernel transform for a node changes, the renderer shall show the new pose in the next presented frame.
- **REQ-VIZ.RENDER.GLYPH.ACTOR-004** Instrument and station symbols shall be project-original parametric shapes, scaled in screen space with a minimum of 24 px, and shall show the instrument frame axes on request.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/stations-3` (three stations with transforms 10 km from origin), when rendered headlessly, then the image matches the golden within the mask and symbol centres project to within 1 px of the CPU-projected f64 positions.
- **AC-2** Given a transform update between two frames, then the second frame shows the new pose (pixel probe of the symbol's render ID moves accordingly).
- **AC-3** (error path) Given a station whose frame is detached from world, then it is not drawn and one `UnresolvedFrame` diagnostic names it.
- **AC-4** Given a fixture mesh node, then every pixel of it carries its render ID in the ID buffer.

## Design

- **Registry command(s):** none.
- **Data structures:** `ActorNode { kind, frame: FrameId, shape: Symbol(SymbolKind) | Mesh(MeshRef) }`; symbol meshes generated in code.
- **Algorithm:** each frame, resolve frame-to-world (f64) via `GEO.FRAME.XFORM`, compose camera-relative (`VIZ.RENDER.PREC`), upload f32 model matrix; meshes drawn through the mesh pass.
- **Errors:** `UnresolvedFrame` (diagnostic).
- **Determinism:** N (pixels); transforms are D0 on the CPU.
- **Performance budget:** 1 000 actor nodes under 1 ms CPU per frame [unverified].
- **Crate:** `fid-render` (module `glyph::actor`).

## Test fixtures

`fixtures/viz/stations-3`: project-authored, CC0.

## Build prompt

> In crate `fid-render`, add module `glyph::actor`: node kinds Instrument, Station, Machine, Fixture; original parametric symbols (minimum 24 px screen size, optional frame axes) and imported meshes drawn via the mesh pass; poses from f64 frame-to-world transforms resolved through `GEO.FRAME` each frame, composed camera-relative and uploaded as f32; unresolved frames produce `UnresolvedFrame` and are not drawn; a transform change shows in the next frame. Tests AC-1..AC-4 with `fixtures/viz/stations-3`. Rules: CONSTITUTION §8; C2; C9 (no incumbent symbols).

## Open questions

- Live instrument poses arrive with HAL and LVM (P2, P3); P1b uses imported or stored station poses.

## Children

_Leaf: none._
