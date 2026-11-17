---
id: CORE.JRN.UNDO
title: Undo, redo, undo tree and supersession
level: 3
parent: CORE.JRN
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.JRN-008, REQ-CORE.JRN-009, REQ-CORE.JRN-013]
covers: [GOM.PRJ.06, PW.PRJ.04]
depends_on: [CORE.JRN.CHAIN, CORE.JRN.BR]
cli: [fid journal undo, fid journal redo, fid journal show-undo-tree, fid journal supersede-record]
mcp: [journal_undo, journal_redo, journal_show_undo_tree]
journal: [journal.undo@1, journal.redo@1, journal.supersede_record@1]
updated: 2026-11-16
---

# CORE.JRN.UNDO: Undo, redo, undo tree and supersession

## Purpose

Undo and redo without ever deleting history. Each undo or redo appends a `HeadMove` entry; a new command after an undo forks the logical DAG, so the undone states stay reachable through an undo tree. Released (W3) records are never undone: they change only by a supersession entry that cites what it supersedes and why. This is the CORE answer to the incumbents' undo features.

## Scope

**In:**
- `HeadMove(branch, from, to)` entries for undo and redo
- Undo-tree model and navigation (redo after a fork chooses a child state)
- Commands declared `not_undoable` and released records as undo barriers
- Supersession of released records with reason and superseded digest
- Persisted undo across sessions (the tree is in the journal)

**Out** (and which node owns it instead):
- Rebuilding document state at the new head (owned by `CORE.RPL.RECON`)
- Branch storage (owned by `CORE.JRN.BR`)
- Human assertion production for W3 supersession (owned by `TRUST`)
- Undo menu and history panel UI (owned by `VIZ`)

## Requirements

- **REQ-CORE.JRN.UNDO-001** When undo is invoked on a branch, the journal shall append a `HeadMove` entry from the current head to its logical parent and shall remove no entry.
- **REQ-CORE.JRN.UNDO-002** When redo is invoked, the journal shall append a `HeadMove` entry to a child state of the current head, choosing the most recently visited child unless a child is named.
- **REQ-CORE.JRN.UNDO-003** When a command commits after an undo, the journal shall make the new state a sibling of the undone state, so both remain reachable in the undo tree.
- **REQ-CORE.JRN.UNDO-004** If an undo would move the head across an entry recording a released (W3) record, then the journal shall refuse it with a typed `released_record` error.
- **REQ-CORE.JRN.UNDO-005** If an undo would move the head across an entry of a command declared `not_undoable`, then the journal shall refuse it with a typed `not_undoable` error.
- **REQ-CORE.JRN.UNDO-006** `journal.supersede_record@1` shall be side-effect class W3 and shall record the superseded record digest, the new record digest and a reason commitment, leaving the original visible.
- **REQ-CORE.JRN.UNDO-007** `journal.undo@1` and `journal.redo@1` shall be side-effect class W2, and `journal.show_undo_tree@1` class R0.

## Interfaces

- **Offers:** `Undo::undo(branch)`, `Undo::redo(branch, child?)`, `UndoTree` view, `Supersede::record(old, new, reason)`.
- **Commands:** `journal.undo@1` (W2), `journal.redo@1` (W2), `journal.show_undo_tree@1` (R0), `journal.supersede_record@1` (W3).
- **Consumes:** `CORE.JRN.CHAIN`, `CORE.JRN.BR`.

## Decomposition hints

MOVE (HeadMove and undo/redo commands), TREE (undo-tree model and query), BARRIER (released and not-undoable barriers), SUPER (supersession).

## Open questions

- Whether an agent's undo on its own branch is W1 (draft) rather than W2; this child keeps one class (W2) and lets `CORE.BUS.ROUTE` place agent undos on the agent branch.

## Children

_None yet: status `open`._
