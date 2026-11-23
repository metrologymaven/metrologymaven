---
id: RPT.OUTPUT.SHEET.CSV
title: CSV export of report tables
level: 4
parent: RPT.OUTPUT.SHEET
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.OUTPUT.SHEET-001, REQ-RPT.OUTPUT.SHEET-005, REQ-RPT.OUTPUT.SHEET-007]
covers: [PCD.RPT.04, SA.RPT.03]
depends_on: [RPT.OUTPUT.SHEET.CELL, RPT.OUTPUT.META]
side_effect: W1
fs_effect: write_root
determinism: D0
cli: [fid report export-csv]
mcp: [report_export_csv]
journal: [report.export_csv@1]
built_in: [fid-report-out]
updated: 2026-11-22
---

# RPT.OUTPUT.SHEET.CSV: CSV export of report tables

## Purpose

Writes each table block of a resolved report as one CSV file in a stated dialect, plus one metadata file holding the `ExportMeta` record, into an approved local export root. Numbers are the canonical ASCII decimal strings, so the digits equal those shown in the PDF.

## Scope

**In:** `report.export_csv@1` (W1, `fs_effect: write_root`); dialect: UTF-8 without BOM, comma separator, `.` decimal point, CRLF line ends, RFC 4180 quoting; file naming `<base>-<nn>.csv` in table order plus `<base>-meta.json`; dry run.

**Out:** cell model and neutralisation (`RPT.OUTPUT.SHEET.CELL`); export-root writes (`RPT.OUTPUT.META`); report-independent results CSV with column maps (`IO.RES.CSV`).

## Requirements

- **REQ-RPT.OUTPUT.SHEET.CSV-001** The command shall write one CSV file per table block in block order, named `<base>-NN.csv` with NN the 1-based two-digit ordinal, and one `<base>-meta.json` holding the canonical `ExportMeta` with renderer `csv`.
- **REQ-RPT.OUTPUT.SHEET.CSV-002** Each CSV shall use UTF-8 without BOM, comma separators, CRLF line ends, the header row first, and RFC 4180 quoting for any field containing a comma, quote, CR or LF.
- **REQ-RPT.OUTPUT.SHEET.CSV-003** Numeric cells shall be written as their canonical decimal string, unquoted.
- **REQ-RPT.OUTPUT.SHEET.CSV-004** Exporting the same resolved report twice with the same exporter version shall give byte-identical files.
- **REQ-RPT.OUTPUT.SHEET.CSV-005** The command shall be side-effect class W1 with `fs_effect: write_root`, write all files through the export sink, and journal one event listing every receipt; if any file fails, files already written by this invocation shall be removed and nothing journaled.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/golden/bracket_3stage.resolved.json` (two table blocks), when `fid report export-csv --resolved <d> --root <h> --base bracket --json` runs, then `bracket-01.csv`, `bracket-02.csv` and `bracket-meta.json` exist, equal golden files byte for byte, and one event with three receipts is journaled.
- **AC-2** Given AC-1 run twice with overwrite, then the files are byte-identical.
- **AC-3** Given the injection fixture, then neutralised cells appear quoted where needed and no field begins with an unprefixed `=`.
- **AC-4** (error path) Given an existing `bracket-02.csv` without overwrite, then the command fails with `report.export_exists`, `bracket-01.csv` from this invocation is removed and nothing is journaled.
- **AC-5** (cross-check) Given AC-1 and the PDF from `RPT.OUTPUT.PDF.WRITE` AC-1, then every numeric CSV field equals the canonical form of the corresponding PDF text.

## Design

- **Registry command:** `report.export_csv@1(resolved: Sha256 | ReportId, root: ExportRootHandle, base: ExportName, overwrite: bool, dry_run: bool) -> ExportResult` (W1, `write_root`).
- **Errors:** `report.unsupported_block`, `report.units_mismatch`, sink errors.
- **Determinism:** D0.
- **Performance budget:** 100 000 cells in under 200 ms [unverified until measured].
- **Crate:** `fid-report-out` (module `sheet::csv`).

## Test fixtures

Golden resolved reports and golden CSV files under `fixtures/rpt/output/sheet/`; CC-BY-4.0.

## Build prompt

> In crate `fid-report-out`, add module `sheet::csv` and register `report.export_csv@1` (W1, `fs_effect: write_root`; CLI `fid report export-csv --json`, dry run). Use the cell model; write one CSV per table block (`<base>-NN.csv`, UTF-8 no BOM, comma, CRLF, header first, RFC 4180 quoting, numbers as canonical unquoted strings) plus `<base>-meta.json` (canonical `ExportMeta`, renderer `csv`), all through the export sink; on any failure remove files already written by this call and journal nothing; on success journal one event with all receipts. Acceptance: AC-1..AC-5 in `spec/rpt/output/sheet/csv/NODE.md`. Constraints: CONSTITUTION §3.1, C6, §8.

## Open questions

- Locales that use comma decimals may prefer a semicolon dialect; not offered, because the canonical form keeps CSV locale-independent.

## Children

_Leaf: none._
