---
id: UX.PATTERN.CMDFLOW
title: Command palette flow and undo presentation
level: 3
parent: UX.PATTERN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.PATTERN-001, REQ-UX.PATTERN-003]
covers: []
depends_on: [UX.PATTERN.SCHEMA, UX.COMP.CMD]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.PATTERN.CMDFLOW: Command palette flow and undo presentation

## Purpose

How a person goes from intent to a committed command and back: search in the palette, enter arguments, preview with dry-run where the command supports it, run, see the result, and undo or redo it. Undo is presented as groups labelled with command titles, matching the journal's commit units.

## Scope

**In:** `patterns/palette-flow.md` (`pat.palette`), `patterns/undo.md` (`pat.undo`), including the history list.
**Out:** palette and argument-form components (`UX.COMP.CMD`); undo-group semantics in the bus (CORE); applying proposals (`UX.PATTERN.PROPOSE`).

## Requirements

- **REQ-UX.PATTERN.CMDFLOW-001** The palette flow shall define the steps search, choose, enter arguments, optional dry-run preview, run, and result feedback, each reachable without a pointer, and shall show the result digest and a "Why?" link for value-producing commands.
- **REQ-UX.PATTERN.CMDFLOW-002** The undo pattern shall present each undo step as one group labelled with its command title, and an applied proposal shall appear and undo as one group labelled with the proposal title and command count.
- **REQ-UX.PATTERN.CMDFLOW-003** The undo pattern shall state which commands cannot be undone (for example W3 release, S1 signing, X1 egress) and shall show that as text before they run, through `pat.confirm`.
- **REQ-UX.PATTERN.CMDFLOW-004** Both specs shall pass `fid-design pattern-check`.

## Acceptance criteria

- **AC-1** Given the two specs, when `fid-design pattern-check` runs, then it exits 0.
- **AC-2** Given `pat.undo`, then the proposal-as-one-group rule and the non-undoable list are present (design-review checklist item).

## Design

- **Deliverable:** two Markdown specs in `pattern-spec@1`.
- **Determinism:** D0.
- **Crate:** none (package content).

## Test fixtures

The shipped specs; CC-BY-4.0.

## Build prompt

> Write `patterns/palette-flow.md` (search, choose, arguments, dry-run preview, run, result with digest and "Why?" link; keyboard-complete; cancel and failure paths) and `patterns/undo.md` (undo groups labelled with command titles; an applied proposal is one group with its title and command count; a history list; the non-undoable classes W3, S1, X1 flagged before running via `pat.confirm`) in `pattern-spec@1`. Original wording. Run `fid-design pattern-check` until it exits 0, then open a design-review record.

## Open questions

- None.

## Children

_Leaf: none._
