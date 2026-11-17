---
id: CORE.JOB
title: Jobs: long-running commands, progress and cancellation
level: 2
parent: CORE
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE-010]
covers: []
depends_on: []
cli: [fid job *]
mcp: [job_*]
journal: []
updated: 2026-11-16
---

# CORE.JOB: Jobs

## Purpose

Runs long commands without blocking any surface. A job wraps one command execution (or one recompute) with an ID, progress reports, cancellation and a final outcome. Only the outcome of a committed job reaches the Record Journal; lifecycle events go to the Activity Log. JOB is separate from the bus so that scheduling and cancellation can be specified and tested on their own, and so that MCP tasks, CLI progress bars and GUI progress share one model.

## Scope

**In:**
- Job model: ID, command, principal, state (`queued | running | succeeded | failed | cancelled`), progress, timestamps
- Progress reporting: fraction complete, stage label, optional ETA
- Cooperative cancellation with a guaranteed no-partial-commit outcome
- Scheduling policy: queue, concurrency limit, priority of interactive over batch work
- Job queries: list, status, wait, cancel
- Mapping of jobs to surface task models (MCP tasks, CLI `--wait`, GUI progress) as a data contract
- Timeouts and resource limits per job

**Out** (and which node owns it instead):
- Deciding when a command becomes a job and committing its result (owned by `CORE.BUS`)
- Journal append of job outcomes (owned by `CORE.JRN`)
- Surface rendering of progress (owned by `AUTO` and `VIZ`)
- Batch processing of many files as a workflow (owned by `AUTO`)

## Requirements

- **REQ-CORE.JOB-001** When a command declared long-running is invoked, the job system shall return a job ID within 100 ms and run the command asynchronously [unverified until measured].
- **REQ-CORE.JOB-002** While a job is running, the job system shall publish progress at least once per second, as a fraction between 0 and 1 and a stage label.
- **REQ-CORE.JOB-003** When cancellation is requested, the job system shall stop the job within 1 s of the next cancellation checkpoint and set its state to `cancelled` [unverified until measured].
- **REQ-CORE.JOB-004** If a job is cancelled or fails, then no partial result of that job shall be committed to the journal.
- **REQ-CORE.JOB-005** The job system shall run at most a configured number of concurrent jobs and shall queue the rest in submission order within each priority.
- **REQ-CORE.JOB-006** While a batch job and an interactive job are both queued, the job system shall start the interactive job first.
- **REQ-CORE.JOB-007** If a job exceeds its declared timeout, then the job system shall cancel it and set its state to `failed` with a typed timeout error.
- **REQ-CORE.JOB-008** Job status, list and wait commands shall be side-effect class R0, and cancel shall be allowed only for the job's own principal or a human principal.
- **REQ-CORE.JOB-009** The job system shall write job lifecycle events to the Activity Log and not to the Record Journal.
- **REQ-CORE.JOB-010** Progress reporting shall not alter the job's computed result; a job run with and without progress listeners shall produce identical output digests.

## Interfaces

- **Offers:** `Jobs::submit(exec) -> JobId`, `Jobs::status`, `Jobs::wait`, `Jobs::cancel`, `CancelToken` and `ProgressSink` handed to handlers.
- **Commands (illustrative):** `job.list_jobs@1` (R0), `job.show_status@1` (R0), `job.wait_job@1` (R0), `job.cancel_job@1` (W1). CLI `fid job *`; MCP `job_*`.
- **Consumes:** nothing inside CORE at specification level; the bus calls it.

## Decomposition hints

MODEL (job states and records), PROG (progress and cancellation tokens), SCHED (queue, priorities, limits, timeouts), CMD (job commands).

## Open questions

- Is `job.cancel_job` a W1 command (it changes job state but no document state) or R0? Proposed W1 so it is attributable; confirm against CONSTITUTION §3 classes.

## Children

_None yet: status `open`._
