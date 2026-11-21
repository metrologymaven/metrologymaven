---
id: AUTO.RUN.BATCH
title: Batch runs and resume
level: 3
parent: AUTO.RUN
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN-006, REQ-AUTO.RUN-007, REQ-AUTO.RUN-009]
covers: [GOM.AUT.07]
depends_on: [AUTO.RUN.EXEC.CMD, AUTO.RUN.BIND, AUTO.RUN.TABLE, CORE.JOB.SCHED, CORE.JOB.PROG]
side_effect: [W2, W2]
determinism: N
cli: [fid program run-batch, fid program resume-batch]
mcp: [program_run_batch, program_resume_batch]
journal: [program.run_batch@1, program.resume_batch@1]
updated: 2026-11-21
---

# AUTO.RUN.BATCH: Batch runs and resume

## Purpose

`program.run_batch@1` runs one released program over N bindings (rows of a table or a list of variants with inputs), one run each, sequentially, as a single CORE job of class `batch`. A failing binding does not stop the others unless the batch declares stop-on-failure. Progress is published per binding and per step, the batch can be cancelled from every surface, and `program.resume_batch@1` continues an interrupted batch without re-running bindings whose runs already reached a final outcome.

## Scope

**In:**
- `program.run_batch@1` (W2) and `program.resume_batch@1` (W2), each submitted as one CORE job of class `batch`
- `BatchRecord` entity: batch ID, program ID and digest, binding-list digest, stop-on-failure flag, and per binding its index, current run ID, state and earlier attempt run IDs
- Per-binding preparation with `prepare_run` (`AUTO.RUN.BIND`), and starting each binding's run through `program.run@1` (`AUTO.RUN.EXEC.CMD`), never by calling `execute_run` directly
- Failure isolation and stop-on-failure
- Resume: skip bindings in a final state, re-run the others under the same batch record
- Progress (binding index, binding count, step index, step count) and cancellation
- Hand-off of the finished records to `results_table` (`AUTO.RUN.TABLE`)

**Out** (and which node owns it instead):
- Single-run execution, run branches and step guards (`AUTO.RUN.EXEC.CMD`, `AUTO.RUN.EXEC.BRANCH`, `AUTO.RUN.EXEC.RETGT`)
- Lifecycle and binding checks, run record schema (`AUTO.RUN.BIND`)
- Table parsing and results-table building (`AUTO.RUN.TABLE`); CSV and spreadsheet files (`IO`)
- Job queue and priorities (`CORE.JOB.SCHED`)
- Parallel or distributed execution across machines (not in scope)

## Requirements

- **REQ-AUTO.RUN.BATCH-001** When a batch over N bindings finishes, its batch record shall reference exactly N current run records, one per binding, in binding order.
- **REQ-AUTO.RUN.BATCH-002** Before any run starts, the batch shall prepare every binding with `prepare_run`, and if any binding is refused, then the batch shall refuse as a whole with every binding's error and journal nothing.
- **REQ-AUTO.RUN.BATCH-003** If a binding's run ends `failed`, then the batch shall continue with the next binding, unless the batch declares `stop_on_failure`, in which case every later binding shall be marked `not_run`.
- **REQ-AUTO.RUN.BATCH-004** When `program.resume_batch@1` runs on a batch record, the system shall skip bindings whose current run ended `succeeded` or `failed`, re-run every binding in state `pending`, `running`, `cancelled` or `not_run` as a new run, move the previous run ID of a re-run binding to its attempts list, and reuse the same batch record.
- **REQ-AUTO.RUN.BATCH-005** If `program.resume_batch@1` names a batch whose program digest no longer matches a released program, then it shall refuse with `program_changed` and start nothing.
- **REQ-AUTO.RUN.BATCH-006** While a batch runs, the system shall publish progress as binding index, binding count, current step index and current step count, with fraction `(binding_index + step_index / step_count) / binding_count`.
- **REQ-AUTO.RUN.BATCH-007** When a batch is cancelled through its CORE job, the system shall cancel the current run through that run's cancellation (the cancel token of the `program.run@1` run the batch started), mark that binding `cancelled`, leave later bindings `pending`, and keep every committed step journaled.
- **REQ-AUTO.RUN.BATCH-008** The batch job shall be submitted with job class `batch`, so queued interactive jobs start first (REQ-CORE.JOB-006).
- **REQ-AUTO.RUN.BATCH-009** When a batch finishes, its result shall include the results table built by `results_table` over the current run records in binding order.
- **REQ-AUTO.RUN.BATCH-010** The batch shall start every run, first runs and resumed re-runs alike, by invoking `program.run@1`, so that the journal entry carrying the run's `RunStarted` event is a `program.run@1` entry. The batch shall never call `execute_run` directly, and no `program.run_batch@1` or `program.resume_batch@1` entry shall carry a `RunStarted` event. `RunStarted` has a single owner, `program.run@1` (REQ-CORE.RPL.RECON.FOLD-008 (b)).

