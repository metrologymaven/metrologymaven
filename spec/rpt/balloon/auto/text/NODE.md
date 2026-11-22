---
id: RPT.BALLOON.AUTO.TEXT
title: Bounded PDF text runs, sheet bounding boxes and display tiles
level: 4
parent: RPT.BALLOON.AUTO
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON.AUTO-001]
covers: []
depends_on: [RPT.BALLOON.SHEET.IMPORT, IO.REG.PLUGIN.DRAW]
side_effect: R1
determinism: D0
cli: [fid report extract-drawing-text]
mcp: [report_extract_drawing_text]
journal: [report.extract_drawing_text@1]
updated: 2026-11-22
---

# RPT.BALLOON.AUTO.TEXT: Bounded PDF text runs, sheet bounding boxes and display tiles

## Purpose

Extracts vector text runs, their exact integer 0.01 mm sheet bounding boxes, and safe display raster tiles from imported 2D PDF drawing sheets (`RPT.BALLOON.SHEET`). This closes the two open gaps noted in `IO.REG.PLUGIN.DRAW` (PDF text extraction and on-screen display rasterisation) in a bounded, fail-closed, deterministic way without executing PDF actions or JavaScript.

## Scope

**In:**
- Command `report.extract_drawing_text@1` (R1, Activity Log only)
- Bounded PDF content-stream interpreter covering text state (`BT`/`ET`, `Tf`, `Tm`, `Td`, `TD`, `T*`, `Tj`, `TJ`, `ToUnicode` CMap) and vector stroke/path bounding boxes for callout boxes/frames
- Coordinate mapping from PDF user space through `/CropBox`/`/MediaBox`, `/Rotate` and `/UserUnit` into top-left origin integer 0.01 mm `SheetRect` bounds
- Typing every extracted string as `UntrustedText` (CONSTITUTION C6) with hard caps on runs per page (50 000) and string bytes (10 MiB per sheet)
- Bounded display-tile rasteriser hook (vector path + glyph tiles at configurable DPI, default 150 dpi, capped at 64 Mpx per sheet) for on-screen ballooning and cropped snippet previews

**Out** (and which node owns it instead):
- Dimension and GD&T grammar parsing (`RPT.BALLOON.AUTO.PARSE`)
- Balloon proposal generation (`RPT.BALLOON.AUTO.PROPOSE`)
- PDF file structure validation and encryption checks (`IO.REG.PLUGIN.DRAW`)

## Requirements

- **REQ-RPT.BALLOON.AUTO.TEXT-001** When `report.extract_drawing_text@1` runs on a vector PDF sheet, the system shall return every visible text run with its `UntrustedText` content, font size in 0.01 mm, rotation in millidegrees and integer 0.01 mm `SheetRect` in sheet coordinates.
- **REQ-RPT.BALLOON.AUTO.TEXT-002** The extractor shall ignore PDF actions, JavaScript, forms and external references, and shall never allocate beyond bounded per-sheet run and memory caps (`limit_exceeded`).
- **REQ-RPT.BALLOON.AUTO.TEXT-003** When a PDF page contains only a raster image and zero vector text runs, the extractor shall return `text_runs: []` with `page_kind: raster_only` so callers can invoke optional OCR candidates.
- **REQ-RPT.BALLOON.AUTO.TEXT-004** Extracting text from the same PDF sheet bytes twice shall return byte-identical canonical JSON and identical `SheetRect` integers (D0).
- **REQ-RPT.BALLOON.AUTO.TEXT-005** The display-tile interface shall render a bounded RGBA8 crop for any requested `SheetRect` at up to 300 dpi for Proposal Inbox snippet review and sheet viewing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/balloon/auto/vector_bracket.pdf` (A3 landscape vector PDF with 12 dimensions and 3 GD&T frames), when `fid report extract-drawing-text --sheet <s1> --json` runs, then all 15 callout runs are extracted with `SheetRect` coordinates within ±5 units (0.05 mm) of the fixture ground truth.
- **AC-2** Given `fixtures/rpt/balloon/auto/scanned_only.pdf` (raster-only page), then `page_kind` equals `raster_only` and `text_runs` is empty.
- **AC-3** (error path) Given a hostile PDF with 100 000 synthetic text runs or nested stream bombs, then extraction fails closed with `limit_exceeded` and zero panics.
- **AC-4** Given two runs on `vector_bracket.pdf`, then the canonical JSON outputs are byte-identical.

## Design

- **Registry command:** `report.extract_drawing_text@1(sheet: SheetId, include_frame_boxes?: bool) -> DrawingTextMap` (R1, MCP exposed, Activity Log only).
- **Data structures:** `SheetRect { x0: i64, y0: i64, x1: i64, y1: i64 }` (0.01 mm, top-left origin); `TextRun { id: u32, text: UntrustedText, bbox: SheetRect, font_h_cmm: i64, angle_mdeg: i32 }`; `DrawingTextMap { sheet: SheetId, page_kind: Vector | Mixed | RasterOnly, runs: Vec<TextRun>, frame_boxes: Vec<SheetRect>, warnings: Vec<String> }`.
- **Algorithm:** walk decoded PDF content streams with bounded operator count and stack depth; resolve CMap/`ToUnicode` and standard PDF encodings to UTF-8; transform glyph origin and advance vectors by CTM × text matrix into top-left 0.01 mm integers with single half-even rounding; sort runs deterministically by `(y0, x0, id)`.
- **Errors:** `report.not_found`, `report.limit_exceeded { limit }`, `report.pdf_malformed { at }`.
- **Determinism:** D0.
- **Performance budget:** 20-page vector engineering drawing extracted in < 1 s [unverified].
- **Crate:** `fid-report` (module `balloon::auto_text`).

## Test fixtures

`fixtures/rpt/balloon/auto/vector_bracket.pdf` and `scanned_only.pdf`: generated by a committed Python script (CC-BY-4.0); hostile seeds from `IO.CORP`.

## Build prompt

> In crate `fid-report`, add module `balloon::auto_text` implementing `report.extract_drawing_text@1` (R1, `--json`, MCP `report_extract_drawing_text`). Parse PDF content streams in bounded pure Rust (no JS/action execution, hard cap 50 000 runs and 10 MiB text per sheet) to emit `TextRun`s (`UntrustedText`, integer 0.01 mm `SheetRect` top-left origin, font height, rotation) plus optional callout frame boxes, classifying pages as `Vector | Mixed | RasterOnly`. Provide a bounded snippet/tile raster crop helper (up to 300 dpi, ≤ 64 Mpx/sheet) for Proposal Inbox previews and sheet display. Deterministic D0 output sorted by `(y0, x0, id)`. Tests AC-1..AC-4. Rules: CONSTITUTION §8 (no panic, no `unsafe`, no `unwrap`/`expect`), C6 untrusted text, no std `HashMap`.

## Open questions

- None blocking.

## Children

_Leaf: none._
