---
id: RPT.DESIGN
title: Report designer
level: 2
parent: RPT
kind: subsystem
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT-001]
covers: [PCD.RPT.02, PW.RPT.01]
depends_on: [RPT.MODEL, VIZ]
cli: [fid report *-layout, fid report *-style]
mcp: [report_*_layout, report_*_style]
journal: [report.*_layout, report.*_style]
updated: 2026-11-16
---

# RPT.DESIGN: Report designer

## Purpose

The interactive editor where users build and adjust reports and templates: place and size blocks on pages, set page format and styles, preview the generated result and save as a template. Every edit is a registry command on the report model, so the designer is a thin UI over commands that agents and scripts can also call. It is separate from the model because it is UI and layout policy, and it ships in a later phase (P1b, CONSTITUTION §7).

## Scope

**In:**
- Page setup: size, orientation, margins, header and footer areas
- Block placement, sizing, alignment, grouping and z-order
- Style sets: fonts, colours, table styles, logo; style sets saved with templates
- Live preview of the resolved report
- "Save as template" and "apply template" from the designer
- Undo and redo through the journal

**Out** (and which node owns it instead):
- Block types, binding and generation (`RPT.MODEL`)
- Rendering to PDF or spreadsheet (`RPT.OUTPUT`)
- The docking workspace and widget toolkit (`VIZ`)
- Form-specific layouts (`RPT.FORMS`)

## Requirements

- **REQ-RPT.DESIGN-001** Every designer edit shall be executed as a registry command and journaled, so that replaying the journal reproduces the same layout.
- **REQ-RPT.DESIGN-002** The designer shall place blocks on a page grid with a position resolution of 0.1 mm or finer.
- **REQ-RPT.DESIGN-003** The designer shall support at least the ISO A4, ISO A3 and US Letter page sizes in portrait and landscape.
- **REQ-RPT.DESIGN-004** When the user edits a layout or style, the designer shall update the preview within 500 ms for a 20-page report on reference hardware [unverified].
- **REQ-RPT.DESIGN-005** A style set shall be stored with a template and applied to every report generated from it.
- **REQ-RPT.DESIGN-006** If a block would extend outside the printable area, then the designer shall flag it and the generator shall report warning `report.block_overflow`.
- **REQ-RPT.DESIGN-007** Layout and style commands shall not change any bound value, decision or formatter choice.

## Interfaces

- **Offers:** commands such as `report.set_layout@1`, `report.move_block@1` (named `*_layout` at leaf), `report.set_style@1`, `report.save_style@1`; the designer panel for the desktop apps.
- **Consumes:** `RPT.MODEL` documents and preview generation; VIZ UI toolkit and workspace.
- **Side effects:** W2 (working document).

## Decomposition hints

PAGE (page setup), PLACE (placement and grid), STYLE (style sets), PREVIEW (live preview), UI (designer panel).

## Split rationale

The split follows **what an edit changes**: page geometry (PAGE), block frames and flow (PLACE), appearance (STYLE), the scratch view of the result (PREVIEW), and the interactive panel (UI). The first three own registry commands; PREVIEW owns one R1 command; UI owns none and only issues its siblings' commands, which keeps the designer a thin UI over the registry (REQ-RPT.DESIGN-001).

- PAGE is finished as a leaf: it is small, has exact integer geometry and a concrete fixture.
- Overflow detection sits in PLACE, not PAGE, because it depends on block frames.

Alternatives considered:
- **Model vs. UI only** (two children). Rejected: the model half would be XL and mix geometry with styles.
- **A separate TEMPLATE child for save and apply.** Rejected: those are `RPT.MODEL.TMPL` commands; the designer only calls them.

## Open questions

- Should the designer also run in the WASM web build (VIZ)? Assumed yes, not required here.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.DESIGN.PAGE | Page setup (leaf) | 001 003 | none |
| RPT.DESIGN.PLACE | Block placement, sizing and overflow | 001 002 006 007 | PW.RPT.01 |
| RPT.DESIGN.STYLE | Style sets | 001 005 007 | none |
| RPT.DESIGN.PREVIEW | Live preview | 004 | none |
| RPT.DESIGN.UI | Designer panel | 001 | PCD.RPT.02, PW.RPT.01 |