## Acceptance criteria

- **AC-1** Given released program `fixtures/program/family/bracket.json` and table `fixtures/tables/bracket-batch.csv` (3 rows), when `fid program run-batch --program bracket --table <handle> --json` runs, then the batch record references 3 run records in row order, each with its own branch, and the result table equals `fixtures/tables/bracket-results.csv`.
- **AC-2** Given table `fixtures/tables/bracket-batch-fail2.csv` where row 2's scan makes its run fail, when the batch runs without stop-on-failure, then rows 1 and 3 end `succeeded` and row 2 ends `failed`; with `--stop-on-failure`, then row 3 is `not_run`.
- **AC-3** Given the 3-row batch interrupted (test harness kills the job) during row 2, when `fid program resume-batch --batch <id> --json` runs, then row 1 is not re-run, row 2 gets a new run ID with the old one in its attempts list, row 3 runs, and the batch record ID is unchanged.
- **AC-4** (error path) Given a table whose row 3 binding is invalid, when the batch is submitted, then it fails with `binding_invalid` listing row 3, no run starts and nothing is journaled.
- **AC-5** (error path) Given a batch record whose program was deprecated after interruption, when resumed by an agent principal, then `program_changed` and nothing starts.
- **AC-6** Given a batch job and an interactive test job queued with one free slot, then the interactive job starts first.
- **AC-7** Given a running batch, when `fid job cancel-job <job>` runs, then the current binding is `cancelled`, later ones stay `pending`, and its committed steps remain on its run branch.

## Design

- **Registry command(s):**
  - `program.run_batch@1`, W2; params `program: ProgramRef`, `bindings: TableHandle | Vec<BindingOrVariant>`, `stop_on_failure: bool = false`, `result_columns: Vec<String> = []`; result `{ batch_id, runs: [{index, run_id, state}], results_table }`; long-running, job class `batch`.
  - `program.resume_batch@1`, W2; params `batch: BatchId`; same result type.
  - Agents may call both over MCP (named tools); their runs fork from the agent's branch (`AUTO.RUN.EXEC.BRANCH`), and any M1 step fails per `AUTO.RUN.EXEC.CMD`/`CMM.EXEC.REQ` rules.
- **Data structures:** `BatchRecord { batch_id: Uuid (v7), program_id, program_digest, bindings_digest, stop_on_failure, entries: Vec<BatchEntry> }`; `BatchEntry { index, run_id: Option<RunId>, state: Pending|Running|Succeeded|Failed|Cancelled|NotRun, attempts: Vec<RunId> }`. Stored in the project through the bus; canonical JSON.
- **Algorithm:** (1) resolve bindings (table via `bindings_from_table`); (2) `prepare_run` every binding, refuse all on any error; (3) write the `BatchRecord` (journal `program.run_batch@1`); (4) for each entry in order, set `Running`, start the run by invoking `program.run@1` with that entry's binding (REQ-010), wait for its final outcome while forwarding its step progress to a child progress sink and the batch job's cancellation to the run, then record the outcome. A refusal by `program.run@1` at start (for example the lifecycle state changed after step 2) ends that entry `failed` with the refusal's error; (5) on stop-on-failure mark the rest `NotRun`; (6) build the results table. Resume reloads the record, checks the program digest is still released (`prepare_run`), and runs step 4 over the non-final entries.
- **Errors:** `binding_invalid`, `not_released`, `deprecated`, `program_changed`, `batch_not_found`, `batch_complete` (resume of a batch with no non-final entry).
- **Determinism:** N: the orchestration depends on job timing and cancellation; each run's own determinism is the interpreter's. The batch record's content is a function of the run outcomes apart from IDs.
- **Performance budget:** orchestration overhead < 50 ms per binding beyond the run itself [unverified until measured].
- **Crate:** `fid-run`

