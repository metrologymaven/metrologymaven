---
id: VIZ.OVERLAY.WHISK
title: Whiskers, flags and vector plots
level: 3
parent: VIZ.OVERLAY
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY-004, REQ-VIZ.OVERLAY-010]
covers: [SA.VIZ.03]
depends_on: [VIZ.RENDER, VIZ.OVERLAY.CMAP, KRN.NUM]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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

- Tagged P1b with scan sections; vector plots for networks follow LVM (P3) [unverified].

## Children

_None yet: status `open`._
