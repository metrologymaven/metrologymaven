---
id: VIZ.OVERLAY
title: Colour maps, legends and annotations
level: 2
parent: VIZ
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-VIZ-002]
covers: [GOM.VIZ.04, GOM.VIZ.05, PW.VIZ.03, PW.VIZ.04, SA.VIZ.03]
depends_on: [VIZ.RENDER, SCAN.CMP, GDT.CHAR, CORE]
cli: [fid view set-colormap, fid view set-legend, fid view add-label, fid view edit-label, fid view remove-label, fid view layout-labels]
mcp: [view_set_colormap, view_set_legend, view_add_label, view_edit_label, view_remove_label, view_layout_labels]
journal: [view.set_colormap@*, view.set_legend@*, view.add_label@*, view.edit_label@*, view.remove_label@*, view.layout_labels@*]
updated: 2026-11-16
---

# VIZ.OVERLAY: Colour maps, legends and annotations

## Purpose

Everything drawn on top of geometry that carries a number or a verdict: deviation colour maps and their legends, the colour-scale editor, colour vector plots, whiskers and flags, labels and leader lines, GD&T annotation glyphs and status markers. It is separate because it holds the one hard rule of the viewer: what the user reads must equal what the kernel computed. The kernel (via SCAN and GDT) assigns each value to its bin; this node only styles and lays out those kernel outputs.

## Scope

**In:**
- ColorMapSpec model: range, steps, palette, out-of-range handling, tolerance bands; created and changed only by commands
- Colour-scale editor UI
- Legends: bins, counts and percentages read from kernel output; legend placement
- Colour vector plots and deviation arrows for point and vector groups
- Whiskers and flags for sections and comparison points
- Labels (deviation, dimension, characteristic) with leader lines, kernel-formatted text, and automatic and manual label layout
- GD&T annotation rendering (feature control frames, datum flags) from the characteristic model
- Status markers (conforms, nonconforms, indeterminate, not decidable, not evaluated, stale) with locked palettes and non-colour cues
- Provenance badges on displayed numbers (computed, measured, entered, reported by source, suggested)

**Out** (and which node owns it instead):
- Computing deviations and assigning kernel bins (`SCAN`, `GDT`)
- GPU drawing primitives and text rasterization (`VIZ.RENDER`)
- Saving a colour map into a named view (`VIZ.VIEW`)
- Report pages and tables (`RPT`)
- Theme tokens and the locked status palette definition in the theme system (`VIZ.SHELL`)

## Requirements

- **REQ-VIZ.OVERLAY-001** The colour bin of every displayed deviation shall be the bin index supplied by the kernel, never one computed by a shader or the UI.
- **REQ-VIZ.OVERLAY-002** Legend bin boundaries, counts and percentages shall equal the kernel's values for the same ColorMapSpec and data digest, byte for byte in their canonical form.
- **REQ-VIZ.OVERLAY-003** When a ColorMapSpec is changed, the system shall request re-binning from the kernel and shall not display the new map until the kernel result is available.
- **REQ-VIZ.OVERLAY-004** Every number shown in a label, legend or whisker shall be produced by the kernel-owned formatter from an f64 value, never from an f32 display copy.
- **REQ-VIZ.OVERLAY-005** Status shall never be conveyed by colour alone: every status marker shall also carry an icon or pattern and a text equivalent.
- **REQ-VIZ.OVERLAY-006** No user palette, theme or white-label skin shall be able to render an out-of-tolerance bin in a colour from the conforming set of the locked status palette.
- **REQ-VIZ.OVERLAY-007** When automatic label layout runs on a view with up to 500 labels, the system shall produce a layout with no overlapping label boxes within 200 ms on the reference profile [unverified].
- **REQ-VIZ.OVERLAY-008** Every displayed number shall carry a provenance badge (computed, measured, entered, reported by source, or suggested).
- **REQ-VIZ.OVERLAY-009** Colour-map, legend and label changes shall be registry commands, journaled, and reproducible from the journal.
- **REQ-VIZ.OVERLAY-010** Vector plots shall draw direction and magnitude from kernel-supplied f64 vectors, with the scale factor shown in the legend.

## Interfaces

- **Offers:** `view.set_colormap@1`, `view.set_legend@1`, `view.add_label@1`, `view.edit_label@1`, `view.remove_label@1`, `view.layout_labels@1` (exact side-effect classes set at leaf level; label and map changes on the working document are W2); ColorMapSpec and LabelSet types consumed by VIZ.VIEW and RPT.
- **Consumes:** SCAN deviation fields and kernel bins; GDT characteristics and ResultRecords; the kernel formatter (KRN/CORE); VIZ.RENDER draw API; theme tokens from VIZ.SHELL.

## Decomposition hints

CMAP (ColorMapSpec, kernel re-binning round trip), LEGEND, SCALEED (colour-scale editor), LABEL (labels, leader lines, layout), WHISK (whiskers, flags), VECTOR (vector plots), GDTGLYPH (FCF and datum rendering), STATUS (status markers, provenance badges).

## Split rationale

The dividing principle is **one kind of kernel output per child**, so each child carries its own "display equals kernel" test:
- CMAP owns the colour-map entity and the bin-index round trip; LEGEND draws the kernel's legend object; SCALEED is the only UI-heavy child and depends on the shell;
- LABEL, WHISK and GDTGLYPH are the three kinds of anchored annotation (text labels, directional vectors, GD&T frames), each with different inputs (formatted values, f64 vectors, semantic characteristics);
- STATUS is the shared, pure status and provenance mapping every other child and the kiosk reuse.

Alternatives considered:
- **CMAP and LEGEND together.** Rejected: the legend is a small, finishable leaf with its own byte-equality test, while the re-binning round trip is not.
- **STATUS folded into the shell theme.** Rejected: the locked-palette rule is a value-display rule (REQ-VIZ.OVERLAY-006) and must stay toolkit-independent.

## Open questions

- Which node owns the kernel-side binning command (SCAN or a shared KRN utility)? This node assumes SCAN supplies bins for surface comparison and GDT for characteristic-level zones.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.OVERLAY.CMAP | ColorMapSpec and kernel re-binning round trip | 001 003 009 | none |
| VIZ.OVERLAY.LEGEND | Kernel legend display and placement (leaf) | 002 009 | PW.VIZ.03 |
| VIZ.OVERLAY.SCALEED | Colour-scale editor | 003 009 | GOM.VIZ.04 |
| VIZ.OVERLAY.LABEL | Labels, leader lines and label layout | 004 007 009 | GOM.VIZ.05, PW.VIZ.04 |
| VIZ.OVERLAY.WHISK | Whiskers, flags and vector plots | 004 010 | SA.VIZ.03 |
| VIZ.OVERLAY.GDTGLYPH | GD&T annotation rendering | 004 005 | PW.VIZ.04 |
| VIZ.OVERLAY.STATUS | Status markers, locked palette guard and provenance badges (leaf) | 005 006 008 | none |
