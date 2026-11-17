---
id: IO.RES.CSV
title: CSV and table result exchange through versioned column maps
level: 3
parent: IO.RES
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.RES-006, REQ-IO.RES-007, REQ-IO.RES-013]
covers: [GOM.EXP.02, GOM.EXP.06, PW.EXP.02]
depends_on: [IO.RES.TRANS, IO.REG.ENV, IO.REG.WRITE]
cli: [fid import results-csv, fid export results-csv]
mcp: [import_results_csv, export_results_csv]
journal: [import.results_csv@1, export.results_csv@1]
updated: 2026-11-17
---

# IO.RES.CSV: CSV and table result exchange

## Purpose

Moves characteristic results in and out of delimited tables through versioned column maps: a column map names which column holds characteristic ID, nominal, actual, limits, unit, outcome and so on. Import produces transcribed values; export writes ResultRecords as tables (also for spreadsheet use, the "export tables" seed) with authored decimals and units kept.

## Scope

**In:**
- Column-map schema, versioning, digest; built-in maps for common layouts; user maps as data
- CSV and TSV read and write with the same delimiter and decimal rules as `IO.CLOUD.ASCII`
- `import.results_csv@1` (W2), `export.results_csv@1` (W1 write_root)
- Columns for `evaluation_basis`, uncertainty class, decision rule and transcription status on export

**Out:**
- Point tables (`IO.CLOUD.ASCII`)
- XLSX writing (not in IO.RES scope; RPT may render spreadsheets)

## Requirements

- **REQ-IO.RES.CSV-001** When a CSV result file is imported, the command shall require a column map, and the map's ID, version and SHA-256 shall be journaled.
- **REQ-IO.RES.CSV-002** When ResultRecords are exported, each value shall be written in its authored decimal form with its unit in the column the map names.
- **REQ-IO.RES.CSV-003** Export maps shall include columns for `evaluation_basis`, uncertainty class, decision rule and record status by default, and a map that omits them shall be flagged `lossy` in the export report.
- **REQ-IO.RES.CSV-004** If an imported row lacks a column the map marks required, then the import shall fail with `malformed` naming the line and column.

## Interfaces

- **Offers:** column-map schema and built-in maps; the two commands.
- **Consumes:** `IO.RES.TRANS`, `IO.REG` envelope and writer.

## Decomposition hints

MAP (schema, versioning, built-ins), IN, OUT.

## Open questions

- Agreed at review: IO owns a bounded workbook (XLSX) reader for `MIG.TEMPL`. It is `IO.RES.CSV.XLSX`, which also lets `import.results_csv@1` read result tables from one workbook sheet. **Cross-domain:** `MIG.TEMPL` should depend on `IO.RES.CSV.XLSX` for its READ step instead of owning a reader.
- Workbook writing stays with `RPT.OUTPUT.SHEET`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| IO.RES.CSV.MAP | Versioned result column maps and table dialect (leaf) | 001 003 | none |
| IO.RES.CSV.IN | Result table import through a column map (leaf) | 001 004 | none |
| IO.RES.CSV.OUT | Result table export through a column map (leaf) | 002 003 | GOM.EXP.02, GOM.EXP.06, PW.EXP.02 |
| IO.RES.CSV.XLSX | Bounded workbook reader for layouts and cell tables (leaf) | 004 | none |
