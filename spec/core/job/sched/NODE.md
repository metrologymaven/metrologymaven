---
id: CORE.JOB.SCHED
title: Job model, queue, priorities, limits and timeouts
level: 3
parent: CORE.JOB
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.JOB-001, REQ-CORE.JOB-004, REQ-CORE.JOB-005, REQ-CORE.JOB-006, REQ-CORE.JOB-007, REQ-CORE.JOB-009]
covers: []
depends_on: [CORE.JOB.PROG]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.JOB.SCHED: Job model, queue, priorities, limits and timeouts

## Purpose

Runs submitted executions as jobs: assigns an ID immediately, queues them by priority, runs at most a configured number at once, enforces timeouts, and hands a result back only when a job succeeds. It emits every lifecycle change to a sink that the journal wires to the Activity Log, never to the Record Journal.

## Scope

**In:** job record and state machine, submission, queue and priority classes, concurrency limit, timeouts, result hand-back rules, lifecycle event emission.
**Out:** progress and cancel handles (`CORE.JOB.PROG`); job commands (`CORE.JOB.CMD`); deciding which commands become jobs and committing results (`CORE.BUS.PIPE`); Activity Log storage (`CORE.JRN.STREAM`).

## Requirements

- **REQ-CORE.JOB.SCHED-001** When an execution is submitted, the job system shall return a job ID within 100 ms and run the execution asynchronously [unverified until measured].
- **REQ-CORE.JOB.SCHED-002** Every job shall be in exactly one state from `queued`, `running`, `succeeded`, `failed` or `cancelled`, and shall move only along queued → running → succeeded, failed or cancelled, or queued → cancelled.
- **REQ-CORE.JOB.SCHED-003** The job system shall run at most the configured number of jobs at once (default: logical CPU count minus one, at least one) and queue the rest.
- **REQ-CORE.JOB.SCHED-004** While jobs of both classes are queued, the job system shall start the oldest `interactive` job before any `batch` job, and within a class shall start jobs in submission order.
- **REQ-CORE.JOB.SCHED-005** If a running job exceeds its timeout, then the job system shall cancel it and set its state to `failed` with error code `timeout`.
- **REQ-CORE.JOB.SCHED-006** If a job ends `failed` or `cancelled`, then the job system shall discard its result and hand nothing to the committer.
- **REQ-CORE.JOB.SCHED-007** When a queued job is cancelled, the job system shall set it to `cancelled` without starting it.
- **REQ-CORE.JOB.SCHED-008** When a job changes state, the job system shall emit one lifecycle event (job ID, command, principal kind, old and new state, error code) to the lifecycle sink, and shall write nothing to the Record Journal.

## Acceptance criteria

- **AC-1** Given a limit of 2 and fixture `fixtures/core/job/sched/mixed_queue.json` (5 batch jobs then 2 interactive jobs, each a test sleep handler), when submitted, then each submission returns within 100 ms and the start order equals the order in `mixed_queue.expected.json` (interactive jobs start as soon as a slot frees).
- **AC-2** Given a job with timeout 200 ms running a 2 s test handler, then it ends `failed` with `timeout` and the committer receives nothing.
- **AC-3** Given a queued job cancelled before a slot frees, then it ends `cancelled`, its handler never runs, and exactly two lifecycle events (submitted-as-queued, cancelled) are emitted.
- **AC-4** Given any run of AC-1..AC-3, then every observed transition is allowed by REQ-002, and a capturing Record Journal stub receives no writes.
- **AC-5** Given 1,000 submissions in a tight loop, then all IDs are unique and the 99th-percentile submit latency is under 100 ms in the benchmark.

## Design

- **Registry command(s):** none; library API `Jobs::submit(exec, JobOpts { class, timeout, principal }) -> JobId`, `Jobs::status(id)`, `Jobs::cancel(id)`, `Jobs::wait(id, deadline)`.
- **Data structures:** `JobRecord { id: Uuid (v7), command, principal_kind, class: Interactive|Batch, state, progress, submitted_at, started_at, ended_at, error }`; two FIFO queues; `trait LifecycleSink`.
- **Algorithm:** a dispatcher loop takes from the interactive queue first, else the batch queue, when a slot is free; each job runs on a worker with its `CancelToken`; a timer cancels on timeout; results go to a one-shot channel that is dropped unless the job succeeded.
- **Errors:** `timeout`, `cancelled`, `handler_failed`, `not_found`.
- **Determinism:** N; start order and timing depend on wall-clock execution, but the start-order rules are deterministic given the same completion order.
- **Performance budget:** REQ-001.
- **Crate:** `fid-jobs` (module `sched`).

## Test fixtures

`fixtures/core/job/sched/mixed_queue.json` and its expected order; test sleep handlers with configurable durations; CC-BY-4.0.

## Build prompt

> In crate `fid-jobs`, add module `sched`. Implement `Jobs::submit/status/cancel/wait` with `JobRecord` (UUIDv7 ID, command, principal kind, class Interactive|Batch, state, progress, timestamps, error). States: queued, running, succeeded, failed, cancelled, with only the transitions queued→running→{succeeded, failed, cancelled} and queued→cancelled. Concurrency limit configurable (default logical CPUs minus one, at least one); interactive jobs start before batch jobs, FIFO within a class. Timeouts cancel the job and set `failed` with `timeout`. Hand a result to the caller only on `succeeded`. Emit one lifecycle event per transition to a `LifecycleSink` trait (wired by the journal to the Activity Log); never write the Record Journal. Use the `progress` module's `CancelToken` and `ProgressSink`. Tests: AC-1..AC-5 with `fixtures/core/job/sched/`. Constraints: CONSTITUTION §8 (no panic, no unsafe, deny unwrap/expect), no unbounded queues without a configured cap (default 10,000 queued jobs, then `queue_full`).

## Open questions

- Whether a long-running interactive job should be able to pre-empt a running batch job; this leaf does not pre-empt.

## Children

_Leaf: none._
