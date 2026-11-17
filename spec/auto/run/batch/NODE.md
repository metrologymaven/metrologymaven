---
id: AUTO.RUN.BATCH
title: Batch runs and resume
level: 3
parent: AUTO.RUN
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN-006, REQ-AUTO.RUN-007, REQ-AUTO.RUN-009]
covers: [GOM.AUT.07]
depends_on: [AUTO.RUN.EXEC, AUTO.RUN.TABLE]
cli: [fid program run-batch]
mcp: [program_run_batch]
journal: [program.run_batch]
updated: 2026-11-16
---

# AUTO.RUN.BATCH: Batch runs and resume

## Purpose

`program.run_batch@1`: runs one released program over N bindings (from a table or a list of variants and inputs), one run record each, sequentially, with failure isolation, progress by binding, cancellation and resume after interruption without re-running completed bindings.

## Scope

**In:**
- `program.run_batch@1` (W2) as a batch-priority CORE job
- Batch record: program digest, binding list digest, per-binding run IDs and states
- Failure isolation and optional stop-on-failure
- Resume: skip completed, re-run incomplete
- Results table hand-off to `AUTO.RUN.TABLE`

**Out:**
- Single-run execution (`AUTO.RUN.EXEC`); table parsing and writing (`AUTO.RUN.TABLE`)
- Parallel execution across machines (not in scope)

## Requirements

- **REQ-AUTO.RUN.BATCH-001** A batch over N bindings shall produce exactly N run records, one per binding, in binding order.
- **REQ-AUTO.RUN.BATCH-002** If a binding's run fails, then the batch shall continue with the next binding unless the batch declares stop-on-failure.
- **REQ-AUTO.RUN.BATCH-003** When an interrupted batch is resumed, the system shall skip bindings whose runs reached a final state and re-run only the others, reusing the same batch record.
- **REQ-AUTO.RUN.BATCH-004** While a batch runs, the system shall publish progress as binding index, binding count and the current run's step index.
- **REQ-AUTO.RUN.BATCH-005** Batch jobs shall run at batch priority so interactive jobs start first (REQ-CORE.JOB-006).

## Interfaces

- **Offers:** `program.run_batch@1`, `program.resume_batch@1` (proposed).
- **Consumes:** `AUTO.RUN.EXEC`; `AUTO.RUN.TABLE`; CORE jobs.

## Decomposition hints

CMD (batch command and record), RESUME, PROGRESS.

## Open questions

- Resume as a separate command (`program.resume_batch@1`) or a flag on `run_batch` with the batch ID; proposed separate command for a clear journal entry.

## Children

_None yet: status `open`._
