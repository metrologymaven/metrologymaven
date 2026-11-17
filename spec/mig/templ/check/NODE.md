---
id: MIG.TEMPL.CHECK
title: Form-map fill check and fill report
level: 3
parent: MIG.TEMPL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.TEMPL-004]
covers: []
depends_on: [MIG.TEMPL.MAP, RPT.MODEL.BIND, RPT.MODEL.FMT, CORE.JRN.CANON]
uses_artifacts: [MIG.COVER.TAXO]
side_effect: R1
determinism: D0
cli: [fid migrate check-form-map]
mcp: [migrate_check_form_map]
journal: [migrate.check_form_map@1]
updated: 2026-11-17
---

# MIG.TEMPL.CHECK: Form-map fill check and fill report

## Purpose

Before a shop trusts a recreated form, it wants to know whether the form map actually fits its data. `migrate.check_form_map@1` binds a form map to a project run at a journal head (through `RPT.MODEL.BIND`, as a scratch result) and reports, with counts, every required cell left unbound, every repetition block that has more characteristics than rows, and every value whose type or format does not fit its cell. The same report (`mig-fillreport@1`), tagged with taxonomy construct IDs, is what `MIG.COVER.GEN` counts across the form corpus.

## Scope

**In:**
- `migrate.check_form_map@1` (R1): params template version (with form map), project run, journal head (default current)
- Findings: `UNBOUND_REQUIRED`, `REPETITION_OVERFLOW` (needed rows, available rows), `TYPE_MISMATCH` (expected cell type versus value type, e.g. number cell receiving text or a decision), `FORMAT_LOSS` (value decimals exceed the cell's number format)
- The fill report with per-binding status `MAPPED`, `PARTIAL`, `UNSUPPORTED` and `form-xlsx` construct IDs (header cell, characteristic cell, repetition block, merged range), plus summary counts

**Out:**
- Creating the form map (owned by `MIG.TEMPL.MAP`)
- Writing a filled workbook (owned by `MIG.TEMPL.FILL`)
- Binding resolution (owned by `RPT.MODEL.BIND`)

## Requirements

- **REQ-MIG.TEMPL.CHECK-001** When `migrate.check_form_map@1` runs, the system shall resolve every binding of the form map against the run at the given journal head and report each required binding with no resolved value as `UNBOUND_REQUIRED`.
- **REQ-MIG.TEMPL.CHECK-002** For each repetition rule, the report shall state the number of rows needed and available, and shall report `REPETITION_OVERFLOW` when needed exceeds available.
- **REQ-MIG.TEMPL.CHECK-003** For each bound value, the report shall flag `TYPE_MISMATCH` when the value type does not fit the cell's expected type, and `FORMAT_LOSS` when the formatted value has more decimals than the cell's number format shows.
- **REQ-MIG.TEMPL.CHECK-004** The report shall give counts per finding kind and per binding status, and each binding item shall carry a `form-xlsx` taxonomy construct ID.
- **REQ-MIG.TEMPL.CHECK-005** The check shall not change any document, branch or file, and the same template version, run and journal head shall produce a byte-identical report.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/templ/check/acme/` (the MAP fixture's form map and a run with 45 characteristics, two header fields missing), when `fid migrate check-form-map --template <v> --run r1 --json` runs, then the report equals `expected/fillreport.json`: 2 `UNBOUND_REQUIRED`, 1 `REPETITION_OVERFLOW` (45 needed, 40 available), 0 `TYPE_MISMATCH`.
- **AC-2** Given a binding of a decision field to a numeric cell, then a `TYPE_MISMATCH` finding names it.
- **AC-3** Given an actual value with 4 decimals into a cell formatted with 3, then a `FORMAT_LOSS` finding names it.
- **AC-4** Given AC-1, then no journal entry, branch head or file changed, and a second run gives a byte-identical report.

## Design

- **Registry command(s):** `migrate.check_form_map@1`, R1; params `template: TemplateVersionId`, `run: RunId`, `head?: JournalHead`; result `FillReport`.
- **Data structures:** `FillReport { schema: "mig-fillreport@1", template_version, form_map_digest, source_workbook_sha256, head, items: Vec<FillItem>, findings: Vec<Finding>, summary }`, `FillItem { binding_id, construct_id, status, reason? }`.
- **Algorithm:** load form map → `report.generate_report@1`-equivalent resolution through `RPT.MODEL.BIND` (scratch) → per binding: value presence, type fit, decimals versus cell format (format parsed from the layout's number-format string, only a bounded subset [unverified coverage of number-format syntax]) → per repetition rule: row count → sort → canonical JSON.
- **Errors:** `template_not_found`, `template_has_no_form_map`, `run_not_found`.
- **Determinism:** D0.
- **Performance budget:** 2,000 bindings in under 300 ms [unverified until measured].
- **Crate:** `fid-mig` (module `templ::check`).

## Test fixtures

`fixtures/mig/templ/check/`: fictional forms and synthetic runs, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `templ::check` and register `migrate.check_form_map@1` (R1; CLI `fid migrate check-form-map`, MCP `migrate_check_form_map`). Implement spec node MIG.TEMPL.CHECK: resolve every form-map binding against a run at a journal head through the RPT binding engine (scratch, no writes); report `UNBOUND_REQUIRED`, `REPETITION_OVERFLOW` (needed vs available rows), `TYPE_MISMATCH`, `FORMAT_LOSS` (decimals vs the cell number format, bounded subset of format syntax); emit `mig-fillreport@1` with per-binding status `MAPPED|PARTIAL|UNSUPPORTED`, `form-xlsx` construct IDs and summary counts; RFC 8785 canonical JSON, byte-identical for identical inputs. Tests for AC-1..AC-4. No `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
