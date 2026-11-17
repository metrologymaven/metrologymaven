---
id: VIZ.VIEW.NAMED.STORE
title: Save, rename, delete and list NamedViews
level: 4
parent: VIZ.VIEW.NAMED
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW.NAMED-001, REQ-VIZ.VIEW.NAMED-005, REQ-VIZ.VIEW.NAMED-006]
covers: [GOM.VIZ.01, PW.VIZ.02]
depends_on: [VIZ.VIEW.STATE, CORE.JRN.ENTRY, CORE.PRJ.TREE, CORE.REG.DEF]
side_effect: [W2, W2, W2, R0]
determinism: D0
cli: [fid view save-view, fid view rename-view, fid view delete-view, fid view list-views]
mcp: [view_save_view, view_rename_view, view_delete_view, view_list_views]
journal: [view.save_view@1, view.rename_view@1, view.delete_view@1, view.list_views@1]
updated: 2026-11-17
---

# VIZ.VIEW.NAMED.STORE: Save, rename, delete and list NamedViews

## Purpose

The NamedView entity and its lifecycle commands: save the complete current ViewState (including the multi-viewport layout when present) under a name with its digest, rename it, delete it (recoverable from journal history), and list saved views. NamedViews are project elements with stable IDs so reports and the journal can reference them.

## Scope

**In:** `NamedView` element (UUIDv7 ID, untrusted name, ViewState or layout bytes, digest, optional thumbnail reference); `view.save_view@1`, `view.rename_view@1`, `view.delete_view@1` (W2), `view.list_views@1` (R0); name validation.
**Out:** restore (`VIZ.VIEW.NAMED.RESTORE`); serialisation for packages (`VIZ.VIEW.NAMED.EXPORT`); thumbnails as images (`VIZ.CAPTURE.SHOT`).

## Requirements

- **REQ-VIZ.VIEW.NAMED.STORE-001** `view.save_view@1` shall store the complete current ViewState of the target viewport (or the full layout when `VIZ.VIEW.MULTI` is active) as canonical bytes with their SHA-256 digest under a new NamedView ID and the given name.
- **REQ-VIZ.VIEW.NAMED.STORE-002** NamedView names shall be stored as untrusted text of 1 to 128 Unicode scalar values with control characters rejected, and duplicates shall be allowed because identity is the ID.
- **REQ-VIZ.VIEW.NAMED.STORE-003** `view.rename_view@1` and `view.delete_view@1` shall be journaled, and a deleted NamedView shall be recoverable by undoing or by reading the journal entry that held it.
- **REQ-VIZ.VIEW.NAMED.STORE-004** `view.list_views@1` shall return ID, name, digest and creation journal entry of every NamedView in ID order and shall change nothing.
- **REQ-VIZ.VIEW.NAMED.STORE-005** If a referenced NamedView ID does not exist, then rename and delete shall fail with `UnknownView` and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube` with one clip plane, when `fid view save-view --viewport main --name "Top clip" --json` runs, then the result holds a view ID and a digest equal to the ViewState digest, and the journal holds one `view.save_view@1` entry.
- **AC-2** Given the saved view, when renamed and then listed, then the list shows the new name, the same digest and the original creation entry.
- **AC-3** Given the view deleted, when the delete is undone, then the view returns with identical bytes.
- **AC-4** (error path) Given a name containing U+0007, then save fails with `InvalidName` and journals nothing; given an unknown ID, rename fails with `UnknownView`.
- **AC-5** (replay) Given the journal from AC-1..AC-3, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `view.save_view@1` W2 `{ viewport, name: Untrusted<String> } -> { view_id, digest }`; `view.rename_view@1` W2 `{ view_id, name } -> { view_id }`; `view.delete_view@1` W2 `{ view_id } -> { view_id }`; `view.list_views@1` R0 `{} -> { views: [{ view_id, name, digest, created_entry }] }`.
- **Data structures:** `NamedView { id: Uuid7, name: Untrusted<String>, payload: ViewState | ViewportLayout (canonical bytes), digest, thumbnail: Option<AssetRef> }` as a project element under the revision.
- **Algorithm:** serialise via `VIZ.VIEW.STATE`; store bytes; delete via the journal's element removal so undo restores bytes.
- **Errors:** `InvalidName`, `UnknownView`, `UnknownViewport`.
- **Determinism:** D0.
- **Performance budget:** each command under 10 ms.
- **Crate:** `fid-view` (module `named`).

## Test fixtures

`fixtures/viz/golden-cube`.

## Build prompt

> In crate `fid-view`, add module `named` defining the `NamedView` project element (UUIDv7, untrusted name of 1–128 scalars without control characters, canonical ViewState or layout bytes, SHA-256 digest, optional thumbnail ref) and registry commands `view.save_view@1`, `view.rename_view@1`, `view.delete_view@1` (W2) and `view.list_views@1` (R0), with CLI `fid view save-view|rename-view|delete-view|list-views` and MCP `view_save_view|view_rename_view|view_delete_view|view_list_views`. Deletes are undoable and recoverable from the journal. Failures (`InvalidName`, `UnknownView`, `UnknownViewport`) journal nothing. Tests AC-1..AC-5 with `fixtures/viz/golden-cube`. Rules: CONSTITUTION §3, §5 (stable IDs), §8; C6 (names are untrusted).

## Open questions

- `view.rename_view@1` is added as the parent node proposed; the VIZ.VIEW `view.*` pattern covers it.

## Children

_Leaf: none._
