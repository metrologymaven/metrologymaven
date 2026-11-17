---
id: VIZ.PICK.SELSET.NAMED
title: Named selection sets
level: 4
parent: VIZ.PICK.SELSET
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.PICK.SELSET-001, REQ-VIZ.PICK.SELSET-004]
covers: []
depends_on: [VIZ.PICK.SELSET.STATE, CORE.PRJ.TREE.MODEL, CORE.REG.DEF]
uses_artifacts: []
side_effect: [W2, W2, R0]
determinism: D0
cli: [fid view save-selection-set, fid view delete-selection-set, fid view list-selection-sets]
mcp: [view_save_selection_set, view_delete_selection_set, view_list_selection_sets]
journal: [view.save_selection_set@1, view.delete_selection_set@1, view.list_selection_sets@1]
updated: 2026-11-17
---

# VIZ.PICK.SELSET.NAMED: Named selection sets

## Purpose

Saves a selection under a name as a document entity so it can be reused, passed to commands and restored after replay. A `SelectionSet` holds the same item kinds as the current selection (entity IDs and GEO selection element references) and nothing else; re-targeted replay re-evaluates the semantic selections it references through GEO.

## Scope

**In:** `SelectionSet` element (name, items, digest); `view.save_selection_set@1` (W2, from the current selection or explicit items; replaces a set of the same name), `view.delete_selection_set@1` (W2), `view.list_selection_sets@1` (R0); restoring a set as current via `view.set_selection@1`.
**Out:** current selection (`VIZ.PICK.SELSET.STATE`); GEO selection elements (`GEO.SEL`).

## Requirements

- **REQ-VIZ.PICK.SELSET.NAMED-001** A `SelectionSet` shall store only a name (1 to 100 characters), entity IDs and GEO selection element references, with a canonical digest.
- **REQ-VIZ.PICK.SELSET.NAMED-002** Saving and deleting shall be W2 commands journaling the set digest, and listing shall be R0 sorted by name.
- **REQ-VIZ.PICK.SELSET.NAMED-003** When a project journal is replayed in re-targeted mode, a set's semantic selection references shall resolve to the re-evaluated elements.
- **REQ-VIZ.PICK.SELSET.NAMED-004** If a set references a deleted entity or selection, then listing shall mark it `stale` with the missing items.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/pick/selset`, when `fid view save-selection-set --name holes --from-current --json` runs, then one entry is journaled and `list-selection-sets` returns `holes` with the digest.
- **AC-2** Given a referenced entity deleted, then the listing shows `holes` as `stale` naming it.
- **AC-3** Given delete, then the set is gone; (replay) exact replay of AC-1 and AC-3 is `IDENTICAL`.
- **AC-4** (error path) Given an empty name, then `BadName` and nothing journaled.

## Design

- **Registry command(s):** `view.save_selection_set@1` W2 `{ name, items? | from_current: bool }`; `view.delete_selection_set@1` W2 `{ name }`; `view.list_selection_sets@1` R0 → `[{ name, items, digest, stale, missing }]`.
- **Data structures:** `SelectionSet { id, name: String, items: Vec<SelectionItem> }` registered as an element type.
- **Errors:** `BadName`, `UnknownSet`, `UnknownItem`.
- **Determinism:** D0.
- **Performance budget:** < 10 ms per command excluding commit [unverified].
- **Crate:** `fid-pick` (module `selset::named`).

## Test fixtures

`fixtures/viz/pick/selset` (shared).

## Build prompt

> In crate `fid-pick`, add module `selset::named`: the `SelectionSet` element (name 1–100 chars, entity IDs and GEO selection references only, canonical digest) with `view.save_selection_set@1` (W2; from current selection or items; replaces same name), `view.delete_selection_set@1` (W2) and `view.list_selection_sets@1` (R0, sorted by name, stale items reported). CLI `fid view save-selection-set|delete-selection-set|list-selection-sets`, MCP `view_save_selection_set|view_delete_selection_set|view_list_selection_sets`. Tests AC-1..AC-4. Rules: no screen-space data (C2); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- None blocking.

## Children

_Leaf: none._
