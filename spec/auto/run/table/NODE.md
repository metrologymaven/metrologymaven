---
id: AUTO.RUN.TABLE
title: Tabular bindings and results tables
level: 3
parent: AUTO.RUN
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN-008]
covers: [SA.MP.03]
depends_on: [AUTO.PROG.FAMILY, AUTO.RUN.BIND]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.RUN.TABLE: Tabular bindings and results tables

## Purpose

Spreadsheet-style input and output for batches. An input table has one row per run and one column per program parameter (with the unit in the column header or in each cell); TABLE turns it into validated bindings. After a batch, TABLE builds a results table with one row per run (run ID, binding, outcome, chosen result columns). Reading and writing CSV or spreadsheet files is IO's job; TABLE works on an in-memory table model.

## Scope

**In:**
- In-memory `Table` model (header row, string cells)
- Header syntax `name` or `name [unit]`; cell syntax `value` or `value unit`
- `bindings_from_table(table, decls) -> Result<Vec<Binding>, Vec<CellError>>`
- `results_table(records, columns) -> Table`

**Out:**
- CSV and spreadsheet readers and writers (`IO`)
- Running the batch (`AUTO.RUN.BATCH`)
- Live two-way spreadsheet links (not in scope; see parent open question)

## Requirements

- **REQ-AUTO.RUN.TABLE-001** `bindings_from_table` shall map each column header to a declared parameter by name, and if a header names no declared parameter, then it shall report `unknown_column`.
- **REQ-AUTO.RUN.TABLE-002** When a header carries a unit in brackets, every cell in that column shall be read as a decimal in that unit; when it does not, a quantity cell shall carry its own unit, and a quantity cell with no unit from either source shall be reported `missing_unit`.
- **REQ-AUTO.RUN.TABLE-003** Every row's binding shall be checked with `check_binding`, and errors shall be reported with row number, column and code; no binding shall be returned if any row has an error.
- **REQ-AUTO.RUN.TABLE-004** Cell values shall be passed as authored strings with no float conversion or reformatting.
- **REQ-AUTO.RUN.TABLE-005** `results_table` shall write one row per run record in batch order, with columns run ID, row number, outcome and each requested result column.

## Acceptance criteria

- **AC-1** Given fixture table `fixtures/tables/bracket-batch.csv` read by the IO CSV reader (3 rows; headers `hole_spacing [mm]`, `hole_count`, `scan`), when `bindings_from_table` runs against `bracket.json`'s declarations, then it returns 3 bindings with `hole_spacing` values exactly as written.
- **AC-2** (error path) Given a table whose row 2 has `hole_count = four` and row 3 lacks `scan`, then errors `(2, hole_count, wrong_type)` and `(3, scan, missing_parameter)` and no bindings.
- **AC-3** (error path) Given a header `colour`, then `unknown_column`.
- **AC-4** Given header `hole_spacing` without unit and a cell `4.724 in`, then that binding holds `4.724` and `in`; a cell `120` gives `missing_unit`.
- **AC-5** Given three run records, then `results_table` equals golden `fixtures/tables/bracket-results.csv` after writing with the IO CSV writer.

## Design

- **Registry command(s):** none; library used by `program.run_batch@1`.
- **Data structures:** `Table { header: Vec<String>, rows: Vec<Vec<String>> }`; `CellError { row, column, code }`.
- **Algorithm:** header parse `^(.+?)\s*\[(.+)\]$`; cell quantity parse shared with `AUTO.CLI.ARGS` grammar; binding check via `fid_program::check_binding`.
- **Errors:** `unknown_column`, `missing_unit`, `wrong_type`, `wrong_unit`, `missing_parameter`.
- **Determinism:** D0.
- **Performance budget:** 10,000 rows × 20 columns in < 200 ms [unverified until measured].
- **Crate:** `fid-run`

## Test fixtures

`fixtures/tables/bracket-batch.csv`, `bracket-results.csv` and error variants (project-authored); `fixtures/program/family/bracket.json`.

## Build prompt

> In crate `fid-run`, implement tabular bindings over an in-memory `Table` (header plus string cells). `bindings_from_table(table, decls)`: map headers to declared parameters (`unknown_column` otherwise); header `name [unit]` applies the unit to every cell in the column, otherwise quantity cells must be `value unit` (`missing_unit`); keep authored strings exactly; validate each row with `fid_program::check_binding` and report all errors with row, column and code, returning no bindings if any error exists. `results_table(records, columns)`: one row per run in batch order with run ID, row number, outcome and requested result columns. Acceptance: the five cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; no file I/O here (IO owns readers and writers).

## Open questions

- None.

## Children

_Leaf: none._
