---
id: UX.PATTERN.ERROR
title: Error, unresolved and not-decidable states; progress and cancel
level: 3
parent: UX.PATTERN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.PATTERN-001, REQ-UX.PATTERN-007, REQ-UX.PATTERN-011]
covers: []
depends_on: [UX.PATTERN.SCHEMA, UX.COMP.CONT, UX.COMP.WHY]
uses_artifacts: [FLOW.DEADEND.CATALOG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.PATTERN.ERROR: Error, unresolved and not-decidable states; progress and cancel

## Purpose

What the user sees when a workflow cannot go on: an error, an unresolved input, or a NOT_DECIDABLE result. The cause is stated in plain words with at least one suggested fix naming a command, taken from FLOW's suggested-fix catalogue. The same node specifies progress for long jobs and the cancel control, including whether cancelling keeps or discards partial results.

## Scope

**In:** `patterns/error.md` (`pat.error`), `patterns/progress.md` (`pat.progress`).
**Out:** the fix catalogue and reason codes (FLOW.DEADEND.CATALOG); job cancellation semantics (CORE); toast and status-bar components (`UX.COMP.CONT`).

## Requirements

- **REQ-UX.PATTERN.ERROR-001** The error pattern shall show the reason code, a plain-words cause, and every suggested fix from the catalogue entry as a command link the current principal may run, with fixes it may not run shown disabled with the reason.
- **REQ-UX.PATTERN.ERROR-002** A NOT_DECIDABLE or INDETERMINATE outcome shall be shown with its glyph and text and a "Why?" link, never as a blank or as a pass or fail.
- **REQ-UX.PATTERN.ERROR-003** The progress pattern shall show determinate progress when the job reports it, otherwise an indeterminate indicator with elapsed time, and a cancel control for every cancellable job with text saying whether partial results are kept or discarded.
- **REQ-UX.PATTERN.ERROR-004** Both specs shall pass `fid-design pattern-check`.

## Acceptance criteria

- **AC-1** Given the two specs, when `fid-design pattern-check` runs, then it exits 0.
- **AC-2** Given `pat.error` and the catalogue fixture copy of `fix-catalogue@1`, then the spec's example for `decide.rule_missing` shows both catalogue fixes as command links (review item).
- **AC-3** Given `pat.progress`, then the keep-or-discard text rule is present (review item).

## Design

- **Deliverable:** two Markdown specs in `pattern-spec@1`.
- **Determinism:** D0.
- **Crate:** none (package content).

## Test fixtures

The shipped specs; a copy of the FLOW `fix-catalogue@1` example entries for the review; CC-BY-4.0.

## Build prompt

> Write `patterns/error.md` (reason code, plain cause, every catalogue fix as a command link; fixes the principal cannot run shown disabled with reason; NOT_DECIDABLE and INDETERMINATE with glyph, text and "Why?" link) and `patterns/progress.md` (determinate or indeterminate with elapsed time; cancel for every cancellable job with keep-or-discard text) in `pattern-spec@1`, using the FLOW suggested-fix catalogue entries as examples. Run `fid-design pattern-check` until it exits 0, then open a design-review record.

## Open questions

- None.

## Children

_Leaf: none._
