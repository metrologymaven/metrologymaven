---
id: RPT.OUTPUT.SHEET
title: Spreadsheet and CSV export of report tables
level: 3
parent: RPT.OUTPUT
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.OUTPUT-004, REQ-RPT.OUTPUT-005, REQ-RPT.OUTPUT-008]
covers: [PCD.RPT.04, PW.RPT.04, SA.RPT.03]
depends_on: [RPT.OUTPUT.META, RPT.MODEL.BIND]
cli: [fid report export-spreadsheet, fid report export-csv]
mcp: [report_export_spreadsheet, report_export_csv]
journal: [report.export_spreadsheet@1, report.export_csv@1]
updated: 2026-11-17
---

# RPT.OUTPUT.SHEET: Spreadsheet and CSV export of report tables

## Purpose

Exports the table blocks of a resolved report as a spreadsheet workbook (one sheet per table block plus a metadata sheet) and as CSV files, so customers can post-process results. Numbers are written as numbers with the unit in the column header, and the displayed text matches the PDF exactly because both come from the same formatter output.

## Scope

**In:**
- Workbook export (Office Open XML spreadsheet): one sheet per table block, one metadata sheet
- CSV export per table block with a stated dialect (UTF-8, comma, RFC 4180 quoting)
- Numeric cells as numbers in the authored unit (or SI on request) with the unit in the header; formatted text kept alongside where the format allows
- Formula-injection protection for text cells
- Deterministic bytes for the same input

**Out** (and which node owns it instead):
- Report-independent results export with column maps (`IO.RES`)
- SPC system exports (`RPT.OUTPUT.SPC`)
- Metadata record and export-root writes (`RPT.OUTPUT.META`)

## Requirements

- **REQ-RPT.OUTPUT.SHEET-001** The system shall export each table block to one spreadsheet sheet and one CSV file, in block order, plus a metadata sheet holding the `RPT.OUTPUT.META` record.
- **REQ-RPT.OUTPUT.SHEET-002** Numeric values shall be written as numeric cells in the authored unit, or in SI when requested, with the unit stated in the column header.
- **REQ-RPT.OUTPUT.SHEET-003** The displayed text of every numeric cell shall equal the formatter output used in the PDF for the same value.
- **REQ-RPT.OUTPUT.SHEET-004** If a text cell begins with `=`, `+`, `-`, `@`, tab or carriage return, then the exporter shall neutralise it so that no spreadsheet application evaluates it as a formula (CONSTITUTION C6).
- **REQ-RPT.OUTPUT.SHEET-005** Exporting the same resolved report twice with the same exporter version shall give byte-identical files.
- **REQ-RPT.OUTPUT.SHEET-006** If a report has no table blocks, or a table block contains a cell type the format cannot hold, then the export shall fail with error `report.unsupported_block` naming the block and write no file.
- **REQ-RPT.OUTPUT.SHEET-007** Spreadsheet and CSV exports shall be side-effect class W1 with `fs_effect: write_root` and write through the export sink.

## Interfaces

- **Offers:** `report.export_spreadsheet@1`, `report.export_csv@1` (W1, `write_root`).
- **Consumes:** `ResolvedReport` (`RPT.MODEL.BIND`), export envelope (`RPT.OUTPUT.META`).

## Decomposition hints

CSV (CSV writer and dialect), XLSX (deterministic workbook writer), SAFE (formula-injection neutralisation).

## Open questions

- Should the numeric cell carry the full-precision f64 or the formatted decimal? Assumed the formatted decimal value, so the cell equals the shown text; full precision belongs to QIF and IO.RES exports.
- **Applied at split (2026-11-17):** numeric cells carry the formatted canonical decimal. SI output is produced by generating the resolved report with an SI unit override (`RPT.MODEL.BIND.GEN`); the exporters never convert values.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.OUTPUT.SHEET.CELL | Table-to-cell model, unit headers and formula-injection neutralisation (leaf) | 002 003 004 006 | PCD.RPT.04 |
| RPT.OUTPUT.SHEET.CSV | CSV export of report tables (leaf) | 001 005 007 | PCD.RPT.04, SA.RPT.03 |
| RPT.OUTPUT.SHEET.XLSX | Deterministic spreadsheet workbook export (leaf) | 001 003 005 007 | PCD.RPT.04, PW.RPT.04, SA.RPT.03 |
