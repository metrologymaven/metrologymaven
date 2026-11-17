---
id: AUTO.RUN.PLAY
title: Operator playback mode
level: 3
parent: AUTO.RUN
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN-005]
covers: [GOM.AUT.06, PW.AUT.04]
depends_on: [AUTO.RUN.EXEC, AUTO.PROG.OPER]
cli: [fid program run --operator]
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.RUN.PLAY: Operator playback mode

## Purpose

Runs a released program for a shop-floor operator: the operator sees only the program's declared operator steps (instructions, prompts, input requests) and the run's progress, and can do nothing outside the program. This is the engine behind kiosk mode; the kiosk screens themselves are VIZ.

## Scope

**In:**
- Restricted run session: allowed actions are answering the current operator step, pausing and cancelling
- Operator-step event stream and answer submission (validated with `AUTO.PROG.OPER`, journaled by the interpreter)
- Program and variant selection from a released list
- Operator principal and its limits

**Out:**
- Kiosk UI (`VIZ.KIOSK`); interpreter (`AUTO.PROG.INTERP`)

## Requirements

- **REQ-AUTO.RUN.PLAY-001** In operator playback, the session shall offer only: choose a released program and variant, answer the current operator step, pause, and cancel.
- **REQ-AUTO.RUN.PLAY-002** If any other command is requested in an operator playback session, then it shall be refused with `not_in_program`.
- **REQ-AUTO.RUN.PLAY-003** Operator playback shall run only released programs, never drafts or trial runs.
- **REQ-AUTO.RUN.PLAY-004** Each operator answer shall be validated against its descriptor before the run continues, and an invalid answer shall be returned to the operator with every field error.

## Interfaces

- **Offers:** operator playback API used by `VIZ.KIOSK` (`view.enter_kiosk@1` and related).
- **Consumes:** `AUTO.RUN.EXEC`; `AUTO.PROG.OPER`.

## Decomposition hints

SESSION (restricted session and allowed actions), EVENTS (operator-step stream), SELECT (program and variant choice).

## Open questions

- Principal kind of a kiosk operator (`human` with a restricted role, or a dedicated operator role in CORE) needs `CORE.BUS` agreement.

## Children

_None yet: status `open`._
