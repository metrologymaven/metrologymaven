---
id: VIZ.OVERLAY.WHISK
title: Whiskers, flags and vector plots
level: 3
parent: VIZ.OVERLAY
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY-004, REQ-VIZ.OVERLAY-010]
covers: [SA.VIZ.03]
depends_on: [VIZ.RENDER.FRAME, VIZ.OVERLAY.CMAP, VIZ.OVERLAY.STATUS, SCAN.SECT.WHISK, SCAN.CMP.CPT, KRN.NUM.FMT.REG]
cli: [fid view set-deviation-glyphs]
mcp: [view_set_deviation_glyphs]
journal: [view.set_deviation_glyphs@*]
updated: 2026-11-17
---

# VIZ.OVERLAY.WHISK: Whiskers, flags and vector plots

## Purpose

Directional deviation displays: whiskers along sections and comparison points, flags and callouts with kernel-formatted values, and colour vector plots (arrows) for point groups and network residuals. Direction and magnitude always come from kernel-supplied f64 vectors; the display amplification factor is shown and is display-only.

## Scope

**In:**
- Whisker sets for sections and comparison points
- Flags and callouts (nominal, actual, deviation, uncertainty, status) with kernel text
- Colour vector plots and arrows for point and vector groups
- Display amplification factor and its legend entry

**Out** (and which node owns it instead):
- Computing deviations and vectors (`SCAN`, `LVM`)
- General labels and layout (`VIZ.OVERLAY.LABEL`)
- Colour LUTs (`VIZ.OVERLAY.CMAP`)

## Requirements

- **REQ-VIZ.OVERLAY.WHISK-001** Whisker and arrow directions and magnitudes shall be drawn from kernel-supplied f64 vectors, converted to f32 offsets only for display.
- **REQ-VIZ.OVERLAY.WHISK-002** The display amplification factor shall be shown in the legend whenever whiskers or vectors are visible.
- **REQ-VIZ.OVERLAY.WHISK-003** Every number in a flag or callout shall come from the kernel formatter.
- **REQ-VIZ.OVERLAY.WHISK-004** Whisker and vector colours shall use kernel bin indices from the active ColorMapSpec.
- **REQ-VIZ.OVERLAY.WHISK-005** Changing the amplification factor shall not change any value shown in flags, callouts or the legend except the factor itself.

## Interfaces

- **Offers:** whisker, flag and vector overlay passes; amplification setting carried in the ColorMapSpec or LabelSet.
- **Consumes:** SCAN section and comparison results; LVM residual vectors; `KRN.NUM`; `VIZ.OVERLAY.CMAP` LUT.

## Decomposition hints

WHISKER, FLAG, VECTOR, AMPLIFY.

## Open questions

- Tagged P1b with scan sections; network residual vectors follow LVM (P3, outside launch-1) and plug in as another `VectorGroup` producer without a dependency now [unverified].
- The amplification factor is stored in a `DeviationGlyphStyle` element per view key and changed only by `view.set_deviation_glyphs@1` (`VIZ.OVERLAY.WHISK.WHISKER`).
- Whisker colours need kernel bins for section-comparison values; `SCAN.CMP.BAND` must confirm that a section-comparison element is a valid field for `compare.surface_bin@1` [cross-domain].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.OVERLAY.WHISK.WHISKER | Section whiskers, flags and amplification (leaf) | 001 002 003 004 005 | none |
| VIZ.OVERLAY.WHISK.VECTOR | Colour vector plots for point groups (leaf) | 001 002 004 | SA.VIZ.03 |
