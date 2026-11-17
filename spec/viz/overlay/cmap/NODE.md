---
id: VIZ.OVERLAY.CMAP
title: ColorMapSpec and kernel re-binning round trip
level: 3
parent: VIZ.OVERLAY
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY-001, REQ-VIZ.OVERLAY-003, REQ-VIZ.OVERLAY-009]
covers: []
depends_on: [VIZ.RENDER.FRAME, VIZ.RENDER.ORACLE, SCAN.CMP.BAND, KRN.NUM.KDEC, CORE.JRN, CORE.REG]
cli: [fid view set-colormap]
mcp: [view_set_colormap]
journal: [view.set_colormap@*]
updated: 2026-11-17
---

# VIZ.OVERLAY.CMAP: ColorMapSpec and kernel re-binning round trip

## Purpose

The ColorMapSpec document entity (mode, display unit, exact-decimal bin edges, palette ID, out-of-range handling, informative uncertainty bands) and the `view.set_colormap` command that creates or changes it; the round trip that asks the kernel to re-bin and holds the old map with an "updating" badge until the new bins arrive; and the GPU LUT lookup that turns a kernel u8 bin index into a colour.

## Scope

**In:**
- ColorMapSpec model and canonical form; edges stored as exact decimal strings in SI
- `view.set_colormap@1` (W2) and its validation (monotonic edges, known palette, unit)
- Re-binning request to the kernel and the pending/updating state
- LUT upload and bin-index lookup in shaders (stepped mode); continuous mode interpolation from f32 display copies with under/over range taken from kernel bins
- Palette catalogue references (stepped tolerance, CVD-safe stepped, cividis, viridis)

**Out** (and which node owns it instead):
- Classifying values into bins (`SCAN.CMP`; characteristic zones `GDT`)
- Drawing the legend (`VIZ.OVERLAY.LEGEND`)
- The editing UI (`VIZ.OVERLAY.SCALEED`)
- Locked status palette rules (`VIZ.OVERLAY.STATUS`)
- Saving a map into a NamedView (`VIZ.VIEW`)

## Requirements

- **REQ-VIZ.OVERLAY.CMAP-001** The colour of every displayed deviation in stepped mode shall be the LUT entry for the kernel-supplied bin index of that point or vertex.
- **REQ-VIZ.OVERLAY.CMAP-002** In continuous mode, under-range and over-range points shall be identified only by kernel bin indices and drawn clamped with a hatch pattern.
- **REQ-VIZ.OVERLAY.CMAP-003** ColorMapSpec bin edges shall be stored as exact decimal strings with an SI unit, and `view.set_colormap@1` shall reject non-monotonic edges with a typed error and journal nothing.
- **REQ-VIZ.OVERLAY.CMAP-004** When a ColorMapSpec changes, the system shall request re-binning from the kernel and shall keep the previous map on screen with an "updating" badge until the kernel result for the new spec digest arrives.
- **REQ-VIZ.OVERLAY.CMAP-005** Creating or changing a ColorMapSpec shall be performed only by `view.set_colormap@1` and journaled with the spec digest.
- **REQ-VIZ.OVERLAY.CMAP-006** When the journal is replayed, the ColorMapSpec digests shall equal those recorded at the original commits.

## Interfaces

- **Offers:** `ColorMapSpec` type; `view.set_colormap@1` (W2); LUT pass for `VIZ.RENDER`.
- **Consumes:** SCAN.CMP bin classification and its result digest; CORE command bus and journal.

## Decomposition hints

SPEC (model, canonical form, validation; likely a leaf), CMD (`view.set_colormap`), REBIN (round trip and updating state), LUT (shader lookup and continuous mode).

## Open questions

- Tagged P1b: deviation colour maps arrive with scan comparison (SCAN.CMP, P1b) [unverified].
- Settled boundary: colour binning is `SCAN.CMP.BAND` (`compare.surface_bin@1`, R1); this module maps its ColorMapSpec to a `BandSet` and only looks up colours. Palette values are the UX artifact `palette-catalogue@1` (`UX.TOKENS.DATAPAL`), read through `uses_artifacts`.
- GDT characteristic-level zones are not binned through this path; if a characteristic colour map is needed, a second kernel source must be named.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.OVERLAY.CMAP.SPEC | ColorMapSpec model and the set-colormap command (leaf) | 003 005 006 | none |
| VIZ.OVERLAY.CMAP.REBIN | Kernel re-binning round trip and updating state (leaf) | 004 | none |
| VIZ.OVERLAY.CMAP.LUT | Bin-index LUT pass and continuous mode (leaf) | 001 002 | none |
