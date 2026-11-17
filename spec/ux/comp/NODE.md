---
id: UX.COMP
title: Component library specification
level: 2
parent: UX
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UX-001, REQ-UX-004]
covers: []
depends_on: [UX.TOKENS, UX.ICONS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.COMP: Component library specification

## Purpose

Toolkit-neutral specifications of the reusable UI components every app is built from: what each component is made of, its states, its keyboard behaviour, its accessible role and the tokens it uses. VIZ implements these specs under whatever toolkit D-005 confirms. It is separate because components are the unit VIZ builds and tests against, and because the hardest ones (element tree, characteristic table, report designer surfaces) are exactly the ones the D-005 spikes measure.

## Scope

**In:**
- Data components: element tree, results and characteristic table, property grid (inspector), legend frame, value display (value, unit, provenance badge, status glyph)
- Command components: command palette, menus, toolbar, approval chips, schema-generated argument forms with explicit units
- Containers: panels, tabs, wizard and stepper (including the workflow rail), dialog, toast and notification, status bar
- The "Why?" panel layout
- Per component: anatomy, states, keyboard map, accessible role, name and state, token usage, density variants, reference renderings

**Out** (and which node owns it instead):
- Cross-component behaviours such as selection, undo, proposal review and operator prompts (`UX.PATTERN`)
- Token values and icons (`UX.TOKENS`, `UX.ICONS`)
- Component implementation, virtualisation and performance (`VIZ.SHELL` and its children)
- Value formatting itself (kernel formatters; `RPT.MODEL.FMT` for reports)
- Report designer canvas behaviour (`RPT.DESIGN.UI`; report styling in `UX.REPORT`)

## Requirements

- **REQ-UX.COMP-001** Each component spec shall define anatomy, the states default, hover, focus, active, disabled, read-only, error and loading where applicable, the tokens used, and compact and comfortable density variants.
- **REQ-UX.COMP-002** Each interactive component spec shall define a complete keyboard interaction, and no action in a spec shall be reachable by pointer only.
- **REQ-UX.COMP-003** Each component spec shall name the accessible role, name source and states that the implementation must expose.
- **REQ-UX.COMP-004** Each component spec shall define a visible focus indicator that meets WCAG 2.2 SC 2.4.7 and the non-text contrast of REQ-UX.TOKENS-003.
- **REQ-UX.COMP-005** The value display component shall show the formatter's output string verbatim with its unit and provenance badge, and the spec shall forbid re-rounding or re-formatting a value of record in the UI.
- **REQ-UX.COMP-006** Every component that shows a conformance status shall show it with at least two cues, a glyph and text, in addition to any colour.
- **REQ-UX.COMP-007** The "Why?" panel spec shall define sections for the value, algorithm ID and version, inputs with digests, decision rule and outcome, uncertainty class (UEC-0 to 3) and journal entry, each linking to its source entity.
- **REQ-UX.COMP-008** When a control is disabled, its spec shall require the reason to be available as text without pointer hover alone.
- **REQ-UX.COMP-009** The tree and table specs shall define behaviour for at least 10⁴ items: sort, filter, column configuration, multi-select and keyboard paging, with no behaviour that requires loading every row into view.
- **REQ-UX.COMP-010** Each component spec shall include reference renderings in all four themes at 100 % and 200 % scale.

## Interfaces

- **Offers:** component specs in the design-system package; reference renderings that VIZ can use as visual-regression baselines.
- **Consumes:** `UX.TOKENS`, `UX.ICONS`. Interface notes (no `depends_on`): `VIZ.SHELL` implements the specs; the D-005 spikes (`VIZ.SHELL.SPKTREE`, `VIZ.SHELL.SPKTABLE`, `VIZ.SHELL.SPKRPT`) should test against the tree and table specs here, and a spike failure may force spec changes; the registry manifest (CORE) supplies command schemas for generated argument forms.

## Decomposition hints

DATA (tree, table, property grid, value display), CMD (palette, menus, approval chips, argument forms), CONT (panels, wizard and rail, dialog, toast, status bar), WHY (the "Why?" panel), REF (reference renderings and visual baselines).

## Split rationale

The dividing principle is **component family, plus a shared format and a shared rendering set**: SCHEMA (the `component-spec@1` format and the check that enforces anatomy, states, keyboard, accessibility, focus and disabled reasons), DATA (tree, table, property grid, legend frame, value display), CMD (palette, menus, toolbar, approval chips, argument forms), CONT (panels, tabs, stepper and rail, dialog, toast, status bar), WHY (the "Why?" panel), REF (reference renderings in four themes at two scales). The format and check come first so every family is checked the same way; the renderings come last because they depend on all specs.

Alternatives considered:
- **WHY inside DATA.** Rejected: the "Why?" panel has its own content contract (REQ-UX.COMP-007) tied to the data model and FLOW.DEADEND.WHY coverage.
- **Renderings inside each family.** Rejected: one completeness and staleness check over all renderings is simpler, and VIZ consumes them as one baseline set.

## Open questions

- Toolkit neutrality after the D-005 spikes is still an assumption to confirm; the specs avoid toolkit terms, and any spike-forced change goes through the package changelog (UX.COMP.DATA AC-4).
- VIZ.SHELL spikes (SPKTREE, SPKTABLE, SPKRPT) should test against UX.COMP.DATA and may use UX.COMP.REF baselines; they would declare `uses_artifacts` on those nodes (not editable from here).
- The registry manifest lacks icon and title-style fields used by UX.COMP.CMD's mapping (CORE.REG).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [UX.COMP.SCHEMA](schema/NODE.md) | Component-spec format and completeness check (leaf) | 001 002 003 004 008 | none |
| [UX.COMP.DATA](data/NODE.md) | Data component specs (leaf) | 001 002 003 005 006 009 | none |
| [UX.COMP.CMD](cmd/NODE.md) | Command component specs (leaf) | 001 002 003 006 008 | none |
| [UX.COMP.CONT](cont/NODE.md) | Container component specs (leaf) | 001 002 003 004 006 008 | none |
| [UX.COMP.WHY](why/NODE.md) | The "Why?" panel spec (leaf) | 001 002 003 006 007 | none |
| [UX.COMP.REF](ref/NODE.md) | Component reference renderings and visual baselines (leaf) | 010 | none |
