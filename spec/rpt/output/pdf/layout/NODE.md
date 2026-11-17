---
id: RPT.OUTPUT.PDF.LAYOUT
title: Page layout engine with page breaks and continuation headers
level: 4
parent: RPT.OUTPUT.PDF
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.OUTPUT.PDF-002]
covers: [GOM.RPT.06]
depends_on: [RPT.OUTPUT.PDF.TEXT, RPT.MODEL.BIND.GEN]
uses_artifacts: [RPT.DESIGN.PAGE, RPT.DESIGN.PLACE, RPT.DESIGN.STYLE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.OUTPUT.PDF.LAYOUT: Page layout engine with page breaks and continuation headers

## Purpose

Turns a `ResolvedReport` into positioned pages: page size and body area, block placement (designer frames when present, otherwise top-to-bottom flow), text wrapping, table row heights, page breaks inside tables, repetition of header rows and identifying blocks on continuation pages, column groups for wide tables, and page-number fields. Its output is a display list in integer units that the PDF writer draws and that the designer preview (`RPT.DESIGN.PREVIEW`) shows, so preview and export cannot disagree.

## Scope

**In:** `PageLayout` display list (pages of boxes with text runs, table cells, image references, paths); page geometry from the report's page layout (default A4 portrait, 15 mm margins when none is set); flow layout and frame layout; table pagination with `repeat_header_rows` and `continuation_block`; column groups with `repeat_columns`; widow control of at least one row per page; page number and page count fill-in; overflow warnings.

**Out:** shaping and metrics (`RPT.OUTPUT.PDF.TEXT`); painting per block kind (`RPT.OUTPUT.PDF.DRAW`, `RPT.OUTPUT.PDF.CHART`); PDF bytes (`RPT.OUTPUT.PDF.WRITE`); editing frames and page formats (`RPT.DESIGN.PLACE`, `RPT.DESIGN.PAGE`, read here as data).

## Requirements

- **REQ-RPT.OUTPUT.PDF.LAYOUT-001** The layout engine shall compute all positions and sizes in integer µm with no floating point, and the same resolved report, style set and engine version shall give an identical display list on x86-64, ARM64 and wasm32.
- **REQ-RPT.OUTPUT.PDF.LAYOUT-002** When a block has a designer frame, the engine shall place it at that frame; otherwise it shall stack blocks in document order within the body area, starting a new page when the next block does not fit.
- **REQ-RPT.OUTPUT.PDF.LAYOUT-003** When a table does not fit in the remaining body area, the engine shall break it between rows, and on every continuation page shall repeat its first `repeat_header_rows` rows and its `continuation_block`, if any.
- **REQ-RPT.OUTPUT.PDF.LAYOUT-004** When a table has a column-repetition slot with more columns than `max_columns_per_group`, the engine shall lay out successive column groups, each repeating the `repeat_columns`.
- **REQ-RPT.OUTPUT.PDF.LAYOUT-005** If a single table row or unbreakable block is taller than the body area, then the engine shall place it clipped on its own page and report warning `report.block_overflow` naming the block.
- **REQ-RPT.OUTPUT.PDF.LAYOUT-006** After pagination, the engine shall fill every page-number and page-count field so that page n of m is correct on every page.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/golden/bracket_3stage.resolved.json`, when laid out on x86-64, ARM64 and wasm32, then the canonical display lists are byte-identical and equal `fixtures/rpt/output/layout/bracket_3stage.layout.json`.
- **AC-2** Given fixture `fixtures/rpt/forms/fai_500rows` resolved with `tmpl:fai-form3`, when laid out, then every continuation page starts with the header row and the identifying block, and footers read "n of m" correctly.
- **AC-3** Given fixture `fixtures/rpt/forms/ppap_10parts`, when laid out, then two column groups (parts 1-6, 7-10) each repeat balloon, specification and outcome columns.
- **AC-4** Given a text block taller than a page, when laid out, then it is clipped on its own page with warning `report.block_overflow`.
- **AC-5** Given a report with designer frames, when laid out, then each framed block's box equals its frame converted exactly from 0.01 mm to µm.

## Design

- **Data structures:** `PageLayout { pages: Vec<LaidPage { size_um: (i64, i64), boxes: Vec<LaidBox> }> }`; `LaidBox { block: BlockId, rect_um, content: Text(Vec<ShapedLine>) | Cells(..) | Image(Sha256) | Paint(BlockId) }`; trait `Measure { fn measure(&self, block: &ResolvedBlock, width_um: i64, text: &TextEngine) -> Measured; }` with per-kind implementations registered by DRAW and CHART.
- **Algorithm:** greedy pagination in document order; Knuth-Plass is not used (greedy line breaking at UAX 14 opportunities); table rows measured cell by cell; continuation repeat as in REQ-003.
- **Errors:** `report.block_overflow` (warning); `report.layout_invalid` for impossible geometry.
- **Determinism:** D0.
- **Performance budget:** lay out 50 pages with 20 images in under 3 s [unverified until measured].
- **Crate:** `fid-report-pdf` (module `layout`).

## Test fixtures

Golden display lists under `fixtures/rpt/output/layout/`; resolved fixtures from `RPT.MODEL.BIND.GEN` and `RPT.FORMS`. CC-BY-4.0.

## Build prompt

> In crate `fid-report-pdf`, add module `layout`: integer-µm page layout of a `ResolvedReport` into a `PageLayout` display list, shared later with the designer preview. Page geometry from the report's page layout (0.01 mm units, converted exactly) or A4 portrait with 15 mm margins; framed blocks at their frames, others stacked in document order; tables break between rows and repeat `repeat_header_rows` and `continuation_block` on continuation pages; column groups repeat `repeat_columns`; oversized rows or blocks are clipped on their own page with `report.block_overflow`; fill page n of m after pagination. Per-kind measuring through a `Measure` trait. No floating point. Acceptance: AC-1..AC-5 in `spec/rpt/output/pdf/layout/NODE.md`. Constraints: CONSTITUTION §4 determinism, §8.

## Open questions

- Designer data (`RPT.DESIGN.PAGE`, `PLACE`, `STYLE`, P1b) is read from the report document as data; in P1a, without the designer, defaults apply.

## Children

_Leaf: none._
