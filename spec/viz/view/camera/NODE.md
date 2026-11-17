---
id: VIZ.VIEW.CAMERA
title: Camera commands, fly-to and agent highlight
level: 3
parent: VIZ.VIEW
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW-006, REQ-VIZ.VIEW-008]
covers: []
depends_on: [VIZ.VIEW.STATE, CORE]
cli: [fid view set-camera, fid view fly-to, fid view highlight]
mcp: [view_set_camera, view_fly_to, view_highlight]
journal: []
updated: 2026-11-16
---

# VIZ.VIEW.CAMERA: Camera commands, fly-to and agent highlight

## Purpose

Everything that moves the camera or points at something: orbit, pan and zoom gestures (transient), standard and custom orientations, `view.set_camera`, fly-to an entity, and highlight with ghost overlays. When an agent highlights or flies to something, the result is a labelled proposal overlay that changes no document state.

## Scope

**In:**
- Orbit, pan, zoom, roll gestures and their keyboard equivalents (transient, not journaled)
- Standard orientations (front, back, top, bottom, left, right, isometric) relative to the active frame
- `view.set_camera@1`, `view.fly_to@1`, `view.highlight@1`
- Ghost overlays for agent-proposed datums or alignments, labelled "proposal"
- Fit-to-view and zoom-to-selection

**Out** (and which node owns it instead):
- View-track capture of transient motion (`VIZ.CAPTURE`)
- Camera animations and turntables (`VIZ.CAPTURE`)
- Ghost drawing (`VIZ.RENDER`)
- Multi-viewport sync (`VIZ.VIEW.MULTI`)

## Requirements

- **REQ-VIZ.VIEW.CAMERA-001** Transient orbit, pan, zoom and roll shall not create journal entries.
- **REQ-VIZ.VIEW.CAMERA-002** `view.set_camera@1` shall set the f64 eye, target, up and projection of a viewport exactly as given.
- **REQ-VIZ.VIEW.CAMERA-003** `view.fly_to@1` shall frame the target entity's f64 bounds in the viewport, ending at a camera computed deterministically from the bounds and current view direction.
- **REQ-VIZ.VIEW.CAMERA-004** When an agent principal issues `view.highlight@1` or `view.fly_to@1`, the system shall label the resulting overlay "proposal" and shall not change document state.
- **REQ-VIZ.VIEW.CAMERA-005** While reduced-motion mode is on, fly-to shall jump without animation.
- **REQ-VIZ.VIEW.CAMERA-006** Standard orientations shall be defined relative to the active coordinate frame supplied by the kernel.

## Interfaces

- **Offers:** `view.set_camera@1`, `view.fly_to@1`, `view.highlight@1` (exact classes fixed at leaf level; the open question below applies).
- **Consumes:** `VIZ.VIEW.STATE`; entity bounds and frames from GEO; principal identity from CORE.

## Decomposition hints

GESTURE (transient navigation), ORIENT (standard views), SETCAM, FLYTO, HILITE (highlight and proposal ghosts).

## Open questions

- Whether `view.set_camera@1` is journaled (W2 on the working document) or presentation-only is to be fixed with CORE at leaf level (parent open question).

## Children

_None yet: status `open`._
