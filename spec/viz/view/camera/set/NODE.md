---
id: VIZ.VIEW.CAMERA.SET
title: Set-camera and fly-to commands
level: 4
parent: VIZ.VIEW.CAMERA
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW.CAMERA-002, REQ-VIZ.VIEW.CAMERA-003, REQ-VIZ.VIEW.CAMERA-004, REQ-VIZ.VIEW.CAMERA-005]
covers: []
depends_on: [VIZ.VIEW.CAMERA.NAV, VIZ.VIEW.STATE, CORE.BUS.PRIN, CORE.REG.DEF]
side_effect: [W2, R1]
determinism: D0
cli: [fid view set-camera, fid view fly-to]
mcp: [view_set_camera, view_fly_to]
journal: [view.set_camera@1, view.fly_to@1]
built_in: [fid-view]
updated: 2026-11-22
---

# VIZ.VIEW.CAMERA.SET: Set-camera and fly-to commands

## Purpose

The two registry commands that move a camera on purpose. `view.set_camera@1` writes an exact f64 camera into a viewport's ViewState and is journaled like the other view-state edits (W2). `view.fly_to@1` computes a framing camera for an entity and applies it as transient presentation (R1), the same way a gesture would, so it changes no document state; when an agent calls it the viewport shows a "proposal" tag until a person moves the camera.

## Scope

**In:** `view.set_camera@1` (W2) and `view.fly_to@1` (R1); validation; proposal tag for agent principals; reduced-motion jump.
**Out:** gesture navigation (`VIZ.VIEW.CAMERA.NAV`); highlight ghosts (`VIZ.VIEW.CAMERA.HILITE`); saved views (`VIZ.VIEW.NAMED`).

## Requirements

- **REQ-VIZ.VIEW.CAMERA.SET-001** `view.set_camera@1` shall set the f64 eye, target and up in metres and the projection of the target viewport's ViewState exactly as given and return the new ViewState digest.
- **REQ-VIZ.VIEW.CAMERA.SET-002** Every successful `view.set_camera@1` shall be journaled with the resulting ViewState digest, and replay shall reproduce that digest.
- **REQ-VIZ.VIEW.CAMERA.SET-003** If eye equals target, up is parallel to the view direction, or any value is non-finite, then `view.set_camera@1` shall fail with a typed error and journal nothing.
- **REQ-VIZ.VIEW.CAMERA.SET-004** `view.fly_to@1` shall compute the end camera from the target entity's f64 bounds and the current view direction by the NAV fit rule, apply it to the live camera only, and return the end camera.
- **REQ-VIZ.VIEW.CAMERA.SET-005** When the calling principal is an agent, `view.fly_to@1` shall show a "proposal" tag on the viewport until the next human camera change, and shall change no document state.
- **REQ-VIZ.VIEW.CAMERA.SET-006** While reduced-motion mode is on, `view.fly_to@1` shall jump without animation.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube`, when `fid view set-camera --viewport main --eye 1,1,1 --target 0,0,0 --up 0,0,1 --perspective-fov-deg 45 --json` runs, then the result digest equals the journal entry's digest and the ViewState camera holds exactly those f64 values.
- **AC-2** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.
- **AC-3** (error path) Given `--eye 0,0,0 --target 0,0,0`, then the command fails with `DegenerateCamera` and journals nothing.
- **AC-4** Given `fid view fly-to --entity <face-id> --json` twice from the same start, then both returned cameras are bit-identical and the journal head is unchanged.
- **AC-5** Given `view_fly_to` called over MCP by an agent, then the viewport shows the proposal tag, the working document digest is unchanged, and the tag clears after a human orbit.

## Design

- **Registry command(s):**
  - `view.set_camera@1`, W2 (agents write to their branch); params `{ viewport: ViewportId, eye_m: [f64;3], target_m: [f64;3], up: [f64;3], projection: Perspective { fov_y_rad } | Ortho { height_m } }`; result `{ view_state_digest }`. Angles accepted as authored decimals with unit and converted once (journaled).
  - `view.fly_to@1`, R1 (Activity Log during recording only, D-031); params `{ viewport, entity: EntityRef }`; result `{ camera: Camera }`.
- **Algorithm:** set: validate, normalise up orthogonal to view direction in f64, write ViewState, digest. Fly-to: bounds from the scene graph, fit rule from NAV, apply to live camera, attach proposal tag if principal kind is agent.
- **Errors:** `DegenerateCamera`, `NonFiniteNumber`, `UnknownViewport`, `UnknownEntity`, `EntityNotDisplayed`.
- **Determinism:** D0.
- **Performance budget:** each command under 5 ms.
- **Crate:** `fid-view` (module `camera`).

## Test fixtures

`fixtures/viz/golden-cube` with entity IDs for its faces.

## Build prompt

> In crate `fid-view`, add module `camera` with two registry commands. `view.set_camera@1` (W2; CLI `fid view set-camera`; MCP `view_set_camera`): set exact f64 eye/target/up and projection on a viewport's ViewState, reject degenerate or non-finite input (`DegenerateCamera`, `NonFiniteNumber`) without journaling, return and journal the ViewState digest. `view.fly_to@1` (R1; CLI `fid view fly-to`; MCP `view_fly_to`): compute the end camera from the entity's f64 bounds with the NAV fit rule, apply it only to the live camera (jump under reduced motion), return it; if the principal is an agent, show a "proposal" tag until the next human camera change; never change document state. Tests AC-1..AC-5 with `fixtures/viz/golden-cube`. Rules: CONSTITUTION §3 (names, side-effect classes, D-031 Activity Log for R1), §8; C5.

## Open questions

- Settles the parent question for this leaf: `view.set_camera@1` is W2 (consistent with `view.set_clip@1`), while `view.fly_to@1` and `view.highlight@1` are R1 presentation commands. Confirm with CORE if the working document should exclude camera state entirely.

## Children

_Leaf: none._
