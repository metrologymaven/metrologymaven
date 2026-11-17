---
id: RPT.DESIGN.PREVIEW.RASTER
title: CPU page rasteriser for preview
level: 4
parent: RPT.DESIGN.PREVIEW
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN.PREVIEW-002]
covers: []
depends_on: [RPT.OUTPUT.PDF, RPT.DESIGN.STYLE.FONT]
side_effect: R1
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.DESIGN.PREVIEW.RASTER: CPU page rasteriser for preview

## Purpose

Turns a page display list (the output of the shared page layout engine) into an RGBA image at a requested resolution, for the designer canvas and for headless PNG previews. It runs on the CPU so the same code works in desktop, headless and WASM builds and can be tested pixel for pixel; showing the image on screen (through wgpu) is the canvas's job. Nothing it produces is a value of record (CONSTITUTION C2).

## Scope

**In:** rasterising display-list primitives (filled and stroked paths, text runs from shaped glyph outlines, images including drawing sheets and view snapshots, clip rectangles) with anti-aliasing; resolution 36–600 dpi; image size limit 100 Mpx per page; a missing-content placeholder for vector PDF drawing sheets that have no raster.

**Out:** layout and text shaping (`RPT.OUTPUT.PDF`); font resolution (`RPT.DESIGN.STYLE.FONT`); display on screen (`RPT.DESIGN.UI.CANVAS`); PNG encoding and commands (`RPT.DESIGN.PREVIEW.HEADLESS`).

## Requirements

- **REQ-RPT.DESIGN.PREVIEW.RASTER-001** The rasteriser shall render every primitive of a display list at the position given by the layout engine, so that each block falls on the same page and at the same place as in the PDF export.
- **REQ-RPT.DESIGN.PREVIEW.RASTER-002** For a fixed display list and dpi, the output pixels shall be bit-identical across runs on the same platform.
- **REQ-RPT.DESIGN.PREVIEW.RASTER-003** If a requested page would exceed 100 Mpx or the dpi is outside 36–600, then the rasteriser shall fail with error `report.preview_too_large` or `report.preview_invalid`.
- **REQ-RPT.DESIGN.PREVIEW.RASTER-004** A drawing sheet without a raster image shall be drawn as a labelled placeholder with its bounds and balloons, never left blank.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/design/raster_primitives` (one display list with each primitive kind), when rendered at 96 dpi, then the result matches `raster_primitives_96.png` within 1 grey level per channel on the reference platform and bit-exactly on repeat runs.
- **AC-2** Given the display list of `twenty_page_report` page 3 and its PDF export rendered by an external reference renderer in CI, then the bounding boxes of all blocks agree within 1 px at 96 dpi.
- **AC-3** (error path) Given an A0 page at 600 dpi, then `report.preview_too_large`.
- **AC-4** Given a vector PDF drawing sheet without raster, then a placeholder with the sheet bounds and balloon glyphs is drawn.

## Design

- **Registry commands:** none (library).
- **API:** `rasterise(page: &PageDisplayList, dpi: u32) -> Result<RgbaImage, RasterError>`.
- **Algorithm:** scanline coverage rasterisation of paths in fixed-point (24.8), glyph outlines from the shaped runs, bilinear image sampling; no SIMD dispatch, no GPU.
- **Errors:** `report.preview_too_large`, `report.preview_invalid`.
- **Determinism:** D1 (per-platform bit-exact; fixed-point coverage makes D0 likely but is not claimed).
- **Performance budget:** an A4 page at 96 dpi in under 20 ms [unverified until measured].
- **Crate:** `fid-report-raster` (a pure-Rust rasteriser from the D-001 allowlist may be used [unverified which]).

## Test fixtures

- `fixtures/rpt/design/raster_primitives` and its reference PNG, generated in-project; CC-BY-4.0.

## Build prompt

> Create crate `fid-report-raster` with `rasterise(page_display_list, dpi) -> RgbaImage`: CPU-only, fixed-point scanline coverage rasterisation of filled and stroked paths, glyph outlines from shaped text runs, images (bilinear) and clip rectangles; dpi 36–600 (`report.preview_invalid`), at most 100 Mpx per page (`report.preview_too_large`); drawing sheets without raster drawn as a labelled placeholder with bounds and balloons. Output must be bit-identical across runs on one platform. No GPU, no runtime SIMD dispatch. Tests AC-1 to AC-4. CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`) and C2 (display only). Spec: `spec/rpt/design/preview/raster/NODE.md`.

## Open questions

- Resolved from the parent: preview pages are rasterised on the CPU (deterministic, works headless and in WASM); wgpu only displays the finished image. Confirm with `RPT.OUTPUT.PDF` owners when its layout engine is specified.

## Children

_Leaf: none._
