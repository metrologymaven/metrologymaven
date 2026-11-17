---
id: AUTO.RUN
title: Re-targeted runs, operator playback and batch processing
level: 2
parent: AUTO
kind: subsystem
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO-007, REQ-AUTO-010]
covers: [GOM.AUT.06, GOM.AUT.07, PW.AUT.04, PW.AUT.05, SA.MP.03]
depends_on: [CORE, AUTO.PROG, AUTO.REC]
cli: [fid program run, fid program run-batch, fid program compare-runs]
mcp: [program_run, program_run_batch, program_compare_runs]
journal: [program.run, program.run_batch, program.compare_runs]
updated: 2026-11-16
---

# AUTO.RUN: Re-targeted runs, operator playback and batch processing

## Purpose

Running released programs on new data. A run binds a released Command Program to new inputs and part-family parameters, executes it through the `AUTO.PROG` interpreter, produces its own journal and run record, and compares that journal with a reference run. The same machinery runs a program for an operator (the restricted launcher behind kiosk mode) and over many datasets as a batch, with results collected into tables.

## Scope

**In:**
- Run binding: released program + input handles + parameter values → run
- Run records (Run in the project model) with program digest, bindings and journal
- Run comparison against a reference run using CORE replay outcomes
- Operator playback mode: run only declared steps and prompts, for kiosk and operator-guided use
- Batch: one program over many inputs or parameter rows, sequential and resumable
- Tabular binding and result tables (spreadsheet-style input rows and output tables; file formats via IO)
- Progress and cancellation of runs and batches

**Out** (and which node owns it instead):
- Interpreter semantics (`AUTO.PROG`)
- Lifecycle and release (`AUTO.REC`)
- Replay engine and outcome classification (`CORE`)
- Kiosk UI (`VIZ`)
- CSV and spreadsheet readers and writers (`IO`)
- Motion approval for CMM steps inside a run (`CMM`)
- Statistics across runs (`RPT`)

## Requirements

- **REQ-AUTO.RUN-001** A run shall bind only a released program, and if the program is draft, registered or deprecated, then the run shall be refused unless a human starts it as a trial run, which shall be marked as such in its run record.
- **REQ-AUTO.RUN-002** Every run shall produce its own journal branch and a run record holding the program digest, the parameter binding, the input data digests and the run's outcome.
- **REQ-AUTO.RUN-003** When a run finishes, the system shall be able to compare its journal with a chosen reference run and report, per step, `IDENTICAL`, `EQUIVALENT`, `DIVERGENT` or `NOT_REPLAYABLE`.
- **REQ-AUTO.RUN-004** If a step marked not re-targetable is reached in a re-targeted run, then the run shall stop at that step with a typed error rather than apply the recorded selection to new data.
- **REQ-AUTO.RUN-005** In operator playback mode, the run shall present only the operator steps the program declares and shall offer no command outside the program.
- **REQ-AUTO.RUN-006** A batch shall run one released program over N bindings and produce N run records, and the failure of one binding shall not stop the others unless the batch declares stop-on-failure.
- **REQ-AUTO.RUN-007** If a batch is interrupted, then on resume it shall skip bindings whose runs completed and re-run only the incomplete ones.
- **REQ-AUTO.RUN-008** A batch shall accept its bindings as rows of a table (one row per run, one column per parameter) and shall write a results table with one row per run.
- **REQ-AUTO.RUN-009** Runs and batches shall report progress (step index and binding index) and shall be cancellable from every surface, and cancellation shall leave every committed step journaled.

## Interfaces

- **Offers:** `program.run@1`, `program.run_batch@1`, `program.compare_runs@1`; run records; operator playback API used by VIZ kiosk mode.
- **Consumes:** `AUTO.PROG` interpreter; `AUTO.REC` lifecycle state; CORE project model (Run), journal branches, replay and jobs; IO table formats.

## Decomposition hints

BIND (binding and run record), EXEC (run orchestration over the interpreter), CMP (run comparison), OPER (operator playback mode), BATCH (batch, resume), TABLE (tabular bindings and results).

## Open questions

- SA.MP.03 (spreadsheet export and interaction) is covered here as tabular bindings and result tables. Live two-way spreadsheet links may need an X1 or plugin path; out of scope here until requested.
- SA.MP.03 is covered by `AUTO.RUN.TABLE` as tabular bindings and results tables; live two-way spreadsheet links remain out of scope as stated above.
- `AUTO.RUN.CMP` needs the interpreter to write step ID and iteration path into each run entry's metadata; recorded as a requirement for `AUTO.PROG.INTERP` when it is split.

## Split rationale

The split separates **deciding a run may happen** (BIND: lifecycle, binding, record), **making it happen** (EXEC for one run, BATCH for many, PLAY for an operator), and **judging what happened** (CMP). TABLE is separate because it is a pure data mapping shared by batch input and batch output and can be finished before any execution exists.

Alternatives considered:
- **BIND inside EXEC.** Rejected: the preconditions (released only, human trial runs, deprecation override) are policy that EXEC, BATCH and PLAY must all apply identically; a library leaf keeps one copy.
- **PLAY as a VIZ concern only.** Rejected: the restriction "nothing outside the program" must hold in the engine, not only in the kiosk screens, or any other client could bypass it.
- **TABLE inside BATCH.** Rejected: the mapping is testable without running anything, and SA.MP.03 (spreadsheet interaction) is about the table contract, not batch control.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AUTO.RUN.BIND | Run binding validation and run records (leaf) | 001 002 | PW.AUT.05 |
| AUTO.RUN.EXEC | Single re-targeted run orchestration | 002 004 009 | PW.AUT.05 |
| AUTO.RUN.CMP | Run comparison against a reference run (leaf) | 003 | none |
| AUTO.RUN.PLAY | Operator playback mode | 005 | GOM.AUT.06, PW.AUT.04 |
| AUTO.RUN.BATCH | Batch runs and resume | 006 007 009 | GOM.AUT.07 |
| AUTO.RUN.TABLE | Tabular bindings and results tables (leaf) | 008 | SA.MP.03 |
