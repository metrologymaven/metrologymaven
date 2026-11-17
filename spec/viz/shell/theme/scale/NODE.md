---
id: VIZ.SHELL.THEME.SCALE
title: UI scale, target sizes and reduced motion
level: 4
parent: VIZ.SHELL.THEME
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.THEME-001, REQ-VIZ.SHELL.THEME-002]
covers: []
depends_on: [VIZ.SHELL.THEME.TOKENS, CORE.STO.SET]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.THEME.SCALE: UI scale, target sizes and reduced motion

## Purpose

Let users scale the UI from 75 % to 200 % in 25 % steps on top of the OS scale factor, enforce the theme's minimum interactive target size, and provide a reduced-motion flag (from a setting, defaulting to the OS preference) that every camera and panel animation reads.

## Scope

**In:** settings `viz.shell.ui_scale` and `viz.shell.reduced_motion`; effective scale = OS factor × user factor; target-size enforcement in `PanelUi` helpers; `MotionPolicy` published to animation users (camera controller, dock, palette).
**Out:** camera behaviour under reduced motion (`VIZ.VIEW.CAMERA.NAV`, which reads the flag); token values for sizes and durations (`UX.TOKENS`); per-platform scaling support (`PLAT`).

## Requirements

- **REQ-VIZ.SHELL.THEME.SCALE-001** The setting `viz.shell.ui_scale` shall accept 75, 100, 125, 150, 175 and 200 percent, reject other values with a validation error, and apply within one frame.
- **REQ-VIZ.SHELL.THEME.SCALE-002** At every scale, interactive widgets created through `PanelUi` helpers shall be at least the active theme's minimum target size in logical pixels.
- **REQ-VIZ.SHELL.THEME.SCALE-003** The setting `viz.shell.reduced_motion` (on, off, system) shall default to system, and when effective on, every animation duration from motion tokens shall be zero.
- **REQ-VIZ.SHELL.THEME.SCALE-004** Changes to either setting shall go only through `settings.set_value@1`.

## Acceptance criteria

- **AC-1** Given the test app, when scale is set to each allowed value, then a reference button's physical size equals its logical size × OS factor × user factor within 1 px.
- **AC-2** (error path) Given `fid settings set-value viz.shell.ui_scale 110`, then validation fails and nothing changes.
- **AC-3** Given reduced motion on, when a panel opens and a fit-to-view runs, then both complete in the next frame.
- **AC-4** Given the shop-floor theme, then every interactive node in the inspection tree is at least the shop-floor minimum target size.

## Design

- **Registry command(s):** none; settings declared via the settings macro.
- **Data structures:** `UiScale(u8)`; `MotionPolicy { reduced: bool }` passed as data to animation users.
- **Errors:** settings validation errors from `CORE.STO.SET`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-shell` (module `theme::scale`).

## Test fixtures

Test app; fake OS scale factors 1.0, 1.5, 2.0.

## Build prompt

> In crate `fid-shell`, add `theme::scale`: declare settings `viz.shell.ui_scale` (75–200 % in 25 % steps) and `viz.shell.reduced_motion` (on/off/system, default system); effective scale = OS factor × user factor applied within a frame; `PanelUi` helpers enforce the theme's minimum target size; publish `MotionPolicy` so animations use zero duration when reduced motion is on. Changes only via `settings.set_value@1`. Tests AC-1..AC-4. Rules: CONSTITUTION §8.

## Open questions

- None.

## Children

_Leaf: none._
