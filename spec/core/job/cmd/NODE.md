---
id: CORE.JOB.CMD
title: Job commands and surface task contract
level: 3
parent: CORE.JOB
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-jobs]
built_pr: 196
satisfies: [REQ-CORE.JOB-008]
covers: []
depends_on: [CORE.JOB.SCHED]
side_effect: W1
determinism: N
cli: [fid job list-jobs, fid job show-status, fid job wait-job, fid job cancel-job]
mcp: [job_list_jobs, job_show_status, job_wait_job, job_cancel_job]
journal: [job.list_jobs@1, job.show_status@1, job.wait_job@1, job.cancel_job@1]
updated: 2026-11-16
---

# CORE.JOB.CMD: Job commands and surface task contract

## Purpose

The registry commands every surface uses to see and control jobs: list, show status, wait and cancel. It also fixes the JSON shape of a job status, which is the one data contract that MCP tasks, CLI `--wait` progress and GUI progress all map onto.

## Scope

**In:** `job.list_jobs@1`, `job.show_status@1`, `job.wait_job@1` (R0) and `job.cancel_job@1` (W1); the job-status JSON schema; the cancel authorisation precondition.
**Out:** scheduling and states (`CORE.JOB.SCHED`); enforcing the cancel rule at the bus (`CORE.BUS.POL`); mapping onto MCP task protocol messages and CLI rendering (`AUTO`).

## Requirements

- **REQ-CORE.JOB.CMD-001** `job.list_jobs@1`, `job.show_status@1` and `job.wait_job@1` shall be side-effect class R0.
- **REQ-CORE.JOB.CMD-002** `job.cancel_job@1` shall be side-effect class W1, and its precondition shall allow the call only when the caller is the job's own principal or a human principal.
- **REQ-CORE.JOB.CMD-003** When `job.list_jobs@1` runs, the system shall return the caller's own jobs, or all jobs for a human principal, sorted by submission time then job ID, filterable by state.
- **REQ-CORE.JOB.CMD-004** When `job.show_status@1` runs, the system shall return the job's ID, command, state, progress fraction, stage label, timestamps and error code in the published job-status schema.
- **REQ-CORE.JOB.CMD-005** When `job.wait_job@1` runs with a timeout, the system shall return the job status as soon as the job reaches a terminal state, or the current status with `timed_out: true` when the timeout elapses first.
- **REQ-CORE.JOB.CMD-006** If a job ID does not exist, then each command shall return a typed `not_found` error.
- **REQ-CORE.JOB.CMD-007** When `job.cancel_job@1` targets a job already in a terminal state, the system shall return the job status unchanged and commit no cancellation.

## Acceptance criteria

- **AC-1** Given a running test job submitted by script principal S, when S runs `fid job show-status <id> --json`, then the output validates against `job-status.schema.json` and the state is `running`.
- **AC-2** Given the same job, when S runs `fid job cancel-job <id> --json`, then the job ends `cancelled` and the Record Journal holds one `job.cancel_job@1` entry.
- **AC-3** (error path) Given a job of human principal H, when agent principal A calls `job_cancel_job` over MCP, then the call fails with `policy` and the job keeps running.
- **AC-4** Given `fid job wait-job <id> --timeout 0.2 --json` on a 2 s job, then it returns after about 0.2 s with `timed_out: true`; with `--timeout 5` it returns the terminal status.
- **AC-5** Given three jobs from S and two from another script principal T, when S lists jobs, then exactly its three are returned; when a human lists, all five are returned in submission order.
- **AC-6** (error path) Given an unknown job ID, then each command fails with `not_found` and writes no Record Journal entry.

## Design

- **Registry command(s):**
  - `job.list_jobs@1` R0, determinism N; params `{ state?: JobState }`; result `{ jobs: [JobStatus] }`.
  - `job.show_status@1` R0, N; params `{ job: JobId }`; result `JobStatus`.
  - `job.wait_job@1` R0, N; params `{ job: JobId, timeout: Duration = 30 s }` (`x-quantity: time`); result `JobStatus & { timed_out: bool }`.
  - `job.cancel_job@1` W1, N, undo `not_undoable`; params `{ job: JobId }`; precondition `job.principal == caller || caller.kind == "human"`; result `JobStatus`.
- **Data structures:** `JobStatus` per `job-status.schema.json` (id, command, state, progress, stage, submitted_at, started_at, ended_at, error).
- **Algorithm:** thin handlers over the `sched` API; wait uses the scheduler's completion notification with a deadline.
- **Errors:** `not_found`, `policy` (from the bus).
- **Determinism:** N; results reflect live job state.
- **Performance budget:** status and list in < 10 ms for 1,000 jobs [unverified until measured].
- **Crate:** `fid-jobs` (module `commands`).

## Test fixtures

Test sleep handlers from `fixtures/core/job/sched/` and `job-status.schema.json`; synthetic principals provided by the test channel harness; CC-BY-4.0.

## Build prompt

> In crate `fid-jobs`, add module `commands` registering `job.list_jobs@1`, `job.show_status@1`, `job.wait_job@1` (all R0, determinism N) and `job.cancel_job@1` (W1, N, undo not_undoable, precondition: caller is the job's principal or a human). Publish `job-status.schema.json` (id, command, state, progress, stage, timestamps, error) and use it for every result. List returns the caller's jobs (all jobs for a human), sorted by submission time then ID, optional state filter. Wait returns at the terminal state or at the timeout with `timed_out: true`. Cancelling a terminal job returns its status and commits nothing. Unknown IDs give `not_found`. CLI `fid job list-jobs | show-status | wait-job | cancel-job`, MCP `job_list_jobs | job_show_status | job_wait_job | job_cancel_job`, all with `--json`. Tests: AC-1..AC-6. Constraints: CONSTITUTION §3.1 (cancel is W1 for one's own jobs; another principal's job needs a human), §8 (no panic, no unsafe).

## Open questions

- Cancelling a terminal job commits nothing, so `job.cancel_job@1` can return without a Record Journal entry; confirm with `CORE.BUS.PIPE` that a W1 command may legitimately commit zero events.

## Children

_Leaf: none._
