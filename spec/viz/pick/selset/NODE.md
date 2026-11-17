---
id: VIZ.PICK.SELSET
title: Selection sets, algebra and grow
level: 3
parent: VIZ.PICK
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.PICK-002, REQ-VIZ.PICK-009]
covers: []
depends_on: [VIZ.PICK.RESOLVE, VIZ.SHELL.PALETTE, GEO.SEL.CMD, GEO.SEL.QUERY.CMD, CORE.JRN.ENTRY, CORE.PRJ.TREE.MODEL]
cli: [fid view set-selection, fid view save-selection-set, fid view delete-selection-set, fid view list-selection-sets]
mcp: [view_set_selection, view_save_selection_set, view_delete_selection_set, view_list_selection_sets]
journal: [view.set_selection@*, view.save_selection_set@*, view.delete_selection_set@*, view.list_selection_sets@*]
updated: 2026-11-17
---

# VIZ.PICK.SELSET: Selection sets, algebra and grow

## Purpose

The current selection and named selection sets: add, subtract, intersect, invert, grow by connectivity or by curvature and normal criteria, and convert picks and lassos into stored selections that domain commands take as arguments. Selections are entity IDs plus index sets or semantic queries only.

## Scope

**In:**
- Current-selection state and `view.set_selection@1`
- Set algebra over index sets and queries
- Grow and shrink by connectivity, curvature or normal-angle criteria (resolved on the CPU)
- Named selection sets as document entities
- Picked point pairs and datum features as typed command arguments for ALN and GDT

**Out** (and which node owns it instead):
- Exposing the selection to agents (`VIZ.PICK.AGENTSEL`)
- Lasso resolution (`VIZ.PICK.LASSO`)
- Selection types (`GEO.SEL`)
- Commands acting on a selection (each domain)

## Requirements

- **REQ-VIZ.PICK.SELSET-001** A stored or journaled selection shall contain only entity IDs, index sets and semantic queries.
- **REQ-VIZ.PICK.SELSET-002** Set operations on the same inputs shall return identical results, with index sets kept sorted and duplicate-free.
- **REQ-VIZ.PICK.SELSET-003** Grow and shrink operations shall be resolved on the CPU in f64 with stated criteria recorded in the query.
- **REQ-VIZ.PICK.SELSET-004** When a selection built from queries is replayed in re-targeted mode, it shall re-resolve deterministically against the new data.
- **REQ-VIZ.PICK.SELSET-005** Changing the current selection through the GUI, CLI or MCP shall use one registry command, `view.set_selection@1`.

## Interfaces

- **Offers:** `view.set_selection@1`; `SelectionSet` entity; typed selection arguments for domain commands.
- **Consumes:** `GEO.SEL`; picks and lassos from sibling modules; CORE command bus.

## Decomposition hints

STATE (current selection and command), ALGEBRA, GROW, NAMED (named sets), ARGS (typed arguments for ALN and GDT).

## Open questions

- Settled: `view.set_selection@1` is R1. The current selection is session scratch state, not document state; it reaches the Activity Log while recording (D-031), and W-class commands acting on it record its digest as an `r1_result` input. Named sets are W2 document entities (`VIZ.PICK.SELSET.NAMED`). A maintainer should confirm this reading of R1.
- Index-level algebra and grow or shrink are GEO commands (`GEO.SEL.CMD`); curvature and normal-angle grow criteria are not available in `GEO.SEL.ALGEBRA` [cross-domain].
- The pick schema keywords used by `VIZ.PICK.SELSET.ARGS` must be added to `CORE.REG.DEF.SCHEMA` [cross-domain].

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| VIZ.PICK.SELSET.STATE | Current selection, set-selection command and algebra orchestration (leaf) | 001 002 003 004 005 | none |
| VIZ.PICK.SELSET.NAMED | Named selection sets (leaf) | 001 004 | none |
| VIZ.PICK.SELSET.ARGS | Picked point pairs and datum features as typed command arguments (leaf) | 001 | none |
