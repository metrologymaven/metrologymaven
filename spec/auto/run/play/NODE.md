---
id: AUTO.RUN.PLAY
title: Operator playback mode
level: 3
parent: AUTO.RUN
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN-005]
covers: [GOM.AUT.06, PW.AUT.04]
depends_on: [AUTO.RUN.EXEC.CMD, AUTO.PROG.OPER]
cli: [fid program start-operator-run, fid program list-operator-programs, fid program answer-step, fid program get-run-state, fid program pause-run, fid program resume-run]
mcp: [program_start_operator_run, program_list_operator_programs, program_answer_step, program_get_run_state, program_pause_run, program_resume_run]
journal: [program.start_operator_run@1, program.answer_step@1, program.pause_run@1, program.resume_run@1]
updated: 2026-11-17
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

- Settled for this split: a kiosk operator is a `human` principal (a GUI widget in an interactive session, REQ-CORE.BUS.PRIN-005). The restriction comes from the **operator session** bound to the channel, not from a new principal kind, so `CORE.BUS.PRIN` needs no change. Roles such as admin, supervisor and operator remain `CORE.BUS` policy.
- `fid program run --operator` is replaced by the command `program.start_operator_run@1` (`fid program start-operator-run`), because the operator session has its own preconditions (released only, human only) and its own journal entry. A CLI caller is `script` and must upgrade to `human` by confirmation in the running app (D-012) to start or answer an operator run.
- Enforcing the session allow-list needs a channel-scoped command filter in the bus (`CORE.BUS.POL`, open): recorded in `AUTO.RUN.PLAY.SESSION`.

## Split rationale

The split follows the operator's interactions: choosing what to run (SELECT, read-only), the restricted session that starts the run and refuses everything else (SESSION), answering the current step (ANSWER) and watching, pausing and resuming the run (STATE). Each has its own command and can be tested alone; SESSION's allow-list names the commands of the other three by ID only.

Alternatives considered:
- **One leaf with all six commands.** Rejected: too large for one PR, and the answer path (validation, untrusted text, human-only) is independent of the session filter.
- **Pause and resume in `AUTO.RUN.EXEC.CMD`.** Rejected: pause is an operator action with its own commands; it plugs into CMD as a `StepGate`, so CMD stays free of it.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AUTO.RUN.PLAY.SELECT | Released program and variant list for operators (leaf) | 001 003 | none |
| AUTO.RUN.PLAY.SESSION | Operator session start and command allow-list (leaf) | 001 002 003 | GOM.AUT.06, PW.AUT.04 |
| AUTO.RUN.PLAY.ANSWER | Operator step answers (leaf) | 001 004 | none |
| AUTO.RUN.PLAY.STATE | Run state, pause and resume (leaf) | 001 | none |
