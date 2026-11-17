---
id: VIZ.SHELL.PALETTE.KEYMAP
title: Generated menus, keymap, overrides and keyboard-completeness check
level: 4
parent: VIZ.SHELL.PALETTE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.PALETTE-003, REQ-VIZ.SHELL.PALETTE-004]
covers: []
depends_on: [VIZ.SHELL.PALETTE.SEARCH, CORE.REG.MAN, CORE.STO.SET]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.PALETTE.KEYMAP: Generated menus, keymap, overrides and keyboard-completeness check

## Purpose

Menus and the default keymap generated from the registry manifest (menu path and default binding are fields of command declarations, plus UI-only actions such as panel focus and camera navigation registered by shell modules), user keymap overrides stored as a setting, conflict detection, and the CI check that every GUI action has a palette entry or a key binding.

## Scope

**In:** menu tree generation; default keymap generation; GUI-action registry for non-command UI actions (focus panel, navigate, open palette); user overrides via the setting `viz.shell.keymap` (changed only by `settings.set_value@1`); conflict and reserved-key detection; `keymap.json` export for docs and `FLOW.REPORT.KBD`; CI completeness check.
**Out:** the palette (`VIZ.SHELL.PALETTE.SEARCH`); settings storage and commands (`CORE.STO.SET`).

## Requirements

- **REQ-VIZ.SHELL.PALETTE.KEYMAP-001** Menus and the default keymap shall be generated from the manifest and the GUI-action registry, and CI shall fail if any menu entry is defined outside them.
- **REQ-VIZ.SHELL.PALETTE.KEYMAP-002** CI shall fail if a registered GUI action or GUI-visible command has neither a palette entry nor a key binding, naming the action.
- **REQ-VIZ.SHELL.PALETTE.KEYMAP-003** User overrides shall be stored in the setting `viz.shell.keymap` and changed only through `settings.set_value@1`.
- **REQ-VIZ.SHELL.PALETTE.KEYMAP-004** If an override binds a key already bound in the same scope or a reserved OS shortcut, then the shell shall reject the override with `KeyConflict` naming both actions and keep the previous keymap.
- **REQ-VIZ.SHELL.PALETTE.KEYMAP-005** The build shall export `keymap.json` (format `fid.keymap`, version 1) with every action, its default binding and menu path.

## Acceptance criteria

- **AC-1** Given `fixtures/viz/palette/manifest-200.json` and the shell's GUI actions, when the completeness check runs, then it exits 0.
- **AC-2** (error path) Given a test GUI action registered with `palette: false` and no binding, then the check exits 1 naming it.
- **AC-3** Given an override of `Ctrl+K` to "fit view" while `Ctrl+K` opens the palette, then `KeyConflict` names both and the keymap is unchanged.
- **AC-4** Given a valid override set via `fid settings set-value viz.shell.keymap ...`, then after restart the binding applies and the per-user settings journal holds the change.
- **AC-5** Given the build, then `keymap.json` validates against `schemas/keymap-1.json`.

## Design

- **Registry command(s):** none of its own; uses `settings.set_value@1`.
- **Data structures:** `GuiAction { id, title: MessageId, palette: bool, default_key: Option<KeyChord>, menu: Option<MenuPath> }`; `Keymap` (BTreeMap of chord to action per scope).
- **Algorithm:** merge defaults with overrides; detect conflicts per scope (global, panel, viewport); reserved list per OS.
- **Errors:** `KeyConflict`, `ReservedKey`, `UnknownAction`.
- **Determinism:** D0.
- **Performance budget:** keymap build under 5 ms.
- **Crate:** `fid-shell` (module `palette::keymap`) and CI check in `fid-shell-ci`.

## Test fixtures

`fixtures/viz/palette/manifest-200.json`; override samples.

## Build prompt

> In crate `fid-shell`, add `palette::keymap`: generate menus and the default keymap from the registry manifest plus a `GuiAction` registry for UI-only actions; user overrides live in setting `viz.shell.keymap` (declared via the settings macro, changed only by `settings.set_value@1`); reject conflicts and reserved OS shortcuts with `KeyConflict`/`ReservedKey`; export `keymap.json` (`fid.keymap` v1). Add a CI check that fails when any GUI action or GUI-visible command has neither a palette entry nor a binding, and when menu entries are defined outside the generator. Tests AC-1..AC-5. Rules: CONSTITUTION §3, §8; C3.

## Open questions

- None.

## Children

_Leaf: none._
