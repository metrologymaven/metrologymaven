---
id: RPT.DESIGN.UI
title: Designer panel
level: 3
parent: RPT.DESIGN
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN-001]
covers: [PCD.RPT.02, PW.RPT.01]
depends_on: [RPT.DESIGN.PAGE, RPT.DESIGN.PLACE, RPT.DESIGN.STYLE, RPT.DESIGN.PREVIEW, RPT.MODEL.DOC, RPT.MODEL.BLOCKS, RPT.MODEL.TMPL, CORE.JRN.UNDO, VIZ.SHELL.DOCK, VIZ.SHELL.PALETTE, VIZ.SHELL.A11Y, VIZ.INBOX]
uses_artifacts: [VIZ.SHELL.SPKRPT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.DESIGN.UI: Designer panel

## Purpose

The interactive report designer in the desktop apps (and, if feasible, the WASM build): a page canvas with selection handles, a block palette, a property panel, a template browser, and undo and redo. It is a thin UI: every edit it makes is a registry command owned by a sibling or by `RPT.MODEL`, so it adds no commands of its own.

## Scope

**In:**
- Page canvas with drag, resize and snap handles that issue `RPT.DESIGN.PLACE` commands
- Block palette that issues `RPT.MODEL.DOC` and `RPT.MODEL.BLOCKS` commands
- Property panel for block payloads, page setup and styles
- Template browser with "save as template" and "apply template" (`RPT.MODEL.TMPL` commands)
- Undo and redo through the journal (`CORE.JRN`)
- Keyboard access and screen-reader labels per the VIZ shell rules

**Out** (and which node owns it instead):
- All model changes (the command owners above)
- Workspace, docking and widget toolkit (`VIZ.SHELL`)
- Rendering of preview pages (`RPT.DESIGN.PREVIEW`)

## Requirements

- **REQ-RPT.DESIGN.UI-001** Every edit made in the designer panel shall be issued as an existing registry command, and the panel shall hold no report state that is not in the report model.
- **REQ-RPT.DESIGN.UI-002** When the user presses undo, the designer shall revert the last designer command through the journal's undo mechanism, and redo shall re-apply it.
- **REQ-RPT.DESIGN.UI-003** The designer shall be operable by keyboard alone for select, move by one grid step, resize by one grid step, and delete.
- **REQ-RPT.DESIGN.UI-004** When an agent-authored layout proposal exists on an agent branch, the designer shall show it as a proposal for a human to adopt (CONSTITUTION C5).
- **REQ-RPT.DESIGN.UI-005** The designer panel shall pass the D-005 report-designer prototype spike before it is built in full.

## Interfaces

- **Offers:** the designer panel for the Report app (and other apps that host reports).
- **Consumes:** commands of `RPT.DESIGN.PAGE`, `RPT.DESIGN.PLACE`, `RPT.DESIGN.STYLE`, `RPT.MODEL.*`; preview surface (`RPT.DESIGN.PREVIEW`); shell and widgets (`VIZ.SHELL`); undo (`CORE.JRN`); proposal display (`VIZ.INBOX`).

## Decomposition hints

SPIKE (D-005 prototype), CANVAS (canvas and handles), PANEL (palette, property panel, template browser), KEYS (keyboard and accessibility).

## Open questions

- Whether the designer also ships in the WASM web build (VIZ.WEB); assumed yes, not required here.
- Layering (CONSTITUTION §2.1): the children are built in an L3 surface crate `fid-report-designer`, so depending on `VIZ.SHELL.*` and `VIZ.INBOX` is layer-legal; the L2 `fid-report` crate never depends on it. Confirm with maintainers.
- The D-005 spike is owned by `VIZ.SHELL.SPKRPT`; `RPT.DESIGN.UI.CANVAS` reads its result record (`uses_artifacts`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.DESIGN.UI.CANVAS | Designer page canvas, handles and proposal overlay (leaf) | 001 004 005 | PW.RPT.01 |
| RPT.DESIGN.UI.PANEL | Block palette, property panel, template browser and undo (leaf) | 001 002 | PCD.RPT.02, PW.RPT.01 |
| RPT.DESIGN.UI.KEYS | Keyboard operation and accessibility of the designer (leaf) | 003 | none |
