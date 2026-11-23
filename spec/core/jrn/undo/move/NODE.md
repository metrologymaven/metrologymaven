---
id: CORE.JRN.UNDO.MOVE
title: Undo, redo, undo tree and barriers
level: 4
parent: CORE.JRN.UNDO
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.JRN.UNDO-001, REQ-CORE.JRN.UNDO-002, REQ-CORE.JRN.UNDO-003, REQ-CORE.JRN.UNDO-004, REQ-CORE.JRN.UNDO-005, REQ-CORE.JRN.UNDO-007]
covers: [GOM.PRJ.06, PW.PRJ.04]
depends_on: [CORE.JRN.CHAIN.APPEND, CORE.JRN.BR.NAME, CORE.REG.DEF]
side_effect: [W2, W2, R0]
determinism: D0
cli: [fid journal undo, fid journal redo, fid journal show-undo-tree]
mcp: [journal_undo, journal_redo, journal_show_undo_tree]
journal: [journal.undo@1, journal.redo@1, journal.show_undo_tree@1]
built_in: [fid-journal, fid-journal-cmd]
updated: 2026-11-23
---

# CORE.JRN.UNDO.MOVE: Undo, redo, undo tree and barriers

## Purpose

Undo and redo without deleting history, the CORE answer to the incumbents' undo features. Undo appends a `head_move` entry from the branch head to its logical parent; redo appends one to a child state. A command committed after an undo becomes a sibling of the undone state, so the undo tree keeps every state reachable across sessions. Released (W3) records and commands declared `not_undoable` are barriers that undo refuses to cross.

## Scope

**In:** `journal.undo@1`, `journal.redo@1`, `journal.show_undo_tree@1`; `head_move` entries; the undo-tree model (children per state, most recently visited child); barriers for W3 entries and `not_undoable` commands.
**Out:** rebuilding document state at the new head (`CORE.RPL.RECON`); supersession (`CORE.JRN.UNDO.SUPER`); branch records (`CORE.JRN.BR.NAME`); undo menu and history panel (`VIZ`).

## Requirements

- **REQ-CORE.JRN.UNDO.MOVE-001** When `journal.undo@1` runs on a branch, the journal shall append a `head_move` entry from the current head to its logical parent and remove no entry.
- **REQ-CORE.JRN.UNDO.MOVE-002** When `journal.redo@1` runs, the journal shall append a `head_move` entry to a child state of the current head: the named child if given, otherwise the most recently visited child.
- **REQ-CORE.JRN.UNDO.MOVE-003** If redo is invoked on a head with no child state, then the journal shall return `precondition` with reason `nothing_to_redo`.
- **REQ-CORE.JRN.UNDO.MOVE-004** When a command commits after an undo, the journal shall set its logical parent to the current head, making it a sibling of the undone state.
- **REQ-CORE.JRN.UNDO.MOVE-005** If an undo would move the head across an entry of a W3 command, then the journal shall return `precondition` with reason `released_record` and commit nothing.
- **REQ-CORE.JRN.UNDO.MOVE-006** If an undo would move the head across an entry of a command declared `not_undoable`, then the journal shall return `precondition` with reason `not_undoable` and commit nothing.
- **REQ-CORE.JRN.UNDO.MOVE-007** `journal.undo@1` and `journal.redo@1` shall be side-effect class W2, and `journal.show_undo_tree@1` class R0, returning the tree from a given state to a depth of at most 1,000 nodes per call.
- **REQ-CORE.JRN.UNDO.MOVE-008** When undo or redo runs with `steps` n (1 to 100), the journal shall move n states in one `head_move` entry, refusing the whole move if any barrier lies on the path.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/jrn/undo/move/linear` (states s0..s5 on `main`), when `fid journal undo --json` runs twice, then the head is s3, two `head_move` entries are appended, and all 5 command entries still exist.
- **AC-2** Given AC-1, when a new command commits, then its logical parent is s3, s4 and the new state are siblings, and `fid journal show-undo-tree --json` shows both; `fid journal redo --child s4` moves to s4.
- **AC-3** Given a head with children visited in order a then b, when redo runs without a child, then the head moves to b.
- **AC-4** (error path) Given fixture `fixtures/core/jrn/undo/move/released` (s2 is a W3 release), when undo is asked to cross s2, then `precondition` / `released_record` and nothing is journaled; the same for a `not_undoable` test command with reason `not_undoable`.
- **AC-5** (replay) Given the journal of AC-1 and AC-2 reopened in a new session, then the undo tree and head are identical, and exact replay is `IDENTICAL`.

## Design

- **Registry command(s):**
  - `journal.undo@1` W2, D0; params `{ branch?: BranchRef, steps?: 1..100 }`; result `{ from, to }`.
  - `journal.redo@1` W2, D0; params `{ branch?, child?: EntryHash, steps?: 1..100 }`; result `{ from, to }`.
  - `journal.show_undo_tree@1` R0; params `{ branch?, from?: EntryHash, depth?: ≤1,000 }`; result `{ nodes: [{ state, parent, children, visited_at, command }] }`.
- **Data structures:** undo tree as a projection of `logical_parent` links and `head_move` entries; `last_visited` per state derived from `head_move` order.
- **Algorithm:** walk `logical_parent` n steps, checking each crossed entry's class and undo flag; emit `HeadMoved { branch, from, to }`; the bus commits it as a `head_move` entry.
- **Errors:** `precondition` (`released_record`, `not_undoable`, `nothing_to_redo`), `not_found`.
- **Determinism:** D0.
- **Performance budget:** undo or redo < 2 ms excluding state reconstruction [unverified until measured].
- **Crate:** `fid-journal` (module `undo`).

## Test fixtures

`fixtures/core/jrn/undo/move/` (`linear`, `released`), generated by a committed script; CC-BY-4.0.

## Build prompt

> In crate `fid-journal`, add module `undo` and register `journal.undo@1` (W2), `journal.redo@1` (W2) and `journal.show_undo_tree@1` (R0); CLI `fid journal undo | redo | show-undo-tree`, MCP `journal_undo`, `journal_redo`, `journal_show_undo_tree`. Undo emits `HeadMoved` from the head to its logical parent (or n steps, 1..100); redo to the named child or the most recently visited child (`nothing_to_redo` if none). Refuse any move that crosses a W3 entry (`released_record`) or a `not_undoable` command (`not_undoable`), all-or-nothing. A command committed after an undo gets the current head as logical parent, so states become siblings. Never delete entries. The undo tree is derived from `logical_parent` links and `head_move` order and survives sessions. Tests: AC-1..AC-5 with `fixtures/core/jrn/undo/move/`. Constraints: CONSTITUTION §6 (append-only), §3 (classes), §8.

## Open questions

- An agent's undo on its own branch stays W2 and is routed to the agent branch by `CORE.BUS.ROUTE`.

## Children

_Leaf: none._
