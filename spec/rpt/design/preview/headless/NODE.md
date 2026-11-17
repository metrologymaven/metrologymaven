---
id: RPT.DESIGN.PREVIEW.HEADLESS
title: Headless PNG preview command
level: 4
parent: RPT.DESIGN.PREVIEW
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN.PREVIEW-003, REQ-RPT.DESIGN.PREVIEW-004]
covers: []
depends_on: [RPT.DESIGN.PREVIEW.INCR, RPT.DESIGN.PREVIEW.RASTER, CORE.STO.CACHE]
side_effect: R1
determinism: D1
cli: [fid report preview-layout]
mcp: [report_preview_layout]
journal: [report.preview_layout@1]
updated: 2026-11-17
---

# RPT.DESIGN.PREVIEW.HEADLESS: Headless PNG preview command

## Purpose

Lets scripts and agents see what a report will look like without the GUI: `report.preview_layout@1` generates the requested pages at a head, rasterises them and returns one PNG per page as scratch artifact handles, plus the layout warnings (overflow, too-tall rows). It is R1: it computes into scratch and journals nothing, so agents can use it freely to check their layout proposals.

## Scope

**In:** the command; page selection; dpi (default 96); PNG encoding (deterministic settings); scratch artifact handles in the evictable cache; returning warnings and display-list digests.

**Out:** incremental sessions (`RPT.DESIGN.PREVIEW.INCR`); rasterisation (`RPT.DESIGN.PREVIEW.RASTER`); export to files in an export root (`RPT.OUTPUT`).

## Requirements

- **REQ-RPT.DESIGN.PREVIEW.HEADLESS-001** `report.preview_layout@1` shall be side-effect class R1 and journal nothing to the Record Journal.
- **REQ-RPT.DESIGN.PREVIEW.HEADLESS-002** The command shall return one PNG scratch artifact handle per requested page, in page order, together with each page's display-list digest and all layout warnings.
- **REQ-RPT.DESIGN.PREVIEW.HEADLESS-003** If more than 50 pages are requested in one call, then the command shall fail with error `report.preview_too_many_pages`.
- **REQ-RPT.DESIGN.PREVIEW.HEADLESS-004** PNG encoding shall use fixed settings (no timestamps or variable metadata), so that equal pixels give byte-identical files.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/design/overflow_report`, when `fid report preview-layout --report R --pages 1 --json` runs, then one PNG handle, its display-list digest and the `report.block_overflow` warning are returned, and the journal length is unchanged.
- **AC-2** When run twice, the two PNG files are byte-identical on the same platform.
- **AC-3** (error path) Requesting 51 pages fails with `report.preview_too_many_pages`.
- **AC-4** Given an MCP agent principal, the tool is exposed and returns the same handles' content as AC-1.

## Design

- **Registry command:** `report.preview_layout@1(report: ReportId, pages: PageSelection, dpi: Option<u32>, head: Option<HeadRef>) -> { pages: Vec<{ page, png: ScratchHandle, digest }>, warnings: Vec<Warning> }` (R1, `--json`).
- **Algorithm:** one-shot `PreviewSession` pinned to the head → rasterise each page → encode PNG (fixed zlib level, no ancillary chunks) → store in the cache store.
- **Errors:** `report.preview_too_many_pages`, rasteriser errors, generation errors passed through.
- **Determinism:** D1 (follows the rasteriser).
- **Performance budget:** 5 A4 pages at 96 dpi in under 500 ms [unverified until measured].
- **Crate:** `fid-report` (module `preview::headless`).

## Test fixtures

- `fixtures/rpt/design/overflow_report` (shared with `RPT.DESIGN.PLACE.FLOW`).

## Build prompt

> In crate `fid-report`, module `preview::headless`, implement `report.preview_layout@1` (R1, `--json`; CLI `fid report preview-layout`; MCP name derived and exposed to agents): pin a one-shot preview session to the requested head, rasterise the requested pages (at most 50, `report.preview_too_many_pages`; dpi default 96), encode PNGs with fixed settings and no variable metadata, store them as scratch handles in the evictable cache, and return handles, display-list digests and layout warnings in page order. Journal nothing. Tests AC-1 to AC-4. CONSTITUTION §8 applies. Spec: `spec/rpt/design/preview/headless/NODE.md`.

## Open questions

- None yet.

## Children

_Leaf: none._
