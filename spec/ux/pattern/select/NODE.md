---
id: UX.PATTERN.SELECT
title: Selection, pick and lasso pattern
level: 3
parent: UX.PATTERN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.PATTERN-001, REQ-UX.PATTERN-002]
covers: []
depends_on: [UX.PATTERN.SCHEMA, UX.COMP.DATA]
uses_artifacts: [FLOW.PERSONA.MAPS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.PATTERN.SELECT: Selection, pick and lasso pattern

## Purpose

One selection shared by the viewport, the tree and the table: how pick, lasso, range and toggle selection work with pointer and keyboard, how a selection change shows in every linked view, and how the selection becomes the context for the command palette and for an agent request.

## Scope

**In:** `patterns/select.md` (`pat.select`), `patterns/lasso.md` (`pat.lasso`).
**Out:** CPU pick resolution and lasso implementation (VIZ.PICK); tree and table component behaviour (`UX.COMP.DATA`).

## Requirements

- **REQ-UX.PATTERN.SELECT-001** The pattern shall define one selection set per document view, shown in viewport, tree and table, and a change from any of them shall update the others within one frame of the result arriving [unverified timing].
- **REQ-UX.PATTERN.SELECT-002** The pattern shall define keyboard selection equal in power to pointer selection: move, extend, toggle, select all, select none, select same type, and lasso replacement by a keyboard region or filter selection.
- **REQ-UX.PATTERN.SELECT-003** The pattern shall show the selection count and the selection's entity types as text in the status bar.
- **REQ-UX.PATTERN.SELECT-004** The pattern shall state that a lasso or pick is resolved to an index set or a semantic query before it becomes command input (CONSTITUTION C2), and the UI shall show "resolving" until it is.
- **REQ-UX.PATTERN.SELECT-005** Both specs shall pass `fid-design pattern-check`.

## Acceptance criteria

- **AC-1** Given the two specs, when `fid-design pattern-check` runs, then it exits 0.
- **AC-2** Given `pat.select`, then each pointer gesture in the spec has a keyboard equivalent in the keyboard path (design-review checklist item).

## Design

- **Deliverable:** two Markdown specs in `pattern-spec@1`, citing the persona journey steps they serve.
- **Determinism:** D0.
- **Crate:** none (package content).

## Test fixtures

The shipped specs; CC-BY-4.0.

## Build prompt

> Write `patterns/select.md` and `patterns/lasso.md` in `pattern-spec@1`: one selection shared by viewport, tree and table; keyboard move, extend, toggle, all, none, same type, region or filter selection instead of lasso; selection count and types in the status bar; picks and lassos resolved on the CPU to index sets or semantic queries before use, with a "resolving" state; selection as context for the palette and agent requests. Cite persona journey map steps. Original wording. Run `fid-design pattern-check` until it exits 0, then open a design-review record.

## Open questions

- None.

## Children

_Leaf: none._
