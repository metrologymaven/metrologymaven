---
id: CORE.PRJ.TREE.GROUP
title: Groups, move, rename and visibility commands
level: 4
parent: CORE.PRJ.TREE
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.PRJ.TREE-005, REQ-CORE.PRJ.TREE-006, REQ-CORE.PRJ.TREE-007]
covers: []
depends_on: [CORE.PRJ.TREE.MODEL, CORE.PRJ.CONT.ID, CORE.BUS.PIPE.HANDLER, CORE.REG.DEF]
side_effect: [W2, W2, W2, W2]
determinism: D0
cli: [fid project create-group, fid project move-element, fid project rename-element, fid project set-visibility]
mcp: [project_create_group, project_move_element, project_rename_element, project_set_visibility]
journal: [project.create_group@1, project.move_element@1, project.rename_element@1, project.set_visibility@1]
updated: 2026-11-17
---

# CORE.PRJ.TREE.GROUP: Groups, move, rename and visibility commands

## Purpose

The four W2 commands that organise the element tree: create a group, move elements or groups under another group, rename an element or group, and set the saved visibility flag. Moves are checked so no group becomes its own ancestor.

## Scope

**In:** `project.create_group@1`, `project.move_element@1` (elements and groups, many at once), `project.rename_element@1`, `project.set_visibility@1`; ancestor check; untrusted names.
**Out:** element record and events application (`CORE.PRJ.TREE.MODEL`); transient viewer hide/show (`VIZ`); tags and metadata (`CORE.PRJ.META`).

## Requirements

- **REQ-CORE.PRJ.TREE.GROUP-001** When `project.create_group@1` runs, the tree shall create a group with a UUIDv7 ID, an untrusted name and the given parent group (the root if none).
- **REQ-CORE.PRJ.TREE.GROUP-002** When `project.move_element@1` runs with up to 10,000 element or group IDs and a target group, the tree shall move all of them in one commit, keeping their relative order.
- **REQ-CORE.PRJ.TREE.GROUP-003** If a move would make a group its own ancestor, then the command shall return `validation` with reason `hierarchy` and commit nothing.
- **REQ-CORE.PRJ.TREE.GROUP-004** When `project.rename_element@1` runs, the tree shall set the element's or group's name to the given untrusted text of 0 to 256 characters without checking uniqueness.
- **REQ-CORE.PRJ.TREE.GROUP-005** When `project.set_visibility@1` runs with element IDs and a flag, the tree shall set the saved visibility of each.
- **REQ-CORE.PRJ.TREE.GROUP-006** All four commands shall be side-effect class W2, D0 and undoable by head move.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/prj/tree/group/p30` (30 elements under root), when `fid project create-group --name Datums --json` and `fid project move-element --ids e1,e2,e3 --to <g> --json` run, then e1..e3 are children of the group in that order and two W2 entries exist.
- **AC-2** (error path) Given groups A ⊃ B, when A is moved into B, then `validation` / `hierarchy` and nothing is journaled.
- **AC-3** Given `rename-element e1 --name "Plane 1"` twice on different elements, then both have that name.
- **AC-4** Given `set-visibility --ids e4,e5 --visible false`, then `describe-element` shows both hidden; `fid journal undo` restores them.
- **AC-5** (replay) Given the journal of AC-1..AC-4, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):**
  - `project.create_group@1` W2; params `{ name, parent?: GroupRef }`.
  - `project.move_element@1` W2; params `{ ids: [EntityRef] (1..10,000), to: GroupRef }`.
  - `project.rename_element@1` W2; params `{ id: EntityRef, name }`.
  - `project.set_visibility@1` W2; params `{ ids: [EntityRef] (1..10,000), visible: bool }`.
- **Data structures:** events `GroupCreated`, `ElementsMoved`, `Renamed`, `VisibilitySet`.
- **Algorithm:** the ancestor check walks parents of the target up to the root; reject if any moved ID is met.
- **Errors:** `validation` (`hierarchy`, `bad_reference`, `name`), `not_found`.
- **Determinism:** D0.
- **Performance budget:** move 10,000 elements in < 50 ms excluding commit [unverified until measured].
- **Crate:** `fid-project` (module `tree_cmds`).

## Test fixtures

`fixtures/core/prj/tree/group/p30`, from a committed generator; CC-BY-4.0.

## Build prompt

> In crate `fid-project`, add module `tree_cmds` and register `project.create_group@1`, `project.move_element@1` (1..10,000 IDs, order kept), `project.rename_element@1` (untrusted name ≤256 chars, not unique) and `project.set_visibility@1` (1..10,000 IDs), all W2, D0, undo head_move; CLI `fid project create-group | move-element | rename-element | set-visibility`, MCP `project_*`. Reject a move that makes a group its own ancestor (`hierarchy`). Handlers read the state view and return events; the bus commits them. Tests: AC-1..AC-5 with `fixtures/core/prj/tree/group/p30`. Constraints: CONSTITUTION C3, C6, §8.

## Open questions

- Saved visibility versus transient viewer hiding needs confirmation with `VIZ.VIEW`.

## Children

_Leaf: none._
