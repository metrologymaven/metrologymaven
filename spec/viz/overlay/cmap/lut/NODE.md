---
id: VIZ.OVERLAY.CMAP.LUT
title: Bin-index LUT pass and continuous mode
level: 4
parent: VIZ.OVERLAY.CMAP
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY.CMAP-001, REQ-VIZ.OVERLAY.CMAP-002]
covers: []
depends_on: [VIZ.OVERLAY.CMAP.REBIN, VIZ.RENDER.FRAME, VIZ.RENDER.ORACLE]
uses_artifacts: [UX.TOKENS.DATAPAL]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.OVERLAY.CMAP.LUT: Bin-index LUT pass and continuous mode

## Purpose

The GPU side of colour maps, which only looks up colours. In stepped mode each vertex or point carries the kernel's bin index (u8 or u16) and the shader reads the colour from a LUT built from the palette; no shader compares a value with an edge. In continuous mode the colour is interpolated from f32 display copies of the values, but whether a point is under or over range is taken only from its kernel bin index, and such points are drawn clamped with a hatch pattern. A pixel-probe test against the CPU oracle proves every probed pixel equals the LUT entry of the kernel bin.

## Scope

**In:** LUT texture build (data bins, below, above, no-data) from palette files; per-vertex bin-index buffer upload; stepped and continuous shaders registered in the frame graph's shader manifest; hatch for out-of-range; the no-data colour; pixel-probe test.
**Out:** bins (`VIZ.OVERLAY.CMAP.REBIN`, `SCAN.CMP.BAND`); legend (`VIZ.OVERLAY.LEGEND`); frame graph (`VIZ.RENDER.FRAME`).

## Requirements

- **REQ-VIZ.OVERLAY.CMAP.LUT-001** In stepped mode, the colour of each vertex or point shall be the LUT entry at its kernel-supplied bin index, and no shader in the pass shall read deviation values.
- **REQ-VIZ.OVERLAY.CMAP.LUT-002** In continuous mode, points whose kernel bin index is below-range or above-range shall be drawn with the end colour and the hatch pattern, regardless of their f32 display value.
- **REQ-VIZ.OVERLAY.CMAP.LUT-003** No-data points (kernel no-data bin) shall be drawn in the palette's neutral no-data colour.
- **REQ-VIZ.OVERLAY.CMAP.LUT-004** On the probe set of fixture points, every probed pixel's colour shall equal the LUT entry for that point's kernel bin within the oracle tolerance, in both the GPU path and the CPU oracle.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/devfield-10k` with `cmap-5bin` bins from the kernel, when rendered headlessly and probed at 500 known point pixels (`VIZ.RENDER.ORACLE` probe API), then every pixel equals the LUT entry of the point's bin.
- **AC-2** Given the shader manifest, then the stepped pass's inputs contain the bin buffer and LUT and no value buffer.
- **AC-3** Given continuous mode and points whose f32 copy lies inside range but whose kernel bin is above-range (edge-rounding case in the fixture), then they are drawn hatched as above-range.
- **AC-4** Given no-data points, then they show the no-data colour.

## Design

- **Registry command(s):** none.
- **Data structures:** `LutTexture { colors: [Rgba8; n_bins + 3] }`; `BinBuffer { u8 | u16 per vertex }`.
- **Algorithm:** build LUT from the palette file named in the spec; upload bins when REBIN swaps; stepped shader: `color = lut[bin]`; continuous: `color = sample(palette, (v - lo)/(hi - lo))` only when bin is a data bin, else end colour plus hatch by screen-space pattern.
- **Errors:** `PaletteLoad` (falls back to the updating state).
- **Determinism:** D1 (same adapter); correctness proven by the probe test, not by bit-equality across GPUs.
- **Performance budget:** < 0.5 ms per frame for 10⁸ displayed points beyond the base point pass [unverified].
- **Crate:** `fid-overlay` (module `cmap::lut`), shaders registered with `fid-render`.

## Test fixtures

`fixtures/viz/devfield-10k` (shared) with a probe list and an edge-rounding subset.

## Build prompt

> In crate `fid-overlay`, add module `cmap::lut`: build a LUT texture from the palette file in `palette-catalogue@1` (data bins plus below, above, no-data), upload the kernel bin-index buffer from `cmap::rebin`, and register two shaders in the frame graph's manifest: stepped (`lut[bin]`, no access to values) and continuous (interpolate f32 display copies only for data bins; below/above by kernel bin → end colour plus hatch; no-data colour). Add the pixel-probe test against the CPU oracle. Tests AC-1..AC-4. Rules: GPU for display only, bins from the kernel only (C2, REQ-VIZ-002); no panics; `unsafe` none; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- None blocking.

## Children

_Leaf: none._
