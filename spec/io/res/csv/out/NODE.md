---
id: IO.RES.CSV.OUT
title: Result table export through a column map
level: 4
parent: IO.RES.CSV
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-io-res]
satisfies: [REQ-IO.RES.CSV-002, REQ-IO.RES.CSV-003]
covers: [GOM.EXP.02, GOM.EXP.06, PW.EXP.02]
depends_on: [IO.RES.CSV.MAP, IO.RES.TRANS.VIEW, IO.REG.ENV.EXPORT, IO.REG.WRITE]
uses_artifacts: [GDT.CHAR.RECORD]
side_effect: W1
determinism: D0
cli: [fid export results-csv]
mcp: [export_results_csv]
journal: [export.results_csv@1]
updated: 2026-11-17
---

# IO.RES.CSV.OUT: Result table export through a column map

## Purpose

`export.results_csv@1` writes ResultRecords and transcribed results as a delimited table through a column map, keeping each value's authored decimal text and unit. With a built-in map the table carries evaluation basis, uncertainty class, decision rule, record status and source software; a user map that drops any of them is allowed but flagged `lossy`. This covers the incumbents' "export values as CSV" and "export tables" features.

## Scope

**In:**
- `export.results_csv@1` (W1, `fs_effect: write_root`) on the export skeleton
- Long and wide layouts from the map
- Formula-injection neutralisation for text cells (leading `=`, `+`, `-`, `@`, tab, carriage return), never for decimal fields
- Transcription marking through `IO.RES.TRANS.VIEW` columns

**Out:**
- Workbook (XLSX) writing (`RPT.OUTPUT.SHEET`)
- Point tables (`IO.CLOUD.ASCII`)

## Requirements

- **REQ-IO.RES.CSV.OUT-001** Each value shall be written in its authored decimal text, converted only for the map's decimal mark, with its unit in the column the map names.
- **REQ-IO.RES.CSV.OUT-002** When the export map is lossy per `IO.RES.CSV.MAP`, the export report and journal event shall carry `lossy` with the omitted fields.
- **REQ-IO.RES.CSV.OUT-003** Every text cell beginning with `=`, `+`, `-`, `@`, tab or carriage return shall be written with a leading apostrophe, and decimal fields shall never be altered this way.
- **REQ-IO.RES.CSV.OUT-004** With a map that includes record status and source software, every transcribed result shall carry its marking and the export shall pass the `IO.RES.TRANS.VIEW` conformance kit.
- **REQ-IO.RES.CSV.OUT-005** Exporting the same records with the same map twice shall give byte-identical files.

## Acceptance criteria

- **AC-1** Given `fixtures/io/res/csv/out/records.json` (10 evaluated and 3 transcribed) and map `fid.results.long@1`, when `fid export results-csv --run R --map fid.results.long@1 --target root://out/r.csv --json` runs, then `r.csv` equals `r.golden.csv` byte for byte and the report is not lossy.
- **AC-2** Given a user map without `decision_rule`, then the report and journal carry `lossy: [decision_rule]`.
- **AC-3** Given a characteristic name `=HYPERLINK(...)`, then the cell is written `'=HYPERLINK(...)`; given an actual `-0.012`, then it is written `-0.012`.
- **AC-4** Given a semicolon map with decimal comma, then `0.0100` is written `0,0100`.
- **AC-5** (error path) Given an existing target without `--overwrite`, then `target_exists`.
- **AC-6** (replay) Given AC-1's journal replayed into an empty root, then `IDENTICAL`.

## Design

- **Registry command:** `export.results_csv@1` W1 `fs_effect: write_root`; params `run?: RunId`, `stages?: [StageId]`, `characteristics?: [CharRef]`, `map: ColMapRef`, `target: RootHandle`, `overwrite: bool = false`, `dry_run: bool = false`; result `ExportReport` with `lossy`.
- **Data structures:** none new.
- **Algorithm:** load records (GDT canonical JSON and transcribed results) → order by characteristic then part → render cells per map → RFC 4180 quoting → atomic write; LF line endings unless the map says CRLF.
- **Errors:** `colmap_invalid`, `no_results`, envelope errors.
- **Determinism:** D0.
- **Performance budget:** 10^5 rows in < 1 s [unverified until measured].
- **Crate:** `fid-io-res` (module `csv::out`).

## Test fixtures

`fixtures/io/res/csv/out/` records and golden CSV files (CC-BY-4.0).

## Build prompt

> In crate `fid-io-res`, add module `csv::out` registering `export.results_csv@1` (W1, `fs_effect: write_root`; CLI `fid export results-csv`, MCP `export_results_csv`) on the export skeleton. Render ResultRecords (GDT canonical JSON) and transcribed results through a column map in long or wide layout: authored decimal text (only the decimal mark changes), unit columns, RFC 4180 quoting, apostrophe prefix for text cells starting with `= + - @`, tab or CR (never decimal fields), transcription marking via `trans::view` with its conformance kit, `lossy` list in report and journal. Byte-identical output for identical input. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- None.

## Children

_Leaf: none._
