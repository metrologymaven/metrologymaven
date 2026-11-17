---
id: MIG.TEMPL.FILL
title: Filling a copy of the customer workbook
level: 3
parent: MIG.TEMPL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.TEMPL-001, REQ-MIG.TEMPL-006]
covers: []
depends_on: [MIG.TEMPL.MAP, MIG.TEMPL.ADOPT, IO.RES.CSV.XLSX, RPT.MODEL.BIND, RPT.MODEL.FMT, RPT.OUTPUT.META, IO.REG.WRITE]
side_effect: W1
determinism: D0
cli: [fid migrate fill-form]
mcp: [migrate_fill_form]
journal: [migrate.fill_form@1]
updated: 2026-11-17
---

# MIG.TEMPL.FILL: Filling a copy of the customer workbook

## Purpose

Produces what the customer actually asked for: their own form, filled. `migrate.fill_form@1` takes the original blank workbook (verified by digest against the form map), resolves the form map's bindings against a run at a journal head, writes the formatted values into exactly the bound cells, and writes the result to an approved export root. Every other part of the workbook (other cells, styles, drawings, sheets) is carried over unchanged, and the output is byte-identical for the same inputs so a filled form can be reproduced and verified later.

## Scope

**In:**
- `migrate.fill_form@1` (W1, `fs_effect: write_root`): params template version, run, journal head, blank workbook handle, export root handle, file name
- Digest check of the supplied blank workbook against the form map's source SHA-256
- Cell patching in the worksheet parts that contain bound cells: values written as numbers (with the cell's existing style) or as inline strings; no shared-string table rewrite beyond appending
- Deterministic package write: original part order, fixed ZIP timestamps and compression settings, unchanged bytes for every part without bound cells
- Release block: refuse filling for a release-labelled export when the form map is not human or adopted
- Export metadata through `RPT.OUTPUT.META`

**Out:**
- Reading the package (owned by the IO workbook reader, `IO.RES.CSV.XLSX`)
- Binding resolution and formatting (owned by `RPT.MODEL.BIND`, `RPT.MODEL.FMT`)
- New-workbook exports of report tables (owned by `RPT.OUTPUT.SHEET`)

## Requirements

- **REQ-MIG.TEMPL.FILL-001** If the supplied blank workbook's SHA-256 differs from the form map's source workbook SHA-256, then the command shall fail with `form_source_mismatch` and write nothing.
- **REQ-MIG.TEMPL.FILL-002** The filled workbook shall differ from the blank workbook only in the values of bound cells and in the package parts that must change to hold them, and every other package part shall be byte-identical to the original.
- **REQ-MIG.TEMPL.FILL-003** Numbers shall be written as numeric cell values formatted by the report formatter, decisions and text as strings, and no formula shall be written.
- **REQ-MIG.TEMPL.FILL-004** The same template version, run, journal head and blank workbook shall produce a byte-identical output file, and the journal event shall carry its SHA-256.
- **REQ-MIG.TEMPL.FILL-005** If a repetition rule overflows its rows, then the command shall fail with `form_overflow` unless the caller passes `allow_truncate`, in which case the output shall be marked truncated in the export metadata.
- **REQ-MIG.TEMPL.FILL-006** If the export is labelled for release and the form map's provenance is `proposed`, then the command shall fail with `form_map_not_adopted` and write nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/templ/fill/acme/` (blank form, adopted form map, run with 38 characteristics), when `fid migrate fill-form --template <v> --run r1 --form <handle> --root out --name acme-r1.xlsx --json` runs, then `out/acme-r1.xlsx` equals `expected/acme-r1.xlsx` byte for byte.
- **AC-2** Given AC-1, when the output and the blank form are unpacked, then every part other than the patched worksheet part(s) and the shared-strings part is byte-identical, and every unbound cell value is unchanged.
- **AC-3** (error path) Given a different blank workbook, then the command fails with `form_source_mismatch` and writes nothing.
- **AC-4** (error path) Given a run with 45 characteristics and 40 rows, then it fails with `form_overflow`; with `--allow-truncate` it succeeds and the metadata says truncated.
- **AC-5** (error path) Given a `proposed` form map and `--release`, then it fails with `form_map_not_adopted`.
- **AC-6** Given AC-1 run twice into empty roots, then outputs are byte-identical.

## Design

- **Registry command(s):** `migrate.fill_form@1`, W1 (`fs_effect: write_root`); params `template`, `run`, `head?`, `form: ImportHandle`, `root: ExportHandle`, `name`, `allow_truncate?: bool`, `release?: bool`; result `{ file, sha256, truncated }`.
- **Data structures:** `CellPatch { sheet_part, address, value: Number(f64) | Text(String) }` sorted by part then address.
- **Algorithm:** verify digest → resolve bindings (RPT) → format values → build patches → open package with the IO reader (bounded) → for each worksheet part with patches, rewrite only the affected `<c>` elements, preserving style indices → append new strings → write ZIP with original entry order, fixed DOS timestamp, deflate level fixed → atomic write via `IO.REG.WRITE`.
- **Errors:** `form_source_mismatch`, `form_overflow`, `form_map_not_adopted`, reader and write errors.
- **Determinism:** D0 (fixed ZIP parameters; the deflate implementation is pinned so compressed bytes are stable [unverified across library versions; a fixture guards it]).
- **Performance budget:** 50,000-cell form filled in under 1 s [unverified until measured].
- **Crate:** `fid-mig` (module `templ::fill`).

## Test fixtures

`fixtures/mig/templ/fill/`: fictional forms authored for the project, adopted form maps and synthetic runs; expected outputs generated once and inspected by a person in a spreadsheet application; CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `templ::fill` and register `migrate.fill_form@1` (W1, `fs_effect: write_root`; CLI `fid migrate fill-form`, MCP `migrate_fill_form`). Implement spec node MIG.TEMPL.FILL: verify the blank workbook SHA-256 against the form map (`form_source_mismatch`); resolve bindings through the RPT binding engine and format with the RPT formatter; fail `form_overflow` on repetition overflow unless `allow_truncate`; refuse release-labelled exports for `proposed` form maps (`form_map_not_adopted`); patch only the bound cells in the affected worksheet parts (numbers as numeric values keeping the cell style, text as strings, never formulas), append to shared strings only, keep every other package part byte-identical; write the ZIP deterministically (original entry order, fixed timestamps, pinned deflate settings) atomically into the export root; journal the output SHA-256. Tests for AC-1..AC-6. Bounded reading; no `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- Owning customer-form filling here rather than in `RPT.OUTPUT` is flagged in the parent's Open questions.

## Children

_Leaf: none._
