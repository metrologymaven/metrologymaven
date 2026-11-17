---
id: MIG.TEMPL
title: Customer form and report template recreation
level: 2
parent: MIG
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-MIG-001, REQ-MIG-007]
covers: []
depends_on: [RPT.MODEL.TMPL, RPT.MODEL.BIND, RPT.OUTPUT.SHEET, IO.REG.ROOTS, MIG.SRC]
cli: [fid migrate import-form, fid migrate check-form-map]
mcp: [migrate_import_form, migrate_check_form_map]
journal: [migrate.import_form@1]
updated: 2026-11-16
---

# MIG.TEMPL: Customer form and report template recreation

## Purpose

Ladder stage 2. Shops have customer report forms (typically spreadsheet workbooks with fixed cells) and report layouts they built in their incumbent. This node turns a customer's blank form into a **form map**: a draft report template whose bindings place characteristic values, tolerances, decisions and header fields into named cells, so our reports fill the customer's own form. The form is the customer's document, read as data; incumbent report layouts are recreated from the user's description and our own blocks, never by copying incumbent templates.

## Scope

**In:**
- Reading a customer form workbook from an approved import root as an untrusted layout source (sheet names, cell addresses, merged ranges, existing labels as untrusted text)
- The form-map model: cell or range to binding (characteristic field, header field, repetition row rule), stored as a draft `RPT.MODEL.TMPL` template version
- Agent-drafted form maps as proposals on an agent branch, adopted by a human (CONSTITUTION C5)
- A fill check: bind a form map to a sample project and report unbound cells, overflowing repetition rows and type mismatches
- `migrate.import_form@1` (W2 draft), `migrate.check_form_map@1` (R1)

**Out** (and which node owns it instead):
- Template format, slots and repetition (owned by `RPT.MODEL.TMPL`)
- Binding resolution and report generation (owned by `RPT.MODEL.BIND`)
- Writing the filled workbook (owned by `RPT.OUTPUT.SHEET`)
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

- **Offers:** `migrate.import_form@1` (W2), `migrate.check_form_map@1` (R1); `FormMap` type; per-form fill reports for `MIG.COVER`.
- **Consumes:** `RPT.MODEL.TMPL`, `RPT.MODEL.BIND`, `RPT.OUTPUT.SHEET`, `IO.REG.ROOTS` import handles, `MIG.SRC` gate.

## Decomposition hints

READ (bounded workbook layout reader), MAP (form-map model and draft template), CHECK (fill check), AGENT (proposal and adoption path).

## Open questions

- Workbook reading needs a bounded spreadsheet reader; `IO` has no workbook reader node yet. If `IO` should own it, a reviewer should move READ there.

## Children

_None yet: status `open`._
