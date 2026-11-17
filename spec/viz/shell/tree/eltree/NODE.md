---
id: VIZ.SHELL.TREE.ELTREE
title: Virtualised element tree with status chips and read-only provenance
level: 4
parent: VIZ.SHELL.TREE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.TREE-001, REQ-VIZ.SHELL.TREE-002, REQ-VIZ.SHELL.TREE-005]
covers: []
depends_on: [VIZ.SHELL.SPKTREE, VIZ.SHELL.DOCK.HOST, VIZ.SHELL.A11Y.AXTREE, VIZ.OVERLAY.STATUS, VIZ.VIEW.VISIB.CMD, CORE.PRJ.TREE]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.TREE.ELTREE: Virtualised element tree with status chips and read-only provenance

## Purpose

The model/element tree panel: parts, features, characteristics, datums, alignments and results in a virtualised tree with status chips (icon, pattern and text from the locked status mapping), filter box, multi-select, rename, drag to reorder or regroup, and visibility toggles. Every edit is a registry command; names from imported content are untrusted plain text; elements the running app cannot edit are shown read-only with their provenance.

## Scope

**In:** tree panel `viz.tree`; virtualised rows; expand, collapse, select, rename (issues the owning domain's rename command), drag and drop (issues move or regroup commands), visibility eye (issues `view.set_visibility@1`), context menu from registry commands; status chips; read-only rows with provenance badge supplied through a `ReadOnlyInfo` trait; stable widget IDs `viz.tree/<entity id>/...`.
**Out:** the provenance data and edit guard (`ECO.APPS.HANDOFF.RO`, which implements `ReadOnlyInfo`); visibility semantics (`VIZ.VIEW.VISIB`); status mapping (`VIZ.OVERLAY.STATUS`); inspector and tables (siblings).

## Requirements

- **REQ-VIZ.SHELL.TREE.ELTREE-001** Every edit from the tree (rename, move, regroup, delete, visibility) shall be submitted as the owning registry command through `ShellBus`, and the tree shall hold no editable copy of document state.
- **REQ-VIZ.SHELL.TREE.ELTREE-002** When the tree holds 10^4 items, p95 UI frame time while scrolling, expanding all and filtering shall stay under 33 ms on the reference profile [unverified].
- **REQ-VIZ.SHELL.TREE.ELTREE-003** Element names, notes and tags from content shall be rendered as plain untrusted text with control and bidi-override characters made visible, never interpreted as markup or links.
- **REQ-VIZ.SHELL.TREE.ELTREE-004** Each row shall show the status chip from the locked status mapping with icon, pattern and text, so status is never conveyed by colour alone.
- **REQ-VIZ.SHELL.TREE.ELTREE-005** When `ReadOnlyInfo` reports an element as read-only, its row shall show a read-only badge with creating command ID, author principal kind and journal entry reference, and edit actions shall be disabled with the reason.
- **REQ-VIZ.SHELL.TREE.ELTREE-006** Row widgets shall use IDs `viz.tree/<entity id>` and carry `entity_id`, role tree item and the `command` attribute on every action.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/tree-10k` (seed 5, shared with SPKTREE), when the harness scrolls, expands all and types a filter on H1, then p95 frame times are reported and judged against 33 ms.
- **AC-2** Given a rename in the tree, then exactly one rename registry command is journaled and the tree updates from the next change batch.
- **AC-3** Given an element named `<b>x</b>\u202E`, then the row shows the literal characters with the override made visible.
- **AC-4** Given a fake `ReadOnlyInfo` marking one element read-only, then its row shows the badge with command ID, principal kind and entry hash, and rename is disabled with the reason.
- **AC-5** Given the inspection tree, then each row's ID is `viz.tree/<entity id>` and the visibility action has `command = view.set_visibility@1`.

## Design

- **Registry command(s):** none of its own; issues domain commands and `view.set_visibility@1`.
- **Data structures:** `TreeRowModel { entity_id, kind, name: Untrusted<String>, status, children_count, read_only: Option<ReadOnlyBadge> }`; `trait ReadOnlyInfo { fn read_only(&self, e: EntityId) -> Option<ReadOnlyBadge>; }`.
- **Algorithm:** flatten visible expanded nodes into a row index; render only rows in view; update rows from scene change batches; widget approach per SPKTREE findings.
- **Errors:** command errors shown inline in the problems panel.
- **Determinism:** D1.
- **Performance budget:** REQ-002.
- **Crate:** `fid-shell` (module `tree::eltree`).

## Test fixtures

`fixtures/viz/tree-10k` (seed 5); fake `ReadOnlyInfo`.

## Build prompt

> In crate `fid-shell`, add `tree::eltree`: a virtualised element-tree panel (`viz.tree`) over the project element tree with status chips (icon, pattern, text from the locked status mapping), filter, multi-select, rename, drag to move or regroup, visibility eye and registry-generated context menus; every edit is the owning registry command via `ShellBus` (visibility via `view.set_visibility@1`). Names are untrusted plain text with control and bidi-override characters made visible. Read-only rows come from a `ReadOnlyInfo` trait (implemented by ECO) and show creating command ID, principal kind and journal entry with edits disabled. Row IDs `viz.tree/<entity id>` with `entity_id` and `command` attributes. Tests AC-1..AC-5 with `fixtures/viz/tree-10k`. Rules: CONSTITUTION §8; C3, C6.

## Open questions

- Widget approach follows SPKTREE findings; if that spike fails, the panel moves with D-005.

## Children

_Leaf: none._
