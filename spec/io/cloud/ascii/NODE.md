---
id: IO.CLOUD.ASCII
title: ASCII, CSV and XYZ point tables in and out
level: 3
parent: IO.CLOUD
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CLOUD-001, REQ-IO.CLOUD-002, REQ-IO.CLOUD-008, REQ-IO.CLOUD-011]
covers: [GOM.IMP.04, GOM.IMP.08, GOM.EXP.02, PCD.EXP.02, PW.IMP.01, PW.EXP.01]
depends_on: [IO.CLOUD.STREAM, IO.REG.ENV, IO.REG.WRITE]
side_effect: W2
determinism: D0
cli: [fid import cloud-ascii, fid export cloud-ascii]
mcp: [import_cloud_ascii, export_cloud_ascii]
journal: [import.cloud_ascii@1, export.cloud_ascii@1]
updated: 2026-11-16
---

# IO.CLOUD.ASCII: ASCII, CSV and XYZ point tables

## Purpose

Imports delimited text point tables, from a dozen nominal points typed in a spreadsheet to scanner exports of 10^8 lines, with an explicit column mapping and a declared role (`nominal` or `actual`). Exports point sets with selected attributes as text with round-trip-exact numbers.

## Scope

**In:**
- Delimiters: comma, semicolon, tab, whitespace runs; decimal point `.` or `,` (only with a non-comma delimiter); optional header row; comment lines starting with `#` or `//`
- Column mapping: given (`x=1,y=2,z=3,nx=4,...,label=0`) or proposed from a header row and echoed in the report; the mapping is journaled
- Role `nominal` or `actual`; optional `label` column as untrusted text
- `skip_bad_rows` with per-line listing in the report
- Export with chosen columns, delimiter, header and digits (shortest round-trip by default, or a fixed decimal count recorded in the journal)
- `import.cloud_ascii@1` (W2), `export.cloud_ascii@1` (W1 write_root)

**Out:**
- Characteristic result tables (`IO.RES.CSV`)
- Streaming storage (`IO.CLOUD.STREAM`)

## Requirements

- **REQ-IO.CLOUD.ASCII-001** When an ASCII or CSV point file is imported, the command shall require a role (`nominal` or `actual`) and a column mapping, either given or proposed from a header row, and the effective mapping shall be journaled.
- **REQ-IO.CLOUD.ASCII-002** If a header-derived mapping is proposed and `confirm_mapping` is not set, then the command shall return the proposal in a dry-run report and import nothing.
- **REQ-IO.CLOUD.ASCII-003** If a row cannot be parsed, then the import shall fail with `malformed` naming the line, unless `skip_bad_rows` is set, in which case each skipped line number and reason shall be listed in the import report.
- **REQ-IO.CLOUD.ASCII-004** Decimal numbers shall be parsed to f64 with correct rounding.
- **REQ-IO.CLOUD.ASCII-005** By default the exporter shall write the shortest decimal form that re-imports to the identical f64; when `digits=n` is given, it shall write n decimals and record `n` in the journal.
- **REQ-IO.CLOUD.ASCII-006** The exporter shall write exactly the selected attribute columns in the requested order, with an optional header row.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cloud/ascii/nominal_10.csv` (header `id,x,y,z`, 10 rows, mm), when `fid import cloud-ascii --role nominal --unit mm --map label=id,x=x,y=y,z=z --json` runs, then 10 nominal points with labels are created and the journal holds the mapping.
- **AC-2** Given the same file without `--map`, then the command returns a proposal `label=0,x=1,y=2,z=3` and imports nothing; with `--confirm-mapping` it imports as AC-1.
- **AC-3** Given `fixtures/io/cloud/ascii/semicolon_comma_decimal.txt` (`1,5;2,25;3,0`), with delimiter `;` and decimal `,`, then the point is (1.5, 2.25, 3.0) mm.
- **AC-4** (error path) Given `fixtures/io/cloud/ascii/bad_row_7.xyz`, then the import fails `malformed{line: 7}`; with `--skip-bad-rows` it imports the other rows and lists line 7.
- **AC-5** (error path) Given no `--role`, then the command fails `role_required`.
- **AC-6** Given a point set of 1,000 seeded random f64 points exported with defaults and re-imported, then all coordinates are bit-identical; with `--digits 3`, the journal records `digits: 3`.
- **AC-7** Given a seeded 10^7-line XYZ file, when imported, then the import streams through `PointSink` and peak resident memory stays below 1.5 GiB [unverified until measured].
- **AC-8** (replay) Given the journal of AC-1, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry commands:** `import.cloud_ascii@1` (W2; `source`, `role`, `unit`, `map?`, `confirm_mapping?`, `delimiter?`, `decimal?`, `header?`, `skip_bad_rows?`, `stage?`, `dry_run?`), `export.cloud_ascii@1` (W1 write_root; `point_set`, `target`, `columns[]`, `delimiter`, `header`, `digits?`, `overwrite?`). Strongest class W2 named in frontmatter.
- **Data structures:** `ColumnMap { label?, x, y, z, nx?, ny?, nz?, intensity?, r?, g?, b?, extra: [(name, col)] }` canonicalised for the journal.
- **Algorithm:** bounded line reader (64 KiB per line); correctly rounded decimal parser; shortest round-trip formatter (Ryu-style).
- **Errors:** `malformed`, `role_required`, `mapping_required`, `unit_required`, `limit_exceeded`.
- **Determinism:** D0.
- **Performance budget:** AC-7; 10^7 lines in < 10 s [unverified until measured].
- **Crate:** `fid-io-cloud`.

## Test fixtures

Hand-written and seeded synthetic text files under `fixtures/io/cloud/ascii/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-cloud`, implement delimited-text point import/export: `import.cloud_ascii@1` (W2; CLI `fid import cloud-ascii`; MCP `import_cloud_ascii`) and `export.cloud_ascii@1` (W1, `fs_effect: write_root`; CLI `fid export cloud-ascii`; MCP `export_cloud_ascii`) on the `fid-io-reg` skeleton. Role `nominal|actual` is required (`role_required`); column mapping is required, or proposed from a header and returned without importing unless `confirm_mapping`; journal the effective mapping. Delimiters `, ; tab whitespace`; decimal `.` or `,`. Bad row → `malformed{line}` unless `skip_bad_rows` (list lines in report). Correctly rounded parsing; export shortest round-trip by default or fixed `digits` recorded in the journal; write exactly the selected columns. Stream through `PointSink` (in-memory sink until `IO.CLOUD.STREAM` lands). Unit required. CONSTITUTION §8: bounded lines, no panics, no unwrap. Tests AC-1..AC-8.

## Open questions

- None.

## Children

_Leaf: none._
