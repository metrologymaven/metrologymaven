---
id: RPT.OUTPUT.SHEET.XLSX
title: Deterministic spreadsheet workbook export
level: 4
parent: RPT.OUTPUT.SHEET
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.OUTPUT.SHEET-001, REQ-RPT.OUTPUT.SHEET-003, REQ-RPT.OUTPUT.SHEET-005, REQ-RPT.OUTPUT.SHEET-007]
covers: [PCD.RPT.04, PW.RPT.04, SA.RPT.03]
depends_on: [RPT.OUTPUT.SHEET.CELL, RPT.OUTPUT.META]
side_effect: W1
fs_effect: write_root
determinism: D0
cli: [fid report export-spreadsheet]
mcp: [report_export_spreadsheet]
journal: [report.export_spreadsheet@1]
built_in: [fid-report-out]
updated: 2026-11-23
---

# RPT.OUTPUT.SHEET.XLSX: Deterministic spreadsheet workbook export

## Purpose

Writes a resolved report's table blocks as an Office Open XML spreadsheet (ISO/IEC 29500 [unverified edition]): one worksheet per table block in block order and one metadata worksheet with the `ExportMeta` record. Numeric cells are real numbers with a number format showing exactly the formatter's decimals, so the shown digits equal the PDF. The ZIP container and XML are written deterministically.

## Scope

**In:** `report.export_spreadsheet@1` (W1, `fs_effect: write_root`); minimal workbook parts (content types, workbook, worksheets, styles with one number format per decimal count, shared strings or inline strings); sheet naming; fixed ZIP entry order, fixed timestamps (1980-01-01 00:00) and pinned deflate settings; dry run.

**Out:** cell model (`RPT.OUTPUT.SHEET.CELL`); export-root writes (`RPT.OUTPUT.META`); charts and images in workbooks (not exported).

## Requirements

- **REQ-RPT.OUTPUT.SHEET.XLSX-001** The workbook shall contain one worksheet per table block in block order, named `T01`, `T02`, ... or the block caption truncated to 31 characters with forbidden sheet-name characters replaced, plus a final worksheet `meta` holding the canonical `ExportMeta` with renderer `xlsx`.
- **REQ-RPT.OUTPUT.SHEET.XLSX-002** Each numeric cell shall store the canonical decimal string as its value and use a number format with exactly the cell's decimals, so a spreadsheet application shows the same digits as the PDF.
- **REQ-RPT.OUTPUT.SHEET.XLSX-003** Exporting the same resolved report twice with the same exporter version shall give byte-identical files: ZIP entries in a fixed order, every entry timestamp 1980-01-01 00:00, no extra fields, pinned deflate settings, and no creation time or author in document properties.
- **REQ-RPT.OUTPUT.SHEET.XLSX-004** The command shall be side-effect class W1 with `fs_effect: write_root`, write through the export sink and journal one event with the receipt.
- **REQ-RPT.OUTPUT.SHEET.XLSX-005** If a table exceeds 1 048 576 rows or 16 384 columns [unverified format limits], then the export shall fail with error `report.unsupported_block` naming the block and write nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/golden/bracket_3stage.resolved.json`, when `fid report export-spreadsheet --resolved <d> --root <h> --name bracket.xlsx --json` runs twice with overwrite, then both files are byte-identical and equal the golden SHA-256, and one event per run is journaled.
- **AC-2** Given AC-1's file, when opened by an independent reader (for example the openpyxl library [unverified availability in CI]), then it has sheets `T01`, `T02`, `meta`; numeric cells are numbers whose formats show the formatter's decimals; injection cells start with an apostrophe.
- **AC-3** (cross-check) Given AC-1 and the PDF of `RPT.OUTPUT.PDF.WRITE` AC-1, then for every numeric cell the value formatted with its number format equals the canonical form of the PDF text.
- **AC-4** (error path) Given a synthetic table of 1 048 577 rows, then the export fails with `report.unsupported_block` and writes nothing.
- **AC-5** Given AC-1 on x86-64, ARM64 and wasm32, then the bytes are identical.

## Design

- **Registry command:** `report.export_spreadsheet@1(resolved: Sha256 | ReportId, root: ExportRootHandle, name: ExportName, overwrite: bool, dry_run: bool, units: AuthoredOrSi) -> ExportResult` (W1, `write_root`).
- **Algorithm:** build parts as canonical XML strings (fixed attribute order, inline strings to avoid a shared-string order dependency) -> ZIP with fixed entry order and timestamps -> sink.
- **Errors:** `report.unsupported_block`, `report.units_mismatch`, sink errors.
- **Determinism:** D0.
- **Performance budget:** 100 000 cells in under 1 s [unverified until measured].
- **Crate:** `fid-report-out` (module `sheet::xlsx`).

## Test fixtures

Golden resolved reports, golden workbook digest under `fixtures/rpt/output/sheet/`; CC-BY-4.0.

## Build prompt

> In crate `fid-report-out`, add module `sheet::xlsx` and register `report.export_spreadsheet@1` (W1, `fs_effect: write_root`; CLI `fid report export-spreadsheet --json`, dry run, `--units authored|si`). Build a minimal Office Open XML workbook from the cell model: one worksheet per table block (`T01`... or sanitised caption, max 31 chars) plus `meta` with the canonical `ExportMeta` (renderer `xlsx`); numeric cells store the canonical decimal and a number format with exactly its decimals; inline strings; deterministic ZIP (fixed entry order, 1980-01-01 timestamps, no extra fields, pinned deflate, no author or dates in properties). Too many rows or columns -> `report.unsupported_block`. Write through the export sink; journal one event with the receipt. Acceptance: AC-1..AC-5 in `spec/rpt/output/sheet/xlsx/NODE.md`. Constraints: CONSTITUTION §3.1, C6, §8; any ZIP or XML library must pass the D-001 allowlist and AC-5.

## Open questions

- Spreadsheet applications show separators by their own locale settings; equality with the PDF is checked on digits in canonical form.

## Children

_Leaf: none._
