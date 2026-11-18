---
id: CORE.JOB.PROG
title: Progress sink and cancellation token
level: 3
parent: CORE.JOB
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-jobs]
built_pr: 48
satisfies: [REQ-CORE.JOB-002, REQ-CORE.JOB-003, REQ-CORE.JOB-010]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.JOB.PROG: Progress sink and cancellation token

## Purpose

The two handles a long-running handler receives: a `ProgressSink` to report how far it has got, and a `CancelToken` to check whether it should stop. Both are one-way, so a handler can never learn anything from its listeners and its result cannot depend on whether anyone is watching.

## Scope

**In:** `ProgressSink` and `CancelToken` types, heartbeat republishing, throttling, input sanitising, cancellation latency guarantee at checkpoints.
**Out:** job states and scheduling (`CORE.JOB.SCHED`); job commands (`CORE.JOB.CMD`); rendering progress (`AUTO`, `VIZ`).

## Requirements

- **REQ-CORE.JOB.PROG-001** The progress sink shall accept a fraction between 0 and 1 and a stage label of at most 128 characters.
- **REQ-CORE.JOB.PROG-002** While a job is running, the job system shall publish its latest progress to listeners at least once per second, republishing the last value if the handler has not reported a new one.
- **REQ-CORE.JOB.PROG-003** The job system shall publish progress to listeners at most 20 times per second per job.
- **REQ-CORE.JOB.PROG-004** If a handler reports a NaN, negative or greater-than-one fraction, then the sink shall ignore the report and record a warning.
- **REQ-CORE.JOB.PROG-005** When cancellation is requested, the cancel token shall report cancelled at the handler's next checkpoint, and the job shall stop within 1 s of that checkpoint [unverified until measured].
- **REQ-CORE.JOB.PROG-006** The progress sink and cancel token shall return no information to the handler other than the cancelled flag.
- **REQ-CORE.JOB.PROG-007** A job run with and without progress listeners shall produce identical output digests.

## Acceptance criteria

- **AC-1** Given the test handler `fixtures/core/job/prog/slow_sum` (sums 10^7 integers, reports every 10^5, checkpoints every 10^4), when run with one listener, then the listener receives at least one update per second and at most 20 per second, with non-NaN fractions in [0, 1].
- **AC-2** Given the same handler run once with 3 listeners and once with none, then the output digests are identical.
- **AC-3** Given a handler that reports nothing for 3 s, then the listener receives at least 3 republished updates with the last value.
- **AC-4** Given cancellation requested mid-run, then the handler observes cancellation at its next checkpoint and the job function returns within 1 s of it.
- **AC-5** (error path) Given a report of NaN or 1.5, then no listener sees it, a warning is recorded, and the process does not panic.

## Design

- **Registry command(s):** none; library types handed to handlers.
- **Data structures:** `ProgressSink { fn report(&self, fraction: f64, stage: &str) }` (returns unit); `CancelToken { fn is_cancelled(&self) -> bool }` backed by an atomic flag; publisher with last-value cell and a 1 s heartbeat timer.
- **Algorithm:** handler writes into a last-value cell; a publisher task reads it on a tick (50 ms minimum interval) and on a 1 s heartbeat; listeners receive copies.
- **Errors:** none surfaced to handlers.
- **Determinism:** D0 for handler results (no feedback path); publication timing is observational and not recorded.
- **Performance budget:** `report` and `is_cancelled` each < 100 ns [unverified until measured].
- **Crate:** `fid-jobs` (module `progress`).

## Test fixtures

Test-only handler `slow_sum` and its expected digest under `fixtures/core/job/prog/`; CC-BY-4.0.

## Build prompt

> In crate `fid-jobs`, add module `progress` with `ProgressSink::report(fraction: f64, stage: &str)` returning unit and `CancelToken::is_cancelled() -> bool` (atomic flag). A publisher keeps the last reported value, publishes to listeners at most 20 times per second and at least once per second (republishing the last value as a heartbeat). Ignore NaN, negative or >1 fractions with a warning; truncate stage labels to 128 characters. Handlers get no other feedback. Tests: AC-1..AC-5 with the `slow_sum` test handler. Constraints: CONSTITUTION §8 (no panic, no unsafe, deny unwrap/expect); results must not depend on listeners.

## Open questions

- An optional ETA field is in the parent's scope; this leaf leaves ETA computation to surfaces, which can derive it from successive fractions.

## Children

_Leaf: none._
