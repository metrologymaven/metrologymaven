---
id: VIZ.RENDER.GLYPH.STYLUS
title: Parametric probe, stylus and ghost-probe display
level: 4
parent: VIZ.RENDER.GLYPH
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.GLYPH-001, REQ-VIZ.RENDER.GLYPH-002, REQ-VIZ.RENDER.GLYPH-004]
covers: [SA.VIZ.02]
depends_on: [VIZ.RENDER.GLYPH.ACTOR, VIZ.RENDER.SHADE.OIT]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.GLYPH.STYLUS: Parametric probe, stylus and ghost-probe display

## Purpose

Build display geometry for a probe assembly (head, extensions, stylus shanks, ball, star and disc tips) from a plain parametric description, with tips drawn exactly at their tip radius, and draw ghost probes and swept-volume ghosts at program steps in a style that reads as approximate and carries no conformance status.

## Scope

**In:** `StylusSpec` input type (data only); tessellation of tips and shanks to scale; head angles from a supplied f64 transform; ghost and swept-volume styles (translucent, dashed outline, "approx." legend tag).
**Out:** the probe configuration entity and qualification (`CMM.PROBE.MODEL`, `CMM.PROBE.HEAD`); collision and swept-volume computation (`CMM.COLL`); stations and machines (`VIZ.RENDER.GLYPH.ACTOR`).

## Requirements

- **REQ-VIZ.RENDER.GLYPH.STYLUS-001** The renderer shall draw each tip as a sphere, disc or star element whose radius equals the tip radius in the `StylusSpec` in metres, tessellated so the silhouette deviates from the true radius by at most 0.5 % [unverified].
- **REQ-VIZ.RENDER.GLYPH.STYLUS-002** The probe assembly shall be placed by the f64 transform supplied for its head angle and step, never by an angle interpolated on the GPU.
- **REQ-VIZ.RENDER.GLYPH.STYLUS-003** Ghost probes and swept-volume ghosts shall be drawn translucent with a dashed outline and an "approximate" tag, and the style resolver shall never give them a conformance style.
- **REQ-VIZ.RENDER.GLYPH.STYLUS-004** If a `StylusSpec` has a non-positive or non-finite dimension, then building it shall fail with `InvalidStylus` and draw nothing for it.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/stylus-star` (a 5-tip star, tip radius 1 mm, shank 2 mm), when rendered orthographically at 100 px per mm, then each tip's measured silhouette radius on the CPU oracle image is 100 px ± 0.5 px.
- **AC-2** Given a ghost probe at step 3, then the pixel probe shows the ghost style and the node's status is `Approximate`, not a decision outcome.
- **AC-3** (error path) Given a tip radius of −1 mm, then `InvalidStylus` is returned.
- **AC-4** Given two head transforms, then the assembly's tip centres project to the CPU-computed f64 positions within 1 px.

## Design

- **Registry command(s):** none.
- **Data structures:** `StylusSpec { elements: Vec<Element { kind: Ball | Disc | Star | Shank | Extension, dims_m, offset_m: [f64;3] }> }` (an app adapter maps `CMM.PROBE.MODEL` entities into it, P2); `GhostStyle`.
- **Algorithm:** generate meshes with segment counts from the radius-error bound; ghosts through the OIT pass.
- **Errors:** `InvalidStylus`.
- **Determinism:** N (pixels); tessellation is D0.
- **Performance budget:** build a 20-element stylus under 1 ms.
- **Crate:** `fid-render` (module `glyph::stylus`).

## Test fixtures

`fixtures/viz/stylus-star`: project-authored, CC0.

## Build prompt

> In crate `fid-render`, add module `glyph::stylus`: build probe display meshes from a data-only `StylusSpec` (ball, disc, star tips, shanks, extensions; dimensions in metres) with tip silhouettes within 0.5 % of the tip radius; place by the supplied f64 head/step transform; draw ghost probes and swept volumes translucent with dashed outline and an "approximate" tag, never with a conformance style; reject invalid dimensions with `InvalidStylus`. Tests AC-1..AC-4 with `fixtures/viz/stylus-star` and the CPU oracle. Rules: CONSTITUTION §8; C2.

## Open questions

- No code edge to `CMM.PROBE.MODEL` (P2): the app maps probe entities to `StylusSpec`, so P1b path review works without CMM.

## Children

_Leaf: none._
