---
id: VIZ.INBOX.GATES.LEAN
title: Count-confirm and reason gate steps
level: 4
parent: VIZ.INBOX.GATES
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX.GATES-003, REQ-VIZ.INBOX.GATES-004]
covers: []
depends_on: [VIZ.INBOX.GATES.PLAN, VIZ.SHELL.DOCK]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.INBOX.GATES.LEAN: Count-confirm and reason gate steps

## Purpose

The two gate steps that need no signing component: confirming the total command count of a batch by typing it, and entering a reason for the apply. They are modal dialogs in the GUI only; the reason is passed to promotion as the human's own text and is journaled by CORE with the promotion. These steps are all the Lean profile uses, so the Inbox is complete in launch-1 without TRUST.ESIG.

## Scope

**In:** `ConfirmCount` dialog (typed number must equal the batch's command count; the dialog lists every side-effect class in the batch); `Reason` dialog (1 to 2,000 characters); keyboard operation; step outcomes `Passed`, `Cancelled`.
**Out:** deciding which steps run (`VIZ.INBOX.GATES.PLAN`); signature and second-person steps (`VIZ.INBOX.GATES.REG`); batch content (`VIZ.INBOX.ACTIONS.BATCH`).

## Requirements

- **REQ-VIZ.INBOX.GATES.LEAN-001** The `ConfirmCount` step shall pass only when the typed integer equals the batch's total command count, and shall show every side-effect class in the batch while open.
- **REQ-VIZ.INBOX.GATES.LEAN-002** The `Reason` step shall pass only with 1 to 2,000 characters of text and shall hand the text to the promotion callback unchanged.
- **REQ-VIZ.INBOX.GATES.LEAN-003** Both steps shall be shown only in the interactive GUI session of a human principal and shall have no registry command, CLI or MCP surface.
- **REQ-VIZ.INBOX.GATES.LEAN-004** Closing a dialog by any means other than its confirm control shall report `Cancelled`.

## Acceptance criteria

- **AC-1** Given a batch of 12 commands with classes W1 and W2, when the user types 11, then confirm stays disabled; typing 12 passes; the dialog showed both classes (UI test).
- **AC-2** Given an empty reason, then confirm is disabled; given 2,001 characters, then input stops at 2,000.
- **AC-3** Given Escape or window close, then the outcome is `Cancelled` and no promotion is called (with GATES.PLAN's runner).
- **AC-4** Given the registry manifest, then no command corresponds to these steps.

## Design

- **Registry command(s):** none.
- **Data structures:** `StepOutcome { Passed { reason: Option<String> }, Cancelled }`.
- **Algorithm:** modal dialogs through the shell's dialog host; integer parse of the typed count with no locale grouping.
- **Errors:** none.
- **Determinism:** D0.
- **Performance budget:** n/a.
- **Crate:** `fid-inbox` (module `gates::lean`).

## Test fixtures

UI test harness with a synthetic batch; no external data.

## Build prompt

> In crate `fid-inbox`, add module `gates::lean` with two GUI-only gate steps for `GateRunner`: `ConfirmCount` (passes only when the typed integer equals the batch's command count; lists every side-effect class) and `Reason` (1–2,000 characters, passed unchanged to the promotion callback). Any close other than confirm is `Cancelled`. Keyboard-operable; no registry command, CLI or MCP. Tests AC-1..AC-4. Rules: human principals only; no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- `CORE.JRN.BR.PROMO`'s promote commands take no reason parameter; the reason is to be recorded either as a parameter added there or as a separate note entry. Requested from CORE [cross-domain].

## Children

_Leaf: none._
