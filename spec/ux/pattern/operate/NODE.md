---
id: UX.PATTERN.OPERATE
title: Operator prompts and controlled-action confirmation
level: 3
parent: UX.PATTERN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.PATTERN-001, REQ-UX.PATTERN-008, REQ-UX.PATTERN-009]
covers: []
depends_on: [UX.PATTERN.SCHEMA, UX.COMP.CONT, UX.TOKENS.TYPE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.PATTERN.OPERATE: Operator prompts and controlled-action confirmation

## Purpose

Two shop-floor-critical patterns: the operator prompt used by kiosk and Approval Station steps (one instruction at a time, large text, clear expected input), and the confirmation of controlled and irreversible actions (W3 release, S1 signing hand-off, X1 egress, deletion), whose buttons name the action and its consequence.

## Scope

**In:** `patterns/operator-prompt.md` (`pat.prompt`), `patterns/confirm.md` (`pat.confirm`).
**Out:** kiosk and station implementation (VIZ.KIOSK, CMM.EXEC.STATION); motion authorization rules (D-008, CMM.EXEC); screens using these patterns (`UX.SCREENS.FLOOR`).

## Requirements

- **REQ-UX.PATTERN.OPERATE-001** The operator prompt shall show one instruction per step in the floor theme, with body text at least 18 px [unverified] from a `type.*` token, the expected input named in text (scan, button, gauge reading, barcode), and the step number and total.
- **REQ-UX.PATTERN.OPERATE-002** The operator prompt shall offer a hold or stop control on every step of a run with motion, visible without scrolling.
- **REQ-UX.PATTERN.OPERATE-003** The confirmation pattern shall label its controls with the verb and object (for example "Release report 1234", "Keep draft") and never with generic "OK" or "Cancel", and shall state the consequence and whether it can be undone.
- **REQ-UX.PATTERN.OPERATE-004** For S1 signing, the confirmation shall hand off to the out-of-band signing step and shall state that the agent cannot sign.
- **REQ-UX.PATTERN.OPERATE-005** Both specs shall pass `fid-design pattern-check`.

## Acceptance criteria

- **AC-1** Given the two specs, when `fid-design pattern-check` runs, then it exits 0.
- **AC-2** Given `pat.confirm`, then no control label is "OK" or "Cancel" (checked by `fid-design words-check` on the example labels, with "OK" and "Cancel" added as reserved-generic words for `confirm.*` IDs).
- **AC-3** Given `pat.prompt`, then the minimum text size names a token whose floor-theme value is at least 18 px (review item).

## Design

- **Deliverable:** two Markdown specs in `pattern-spec@1`, with example message IDs `confirm.*` and `prompt.*`.
- **Determinism:** D0.
- **Crate:** none (package content).

## Test fixtures

The shipped specs and their example label export (JSON Lines) for `words-check`; CC-BY-4.0.

## Build prompt

> Write `patterns/operator-prompt.md` (one instruction per step, floor theme, body text from a type token at least 18 px, expected input named, step n of N, hold/stop visible on motion steps) and `patterns/confirm.md` (verb-and-object labels, consequence and undoability stated, never "OK"/"Cancel"; S1 hand-off to out-of-band signing stating the agent cannot sign) in `pattern-spec@1`, with example labels exported as JSON Lines. Run `fid-design pattern-check` and `fid-design words-check` until both exit 0, then open a design-review record.

## Open questions

- None.

## Children

_Leaf: none._
