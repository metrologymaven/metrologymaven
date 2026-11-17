---
id: AUTO.RUN.PLAY.STATE
title: Run state, pause and resume
level: 4
parent: AUTO.RUN.PLAY
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN.PLAY-001]
covers: []
depends_on: [AUTO.RUN.EXEC.CMD, CORE.PRJ.CONT, CORE.JOB.SCHED]
side_effect: [R0, W1, W1]
determinism: N
cli: [fid program get-run-state, fid program pause-run, fid program resume-run]
mcp: [program_get_run_state, program_pause_run, program_resume_run]
journal: [program.get_run_state@1, program.pause_run@1, program.resume_run@1]
updated: 2026-11-17
---

# AUTO.RUN.PLAY.STATE: Run state, pause and resume

## Purpose

What an operator (and any other client) sees and controls of a running program besides answering steps. `program.get_run_state@1` returns the run's state, current step, the waiting operator descriptor if any, progress and the last committed step, which the kiosk uses to redraw and to resume after a restart. `program.pause_run@1` and `program.resume_run@1` hold and release a run at the next step boundary through a `StepGate`, without cancelling it.

## Scope

**In:**
- `program.get_run_state@1` (R0)
- `program.pause_run@1` and `program.resume_run@1` (W1, journaled; own runs, or any run for a human principal)
- `PauseGate` implementing `StepGate`, registered into every run's gate chain
- Run state model built from `RunEvent`s and the Run entity

**Out** (and which node owns it instead):
- Cancellation (`job.cancel_job@1`, `CORE.JOB.CMD`; outcome handling in `AUTO.RUN.EXEC.CMD`)
- Answers (`AUTO.RUN.PLAY.ANSWER`); session allow-list (`AUTO.RUN.PLAY.SESSION`)
- Kiosk status display and restart logic (`VIZ.KIOSK.STATUS`, `VIZ.KIOSK.SESSION`)

## Requirements

- **REQ-AUTO.RUN.PLAY.STATE-001** `program.get_run_state@1` shall return run ID, state (`queued`, `running`, `waiting_operator`, `paused`, `awaiting_approval`, `succeeded`, `failed`, `cancelled`), current step key, step index and count, the waiting operator descriptor when the state is `waiting_operator`, and the step key and journal entry hash of the last committed step.
- **REQ-AUTO.RUN.PLAY.STATE-002** When `program.pause_run@1` is accepted, the run shall finish its current step, then hold before the next step in state `paused` until resumed or cancelled.
- **REQ-AUTO.RUN.PLAY.STATE-003** When `program.resume_run@1` is accepted for a paused run, the run shall continue from the next step, and if the run is not paused, then the command shall refuse with `not_paused`.
- **REQ-AUTO.RUN.PLAY.STATE-004** If a principal other than the run's own principal, and not human, requests pause or resume, then the command shall refuse with `not_own_run`.
- **REQ-AUTO.RUN.PLAY.STATE-005** If a paused run is cancelled, then it shall end `cancelled` without running another step.
- **REQ-AUTO.RUN.PLAY.STATE-006** After an application restart, `program.get_run_state@1` for a run interrupted mid-way shall report the last committed step from the run branch, so that a client can show where the run stopped.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/program/family/bracket.json` running and waiting at operator step `op2`, when `fid program get-run-state --run <id> --json` runs, then state is `waiting_operator`, the descriptor of `op2` is returned and the last committed step is the step before `op2`.
- **AC-2** Given a running 12-step run, when `program.pause_run@1` is sent during step 3, then step 3 completes, no step 4 entry appears while paused, state is `paused`; after `program.resume_run@1`, step 4 runs and the run ends `succeeded`.
- **AC-3** (error path) Given a running (not paused) run, when `program.resume_run@1` is sent, then `not_paused`.
- **AC-4** (error path) Given a run started by human H, when an agent principal sends `program.pause_run@1`, then `not_own_run`.
- **AC-5** Given a paused run, when its job is cancelled, then it ends `cancelled` and step 4 never runs.
- **AC-6** Given a run killed by the test harness after step 5 and the app restarted, then `get_run_state` reports state `failed` with reason `interrupted` and last committed step 5 with its entry hash.

## Design

- **Registry command(s):** `program.get_run_state@1` (R0; params `run: RunId`; result `RunState`); `program.pause_run@1` (W1; params `run`); `program.resume_run@1` (W1; params `run`). W1 follows CONSTITUTION §3.1's rule for cancelling one's own job.
- **Data structures:** `RunState { run_id, state, step: Option<StepKey>, step_index, step_count, waiting: Option<OperatorDescriptor>, last_committed: Option<(StepKey, EntryHash)>, reason: Option<String> }`; `PauseGate { paused: AtomicFlag, resume: Condvar }` implementing `StepGate` (`before_step` returns `Hold(paused)` while the flag is set).
- **Algorithm:** state is folded from the run's `RunEvent`s while live; after a restart it is read from the Run entity and the run branch head (`CORE.PRJ.CONT`, journal). A run whose job no longer exists and whose entity has no final outcome is reported `failed` with reason `interrupted`.
- **Errors:** `run_not_found`, `not_paused`, `already_final`, `not_own_run`.
- **Determinism:** N (timing of the step boundary at which a pause takes effect).
- **Performance budget:** `get_run_state` < 10 ms; pause takes effect at the next boundary [unverified until measured].
- **Crate:** `fid-run` (module `play::state`)

## Test fixtures

`fixtures/program/family/bracket.json` (shared; 12 steps including operator steps `op1`, `op2`); job kill and app restart through the CORE test harness; project-authored.

## Build prompt

> In crate `fid-run`, add module `play::state` with registry commands `program.get_run_state@1` (R0; CLI `fid program get-run-state`, MCP `program_get_run_state`), `program.pause_run@1` and `program.resume_run@1` (W1, journaled; CLI `fid program pause-run`, `fid program resume-run`; MCP `program_pause_run`, `program_resume_run`). Run state: run ID, state (`queued`, `running`, `waiting_operator`, `paused`, `awaiting_approval`, `succeeded`, `failed`, `cancelled`), current step key, step index and count, the waiting descriptor when waiting, and the last committed step key with its entry hash; fold it from `RunEvent`s while live, and after a restart read it from the Run entity and run branch head, reporting an unfinished run with no live job as `failed`/`interrupted`. Pause: a `PauseGate` registered in every run's gate chain (the `StepGate` trait from `exec::cmd`) holds before the next step while paused; resume releases it (`not_paused` if not paused); cancellation of a paused run ends it `cancelled`. Only the run's own principal or a human may pause or resume (`not_own_run`). Acceptance: AC-1..AC-6 in the spec. Constraints: CONSTITUTION §8 (no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`); ordered collections only.

## Open questions

- None.

## Children

_Leaf: none._
