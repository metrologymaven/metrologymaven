---
id: VIZ.OVERLAY.WHISK.VECTOR
title: Colour vector plots for point groups
level: 4
parent: VIZ.OVERLAY.WHISK
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY.WHISK-001, REQ-VIZ.OVERLAY.WHISK-002, REQ-VIZ.OVERLAY.WHISK-004]
covers: [SA.VIZ.03]
depends_on: [VIZ.OVERLAY.WHISK.WHISKER, SCAN.CMP.CPT, GDT.CHAR.RECORD, VIZ.RENDER.FRAME]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.OVERLAY.WHISK.VECTOR: Colour vector plots for point groups

## Purpose

Draws deviation arrows for groups of points: each arrow starts at the nominal point and points along the kernel-supplied f64 deviation vector, scaled by the shared amplification factor of `DeviationGlyphStyle`, and is coloured by the kernel bin of its value. In P1b the sources are comparison-point groups (`SCAN.CMP.CPT`: deviation along the point's direction); any producer that supplies a `VectorGroup` (positions and f64 vectors with bin indices) can be drawn, which is how network residuals (LVM, P3) will plug in later.

## Scope

**In:** the `VectorGroup` input type; extraction from comparison-point ResultRecords (value × unit direction); arrow geometry (shaft and head sized in screen space, length in model space × amplification); colour from kernel bins; legend scale entry ("arrow length ×a"); visibility per group.
**Out:** amplification setting and its command (`VIZ.OVERLAY.WHISK.WHISKER`); computing vectors (`SCAN`, later `LVM`); binning (`SCAN.CMP.BAND`).

## Requirements

- **REQ-VIZ.OVERLAY.WHISK.VECTOR-001** Each arrow shall start at the point's nominal position and end at nominal + amplification × the kernel's f64 deviation vector, computed in f64 and converted to f32 only as a camera-relative display offset.
- **REQ-VIZ.OVERLAY.WHISK.VECTOR-002** Arrow colour shall be the LUT entry of the kernel bin of the point's value, and points without a value (NO_MATCH, NOT_DECIDABLE without value) shall be drawn as a hollow marker with no arrow.
- **REQ-VIZ.OVERLAY.WHISK.VECTOR-003** While any vector plot is visible, the legend shall show the amplification factor.
- **REQ-VIZ.OVERLAY.WHISK.VECTOR-004** The module shall accept any `VectorGroup` whose vectors are supplied as f64 by a kernel-side producer, and shall compute no vector itself beyond scaling.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/whisk/cpgroup200` (200 evaluated comparison points with known vectors), when rendered with amplification 20, then each arrow end projects to the f64 reference end point within display tolerance and colours equal LUT entries of kernel bins.
- **AC-2** Given 5 points with NO_MATCH, then they show hollow markers without arrows.
- **AC-3** Given vectors visible, then the legend shows "×20".

## Design

- **Registry command(s):** none; style via `view.set_deviation_glyphs@1`.
- **Data structures:** `VectorGroup { id, points: [{ entity, nominal_m: [f64; 3], vector_m?: [f64; 3], bin?: u16 }] }`.
- **Algorithm:** build groups from ResultRecords (value × stored unit direction, both f64 from the record); instance-draw arrows; screen-space head size.
- **Errors:** `GroupEmpty` (nothing drawn).
- **Determinism:** D0 for geometry.
- **Performance budget:** 100,000 arrows at 60 fps [unverified].
- **Crate:** `fid-overlay` (module `whisk::vector`).

## Test fixtures

`fixtures/viz/whisk/cpgroup200`: generated through SCAN commands on a synthetic part; CC-BY-4.0.

## Build prompt

> In crate `fid-overlay`, add module `whisk::vector`: draw arrows for `VectorGroup`s (nominal position plus kernel-supplied f64 vector, optional kernel bin) built from comparison-point ResultRecords (value × stored direction); end = nominal + a·vector in f64 with the shared amplification `a`, camera-relative f32 for display; colour by LUT[kernel bin]; hollow markers for points without a value; legend shows the factor. Compute no vector beyond scaling. Tests AC-1..AC-3. Rules: values from the kernel only (C1, C2); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Network residual vectors (LVM, P3, outside launch-1) will be added as another `VectorGroup` producer; no dependency on LVM is declared now.

## Children

_Leaf: none._
