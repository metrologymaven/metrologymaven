---
id: VIZ.VIEW.CAMERA.NAV
title: Transient navigation, standard orientations and fit
level: 4
parent: VIZ.VIEW.CAMERA
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-view]
built_pr: 495
satisfies: [REQ-VIZ.VIEW.CAMERA-001, REQ-VIZ.VIEW.CAMERA-005, REQ-VIZ.VIEW.CAMERA-006]
covers: []
depends_on: [VIZ.VIEW.STATE, VIZ.RENDER.SCENE.PROJ, GEO.FRAME.TREE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# VIZ.VIEW.CAMERA.NAV: Transient navigation, standard orientations and fit

## Purpose

The camera controller of a live viewport: orbit, pan, zoom and roll from pointer, wheel, touch and keyboard; the seven standard orientations relative to the active kernel frame; fit-to-view and zoom-to-selection. All of it is transient presentation state: it never writes the journal and is recorded only by the view track. Animations obey reduced-motion mode.

## Scope

**In:** `CameraController` state machine; gesture and key mappings (keys registered as palette actions); orbit pivot at the picked point or scene centre; standard orientations (front, back, top, bottom, left, right, isometric) from the active frame's axes; fit and zoom-to-selection from f64 bounds; animation easing with reduced-motion jump.
**Out:** journaled camera changes (`VIZ.VIEW.CAMERA.SET`); highlight (`VIZ.VIEW.CAMERA.HILITE`); view-track capture (`VIZ.CAPTURE.TRACK`); multi-viewport sync (`VIZ.VIEW.MULTI`).

## Requirements

- **REQ-VIZ.VIEW.CAMERA.NAV-001** Orbit, pan, zoom and roll shall update only the viewport's live camera and shall create no journal or Activity Log entry.
- **REQ-VIZ.VIEW.CAMERA.NAV-002** Each standard orientation shall place the view direction along an axis (or the (1,1,1) diagonal for isometric) of the active frame supplied by `GEO.FRAME`, with up along the frame's +Z, or +Y when looking along Z.
- **REQ-VIZ.VIEW.CAMERA.NAV-003** Fit-to-view and zoom-to-selection shall frame the f64 bounds of the visible scene or the selection so that the bounding sphere fills 90 % of the shorter viewport side.
- **REQ-VIZ.VIEW.CAMERA.NAV-004** While reduced-motion mode is on, orientation changes and fits shall jump to the end camera with no intermediate frames; otherwise they shall animate for at most 250 ms.
- **REQ-VIZ.VIEW.CAMERA.NAV-005** Every navigation action shall have a keyboard equivalent registered with the shell's keymap.
- **REQ-VIZ.VIEW.CAMERA.NAV-006** The end camera of every orientation and fit shall be computed in f64 deterministically from bounds, frame and current view direction.
- **REQ-VIZ.VIEW.CAMERA.NAV-007** When an orbit gesture begins over geometry, the controller shall automatically set the orbit pivot to the 3D surface point under the cursor (falling back to scene or selection centroid when over background), and the viewport shall display an interactive DRF-aligned orientation cube with a one-click Orthographic/Perspective toggle (defaulting to Orthographic for dimensional inspection).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube` with a rotated active frame, when "top" is applied (by key or orientation-cube face click), then the camera direction equals the frame's −Z axis within 1e-12 and up equals +Y of the frame.
- **AC-2** Given a scripted 5 s orbit, then the journal head and the Activity Log are unchanged.
- **AC-3** Given reduced-motion on, when "isometric" is applied, then exactly one frame later the camera equals the end camera.
- **AC-4** Given a selection of two features, when zoom-to-selection runs twice from the same start, then both end cameras are bit-identical.
- **AC-5** Given the keymap export, then every navigation action (including Ortho/Perspective toggle) has a binding.
- **AC-6** Given an orbit started over a vertex of `golden-cube`, then the pivot equals that surface point rather than the world origin, and starting an orbit over empty background falls back to the scene centroid.

## Design

- **Registry command(s):** none (transient presentation; deliberate camera commands are in `VIZ.VIEW.CAMERA.SET`).
- **Data structures:** `CameraController { live: Camera, pivot_m, anim: Option<Anim>, projection_default }`; `StdView` enum; `OrientationCubeState { active_frame_axes, hovered_face, projection }`.
- **Algorithm:** arcball orbit about cursor-picked f64 pivot (supplied from async depth/ID sample on pointer down, fallback to bounds centroid); pan in view plane scaled by depth at cursor; zoom toward cursor; interactive corner orientation cube reflecting active kernel DRF axes with clickable faces/edges/corners and Ortho/Persp toggle (`research/viz-ux-sota/UX-INTERACTION-SPEC.md`); end cameras from bounds sphere; easing cubic, disabled under reduced motion (setting read from `VIZ.SHELL.THEME.SCALE`, passed as data).
- **Errors:** `NoActiveFrame` (falls back to world frame with a diagnostic).
- **Determinism:** D0 for end cameras; gestures are user input.
- **Performance budget:** controller update under 0.1 ms.
- **Crate:** `fid-view` (module `nav`).

## Test fixtures

`fixtures/viz/golden-cube` with a stored rotated frame.

## Build prompt

> In crate `fid-view`, add module `nav` with a `CameraController` for live viewports: arcball orbit automatically pivoting on the 3D surface point under the cursor (fallback to scene/selection centroid), pan, zoom toward cursor, roll; standard orientations from the active kernel frame's axes exposed both via keys and an interactive DRF-aligned corner orientation cube with one-click Orthographic/Perspective toggle (default Orthographic for inspection, per `research/viz-ux-sota/UX-INTERACTION-SPEC.md`); fit-to-view and zoom-to-selection framing the f64 bounding sphere at 90 % of the shorter side; animations ≤ 250 ms, jumps when reduced motion is on (flag passed in as data). Nothing here journals or logs. Register keyboard equivalents as palette actions. End cameras are deterministic f64. Tests AC-1..AC-6 with `fixtures/viz/golden-cube`. Rules: CONSTITUTION §8; REQ-VIZ.VIEW-006 (transient motion only on the view track).

## Open questions

- None.

## Children

_Leaf: none._
