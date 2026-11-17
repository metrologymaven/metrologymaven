---
id: RPT.DESIGN.UI.KEYS
title: Keyboard operation and accessibility of the designer
level: 4
parent: RPT.DESIGN.UI
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN.UI-003]
covers: []
depends_on: [RPT.DESIGN.UI.CANVAS, RPT.DESIGN.UI.PANEL, VIZ.SHELL.PALETTE, VIZ.SHELL.A11Y]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.DESIGN.UI.KEYS: Keyboard operation and accessibility of the designer

## Purpose

Makes the designer usable without a mouse and with a screen reader: keyboard selection, move and resize by one grid step, delete, and undo and redo, all mapped to the same registry commands as mouse gestures; plus accessible names and roles for blocks, handles and panels through the shell's accessibility tree. The key map is declared in the shell's keymap so users can rebind it.

## Scope

**In:** key bindings (Tab and Shift+Tab to cycle blocks, arrows to move one grid step, Shift+arrows to resize one grid step, Delete to remove, Ctrl+Z and Ctrl+Y or platform equivalents); mapping to `report.move_block_layout@1`, `report.remove_block@1` and journal undo and redo; accessible names (block kind, number, frame in mm) and roles; focus order.

**Out:** mouse gestures (`RPT.DESIGN.UI.CANVAS`); panel forms (`RPT.DESIGN.UI.PANEL`); the keymap and accessibility frameworks (`VIZ.SHELL.PALETTE`, `VIZ.SHELL.A11Y`).

## Requirements

- **REQ-RPT.DESIGN.UI.KEYS-001** The designer shall support select, move by one grid step, resize by one grid step, and delete using the keyboard alone, each issuing exactly one registry command per key press.
- **REQ-RPT.DESIGN.UI.KEYS-002** The key bindings shall be declared in the shell keymap and be rebindable.
- **REQ-RPT.DESIGN.UI.KEYS-003** Every block on the canvas shall expose an accessible name containing its kind and its frame position and size in millimetres, and the focused block shall be announced on change.
- **REQ-RPT.DESIGN.UI.KEYS-004** The mapping from key event and selection to command shall be a pure function, so that it can be tested without a window.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/design/four_blocks` and grid pitch 1 mm, when Tab selects B2 and Right is pressed three times, then three `report.move_block_layout@1` commands with delta +100 units are produced by the pure mapping function and B2's x grows by 300.
- **AC-2** Shift+Down then grows B2's height by 100; Delete produces `report.remove_block@1` for B2.
- **AC-3** The accessibility tree contains a node for each block whose name gives the block kind and its x, y, width and height in mm with two decimals (B2 after AC-1: x 34.00 mm, width 10.00 mm).
- **AC-4** Rebinding move-right to L in the keymap makes L produce the same command.

## Design

- **Registry commands:** none.
- **API:** `fn map_key(event: KeyEvent, selection: &Selection, grid: Length, keymap: &Keymap) -> Option<CommandRequest>`.
- **Determinism:** D0 for the mapping function.
- **Performance budget:** trivial.
- **Crate:** `fid-report-designer` (L3 surface crate, module `keys`).

## Test fixtures

- `fixtures/rpt/design/four_blocks` (shared).

## Build prompt

> In the L3 surface crate `fid-report-designer`, module `keys`, implement keyboard operation of the report designer as a pure `map_key(event, selection, grid, keymap) -> Option<CommandRequest>` (Tab/Shift+Tab select, arrows move one grid step, Shift+arrows resize one grid step, Delete removes, undo and redo), with bindings declared in the shell keymap (rebindable), plus accessible names and roles for blocks (kind and frame in mm) through the shell accessibility tree. One registry command per key press. Tests AC-1 to AC-4. CONSTITUTION §8 applies. Spec: `spec/rpt/design/ui/keys/NODE.md`.

## Open questions

- None yet.

## Children

_Leaf: none._
