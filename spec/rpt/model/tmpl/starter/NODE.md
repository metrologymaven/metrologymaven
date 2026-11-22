---
id: RPT.MODEL.TMPL.STARTER
title: Built-in starter templates
level: 4
parent: RPT.MODEL.TMPL
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-report]
satisfies: [REQ-RPT.MODEL.TMPL-010]
covers: [GOM.RPT.02, PCD.RPT.01, PW.RPT.02, SA.RPT.01]
depends_on: [RPT.MODEL.TMPL.FORMAT, RPT.MODEL.TMPL.SLOT, RPT.MODEL.TMPL.VER]
uses_artifacts: [I18N.CATALOG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.MODEL.TMPL.STARTER: Built-in starter templates

## Purpose

Three ready-made templates so a first report takes one command: a characteristic table, a deviation summary and a multi-stage comparison table. They are data in the template format, registered through the built-in registry, and serve as the reference fixtures for generation tests across RPT.

## Scope

**In:** `tmpl:char-table`, `tmpl:deviation-summary`, `tmpl:multistage-table` as canonical JSON; their label keys and English source text; registration; an end-to-end test that generates each against the reference project.

**Out:** form templates (`RPT.FORMS.LAYOUT`); styles (`RPT.DESIGN.STYLE`); the generator (`RPT.MODEL.BIND.GEN`), which this leaf exercises only through the `fid` binary in an integration test, not as a code dependency.

## Requirements

- **REQ-RPT.MODEL.TMPL.STARTER-001** The crate shall register three unlocked built-in templates: a characteristic table (one row per characteristic with ID, nominal, limits, measured, deviation, decision), a deviation summary (counts per outcome, then nonconforming rows only) and a multi-stage comparison table (one row per characteristic, one measured-value column per stage).
- **REQ-RPT.MODEL.TMPL.STARTER-002** Each starter template shall pass format and slot validation, and its labels shall be catalog keys with English source text.
- **REQ-RPT.MODEL.TMPL.STARTER-003** When each starter template is generated against fixture `fixtures/rpt/model/bracket_3stage` with `fid report generate-report`, the result shall have no warnings and shall equal a committed golden digest.

## Acceptance criteria

- **AC-1** Given the built crate, when `fid report list-template --json` runs, then the three starter IDs appear, unlocked, version 1.
- **AC-2** Given the three templates, when validated, then format and slot validation pass.
- **AC-3** (integration, runs once `RPT.MODEL.BIND.GEN` exists) Given fixture `fixtures/rpt/model/bracket_3stage`, when `fid report generate-report --template <each> --json` runs, then the warnings list is empty and the digests equal `fixtures/rpt/model/golden/starter-*.digest`.

## Design

- **Data:** three template documents under `templates/starter/` using `table` and `text` blocks with row slots (characteristics, ascending ID) and, for the multi-stage table, a column slot over stages.
- **Registration:** `BuiltinRegistry::register` at crate initialisation.
- **Errors:** registration validation failure fails the crate's startup test.
- **Determinism:** D0.
- **Crate:** `fid-report` (data under `templates/starter/`).

## Test fixtures

`fixtures/rpt/model/bracket_3stage` (shared) and golden digests; CC-BY-4.0.

## Build prompt

> In crate `fid-report`, add three built-in, unlocked starter templates as canonical-JSON template documents (`tmpl:char-table`, `tmpl:deviation-summary`, `tmpl:multistage-table`) with catalog-key labels, registered through the built-in template registry. Add an integration test (in the workspace test harness, driving the `fid` binary) that generates each against `fixtures/rpt/model/bracket_3stage` and checks zero warnings and golden digests. Acceptance: AC-1..AC-3 in `spec/rpt/model/tmpl/starter/NODE.md`. Constraints: CONSTITUTION C9 (original content), §8.

## Open questions

- The integration test runs the `fid` binary rather than linking the generator, which avoids a dependency cycle between TMPL and BIND.

## Children

_Leaf: none._
