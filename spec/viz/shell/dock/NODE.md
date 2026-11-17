---
id: VIZ.SHELL.DOCK
title: Workspace frame, docking, status bar and role layouts
level: 3
parent: VIZ.SHELL
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL-001, REQ-VIZ.SHELL-008]
covers: []
depends_on: [VIZ.SHELL.SPKTREE, VIZ.SHELL.SPKTABLE, VIZ.SHELL.SPKRPT, CORE, VIZ.RENDER]
cli: [fid view save-layout, fid view apply-layout, fid view list-layouts]
mcp: [view_save_layout, view_apply_layout, view_list_layouts]
journal: [view.save_layout@*, view.apply_layout@*]
updated: 2026-11-17
---

# VIZ.SHELL.DOCK: Workspace frame, docking, status bar and role layouts

## Purpose

The application frame every focused app shares: windows, dockable panels and the panel host API that other modules (Inbox, kiosk, app panels) plug into; the viewport host; the status bar as provenance strip; the workflow rail; and familiar-workflow role layouts saved and applied as commands.

## Scope

**In:**
- Window management and docking (split, tab, float, restore)
- Panel host API (register, show, focus, close) for VIZ and app panels
- Viewport host for the renderer texture
- Status bar: kernel manifest digest, journal head, units, capability tier, LOD counts, recording state
- Workflow rail (non-modal stepper; completion from the dependency graph)
- Role layouts (for example Inspector, Programmer, Reviewer): save, apply, list commands
- The shell's single write path: every state change through the command bus

**Out** (and which node owns it instead):
- Panel contents: tree, inspector, tables (`VIZ.SHELL.TREE`); palette (`VIZ.SHELL.PALETTE`); Inbox (`VIZ.INBOX`)
- Theme tokens and scaling (`VIZ.SHELL.THEME`)
- Accessibility tree (`VIZ.SHELL.A11Y`)
- Recording indicator behaviour (`VIZ.CAPTURE`; DOCK only hosts it)

## Requirements

- **REQ-VIZ.SHELL.DOCK-001** Every GUI action in the frame that changes document, record or configuration state shall invoke a registry command, and the shell crate shall have no other write path to the engine.
- **REQ-VIZ.SHELL.DOCK-002** `view.save_layout@1` and `view.apply_layout@1` shall save and apply role layouts, and applying a saved layout on the same screen configuration shall restore panel positions and sizes identically.
- **REQ-VIZ.SHELL.DOCK-003** The status bar shall show the kernel manifest digest, journal head, display units, capability tier, displayed versus total primitive counts and recording state at all times.
- **REQ-VIZ.SHELL.DOCK-004** Workflow-rail step completion shall come from the dependency graph, never from UI-local state.
- **REQ-VIZ.SHELL.DOCK-005** When a layout saved on a different screen configuration is applied, the shell shall adapt it to the available screens without hiding any panel off-screen.
- **REQ-VIZ.SHELL.DOCK-006** The panel host shall let other modules register panels without depending on the shell's internal widget types.

## Interfaces

- **Offers:** shell frame; panel host API; `view.save_layout@1`, `view.apply_layout@1`, `view.list_layouts@1` (R0).
- **Consumes:** CORE registry, journal head and dependency graph status; VIZ.RENDER viewport texture and render info; recording state from VIZ.CAPTURE as data.

## Decomposition hints

FRAME (windows and docking), HOST (panel host API), STATBAR, RAIL, LAYOUT (role layouts and commands).

## Open questions

- Depends on the D-005 spikes passing; if a spike fails, the affected panels may move to another toolkit while the frame stays (concept doc migration triggers).
- Applied: the panel catalogue (`panels.json`, HOST) and layout catalogue (`layouts.json`, LAYOUT) are the formats `ECO.APPS.MANIFEST.SCHEMA` reads; that node should declare `uses_artifacts` on them.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.SHELL.DOCK.HOST | Window frame, docking, panel host and viewport host (leaf) | 001 006 | none |
| VIZ.SHELL.DOCK.STATBAR | Status bar as provenance strip (leaf) | 003 | none |
| VIZ.SHELL.DOCK.RAIL | Workflow rail driven by the dependency graph (leaf) | 004 | none |
| VIZ.SHELL.DOCK.LAYOUT | Role layouts saved and applied as commands (leaf) | 002 005 | none |
