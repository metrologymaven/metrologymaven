---
id: UX.COMP.DATA
title: Data component specs (tree, table, property grid, legend frame, value display)
level: 3
parent: UX.COMP
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.COMP-001, REQ-UX.COMP-002, REQ-UX.COMP-003, REQ-UX.COMP-005, REQ-UX.COMP-006, REQ-UX.COMP-009]
covers: []
depends_on: [UX.COMP.SCHEMA, UX.ICONS.CLASS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.COMP.DATA: Data component specs

## Purpose

The specs of the components that show measurement data: the element tree, the results and characteristic table, the property grid (inspector), the legend frame, and the value display (value, unit, provenance badge, status glyph). These are the components the D-005 spikes measure, so their large-data behaviour is specified precisely and without assuming a toolkit.

## Scope

**In:** `components/tree.md`, `components/table.md`, `components/property-grid.md`, `components/legend-frame.md`, `components/value.md`, each in `component-spec@1`.
**Out:** virtualisation and performance (VIZ.SHELL); legend content and colour-map scale editing (VIZ.OVERLAY); value formatting (kernel formatters; RPT.MODEL.FMT); selection sync across views (`UX.PATTERN.SELECT`); reference renderings (`UX.COMP.REF`).

## Requirements

- **REQ-UX.COMP.DATA-001** The tree and table specs shall define sort, filter, column configuration (table), multi-select (range and toggle), type-ahead find and keyboard paging (Page Up, Page Down, Home, End) for at least 10^4 items, and shall state that only visible rows need to be materialised and that scroll position, selection and focus survive a data refresh.
- **REQ-UX.COMP.DATA-002** The value display spec shall show the formatter's output string verbatim, its unit and exactly one provenance badge, and shall forbid re-rounding, truncation or re-formatting of a value of record; when width is short, it shall elide with a visible marker and expose the full string to the accessible name and a focus-reachable tooltip.
- **REQ-UX.COMP.DATA-003** Every data component that shows a conformance status shall show the status glyph and the status text in addition to any colour, including in compact density.
- **REQ-UX.COMP.DATA-004** The property-grid spec shall mark read-only fields with a lock cue and text, show units next to every editable quantity field, and state that edits are submitted as registry commands, not applied locally.
- **REQ-UX.COMP.DATA-005** Each spec shall pass `fid-design comp-check`.

## Acceptance criteria

- **AC-1** Given the five shipped specs, when `fid-design comp-check --root components --package design --json` runs, then it exits 0.
- **AC-2** Given `components/value.md`, when a reviewer reads its `## Behaviour` section, then it contains the verbatim-formatter rule, the elision rule and the single-badge rule (design-review checklist item).
- **AC-3** Given `components/table.md`, when a reviewer checks it against REQ-001, then every listed behaviour has a keyboard entry in the front block (verified by `comp-check` keyboard coverage).
- **AC-4** Given the VIZ.SHELL tree and table spikes, when they report results, then any spec change they force is recorded as a changelog entry in the package (process check).

## Design

- **Deliverable:** five Markdown specs in `component-spec@1`, written in the project's own words with original diagrams.
- **Key content:** tree rows (icon, name, status glyph, count badge), lazy expansion, keyboard Left/Right expand-collapse; table columns with header sort, per-column filter, pinned columns, sticky header, cell edit by command; property grid groups with units; legend frame (title, scale area, unit, palette ID, out-of-range swatches); value display (string, unit, badge, glyph, "Why?" affordance).
- **Determinism:** D0 (checked text artefacts).
- **Crate:** none (design-system package content); checked by `fid-design comp-check`.

## Test fixtures

The shipped specs are the fixtures for `comp-check`; synthetic example data in the specs is CC-BY-4.0.

## Build prompt

> Write five component specs in `component-spec@1` (see `components/TEMPLATE.md`) in the design-system package: `tree.md`, `table.md`, `property-grid.md`, `legend-frame.md`, `value.md`. Cover anatomy, all applicable states, compact and comfortable density, a complete keyboard map (sort, filter, column configuration, range and toggle multi-select, type-ahead, Page Up/Down, Home/End, expand/collapse), accessible role, name and states, focus indicator, disabled reasons and tokens per part. The tree and table must work for 10^4+ items with only visible rows materialised and must keep scroll, selection and focus across refresh. The value display shows the formatter's string verbatim with unit and exactly one provenance badge, never re-rounds, elides with a marker and exposes the full string accessibly. Every status display uses glyph and text besides colour. Property-grid edits are registry commands. Use original wording and diagrams only. Run `fid-design comp-check` until it exits 0, then open a design-review record.

## Open questions

- If a D-005 spike fails and the toolkit changes, these specs should need no change because they are toolkit-neutral; to confirm after the spikes.

## Children

_Leaf: none._
