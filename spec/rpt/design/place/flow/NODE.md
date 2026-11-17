---
id: RPT.DESIGN.PLACE.FLOW
title: Overflow detection and page-break flow
level: 4
parent: RPT.DESIGN.PLACE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN.PLACE-004, REQ-RPT.DESIGN.PLACE-005, REQ-RPT.DESIGN.PLACE-006]
covers: []
depends_on: [RPT.DESIGN.PLACE.FRAME, RPT.DESIGN.PAGE]
side_effect: W2
determinism: D0
cli: [fid report set-break-block-layout]
mcp: [report_set_break_block_layout]
journal: [report.set_break_block_layout@1]
updated: 2026-11-17
---

# RPT.DESIGN.PLACE.FLOW: Overflow detection and page-break flow

## Purpose

Two things decide whether a laid-out report is complete: no block may silently leave the printable body, and tables or repeated page groups that are taller than their frame must continue on following pages instead of being cut. This leaf offers the overflow check (used by the designer, preview and generation) and the pagination function that splits a growing block into continuation fragments according to its page-break rule. Row heights depend on fonts, so pagination takes them through a `RowMeasure` trait that the renderer implements; this leaf never measures text itself.

## Scope

**In:** `check_overflow(report, layouts, frames) -> Vec<Overflow>` producing warning `report.block_overflow`; page-break rules per block (`continue_on_next_page`, `new_page_before`, `keep_together`, `repeat_header_rows: u32`); `report.set_break_block_layout@1`; `paginate(report, layouts, frames, measure) -> PagePlan`; the `RowMeasure` trait (height in 0.01 mm of each row or block part, given the block and its frame width).

**Out:** page geometry (`RPT.DESIGN.PAGE`); frames (`RPT.DESIGN.PLACE.FRAME`); text shaping and measuring (`RPT.OUTPUT.PDF`, which implements `RowMeasure`); rendering of continuation pages (`RPT.OUTPUT.PDF`, `RPT.DESIGN.PREVIEW`).

## Requirements

- **REQ-RPT.DESIGN.PLACE.FLOW-001** The overflow check shall report every block whose frame extends outside its page's body rectangle as warning `report.block_overflow` naming the block ID, page ID and the overflow in 0.01 mm per side.
- **REQ-RPT.DESIGN.PLACE.FLOW-002** When a block with rule `continue_on_next_page` has content taller than its frame, pagination shall place the remaining rows in a continuation fragment at the top of the body of the following page (adding pages as needed), repeating the configured number of header rows.
- **REQ-RPT.DESIGN.PLACE.FLOW-003** Pagination shall place every row exactly once and shall never drop a row; if a single row is taller than a whole body, then it shall report warning `report.row_too_tall` and place the row clipped on its own page.
- **REQ-RPT.DESIGN.PLACE.FLOW-004** A block with rule `keep_together` that does not fit its remaining space shall move whole to the next page, and if it does not fit an empty body, then pagination shall report `report.block_overflow`.
- **REQ-RPT.DESIGN.PLACE.FLOW-005** `report.set_break_block_layout@1` shall be one journaled W2 command and shall change only the block's break rule.
- **REQ-RPT.DESIGN.PLACE.FLOW-006** Pagination shall be a pure function of the report, layouts, frames and measured heights, so that the same inputs give the same `PagePlan`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/design/overflow_report` (a block whose frame ends 250 units below the body on page 1), when the overflow check runs (for example through `fid report preview-layout --json`), then exactly one `report.block_overflow` names that block with bottom overflow 250.
- **AC-2** Given fixture `fixtures/rpt/design/long_table` (a 120-row table whose rows 1–2 are header rows, constant row height 500 units by a stub `RowMeasure`, 2 header rows repeated, frame at the body top with height 15000 units on a body of 24700 units), when paginated, then the plan has fragments on 3 pages holding rows 1–30, 31–77 and 78–120, the continuation fragments each start with repeated rows 1–2, and every row appears once.
- **AC-3** Given a `keep_together` block of 30000 units on an A4 body, then pagination reports `report.block_overflow` for it.
- **AC-4** (error path) Given `set-break-block-layout --repeat-header-rows -1`, then it fails with `report.layout_invalid` and journals nothing.
- **AC-5** Given the same inputs twice, the two `PagePlan` canonical encodings are byte-identical.

## Design

- **Registry command:** `report.set_break_block_layout@1(report: ReportId, block: BlockId, rule: BreakRule) -> BreakRule` (W2, `--json`).
- **Trait:** `RowMeasure { fn rows(&self, block: &Block, frame_width: i64) -> Result<Vec<i64>, MeasureError>; }`, implemented by the renderer's layout engine (and by stubs in tests).
- **Data structures:** `BreakRule { mode: Continue | NewPageBefore | KeepTogether, repeat_header_rows: u32 }`; `PagePlan { pages: Vec<PlannedPage { source_page, fragments: Vec<Fragment { block, rows: Range<u32>, frame }> }> }`.
- **Algorithm:** walk pages in order; for each block in z-order, fit rows greedily into the remaining body height; continuation fragments start at the body top with header rows; all heights `i64`.
- **Errors:** `report.layout_invalid`, `report.not_found`; warnings `report.block_overflow`, `report.row_too_tall`.
- **Determinism:** D0 given measured heights; measurement determinism belongs to the renderer.
- **Performance budget:** paginate a 20-page report with 5 000 table rows in under 50 ms [unverified until measured].
- **Crate:** `fid-report` (module `layout::flow`).

## Test fixtures

- `fixtures/rpt/design/overflow_report`, `fixtures/rpt/design/long_table`: built by a test builder; stub `RowMeasure` with constant heights; CC-BY-4.0.

## Build prompt

> In crate `fid-report`, module `layout::flow`, implement: (1) `check_overflow` returning warning `report.block_overflow { block, page, per-side overflow }` for every frame outside its page body; (2) break rules per block (`continue_on_next_page`, `new_page_before`, `keep_together`, `repeat_header_rows`) set by `report.set_break_block_layout@1` (W2, one journal event, `--json`; CLI `fid report set-break-block-layout`; MCP name derived); (3) a pure `paginate(report, layouts, frames, measure) -> PagePlan` that places every row exactly once, continues overflowing blocks at the top of following bodies with repeated header rows, moves `keep_together` blocks whole, and warns `report.row_too_tall` for rows taller than a body. Row heights come only from a `RowMeasure` trait implemented by the renderer (stub in tests). Integer arithmetic, ordered collections. Tests AC-1 to AC-5. CONSTITUTION §8 applies. Spec: `spec/rpt/design/place/flow/NODE.md`.

## Open questions

- **Cross-domain:** `RPT.OUTPUT.PDF` (open) must implement `RowMeasure` from its text shaping, with deterministic heights, and use the same `PagePlan` for export; this is the measurement contract the parent asked for.

## Children

_Leaf: none._
