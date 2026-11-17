---
id: RPT.OUTPUT.SHEET.CELL
title: Table-to-cell model, unit headers and formula-injection neutralisation
level: 4
parent: RPT.OUTPUT.SHEET
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.OUTPUT.SHEET-002, REQ-RPT.OUTPUT.SHEET-003, REQ-RPT.OUTPUT.SHEET-004, REQ-RPT.OUTPUT.SHEET-006]
covers: [PCD.RPT.04]
depends_on: [RPT.MODEL.BIND.GEN, RPT.MODEL.BLOCKS.TABLE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.OUTPUT.SHEET.CELL: Table-to-cell model, unit headers and formula-injection neutralisation

## Purpose

The format-neutral step shared by the CSV and workbook writers: it turns each resolved `table` block into a grid of typed cells. Numeric cells carry the kernel formatter's canonical decimal string (so the cell value and the PDF text show the same digits) and the number of decimals; the unit moves into the column header; text cells are neutralised so no spreadsheet treats them as formulas.

## Scope

**In:** `SheetTable { name, header: Vec<HeaderCell>, rows: Vec<Vec<Cell>> }` and `Cell { Number { canonical, decimals } | Text(Untrusted) | Empty }`; header text with unit (`Measured [mm]`); unit-system check against the resolved report's unit override; formula-injection neutralisation; refusal of reports with no table block or with cells that cannot be represented.

**Out:** CSV bytes (`RPT.OUTPUT.SHEET.CSV`); workbook bytes (`RPT.OUTPUT.SHEET.XLSX`); value text (`RPT.MODEL.FMT`).

## Requirements

- **REQ-RPT.OUTPUT.SHEET.CELL-001** For each `ValueText` in a table, the cell model shall produce a numeric cell whose value is the `ValueText.canonical` string and whose decimals equal the digits after its decimal point, and shall put the unit symbol in the column header, not in the cell.
- **REQ-RPT.OUTPUT.SHEET.CELL-002** If a column holds values in more than one unit, then the cell model shall split it into one column per unit, each with its unit in the header, in order of first appearance.
- **REQ-RPT.OUTPUT.SHEET.CELL-003** When SI output is requested and the resolved report's unit override is not SI, the cell model shall fail with error `report.units_mismatch` telling the caller to regenerate with SI units; it shall never convert values itself.
- **REQ-RPT.OUTPUT.SHEET.CELL-004** If a text cell begins with `=`, `+`, `-`, `@`, tab (U+0009), carriage return (U+000D) or a full-width variant of `=`, `+`, `-` or `@`, then the cell model shall prefix it with an apostrophe so no spreadsheet application evaluates it, and shall keep the original text in the cell's untrusted source field.
- **REQ-RPT.OUTPUT.SHEET.CELL-005** If the resolved report has no `table` block, or a table cell is of a kind the cell model cannot represent, then it shall fail with error `report.unsupported_block` naming the block.
- **REQ-RPT.OUTPUT.SHEET.CELL-006** Values with a nonconformance marker shall become text cells holding the display string with the marker, so the marker is never lost.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/golden/bracket_3stage.resolved.json`, when converted, then every numeric cell's canonical string equals the corresponding `ValueText.canonical`, decimals match, and headers carry units.
- **AC-2** Given a table mixing mm and deg values in one column, then two columns `... [mm]` and `... [deg]` are produced.
- **AC-3** (C6) Given fixture `fixtures/rpt/output/sheet/injection.json` (`=HYPERLINK(...)`, `+1`, `-2+3`, `@SUM`, tab-prefixed, full-width `＝1+1`), when converted, then each text cell starts with an apostrophe and keeps its original in the source field.
- **AC-4** (error path) Given a report with only text blocks, then conversion fails with `report.unsupported_block`; given SI requested on a non-SI resolved report, it fails with `report.units_mismatch`.
- **AC-5** Given a value with a tie marker, then its cell is text with the marker.

## Design

- **Algorithm:** walk table blocks in document order; map each cell; split multi-unit columns; neutralise text; no arithmetic.
- **Errors:** `report.unsupported_block`, `report.units_mismatch`.
- **Determinism:** D0.
- **Performance budget:** 100 000 cells in under 100 ms [unverified until measured].
- **Crate:** `fid-report-out` (module `sheet::cell`).

## Test fixtures

Golden resolved reports from `RPT.MODEL.BIND.GEN`; `fixtures/rpt/output/sheet/injection.json` (in-project, CC-BY-4.0).

## Build prompt

> In crate `fid-report-out`, add module `sheet::cell`: convert each resolved `table` block into a `SheetTable` of typed cells. Numeric cells hold `ValueText.canonical` and its decimal count; units go into headers (split multi-unit columns); SI output requires a resolved report generated with SI units (`report.units_mismatch`, never convert); text starting with `= + - @`, tab, CR or their full-width forms gets an apostrophe prefix, original kept as untrusted source; marked values become text cells with the marker; no table or unrepresentable cells -> `report.unsupported_block`. Acceptance: AC-1..AC-5 in `spec/rpt/output/sheet/cell/NODE.md`. Constraints: CONSTITUTION C1, C6, §8.

## Open questions

- The numeric cell carries the formatted decimal, not the full-precision f64, as the parent assumed; full precision belongs to QIF and `IO.RES` exports.

## Children

_Leaf: none._
