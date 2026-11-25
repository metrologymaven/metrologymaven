---
id: UX.COMP.CMD
title: Command component specs (palette, menus, toolbar, approval chips, argument forms)
level: 3
parent: UX.COMP
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.COMP-001, REQ-UX.COMP-002, REQ-UX.COMP-003, REQ-UX.COMP-006, REQ-UX.COMP-008]
covers: []
depends_on: [UX.COMP.SCHEMA, UX.ICONS.CLASS]
uses_artifacts: [CORE.REG.MAN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-24
---

# UX.COMP.CMD: Command component specs

## Purpose

The components through which people invoke registry commands: the command palette, menus and context menus, the toolbar, approval chips, and schema-generated argument forms. Because menus, toolbars and forms are generated from the registry manifest (CONSTITUTION C3), the specs say how a manifest entry maps to each component, not which commands appear.

## Scope

**In:** `components/palette.md`, `components/menu.md`, `components/toolbar.md`, `components/approval-chip.md`, `components/arg-form.md`; the manifest-to-component mapping table.
**Out:** the palette flow across steps (`UX.PATTERN.CMDFLOW`); proposal review (`UX.PATTERN.PROPOSE`); generating the components (VIZ.SHELL); command schemas (CORE.REG).

## Requirements

- **REQ-UX.COMP.CMD-001** The argument-form spec shall map each manifest parameter type (quantity with unit, enum, boolean, string, entity reference, handle to an approved root, list) to one control, and every quantity field shall show its unit explicitly and offer only units of the parameter's quantity kind.
- **REQ-UX.COMP.CMD-002** The argument-form spec shall forbid a raw file-path field for any principal and shall use the approved-root handle picker instead.
- **REQ-UX.COMP.CMD-003** Every command entry in palette, menu and toolbar shall show the side-effect class icon for W3, X1, D1, M1, S1 and A0 commands, and the approval chip shall always show the class icon and the command count with text.
- **REQ-UX.COMP.CMD-004** When a command is unavailable, the palette and menu specs shall show it disabled with its refusal reason as text, reachable by keyboard, rather than hiding it, unless the principal may never run it.
- **REQ-UX.COMP.CMD-005** Each spec shall pass `fid-design comp-check`.

## Acceptance criteria

- **AC-1** Given the five shipped specs, when `fid-design comp-check` runs, then it exits 0.
- **AC-2** Given the mapping table and fixture manifest `fixtures/ux/comp/cmd/manifest-sample.json`, when a reviewer maps each parameter type, then every type in the sample has exactly one control row (design-review checklist item).
- **AC-3** Given `components/arg-form.md`, then it contains no raw-path control and names the handle picker (review item).

## Design

- **Deliverable:** five Markdown specs in `component-spec@1` and `components/notes/manifest-mapping.md`.
- **Determinism:** D0.
- **Crate:** none (package content); checked by `fid-design comp-check`.

## Test fixtures

`fixtures/ux/comp/cmd/manifest-sample.json`: a synthetic manifest excerpt with one parameter of each type; CC-BY-4.0.

## Build prompt

> Write five component specs in `component-spec@1`: `palette.md` (search field, result list with title, class icon, shortcut, disabled reason), `menu.md` (menus and context menus), `toolbar.md`, `approval-chip.md` (class icon, command count as text, count confirmation hook), `arg-form.md` (one control per manifest parameter type; quantity fields with explicit units restricted to the quantity kind; no raw file-path field, use the approved-root handle picker; dry-run button where the command supports dry-run). Add `components/notes/manifest-mapping.md` mapping manifest fields to component parts. Show class icons for W3, X1, D1, M1, S1, A0 commands; show unavailable commands disabled with a text reason. Original wording and diagrams. Run `fid-design comp-check` until it exits 0, then open a design-review record.

## Open questions

- The manifest mapping lives in `components/notes/`, not in `components/`: `fid-design comp-check` treats every `*.md` directly under `components/` as a `component-spec@1` file (UX.COMP.SCHEMA) and does not walk subdirectories.
- The manifest has no icon or title-style field yet (CORE.REG.MAN); the mapping uses the command ID and class until it does.

## Children

_Leaf: none._
