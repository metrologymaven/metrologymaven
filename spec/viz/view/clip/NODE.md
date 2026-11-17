---
id: VIZ.VIEW.CLIP
title: Clipping planes and boxes
level: 3
parent: VIZ.VIEW
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW-005, REQ-VIZ.VIEW-007]
covers: [GOM.VIZ.02]
depends_on: [VIZ.VIEW.STATE, CORE]
side_effect: W2
determinism: D0
cli: [fid view set-clip]
mcp: [view_set_clip]
journal: [view.set_clip@1]
updated: 2026-11-16
---

# VIZ.VIEW.CLIP: Clipping planes and boxes

## Purpose

Display clipping: add, change and remove clip planes and clip boxes in a viewport's ViewState through one journaled command. Display clips are view settings with no metrology meaning; kernel-computed sections are a different thing owned by SCAN.

## Scope

**In:** `view.set_clip@1`, clip-plane and clip-box state in ViewState, validation, limits.
**Out:** clip rasterization and caps (`VIZ.RENDER.SHADE`); metrology sections (`SCAN.SECT`); interactive gizmo widgets (`VIZ.SHELL`).

## Requirements

- **REQ-VIZ.VIEW.CLIP-001** `view.set_clip@1` shall add, replace or remove a clip plane (f64 point and unit normal in metres) or a clip box (f64 centre, half-extents and orientation) in a target viewport's ViewState.
- **REQ-VIZ.VIEW.CLIP-002** The system shall support at least 6 simultaneous clip planes per viewport and shall reject a seventh only if the renderer tier reports fewer [unverified].
- **REQ-VIZ.VIEW.CLIP-003** Every successful `view.set_clip@1` shall be journaled with the resulting ViewState digest.
- **REQ-VIZ.VIEW.CLIP-004** If a plane normal is zero-length or non-finite, then the command shall fail with a typed error and journal nothing.
- **REQ-VIZ.VIEW.CLIP-005** Clip state shall never be labelled or exported as a section result.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube`, when `fid view set-clip --viewport main --add-plane --point 0,0,0 --normal 0,0,1 --json` runs, then the result's ViewState contains that plane and its digest matches the journal entry.
- **AC-2** Given six planes added one by one, then all six are present in the ViewState in canonical order.
- **AC-3** (replay) Given the journal from AC-2, when replayed in exact mode, then the outcome is `IDENTICAL` and the final ViewState digest matches.
- **AC-4** (error path) Given `--normal 0,0,0`, then the command fails with `InvalidNormal` and journals nothing.
- **AC-5** Given a clip box and a headless render, then the rendered image matches golden `cube-clipbox.png` within the golden tolerance mask.

## Design

- **Registry command(s):** `view.set_clip@1`, W2. Parameters: `viewport: ViewportId`, `op: Add | Replace { id } | Remove { id } | Clear`, `shape: Plane { point_m: [f64;3], normal: [f64;3] } | Box { centre_m, half_m, rotation: [f64;4] }`, `enabled: bool`. Result: `{ clip_id, view_state_digest }`. Normals are normalised in f64 before storage.
- **Data structures:** `ClipPlane`, `ClipBox` in `ViewState.clips`.
- **Algorithm:** validate, normalise, update the ViewState, recompute digest.
- **Errors:** `InvalidNormal`, `NonFiniteNumber`, `UnknownClip`, `LimitExceeded`, `UnknownViewport`.
- **Determinism:** D0.
- **Performance budget:** command under 5 ms.
- **Crate:** `fid-view` (module `clip`).

## Test fixtures

`fixtures/viz/golden-cube` (shared with VIZ.RENDER.HEADLESS) plus golden `cube-clipbox.png`; in-repo.

## Build prompt

> In crate `fid-view`, add module `clip` and registry command `view.set_clip@1` (W2; CLI `fid view set-clip`, MCP `view_set_clip`). Operations Add, Replace, Remove, Clear on clip planes (f64 point and normal, normalised in f64) and clip boxes (f64 centre, half-extents, quaternion) in a viewport's `ViewState`; at least 6 planes; journal the resulting ViewState digest. Reject zero or non-finite normals (`InvalidNormal`, `NonFiniteNumber`) without journaling. Display clips carry no metrology meaning. Tests: add plane; six planes in canonical order; exact replay `IDENTICAL`; invalid normal error; headless golden image with a clip box. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Whether agent-issued clip changes go to the agent branch (W2 rule) or are presentation-only is CORE's call; this leaf follows the W2 default.

## Children

_Leaf: none._