## Test fixtures

`fixtures/program/family/bracket.json`, `fixtures/tables/bracket-batch.csv`, `bracket-results.csv` (shared with `AUTO.RUN.TABLE`); new `fixtures/tables/bracket-batch-fail2.csv` and `bracket-batch-badrow3.csv`; scans from the seeded synthetic generator; job kill through the CORE job test harness. All project-authored.

## Build prompt

> In crate `fid-run`, implement registry commands `program.run_batch@1` and `program.resume_batch@1` (both W2; CLI `fid program run-batch`, `fid program resume-batch`; MCP `program_run_batch`, `program_resume_batch`), each submitted as one CORE job of class `batch`. Run batch: resolve bindings (a table through `bindings_from_table`, or a list of bindings or variants); call `prepare_run` for every binding first and refuse the whole batch with all errors if any fails (journal nothing); write a `BatchRecord` (UUIDv7 batch ID, program ID and digest, binding-list digest, stop-on-failure, one entry per binding with state, current run ID and attempts); then start each binding's run in order by invoking `program.run@1` (`AUTO.RUN.EXEC.CMD`), never `execute_run` directly, because `RunStarted` is reserved to `program.run@1` (REQ-CORE.RPL.RECON.FOLD-008 (b)); forward its progress to a child progress sink and the batch job's cancellation to the run, and treat a start-time refusal as that entry `failed`. A failed run does not stop the batch unless `stop_on_failure`, which marks later entries `not_run`. Progress fraction `(binding_index + step_index/step_count) / binding_count`, with binding and step indexes in the stage. Cancellation cancels the current run, marks it `cancelled` and leaves later entries `pending`. Resume: reload the record, refuse with `program_changed` if `prepare_run` no longer accepts the program digest, skip entries ending `succeeded` or `failed`, re-run the rest as new runs moving the old run ID to `attempts`, and keep the same batch ID. Return the batch ID, entry states and the results table from `results_table`. Acceptance: AC-1..AC-7 in the spec (3-row bracket table, failure isolation and stop-on-failure, resume after kill, invalid row refusal, `program_changed`, batch priority, cancel), and REQ-010. Constraints: CONSTITUTION §8 (no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`); ordered collections only; no file paths, only root handles; agents never trigger motion (D-008).

## Open questions

- The earlier question "resume as its own command or a flag" is settled: `program.resume_batch@1` is a separate command, so the journal shows the resume as its own entry.
- How `program.run@1` runs inside the batch's job so that the run's work keeps job class `batch` (REQ-008) rather than `interactive` is for `AUTO.RUN.EXEC.CMD` to state (for example an in-job entry point of the command that still journals as `program.run@1`). Until it does, the batch shall not start runs through any path that emits `RunStarted` outside `program.run@1`.

## Change log

- 2026-11-21 (maintainer decision, recorded in #390): `RunStarted` has a single owner, `program.run@1` (REQ-CORE.RPL.RECON.FOLD-008 (b), reserved effects). New REQ-010: the batch starts every run, including resumed re-runs, through `program.run@1` and never calls `execute_run` directly. REQ-007 text now cancels the current run through that run's cancellation instead of `execute_run`'s token. Scope, Algorithm step 4 and the Build prompt follow; a start-time refusal by `program.run@1` ends that entry `failed`. No REQ ID was renumbered.

## Children

_Leaf: none._
