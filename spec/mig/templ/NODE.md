---
id: MIG.TEMPL
title: Customer form and report template recreation
level: 2
parent: MIG
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-MIG-001, REQ-MIG-007]
covers: []
depends_on: [IO.RES.CSV.XLSX, RPT.MODEL.TMPL, RPT.MODEL.BIND, RPT.MODEL.FMT, RPT.OUTPUT.META, IO.REG.ROOTS, IO.REG.WRITE, CORE.BUS.PRIN, MIG.SRC.GATE]
uses_artifacts: [MIG.COVER.TAXO]
cli: [fid migrate import-form, fid migrate check-form-map, fid migrate fill-form, fid migrate adopt-form-map]
mcp: [migrate_import_form, migrate_check_form_map, migrate_fill_form, migrate_adopt_form_map]
journal: [migrate.import_form@1, migrate.check_form_map@1, migrate.fill_form@1, migrate.adopt_form_map@1]
updated: 2026-11-17
---

# MIG.TEMPL: Customer form and report template recreation

## Purpose

Ladder stage 2. Shops have customer report forms (typically spreadsheet workbooks with fixed cells) and report layouts they built in their incumbent. This node turns a customer's blank form into a **form map**: a draft report template whose bindings place characteristic values, tolerances, decisions and header fields into named cells, so our reports fill the customer's own form. The form is the customer's document, read as data; incumbent report layouts are recreated from the user's description and our own blocks, never by copying incumbent templates.

## Scope

**In:**
- Reading a customer form workbook from an approved import root as an untrusted layout source (sheet names, cell addresses, merged ranges, existing labels as untrusted text), through the IO workbook reader
- The form-map model: cell or range to binding (characteristic field, header field, repetition row rule), stored as a draft `RPT.MODEL.TMPL` template version
- Agent-drafted form maps as proposals on an agent branch, adopted by a human (CONSTITUTION C5)
- A fill check: bind a form map to a sample project and report unbound cells, overflowing repetition rows and type mismatches
- Filling a copy of the customer's workbook from a form map, leaving all other cells unchanged
- `migrate.import_form@1` (W2 draft), `migrate.check_form_map@1` (R1), `migrate.fill_form@1` (W1, `fs_effect: write_root`), `migrate.adopt_form_map@1` (W2, `human_only`, D-031)

**Out** (and which node owns it instead):
- Workbook (XLSX) parsing (owned by `IO.RES.CSV.XLSX`, format `xlsx-workbook`)
- Template format, slots and repetition (owned by `RPT.MODEL.TMPL`)
- Binding resolution and report generation (owned by `RPT.MODEL.BIND`)
- New-workbook spreadsheet export of report tables (owned by `RPT.OUTPUT.SHEET`)
- Fixed AS9102 and PPAP forms (owned by `RPT.FORMS`)
- Deciding whether an incumbent template file format may be read (owned by `MIG.SRC`)

## Requirements

- **REQ-MIG.TEMPL-001** When a customer form workbook is imported, the system shall create a draft template version whose form map lists every bound cell or range with its address and binding, and shall leave the workbook's other cells unchanged in the filled output.
- **REQ-MIG.TEMPL-002** Text read from a customer form shall be typed as untrusted and shall never be evaluated as a formula, macro or instruction; workbooks with macros shall be read with macros ignored and the import report shall say so.
- **REQ-MIG.TEMPL-003** If an import source is an incumbent template file rather than a customer form, then the import shall go through the `MIG.SRC` gate and be refused unless the format is admitted.
- **REQ-MIG.TEMPL-004** When `migrate.check_form_map@1` runs against a project, the system shall report every unbound required cell, every repetition rule that overflows the form's rows, and every binding whose value type does not fit the cell, with counts.
- **REQ-MIG.TEMPL-005** A form map drafted by an agent shall stay on an agent branch and shall not be usable for a released report until a human adopts it.
- **REQ-MIG.TEMPL-006** Filling the same form map from the same journal head shall produce a byte-identical workbook.

## Interfaces

- **Offers:** `migrate.import_form@1` (W2), `migrate.check_form_map@1` (R1), `migrate.fill_form@1` (W1 write_root), `migrate.adopt_form_map@1` (W2, human_only); `FormMap` type (`mig-formmap@1`); per-form fill reports (`mig-fillreport@1`) read by `MIG.COVER.GEN`; a `release_block` contribution for reports bound to unadopted form maps.
- **Consumes:** IO workbook reader `IO.RES.CSV.XLSX` (layout view), `RPT.MODEL.TMPL`, `RPT.MODEL.BIND`, `RPT.MODEL.FMT`, `RPT.OUTPUT.META`, `IO.REG.ROOTS`, `IO.REG.WRITE`, `CORE.BUS.PRIN`, `MIG.SRC.GATE`; `form-xlsx` construct IDs from `MIG.COVER.TAXO` (artifact).

## Decomposition hints

READ (bounded workbook layout reader), MAP (form-map model and draft template), CHECK (fill check), AGENT (proposal and adoption path).

## Open questions

- Resolved: the bounded workbook reader is owned by IO as `IO.RES.CSV.XLSX` (layout view for forms), so the READ hint is not a child here.
- Filling a customer's own workbook (patching bound cells, keeping everything else) is not covered by `RPT.OUTPUT.SHEET`, which writes new workbooks. `MIG.TEMPL.FILL` owns it; RPT reviewers may prefer to move it under `RPT.OUTPUT`.
- `migrate.fill_form@1` and `migrate.adopt_form_map@1` are added beyond the original command list: REQ-001/006 need a fill command, and REQ-005 needs a `human_only` adoption command (D-031).

## Split rationale

Split by **command and side-effect class**: create the draft form map (MAP, W2), check it against a project (CHECK, R1), write the filled workbook (FILL, W1 write_root), and adopt it (ADOPT, W2 human_only). The READ hint moved to IO, which owns all format parsing.

Alternatives considered:
- **CHECK inside MAP.** Rejected: the check is a pure, repeatable read used by `MIG.COVER` over a corpus; mapping writes drafts.
- **Adoption through generic branch promotion only.** Rejected: C5 needs a provenance label on the form map and a release block, which a branch merge alone does not give.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [MIG.TEMPL.MAP](map/NODE.md) | Form import and form-map draft template (leaf) | 001 002 003 | none |
| [MIG.TEMPL.CHECK](check/NODE.md) | Form-map fill check and fill report (leaf) | 004 | none |
| [MIG.TEMPL.FILL](fill/NODE.md) | Filling a copy of the customer workbook (leaf) | 001 006 | none |
| [MIG.TEMPL.ADOPT](adopt/NODE.md) | Agent provenance and human adoption of form maps (leaf) | 005 | none |
