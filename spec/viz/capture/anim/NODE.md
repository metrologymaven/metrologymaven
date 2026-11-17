---
id: VIZ.CAPTURE.ANIM
title: Turntable and camera-path animations
level: 3
parent: VIZ.CAPTURE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE-010]
covers: [GOM.VIZ.08]
depends_on: [VIZ.CAPTURE.SHOT, VIZ.VIEW.STATE]
side_effect: W2
determinism: D1
cli: [fid capture render-animation]
mcp: [capture_render_animation]
journal: [capture.render_animation@1]
updated: 2026-11-16
---

# VIZ.CAPTURE.ANIM: Turntable and camera-path animations

## Purpose

Scripted camera animations rendered headlessly: turntables around an axis and view-to-view flights through a list of NamedViews. The camera path is computed deterministically in f64 from its parameters, so rendering twice gives the same frames; the output is a content-addressed frame sequence with a manifest, which VIDEO can encode.

## Scope

**In:** `capture.render_animation@1`, turntable and NamedView-path specifications, deterministic camera interpolation, headless frame rendering, frame-sequence storage and manifest.
**Out:** video encoding (`VIZ.CAPTURE.VIDEO`); live viewport animation and reduced-motion handling (`VIZ.VIEW.CAMERA`); NamedViews (`VIZ.VIEW.NAMED`).

## Requirements

- **REQ-VIZ.CAPTURE.ANIM-001** A turntable shall be specified by axis (f64 point and direction), total angle, frame count and resolution, and the camera for frame i shall be the start camera rotated by i × angle ÷ (frame count) about the axis, computed in f64.
- **REQ-VIZ.CAPTURE.ANIM-002** A camera path shall be specified by an ordered list of NamedViews and frames per segment, with camera positions interpolated linearly and orientations by spherical linear interpolation in f64.
- **REQ-VIZ.CAPTURE.ANIM-003** When an animation is rendered twice from the same inputs on the same adapter, the frame digests shall be identical; across adapters they shall match within the golden-image tolerance.
- **REQ-VIZ.CAPTURE.ANIM-004** The command shall store frames as a content-addressed sequence with a manifest listing every frame digest and the camera of each frame, and shall journal the manifest digest.
- **REQ-VIZ.CAPTURE.ANIM-005** If the frame count exceeds 3 600 or the total pixel count exceeds 8 × 10⁹, then the command shall fail with `AnimationTooLarge` and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube`, when `fid capture render-animation --turntable --axis 0,0,0:0,0,1 --angle 360 --frames 36 --width 320 --height 240 --json` runs, then 36 frames are stored and the manifest lists 36 digests and cameras.
- **AC-2** Given AC-1 run twice, then the manifest digests are identical.
- **AC-3** Given frame 9 of AC-1, then its camera equals the start camera rotated by exactly 90° (f64 comparison within 1e-12 m on position).
- **AC-4** Given a path through NamedViews `front` and `iso` with 10 frames, then the first and last cameras equal the saved cameras exactly.
- **AC-5** (error path) Given `--frames 5000`, then the command fails with `AnimationTooLarge` and journals nothing.

## Design

- **Registry command(s):** `capture.render_animation@1`, W2. Parameters: `spec: Turntable { axis_point_m, axis_dir, angle_deg: Decimal, frames, start_view } | Path { views: [NamedViewId], frames_per_segment }`, `width`, `height`, `background`. Result: `{ manifest_digest, frame_count }`.
- **Algorithm:** generate cameras in f64 (rotation by Rodrigues' formula; slerp for orientations); render each frame through the screenshot pipeline without per-frame journal entries; store frames; journal one entry for the manifest.
- **Errors:** `AnimationTooLarge`, `UnknownView`, `InvalidAxis`, `PolicyForbidden`.
- **Determinism:** D1 (camera generation is D0; pixels identical on the same adapter and driver).
- **Performance budget:** 36 frames at 1920×1080 under 30 s on the reference profile [unverified].
- **Crate:** `fid-capture` (module `anim`).

## Test fixtures

`fixtures/viz/golden-cube` with NamedViews `front` and `iso`; in-repo.

## Build prompt

> In crate `fid-capture`, add module `anim` and registry command `capture.render_animation@1` (W2; CLI `fid capture render-animation`, MCP `capture_render_animation`). Support a turntable (f64 axis point and direction, decimal total angle, frame count, start view) and a path through NamedViews (linear position, slerp orientation, frames per segment), with cameras computed in f64 without FMA. Render every frame headlessly through the screenshot pipeline, store frames content-addressed, write one manifest (frame digests and per-frame cameras) and journal only the manifest digest. Reject more than 3 600 frames or 8e9 total pixels with `AnimationTooLarge`. Tests: 36-frame turntable; repeat gives identical manifest; frame 9 camera at exactly 90°; path endpoints equal saved cameras; size limit error. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Easing curves for flights are deliberately left out (linear only) to keep the path definition simple and deterministic.

## Children

_Leaf: none._
