---
id: VIZ.OVERLAY.GDTGLYPH.PLACE
title: GD&T annotation anchoring, occlusion fade and hover
level: 4
parent: VIZ.OVERLAY.GDTGLYPH
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY.GDTGLYPH-004]
covers: [PW.VIZ.04]
depends_on: [VIZ.OVERLAY.GDTGLYPH.COMPOSE, VIZ.OVERLAY.GDTGLYPH.ATLAS, VIZ.RENDER.FRAME, VIZ.VIEW.CAMERA, GDT.CHAR.SPEC]
uses_artifacts: []
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.OVERLAY.GDTGLYPH.PLACE: GD&T annotation anchoring, occlusion fade and hover

## Purpose

Puts composed frames into the 3D view and makes them inspectable. In P1a, where characteristics come from drawings and QIF without CAD PMI, frames anchor as screen-facing billboards at the toleranced feature's reference point; when an annotation plane is known (CAD PMI, P1b), they lie in that plane. Frames behind geometry fade instead of disappearing. Hovering a frame highlights the toleranced feature and datum features and shows the characteristic's standard reference IDs (designator, edition, clause), never standards text.

## Scope

**In:** anchors (`Billboard { point }`, `Plane { origin, x, y }`); leader from frame to feature; MSDF glyph draw pass in the frame graph; occlusion fade by depth test against the scene (display only); hover hit test on frame rectangles; highlight through `view.highlight` of `VIZ.VIEW.CAMERA`; hover card with standard reference IDs.
**Out:** layout (`VIZ.OVERLAY.GDTGLYPH.COMPOSE`); atlas (`VIZ.OVERLAY.GDTGLYPH.ATLAS`); graphic PMI comparison (`VIZ.OVERLAY.GDTGLYPH.PMIQA`); label layout (`VIZ.OVERLAY.LABEL`).

## Requirements

- **REQ-VIZ.OVERLAY.GDTGLYPH.PLACE-001** When the pointer rests on a frame, the view shall highlight the toleranced feature and every referenced datum feature within 100 ms [unverified].
- **REQ-VIZ.OVERLAY.GDTGLYPH.PLACE-002** The hover card shall show the governing spec's standard designators, editions and clause IDs only, and shall contain no text from any standard.
- **REQ-VIZ.OVERLAY.GDTGLYPH.PLACE-003** A characteristic without an annotation plane shall be drawn as a screen-facing billboard at its anchor point, and one with a plane shall be drawn in that plane.
- **REQ-VIZ.OVERLAY.GDTGLYPH.PLACE-004** Frames occluded by geometry shall be drawn at reduced opacity (25 %) rather than hidden.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/gdt/placed.json` (a part with 10 characteristics, 3 with annotation planes) on `fixtures/viz/golden-cube`, when rendered headlessly, then the image matches the golden within tolerance (billboards for 7, planes for 3).
- **AC-2** Given a hover on a position frame with datums A and B, then the highlight set holds the feature and both datum features, and the card shows designator, edition and clause IDs only (string check against the governing spec).
- **AC-3** Given a frame behind the cube, then it is drawn at 25 % opacity.

## Design

- **Registry command(s):** none owned; hover highlight uses `VIZ.VIEW.CAMERA`'s highlight API (non-journaled for local hover).
- **Data structures:** `PlacedFrame { characteristic, glyph: FrameGlyph, anchor, leader_to }`.
- **Algorithm:** project anchors with the f64 camera relative to the camera origin (camera-relative precision), size glyphs in screen space, draw with the MSDF pass; occlusion via depth comparison in the shader (display only).
- **Errors:** `MissingAnchor` (frame listed in the problems panel, not drawn).
- **Determinism:** D1.
- **Performance budget:** 500 frames at 60 fps on the reference profile [unverified].
- **Crate:** `fid-overlay` (module `gdt::place`).

## Test fixtures

`fixtures/viz/gdt/placed.json` and a golden image; CC-BY-4.0.

## Build prompt

> In crate `fid-overlay`, add module `gdt::place`: place composed `FrameGlyph`s as billboards (no annotation plane) or in their annotation plane, draw them with the MSDF atlas in a frame-graph pass with a leader to the feature, fade occluded frames to 25 % opacity, hit-test hovers, highlight the toleranced and datum features through the view highlight API and show a hover card with standard designator, edition and clause IDs only. Tests AC-1..AC-3. Rules: never show or store standards text; GPU for display only (C2); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- P1a anchors on billboards because annotation planes need CAD PMI (P1b) [unverified].

## Children

_Leaf: none._
