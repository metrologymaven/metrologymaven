---
id: RPT.BALLOON.AUTO
title: Automated 2D PDF callout extraction, balloon proposals and quick pack export
level: 3
parent: RPT.BALLOON
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.BALLOON-001, REQ-RPT.BALLOON-002, REQ-RPT.BALLOON-009, REQ-RPT.BALLOON-010, REQ-RPT.BALLOON-011]
covers: []
depends_on: [RPT.BALLOON.SHEET, RPT.BALLOON.REG, RPT.BALLOON.CHAR, RPT.FORMS.ROWS, IO.REG.PLUGIN.DRAW, CORE.JRN.BR]
cli: [fid report extract-drawing-text, fid report propose-balloons, fid report export-balloon-pack]
mcp: [report_extract_drawing_text, report_propose_balloons, report_export_balloon_pack]
journal: [report.extract_drawing_text@1, report.propose_balloons@1, report.export_balloon_pack@1]
updated: 2026-11-22
---

# RPT.BALLOON.AUTO: Automated 2D PDF callout extraction, balloon proposals and quick pack export

## Purpose

Delivers the high-velocity Day-1 shop-floor wedge for 2D engineering drawings (`research/pdf-ballooning/DESIGN.md`): extract bounded text runs, bounding boxes and display raster tiles from PDF/raster sheets; deterministically parse engineering dimensions, tolerances and GD&T feature control frames into exact authored-decimal candidates; propose numbered balloons and characteristics onto a draft/agent branch for batched human H2 review in the Proposal Inbox (with optional local/MCP vision-OCR input for scanned drawings); and export a ready-to-use ballooned PDF plus AS9102 Form 3 starter spreadsheet in one command.

## Scope

**In:**
- Bounded PDF vector text-run extraction with integer 0.01 mm sheet bounding boxes and safe display-tile rasterisation for on-screen ballooning
- Deterministic local callout parser for linear/diameter/radius/angle dimensions, symmetric/bilateral/limit tolerances, basic dimensions, surface finish and GD&T feature control frames with exact authored decimal strings
- Optional external/local vision-OCR candidate ingestion (treated as untrusted data, C6) for scanned raster-only drawings
- `report.propose_balloons@1` (W1 on draft/agent branch) producing a batched Proposal Inbox queue with cropped sheet snippets, bounding-box provenance, drawing zone lookup and confidence words
- `report.export_balloon_pack@1` (W1 local export root) producing a vector-overlaid ballooned PDF and AS9102 Form 3 starter CSV/spreadsheet

**Out** (and which node owns it instead):
- Balloon register rules and renumbering (`RPT.BALLOON.REG`)
- Atomic working-document creation of characteristics and balloons (`RPT.BALLOON.CHAR`, invoked upon human adoption)
- Final W3 AS9102 form sign-off (`RPT.FORMS.FINAL`)
- Running LLMs inside the deterministic kernel (forbidden by C1; vision OCR is an optional external/MCP candidate source whose output is validated and adopted by a human)

## Requirements

- **REQ-RPT.BALLOON.AUTO-001** The extractor shall read PDF vector text runs and their sheet bounding boxes in integer 0.01 mm units without executing PDF actions or JavaScript, typing every extracted string as untrusted text (CONSTITUTION C6), and shall render bounded display raster tiles for on-screen sheet viewing.
- **REQ-RPT.BALLOON.AUTO-002** The callout parser shall deterministically parse linear, diameter, radius, angle, tolerance (±, bilateral, limit) and GD&T feature control frame callouts into exact authored decimal strings and typed characteristic candidates without binary floating-point rounding.
- **REQ-RPT.BALLOON.AUTO-003** `report.propose_balloons@1` shall write proposed balloons and characteristics only to a draft or agent branch (W1), attaching sheet bounding-box provenance, raw untrusted source text and a confidence word to every item, and shall never commit directly to the working document without human H2 adoption.
- **REQ-RPT.BALLOON.AUTO-004** Scanned raster sheets without embedded vector text shall accept externally supplied OCR candidate boxes (e.g., from a local or MCP vision model) through the same untrusted validation and human-adoption path.
- **REQ-RPT.BALLOON.AUTO-005** `report.export_balloon_pack@1` shall write a vector-overlaid ballooned PDF and an AS9102 Form 3 starter CSV into an approved local export root in ascending balloon-number order, failing closed on hostile paths or unapproved roots.

## Interfaces

- **Offers:** `report.extract_drawing_text@1` (R1), `report.propose_balloons@1` (W1), `report.export_balloon_pack@1` (W1, `fs_effect: write_root`); types `TextRun`, `SheetRect`, `ParsedCallout`, `BalloonProposal`.
- **Consumes:** `IO.REG.PLUGIN.DRAW`, `RPT.BALLOON.SHEET`, `RPT.BALLOON.REG`, `RPT.BALLOON.CHAR`, `RPT.FORMS.ROWS`, `CORE.JRN.BR`, `UX.REPORT.BALLOON` (as artifact).

## Decomposition hints

TEXT (PDF vector text + bbox + display tiles), PARSE (deterministic dimension & GD&T callout grammar), PROPOSE (draft-branch proposal command + OCR candidate merge + H2 batch queue), EXPORT (one-command ballooned PDF + Form 3 CSV pack).

## Split rationale

The split follows the **four clean data stages from raw PDF bytes to shop-floor deliverable**: `TEXT` isolates bounded untrusted PDF parsing and display tiling (solving the two open gaps noted in `IO.REG.PLUGIN.DRAW`); `PARSE` is a pure, deterministic D0 callout grammar with zero I/O; `PROPOSE` owns the W1 agent/draft-branch proposal lifecycle, optional OCR candidate ingestion and Proposal Inbox hand-off; `EXPORT` delivers the immediate Day-1 ballooned PDF + Form 3 starter pack so shops get value on first run.

## Open questions

- None blocking; all four children are buildable P1a leaves.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.BALLOON.AUTO.TEXT | Bounded PDF text runs, sheet bounding boxes and display tiles (leaf) | 001 | none |
| RPT.BALLOON.AUTO.PARSE | Deterministic dimension and GD&T callout parser (leaf) | 002 | none |
| RPT.BALLOON.AUTO.PROPOSE | Batched balloon and characteristic proposal command (leaf) | 003 004 | none |
| RPT.BALLOON.AUTO.EXPORT | Quick ballooned-PDF and AS9102 Form 3 starter pack export (leaf) | 005 | none |
