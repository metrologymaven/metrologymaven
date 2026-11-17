---
id: RPT.DESIGN.UI.PANEL
title: Block palette, property panel, template browser and undo
level: 4
parent: RPT.DESIGN.UI
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN.UI-001, REQ-RPT.DESIGN.UI-002]
covers: [PCD.RPT.02, PW.RPT.01]
depends_on: [RPT.MODEL.DOC, RPT.MODEL.BLOCKS, RPT.MODEL.TMPL, RPT.DESIGN.PAGE, RPT.DESIGN.STYLE.SET, RPT.DESIGN.STYLE.OVR, CORE.JRN.UNDO, VIZ.SHELL.PALETTE]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.DESIGN.UI.PANEL: Block palette, property panel, template browser and undo

## Purpose

The side panels of the report designer: a palette to add blocks, a property panel to edit the selected block's payload, the page setup and the styles, a template browser to save and apply templates, and undo and redo buttons. Each panel edit is an existing registry command, and its forms are generated from the commands' parameter schemas by the shell's form generator, so the panel stays a thin UI.

## Scope

**In:** block palette (`report.add_block@1` with a default payload per kind); property panel (block payload commands such as `report.set_table_block@1`, `report.set_drawing_balloon@1`, page layout, style and block-style commands) using generated forms; template browser (`report.list_template@1`, `report.save_template@1`, `report.apply_template@1`); undo and redo through `journal.undo@1` and `journal.redo@1`; edit coalescing rule (one command per committed field edit).

**Out:** canvas and handles (`RPT.DESIGN.UI.CANVAS`); keyboard shortcuts and accessibility labels (`RPT.DESIGN.UI.KEYS`); the commands themselves (their owners).

## Requirements

- **REQ-RPT.DESIGN.UI.PANEL-001** Each committed field edit in the property panel shall issue exactly one existing registry command, and the panel shall hold no report state beyond uncommitted field text.
- **REQ-RPT.DESIGN.UI.PANEL-002** When the user presses undo, the panel shall issue `journal.undo@1` for the report's branch, and redo shall issue `journal.redo@1`, so that the designer and model return to the previous and next states.
- **REQ-RPT.DESIGN.UI.PANEL-003** Property forms shall be generated from the registered command parameter schemas, with no hand-written field lists.
- **REQ-RPT.DESIGN.UI.PANEL-004** The template browser shall show template name, version and digest and shall apply templates only through `report.apply_template@1`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/design/one_page_report` in a UI test harness, when a table block is added from the palette and its column list edited and committed, then exactly two commands are observed on the bus.
- **AC-2** When undo is pressed twice, then the report equals its initial state (digest) and redo twice restores the edited state.
- **AC-3** A test asserts that every property form in the panel is produced by the generated-forms API from a command schema.
- **AC-4** Saving and applying a template from the browser issues `report.save_template@1` and `report.apply_template@1` and shows the new version and digest.

## Design

- **Registry commands:** none (issues commands owned by `RPT.MODEL.*`, `RPT.DESIGN.*` and `CORE.JRN.UNDO`).
- **Structure:** egui panels in the report-designer surface crate, using the shell's generated forms and menus.
- **Determinism:** N (interactive surface).
- **Performance budget:** panel refresh under 16 ms on selection change [unverified until measured].
- **Crate:** `fid-report-designer` (L3 surface crate).

## Test fixtures

- `fixtures/rpt/design/one_page_report` and a template built by a test builder.

## Build prompt

> In the L3 surface crate `fid-report-designer`, implement the designer side panels: block palette (adds blocks with `report.add_block@1`), property panel with forms generated from command parameter schemas (block payload, page layout, style and block-style commands), template browser (list, save, apply; shows name, version, digest), and undo/redo buttons issuing `journal.undo@1` and `journal.redo@1`. One registry command per committed field edit; no report state in the panel. Tests AC-1 to AC-4 in a UI test harness. CONSTITUTION §8 and C3 (everything through the registry). Spec: `spec/rpt/design/ui/panel/NODE.md`.

## Open questions

- None yet.

## Children

_Leaf: none._
