---
id: IO.RES.CSV.XLSX
title: Bounded workbook reader for layouts and cell tables
level: 4
parent: IO.RES.CSV
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.RES.CSV-004]
covers: []
depends_on: [IO.QIF.XML.PARSE, IO.REG.LIMITS, IO.REG.DETECT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.RES.CSV.XLSX: Bounded workbook reader for layouts and cell tables

## Purpose

The IO-owned reader for Office Open XML workbooks (`.xlsx`, `.xlsm`) agreed at review. It gives two views of the same decoded workbook: a **layout view** (sheet names, used ranges, merged ranges, cell addresses, existing labels as untrusted text) that `MIG.TEMPL` uses to turn a customer's blank form into a form map, and a **cell-table view** that `IO.RES.CSV.IN` uses to import result tables kept in spreadsheets. It never evaluates formulas and never reads macros.

## Scope

**In:**
- Bounded ZIP container reading: entry count, per-entry and total uncompressed size, compression ratio and path checks; no ZIP64 tricks, no encrypted entries
- Parts: workbook, sheets, shared strings, inline strings, merged cells, defined names (as untrusted text)
- Cell kinds: number (lexical text kept), string (untrusted), boolean, error code, formula (formula text untrusted plus cached value)
- `macros_present` flag when a VBA project part exists (never opened); external links listed, never followed
- Registration with `IO.REG.DETECT` as format `xlsx-workbook` (read)

**Out:**
- Writing workbooks (`RPT.OUTPUT.SHEET`)
- Form-map semantics and template creation (`MIG.TEMPL`)
- Legacy binary `.xls` and strict OOXML (rejected; see open questions)

## Requirements

- **REQ-IO.RES.CSV.XLSX-001** The reader shall refuse a container with more than 10,000 entries, more than 1 GiB total uncompressed size, an entry with compression ratio above 200:1, an encrypted entry, or an entry path that is absolute or contains `..`, failing with `limit_exceeded` or `xlsx_container_invalid` [unverified: tune limits on corpus].
- **REQ-IO.RES.CSV.XLSX-002** XML parts shall be read with the hardened reader of `IO.QIF.XML.PARSE`.
- **REQ-IO.RES.CSV.XLSX-003** The reader shall never evaluate a formula, and shall return each formula cell's formula text as untrusted text and its cached value as stored, or `no_cached_value`.
- **REQ-IO.RES.CSV.XLSX-004** When a VBA project or external links are present, the reader shall set `macros_present` or list the links, and shall not open, run or follow them.
- **REQ-IO.RES.CSV.XLSX-005** Numeric cells shall be returned as the exact lexical text stored in the file, without passing through binary floating point.
- **REQ-IO.RES.CSV.XLSX-006** The layout view shall list for each sheet its name, used range, merged ranges and every non-empty cell's address, kind and text, with all text typed untrusted.
- **REQ-IO.RES.CSV.XLSX-007** When the cell-table view is used for a results import and a required column is empty in a row, the position shall be reported as `<sheet>!<cell>` for the `malformed` error of `IO.RES.CSV.IN`.

## Acceptance criteria

- **AC-1** Given `fixtures/io/res/csv/xlsx/form_blank.xlsx` (2 sheets, 5 merged ranges, 30 labels), then the layout view equals `form_blank.layout.json`.
- **AC-2** Given `formulas.xlsx` with `=A1*2` cached as `4`, then the cell kind is formula with text `A1*2` (untrusted) and cached value `4`, and no evaluation happens (a cell with a stale cache returns the stale value).
- **AC-3** Given `macro.xlsm`, then `macros_present: true` and the VBA part is never decompressed.
- **AC-4** (error path) Given `zip_bomb.xlsx` (ratio above 200:1), then `limit_exceeded`; given `traversal.xlsx` with an entry `../x.xml`, then `xlsx_container_invalid`.
- **AC-5** Given `numbers.xlsx` with stored value `0.1`, then the lexical text is `0.1`.
- **AC-6** Given the fuzz target and hostile ZIP and XML seeds from `IO.CORP`, when run for the CI budget, then no panic or timeout.

## Design

- **Registry commands:** none (library; used by `import.results_csv@1` and by `MIG.TEMPL`'s `migrate.import_form@1`).
- **Data structures:** `Workbook { sheets: Vec<Sheet>, defined_names, macros_present, external_links }`; `Sheet { name: UntrustedText, used: Range, merged: Vec<Range>, cells: BTreeMap<(u32, u32), Cell> }`; `Cell { kind: Number(Lexical) | Str(UntrustedText) | Bool | Error(code) | Formula { text: UntrustedText, cached: Option<Box<Cell>> } }`.
- **Algorithm:** read the central directory first and check all limits before inflating; inflate each needed part through a bounded inflater charged to the budget; parse parts with `fid-xml`; resolve shared strings by index with bounds checks.
- **Errors:** `xlsx_container_invalid`, `xlsx_strict_unsupported`, `limit_exceeded`, `malformed{at}`, `format_unknown` (for `.xls`).
- **Determinism:** D0.
- **Performance budget:** 50 MB workbook read in < 3 s [unverified until measured].
- **Crate:** `fid-io-res` (module `csv::xlsx`), or a small `fid-io-xlsx` crate if `MIG` should not depend on all of `fid-io-res`.

## Test fixtures

Workbooks under `fixtures/io/res/csv/xlsx/` generated by a committed script (CC-BY-4.0); hostile ZIP and XML seeds from `IO.CORP`.

## Build prompt

> In crate `fid-io-res` (module `csv::xlsx`, or crate `fid-io-xlsx`), implement a bounded OOXML workbook reader registered as format `xlsx-workbook`: check the ZIP central directory before inflating (≤ 10,000 entries, ≤ 1 GiB total, ratio ≤ 200:1, no encryption, no absolute or `..` paths; `limit_exceeded`/`xlsx_container_invalid`); inflate with a budgeted inflater; parse parts with `fid-xml`; return a layout view (sheets, used and merged ranges, non-empty cells with address, kind and untrusted text) and a cell-table view; keep numbers as lexical text; never evaluate formulas (formula text untrusted plus cached value); flag `macros_present` and list external links without touching them; reject `.xls` (`format_unknown`) and strict OOXML (`xlsx_strict_unsupported`). Add a fuzz target. CONSTITUTION §8: no panics, no unwrap, no `unsafe`. Tests AC-1..AC-6.

## Open questions

- Strict OOXML and legacy `.xls` are rejected in P1a; whether customers send them often enough to matter is [unverified].
- `MIG.TEMPL` should depend on this leaf for its READ step (cross-domain note for MIG; IO owns the reader as agreed at review).

## Children

_Leaf: none._
