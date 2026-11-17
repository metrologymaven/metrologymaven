---
id: VIZ.SHELL.DOCK.LAYOUT
title: Role layouts saved and applied as commands
level: 4
parent: VIZ.SHELL.DOCK
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.DOCK-002, REQ-VIZ.SHELL.DOCK-005]
covers: []
depends_on: [VIZ.SHELL.DOCK.HOST, CORE.STO.SET, CORE.JRN.CANON, CORE.REG.DEF]
side_effect: [W2, W2, R0]
determinism: D1
cli: [fid view save-layout, fid view apply-layout, fid view list-layouts]
mcp: [view_save_layout, view_apply_layout, view_list_layouts]
journal: [view.save_layout@1, view.apply_layout@1, view.list_layouts@1]
updated: 2026-11-17
---

# VIZ.SHELL.DOCK.LAYOUT: Role layouts saved and applied as commands

## Purpose

Familiar-workflow role layouts (Inspector, Programmer, Reviewer and user-defined ones) that capture the dock tree, panel visibility and sizes, saved and applied through registry commands and restored identically on the same screen configuration, adapted without hiding panels on a different one. It also publishes the built-in layout catalogue that app manifests reference.

## Scope

**In:** `Layout` data (dock tree, panel IDs, relative sizes, screen configuration fingerprint); `view.save_layout@1` (W2), `view.apply_layout@1` (W2), `view.list_layouts@1` (R0); storage in the per-user settings journal; built-in layouts; adaptation to another screen configuration; `layouts.json` catalogue export.
**Out:** the dock itself (`VIZ.SHELL.DOCK.HOST`); viewport splits inside the 3D view (`VIZ.VIEW.MULTI`); app-specific defaults (`ECO.APPS`, which reference layout IDs).

## Requirements

- **REQ-VIZ.SHELL.DOCK.LAYOUT-001** `view.save_layout@1` shall store the current dock tree, panel set and relative sizes with the screen configuration fingerprint (monitor count, logical sizes, scale factors) under a layout ID and untrusted name, in the per-user settings journal.
- **REQ-VIZ.SHELL.DOCK.LAYOUT-002** When `view.apply_layout@1` applies a layout on the screen configuration it was saved on, the resulting dock tree serialisation shall equal the saved serialisation byte for byte.
- **REQ-VIZ.SHELL.DOCK.LAYOUT-003** When a layout saved on a different screen configuration is applied, the shell shall map windows to available monitors in saved order, clamp every window inside a visible monitor's work area, and leave no registered panel off-screen.
- **REQ-VIZ.SHELL.DOCK.LAYOUT-004** If a layout names a panel that is not registered in the running app, then apply shall skip that panel, apply the rest and report the skipped panel IDs.
- **REQ-VIZ.SHELL.DOCK.LAYOUT-005** `view.list_layouts@1` shall return built-in and user layouts with ID, name, origin and fingerprint, sorted by ID, and change nothing.
- **REQ-VIZ.SHELL.DOCK.LAYOUT-006** If an agent principal calls `view.save_layout@1` or `view.apply_layout@1`, then the command shall fail with `policy`, because layouts are user-scope configuration (consistent with REQ-CORE.STO.SET-009).
- **REQ-VIZ.SHELL.DOCK.LAYOUT-007** The build shall export `layouts.json` (format `fid.layouts`, version 1) listing built-in layout IDs, names and the panel IDs they reference.

## Acceptance criteria

- **AC-1** Given the test app on a fake single 1920 × 1080 monitor at 100 %, when `fid view save-layout --name "Review" --json` then `fid view apply-layout --layout <id> --json` run after rearranging panels, then the dock serialisation equals the saved bytes.
- **AC-2** Given a layout saved on two monitors applied on one 1366 × 768 monitor, then every window lies inside the work area and every panel is reachable (dock snapshot check).
- **AC-3** Given a layout referencing `rpt.designer` in an app without it, then apply succeeds and reports `skipped: ["rpt.designer"]`.
- **AC-4** (agent) Given an MCP agent session, then `view_apply_layout` fails with `policy` and nothing is journaled.
- **AC-5** Given the build, then `layouts.json` validates against `schemas/layouts-1.json` and lists Inspector, Programmer and Reviewer.
- **AC-6** (replay) Given the per-user settings journal of AC-1, when replayed on the same fake monitor, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `view.save_layout@1` W2 `{ name: Untrusted<String>, layout_id? } -> { layout_id, digest }`; `view.apply_layout@1` W2 `{ layout_id } -> { digest, skipped: [PanelId], adapted: bool }`; `view.list_layouts@1` R0 `{} -> { layouts: [...] }`. Commits go to the per-user settings journal (CORE.STO.SET), not the project journal.
- **Data structures:** `Layout { id, name, origin: BuiltIn | User, fingerprint: ScreenFingerprint, dock: DockTree (canonical JSON) }`.
- **Algorithm:** canonical serialisation of the dock tree; adaptation maps windows by saved order to monitors, scales by logical size ratio, clamps to work areas, re-docks floating windows that would not fit.
- **Errors:** `UnknownLayout`, `InvalidName`, `policy`.
- **Determinism:** D1 (depends on the screen configuration; identical on the same one).
- **Performance budget:** apply under 50 ms.
- **Crate:** `fid-shell` (module `dock::layout`).

## Test fixtures

Fake monitor configurations; `fixtures/projects/min.fid`; schema `schemas/layouts-1.json`.

## Build prompt

> In crate `fid-shell`, add `dock::layout` with registry commands `view.save_layout@1` (W2), `view.apply_layout@1` (W2) and `view.list_layouts@1` (R0) (CLI `fid view save-layout|apply-layout|list-layouts`; MCP `view_save_layout|view_apply_layout|view_list_layouts`). A layout stores the canonical dock tree, panel set, relative sizes and screen fingerprint in the per-user settings journal; apply on the same configuration reproduces the saved bytes, on another configuration maps and clamps windows so no panel is off-screen, and skips unregistered panels with a report. Agents get `policy`. Ship built-in Inspector, Programmer and Reviewer layouts and export `layouts.json` (`fid.layouts` v1). Tests AC-1..AC-6. Rules: CONSTITUTION §3, §8; C6 (names untrusted); C9 (familiar workflows, original layouts).

## Open questions

- None.

## Children

_Leaf: none._
