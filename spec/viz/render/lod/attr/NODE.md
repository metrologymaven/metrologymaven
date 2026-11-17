---
id: VIZ.RENDER.LOD.ATTR
title: Point attribute modes, eye-dome lighting and excluded style
level: 4
parent: VIZ.RENDER.LOD
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER.LOD-007]
covers: []
depends_on: [VIZ.RENDER.LOD.RASTER]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.RENDER.LOD.ATTR: Point attribute modes, eye-dome lighting and excluded style

## Purpose

Colour points by what the user wants to see (RGB, intensity, deviation bin, scan pass, timestamp, uncertainty or kernel classification) using only kernel-supplied per-point data and LUTs, add eye-dome lighting and normal shading for shape perception, and draw points the kernel excluded in a distinct desaturated style without hiding them.

## Scope

**In:** attribute-mode selection per cloud node; LUT lookup by kernel-supplied bin index; eye-dome lighting pass; normal shading; excluded-class style.
**Out:** LUT content and legends (`VIZ.OVERLAY.CMAP`, `VIZ.OVERLAY.LEGEND`); hiding classes (`VIZ.VIEW.VISIB`); computing bins or classes (kernel, `SCAN`).

## Requirements

- **REQ-VIZ.RENDER.LOD.ATTR-001** The renderer shall support attribute modes RGB, intensity, deviation bin, scan pass, timestamp, uncertainty and classification, and every mode shall map kernel-supplied per-point values through a LUT or linear ramp without computing a bin on the GPU.
- **REQ-VIZ.RENDER.LOD.ATTR-002** In deviation-bin mode, the colour of a point shall equal the LUT entry of the kernel-supplied bin index exactly.
- **REQ-VIZ.RENDER.LOD.ATTR-003** Points the kernel classified as excluded shall be drawn desaturated with the excluded style carried as data in the node's style token (values from `VIZ.OVERLAY.STATUS`), and shall be hidden only by an explicit visibility rule for that class.
- **REQ-VIZ.RENDER.LOD.ATTR-004** Eye-dome lighting shall be toggleable per viewport and shall modify only luminance, never the hue of a LUT colour.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/points-bins` (1 000 points with kernel bin indices 0..9) in deviation-bin mode, when the pixel-probe harness samples each point with eye-dome lighting off, then every pixel equals its LUT entry exactly.
- **AC-2** Given 100 points flagged excluded, then they are drawn and their sampled colour equals the excluded style, and none is missing from the ID buffer.
- **AC-3** Given eye-dome lighting on, then sampled hues (HSV) of LUT colours differ from the off case by less than 1 degree.
- **AC-4** Given a cloud with no attribute for the chosen mode, then the renderer falls back to RGB or a neutral colour and records a `ModeUnavailable` diagnostic.

## Design

- **Registry command(s):** none; the mode is part of the cloud's display style set through overlay or view commands.
- **Data structures:** `AttrMode` enum; per-mode uniform block; LUT texture handle supplied by `VIZ.OVERLAY`.
- **Algorithm:** fragment stage reads the per-point attribute stream and indexes the LUT with the integer bin index (no arithmetic on values); EDL as a post-pass on depth, multiplying luminance.
- **Errors:** `ModeUnavailable` (diagnostic).
- **Determinism:** N (GPU pixels); exactness of LUT colours checked by REQ-002.
- **Performance budget:** EDL pass under 1 ms at 1920 × 1080 on the reference profile [unverified].
- **Crate:** `fid-render` (module `lod::attr`).

## Test fixtures

`fixtures/viz/points-bins`: generated in-repo, CC0.

## Build prompt

> In crate `fid-render`, add module `lod::attr`: attribute modes (RGB, intensity, deviation bin, scan pass, timestamp, uncertainty, classification) mapping kernel-supplied per-point data through LUTs or ramps; in deviation-bin mode the colour is the LUT entry of the kernel bin index, exactly; excluded points use the excluded style from the overlay status mapping and stay drawn and pickable; eye-dome lighting post-pass modifies luminance only. Tests AC-1..AC-4 with `fixtures/viz/points-bins` and the pixel-probe harness. Rules: CONSTITUTION §8; C2 (no bin, class or value computed on the GPU).

## Open questions

- `VIZ.OVERLAY.STATUS` depends on the whole `VIZ.RENDER`, so this leaf takes the excluded style as data in the scene node's style token instead of a code edge.

## Children

_Leaf: none._
