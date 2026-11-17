---
id: RPT.DESIGN.PREVIEW
title: Live preview
level: 3
parent: RPT.DESIGN
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN-004]
covers: []
depends_on: [RPT.MODEL.BIND, RPT.DESIGN.PLACE, RPT.DESIGN.STYLE, RPT.OUTPUT.PDF, CORE.STO.CACHE]
cli: [fid report preview-layout]
mcp: [report_preview_layout]
journal: [report.preview_layout@1]
updated: 2026-11-17
---

# RPT.DESIGN.PREVIEW: Live preview

## Purpose

Shows the resolved report as it will print, while the user edits layout and style. It regenerates incrementally after each edit, using the same generation and layout path as export so that what you see is what is exported. Preview is a scratch computation and journals nothing.

## Scope

**In:**
- Incremental regeneration of only the affected pages after a layout or style edit
- Page rasterisation for the designer canvas, from the same page layout used by PDF output
- Preview against the current journal head or a pinned head
- Headless preview to PNG for scripts and agents

**Out** (and which node owns it instead):
- Binding and generation (`RPT.MODEL.BIND`)
- PDF rendering (`RPT.OUTPUT.PDF`)
- Canvas interaction (`RPT.DESIGN.UI`)

## Requirements

- **REQ-RPT.DESIGN.PREVIEW-001** When the user edits a layout or style, the preview shall update within 500 ms for a 20-page report on reference hardware [unverified].
- **REQ-RPT.DESIGN.PREVIEW-002** The preview shall use the same page layout result as PDF output, so that every block falls on the same page in both.
- **REQ-RPT.DESIGN.PREVIEW-003** Preview shall be side-effect class R1 and journal nothing.
- **REQ-RPT.DESIGN.PREVIEW-004** When `report.preview_layout@1` runs headless, the system shall return one PNG per requested page as a scratch artifact handle.
- **REQ-RPT.DESIGN.PREVIEW-005** If a regeneration error occurs during preview, then the preview shall keep the last good pages and show the error with its code.

## Interfaces

- **Offers:** `report.preview_layout@1` (R1); a preview surface for `RPT.DESIGN.UI`.
- **Consumes:** generation (`RPT.MODEL.BIND`), frames (`RPT.DESIGN.PLACE`), styles (`RPT.DESIGN.STYLE`), the page layout engine shared with `RPT.OUTPUT.PDF`.

## Decomposition hints

INCR (incremental regeneration), RASTER (page rasterisation), HEADLESS (PNG preview command).

## Open questions

- Resolved at leaf level: pages are rasterised on the CPU (`RPT.DESIGN.PREVIEW.RASTER`) from display lists of the page layout engine shared with `RPT.OUTPUT.PDF`; wgpu only displays the image. **Cross-domain within RPT:** `RPT.OUTPUT.PDF` (open) must expose that engine as page display lists.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.DESIGN.PREVIEW.INCR | Incremental preview regeneration (leaf) | 001 002 005 | none |
| RPT.DESIGN.PREVIEW.RASTER | CPU page rasteriser for preview (leaf) | 002 | none |
| RPT.DESIGN.PREVIEW.HEADLESS | Headless PNG preview command (leaf) | 003 004 | none |
