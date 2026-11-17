---
id: MIG.TEMPL.MAP
title: Form import and form-map draft template
level: 3
parent: MIG.TEMPL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.TEMPL-001, REQ-MIG.TEMPL-002, REQ-MIG.TEMPL-003]
covers: []
depends_on: [IO.RES.CSV.XLSX, RPT.MODEL.TMPL, IO.REG.ROOTS, CORE.BUS.PRIN, MIG.SRC.GATE, CORE.JRN.CANON]
side_effect: W2
determinism: D0
cli: [fid migrate import-form]
mcp: [migrate_import_form]
journal: [migrate.import_form@1]
updated: 2026-11-17
---

# MIG.TEMPL.MAP: Form import and form-map draft template

## Purpose

`migrate.import_form@1` reads a customer's blank form workbook as an untrusted layout (sheets, cell addresses, merged ranges, existing label text, cell number formats) through the IO workbook reader, combines it with a set of bindings supplied by the user or drafted by an agent, and stores the result as a **form map** inside a draft template version. Each binding says which value goes into which cell: a header field (part number, serial, date, inspector), a characteristic field (label, nominal, limits, actual, deviation, decision), or a repetition block that lays one characteristic per row. Nothing in the workbook is ever executed.

## Scope

**In:**
- `migrate.import_form@1` (W2): params form handle, bindings (optional; without them the form map holds the layout only), template name; result: template version ID, form map, import report
- The `FormMap` model (`mig-formmap@1`): source workbook digest, layout summary, bindings (`Cell { sheet, address }` or `Range`), `HeaderBinding`, `CharFieldBinding`, `RepetitionRule { sheet, first_row, last_row, step, columns: BTreeMap<Column, CharField>, order: ByCharId | ByBalloon }`, required flags, expected cell value types
- Validation of bindings against the layout (addresses exist, ranges inside sheet bounds, repetition rows not overlapping other bindings)
- The import report: macros present and ignored, formulas present (kept as untrusted text, never evaluated), external links ignored, cell count, untrusted labels
- Format check: customer workbooks (OOXML spreadsheet) are user documents and need no source-register entry; any other detected format (an incumbent template file) goes through the `MIG.SRC` gate

**Out:**
- Workbook parsing (owned by the IO workbook reader, `IO.RES.CSV.XLSX`)
- Template storage and slot semantics (owned by `RPT.MODEL.TMPL`)
- Checking against a project (owned by `MIG.TEMPL.CHECK`), filling (owned by `MIG.TEMPL.FILL`), adoption (owned by `MIG.TEMPL.ADOPT`)

## Requirements

- **REQ-MIG.TEMPL.MAP-001** When `migrate.import_form@1` succeeds, the system shall store a draft template version whose form map lists every binding with its sheet, cell or range address and bound field, plus the source workbook SHA-256.
- **REQ-MIG.TEMPL.MAP-002** If a binding names a sheet or address absent from the workbook, or a repetition rule overlaps another binding's cells, then the command shall fail with `formmap_invalid_binding` naming the binding and journal nothing.
- **REQ-MIG.TEMPL.MAP-003** All text read from the workbook (labels, sheet names, formulas, comments) shall be stored as untrusted text, and no formula, macro, external link or embedded object shall be evaluated or followed.
- **REQ-MIG.TEMPL.MAP-004** If the workbook contains macros, formulas or external links, then the import report shall state each kind with its count.
- **REQ-MIG.TEMPL.MAP-005** If the detected format is not an OOXML spreadsheet workbook, then the command shall call the `MIG.SRC` gate and fail with `source_not_admitted` unless the format's entry is `ADMITTED` or `OPEN_EXPORT_ONLY`.
- **REQ-MIG.TEMPL.MAP-006** When the principal is an agent, the template version shall be created on the agent's branch with provenance `proposed`; when the principal is human, the provenance shall be `human`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/templ/map/acme-form.xlsx` (a fictional customer form, 2 sheets, 14 header cells, a 40-row characteristic table) and `bindings.json`, when `fid migrate import-form --form <handle> --bindings <handle> --name acme-v1 --json` runs as a human, then a draft template version exists whose form map equals `expected/formmap.json` and the import report states 0 macros.
- **AC-2** Given `acme-form-macro.xlsm` (with a macro and two formulas), then the import succeeds, the report states 1 macro ignored and 2 formulas kept as untrusted text, and the test's instrumented reader confirms no formula evaluation.
- **AC-3** (error path) Given a binding to `Sheet3!B4` where Sheet3 does not exist, then the command fails with `formmap_invalid_binding` and journals nothing.
- **AC-4** (error path) Given a file detected as `native:demoreport-template` with register status `NOT_ADMITTED`, then the command fails with `source_not_admitted`.
- **AC-5** Given AC-1 run by principal `agent-1`, then the template version exists only on `agent-1`'s branch with provenance `proposed`.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `migrate.import_form@1`, W2; params `form: ImportHandle`, `bindings?: ImportHandle | inline FormBindings`, `name: String`, `stage?`; result `{ template_version, form_map_digest, import_report }`.
- **Data structures:** `FormMap` as above, embedded in the template version as a typed extension block; `FormImportReport { macros, formulas, external_links, embedded_objects, cells, sheets }`.
- **Algorithm:** resolve handle → detect format → if not format `xlsx-workbook`, `source_gate` → read layout with the IO workbook reader (bounded: at most 64 sheets, 1,048,576 rows declared, 200,000 non-empty cells [unverified limits]) → validate bindings → build `FormMap` → canonical digest → create draft template version through the `RPT.MODEL.TMPL` API on the principal's branch with provenance.
- **Errors:** `formmap_invalid_binding`, `source_not_admitted`, reader limit errors (`IO.REG.LIMITS`).
- **Determinism:** D0.
- **Performance budget:** 50,000-cell form in under 500 ms [unverified until measured].
- **Crate:** `fid-mig` (module `templ::map`).

## Test fixtures

`fixtures/mig/templ/map/`: fictional customer forms authored for the project in a spreadsheet application and saved as XLSX/XLSM, plus binding files; CC0-1.0. No real customer or incumbent template is committed.

## Build prompt

> In crate `fid-mig`, add module `templ::map` and register `migrate.import_form@1` (W2; CLI `fid migrate import-form`, MCP `migrate_import_form`). Implement spec node MIG.TEMPL.MAP: resolve the import-root handle, detect the format, call the MIG.SRC `source_gate` for anything other than an OOXML spreadsheet workbook, read the layout through the IO workbook reader (bounded; never evaluate formulas, macros, external links or embedded objects; all text untrusted), validate the supplied bindings (`formmap_invalid_binding` for missing sheets/addresses or overlapping repetition rows), build the `mig-formmap@1` form map (header bindings, characteristic-field bindings, repetition rules, required flags, expected types, source workbook SHA-256), and store it as a draft template version through the RPT template API on the principal's branch with provenance `proposed` for agents and `human` for humans. Return an import report counting macros, formulas, external links and embedded objects. Tests for AC-1..AC-6. No `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
