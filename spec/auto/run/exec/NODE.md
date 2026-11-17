---
id: AUTO.RUN.EXEC
title: Single re-targeted run orchestration
level: 3
parent: AUTO.RUN
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN-002, REQ-AUTO.RUN-004, REQ-AUTO.RUN-009]
covers: [PW.AUT.05]
depends_on: [AUTO.RUN.BIND, AUTO.PROG.INTERP, CORE.JOB]
cli: [fid program run]
mcp: [program_run]
journal: [program.run]
updated: 2026-11-16
---

# AUTO.RUN.EXEC: Single re-targeted run orchestration

## Purpose

`program.run@1`: takes a prepared run plan, creates the run's own journal branch and Run entity, executes the program through the interpreter as a CORE job, stops at any step marked not re-targetable when the inputs differ from the recording, reports progress by step, supports cancellation, and finalises the run record's outcome.

## Scope

**In:**
- `program.run@1` (W2): plan → branch → interpreter → outcome
- Run branch creation and naming; Run entity write
- Not-re-targetable step handling
- Progress (step index and count) and cancellation through CORE jobs
- Hand-off points for CMM approval when a step needs motion (the run waits; it never approves)

**Out:**
- Preconditions and record schema (`AUTO.RUN.BIND`); interpreter (`AUTO.PROG.INTERP`)
- Motion approval (`CMM.EXEC`); batches (`AUTO.RUN.BATCH`)

## Requirements

- **REQ-AUTO.RUN.EXEC-001** When a run starts, the system shall create a new journal branch for it and write the Run entity, and all of the run's commits shall land on that branch.
- **REQ-AUTO.RUN.EXEC-002** If a step marked `retargetable: false` is reached and any bound input differs from the recorded input, then the run shall stop at that step with `not_retargetable` and finalise the record as failed.
- **REQ-AUTO.RUN.EXEC-003** While a run executes, the system shall publish progress as current step index and total step count.
- **REQ-AUTO.RUN.EXEC-004** When a run is cancelled, the system shall stop before the next step, keep every committed step journaled and finalise the record as cancelled.
- **REQ-AUTO.RUN.EXEC-005** If a step requires an M1 command, then the run shall pause in state `awaiting_approval` and continue only after the approval station reports the outcome.

## Interfaces

- **Offers:** `program.run@1`; run events for VIZ and `AUTO.RUN.PLAY`.
- **Consumes:** `AUTO.RUN.BIND`; `AUTO.PROG.INTERP`; CORE journal branches, jobs, project Run entity; `CMM.EXEC` approval events.

## Decomposition hints

CMD (`program.run@1` and job), BRANCH (branch and Run entity), RETARGET (not-re-targetable handling), APPROVAL (motion hand-off).

## Open questions

- How a run waits for CMM approval (event subscription vs polling a ticket) depends on `CMM.EXEC`'s ticket interface.

## Children

_None yet: status `open`._
