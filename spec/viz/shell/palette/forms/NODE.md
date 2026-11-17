---
id: VIZ.SHELL.PALETTE.FORMS
title: Schema-generated argument forms with units and exact decimals
level: 4
parent: VIZ.SHELL.PALETTE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.PALETTE-002, REQ-VIZ.SHELL.PALETTE-006]
covers: []
depends_on: [VIZ.SHELL.PALETTE.SEARCH, CORE.REG.DEF.SCHEMA, CORE.BUS.UNIT, VIZ.SHELL.A11Y.AXTREE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.PALETTE.FORMS: Schema-generated argument forms with units and exact decimals

## Purpose

Generate an argument form for any registry command from its parameter schema: numbers with their quantity kind and explicit SI unit (and the user's display unit when different), enums as choices, entity references as pickers fed by the current selection, strings marked untrusted, nested objects and arrays. Decimal input is passed as the exact typed string with its unit, so the bus does the single journaled conversion. The same generator renders the inspector's editors.

## Scope

**In:** schema-to-form generator for the registry keyword dialect (`x-quantity`, `x-unit` and the other project keywords); per-field validation before submit; entity-reference picker fed through a `SelectionSource` trait; exact decimal strings; error display from bus validation errors; form widget IDs derived from command ID and parameter path.
**Out:** the palette list (`VIZ.SHELL.PALETTE.SEARCH`); unit conversion (`CORE.BUS.UNIT`); display-unit preferences (`I18N.UNITS`); viewport picking (`VIZ.PICK`, which supplies selections as data).

## Requirements

- **REQ-VIZ.SHELL.PALETTE.FORMS-001** For every parameter with `x-quantity`, the form shall show the unit symbol next to the field and shall reject a submission whose unit is not of that quantity kind.
- **REQ-VIZ.SHELL.PALETTE.FORMS-002** Decimal input shall be passed to the bus as the exact typed string with its unit, with no conversion through a binary float in the shell.
- **REQ-VIZ.SHELL.PALETTE.FORMS-003** The generator shall render every schema construct of the registry dialect used in the manifest, and CI shall fail if any manifest command's schema produces an unsupported-construct placeholder.
- **REQ-VIZ.SHELL.PALETTE.FORMS-004** Locale-formatted input (for example a decimal comma) shall be normalised to the locale-invariant decimal string before submission, and the submitted payload shall be identical under every UI language.
- **REQ-VIZ.SHELL.PALETTE.FORMS-005** Field widget IDs shall be `<command id>/<parameter path>`, and every field shall have an accessible name from the parameter's catalog label.

## Acceptance criteria

- **AC-1** Given fixture manifest `fixtures/viz/palette/manifest-200.json`, when forms are generated for every command, then none contains an unsupported-construct placeholder.
- **AC-2** Given a length parameter and input `12.3450` with unit `mm`, then the submitted payload holds `"12.3450"` and `"mm"` exactly.
- **AC-3** Given German UI and input `12,5`, then the payload holds `"12.5"`, identical to the English UI payload.
- **AC-4** (error path) Given unit `deg` for a length parameter, then the form shows a unit-kind error and nothing is submitted.
- **AC-5** Given the accessibility inspection, then each field ID is `<command id>/<path>` and its name equals the catalog label.

## Design

- **Registry command(s):** none.
- **Data structures:** `FormModel` tree from schema; `FieldValue::Decimal { text, unit }`; `trait SelectionSource { fn current(&self) -> Vec<EntityRef>; }`.
- **Algorithm:** walk schema; map keywords to widgets; validate locally (kind, enum, bounds as given in schema); normalise locale input via the I18N parse function supplied through THEME.L10N; submit via `ShellBus`.
- **Errors:** `UnitKindMismatch`, `UnsupportedConstruct` (CI), bus validation errors shown inline.
- **Determinism:** D0 (payload from input).
- **Performance budget:** form for a 50-field command under 5 ms.
- **Crate:** `fid-shell` (module `palette::forms`).

## Test fixtures

`fixtures/viz/palette/manifest-200.json`.

## Build prompt

> In crate `fid-shell`, add `palette::forms`: generate argument forms from registry parameter schemas (JSON Schema 2020-12 plus project keywords), showing quantity units, enums, entity pickers fed by a `SelectionSource` trait, untrusted strings, objects and arrays. Pass decimals as exact strings with units (no binary float), normalise locale input to invariant decimals before submit, check unit kinds, and give each field widget ID `<command id>/<parameter path>` and its catalog label as accessible name. CI fails on any unsupported schema construct in the manifest. Tests AC-1..AC-5 with `fixtures/viz/palette/manifest-200.json`. Rules: CONSTITUTION §4 (authored decimals keep value and unit; one journaled conversion in the bus), §8; C6.

## Open questions

- None.

## Children

_Leaf: none._
