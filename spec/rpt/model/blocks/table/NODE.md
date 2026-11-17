---
id: RPT.MODEL.BLOCKS.TABLE
title: Characteristic table block with column and row customisation
level: 4
parent: RPT.MODEL.BLOCKS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.MODEL.BLOCKS-001, REQ-RPT.MODEL.BLOCKS-002, REQ-RPT.MODEL.BLOCKS-003]
covers: [GOM.RPT.04, PW.RPT.03, SA.RPT.04]
depends_on: [RPT.MODEL.DOC, RPT.MODEL.BIND.RESOLVE, RPT.MODEL.FMT, GDT.CHAR.RECORD]
side_effect: W2
determinism: D0
cli: [fid report set-table-block]
mcp: [report_set_table_block]
journal: [report.set_table_block@1]
updated: 2026-11-17
---

# RPT.MODEL.BLOCKS.TABLE: Characteristic table block with column and row customisation

## Purpose

The `table` block kind: the workhorse of every inspection report. The user picks which columns to show from ResultRecord fields and characteristic attributes, orders and renames them, and filters rows by decision outcome and tag. The block stores only that description plus bindings; values arrive at generation time through the formatter layer.

## Scope

**In:** `table` payload schema v1 (column list, header labels, row filters, optional sort override); the closed list of selectable column sources; `BlockResolver` implementation producing resolved cells; `report.set_table_block@1` (W2).

**Out:** row repetition over characteristics (`RPT.MODEL.BIND.EXPAND`); value text (`RPT.MODEL.FMT`); cell styling and widths (`RPT.DESIGN.STYLE`, `RPT.DESIGN.PLACE`); spreadsheet export (`RPT.OUTPUT.SHEET`).

## Requirements

- **REQ-RPT.MODEL.BLOCKS.TABLE-001** The `table` kind shall register payload schema version 1 with the block-kind registry, and a payload that fails it shall be rejected with error `report.invalid_block_payload`.
- **REQ-RPT.MODEL.BLOCKS.TABLE-002** A column shall name one source from a closed list (characteristic ID, name, type, nominal, lower and upper limit, measured value, deviation, decision outcome, decision rule ID, uncertainty status, evaluation basis, source label, stage, run, tag), an optional renamed header of at most 128 bytes of untrusted text, and its position, with 1 to 64 columns per table.
- **REQ-RPT.MODEL.BLOCKS.TABLE-003** When `report.set_table_block@1` runs, the system shall replace the block's column list, headers and filters as given and journal one event, and if a column source is not in the closed list it shall fail with error `report.column_unknown`.
- **REQ-RPT.MODEL.BLOCKS.TABLE-004** At generation, the resolver shall drop rows whose decision outcome is not in the outcome filter or whose characteristic has none of the filter tags, applying both filters when both are set.
- **REQ-RPT.MODEL.BLOCKS.TABLE-005** Every numeric cell, decision and uncertainty cell shall be a `ValueText` from `RPT.MODEL.FMT`, and a cell for a missing value shall be the fixed text "no result".

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/bracket_3stage` and a table block, when `fid report set-table-block --columns id,nominal,measured,deviation,decision --rename measured="Actual" --json` runs, then the payload stores 5 columns in that order with one renamed header and one event is journaled.
- **AC-2** Given AC-1 with filter `--outcome NONCONFORMS`, when generated, then only nonconforming rows appear; with `--tag critical` added, only nonconforming critical rows appear.
- **AC-3** (error path) Given `--columns id,colour`, then it fails with `report.column_unknown`; given 65 columns, it fails with `report.invalid_block_payload`.
- **AC-4** Given AC-1, when generated, then each numeric cell records a formatter ID and a UEC-0 row shows "not evaluated" in an uncertainty column.
- **AC-5** (replay) Given the journal of AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `report.set_table_block@1(report, block, columns: Vec<ColumnSpec>, filter: RowFilter) -> TablePayload` (W2).
- **Data structures:** `ColumnSpec { source: ColumnSource, header: Option<Untrusted<Text>> }`; `RowFilter { outcomes: BTreeSet<Outcome>, tags: BTreeSet<TagId> }`; resolved `TableCells { header: Vec<Text>, rows: Vec<Vec<CellText>> }`.
- **Errors:** `report.invalid_block_payload`, `report.column_unknown`, `report.not_found`, `report.text_invalid`.
- **Determinism:** D0.
- **Performance budget:** resolve a 500-row, 10-column table in under 100 ms [unverified until measured].
- **Crate:** `fid-report`.

## Test fixtures

`fixtures/rpt/model/bracket_3stage` (shared; includes tags `critical` and `major` and one UEC-0 record).

## Build prompt

> In crate `fid-report`, add the `table` block kind: payload schema v1 (1-64 `ColumnSpec`s from a closed `ColumnSource` enum, optional untrusted header up to 128 bytes, `RowFilter` by outcome set and tag set), registered with the block-kind registry; a `BlockResolver` that builds header and cells, applying both filters at generation and taking every value, decision and uncertainty text from the report formatter layer ("no result" for missing values); and `report.set_table_block@1` (W2, one event; CLI `fid report set-table-block --json`; `report.column_unknown` for unknown sources). Acceptance: AC-1..AC-5 in `spec/rpt/model/blocks/table/NODE.md`. Constraints: CONSTITUTION C1, C6, §8 (no panic, no `unsafe`, deny `unwrap`/`expect`); no std HashMap/HashSet.

## Open questions

- Per-column decimals override is not offered; decimals come from the formatter selection policy in `RPT.MODEL.FMT`.

## Children

_Leaf: none._
